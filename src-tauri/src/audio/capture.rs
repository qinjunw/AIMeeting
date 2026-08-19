use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::time::{Duration, Instant};

use thiserror::Error;

use super::frame::{AudioFrame, AudioSource};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CaptureSourceKind {
    Microphone,
    System,
}

impl CaptureSourceKind {
    pub fn audio_source(self) -> AudioSource {
        match self {
            Self::Microphone => AudioSource::Microphone,
            Self::System => AudioSource::System,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeSampleFormat {
    F32,
    I16,
    U16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub kind: CaptureSourceKind,
    pub is_default: bool,
    pub sample_format: NativeSampleFormat,
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceSelection {
    pub microphone: bool,
    pub system: bool,
}

impl SourceSelection {
    pub const fn microphone_only() -> Self {
        Self {
            microphone: true,
            system: false,
        }
    }

    pub const fn system_only() -> Self {
        Self {
            microphone: false,
            system: true,
        }
    }

    pub const fn mixed() -> Self {
        Self {
            microphone: true,
            system: true,
        }
    }

    pub fn includes(self, kind: CaptureSourceKind) -> bool {
        match kind {
            CaptureSourceKind::Microphone => self.microphone,
            CaptureSourceKind::System => self.system,
        }
    }

    fn validate(self) -> Result<(), CaptureError> {
        if self.microphone || self.system {
            Ok(())
        } else {
            Err(CaptureError::NoSourcesSelected)
        }
    }
}

pub enum NativeSamples<'a> {
    F32(&'a [f32]),
    I16(&'a [i16]),
    U16(&'a [u16]),
}

pub fn convert_samples_to_f32(samples: NativeSamples<'_>) -> Vec<f32> {
    match samples {
        NativeSamples::F32(samples) => samples
            .iter()
            .map(|sample| sample.clamp(-1.0, 1.0))
            .collect(),
        NativeSamples::I16(samples) => samples
            .iter()
            .map(|sample| *sample as f32 / 32_768.0)
            .collect(),
        NativeSamples::U16(samples) => samples
            .iter()
            .map(|sample| (*sample as f32 - 32_768.0) / 32_768.0)
            .collect(),
    }
}

#[derive(Clone)]
pub struct CaptureFrameSink {
    sender: SyncSender<AudioFrame>,
    started: Instant,
}

impl CaptureFrameSink {
    pub fn try_send(&self, frame: AudioFrame) -> Result<(), CaptureError> {
        self.sender.try_send(frame).map_err(|error| match error {
            TrySendError::Full(_) => CaptureError::CallbackQueueFull,
            TrySendError::Disconnected(_) => CaptureError::CallbackReceiverDisconnected,
        })
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}

pub fn capture_channel(
    capacity_frames: usize,
) -> Result<(CaptureFrameSink, Receiver<AudioFrame>), CaptureError> {
    if capacity_frames == 0 {
        return Err(CaptureError::ZeroCallbackQueueCapacity);
    }
    let (sender, receiver) = sync_channel(capacity_frames);
    Ok((
        CaptureFrameSink {
            sender,
            started: Instant::now(),
        },
        receiver,
    ))
}

pub trait AudioCaptureSource: Send {
    fn info(&self) -> &AudioDeviceInfo;
    fn start(&mut self, sink: CaptureFrameSink) -> Result<(), CaptureError>;
    fn stop(&mut self) -> Result<(), CaptureError>;
    fn take_error(&self) -> Option<String>;

    fn take_warning(&self) -> Option<String> {
        None
    }
}

pub struct CaptureCoordinator {
    microphone: Option<Box<dyn AudioCaptureSource>>,
    system: Option<Box<dyn AudioCaptureSource>>,
    active: Option<SourceSelection>,
}

impl CaptureCoordinator {
    pub fn new(
        microphone: Option<Box<dyn AudioCaptureSource>>,
        system: Option<Box<dyn AudioCaptureSource>>,
    ) -> Self {
        Self {
            microphone,
            system,
            active: None,
        }
    }

    pub fn start(
        &mut self,
        selection: SourceSelection,
        sink: CaptureFrameSink,
    ) -> Result<(), CaptureError> {
        selection.validate()?;
        if self.active.is_some() {
            return Err(CaptureError::CoordinatorAlreadyRunning);
        }
        if selection.microphone && self.microphone.is_none() {
            return Err(CaptureError::MissingSource(CaptureSourceKind::Microphone));
        }
        if selection.system && self.system.is_none() {
            return Err(CaptureError::MissingSource(CaptureSourceKind::System));
        }

        if selection.microphone {
            self.microphone
                .as_mut()
                .expect("source presence was checked")
                .start(sink.clone())?;
        }
        if selection.system {
            if let Err(error) = self
                .system
                .as_mut()
                .expect("source presence was checked")
                .start(sink)
            {
                if selection.microphone {
                    let _ = self
                        .microphone
                        .as_mut()
                        .expect("source presence was checked")
                        .stop();
                }
                return Err(error);
            }
        }
        self.active = Some(selection);
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), CaptureError> {
        let Some(selection) = self.active.take() else {
            return Ok(());
        };

        let mut first_error = None;
        if selection.microphone {
            if let Err(error) = self
                .microphone
                .as_mut()
                .expect("active source must exist")
                .stop()
            {
                first_error = Some(error);
            }
        }
        if selection.system {
            if let Err(error) = self
                .system
                .as_mut()
                .expect("active source must exist")
                .stop()
            {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    pub fn source_errors(&self) -> Vec<(CaptureSourceKind, String)> {
        let mut errors = Vec::new();
        for (kind, source) in [
            (CaptureSourceKind::Microphone, self.microphone.as_ref()),
            (CaptureSourceKind::System, self.system.as_ref()),
        ] {
            if let Some(error) = source.and_then(|source| source.take_error()) {
                errors.push((kind, error));
            }
        }
        errors
    }

    pub fn source_warnings(&self) -> Vec<(CaptureSourceKind, String)> {
        let mut warnings = Vec::new();
        for (kind, source) in [
            (CaptureSourceKind::Microphone, self.microphone.as_ref()),
            (CaptureSourceKind::System, self.system.as_ref()),
        ] {
            if let Some(warning) = source.and_then(|source| source.take_warning()) {
                warnings.push((kind, warning));
            }
        }
        warnings
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum CaptureError {
    #[error("at least one capture source must be selected")]
    NoSourcesSelected,
    #[error("capture source {0:?} is unavailable")]
    MissingSource(CaptureSourceKind),
    #[error("capture coordinator is already running")]
    CoordinatorAlreadyRunning,
    #[error("capture source {0} is already running")]
    AlreadyRunning(String),
    #[error("capture callback queue capacity must be greater than zero")]
    ZeroCallbackQueueCapacity,
    #[error("capture callback queue is full")]
    CallbackQueueFull,
    #[error("capture callback receiver is disconnected")]
    CallbackReceiverDisconnected,
    #[error("audio device error: {0}")]
    Device(String),
    #[error("audio stream error: {0}")]
    Stream(String),
    #[error("sample format {0} is not supported")]
    UnsupportedSampleFormat(String),
}

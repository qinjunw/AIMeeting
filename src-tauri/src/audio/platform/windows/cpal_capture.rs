use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, DeviceId, ErrorKind, SampleFormat, Stream, SupportedStreamConfig};

use crate::audio::capture::{
    convert_samples_to_f32, AudioCaptureSource, AudioDeviceInfo, CaptureError, CaptureFrameSink,
    CaptureSourceKind, NativeSampleFormat, NativeSamples,
};
use crate::audio::frame::AudioFrame;

pub fn enumerate_audio_devices() -> Result<Vec<AudioDeviceInfo>, CaptureError> {
    let host = cpal::default_host();
    let default_input = host
        .default_input_device()
        .and_then(|device| device.id().ok())
        .map(|id| id.to_string());
    let default_output = host
        .default_output_device()
        .and_then(|device| device.id().ok())
        .map(|id| id.to_string());

    let mut devices = Vec::new();
    for device in host
        .input_devices()
        .map_err(|error| CaptureError::Device(error.to_string()))?
    {
        if let Ok(info) = describe_device(
            &device,
            CaptureSourceKind::Microphone,
            default_input.as_deref(),
        ) {
            devices.push(info);
        }
    }
    for device in host
        .output_devices()
        .map_err(|error| CaptureError::Device(error.to_string()))?
    {
        if let Ok(info) = describe_device(
            &device,
            CaptureSourceKind::System,
            default_output.as_deref(),
        ) {
            devices.push(info);
        }
    }
    devices.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| right.is_default.cmp(&left.is_default))
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(devices)
}

pub fn default_capture_source(kind: CaptureSourceKind) -> Result<CpalCaptureSource, CaptureError> {
    let host = cpal::default_host();
    let device = match kind {
        CaptureSourceKind::Microphone => host.default_input_device(),
        CaptureSourceKind::System => host.default_output_device(),
    }
    .ok_or(CaptureError::MissingSource(kind))?;
    CpalCaptureSource::from_device(device, kind, true)
}

pub struct CpalCaptureSource {
    device: Device,
    config: SupportedStreamConfig,
    info: AudioDeviceInfo,
    stream: Option<Stream>,
    callback_error: Arc<Mutex<Option<String>>>,
    callback_warning: Arc<Mutex<Option<String>>>,
}

impl CpalCaptureSource {
    pub fn open(device_id: &str, kind: CaptureSourceKind) -> Result<Self, CaptureError> {
        let host = cpal::default_host();
        let id = DeviceId::from_str(device_id)
            .map_err(|error| CaptureError::Device(error.to_string()))?;
        let device = host
            .device_by_id(&id)
            .ok_or_else(|| CaptureError::Device(format!("device {device_id} is unavailable")))?;
        Self::from_device(device, kind, false)
    }

    fn from_device(
        device: Device,
        kind: CaptureSourceKind,
        is_default: bool,
    ) -> Result<Self, CaptureError> {
        let info = describe_device(&device, kind, None)?;
        let config = default_config(&device, kind)?;
        Ok(Self {
            device,
            config,
            info: AudioDeviceInfo { is_default, ..info },
            stream: None,
            callback_error: Arc::new(Mutex::new(None)),
            callback_warning: Arc::new(Mutex::new(None)),
        })
    }
}

impl AudioCaptureSource for CpalCaptureSource {
    fn info(&self) -> &AudioDeviceInfo {
        &self.info
    }

    fn start(&mut self, sink: CaptureFrameSink) -> Result<(), CaptureError> {
        if self.stream.is_some() {
            return Err(CaptureError::AlreadyRunning(self.info.id.clone()));
        }

        if let Ok(mut error) = self.callback_error.lock() {
            *error = None;
        }
        if let Ok(mut warning) = self.callback_warning.lock() {
            *warning = None;
        }
        let stream = build_stream(
            &self.device,
            self.config,
            self.info.kind,
            sink,
            Arc::clone(&self.callback_error),
            Arc::clone(&self.callback_warning),
        )?;
        stream
            .play()
            .map_err(|error| CaptureError::Stream(error.to_string()))?;
        self.stream = Some(stream);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        self.stream.take();
        Ok(())
    }

    fn take_error(&self) -> Option<String> {
        self.callback_error
            .lock()
            .ok()
            .and_then(|mut error| error.take())
    }

    fn take_warning(&self) -> Option<String> {
        self.callback_warning
            .lock()
            .ok()
            .and_then(|mut warning| warning.take())
    }
}

fn describe_device(
    device: &Device,
    kind: CaptureSourceKind,
    default_id: Option<&str>,
) -> Result<AudioDeviceInfo, CaptureError> {
    let id = device
        .id()
        .map_err(|error| CaptureError::Device(error.to_string()))?
        .to_string();
    let name = device
        .description()
        .map(|description| description.name().to_string())
        .unwrap_or_else(|_| device.to_string());
    let config = default_config(device, kind)?;
    let sample_format = native_sample_format(config.sample_format())?;
    Ok(AudioDeviceInfo {
        is_default: default_id.is_some_and(|default| default == id),
        id,
        name,
        kind,
        sample_format,
        sample_rate: config.sample_rate(),
        channels: config.channels(),
    })
}

fn default_config(
    device: &Device,
    kind: CaptureSourceKind,
) -> Result<SupportedStreamConfig, CaptureError> {
    match kind {
        CaptureSourceKind::Microphone => device.default_input_config(),
        CaptureSourceKind::System => device.default_output_config(),
    }
    .map_err(|error| CaptureError::Device(error.to_string()))
}

fn native_sample_format(format: SampleFormat) -> Result<NativeSampleFormat, CaptureError> {
    match format {
        SampleFormat::F32 => Ok(NativeSampleFormat::F32),
        SampleFormat::I16 => Ok(NativeSampleFormat::I16),
        SampleFormat::U16 => Ok(NativeSampleFormat::U16),
        other => Err(CaptureError::UnsupportedSampleFormat(other.to_string())),
    }
}

fn build_stream(
    device: &Device,
    config: SupportedStreamConfig,
    kind: CaptureSourceKind,
    sink: CaptureFrameSink,
    callback_error: Arc<Mutex<Option<String>>>,
    callback_warning: Arc<Mutex<Option<String>>>,
) -> Result<Stream, CaptureError> {
    let sample_rate = config.sample_rate();
    let channels = config.channels();
    let stream_config = config.into();

    let stream = match config.sample_format() {
        SampleFormat::F32 => {
            let error_state = Arc::clone(&callback_error);
            device.build_input_stream(
                stream_config,
                move |data: &[f32], _| {
                    emit_frame(
                        NativeSamples::F32(data),
                        kind,
                        sample_rate,
                        channels,
                        &sink,
                        &error_state,
                    );
                },
                stream_error_callback(callback_error, callback_warning),
                None,
            )
        }
        SampleFormat::I16 => {
            let error_state = Arc::clone(&callback_error);
            device.build_input_stream(
                stream_config,
                move |data: &[i16], _| {
                    emit_frame(
                        NativeSamples::I16(data),
                        kind,
                        sample_rate,
                        channels,
                        &sink,
                        &error_state,
                    );
                },
                stream_error_callback(callback_error, callback_warning),
                None,
            )
        }
        SampleFormat::U16 => {
            let error_state = Arc::clone(&callback_error);
            device.build_input_stream(
                stream_config,
                move |data: &[u16], _| {
                    emit_frame(
                        NativeSamples::U16(data),
                        kind,
                        sample_rate,
                        channels,
                        &sink,
                        &error_state,
                    );
                },
                stream_error_callback(callback_error, callback_warning),
                None,
            )
        }
        other => return Err(CaptureError::UnsupportedSampleFormat(other.to_string())),
    };
    stream.map_err(|error| CaptureError::Stream(error.to_string()))
}

#[allow(clippy::too_many_arguments)]
fn emit_frame(
    samples: NativeSamples<'_>,
    kind: CaptureSourceKind,
    sample_rate: u32,
    channels: u16,
    sink: &CaptureFrameSink,
    callback_error: &Arc<Mutex<Option<String>>>,
) {
    let sample_count = match &samples {
        NativeSamples::F32(samples) => samples.len(),
        NativeSamples::I16(samples) => samples.len(),
        NativeSamples::U16(samples) => samples.len(),
    };
    let sample_frames = sample_count / channels as usize;
    let frame_duration = Duration::from_secs_f64(sample_frames as f64 / sample_rate as f64);
    let frame = AudioFrame::new(
        kind.audio_source(),
        sink.elapsed().saturating_sub(frame_duration),
        sample_rate,
        channels,
        convert_samples_to_f32(samples),
    );
    let result = frame
        .map_err(|error| CaptureError::Stream(error.to_string()))
        .and_then(|frame| sink.try_send(frame));
    if let Err(error) = result {
        record_first_error(callback_error, error.to_string());
    }
}

fn stream_error_callback(
    callback_error: Arc<Mutex<Option<String>>>,
    callback_warning: Arc<Mutex<Option<String>>>,
) -> impl FnMut(cpal::Error) + Send + 'static {
    move |error| {
        let state = if is_recoverable_stream_issue(error.kind()) {
            &callback_warning
        } else {
            &callback_error
        };
        record_first_error(state, error.to_string());
    }
}

fn is_recoverable_stream_issue(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied | ErrorKind::Xrun
    )
}

fn record_first_error(error_state: &Arc<Mutex<Option<String>>>, message: String) {
    if let Ok(mut error) = error_state.lock() {
        if error.is_none() {
            *error = Some(message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xrun_and_runtime_quality_events_are_recoverable() {
        assert!(is_recoverable_stream_issue(ErrorKind::Xrun));
        assert!(is_recoverable_stream_issue(ErrorKind::DeviceChanged));
        assert!(is_recoverable_stream_issue(ErrorKind::RealtimeDenied));
    }

    #[test]
    fn invalidated_and_backend_errors_remain_fatal() {
        assert!(!is_recoverable_stream_issue(ErrorKind::StreamInvalidated));
        assert!(!is_recoverable_stream_issue(ErrorKind::BackendError));
    }
}

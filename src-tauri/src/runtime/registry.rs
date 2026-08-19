use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Serialize;

use crate::audio::capture::{
    capture_channel, AudioCaptureSource, CaptureCoordinator, CaptureSourceKind, SourceSelection,
};
use crate::audio::engine::{AudioEngine, AudioEngineConfig};
use crate::audio::ogg_opus::{OggOpusConfig, OggOpusWriter};
use crate::audio::platform::windows::default_capture_source;

const CAPTURE_POLL_INTERVAL: Duration = Duration::from_millis(20);

pub trait CaptureSourceFactory: Send + Sync + 'static {
    fn create(&self, kind: CaptureSourceKind) -> Result<Box<dyn AudioCaptureSource>, String>;
}

#[derive(Default)]
pub struct NativeCaptureSourceFactory;

impl CaptureSourceFactory for NativeCaptureSourceFactory {
    fn create(&self, kind: CaptureSourceKind) -> Result<Box<dyn AudioCaptureSource>, String> {
        default_capture_source(kind)
            .map(|source| Box::new(source) as Box<dyn AudioCaptureSource>)
            .map_err(|error| error.to_string())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeRecordingStatus {
    Recording,
    Paused,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveRecordingSnapshot {
    pub meeting_id: String,
    pub generation: u64,
    pub status: RuntimeRecordingStatus,
    pub microphone_enabled: bool,
    pub system_enabled: bool,
    pub audio_path: PathBuf,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RecordingCheckpoint {
    pub completed_runs: u64,
    pub recorded_samples: u64,
}

pub struct RecordingRegistry {
    factory: Arc<dyn CaptureSourceFactory>,
    active: Option<ActiveRecording>,
}

impl Default for RecordingRegistry {
    fn default() -> Self {
        Self::new(Arc::new(NativeCaptureSourceFactory))
    }
}

impl RecordingRegistry {
    pub fn new(factory: Arc<dyn CaptureSourceFactory>) -> Self {
        Self {
            factory,
            active: None,
        }
    }

    pub fn start(
        &mut self,
        meeting_id: String,
        generation: u64,
        selection: SourceSelection,
        audio_path: PathBuf,
    ) -> Result<ActiveRecordingSnapshot, String> {
        if self.active.is_some() {
            return Err("已有会议正在录音，请先暂停或结束当前会议。".to_string());
        }
        if !selection.microphone && !selection.system {
            return Err("请至少选择麦克风或系统声音中的一路。".to_string());
        }
        if let Some(parent) = audio_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }

        let (command_tx, command_rx) = mpsc::channel();
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let worker_factory = Arc::clone(&self.factory);
        let worker_path = audio_path.clone();
        let handle = thread::Builder::new()
            .name(format!("aimeeting-recorder-{meeting_id}"))
            .spawn(move || {
                recording_worker(
                    worker_factory,
                    worker_path,
                    generation,
                    selection,
                    command_rx,
                    startup_tx,
                )
            })
            .map_err(|error| format!("无法启动录音线程：{error}"))?;

        startup_rx
            .recv()
            .map_err(|_| "录音线程在初始化前意外退出。".to_string())??;

        let active = ActiveRecording {
            meeting_id,
            generation,
            selection,
            audio_path,
            status: RuntimeRecordingStatus::Recording,
            command_tx,
            handle,
        };
        let snapshot = active.snapshot();
        self.active = Some(active);
        Ok(snapshot)
    }

    pub fn pause(&mut self, meeting_id: &str) -> Result<ActiveRecordingSnapshot, String> {
        let active = self.require_active_mut(meeting_id)?;
        if active.status == RuntimeRecordingStatus::Paused {
            return Ok(active.snapshot());
        }
        request_checkpoint(&active.command_tx, ControlCommandKind::Pause)?;
        active.status = RuntimeRecordingStatus::Paused;
        Ok(active.snapshot())
    }

    pub fn resume(
        &mut self,
        meeting_id: &str,
        generation: u64,
        selection: SourceSelection,
    ) -> Result<ActiveRecordingSnapshot, String> {
        let active = self.require_active_mut(meeting_id)?;
        if active.status == RuntimeRecordingStatus::Recording {
            return Ok(active.snapshot());
        }
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        active
            .command_tx
            .send(ControlCommand::Resume {
                generation,
                selection,
                reply: reply_tx,
            })
            .map_err(|_| "录音线程已经结束，无法继续录音。".to_string())?;
        receive_reply(reply_rx)?;
        active.generation = generation;
        active.selection = selection;
        active.status = RuntimeRecordingStatus::Recording;
        Ok(active.snapshot())
    }

    pub fn stop(&mut self, meeting_id: &str) -> Result<RecordingCheckpoint, String> {
        let active = self
            .active
            .take()
            .ok_or_else(|| "当前没有正在录音的会议。".to_string())?;
        if active.meeting_id != meeting_id {
            self.active = Some(active);
            return Err("请求结束的会议不是当前活动会议。".to_string());
        }

        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        active
            .command_tx
            .send(ControlCommand::Stop { reply: reply_tx })
            .map_err(|_| "录音线程已经意外退出。".to_string())?;
        let checkpoint = receive_reply(reply_rx);
        let joined = active
            .handle
            .join()
            .map_err(|_| "录音线程发生未捕获异常。".to_string())?;
        checkpoint?;
        joined
    }

    pub fn active(&self) -> Option<ActiveRecordingSnapshot> {
        self.active.as_ref().map(ActiveRecording::snapshot)
    }

    fn require_active_mut(&mut self, meeting_id: &str) -> Result<&mut ActiveRecording, String> {
        let active = self
            .active
            .as_mut()
            .ok_or_else(|| "当前没有正在录音的会议。".to_string())?;
        if active.meeting_id != meeting_id {
            return Err("请求操作的会议不是当前活动会议。".to_string());
        }
        Ok(active)
    }
}

struct ActiveRecording {
    meeting_id: String,
    generation: u64,
    selection: SourceSelection,
    audio_path: PathBuf,
    status: RuntimeRecordingStatus,
    command_tx: Sender<ControlCommand>,
    handle: JoinHandle<Result<RecordingCheckpoint, String>>,
}

impl ActiveRecording {
    fn snapshot(&self) -> ActiveRecordingSnapshot {
        ActiveRecordingSnapshot {
            meeting_id: self.meeting_id.clone(),
            generation: self.generation,
            status: self.status,
            microphone_enabled: self.selection.microphone,
            system_enabled: self.selection.system,
            audio_path: self.audio_path.clone(),
        }
    }
}

enum ControlCommand {
    Pause {
        reply: mpsc::SyncSender<Result<RecordingCheckpoint, String>>,
    },
    Resume {
        generation: u64,
        selection: SourceSelection,
        reply: mpsc::SyncSender<Result<RecordingCheckpoint, String>>,
    },
    Stop {
        reply: mpsc::SyncSender<Result<RecordingCheckpoint, String>>,
    },
}

enum ControlCommandKind {
    Pause,
}

fn request_checkpoint(
    command_tx: &Sender<ControlCommand>,
    kind: ControlCommandKind,
) -> Result<RecordingCheckpoint, String> {
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    let command = match kind {
        ControlCommandKind::Pause => ControlCommand::Pause { reply: reply_tx },
    };
    command_tx
        .send(command)
        .map_err(|_| "录音线程已经意外退出。".to_string())?;
    receive_reply(reply_rx)
}

fn receive_reply(
    receiver: Receiver<Result<RecordingCheckpoint, String>>,
) -> Result<RecordingCheckpoint, String> {
    receiver
        .recv()
        .map_err(|_| "录音线程没有返回操作结果。".to_string())?
}

fn recording_worker(
    factory: Arc<dyn CaptureSourceFactory>,
    audio_path: PathBuf,
    initial_generation: u64,
    initial_selection: SourceSelection,
    command_rx: Receiver<ControlCommand>,
    startup_tx: mpsc::SyncSender<Result<(), String>>,
) -> Result<RecordingCheckpoint, String> {
    let mut writer = match OggOpusWriter::create(&audio_path, OggOpusConfig::default()) {
        Ok(writer) => writer,
        Err(error) => {
            let message = format!("无法创建录音文件：{error}");
            let _ = startup_tx.send(Err(message.clone()));
            return Err(message);
        }
    };
    let mut checkpoint = RecordingCheckpoint::default();
    let mut run = match ActiveRun::start(
        factory.as_ref(),
        &mut writer,
        initial_generation,
        initial_selection,
    ) {
        Ok(run) => run,
        Err(error) => {
            let _ = startup_tx.send(Err(error.clone()));
            return Err(error);
        }
    };
    let _ = startup_tx.send(Ok(()));

    loop {
        run.pump(&mut writer)?;
        let command = match command_rx.try_recv() {
            Ok(command) => Some(command),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                let _ = run.finish(&mut writer, &mut checkpoint);
                let _ = writer.finalize();
                return Err("录音控制通道意外断开。".to_string());
            }
        };
        let Some(command) = command else {
            continue;
        };

        match command {
            ControlCommand::Pause { reply } => {
                if let Err(error) = run.finish(&mut writer, &mut checkpoint) {
                    let _ = reply.send(Err(error.clone()));
                    return Err(error);
                }
                let _ = reply.send(Ok(checkpoint));
                let next =
                    wait_while_paused(factory.as_ref(), &mut writer, &mut checkpoint, &command_rx)?;
                match next {
                    PausedExit::Resumed(resumed) => run = *resumed,
                    PausedExit::Stopped => {
                        writer.finalize().map_err(|error| error.to_string())?;
                        return Ok(checkpoint);
                    }
                }
            }
            ControlCommand::Resume { reply, .. } => {
                let _ = reply.send(Err("录音尚未暂停。".to_string()));
            }
            ControlCommand::Stop { reply } => {
                if let Err(error) = run.finish(&mut writer, &mut checkpoint) {
                    let _ = reply.send(Err(error.clone()));
                    return Err(error);
                }
                let _ = reply.send(Ok(checkpoint));
                writer.finalize().map_err(|error| error.to_string())?;
                return Ok(checkpoint);
            }
        }
    }
}

enum PausedExit {
    Resumed(Box<ActiveRun>),
    Stopped,
}

fn wait_while_paused(
    factory: &dyn CaptureSourceFactory,
    writer: &mut OggOpusWriter,
    checkpoint: &mut RecordingCheckpoint,
    command_rx: &Receiver<ControlCommand>,
) -> Result<PausedExit, String> {
    loop {
        match command_rx
            .recv()
            .map_err(|_| "录音控制通道意外断开。".to_string())?
        {
            ControlCommand::Pause { reply } => {
                let _ = reply.send(Ok(*checkpoint));
            }
            ControlCommand::Resume {
                generation,
                selection,
                reply,
            } => match ActiveRun::start(factory, writer, generation, selection) {
                Ok(run) => {
                    let _ = reply.send(Ok(*checkpoint));
                    return Ok(PausedExit::Resumed(Box::new(run)));
                }
                Err(error) => {
                    let _ = reply.send(Err(error));
                }
            },
            ControlCommand::Stop { reply } => {
                let _ = reply.send(Ok(*checkpoint));
                return Ok(PausedExit::Stopped);
            }
        }
    }
}

struct ActiveRun {
    coordinator: CaptureCoordinator,
    receiver: Receiver<crate::audio::frame::AudioFrame>,
    clock: crate::audio::capture::CaptureFrameSink,
    engine: AudioEngine,
}

impl ActiveRun {
    fn start(
        factory: &dyn CaptureSourceFactory,
        writer: &mut OggOpusWriter,
        generation: u64,
        selection: SourceSelection,
    ) -> Result<Self, String> {
        let microphone =
            source_if_enabled(factory, selection.microphone, CaptureSourceKind::Microphone)?;
        let system = source_if_enabled(factory, selection.system, CaptureSourceKind::System)?;
        let mut coordinator = CaptureCoordinator::new(microphone, system);
        let (sink, receiver) = capture_channel(256).map_err(|error| error.to_string())?;
        let clock = sink.clone();
        let engine = AudioEngine::new(AudioEngineConfig::default(), selection)
            .map_err(|error| error.to_string())?;
        let serial =
            u32::try_from(generation).map_err(|_| "录音分段序号超出 Ogg 支持范围。".to_string())?;
        writer
            .begin_run(serial)
            .map_err(|error| error.to_string())?;
        if let Err(error) = coordinator.start(selection, sink) {
            let _ = writer.finish_run();
            return Err(error.to_string());
        }
        Ok(Self {
            coordinator,
            receiver,
            clock,
            engine,
        })
    }

    fn pump(&mut self, writer: &mut OggOpusWriter) -> Result<(), String> {
        match self.receiver.recv_timeout(CAPTURE_POLL_INTERVAL) {
            Ok(frame) => self
                .engine
                .ingest(frame)
                .map_err(|error| error.to_string())?,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("音频采集回调通道已经断开。".to_string());
            }
        }
        self.engine
            .advance_to(self.clock.elapsed())
            .map_err(|error| error.to_string())?;
        drain_recorder(&mut self.engine, writer)?;
        if let Some((kind, error)) = self.coordinator.source_errors().into_iter().next() {
            return Err(format!("{kind:?} 采集失败：{error}"));
        }
        Ok(())
    }

    fn finish(
        &mut self,
        writer: &mut OggOpusWriter,
        checkpoint: &mut RecordingCheckpoint,
    ) -> Result<(), String> {
        self.coordinator.stop().map_err(|error| error.to_string())?;
        for frame in self.receiver.try_iter() {
            self.engine
                .ingest(frame)
                .map_err(|error| error.to_string())?;
        }
        self.engine
            .flush_to(self.clock.elapsed())
            .map_err(|error| error.to_string())?;
        drain_recorder(&mut self.engine, writer)?;
        let summary = writer.finish_run().map_err(|error| error.to_string())?;
        checkpoint.completed_runs += 1;
        checkpoint.recorded_samples += summary.input_samples;
        Ok(())
    }
}

fn source_if_enabled(
    factory: &dyn CaptureSourceFactory,
    enabled: bool,
    kind: CaptureSourceKind,
) -> Result<Option<Box<dyn AudioCaptureSource>>, String> {
    if enabled {
        factory.create(kind).map(Some)
    } else {
        Ok(None)
    }
}

fn drain_recorder(engine: &mut AudioEngine, writer: &mut OggOpusWriter) -> Result<(), String> {
    while let Some(frame) = engine.pop_recorder() {
        writer
            .write_pcm(frame.samples())
            .map_err(|error| error.to_string())?;
    }
    while engine.pop_asr().is_some() {}
    Ok(())
}

pub fn recording_file_size(path: &Path) -> Result<u64, String> {
    std::fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|error| error.to_string())
}

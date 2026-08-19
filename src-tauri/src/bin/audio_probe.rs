#[cfg(target_os = "windows")]
mod windows_probe {
    use std::error::Error;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use aimeeting_lib::audio::capture::{
        capture_channel, AudioCaptureSource, CaptureCoordinator, CaptureSourceKind, SourceSelection,
    };
    use aimeeting_lib::audio::engine::{AudioEngine, AudioEngineConfig};
    use aimeeting_lib::audio::ogg_opus::{OggOpusConfig, OggOpusWriter};
    use aimeeting_lib::audio::platform::windows::{
        default_capture_source, enumerate_audio_devices,
    };

    const DEFAULT_CAPTURE_SECONDS: u64 = 10;

    pub fn run() -> Result<(), Box<dyn Error>> {
        match ProbeOptions::parse(std::env::args().skip(1))? {
            ProbeOptions::List => list_devices(),
            ProbeOptions::Capture {
                selection,
                seconds,
                output,
            } => capture(selection, seconds, output),
        }
    }

    fn list_devices() -> Result<(), Box<dyn Error>> {
        let devices = enumerate_audio_devices()?;
        if devices.is_empty() {
            println!("No supported CPAL audio devices were found.");
            return Ok(());
        }

        println!("Available CPAL audio devices:");
        for device in devices {
            let default = if device.is_default { " default" } else { "" };
            println!(
                "- {:?}{default}: {} | id={} | {:?} {} Hz {} ch",
                device.kind,
                device.name,
                device.id,
                device.sample_format,
                device.sample_rate,
                device.channels
            );
        }
        Ok(())
    }

    fn capture(
        selection: SourceSelection,
        seconds: u64,
        output: PathBuf,
    ) -> Result<(), Box<dyn Error>> {
        let microphone = source_if_enabled(selection.microphone, CaptureSourceKind::Microphone)?;
        let system = source_if_enabled(selection.system, CaptureSourceKind::System)?;
        for source in [microphone.as_ref(), system.as_ref()].into_iter().flatten() {
            let info = source.info();
            println!(
                "Opening {:?}: {} ({} Hz, {} ch, {:?})",
                info.kind, info.name, info.sample_rate, info.channels, info.sample_format
            );
        }

        let mut coordinator = CaptureCoordinator::new(microphone, system);
        let (sink, receiver) = capture_channel(256)?;
        let mut engine = AudioEngine::new(AudioEngineConfig::default(), selection)?;
        let mut writer = OggOpusWriter::create(&output, OggOpusConfig::default())?;
        writer.begin_run(1)?;

        coordinator.start(selection, sink)?;
        println!(
            "Capturing for {seconds} seconds. Output: {}",
            output.display()
        );
        let capture_result = capture_until(
            Instant::now() + Duration::from_secs(seconds),
            &receiver,
            &mut coordinator,
            &mut engine,
            &mut writer,
        );
        let stop_result = coordinator.stop();
        capture_result?;
        stop_result?;

        while let Some(frame) = engine.pop_recorder() {
            writer.write_pcm(frame.samples())?;
        }
        let summary = writer.finish_run()?;
        writer.finalize()?;

        for warning in engine.health_warnings() {
            eprintln!(
                "Warning: {:?} remained near zero for {:.1} seconds",
                warning.source,
                warning.observed_for.as_secs_f32()
            );
        }
        println!(
            "Capture complete: {} samples, {} Opus packets, {} ASR probe drops",
            summary.input_samples,
            summary.packet_count,
            engine.metrics().asr_dropped_frames
        );
        if summary.input_samples == 0 {
            return Err("capture completed without receiving audio samples".into());
        }
        Ok(())
    }

    fn source_if_enabled(
        enabled: bool,
        kind: CaptureSourceKind,
    ) -> Result<Option<Box<dyn AudioCaptureSource>>, Box<dyn Error>> {
        if enabled {
            Ok(Some(Box::new(default_capture_source(kind)?)))
        } else {
            Ok(None)
        }
    }

    fn capture_until(
        deadline: Instant,
        receiver: &std::sync::mpsc::Receiver<aimeeting_lib::audio::frame::AudioFrame>,
        coordinator: &mut CaptureCoordinator,
        engine: &mut AudioEngine,
        writer: &mut OggOpusWriter,
    ) -> Result<(), Box<dyn Error>> {
        while Instant::now() < deadline {
            match receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(frame) => engine.ingest(frame)?,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("capture callback channel disconnected".into());
                }
            }

            while let Some(frame) = engine.pop_recorder() {
                writer.write_pcm(frame.samples())?;
            }
            while engine.pop_asr().is_some() {}

            for (kind, warning) in coordinator.source_warnings() {
                eprintln!("Warning: {kind:?} capture reported a recoverable issue: {warning}");
            }

            if let Some((kind, error)) = coordinator.source_errors().into_iter().next() {
                return Err(format!("{kind:?} capture failed: {error}").into());
            }
        }
        Ok(())
    }

    enum ProbeOptions {
        List,
        Capture {
            selection: SourceSelection,
            seconds: u64,
            output: PathBuf,
        },
    }

    impl ProbeOptions {
        fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, Box<dyn Error>> {
            let mut arguments = arguments.into_iter();
            let mut list = false;
            let mut source = None;
            let mut seconds = DEFAULT_CAPTURE_SECONDS;
            let mut output = None;

            while let Some(argument) = arguments.next() {
                match argument.as_str() {
                    "--list" => list = true,
                    "--source" => {
                        let value = arguments.next().ok_or("--source requires a value")?;
                        source = Some(parse_source(&value)?);
                    }
                    "--seconds" => {
                        let value = arguments.next().ok_or("--seconds requires a value")?;
                        seconds = value.parse()?;
                        if seconds == 0 {
                            return Err("--seconds must be greater than zero".into());
                        }
                    }
                    "--output" => {
                        output = Some(PathBuf::from(
                            arguments.next().ok_or("--output requires a value")?,
                        ));
                    }
                    "--help" | "-h" => return Err(usage().into()),
                    other => return Err(format!("unknown argument: {other}\n{}", usage()).into()),
                }
            }

            if list {
                if source.is_some() || output.is_some() {
                    return Err("--list cannot be combined with capture options".into());
                }
                return Ok(Self::List);
            }

            let selection = source.ok_or_else(usage)?;
            Ok(Self::Capture {
                selection,
                seconds,
                output: output.unwrap_or_else(|| PathBuf::from("audio-probe.opus")),
            })
        }
    }

    fn parse_source(value: &str) -> Result<SourceSelection, Box<dyn Error>> {
        match value {
            "microphone" => Ok(SourceSelection::microphone_only()),
            "system" => Ok(SourceSelection::system_only()),
            "mixed" => Ok(SourceSelection::mixed()),
            _ => Err("--source must be microphone, system, or mixed".into()),
        }
    }

    fn usage() -> String {
        "Usage:\n  audio_probe --list\n  audio_probe --source microphone|system|mixed [--seconds N] [--output FILE]"
            .to_string()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn list_mode_does_not_require_a_capture_source() {
            assert!(matches!(
                ProbeOptions::parse(["--list".to_string()]).unwrap(),
                ProbeOptions::List
            ));
        }

        #[test]
        fn mixed_capture_defaults_to_ten_seconds_and_an_opus_file() {
            let options =
                ProbeOptions::parse(["--source".to_string(), "mixed".to_string()]).unwrap();
            let ProbeOptions::Capture {
                selection,
                seconds,
                output,
            } = options
            else {
                panic!("capture options expected");
            };

            assert_eq!(selection, SourceSelection::mixed());
            assert_eq!(seconds, DEFAULT_CAPTURE_SECONDS);
            assert_eq!(output, PathBuf::from("audio-probe.opus"));
        }
    }
}

#[cfg(target_os = "windows")]
fn main() {
    if let Err(error) = windows_probe::run() {
        eprintln!("{error}");
        std::process::exit(2);
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("audio_probe currently supports Windows only");
    std::process::exit(2);
}

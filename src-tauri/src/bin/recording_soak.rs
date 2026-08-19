use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use aimeeting_lib::audio::capture::SourceSelection;
use aimeeting_lib::audio::engine::{AudioEngine, AudioEngineConfig};
use aimeeting_lib::audio::fake::SyntheticAudioSource;
use aimeeting_lib::audio::frame::AudioSource;
use aimeeting_lib::runtime::registry::RecordingRegistry;

const SYNTHETIC_FRAME_DURATION: Duration = Duration::from_millis(20);

#[derive(Clone, Debug, Eq, PartialEq)]
enum SoakMode {
    Synthetic {
        minutes: u64,
    },
    Realtime {
        minutes: u64,
        selection: SourceSelection,
        output_dir: PathBuf,
    },
}

fn main() -> Result<(), Box<dyn Error>> {
    let mode = parse_args(env::args().skip(1))?;
    match mode {
        SoakMode::Synthetic { minutes } => run_synthetic(minutes),
        SoakMode::Realtime {
            minutes,
            selection,
            output_dir,
        } => run_realtime(minutes, selection, output_dir),
    }
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<SoakMode, String> {
    let mut args = args.into_iter();
    let Some(mode) = args.next() else {
        return Err(usage());
    };
    let minutes = args
        .next()
        .ok_or_else(usage)?
        .parse::<u64>()
        .map_err(|_| "minutes must be a positive integer".to_string())?;
    if minutes == 0 {
        return Err("minutes must be greater than zero".to_string());
    }

    match mode.as_str() {
        "--synthetic-minutes" => {
            if args.next().is_some() {
                return Err(usage());
            }
            Ok(SoakMode::Synthetic { minutes })
        }
        "--realtime-minutes" => {
            let mut source = SourceSelection::mixed();
            let mut output_dir = PathBuf::from("soak-output");
            while let Some(flag) = args.next() {
                match flag.as_str() {
                    "--source" => {
                        source = parse_source(
                            &args
                                .next()
                                .ok_or_else(|| "--source requires a value".to_string())?,
                        )?;
                    }
                    "--output" => {
                        output_dir = PathBuf::from(
                            args.next()
                                .ok_or_else(|| "--output requires a path".to_string())?,
                        );
                    }
                    _ => return Err(format!("unknown option: {flag}\n{}", usage())),
                }
            }
            Ok(SoakMode::Realtime {
                minutes,
                selection: source,
                output_dir,
            })
        }
        _ => Err(usage()),
    }
}

fn parse_source(value: &str) -> Result<SourceSelection, String> {
    match value {
        "microphone" => Ok(SourceSelection::microphone_only()),
        "system" => Ok(SourceSelection::system_only()),
        "mixed" => Ok(SourceSelection::mixed()),
        _ => Err("source must be microphone, system, or mixed".to_string()),
    }
}

fn usage() -> String {
    [
        "usage:",
        "  recording_soak --synthetic-minutes <minutes>",
        "  recording_soak --realtime-minutes <minutes> [--source microphone|system|mixed] [--output <dir>]",
    ]
    .join("\n")
}

fn run_synthetic(minutes: u64) -> Result<(), Box<dyn Error>> {
    let duration = Duration::from_secs(minutes.checked_mul(60).ok_or("duration overflow")?);
    let config = AudioEngineConfig::default();
    let mut engine = AudioEngine::new(config, SourceSelection::mixed())?;
    let mut microphone = SyntheticAudioSource::silence(
        AudioSource::Microphone,
        44_100,
        1,
        SYNTHETIC_FRAME_DURATION,
        duration,
    )?;
    let mut system = SyntheticAudioSource::silence(
        AudioSource::System,
        48_000,
        2,
        SYNTHETIC_FRAME_DURATION,
        duration,
    )?;
    let frame_count = duration.as_millis() / SYNTHETIC_FRAME_DURATION.as_millis();
    let report_every =
        (Duration::from_secs(5 * 60).as_millis() / SYNTHETIC_FRAME_DURATION.as_millis()).max(1);
    let started = Instant::now();
    let mut recorder_samples = 0_u64;
    let mut asr_samples = 0_u64;

    for frame_index in 0..frame_count {
        engine.ingest(microphone.next_frame().ok_or("microphone ended early")?)?;
        engine.ingest(system.next_frame().ok_or("system source ended early")?)?;
        let horizon =
            SYNTHETIC_FRAME_DURATION.mul_f64((frame_index + 1) as f64) + config.alignment_latency;
        engine.advance_to(horizon)?;
        drain_engine(&mut engine, &mut recorder_samples, &mut asr_samples);

        if (frame_index + 1) % report_every == 0 || frame_index + 1 == frame_count {
            let simulated = SYNTHETIC_FRAME_DURATION.mul_f64((frame_index + 1) as f64);
            report("synthetic", simulated, recorder_samples, None);
        }
    }

    engine.flush_to(duration)?;
    drain_engine(&mut engine, &mut recorder_samples, &mut asr_samples);
    let expected_samples = duration.as_secs() * u64::from(config.output_sample_rate);
    if recorder_samples != expected_samples {
        return Err(format!(
            "recorder sample mismatch: expected {expected_samples}, got {recorder_samples}"
        )
        .into());
    }
    if asr_samples != expected_samples {
        return Err(
            format!("ASR sample mismatch: expected {expected_samples}, got {asr_samples}").into(),
        );
    }
    let metrics = engine.metrics();
    if metrics.recorder_failures != 0 || metrics.asr_dropped_frames != 0 {
        return Err(format!("unexpected engine degradation: {metrics:?}").into());
    }

    println!(
        "PASS synthetic_minutes={minutes} samples={recorder_samples} wall_seconds={:.2}",
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

fn drain_engine(engine: &mut AudioEngine, recorder_samples: &mut u64, asr_samples: &mut u64) {
    while let Some(frame) = engine.pop_recorder() {
        *recorder_samples += frame.sample_frames() as u64;
    }
    while let Some(frame) = engine.pop_asr() {
        *asr_samples += frame.sample_frames() as u64;
    }
}

fn run_realtime(
    minutes: u64,
    selection: SourceSelection,
    output_dir: PathBuf,
) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(&output_dir)?;
    let output_path = output_dir.join(format!(
        "recording-soak-{}m-{}.opus",
        minutes,
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    ));
    let meeting_id = format!("recording-soak-{}", uuid::Uuid::new_v4());
    let mut registry = RecordingRegistry::default();
    registry.start(meeting_id.clone(), 1, selection, output_path.clone())?;

    let duration = Duration::from_secs(minutes.checked_mul(60).ok_or("duration overflow")?);
    let started = Instant::now();
    let report_interval = Duration::from_secs(5);
    println!(
        "START realtime_minutes={minutes} microphone={} system={} output={}",
        selection.microphone,
        selection.system,
        output_path.display()
    );

    while started.elapsed() < duration {
        thread::sleep(report_interval.min(duration.saturating_sub(started.elapsed())));
        let file_size = fs::metadata(&output_path).map(|value| value.len()).ok();
        report("realtime", started.elapsed(), 0, file_size);
    }

    let checkpoint = registry.stop(&meeting_id)?;
    let file_size = fs::metadata(&output_path)?.len();
    if file_size == 0 || checkpoint.recorded_samples == 0 {
        return Err("recording completed without audio data".into());
    }
    println!(
        "PASS realtime_minutes={minutes} recorded_samples={} file_bytes={} output={}",
        checkpoint.recorded_samples,
        file_size,
        output_path.display()
    );
    Ok(())
}

fn report(mode: &str, elapsed: Duration, samples: u64, file_size: Option<u64>) {
    let metrics = process_metrics();
    println!(
        "METRIC mode={mode} elapsed_seconds={:.1} samples={samples} file_bytes={} working_set_mib={:.1} cpu_seconds={:.2}",
        elapsed.as_secs_f64(),
        file_size.unwrap_or_default(),
        metrics.working_set_bytes as f64 / 1024.0 / 1024.0,
        metrics.cpu_seconds
    );
}

#[derive(Clone, Copy, Debug, Default)]
struct ProcessMetrics {
    working_set_bytes: u64,
    cpu_seconds: f64,
}

#[cfg(windows)]
fn process_metrics() -> ProcessMetrics {
    use std::mem::size_of;
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

    let process = unsafe { GetCurrentProcess() };
    let mut memory = PROCESS_MEMORY_COUNTERS {
        cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    let memory_ok = unsafe { K32GetProcessMemoryInfo(process, &mut memory, memory.cb) } != 0;

    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let times_ok =
        unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) } != 0;
    let cpu_100ns = if times_ok {
        filetime_value(kernel).saturating_add(filetime_value(user))
    } else {
        0
    };

    ProcessMetrics {
        working_set_bytes: if memory_ok {
            memory.WorkingSetSize as u64
        } else {
            0
        },
        cpu_seconds: cpu_100ns as f64 / 10_000_000.0,
    }
}

#[cfg(windows)]
fn filetime_value(value: windows_sys::Win32::Foundation::FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}

#[cfg(not(windows))]
fn process_metrics() -> ProcessMetrics {
    ProcessMetrics::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_synthetic_mode() {
        assert_eq!(
            parse_args(["--synthetic-minutes".to_string(), "30".to_string()]),
            Ok(SoakMode::Synthetic { minutes: 30 })
        );
    }

    #[test]
    fn parses_realtime_mode_and_source() {
        assert_eq!(
            parse_args([
                "--realtime-minutes".to_string(),
                "1".to_string(),
                "--source".to_string(),
                "microphone".to_string(),
                "--output".to_string(),
                "custom-output".to_string(),
            ]),
            Ok(SoakMode::Realtime {
                minutes: 1,
                selection: SourceSelection::microphone_only(),
                output_dir: PathBuf::from("custom-output"),
            })
        );
    }

    #[test]
    fn rejects_zero_minutes() {
        assert_eq!(
            parse_args(["--synthetic-minutes".to_string(), "0".to_string()]),
            Err("minutes must be greater than zero".to_string())
        );
    }
}

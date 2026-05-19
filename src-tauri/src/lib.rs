use base64::Engine;
use reqwest::multipart;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
    thread,
    time::Duration,
};
use tauri::Manager;

#[derive(Serialize)]
struct CaptureCapabilities {
    platform: String,
    system_audio: String,
    microphone: String,
    note: String,
}

#[derive(Default)]
struct AsrState {
    child: Mutex<Option<LocalAsrProcess>>,
}

struct LocalAsrProcess {
    child: Child,
    url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AsrRuntimeStatus {
    whisper_server_path: Option<String>,
    model_path: Option<String>,
    vad_model_path: Option<String>,
    local_server_url: Option<String>,
    local_ready: bool,
    runtime_label: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TranscribeAudioRequest {
    audio_base64: String,
    mime_type: String,
    cloud_base_url: String,
    cloud_api_key: String,
    cloud_model: String,
    language: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TranscribeAudioResponse {
    text: String,
    provider_label: String,
    used_fallback: bool,
    warning: Option<String>,
    local_server_url: Option<String>,
}

#[derive(Deserialize)]
struct CloudTranscriptionResponse {
    text: Option<String>,
}

#[derive(Deserialize)]
struct WhisperServerResponse {
    text: Option<String>,
}

struct WhisperModelCandidate {
    file_name: &'static str,
    min_size_bytes: u64,
}

const LOCAL_ASR_START_PORT: u16 = 18091;
const WHISPER_SERVER_ENV: &str = "AIMEETING_WHISPER_SERVER";
const WHISPER_MODEL_ENV: &str = "AIMEETING_WHISPER_MODEL";
const SILERO_VAD_MODEL_ENV: &str = "AIMEETING_SILERO_VAD_MODEL";

#[tauri::command]
fn capture_capabilities() -> CaptureCapabilities {
    CaptureCapabilities {
        platform: std::env::consts::OS.to_string(),
        system_audio: "windows-wasapi-loopback-planned".to_string(),
        microphone: "cpal-or-wasapi-planned".to_string(),
        note: "The React prototype probes browser media. Native capture belongs behind this command boundary.".to_string(),
    }
}

#[tauri::command]
fn secure_key_storage_status() -> String {
    "planned: use Windows Credential Manager or a Tauri keyring plugin before persisting provider keys".to_string()
}

#[tauri::command]
fn asr_runtime_status(state: tauri::State<'_, AsrState>) -> AsrRuntimeStatus {
    let local_server_url = state
        .child
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(|process| process.url.clone()));
    let whisper_server_path = find_whisper_server();
    let model_path = find_whisper_model();
    let vad_model_path = find_silero_vad_model();
    let local_available =
        whisper_server_path.is_some() && model_path.is_some() && vad_model_path.is_some();
    let runtime_label = model_path
        .as_ref()
        .and_then(|path| path.file_stem())
        .and_then(|name| name.to_str())
        .map(|name| format!("whisper.cpp {name} + Silero VAD"))
        .unwrap_or_else(|| "whisper.cpp + Silero VAD".to_string());

    AsrRuntimeStatus {
        whisper_server_path: whisper_server_path.map(|path| path.display().to_string()),
        model_path: model_path.map(|path| path.display().to_string()),
        vad_model_path: vad_model_path.map(|path| path.display().to_string()),
        local_ready: local_server_url.is_some() || local_available,
        local_server_url,
        runtime_label,
    }
}

#[tauri::command]
async fn transcribe_audio_chunk(
    state: tauri::State<'_, AsrState>,
    request: TranscribeAudioRequest,
) -> Result<TranscribeAudioResponse, String> {
    let audio = base64::engine::general_purpose::STANDARD
        .decode(request.audio_base64.trim())
        .map_err(|error| format!("音频数据不是有效的 base64：{error}"))?;

    if audio.is_empty() {
        return Err("音频片段为空。".to_string());
    }

    let wants_cloud = !request.cloud_base_url.trim().is_empty()
        && !request.cloud_api_key.trim().is_empty()
        && !request.cloud_model.trim().is_empty();

    if wants_cloud {
        match transcribe_with_cloud(&request, audio.clone()).await {
            Ok(text) => {
                return Ok(TranscribeAudioResponse {
                    text,
                    provider_label: format!("{} via cloud ASR", request.cloud_model.trim()),
                    used_fallback: false,
                    warning: None,
                    local_server_url: None,
                });
            }
            Err(error) => {
                let local = transcribe_with_local(&state, &request, &audio).await?;
                return Ok(TranscribeAudioResponse {
                    warning: Some(format!("云端 ASR 失败，已自动切换本地 Whisper：{error}")),
                    used_fallback: true,
                    ..local
                });
            }
        }
    }

    transcribe_with_local(&state, &request, &audio).await
}

async fn transcribe_with_cloud(
    request: &TranscribeAudioRequest,
    audio: Vec<u8>,
) -> Result<String, String> {
    let base_url = request.cloud_base_url.trim().trim_end_matches('/');
    let endpoint = format!("{base_url}/audio/transcriptions");
    let extension = audio_extension(&request.mime_type);
    let part = multipart::Part::bytes(audio)
        .file_name(format!("chunk.{extension}"))
        .mime_str(&request.mime_type)
        .map_err(|error| format!("音频 MIME 类型无效：{error}"))?;
    let form = multipart::Form::new()
        .text("model", request.cloud_model.trim().to_string())
        .text(
            "language",
            cloud_language_code(&request.language).to_string(),
        )
        .part("file", part);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(|error| format!("无法创建 ASR HTTP 客户端：{error}"))?;
    let response = client
        .post(endpoint)
        .bearer_auth(request.cloud_api_key.trim())
        .multipart(form)
        .send()
        .await
        .map_err(|error| format!("云端 ASR 请求失败：{error}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let detail = response
            .text()
            .await
            .unwrap_or_else(|_| "无法读取错误详情".to_string());
        return Err(format!(
            "云端 ASR 返回 {status}: {}",
            truncate(&detail, 240)
        ));
    }

    let payload = response
        .json::<CloudTranscriptionResponse>()
        .await
        .map_err(|error| format!("云端 ASR 响应不是预期 JSON：{error}"))?;
    clean_asr_text(payload.text.unwrap_or_default())
        .ok_or_else(|| "云端 ASR 没有返回可用文本。".to_string())
}

async fn transcribe_with_local(
    state: &tauri::State<'_, AsrState>,
    request: &TranscribeAudioRequest,
    audio: &[u8],
) -> Result<TranscribeAudioResponse, String> {
    let server_url = ensure_local_asr_server(state).await?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| format!("无法创建本地 ASR HTTP 客户端：{error}"))?;
    let extension = audio_extension(&request.mime_type);
    let part = multipart::Part::bytes(audio.to_vec())
        .file_name(format!("chunk.{extension}"))
        .mime_str(&request.mime_type)
        .map_err(|error| format!("音频 MIME 类型无效：{error}"))?;
    let form = multipart::Form::new()
        .text("response_format", "json")
        .text("language", language_code(&request.language).to_string())
        .text("temperature", "0")
        .text("temperature_inc", "0")
        .part("file", part);
    let response = client
        .post(format!("{server_url}/inference"))
        .multipart(form)
        .send()
        .await
        .map_err(|error| format!("本地 Whisper 请求失败：{error}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let detail = response
            .text()
            .await
            .unwrap_or_else(|_| "无法读取错误详情".to_string());
        return Err(format!(
            "本地 Whisper 返回 {status}: {}",
            truncate(&detail, 240)
        ));
    }

    let payload = response
        .json::<WhisperServerResponse>()
        .await
        .map_err(|error| format!("本地 Whisper 响应不是预期 JSON：{error}"))?;
    let text = clean_asr_text(payload.text.unwrap_or_default()).unwrap_or_default();

    Ok(TranscribeAudioResponse {
        text,
        provider_label: "local Whisper small + Silero VAD".to_string(),
        used_fallback: false,
        warning: None,
        local_server_url: Some(server_url),
    })
}

async fn ensure_local_asr_server(state: &tauri::State<'_, AsrState>) -> Result<String, String> {
    if let Some(url) = current_local_url(state) {
        if local_server_ready(&url).await {
            return Ok(url);
        }
        stop_local_asr_server(state);
    }

    let whisper_server = find_whisper_server().ok_or_else(|| {
        format!("找不到 whisper-server.exe。可设置 {WHISPER_SERVER_ENV} 指向 whisper.cpp 的 whisper-server.exe。")
    })?;
    let model = find_whisper_model().ok_or_else(|| {
        format!("找不到可用 Whisper 模型。可设置 {WHISPER_MODEL_ENV} 指向完整的 ggml-large-v3-turbo-q5_0.bin 或 ggml-small.bin。")
    })?;
    let vad_model = find_silero_vad_model().ok_or_else(|| {
        format!(
            "找不到 Silero VAD 模型。可设置 {SILERO_VAD_MODEL_ENV} 指向 ggml-silero-v5.1.2.bin。"
        )
    })?;
    let port = find_free_port(LOCAL_ASR_START_PORT, 24)
        .ok_or_else(|| "找不到可用于本地 ASR 的空闲端口。".to_string())?;
    let url = format!("http://127.0.0.1:{port}");
    let mut command = Command::new(whisper_server);
    command
        .arg("-m")
        .arg(model)
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .arg("-l")
        .arg("auto")
        .arg("-mc")
        .arg("0")
        .arg("-nf")
        .arg("-nt")
        .arg("-sns")
        .arg("-nth")
        .arg("0.75")
        .arg("--vad")
        .arg("--vad-model")
        .arg(vad_model)
        .arg("-vsd")
        .arg("450")
        .arg("-vspd")
        .arg("250")
        .arg("-vp")
        .arg("80")
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }

    let child = command
        .spawn()
        .map_err(|error| format!("无法启动本地 Whisper 服务：{error}"))?;

    {
        let mut guard = state
            .child
            .lock()
            .map_err(|_| "本地 ASR 状态锁不可用。".to_string())?;
        *guard = Some(LocalAsrProcess {
            child,
            url: url.clone(),
        });
    }

    for _ in 0..120 {
        if local_server_ready(&url).await {
            return Ok(url);
        }
        thread::sleep(Duration::from_millis(500));
    }

    stop_local_asr_server(state);
    Err("本地 Whisper 服务启动超时。".to_string())
}

async fn local_server_ready(url: &str) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(client) => client,
        Err(_) => return false,
    };
    client
        .get(url)
        .send()
        .await
        .map(|response| response.status().is_success())
        .unwrap_or(false)
}

fn current_local_url(state: &tauri::State<'_, AsrState>) -> Option<String> {
    let mut guard = state.child.lock().ok()?;
    let process = guard.as_mut()?;

    match process.child.try_wait() {
        Ok(Some(_)) => {
            *guard = None;
            None
        }
        Ok(None) => Some(process.url.clone()),
        Err(_) => {
            *guard = None;
            None
        }
    }
}

fn stop_local_asr_server(state: &tauri::State<'_, AsrState>) {
    if let Ok(mut guard) = state.child.lock() {
        if let Some(mut process) = guard.take() {
            let _ = process.child.kill();
            let _ = process.child.wait();
        }
    }
}

fn find_free_port(start: u16, attempts: u16) -> Option<u16> {
    (start..start.saturating_add(attempts)).find(|port| {
        TcpListener::bind(("127.0.0.1", *port))
            .map(|listener| {
                drop(listener);
                true
            })
            .unwrap_or(false)
    })
}

fn find_whisper_server() -> Option<PathBuf> {
    if let Some(path) = existing_path_from_env(WHISPER_SERVER_ENV) {
        return Some(path);
    }

    let home = user_home()?;
    for candidate in [
        home.join(".aimeeting/tools/whisper.cpp/Release/whisper-server.exe"),
        home.join(".aimeeting/tools/whisper.cpp/whisper-server.exe"),
    ] {
        if candidate.exists() {
            return Some(candidate);
        }
    }

    find_on_path("whisper-server.exe")
}

fn find_whisper_model() -> Option<PathBuf> {
    if let Some(path) = existing_path_from_env(WHISPER_MODEL_ENV) {
        return Some(path);
    }

    let home = user_home()?;
    let model_names = [
        WhisperModelCandidate {
            file_name: "ggml-large-v3-turbo-q5_0.bin",
            min_size_bytes: 500_000_000,
        },
        WhisperModelCandidate {
            file_name: "ggml-large-v3-turbo-q8_0.bin",
            min_size_bytes: 780_000_000,
        },
        WhisperModelCandidate {
            file_name: "ggml-large-v3-turbo.bin",
            min_size_bytes: 1_500_000_000,
        },
        WhisperModelCandidate {
            file_name: "ggml-small.bin",
            min_size_bytes: 450_000_000,
        },
        WhisperModelCandidate {
            file_name: "ggml-small-q8_0.bin",
            min_size_bytes: 240_000_000,
        },
        WhisperModelCandidate {
            file_name: "ggml-small-q5_0.bin",
            min_size_bytes: 170_000_000,
        },
        WhisperModelCandidate {
            file_name: "ggml-small-q5_1.bin",
            min_size_bytes: 170_000_000,
        },
        WhisperModelCandidate {
            file_name: "ggml-base.bin",
            min_size_bytes: 130_000_000,
        },
        WhisperModelCandidate {
            file_name: "ggml-base-q8_0.bin",
            min_size_bytes: 70_000_000,
        },
        WhisperModelCandidate {
            file_name: "ggml-tiny.bin",
            min_size_bytes: 70_000_000,
        },
    ];
    for root in [
        home.join(".aimeeting/models/whisper.cpp"),
        home.join(".lmstudio/models"),
    ] {
        if let Some(path) = find_prioritized_model(&root, &model_names) {
            return Some(path);
        }
    }

    None
}

fn find_silero_vad_model() -> Option<PathBuf> {
    if let Some(path) = existing_path_from_env(SILERO_VAD_MODEL_ENV) {
        return Some(path);
    }

    let home = user_home()?;
    let model_names = [
        "ggml-silero-v5.1.2.bin",
        "ggml-silero-v5.1.1.bin",
        "ggml-silero.bin",
    ];
    for root in [
        home.join(".aimeeting/models/whisper.cpp"),
        home.join(".lmstudio/models"),
    ] {
        if let Some(path) = find_prioritized_file(&root, &model_names) {
            return Some(path);
        }
    }

    None
}

fn existing_path_from_env(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os(name)?);
    path.exists().then_some(path)
}

fn find_prioritized_file(root: &Path, file_names: &[&str]) -> Option<PathBuf> {
    for file_name in file_names {
        if let Some(path) = find_file_named(root, file_name) {
            return Some(path);
        }
    }
    None
}

fn find_prioritized_model(root: &Path, candidates: &[WhisperModelCandidate]) -> Option<PathBuf> {
    for candidate in candidates {
        if let Some(path) = find_file_named(root, candidate.file_name) {
            if file_size(&path).is_some_and(|size| size >= candidate.min_size_bytes) {
                return Some(path);
            }
        }
    }
    None
}

fn file_size(path: &Path) -> Option<u64> {
    fs::metadata(path).ok().map(|metadata| metadata.len())
}

fn find_file_named(root: &Path, file_name: &str) -> Option<PathBuf> {
    if !root.exists() {
        return None;
    }

    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                directories.push(path);
                continue;
            }
            let matches = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case(file_name));
            if matches {
                return Some(path);
            }
        }
    }

    None
}

fn find_on_path(file_name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|path| path.join(file_name))
            .find(|candidate| candidate.exists())
    })
}

fn user_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn audio_extension(mime_type: &str) -> &'static str {
    if mime_type.contains("wav") {
        "wav"
    } else if mime_type.contains("mp4") || mime_type.contains("m4a") {
        "m4a"
    } else if mime_type.contains("ogg") {
        "ogg"
    } else {
        "webm"
    }
}

fn language_code(language: &str) -> &'static str {
    if language.starts_with("zh") {
        "zh"
    } else if language.starts_with("ja") {
        "ja"
    } else {
        "en"
    }
}

fn cloud_language_code(language: &str) -> &'static str {
    language_code(language)
}

fn clean_asr_text(raw: String) -> Option<String> {
    let mut text = raw
        .lines()
        .map(|line| {
            let line = line.trim();
            if line.starts_with('[') {
                if let Some(index) = line.find(']') {
                    return line[index + 1..].trim();
                }
            }
            line
        })
        .filter(|line| {
            !line.is_empty()
                && !line.eq_ignore_ascii_case("[BLANK_AUDIO]")
                && !line.eq_ignore_ascii_case("(silence)")
                && !line.eq_ignore_ascii_case("[silence]")
        })
        .collect::<Vec<_>>()
        .join(" ");
    if let Some(index) = text.find("<asr_text>") {
        text = text[index + "<asr_text>".len()..].to_string();
    }
    let text = text
        .replace("</asr_text>", "")
        .replace("<|im_end|>", "")
        .trim()
        .to_string();
    if is_instruction_echo(&text) || is_common_silence_hallucination(&text) {
        return None;
    }
    (!text.is_empty()).then_some(text)
}

fn is_instruction_echo(text: &str) -> bool {
    let normalized = normalize_asr_text(text);
    matches!(
        normalized.as_str(),
        "请转写为简体中文"
            | "请转写成简体中文"
            | "请转写为中文"
            | "以下是会议录音请转写为简体中文"
            | "以下是会议录音请转写成简体中文"
    )
}

fn is_common_silence_hallucination(text: &str) -> bool {
    let normalized = normalize_asr_text(text);
    matches!(
        normalized.as_str(),
        "还有一点点点" | "还有一点点" | "谢谢观看" | "感谢观看" | "字幕由Amaraorg社区提供"
    )
}

fn normalize_asr_text(text: &str) -> String {
    text.chars()
        .filter(|character| {
            !character.is_whitespace()
                && !matches!(
                    character,
                    '。' | '，'
                        | ','
                        | '.'
                        | '！'
                        | '!'
                        | '？'
                        | '?'
                        | '、'
                        | '：'
                        | ':'
                        | '；'
                        | ';'
                        | '“'
                        | '”'
                        | '"'
                )
        })
        .collect()
}

fn truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AsrState::default())
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                let state = window.state::<AsrState>();
                stop_local_asr_server(&state);
            }
        })
        .invoke_handler(tauri::generate_handler![
            capture_capabilities,
            secure_key_storage_status,
            asr_runtime_status,
            transcribe_audio_chunk
        ])
        .run(tauri::generate_context!())
        .expect("error while running AIMeeting");
}

#[cfg(test)]
mod tests {
    use super::clean_asr_text;

    #[test]
    fn clean_asr_text_drops_instruction_echoes() {
        assert_eq!(clean_asr_text("请转写为简体中文。".to_string()), None);
        assert_eq!(
            clean_asr_text("以下是会议录音，请转写为简体中文。".to_string()),
            None
        );
    }

    #[test]
    fn clean_asr_text_keeps_real_transcript() {
        assert_eq!(
            clean_asr_text("我们现在讨论本地 Whisper 接入。".to_string()).as_deref(),
            Some("我们现在讨论本地 Whisper 接入。")
        );
    }
}

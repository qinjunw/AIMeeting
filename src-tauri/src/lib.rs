use base64::Engine;
use reqwest::multipart;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    net::TcpListener,
    path::PathBuf,
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
    llama_server_path: Option<String>,
    model_path: Option<String>,
    mmproj_path: Option<String>,
    local_server_url: Option<String>,
    local_ready: bool,
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
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: String,
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Option<Vec<ModelItem>>,
    models: Option<Vec<ModelItem>>,
}

#[derive(Deserialize)]
struct ModelItem {
    id: Option<String>,
    name: Option<String>,
    model: Option<String>,
}

const LOCAL_ASR_MODEL_ID: &str = "qwen3-asr-1.7b";
const LOCAL_ASR_START_PORT: u16 = 18081;

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

    AsrRuntimeStatus {
        llama_server_path: find_llama_server().map(|path| path.display().to_string()),
        model_path: find_qwen_asr_model().map(|path| path.display().to_string()),
        mmproj_path: find_qwen_asr_mmproj().map(|path| path.display().to_string()),
        local_ready: local_server_url.is_some(),
        local_server_url,
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
                    warning: Some(format!("云端 ASR 失败，已自动切换本地 Qwen3-ASR：{error}")),
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
        .text("language", cloud_language_code(&request.language).to_string())
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
        return Err(format!("云端 ASR 返回 {status}: {}", truncate(&detail, 240)));
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
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|error| format!("无法创建本地 ASR HTTP 客户端：{error}"))?;
    let body = serde_json::json!({
        "model": LOCAL_ASR_MODEL_ID,
        "messages": [
            {
                "role": "user",
                "content": [
                    {
                        "type": "text",
                        "text": format!(
                            "Transcribe the audio in {}. Return only the recognized text.",
                            local_language_hint(&request.language)
                        )
                    },
                    {
                        "type": "input_audio",
                        "input_audio": {
                            "data": base64::engine::general_purpose::STANDARD.encode(audio),
                            "format": audio_format(&request.mime_type)
                        }
                    }
                ]
            }
        ],
        "temperature": 0,
        "max_tokens": 192
    });
    let response = client
        .post(format!("{server_url}/v1/chat/completions"))
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("本地 Qwen3-ASR 请求失败：{error}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let detail = response
            .text()
            .await
            .unwrap_or_else(|_| "无法读取错误详情".to_string());
        return Err(format!("本地 Qwen3-ASR 返回 {status}: {}", truncate(&detail, 240)));
    }

    let payload = response
        .json::<ChatCompletionResponse>()
        .await
        .map_err(|error| format!("本地 Qwen3-ASR 响应不是预期 JSON：{error}"))?;
    let raw = payload
        .choices
        .first()
        .map(|choice| choice.message.content.clone())
        .unwrap_or_default();
    let text = clean_asr_text(raw).ok_or_else(|| "本地 Qwen3-ASR 没有返回可用文本。".to_string())?;

    Ok(TranscribeAudioResponse {
        text,
        provider_label: "local Qwen3-ASR".to_string(),
        used_fallback: false,
        warning: None,
        local_server_url: Some(server_url),
    })
}

async fn ensure_local_asr_server(state: &tauri::State<'_, AsrState>) -> Result<String, String> {
    if let Some(url) = current_local_url(state) {
        if local_server_has_model(&url).await {
            return Ok(url);
        }
        stop_local_asr_server(state);
    }

    let llama_server = find_llama_server().ok_or_else(|| {
        "找不到 llama-server.exe。请确认已通过 winget 安装 ggml.llamacpp。".to_string()
    })?;
    let model = find_qwen_asr_model().ok_or_else(|| {
        "找不到 Qwen3-ASR 主模型：Qwen3-ASR-1.7B-Q8_0.gguf。".to_string()
    })?;
    let mmproj = find_qwen_asr_mmproj().ok_or_else(|| {
        "找不到 Qwen3-ASR mmproj：mmproj-Qwen3-ASR-1.7B-bf16.gguf。".to_string()
    })?;
    let port = find_free_port(LOCAL_ASR_START_PORT, 24)
        .ok_or_else(|| "找不到可用于本地 ASR 的空闲端口。".to_string())?;
    let url = format!("http://127.0.0.1:{port}");
    let mut command = Command::new(llama_server);
    command
        .arg("-m")
        .arg(model)
        .arg("--mmproj")
        .arg(mmproj)
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .arg("--alias")
        .arg(LOCAL_ASR_MODEL_ID)
        .arg("--jinja")
        .arg("-n")
        .arg("192")
        .arg("--no-webui")
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }

    let child = command
        .spawn()
        .map_err(|error| format!("无法启动本地 Qwen3-ASR 服务：{error}"))?;

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

    for _ in 0..90 {
        if local_server_has_model(&url).await {
            return Ok(url);
        }
        thread::sleep(Duration::from_millis(500));
    }

    stop_local_asr_server(state);
    Err("本地 Qwen3-ASR 服务启动超时。".to_string())
}

async fn local_server_has_model(url: &str) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(client) => client,
        Err(_) => return false,
    };
    let response = match client.get(format!("{url}/v1/models")).send().await {
        Ok(response) if response.status().is_success() => response,
        _ => return false,
    };
    let payload = match response.json::<ModelsResponse>().await {
        Ok(payload) => payload,
        Err(_) => return false,
    };
    let mut models = Vec::new();
    if let Some(data) = payload.data {
        models.extend(data);
    }
    if let Some(data) = payload.models {
        models.extend(data);
    }
    models.iter().any(|item| {
        item.id.as_deref() == Some(LOCAL_ASR_MODEL_ID)
            || item.name.as_deref() == Some(LOCAL_ASR_MODEL_ID)
            || item.model.as_deref() == Some(LOCAL_ASR_MODEL_ID)
    })
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

fn find_llama_server() -> Option<PathBuf> {
    let home = user_home()?;
    let winget_root = home.join("AppData/Local/Microsoft/WinGet/Packages");
    if let Ok(entries) = fs::read_dir(&winget_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("ggml.llamacpp_"))
            {
                let candidate = path.join("llama-server.exe");
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }

    find_on_path("llama-server.exe")
}

fn find_qwen_asr_model() -> Option<PathBuf> {
    let home = user_home()?;
    let path = home.join(".lmstudio/models/ggml-org/Qwen3-ASR-1.7B-GGUF/Qwen3-ASR-1.7B-Q8_0.gguf");
    path.exists().then_some(path)
}

fn find_qwen_asr_mmproj() -> Option<PathBuf> {
    let home = user_home()?;
    let path = home.join(".lmstudio/models/ggml-org/Qwen3-ASR-1.7B-GGUF/mmproj-Qwen3-ASR-1.7B-bf16.gguf");
    path.exists().then_some(path)
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

fn audio_format(mime_type: &str) -> &'static str {
    if mime_type.contains("wav") {
        "wav"
    } else if mime_type.contains("mp4") || mime_type.contains("m4a") {
        "mp4"
    } else if mime_type.contains("ogg") {
        "ogg"
    } else {
        "webm"
    }
}

fn cloud_language_code(language: &str) -> &'static str {
    if language.starts_with("zh") {
        "zh"
    } else if language.starts_with("ja") {
        "ja"
    } else {
        "en"
    }
}

fn local_language_hint(language: &str) -> &'static str {
    if language.starts_with("zh") {
        "Chinese"
    } else if language.starts_with("ja") {
        "Japanese"
    } else {
        "English"
    }
}

fn clean_asr_text(raw: String) -> Option<String> {
    let mut text = raw.trim().to_string();
    if let Some(index) = text.find("<asr_text>") {
        text = text[index + "<asr_text>".len()..].to_string();
    }
    let text = text
        .replace("</asr_text>", "")
        .replace("<|im_end|>", "")
        .trim()
        .to_string();
    (!text.is_empty()).then_some(text)
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

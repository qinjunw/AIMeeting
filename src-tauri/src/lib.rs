use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use reqwest::multipart;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tauri::{AppHandle, Emitter};
use tokio::{
    sync::{mpsc, oneshot, Mutex},
    time::timeout,
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
};

const DASHSCOPE_STREAMING_WS_URL: &str = "wss://dashscope.aliyuncs.com/api-ws/v1/inference";
const STREAMING_AUDIO_QUEUE_CAPACITY: usize = 60;
const STREAMING_START_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Serialize)]
struct CaptureCapabilities {
    platform: String,
    system_audio: String,
    microphone: String,
    note: String,
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
}

#[derive(Clone)]
struct StreamingAsrSessionHandle {
    audio_tx: mpsc::Sender<StreamingAsrCommand>,
}

#[derive(Default)]
struct StreamingAsrState {
    sessions: Arc<Mutex<HashMap<String, StreamingAsrSessionHandle>>>,
}

struct StreamingAsrSession {
    session_id: String,
    meeting_id: String,
    recording_run_id: String,
    task_id: String,
    api_key: String,
    model: String,
    language: String,
    provider_label: String,
}

enum StreamingAsrCommand {
    Audio(Vec<u8>),
    Finish,
    Cancel,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartStreamingAsrRequest {
    cloud_base_url: String,
    cloud_api_key: String,
    cloud_model: String,
    language: String,
    meeting_id: String,
    recording_run_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StartStreamingAsrResponse {
    session_id: String,
    provider_label: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PushStreamingAsrAudioRequest {
    session_id: String,
    audio_base64: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreamingAsrSessionRequest {
    session_id: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StreamingAsrEvent {
    session_id: String,
    meeting_id: String,
    recording_run_id: String,
    status: String,
    text: String,
    begin_ms: Option<i64>,
    end_ms: Option<i64>,
    provider_label: String,
    error_message: Option<String>,
}

#[derive(Deserialize)]
struct CloudTranscriptionResponse {
    text: Option<String>,
}

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
async fn transcribe_audio_chunk(
    request: TranscribeAudioRequest,
) -> Result<TranscribeAudioResponse, String> {
    let audio = base64::engine::general_purpose::STANDARD
        .decode(request.audio_base64.trim())
        .map_err(|error| format!("音频数据不是有效的 base64：{error}"))?;

    if audio.is_empty() {
        return Err("音频片段为空。".to_string());
    }

    if request.cloud_base_url.trim().is_empty()
        || request.cloud_api_key.trim().is_empty()
        || request.cloud_model.trim().is_empty()
    {
        return Err("请先完整配置云端 ASR Provider 的 Base URL、Model 和 API key。".to_string());
    }

    let text = transcribe_with_cloud(&request, audio).await?;
    Ok(TranscribeAudioResponse {
        text,
        provider_label: format!("{} via cloud ASR", request.cloud_model.trim()),
    })
}

#[tauri::command]
async fn start_streaming_asr_session(
    app: AppHandle,
    state: tauri::State<'_, StreamingAsrState>,
    request: StartStreamingAsrRequest,
) -> Result<StartStreamingAsrResponse, String> {
    if request.cloud_base_url.trim().is_empty()
        || request.cloud_api_key.trim().is_empty()
        || request.cloud_model.trim().is_empty()
    {
        return Err("请先完整配置云端 ASR Provider 的 Base URL、Model 和 API key。".to_string());
    }
    if !request
        .cloud_base_url
        .trim()
        .to_ascii_lowercase()
        .contains("dashscope.aliyuncs.com")
    {
        return Err(
            "实时 ASR 第一版固定使用 DashScope，请将 ASR Base URL 填为 https://dashscope.aliyuncs.com/compatible-mode/v1。"
                .to_string(),
        );
    }
    if request.cloud_model.trim() != "paraformer-realtime-v2" {
        return Err("实时 ASR 第一版请使用 DashScope 模型 paraformer-realtime-v2。".to_string());
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    let provider_label = format!("{} via DashScope realtime ASR", request.cloud_model.trim());
    let session = StreamingAsrSession {
        session_id: session_id.clone(),
        meeting_id: request.meeting_id,
        recording_run_id: request.recording_run_id,
        task_id: uuid::Uuid::new_v4().to_string(),
        api_key: request.cloud_api_key.trim().to_string(),
        model: request.cloud_model.trim().to_string(),
        language: request.language,
        provider_label: provider_label.clone(),
    };
    let (audio_tx, audio_rx) = mpsc::channel(STREAMING_AUDIO_QUEUE_CAPACITY);
    let (started_tx, started_rx) = oneshot::channel();

    state.sessions.lock().await.insert(
        session_id.clone(),
        StreamingAsrSessionHandle {
            audio_tx: audio_tx.clone(),
        },
    );

    let sessions = state.sessions.clone();
    tauri::async_runtime::spawn(run_streaming_asr_session(
        app, sessions, session, audio_rx, started_tx,
    ));

    match timeout(STREAMING_START_TIMEOUT, started_rx).await {
        Ok(Ok(Ok(()))) => Ok(StartStreamingAsrResponse {
            session_id,
            provider_label,
        }),
        Ok(Ok(Err(error))) => {
            state.sessions.lock().await.remove(&session_id);
            Err(error)
        }
        Ok(Err(_)) => {
            state.sessions.lock().await.remove(&session_id);
            Err("实时 ASR 启动任务提前结束。".to_string())
        }
        Err(_) => {
            let _ = audio_tx.try_send(StreamingAsrCommand::Cancel);
            state.sessions.lock().await.remove(&session_id);
            Err("实时 ASR 等待 task-started 超时。".to_string())
        }
    }
}

#[tauri::command]
async fn push_streaming_asr_audio(
    state: tauri::State<'_, StreamingAsrState>,
    request: PushStreamingAsrAudioRequest,
) -> Result<(), String> {
    let audio = base64::engine::general_purpose::STANDARD
        .decode(request.audio_base64.trim())
        .map_err(|error| format!("实时 ASR 音频帧不是有效的 base64：{error}"))?;

    if audio.is_empty() {
        return Ok(());
    }

    let audio_tx = {
        let sessions = state.sessions.lock().await;
        sessions
            .get(&request.session_id)
            .map(|session| session.audio_tx.clone())
            .ok_or_else(|| "实时 ASR session 不存在或已经结束。".to_string())?
    };

    audio_tx
        .try_send(StreamingAsrCommand::Audio(audio))
        .map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => {
                "实时 ASR 音频队列已满，当前网络或 Provider 处理速度跟不上。".to_string()
            }
            mpsc::error::TrySendError::Closed(_) => "实时 ASR session 已关闭。".to_string(),
        })
}

#[tauri::command]
async fn finish_streaming_asr_session(
    state: tauri::State<'_, StreamingAsrState>,
    request: StreamingAsrSessionRequest,
) -> Result<(), String> {
    let audio_tx = {
        let sessions = state.sessions.lock().await;
        sessions
            .get(&request.session_id)
            .map(|session| session.audio_tx.clone())
    };

    if let Some(audio_tx) = audio_tx {
        audio_tx
            .send(StreamingAsrCommand::Finish)
            .await
            .map_err(|_| "实时 ASR session 已关闭。".to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn cancel_streaming_asr_session(
    state: tauri::State<'_, StreamingAsrState>,
    request: StreamingAsrSessionRequest,
) -> Result<(), String> {
    let audio_tx = {
        let mut sessions = state.sessions.lock().await;
        sessions
            .remove(&request.session_id)
            .map(|session| session.audio_tx)
    };

    if let Some(audio_tx) = audio_tx {
        let _ = audio_tx.send(StreamingAsrCommand::Cancel).await;
    }
    Ok(())
}

async fn transcribe_with_cloud(
    request: &TranscribeAudioRequest,
    audio: Vec<u8>,
) -> Result<String, String> {
    if uses_qwen_asr_chat_endpoint(request) {
        return transcribe_with_qwen_asr_chat(request, audio).await;
    }

    transcribe_with_openai_audio_endpoint(request, audio).await
}

async fn transcribe_with_openai_audio_endpoint(
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

async fn transcribe_with_qwen_asr_chat(
    request: &TranscribeAudioRequest,
    audio: Vec<u8>,
) -> Result<String, String> {
    let base_url = request.cloud_base_url.trim().trim_end_matches('/');
    let endpoint = format!("{base_url}/chat/completions");
    let mime_type = if request.mime_type.trim().is_empty() {
        "audio/wav"
    } else {
        request.mime_type.trim()
    };
    let data_url = format!(
        "data:{mime_type};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(audio)
    );
    let body = json!({
        "model": request.cloud_model.trim(),
        "messages": [
            {
                "role": "user",
                "content": [
                    {
                        "type": "input_audio",
                        "input_audio": {
                            "data": data_url
                        }
                    }
                ]
            }
        ],
        "asr_options": {
            "language": cloud_language_code(&request.language),
            "enable_itn": false
        }
    });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(|error| format!("无法创建 ASR HTTP 客户端：{error}"))?;
    let response = client
        .post(endpoint)
        .bearer_auth(request.cloud_api_key.trim())
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("Qwen ASR 请求失败：{error}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let detail = response
            .text()
            .await
            .unwrap_or_else(|_| "无法读取错误详情".to_string());
        return Err(format!(
            "Qwen ASR 返回 {status}: {}",
            truncate(&detail, 240)
        ));
    }

    let payload = response
        .json::<Value>()
        .await
        .map_err(|error| format!("Qwen ASR 响应不是预期 JSON：{error}"))?;
    clean_asr_text(extract_qwen_asr_text(&payload).unwrap_or_default())
        .ok_or_else(|| "Qwen ASR 没有返回可用文本。".to_string())
}

async fn run_streaming_asr_session(
    app: AppHandle,
    sessions: Arc<Mutex<HashMap<String, StreamingAsrSessionHandle>>>,
    session: StreamingAsrSession,
    mut audio_rx: mpsc::Receiver<StreamingAsrCommand>,
    started_tx: oneshot::Sender<Result<(), String>>,
) {
    let session_id = session.session_id.clone();
    let mut started_tx = Some(started_tx);

    let startup = async {
        let mut request = DASHSCOPE_STREAMING_WS_URL
            .into_client_request()
            .map_err(|error| format!("无法创建 DashScope WebSocket 请求：{error}"))?;
        let auth_value = format!("Bearer {}", session.api_key)
            .parse()
            .map_err(|error| format!("DashScope 鉴权头无效：{error}"))?;
        request.headers_mut().insert("Authorization", auth_value);
        request.headers_mut().insert(
            "user-agent",
            "AIMeeting/0.1".parse().expect("static user agent"),
        );

        let (mut websocket, _) = connect_async(request)
            .await
            .map_err(|error| format!("无法连接 DashScope 实时 ASR：{error}"))?;
        websocket
            .send(Message::Text(build_dashscope_run_task(&session).into()))
            .await
            .map_err(|error| format!("无法发送 DashScope run-task：{error}"))?;

        loop {
            let message = websocket
                .next()
                .await
                .ok_or_else(|| "DashScope 实时 ASR 连接在 task-started 前关闭。".to_string())?
                .map_err(|error| format!("读取 DashScope 启动事件失败：{error}"))?;

            if let Some(event) = handle_dashscope_message(&app, &session, message) {
                match event.as_str() {
                    "task-started" => return Ok(websocket),
                    "task-failed" => {
                        return Err("DashScope 实时 ASR task-failed，详情见事件日志。".to_string())
                    }
                    _ => {}
                }
            }
        }
    }
    .await;

    let websocket = match startup {
        Ok(websocket) => {
            if let Some(started_tx) = started_tx.take() {
                let _ = started_tx.send(Ok(()));
            }
            websocket
        }
        Err(error) => {
            if let Some(started_tx) = started_tx.take() {
                let _ = started_tx.send(Err(error.clone()));
            }
            emit_streaming_asr_event(&app, &session, "error", "", None, None, Some(error));
            sessions.lock().await.remove(&session_id);
            return;
        }
    };

    let (mut write, mut read) = websocket.split();
    let mut finish_sent = false;

    loop {
        tokio::select! {
            command = audio_rx.recv() => {
                match command {
                    Some(StreamingAsrCommand::Audio(audio)) => {
                        if finish_sent {
                            continue;
                        }
                        if let Err(error) = write.send(Message::Binary(audio.into())).await {
                            emit_streaming_asr_event(
                                &app,
                                &session,
                                "error",
                                "",
                                None,
                                None,
                                Some(format!("发送实时 ASR 音频帧失败：{error}")),
                            );
                            break;
                        }
                    }
                    Some(StreamingAsrCommand::Finish) => {
                        if !finish_sent {
                            finish_sent = true;
                            if let Err(error) = write.send(Message::Text(build_dashscope_finish_task(&session).into())).await {
                                emit_streaming_asr_event(
                                    &app,
                                    &session,
                                    "error",
                                    "",
                                    None,
                                    None,
                                    Some(format!("发送 DashScope finish-task 失败：{error}")),
                                );
                                break;
                            }
                        }
                    }
                    Some(StreamingAsrCommand::Cancel) | None => {
                        let _ = write.close().await;
                        break;
                    }
                }
            }
            message = read.next() => {
                let Some(message) = message else {
                    break;
                };
                match message {
                    Ok(message) => {
                        if let Some(event) = handle_dashscope_message(&app, &session, message) {
                            match event.as_str() {
                                "task-finished" | "task-failed" => break,
                                _ => {}
                            }
                        }
                    }
                    Err(error) => {
                        emit_streaming_asr_event(
                            &app,
                            &session,
                            "error",
                            "",
                            None,
                            None,
                            Some(format!("读取 DashScope 实时 ASR 事件失败：{error}")),
                        );
                        break;
                    }
                }
            }
        }
    }

    sessions.lock().await.remove(&session_id);
}

fn build_dashscope_run_task(session: &StreamingAsrSession) -> String {
    json!({
        "header": {
            "action": "run-task",
            "task_id": session.task_id,
            "streaming": "duplex"
        },
        "payload": {
            "task_group": "audio",
            "task": "asr",
            "function": "recognition",
            "model": session.model,
            "parameters": {
                "format": "pcm",
                "sample_rate": 16000,
                "disfluency_removal_enabled": false,
                "language_hints": dashscope_language_hints(&session.language),
                "max_sentence_silence": 800,
                "punctuation_prediction_enabled": true,
                "inverse_text_normalization_enabled": true,
                "heartbeat": true
            },
            "input": {}
        }
    })
    .to_string()
}

fn build_dashscope_finish_task(session: &StreamingAsrSession) -> String {
    json!({
        "header": {
            "action": "finish-task",
            "task_id": session.task_id,
            "streaming": "duplex"
        },
        "payload": {
            "input": {}
        }
    })
    .to_string()
}

fn handle_dashscope_message(
    app: &AppHandle,
    session: &StreamingAsrSession,
    message: Message,
) -> Option<String> {
    let Message::Text(text) = message else {
        return None;
    };
    let payload = match serde_json::from_str::<Value>(&text) {
        Ok(payload) => payload,
        Err(error) => {
            emit_streaming_asr_event(
                app,
                session,
                "error",
                "",
                None,
                None,
                Some(format!("DashScope 实时 ASR 事件不是有效 JSON：{error}")),
            );
            return Some("parse-error".to_string());
        }
    };
    let event = payload
        .pointer("/header/event")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    match event.as_str() {
        "task-started" => {
            emit_streaming_asr_event(app, session, "started", "", None, None, None);
        }
        "result-generated" => {
            if let Some(result) = parse_dashscope_result_event(&payload, session) {
                let _ = app.emit("streaming-asr-event", result);
            }
        }
        "task-finished" => {
            emit_streaming_asr_event(app, session, "finished", "", None, None, None);
        }
        "task-failed" => {
            let error = payload
                .pointer("/header/error_message")
                .and_then(Value::as_str)
                .unwrap_or("DashScope 实时 ASR task-failed。");
            emit_streaming_asr_event(
                app,
                session,
                "error",
                "",
                None,
                None,
                Some(error.to_string()),
            );
        }
        _ => {}
    }

    (!event.is_empty()).then_some(event)
}

fn parse_dashscope_result_event(
    payload: &Value,
    session: &StreamingAsrSession,
) -> Option<StreamingAsrEvent> {
    let sentence = payload.pointer("/payload/output/sentence")?;
    if sentence
        .get("heartbeat")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }

    let text = sentence.get("text").and_then(Value::as_str)?.trim();
    if text.is_empty() {
        return None;
    }

    let begin_ms = read_i64_alias(sentence, &["begin_time", "beginTime"]);
    let end_value = read_value_alias(sentence, &["end_time", "endTime"]);
    let end_ms = end_value.and_then(Value::as_i64);
    let sentence_end = read_bool_alias(sentence, &["sentence_end", "sentenceEnd"]).unwrap_or(false);
    let status = if end_ms.is_some() || (end_value.is_none() && sentence_end) {
        "final"
    } else {
        "interim"
    };

    Some(StreamingAsrEvent {
        session_id: session.session_id.clone(),
        meeting_id: session.meeting_id.clone(),
        recording_run_id: session.recording_run_id.clone(),
        status: status.to_string(),
        text: text.to_string(),
        begin_ms,
        end_ms,
        provider_label: session.provider_label.clone(),
        error_message: None,
    })
}

fn emit_streaming_asr_event(
    app: &AppHandle,
    session: &StreamingAsrSession,
    status: &str,
    text: &str,
    begin_ms: Option<i64>,
    end_ms: Option<i64>,
    error_message: Option<String>,
) {
    let event = StreamingAsrEvent {
        session_id: session.session_id.clone(),
        meeting_id: session.meeting_id.clone(),
        recording_run_id: session.recording_run_id.clone(),
        status: status.to_string(),
        text: text.to_string(),
        begin_ms,
        end_ms,
        provider_label: session.provider_label.clone(),
        error_message,
    };
    let _ = app.emit("streaming-asr-event", event);
}

fn read_value_alias<'a>(value: &'a Value, names: &[&str]) -> Option<&'a Value> {
    names.iter().find_map(|name| value.get(*name))
}

fn read_i64_alias(value: &Value, names: &[&str]) -> Option<i64> {
    read_value_alias(value, names).and_then(Value::as_i64)
}

fn read_bool_alias(value: &Value, names: &[&str]) -> Option<bool> {
    read_value_alias(value, names).and_then(Value::as_bool)
}

fn uses_qwen_asr_chat_endpoint(request: &TranscribeAudioRequest) -> bool {
    let model = request.cloud_model.trim().to_ascii_lowercase();
    model.starts_with("qwen3-asr") || model.starts_with("qwen-asr")
}

fn extract_qwen_asr_text(payload: &Value) -> Option<String> {
    let content = payload.pointer("/choices/0/message/content")?;
    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }

    content.as_array().and_then(|items| {
        items
            .iter()
            .find_map(|item| item.get("text").and_then(Value::as_str))
            .map(ToString::to_string)
    })
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

fn dashscope_language_hints(language: &str) -> Vec<&'static str> {
    if language.starts_with("zh-HK") {
        vec!["yue", "zh", "en"]
    } else if language.starts_with("zh") {
        vec!["zh", "en"]
    } else if language.starts_with("ja") {
        vec!["ja"]
    } else {
        vec!["en"]
    }
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
        .manage(StreamingAsrState::default())
        .invoke_handler(tauri::generate_handler![
            capture_capabilities,
            secure_key_storage_status,
            transcribe_audio_chunk,
            start_streaming_asr_session,
            push_streaming_asr_audio,
            finish_streaming_asr_session,
            cancel_streaming_asr_session
        ])
        .run(tauri::generate_context!())
        .expect("error while running AIMeeting");
}

#[cfg(test)]
mod tests {
    use super::{
        clean_asr_text, extract_qwen_asr_text, parse_dashscope_result_event, StreamingAsrSession,
    };
    use serde_json::json;

    fn test_streaming_session() -> StreamingAsrSession {
        StreamingAsrSession {
            session_id: "session-1".to_string(),
            meeting_id: "meeting-1".to_string(),
            recording_run_id: "run-1".to_string(),
            task_id: "task-1".to_string(),
            api_key: "secret".to_string(),
            model: "paraformer-realtime-v2".to_string(),
            language: "zh-CN".to_string(),
            provider_label: "paraformer-realtime-v2 via DashScope realtime ASR".to_string(),
        }
    }

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
            clean_asr_text("我们现在讨论云端 ASR 接入。".to_string()).as_deref(),
            Some("我们现在讨论云端 ASR 接入。")
        );
    }

    #[test]
    fn extract_qwen_asr_text_reads_chat_completion_content() {
        let payload = json!({
            "choices": [
                {
                    "message": {
                        "content": "欢迎使用阿里云。"
                    }
                }
            ]
        });

        assert_eq!(
            extract_qwen_asr_text(&payload).as_deref(),
            Some("欢迎使用阿里云。")
        );
    }

    #[test]
    fn extract_qwen_asr_text_reads_content_array_text() {
        let payload = json!({
            "choices": [
                {
                    "message": {
                        "content": [
                            {
                                "text": "会议测试。"
                            }
                        ]
                    }
                }
            ]
        });

        assert_eq!(
            extract_qwen_asr_text(&payload).as_deref(),
            Some("会议测试。")
        );
    }

    #[test]
    fn parse_dashscope_result_marks_null_end_time_as_interim() {
        let payload = json!({
            "payload": {
                "output": {
                    "sentence": {
                        "begin_time": 170,
                        "end_time": null,
                        "text": "我们正在讨论实时转写",
                        "sentence_end": true
                    }
                }
            }
        });

        let event = parse_dashscope_result_event(&payload, &test_streaming_session()).unwrap();
        assert_eq!(event.status, "interim");
        assert_eq!(event.begin_ms, Some(170));
        assert_eq!(event.end_ms, None);
    }

    #[test]
    fn parse_dashscope_result_marks_number_end_time_as_final() {
        let payload = json!({
            "payload": {
                "output": {
                    "sentence": {
                        "begin_time": 170,
                        "end_time": 2320,
                        "text": "会议纪要只接收最终结果。"
                    }
                }
            }
        });

        let event = parse_dashscope_result_event(&payload, &test_streaming_session()).unwrap();
        assert_eq!(event.status, "final");
        assert_eq!(event.text, "会议纪要只接收最终结果。");
        assert_eq!(event.end_ms, Some(2320));
    }
}

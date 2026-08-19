use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tauri::{AppHandle, Emitter};
use tokio::{
    sync::{mpsc, oneshot, Mutex},
    time::{timeout, Instant},
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
};

const DASHSCOPE_STREAMING_WS_URL: &str = "wss://dashscope.aliyuncs.com/api-ws/v1/inference";
const STREAMING_AUDIO_QUEUE_CAPACITY: usize = 60;
const STREAMING_START_TIMEOUT: Duration = Duration::from_secs(20);
const STREAMING_FINISH_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone)]
struct StreamingAsrSessionHandle {
    audio_tx: mpsc::Sender<StreamingAsrCommand>,
}

#[derive(Clone, Default)]
pub(crate) struct StreamingAsrState {
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

pub(crate) struct NativeStreamingAsrConfig {
    pub cloud_base_url: String,
    pub cloud_api_key: String,
    pub cloud_model: String,
    pub language: String,
    pub meeting_id: String,
    pub recording_run_id: String,
}

enum StreamingAsrCommand {
    Audio(Vec<u8>),
    Finish,
    Cancel,
}

#[derive(Debug, PartialEq, Eq)]
enum StreamingAsrLoopExit {
    NaturalClose,
    FinishTimedOut,
}

#[derive(Debug, PartialEq, Eq)]
struct StreamingAsrTerminalEvent {
    status: StreamingAsrStatus,
    error_message: String,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum StreamingAsrStatus {
    Started,
    Interim,
    Final,
    Finished,
    Error,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartStreamingAsrRequest {
    cloud_base_url: String,
    cloud_api_key: String,
    cloud_model: String,
    language: String,
    meeting_id: String,
    recording_run_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartStreamingAsrResponse {
    session_id: String,
    provider_label: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StreamingAsrSessionRequest {
    session_id: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StreamingAsrEvent {
    session_id: String,
    meeting_id: String,
    recording_run_id: String,
    status: StreamingAsrStatus,
    text: String,
    begin_ms: Option<i64>,
    end_ms: Option<i64>,
    provider_label: String,
    error_message: Option<String>,
}

pub(crate) async fn start_streaming_asr_session(
    app: AppHandle,
    state: &StreamingAsrState,
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

pub(crate) async fn run_native_streaming_asr_bridge(
    app: AppHandle,
    state: StreamingAsrState,
    config: NativeStreamingAsrConfig,
    mut audio_rx: mpsc::Receiver<Vec<u8>>,
) {
    let response = start_streaming_asr_session(
        app,
        &state,
        StartStreamingAsrRequest {
            cloud_base_url: config.cloud_base_url,
            cloud_api_key: config.cloud_api_key,
            cloud_model: config.cloud_model,
            language: config.language,
            meeting_id: config.meeting_id,
            recording_run_id: config.recording_run_id,
        },
    )
    .await;
    let Ok(response) = response else {
        return;
    };

    while let Some(audio) = audio_rx.recv().await {
        let audio_tx = {
            let sessions = state.sessions.lock().await;
            sessions
                .get(&response.session_id)
                .map(|session| session.audio_tx.clone())
        };
        let Some(audio_tx) = audio_tx else {
            return;
        };
        if audio_tx
            .send(StreamingAsrCommand::Audio(audio))
            .await
            .is_err()
        {
            return;
        }
    }

    let _ = finish_streaming_asr_session(
        &state,
        StreamingAsrSessionRequest {
            session_id: response.session_id,
        },
    )
    .await;
}

pub(crate) async fn finish_streaming_asr_session(
    state: &StreamingAsrState,
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
            concat!("AIMeeting/", env!("CARGO_PKG_VERSION"))
                .parse()
                .expect("package version is a valid user agent"),
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
            emit_streaming_asr_event(
                &app,
                &session,
                StreamingAsrStatus::Error,
                "",
                (None, None),
                Some(error),
            );
            sessions.lock().await.remove(&session_id);
            return;
        }
    };

    let (mut write, mut read) = websocket.split();
    let mut finish_sent = false;
    let finish_timeout = tokio::time::sleep(STREAMING_FINISH_TIMEOUT);
    tokio::pin!(finish_timeout);
    let mut loop_exit = None;

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
                                StreamingAsrStatus::Error,
                                "",
                                (None, None),
                                Some(format!("发送实时 ASR 音频帧失败：{error}")),
                            );
                            break;
                        }
                    }
                    Some(StreamingAsrCommand::Finish) => {
                        if !finish_sent {
                            if let Err(error) = write.send(Message::Text(build_dashscope_finish_task(&session).into())).await {
                                emit_streaming_asr_event(
                                    &app,
                                    &session,
                                    StreamingAsrStatus::Error,
                                    "",
                                    (None, None),
                                    Some(format!("发送 DashScope finish-task 失败：{error}")),
                                );
                                break;
                            }
                            finish_sent = true;
                            finish_timeout
                                .as_mut()
                                .reset(Instant::now() + STREAMING_FINISH_TIMEOUT);
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
                    loop_exit = Some(StreamingAsrLoopExit::NaturalClose);
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
                            StreamingAsrStatus::Error,
                            "",
                            (None, None),
                            Some(format!("读取 DashScope 实时 ASR 事件失败：{error}")),
                        );
                        break;
                    }
                }
            }
            _ = &mut finish_timeout, if finish_sent => {
                loop_exit = Some(StreamingAsrLoopExit::FinishTimedOut);
                break;
            }
        }
    }

    if let Some(loop_exit) = loop_exit {
        let terminal = terminal_event_for_loop_exit(loop_exit);
        emit_streaming_asr_event(
            &app,
            &session,
            terminal.status,
            "",
            (None, None),
            Some(terminal.error_message),
        );
    }

    sessions.lock().await.remove(&session_id);
}

fn terminal_event_for_loop_exit(exit: StreamingAsrLoopExit) -> StreamingAsrTerminalEvent {
    let error_message = match exit {
        StreamingAsrLoopExit::NaturalClose => "DashScope 实时 ASR 连接在 task-finished 前关闭。",
        StreamingAsrLoopExit::FinishTimedOut => "DashScope 实时 ASR 等待 task-finished 超时。",
    };

    StreamingAsrTerminalEvent {
        status: StreamingAsrStatus::Error,
        error_message: error_message.to_string(),
    }
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
                StreamingAsrStatus::Error,
                "",
                (None, None),
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
            emit_streaming_asr_event(
                app,
                session,
                StreamingAsrStatus::Started,
                "",
                (None, None),
                None,
            );
        }
        "result-generated" => {
            if let Some(result) = parse_dashscope_result_event(&payload, session) {
                let _ = app.emit("streaming-asr-event", result);
            }
        }
        "task-finished" => {
            emit_streaming_asr_event(
                app,
                session,
                StreamingAsrStatus::Finished,
                "",
                (None, None),
                None,
            );
        }
        "task-failed" => {
            let error = payload
                .pointer("/header/error_message")
                .and_then(Value::as_str)
                .unwrap_or("DashScope 实时 ASR task-failed。");
            emit_streaming_asr_event(
                app,
                session,
                StreamingAsrStatus::Error,
                "",
                (None, None),
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
        StreamingAsrStatus::Final
    } else {
        StreamingAsrStatus::Interim
    };

    Some(StreamingAsrEvent {
        session_id: session.session_id.clone(),
        meeting_id: session.meeting_id.clone(),
        recording_run_id: session.recording_run_id.clone(),
        status,
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
    status: StreamingAsrStatus,
    text: &str,
    timing: (Option<i64>, Option<i64>),
    error_message: Option<String>,
) {
    let event = StreamingAsrEvent {
        session_id: session.session_id.clone(),
        meeting_id: session.meeting_id.clone(),
        recording_run_id: session.recording_run_id.clone(),
        status,
        text: text.to_string(),
        begin_ms: timing.0,
        end_ms: timing.1,
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

#[cfg(test)]
mod tests {
    use super::{
        parse_dashscope_result_event, terminal_event_for_loop_exit, StreamingAsrLoopExit,
        StreamingAsrSession, StreamingAsrStatus, StreamingAsrTerminalEvent,
    };
    use serde_json::json;

    fn test_streaming_session() -> StreamingAsrSession {
        StreamingAsrSession {
            session_id: "session-1".to_string(),
            meeting_id: "meeting-1".to_string(),
            recording_run_id: "run-1".to_string(),
            task_id: "task-1".to_string(),
            api_key: "test-api-key".to_string(),
            model: "paraformer-realtime-v2".to_string(),
            language: "zh-CN".to_string(),
            provider_label: "paraformer-realtime-v2 via DashScope realtime ASR".to_string(),
        }
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
        assert_eq!(event.status, StreamingAsrStatus::Interim);
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
        assert_eq!(event.status, StreamingAsrStatus::Final);
        assert_eq!(event.text, "会议纪要只接收最终结果。");
        assert_eq!(event.end_ms, Some(2320));
    }

    #[test]
    fn streaming_status_serializes_to_existing_frontend_values() {
        assert_eq!(
            serde_json::to_value(StreamingAsrStatus::Started).unwrap(),
            "started"
        );
        assert_eq!(
            serde_json::to_value(StreamingAsrStatus::Interim).unwrap(),
            "interim"
        );
        assert_eq!(
            serde_json::to_value(StreamingAsrStatus::Final).unwrap(),
            "final"
        );
        assert_eq!(
            serde_json::to_value(StreamingAsrStatus::Finished).unwrap(),
            "finished"
        );
        assert_eq!(
            serde_json::to_value(StreamingAsrStatus::Error).unwrap(),
            "error"
        );
    }

    #[test]
    fn natural_websocket_close_is_reported_as_an_error() {
        assert_eq!(
            terminal_event_for_loop_exit(StreamingAsrLoopExit::NaturalClose),
            StreamingAsrTerminalEvent {
                status: StreamingAsrStatus::Error,
                error_message: "DashScope 实时 ASR 连接在 task-finished 前关闭。".to_string(),
            }
        );
    }

    #[test]
    fn finish_timeout_is_reported_as_an_error() {
        assert_eq!(
            terminal_event_for_loop_exit(StreamingAsrLoopExit::FinishTimedOut),
            StreamingAsrTerminalEvent {
                status: StreamingAsrStatus::Error,
                error_message: "DashScope 实时 ASR 等待 task-finished 超时。".to_string(),
            }
        );
    }
}

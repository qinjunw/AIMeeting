use base64::Engine;
use reqwest::multipart;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscribeAudioRequest {
    audio_base64: String,
    mime_type: String,
    cloud_base_url: String,
    cloud_api_key: String,
    cloud_model: String,
    language: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscribeAudioResponse {
    text: String,
    provider_label: String,
}

#[derive(Deserialize)]
struct CloudTranscriptionResponse {
    text: Option<String>,
}

pub(crate) async fn transcribe_audio_chunk(
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

fn cloud_language_code(language: &str) -> &'static str {
    if language.starts_with("zh") {
        "zh"
    } else if language.starts_with("ja") {
        "ja"
    } else {
        "en"
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

#[cfg(test)]
mod tests {
    use super::{clean_asr_text, extract_qwen_asr_text};
    use serde_json::json;

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
}

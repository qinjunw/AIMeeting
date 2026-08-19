use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::Notify;

use crate::persistence::secrets::SecretString;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextEndpointFlavor {
    ChatCompletions,
    Responses,
}

#[derive(Clone)]
pub struct OpenAiCompatibleConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: SecretString,
    pub endpoint_flavor: TextEndpointFlavor,
    pub timeout: Duration,
}

impl std::fmt::Debug for OpenAiCompatibleConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenAiCompatibleConfig")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key", &"[REDACTED]")
            .field("endpoint_flavor", &self.endpoint_flavor)
            .field("timeout", &self.timeout)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct MinutesInput {
    pub meeting_id: String,
    pub transcript_revision: u64,
    pub previous_minutes: String,
    pub transcript: String,
}

#[derive(Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
    notification: Arc<Notify>,
}

impl CancellationToken {
    pub fn cancel(&self) {
        if !self.cancelled.swap(true, Ordering::AcqRel) {
            self.notification.notify_waiters();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    async fn cancelled(&self) {
        loop {
            if self.is_cancelled() {
                return;
            }
            let notified = self.notification.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

pub struct OpenAiCompatibleMinutesClient {
    client: Client,
    config: OpenAiCompatibleConfig,
}

impl std::fmt::Debug for OpenAiCompatibleMinutesClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenAiCompatibleMinutesClient")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl OpenAiCompatibleMinutesClient {
    pub fn new(config: OpenAiCompatibleConfig) -> Result<Self, MinutesClientError> {
        if config.base_url.trim().is_empty()
            || config.model.trim().is_empty()
            || config.api_key.is_empty()
        {
            return Err(MinutesClientError::IncompleteConfiguration);
        }
        let client = Client::builder().timeout(config.timeout).build()?;
        Ok(Self { client, config })
    }

    pub async fn generate(
        &self,
        input: &MinutesInput,
        cancellation: &CancellationToken,
    ) -> Result<String, MinutesClientError> {
        if cancellation.is_cancelled() {
            return Err(MinutesClientError::Cancelled);
        }
        let endpoint = match self.config.endpoint_flavor {
            TextEndpointFlavor::ChatCompletions => "chat/completions",
            TextEndpointFlavor::Responses => "responses",
        };
        let request = self
            .client
            .post(format!(
                "{}/{}",
                self.config.base_url.trim_end_matches('/'),
                endpoint
            ))
            .bearer_auth(self.config.api_key.expose())
            .json(&build_payload(
                &self.config.model,
                self.config.endpoint_flavor,
                input,
            ))
            .send();
        let response = tokio::select! {
            _ = cancellation.cancelled() => return Err(MinutesClientError::Cancelled),
            response = request => response?,
        };
        let status = response.status();
        if !status.is_success() {
            return Err(MinutesClientError::Provider(status.as_u16()));
        }
        let payload = tokio::select! {
            _ = cancellation.cancelled() => return Err(MinutesClientError::Cancelled),
            payload = response.json::<Value>() => payload?,
        };
        let raw = extract_provider_text(&payload).ok_or(MinutesClientError::MissingMinutes)?;
        Ok(normalize_simplified_chinese(&parse_structured_minutes(
            raw,
        )?))
    }
}

pub fn parse_structured_minutes(raw: &str) -> Result<String, MinutesClientError> {
    let payload = serde_json::from_str::<Value>(raw).ok().or_else(|| {
        let start = raw.find('{')?;
        let end = raw.rfind('}')?;
        serde_json::from_str(&raw[start..=end]).ok()
    });
    payload
        .as_ref()
        .and_then(|value| value.get("minutes").or_else(|| value.get("digest")))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|minutes| !minutes.is_empty())
        .map(ToString::to_string)
        .ok_or(MinutesClientError::MissingMinutes)
}

pub fn normalize_simplified_chinese(input: &str) -> String {
    const REPLACEMENTS: &[(&str, &str)] = &[
        ("會議", "会议"),
        ("決策", "决策"),
        ("負責", "负责"),
        ("後續", "后续"),
        ("錄音", "录音"),
        ("轉寫", "转写"),
        ("問題", "问题"),
        ("行動", "行动"),
        ("總結", "总结"),
        ("時間", "时间"),
        ("與", "与"),
        ("為", "为"),
        ("這", "这"),
        ("個", "个"),
        ("開", "开"),
        ("關", "关"),
        ("進", "进"),
        ("時", "时"),
    ];
    REPLACEMENTS
        .iter()
        .fold(input.to_string(), |text, (traditional, simplified)| {
            text.replace(traditional, simplified)
        })
}

fn build_payload(model: &str, flavor: TextEndpointFlavor, input: &MinutesInput) -> Value {
    let system = "你是会议纪要整理助手。只输出包含 minutes 字段的 JSON，使用简体中文；保留事实、数字、负责人和行动项，不得补充转写中不存在的内容。";
    let user = format!(
        "meetingId={} transcriptRevision={}\n\n上一版纪要：\n{}\n\n完整转写：\n{}",
        input.meeting_id, input.transcript_revision, input.previous_minutes, input.transcript
    );
    let messages = json!([
        { "role": "system", "content": system },
        { "role": "user", "content": user }
    ]);
    match flavor {
        TextEndpointFlavor::ChatCompletions => json!({
            "model": model,
            "messages": messages,
            "temperature": 0.2,
            "response_format": { "type": "json_object" }
        }),
        TextEndpointFlavor::Responses => json!({
            "model": model,
            "input": messages,
            "temperature": 0.2
        }),
    }
}

fn extract_provider_text(payload: &Value) -> Option<&str> {
    payload
        .get("output_text")
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
        })
        .or_else(|| {
            payload
                .pointer("/output/0/content/0/text")
                .and_then(Value::as_str)
        })
}

#[derive(Debug, thiserror::Error)]
pub enum MinutesClientError {
    #[error("minutes provider configuration is incomplete")]
    IncompleteConfiguration,
    #[error("minutes request was cancelled")]
    Cancelled,
    #[error("minutes provider request failed")]
    Request(#[from] reqwest::Error),
    #[error("minutes provider returned HTTP {0}")]
    Provider(u16),
    #[error("minutes provider returned no structured minutes")]
    MissingMinutes,
}

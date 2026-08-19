pub mod openai_compatible;

use std::time::Duration;

use futures_util::future::BoxFuture;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::traits::MinutesGateway;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EndpointFlavor {
    ChatCompletions,
    Responses,
}

#[derive(Clone, Debug)]
pub struct MinutesProviderConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub endpoint_flavor: EndpointFlavor,
    pub temperature: f32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MinutesRequest {
    pub meeting_id: String,
    pub transcript_revision: u64,
    pub previous_minutes: String,
    pub transcript: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MinutesResult {
    pub meeting_id: String,
    pub transcript_revision: u64,
    pub minutes: String,
    pub provider_label: String,
}

#[derive(Debug, thiserror::Error)]
pub enum MinutesError {
    #[error("minutes provider configuration is incomplete")]
    IncompleteConfiguration,
    #[error("minutes provider request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("minutes provider returned {status}: {detail}")]
    Provider { status: u16, detail: String },
    #[error("minutes provider response did not contain non-empty minutes")]
    MissingMinutes,
}

pub struct OpenAiCompatibleMinutesGateway {
    client: Client,
    config: MinutesProviderConfig,
}

impl OpenAiCompatibleMinutesGateway {
    pub fn new(config: MinutesProviderConfig) -> Result<Self, MinutesError> {
        if config.base_url.trim().is_empty()
            || config.api_key.trim().is_empty()
            || config.model.trim().is_empty()
        {
            return Err(MinutesError::IncompleteConfiguration);
        }
        let client = Client::builder().timeout(Duration::from_secs(45)).build()?;
        Ok(Self { client, config })
    }

    async fn generate_minutes(
        &self,
        request: &MinutesRequest,
    ) -> Result<MinutesResult, MinutesError> {
        let endpoint = provider_endpoint(&self.config.base_url, self.config.endpoint_flavor);
        let payload = build_minutes_payload_with_temperature(
            &self.config.model,
            self.config.endpoint_flavor,
            self.config.temperature,
            request,
        );
        let response = self
            .client
            .post(endpoint)
            .bearer_auth(&self.config.api_key)
            .json(&payload)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let detail = response.text().await.unwrap_or_default();
            return Err(MinutesError::Provider {
                status: status.as_u16(),
                detail: detail.chars().take(240).collect(),
            });
        }
        let payload = response.json::<Value>().await?;
        let minutes = parse_minutes_response(&payload)?;
        Ok(MinutesResult {
            meeting_id: request.meeting_id.clone(),
            transcript_revision: request.transcript_revision,
            minutes,
            provider_label: format!(
                "{} via {:?}",
                self.config.model, self.config.endpoint_flavor
            ),
        })
    }
}

impl MinutesGateway for OpenAiCompatibleMinutesGateway {
    fn generate<'a>(
        &'a self,
        request: &'a MinutesRequest,
    ) -> BoxFuture<'a, Result<MinutesResult, MinutesError>> {
        Box::pin(self.generate_minutes(request))
    }
}

pub fn build_minutes_payload(
    model: &str,
    endpoint_flavor: EndpointFlavor,
    request: &MinutesRequest,
) -> Value {
    build_minutes_payload_with_temperature(model, endpoint_flavor, 0.2, request)
}

fn build_minutes_payload_with_temperature(
    model: &str,
    endpoint_flavor: EndpointFlavor,
    temperature: f32,
    request: &MinutesRequest,
) -> Value {
    let system = [
        "你是会议纪要整理助手，只输出严格 JSON，结构为 {\"minutes\":\"...\"}。",
        "使用简体中文，固定包含：会议概览、关键讨论、决策结论、行动项、未决问题。",
        "合并重复和口头语，保留专有名词、数字、负责人和时间；不得补充转写中没有的事实。",
    ]
    .join("\n");
    let user = format!(
        "meetingId={} transcriptRevision={}\n\n上一版纪要：\n{}\n\n完整转写：\n{}",
        request.meeting_id,
        request.transcript_revision,
        empty_label(&request.previous_minutes),
        empty_label(&request.transcript),
    );
    let messages = json!([
        { "role": "system", "content": system },
        { "role": "user", "content": user }
    ]);

    match endpoint_flavor {
        EndpointFlavor::ChatCompletions => json!({
            "model": model,
            "messages": messages,
            "temperature": temperature,
            "response_format": { "type": "json_object" }
        }),
        EndpointFlavor::Responses => json!({
            "model": model,
            "input": messages,
            "temperature": temperature
        }),
    }
}

pub fn parse_minutes_response(payload: &Value) -> Result<String, MinutesError> {
    let raw = payload
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
        .ok_or(MinutesError::MissingMinutes)?;

    let parsed = parse_json_object(raw).ok_or(MinutesError::MissingMinutes)?;
    let minutes = parsed
        .get("minutes")
        .or_else(|| parsed.get("digest"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|minutes| !minutes.is_empty())
        .ok_or(MinutesError::MissingMinutes)?;
    Ok(minutes.to_string())
}

fn provider_endpoint(base_url: &str, endpoint_flavor: EndpointFlavor) -> String {
    let base_url = base_url.trim_end_matches('/');
    match endpoint_flavor {
        EndpointFlavor::ChatCompletions => format!("{base_url}/chat/completions"),
        EndpointFlavor::Responses => format!("{base_url}/responses"),
    }
}

fn parse_json_object(raw: &str) -> Option<Value> {
    serde_json::from_str(raw).ok().or_else(|| {
        let start = raw.find('{')?;
        let end = raw.rfind('}')?;
        serde_json::from_str(&raw[start..=end]).ok()
    })
}

fn empty_label(value: &str) -> &str {
    if value.trim().is_empty() {
        "（空）"
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::{build_minutes_payload, parse_minutes_response, EndpointFlavor, MinutesRequest};
    use serde_json::json;

    fn request() -> MinutesRequest {
        MinutesRequest {
            meeting_id: "meeting-1".to_string(),
            transcript_revision: 7,
            previous_minutes: "上一版纪要".to_string(),
            transcript: "讨论了发布风险和负责人。".to_string(),
        }
    }

    #[test]
    fn chat_payload_requires_simplified_chinese_structured_minutes() {
        let payload =
            build_minutes_payload("qwen-plus", EndpointFlavor::ChatCompletions, &request());
        let system = payload["messages"][0]["content"].as_str().unwrap();

        assert_eq!(payload["model"], "qwen-plus");
        assert!(system.contains("简体中文"));
        assert!(system.contains("行动项"));
        assert!(system.contains("不得补充"));
        assert_eq!(payload["response_format"], json!({ "type": "json_object" }));
    }

    #[test]
    fn responses_payload_uses_the_same_revision_bearing_input() {
        let payload = build_minutes_payload("gpt-test", EndpointFlavor::Responses, &request());

        assert_eq!(payload["model"], "gpt-test");
        assert!(payload["input"].as_array().unwrap().len() >= 2);
        assert!(payload.to_string().contains("transcriptRevision=7"));
    }

    #[test]
    fn response_parser_accepts_chat_and_responses_shapes() {
        let chat = json!({
            "choices": [{ "message": { "content": "{\"minutes\":\"会议结论\"}" } }]
        });
        let responses = json!({ "output_text": "{\"minutes\":\"行动计划\"}" });

        assert_eq!(parse_minutes_response(&chat).unwrap(), "会议结论");
        assert_eq!(parse_minutes_response(&responses).unwrap(), "行动计划");
    }

    #[test]
    fn response_parser_rejects_missing_or_empty_minutes() {
        assert!(parse_minutes_response(&json!({ "choices": [] })).is_err());
        assert!(parse_minutes_response(&json!({ "output_text": "{\"minutes\":\"  \"}" })).is_err());
    }
}

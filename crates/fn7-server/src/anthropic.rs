//! Claude (Anthropic Messages API) backend for the Theatre LLM gateway, over raw HTTP
//! (there is no official Rust SDK). Adapted from Final Night's backend; every attempt is recorded through
//! `theatre_trace::http` (request/response bodies and headers, credentials removed).
//!
//! * `POST /v1/messages` with `x-api-key` and `anthropic-version: 2026-10-21`.
//! * JSON-schema requests use structured outputs (`output_config.format`).
//! * The Messages API has no `n`: N samples are N parallel requests.
//! * No `temperature`: current models reject non-default sampling parameters.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use async_trait::async_trait;
use serde_json::{json, Map, Value};
use theatre_llm::{Backend, BackendReply, CallError};
use theatre_plugin_api::{LlmRequest, Role};

pub const DEFAULT_MODEL: &str = "claude-haiku-5-5";
const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2026-10-21";

#[derive(Debug, Default)]
pub struct Usage {
    pub requests: AtomicU64,
    pub input_tokens: AtomicU64,
    pub output_tokens: AtomicU64,
    pub refusals: AtomicU64,
}

impl Usage {
    pub fn snapshot(&self) -> (u64, u64, u64, u64) {
        (
            self.requests.load(Ordering::Relaxed),
            self.input_tokens.load(Ordering::Relaxed),
            self.output_tokens.load(Ordering::Relaxed),
            self.refusals.load(Ordering::Relaxed),
        )
    }
}

pub struct AnthropicBackend {
    client: reqwest::Client,
    api_key: String,
    url: String,
    /// `low` | `medium` | `high` …
    effort: String,
    /// Send `thinking: {type: "disabled"}` (allowed on Haiku 5.5 at effort high or below).
    no_thinking: bool,
    /// Caps HTTP requests in flight across everything using this backend (accounts have a
    /// concurrent-request limit; n samples per call multiply quickly).
    inflight: Arc<tokio::sync::Semaphore>,
    max_attempts: u32,
    pub usage: Arc<Usage>,
}

impl AnthropicBackend {
    pub fn new(api_key: String, timeout: Duration) -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder().timeout(timeout).build()?,
            api_key,
            url: std::env::var("ANTHROPIC_MESSAGES_URL").unwrap_or_else(|_| API_URL.to_string()),
            effort: std::env::var("ANTHROPIC_EFFORT").unwrap_or_else(|_| "low".into()),
            no_thinking: std::env::var("ANTHROPIC_THINKING")
                .map(|v| v != "on")
                .unwrap_or(true),
            inflight: Arc::new(tokio::sync::Semaphore::new(
                std::env::var("ANTHROPIC_MAX_INFLIGHT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(8),
            )),
            max_attempts: 6,
            usage: Arc::new(Usage::default()),
        })
    }

    pub fn from_env(timeout: Duration) -> Option<anyhow::Result<Self>> {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|k| !k.is_empty())?;
        Some(Self::new(key, timeout))
    }

    pub fn body(&self, req: &LlmRequest, model: &str) -> Value {
        let messages: Vec<Value> = req
            .messages
            .iter()
            .map(|(r, c)| json!({ "role": match r { Role::User => "user", Role::Assistant => "assistant" }, "content": c }))
            .collect();
        let mut output_config = Map::new();
        output_config.insert("effort".into(), self.effort.clone().into());
        if let Some(schema) = &req.json_schema {
            output_config.insert(
                "format".into(),
                json!({ "type": "json_schema", "schema": strictify(schema) }),
            );
        }
        let mut body = json!({
            "model": model,
            "max_tokens": req.max_tokens.max(256),
            "system": req.system,
            "messages": messages,
            "output_config": output_config,
        });
        if self.no_thinking {
            body["thinking"] = json!({ "type": "disabled" });
        }
        body
    }

    /// One sample: rate-limited, with its own retries on 429/529/5xx (honouring `retry-after`).
    /// Every attempt is reported as its own HTTP record.
    async fn one(&self, body: &Value, sample: u32) -> Result<String, CallError> {
        let mut attempt = 0;
        loop {
            let permit = self
                .inflight
                .acquire()
                .await
                .map_err(|e| CallError::Fatal(e.into()))?;
            let started_at = theatre_trace::now_ms();
            let t = std::time::Instant::now();
            let mut rec = theatre_trace::HttpRecord {
                sample,
                attempt,
                started_at,
                url: self.url.clone(),
                request: body.clone(),
                ..Default::default()
            };
            let mut retry_after: Option<f64> = None;
            let out = self.exchange(body, &mut rec, &mut retry_after).await;
            drop(permit);
            rec.duration_ms = t.elapsed().as_secs_f64() * 1000.0;
            if let Err(e) = &out {
                rec.error = Some(match e {
                    CallError::Retryable(e) | CallError::Fatal(e) => format!("{e:#}"),
                });
            }
            theatre_trace::http(rec);
            match out {
                Err(CallError::Retryable(_)) if attempt + 1 < self.max_attempts => {
                    let wait = retry_after
                        .unwrap_or(0.5 * 2f64.powi(attempt as i32))
                        .clamp(0.2, 30.0);
                    tokio::time::sleep(Duration::from_secs_f64(wait)).await;
                    attempt += 1;
                }
                other => return other,
            }
        }
    }

    async fn exchange(
        &self,
        body: &Value,
        rec: &mut theatre_trace::HttpRecord,
        retry_after: &mut Option<f64>,
    ) -> Result<String, CallError> {
        let resp = self
            .client
            .post(&self.url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| CallError::Retryable(e.into()))?;
        let status = resp.status();
        rec.status = Some(status.as_u16());
        rec.request_headers = json!({ "anthropic-version": API_VERSION, "content-type": "application/json", "x-api-key": "<redacted>" });
        rec.response_headers = Value::Object(
            resp.headers()
                .iter()
                .map(|(k, v)| {
                    (
                        k.as_str().to_string(),
                        Value::String(v.to_str().unwrap_or("<binary>").to_string()),
                    )
                })
                .collect(),
        );
        *retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<f64>().ok());
        let raw = resp
            .text()
            .await
            .map_err(|e| CallError::Retryable(e.into()))?;
        let v: Value = serde_json::from_str(&raw).unwrap_or_else(|_| Value::String(raw.clone()));
        rec.response = v.clone();
        rec.input_tokens = v["usage"]["input_tokens"].as_u64();
        rec.output_tokens = v["usage"]["output_tokens"].as_u64();
        rec.stop_reason = v["stop_reason"].as_str().map(str::to_string);
        if !status.is_success() {
            let msg = v["error"]["message"].as_str().unwrap_or("").to_string();
            let err = anyhow!("anthropic {status}: {msg}");
            return Err(
                if status.as_u16() == 429 || status.as_u16() == 529 || status.is_server_error() {
                    CallError::Retryable(err)
                } else {
                    CallError::Fatal(err)
                },
            );
        }
        self.usage.requests.fetch_add(1, Ordering::Relaxed);
        self.usage.input_tokens.fetch_add(
            v["usage"]["input_tokens"].as_u64().unwrap_or(0),
            Ordering::Relaxed,
        );
        self.usage.output_tokens.fetch_add(
            v["usage"]["output_tokens"].as_u64().unwrap_or(0),
            Ordering::Relaxed,
        );
        if v["stop_reason"] == "refusal" {
            self.usage.refusals.fetch_add(1, Ordering::Relaxed);
            return Err(CallError::Fatal(anyhow!(
                "model declined (refusal: {})",
                v["stop_details"]["category"]
            )));
        }
        let text: String = v["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("");
        if text.trim().is_empty() {
            return Err(CallError::Retryable(anyhow!(
                "empty response (stop_reason {})",
                v["stop_reason"]
            )));
        }
        Ok(text)
    }
}

#[async_trait]
impl Backend for AnthropicBackend {
    async fn call(&self, req: &LlmRequest, model: &str) -> Result<BackendReply, CallError> {
        let body = self.body(req, model);
        let n = req.n.max(1) as usize;
        let results = futures::future::join_all((0..n).map(|i| self.one(&body, i as u32))).await;
        let mut choices = Vec::new();
        let mut last_err = None;
        for r in results {
            match r {
                Ok(t) => choices.push(t),
                Err(e) => last_err = Some(e),
            }
        }
        if choices.is_empty() {
            return Err(last_err.unwrap_or_else(|| CallError::Fatal(anyhow!("no choices"))));
        }
        Ok(BackendReply {
            choices,
            total_tokens: 0,
        })
    }
}

/// Structured outputs accept a subset of JSON Schema: rewrite `"type": [t, "null"]` as `anyOf`,
/// and make sure every object has `additionalProperties: false`.
pub fn strictify(schema: &Value) -> Value {
    // The Theatre's strict_schema (since 964d523) also rewrites `oneOf` → `anyOf`, which Anthropic requires.
    theatre_toolkit::structured::strict_schema(schema.clone())
}

/// $ per million tokens (input, output) for known models, for cost estimates.
pub fn price_per_mtok(model: &str) -> Option<(f64, f64)> {
    match model {
        "claude-haiku-5-5" => Some((0.10, 0.50)),
        "claude-sonnet-5-5" => Some((2.0, 10.0)),
        "claude-opus-5-5" => Some((4.0, 20.0)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_one_of_reaches_the_api() {
        let s = strictify(&json!({"type":"object","properties":{"e":{"oneOf":[
            {"type":"object","properties":{"k":{"const":"a"},"x":{"oneOf":[{"type":"string"},{"type":"integer"}]}}},
            {"type":"object","properties":{"k":{"const":"b"}}}]}}}));
        let txt = s.to_string();
        assert!(!txt.contains("oneOf"), "{txt}");
        assert_eq!(s["properties"]["e"]["anyOf"].as_array().unwrap().len(), 2);
        assert!(s["properties"]["e"]["anyOf"][0]["properties"]["x"]["anyOf"].is_array());
    }

    #[test]
    fn body_shape() {
        let b = AnthropicBackend::new("k".into(), Duration::from_secs(5)).unwrap();
        let req = LlmRequest::new("sys").user("hi").json_schema(json!({"type":"object","properties":{"a":{"type":["number","null"]}},"required":["a"]})).samples(5);
        let v = b.body(&req, DEFAULT_MODEL);
        assert_eq!(v["system"], "sys");
        assert!(v.get("temperature").is_none());
        assert!(v.get("n").is_none());
        assert_eq!(v["output_config"]["format"]["type"], "json_schema");
        let s = &v["output_config"]["format"]["schema"];
        assert_eq!(s["additionalProperties"], false);
        assert_eq!(s["properties"]["a"]["anyOf"][1]["type"], "null");
    }
}

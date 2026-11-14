//! Image generators: [`MockGenerator`] (free, deterministic SVG placeholders, for tests and UI work) and
//! [`OpenAiImages`] (the OpenAI Images API over raw HTTPS; paid, so only ever built when the host opts in).

use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::prompt::ImageKind;

/// What a generator is asked for.
#[derive(Debug, Clone)]
pub struct GenRequest {
    /// The image key (sha256 hex), for logs and placeholders.
    pub key: String,
    pub kind: ImageKind,
    /// The full prompt text.
    pub prompt: String,
}

/// What a generator made.
#[derive(Debug, Clone)]
pub struct Generated {
    pub bytes: Vec<u8>,
    /// File extension without the dot: `png`, `svg`, `webp`, `jpeg`.
    pub ext: &'static str,
    /// What this image cost (USD, from the price table).
    pub cost_usd: f64,
    pub model: String,
}

#[async_trait]
pub trait ImageGenerator: Send + Sync {
    /// `"mock"`, `"openai"`.
    fn provider(&self) -> &'static str;
    /// Everything that changes the picture for the same prompt (provider, model, size, quality). Part of the image
    /// key, so switching models never serves an image from another one.
    fn label(&self) -> String;
    /// The price of one image of this kind in USD, or `None` when unknown (then nothing is generated: a budget can't
    /// be enforced on an unknown price).
    fn price_usd(&self, kind: ImageKind) -> Option<f64>;
    async fn generate(&self, req: &GenRequest) -> anyhow::Result<Generated>;
}

// ---------------------------------------------------------------------------------------------------------------------
// Mock
// ---------------------------------------------------------------------------------------------------------------------

/// Writes a small deterministic SVG card: colors from the key, the kind, the key's first 12 hex digits and the start
/// of the prompt. No network, no cost (unless a test sets a price to exercise budgets).
#[derive(Debug, Clone, Default)]
pub struct MockGenerator {
    pub price_usd: f64,
    /// Fail every request (to test the failure path).
    pub fail: bool,
    /// Pretend latency.
    pub delay: Duration,
}

impl MockGenerator {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ImageGenerator for MockGenerator {
    fn provider(&self) -> &'static str {
        "mock"
    }
    fn label(&self) -> String {
        "mock".into()
    }
    fn price_usd(&self, _: ImageKind) -> Option<f64> {
        Some(self.price_usd)
    }
    async fn generate(&self, req: &GenRequest) -> anyhow::Result<Generated> {
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        if self.fail {
            anyhow::bail!("mock generator set to fail");
        }
        Ok(Generated {
            bytes: placeholder_svg(&req.key, req.kind, &req.prompt).into_bytes(),
            ext: "svg",
            cost_usd: self.price_usd,
            model: "mock".into(),
        })
    }
}

/// The placeholder card (256×256 SVG). Deterministic in its inputs.
pub fn placeholder_svg(key: &str, kind: ImageKind, prompt: &str) -> String {
    let h = Sha256::digest(key.as_bytes());
    let hue = |b: u8| (b as u32 * 360) / 256;
    let (a, b) = (hue(h[0]), hue(h[1]));
    let mut lines: Vec<String> = vec![];
    let mut cur = String::new();
    for w in prompt.split_whitespace() {
        if cur.chars().count() + w.chars().count() + 1 > 34 {
            lines.push(std::mem::take(&mut cur));
            if lines.len() == 9 {
                break;
            }
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(w);
    }
    if lines.len() < 9 && !cur.is_empty() {
        lines.push(cur);
    }
    let text: String = lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            format!(
                "<tspan x=\"14\" y=\"{}\">{}</tspan>",
                92 + i * 15,
                xml_escape(l)
            )
        })
        .collect();
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"256\" height=\"256\" viewBox=\"0 0 256 256\">\
<defs><linearGradient id=\"g\" x1=\"0\" y1=\"0\" x2=\"1\" y2=\"1\">\
<stop offset=\"0\" stop-color=\"hsl({a},80%,45%)\"/><stop offset=\"1\" stop-color=\"hsl({b},80%,25%)\"/></linearGradient></defs>\
<rect width=\"256\" height=\"256\" rx=\"18\" fill=\"url(#g)\"/>\
<text x=\"14\" y=\"40\" font-family=\"sans-serif\" font-size=\"26\" font-weight=\"bold\" fill=\"#fff\">{kind}</text>\
<text x=\"14\" y=\"62\" font-family=\"monospace\" font-size=\"13\" fill=\"#fff\" opacity=\"0.8\">{short}</text>\
<text font-family=\"sans-serif\" font-size=\"11\" fill=\"#fff\">{text}</text></svg>",
        kind = kind.as_str().to_uppercase(),
        short = &key[..key.len().min(12)],
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// ---------------------------------------------------------------------------------------------------------------------
// OpenAI Images
// ---------------------------------------------------------------------------------------------------------------------

/// Settings for [`OpenAiImages`].
#[derive(Debug, Clone)]
pub struct OpenAiConfig {
    pub api_key: String,
    /// `https://api.openai.com/v1` unless overridden.
    pub base_url: String,
    pub model: String,
    pub size: String,
    /// `low|medium|high|auto` for gpt-image models, `standard|hd` for dall-e-3, ignored for dall-e-2.
    pub quality: String,
    /// A price per image that overrides the table (for models the table doesn't know).
    pub price_override: Option<f64>,
    pub timeout: Duration,
    /// Retries after a 429 or 5xx (each attempt is recorded).
    pub retries: u32,
}

impl OpenAiConfig {
    pub const DEFAULT_MODEL: &'static str = "gpt-image-1-mini";
    pub const DEFAULT_SIZE: &'static str = "1024x1024";
    pub const DEFAULT_QUALITY: &'static str = "medium";
}

/// The OpenAI Images API (`POST /images/generations`), raw HTTPS through reqwest. Never constructed unless the host sets
/// `FN7_IMAGES=openai` and `OPENAI_API_KEY` (see `MediaConfig::from_lookup`).
pub struct OpenAiImages {
    cfg: OpenAiConfig,
    http: reqwest::Client,
}

impl OpenAiImages {
    pub fn new(cfg: OpenAiConfig) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder().timeout(cfg.timeout).build()?;
        Ok(Self { cfg, http })
    }

    fn is_gpt_image(&self) -> bool {
        self.cfg.model.starts_with("gpt-image")
    }

    /// The exact request body (no credentials).
    pub fn body(&self, req: &GenRequest) -> Value {
        let mut b =
            json!({ "model": self.cfg.model, "prompt": req.prompt, "n": 1, "size": self.cfg.size });
        if self.is_gpt_image() {
            b["quality"] = json!(self.cfg.quality);
            b["output_format"] = json!("png");
            if req.kind.wants_transparency() {
                b["background"] = json!("transparent");
            }
        } else {
            b["response_format"] = json!("b64_json");
            if self.cfg.model == "dall-e-3" {
                b["quality"] = json!(if self.cfg.quality == "hd" {
                    "hd"
                } else {
                    "standard"
                });
            }
        }
        b
    }
}

/// USD per image from OpenAI's published prices (per image, by model, size and quality). Prices change: check
/// <https://openai.com/api/pricing> and set `FN7_IMAGE_PRICE_USD` to override. `None` = unknown (nothing is generated).
pub fn price_per_image(model: &str, size: &str, quality: &str) -> Option<f64> {
    let square = size == "1024x1024";
    let wide = matches!(size, "1024x1536" | "1536x1024" | "1024x1792" | "1792x1024");
    let q = quality.to_ascii_lowercase();
    let p = match model {
        "gpt-image-1-mini" => match (q.as_str(), square, wide) {
            ("low", true, _) => 0.005,
            ("low", _, true) => 0.006,
            ("medium" | "auto", true, _) => 0.011,
            ("medium" | "auto", _, true) => 0.015,
            ("high", true, _) => 0.036,
            ("high", _, true) => 0.052,
            _ => return None,
        },
        "gpt-image-1" => match (q.as_str(), square, wide) {
            ("low", true, _) => 0.011,
            ("low", _, true) => 0.016,
            ("medium", true, _) => 0.042,
            ("medium", _, true) => 0.063,
            ("high" | "auto", true, _) => 0.167,
            ("high" | "auto", _, true) => 0.25,
            _ => return None,
        },
        "dall-e-3" => match (q.as_str() == "hd", square, wide) {
            (false, true, _) => 0.04,
            (false, _, true) => 0.08,
            (true, true, _) => 0.08,
            (true, _, true) => 0.12,
            _ => return None,
        },
        "dall-e-2" => match size {
            "256x256" => 0.016,
            "512x512" => 0.018,
            "1024x1024" => 0.02,
            _ => return None,
        },
        _ => return None,
    };
    Some(p)
}

/// A response body fit for the record: base64 image data replaced by its length.
pub fn redact_response(v: &Value) -> Value {
    let mut v = v.clone();
    if let Some(data) = v.get_mut("data").and_then(Value::as_array_mut) {
        for d in data {
            if let Some(b) = d.get("b64_json").and_then(Value::as_str) {
                let n = b.len();
                d["b64_json"] = json!(format!("<{n} base64 chars omitted>"));
            }
        }
    }
    v
}

fn headers_json(h: &reqwest::header::HeaderMap) -> Value {
    Value::Object(
        h.iter()
            .map(|(k, v)| (k.to_string(), json!(v.to_str().unwrap_or(""))))
            .collect(),
    )
}

#[async_trait]
impl ImageGenerator for OpenAiImages {
    fn provider(&self) -> &'static str {
        "openai"
    }
    fn label(&self) -> String {
        format!(
            "openai:{}:{}:{}",
            self.cfg.model, self.cfg.size, self.cfg.quality
        )
    }
    fn price_usd(&self, _: ImageKind) -> Option<f64> {
        self.cfg
            .price_override
            .or_else(|| price_per_image(&self.cfg.model, &self.cfg.size, &self.cfg.quality))
    }

    async fn generate(&self, req: &GenRequest) -> anyhow::Result<Generated> {
        let url = format!(
            "{}/images/generations",
            self.cfg.base_url.trim_end_matches('/')
        );
        let body = self.body(req);
        let cost = self.price_usd(req.kind).unwrap_or(0.0);
        let mut attempt = 0u32;
        loop {
            let started_at = theatre_trace::now_ms();
            let t0 = Instant::now();
            let sent = self
                .http
                .post(&url)
                .bearer_auth(&self.cfg.api_key)
                .json(&body)
                .send()
                .await;
            let mut rec = theatre_trace::HttpRecord {
                attempt,
                started_at,
                url: url.clone(),
                request: body.clone(),
                request_headers: json!({ "content-type": "application/json", "authorization": "Bearer <redacted>" }),
                ..Default::default()
            };
            let resp = match sent {
                Ok(r) => r,
                Err(e) => {
                    rec.duration_ms = t0.elapsed().as_secs_f64() * 1000.0;
                    rec.error = Some(e.to_string());
                    theatre_trace::http(rec);
                    if attempt < self.cfg.retries && (e.is_timeout() || e.is_connect()) {
                        attempt += 1;
                        tokio::time::sleep(Duration::from_secs(2u64.pow(attempt))).await;
                        continue;
                    }
                    return Err(e.into());
                }
            };
            let status = resp.status();
            rec.status = Some(status.as_u16());
            rec.response_headers = headers_json(resp.headers());
            let text = resp.text().await.unwrap_or_default();
            rec.duration_ms = t0.elapsed().as_secs_f64() * 1000.0;
            let parsed: Value =
                serde_json::from_str(&text).unwrap_or_else(|_| json!({ "raw": text }));
            rec.response = redact_response(&parsed);
            rec.input_tokens = parsed
                .pointer("/usage/input_tokens")
                .and_then(Value::as_u64);
            rec.output_tokens = parsed
                .pointer("/usage/output_tokens")
                .and_then(Value::as_u64);
            if !status.is_success() {
                let msg = parsed
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("request failed")
                    .to_string();
                rec.error = Some(format!("{status}: {msg}"));
                theatre_trace::http(rec);
                if attempt < self.cfg.retries
                    && (status.as_u16() == 429 || status.is_server_error())
                {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_secs(2u64.pow(attempt))).await;
                    continue;
                }
                anyhow::bail!("openai images {status}: {msg}");
            }
            theatre_trace::http(rec);
            let first = parsed.pointer("/data/0").cloned().unwrap_or(Value::Null);
            let bytes = if let Some(b64) = first.get("b64_json").and_then(Value::as_str) {
                base64::engine::general_purpose::STANDARD.decode(b64)?
            } else if let Some(u) = first.get("url").and_then(Value::as_str) {
                self.http
                    .get(u)
                    .send()
                    .await?
                    .error_for_status()?
                    .bytes()
                    .await?
                    .to_vec()
            } else {
                anyhow::bail!("openai images: no image in the response");
            };
            return Ok(Generated {
                bytes,
                ext: sniff_ext(&body),
                cost_usd: cost,
                model: self.cfg.model.clone(),
            });
        }
    }
}

fn sniff_ext(body: &Value) -> &'static str {
    match body.get("output_format").and_then(Value::as_str) {
        Some("webp") => "webp",
        Some("jpeg") => "jpeg",
        _ => "png",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_table_knows_the_defaults_and_refuses_the_unknown() {
        assert_eq!(
            price_per_image(
                OpenAiConfig::DEFAULT_MODEL,
                OpenAiConfig::DEFAULT_SIZE,
                OpenAiConfig::DEFAULT_QUALITY
            ),
            Some(0.011)
        );
        assert_eq!(price_per_image("dall-e-3", "1024x1024", "hd"), Some(0.08));
        assert_eq!(
            price_per_image("some-new-model", "1024x1024", "medium"),
            None
        );
    }

    #[test]
    fn request_body_has_no_key_and_asks_for_transparency_where_wanted() {
        let g = OpenAiImages::new(OpenAiConfig {
            api_key: "sk-test".into(),
            base_url: "http://127.0.0.1:9".into(),
            model: "gpt-image-1-mini".into(),
            size: "1024x1024".into(),
            quality: "medium".into(),
            price_override: None,
            timeout: Duration::from_secs(1),
            retries: 0,
        })
        .unwrap();
        let r = |kind| GenRequest {
            key: "k".into(),
            kind,
            prompt: "p".into(),
        };
        let b = g.body(&r(ImageKind::Statue));
        assert_eq!(b["background"], "transparent");
        assert!(!b.to_string().contains("sk-test"));
        assert!(g.body(&r(ImageKind::Event)).get("background").is_none());
    }

    #[test]
    fn redaction_drops_image_bytes() {
        let v = json!({ "data": [ { "b64_json": "AAAA" } ], "usage": { "input_tokens": 5 } });
        assert_eq!(
            redact_response(&v)["data"][0]["b64_json"],
            "<4 base64 chars omitted>"
        );
    }

    #[test]
    fn placeholder_is_deterministic_and_escaped() {
        let a = placeholder_svg(
            "abcdef0123456789",
            ImageKind::Item,
            "a <cursed> \"box\" & more",
        );
        assert_eq!(
            a,
            placeholder_svg(
                "abcdef0123456789",
                ImageKind::Item,
                "a <cursed> \"box\" & more"
            )
        );
        assert!(
            a.contains("&lt;cursed&gt;") && a.contains("abcdef012345") && !a.contains("<cursed>")
        );
    }
}

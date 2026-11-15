//! Image generators: [`MockGenerator`] (free, deterministic SVG placeholders, for tests and UI work) and
//! [`OpenAiImages`] (the OpenAI Images API over raw HTTPS; paid, so only ever built when the host opts in). With
//! reference images on a gpt-image model, [`OpenAiImages`] calls `POST /images/edits` (multipart, one `image[]` part
//! per reference); otherwise `POST /images/generations`.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::prompt::ImageKind;
use crate::style::RefImage;

/// What a generator is asked for.
#[derive(Debug, Clone)]
pub struct GenRequest {
    /// The image key (sha256 hex), for logs and placeholders.
    pub key: String,
    pub kind: ImageKind,
    /// The full prompt text.
    pub prompt: String,
    /// `WIDTHxHEIGHT` (from the style's kind).
    pub size: String,
    /// Reference images to send with it (empty = none; generators that can't use them ignore them).
    pub refs: Vec<RefImage>,
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
    /// Everything besides the prompt and size that changes the picture (provider, model, quality). Part of the image
    /// key (with the size), so switching models never serves an image from another one.
    fn label(&self) -> String;
    /// The price of one image of this kind and size in USD, or `None` when unknown (then nothing is generated: a
    /// budget can't be enforced on an unknown price).
    fn price_usd(&self, kind: ImageKind, size: &str) -> Option<f64>;
    /// The price of one image sent with `refs` reference images (what budgets reserve). By default the same as
    /// [`ImageGenerator::price_usd`]; [`OpenAiImages`] adds the input-image cost of an edits call.
    fn price_for(&self, kind: ImageKind, size: &str, refs: usize) -> Option<f64> {
        let _ = refs;
        self.price_usd(kind, size)
    }
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
    fn price_usd(&self, _: ImageKind, _: &str) -> Option<f64> {
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
    /// The size for a request that names none (the style config gives every kind its own size).
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

/// The OpenAI Images API (`POST /images/generations`, or `POST /images/edits` with reference images on a gpt-image
/// model), raw HTTPS through reqwest. Never constructed unless the host sets
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

    fn size_of<'a>(&'a self, size: &'a str) -> &'a str {
        if size.trim().is_empty() {
            &self.cfg.size
        } else {
            size
        }
    }

    fn is_gpt_image(&self) -> bool {
        self.cfg.model.starts_with("gpt-image")
    }

    /// The exact request body (no credentials).
    pub fn body(&self, req: &GenRequest) -> Value {
        let mut b =
            json!({ "model": self.cfg.model, "prompt": req.prompt, "n": 1, "size": self.size_of(&req.size) });
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

    /// Whether this request goes to `/images/edits` (reference images on a gpt-image model).
    pub fn uses_edits(&self, req: &GenRequest) -> bool {
        self.is_gpt_image() && !req.refs.is_empty()
    }

    /// The text fields of an edits request: model, prompt, size, quality, n = 1, output_format, and `background:
    /// transparent` when wanted and not yet refused by the model.
    pub fn edit_fields(&self, req: &GenRequest, transparent: bool) -> Vec<(&'static str, String)> {
        let mut f = vec![
            ("model", self.cfg.model.clone()),
            ("prompt", req.prompt.clone()),
            ("size", self.size_of(&req.size).to_string()),
            ("quality", self.cfg.quality.clone()),
            ("n", "1".to_string()),
            ("output_format", "png".to_string()),
        ];
        if transparent && req.kind.wants_transparency() {
            f.push(("background", "transparent".to_string()));
        }
        f
    }

    /// An edits request: `(content-type, multipart body, record)`. `images` are `(file name, bytes)` in order, one
    /// `image[]` part each. The record is what the HTTP log keeps: the text fields, and for each image its file name,
    /// size and sha256 (never the bytes; never the key, which only ever travels in the authorization header).
    pub fn edit_request(
        &self,
        req: &GenRequest,
        images: &[(String, Vec<u8>)],
        transparent: bool,
    ) -> (String, Vec<u8>, Value) {
        let fields = self.edit_fields(req, transparent);
        let boundary = format!("fn7-media-{}", &req.key[..req.key.len().min(32)]);
        let body = multipart(&boundary, &fields, images);
        let mut rec = serde_json::Map::new();
        for (k, v) in &fields {
            rec.insert((*k).to_string(), json!(v));
        }
        rec.insert(
            "image[]".into(),
            json!(images
                .iter()
                .map(|(n, b)| json!({ "file": n, "bytes": b.len(), "sha256": hex::encode(Sha256::digest(b)) }))
                .collect::<Vec<_>>()),
        );
        rec.insert("encoding".into(), json!("multipart/form-data"));
        (format!("multipart/form-data; boundary={boundary}"), body, Value::Object(rec))
    }

    /// What one call actually cost, from the response's `usage` (edits on a model whose token rates are known): the
    /// per-image output price plus input text and image tokens. `None` = use the reserved price.
    fn measured_cost(&self, size: &str, usage: &Value) -> Option<f64> {
        if self.cfg.price_override.is_some() {
            return None;
        }
        let (text_rate, image_rate) = input_rates(&self.cfg.model)?;
        let out = price_per_image(&self.cfg.model, self.size_of(size), &self.cfg.quality)?;
        let image = usage.pointer("/input_tokens_details/image_tokens")?.as_u64()? as f64;
        let text = usage
            .pointer("/input_tokens_details/text_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0) as f64;
        Some(out + image * image_rate + text * text_rate)
    }
}

/// A multipart/form-data body: the text fields, then one `image[]` part per image.
pub fn multipart(boundary: &str, fields: &[(&str, String)], images: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut b: Vec<u8> = Vec::new();
    for (k, v) in fields {
        b.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n").as_bytes(),
        );
    }
    for (name, bytes) in images {
        let safe: String = name.chars().filter(|c| *c != '"' && *c != '\r' && *c != '\n').collect();
        b.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"image[]\"; filename=\"{safe}\"\r\n\
                 Content-Type: {}\r\n\r\n",
                image_mime(name)
            )
            .as_bytes(),
        );
        b.extend_from_slice(bytes);
        b.extend_from_slice(b"\r\n");
    }
    b.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    b
}

fn image_mime(name: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    if n.ends_with(".png") {
        "image/png"
    } else if n.ends_with(".webp") {
        "image/webp"
    } else {
        "image/jpeg"
    }
}

/// Input token prices (USD per token: text, image) for edits calls. **[estimate]**: OpenAI's published per-1M-token
/// prices as we know them (gpt-image-1-mini $2.00 text / $2.50 image; gpt-image-1 $5 / $10), not confirmed against
/// the live pricing page; `FN7_IMAGE_PRICE_USD` overrides the whole per-image price.
pub fn input_rates(model: &str) -> Option<(f64, f64)> {
    match model {
        "gpt-image-1-mini" => Some((2.0e-6, 2.5e-6)),
        "gpt-image-1" => Some((5.0e-6, 10.0e-6)),
        _ => None,
    }
}

/// Input image tokens reserved per reference image (measured: 1,128 per image for gpt-image-1-mini, 4 refs = 4,512
/// and 6 refs = 6,768; rounded up).
pub const EST_TOKENS_PER_REF: f64 = 1200.0;
/// Input text tokens reserved per edits call (measured 260–340 for house-style prompts; rounded up).
pub const EST_TEXT_TOKENS: f64 = 600.0;

/// USD per edits call with `refs` reference images: the per-image price plus the estimated input cost. `None` when the
/// model's image price or input rates are unknown.
pub fn price_per_edit(model: &str, size: &str, quality: &str, refs: usize) -> Option<f64> {
    let base = price_per_image(model, size, quality)?;
    if refs == 0 {
        return Some(base);
    }
    let (text, image) = input_rates(model)?;
    Some(base + refs as f64 * EST_TOKENS_PER_REF * image + EST_TEXT_TOKENS * text)
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
        format!("openai:{}:{}", self.cfg.model, self.cfg.quality)
    }
    fn price_usd(&self, _: ImageKind, size: &str) -> Option<f64> {
        self.cfg
            .price_override
            .or_else(|| price_per_image(&self.cfg.model, self.size_of(size), &self.cfg.quality))
    }
    fn price_for(&self, kind: ImageKind, size: &str, refs: usize) -> Option<f64> {
        if refs == 0 || !self.is_gpt_image() {
            return self.price_usd(kind, size);
        }
        self.cfg
            .price_override
            .or_else(|| price_per_edit(&self.cfg.model, self.size_of(size), &self.cfg.quality, refs))
    }

    async fn generate(&self, req: &GenRequest) -> anyhow::Result<Generated> {
        let edits = self.uses_edits(req);
        let base = self.cfg.base_url.trim_end_matches('/');
        let url = if edits {
            format!("{base}/images/edits")
        } else {
            format!("{base}/images/generations")
        };
        let mut images: Vec<(String, Vec<u8>)> = Vec::new();
        if edits {
            for r in &req.refs {
                let b = tokio::fs::read(&r.path)
                    .await
                    .map_err(|e| anyhow::anyhow!("reference image {}: {e}", r.path.display()))?;
                images.push((r.file_name(), b));
            }
        }
        let json_body = self.body(req);
        let refs_sent = if edits { req.refs.len() } else { 0 };
        let cost = self.price_for(req.kind, &req.size, refs_sent).unwrap_or(0.0);
        // Edits may refuse `background: transparent` on some models: then it is dropped once and the call repeated
        // (the picture comes back opaque).
        let mut transparent = true;
        let mut attempt = 0u32;
        loop {
            let (content_type, bytes, record) = if edits {
                self.edit_request(req, &images, transparent)
            } else {
                (
                    "application/json".to_string(),
                    serde_json::to_vec(&json_body)?,
                    json_body.clone(),
                )
            };
            let started_at = theatre_trace::now_ms();
            let t0 = Instant::now();
            let sent = self
                .http
                .post(&url)
                .bearer_auth(&self.cfg.api_key)
                .header(reqwest::header::CONTENT_TYPE, content_type.as_str())
                .body(bytes)
                .send()
                .await;
            let mut rec = theatre_trace::HttpRecord {
                attempt,
                started_at,
                url: url.clone(),
                request: record,
                request_headers: json!({ "content-type": content_type, "authorization": "Bearer <redacted>" }),
                ..Default::default()
            };
            let resp = match sent {
                Ok(r) => r,
                Err(e) => {
                    rec.duration_ms = t0.elapsed().as_secs_f64() * 1000.0;
                    rec.error = Some(e.to_string());
                    crate::observe::http(rec);
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
                crate::observe::http(rec);
                if edits
                    && transparent
                    && req.kind.wants_transparency()
                    && status.as_u16() == 400
                    && msg.to_ascii_lowercase().contains("background")
                {
                    tracing::warn!(model = %self.cfg.model, "images/edits refused background=transparent; retrying opaque");
                    transparent = false;
                    continue;
                }
                if attempt < self.cfg.retries
                    && (status.as_u16() == 429 || status.is_server_error())
                {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_secs(2u64.pow(attempt))).await;
                    continue;
                }
                anyhow::bail!("openai images {status}: {msg}");
            }
            crate::observe::http(rec);
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
            let cost = match edits {
                true => parsed
                    .get("usage")
                    .and_then(|u| self.measured_cost(&req.size, u))
                    .unwrap_or(cost),
                false => cost,
            };
            return Ok(Generated {
                bytes,
                ext: sniff_ext(&json_body),
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
            size: "1536x1024".into(),
            refs: vec![],
        };
        let b = g.body(&r(ImageKind::Statue));
        assert_eq!(b["background"], "transparent");
        assert!(!b.to_string().contains("sk-test"));
        assert!(g.body(&r(ImageKind::Event)).get("background").is_none());
        assert_eq!(b["size"], "1536x1024", "the style's size wins");
        assert_eq!(g.price_usd(ImageKind::Event, "1536x1024"), Some(0.015));
        assert_eq!(g.price_usd(ImageKind::Event, ""), Some(0.011));
    }

    fn gen(model: &str, base_url: &str, price_override: Option<f64>) -> OpenAiImages {
        OpenAiImages::new(OpenAiConfig {
            api_key: "sk-test-SECRET".into(),
            base_url: base_url.into(),
            model: model.into(),
            size: "1024x1024".into(),
            quality: "medium".into(),
            price_override,
            timeout: Duration::from_secs(5),
            retries: 0,
        })
        .unwrap()
    }

    fn refs_in(dir: &std::path::Path, n: usize) -> Vec<RefImage> {
        std::fs::create_dir_all(dir).unwrap();
        (0..n)
            .map(|i| {
                let path = dir.join(format!("IMG_{i}.jpg"));
                std::fs::write(&path, vec![0xffu8, 0xd8, i as u8, 0x42]).unwrap();
                RefImage { name: format!("style_refs/IMG_{i}.jpg"), path, sha256: format!("{i:064}") }
            })
            .collect()
    }

    fn temp(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("fn7-gen-{tag}-{}-{}", std::process::id(), theatre_trace::now_ms()))
    }

    #[test]
    fn edits_request_is_multipart_with_one_image_part_per_ref_and_no_key() {
        let g = gen("gpt-image-1-mini", "http://127.0.0.1:9", None);
        let refs = refs_in(&temp("mp"), 6);
        let req = GenRequest {
            key: "ab".repeat(32),
            kind: ImageKind::Diorama,
            prompt: "A tiny figure. Style: x.".into(),
            size: "1024x1024".into(),
            refs: refs.clone(),
        };
        assert!(g.uses_edits(&req));
        let images: Vec<(String, Vec<u8>)> =
            refs.iter().map(|r| (r.file_name(), std::fs::read(&r.path).unwrap())).collect();
        let (ct, body, rec) = g.edit_request(&req, &images, true);
        let text = String::from_utf8_lossy(&body);
        assert!(ct.starts_with("multipart/form-data; boundary=fn7-media-"));
        assert_eq!(text.matches("name=\"image[]\"").count(), 6, "one image[] part per reference");
        for f in ["model", "prompt", "size", "quality", "n", "output_format", "background"] {
            assert_eq!(text.matches(&format!("name=\"{f}\"\r\n")).count(), 1, "{f}");
        }
        assert!(text.contains("filename=\"IMG_0.jpg\"\r\nContent-Type: image/jpeg"));
        assert!(text.contains("gpt-image-1-mini") && text.contains("\r\n1\r\n"), "n=1");
        assert!(text.ends_with("--\r\n"));
        // The record: fields and file names, never the key nor the image bytes.
        assert!(!text.contains("sk-test") && !rec.to_string().contains("sk-test"));
        assert_eq!(rec["image[]"].as_array().unwrap().len(), 6);
        assert_eq!(rec["image[]"][0]["file"], "IMG_0.jpg");
        assert_eq!(rec["image[]"][0]["bytes"], 4);
        assert_eq!(rec["model"], "gpt-image-1-mini");
        assert_eq!(rec["background"], "transparent");
        // Opaque fallback drops only `background`; a portrait never asks for it.
        let (_, body2, rec2) = g.edit_request(&req, &images, false);
        assert!(!String::from_utf8_lossy(&body2).contains("name=\"background\"") && rec2.get("background").is_none());
        let portrait = GenRequest { kind: ImageKind::Portrait, ..req.clone() };
        assert!(g.edit_fields(&portrait, true).iter().all(|(k, _)| *k != "background"));
        // No refs, or a non-gpt-image model: generations as before.
        assert!(!g.uses_edits(&GenRequest { refs: vec![], ..req.clone() }));
        assert!(!gen("dall-e-3", "http://127.0.0.1:9", None).uses_edits(&req));
    }

    #[test]
    fn edits_cost_more_by_the_input_images_and_the_override_wins() {
        let g = gen("gpt-image-1-mini", "http://127.0.0.1:9", None);
        let six = g.price_for(ImageKind::Portrait, "1024x1024", 6).unwrap();
        let expect = 0.011 + 6.0 * EST_TOKENS_PER_REF * 2.5e-6 + EST_TEXT_TOKENS * 2.0e-6;
        assert!((six - expect).abs() < 1e-12, "{six} vs {expect}");
        assert!(six > 0.028 && six < 0.032, "{six}");
        assert_eq!(g.price_for(ImageKind::Portrait, "1024x1024", 0), Some(0.011));
        assert_eq!(gen("gpt-image-1-mini", "x", Some(0.05)).price_for(ImageKind::Event, "1536x1024", 6), Some(0.05));
        assert_eq!(gen("gpt-image-2-new", "x", None).price_for(ImageKind::Event, "1024x1024", 4), None);
        // The measured cost comes from usage (the prototype's 6-ref call: 6,768 image + 277 text tokens).
        let usage = json!({ "input_tokens_details": { "image_tokens": 6768, "text_tokens": 277 } });
        let m = g.measured_cost("1024x1024", &usage).unwrap();
        assert!((m - (0.011 + 6768.0 * 2.5e-6 + 277.0 * 2.0e-6)).abs() < 1e-12);
        assert!(m <= six, "the reservation covers a measured call");
    }

    #[tokio::test]
    async fn edits_call_end_to_end_with_opaque_fallback() {
        use axum::{body::Bytes, http::HeaderMap, routing::post, Router};
        use std::sync::{Arc, Mutex};
        let seen: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
        let s2 = seen.clone();
        let app = Router::new().route(
            "/images/edits",
            post(move |headers: HeaderMap, body: Bytes| {
                let seen = s2.clone();
                async move {
                    let text = String::from_utf8_lossy(&body).to_string();
                    let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
                    let first = { let mut s = seen.lock().unwrap(); s.push((auth, text.clone())); s.len() == 1 };
                    if first && text.contains("name=\"background\"") {
                        return (axum::http::StatusCode::BAD_REQUEST,
                            axum::Json(json!({ "error": { "message": "Invalid value for background: transparent" } })));
                    }
                    (axum::http::StatusCode::OK, axum::Json(json!({
                        "data": [ { "b64_json": base64::engine::general_purpose::STANDARD.encode(b"PNGBYTES") } ],
                        "usage": { "input_tokens_details": { "image_tokens": 4512, "text_tokens": 300 } } })))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let g = gen("gpt-image-1-mini", &format!("http://{addr}"), None);
        let req = GenRequest {
            key: "cd".repeat(32),
            kind: ImageKind::Diorama,
            prompt: "p".into(),
            size: "1024x1024".into(),
            refs: refs_in(&temp("e2e"), 4),
        };
        let out = g.generate(&req).await.unwrap();
        assert_eq!(out.bytes, b"PNGBYTES");
        assert!((out.cost_usd - (0.011 + 4512.0 * 2.5e-6 + 300.0 * 2.0e-6)).abs() < 1e-12, "measured cost");
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2, "refused transparent once, then opaque");
        assert_eq!(seen[0].0, "Bearer sk-test-SECRET", "the key travels only in the header");
        assert!(seen[0].1.contains("name=\"background\"") && !seen[1].1.contains("name=\"background\""));
        assert_eq!(seen[1].1.matches("name=\"image[]\"").count(), 4);
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

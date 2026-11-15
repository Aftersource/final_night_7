//! The observer seam: one optional [`MediaObserver`] per [`crate::Media`], told about every image at the moment the
//! service records it (the same five places that write the `media_*` trace events): requested, served from the disk
//! cache, refused, ready, failed. The asset registry (`fn7-assets`) hangs on it; nothing in this crate depends on who
//! listens. An observer must return quickly (it runs on the caller's thread, inside a hook or the queue worker).
//!
//! [`collect`] gathers the provider exchanges one generation made ([`HttpCall`]: timing, the request as recorded,
//! the response's `usage`, the provider's request id), so an observer sees exactly what was sent and what it cost.

use std::path::Path;
use std::sync::Mutex;

use serde_json::Value;

use crate::cache::Sidecar;
use crate::prompt::ImagePrompt;

/// One provider exchange of a generation (one HTTP request and its answer). Never holds a key or image bytes.
#[derive(Debug, Clone, Default)]
pub struct HttpCall {
    pub attempt: u32,
    /// Unix ms.
    pub started_at: i64,
    pub duration_ms: f64,
    pub url: String,
    pub status: Option<u16>,
    /// The request as recorded: the JSON body, or for a multipart call its text fields plus `{file, bytes, sha256}`
    /// per reference image.
    pub request: Value,
    /// The response's `usage` object, when the provider sent one.
    pub usage: Option<Value>,
    /// The provider's request id (`x-request-id`), when sent.
    pub request_id: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Accepted and queued.
    Requested,
    /// Already on disk: nothing is generated.
    Cached,
    /// Over budget, over a cap, unknown price or a full queue: nothing is generated.
    Refused,
    /// Generated and stored.
    Ready,
    /// The generator failed.
    Failed,
}

/// What an observer is told. Borrowed: copy what you keep.
#[derive(Debug)]
pub struct MediaEvent<'a> {
    pub stage: Stage,
    /// The image key (the `/media` hash).
    pub hash: &'a str,
    /// The prompt exactly as sent (`text`), its kind, size and reference images (path + sha256).
    pub prompt: &'a ImagePrompt,
    /// `"mock"`, `"openai"`.
    pub provider: &'a str,
    /// The generator label (`openai:<model>:<quality>`, `mock`).
    pub label: &'a str,
    /// The match that asked (the recorder's run id; `prewarm` for the startup prewarm).
    pub match_id: Option<&'a str>,
    /// The beat of the hook that asked, when it ran inside a match's trace context.
    pub round: Option<u32>,
    /// Unix ms of this event (the wall clock, not the manifest's cursor, which may run a few ms ahead).
    pub at_ms: i64,
    /// The price reserved for it (USD), 0 when unknown or free.
    pub price_usd: f64,
    /// Why it was refused, or the generator's error.
    pub reason: Option<&'a str>,
    /// Ready and Cached: the sidecar (model, ext, bytes, created_at, cost_usd, latency_ms, first match).
    pub sidecar: Option<&'a Sidecar>,
    /// Ready and Cached: the image file in the cache.
    pub path: Option<&'a Path>,
    /// Ready and Failed: how long the generator took.
    pub latency_ms: Option<f64>,
    /// Ready and Failed: every provider exchange of this generation, in order (empty for the mock).
    pub calls: &'a [HttpCall],
}

pub trait MediaObserver: Send + Sync {
    fn on_media(&self, ev: &MediaEvent<'_>);
}

tokio::task_local! {
    static CALLS: Mutex<Vec<HttpCall>>;
}

/// Run a generation and return its result with the provider exchanges it reported through [`http`].
pub async fn collect<F: std::future::Future>(fut: F) -> (F::Output, Vec<HttpCall>) {
    CALLS
        .scope(Mutex::new(Vec::new()), async move {
            let out = fut.await;
            let calls = CALLS.with(|c| std::mem::take(&mut *c.lock().unwrap_or_else(|e| e.into_inner())));
            (out, calls)
        })
        .await
}

/// Record one provider exchange: into the trace (as before) and into the generation being [`collect`]ed, if any.
pub fn http(rec: theatre_trace::HttpRecord) {
    let _ = CALLS.try_with(|c| {
        let header = |k: &str| rec.response_headers.get(k).and_then(Value::as_str).map(str::to_string);
        c.lock().unwrap_or_else(|e| e.into_inner()).push(HttpCall {
            attempt: rec.attempt,
            started_at: rec.started_at,
            duration_ms: rec.duration_ms,
            url: rec.url.clone(),
            status: rec.status,
            request: rec.request.clone(),
            usage: rec.response.get("usage").filter(|u| !u.is_null()).cloned(),
            request_id: header("x-request-id"),
            error: rec.error.clone(),
        });
    });
    theatre_trace::http(rec);
}

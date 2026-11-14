//! The asset routes, merged into `fn7-server`'s router (`fn7_assets::http::router(registry, cfg)`):
//!
//! | Route | Answer |
//! |---|---|
//! | `GET /api/assets?subject_kind=&subject=&kind=&match=&beat=&status=&current=&media_hash=&sha256=&limit=&after=` | a page of assets, newest first: `{items, next, limit, total}` |
//! | `GET /api/assets/{id}` | `{asset, current, generations, lineage: {parents, children}, uses}`; `{id}` may be a media hash |
//! | `GET /api/assets/{id}/file` | the bytes: immutable cache headers, `ETag` = sha256, `Range` requests |
//! | `GET /api/assets/{id}/chain` | the whole lineage chain, oldest ancestor first |
//! | `GET /api/assets/figures?name=` | the current statue / figure of a demigod or god, and the mannequin |
//! | `GET /api/assets/costs?by=day\|kind\|provider\|match\|subject\|basis\|model&usd_per_credit=` | roll-ups |
//! | `GET /api/assets/at?match=&beat=&demigod=` | time travel: what was shown at that beat |
//! | `GET /api/assets/mine` | the assets of the signed-in account's names (`Authorization: Bearer <session>`) |
//! | `GET /api/assets/stats` | counts by kind and status, cost totals, known gaps |
//! | `POST /api/assets/register` | admin: register an operator-made asset |
//!
//! **Access.** Like the profile routes: when a key is configured (`FN7_ASSETS_KEY`, else `FN7_PROFILE_KEY`), every
//! request needs `?key=<KEY>` (403 otherwise). `mine` also needs a session; `register` needs the engine's admin token
//! (`Authorization: Bearer <THEATRE_ADMIN_TOKEN>`; 404 when the server has none) and no key.

use std::io::SeekFrom;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::query::Filter;
use crate::subject::{name_key, Subject};
use crate::{asset_id_for_key, generation_id_for_key, now_ms, AssetIn, GenerationIn, Registry};

/// Who a session token belongs to: the account id and the names it holds (its profile and its demigods).
#[derive(Debug, Clone)]
pub struct Me {
    pub account: String,
    pub names: Vec<String>,
}

/// Resolves a session token (the server implements it over the accounts store).
pub trait Sessions: Send + Sync {
    fn me(&self, token: &str) -> Option<Me>;
}

#[derive(Clone, Default)]
pub struct HttpConfig {
    /// When set, every request needs `?key=<this>`.
    pub key: Option<String>,
    /// The engine's admin token (`register`).
    pub admin_token: Option<String>,
    pub sessions: Option<Arc<dyn Sessions>>,
}

impl HttpConfig {
    /// `FN7_ASSETS_KEY`, else `FN7_PROFILE_KEY` (empty = no app-level check; the tunnel's gate still applies).
    pub fn from_env(env: impl Fn(&str) -> Option<String>) -> Self {
        let key = env("FN7_ASSETS_KEY").filter(|k| !k.trim().is_empty()).or_else(|| env("FN7_PROFILE_KEY").filter(|k| !k.trim().is_empty()));
        Self { key, admin_token: None, sessions: None }
    }
}

#[derive(Clone)]
struct App {
    reg: Arc<Registry>,
    cfg: Arc<HttpConfig>,
}

/// The largest file `register` accepts inline (base64 in JSON). Bigger files: `fn7-assets import-jobs`.
const MAX_REGISTER_BYTES: usize = 96 << 20;

pub fn router(reg: Arc<Registry>, cfg: HttpConfig) -> Router {
    Router::new()
        .route("/api/assets", get(list))
        .route("/api/assets/figures", get(figures))
        .route("/api/assets/costs", get(costs))
        .route("/api/assets/at", get(at))
        .route("/api/assets/mine", get(mine))
        .route("/api/assets/stats", get(stats))
        .route("/api/assets/register", post(register).layer(DefaultBodyLimit::max(MAX_REGISTER_BYTES * 4 / 3 + (1 << 20))))
        .route("/api/assets/{id}", get(detail))
        .route("/api/assets/{id}/file", get(file))
        .route("/api/assets/{id}/chain", get(chain))
        .with_state(App { reg, cfg: Arc::new(cfg) })
}

#[derive(Deserialize, Default)]
struct Q {
    key: Option<String>,
    subject_kind: Option<String>,
    subject: Option<String>,
    kind: Option<String>,
    #[serde(rename = "match")]
    match_id: Option<String>,
    beat: Option<i64>,
    status: Option<String>,
    current: Option<String>,
    media_hash: Option<String>,
    sha256: Option<String>,
    limit: Option<u32>,
    after: Option<String>,
    name: Option<String>,
    by: Option<String>,
    usd_per_credit: Option<f64>,
    demigod: Option<String>,
}

fn eq_ct(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn finish(mut r: Response) -> Response {
    let h = r.headers_mut();
    h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    if !h.contains_key(header::CACHE_CONTROL) {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    r
}

fn fail(code: StatusCode, error: &str, detail: impl Into<String>) -> Response {
    finish((code, Json(json!({ "error": error, "detail": detail.into() }))).into_response())
}

fn gate(app: &App, q: &Q) -> Option<Response> {
    let want = app.cfg.key.as_deref()?;
    match q.key.as_deref() {
        Some(k) if eq_ct(k, want) => None,
        _ => Some(finish((StatusCode::FORBIDDEN, "forbidden").into_response())),
    }
}

fn bearer(h: &HeaderMap) -> Option<String> {
    h.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Bearer ").map(|t| t.trim().to_string())
}

fn nonempty(s: &Option<String>) -> Option<String> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

fn filter_of(q: &Q) -> Filter {
    Filter {
        subject_kind: nonempty(&q.subject_kind),
        subject: nonempty(&q.subject),
        kind: nonempty(&q.kind),
        match_id: nonempty(&q.match_id),
        beat: q.beat,
        status: nonempty(&q.status),
        media_hash: nonempty(&q.media_hash),
        sha256: nonempty(&q.sha256),
        current: matches!(q.current.as_deref(), Some("1" | "true" | "yes")),
        mine: None,
        limit: q.limit,
        after: nonempty(&q.after),
    }
}

/// Run a registry call off the async runtime and answer with its JSON.
async fn answer(app: &App, f: impl FnOnce(&Registry) -> crate::Result<Option<Value>> + Send + 'static) -> Response {
    let reg = app.reg.clone();
    match tokio::task::spawn_blocking(move || f(&reg)).await {
        Ok(Ok(Some(v))) => finish(Json(v).into_response()),
        Ok(Ok(None)) => fail(StatusCode::NOT_FOUND, "not_found", "no such asset"),
        Ok(Err(e)) => {
            let msg = e.to_string();
            let code = if msg.starts_with("by=") || msg.starts_with("invalid:") { StatusCode::BAD_REQUEST } else { StatusCode::INTERNAL_SERVER_ERROR };
            fail(code, if code == StatusCode::BAD_REQUEST { "invalid" } else { "store" }, msg)
        }
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, "store", e.to_string()),
    }
}

async fn list(State(app): State<App>, Query(q): Query<Q>) -> Response {
    if let Some(r) = gate(&app, &q) {
        return r;
    }
    let f = filter_of(&q);
    answer(&app, move |r| r.list(&f).map(Some)).await
}

async fn mine(State(app): State<App>, Query(q): Query<Q>, headers: HeaderMap) -> Response {
    if let Some(r) = gate(&app, &q) {
        return r;
    }
    let Some(sessions) = app.cfg.sessions.clone() else {
        return fail(StatusCode::NOT_FOUND, "not_found", "accounts are not attached");
    };
    let Some(token) = bearer(&headers) else {
        return fail(StatusCode::UNAUTHORIZED, "unauthorized", "send the session as Authorization: Bearer");
    };
    let me = match tokio::task::spawn_blocking(move || sessions.me(&token)).await {
        Ok(Some(me)) => me,
        _ => return fail(StatusCode::UNAUTHORIZED, "unauthorized", "no such session"),
    };
    let mut f = filter_of(&q);
    let keys: Vec<String> = me.names.iter().filter_map(|n| name_key(n)).collect();
    f.mine = Some((keys.clone(), me.account.clone()));
    answer(&app, move |r| {
        let mut v = r.list(&f)?;
        v["account"] = json!(me.account);
        v["subjects"] = json!(keys);
        Ok(Some(v))
    })
    .await
}

async fn detail(State(app): State<App>, Path(id): Path<String>, Query(q): Query<Q>) -> Response {
    if let Some(r) = gate(&app, &q) {
        return r;
    }
    answer(&app, move |r| match r.resolve_id(&id)? {
        Some(id) => r.detail(&id),
        None => Ok(None),
    })
    .await
}

async fn chain(State(app): State<App>, Path(id): Path<String>, Query(q): Query<Q>) -> Response {
    if let Some(r) = gate(&app, &q) {
        return r;
    }
    answer(&app, move |r| match r.resolve_id(&id)? {
        Some(id) => Ok(Some(json!({ "id": id, "chain": r.chain(&id)? }))),
        None => Ok(None),
    })
    .await
}

async fn figures(State(app): State<App>, Query(q): Query<Q>) -> Response {
    if let Some(r) = gate(&app, &q) {
        return r;
    }
    let name = nonempty(&q.name);
    answer(&app, move |r| r.figures(name.as_deref()).map(Some)).await
}

async fn costs(State(app): State<App>, Query(q): Query<Q>) -> Response {
    if let Some(r) = gate(&app, &q) {
        return r;
    }
    let by = nonempty(&q.by).unwrap_or_else(|| "day".into());
    let rate = q.usd_per_credit.filter(|r| r.is_finite() && *r >= 0.0);
    answer(&app, move |r| r.costs(&by, rate).map(Some)).await
}

async fn at(State(app): State<App>, Query(q): Query<Q>) -> Response {
    if let Some(r) = gate(&app, &q) {
        return r;
    }
    let Some(m) = nonempty(&q.match_id) else {
        return fail(StatusCode::BAD_REQUEST, "invalid", "`match` is required");
    };
    let (beat, demigod) = (q.beat, nonempty(&q.demigod));
    answer(&app, move |r| r.at(&m, beat, demigod.as_deref()).map(Some)).await
}

async fn stats(State(app): State<App>, Query(q): Query<Q>) -> Response {
    if let Some(r) = gate(&app, &q) {
        return r;
    }
    answer(&app, move |r| crate::ingest::stats(r).map(Some)).await
}

/// `bytes=a-b`, `bytes=a-`, `bytes=-n` → (start, end inclusive). `None` = not a single satisfiable range.
fn parse_range(h: &str, len: u64) -> Option<(u64, u64)> {
    let spec = h.trim().strip_prefix("bytes=")?;
    if spec.contains(',') || len == 0 {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let (start, end) = match (a.trim(), b.trim()) {
        ("", n) => {
            let n: u64 = n.parse().ok()?;
            (len.saturating_sub(n.max(1).min(len)), len - 1)
        }
        (s, "") => (s.parse().ok()?, len - 1),
        (s, e) => (s.parse().ok()?, e.parse::<u64>().ok()?.min(len - 1)),
    };
    (start <= end && start < len).then_some((start, end))
}

async fn file(State(app): State<App>, Path(id): Path<String>, Query(q): Query<Q>, headers: HeaderMap) -> Response {
    if let Some(r) = gate(&app, &q) {
        return r;
    }
    let reg = app.reg.clone();
    let found = tokio::task::spawn_blocking(move || -> crate::Result<Option<(String, String, String)>> {
        let Some(id) = reg.resolve_id(&id)? else { return Ok(None) };
        reg.with(|c| {
            use rusqlite::OptionalExtension;
            Ok(c.query_row("SELECT path, COALESCE(mime, 'application/octet-stream'), sha256 FROM assets WHERE id = ?1 AND path IS NOT NULL", [&id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?)
        })
    })
    .await;
    let (rel, mime, sha) = match found {
        Ok(Ok(Some(x))) => x,
        Ok(Ok(None)) => return fail(StatusCode::NOT_FOUND, "not_found", "no such asset, or it has no file"),
        _ => return fail(StatusCode::INTERNAL_SERVER_ERROR, "store", "lookup failed"),
    };
    let etag = format!("\"{sha}\"");
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()).is_some_and(|v| v.split(',').any(|t| t.trim() == etag)) {
        let mut r = StatusCode::NOT_MODIFIED.into_response();
        r.headers_mut().insert(header::ETAG, HeaderValue::from_str(&etag).unwrap());
        r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=31536000, immutable"));
        return finish(r);
    }
    let Some(path) = app.reg.file_path(&rel) else {
        return fail(StatusCode::NOT_FOUND, "not_found", "bad path");
    };
    let Ok(mut f) = tokio::fs::File::open(&path).await else {
        return fail(StatusCode::NOT_FOUND, "file_missing", "the registry row has a file that is not on disk");
    };
    let len = f.metadata().await.map(|m| m.len()).unwrap_or(0);
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    let (status, start, end) = match range {
        None => (StatusCode::OK, 0, len.saturating_sub(1)),
        Some(h) => match parse_range(h, len) {
            Some((a, b)) => (StatusCode::PARTIAL_CONTENT, a, b),
            None => {
                let mut r = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
                r.headers_mut().insert(header::CONTENT_RANGE, HeaderValue::from_str(&format!("bytes */{len}")).unwrap());
                return finish(r);
            }
        },
    };
    let n = if len == 0 { 0 } else { end - start + 1 };
    if start > 0 && f.seek(SeekFrom::Start(start)).await.is_err() {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, "store", "seek failed");
    }
    // Streamed in 64 KiB pieces: a 40 MB model never sits in memory whole.
    let stream = futures_stream(f.take(n));
    let mut r = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, n)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::ETAG, &etag)
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .header("x-asset-sha256", &sha);
    if status == StatusCode::PARTIAL_CONTENT {
        r = r.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"));
    }
    finish(r.body(stream).unwrap_or_else(|_| Response::new(Body::empty())))
}

/// A body that reads the file as it is sent.
fn futures_stream(reader: tokio::io::Take<tokio::fs::File>) -> Body {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Vec<u8>, std::io::Error>>(4);
    tokio::spawn(async move {
        let mut reader = reader;
        loop {
            let mut buf = vec![0u8; 1 << 16];
            match reader.read(&mut buf).await {
                Ok(0) => break,
                Ok(k) => {
                    buf.truncate(k);
                    if tx.send(Ok(buf)).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    break;
                }
            }
        }
    });
    Body::from_stream(ReceiverStream(rx))
}

struct ReceiverStream(tokio::sync::mpsc::Receiver<Result<Vec<u8>, std::io::Error>>);

impl futures_core::Stream for ReceiverStream {
    type Item = Result<Vec<u8>, std::io::Error>;
    fn poll_next(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Self::Item>> {
        self.0.poll_recv(cx)
    }
}

#[derive(Deserialize)]
struct RegisterBody {
    kind: String,
    subkind: String,
    subject_kind: String,
    /// A name (normalized into the key) or, for `shared` / `match`, the key itself.
    subject: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    variant: String,
    /// The file: base64 in `data_base64` with its `ext`, or a `path` under the assets root's `incoming/` directory.
    #[serde(default)]
    data_base64: Option<String>,
    #[serde(default)]
    ext: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    meta: Option<Value>,
    #[serde(default)]
    generation: Option<Value>,
    /// `[{parent, role}]`: parent asset ids.
    #[serde(default)]
    lineage: Vec<Value>,
}

fn admin(app: &App, headers: &HeaderMap) -> Option<Response> {
    let Some(want) = app.cfg.admin_token.as_deref().filter(|t| !t.is_empty()) else {
        return Some(fail(StatusCode::NOT_FOUND, "not_found", "not found"));
    };
    match bearer(headers) {
        Some(t) if eq_ct(&t, want) => None,
        _ => Some(fail(StatusCode::UNAUTHORIZED, "unauthorized", "bad admin token")),
    }
}

async fn register(State(app): State<App>, headers: HeaderMap, Json(b): Json<RegisterBody>) -> Response {
    if let Some(r) = admin(&app, &headers) {
        return r;
    }
    let reg = app.reg.clone();
    let done = tokio::task::spawn_blocking(move || -> crate::Result<Value> {
        let bad = |m: &str| anyhow::anyhow!("invalid: {m}");
        if !crate::schema::KINDS.contains(&b.kind.as_str()) {
            return Err(bad("kind must be image | model | texture | animation"));
        }
        if !crate::schema::SUBJECT_KINDS.contains(&b.subject_kind.as_str()) {
            return Err(bad("subject_kind must be demigod | god | item | place | match | shared | person"));
        }
        if b.subkind.trim().is_empty() || b.subject.trim().is_empty() {
            return Err(bad("subkind and subject are required"));
        }
        let skind: &'static str = crate::schema::SUBJECT_KINDS.iter().copied().find(|k| *k == b.subject_kind).unwrap();
        let subject = match skind {
            "shared" | "match" => Subject::new(skind, b.subject.trim(), b.display_name.clone().unwrap_or_else(|| b.subject.trim().to_string())),
            _ => {
                let mut s = Subject::named(skind, &b.subject);
                if s.key == "unknown" {
                    return Err(bad("subject is not a usable name"));
                }
                if let Some(d) = &b.display_name {
                    s.name = d.clone();
                }
                s
            }
        };
        let stored = match (&b.data_base64, &b.path) {
            (Some(d), _) => {
                let bytes = decode_base64(d).ok_or_else(|| bad("data_base64 is not base64"))?;
                if bytes.len() > MAX_REGISTER_BYTES {
                    return Err(bad("file too large for register; use fn7-assets import-jobs"));
                }
                reg.store_bytes(&bytes, &b.kind, &b.subkind, b.ext.as_deref().unwrap_or("bin"))?
            }
            (None, Some(p)) => {
                let src = reg.file_path(&format!("incoming/{}", p.trim_start_matches('/'))).ok_or_else(|| bad("path must stay under incoming/"))?;
                if !src.is_file() {
                    return Err(bad("no such file under the assets root's incoming/ directory"));
                }
                reg.store_file(&src, &b.kind, &b.subkind, crate::Link::Copy)?
            }
            (None, None) => return Err(bad("send the file as data_base64 (+ ext) or a path under incoming/")),
        };
        let id = asset_id_for_key(&format!("file:{}:{}/{}:{}:{}:{}", stored.sha256, b.kind, b.subkind, subject.kind, subject.key, b.variant));
        let now = now_ms();
        let g = b.generation.clone().unwrap_or(Value::Null);
        reg.tx(|c| {
            crate::upsert_asset(c, &AssetIn {
                id: id.clone(), kind: b.kind.clone(), subkind: b.subkind.clone(), subject: Some(subject.clone()), subject_strong: true,
                variant: b.variant.clone(), file: Some(stored.clone()), created_at: now, status: "ready".into(), source: "operator".into(),
                meta: b.meta.clone(), ..Default::default()
            })?;
            if g.is_object() {
                let s = |k: &str| g[k].as_str().map(str::to_string);
                crate::upsert_generation(c, &GenerationIn {
                    id: generation_id_for_key(&format!("operator:{id}:{}", s("task_id").unwrap_or_default())),
                    asset_id: id.clone(),
                    provider: s("provider").unwrap_or_else(|| "operator".into()),
                    model: s("model"), operation: s("operation"), endpoint: s("endpoint"), prompt: s("prompt"),
                    negative_prompt: s("negative_prompt"), params: Some(g["params"].clone()).filter(|p| !p.is_null()),
                    provider_task_id: s("task_id"), requested_at: g["requested_at"].as_i64(), started_at: g["started_at"].as_i64(),
                    finished_at: g["finished_at"].as_i64().or(Some(now)), duration_ms: g["duration_ms"].as_f64(),
                    cost_usd: g["cost_usd"].as_f64(), cost_credits: g["cost_credits"].as_f64().or(g["credits"].as_f64()),
                    cost_basis: s("cost_basis"), usage: Some(g["usage"].clone()).filter(|p| !p.is_null()), status: "ok".into(),
                    requested_by: Some("operator".into()), source: "operator".into(), ..Default::default()
                })?;
            }
            for l in &b.lineage {
                let (Some(parent), Some(role)) = (l["parent"].as_str(), l["role"].as_str()) else { continue };
                crate::add_lineage(c, parent, &id, role, l["note"].as_str()).map_err(|e| anyhow::anyhow!("invalid: {e}"))?;
            }
            Ok(())
        })?;
        Ok(reg.detail(&id)?.unwrap_or(Value::Null))
    })
    .await;
    match done {
        Ok(Ok(v)) => finish((StatusCode::CREATED, Json(v)).into_response()),
        Ok(Err(e)) => {
            let m = e.to_string();
            match m.strip_prefix("invalid: ") {
                Some(d) => fail(StatusCode::BAD_REQUEST, "invalid", d),
                None if m.contains("FOREIGN KEY") => fail(StatusCode::BAD_REQUEST, "invalid", "a lineage parent does not exist"),
                None => fail(StatusCode::INTERNAL_SERVER_ERROR, "store", m),
            }
        }
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, "store", e.to_string()),
    }
}

/// Standard base64 (padding optional, whitespace ignored). Small and dependency-free.
fn decode_base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for b in s.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

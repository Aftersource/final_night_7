//! The profile routes, merged into `fn7-server`'s router at integration (`fn7_profile::http::router(store, cfg)`):
//!
//! | Route | Answer |
//! |---|---|
//! | `GET /api/profiles/{name}` | [`crate::Profile`] (404 if none) |
//! | `GET /api/profiles/{name}/history?page=&per_page=` | [`crate::Page`] of [`crate::MatchRecord`], newest first |
//! | `GET /api/profiles/{name}/library?page=&per_page=&kind=` | [`crate::Page`] of [`crate::LibraryImage`] |
//! | `PUT /api/profiles/{name}/description` `{text, secret?}` | the profile (403 if claimed and the secret is wrong) |
//! | `POST /api/profiles/{name}/claim` `{secret, old_secret?}` | 204 |
//!
//! **Access.** Behind the ngrok tunnel every request already needs `?key=<KEY>` (the host's gate). The routes add
//! their own check too ([`HttpConfig::key`], from `FN7_PROFILE_KEY` or the tunnel's key): when set, a request without
//! the matching `key` query parameter is refused with 403, so the LAN port is gated as well. Nothing private is ever
//! served: profiles hold only what every seat saw at match end, never a `player_id`, resume token or claim hash.
//! Writes are not rate-limited here (they are rare and small, and the store caps text sizes); the server may add a
//! limiter at integration.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;

use crate::{ProfileError, ProfileStore};

#[derive(Debug, Clone, Default)]
pub struct HttpConfig {
    /// When set, every request needs `?key=<this>`.
    pub key: Option<String>,
}

impl HttpConfig {
    /// `FN7_PROFILE_KEY` (empty = no app-level check; the tunnel's gate still applies).
    pub fn from_env(env: impl Fn(&str) -> Option<String>) -> Self {
        Self {
            key: env("FN7_PROFILE_KEY").filter(|k| !k.trim().is_empty()),
        }
    }
}

#[derive(Clone)]
struct AppState {
    store: Arc<ProfileStore>,
    cfg: Arc<HttpConfig>,
}

pub fn router(store: Arc<ProfileStore>, cfg: HttpConfig) -> Router {
    Router::new()
        .route("/api/profiles/{name}", get(profile))
        .route("/api/profiles/{name}/history", get(history))
        .route("/api/profiles/{name}/library", get(library))
        .route("/api/profiles/{name}/description", put(description))
        .route("/api/profiles/{name}/claim", post(claim))
        .with_state(AppState {
            store,
            cfg: Arc::new(cfg),
        })
}

#[derive(Deserialize, Default)]
struct Q {
    key: Option<String>,
    page: Option<u32>,
    per_page: Option<u32>,
    kind: Option<String>,
}

#[derive(Deserialize)]
struct DescriptionBody {
    text: String,
    #[serde(default)]
    secret: Option<String>,
}

#[derive(Deserialize)]
struct ClaimBody {
    secret: String,
    #[serde(default)]
    old_secret: Option<String>,
}

fn eq_ct(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn finish(mut r: Response) -> Response {
    let h = r.headers_mut();
    h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

fn err(e: ProfileError) -> Response {
    let (code, msg) = match &e {
        ProfileError::BadName => (StatusCode::BAD_REQUEST, "bad_name"),
        ProfileError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
        ProfileError::Locked => (StatusCode::FORBIDDEN, "claimed"),
        ProfileError::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid"),
        ProfileError::Sql(_) | ProfileError::Json(_) => (StatusCode::INTERNAL_SERVER_ERROR, "store"),
    };
    let detail = match &e {
        ProfileError::Invalid(s) => s.clone(),
        _ => msg.to_string(),
    };
    finish((code, Json(serde_json::json!({ "error": msg, "detail": detail }))).into_response())
}

fn gate(st: &AppState, q: &Q) -> Option<Response> {
    let want = st.cfg.key.as_deref()?;
    match q.key.as_deref() {
        Some(k) if eq_ct(k, want) => None,
        _ => Some(finish((StatusCode::FORBIDDEN, "forbidden").into_response())),
    }
}

/// Run a store call off the async runtime.
async fn blocking<T: Send + 'static>(
    st: &AppState,
    f: impl FnOnce(&ProfileStore) -> crate::Result<T> + Send + 'static,
) -> crate::Result<T> {
    let store = st.store.clone();
    tokio::task::spawn_blocking(move || f(&store))
        .await
        .unwrap_or_else(|e| Err(ProfileError::Invalid(format!("task: {e}"))))
}

async fn profile(State(st): State<AppState>, Path(name): Path<String>, Query(q): Query<Q>) -> Response {
    if let Some(r) = gate(&st, &q) {
        return r;
    }
    match blocking(&st, move |s| s.profile(&name)).await {
        Ok(Some(p)) => finish(Json(p).into_response()),
        Ok(None) => err(ProfileError::NotFound),
        Err(e) => err(e),
    }
}

async fn history(State(st): State<AppState>, Path(name): Path<String>, Query(q): Query<Q>) -> Response {
    if let Some(r) = gate(&st, &q) {
        return r;
    }
    let (page, per) = (q.page.unwrap_or(0), q.per_page);
    match blocking(&st, move |s| {
        s.profile(&name)?.ok_or(ProfileError::NotFound)?;
        s.history(&name, page, per)
    })
    .await
    {
        Ok(p) => finish(Json(p).into_response()),
        Err(e) => err(e),
    }
}

async fn library(State(st): State<AppState>, Path(name): Path<String>, Query(q): Query<Q>) -> Response {
    if let Some(r) = gate(&st, &q) {
        return r;
    }
    let (page, per, kind) = (q.page.unwrap_or(0), q.per_page, q.kind.clone());
    match blocking(&st, move |s| {
        s.profile(&name)?.ok_or(ProfileError::NotFound)?;
        s.library(&name, page, per, kind.as_deref())
    })
    .await
    {
        Ok(p) => finish(Json(p).into_response()),
        Err(e) => err(e),
    }
}

async fn description(
    State(st): State<AppState>,
    Path(name): Path<String>,
    Query(q): Query<Q>,
    Json(b): Json<DescriptionBody>,
) -> Response {
    if let Some(r) = gate(&st, &q) {
        return r;
    }
    match blocking(&st, move |s| s.set_description(&name, &b.text, b.secret.as_deref())).await {
        Ok(p) => finish(Json(p).into_response()),
        Err(e) => err(e),
    }
}

async fn claim(
    State(st): State<AppState>,
    Path(name): Path<String>,
    Query(q): Query<Q>,
    Json(b): Json<ClaimBody>,
) -> Response {
    if let Some(r) = gate(&st, &q) {
        return r;
    }
    match blocking(&st, move |s| s.claim(&name, &b.secret, b.old_secret.as_deref())).await {
        Ok(()) => finish(StatusCode::NO_CONTENT.into_response()),
        Err(e) => err(e),
    }
}

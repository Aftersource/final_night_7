//! The media routes, merged into the game server's router by `fn7-server`:
//!
//! - `GET /media/index?since=<cursor>`: the manifest (see [`Media::manifest`]).
//! - `GET /media/<hash>.<ext>`: a cached image (`png`, `svg`, `webp`, `jpeg`), immutable.
//!
//! Both answer with `Access-Control-Allow-Origin: *` so a browser client on another origin can use them.

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use crate::cache::{content_type, valid_key, EXTS};
use crate::Media;

pub fn router(media: Media) -> Router {
    Router::new()
        .route("/media/index", get(index))
        .route("/media/{file}", get(file))
        .with_state(media)
}

#[derive(Deserialize)]
struct IndexQuery {
    since: Option<i64>,
}

fn cors(mut r: Response) -> Response {
    r.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    r
}

async fn index(State(media): State<Media>, Query(q): Query<IndexQuery>) -> Response {
    let mut r = Json(media.manifest(q.since.unwrap_or(0))).into_response();
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    cors(r)
}

async fn file(State(media): State<Media>, Path(file): Path<String>) -> Response {
    let not_found = || cors((StatusCode::NOT_FOUND, "no such image").into_response());
    let Some((key, ext)) = file.split_once('.') else {
        return not_found();
    };
    if !valid_key(key) || !EXTS.contains(&ext) {
        return not_found();
    }
    match tokio::fs::read(media.cache().image_path(key, ext)).await {
        Ok(bytes) => cors(
            (
                [
                    (header::CONTENT_TYPE, content_type(ext)),
                    (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
                ],
                bytes,
            )
                .into_response(),
        ),
        Err(_) => not_found(),
    }
}

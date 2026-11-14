//! `fn7-server`: the Theatre server with Final Night 7 compiled in. Same configuration as `theatre` (THEATRE_*, LLM_*
//! env vars; THEATRE_GAME is ignored). Claude via `ANTHROPIC_API_KEY` (model `LLM_DEFAULT_MODEL`, default
//! `claude-haiku-5-5`, effort `ANTHROPIC_EFFORT`, default low) when `LLM_BASE_URL` is unset; otherwise the core's
//! OpenAI-compatible gateway, or no LLM at all (every mind uses its rules fallback); `THEATRE_REPLAY_FROM` answers every
//! call from a recording instead. Recording is the core's (theatre P1): `THEATRE_RECORD=off|events|full` (use `full`)
//! writes `<THEATRE_RECORD_DIR>/<match_id>.jsonl`; the admin API (`THEATRE_ADMIN_TOKEN`) reads and forks those.
//! Images: `FN7_IMAGES` (off by default) and the `/media` routes, see `docs/MEDIA.md`.
//! Profiles (`fn7-profile`, docs/DEMIGODS_AND_ITEMS.md §7–§9): the store at `FN7_PROFILE_DB`, else
//! `<THEATRE_DATA_DIR>/fn7/profiles.sqlite`; the `/api/profiles` routes (`FN7_PROFILE_KEY` gates them); the
//! `game.profile`/`game.history`/`game.library` queries; history rows at every match end. At startup an empty store
//! (or `FN7_PROFILE_BACKFILL=1`) is backfilled in the background from `FN7_PROFILE_BACKFILL_DIR`, else the recordings
//! directory (`THEATRE_RECORD_DIR`); recordings are only read.

use std::sync::Arc;

use clap::Parser;
use fn7_plugin::FinalNight7Plugin;
use theatre_plugin_api::GamePlugin;
use theatre_server::{restore, AppState, Config};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    let cfg = Config::parse();
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    if cfg.log_format == "json" {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }

    let built = fn7_server::llm::from_config(&cfg)?;
    tracing::info!(llm = %built.label, "LLM");

    // Images (fn7-media): off unless FN7_IMAGES=mock|openai; see docs/MEDIA.md. The /media routes are always served.
    // The style config (FN7_MEDIA_STYLE / data/media_style.json) is validated here: a bad one stops the server.
    let media = fn7_media::Media::from_env()?;
    let style = media.style();
    tracing::info!(images = %media.config().note, dir = %media.config().dir.display(), style = %style.source,
                   style_version = %style.version, style_hash = %&style.hash[..12], "media");
    // Prewarm (FN7_MEDIA_PREWARM, default on; FN7_MEDIA_PREWARM_NAMES adds names): every built-in AI demigod's
    // portrait and diorama, identity-only, so they are cached before any match needs them.
    if media.enabled() {
        let names = fn7_server::media::prewarm_names(|k| std::env::var(k).ok());
        if !names.is_empty() {
            let w = fn7_server::media::prewarm(&media, &names);
            tracing::info!(names = names.len(), queued = w.queued, cached = w.cached, refused = w.refused,
                           "media prewarm: {} queued, {} already cached", w.queued, w.cached);
        }
    }
    // Profiles: open (and migrate) the store before the plugin, so its queries are in the core's manifest.
    let env = |k: &str| std::env::var(k).ok();
    let (store, db) = fn7_server::profile::open(&cfg.data_dir, env)?;
    let profiles = fn7_server::profile::Profiles::new(store.clone());
    tracing::info!(db = %db.display(), matches = store.match_count().unwrap_or(0), "profiles");
    // The core records every match (THEATRE_RECORD / THEATRE_RECORD_DIR) and runs every hook in the match's trace
    // context, so the plugin's events, LLM calls and the images' media_* events land in its run.
    let plugin: Arc<dyn GamePlugin> =
        Arc::new(fn7_server::media::MediaPlugin::new(
            FinalNight7Plugin::new().with_media_style(media.style().ruleset_json()),
            media.clone(),
        ).with_profiles(profiles.clone()));
    let app = AppState::new(cfg.clone(), plugin, built.llm);
    restore(&app);
    // The match-end rows read the core's recording of the match (flushed first) for the room code, seats and falls.
    let record_dir = app.recorder.as_ref().map(|r| r.dir.clone()).or_else(|| cfg.record_dir.clone());
    if let Some(r) = &app.recorder {
        profiles.attach_recordings(r.dir.clone(), Some(r.sink.clone()));
    }
    // Backfill (empty store or FN7_PROFILE_BACKFILL=1): streaming, off the runtime, never blocks startup.
    let backfill_dir = env("FN7_PROFILE_BACKFILL_DIR").filter(|d| !d.trim().is_empty()).map(Into::into).or(record_dir);
    if let Some(dir) = backfill_dir.filter(|_| fn7_server::profile::should_backfill(&store, env)) {
        let store = store.clone();
        tokio::task::spawn_blocking(move || match fn7_server::profile::backfill(&store, &dir) {
            Ok(r) => tracing::info!(dir = %dir.display(), files = r.files, ended = r.ended, unfinished = r.unfinished,
                                    known = r.known, other = r.other, errors = r.errors, mb = r.bytes / 1_000_000,
                                    ms = r.millis, "profiles: backfill done"),
            Err(e) => tracing::warn!(dir = %dir.display(), error = %e, "profiles: backfill failed"),
        });
    }
    // The simulator API (/sim/*, admin token; see simapi.rs): its matches run on their own threads, never on rooms'.
    let sim = fn7_server::simapi::SimConfig::from_env(
        &cfg,
        app.recorder.as_ref().map(|r| r.dir.clone()),
        fn7_server::simapi::claude_factory(&cfg, &built.label),
    );

    let listener = TcpListener::bind(cfg.bind).await?;
    tracing::info!(addr = %listener.local_addr()?, game = fn7_plugin::PLUGIN_ID, rules = fn7_plugin::RULES_VERSION,
                   "final night 7 listening (ws at /ws, images at /media, simulator at /sim, profiles at /api/profiles)");
    // The engine's own serve, with the /media, /sim and /api/profiles routes merged in (engine route extension,
    // theatre 330896e).
    let extra = fn7_media::http::router(media)
        .merge(fn7_server::simapi::router(sim))
        .merge(fn7_profile::http::router(store, fn7_profile::http::HttpConfig::from_env(env)));
    theatre_server::serve_with(listener, app, extra, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
}

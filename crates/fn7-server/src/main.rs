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
//! Assets (`fn7-assets`, docs/ASSETS.md): the registry at `FN7_ASSETS_DIR`, else `<THEATRE_DATA_DIR>/fn7/assets`
//! (`assets.sqlite` + content-addressed files), written live from the media layer; the `/api/assets` routes
//! (`FN7_ASSETS_KEY`, else `FN7_PROFILE_KEY`, gates them); `FN7_ASSETS=off` turns it off.
//! Accounts (`fn7_server::accounts`, docs/ACCOUNTS.md): `/api/auth/*`, `/api/me*`, `/api/game/bind`, in the same store.
//! Google sign-in needs `FN7_GOOGLE_CLIENT_ID`, `FN7_GOOGLE_CLIENT_SECRET` and `FN7_PUBLIC_URL` (guests work without);
//! `FN7_NAME_GUARD=off` stops the name guard; `FN7_MODERATION=rules` judges demigod sheets by rules only.

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
    // The asset registry (docs/ASSETS.md): attached before the prewarm, so every image the media layer touches from
    // here on is written to SQL as it happens.
    // A registry that cannot be opened never stops the game: the server runs without it and says so.
    let assets = fn7_server::assets::open(&cfg.data_dir, |k| std::env::var(k).ok()).unwrap_or_else(|e| {
        tracing::error!(error = %e, "assets: the registry could not be opened; running without it (no /api/assets, no asset rows)");
        None
    });
    let asset_hook = assets.as_ref().map(|(reg, root)| {
        tracing::info!(dir = %root.display(), schema = reg.schema_version().unwrap_or(0), "assets");
        fn7_server::assets::attach(reg.clone(), &media)
    });
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
    // Exact resume (docs/TIME_TRAVEL.md): the plugin journals every accepted instant action under
    // `FN7_JOURNAL_DIR` (default `<THEATRE_DATA_DIR>/fn7/journal`) so a restart loses none of them; `FN7_JOURNAL=off`
    // keeps it in memory only.
    let game = FinalNight7Plugin::new().with_media_style(media.style().ruleset_json());
    // The duel picker (docs/DUEL_MOVES.md §4.1): one structured call per typed human duel line, in rooms with
    // `duel_picker: on`, when there is a live LLM (never a replayed one); `FN7_DUEL_PICKER=off` turns it off here.
    // The duel move list (docs/DUEL_MOVES.md §1): FN7_DUEL_MOVES, else data/duel_moves.json, else the compiled one.
    // Validated like the media style: a bad file stops the server.
    let list = fn7_plugin::moves::install(fn7_plugin::moves::load_lookup(env).map_err(anyhow::Error::msg)?);
    tracing::info!(source = %list.source, version = %list.version, compiled = list.is_compiled(), "duel moves");
    // The duel data store (docs/DUEL_MOVES.md §2): one row per typed line, the pick cache, /api/duel/lines.
    let duel_lines = fn7_server::duel_lines::open(&cfg.data_dir, env).unwrap_or_else(|e| {
        tracing::error!(error = %e, "duel lines: the store could not be opened; running without it");
        None
    });
    if let Some(d) = &duel_lines {
        tracing::info!(db = %d.path.display(), lines = d.count(), "duel lines");
    }
    let data = duel_lines.clone().map(|d| d as Arc<dyn fn7_plugin::picker::DuelData>);
    let picker_off = env("FN7_DUEL_PICKER").is_some_and(|v| matches!(v.trim(), "off" | "0" | "false"));
    let game = if built.label != "none" && cfg.replay_from.is_none() && !picker_off {
        tracing::info!(model = fn7_agents::duel_pick::MODEL, "duel picker: on");
        game.with_picker(built.llm.clone(), data)
    } else {
        tracing::info!("duel picker: off");
        match data {
            Some(d) => game.with_duel_data(d),
            None => game,
        }
    };
    let journal_off = env("FN7_JOURNAL").is_some_and(|v| matches!(v.trim(), "off" | "0" | "false"));
    let game = if journal_off {
        tracing::info!("journal: off (memory only)");
        game
    } else {
        let dir = env("FN7_JOURNAL_DIR").filter(|d| !d.trim().is_empty()).map(std::path::PathBuf::from)
            .unwrap_or_else(|| cfg.data_dir.join("fn7").join("journal"));
        tracing::info!(dir = %dir.display(), "journal");
        game.with_journal(dir)
    };
    // Accounts (docs/ACCOUNTS.md): sessions, settings, demigod personalities (moderated by the LLM when there is one,
    // never by a replayed one), Google sign-in when FN7_GOOGLE_* and FN7_PUBLIC_URL are set.
    let moderator = (built.label != "none" && cfg.replay_from.is_none()).then(|| built.llm.clone());
    let accounts = fn7_server::accounts::Accounts::from_env(store.clone(), moderator, env);
    tracing::info!(google = accounts.google_configured(), moderation = accounts.moderation_label(),
                   name_guard = accounts.guard_on(), "accounts");
    let with_media = fn7_server::media::MediaPlugin::new(
        fn7_server::accounts::plugin::AccountsPlugin::new(
            game,
            accounts.clone(),
        ),
        media.clone(),
    ).with_profiles(profiles.clone());
    // The registry's wrapper is outermost: it only reads what goes out (which images, at which beat, on whom).
    let plugin: Arc<dyn GamePlugin> = match asset_hook {
        Some(hook) => Arc::new(fn7_server::assets::AssetsPlugin::new(with_media, hook)),
        None => Arc::new(with_media),
    };
    let app = AppState::new(cfg.clone(), plugin, built.llm);
    restore(&app);
    // The accounts layer sees the engine's players (to bind a session to one) and runs the name guard.
    accounts.attach(Arc::new(fn7_server::accounts::EngineDirectory(app.clone())));
    accounts.spawn_guard();
    // The always-on house room (THEATRE_HOUSE_ROOM=true): the one game every client joins. No-op when unset.
    theatre_server::start_house(&app)?;
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
                   "final night 7 listening (ws at /ws, images at /media, simulator at /sim, profiles at /api/profiles, assets at /api/assets, accounts at /api/auth and /api/me)");
    // The engine's own serve, with the /media, /sim and /api/profiles routes merged in (engine route extension,
    // theatre 330896e).
    let mut extra = fn7_media::http::router(media)
        .merge(fn7_server::simapi::router(sim))
        .merge(fn7_profile::http::router(store.clone(), fn7_profile::http::HttpConfig::from_env(env)))
        .merge(fn7_server::accounts::http::router(accounts));
    if let Some(d) = duel_lines {
        extra = extra.merge(fn7_server::duel_lines::router(d, fn7_server::duel_lines::key_from(env, cfg.admin_token.clone())));
    }
    if let Some((reg, _)) = assets {
        let mut http = fn7_assets::http::HttpConfig::from_env(env);
        http.admin_token = cfg.admin_token.clone().filter(|t| !t.is_empty());
        http.sessions = Some(Arc::new(fn7_server::assets::AccountSessions(store)));
        extra = extra.merge(fn7_assets::http::router(reg, http));
    }
    theatre_server::serve_with(listener, app, extra, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
}

//! `fn7-server`: the Theatre server with Final Night 7 compiled in. Same configuration as `theatre` (THEATRE_*, LLM_*
//! env vars; THEATRE_GAME is ignored). Claude via `ANTHROPIC_API_KEY` (model `LLM_DEFAULT_MODEL`, default
//! `claude-haiku-5-5`, effort `ANTHROPIC_EFFORT`, default low) when `LLM_BASE_URL` is unset; otherwise the core's
//! OpenAI-compatible gateway, or no LLM at all (every mind uses its rules fallback). Every match is recorded to
//! `THEATRE_RECORD` (default `runs/fn7.sqlite`; `off` disables). Images: `FN7_IMAGES` (off by default) and the `/media`
//! routes, see `docs/MEDIA.md`.

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

    let record = std::env::var("THEATRE_RECORD").unwrap_or_else(|_| "runs/fn7.sqlite".into());
    // Images (fn7-media): off unless FN7_IMAGES=mock|openai; see docs/MEDIA.md. The /media routes are always served.
    let media = fn7_media::Media::from_env()?;
    tracing::info!(images = %media.config().note, dir = %media.config().dir.display(), "media");
    let game = || fn7_server::media::MediaPlugin::new(FinalNight7Plugin::new(), media.clone());
    let plugin: Arc<dyn GamePlugin> = if record == "off" {
        Arc::new(game())
    } else {
        if let Some(dir) = std::path::Path::new(&record)
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
        {
            std::fs::create_dir_all(dir)?;
        }
        let sink = Arc::new(theatre_recorder::SqliteSink::open(&record)?);
        tracing::info!(path = %record, "recording matches");
        Arc::new(theatre_recorder::RecordingPlugin::new(game(), sink))
    };
    let app = AppState::new(cfg.clone(), plugin, built.llm);
    restore(&app);

    let listener = TcpListener::bind(cfg.bind).await?;
    tracing::info!(addr = %listener.local_addr()?, game = fn7_plugin::PLUGIN_ID, rules = fn7_plugin::RULES_VERSION,
                   "final night 7 listening (ws at /ws, images at /media)");
    fn7_server::media::serve(listener, app, media, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
}

//! Images in the server (`fn7-media`): the `/media` routes merged into the Theatre's router, and [`MediaPlugin`], a
//! `GamePlugin` wrapper that annotates outgoing payloads (an `image` object next to every `image_prompt`,
//! `statue_image` next to `statue_prompt`; see `fn7_media::annotate`) and queues their generation.
//!
//! The wrapper sits inside the recorder (`RecordingPlugin(MediaPlugin(FinalNight7Plugin))`), so the recorded hook
//! outputs are what clients got, and the `media_*` events land in the match's run. It never touches the plugin state:
//! replays and snapshots are unchanged. With images off (the default) it passes everything through untouched.

use std::future::Future;
use std::sync::Arc;

use async_trait::async_trait;
use fn7_media::Media;
use serde_json::Value;
use theatre_plugin_api::*;
use theatre_server::AppState;
use tokio::net::TcpListener;

pub struct MediaPlugin<P> {
    inner: P,
    media: Media,
}

impl<P: GamePlugin> MediaPlugin<P> {
    pub fn new(inner: P, media: Media) -> Self {
        Self { inner, media }
    }

    fn run() -> Option<String> {
        theatre_trace::current().map(|c| c.run)
    }

    fn mark(&self, v: &mut Value) {
        let run = Self::run();
        self.media.annotate(v, run.as_deref());
    }

    fn mark_seats(&self, public: &mut Value, per_seat: &mut [(Seat, Value)]) {
        self.mark(public);
        for (_, v) in per_seat.iter_mut() {
            self.mark(v);
        }
    }
}

#[async_trait]
impl<P: GamePlugin> GamePlugin for MediaPlugin<P> {
    fn info(&self) -> PluginInfo {
        self.inner.info()
    }
    fn validate_settings(&self, settings: &Value) -> Result<Value, String> {
        self.inner.validate_settings(settings)
    }
    async fn on_match_start(
        &self,
        seats: &[SeatInfo],
        settings: &Value,
        llm: &dyn LlmProvider,
    ) -> anyhow::Result<Value> {
        self.inner.on_match_start(seats, settings, llm).await
    }
    async fn on_round_start(
        &self,
        state: &Value,
        round: u32,
        llm: &dyn LlmProvider,
    ) -> anyhow::Result<RoundStart> {
        let mut r = self.inner.on_round_start(state, round, llm).await?;
        self.mark_seats(&mut r.public, &mut r.per_seat);
        Ok(r)
    }
    async fn brief(
        &self,
        state: &Value,
        round: u32,
        llm: &dyn LlmProvider,
    ) -> anyhow::Result<Brief> {
        let mut b = self.inner.brief(state, round, llm).await?;
        self.mark_seats(&mut b.public, &mut b.per_seat);
        Ok(b)
    }
    fn validate_action(
        &self,
        state: &Value,
        seat: Seat,
        text: &str,
        fields: &Value,
    ) -> Result<(), String> {
        self.inner.validate_action(state, seat, text, fields)
    }
    fn default_action(&self, state: &Value, seat: Seat) -> Action {
        self.inner.default_action(state, seat)
    }
    async fn bot_action(
        &self,
        state: &Value,
        seat: Seat,
        llm: &dyn LlmProvider,
    ) -> anyhow::Result<Action> {
        self.inner.bot_action(state, seat, llm).await
    }
    async fn resolve_round(
        &self,
        state: &Value,
        round: u32,
        actions: &[Action],
        llm: &dyn LlmProvider,
        progress: &dyn Progress,
    ) -> anyhow::Result<RoundOutcome> {
        let mut r = self
            .inner
            .resolve_round(state, round, actions, llm, progress)
            .await?;
        self.mark_seats(&mut r.public, &mut r.per_seat);
        Ok(r)
    }
    fn fallback_outcome(&self, state: &Value, round: u32, actions: &[Action]) -> RoundOutcome {
        let mut r = self.inner.fallback_outcome(state, round, actions);
        self.mark_seats(&mut r.public, &mut r.per_seat);
        r
    }
    fn default_choice(&self, state: &Value, seat: Seat, offered: &[Choice]) -> String {
        self.inner.default_choice(state, seat, offered)
    }
    async fn commit_round(
        &self,
        state: &Value,
        round: u32,
        picked: &[Picked],
        llm: &dyn LlmProvider,
    ) -> anyhow::Result<Committed> {
        self.inner.commit_round(state, round, picked, llm).await
    }
    fn is_match_over(&self, state: &Value, round: u32) -> bool {
        self.inner.is_match_over(state, round)
    }
    fn public_view(&self, state: &Value) -> Value {
        let mut v = self.inner.public_view(state);
        self.mark(&mut v);
        v
    }
    fn seat_view(&self, state: &Value, seat: Seat) -> Value {
        let mut v = self.inner.seat_view(state, seat);
        self.mark(&mut v);
        v
    }
    async fn summarize(&self, state: &Value, llm: &dyn LlmProvider) -> anyhow::Result<Summary> {
        let mut s = self.inner.summarize(state, llm).await?;
        self.mark(&mut s.summary);
        Ok(s)
    }
}

/// The Theatre's router (`/ws`, `/healthz`, `/stats`) plus the media routes (`/media/index`, `/media/<hash>.<ext>`).
pub fn router(app: Arc<AppState>, media: Media) -> axum::Router {
    theatre_server::gateway::router(app).merge(fn7_media::http::router(media))
}

/// `theatre_server::serve` with the media routes: serve until `signal` resolves, then shut down cleanly.
pub async fn serve(
    listener: TcpListener,
    app: Arc<AppState>,
    media: Media,
    signal: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let sweeper = theatre_server::spawn_sweeper(app.clone());
    axum::serve(listener, router(app.clone(), media))
        .with_graceful_shutdown(signal)
        .await?;
    tracing::info!("shutting down");
    sweeper.abort();
    theatre_server::shutdown(&app).await;
    Ok(())
}

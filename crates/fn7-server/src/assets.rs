//! The asset registry in the server (`fn7-assets`, docs/ASSETS.md).
//!
//! - [`open`]: the registry at `FN7_ASSETS_DIR`, else `<THEATRE_DATA_DIR>/fn7/assets` (created and migrated).
//! - [`attach`]: hangs the registry's live writer on the media layer's observer seam, so every image the media layer
//!   requests, serves from its cache, refuses, finishes or fails is written to SQL at that moment.
//! - [`AssetsPlugin`]: the outermost `GamePlugin` wrapper. It changes nothing: after each hook it reads the payload
//!   that is going out (already annotated by [`crate::media::MediaPlugin`]) and reports which images it shows, at
//!   which beat of which match, on which demigod or item (`uses` rows); and it tells the observer who is asking (the
//!   seat and its account) while a hook runs. State, payloads and recordings are untouched, so replays are too.
//! - [`AccountSessions`]: resolves a session token for `GET /api/assets/mine`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use fn7_assets::http::{Me, Sessions};
use fn7_assets::live::{Asker, LiveHook};
use fn7_assets::Registry;
use fn7_media::Media;
use fn7_profile::ProfileStore;
use serde_json::Value;
use theatre_plugin_api::*;

/// Open (create, migrate) the registry. `FN7_ASSETS=off` turns the registry off (`None`).
pub fn open(data_dir: &Path, env: impl Fn(&str) -> Option<String>) -> anyhow::Result<Option<(Arc<Registry>, PathBuf)>> {
    if env("FN7_ASSETS").is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "off" | "0" | "false")) {
        return Ok(None);
    }
    let root = fn7_assets::default_root(data_dir, env);
    let reg = Registry::open(&root).map_err(|e| anyhow::anyhow!("asset registry {}: {e}", root.display()))?;
    Ok(Some((Arc::new(reg), root)))
}

/// Start the live writer and make it the media layer's observer.
pub fn attach(registry: Arc<Registry>, media: &Media) -> Arc<LiveHook> {
    let hook = LiveHook::for_media(registry, media);
    media.set_observer(hook.clone());
    hook
}

/// Session tokens → the account and the names it holds (its display name, its profiles, its two demigods and their
/// hands' names as the plugin builds them).
pub struct AccountSessions(pub Arc<ProfileStore>);

impl Sessions for AccountSessions {
    fn me(&self, token: &str) -> Option<Me> {
        let (acc, _) = self.0.session_account(token).ok()??;
        let mut names = vec![acc.display_name.clone(), acc.profile.clone()];
        names.extend(acc.past_profiles.iter().cloned());
        names.extend(fn7_plugin::payload::hand_names(&acc.display_name));
        for hand in ["left", "right"] {
            if let Ok(Some(d)) = self.0.demigod(&acc.id, hand) {
                names.extend(d.r#final.name.clone());
            }
        }
        names.retain(|n| !n.trim().is_empty());
        names.dedup();
        Some(Me { account: acc.id, names })
    }
}

pub struct AssetsPlugin<P> {
    inner: P,
    hook: Arc<LiveHook>,
}

impl<P: GamePlugin> AssetsPlugin<P> {
    pub fn new(inner: P, hook: Arc<LiveHook>) -> Self {
        Self { inner, hook }
    }

    fn run() -> Option<String> {
        theatre_trace::current().map(|c| c.run)
    }

    fn beat_of(state: &Value) -> u32 {
        state["meta"]["beat"].as_u64().unwrap_or(0) as u32
    }

    fn asker(state: &Value, seat: Option<Seat>) -> Asker {
        let account = seat.and_then(|s| {
            state["seats"].as_array()?.iter().find(|x| x["seat"].as_u64() == Some(s as u64))?["account"].as_str().map(str::to_string)
        });
        Asker { seat, account }
    }

    fn saw(&self, beat: u32, seat: Option<Seat>, v: &Value) {
        if let Some(run) = Self::run() {
            self.hook.saw(&run, beat, seat, v);
        }
    }

    fn saw_seats(&self, beat: u32, public: &Value, per_seat: &[(Seat, Value)]) {
        self.saw(beat, None, public);
        for (s, v) in per_seat {
            self.saw(beat, Some(*s), v);
        }
    }
}

#[async_trait]
impl<P: GamePlugin> GamePlugin for AssetsPlugin<P> {
    fn info(&self) -> PluginInfo {
        self.inner.info()
    }
    fn validate_settings(&self, settings: &Value) -> Result<Value, String> {
        self.inner.validate_settings(settings)
    }
    fn queries(&self) -> Vec<QuerySpec> {
        self.inner.queries()
    }
    fn query(&self, state: &Value, seat: Option<Seat>, name: &str, args: &Value) -> Result<Value, String> {
        let v = LiveHook::asking_sync(Self::asker(state, seat), || self.inner.query(state, seat, name, args))?;
        self.saw(Self::beat_of(state), seat, &v);
        Ok(v)
    }
    async fn on_match_start(&self, seats: &[SeatInfo], settings: &Value, llm: &dyn LlmProvider) -> anyhow::Result<Value> {
        self.inner.on_match_start(seats, settings, llm).await
    }
    async fn on_round_start(&self, state: &Value, round: u32, llm: &dyn LlmProvider) -> anyhow::Result<RoundStart> {
        let r = LiveHook::asking(Asker::default(), self.inner.on_round_start(state, round, llm)).await?;
        self.saw_seats(round, &r.public, &r.per_seat);
        Ok(r)
    }
    async fn brief(&self, state: &Value, round: u32, llm: &dyn LlmProvider) -> anyhow::Result<Brief> {
        let b = LiveHook::asking(Asker::default(), self.inner.brief(state, round, llm)).await?;
        self.saw_seats(round, &b.public, &b.per_seat);
        Ok(b)
    }
    fn validate_action(&self, state: &Value, seat: Seat, text: &str, fields: &Value, kind: SubmitKind) -> Result<(), String> {
        self.inner.validate_action(state, seat, text, fields, kind)
    }
    fn on_seat_takeover(&self, state: &Value, takeover: &SeatTakeover) -> Option<Value> {
        self.inner.on_seat_takeover(state, takeover)
    }
    fn default_action(&self, state: &Value, seat: Seat) -> Action {
        self.inner.default_action(state, seat)
    }
    fn on_action_begin(&self, state: &Value, round: u32, seat: Seat, window: Option<&WindowInfo>, next_window_id: u64) -> PhaseControl {
        let c = LiveHook::asking_sync(Self::asker(state, Some(seat)), || self.inner.on_action_begin(state, round, seat, window, next_window_id));
        if let Some(p) = &c.public {
            self.saw(round, None, p);
        }
        c
    }
    fn on_instant(&self, state: &Value, seat: Seat, action: &Action) -> Option<Value> {
        let v = LiveHook::asking_sync(Self::asker(state, Some(seat)), || self.inner.on_instant(state, seat, action));
        if let Some(p) = &v {
            self.saw(Self::beat_of(state), None, p);
        }
        v
    }
    async fn bot_action(&self, state: &Value, seat: Seat, llm: &dyn LlmProvider) -> anyhow::Result<Action> {
        self.inner.bot_action(state, seat, llm).await
    }
    async fn resolve_round(
        &self,
        state: &Value,
        round: u32,
        actions: &[Action],
        ctx: &RoundContext,
        llm: &dyn LlmProvider,
        progress: &dyn Progress,
    ) -> anyhow::Result<RoundOutcome> {
        let r = LiveHook::asking(Asker::default(), self.inner.resolve_round(state, round, actions, ctx, llm, progress)).await?;
        self.saw_seats(round, &r.public, &r.per_seat);
        Ok(r)
    }
    fn fallback_outcome(&self, state: &Value, round: u32, actions: &[Action], ctx: &RoundContext) -> RoundOutcome {
        let r = self.inner.fallback_outcome(state, round, actions, ctx);
        self.saw_seats(round, &r.public, &r.per_seat);
        r
    }
    fn default_choice(&self, state: &Value, seat: Seat, offered: &[Choice]) -> String {
        self.inner.default_choice(state, seat, offered)
    }
    async fn commit_round(&self, state: &Value, round: u32, picked: &[Picked], llm: &dyn LlmProvider) -> anyhow::Result<Committed> {
        self.inner.commit_round(state, round, picked, llm).await
    }
    fn is_match_over(&self, state: &Value, round: u32) -> bool {
        self.inner.is_match_over(state, round)
    }
    fn public_view(&self, state: &Value) -> Value {
        let v = self.inner.public_view(state);
        self.saw(Self::beat_of(state), None, &v);
        v
    }
    fn seat_view(&self, state: &Value, seat: Seat) -> Value {
        let v = LiveHook::asking_sync(Self::asker(state, Some(seat)), || self.inner.seat_view(state, seat));
        self.saw(Self::beat_of(state), Some(seat), &v);
        v
    }
    async fn summarize(&self, state: &Value, llm: &dyn LlmProvider) -> anyhow::Result<Summary> {
        let s = LiveHook::asking(Asker::default(), self.inner.summarize(state, llm)).await?;
        self.saw(Self::beat_of(state), None, &s.summary);
        Ok(s)
    }
}

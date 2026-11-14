//! Images in the server (`fn7-media`): [`MediaPlugin`], a
//! `GamePlugin` wrapper that annotates outgoing payloads (an `image` object next to every `image_prompt`,
//! `statue_image` next to `statue_prompt`; see `fn7_media::annotate`) and queues their generation.
//!
//! The core records the hooks of the plugin it is given (`MediaPlugin(FinalNight7Plugin)`), so the recorded hook
//! outputs are what clients got, and the `media_*` events land in the match's run (the core's trace context). It never touches the plugin state:
//! replays and snapshots are unchanged. With images off (the default) it passes everything through untouched.
//! The `/media` routes are served through the engine's route extension (`theatre_server::serve_with`, see main.rs).
//!
//! **Images by reference** (`game.media {kind, ref}`, WebSocket and HTTP): the plugin checks visibility and writes the
//! subject from the state; this wrapper turns it into a styled prompt, applies the per-seat rate limit and the
//! budgets, and answers `{kind, ref, name, image, reason?, visual?}` at once (enqueue only; the picture arrives via the
//! manifest). A rate-limited request is a query error; a refused one (budget, images off) is an image with status
//! `off` and its `reason`.
//!
//! **Profiles** (`with_profiles`, see [`crate::profile`]): the wrapper also answers `game.profile`, `game.history` and
//! `game.library` from the persistent store (the plugin never sees them), writes the match's history rows in
//! `summarize` (after annotation, so image hashes are present), and adds a player's demigod portraits and dioramas
//! asked for through `game.media` to their library once ready.
//!
//! **Gear looks** (docs/MEDIA.md "Looks"): the look pass (`fn7_media::Media::annotate_seen`) turns each demigod's
//! `look` and each `look.changed` event into images (the gear image when ready, else the previous look's, with
//! `pending`). This wrapper adds two things it alone can: every gear portrait and diorama of a human player's demigod
//! goes into that player's library once ready, with its match, beat and loadout; and a `look.changed` event that
//! went out while its images were still rendering is sent again, with `ready: true`, in the `looks[]` of the next
//! public payload of that match after they exist.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fn7_media::{ImageKind, LookSeen, Media, Status};
use fn7_plugin::payload::{diorama_prompt_for, hand_names, portrait_prompt_for};
use fn7_profile::NewImage;
use serde_json::Value;
use theatre_plugin_api::*;

use crate::profile::{self, Profiles};

pub struct MediaPlugin<P> {
    inner: P,
    media: Media,
    profiles: Option<Arc<Profiles>>,
    /// Per match (run id): the `look.changed` events sent while their gear images were still rendering.
    waiting: Mutex<HashMap<String, Vec<Waiting>>>,
}

/// A `look.changed` event waiting for its images.
struct Waiting {
    /// The event as the plugin sent it.
    event: Value,
    /// The gear images it waits for.
    hashes: Vec<String>,
}

/// The most events one match keeps waiting (a cap on memory; older ones are dropped).
const WAITING_MAX: usize = 64;

impl<P: GamePlugin> MediaPlugin<P> {
    pub fn new(inner: P, media: Media) -> Self {
        Self { inner, media, profiles: None, waiting: Mutex::new(HashMap::new()) }
    }

    /// Answer the profile queries, record history at match end and register library images (see the module doc).
    pub fn with_profiles(mut self, profiles: Arc<Profiles>) -> Self {
        self.profiles = Some(profiles);
        self
    }

    fn run() -> Option<String> {
        theatre_trace::current().map(|c| c.run)
    }

    fn mark(&self, v: &mut Value) {
        let run = Self::run();
        let (_, looks) = self.media.annotate_seen(v, run.as_deref());
        for l in looks {
            self.saw_look(l, run.as_deref());
        }
    }

    /// A look went out: its gear images go to the player's library once ready, and a `look.changed` event whose
    /// images are still rendering is kept to be sent again when they are.
    fn saw_look(&self, l: LookSeen, run: Option<&str>) {
        let imgs: Vec<(&'static str, &fn7_media::ImageRef)> =
            [("portrait", l.portrait.as_ref()), ("diorama", l.diorama.as_ref())].into_iter().filter_map(|(k, i)| i.map(|i| (k, i))).collect();
        if imgs.is_empty() {
            return;
        }
        if let (Some(profiles), Some(player)) = (&self.profiles, l.player.as_ref().filter(|p| !p.trim().is_empty())) {
            let name = l.name.clone().unwrap_or_default();
            for (kind, img) in &imgs {
                if img.status == "off" {
                    continue;
                }
                let with = if l.loadout.is_empty() { "gear".to_string() } else { l.loadout.join(", ") };
                profiles.register_when_ready(&self.media, player.clone(), NewImage {
                    hash: img.hash.clone(),
                    kind: kind.to_string(),
                    caption: format!("{name}, {kind} with {with}"),
                    subject: l.demigod.clone(),
                    subject_name: l.name.clone(),
                    match_id: run.map(str::to_string),
                    at: None,
                    beat: l.since_beat.map(|b| b as u32),
                    detail: Some(serde_json::json!({ "loadout_hash": l.loadout_hash, "loadout": l.loadout })),
                });
            }
        }
        let (Some(event), Some(run)) = (l.event, run) else { return };
        let hashes: Vec<String> = imgs.iter().filter(|(_, i)| i.status == "pending").map(|(_, i)| i.hash.clone()).collect();
        if hashes.is_empty() || event.get("ready").is_some() {
            return;
        }
        let mut w = self.waiting.lock().unwrap_or_else(|e| e.into_inner());
        let list = w.entry(run.to_string()).or_default();
        if !list.iter().any(|x| x.event["memory"] == event["memory"] && x.event["demigod"] == event["demigod"]) {
            list.push(Waiting { event, hashes });
            if list.len() > WAITING_MAX {
                list.remove(0);
            }
        }
    }

    /// The `look.changed` events of this match whose images now exist (each is handed out once, with `ready: true`);
    /// ones whose images failed or were refused are dropped.
    fn ready_looks(&self, run: &str) -> Vec<Value> {
        let mut w = self.waiting.lock().unwrap_or_else(|e| e.into_inner());
        let Some(list) = w.get_mut(run) else { return vec![] };
        let status = |h: &String| self.media.entry(h).map(|e| e.status);
        let mut out = vec![];
        list.retain(|x| {
            let st: Vec<Option<Status>> = x.hashes.iter().map(status).collect();
            if st.iter().all(|s| *s == Some(Status::Ready)) {
                let mut e = x.event.clone();
                e["ready"] = true.into();
                out.push(e);
                false
            } else {
                st.iter().all(|s| matches!(s, Some(Status::Ready | Status::Pending)))
            }
        });
        if list.is_empty() {
            w.remove(run);
        }
        out
    }

    /// `game.media`: the plugin's `{kind, ref, name, image_prompt, image_kind}` → `{kind, ref, name, image, reason?}`.
    fn media_reply(&self, state: &Value, mut v: Value, seat: Option<Seat>) -> Result<Value, String> {
        let subject = v["image_prompt"].as_str().unwrap_or_default().to_string();
        let kind = v["image_kind"]
            .as_str()
            .and_then(ImageKind::parse)
            .ok_or("media: the plugin named no image kind")?;
        let prompt = self.media.prompt(kind, &subject).ok_or("nothing to draw for that ref")?;
        let who = seat.map(|s| format!("seat:{s}")).unwrap_or_else(|| "public".into());
        let run = Self::run();
        let (img, reason) = self.media.request_by_ref(&prompt, run.as_deref(), &who)?;
        self.register(state, &v, kind, &img.hash, run.as_deref());
        if let Some(o) = v.as_object_mut() {
            o.remove("image_prompt");
            o.remove("image_kind");
            o.insert("image".into(), img.to_json());
            if let Some(r) = reason {
                o.insert("reason".into(), r.into());
            }
        }
        Ok(v)
    }

    /// A portrait or diorama of a player's demigod goes into that player's library once it is ready.
    fn register(&self, state: &Value, v: &Value, kind: ImageKind, hash: &str, run: Option<&str>) {
        let Some(profiles) = &self.profiles else { return };
        if !matches!(kind, ImageKind::Portrait | ImageKind::Diorama) {
            return;
        }
        let Some(r) = v["ref"].as_str() else { return };
        let Some((player, subject)) = profile::owner_of_ref(state, r) else { return };
        let kind = if kind == ImageKind::Portrait { "portrait" } else { "diorama" };
        let img = NewImage {
            hash: hash.to_string(),
            kind: kind.into(),
            caption: format!("{subject}, {kind}"),
            subject: Some(r.to_string()),
            subject_name: Some(subject),
            match_id: run.map(str::to_string),
            at: None,
            ..Default::default()
        };
        profiles.register_when_ready(&self.media, player, img);
    }

    fn mark_seats(&self, public: &mut Value, per_seat: &mut [(Seat, Value)]) {
        // Looks announced earlier whose images have arrived ride in this payload's `looks[]`.
        if let Some(run) = Self::run() {
            let ready = self.ready_looks(&run);
            if !ready.is_empty() && public.is_object() {
                match public["looks"].as_array_mut() {
                    Some(a) => a.extend(ready),
                    None => public["looks"] = ready.into(),
                }
            }
        }
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
    fn queries(&self) -> Vec<QuerySpec> {
        let mut q = self.inner.queries();
        if self.profiles.is_some() {
            q.retain(|s| !profile::QUERIES.contains(&s.name.as_str()));
            q.extend(Profiles::queries());
        }
        q
    }
    fn query(&self, state: &Value, seat: Option<Seat>, name: &str, args: &Value) -> Result<Value, String> {
        if let Some(p) = self.profiles.as_ref().filter(|_| profile::QUERIES.contains(&name)) {
            return p.answer(state, seat, name, args);
        }
        let mut v = self.inner.query(state, seat, name, args)?;
        if name == "media" {
            return self.media_reply(state, v, seat);
        }
        self.mark(&mut v);
        Ok(v)
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
        kind: SubmitKind,
    ) -> Result<(), String> {
        self.inner.validate_action(state, seat, text, fields, kind)
    }
    /// State only (no payload to annotate): forwarded untouched.
    fn on_seat_takeover(&self, state: &Value, takeover: &SeatTakeover) -> Option<Value> {
        self.inner.on_seat_takeover(state, takeover)
    }
    fn default_action(&self, state: &Value, seat: Seat) -> Action {
        self.inner.default_action(state, seat)
    }
    fn on_action_begin(
        &self,
        state: &Value,
        round: u32,
        seat: Seat,
        window: Option<&WindowInfo>,
        next_window_id: u64,
    ) -> PhaseControl {
        let mut c = self.inner.on_action_begin(state, round, seat, window, next_window_id);
        if let Some(p) = c.public.as_mut() {
            self.mark(p);
        }
        c
    }
    fn on_instant(&self, state: &Value, seat: Seat, action: &Action) -> Option<Value> {
        let mut v = self.inner.on_instant(state, seat, action);
        if let Some(p) = v.as_mut() {
            self.mark(p);
        }
        v
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
        ctx: &RoundContext,
        llm: &dyn LlmProvider,
        progress: &dyn Progress,
    ) -> anyhow::Result<RoundOutcome> {
        let mut r = self
            .inner
            .resolve_round(state, round, actions, ctx, llm, progress)
            .await?;
        self.mark_seats(&mut r.public, &mut r.per_seat);
        Ok(r)
    }
    fn fallback_outcome(&self, state: &Value, round: u32, actions: &[Action], ctx: &RoundContext) -> RoundOutcome {
        let mut r = self.inner.fallback_outcome(state, round, actions, ctx);
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
        if let Some(p) = self.profiles.clone() {
            // History rows (profile store). Never fails the match.
            let run = Self::run();
            let seats = profile::seats_from_state(state);
            let public = self.public_view(state);
            let summary = s.summary.clone();
            let id = run.clone();
            match tokio::task::spawn_blocking(move || p.record_match_end(id, seats, summary, public)).await {
                Ok(Ok(r)) => tracing::info!(match_id = %r.match_id, players = ?r.players, images = r.images,
                                            "profiles: match recorded"),
                Ok(Err(e)) => tracing::warn!(match_id = ?run, error = %e, "profiles: could not record the match"),
                Err(e) => tracing::warn!(match_id = ?run, error = %e, "profiles: record task failed"),
            }
        }
        Ok(s)
    }
}

/// What [`prewarm`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Prewarm {
    pub queued: usize,
    pub cached: usize,
    /// Refused (budget, unknown price, full queue) or images off.
    pub refused: usize,
}

/// The names to prewarm: the built-in AI demigods (`fn7_plugin::phases::AI_NAMES`) plus `FN7_MEDIA_PREWARM_NAMES`
/// (comma-separated), deduplicated by identity (trimmed, lowercased). Empty when `FN7_MEDIA_PREWARM` is
/// `0`/`off`/`false` (default on).
pub fn prewarm_names(get: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let on = get("FN7_MEDIA_PREWARM")
        .map(|v| !matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "off" | "false" | "no"))
        .unwrap_or(true);
    if !on {
        return vec![];
    }
    let extra = get("FN7_MEDIA_PREWARM_NAMES").unwrap_or_default();
    let mut seen = std::collections::BTreeSet::new();
    fn7_plugin::phases::AI_NAMES
        .iter()
        .map(|n| n.to_string())
        .chain(extra.split(',').map(|n| n.trim().chars().take(40).collect::<String>()))
        .filter(|n| !n.is_empty() && seen.insert(fn7_plugin::visual::identity(n)))
        .collect()
}

/// Enqueue the portrait and the diorama of every name and of its two hands (`"<name>'s Left Hand"`, `"<name>'s Right
/// Hand"`: the player's two demigods), 6 images per name (identity-only prompts: the same images every match serves),
/// at the queue's top priority, outside any match (budget bucket `prewarm`; the global budget applies). Images already
/// on disk count as cached and cost nothing.
pub fn prewarm(media: &Media, names: &[String]) -> Prewarm {
    let mut out = Prewarm::default();
    if !media.enabled() {
        return out;
    }
    let all = names.iter().flat_map(|n| {
        let [l, r] = hand_names(n);
        [n.clone(), l, r]
    });
    for name in all {
        for (kind, subject) in [
            (ImageKind::Portrait, portrait_prompt_for(&name)),
            (ImageKind::Diorama, diorama_prompt_for(&name)),
        ] {
            let Some(p) = media.prompt(kind, &subject) else { continue };
            match media.request_with(&p, Some("prewarm"), false).status {
                "ready" => out.cached += 1,
                "pending" => out.queued += 1,
                _ => out.refused += 1,
            }
        }
    }
    out
}

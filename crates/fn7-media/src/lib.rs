//! # fn7-media
//!
//! Images for Final Night 7: portraits (demigods, memorized characters), items, big events and end-of-match statues.
//!
//! - [`prompt`]: house-style prompts, deterministic, engine ids scrubbed.
//! - [`generator`]: the [`ImageGenerator`] trait, [`MockGenerator`] (free SVG placeholders) and [`OpenAiImages`]
//!   (paid; built only when the host sets `FN7_IMAGES=openai` and `OPENAI_API_KEY`, and spends only within
//!   `FN7_IMAGE_BUDGET_USD`, default 0).
//! - [`cache`]: `runs/media/<key>.<ext>` + `<key>.json` sidecars.
//! - [`Media`]: the service. [`Media::request`] turns a prompt into an [`ImageRef`] at once (never waits on a
//!   generator); generation runs on a bounded background queue, and finished images show up in the manifest
//!   ([`Media::manifest`], served as `GET /media/index?since=<cursor>`).
//! - [`annotate`]: walks an outgoing payload and adds an `image` object next to every `image_prompt` (and
//!   `statue_image` next to `statue_prompt`).
//! - [`http`]: the axum routes `GET /media/index` and `GET /media/<key>.<ext>`.
//!
//! Every request, cache hit, refusal, success and failure is recorded through `theatre_trace` (`media_*` events, and an
//! HTTP record per OpenAI exchange with the image bytes redacted), so images are part of the match data.

pub mod annotate;
pub mod cache;
pub mod generator;
pub mod http;
pub mod prompt;
pub mod style;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, Semaphore};

pub use cache::{MediaCache, Sidecar};
pub use generator::{
    GenRequest, Generated, ImageGenerator, MockGenerator, OpenAiConfig, OpenAiImages,
};
pub use prompt::{ImageKind, ImagePrompt, PROMPT_VERSION};
pub use style::{EventSelection, KindStyle, MediaStyle};

// ---------------------------------------------------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// No images (the default). Payloads are not touched by the server wrapper.
    Off,
    /// Free SVG placeholders.
    Mock,
    /// The OpenAI Images API (paid, budgeted).
    OpenAi,
}

/// Media settings. Read from the environment with [`MediaConfig::from_env`] (see `docs/MEDIA.md`).
#[derive(Debug, Clone)]
pub struct MediaConfig {
    pub mode: Mode,
    /// Why the mode is what it is ("FN7_IMAGES unset", "OPENAI_API_KEY missing", …).
    pub note: String,
    /// `FN7_MEDIA_DIR`, default `runs/media`.
    pub dir: PathBuf,
    /// `FN7_MEDIA_URL_PREFIX`, default `/media` (relative to the server's HTTP origin).
    pub url_prefix: String,
    /// `FN7_IMAGE_BUDGET_USD`, default 0: the most ever spent on images in `dir` (the sum of the sidecars' costs counts
    /// what was spent before this process started).
    pub budget_usd: f64,
    /// `FN7_IMAGE_MATCH_BUDGET_USD`, default = the global budget: the most one match may spend.
    pub match_budget_usd: f64,
    /// Event images per match: `FN7_IMAGE_MAX_EVENTS`, else the style's `event_selection.max_per_match` (12).
    pub max_events_per_match: u32,
    /// `FN7_MEDIA_RATE_PER_MIN`, default 30: new images one requester (a seat, or the public HTTP asker) may ask for
    /// by reference (`game.media`) per minute in one match (a token bucket of this size). 0 = no limit. Cache hits and
    /// repeats are free.
    pub rate_per_min: u32,
    /// The style config (`FN7_MEDIA_STYLE` / `data/media_style.json` / compiled in).
    pub style: MediaStyle,
    /// `FN7_IMAGE_CONCURRENCY`, default 2: generations in flight.
    pub concurrency: usize,
    /// `FN7_IMAGE_QUEUE`, default 256: queued generations; beyond it requests are dropped (status `off`).
    pub queue: usize,
    /// OpenAI settings (only when `mode == OpenAi`).
    pub openai: Option<OpenAiConfig>,
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self {
            mode: Mode::Off,
            note: "FN7_IMAGES unset".into(),
            dir: PathBuf::from("runs/media"),
            url_prefix: "/media".into(),
            budget_usd: 0.0,
            match_budget_usd: 0.0,
            max_events_per_match: MediaStyle::builtin().event_selection.max_per_match,
            rate_per_min: 30,
            style: MediaStyle::builtin(),
            concurrency: 2,
            queue: 256,
            openai: None,
        }
    }
}

impl MediaConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    /// Settings from any key lookup (tests pass a map instead of touching the process environment). Fails only on an
    /// invalid style config (the message names the file and the field).
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let style = MediaStyle::from_lookup(&get).map_err(|e| anyhow::anyhow!("media style: {e}"))?;
        let get = |k: &str| {
            get(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let num = |k: &str, d: f64| {
            get(k)
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|x| x.is_finite() && *x >= 0.0)
                .unwrap_or(d)
        };
        let mut c = MediaConfig {
            max_events_per_match: style.event_selection.max_per_match,
            style,
            ..Default::default()
        };
        if let Some(d) = get("FN7_MEDIA_DIR") {
            c.dir = d.into();
        }
        if let Some(p) = get("FN7_MEDIA_URL_PREFIX") {
            c.url_prefix = p.trim_end_matches('/').to_string();
        }
        c.budget_usd = num("FN7_IMAGE_BUDGET_USD", 0.0);
        c.match_budget_usd = num("FN7_IMAGE_MATCH_BUDGET_USD", c.budget_usd);
        c.max_events_per_match =
            num("FN7_IMAGE_MAX_EVENTS", c.max_events_per_match as f64) as u32;
        c.rate_per_min = num("FN7_MEDIA_RATE_PER_MIN", 30.0).min(100_000.0) as u32;
        c.concurrency = (num("FN7_IMAGE_CONCURRENCY", 2.0) as usize).clamp(1, 16);
        c.queue = (num("FN7_IMAGE_QUEUE", 256.0) as usize).clamp(1, 10_000);
        match get("FN7_IMAGES").map(|m| m.to_ascii_lowercase()).as_deref() {
            None | Some("off") | Some("0") | Some("false") => {}
            Some("mock") => {
                c.mode = Mode::Mock;
                c.note = "mock placeholders (free)".into();
            }
            Some("openai") => match get("OPENAI_API_KEY") {
                None => {
                    c.note = "FN7_IMAGES=openai but OPENAI_API_KEY is missing: images off".into()
                }
                Some(api_key) => {
                    c.mode = Mode::OpenAi;
                    c.openai = Some(OpenAiConfig {
                        api_key,
                        base_url: get("FN7_IMAGE_BASE_URL")
                            .unwrap_or_else(|| "https://api.openai.com/v1".into()),
                        model: get("FN7_IMAGE_MODEL")
                            .unwrap_or_else(|| OpenAiConfig::DEFAULT_MODEL.into()),
                        size: get("FN7_IMAGE_SIZE")
                            .unwrap_or_else(|| OpenAiConfig::DEFAULT_SIZE.into()),
                        quality: get("FN7_IMAGE_QUALITY")
                            .unwrap_or_else(|| OpenAiConfig::DEFAULT_QUALITY.into()),
                        price_override: get("FN7_IMAGE_PRICE_USD").and_then(|v| v.parse().ok()),
                        timeout: Duration::from_secs_f64(
                            num("FN7_IMAGE_TIMEOUT_S", 120.0).max(5.0),
                        ),
                        retries: 2,
                    });
                    c.note = format!(
                        "openai (budget ${:.2} total, ${:.2} per match)",
                        c.budget_usd, c.match_budget_usd
                    );
                }
            },
            Some(other) => {
                c.note = format!("FN7_IMAGES={other} is not off|mock|openai: images off")
            }
        }
        Ok(c)
    }

    /// The generator these settings call for (`None` when off).
    pub fn generator(&self) -> anyhow::Result<Option<Arc<dyn ImageGenerator>>> {
        Ok(match self.mode {
            Mode::Off => None,
            Mode::Mock => Some(Arc::new(MockGenerator::new())),
            Mode::OpenAi => match &self.openai {
                Some(o) => Some(Arc::new(OpenAiImages::new(o.clone())?)),
                None => None,
            },
        })
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The service
// ---------------------------------------------------------------------------------------------------------------------

/// Where one image stands (manifest form).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pending,
    Ready,
    /// The generator failed (see `reason`).
    Failed,
    /// Over budget, over the event cap, unknown price or a full queue (see `reason`).
    Refused,
    /// Images are off.
    Off,
}

impl Status {
    /// The three statuses a payload shows: `pending`, `ready`, or `off` (everything that won't become ready).
    pub fn payload(self) -> &'static str {
        match self {
            Status::Pending => "pending",
            Status::Ready => "ready",
            _ => "off",
        }
    }
}

/// One manifest entry.
#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub hash: String,
    pub kind: ImageKind,
    pub status: Status,
    pub url: Option<String>,
    pub ext: Option<String>,
    pub prompt: String,
    /// Monotonic ms cursor: strictly increases with every change in this process.
    pub updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub cost_usd: f64,
    #[serde(skip)]
    match_id: Option<String>,
}

/// What a payload carries for one image: `{hash, kind, status, url, prompt}`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImageRef {
    pub hash: String,
    pub kind: ImageKind,
    /// `pending` | `ready` | `off`.
    pub status: &'static str,
    pub url: Option<String>,
    pub prompt: String,
}

impl ImageRef {
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// One requester's token bucket (requests by reference).
struct Bucket {
    tokens: f64,
    at: Instant,
}

struct Job {
    key: String,
    prompt: ImagePrompt,
    price: f64,
    match_id: Option<String>,
    ctx: Option<theatre_trace::Ctx>,
}

#[derive(Default)]
struct MatchSpend {
    spent: f64,
    events: u32,
}

#[derive(Default)]
struct State {
    entries: HashMap<String, Entry>,
    /// Spent (and reserved) in `dir`, including what was spent before this process.
    spent_total: f64,
    matches: HashMap<String, MatchSpend>,
    clock: i64,
    /// (match, requester) → bucket.
    rates: HashMap<(String, String), Bucket>,
}

impl State {
    fn tick(&mut self) -> i64 {
        self.clock = theatre_trace::now_ms().max(self.clock + 1);
        self.clock
    }
}

struct Inner {
    cfg: MediaConfig,
    gen: Option<Arc<dyn ImageGenerator>>,
    label: String,
    cache: MediaCache,
    state: Mutex<State>,
    tx: Option<mpsc::Sender<Job>>,
    /// Set once any payload carries `image_prompt`: from then on the interim prompts (omens, demigod lists) stop.
    seen_image_prompt: AtomicBool,
}

/// The image service. Cheap to clone. All methods return at once; generation happens on a background queue.
#[derive(Clone)]
pub struct Media {
    inner: Arc<Inner>,
}

const MATCH_UNKNOWN: &str = "-";

impl Media {
    /// Images off: every request answers `status: "off"`; the manifest is empty.
    pub fn off() -> Self {
        Self::start(MediaConfig::default(), None)
    }

    /// The service from the environment ([`MediaConfig::from_env`]). Needs a tokio runtime when images are on.
    pub fn from_env() -> anyhow::Result<Self> {
        let cfg = MediaConfig::from_env()?;
        let gen = cfg.generator()?;
        Ok(Self::start(cfg, gen))
    }

    /// Start the service with this generator (`None` = off). With a generator, it spawns the queue worker on the
    /// current tokio runtime (without a runtime, images are off).
    pub fn start(mut cfg: MediaConfig, gen: Option<Arc<dyn ImageGenerator>>) -> Self {
        let gen = gen.filter(|_| tokio::runtime::Handle::try_current().is_ok());
        if gen.is_none() {
            cfg.mode = Mode::Off;
        }
        let label = gen
            .as_ref()
            .map(|g| g.label())
            .unwrap_or_else(|| "off".into());
        let cache = MediaCache::new(cfg.dir.clone());
        let spent_total = if gen.is_some() {
            cache.total_cost()
        } else {
            0.0
        };
        let (tx, rx) = match &gen {
            Some(_) => {
                let (tx, rx) = mpsc::channel::<Job>(cfg.queue);
                (Some(tx), Some(rx))
            }
            None => (None, None),
        };
        let inner = Arc::new(Inner {
            cfg,
            gen,
            label,
            cache,
            state: Mutex::new(State {
                spent_total,
                ..Default::default()
            }),
            tx,
            seen_image_prompt: AtomicBool::new(false),
        });
        if let Some(rx) = rx {
            tokio::spawn(worker(Arc::downgrade(&inner), rx));
        }
        Self { inner }
    }

    pub fn mode(&self) -> Mode {
        self.inner.cfg.mode
    }

    pub fn enabled(&self) -> bool {
        self.inner.gen.is_some()
    }

    pub fn config(&self) -> &MediaConfig {
        &self.inner.cfg
    }

    pub fn cache(&self) -> &MediaCache {
        &self.inner.cache
    }

    /// The style config in use.
    pub fn style(&self) -> &MediaStyle {
        &self.inner.cfg.style
    }

    /// A prompt in this service's style (`None` when nothing is left of the subject after scrubbing ids).
    pub fn prompt(&self, kind: ImageKind, subject: &str) -> Option<ImagePrompt> {
        ImagePrompt::styled(self.style(), kind, subject)
    }

    /// The image key of a prompt under the current generator: sha256 of `PROMPT_VERSION`, the generator label, the size
    /// and the prompt text, as 64 hex chars.
    pub fn key_for(&self, prompt: &ImagePrompt) -> String {
        image_key(&self.inner.label, prompt)
    }

    fn url(&self, key: &str, ext: &str) -> String {
        format!("{}/{key}.{ext}", self.inner.cfg.url_prefix)
    }

    fn image_ref(&self, e: &Entry) -> ImageRef {
        ImageRef {
            hash: e.hash.clone(),
            kind: e.kind,
            status: e.status.payload(),
            url: e.url.clone(),
            prompt: e.prompt.clone(),
        }
    }

    /// Ask for an image. Returns at once: `ready` (cached) with its URL, `pending` (queued; watch the manifest), or
    /// `off` (images off, refused or failed). Asking again for the same prompt is cheap and never generates twice.
    /// `match_id` scopes the per-match budget and event cap (the recorder's run id; `None` = shared bucket).
    pub fn request(&self, prompt: &ImagePrompt, match_id: Option<&str>) -> ImageRef {
        self.request_with(prompt, match_id, true)
    }

    /// [`Media::request`]; `capped = false` exempts an event image from the per-match event cap (storyboard shots and
    /// requests by reference: the cap is the moment-card selection's `max_per_match`). Budgets always apply.
    pub fn request_with(&self, prompt: &ImagePrompt, match_id: Option<&str>, capped: bool) -> ImageRef {
        let key = self.key_for(prompt);
        let Some(gen) = self.inner.gen.clone() else {
            return ImageRef {
                hash: key,
                kind: prompt.kind,
                status: "off",
                url: None,
                prompt: prompt.text.clone(),
            };
        };
        let m = match_id.unwrap_or(MATCH_UNKNOWN).to_string();
        let mut st = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(e) = st.entries.get(&key) {
            // Refusals and failures are final within their match; another match gets a fresh look.
            let retry = matches!(e.status, Status::Refused | Status::Failed)
                && e.match_id.as_deref() != Some(m.as_str());
            if !retry {
                return self.image_ref(e);
            }
        }
        let now = st.tick();
        // On disk already (an earlier process or match made it).
        if let Some(side) = self.inner.cache.lookup(&key) {
            let e = Entry {
                hash: key.clone(),
                kind: prompt.kind,
                status: Status::Ready,
                url: Some(self.url(&key, &side.ext)),
                ext: Some(side.ext.clone()),
                prompt: prompt.text.clone(),
                updated_at: now,
                reason: None,
                cost_usd: 0.0,
                match_id: Some(m.clone()),
            };
            let r = self.image_ref(&e);
            st.entries.insert(key.clone(), e);
            drop(st);
            theatre_trace::event(
                "media_cached",
                None,
                json!({ "hash": key, "kind": prompt.kind, "provider": gen.provider(), "path": self.inner.cache.image_path(&key, &side.ext).display().to_string() }),
            );
            return r;
        }
        // Budget, caps, queue.
        let cfg = &self.inner.cfg;
        let price = gen.price_usd(prompt.kind, &prompt.size);
        let spend = st.matches.entry(m.clone()).or_default();
        let refusal = match price {
            None => Some("unknown price for this model: set FN7_IMAGE_PRICE_USD".to_string()),
            Some(_)
                if capped
                    && prompt.kind == ImageKind::Event
                    && spend.events >= cfg.max_events_per_match =>
            {
                Some(format!(
                    "event image cap reached ({} per match)",
                    cfg.max_events_per_match
                ))
            }
            Some(p) if p > 0.0 && spend.spent + p > cfg.match_budget_usd + 1e-9 => Some(format!(
                "match image budget reached (${:.3} of ${:.2})",
                spend.spent, cfg.match_budget_usd
            )),
            Some(p) if p > 0.0 && st.spent_total + p > cfg.budget_usd + 1e-9 => Some(format!(
                "image budget reached (${:.3} of ${:.2})",
                st.spent_total, cfg.budget_usd
            )),
            Some(_) => None,
        };
        let price = price.unwrap_or(0.0);
        let mut status = Status::Pending;
        let mut reason = refusal;
        if reason.is_none() {
            let job = Job {
                key: key.clone(),
                prompt: prompt.clone(),
                price,
                match_id: match_id.map(str::to_string),
                ctx: theatre_trace::current(),
            };
            let sent = self
                .inner
                .tx
                .as_ref()
                .is_some_and(|tx| tx.try_send(job).is_ok());
            match sent {
                true => {
                    let spend = st.matches.entry(m.clone()).or_default();
                    spend.spent += price;
                    if capped && prompt.kind == ImageKind::Event {
                        spend.events += 1;
                    }
                    st.spent_total += price;
                }
                false => reason = Some("image queue full".into()),
            }
        }
        if reason.is_some() {
            status = Status::Refused;
        }
        let e = Entry {
            hash: key.clone(),
            kind: prompt.kind,
            status,
            url: None,
            ext: None,
            prompt: prompt.text.clone(),
            updated_at: now,
            reason: reason.clone(),
            cost_usd: 0.0,
            match_id: Some(m),
        };
        let r = self.image_ref(&e);
        st.entries.insert(key.clone(), e);
        drop(st);
        let (kind, data) = match reason {
            Some(why) => (
                "media_refused",
                json!({ "hash": key, "kind": prompt.kind, "provider": gen.provider(), "reason": why, "prompt": prompt.text }),
            ),
            None => (
                "media_request",
                json!({ "hash": key, "kind": prompt.kind, "provider": gen.provider(), "model": self.inner.label, "price_usd": price, "prompt": prompt.text }),
            ),
        };
        theatre_trace::event(kind, None, data);
        r
    }

    /// Ask for an image **by reference** (`game.media`): like [`Media::request`], plus the per-requester rate limit
    /// (`who`: `"seat:N"` or `"public"`, scoped to the match). Only a new image takes a token: an image this process
    /// already knows, or one on disk, answers at once for free. Returns the image and, when it is `off`, why.
    /// `Err` = rate limited (nothing was queued; ask again later).
    pub fn request_by_ref(
        &self,
        prompt: &ImagePrompt,
        match_id: Option<&str>,
        who: &str,
    ) -> Result<(ImageRef, Option<String>), String> {
        if !self.enabled() {
            return Ok((self.request(prompt, match_id), Some("images are off".into())));
        }
        let key = self.key_for(prompt);
        let known = {
            let st = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
            st.entries.contains_key(&key)
        } || self.inner.cache.lookup(&key).is_some();
        if !known {
            self.take_token(match_id, who)?;
        }
        let r = self.request_with(prompt, match_id, false);
        let reason = match r.status {
            "off" => self.entry(&r.hash).and_then(|e| e.reason),
            _ => None,
        };
        Ok((r, reason))
    }

    fn take_token(&self, match_id: Option<&str>, who: &str) -> Result<(), String> {
        let rate = self.inner.cfg.rate_per_min;
        if rate == 0 {
            return Ok(());
        }
        let cap = rate as f64;
        let now = Instant::now();
        let mut st = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        let b = st
            .rates
            .entry((match_id.unwrap_or(MATCH_UNKNOWN).to_string(), who.to_string()))
            .or_insert(Bucket { tokens: cap, at: now });
        b.tokens = (b.tokens + now.duration_since(b.at).as_secs_f64() * cap / 60.0).min(cap);
        b.at = now;
        if b.tokens < 1.0 {
            let wait = ((1.0 - b.tokens) * 60.0 / cap).ceil().max(1.0);
            drop(st);
            theatre_trace::event(
                "media_rate_limited",
                None,
                json!({ "who": who, "rate_per_min": rate, "retry_after_s": wait }),
            );
            return Err(format!(
                "rate limited: at most {rate} new images per minute per seat; try again in {wait} s"
            ));
        }
        b.tokens -= 1.0;
        Ok(())
    }

    /// The manifest: every image this process has been asked for whose entry changed after `since` (a cursor from an
    /// earlier answer; 0 = everything), oldest change first.
    pub fn manifest(&self, since: i64) -> Value {
        let st = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut items: Vec<&Entry> = st
            .entries
            .values()
            .filter(|e| e.updated_at > since)
            .collect();
        items.sort_by(|a, b| a.updated_at.cmp(&b.updated_at).then(a.hash.cmp(&b.hash)));
        let cursor = items
            .last()
            .map(|e| e.updated_at)
            .unwrap_or(since)
            .max(since);
        json!({
            "enabled": self.enabled(),
            "mode": self.inner.cfg.mode,
            "cursor": cursor,
            "items": items,
        })
    }

    /// One entry by hash.
    pub fn entry(&self, hash: &str) -> Option<Entry> {
        let st = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        st.entries.get(hash).cloned()
    }

    /// Spent so far: (everything in the cache dir, this match).
    pub fn spent(&self, match_id: Option<&str>) -> (f64, f64) {
        let st = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        let m = st
            .matches
            .get(match_id.unwrap_or(MATCH_UNKNOWN))
            .map(|s| s.spent)
            .unwrap_or(0.0);
        (st.spent_total, m)
    }

    /// Wait until nothing is pending (tests, `fn7-sim`), or the timeout passes. Returns whether it went idle.
    pub async fn wait_idle(&self, timeout: Duration) -> bool {
        let t0 = Instant::now();
        loop {
            let pending = {
                let st = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
                st.entries.values().any(|e| e.status == Status::Pending)
            };
            if !pending {
                return true;
            }
            if t0.elapsed() > timeout {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub(crate) fn note_image_prompt(&self) {
        self.inner.seen_image_prompt.store(true, Ordering::Relaxed);
    }

    pub(crate) fn interim_prompts(&self) -> bool {
        !self.inner.seen_image_prompt.load(Ordering::Relaxed)
    }
}

/// sha256(`PROMPT_VERSION` \n label \n size \n prompt text), hex.
pub fn image_key(label: &str, prompt: &ImagePrompt) -> String {
    let mut h = Sha256::new();
    h.update(PROMPT_VERSION.as_bytes());
    h.update(b"\n");
    h.update(label.as_bytes());
    h.update(b"\n");
    h.update(prompt.size.as_bytes());
    h.update(b"\n");
    h.update(prompt.text.as_bytes());
    hex::encode(h.finalize())
}

async fn worker(inner: Weak<Inner>, mut rx: mpsc::Receiver<Job>) {
    let concurrency = inner.upgrade().map(|i| i.cfg.concurrency).unwrap_or(1);
    let sem = Arc::new(Semaphore::new(concurrency));
    while let Some(job) = rx.recv().await {
        let Ok(permit) = sem.clone().acquire_owned().await else {
            return;
        };
        let Some(inner) = inner.upgrade() else {
            return;
        };
        tokio::spawn(async move {
            let _permit = permit;
            let ctx = job.ctx.clone();
            let fut = run_job(inner, job);
            match ctx {
                Some(c) => theatre_trace::within(c, fut).await,
                None => fut.await,
            }
        });
    }
}

async fn run_job(inner: Arc<Inner>, job: Job) {
    let Some(gen) = inner.gen.clone() else {
        return;
    };
    let req = GenRequest {
        key: job.key.clone(),
        kind: job.prompt.kind,
        prompt: job.prompt.text.clone(),
        size: job.prompt.size.clone(),
    };
    let t0 = Instant::now();
    let res = gen.generate(&req).await;
    let latency_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let stored = res.and_then(|g| {
        let side = Sidecar {
            key: job.key.clone(),
            kind: job.prompt.kind,
            prompt: job.prompt.text.clone(),
            provider: gen.provider().into(),
            model: g.model.clone(),
            ext: g.ext.into(),
            bytes: g.bytes.len() as u64,
            created_at: theatre_trace::now_ms(),
            cost_usd: g.cost_usd,
            latency_ms,
            match_id: job.match_id.clone(),
        };
        let path = inner.cache.store(&g.bytes, &side)?;
        Ok((side, path))
    });
    let m = job.match_id.clone().unwrap_or_else(|| MATCH_UNKNOWN.into());
    let mut st = inner.state.lock().unwrap_or_else(|e| e.into_inner());
    let now = st.tick();
    match stored {
        Ok((side, path)) => {
            // Settle the reservation at the actual cost.
            let delta = side.cost_usd - job.price;
            st.spent_total += delta;
            st.matches.entry(m).or_default().spent += delta;
            if let Some(e) = st.entries.get_mut(&job.key) {
                e.status = Status::Ready;
                e.url = Some(format!("{}/{}.{}", inner.cfg.url_prefix, job.key, side.ext));
                e.ext = Some(side.ext.clone());
                e.cost_usd = side.cost_usd;
                e.updated_at = now;
            }
            drop(st);
            tracing::debug!(hash = %job.key, kind = side.kind.as_str(), latency_ms, "image ready");
            theatre_trace::event(
                "media_ready",
                None,
                json!({ "hash": job.key, "kind": side.kind, "provider": side.provider, "model": side.model,
                        "status": "ready", "latency_ms": latency_ms, "cost_usd": side.cost_usd, "bytes": side.bytes,
                        "path": path.display().to_string() }),
            );
        }
        Err(err) => {
            st.spent_total -= job.price;
            st.matches.entry(m).or_default().spent -= job.price;
            if let Some(e) = st.entries.get_mut(&job.key) {
                e.status = Status::Failed;
                e.reason = Some(err.to_string());
                e.updated_at = now;
            }
            drop(st);
            tracing::warn!(hash = %job.key, error = %err, "image failed");
            theatre_trace::event(
                "media_failed",
                None,
                json!({ "hash": job.key, "kind": job.prompt.kind, "provider": gen.provider(), "status": "failed",
                        "latency_ms": latency_ms, "error": err.to_string() }),
            );
        }
    }
}

#[cfg(test)]
mod tests;

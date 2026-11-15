//! The duel picker at run time (state schema 14; the call itself is `fn7_agents::duel_pick`; the design is
//! docs/DUEL_MOVES.md §4.1).
//!
//! **The replay problem and how it is solved.** A blow is resolved inside pure, synchronous hooks (`on_instant`, the
//! live view, the resolve, a replay all compute the same blow and the same panel from the same inputs), and none of
//! them may wait for a model. So a pick is a **side input ordered with the duel's actions**:
//!
//! 1. A typed line arrives (`on_instant`). If the pick cache holds `(normalized line, range, list version)`, the pick
//!    is known NOW: it is journaled as a pick record stamped with the line's own `(received_at_ms, seq)`, ordered just
//!    before the line, so the line's blow takes the pick's **move class** (gated by range, bolts and armies as any
//!    class) and its **presentation** (clips, path, object, body part). The d20 and the matrix decide the outcome.
//! 2. Otherwise the line is resolved at once with the keyword mapper's class (as before), and the server makes the
//!    call **outside the hooks** (a spawned task, `duel_picker_timeout_ms`). Its answer (or its timeout or error:
//!    `pick: null`) is journaled as a pick record stamped with the server clock when it came back. A panel stages
//!    the line with the pick's presentation only if that stamp is at or before the panel's input close (and the pick's
//!    clip serves the class the rules resolved); else the mapper's. A late answer still fills the cache.
//! 3. Pick records are entries of the plugin's journal (`fields.duel_pick`), so they are ordered with the duel's
//!    actions by `(received_at_ms, seq)` in the live view, survive a restart, and are handed to the resolve
//!    (those stamped at or before its batch's newest action), which records them as the `duel_picks` event; a replay
//!    feeds that event back ([`crate::present::Feed`]) and never calls a model. A client can never send one
//!    (`validate_action` refuses `fields.duel_pick`).
//!
//! What was picked, how long it took, its tokens and cost, and whether it was used land in each line's `duel_line`
//! record and the SQLite rows ([`DuelData`]).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fn7_agents::duel_pick::{self, Pick, PickCtx};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use theatre_plugin_api::LlmProvider;
use theatre_trace::{Ctx, Record, Sink};

/// One pick, as journaled and as kept in the bout (`Bout::picks`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PickRec {
    pub duel_id: String,
    /// The line it is for: its `seq` and receive stamp (a human line).
    pub line_seq: u64,
    pub line_at_ms: i64,
    /// Known at the line's arrival (the cache): it may set the class.
    #[serde(default)]
    pub pre: bool,
    /// When it was known (server clock): its place in the duel's order.
    pub at_ms: i64,
    /// `null`: the call timed out or failed (the mapper's pick stands).
    pub pick: Option<Pick>,
    #[serde(default)]
    pub cached: bool,
    #[serde(default)]
    pub latency_ms: i64,
    #[serde(default)]
    pub tokens_in: u64,
    #[serde(default)]
    pub tokens_out: u64,
    #[serde(default)]
    pub cost_usd: f64,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub error: Option<String>,
    /// The cache key: `normalized line|range|list version`.
    #[serde(default)]
    pub key: String,
}

/// Where the server keeps the duel data (docs/DUEL_MOVES.md §2): one row per typed line, and the pick cache.
pub trait DuelData: Send + Sync {
    /// One `duel_line` record of match `run` (an upsert by match, duel and blow number).
    fn line(&self, run: &str, row: &Value);
    /// The cached picks (read once at startup).
    fn cached(&self) -> Vec<(String, Pick)> {
        vec![]
    }
    /// A pick to cache.
    fn cache(&self, _key: &str, _pick: &Pick, _model: &str) {}
}

/// The cache key of a line.
pub fn key(text: &str, range: &str, list_version: &str) -> String {
    format!("{}|{range}|{list_version}", crate::moves::normalize(text))
}

/// $ per million tokens (input, output) of the picker's model.
pub const PRICE_PER_MTOK: (f64, f64) = (0.10, 0.50);

/// The server's picker: the provider, the cache, the store.
pub struct Picker {
    llm: Arc<dyn LlmProvider>,
    cache: Mutex<HashMap<String, Pick>>,
    data: Option<Arc<dyn DuelData>>,
}

impl Picker {
    pub fn new(llm: Arc<dyn LlmProvider>, data: Option<Arc<dyn DuelData>>) -> Picker {
        let cache = data.as_ref().map(|d| d.cached().into_iter().collect()).unwrap_or_default();
        Picker { llm, cache: Mutex::new(cache), data }
    }

    pub fn cached(&self, key: &str) -> Option<Pick> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).get(key).cloned()
    }

    /// Put a pick in the cache by hand (tests, a warm-up).
    pub fn prime(&self, key: &str, pick: &Pick) {
        self.put(key, pick);
    }

    fn put(&self, key: &str, pick: &Pick) {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).insert(key.to_string(), pick.clone());
        if let Some(d) = &self.data {
            d.cache(key, pick, duel_pick::MODEL);
        }
    }

    /// Ask in the background: `done` gets the record (stamped when it came back, or at the timeout). A late answer
    /// still fills the cache. Without a tokio runtime nothing happens (no pick: the mapper's stands).
    pub fn spawn(self: &Arc<Self>, list: Value, ctx: PickCtx, mut rec: PickRec, timeout: Duration, done: impl FnOnce(PickRec) + Send + 'static) {
        let Ok(rt) = tokio::runtime::Handle::try_current() else { return };
        let me = self.clone();
        let trace = theatre_trace::current();
        rt.spawn(async move {
            let started = std::time::Instant::now();
            let tally = Arc::new(Tally::default());
            let inner = me.clone();
            let key = rec.key.clone();
            let (l2, c2) = (list.clone(), ctx.clone());
            let t2 = tally.clone();
            // The call runs to its end even past the timeout (its answer warms the cache).
            let call = tokio::spawn(async move {
                let req = duel_pick::build_request(&l2, &c2);
                let ask = inner.llm.complete(req);
                let out = match trace {
                    Some(t) => {
                        let mut c = Ctx::new(Arc::new(Tee { inner: Some(t.sink.clone()), tally: t2 }), t.run.clone());
                        c.round = t.round;
                        c.step = Some(duel_pick::STEP.into());
                        theatre_trace::within(c, ask).await
                    }
                    None => theatre_trace::within(Ctx::new(Arc::new(Tee { inner: None, tally: t2 }), "duel_pick"), ask).await,
                };
                let pick = out.map_err(|e| format!("{e:#}")).and_then(|r| duel_pick::parse(r.first().unwrap_or(""), &l2, &c2));
                if let Ok(p) = &pick {
                    inner.put(&key, p);
                }
                pick
            });
            let got = tokio::time::timeout(timeout, call).await;
            rec.latency_ms = started.elapsed().as_millis() as i64;
            rec.at_ms = theatre_trace::now_ms();
            rec.model = duel_pick::MODEL.into();
            let (i, o) = tally.get();
            rec.tokens_in = i;
            rec.tokens_out = o;
            rec.cost_usd = (i as f64 * PRICE_PER_MTOK.0 + o as f64 * PRICE_PER_MTOK.1) / 1e6;
            match got {
                Ok(Ok(Ok(p))) => rec.pick = Some(p),
                Ok(Ok(Err(e))) => rec.error = Some(e),
                Ok(Err(e)) => rec.error = Some(format!("task: {e}")),
                Err(_) => rec.error = Some("timeout".into()),
            }
            done(rec);
        });
    }
}

/// Token counts of the HTTP exchanges a call made.
#[derive(Default)]
struct Tally(Mutex<(u64, u64)>);

impl Tally {
    fn get(&self) -> (u64, u64) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Forwards every record to the match's recording (when there is one) and counts the tokens.
struct Tee {
    inner: Option<Arc<dyn Sink>>,
    tally: Arc<Tally>,
}

impl Sink for Tee {
    fn next_id(&self) -> i64 {
        self.inner.as_ref().map(|s| s.next_id()).unwrap_or(0)
    }
    fn write(&self, record: Record) {
        if let Record::Http { http, .. } = &record {
            let mut t = self.tally.0.lock().unwrap_or_else(|e| e.into_inner());
            t.0 += http.input_tokens.unwrap_or(0);
            t.1 += http.output_tokens.unwrap_or(0);
        }
        if let Some(s) = &self.inner {
            s.write(record);
        }
    }
}

/// Does the pick's first clip serve `class` (by the list's `for`, or by its weapon clip sets)?
pub fn serves(clip: &str, class: &str) -> bool {
    let list = crate::moves::current();
    list.clip_for(clip).is_some_and(|f| f.split(", ").any(|x| x == class || (class == "close" && x == "close")))
        || list.weapon_sets.iter().any(|w| w.sets.iter().any(|(c, clips)| c == class && clips.iter().any(|x| *x == clip)))
}

//! Token-play caption polish at run time (state schema 19; the call itself is `fn7_agents::scene_polish`).
//!
//! The duel picker's replay design ([`crate::picker`]), for a scene: the resolve builds the scene with its rule
//! captions and never waits for a model. With the room's `token_play_llm` (off by default), `llm` and a server LLM,
//! the plugin asks for the captions of each new scene of a human seat **outside the hooks** (a spawned task,
//! `token_play_llm_timeout_ms`). The answer (or its timeout or error: `captions: null`) is journaled as a caption
//! record (`fields.scene_captions`) stamped with the server clock when it came back:
//!
//! - `present` shows the polished captions at once (display only);
//! - the next resolve is handed every caption record of a kept, unpolished scene, records them as the
//!   `scene_captions` event (the side input) and applies them to the scene in the state (`scene.captions` in
//!   `round.outcome.public.scene_captions[]`); a replay feeds that event back ([`crate::present::Feed`]) and never
//!   calls a model. A client can never send one (`validate_action` refuses `fields.scene_captions`).
//!
//! With the polish off, failing or late, the rule captions stand.

use std::sync::Arc;
use std::time::Duration;

use fn7_agents::scene_polish::{self, PolishCtx};
use serde::{Deserialize, Serialize};
use theatre_plugin_api::LlmProvider;
use theatre_trace::Ctx;

use crate::picker::{Tally, Tee};
use crate::scene::Scene;

/// One polish answer, as journaled and handed to a resolve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptionRec {
    pub scene_id: String,
    /// The scene's beat.
    pub beat: u32,
    /// When it came back (server clock).
    pub at_ms: i64,
    /// `null`: the call timed out or failed (the rule captions stand).
    pub captions: Option<Vec<String>>,
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
}

/// What the polish model is shown of a scene (names, verbs, the rule captions, the public outcome).
pub fn ctx_of(s: &Scene) -> PolishCtx {
    let name = |id: &Option<String>| -> String {
        id.as_ref().and_then(|id| s.actors.iter().find(|a| &a.id == id)).map(|a| a.name.clone()).unwrap_or_default()
    };
    PolishCtx {
        place: s.place.name.clone(),
        result: s.outcome.result.clone(),
        outcome: s.outcome.text.clone(),
        steps: s
            .steps
            .iter()
            .map(|x| (name(&Some(x.actor.clone())), x.verb.clone(), name(&x.target_actor), x.caption.clone(), x.mood.clone()))
            .collect(),
    }
}

/// The server's polisher: the provider.
pub struct Polisher {
    llm: Arc<dyn LlmProvider>,
}

impl Polisher {
    pub fn new(llm: Arc<dyn LlmProvider>) -> Polisher {
        Polisher { llm }
    }

    /// Ask in the background: `done` gets the record. Without a tokio runtime nothing happens (the rule captions
    /// stand).
    pub fn spawn(self: &Arc<Self>, scene: &Scene, timeout: Duration, done: impl FnOnce(CaptionRec) + Send + 'static) {
        let Ok(rt) = tokio::runtime::Handle::try_current() else { return };
        let me = self.clone();
        let ctx = ctx_of(scene);
        let mut rec = CaptionRec { scene_id: scene.scene_id.clone(), beat: scene.beat, at_ms: 0, captions: None, latency_ms: 0,
                                   tokens_in: 0, tokens_out: 0, cost_usd: 0.0, model: scene_polish::MODEL.into(), error: None };
        let trace = theatre_trace::current();
        rt.spawn(async move {
            let started = std::time::Instant::now();
            let tally = Arc::new(Tally::default());
            let t2 = tally.clone();
            let c2 = ctx.clone();
            let call = async move {
                let ask = me.llm.complete(scene_polish::build_request(&c2));
                let out = match trace {
                    Some(t) => {
                        let mut c = Ctx::new(Arc::new(Tee { inner: Some(t.sink.clone()), tally: t2 }), t.run.clone());
                        c.round = t.round;
                        c.step = Some(scene_polish::STEP.into());
                        theatre_trace::within(c, ask).await
                    }
                    None => theatre_trace::within(Ctx::new(Arc::new(Tee { inner: None, tally: t2 }), "scene_polish"), ask).await,
                };
                out.map_err(|e| format!("{e:#}")).and_then(|r| scene_polish::parse(r.first().unwrap_or(""), &c2))
            };
            let got = tokio::time::timeout(timeout, call).await;
            rec.latency_ms = started.elapsed().as_millis() as i64;
            rec.at_ms = theatre_trace::now_ms();
            let (i, o) = tally.get();
            rec.tokens_in = i;
            rec.tokens_out = o;
            rec.cost_usd = (i as f64 * crate::picker::PRICE_PER_MTOK.0 + o as f64 * crate::picker::PRICE_PER_MTOK.1) / 1e6;
            match got {
                Ok(Ok(c)) => rec.captions = Some(c),
                Ok(Err(e)) => rec.error = Some(e),
                Err(_) => rec.error = Some("timeout".into()),
            }
            done(rec);
        });
    }
}

//! Run a beat's minds concurrently. Each call gets a per-call timeout; a late call is dropped and replaced by its rules
//! fallback, so the beat never waits on the slowest model. The report says which calls fell back and why.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use theatre_plugin_api::LlmProvider;

use crate::common::{trace_fallback, Fallback, Outcome};
use crate::fold::{self, FoldCtx};
use crate::general::{self, GeneralCtx};
use crate::god::{self, GodCtx, GodOut};
use crate::hand::{self, HandCtx, HandOut};
use crate::materialize::{self, MaterializeCtx};
use crate::narrator::{self, Narration, NarratorCtx};
use crate::ramify::{self, RamifyCtx, RamifyOut};
use crate::resolver::{self, ResolverCtx, ResolverOut};
use crate::step::{self, StepCtx, StepOut};

/// Run one mind future with a timeout; on timeout, the fallback (recorded as `Fallback::Timeout`).
pub async fn run_job<T, F>(
    step: &str,
    actor: Option<&str>,
    timeout: Duration,
    fut: F,
    fallback: impl FnOnce() -> T,
) -> Outcome<T>
where
    F: Future<Output = Outcome<T>>,
{
    match tokio::time::timeout(timeout, fut).await {
        Ok(o) => o,
        Err(_) => {
            trace_fallback(step, actor, &Fallback::Timeout);
            Outcome::fell_back(fallback(), Fallback::Timeout)
        }
    }
}

/// One call of any mind.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum MindCall {
    Hand(HandCtx),
    /// Subagent, possession or paladin (by `StepCtx::kind`).
    Step(StepCtx),
    God(GodCtx),
    Resolver(ResolverCtx),
    Narrator(NarratorCtx),
    Materialize(MaterializeCtx),
    Fold(FoldCtx),
    Ramify(RamifyCtx),
    General(GeneralCtx),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mind", content = "out")]
pub enum MindReply {
    Hand(HandOut),
    Step(StepOut),
    God(GodOut),
    Resolver(ResolverOut),
    Narrator(Narration),
    Materialize(String),
    Fold(String),
    Ramify(RamifyOut),
    General(fn7_model::Judgment),
}

impl MindCall {
    /// The DATA_MODEL §3 step name.
    pub fn step(&self) -> &'static str {
        match self {
            MindCall::Hand(_) => hand::STEP,
            MindCall::Step(c) => c.kind.step(),
            MindCall::God(_) => god::STEP,
            MindCall::Resolver(_) => resolver::STEP,
            MindCall::Narrator(_) => narrator::STEP,
            MindCall::Materialize(_) => materialize::STEP,
            MindCall::Fold(_) => fold::STEP,
            MindCall::Ramify(_) => ramify::STEP,
            MindCall::General(_) => general::STEP,
        }
    }

    /// The trace actor (a Ref string), as in DATA_MODEL §3.
    pub fn actor(&self) -> Option<String> {
        match self {
            MindCall::Hand(c) => Some(c.agent.to_string()),
            MindCall::Step(c) => Some(c.agent.to_string()),
            MindCall::God(c) => Some(c.god.to_string()),
            MindCall::Resolver(c) => Some(c.place.to_string()),
            MindCall::Narrator(_) => None,
            MindCall::Materialize(c) => Some(c.cluster.to_string()),
            MindCall::Fold(c) => Some(c.owner.to_string()),
            MindCall::Ramify(c) => Some(c.region.to_string()),
            MindCall::General(c) => Some(c.army.to_string()),
        }
    }

    pub async fn run(&self, llm: &dyn LlmProvider) -> Outcome<MindReply> {
        match self {
            MindCall::Hand(c) => hand::run(llm, c).await.map(MindReply::Hand),
            MindCall::Step(c) => step::run(llm, c).await.map(MindReply::Step),
            MindCall::God(c) => god::run(llm, c).await.map(MindReply::God),
            MindCall::Resolver(c) => resolver::run(llm, c).await.map(MindReply::Resolver),
            MindCall::Narrator(c) => narrator::run(llm, c).await.map(MindReply::Narrator),
            MindCall::Materialize(c) => materialize::run(llm, c).await.map(MindReply::Materialize),
            MindCall::Fold(c) => fold::run(llm, c).await.map(MindReply::Fold),
            MindCall::Ramify(c) => ramify::run(llm, c).await.map(MindReply::Ramify),
            MindCall::General(c) => general::run(llm, c).await.map(MindReply::General),
        }
    }

    pub fn fallback(&self) -> MindReply {
        match self {
            MindCall::Hand(c) => MindReply::Hand(hand::fallback(c)),
            MindCall::Step(c) => MindReply::Step(step::fallback(c)),
            MindCall::God(c) => MindReply::God(god::fallback(c)),
            MindCall::Resolver(c) => MindReply::Resolver(resolver::fallback(c)),
            MindCall::Narrator(c) => MindReply::Narrator(narrator::fallback(c)),
            MindCall::Materialize(c) => MindReply::Materialize(materialize::fallback(c)),
            MindCall::Fold(c) => MindReply::Fold(fold::fallback(c)),
            MindCall::Ramify(c) => MindReply::Ramify(ramify::fallback(c)),
            MindCall::General(c) => MindReply::General(general::fallback(c)),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunReport {
    /// One outcome per call, in call order.
    pub replies: Vec<Outcome<MindReply>>,
    /// Indices of the calls that fell back (error, timeout or junk).
    pub fell_back: Vec<usize>,
    /// Calls answered by rules by design (no LLM asked; not fallbacks).
    pub rules_only: usize,
}

impl RunReport {
    pub fn llm_calls(&self) -> usize {
        self.replies.len() - self.rules_only
    }
    pub fn fallbacks(&self) -> usize {
        self.fell_back.len()
    }
}

/// Run every call concurrently (`concurrency` = max in flight; 0 = all at once), each under `timeout`.
pub async fn run_minds(
    llm: &dyn LlmProvider,
    calls: &[MindCall],
    timeout: Duration,
    concurrency: usize,
) -> RunReport {
    let width = if concurrency == 0 {
        calls.len().max(1)
    } else {
        concurrency
    };
    // Boxed with an explicit type: an `async move` closure inside `.map` trips the compiler's higher-ranked lifetime
    // inference when this future must be `Send` (e.g. inside an `async_trait` method).
    type Job<'a> = Pin<Box<dyn Future<Output = (usize, Outcome<MindReply>)> + Send + 'a>>;
    let jobs: Vec<Job<'_>> = calls
        .iter()
        .enumerate()
        .map(|(i, c)| -> Job<'_> {
            Box::pin(async move {
                let actor = c.actor();
                let o = run_job(c.step(), actor.as_deref(), timeout, c.run(llm), || {
                    c.fallback()
                })
                .await;
                (i, o)
            })
        })
        .collect();
    let mut done: Vec<(usize, Outcome<MindReply>)> =
        stream::iter(jobs).buffer_unordered(width).collect().await;
    done.sort_by_key(|(i, _)| *i);
    let fell_back = done
        .iter()
        .filter(|(_, o)| o.is_fallback())
        .map(|(i, _)| *i)
        .collect();
    RunReport {
        replies: done.into_iter().map(|(_, o)| o).collect(),
        fell_back,
        rules_only: 0,
    }
}

//! # fn7-agents
//!
//! Every LLM mind, each a pure `async fn(llm, &context) -> Outcome<reply>` with a deterministic rules fallback:
//! - [`hand`]: utterances → intents (verb, targets/mentions, magnitude, route), subagent spawns, recalls
//! - [`step`]: subagents, possessions and paladins: one step toward the goal within scope; report when done
//! - [`god`]: rule on an invocation (grant / refuse / bargain / twist), name the price, dispatch a paladin
//! - [`resolver`]: ordinary acts per vicinity (Final Night's game master) → proposals
//! - [`narrator`]: caption and callouts (never blocks the beat)
//! - [`materialize`]: one line of flavor for a newly memorized person
//! - [`fold`]: old memory items → one summary
//! - [`general`]: a general's reading of an order or a decision point (obey, misread, stall, refuse, mutiny, defect)
//! - [`manifest`]: names and describes what the item engine (rules) made; never decides anything
//! - [`ramify`]: god powers that landed in a region → Entropy's yes and N geotagged ramifications (and world mods)
//!
//! Every call runs under `theatre_trace::annotate(step, actor)`, so prompts, replies and raw HTTP are recorded.
//! [`runner::run_minds`] runs a beat's calls concurrently with a per-call timeout; a late, failed or junk call is
//! replaced by its fallback and reported, so the beat never waits.
//!
//! **Prompts never contain engine ids.** Entities get short local labels ("P1", "C7", "K2"); [`common::Labels`] maps
//! them back to `Ref`s, and an unknown label becomes a `Target::Mention`. Player text only ever appears inside
//! `delimit_untrusted` fences in user messages, never in a system prompt.

pub mod common;
pub mod fold;
pub mod general;
pub mod god;
pub mod hand;
pub mod manifest;
pub mod materialize;
pub mod narrator;
pub mod ramify;
pub mod resolver;
pub mod rulebook;
pub mod runner;
pub mod step;
pub mod summon;
pub mod duel;

pub use common::{Entity, Fallback, Labels, Outcome};
pub use runner::{run_job, run_minds, MindCall, MindReply, RunReport};

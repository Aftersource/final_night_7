//! The pantheon. Definitions are data (`data/pantheon.json`); runtime state lives in the world.

use serde::{Deserialize, Serialize};

use crate::ids::*;
use crate::refs::Ref;

/// A god's definition. Adding a god is adding one of these.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GodDef {
    /// Stable slug ("time"); the `GodId` is its index in the loaded pantheon.
    pub slug: String,
    pub name: String,
    pub domain: String,
    /// Words that route an intent to this god without an LLM call (checked on raw text, never stemmed).
    pub keywords: Vec<String>,
    /// Voice and agenda, for the god's system prompt.
    pub persona: String,
    pub agenda: String,
    pub prices: Vec<PriceRule>,
    /// Slugs of gods it interferes with.
    pub rivals: Vec<String>,
    pub paladin: PaladinTemplate,
    /// Precedence when proposals conflict (higher wins), before special rules.
    pub precedence: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceRule {
    /// What it is charged for ("rewind", "slow", "raise").
    pub act: String,
    pub price: PriceKind,
    /// Scales with the intent's magnitude.
    pub base: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum PriceKind {
    /// Lost maximum health.
    Life,
    /// A debt owed to this god, collected later.
    Debt,
    /// A memorized soul chosen by the god.
    Soul,
    /// Something the demigod forgets (an item, an ally, a hand's memory).
    Forget,
    /// Something summoned turns wild.
    Wild,
    /// A secret clause revealed later.
    Clause,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaladinTemplate {
    pub title: String,
    pub traits: Vec<String>,
    pub mission: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Pantheon {
    pub version: String,
    pub gods: Vec<GodDef>,
}

impl Pantheon {
    pub fn id_of(&self, slug: &str) -> Option<GodId> {
        self.gods
            .iter()
            .position(|g| g.slug == slug)
            .map(GodId::from_ix)
    }
    pub fn get(&self, id: GodId) -> Option<&GodDef> {
        self.gods.get(id.ix())
    }
}

/// A god's runtime state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GodState {
    pub id: GodId,
    /// Display name, copied from the pantheon at setup (what [`crate::World::name_of`] returns).
    #[serde(default)]
    pub name: String,
    /// Favor per demigod (index = DemigodId), -10..10.
    pub favor: Vec<f32>,
    /// 0..1: how closely the god is watching this beat (wakes at invocations, decays).
    pub attention: f32,
    pub paladins: Vec<CharId>,
    /// The god's own mind (memory owner, subagent parent).
    pub agent: AgentId,
    pub invocations: u32,
    /// The god's own health (0..10): high Entropy wounds gods; an unmade god (0) can't rule, and Entropy answers its
    /// invocations instead.
    #[serde(default = "god_health")]
    pub health: f32,
}

fn god_health() -> f32 {
    10.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Grant,
    Refuse,
    /// Granted for a price.
    Bargain,
    /// Granted, and twisted (genie rules).
    Twist,
}

/// A god's decision on one invocation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ruling {
    pub id: RulingId,
    pub beat: u32,
    pub god: GodId,
    pub intent: IntentId,
    pub invoker: Ref,
    pub verdict: Verdict,
    pub price: Option<(PriceKind, f32)>,
    /// The god's words (shown on the feed).
    pub speech: String,
    /// An existing paladin dispatched to enforce it.
    pub dispatched: Option<CharId>,
    /// The mission of a dispatch, when the ruling sends one. With `dispatched: None` it means a new paladin is to be
    /// raised for it (the plugin spawns it via `Effect::Dispatch`).
    #[serde(default)]
    pub dispatch_mission: Option<String>,
    /// Whether it came from the LLM or the rules fallback.
    pub fallback: bool,
    /// Lightning bolts the invoker paid for it.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub bolts: u32,
    /// Where it was invoked (the invoker's position).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<crate::society::LonLat>,
}

fn is_zero(x: &u32) -> bool {
    *x == 0
}

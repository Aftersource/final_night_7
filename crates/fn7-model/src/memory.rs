//! Holistic memory. Every mind (hand, subagent, god, memorized character, cluster, group) owns memory items. Retrieval is
//! by `about` refs, salience and recency; old items fold into summaries. No embeddings.

use serde::{Deserialize, Serialize};

use crate::ids::*;
use crate::refs::Ref;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    /// Saw it happen.
    Witnessed,
    /// Did it.
    Did,
    /// Was told by someone.
    Told,
    /// Heard through a group (collective; confidence < 1).
    Rumor,
    /// Inherited at materialization from the cluster's collective memory.
    Inherited,
    /// Planted by Memory or a trickster.
    Planted,
    /// A fold of older items.
    Summary,
    /// A subagent's report to its hand.
    Report,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryItem {
    pub id: MemoryId,
    pub owner: Ref,
    pub beat: u32,
    pub kind: MemoryKind,
    /// Prompt-ready prose, from the owner's point of view, no engine ids.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// Who and what it is about (the retrieval key).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub about: Vec<Ref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ledger: Option<LedgerId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Ref>,
    /// 0–10: how much it matters to the owner.
    pub salience: u8,
    /// 0..1: true memory 1.0; rumors less; planted memories claim 1.0.
    pub confidence: f32,
    /// Erased by Memory; kept for the record, never retrieved.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub forgotten: bool,
    /// Folded into this summary item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folded_into: Option<MemoryId>,
    /// The beat it was folded (set with `folded_into`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folded_beat: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MemoryStore {
    pub items: Vec<MemoryItem>,
}

impl MemoryStore {
    pub fn next_id(&self) -> MemoryId {
        MemoryId::from_ix(self.items.len())
    }
}

/// What to pull for a prompt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recall {
    pub owner: Ref,
    /// Entities in play; items about any of them rank first.
    pub about: Vec<Ref>,
    pub max_items: usize,
    pub now: u32,
}

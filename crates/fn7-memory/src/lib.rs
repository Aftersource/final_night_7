//! # fn7-memory
//!
//! One store, many owners (hands, subagents, gods, characters, clusters, groups). Writes come from ledger rows (witnessed,
//! did, told, rumor), materialization (inherited), Memory's powers (planted, forgotten) and subagent reports.
//! Recall ranks by `about` overlap, then salience, then recency, under a budget; folding turns old items into summaries
//! so prompts and state stay bounded. The recording keeps every original item.
//!
//! Everything here is pure and deterministic: no LLM, no I/O, no hash maps in anything that affects output.

mod fold;
mod powers;
mod recall;
mod text;
mod write;

pub use fold::{apply_fold, enforce_budget, fold_plan, rules_summary, FoldPlan};
pub use powers::{forget, plant, PLANTED_SALIENCE};
pub use recall::{ago, hedge, is_active, recall, recall_indexed, render, sources_of, MemoryIndex};
pub use text::{has_engine_id, name, scrub};
pub use write::{
    can_remember, from_row, inherit, rumor_confidence, write, write_row_memories, NewMemory,
    INHERIT_CAP,
};

#[cfg(test)]
mod tests;

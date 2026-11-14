//! Folding: old items become one `Summary` so prompts and live state stay bounded. Originals stay in the store (marked
//! `folded_into`) and in the recording.

use std::collections::BTreeMap;

use fn7_model::*;
use serde::{Deserialize, Serialize};

use crate::recall::{ago, is_active};
use crate::text::{clip, name, scrub};
use crate::write::{write, NewMemory};

/// Which of an owner's items to fold into one summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FoldPlan {
    pub owner: Ref,
    /// Oldest first (by beat, then id).
    pub items: Vec<MemoryId>,
    /// First and last beat covered.
    pub beats: (u32, u32),
}

/// An owner's live items, oldest first.
fn active_of(store: &MemoryStore, owner: Ref) -> Vec<&MemoryItem> {
    let mut v: Vec<&MemoryItem> = store
        .items
        .iter()
        .filter(|m| m.owner == owner && is_active(m))
        .collect();
    v.sort_by_key(|m| (m.beat, m.id));
    v
}

/// If `owner` has more than `max_items` live items, plan to fold all but the `keep_recent` newest into one summary
/// (`keep_recent` is clamped below `max_items`, so the owner ends at or under budget). `None` when within budget.
pub fn fold_plan(
    store: &MemoryStore,
    owner: Ref,
    keep_recent: usize,
    max_items: usize,
) -> Option<FoldPlan> {
    let max_items = max_items.max(1);
    let act = active_of(store, owner);
    if act.len() <= max_items {
        return None;
    }
    let keep = keep_recent.min(max_items - 1);
    let old = &act[..act.len() - keep];
    if old.len() < 2 {
        return None;
    }
    Some(FoldPlan {
        owner,
        items: old.iter().map(|m| m.id).collect(),
        beats: (
            old[0].beat,
            old.iter().map(|m| m.beat).max().unwrap_or(old[0].beat),
        ),
    })
}

/// Write the `Summary` for `plan` and mark the originals folded into it (at `beat`). The summary takes the union of `about`, the
/// highest salience, the mean confidence, and the latest folded beat (capped at `beat`) so it doesn't outrank fresher
/// items on recency.
pub fn apply_fold(
    store: &mut MemoryStore,
    plan: &FoldPlan,
    summary_text: &str,
    beat: u32,
) -> MemoryId {
    let mut about = Vec::new();
    let mut salience = 0u8;
    let mut conf = 0.0f64;
    let mut n = 0usize;
    for id in &plan.items {
        if let Some(m) = store.items.get(id.ix()) {
            about.extend(m.about.iter().copied());
            salience = salience.max(m.salience);
            conf += m.confidence as f64;
            n += 1;
        }
    }
    let confidence = if n == 0 {
        1.0
    } else {
        (conf / n as f64) as f32
    };
    let id = write(
        store,
        NewMemory {
            owner: plan.owner,
            beat: plan.beats.1.min(beat),
            kind: MemoryKind::Summary,
            text: summary_text.to_string(),
            about,
            ledger: None,
            from: None,
            salience,
            confidence,
        },
    );
    for i in &plan.items {
        if let Some(m) = store.items.get_mut(i.ix()) {
            m.folded_into = Some(id);
            m.folded_beat = Some(beat);
        }
    }
    id
}

/// Deterministic summary with no LLM: the span, the three most salient memories (clipped), and who it was mostly about.
pub fn rules_summary(w: &World, plan: &FoldPlan) -> String {
    let now = w.meta.beat;
    let mut items: Vec<&MemoryItem> = plan
        .items
        .iter()
        .filter_map(|i| w.memory.items.get(i.ix()))
        .collect();
    let span = if plan.beats.0 == plan.beats.1 {
        ago(now, plan.beats.0)
    } else {
        format!(
            "from {} to {}",
            ago(now, plan.beats.0),
            ago(now, plan.beats.1)
        )
    };
    let mut counts: BTreeMap<Ref, usize> = BTreeMap::new();
    for m in &items {
        for r in &m.about {
            *counts.entry(*r).or_default() += 1;
        }
    }
    items.sort_by(|a, b| {
        b.salience
            .cmp(&a.salience)
            .then(b.beat.cmp(&a.beat))
            .then(a.id.cmp(&b.id))
    });
    let gist: Vec<String> = items
        .iter()
        .take(3)
        .map(|m| clip(&scrub(w, &m.text), 110))
        .collect();
    let mut s = format!(
        "Older memories ({} of them, {span}), in short: {}",
        items.len(),
        gist.join(" / ")
    );
    let mut top: Vec<(Ref, usize)> = counts.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let names: Vec<String> = top.iter().take(3).map(|(r, _)| name(w, *r)).collect();
    if !names.is_empty() {
        s.push_str(&format!(" Mostly about {}.", names.join(", ")));
    }
    s
}

/// Fold every owner over `per_owner` live items with rules summaries (keeping the newest half). Owners are visited in
/// ref order, so the result is deterministic. Returns the new summary ids.
pub fn enforce_budget(w: &mut World, per_owner: usize, beat: u32) -> Vec<MemoryId> {
    let per_owner = per_owner.max(2);
    let mut counts: BTreeMap<Ref, usize> = BTreeMap::new();
    for m in w.memory.items.iter().filter(|m| is_active(m)) {
        *counts.entry(m.owner).or_default() += 1;
    }
    let mut out = Vec::new();
    for (owner, n) in counts {
        if n <= per_owner {
            continue;
        }
        if let Some(plan) = fold_plan(&w.memory, owner, per_owner / 2, per_owner) {
            let text = rules_summary(w, &plan);
            out.push(apply_fold(&mut w.memory, &plan, &text, beat));
        }
    }
    out
}

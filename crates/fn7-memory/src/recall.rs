//! Recall for prompts (rank by `about` overlap, salience, recency) and rendering to compact prose.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use fn7_model::*;

use crate::text::{name, scrub};

/// Every store a mind draws on: its own, its groups' collective memory, and (for a character materialized from a
/// cluster) that cluster's. Sorted, deduplicated.
pub fn sources_of(w: &World, owner: Ref) -> Vec<Ref> {
    let mut v = vec![owner];
    v.extend(w.groups_of(owner).as_slice().iter().map(|g| Ref::Group(*g)));
    if let Ref::Char(c) = owner {
        if let Some(Origin::Materialized { cluster, .. }) =
            w.characters.get(c.ix()).map(|c| c.origin)
        {
            v.push(Ref::Cluster(cluster));
        }
    }
    v.sort_unstable();
    v.dedup();
    v
}

/// Live in recall: neither forgotten nor folded into a summary.
#[inline]
pub fn is_active(m: &MemoryItem) -> bool {
    !m.forgotten && m.folded_into.is_none()
}

#[inline]
fn overlap(m: &MemoryItem, about: &[Ref]) -> usize {
    if about.is_empty() {
        return 0;
    }
    m.about
        .iter()
        .filter(|r| about.binary_search(r).is_ok())
        .count()
}

/// Overlap desc, salience desc, beat desc, id desc. A total order (ids are unique).
fn rank(a: &(usize, &MemoryItem), b: &(usize, &MemoryItem)) -> Ordering {
    b.0.cmp(&a.0)
        .then(b.1.salience.cmp(&a.1.salience))
        .then(b.1.beat.cmp(&a.1.beat))
        .then(b.1.id.cmp(&a.1.id))
}

fn top(mut c: Vec<(usize, &MemoryItem)>, max: usize) -> Vec<&MemoryItem> {
    if max == 0 {
        return Vec::new();
    }
    if c.len() > max {
        c.select_nth_unstable_by(max - 1, rank);
        c.truncate(max);
    }
    c.sort_unstable_by(rank);
    c.into_iter().map(|(_, m)| m).collect()
}

fn sorted_about(r: &Recall) -> Vec<Ref> {
    let mut a = r.about.clone();
    a.sort_unstable();
    a.dedup();
    a
}

/// The memories to put in `r.owner`'s prompt. Linear scan of the store (see [`MemoryIndex`] for the indexed path;
/// both return identical results).
pub fn recall<'a>(w: &'a World, r: &Recall) -> Vec<&'a MemoryItem> {
    let src = sources_of(w, r.owner);
    let about = sorted_about(r);
    let c = w
        .memory
        .items
        .iter()
        .filter(|m| src.contains(&m.owner) && is_active(m) && m.beat <= r.now)
        .map(|m| (overlap(m, &about), m))
        .collect();
    top(c, r.max_items)
}

/// Per-owner index over a [`MemoryStore`]. Not serialized: rebuild with [`MemoryIndex::build`] after load, and call
/// [`MemoryIndex::sync`] after writes (the store is append-only, so syncing only indexes the new tail). A stale index
/// is still correct: [`recall_indexed`] scans any unindexed tail.
#[derive(Debug, Clone, Default)]
pub struct MemoryIndex {
    by_owner: BTreeMap<Ref, Vec<u32>>,
    indexed: usize,
}

impl MemoryIndex {
    pub fn build(store: &MemoryStore) -> Self {
        let mut ix = Self::default();
        ix.sync(store);
        ix
    }
    /// Index items appended since the last build/sync.
    pub fn sync(&mut self, store: &MemoryStore) {
        if store.items.len() < self.indexed {
            *self = Self::default();
        }
        for (i, m) in store.items.iter().enumerate().skip(self.indexed) {
            self.by_owner.entry(m.owner).or_default().push(i as u32);
        }
        self.indexed = store.items.len();
    }
    /// All item positions owned by `owner` (any state), oldest first.
    pub fn of(&self, owner: Ref) -> &[u32] {
        self.by_owner.get(&owner).map(Vec::as_slice).unwrap_or(&[])
    }
    pub fn indexed(&self) -> usize {
        self.indexed
    }
}

/// [`recall`] through a [`MemoryIndex`].
pub fn recall_indexed<'a>(w: &'a World, ix: &MemoryIndex, r: &Recall) -> Vec<&'a MemoryItem> {
    let src = sources_of(w, r.owner);
    let about = sorted_about(r);
    let items = &w.memory.items;
    let ok = |m: &MemoryItem| is_active(m) && m.beat <= r.now;
    let mut c = Vec::new();
    for s in &src {
        for &i in ix.of(*s) {
            if let Some(m) = items.get(i as usize).filter(|m| ok(m)) {
                c.push((overlap(m, &about), m));
            }
        }
    }
    for m in items.iter().skip(ix.indexed.min(items.len())) {
        if ok(m) && src.binary_search(&m.owner).is_ok() {
            c.push((overlap(m, &about), m));
        }
    }
    top(c, r.max_items)
}

/// "just now", "1 beat ago", "3 beats ago".
pub fn ago(now: u32, beat: u32) -> String {
    match now.saturating_sub(beat) {
        0 => "just now".into(),
        1 => "1 beat ago".into(),
        n => format!("{n} beats ago"),
    }
}

/// How sure the owner is, as a hedge (empty when they're sure). Planted memories always feel true.
pub fn hedge(m: &MemoryItem) -> &'static str {
    if m.kind == MemoryKind::Planted || m.confidence >= 0.95 {
        ""
    } else if m.confidence >= 0.7 {
        ", people say"
    } else if m.confidence >= 0.4 {
        ", rumor has it"
    } else {
        ", a faint whisper"
    }
}

/// Compact bullet lines for a prompt, one per item, with beat-relative time and hedges; collective memories name
/// whose they are. Relative to `w.meta.beat`. No engine ids.
pub fn render(w: &World, items: &[&MemoryItem]) -> String {
    let now = w.meta.beat;
    let mut out = String::new();
    for m in items {
        let when = ago(now, m.beat);
        // Rumor text already names the group it went around.
        let whose = match (m.owner, m.kind) {
            (_, MemoryKind::Rumor) => String::new(),
            (Ref::Group(_) | Ref::Cluster(_), _) => format!(" (among the {})", name(w, m.owner)),
            _ => String::new(),
        };
        out.push_str(&format!(
            "- {when}{whose}{}: {}\n",
            hedge(m),
            scrub(w, &m.text)
        ));
    }
    out
}

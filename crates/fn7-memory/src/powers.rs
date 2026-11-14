//! Memory's powers: forgetting and planting.

use fn7_model::*;

use crate::text::sentence;
use crate::write::{write, NewMemory};

/// Salience of a planted memory (it was put there to matter).
pub const PLANTED_SALIENCE: u8 = 6;

/// Erase from `owner` every one of its own items about any of `about` (folded ones too, so a later unfold can't bring
/// them back). Rows linked to an erased item get `owner` in `erased_from`, so [`crate::from_row`] never rewrites them.
/// An empty `about` forgets nothing. Collective memories of the owner's groups are untouched (the group still knows).
/// Returns the newly forgotten ids, ascending.
pub fn forget(w: &mut World, owner: Ref, about: &[Ref]) -> Vec<MemoryId> {
    let mut out = Vec::new();
    let mut rows = Vec::new();
    for m in w.memory.items.iter_mut() {
        if m.owner != owner || m.forgotten || !m.about.iter().any(|r| about.contains(r)) {
            continue;
        }
        m.forgotten = true;
        out.push(m.id);
        rows.extend(m.ledger);
    }
    rows.sort_unstable();
    rows.dedup();
    for id in rows {
        if let Some(r) = w.ledger.rows.get_mut(id.ix()) {
            if !r.erased_from.contains(&owner) {
                r.erased_from.push(owner);
            }
        }
    }
    out
}

/// Plant a false (or true) memory: kind `Planted`, confidence 1.0. Engine ids in `text` are replaced with names.
pub fn plant(w: &mut World, owner: Ref, text: &str, about: &[Ref], beat: u32) -> MemoryId {
    let text = sentence(w, text);
    let mut about = about.to_vec();
    about.retain(|r| *r != owner);
    write(
        &mut w.memory,
        NewMemory {
            owner,
            beat,
            kind: MemoryKind::Planted,
            text,
            about,
            ledger: None,
            from: None,
            salience: PLANTED_SALIENCE,
            confidence: 1.0,
        },
    )
}

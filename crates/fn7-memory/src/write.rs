//! Writes: the one entry point ([`write`]), memories derived from ledger rows ([`from_row`]) and inheritance at
//! materialization ([`inherit`]).

use fn7_model::*;
use serde::{Deserialize, Serialize};

use crate::text::{name, owner_voice, sentence, Voice};

/// A memory item before it gets an id. `forgotten` starts false and `folded_into` empty.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewMemory {
    pub owner: Ref,
    pub beat: u32,
    pub kind: MemoryKind,
    pub text: String,
    pub about: Vec<Ref>,
    pub ledger: Option<LedgerId>,
    pub from: Option<Ref>,
    pub salience: u8,
    pub confidence: f32,
}

/// Most inherited memories a newly materialized character starts with.
pub const INHERIT_CAP: usize = 8;

/// Append one item. `about` is sorted and deduplicated so retrieval and serialization are stable.
pub fn write(store: &mut MemoryStore, item: NewMemory) -> MemoryId {
    let id = store.next_id();
    let mut about = item.about;
    about.sort_unstable();
    about.dedup();
    store.items.push(MemoryItem {
        id,
        owner: item.owner,
        beat: item.beat,
        kind: item.kind,
        text: item.text,
        about,
        ledger: item.ledger,
        from: item.from,
        salience: item.salience.min(10),
        confidence: item.confidence.clamp(0.0, 1.0),
        forgotten: false,
        folded_into: None,
        folded_beat: None,
    });
    id
}

/// Refs that can own memories (have a mind, or are a collective).
pub fn can_remember(r: Ref) -> bool {
    matches!(
        r,
        Ref::Demigod(_)
            | Ref::Char(_)
            | Ref::Cluster(_)
            | Ref::Group(_)
            | Ref::God(_)
            | Ref::Agent(_)
    )
}

/// What a row is about, from `owner`'s point of view (the owner itself is left out).
fn row_about(row: &LedgerRow, owner: Ref) -> Vec<Ref> {
    let mut v: Vec<Ref> = std::iter::once(row.actor)
        .chain(row.targets.iter().copied())
        .collect();
    if let Some(p) = row.place {
        v.push(Ref::Place(p));
    }
    v.retain(|r| *r != owner);
    v.sort_unstable();
    v.dedup();
    v
}

fn clamp10(x: i32) -> u8 {
    x.clamp(0, 10) as u8
}

/// Confidence of a rumor that is `hops` steps from the event, entering a group of the given permeability.
pub fn rumor_confidence(hops: u32, permeability: f32) -> f32 {
    let hops = hops.max(1);
    let base = 0.85 * 0.8f32.powi(hops as i32 - 1);
    (base * (0.6 + 0.4 * permeability.clamp(0.0, 1.0))).clamp(0.15, 0.95)
}

/// Owners that already hold a memory linked to `row` (any state: forgotten and folded count, so nothing comes back).
fn owners_with_row(store: &MemoryStore, row: LedgerId) -> Vec<Ref> {
    let mut v: Vec<Ref> = store
        .items
        .iter()
        .filter(|m| m.ledger == Some(row))
        .map(|m| m.owner)
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Memories one ledger row produces at `beat`: `Did` for the actor, `Witnessed` for each witness (and for each target
/// with a mind, since it happened to them), `Told` for `told_to`, and a `Rumor` for each group in `known_by`.
///
/// Idempotent: owners that already hold a memory of this row, and owners in `erased_from`, are skipped, so calling it
/// again after news spreads only writes rumors for newly reached groups. A rumor's hops are the beats between the event
/// and the beat its group learned it (`LedgerRow::known_at`; at least 1), so writing late doesn't weaken it. Groups
/// missing from `known_at` (schema-1 rows) count from `beat`.
pub fn from_row(w: &World, row: LedgerId, beat: u32) -> Vec<NewMemory> {
    let Some(r) = w.ledger.get(row) else {
        return Vec::new();
    };
    let mut seen = owners_with_row(&w.memory, row);
    seen.extend(r.erased_from.iter().copied());
    seen.sort_unstable();
    seen.dedup();
    let mut out = Vec::new();
    let mut take = |owner: Ref, out: &mut Vec<NewMemory>, f: &dyn Fn(Ref) -> NewMemory| {
        if !can_remember(owner) {
            return;
        }
        if let Err(i) = seen.binary_search(&owner) {
            seen.insert(i, owner);
            out.push(f(owner));
        }
    };
    let mag = r.magnitude as i32;
    let val = (r.valence as i32).abs();
    let act = sentence(w, &r.act);
    let base =
        |owner: Ref, kind: MemoryKind, text: String, salience: u8, confidence: f32| NewMemory {
            owner,
            beat,
            kind,
            text,
            about: row_about(r, owner),
            ledger: Some(row),
            from: None,
            salience,
            confidence,
        };

    take(r.actor, &mut out, &|o| {
        base(
            o,
            MemoryKind::Did,
            format!("{} did this: {act}", owner_voice(o, Voice::Subject)),
            clamp10(mag + 1),
            1.0,
        )
    });
    let minds_hit = r
        .targets
        .iter()
        .copied()
        .filter(|t| can_remember(*t) && !matches!(t, Ref::Group(_)));
    for o in r.witnessed_by.iter().copied().chain(minds_hit) {
        let target = r.targets.contains(&o);
        take(o, &mut out, &|o| {
            let text = if target {
                format!("It happened to {}: {act}", owner_voice(o, Voice::Object))
            } else {
                format!("{} saw it happen: {act}", owner_voice(o, Voice::Subject))
            };
            let sal = mag * 3 / 4 + val / 2 + if target { 3 } else { 1 };
            base(o, MemoryKind::Witnessed, text, clamp10(sal), 1.0)
        });
    }
    for o in r.told_to.iter().copied() {
        take(o, &mut out, &|o| {
            let text = format!("{} told: {act}", owner_voice(o, Voice::Passive));
            let mut m = base(o, MemoryKind::Told, text, clamp10(mag * 2 / 3 + 1), 0.9);
            m.from = Some(r.actor);
            m
        });
    }
    for g in r.known_by.as_slice() {
        let perm = w.groups.get(g.ix()).map(|x| x.permeability).unwrap_or(0.5);
        let hops = r
            .known_since(*g)
            .unwrap_or(beat)
            .saturating_sub(r.beat)
            .max(1);
        take(Ref::Group(*g), &mut out, &|o| {
            let text = format!("Word going around among the {}: {act}", name(w, o));
            base(
                o,
                MemoryKind::Rumor,
                text,
                clamp10(mag / 2 + val / 3),
                rumor_confidence(hops, perm),
            )
        });
    }
    out
}

/// [`from_row`] then [`write`] each; returns the new ids in order.
pub fn write_row_memories(w: &mut World, row: LedgerId, beat: u32) -> Vec<MemoryId> {
    let news = from_row(w, row, beat);
    news.into_iter().map(|m| write(&mut w.memory, m)).collect()
}

/// Starting memories for a newly materialized character: the rows among `rows` that its cluster (or any of its groups)
/// could know, as `Inherited` items. Highest magnitude first (then most recent, then lowest id), capped at
/// [`INHERIT_CAP`]. Rows the character already remembers, or that were erased from it or its cluster, are skipped.
pub fn inherit(w: &World, character: CharId, rows: &[LedgerId], beat: u32) -> Vec<NewMemory> {
    let Some(ch) = w.characters.get(character.ix()) else {
        return Vec::new();
    };
    let me = Ref::Char(character);
    let cluster = match ch.origin {
        Origin::Materialized { cluster, .. } => Some(Ref::Cluster(cluster)),
        _ => None,
    };
    let mut groups = ch.groups.clone();
    if let Some(Ref::Cluster(c)) = cluster {
        if let Some(cl) = w.clusters.get(c.ix()) {
            groups = groups.union(&cl.groups);
        }
    }
    let known = owners_rows(&w.memory, me);
    let mut picks: Vec<(&LedgerRow, f32)> = Vec::new();
    let mut ids: Vec<LedgerId> = rows.to_vec();
    ids.sort_unstable();
    ids.dedup();
    for id in ids {
        let Some(r) = w.ledger.get(id) else { continue };
        if known.binary_search(&id).is_ok() || r.erased_from.contains(&me) {
            continue;
        }
        if cluster.is_some_and(|c| r.erased_from.contains(&c)) {
            continue;
        }
        let first_hand = cluster.is_some_and(|c| {
            r.actor == c
                || r.witnessed_by.contains(&c)
                || r.targets.contains(&c)
                || r.told_to.contains(&c)
        });
        let heard = r.known_by.intersects(&groups);
        if first_hand {
            picks.push((r, 0.9));
        } else if heard {
            picks.push((r, 0.6));
        }
    }
    picks.sort_by(|a, b| {
        b.0.magnitude
            .cmp(&a.0.magnitude)
            .then(b.0.beat.cmp(&a.0.beat))
            .then(a.0.id.cmp(&b.0.id))
    });
    picks.truncate(INHERIT_CAP);
    let origin = cluster.map(|c| name(w, c));
    picks
        .into_iter()
        .map(|(r, conf)| {
            let act = sentence(w, &r.act);
            let text = match &origin {
                Some(label) => format!("Back when I was one of the {label}, we knew this: {act}"),
                None => format!("Before anyone knew my name, I heard this: {act}"),
            };
            NewMemory {
                owner: me,
                beat,
                kind: MemoryKind::Inherited,
                text,
                about: row_about(r, me),
                ledger: Some(r.id),
                from: cluster,
                salience: clamp10(r.magnitude as i32 / 2 + (r.valence as i32).abs() / 2 + 1),
                confidence: conf,
            }
        })
        .collect()
}

fn owners_rows(store: &MemoryStore, owner: Ref) -> Vec<LedgerId> {
    let mut v: Vec<LedgerId> = store
        .items
        .iter()
        .filter(|m| m.owner == owner)
        .filter_map(|m| m.ledger)
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

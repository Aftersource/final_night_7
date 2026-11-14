//! Small lookups over refs shared by every module.

use fn7_model::seed::Rng;
use fn7_model::{AgentKind, AgentStatus, DemigodId, GroupSet, PlaceId, Ref, World};

/// Does the ref point at an existing row?
pub fn exists(w: &World, r: Ref) -> bool {
    match r {
        Ref::Demigod(i) => i.ix() < w.demigods.len(),
        Ref::Char(i) => i.ix() < w.characters.len(),
        Ref::Cluster(i) => i.ix() < w.clusters.len(),
        Ref::Group(i) => i.ix() < w.groups.len(),
        Ref::God(i) => i.ix() < w.gods.len(),
        Ref::Place(i) => i.ix() < w.places.len(),
        Ref::Item(i) => i.ix() < w.items.len(),
        Ref::Agent(i) => i.ix() < w.agents.len(),
        Ref::Ledger(i) => i.ix() < w.ledger.rows.len(),
        Ref::Cell(i) => i.ix() < w.cells.len(),
        Ref::Mob(i) => i.ix() < w.mobs.len(),
    }
}

/// Alive for things with a body; agents are alive while their demigod (or body) is; everything else is always "alive".
pub fn is_alive(w: &World, r: Ref) -> bool {
    match r {
        Ref::Demigod(i) => w.demigods.get(i.ix()).is_some_and(|d| d.alive),
        Ref::Char(i) => w.characters.get(i.ix()).is_some_and(|c| c.alive),
        Ref::Cell(i) => w.cells.get(i.ix()).is_some_and(|c| c.ended.is_none() && c.size > 0),
        Ref::Mob(i) => w.mobs.get(i.ix()).is_some_and(|m| m.ended.is_none() && m.size > 0),
        Ref::Agent(a) => {
            let Some(ag) = w.agents.get(a.ix()) else {
                return false;
            };
            match ag.kind {
                AgentKind::Paladin { body, .. } | AgentKind::Possession { body, .. } => {
                    is_alive(w, Ref::Char(body))
                        && demigod_of(w, r).is_none_or(|d| is_alive(w, Ref::Demigod(d)))
                }
                _ => demigod_of(w, r).is_none_or(|d| is_alive(w, Ref::Demigod(d))),
            }
        }
        _ => exists(w, r),
    }
}

/// The demigod a ref acts for: itself, a hand's owner (walking up subagents), a possessed body's player, a Following
/// group's demigod.
pub fn demigod_of(w: &World, r: Ref) -> Option<DemigodId> {
    match r {
        Ref::Demigod(d) => Some(d),
        Ref::Agent(mut a) => {
            for _ in 0..32 {
                let ag = w.agents.get(a.ix())?;
                match ag.kind {
                    AgentKind::Hand { demigod, .. } => return Some(demigod),
                    AgentKind::Subagent { parent } | AgentKind::Possession { parent, .. } => {
                        a = parent
                    }
                    _ => return None,
                }
            }
            None
        }
        Ref::Char(c) => w
            .characters
            .get(c.ix())?
            .possessed_by
            .and_then(|a| demigod_of(w, Ref::Agent(a))),
        Ref::Group(g) => w.groups.get(g.ix())?.follows,
        _ => None,
    }
}

pub fn place_of(w: &World, r: Ref) -> Option<PlaceId> {
    match r {
        Ref::Demigod(d) => w.demigods.get(d.ix()).map(|x| x.place),
        Ref::Char(c) => w.characters.get(c.ix()).map(|x| x.place),
        Ref::Cluster(c) => w.clusters.get(c.ix()).map(|x| x.place),
        Ref::Place(p) => Some(p),
        Ref::Cell(c) => w.cells.get(c.ix()).and_then(|x| w.clusters.get(x.crowd.ix())).map(|x| x.place),
        Ref::Mob(m) => w.mobs.get(m.ix()).map(|x| x.place),
        _ => None,
    }
}

/// Groups of a ref, including a group ref itself (for witnesses and audiences).
pub fn groups_incl(w: &World, r: Ref) -> GroupSet {
    match r {
        Ref::Group(g) if g.ix() < w.groups.len() => GroupSet::new(vec![g]),
        _ => w.groups_of(r),
    }
}

/// Upsert into a sparse `(key, value)` vector sorted by key; returns the new value (clamped).
pub fn bump<K: Ord + Copy>(v: &mut Vec<(K, f32)>, k: K, delta: f32, lo: f32, hi: f32) -> f32 {
    match v.binary_search_by(|x| x.0.cmp(&k)) {
        Ok(i) => {
            v[i].1 = (v[i].1 + delta).clamp(lo, hi);
            v[i].1
        }
        Err(i) => {
            let x = delta.clamp(lo, hi);
            v.insert(i, (k, x));
            x
        }
    }
}

pub fn get<K: Ord + Copy>(v: &[(K, f32)], k: K) -> f32 {
    v.binary_search_by(|x| x.0.cmp(&k))
        .map(|i| v[i].1)
        .unwrap_or(0.0)
}

/// Weighted index pick (weights ≤ 0 never chosen); `None` if all weights are ≤ 0.
pub fn weighted(rng: &mut Rng, weights: &[f64]) -> Option<usize> {
    let total: f64 = weights.iter().filter(|w| **w > 0.0).sum();
    if total <= 0.0 {
        return None;
    }
    let mut x = rng.f64() * total;
    for (i, w) in weights.iter().enumerate() {
        if *w <= 0.0 {
            continue;
        }
        if x < *w {
            return Some(i);
        }
        x -= w;
    }
    weights.iter().rposition(|w| *w > 0.0)
}

/// `k` distinct items in a seeded order.
pub fn sample<T: Clone>(rng: &mut Rng, xs: &[T], k: usize) -> Vec<T> {
    let mut idx: Vec<usize> = (0..xs.len()).collect();
    let k = k.min(xs.len());
    for i in 0..k {
        let j = i + rng.below((idx.len() - i) as u64) as usize;
        idx.swap(i, j);
    }
    idx[..k].iter().map(|i| xs[*i].clone()).collect()
}

/// Mark agents as ended when their demigod is eliminated or their body dies.
pub fn end_agents_for(w: &mut World, dead: Ref, beat: u32) {
    let ids: Vec<usize> = (0..w.agents.len())
        .filter(|i| {
            let a = &w.agents[*i];
            a.is_live()
                && match (dead, a.kind) {
                    (
                        Ref::Char(c),
                        AgentKind::Paladin { body, .. } | AgentKind::Possession { body, .. },
                    ) => body == c,
                    (
                        Ref::Demigod(d),
                        AgentKind::Hand { .. }
                        | AgentKind::Subagent { .. }
                        | AgentKind::Possession { .. },
                    ) => demigod_of(w, Ref::Agent(a.id)) == Some(d),
                    _ => false,
                }
        })
        .collect();
    for i in ids {
        w.agents[i].status = AgentStatus::Ended;
        w.agents[i].ended_beat = Some(beat);
    }
    if let Ref::Char(c) = dead {
        if let Some(ch) = w.characters.get_mut(c.ix()) {
            ch.possessed_by = None;
        }
    }
}

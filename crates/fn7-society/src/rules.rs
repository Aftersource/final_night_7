//! Per-beat rules: rumor spread over the Venn, mood/allegiance/worship drift, debts coming due, the shrinking arena and
//! the rewind ring. All deterministic (seeded by `meta.seed`, beat, row and group).

use fn7_model::seed::{hash_str, Rng};
use fn7_model::*;

use crate::geo::distance_km;
use crate::util::{bump, demigod_of, get};
use crate::venn::shared_heads;

/// Society rows below this magnitude don't spread beyond the groups of their witnesses.
pub const SOCIETY_SPREAD_MIN: u8 = 6;
/// Rows older than this (beats) stop spreading.
pub const SPREAD_HORIZON: u32 = 40;
/// Per-beat decay of a row's spread chance with age.
pub const SPREAD_DECAY: f32 = 0.92;
/// Rows this recent (beats) still move moods.
pub const DRIFT_WINDOW: u32 = 2;
/// Share of the gap to its baseline (`Cluster::baseline`) a mood closes each beat.
pub const MOOD_RELAX: f32 = 0.04;
/// Allegiance at which a cluster joins a demigod's Following, and below which it leaves.
pub const FOLLOW_JOIN: f32 = 0.6;
pub const FOLLOW_LEAVE: f32 = 0.3;
/// The arena's final radius as a fraction of its starting radius.
pub const ARENA_END_FRACTION: f64 = 0.15;

/// Each live row's `known_by` grows by at most one hop: a group not yet knowing it, overlapping (sharing members with)
/// a group that knew it at the start of this beat, may learn it with chance
/// `permeability × √carry × 0.92^age × (0.4 + 0.6 × magnitude/10)`, where carry = the share of the knowing group's
/// people who are also in the new group, decided by `Rng::derive(seed, [beat, row, group])`.
/// Disjoint groups never learn it. Each new group is appended to the row's `known_at` with this beat.
/// Returns the hops in row, then group order.
pub fn spread(w: &mut World, beat: u32) -> Vec<SpreadHop> {
    let (n, shared, total) = shared_heads(w);
    let mut hops = vec![];
    for ri in 0..w.ledger.rows.len() {
        let r = &w.ledger.rows[ri];
        if r.known_by.is_empty() || r.beat > beat || beat - r.beat > SPREAD_HORIZON {
            continue;
        }
        // The society's small events stay local news (only its big ones travel the Venn).
        if r.magnitude < SOCIETY_SPREAD_MIN && SocKind::of(&r.verb).is_some() {
            continue;
        }
        let age = beat - r.beat;
        let decay = SPREAD_DECAY.powi(age as i32) * (0.4 + 0.6 * r.magnitude.min(10) as f32 / 10.0);
        let known: Vec<GroupId> = r.known_by.as_slice().to_vec();
        let mut new: Vec<SpreadHop> = vec![];
        for g in 0..n {
            let gid = GroupId::from_ix(g);
            let grp = &w.groups[g];
            if grp.dissolved_beat.is_some() || r.known_by.contains(gid) {
                continue;
            }
            // Strongest overlap with any knowing group.
            let mut best: Option<(f32, GroupId)> = None;
            for k in &known {
                if k.ix() >= n {
                    continue;
                }
                let s = shared[k.ix() * n + g];
                if s == 0 {
                    continue;
                }
                // Share of the knowing group's people who are also in `g` (directional: a small faction inside a big
                // neighborhood carries news out easily, the neighborhood carries it in slowly).
                let ov = (s as f64 / total[k.ix()].max(1) as f64).min(1.0) as f32;
                if best.is_none_or(|b| ov > b.0) {
                    best = Some((ov, *k));
                }
            }
            let Some((ov, via)) = best else { continue };
            let p = (grp.permeability * ov.sqrt() * decay).clamp(0.0, 1.0);
            let roll = Rng::derive(
                w.meta.seed,
                &[hash_str("spread"), beat as u64, ri as u64, g as u64],
            )
            .f64() as f32;
            if roll < p {
                new.push(SpreadHop {
                    row: r.id,
                    group: gid,
                    via,
                    beat,
                    p,
                });
            }
        }
        let row = &mut w.ledger.rows[ri];
        for h in &new {
            row.known_by.insert(h.group);
            row.known_at.push((h.group, beat));
        }
        hops.extend(new);
    }
    hops
}

struct RowView {
    beat: u32,
    actor_dg: Option<DemigodId>,
    god: Option<GodId>,
    mag: f32,
    val: f32,
    targets: Vec<Ref>,
    witnesses: Vec<Ref>,
    known: GroupSet,
    /// A society row (schema 4): the society rules already moved the moods around it, so only the crowds in it
    /// (targets, witnesses) feel it here, not everyone who heard.
    society: bool,
}

/// How strongly a member is touched by a row: targeted or witnessed 1.0, heard through its groups 0.5 (society rows:
/// not at all).
fn exposure(rv: &RowView, me: Ref, groups: &GroupSet) -> f32 {
    if rv.targets.contains(&me) || rv.witnesses.contains(&me) {
        if rv.society {
            0.3
        } else {
            1.0
        }
    } else if !rv.society && rv.known.intersects(groups) {
        0.5
    } else {
        0.0
    }
}

/// Moods relax toward each cluster's baseline; recent rows move them. Harm (valence < 0) raises anger and fear and lowers allegiance to
/// the acting demigod; miracles (valence > 0) raise hope and devotion, allegiance to the actor, and worship of the
/// granting god. Clusters cross into (or out of) a demigod's Following at the allegiance thresholds.
pub fn drift(w: &mut World, beat: u32) {
    let rows: Vec<RowView> = w
        .ledger
        .rows
        .iter()
        .filter(|r| r.beat <= beat && beat - r.beat <= DRIFT_WINDOW && r.valence != 0)
        .map(|r| RowView {
            beat: r.beat,
            actor_dg: demigod_of(w, r.actor),
            god: match r.source {
                Source::God { god, .. } => Some(god),
                _ => match r.actor {
                    Ref::God(g) => Some(g),
                    _ => None,
                },
            },
            mag: (r.magnitude.max(1) as f32) / 10.0,
            val: r.valence as f32 / 5.0,
            targets: r.targets.clone(),
            witnesses: r.witnessed_by.clone(),
            known: r.known_by.clone(),
            society: SocKind::of(&r.verb).is_some(),
        })
        .collect();

    for c in w.clusters.iter_mut() {
        let (m, b) = (&mut c.mood, c.baseline);
        for (x, rest) in [
            (&mut m.anger, b.anger),
            (&mut m.fear, b.fear),
            (&mut m.hope, b.hope),
            (&mut m.devotion, b.devotion),
        ] {
            *x += (rest - *x) * MOOD_RELAX;
        }
        let me = Ref::Cluster(c.id);
        for rv in &rows {
            let e = exposure(rv, me, &c.groups);
            if e == 0.0 {
                continue;
            }
            let s = e * 0.5f32.powi((beat - rv.beat) as i32) * rv.mag;
            let direct = if rv.targets.contains(&me) { 1.5 } else { 1.0 };
            let m = &mut c.mood;
            if rv.val < 0.0 {
                let v = -rv.val;
                m.anger += 0.12 * s * v;
                m.fear += 0.10 * s * v * direct;
                if let Some(d) = rv.actor_dg {
                    bump(&mut c.allegiance, d, -0.15 * s * v, -1.0, 1.0);
                }
            } else {
                let v = rv.val;
                m.hope += 0.12 * s * v;
                m.devotion += 0.08 * s * v;
                if let Some(d) = rv.actor_dg {
                    bump(&mut c.allegiance, d, 0.15 * s * v, -1.0, 1.0);
                }
                if let Some(g) = rv.god {
                    bump(&mut c.worship, g, 0.05 * s * v, 0.0, 1.0);
                }
            }
        }
        let m = &mut c.mood;
        for x in [&mut m.anger, &mut m.fear, &mut m.hope, &mut m.devotion] {
            *x = crate::setup::round3(x.clamp(0.0, 1.0));
        }
    }
    for ch in w.characters.iter_mut().filter(|c| c.alive) {
        let me = Ref::Char(ch.id);
        for rv in &rows {
            let Some(d) = rv.actor_dg else { continue };
            let e = exposure(rv, me, &ch.groups);
            if e > 0.0 {
                let s = e * 0.5f32.powi((beat - rv.beat) as i32) * rv.mag;
                bump(&mut ch.allegiance, d, 0.15 * s * rv.val, -1.0, 1.0);
            }
        }
    }
    // Following membership at thresholds.
    let follow: Vec<(DemigodId, GroupId, bool)> = w
        .demigods
        .iter()
        .map(|d| (d.id, d.following, d.alive))
        .collect();
    for c in w.clusters.iter_mut() {
        for (d, g, alive) in &follow {
            let a = get(&c.allegiance, *d);
            if *alive && a >= FOLLOW_JOIN {
                c.groups.insert(*g);
            } else if !*alive || a < FOLLOW_LEAVE {
                c.groups.remove(*g);
            }
        }
    }
}

/// Rows whose open or delayed debt is due by `beat` (everything outstanding on the Final Night), oldest first.
pub fn due_debts(w: &World, beat: u32) -> Vec<LedgerId> {
    let final_night = beat >= w.meta.beats_total;
    w.ledger
        .open_debts()
        .filter(|r| final_night || r.debt.is_some_and(|d| d.due_beat <= beat))
        .map(|r| r.id)
        .collect()
}

/// Safe radius at `beat`: linear from the arena's starting radius to `meta.arena_end_radius_km` at `beats_total`
/// (schema-1 states without it: `ARENA_END_FRACTION` of the start).
pub fn arena_radius_at(w: &World, beat: u32) -> f64 {
    let start = w
        .places
        .get(w.meta.arena.ix())
        .map(|p| p.radius_km)
        .unwrap_or(w.meta.arena_radius_km);
    let end = if w.meta.arena_end_radius_km > 0.0 {
        w.meta.arena_end_radius_km
    } else {
        start * ARENA_END_FRACTION
    };
    let t = (beat as f64 / w.meta.beats_total.max(1) as f64).clamp(0.0, 1.0);
    // Ease-out: Time's circle closes fastest early (a third of the way by a quarter of the match), then tightens
    // slowly toward the Final Night.
    start + (end - start) * (1.0 - (1.0 - t).powf(ARENA_EASE))
}

/// The exponent of the arena's ease-out curve (1 = linear).
pub const ARENA_EASE: f64 = 1.8;

/// Shrink the arena to this beat's radius (`meta.beat`) and return who is outside it (alive demigods, then alive
/// characters, by id).
pub fn shrink_arena(w: &mut World) -> Vec<Ref> {
    let r = arena_radius_at(w, w.meta.beat);
    w.meta.arena_radius_km = r;
    let c = w
        .places
        .get(w.meta.arena.ix())
        .map(|p| p.center)
        .unwrap_or([0.0, 0.0]);
    let mut out: Vec<Ref> = w
        .demigods
        .iter()
        .filter(|d| d.alive && distance_km(c, d.pos) > r)
        .map(|d| Ref::Demigod(d.id))
        .collect();
    out.extend(
        w.characters
            .iter()
            .filter(|x| x.alive && distance_km(c, x.pos) > r)
            .map(|x| Ref::Char(x.id)),
    );
    out
}

/// Push a rewind frame for `meta.beat`: every demigod and every live character.
pub fn capture_rewind(w: &mut World) {
    let mut entries: Vec<RewindEntry> = w
        .demigods
        .iter()
        .map(|d| RewindEntry {
            who: Ref::Demigod(d.id),
            pos: Some(d.pos),
            health: Some(d.health),
            alive: Some(d.alive),
        })
        .collect();
    // Every live character a player or god has touched, and the simulated people who matter (generals,
    // ringleaders, prophets, heroes); the crowd of minor people the society produced isn't rewound.
    entries.extend(
        w.characters
            .iter()
            .filter(|c| c.alive)
            .filter(|c| match c.life.as_deref() {
                None => true,
                Some(l) => {
                    c.memorized_by.is_some()
                        || c.possessed_by.is_some()
                        || matches!(l.role, Role::General | Role::Ringleader | Role::Prophet | Role::Hero)
                }
            })
            .map(|c| RewindEntry {
                who: Ref::Char(c.id),
                pos: Some(c.pos),
                health: Some(c.health),
                alive: Some(true),
            }),
    );
    let beat = w.meta.beat;
    w.rewind.push(RewindFrame { beat, entries });
}

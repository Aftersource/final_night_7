//! Benito's rule (rules 0.7.0): no two players may occupy the same hex at the people level unless engaging, meaning fighting to the
//! death, intercourse or the like".
//!
//! Cells are the client's own hex cells (`fn7_model::hexgrid`, people level: 3 m hexes, keys `"<face>:people:<q>:<r>"`).
//! After each beat's moves, no two standing demigods share a people-level cell unless they are **engaged**: this beat
//! one acted directly on the other (a ledger row whose actor is one of them, or one of its agents or bodies, and whose
//! targets include the other: a fight, a seduction, a parley, a theft). Otherwise the **later mover** is nudged to the
//! nearest free neighbouring cell (rings 1..=[`MAX_RING`] around its cell, in the grid's fixed ring order), its
//! position set to that cell's centre. Deterministic: demigods that did not move this beat keep their cells first (by
//! id), then movers in the order of their last move row (earlier first; ties by id). Applies to every pair of demigods,
//! a player's own two included (two bodies can't stand on one 3 m hex). Nudges are a few metres, so a demigod's place
//! never changes; each is recorded as an `occupancy_nudge` trace event.

use std::collections::BTreeMap;

use fn7_model::hexgrid::{cell_at, HexCell, HexLevel};
use fn7_model::*;
use serde_json::json;

use crate::world::event;

/// The farthest ring searched for a free cell (beyond it the demigod stays put: never seen in practice).
pub const MAX_RING: i64 = 6;

/// Bookkeeping verbs that are not an engagement (a god's ledger entries about a demigod).
const NOT_ENGAGING: &[&str] = &["debt", "favor", "price", "collect", "delay", "bolts", "note", "tell"];

/// Pairs (low id, high id) of demigods engaged with each other in the rows `from..` of the ledger.
pub fn engaged_pairs(w: &World, from: usize) -> Vec<(DemigodId, DemigodId)> {
    let mut out = vec![];
    for r in w.ledger.rows.iter().skip(from) {
        if NOT_ENGAGING.contains(&r.verb.as_str()) {
            continue;
        }
        let Some(a) = fn7_society::demigod_of(w, r.actor) else {
            continue;
        };
        for t in &r.targets {
            if let Some(b) = fn7_society::demigod_of(w, *t).filter(|b| *b != a) {
                if !matches!(t, Ref::Demigod(_) | Ref::Char(_)) {
                    continue;
                }
                let p = if a < b { (a, b) } else { (b, a) };
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// Enforce the rule after a beat's moves. `start` holds every demigod's position at the beat's start; `from` is the
/// first ledger row of the beat (engagements and move order are read from the rows since). Returns the nudges as
/// `(demigod, from cell, to cell)`. A no-op before schema 5.
pub fn enforce(w: &mut World, beat: u32, from: usize, start: &[LonLat]) -> Vec<(DemigodId, String, String)> {
    if !w.two_bodies() {
        return vec![];
    }
    let engaged = engaged_pairs(w, from);
    // The row of each demigod's last move this beat.
    let mut last_move: BTreeMap<DemigodId, usize> = BTreeMap::new();
    for (i, r) in w.ledger.rows.iter().enumerate().skip(from) {
        if r.verb == "move" {
            for t in &r.targets {
                if let Ref::Demigod(d) = t {
                    last_move.insert(*d, i);
                }
            }
        }
    }
    let mut order: Vec<(u8, usize, DemigodId)> = w
        .demigods
        .iter()
        .filter(|d| d.alive)
        .map(|d| {
            let moved = start.get(d.id.ix()).is_none_or(|p| *p != d.pos);
            if moved {
                (1, last_move.get(&d.id).copied().unwrap_or(usize::MAX), d.id)
            } else {
                (0, 0, d.id)
            }
        })
        .collect();
    order.sort();
    let mut taken: Vec<(HexCell, DemigodId)> = vec![];
    let mut nudges = vec![];
    for (_, _, d) in order {
        let here = cell_at(w.demigods[d.ix()].pos, HexLevel::People);
        let blocked = |c: &HexCell, taken: &[(HexCell, DemigodId)]| {
            taken.iter().any(|(tc, o)| tc == c && !engaged.contains(&if *o < d { (*o, d) } else { (d, *o) }))
        };
        if !blocked(&here, &taken) {
            taken.push((here, d));
            continue;
        }
        let free = (1..=MAX_RING)
            .flat_map(|k| here.ring(k))
            .find(|c| !taken.iter().any(|(tc, _)| tc == c));
        match free {
            Some(c) => {
                w.demigods[d.ix()].pos = c.center();
                event(
                    "occupancy_nudge",
                    Some(Ref::Demigod(d).to_string()),
                    json!({ "beat": beat, "demigod": d, "from": here.key(), "to": c.key() }),
                );
                nudges.push((d, here.key(), c.key()));
                taken.push((c, d));
            }
            None => taken.push((here, d)),
        }
    }
    nudges
}

#[cfg(test)]
mod tests {
    use super::*;
    use fn7_model::hexgrid::people_cell;

    #[test]
    fn the_later_mover_steps_aside_unless_engaged() {
        let mut w = crate::testkit::world(2, 5);
        assert_eq!(w.demigods.len(), 4, "two players, two demigods each");
        let start: Vec<LonLat> = w.demigods.iter().map(|d| d.pos).collect();
        // Demigod 2 (player 1's... any rival) walks onto demigod 0's spot this beat.
        let spot = w.demigods[0].pos;
        let from = w.ledger.rows.len();
        w.demigods[2].pos = spot;
        let n = enforce(&mut w, 1, from, &start);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].0, DemigodId(2), "the mover is nudged, not the one standing there");
        assert_eq!(people_cell(w.demigods[0].pos), people_cell(spot));
        assert_ne!(people_cell(w.demigods[2].pos), people_cell(spot));
        let to = HexCell::parse(&n[0].2).unwrap();
        assert!(cell_at(spot, HexLevel::People).neighbors().contains(&to), "the nearest free neighbour");
        // Engaged (2 attacks 0 this beat): they may share the cell.
        let mut w2 = crate::testkit::world(2, 5);
        let from = w2.ledger.rows.len();
        w2.demigods[2].pos = spot;
        let row = w2.ledger.next_id();
        w2.ledger.rows.push(LedgerRow {
            id: row,
            beat: 1,
            source: Source::Resolver,
            actor: Ref::Demigod(DemigodId(2)),
            verb: "damage".into(),
            act: "a duel".into(),
            targets: vec![Ref::Demigod(DemigodId(0))],
            place: None,
            magnitude: 5,
            valence: -3,
            witnessed_by: vec![],
            known_by: Default::default(),
            known_at: vec![],
            told_to: vec![],
            erased_from: vec![],
            debt: None,
            cause: None,
            intent: None,
            at: None,
        });
        assert!(enforce(&mut w2, 1, from, &start).is_empty(), "fighting to the death shares a hex");
        // Schema 4 worlds keep the old rules.
        let mut w3 = crate::testkit::world(2, 5);
        w3.meta.schema = 4;
        w3.demigods[2].pos = spot;
        assert!(enforce(&mut w3, 1, 0, &start).is_empty());
    }
}

//! The table: who controls each demigod, and humans taking seats in a running match.
//!
//! **Controller.** A demigod is `"human"` when a Theatre seat plays it and that seat's player is a human (as far as the
//! plugin knows: `SeatState.is_bot`, set at match start from the core's seats and flipped at a takeover), else `"ai"`
//! (a Theatre bot seat, or one of the built-in AI demigods, which have synthetic seats ≥ [`SYNTHETIC_SEAT`]).
//!
//! The core never tells the plugin about presence: a human who disconnects keeps `"human"`, and a human who *leaves*
//! mid-match (the core hands their seat to a bot) is still `"human"` here until someone takes that seat over. So
//! `connected` is `true` for an AI-controlled demigod (always there) and `null` for a human one: clients combine it with
//! `room.updated` `seats[seat].connected` / `is_bot` (the source of truth for presence).
//!
//! **Mid-match join** ([`seat_takeover`], the engine's `on_seat_takeover`, called at the round boundary before
//! `on_round_start`, room setting `allow_midmatch_join`):
//! - a taken-over **bot seat** keeps its demigod, which is now human-controlled;
//! - a **new seat** takes over one of the built-in AI demigods that is still alive: the one with the most health, then
//!   the lowest id (deterministic). Its synthetic seat becomes the new seat, so its hands follow that seat's orders
//!   from this beat on (and its eliminations are reported to the Theatre);
//! - a new seat when no built-in AI demigod is alive gets **no demigod**: the state is unchanged (`None`) and the
//!   joiner spectates. Their seat is `active` to the core, but `seat_view` is null, their orders are ignored and no
//!   demigod in the public view has their seat.
//!
//! Every takeover is a `seat_takeover` trace event (or `seat_takeover_spectator`).

use fn7_model::*;
use serde_json::{json, Value};
use theatre_plugin_api::SeatTakeover;

use crate::world::{event, seat_state};
use crate::SYNTHETIC_SEAT;

/// `"human"` or `"ai"` (see the module doc).
pub fn controller(w: &World, d: &Demigod) -> &'static str {
    if d.seat < SYNTHETIC_SEAT && seat_state(w, d.seat).is_some_and(|s| !s.is_bot) {
        "human"
    } else {
        "ai"
    }
}

/// The table fields every public demigod carries: `controller`, `connected` (`true` for AI, `null` for a human: the
/// core's `room.updated` seats say) and `seat` (`null` for the built-in AI).
pub fn table_fields(w: &World, d: &Demigod) -> Value {
    let c = controller(w, d);
    json!({
        "controller": c,
        "connected": if c == "ai" { Value::Bool(true) } else { Value::Null },
        "seat": (d.seat < SYNTHETIC_SEAT).then_some(d.seat),
    })
}

/// Merge [`table_fields`] into a demigod object.
pub fn add_table_fields(w: &World, d: &Demigod, v: &mut Value) {
    if let (Some(o), Value::Object(t)) = (v.as_object_mut(), table_fields(w, d)) {
        o.extend(t);
    }
}

/// A human starts playing `t.seat` (see the module doc). Returns whether the world changed.
pub fn seat_takeover(w: &mut World, t: &SeatTakeover) -> bool {
    let base = json!({ "round": t.round, "seat": t.seat, "display_name": t.display_name, "new_seat": t.new_seat,
                       "previous_name": t.previous_name });
    let with = |mut v: Value, extra: Value| {
        if let (Some(o), Value::Object(e)) = (v.as_object_mut(), extra) {
            o.extend(e);
        }
        v
    };
    if t.seat >= SYNTHETIC_SEAT {
        event("seat_takeover_spectator", None, with(base, json!({ "reason": "seat number out of range" })));
        return false;
    }
    // A seat the world already knows (a bot's seat taken over, or a seat a human left and the core gave a bot).
    if let Some(i) = w.seats.iter().position(|s| s.seat == t.seat) {
        let d = w.seats[i].demigod;
        let was_bot = w.seats[i].is_bot;
        w.seats[i].is_bot = false;
        let x = &w.demigods[d.ix()];
        event(
            "seat_takeover",
            Some(Ref::Demigod(d).to_string()),
            with(base, json!({ "kind": "seat", "demigod": d, "name": x.name, "alive": x.alive, "health": x.health,
                               "was_bot": was_bot, "controller": "human" })),
        );
        return was_bot;
    }
    // A new seat: the built-in AI demigod still alive with the most health (then the lowest id).
    let pick = w
        .seats
        .iter()
        .filter(|s| s.seat >= SYNTHETIC_SEAT)
        .map(|s| &w.demigods[s.demigod.ix()])
        .filter(|d| d.alive)
        .max_by(|a, b| a.health.total_cmp(&b.health).then(b.id.cmp(&a.id)))
        .map(|d| d.id);
    let Some(d) = pick else {
        event(
            "seat_takeover_spectator",
            None,
            with(base, json!({ "reason": "no built-in AI demigod is alive: the joiner spectates" })),
        );
        return false;
    };
    let from = w.demigods[d.ix()].seat;
    w.demigods[d.ix()].seat = t.seat;
    if let Some(s) = w.seats.iter_mut().find(|s| s.demigod == d) {
        s.seat = t.seat;
        s.is_bot = false;
    }
    w.seats.sort_by_key(|s| s.seat);
    let x = &w.demigods[d.ix()];
    event(
        "seat_takeover",
        Some(Ref::Demigod(d).to_string()),
        with(base, json!({ "kind": "ai_demigod", "demigod": d, "name": x.name, "alive": x.alive, "health": x.health,
                           "from_synthetic_seat": from, "controller": "human" })),
    );
    true
}

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
//! - a taken-over **bot seat** keeps its demigod, which is now human-controlled (rules 0.8.0: renamed for the joiner,
//!   `"<name>'s Left Hand"` / `"<name>'s Right Hand"`, same refs);
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

/// Rules 0.8.0: a taken-over bot seat's demigods are renamed `"<name>'s Left Hand"` / `"<name>'s Right Hand"` (a name
/// another demigod already carries gets the seat appended), with their hand agents; each change is remembered.
fn rename_bodies(w: &mut World, seat: u8, name: &str, round: u32) {
    let ids: Vec<DemigodId> = w.bodies_of_seat(seat).filter(|d| d.side.is_some()).map(|d| d.id).collect();
    for d in ids {
        let side = if w.demigods[d.ix()].side == Some(Hand::Right) { "Right" } else { "Left" };
        let mut new = format!("{name}'s {side} Hand");
        if w.demigods.iter().any(|x| x.id != d && x.name == new) {
            new = format!("{new} ({seat})");
        }
        let old = std::mem::replace(&mut w.demigods[d.ix()].name, new.clone());
        if old == new {
            continue;
        }
        for h in w.demigods[d.ix()].hands {
            if let Some(a) = w.agents.get_mut(h.ix()) {
                a.name = new.clone();
            }
        }
        let pos = w.demigods[d.ix()].pos;
        let wh = crate::dm::locate(w, pos, fn7_model::hexgrid::HexLevel::People);
        crate::dm::remember(w, round, &wh, crate::dm::Decision {
            scope: fn7_model::dm::Scope::Cell,
            kind: "seat.takeover",
            query: Some(format!("takeover:{round}:{}", d.0)),
            public: true,
            text: format!("{name} took up the mantle of {old}"),
            data: json!({ "demigod": Ref::Demigod(d), "from": old, "to": new, "seat": seat }),
        });
        event("demigod_renamed", Some(Ref::Demigod(d).to_string()), json!({ "round": round, "from": old, "to": new }));
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
        // Schema 5: the seat's player is now the joiner (for profiles); before schema 6 their demigods keep their names.
        if w.two_bodies() && was_bot && !t.display_name.trim().is_empty() {
            let name: String = t.display_name.trim().chars().take(40).collect();
            w.seats[i].display_name = name.clone();
            let seat = w.seats[i].seat;
            for x in w.demigods.iter_mut().filter(|x| x.seat == seat && x.side.is_some()) {
                x.player = name.clone();
            }
            // Rules 0.8.0: the bot's demigods take the joiner's names (refs stay; their hand agents follow), so the
            // portraits and dioramas prewarmed under those names apply, and the DM remembers the change of hands.
            if w.dm_rules() {
                rename_bodies(w, seat, &name, t.round);
            }
        }
        let x = &w.demigods[d.ix()];
        event(
            "seat_takeover",
            Some(Ref::Demigod(d).to_string()),
            with(base, json!({ "kind": "seat", "demigod": d, "name": x.name, "alive": x.alive, "health": x.health,
                               "was_bot": was_bot, "controller": "human" })),
        );
        return was_bot;
    }
    // A new seat: the built-in AI player still standing with the most health over its demigods (then the lowest id;
    // schema 4: the AI demigod with the most health). Both of its demigods come with it.
    let health = |seat: u8| w.bodies_of_seat(seat).filter(|d| d.alive).map(|d| d.health).sum::<f32>();
    let pick = w
        .seats
        .iter()
        .filter(|s| s.seat >= SYNTHETIC_SEAT && w.seat_alive(s.seat))
        .max_by(|a, b| health(a.seat).total_cmp(&health(b.seat)).then(b.demigod.cmp(&a.demigod)))
        .map(|s| s.demigod);
    let Some(d) = pick else {
        event(
            "seat_takeover_spectator",
            None,
            with(base, json!({ "reason": "no built-in AI demigod is alive: the joiner spectates" })),
        );
        return false;
    };
    let from = w.demigods[d.ix()].seat;
    for x in w.demigods.iter_mut().filter(|x| x.seat == from) {
        x.seat = t.seat;
        // The joiner's name becomes the player's (the demigods keep their names for this match).
        if x.side.is_some() && !t.display_name.trim().is_empty() {
            x.player = t.display_name.trim().chars().take(40).collect();
        }
    }
    let two = w.two_bodies();
    if let Some(s) = w.seats.iter_mut().find(|s| s.demigod == d) {
        s.seat = t.seat;
        s.is_bot = false;
        if two && !t.display_name.trim().is_empty() {
            s.display_name = t.display_name.trim().chars().take(40).collect();
        }
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

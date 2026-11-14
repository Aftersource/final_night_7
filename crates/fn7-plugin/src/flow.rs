//! Flow and slow (text entry is the God of Time interaction), on the Theatre's real-time input (engine v2.2).
//!
//! The world's action never waits for players: every beat is a flow beat (`flow_ms`, the room's Compose window) that
//! resolves whether or not anyone typed, and the hands' standing orders keep the demigods acting. Typing is an overlay,
//! and the Theatre core runs it:
//!
//! - **Opening.** A client sends `action.begin` when its player starts typing. [`crate::FinalNight7Plugin`]'s
//!   `on_action_begin` opens a core action window (`deadline_in_ms` = `slow_ms`, closed by the initiator's submit,
//!   missing seats dropped) and pushes a public `phase.event` naming who slowed time. It refuses (no window) when the
//!   demigod is dead or has no bolt to pay Time. A begin from another seat while a window is open only registers that
//!   seat as engaged: nobody is told.
//! - **While open** the core holds every seat's orders (hidden) and the flow beats keep resolving.
//! - **Enter.** The initiator's submit closes the window at once; orders that missed it get a private `cut` reply from
//!   the core and never reach the plugin. Held orders arrive at the next resolve with `windowed: true`, `window_id`,
//!   `received_at_ms` and `seq`.
//! - **Penalty**, settled here at resolve (hooks are read-only) from the core's `RoundContext.windows_closed` (theatre
//!   faa7220): Time takes one bolt (a toll: since rules 0.6.0 no rival gains from it) and writes a debt
//!   `0.5 × (1 + seconds open / 10) × (1 + 0.5 × the initiator's slows in the last 20 beats)`, doubled when the bolt
//!   can't be paid; Entropy rises by `0.5 + seconds / 30`. Seconds open run from the window's `info.opened_ms` to its
//!   `closed_ms` (an Enter), or to its deadline when it timed out, even with nobody submitting: the opener pays for
//!   the full slow. Pressing Enter early is cheap, and it cuts off rivals.
//! - **Orders without a window** (Compose submissions: core bots, the plugin's AI, clients that skip `action.begin`)
//!   are an instant order: the same bolt and Entropy as a zero-second slow, but a flat debt of `instant_order_debt`
//!   (default 0.5; no repeat or seconds factor; doubled when the bolt can't be paid). They still count as slows for a
//!   later window's repeat factor.
//!
//! Everything a bill reads comes from the resolve's inputs (actions and `RoundContext`), which the core records in the
//! `resolve_round` hook, so a replay is byte-identical with no plugin memory to feed back. Who engaged a window and who
//! was dropped is hidden information: it is recorded in `slow_closed` (data) and never put in a payload.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use fn7_model::*;
use serde_json::{json, Value};
use theatre_plugin_api::ClosedWindow;

use crate::merge;
use crate::settings::Settings;
use crate::world::event;

/// Base of Time's slow debt for a real window (damage points), before seconds and repeats.
pub const SLOW_DEBT_BASE: f32 = 0.5;
/// Slows within this many beats count as repeats.
pub const SLOW_REPEAT_BEATS: u32 = 20;
/// Entropy a closed window adds (plus seconds open / 30).
pub const SLOW_ENTROPY: f32 = 0.5;
// ---------------------------------------------------------------------------------------------------------------------
// Settling at resolve
// ---------------------------------------------------------------------------------------------------------------------

/// One submission's arrival facts (from the core's `Action` metadata).
#[derive(Debug, Clone)]
pub struct Sub {
    pub demigod: DemigodId,
    pub seat: u8,
    /// Delivered from this core window (`windowed: true`).
    pub window_id: Option<u64>,
    pub seq: u64,
    pub at_ms: i64,
    /// It carries orders (text entry).
    pub orders: bool,
    /// The plugin's own AI (not a Theatre seat).
    pub ai: bool,
}

/// What the slow/flow stage decided this beat.
#[derive(Debug, Default)]
pub struct FlowOut {
    /// Public announcements, in order (`closed`, `instant`).
    pub public: Vec<Value>,
    /// Demigods whose orders this beat came through a window open at least one flow beat: they get
    /// `max_intents_per_window` acts per hand.
    pub windowed: BTreeSet<DemigodId>,
}

/// The seat's lead demigod (its first standing one; schema 4: its only one): who opened the window and owes Time.
fn demigod_of(w: &World, seat: u8) -> Option<DemigodId> {
    let s = w.seats.iter().find(|s| s.seat == seat)?;
    Some(w.lead_of_seat(s.seat).unwrap_or(s.demigod))
}

/// Record a slow on the player (both of a player's demigods keep the same list: slows are the player's).
fn push_slow(w: &mut World, d: DemigodId, beat: u32) {
    for s in w.siblings(d) {
        w.demigods[s.ix()].slows.push(beat);
    }
}

fn announce(w: &World, ev: &str, d: DemigodId, extra: Value) -> Value {
    // The player (both of their demigods are theirs; schema 4: the demigod's name).
    let name = w.demigods[d.ix()].player_name().to_string();
    let mut v = json!({
        "event": ev,
        "by": name,
        "by_ref": Ref::Demigod(d),
        "text": match ev {
            "instant" => format!("{name} gave orders without slowing time"),
            _ => format!("Time snaps back: {name}'s window closed"),
        },
    });
    if let Value::Object(o) = extra {
        for (k, x) in o {
            v[k] = x;
        }
    }
    v
}

/// Settle Time's slow windows (the core's closed windows, oldest first, and window-less orders) from this beat's
/// submissions, merge Time's penalties, and record everything.
///
/// `ordered`: per seat, the demigods that received orders this beat (rules 0.7.0: Time's debt is split evenly between
/// them, since the acting bodies owe; the bolt comes once from the player's purse). Empty for a seat: its lead owes.
pub fn process(
    w: &mut World,
    beat: u32,
    subs: &[Sub],
    closed: &[ClosedWindow],
    pantheon: &Arc<Pantheon>,
    st: &Settings,
    ordered: &BTreeMap<u8, Vec<DemigodId>>,
) -> FlowOut {
    let mut out = FlowOut::default();
    let time = pantheon.id_of("time").unwrap_or(GodId(0));
    let mut props: Vec<Proposal> = vec![];

    let mut by_window: BTreeMap<u64, Vec<&Sub>> = BTreeMap::new();
    for s in subs {
        if let Some(id) = s.window_id {
            by_window.entry(id).or_default().push(s);
        }
    }
    for v in by_window.values_mut() {
        v.sort_by_key(|s| s.seq);
    }
    for cw in closed {
        let id = cw.info.window_id;
        let acts = by_window.remove(&id).unwrap_or_default();
        let (by_seat, opened_ms, deadline_ms) = (cw.info.by_seat, cw.info.opened_ms, cw.info.deadline_ms);
        let Some(d) = demigod_of(w, by_seat) else {
            continue;
        };
        let enter = cw.reason == "submitted";
        // An Enter: until the core closed it. A timeout (or the match ending): the whole slow, to its deadline.
        let closed_ms = if enter { cw.closed_ms } else { deadline_ms.max(opened_ms) };
        let full = (deadline_ms - opened_ms).max(0);
        let ms_open = (closed_ms - opened_ms).clamp(0, full) as u64;
        let recent = recent_slows(w, d, beat);
        push_slow(w, d, beat);
        event(
            "slow_started",
            Some(Ref::Demigod(d).to_string()),
            json!({ "beat": beat, "window_id": id, "initiator": d, "by_seat": by_seat, "opened_ms": opened_ms,
                    "begun_ms": opened_ms, "deadline_ms": deadline_ms, "measured": "core", "recent_slows": recent }),
        );
        if ms_open >= st.flow_ms as u64 {
            for s in acts.iter().filter(|s| s.orders) {
                out.windowed.extend(w.siblings(s.demigod));
            }
        }
        let bill = bill(w, &mut props, time, d, Some(ms_open), beat, st, ordered.get(&by_seat));
        let how_s = match cw.reason.as_str() {
            "submitted" => "enter",
            "deadline" => "timeout",
            other => other,
        };
        event(
            "slow_closed",
            Some(Ref::Demigod(d).to_string()),
            json!({ "beat": beat, "window_id": id, "initiator": d, "by_seat": by_seat, "how": how_s,
                    "opened_ms": opened_ms, "closed_ms": closed_ms, "core_closed_ms": cw.closed_ms,
                    "deadline_ms": deadline_ms, "ms_open": ms_open, "measured": "core",
                    "submitted": cw.submitted, "engaged": cw.info.engaged, "dropped": cw.dropped,
                    "seqs": acts.iter().map(|s| s.seq).collect::<Vec<_>>(),
                    "debt": bill.debt, "entropy": bill.entropy, "recent_slows": bill.recent, "paid_bolt": bill.paid }),
        );
        // Public: who slowed, how it ended, the bill. Never who else engaged or was cut off.
        out.public.push(announce(
            w,
            "closed",
            d,
            json!({ "window_id": id, "how": how_s, "after_ms": ms_open, "debt": bill.debt,
                    "paid_bolt": bill.paid }),
        ));
        w.slow = None;
        w.slow_settled.push(SettledWindow { window_id: id, begun_ms: opened_ms });
        let n = w.slow_settled.len();
        if n > SETTLED_KEPT {
            w.slow_settled.drain(..n - SETTLED_KEPT);
        }
    }
    // Safety net: windowed orders whose window isn't in the context (the core delivers both together, so this
    // shouldn't happen) go through unbilled.
    for (id, acts) in by_window {
        for s in &acts {
            out.windowed.extend(w.siblings(s.demigod));
        }
        event(
            "slow_late_orders",
            None,
            json!({ "beat": beat, "window_id": id, "seats": acts.iter().map(|s| s.seat).collect::<Vec<_>>() }),
        );
    }

    // Orders without a window (Compose): an instant order, billed Time's flat `instant_order_debt`. One bill per
    // demigod.
    let mut billed: BTreeSet<DemigodId> = BTreeSet::new();
    let mut compose: Vec<&Sub> = subs.iter().filter(|s| s.window_id.is_none() && s.orders).collect();
    compose.sort_by_key(|s| (!s.ai, s.seq, s.demigod.0));
    for s in compose {
        if !billed.insert(s.demigod) || !w.demigods[s.demigod.ix()].alive {
            continue;
        }
        let d = s.demigod;
        let recent = recent_slows(w, d, beat);
        push_slow(w, d, beat);
        let bill = bill(w, &mut props, time, d, None, beat, st, ordered.get(&s.seat));
        event(
            "slow_started",
            Some(Ref::Demigod(d).to_string()),
            json!({ "beat": beat, "window_id": null, "initiator": d, "by_seat": (!s.ai).then_some(s.seat),
                    "opened_ms": s.at_ms, "measured": "compose", "recent_slows": recent }),
        );
        event(
            "slow_closed",
            Some(Ref::Demigod(d).to_string()),
            json!({ "beat": beat, "window_id": null, "initiator": d, "how": "compose", "opened_ms": s.at_ms,
                    "closed_ms": s.at_ms, "ms_open": 0, "measured": "compose", "seqs": [s.seq],
                    "debt": bill.debt, "entropy": bill.entropy, "recent_slows": bill.recent, "paid_bolt": bill.paid }),
        );
        out.public.push(announce(w, "instant", d, json!({ "after_ms": 0, "debt": bill.debt, "paid_bolt": bill.paid })));
    }

    if !props.is_empty() {
        merge::run(w, props, pantheon, beat, st.max_witnesses as usize);
    }
    out
}

/// The initiator's slows in the last [`SLOW_REPEAT_BEATS`] beats, before this one is recorded.
fn recent_slows(w: &World, d: DemigodId, beat: u32) -> u32 {
    w.demigods[d.ix()]
        .slows
        .iter()
        .filter(|b| **b + SLOW_REPEAT_BEATS > beat)
        .count() as u32
}

struct Bill {
    debt: f32,
    entropy: f32,
    recent: u32,
    paid: bool,
}

/// Time's bill for a window open `ms_open` milliseconds (`None`: an instant order, no window): a bolt (if there is
/// one; a toll, so no rival gains from it), a debt, Entropy. The debt is split evenly between `owers` (the player's
/// demigods that got orders; rules 0.7.0), else `d` owes it all.
#[allow(clippy::too_many_arguments)]
fn bill(
    w: &World,
    props: &mut Vec<Proposal>,
    time: GodId,
    d: DemigodId,
    ms_open: Option<u64>,
    beat: u32,
    st: &Settings,
    owers: Option<&Vec<DemigodId>>,
) -> Bill {
    let secs = ms_open.unwrap_or(0) as f32 / 1000.0;
    let dg = &w.demigods[d.ix()];
    let alive = dg.alive;
    // This slow is already in `slows`; repeats are the others in the window.
    let recent = recent_slows(w, d, beat).saturating_sub(1);
    let paid = alive && dg.bolts >= 1;
    let name = dg.name.clone();
    if paid {
        props.push(time_prop(
            time,
            Effect::Bolts {
                who: d,
                delta: -1,
                to: Some(time),
                toll: true,
            },
            format!("{name} pays Time a bolt to slow the world down"),
            d,
        ));
    }
    let base = match ms_open {
        Some(_) => SLOW_DEBT_BASE * (1.0 + secs / 10.0) * (1.0 + 0.5 * recent as f32),
        None => st.instant_order_debt,
    };
    let debt = base * if paid { 1.0 } else { 2.0 };
    let debt = (debt.min(10.0) * 100.0).round() / 100.0;
    let entropy = ((SLOW_ENTROPY + secs / 30.0) * 100.0).round() / 100.0;
    if alive {
        let who: Vec<DemigodId> = match owers {
            Some(v) if v.len() > 1 => v.iter().copied().filter(|x| w.demigods[x.ix()].alive).collect(),
            _ => vec![d],
        };
        let who = if who.is_empty() { vec![d] } else { who };
        let share = if who.len() > 1 { ((debt / who.len() as f32) * 100.0).round() / 100.0 } else { debt };
        for o in who {
            let oname = &w.demigods[o.ix()].name;
            props.push(time_prop(
                time,
                Effect::Debt {
                    owed_by: Ref::Demigod(o),
                    god: time,
                    amount: share,
                    due_in: st.debt_due_beats,
                },
                if secs < 0.05 {
                    format!("Time bills {} for orders sent without slowing the world", if o == d { &name } else { oname })
                } else {
                    format!("Time bills {} for {secs:.1} seconds of slowed time", if o == d { &name } else { oname })
                },
                o,
            ));
        }
    }
    props.push(time_prop(
        time,
        Effect::Entropy { delta: entropy },
        "The world lurches back up to speed, a little wackier".into(),
        d,
    ));
    Bill {
        debt: if alive { debt } else { 0.0 },
        entropy,
        recent,
        paid,
    }
}

fn time_prop(time: GodId, effect: Effect, act: String, d: DemigodId) -> Proposal {
    Proposal {
        source: Source::God { god: time, ruling: None },
        intent: None,
        effect,
        act,
        actor: Ref::God(time),
        magnitude: 3,
        valence: -1,
        witnesses: vec![Ref::Demigod(d)],
        cause: None,
    }
}

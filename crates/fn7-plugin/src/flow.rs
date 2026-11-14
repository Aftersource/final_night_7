//! Flow and slow, v0 (text entry is the God of Time interaction).
//!
//! The world's action never waits for players: every beat is a flow beat (`flow_ms`, the room's Compose window) that
//! resolves whether or not anyone typed, and the hands' standing orders keep the demigods acting. Typing is an overlay:
//!
//! - **Opening.** A seat that sends `fields.slow: true` (it started typing), or sends orders while no window is open,
//!   opens a slow-time window. It becomes the window's initiator; everyone is told who ("X slowed time"). Time takes one
//!   bolt for it (every other demigod gains 2). One window at a time.
//! - **While open** (up to `ceil(slow_ms / flow_ms)` flow beats, which keep resolving): orders other seats send are
//!   held in the state, hidden (no hand replies, nothing public), and go through when the window closes.
//! - **Enter.** The initiator's orders close the window in the beat they arrive. Held orders go through with them, and
//!   so do orders other seats sent in that same beat *before* the initiator's (by `fields.sent_at`); later ones, and
//!   orders tagged for a window that already closed, are cut off (`orders_cut_off`). Orders sent with the slow request
//!   itself close the window at once.
//! - **Penalty.** On close Time writes a debt that grows with every second the window stayed open (measured from the
//!   client timestamps, else from beats × `flow_ms`) and with the initiator's recent slows (doubled if it couldn't pay
//!   the bolt); Entropy rises. Pressing Enter early is cheap, and it cuts off rivals.
//!
//! What this v0 is not (the engine can't, today, and the plugin doesn't pretend): the core's Compose timer is fixed per
//! room and closes only when every seat has submitted or the deadline passes, the plugin can't push mid-phase, and a
//! client's "started typing" isn't forwarded. So a slow window is a game rule over the actions the core delivers each
//! flow beat, decided at Resolve; the real mechanic waits for engine proposal v2.2. Arrival order comes from the
//! client's `fields.sent_at` (the core delivers actions keyed by seat, without order); the plugin's own AI demigods
//! act at the start of each Compose and count as first.

use std::collections::BTreeSet;
use std::sync::Arc;

use fn7_model::*;
use serde_json::{json, Value};

use crate::beat::Turn;
use crate::merge;
use crate::settings::Settings;
use crate::world::event;
use crate::SYNTHETIC_SEAT;

/// Base of Time's slow debt (damage points), before seconds and repeats.
pub const SLOW_DEBT_BASE: f32 = 0.5;
/// Slows within this many beats count as repeats.
pub const SLOW_REPEAT_BEATS: u32 = 20;
/// Entropy a closed window adds (plus seconds open / 30).
pub const SLOW_ENTROPY: f32 = 0.5;

/// What the slow/flow stage decided this beat.
#[derive(Debug, Default)]
pub struct FlowOut {
    /// Public announcements, in order (`opened`, `open`, `closed`).
    pub public: Vec<Value>,
    /// Demigods whose orders are held this beat.
    pub held: BTreeSet<DemigodId>,
    /// Demigods whose orders were cut off this beat.
    pub cut_off: BTreeSet<DemigodId>,
    /// Demigods whose orders this beat closed (or were released by) a window open at least one beat: they get
    /// `max_intents_per_window` acts per hand.
    pub windowed: BTreeSet<DemigodId>,
}

/// Window length in beats.
pub fn window_beats(st: &Settings) -> u32 {
    st.slow_ms.div_ceil(st.flow_ms.max(1)).max(1)
}

/// Arrival key: the plugin's AI first, then by client timestamp (none = after everyone with one), then demigod.
fn key(t: &Turn) -> (i64, u32) {
    let at = if t.seat >= SYNTHETIC_SEAT {
        i64::MIN
    } else {
        t.input.sent_at.unwrap_or(i64::MAX)
    };
    (at, t.demigod.0)
}

/// Did `other` send before `enter` (the initiator's Enter)?
fn before(other: &Turn, enter: Option<i64>) -> bool {
    if other.seat >= SYNTHETIC_SEAT {
        return true;
    }
    match (other.input.sent_at, enter) {
        (Some(a), Some(b)) => a < b,
        // The initiator has no timestamp: nothing can be ordered against it.
        (_, None) => true,
        // A seat without a timestamp can't prove it came first.
        (None, Some(_)) => false,
    }
}

fn announce(w: &World, ev: &str, win: &SlowWindow, beat: u32, st: &Settings, extra: Value) -> Value {
    let name = w.demigods[win.initiator.ix()].name.clone();
    let beats_left = win.deadline_beat.saturating_sub(beat);
    let mut v = json!({
        "event": ev,
        "by": name,
        "by_ref": Ref::Demigod(win.initiator),
        "window": win.opened_beat,
        "closes_by_beat": win.deadline_beat,
        "beats_left": beats_left,
        "ms_left": beats_left as u64 * st.flow_ms as u64,
        "text": match ev {
            "opened" | "open" => format!("{name} slowed time"),
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

/// Run the slow/flow rules over this beat's turns (mutating which orders go through), merge Time's penalties, and
/// record everything.
pub fn process(
    w: &mut World,
    beat: u32,
    turns: &mut [Turn],
    pantheon: &Arc<Pantheon>,
    st: &Settings,
) -> FlowOut {
    let mut out = FlowOut::default();
    let time = pantheon.id_of("time").unwrap_or(GodId(0));
    let mut props: Vec<Proposal> = vec![];
    let mut order: Vec<usize> = (0..turns.len()).collect();
    order.sort_by_key(|&i| key(&turns[i]));

    // Orders tagged for a window that already closed are late: cut off.
    let open_id = w.slow.as_ref().map(|s| s.opened_beat);
    for t in turns.iter_mut() {
        if let Some(win) = t.input.window {
            if t.input.has_orders() && win < beat && Some(win) != open_id {
                cut(t, &mut out, beat, win, "late: the window already closed", None);
            }
        }
    }

    // Open a window if none is open: the first seat (arrival order) that asked to slow time or sent orders.
    let mut opened_now = false;
    if w.slow.is_none() {
        if let Some(&i) = order
            .iter()
            .find(|&&i| turns[i].input.slow || turns[i].input.has_orders())
        {
            let t = &turns[i];
            let d = t.demigod;
            let paid = w.demigods[d.ix()].bolts >= 1;
            let win = SlowWindow {
                initiator: d,
                opened_beat: beat,
                deadline_beat: beat + window_beats(st),
                opened_at: t.input.sent_at,
                paid,
                held: vec![],
            };
            if paid {
                props.push(time_prop(
                    w,
                    time,
                    Effect::Bolts {
                        who: d,
                        delta: -1,
                        to: Some(time),
                    },
                    format!("{} pays Time a bolt to slow the world down", w.demigods[d.ix()].name),
                    d,
                ));
            }
            w.demigods[d.ix()].slows.push(beat);
            event(
                "slow_started",
                Some(Ref::Demigod(d).to_string()),
                json!({ "beat": beat, "initiator": d, "window": beat, "sent_at": t.input.sent_at,
                        "deadline_beat": win.deadline_beat, "paid_bolt": paid,
                        "with_orders": t.input.has_orders() }),
            );
            out.public
                .push(announce(w, "opened", &win, beat, st, json!({})));
            w.slow = Some(win);
            opened_now = true;
        }
    }

    // Refuse other slow requests while a window is open (one at a time).
    if let Some(win) = w.slow.clone() {
        for t in turns.iter().filter(|t| t.input.slow && t.demigod != win.initiator) {
            event(
                "slow_refused",
                Some(Ref::Demigod(t.demigod).to_string()),
                json!({ "beat": beat, "demigod": t.demigod, "window": win.opened_beat,
                        "reason": "time is already slowed" }),
            );
        }
    }

    let Some(mut win) = w.slow.clone() else {
        merge_props(w, props, pantheon, beat, st);
        return out;
    };
    let initiator_alive = w.demigods[win.initiator.ix()].alive;
    let enter_ix = turns
        .iter()
        .position(|t| t.demigod == win.initiator && t.input.has_orders());
    let enter_at = enter_ix.and_then(|i| turns[i].input.sent_at);
    let how = if enter_ix.is_some() {
        Some("enter")
    } else if !initiator_alive {
        Some("fell")
    } else if beat >= win.deadline_beat {
        Some("timeout")
    } else {
        None
    };

    // Other seats' orders this beat: held while the window stays open; at close, through if sent before the Enter.
    for t in turns.iter_mut() {
        if t.demigod == win.initiator || !t.input.has_orders() || out.cut_off.contains(&t.demigod) {
            continue;
        }
        match how {
            Some("enter") if !before(t, enter_at) => {
                cut(t, &mut out, beat, win.opened_beat, "after the initiator's Enter", enter_at)
            }
            Some(_) => {}
            None => {
                win.held.retain(|h| h.demigod != t.demigod);
                win.held.push(HeldOrders {
                    demigod: t.demigod,
                    left: std::mem::take(&mut t.input.left),
                    right: std::mem::take(&mut t.input.right),
                    beat,
                    sent_at: t.input.sent_at,
                });
                out.held.insert(t.demigod);
                event(
                    "orders_held",
                    Some(Ref::Demigod(t.demigod).to_string()),
                    json!({ "beat": beat, "demigod": t.demigod, "window": win.opened_beat,
                            "sent_at": t.input.sent_at }),
                );
            }
        }
    }

    let Some(how) = how else {
        if !opened_now {
            out.public.push(announce(w, "open", &win, beat, st, json!({})));
        }
        w.slow = Some(win);
        merge_props(w, props, pantheon, beat, st);
        return out;
    };

    // Close: release the held orders (merged in front of anything the same seat sent this beat).
    let released: Vec<DemigodId> = win.held.iter().map(|h| h.demigod).collect();
    for h in std::mem::take(&mut win.held) {
        if let Some(t) = turns.iter_mut().find(|t| t.demigod == h.demigod) {
            t.input.left = join(&h.left, &t.input.left);
            t.input.right = join(&h.right, &t.input.right);
        }
        out.windowed.insert(h.demigod);
    }
    if !released.is_empty() {
        event(
            "orders_released",
            None,
            json!({ "beat": beat, "window": win.opened_beat, "demigods": released }),
        );
    }
    let beats_open = beat - win.opened_beat;
    if how == "enter" && beats_open > 0 {
        out.windowed.insert(win.initiator);
    }
    let (ms_open, measured) = match (win.opened_at, enter_at) {
        (Some(a), Some(b)) if b >= a && b - a <= 2 * st.slow_ms as i64 => ((b - a) as u64, "client"),
        _ => (
            if how == "timeout" {
                st.slow_ms as u64
            } else {
                beats_open as u64 * st.flow_ms as u64
            },
            "beats",
        ),
    };
    let secs = ms_open as f32 / 1000.0;
    let d = win.initiator;
    let recent = w.demigods[d.ix()]
        .slows
        .iter()
        .filter(|b| **b + SLOW_REPEAT_BEATS > beat && **b != win.opened_beat)
        .count() as f32;
    let debt = SLOW_DEBT_BASE * (1.0 + secs / 10.0) * (1.0 + 0.5 * recent) * if win.paid { 1.0 } else { 2.0 };
    let debt = (debt.min(10.0) * 100.0).round() / 100.0;
    let entropy = ((SLOW_ENTROPY + secs / 30.0) * 100.0).round() / 100.0;
    let name = w.demigods[d.ix()].name.clone();
    if initiator_alive {
        props.push(time_prop(
            w,
            time,
            Effect::Debt {
                owed_by: Ref::Demigod(d),
                god: time,
                amount: debt,
                due_in: st.debt_due_beats,
            },
            if secs < 0.05 {
                format!("Time bills {name} for orders sent without slowing the world")
            } else {
                format!("Time bills {name} for {secs:.1} seconds of slowed time")
            },
            d,
        ));
    }
    props.push(time_prop(
        w,
        time,
        Effect::Entropy { delta: entropy },
        "The world lurches back up to speed, a little wackier".into(),
        d,
    ));
    event(
        "slow_closed",
        Some(Ref::Demigod(d).to_string()),
        json!({ "beat": beat, "initiator": d, "window": win.opened_beat, "how": how,
                "beats_open": beats_open, "ms_open": ms_open, "measured": measured,
                "opened_at": win.opened_at, "enter_at": enter_at,
                "released": released.len(), "cut_off": out.cut_off.len(),
                "debt": debt, "entropy": entropy, "recent_slows": recent as u32, "paid_bolt": win.paid }),
    );
    out.public.push(announce(
        w,
        "closed",
        &win,
        beat,
        st,
        json!({ "how": how, "after_ms": ms_open, "beats_left": 0, "ms_left": 0 }),
    ));
    w.slow = None;
    merge_props(w, props, pantheon, beat, st);
    out
}

fn join(a: &str, b: &str) -> String {
    match (a.trim().is_empty(), b.trim().is_empty()) {
        (true, _) => b.to_string(),
        (_, true) => a.to_string(),
        _ => format!("{a}\n{b}"),
    }
}

fn cut(t: &mut Turn, out: &mut FlowOut, beat: u32, window: u32, reason: &str, enter_at: Option<i64>) {
    t.input.left.clear();
    t.input.right.clear();
    out.cut_off.insert(t.demigod);
    event(
        "orders_cut_off",
        Some(Ref::Demigod(t.demigod).to_string()),
        json!({ "beat": beat, "demigod": t.demigod, "window": window, "reason": reason,
                "sent_at": t.input.sent_at, "enter_at": enter_at }),
    );
}

fn time_prop(w: &World, time: GodId, effect: Effect, act: String, d: DemigodId) -> Proposal {
    let _ = w;
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

fn merge_props(w: &mut World, props: Vec<Proposal>, pantheon: &Arc<Pantheon>, beat: u32, st: &Settings) {
    if !props.is_empty() {
        merge::run(w, props, pantheon, beat, st.max_witnesses as usize);
    }
}

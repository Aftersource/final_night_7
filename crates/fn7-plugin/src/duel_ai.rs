//! AI duellists type (state schema 12): a duel against an AI reads like a duel against a human.
//!
//! The designer, duelling an AI in the live game, could not see the AI typing, and said the duel view should show the opponent's text
//! on the top ... top = opponents *****p's, bottom is your words, enter to enter". An AI fighter has no client, so
//! nothing ever showed it typing and it only struck as an automatic counter.
//!
//! In a duel with a clock (panels on) an AI fighter now **acts in every input window**:
//!
//! - when a window opens (the lock for window 0, then every window close: a `duel_tick`, a duel line that closes
//!   windows, or a resolve) the rules pick its move class ([`choose`]: the range, the initiative, the stances, the
//!   defence the opponent has set, the opponent's last move, `duel_ai_aggression`) and a line of its own for it
//!   ([`line`]: a sentence in its voice naming a body part, a scene object or its rival), seeded and deterministic;
//! - the line goes out at once as a **typing script** ([`Script`]: `duel.typing_script`, also on the panel that opens
//!   the window as `a|b.typing_script`, and in `present.duels[].typing_scripts`): the text, when it starts, its
//!   speed (6 to 9 characters a second, seeded per demigod), its pauses and when it presses Enter (`ends_at_ms`, at
//!   least [`GUARD_MS`] before the window's `input_closes_ms`). Every client plays the same masked letters at the
//!   same moments; the server sends no frames and has no timer;
//! - at `ends_at_ms` the line is a blow, resolved by the same d20 and matrix as a human's
//!   ([`crate::duel::Bout::ai_blow`]): it is struck when the first duel action stamped at or after it arrives, or
//!   when its window closes, whichever is processed first. Both depend only on stamps, so the live view, the resolve
//!   and a replay strike the same blow at the same place in the order.
//!
//! **The window fits the line.** A 20-character line at 6 characters a second does not fit a 2.4 s panel, so the
//! panel that opens a window an AI types in plays as long as the line needs ([`open_window`]: `play_ms` up to about
//! 5.5 s instead of 2.4 to 3.6 s; the staged strikes keep their timing, the rest is guard). A window too close to the
//! deadline to hold a line gets none (the 0.8.1 automatic counter still answers there).
//!
//! **No double strike.** An AI that typed a line for a window does not also counter automatically in it, and its
//! typed line never draws an automatic counter from an idle human: whoever types nothing only gets hit.
//!
//! Everything is in the bout (`Bout::ai`: the scripts not yet entered), so resume, restart, replay and fork carry it.
//! A state before schema 12, a duel with no clock (a simulation) or `duel_ai_lines_per_window: 0` has none of this.

use fn7_model::seed::{hash_str, Rng};
use fn7_model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::choreo::{Flow, MoveClass, Range, Stance};
use crate::duel::{Bout, Duel};
use crate::settings::Settings;

/// A script's Enter (`ends_at_ms`) is at least this long before its window's `input_closes_ms`.
pub const GUARD_MS: i64 = 400;
/// A human silent this long (since the lock or its last line) is idle: the AI's handicap is waived against it.
pub const IDLE_MS: i64 = 12_000;
/// The shortest and the longest line an AI types (characters).
pub const MIN_CHARS: usize = 20;
pub const MAX_CHARS: usize = 70;
/// The typing time a line may take (ms): its length is capped at this many ms of the typist's speed.
const TYPE_BUDGET_MS: i64 = 3900;
const SALT: u64 = 0xA1D0;

/// One typing pause: after `after` characters are out, the next one comes `ms` later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pause {
    pub after: u32,
    pub ms: i64,
}

/// An AI's line for one input window, as it will be typed (server clock, epoch ms).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Script {
    /// 0 = a (the challenger), 1 = b.
    pub side: usize,
    pub demigod: DemigodId,
    /// The panel whose input window it is typed in.
    pub window: u32,
    pub text: String,
    pub starts_at_ms: i64,
    /// Ms per character (`round(1000 / cps)`).
    pub key_ms: i64,
    pub cps: f64,
    pub pauses: Vec<Pause>,
    /// Enter: the line becomes a blow.
    pub ends_at_ms: i64,
    pub input_closes_ms: i64,
}

impl Script {
    /// When character `k` (1-based) appears.
    pub fn char_at_ms(&self, k: usize) -> i64 {
        self.starts_at_ms + k as i64 * self.key_ms + self.pauses.iter().filter(|p| (p.after as usize) < k).map(|p| p.ms).sum::<i64>()
    }

    /// Characters out at `at_ms` (what a `duel.typing` frame's `len` would be).
    pub fn len_at(&self, at_ms: i64) -> usize {
        let n = self.text.chars().count();
        (1..=n).take_while(|k| self.char_at_ms(*k) <= at_ms).count()
    }

    /// The public `duel.typing_script` event (the same object rides on its panel as `a|b.typing_script`).
    pub fn json(&self, duel_id: &str) -> Value {
        json!({ "event": "duel.typing_script", "duel_id": duel_id, "by": if self.side == 0 { "a" } else { "b" },
                "demigod": Ref::Demigod(self.demigod), "window": self.window, "text": self.text,
                "len": self.text.chars().count(), "starts_at_ms": self.starts_at_ms, "cps": self.cps, "key_ms": self.key_ms,
                "pauses": self.pauses, "ends_at_ms": self.ends_at_ms, "input_closes_ms": self.input_closes_ms })
    }
}

/// Where a bout's AI typing stands (kept in the bout; absent = the AI does not type in this duel).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    /// The scripts of the open window not yet entered.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending: Vec<Script>,
    /// Lines the AI entered so far.
    #[serde(default)]
    pub struck: u32,
    /// The newest window each side typed a line for.
    #[serde(default)]
    pub window: [Option<u32>; 2],
}

/// Does an AI type in this duel? Schema 12, the setting on, a clock (a stamped lock) and an AI fighter.
pub fn enabled(w: &World, st: &Settings, d: &Duel) -> bool {
    w.ai_duel_rules() && st.duel_ai_lines_per_window > 0.0 && d.started_ms > 0 && d.f.iter().any(|f| f.ai)
}

/// A demigod's typing speed in this match: 6.0 to 9.0 characters a second (tenths), and the ms per character.
pub fn typist(seed: u64, d: DemigodId) -> (f64, i64) {
    let cps10 = 60 + Rng::derive(seed, &[SALT, d.0 as u64, 0xC95]).below(31) as i64;
    (cps10 as f64 / 10.0, (10_000 + cps10 / 2) / cps10)
}

/// The longest line a typist of `key_ms` is given (so its window stays near 5 s).
pub fn cap(key_ms: i64) -> usize {
    ((TYPE_BUDGET_MS / key_ms.max(1)) as usize).clamp(MIN_CHARS + 2, 44)
}

// ---------------------------------------------------------------------------------------------------------------------
// The move
// ---------------------------------------------------------------------------------------------------------------------

/// What the opponent did last: its move class and whether it hurt.
fn last_move(b: &Bout, foe: usize) -> Option<(MoveClass, bool)> {
    let who = json!(Ref::Demigod(b.duel.f[foe].id));
    let name = if foe == 0 { "a" } else { "b" };
    let of = |ev: &Value| -> Option<(MoveClass, bool)> {
        let class = serde_json::from_value::<MoveClass>(ev["choreo"][name]["move_class"].clone()).ok()?;
        Some((class, ev["damage"].as_f64().unwrap_or(0.0) > 0.0))
    };
    b.blows.iter().rev().find_map(|ev| {
        if ev["counter"]["by"] == who {
            of(&ev["counter"])
        } else if ev["by"] == who {
            of(ev)
        } else {
            None
        }
    })
}

/// The AI's move for a window, by rules: what the range allows, what the opponent's stance and set defence invite,
/// how shaken it is itself, and `aggression` (0 = cautious, 1 = it only swings). Seeded by `r`.
pub fn choose(f: &Flow, me: usize, foe_last: Option<MoveClass>, hurt: bool, army: bool, aggression: f32, r: &mut Rng) -> MoveClass {
    let foe = 1 - me;
    let a = aggression.clamp(0.0, 1.0) as f64;
    let (x, y) = (r.f64(), r.f64());
    // A strike from far is a step in anyway: say so. In a grapple, throw.
    if f.range == Range::Far {
        return MoveClass::CloseIn;
    }
    if f.range == Range::Grapple {
        return if x < 0.5 + 0.4 * a { MoveClass::Throw } else { MoveClass::Strike };
    }
    // The opponent is open (it committed to a heavy, was parried, is down): hit it hard.
    if matches!(f.stance[foe], Stance::Committed | Stance::OffBalance | Stance::Downed) {
        return if x < 0.3 + 0.5 * a { MoveClass::Heavy } else { MoveClass::Strike };
    }
    // The opponent has set a defence: break it (a heavy through a guard or a parry) or waste it (a feint).
    match f.set[foe] {
        Some(MoveClass::Guard) => return if x < 0.3 + 0.5 * a { MoveClass::Heavy } else { MoveClass::Feint },
        Some(MoveClass::Parry) => return if x < 0.5 * a { MoveClass::Heavy } else { MoveClass::Feint },
        Some(MoveClass::Evade) => return if x < 0.5 { MoveClass::Feint } else { MoveClass::Strike },
        _ => {}
    }
    // An opponent who has done nothing yet is pressed: no defence, no taunt.
    let Some(foe_did) = foe_last else {
        return if y < 0.2 + 0.3 * a { MoveClass::Heavy } else { MoveClass::Strike };
    };
    let foe_last = Some(foe_did);
    // Shaken, hurt or just swung at: set a defence that answers the opponent's last move.
    let shaken = matches!(f.stance[me], Stance::OffBalance | Stance::Committed) as u8 as f64;
    let defend = (1.0 - a) * (0.25 + 0.25 * shaken + 0.15 * hurt as u8 as f64);
    if x < defend && f.set[me].is_none() {
        return match foe_last {
            Some(MoveClass::Heavy) => MoveClass::Evade,
            Some(MoveClass::Strike) => MoveClass::Parry,
            _ if y < 0.5 => MoveClass::Guard,
            _ => MoveClass::Parry,
        };
    }
    if f.range == Range::Close && y < 0.06 + 0.12 * a {
        // Take hold, or (as often) throw outright.
        if x < 0.5 { MoveClass::Grapple } else { MoveClass::Throw }
    } else if army && y > 0.9 {
        MoveClass::Command
    } else if f.initiative != me && y < 0.22 {
        MoveClass::Taunt
    } else if y < 0.22 + 0.28 * a {
        MoveClass::Heavy
    } else if f.range == Range::Close && y > 0.97 - 0.1 * (1.0 - a) {
        MoveClass::Retreat
    } else {
        MoveClass::Strike
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The line
// ---------------------------------------------------------------------------------------------------------------------

const PARTS: &[&str] = &["jaw", "ribs", "knee", "nose", "shin", "chin", "ego", "temple"];
const HEADS: &[&str] = &["jaw", "nose", "chin", "temple", "ear"];
const LEGS: &[&str] = &["knee", "shin", "ankle"];

/// The sentences of each move class (`{part}` a body part, `{head}` / `{leg}` one of the head or the leg, `{obj}` a
/// scene object, `{foe}` the rival's first name). Every one classifies as its class ([`choreo::classify`]);
/// [`rules_line`] checks the filled line again.
fn cores(class: MoveClass) -> &'static [&'static str] {
    match class {
        MoveClass::Strike => &["I kick your {leg}", "I jab your {head} twice", "My fist finds your {part}", "I bounce your {head} off {obj}",
                               "I stomp on your {leg}", "I hook your {head}, {foe}", "My elbow meets your {head}"],
        MoveClass::Heavy => &["I smash your {part}", "Full force on your {part}", "I crush you against {obj}",
                              "An overhead onto your {head}", "I headbutt your {head}", "A haymaker to your {head}"],
        MoveClass::Guard => &["I brace behind {obj}", "My guard is up, {foe}", "I block whatever comes", "I shield my {head}"],
        MoveClass::Parry => &["I parry and wait for you", "I deflect it past my ear", "I counter off the back foot", "I parry, {foe}. Try again"],
        MoveClass::Feint => &["I feint at your {part}", "A fake left, then right", "I bluff a swing at you", "I pretend to stumble"],
        MoveClass::Evade => &["I sidestep around {obj}", "I duck under your arm", "I weave out of reach", "I vanish behind {obj}",
                              "I dodge left, {foe}"],
        MoveClass::CloseIn => &["I rush you past {obj}", "I close in on you, {foe}", "I advance, eyes on you", "I charge straight at you",
                                "I step in, fists up"],
        MoveClass::Retreat => &["I step back behind {obj}", "I back off and breathe", "I withdraw two paces", "I fall back to {obj}"],
        MoveClass::Grapple => &["I grab your arm, {foe}", "I seize your wrist hard", "I clinch and hang on", "I grab you by the collar"],
        MoveClass::Throw => &["I throw you into {obj}", "I hurl you at {obj}", "I toss you on your back", "I suplex you onto {obj}",
                              "I throw you over my hip"],
        MoveClass::Command => &["My legion, bury you now", "My faithful, swarm {foe}", "My army answers me, {foe}", "Soldiers, take the {leg}"],
        MoveClass::Taunt | MoveClass::Invoke => &["I yawn at you, {foe}", "I laugh at your stance", "I mock your footwork", "I sneer at your {part}"],
    }
}

/// A line that always fits (20 to 22 characters, no slots) and always classifies as its class.
fn fallback(class: MoveClass) -> &'static str {
    match class {
        MoveClass::Strike => "I punch your left ear.",
        MoveClass::Heavy => "I smash you down flat.",
        MoveClass::Guard => "I brace and guard now.",
        MoveClass::Parry => "I parry and then wait.",
        MoveClass::Feint => "I feint to your left.",
        MoveClass::Evade => "I sidestep your swing.",
        MoveClass::CloseIn => "I close in on you now.",
        MoveClass::Retreat => "I step back two paces.",
        MoveClass::Grapple => "I grab you by the arm.",
        MoveClass::Throw => "I throw you in the mud",
        MoveClass::Command => "My army, onto you now.",
        MoveClass::Taunt | MoveClass::Invoke => "I yawn. Is that all?",
    }
}

/// The four voices (seeded by the demigod's name): what it adds after a line.
const VOICES: &[(&str, &[&str])] = &[
    ("brash", &["Ha!", "Too easy.", "Stay down.", "Again!"]),
    ("cold", &["Hold still.", "As planned.", "Predictable."]),
    ("sly", &["Did that sting?", "Surprise.", "Look at me."]),
    ("grim", &["No way out.", "Bleed.", "It ends here."]),
];

/// The voice of a demigod: `brash`, `cold`, `sly` or `grim`.
pub fn voice(name: &str) -> &'static str {
    VOICES[(hash_str(name) % VOICES.len() as u64) as usize].0
}

/// What it says first, to the opponent's last move.
fn opener(foe_last: Option<(MoveClass, bool)>) -> Option<&'static str> {
    Some(match foe_last? {
        (MoveClass::Heavy, _) => "Too slow.",
        (MoveClass::Parry | MoveClass::Guard, _) => "Hiding?",
        (MoveClass::Evade, _) => "Stand still!",
        (MoveClass::Taunt, _) => "Cute.",
        (MoveClass::Feint, _) => "Nice try.",
        (MoveClass::Grapple | MoveClass::Throw, _) => "Hands off!",
        (MoveClass::CloseIn | MoveClass::Retreat, _) => "Come on, then.",
        (_, true) => "Is that all?",
        (_, false) => "Missed me.",
    })
}

/// What a line is written from.
pub struct LineCtx<'a> {
    pub duel: &'a Duel,
    pub side: usize,
    pub class: MoveClass,
    pub flow: &'a Flow,
    /// The kinds of the scene's objects (`crate`, `pillar`, ...).
    pub objects: Vec<String>,
    pub foe_last: Option<(MoveClass, bool)>,
    /// The longest line this typist is given.
    pub cap: usize,
}

fn fits(text: &str, class: MoveClass, cap: usize) -> bool {
    let n = text.chars().count();
    (MIN_CHARS..=cap.min(MAX_CHARS)).contains(&n) && crate::moves::classify(text) == class
}

/// >>> LLM HOOK <<<
///
/// An LLM-authored line for this window would replace the rules line HERE. It must come from a recorded side input
/// (authored ahead of the window, e.g. at the beat's Start or by the server beside the plugin, and fed back on
/// replay like `duel_clock`), never from a call inside this function: the line is computed inside pure hooks
/// (`on_instant`, the resolve, the live view) and must be the same in all of them. Return `Some(text)` (20 to
/// `ctx.cap` characters; any move class: the blow is resolved from the text as a human's is) to use it. Not built:
/// the rules line ships.
fn authored_line(_ctx: &LineCtx) -> Option<String> {
    None
}

/// The AI's line for a window: the authored one if there is one and it fits, else the rules line.
pub fn line(ctx: &LineCtx, r: &mut Rng) -> String {
    match authored_line(ctx) {
        Some(t) if (MIN_CHARS..=ctx.cap.min(MAX_CHARS)).contains(&t.chars().count()) => t,
        _ => rules_line(ctx, r),
    }
}

/// The rules line: a sentence of the move class in the demigod's voice (`{opener} {core}. {closer}`), 20 to `cap`
/// characters, that classifies as the class. Seeded by `r`.
pub fn rules_line(ctx: &LineCtx, r: &mut Rng) -> String {
    let (me, foe) = (&ctx.duel.f[ctx.side], &ctx.duel.f[1 - ctx.side]);
    let closers = VOICES[(hash_str(&me.name) % VOICES.len() as u64) as usize].1;
    let part = PARTS[r.below(PARTS.len() as u64) as usize];
    let head = HEADS[r.below(HEADS.len() as u64) as usize];
    let leg = LEGS[r.below(LEGS.len() as u64) as usize];
    let obj = (!ctx.objects.is_empty()).then(|| format!("the {}", ctx.objects[r.below(ctx.objects.len() as u64) as usize]));
    let first = foe.name.split(['\'', ' ']).next().unwrap_or("").to_string();
    let first = Some(first).filter(|n| (2..=10).contains(&n.chars().count()) && n.chars().all(char::is_alphabetic));
    let open = opener(ctx.foe_last);
    let closer = closers[r.below(closers.len() as u64) as usize];
    let (want_open, want_close) = (r.below(2) == 0, r.below(2) == 0);
    // The cores in a seeded order; for each, the dressings in this window's order of preference.
    let mut order: Vec<&str> = cores(ctx.class).to_vec();
    for k in (1..order.len()).rev() {
        order.swap(k, r.below(k as u64 + 1) as usize);
    }
    for core in order {
        let Some(mut text) = (match (core.contains("{obj}"), core.contains("{foe}")) {
            (true, _) if obj.is_none() => None,
            (_, true) if first.is_none() => None,
            _ => Some(core.to_string()),
        }) else {
            continue;
        };
        text = text.replace("{part}", part).replace("{head}", head).replace("{leg}", leg).replace("{obj}", obj.as_deref().unwrap_or(""))
            .replace("{foe}", first.as_deref().unwrap_or(""));
        text.push('.');
        for (o, c) in [(want_open, want_close), (want_open, !want_close), (!want_open, want_close), (!want_open, !want_close)] {
            let mut s = String::new();
            if let (true, Some(x)) = (o, open) {
                s.push_str(x);
                s.push(' ');
            }
            s.push_str(&text);
            if c {
                s.push(' ');
                s.push_str(closer);
            }
            if fits(&s, ctx.class, ctx.cap) {
                return s;
            }
        }
    }
    fallback(ctx.class).to_string()
}

// ---------------------------------------------------------------------------------------------------------------------
// The script and the window
// ---------------------------------------------------------------------------------------------------------------------

/// The scripts of the window that opens at `opens_ms` (window number = its panel's `n`): one per AI fighter that
/// types in it (`duel_ai_lines_per_window`). `input_closes_ms` is set by [`open_window`].
fn plan(b: &Bout, st: &Settings, window: u32, opens_ms: i64) -> Vec<Script> {
    let d = &b.duel;
    let objects: Vec<String> = b
        .scene
        .as_ref()
        .and_then(|s| s["objects"].as_array())
        .map(|a| a.iter().filter_map(|o| o["kind"].as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let mut out = vec![];
    for side in 0..2 {
        let me = &d.f[side];
        if !me.ai {
            continue;
        }
        let mut r = Rng::derive(d.seed, &[SALT, hash_str(&d.id), window as u64, side as u64]);
        if r.f64() >= st.duel_ai_lines_per_window as f64 {
            continue;
        }
        let foe_last = last_move(b, 1 - side);
        let hurt = b.hp[side] <= 3.0;
        let class = choose(&b.flow, side, foe_last.map(|x| x.0), hurt, me.army, st.duel_ai_aggression, &mut r);
        let (cps, key_ms) = typist(d.seed, me.id);
        let ctx = LineCtx { duel: d, side, class, flow: &b.flow, objects: objects.clone(), foe_last, cap: cap(key_ms) };
        let text = line(&ctx, &mut r);
        let n = text.chars().count();
        // It thinks, types (pausing once or twice between words), then presses Enter.
        let starts_at_ms = opens_ms + 120 + r.below(181) as i64;
        let mut gaps: Vec<u32> = text.chars().enumerate().filter(|(i, c)| *c == ' ' && *i >= 3 && *i + 4 < n).map(|(i, _)| i as u32 + 1).collect();
        let mut pauses = vec![];
        for _ in 0..r.below(3) {
            if gaps.is_empty() {
                break;
            }
            let after = gaps.remove(r.below(gaps.len() as u64) as usize);
            pauses.push(Pause { after, ms: 80 + r.below(101) as i64 });
        }
        pauses.sort_by_key(|p| p.after);
        let typed: i64 = n as i64 * key_ms + pauses.iter().map(|p| p.ms).sum::<i64>();
        let ends_at_ms = starts_at_ms + typed + 120 + r.below(121) as i64;
        out.push(Script { side, demigod: me.id, window, text, starts_at_ms, key_ms, cps, pauses, ends_at_ms, input_closes_ms: 0 });
    }
    out
}

/// Open window `window` for the AI: the panel that opens it starts at `starts_ms` and would play `base_ms`; its
/// input opened at `opens_ms`. Returns the scripts typed in it and how long the panel plays so that every script
/// ends [`GUARD_MS`] before the input closes (`play_ms − LEAD_MS` after the panel starts): at least `base_ms`.
/// A window that would then reach the deadline gets no script and keeps `base_ms`.
pub fn open_window(b: &Bout, st: &Settings, window: u32, opens_ms: i64, starts_ms: i64, base_ms: i64) -> (Vec<Script>, i64) {
    if b.ai.is_none() || b.ko.is_some() {
        return (vec![], base_ms);
    }
    let mut scripts = plan(b, st, window, opens_ms);
    let Some(last) = scripts.iter().map(|s| s.ends_at_ms).max() else { return (vec![], base_ms) };
    let play = base_ms.max(last + GUARD_MS + crate::panel::LEAD_MS - starts_ms);
    let deadline = b.duel.deadline_ms;
    if deadline > 0 && starts_ms + play >= deadline {
        return (vec![], base_ms);
    }
    for s in scripts.iter_mut() {
        s.input_closes_ms = starts_ms + play - crate::panel::LEAD_MS;
    }
    (scripts, play)
}

/// Enter every pending script whose Enter has come by `now_ms` (`None`: its window is closing: all of them), in
/// order: each is a blow ([`Bout::ai_blow`]).
pub fn enter_due(b: &mut Bout, w: &World, st: &Settings, now_ms: Option<i64>) {
    loop {
        let Some(p) = b.ai.as_mut() else { return };
        let due = p
            .pending
            .iter()
            .enumerate()
            .filter(|(_, s)| now_ms.is_none_or(|t| s.ends_at_ms <= t))
            .min_by_key(|(_, s)| (s.ends_at_ms, s.side))
            .map(|(i, _)| i);
        let Some(i) = due else { return };
        let s = p.pending.remove(i);
        b.ai_blow(w, st, &s);
    }
}

/// The window closes with nothing more entered (the duel was ended by a resolve: nobody can answer any more).
pub fn drop_pending(b: &mut Bout) {
    if let Some(p) = b.ai.as_mut() {
        p.pending.clear();
    }
}

/// Did `side` type a line for the window open now (so it does not also counter automatically in it)?
pub fn typed_this_window(b: &Bout, side: usize) -> bool {
    let now = b.panels.len().checked_sub(1).map(|n| n as u32);
    now.is_some() && b.ai.as_ref().is_some_and(|p| p.window[side] == now)
}

/// The scripts being typed now, as `duel.typing_script` events (`present.duels[].typing_scripts`).
pub fn pending_json(b: &Bout) -> Vec<Value> {
    b.ai.as_ref().map(|p| p.pending.iter().map(|s| s.json(&b.duel.id)).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [MoveClass; 12] = [MoveClass::Strike, MoveClass::Heavy, MoveClass::Guard, MoveClass::Parry, MoveClass::Feint, MoveClass::Evade,
                                  MoveClass::CloseIn, MoveClass::Retreat, MoveClass::Grapple, MoveClass::Throw, MoveClass::Taunt, MoveClass::Command];

    #[test]
    fn every_line_is_a_sentence_of_its_class_that_fits() {
        let w = crate::testkit::world(4, 3);
        let d = Duel::new(&w, 1, DemigodId(0), DemigodId(2), "regions", None, 1_000, &[]);
        let flow = Flow::new("regions");
        for class in ALL {
            let f = fallback(class);
            assert!(fits(f, class, MIN_CHARS + 2), "the fallback of {class:?}: {f:?} ({} chars)", f.chars().count());
            for core in cores(class) {
                for (part, head, leg) in [("temple", "temple", "ankle"), ("ego", "ear", "knee"), ("ribs", "jaw", "shin"), ("chin", "nose", "shin")] {
                    let filled = core.replace("{part}", part).replace("{head}", head).replace("{leg}", leg).replace("{obj}", "the boulder").replace("{foe}", "Brix");
                    assert!(!filled.contains('{') && crate::moves::classify(&filled) == class, "{filled}");
                }
            }
            for seed in 0..200u64 {
                for (cap, objects) in [(22, vec![]), (23, vec!["crate".to_string(), "boulder".into()]), (35, vec!["pillar".into()]), (44, vec!["altar".into()])] {
                    let last = [None, Some((MoveClass::Heavy, false)), Some((MoveClass::Strike, true)), Some((MoveClass::Parry, false))][(seed % 4) as usize];
                    let ctx = LineCtx { duel: &d, side: (seed % 2) as usize, class, flow: &flow, objects, foe_last: last, cap };
                    let t = rules_line(&ctx, &mut Rng::derive(seed, &[1]));
                    assert!(fits(&t, class, cap), "{class:?} cap {cap}: {t:?} ({} chars) is {:?}", t.chars().count(), crate::moves::classify(&t));
                    assert_eq!(t, rules_line(&ctx, &mut Rng::derive(seed, &[1])), "seeded");
                    assert!(t.chars().count() <= crate::actions::MAX_DUEL_CHARS);
                }
            }
        }
        // A name that would change the class (Hugo: "hug" is a grapple) or is no word is never used.
        let mut odd = d.clone();
        odd.f[1].name = "Hugo's Left Hand".into();
        for seed in 0..100u64 {
            let ctx = LineCtx { duel: &odd, side: 0, class: MoveClass::Strike, flow: &flow, objects: vec![], foe_last: None, cap: 40 };
            assert!(fits(&rules_line(&ctx, &mut Rng::derive(seed, &[2])), MoveClass::Strike, 40));
        }
    }

    #[test]
    fn the_typist_and_the_choice_follow_the_rules() {
        for d in 0..40u32 {
            let (cps, key) = typist(9, DemigodId(d));
            assert!((6.0..=9.0).contains(&cps) && key == (1000.0 / cps).round() as i64, "{cps} {key}");
            assert_eq!(typist(9, DemigodId(d)), (cps, key));
            assert!((MIN_CHARS + 2..=44).contains(&cap(key)));
        }
        assert!((0..40u32).map(|d| typist(9, DemigodId(d)).1).collect::<std::collections::BTreeSet<_>>().len() > 5, "typists differ");
        let mut f = Flow::new("regions");
        let mut r = Rng::new(5);
        f.range = Range::Far;
        assert_eq!(choose(&f, 1, None, false, false, 0.6, &mut r), MoveClass::CloseIn);
        f.range = Range::Grapple;
        assert!((0..50).all(|_| matches!(choose(&f, 1, None, false, false, 0.6, &mut r), MoveClass::Throw | MoveClass::Strike)));
        f.range = Range::Mid;
        f.stance[0] = Stance::Committed;
        assert!((0..50).all(|_| matches!(choose(&f, 1, None, false, false, 0.6, &mut r), MoveClass::Heavy | MoveClass::Strike)), "an opening is hit");
        f.stance[0] = Stance::Guarding;
        f.set[0] = Some(MoveClass::Guard);
        assert!((0..50).all(|_| matches!(choose(&f, 1, None, false, false, 0.6, &mut r), MoveClass::Heavy | MoveClass::Feint)), "a guard is broken or wasted");
        // Aggression: it swings more and defends less.
        let f = Flow::new("regions");
        let share = |a: f32| {
            let mut r = Rng::new(11);
            (0..2000).filter(|_| matches!(choose(&f, 1, Some(MoveClass::Strike), true, false, a, &mut r), MoveClass::Strike | MoveClass::Heavy | MoveClass::Grapple)).count()
        };
        assert!(share(0.0) < share(0.6) && share(0.6) < share(1.0), "{} {} {}", share(0.0), share(0.6), share(1.0));
        let mut r = Rng::new(3);
        assert!((0..500).all(|_| matches!(choose(&f, 1, None, true, false, 0.0, &mut r), MoveClass::Strike | MoveClass::Heavy)), "an idle opponent is pressed");
        assert!((0..500).all(|_| !matches!(choose(&f, 1, Some(MoveClass::Strike), true, false, 1.0, &mut r), MoveClass::Guard | MoveClass::Parry | MoveClass::Evade)));
    }
}

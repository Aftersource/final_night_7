//! Challenge mode's formal model (rules 0.8.1, "the flips"): a duel as a series of interrelations and flips, so tiny
//! posable figures (jointed wooden mannequins) can act it out exactly.
//!
//! **State** per duel ([`Flow`]): the `range` (far, mid, close, grapple), the `initiative` (who holds the momentum)
//! and each fighter's `stance` (neutral, guarding, committed, off_balance, airborne, downed), plus the last defensive
//! set each one holds (a guard, parry or evade waits for the next attack).
//!
//! **Move class** per blow ([`classify`]), by rules from the typed line: strike, heavy, guard, parry, feint, evade,
//! close_in, retreat, grapple, throw, invoke (a god power: ignores range, costs a bolt), command (an army or followers:
//! needs one near), taunt. Classes that can't apply become what they can ([`effective`]): a strike or heavy from far
//! is a close_in, a grapple short of close range is a close_in, a throw outside a grapple is a strike, an invoke
//! without a bolt or a command without an army is a taunt.
//!
//! **The d20 still decides success** (with initiative +2, the stance and the opening as modifiers: [`situation`]);
//! **the matrix decides what a success means** ([`exchange`]), against the defender's set:
//!
//! | Attack | Defender's set | Success | Failure |
//! |---|---|---|---|
//! | strike | parry | parried unless it beats the DC by 5 (then half damage) | parried |
//! | heavy | parry | breaks the parry: full damage, defender off balance | parried, attacker committed |
//! | strike | guard | −1 damage | nothing |
//! | heavy | guard | guard broken: full damage, defender off balance | attacker committed |
//! | feint | guard / parry | the set is wasted (defender off balance) and initiative flips | nothing |
//! | strike | evade | misses unless it beats the DC by 3 | misses |
//! | heavy | evade | misses unless natural 20; the attacker is committed and open, initiative flips | the same |
//! | any | — | full effect | nothing |
//!
//! Parried: the attacker goes off balance and the initiative FLIPS to the parrier. A heavy always leaves its user
//! committed. grapple (close only): range grapple, defender off balance; throw (from a grapple): +1 damage, defender
//! downed, range mid; close_in / retreat move the range a step; guard / parry / evade set a defence (the user guards,
//! or goes airborne for a leap); taunt: initiative to the taunter, defender off balance. A landed strike at mid closes
//! to close range. Whoever lands (or sets a defence on a success) takes the initiative.
//!
//! **Choreography** ([`choreo_json`]): every strike carries `choreo {range_before, range_after, initiative_before,
//! initiative_after, flip, a, b, camera, duration_ms}` where `a` / `b` are the two figures (challenger, target):
//! `{move_class, pose_keys: [{pose, t_ms}], contact: {limb, target_limb, t_ms} | null, stance_after}` with poses from
//! the fixed [`POSES`] vocabulary, 600–1400 ms per exchange. Deterministic (seeded by the strike's query).

use fn7_model::seed::{hash_str, Rng};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Range {
    Far,
    Mid,
    Close,
    Grapple,
}

impl Range {
    fn closer(self) -> Range {
        match self {
            Range::Far => Range::Mid,
            Range::Mid => Range::Close,
            _ => Range::Close,
        }
    }
    fn farther(self) -> Range {
        match self {
            Range::Grapple | Range::Close => Range::Mid,
            _ => Range::Far,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stance {
    Neutral,
    Guarding,
    Committed,
    OffBalance,
    Airborne,
    Downed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveClass {
    Strike,
    Heavy,
    Guard,
    Parry,
    Feint,
    Evade,
    CloseIn,
    Retreat,
    Grapple,
    Throw,
    Invoke,
    Command,
    Taunt,
}

/// The duel's state between blows.
#[derive(Debug, Clone, PartialEq)]
pub struct Flow {
    pub range: Range,
    /// The side (0 = a, 1 = b) holding the momentum.
    pub initiative: usize,
    pub stance: [Stance; 2],
    /// The defence each side has set and not yet used (guard, parry, evade).
    pub set: [Option<MoveClass>; 2],
}

impl Flow {
    /// A duel opens at mid range (close at the people level), the challenger holding the initiative.
    pub fn new(level: &str) -> Flow {
        let range = if level == "people" { Range::Close } else { Range::Mid };
        Flow { range, initiative: 0, stance: [Stance::Neutral; 2], set: [None; 2] }
    }
}

const GODS: &[&str] = &["time", "nature", "consequence", "death", "chance", "memory", "entropy", "god", "gods", "pray", "divine", "invoke", "smite", "heavens"];
const ARMY: &[&str] = &["army", "armies", "legion", "troops", "soldiers", "followers", "host", "horde", "battalion", "faithful", "minions"];
const THROW: &[&str] = &["throw", "hurl", "toss", "suplex", "flip", "slam you down", "body-slam"];
const GRAPPLE: &[&str] = &["grab", "choke", "wrestle", "clinch", "pin", "seize", "headlock", "hold you", "throttle", "hug"];
const PARRY: &[&str] = &["parry", "deflect", "counter", "riposte"];
const GUARD: &[&str] = &["block", "guard", "shield", "brace", "defend"];
const EVADE: &[&str] = &["dodge", "duck", "sidestep", "evade", "roll", "weave", "leap", "jump", "vanish"];
const FEINT: &[&str] = &["feint", "fake", "trick", "bluff", "pretend"];
const RETREAT: &[&str] = &["retreat", "back off", "step back", "withdraw", "flee", "fall back", "back away"];
const CLOSE: &[&str] = &["charge", "rush", "advance", "close in", "step in", "approach", "lunge at", "dash"];
const HEAVY: &[&str] = &["smash", "crush", "cleave", "overhead", "haymaker", "full force", "all my strength", "uppercut", "headbutt", "pulverize", "obliterate"];
const TAUNT: &[&str] = &["mock", "taunt", "insult", "laugh", "jeer", "sneer", "yawn", "scoff"];

fn has(text: &str, ws: &[String], keys: &[&str]) -> bool {
    keys.iter().any(|k| if k.contains(' ') { text.contains(k) } else { ws.iter().any(|w| w.starts_with(k)) })
}

/// The move class a line asks for (before range, bolts and armies: see [`effective`]).
pub fn classify(text: &str) -> MoveClass {
    let t = text.to_lowercase();
    let ws: Vec<String> = t.split(|c: char| !(c.is_alphanumeric() || c == '-')).filter(|w| !w.is_empty()).map(str::to_string).collect();
    // Order matters: the most specific intent first.
    if has(&t, &ws, GODS) {
        MoveClass::Invoke
    } else if has(&t, &ws, ARMY) {
        MoveClass::Command
    } else if has(&t, &ws, THROW) {
        MoveClass::Throw
    } else if has(&t, &ws, GRAPPLE) {
        MoveClass::Grapple
    } else if has(&t, &ws, FEINT) {
        MoveClass::Feint
    } else if has(&t, &ws, PARRY) {
        MoveClass::Parry
    } else if has(&t, &ws, GUARD) {
        MoveClass::Guard
    } else if has(&t, &ws, RETREAT) {
        MoveClass::Retreat
    } else if has(&t, &ws, EVADE) {
        MoveClass::Evade
    } else if has(&t, &ws, HEAVY) {
        MoveClass::Heavy
    } else if has(&t, &ws, CLOSE) {
        MoveClass::CloseIn
    } else if has(&t, &ws, TAUNT) {
        MoveClass::Taunt
    } else {
        MoveClass::Strike
    }
}

/// What the class can actually be here: range, bolts and armies gate it.
pub fn effective(class: MoveClass, range: Range, has_bolt: bool, has_army: bool) -> MoveClass {
    match class {
        MoveClass::Strike | MoveClass::Heavy if range == Range::Far => MoveClass::CloseIn,
        MoveClass::Grapple if range < Range::Close => MoveClass::CloseIn,
        MoveClass::Throw if range != Range::Grapple => MoveClass::Strike,
        MoveClass::Invoke if !has_bolt => MoveClass::Taunt,
        MoveClass::Command if !has_army => MoveClass::Taunt,
        c => c,
    }
}

/// The situational modifier for `side` attacking: +2 holding the initiative; its own stance (off balance −2,
/// committed −1, downed −3, airborne +1); +2 when the defender is open (committed, off balance or downed).
pub fn situation(f: &Flow, side: usize) -> i32 {
    let mine = match f.stance[side] {
        Stance::OffBalance => -2,
        Stance::Committed => -1,
        Stance::Downed => -3,
        Stance::Airborne => 1,
        _ => 0,
    };
    let open = matches!(f.stance[1 - side], Stance::Committed | Stance::OffBalance | Stance::Downed) as i32 * 2;
    (f.initiative == side) as i32 * 2 + mine + open
}

/// How an exchange came out, for the figures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Result {
    Landed,
    Absorbed,
    Parried,
    Evaded,
    Missed,
    Thrown,
    Grappled,
    Wasted,
    Moved,
    Set,
    Taunted,
}

/// One exchange resolved by the matrix.
#[derive(Debug, Clone, PartialEq)]
pub struct Exchange {
    pub class: MoveClass,
    pub result: Result,
    /// Multiplies the dice's damage (0, 0.5, 1) and then adds `bonus`.
    pub mult: f32,
    pub bonus: f32,
    pub after: Flow,
}

/// Resolve one exchange: `side` attacks with `class` (already [`effective`]); `ok` = the d20 succeeded, `margin` =
/// total − DC, `crit` = a natural 20. Pure.
pub fn exchange(f: &Flow, side: usize, class: MoveClass, ok: bool, margin: i32, crit: bool) -> Exchange {
    let foe = 1 - side;
    let mut a = f.clone();
    let set = f.set[foe];
    let mut result = if ok { Result::Landed } else { Result::Missed };
    let (mut mult, mut bonus) = (if ok { 1.0 } else { 0.0 }, 0.0);
    // Acting clears your own set and lifts you from the ground.
    a.set[side] = None;
    a.stance[side] = Stance::Neutral;
    let parried = |a: &mut Flow| {
        a.stance[side] = Stance::OffBalance;
        a.initiative = foe;
        a.set[foe] = None;
    };
    match class {
        MoveClass::Strike => match set {
            Some(MoveClass::Parry) if !(ok && margin >= 5) => {
                result = Result::Parried;
                mult = 0.0;
                parried(&mut a);
            }
            Some(MoveClass::Parry) => {
                mult = 0.5;
                a.set[foe] = None;
            }
            Some(MoveClass::Guard) if ok => {
                result = Result::Absorbed;
                bonus = -1.0;
                a.set[foe] = None;
            }
            Some(MoveClass::Evade) if !(ok && margin >= 3) => {
                result = Result::Evaded;
                mult = 0.0;
                a.set[foe] = None;
            }
            _ => {}
        },
        MoveClass::Heavy => {
            match set {
                Some(MoveClass::Parry) | Some(MoveClass::Guard) if ok => {
                    a.stance[foe] = Stance::OffBalance;
                    a.set[foe] = None;
                    bonus = 1.0;
                }
                Some(MoveClass::Parry) => {
                    result = Result::Parried;
                    parried(&mut a);
                }
                Some(MoveClass::Evade) if !crit => {
                    result = Result::Evaded;
                    mult = 0.0;
                    a.set[foe] = None;
                    a.initiative = foe;
                }
                _ if ok => bonus = 1.0,
                _ => {}
            }
            // A heavy always leaves its user committed (off balance once parried).
            if a.stance[side] != Stance::OffBalance {
                a.stance[side] = Stance::Committed;
            }
        }
        MoveClass::Feint => {
            mult = 0.0;
            if ok {
                result = Result::Wasted;
                if matches!(set, Some(MoveClass::Guard) | Some(MoveClass::Parry)) {
                    a.stance[foe] = Stance::OffBalance;
                }
                a.set[foe] = None;
                a.initiative = side;
            }
        }
        MoveClass::Guard | MoveClass::Parry | MoveClass::Evade => {
            mult = 0.0;
            if ok {
                result = Result::Set;
                a.set[side] = Some(class);
                a.stance[side] = if class == MoveClass::Evade { Stance::Airborne } else { Stance::Guarding };
                a.initiative = side;
            }
        }
        MoveClass::CloseIn | MoveClass::Retreat => {
            mult = 0.0;
            if ok {
                result = Result::Moved;
                a.range = if class == MoveClass::CloseIn { f.range.closer() } else { f.range.farther() };
                a.initiative = side;
            }
        }
        MoveClass::Grapple => {
            mult = 0.0;
            if ok {
                result = Result::Grappled;
                a.range = Range::Grapple;
                a.stance[foe] = Stance::OffBalance;
                a.set[foe] = None;
            }
        }
        MoveClass::Throw => {
            if ok {
                result = Result::Thrown;
                bonus = 1.0;
                a.stance[foe] = Stance::Downed;
                a.range = Range::Mid;
                a.set[foe] = None;
            }
        }
        MoveClass::Invoke | MoveClass::Command => {
            if ok {
                bonus = if class == MoveClass::Invoke { 1.0 } else { 0.0 };
                a.set[foe] = None;
            }
        }
        MoveClass::Taunt => {
            mult = 0.0;
            if ok {
                result = Result::Taunted;
                a.initiative = side;
                if a.stance[foe] == Stance::Neutral {
                    a.stance[foe] = Stance::OffBalance;
                }
            }
        }
    }
    if result == Result::Landed || result == Result::Absorbed || result == Result::Thrown {
        a.initiative = side;
        if f.range == Range::Mid && matches!(class, MoveClass::Strike | MoveClass::Heavy) {
            a.range = Range::Close;
        }
        if class == MoveClass::Heavy && matches!(a.stance[foe], Stance::Neutral | Stance::Guarding | Stance::Airborne) {
            a.stance[foe] = Stance::OffBalance;
        }
    }
    Exchange { class, result, mult, bonus, after: a }
}

/// The fixed pose vocabulary of `choreo.*.pose_keys`.
pub const POSES: &[&str] = &[
    "idle_guard", "step_in", "lunge_extend", "slash_high", "slash_low", "overhead_windup", "overhead_strike", "parry_high",
    "parry_low", "block_cross", "dodge_left", "dodge_right", "duck", "leap_up", "land", "feint_shoulder", "grab_reach",
    "grapple_lock", "throw_hip", "stagger_back", "stumble", "fall_back", "kneel_hit", "rise", "cast_raise", "cast_release",
    "command_point", "taunt_beckon", "victory", "defeat",
];

const HEAD: &[&str] = &["jaw", "chin", "nose", "eye", "eyes", "ear", "ears", "skull", "head", "temple", "face", "mouth", "teeth", "brow", "lip", "lips", "tongue"];
const ARM: &[&str] = &["arm", "elbow", "wrist", "hand", "fist", "finger", "fingers", "shoulder"];
const LEG: &[&str] = &["knee", "knees", "shin", "ankle", "foot", "feet", "thigh", "leg", "legs", "hip", "toe", "toes"];

fn target_limb(text: &str) -> &'static str {
    let t = text.to_lowercase();
    let ws: Vec<&str> = t.split(|c: char| !c.is_alphanumeric()).collect();
    if ws.iter().any(|w| HEAD.contains(w)) {
        "head"
    } else if ws.iter().any(|w| LEG.contains(w)) {
        "legs"
    } else if ws.iter().any(|w| ARM.contains(w)) {
        "arms"
    } else {
        "torso"
    }
}

fn striking_limb(text: &str, class: MoveClass) -> &'static str {
    let t = text.to_lowercase();
    if ["kick", "stomp", "sweep", "knee"].iter().any(|k| t.contains(k)) {
        "right_foot"
    } else if class == MoveClass::Throw || class == MoveClass::Grapple {
        "left_hand"
    } else {
        "right_hand"
    }
}

/// The attacker's and the defender's pose sequences for an exchange.
fn poses(x: &Exchange, f: &Flow, side: usize, r: &mut Rng, ko: bool) -> (Vec<&'static str>, Vec<&'static str>) {
    let pick = |r: &mut Rng, xs: &[&'static str]| xs[r.below(xs.len() as u64) as usize];
    let mut atk: Vec<&'static str> = vec![];
    let mut def: Vec<&'static str> = vec![];
    if f.stance[side] == Stance::Downed {
        atk.push("rise");
    }
    match x.class {
        MoveClass::Strike => {
            atk.push("step_in");
            atk.push(pick(r, &["lunge_extend", "slash_high", "slash_low"]));
        }
        MoveClass::Heavy => {
            atk.push("overhead_windup");
            atk.push("overhead_strike");
        }
        MoveClass::Guard => atk.push("block_cross"),
        MoveClass::Parry => atk.push(pick(r, &["parry_high", "parry_low"])),
        MoveClass::Evade => {
            if x.after.stance[side] == Stance::Airborne {
                atk.push("leap_up");
                atk.push("land");
            } else {
                atk.push(pick(r, &["dodge_left", "dodge_right", "duck"]));
            }
        }
        MoveClass::Feint => {
            atk.push("feint_shoulder");
            atk.push("step_in");
        }
        MoveClass::CloseIn => atk.push("step_in"),
        MoveClass::Retreat => atk.push(pick(r, &["dodge_left", "dodge_right"])),
        MoveClass::Grapple => {
            atk.push("grab_reach");
            if x.result == Result::Grappled {
                atk.push("grapple_lock");
            }
        }
        MoveClass::Throw => {
            atk.push("grapple_lock");
            atk.push("throw_hip");
        }
        MoveClass::Invoke => {
            atk.push("cast_raise");
            atk.push("cast_release");
        }
        MoveClass::Command => atk.push("command_point"),
        MoveClass::Taunt => atk.push("taunt_beckon"),
    }
    match x.result {
        Result::Landed if ko => def.extend(["stagger_back", "fall_back", "defeat"]),
        Result::Landed if x.class == MoveClass::Heavy || x.bonus > 0.0 => def.push("kneel_hit"),
        Result::Landed => def.push("stagger_back"),
        Result::Absorbed => def.push("block_cross"),
        Result::Parried => {
            def.push(pick(r, &["parry_high", "parry_low"]));
            atk.push("stumble");
        }
        Result::Evaded => def.push(pick(r, &["dodge_left", "dodge_right", "duck"])),
        Result::Thrown if ko => def.extend(["fall_back", "defeat"]),
        Result::Thrown => def.push("fall_back"),
        Result::Grappled => def.push("grapple_lock"),
        Result::Wasted => def.push("stumble"),
        Result::Taunted => def.push("stagger_back"),
        Result::Moved | Result::Set | Result::Missed => def.push("idle_guard"),
    }
    if x.class == MoveClass::Heavy && x.result != Result::Landed && x.result != Result::Absorbed {
        atk.push("stumble");
    }
    if ko {
        atk.push("victory");
    } else if !matches!(atk.last(), Some(&"idle_guard")) && x.after.stance[side] == Stance::Neutral {
        atk.push("idle_guard");
    }
    (atk, def)
}

/// The `choreo` object of one strike by `side` (`dmg` dealt, `ko` it knocked the defender out). Seeded by `query`.
#[allow(clippy::too_many_arguments)]
pub fn choreo_json(seed: u64, query: &str, f: &Flow, x: &Exchange, side: usize, text: &str, dmg: f32, ko: bool, camera: &str) -> Value {
    let mut r = Rng::derive(seed, &[0xC0E0, hash_str(query)]);
    let (atk, def) = poses(x, f, side, &mut r, ko);
    let n = atk.len().max(def.len()).max(1);
    let duration = (600 + 200 * n as u32).clamp(600, 1400);
    let step = duration / (n as u32 + 1);
    let keys = |ps: &[&str], offset: u32| -> Vec<Value> {
        ps.iter().enumerate().map(|(k, p)| json!({ "pose": p, "t_ms": offset + step * k as u32 })).collect()
    };
    // The defender reacts one beat after the attacker's strike pose.
    let strike_t = step * (atk.len().saturating_sub(if ko { 2 } else { 1 })).min(n) as u32;
    let contact = (dmg > 0.0).then(|| json!({ "limb": striking_limb(text, x.class), "target_limb": target_limb(text), "t_ms": strike_t }));
    let atk_json = json!({ "move_class": x.class, "pose_keys": keys(&atk, 0), "contact": contact, "stance_after": x.after.stance[side] });
    let def_json = json!({ "move_class": f.set[1 - side], "pose_keys": keys(&def, strike_t + step / 2),
                           "contact": Value::Null, "stance_after": if ko { Stance::Downed } else { x.after.stance[1 - side] } });
    let (a, b) = if side == 0 { (atk_json, def_json) } else { (def_json, atk_json) };
    let side_name = |s: usize| if s == 0 { "a" } else { "b" };
    json!({ "range_before": f.range, "range_after": x.after.range,
            "initiative_before": side_name(f.initiative), "initiative_after": side_name(x.after.initiative),
            "flip": f.initiative != x.after.initiative, "result": x.result, "a": a, "b": b, "camera": camera,
            "duration_ms": duration })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow() -> Flow {
        Flow::new("villages")
    }

    fn with_set(side: usize, c: MoveClass) -> Flow {
        let mut f = flow();
        f.set[side] = Some(c);
        f.stance[side] = Stance::Guarding;
        f
    }

    #[test]
    fn classes_from_words() {
        assert_eq!(classify("I parry your blade"), MoveClass::Parry);
        assert_eq!(classify("feint left then go right"), MoveClass::Feint);
        assert_eq!(classify("I smash your skull with full force"), MoveClass::Heavy);
        assert_eq!(classify("grab your collar"), MoveClass::Grapple);
        assert_eq!(classify("I throw you over my hip"), MoveClass::Throw);
        assert_eq!(classify("Death, take him"), MoveClass::Invoke);
        assert_eq!(classify("my army, charge!"), MoveClass::Command);
        assert_eq!(classify("I charge at you"), MoveClass::CloseIn);
        assert_eq!(classify("I step back out of reach"), MoveClass::Retreat);
        assert_eq!(classify("I duck under it"), MoveClass::Evade);
        assert_eq!(classify("I block with my shield"), MoveClass::Guard);
        assert_eq!(classify("you fight like a farmer, I mock you"), MoveClass::Taunt);
        assert_eq!(classify("I punch your nose"), MoveClass::Strike);
    }

    #[test]
    fn gates_turn_moves_into_what_they_can_be() {
        assert_eq!(effective(MoveClass::Strike, Range::Far, true, true), MoveClass::CloseIn);
        assert_eq!(effective(MoveClass::Grapple, Range::Mid, true, true), MoveClass::CloseIn);
        assert_eq!(effective(MoveClass::Grapple, Range::Close, true, true), MoveClass::Grapple);
        assert_eq!(effective(MoveClass::Throw, Range::Close, true, true), MoveClass::Strike);
        assert_eq!(effective(MoveClass::Throw, Range::Grapple, true, true), MoveClass::Throw);
        assert_eq!(effective(MoveClass::Invoke, Range::Far, false, true), MoveClass::Taunt);
        assert_eq!(effective(MoveClass::Invoke, Range::Far, true, true), MoveClass::Invoke, "invoke ignores range");
        assert_eq!(effective(MoveClass::Command, Range::Mid, true, false), MoveClass::Taunt);
    }

    #[test]
    fn parry_vs_strike_flips_the_initiative() {
        let x = exchange(&with_set(1, MoveClass::Parry), 0, MoveClass::Strike, true, 2, false);
        assert_eq!((x.result, x.mult, x.after.stance[0], x.after.initiative), (Result::Parried, 0.0, Stance::OffBalance, 1));
        let y = exchange(&with_set(1, MoveClass::Parry), 0, MoveClass::Strike, true, 6, false);
        assert_eq!((y.result, y.mult, y.after.initiative), (Result::Landed, 0.5, 0), "beating the DC by 5 breaks through");
    }

    #[test]
    fn heavy_breaks_guards_and_commits() {
        let x = exchange(&with_set(1, MoveClass::Guard), 0, MoveClass::Heavy, true, 1, false);
        assert_eq!((x.result, x.bonus, x.after.stance[0], x.after.stance[1]), (Result::Landed, 1.0, Stance::Committed, Stance::OffBalance));
        let y = exchange(&with_set(1, MoveClass::Parry), 0, MoveClass::Heavy, false, -3, false);
        assert_eq!((y.result, y.after.stance[0], y.after.initiative), (Result::Parried, Stance::OffBalance, 1));
        let z = exchange(&flow(), 0, MoveClass::Heavy, true, 0, false);
        assert_eq!((z.bonus, z.after.stance[0]), (1.0, Stance::Committed), "heavy wins if it lands, leaves its user committed");
    }

    #[test]
    fn strike_into_a_guard_is_absorbed() {
        let x = exchange(&with_set(1, MoveClass::Guard), 0, MoveClass::Strike, true, 3, false);
        assert_eq!((x.result, x.bonus), (Result::Absorbed, -1.0));
    }

    #[test]
    fn feint_wastes_the_guard_and_flips() {
        let mut f = with_set(1, MoveClass::Guard);
        f.initiative = 1;
        let x = exchange(&f, 0, MoveClass::Feint, true, 1, false);
        assert_eq!((x.result, x.mult, x.after.set[1], x.after.stance[1], x.after.initiative), (Result::Wasted, 0.0, None, Stance::OffBalance, 0));
    }

    #[test]
    fn evade_beats_heavy_and_leaves_it_open() {
        let x = exchange(&with_set(1, MoveClass::Evade), 0, MoveClass::Heavy, true, 4, false);
        assert_eq!((x.result, x.mult, x.after.stance[0], x.after.initiative), (Result::Evaded, 0.0, Stance::Committed, 1));
        assert_eq!(exchange(&with_set(1, MoveClass::Evade), 0, MoveClass::Heavy, true, 9, true).result, Result::Landed, "a natural 20 connects");
        assert_eq!(exchange(&with_set(1, MoveClass::Evade), 0, MoveClass::Strike, true, 1, false).result, Result::Evaded);
        assert_eq!(exchange(&with_set(1, MoveClass::Evade), 0, MoveClass::Strike, true, 3, false).result, Result::Landed);
    }

    #[test]
    fn grapple_throw_and_range() {
        let mut f = flow();
        f.range = Range::Close;
        let g = exchange(&f, 0, MoveClass::Grapple, true, 1, false);
        assert_eq!((g.after.range, g.after.stance[1]), (Range::Grapple, Stance::OffBalance));
        let t = exchange(&g.after, 0, MoveClass::Throw, true, 1, false);
        assert_eq!((t.result, t.bonus, t.after.stance[1], t.after.range), (Result::Thrown, 1.0, Stance::Downed, Range::Mid));
        let c = exchange(&flow(), 1, MoveClass::CloseIn, true, 0, false);
        assert_eq!((c.after.range, c.after.initiative), (Range::Close, 1));
        let r = exchange(&c.after, 0, MoveClass::Retreat, true, 0, false);
        assert_eq!(r.after.range, Range::Mid);
    }

    #[test]
    fn situation_counts_initiative_stance_and_openings() {
        let mut f = flow();
        assert_eq!((situation(&f, 0), situation(&f, 1)), (2, 0));
        f.stance[1] = Stance::Committed;
        assert_eq!(situation(&f, 0), 4);
        f.stance[0] = Stance::OffBalance;
        assert_eq!(situation(&f, 0), 2);
    }

    #[test]
    fn choreography_uses_the_vocabulary_and_timing() {
        let f = with_set(1, MoveClass::Parry);
        for (class, ok, ko) in [(MoveClass::Strike, true, false), (MoveClass::Heavy, true, true), (MoveClass::Throw, true, false),
                                (MoveClass::Invoke, false, false), (MoveClass::Evade, true, false)] {
            let x = exchange(&f, 0, class, ok, 6, false);
            let v = choreo_json(7, "q", &f, &x, 0, "I punch your jaw", if x.mult > 0.0 { 2.0 } else { 0.0 }, ko, "close");
            let d = v["duration_ms"].as_u64().unwrap();
            assert!((600..=1400).contains(&d));
            for s in ["a", "b"] {
                for k in v[s]["pose_keys"].as_array().unwrap() {
                    assert!(POSES.contains(&k["pose"].as_str().unwrap()), "{k}");
                    assert!(k["t_ms"].as_u64().unwrap() <= d);
                }
            }
            assert_eq!(v, choreo_json(7, "q", &f, &x, 0, "I punch your jaw", if x.mult > 0.0 { 2.0 } else { 0.0 }, ko, "close"), "deterministic");
        }
        let x = exchange(&flow(), 0, MoveClass::Strike, true, 2, false);
        let v = choreo_json(7, "q", &flow(), &x, 0, "I kick your knee", 2.0, false, "over_a");
        assert_eq!((v["a"]["contact"]["limb"].as_str(), v["a"]["contact"]["target_limb"].as_str()), (Some("right_foot"), Some("legs")));
        assert_eq!(v["flip"], false);
    }
}

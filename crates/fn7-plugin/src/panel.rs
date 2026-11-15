//! Duel panels (rules 0.8.2, state schema 9): a duel plays as comic panels. The client design is `duel_lab/`
//! (README.md, duel.json): a scene of primitive objects on a hex stage, and a script of panels, each with a caption, a
//! camera shot, per-fighter clip segments along paths, and timed contacts.
//!
//! **Panels are a presentation of the blows, never a second source of truth.** The 0.8.1 rules still resolve every
//! typed line at once (the d20, the wit, the interaction matrix: [`crate::duel::Bout::step`]). A panel packages the
//! blows that arrived in one input window into one scene played in the next slot:
//!
//! - panel 0 opens at the lock (`starts_at_ms` = the challenge's receive stamp);
//! - panel `n` plays for `play_ms`; its input closes at `input_closes_ms` = start + play − [`LEAD_MS`] (the render
//!   lead: the next panel is known that long before it starts);
//! - the blows stamped up to that close (and after the previous one) make panel `n + 1`, which starts when `n` ends;
//!   a fighter with no blow in the window gets the rules default (`circle` holding the initiative, else `guard`);
//! - a knockout's panel is followed by the outro and the cycle is done; so is the first slot at or past the deadline.
//!
//! **Pacing without a timer.** The plugin runs only on actions and beats, so the cycle advances on stamps: any duel
//! action of the duel (a line, or a `fields.duel_tick {duel_id}` a duellist's client sends at each input close)
//! closes every window whose close its stamp has passed, and each resolve does the same with its batch's newest
//! stamp. A tick up to [`TICK_GRACE_MS`] early closes the window at the tick (clocks differ by a little). What a panel
//! shows depends only on the blows before its close, never on when it was computed, so the live view, the resolve and
//! a replay build the same panels.
//!
//! Everything is in the bout (`scene`, `panels`, `cycle`), so resume, restart, replay and fork carry it.

use fn7_model::hexgrid::HexLevel;
use fn7_model::seed::{hash_str, Rng};
use fn7_model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::choreo::Range;
use crate::duel::{Bout, Duel};
use crate::settings::Settings;

/// The render lead: a panel's input closes this long before it ends.
pub const LEAD_MS: i64 = 700;
/// A duellist's tick this early still closes the window (at the tick).
pub const TICK_GRACE_MS: i64 = 150;
const INTRO_MS: i64 = 2600;
const OUTRO_MS: i64 = 2600;
const IDLE_MS: i64 = 2400;
const SCENE_SALT: u64 = 0x5CE2;
const PANEL_SALT: u64 = 0x9A2E;

/// The clip vocabulary of `segs[].clip` (duel_lab/assets/anims).
pub const CLIPS: &[&str] = &[
    "airborne_hit", "block", "block_cross", "bow", "combo_triple", "command", "counter", "defeat", "downed", "evade_roll",
    "evade_spin", "evade_stand", "grapple_throw_down", "ground_slam", "heavy_charged", "heavy_hammer", "idle_guard", "invoke",
    "jump_attack", "kick", "kneel", "lunge", "parry_back", "parry_high", "rise", "run", "shield_push", "stagger_back",
    "stagger_waist", "strike_side", "strike_slash", "taunt", "throw_shoulder", "victory", "walk",
    // Schema 12: the new clips of the same rig (a client without one plays its fallback: [`CLIP_FALLBACKS`]).
    "flying_kick", "roundhouse_kick", "high_kick", "leg_sweep", "elbow_strike", "uppercut", "jab", "punch_combo", "blade_spin", "axe_spin",
    "spin_rise", "mage_cast", "dodge_roll_back", "knee_strike", "sword_judgment",
];
/// Schema 12: what a client without a new clip plays instead (`clip_fallbacks`).
pub const CLIP_FALLBACKS: &[(&str, &str)] = &[
    ("flying_kick", "kick"), ("roundhouse_kick", "kick"), ("high_kick", "kick"), ("leg_sweep", "kick"), ("elbow_strike", "strike_side"),
    ("uppercut", "strike_side"), ("jab", "strike_side"), ("punch_combo", "combo_triple"), ("blade_spin", "strike_slash"),
    ("axe_spin", "heavy_hammer"), ("spin_rise", "rise"), ("mage_cast", "invoke"), ("dodge_roll_back", "evade_roll"), ("knee_strike", "kick"),
    ("sword_judgment", "heavy_charged"),
];
/// The path vocabulary of `segs[].path`.
pub const PATHS: &[&str] = &["dash", "leap", "flip", "knock", "vault", "wallrun", "wallkick", "mantle", "drop", "slide"];
/// The camera shots.
pub const SHOTS: &[&str] = &["wide", "side", "low", "over_a", "over_b", "close_a", "close_b"];

/// Where the panel cycle stands (kept in the bout).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cycle {
    /// The newest panel's input close (0: it takes no input).
    pub closes_ms: i64,
    /// When the next panel starts (the newest one's end).
    pub next_ms: i64,
    /// Blows already shown in a panel.
    pub fed: usize,
    /// Where the fighters stand after the newest panel (hex `[q, r]`).
    pub pos: [[i32; 2]; 2],
    pub hp: [f64; 2],
    pub range: Range,
    pub initiative: usize,
    /// The outro is out: no more panels.
    pub done: bool,
}

fn side_name(s: usize) -> &'static str {
    if s == 0 { "a" } else { "b" }
}

/// Hexes between the fighters at a range band.
fn sep(r: Range) -> i32 {
    match r {
        Range::Far => 8,
        Range::Mid => 5,
        Range::Close => 2,
        Range::Grapple => 1,
    }
}

fn range_of(v: &Value) -> Option<Range> {
    serde_json::from_value(v.clone()).ok()
}

fn hex_dist(a: [i32; 2], b: [i32; 2]) -> i32 {
    let (dq, dr) = (a[0] - b[0], a[1] - b[1]);
    (dq.abs() + dr.abs() + (dq + dr).abs()) / 2
}

// ---------------------------------------------------------------------------------------------------------------------
// The scene: the rules fallback (an LLM-authored scene can replace it behind the same shape)
// ---------------------------------------------------------------------------------------------------------------------

fn part(shape: &str, size: &[f64], offset: [f64; 3], material: &str) -> Value {
    json!({ "shape": shape, "size": size, "offset": offset, "material": material })
}

/// An object of a known kind: its primitive parts, whether it is built into the ground, and what a fighter can do with
/// it (`affords`: the paths that may name it).
fn object(kind: &str) -> (Vec<Value>, bool, &'static [&'static str], &'static str) {
    match kind {
        "pillar" => (
            vec![part("box", &[0.9, 0.35, 0.9], [0.0, 0.175, 0.0], "stone"), part("cylinder", &[0.3, 1.5], [0.0, 1.1, 0.0], "stone"),
                 part("cylinder", &[0.3, 1.5], [0.0, 2.6, 0.0], "stone"), part("box", &[0.8, 0.28, 0.8], [0.0, 3.49, 0.0], "stone")],
            true, &["wallrun", "wallkick"], "an old stone pillar, the mortar long gone"),
        "wall" => (
            vec![part("box", &[2.4, 1.2, 0.4], [0.0, 0.6, 0.0], "stone"), part("box", &[2.4, 1.2, 0.4], [0.0, 1.8, 0.0], "stone")],
            true, &["wallrun", "wallkick"], "a stretch of broken wall"),
        "crate" => (vec![json!({ "shape": "box", "size": [0.85, 0.75, 0.85], "offset": [0.0, 0.375, 0.0], "material": "wood", "fill": 0.12 })],
                    false, &["vault"], "a crate somebody meant to come back for"),
        "table" => (
            vec![part("box", &[1.6, 0.08, 0.8], [0.0, 0.86, 0.0], "wood"), part("box", &[0.1, 0.82, 0.1], [-0.7, 0.41, -0.3], "wood"),
                 part("box", &[0.1, 0.82, 0.1], [0.7, 0.41, -0.3], "wood"), part("box", &[0.1, 0.82, 0.1], [-0.7, 0.41, 0.3], "wood"),
                 part("box", &[0.1, 0.82, 0.1], [0.7, 0.41, 0.3], "wood")],
            false, &["slide", "vault"], "a trestle table"),
        "boulder" => (vec![part("dodeca", &[0.8], [0.0, 0.7, 0.0], "stone")], true, &["mantle", "wallkick"], "a boulder older than the road"),
        "rock" => (vec![part("ico", &[0.45], [0.0, 0.4, 0.0], "stone")], false, &["vault"], "a rock the size of a sheep"),
        "tree" => (vec![part("cylinder", &[0.22, 2.4], [0.0, 1.2, 0.0], "wood"), part("cone", &[1.1, 2.2], [0.0, 3.3, 0.0], "leaf")],
                   true, &["wallkick"], "a tree that has seen worse"),
        "log" => (vec![json!({ "shape": "cylinder", "size": [0.3, 1.8], "offset": [0.0, 0.3, 0.0], "rot": [0, 0, 90], "material": "wood" })],
                  false, &["vault"], "a felled trunk"),
        "jar" => (vec![part("cylinder", &[0.25, 0.6], [0.0, 0.3, 0.0], "clay")], false, &[], "a clay jar, full of something"),
        "brazier" => (vec![part("cylinder", &[0.12, 0.8], [0.0, 0.4, 0.0], "iron"),
                           json!({ "shape": "cone", "size": [0.35, 0.3], "offset": [0.0, 0.95, 0.0], "rot": [180, 0, 0], "material": "iron", "glow": 1 })],
                      false, &[], "a brazier, still lit"),
        "altar" => (vec![part("box", &[1.2, 0.9, 0.7], [0.0, 0.45, 0.0], "marble"), part("octa", &[0.22], [0.0, 1.15, 0.0], "gold")],
                    true, &["vault"], "a shrine's altar"),
        "chest" => (vec![json!({ "shape": "box", "size": [0.7, 0.45, 0.45], "offset": [0.0, 0.225, 0.0], "material": "wood", "fill": 0.3 }),
                         part("box", &[0.72, 0.06, 0.47], [0.0, 0.48, 0.0], "iron")],
                    false, &["vault"], "what somebody dropped here"),
        "bones" => (vec![part("sphere", &[0.14], [0.0, 0.14, 0.0], "bone"), part("box", &[0.6, 0.06, 0.06], [0.2, 0.05, 0.1], "bone")],
                    false, &[], "old bones"),
        _ => (vec![part("cone", &[0.4, 0.7], [0.0, 0.35, 0.0], "stone")], true, &[], "a cairn: something was decided here"),
    }
}

/// The duel's scene (`duel.scene`): the stage dressing for the box, in the duel lab's scene shape. A deterministic
/// rules fallback from what the DM knows of the place: its words (harbour, forest, desert, hills, a town), the
/// settlement's level, the time of day, a shrine or dropped loot on the hex, and a cairn for each of the newest things
/// the DM remembers there. Seeded by the match seed and the duel's hex.
pub fn scene(w: &World, d: &Duel) -> Value {
    let mut r = Rng::derive(d.seed, &[SCENE_SALT, hash_str(&d.cell)]);
    let wh = crate::dm::locate(w, d.at, HexLevel::People);
    let level = wh.settlement.and_then(|p| w.dm.settlements.iter().find(|s| s.place == p)).map(|s| s.level).unwrap_or(0);
    let has = |keys: &[&str]| d.terrain.iter().any(|t| keys.iter().any(|k| t.contains(k)));
    let (biome, mut kinds): (&str, Vec<&str>) = if has(&["harbo", "dock", "port", "sea", "shore", "beach", "coast", "bay"]) {
        ("harbour", vec!["pillar", "crate", "crate", "table", "jar", "crate"])
    } else if has(&["forest", "wood", "jungle", "grove"]) {
        ("forest", vec!["tree", "log", "tree", "rock", "boulder", "tree"])
    } else if has(&["desert", "dune", "sand", "sahara"]) {
        ("desert", vec!["wall", "rock", "bones", "boulder", "jar", "rock"])
    } else if has(&["mountain", "hill", "rock", "cliff", "alp", "andes"]) {
        ("hills", vec!["boulder", "rock", "cairn", "log", "rock", "tree"])
    } else if level >= 2 {
        ("town", vec!["pillar", "crate", "table", "jar", "wall", "crate"])
    } else {
        ("wild", vec!["boulder", "tree", "rock", "log", "tree", "rock"])
    };
    if d.night {
        kinds.push("brazier");
    }
    let cell = wh.cell.clone();
    if w.dm.shrines.iter().any(|s| s.cell == cell) {
        kinds.push("altar");
    }
    if w.dm.loot.iter().any(|l| l.cell == cell && l.taken_beat.is_none()) {
        kinds.push("chest");
    }
    // What the DM remembers of this hex: a cairn each for the two newest public entries.
    let remembered: Vec<String> = w.dm.memory.iter().rev().filter(|e| e.public && e.place == cell && e.kind != "dice.rolled")
        .take(2).map(|e| e.text.chars().take(80).collect()).collect();
    let start = starts(crate::choreo::Flow::new(&d.level).range);
    // Hexes off the fighters' lane, nearest rows first, shuffled by the seed.
    let mut spots: Vec<[i32; 2]> = vec![];
    for rr in [-2, 2, -3, 1, -1, 3] {
        for q in -5..=5 {
            if hex_dist([q, rr], [0, 0]) <= 5 && !(rr.abs() == 1 && (q - start[0][0]).abs().min((q - start[1][0]).abs()) < 1) {
                spots.push([q, rr]);
            }
        }
    }
    let mut objects: Vec<Value> = vec![];
    let mut put = |kind: &str, note: Option<String>, r: &mut Rng, spots: &mut Vec<[i32; 2]>| {
        if spots.is_empty() {
            return;
        }
        let at = spots.remove(r.below(spots.len().min(9) as u64) as usize);
        // Keep its neighbours free.
        spots.retain(|s| hex_dist(*s, at) > 1);
        let (parts, fixed, affords, about) = object(kind);
        let n = objects.iter().filter(|o: &&Value| o["kind"] == kind).count();
        objects.push(json!({ "id": format!("{kind}_{}", (b'a' + n as u8) as char), "kind": kind, "at": at, "yaw": (r.below(8) * 45) as i64,
                             "fixed": fixed, "note": note.unwrap_or_else(|| about.to_string()), "affords": affords, "parts": parts }));
    };
    for k in kinds {
        put(k, None, &mut r, &mut spots);
    }
    for text in remembered {
        put("cairn", Some(text), &mut r, &mut spots);
    }
    json!({ "event": "duel.scene", "duel_id": d.id, "source": "rules", "biome": biome, "place": d.place, "time_of_day": d.phase,
            "cell": d.cell, "level": d.level, "settlement_level": level,
            "backstory": format!("{} at {}. {} and {} have chosen this hex.", d.place, d.phase, d.f[0].name, d.f[1].name),
            "hex": { "x": "1.732 (q + r/2)", "z": "1.5 r", "unit": "m" }, "radius": 5,
            "a_start": start[0], "b_start": start[1], "objects": objects })
}

fn starts(range: Range) -> [[i32; 2]; 2] {
    let s = sep(range);
    [[-(s + 1) / 2, 0], [s / 2, 0]]
}

// ---------------------------------------------------------------------------------------------------------------------
// The cycle
// ---------------------------------------------------------------------------------------------------------------------

/// Open the cycle at the lock: the scene and panel 0. Nothing in a state without panel rules, or for a duel with no
/// stamped lock (an AI's challenge in a simulation: its duel is fought out at one resolve).
pub fn open(b: &mut Bout, w: &World) {
    if !w.panel_rules() || b.duel.started_ms <= 0 || b.cycle.is_some() {
        return;
    }
    b.scene = Some(scene(w, &b.duel));
    let t0 = b.duel.started_ms;
    let pos = starts(b.flow.range);
    let hp = [n1(b.hp[0]), n1(b.hp[1])];
    // Schema 12: its AI fighters type their first lines in window 0, and the intro plays as long as they need.
    let st = crate::world::settings(w);
    let mut play = INTRO_MS;
    let mut scripts = vec![];
    if crate::duel_ai::enabled(w, &st, &b.duel) {
        b.ai = Some(Default::default());
        (scripts, play) = crate::duel_ai::open_window(b, &st, 0, t0, t0, INTRO_MS);
    }
    b.cycle = Some(Cycle { closes_ms: t0 + play - LEAD_MS, next_ms: t0 + play, fed: 0, pos, hp,
                           range: b.flow.range, initiative: b.flow.initiative, done: false });
    let typing = b.ai.is_some();
    let id = b.duel.id.clone();
    let (a, bb) = (&b.duel.f[0].name, &b.duel.f[1].name);
    let seg = |clip: &str| json!([{ "clip": clip, "ms": play, "loop": clip == "idle_guard" }]);
    let fighter = |s: usize, clip: &str| {
        let mut f = json!({ "line": null, "line_masked_len": 0, "lines": 0, "move_class": null, "default": null,
                            "auto": false, "segs": seg(clip), "at": pos[s] });
        if typing {
            f["typing_script"] = scripts.iter().find(|x| x.side == s).map(|x| x.json(&id)).unwrap_or(Value::Null);
        }
        f
    };
    let intro = json!({
        "event": "duel.panel", "duel_id": b.duel.id, "n": 0, "kind": "intro",
        "starts_at_ms": t0, "play_ms": play, "input_opens_ms": t0, "input_closes_ms": t0 + play - LEAD_MS,
        "caption": format!("{a} and {bb}: two wills, one hex. The box opens."), "shot": "wide",
        "a": fighter(0, if b.flow.initiative == 0 { "taunt" } else { "idle_guard" }),
        "b": fighter(1, if b.flow.initiative == 1 { "taunt" } else { "idle_guard" }),
        "contacts": [], "blows": [], "flip": false,
        "hp": { "a": hp[0], "b": hp[1] }, "range": b.flow.range, "initiative": side_name(b.flow.initiative),
    });
    b.panels.push(intro);
    note_scripts(b, scripts);
}

/// The scripts of the window that just opened are now being typed.
fn note_scripts(b: &mut Bout, scripts: Vec<crate::duel_ai::Script>) {
    if let Some(p) = b.ai.as_mut() {
        for s in &scripts {
            p.window[s.side.min(1)] = Some(s.window);
        }
        p.pending = scripts;
    }
}

fn n1(x: f32) -> f64 {
    (x as f64 * 10.0).round() / 10.0
}

/// The cycle is still running (its duellists' ticks are still taken).
pub fn running(b: &Bout) -> bool {
    b.cycle.as_ref().is_some_and(|c| !c.done)
}

/// Emit every panel that is due at `now_ms`: each window whose close has passed (`tick`: a duellist's tick may close
/// the current one up to [`TICK_GRACE_MS`] early). Returns how many panels were added.
///
/// Schema 12: in the same pass, in time order, every AI line whose Enter (`ends_at_ms`) has come by `now_ms` becomes
/// a blow ([`crate::duel_ai::enter_due`]); a window that closes enters what is left of its own.
pub fn advance(b: &mut Bout, w: &World, st: &Settings, now_ms: i64, mut tick: bool) -> usize {
    let before = b.panels.len();
    loop {
        if now_ms > 0 {
            crate::duel_ai::enter_due(b, w, st, Some(now_ms));
        }
        // Schema 12: the duel ends AT its deadline: the first stamp past it closes it (every panel left, the outro).
        if b.moves && b.duel.deadline_ms > 0 && now_ms > b.duel.deadline_ms && running(b) {
            finish(b, w, st, false);
            break;
        }
        let Some(c) = b.cycle.as_ref().filter(|c| !c.done) else { break };
        let due = now_ms >= c.closes_ms || (tick && now_ms + TICK_GRACE_MS >= c.closes_ms);
        if !due {
            break;
        }
        tick = false;
        next(b, w, st, true);
    }
    b.panels.len() - before
}

/// The duel is over (the resolve ended it): every remaining panel, through the outro, whatever the clock says.
/// `typing` (schema 12, two AIs): the AI fighters keep typing and entering their lines in the windows left.
pub fn finish(b: &mut Bout, w: &World, st: &Settings, typing: bool) -> usize {
    let before = b.panels.len();
    while running(b) {
        next(b, w, st, typing);
    }
    b.panels.len() - before
}

/// The room stood still for `ms`: the schedule moves with it.
pub fn shift(b: &mut Bout, ms: i64) {
    if let Some(c) = b.cycle.as_mut().filter(|c| !c.done) {
        c.closes_ms += ms;
        c.next_ms += ms;
    }
    // Schema 12: so do the lines being typed.
    if let Some(p) = b.ai.as_mut() {
        for s in p.pending.iter_mut() {
            s.starts_at_ms += ms;
            s.ends_at_ms += ms;
            s.input_closes_ms += ms;
        }
    }
}

fn next(b: &mut Bout, w: &World, st: &Settings, typing: bool) {
    // Schema 12: the window closes: an AI line of it not entered yet is entered now (or dropped: the duel was ended).
    if typing {
        crate::duel_ai::enter_due(b, w, st, None);
    } else {
        crate::duel_ai::drop_pending(b);
    }
    let Some(c) = b.cycle.clone() else { return };
    let starts = c.next_ms;
    let deadline = b.duel.deadline_ms;
    let unfed = b.blows.len() > c.fed;
    let last_was_ko = b.panels.last().is_some_and(|p| p["kind"] == "ko");
    // The outro: after a knockout's panel, or when the next slot would start at or past the deadline with nothing
    // left to show.
    if last_was_ko || (!unfed && (b.ko.is_some() || (deadline > 0 && starts >= deadline))) {
        outro(b, starts);
        return;
    }
    exchange(b, st, starts, typing);
}

fn outro(b: &mut Bout, starts: i64) {
    let Some(c) = b.cycle.as_mut() else { return };
    c.done = true;
    c.closes_ms = 0;
    c.next_ms = starts + OUTRO_MS;
    let (pos, hp, range, init) = (c.pos, c.hp, c.range, c.initiative);
    let win = b.result().map(|r| r.0);
    let clip = |s: usize| match win {
        Some(x) if x == s => "victory",
        Some(_) if b.ko.is_some() => "defeat",
        Some(_) => "kneel",
        None => "bow",
    };
    let fighter = |s: usize| json!({ "line": null, "line_masked_len": 0, "lines": 0, "move_class": null, "default": null, "auto": false,
                                      "segs": [{ "clip": clip(s), "ms": OUTRO_MS }], "at": pos[s] });
    let caption = match win {
        Some(x) => format!("The box remembers who stood: {}.", b.duel.f[x].name),
        None => "Neither will bends. The box closes on a draw.".to_string(),
    };
    let n = b.panels.len();
    b.panels.push(json!({
        "event": "duel.panel", "duel_id": b.duel.id, "n": n, "kind": "outro",
        "starts_at_ms": starts, "play_ms": OUTRO_MS, "input_opens_ms": null, "input_closes_ms": null,
        "caption": caption, "shot": match win { Some(0) => "close_a", Some(_) => "close_b", None => "wide" },
        "a": fighter(0), "b": fighter(1), "contacts": [], "blows": [], "flip": false,
        "hp": { "a": hp[0], "b": hp[1] }, "range": range, "initiative": side_name(init),
        "winner": win.map(side_name), "ko": b.ko.map(side_name),
    }));
}

/// One strike of the window, flattened (a blow, or the counter riding on it).
struct Strike<'a> {
    side: usize,
    ev: &'a Value,
    typed: bool,
}

fn exchange(b: &mut Bout, st: &Settings, starts: i64, typing: bool) {
    let Some(mut c) = b.cycle.clone() else { return };
    let n = b.panels.len();
    let window: Vec<Value> = b.blows[c.fed..].to_vec();
    c.fed = b.blows.len();
    let a_ref = json!(Ref::Demigod(b.duel.f[0].id));
    let mut strikes: Vec<Strike> = vec![];
    for ev in &window {
        let side = if ev["by"] == a_ref { 0 } else { 1 };
        strikes.push(Strike { side, ev, typed: ev["auto"] != true });
        if ev["counter"].is_object() {
            strikes.push(Strike { side: 1 - side, ev: &ev["counter"], typed: false });
        }
    }
    let mut r = Rng::derive(b.duel.seed, &[PANEL_SALT, hash_str(&b.duel.id), n as u64]);
    let objects: Vec<Value> = b.scene.as_ref().and_then(|s| s["objects"].as_array().cloned()).unwrap_or_default();
    let shown = strikes.len().min(3);
    let base = if shown == 0 { IDLE_MS } else { (600 + 1000 * shown as i64).clamp(2400, 3600) };
    let slot = if shown == 0 { base } else { base / shown as i64 };
    // Schema 12: the AI's lines for the window this panel opens, and the play time that fits them (the strikes are
    // staged in `base`; the rest is guard). None after a knockout or when the slot reaches the deadline.
    let deadline = b.duel.deadline_ms;
    let opens = if c.closes_ms > 0 { c.closes_ms } else { starts };
    let (scripts, play) = if typing && b.ko.is_none() && !(deadline > 0 && starts + base >= deadline) {
        crate::duel_ai::open_window(b, st, n as u32, opens, starts, base)
    } else {
        (vec![], base)
    };
    let ai_types = b.ai.is_some();
    let duel_id = b.duel.id.clone();
    let mut segs: [Vec<Value>; 2] = [vec![], vec![]];
    let mut contacts: Vec<Value> = vec![];
    let mut class: [Value; 2] = [Value::Null, Value::Null];
    let mut pos = c.pos;
    let init_before = c.initiative;
    let mut ko: Option<usize> = None;
    let mut decisive: Option<&Strike> = None;
    for (k, s) in strikes.iter().enumerate() {
        let ch = &s.ev["choreo"];
        let (me, foe) = (s.side, 1 - s.side);
        let dmg = s.ev["damage"].as_f64().unwrap_or(0.0);
        let result = ch["result"].as_str().unwrap_or("penalty");
        let cls = ch[side_name(me)]["move_class"].as_str().unwrap_or("");
        if let Some(rg) = range_of(&ch["range_after"]) {
            c.range = rg;
        }
        if let Some(i) = ch["initiative_after"].as_str() {
            c.initiative = if i == "a" { 0 } else { 1 };
        }
        let down = s.ev["ko"] == true && s.ev["hp"][side_name(foe)].as_f64().is_some_and(|h| h <= 0.0);
        if down {
            ko = Some(foe);
        }
        if decisive.is_none_or(|d| dmg >= d.ev["damage"].as_f64().unwrap_or(0.0)) {
            decisive = Some(s);
        }
        class[me] = ch[side_name(me)]["move_class"].clone();
        if k >= shown {
            continue; // more blows than a panel can stage: they still count (hp, range), unseen
        }
        let t0 = slot * k as i64;
        let mv = &s.ev["move"];
        let foot = match mv["clip"].as_str() {
            // Schema 12: the clip the words picked says which limb.
            Some(clip) => clip.contains("kick") || clip.contains("sweep") || clip.contains("knee"),
            None => ch["contacts"][0]["limb"].as_str().is_some_and(|l| l.contains("foot")) || cls == "strike" && s.ev["text"].as_str().is_some_and(|t| t.to_lowercase().contains("kick")),
        };
        let (mine, theirs) = stage(&mut r, &objects, &mut pos, me, cls, foot, result, dmg, down, c.range, slot, mv);
        segs[me].extend(mine);
        segs[foe].extend(theirs);
        // The contact: on the foe when it lands, on the scenery or the air when it does not.
        let kind = if matches!(cls, "heavy" | "throw") { "heavy" } else if foot { "kick" } else { "hand" };
        let limb = if foot { "foot_r" } else if matches!(cls, "grapple" | "throw") { "hand_l" } else { "hand_r" };
        let t_ms = t0 + slot * 55 / 100;
        if matches!(cls, "strike" | "heavy" | "grapple" | "throw" | "invoke" | "command") {
            let zone = match ch["contacts"][0]["target_limb"].as_str().unwrap_or("torso") {
                "head" => "head",
                "legs" => "legs",
                "arms" => "chest",
                _ => "belly",
            };
            let words: &[&str] = match (dmg > 0.0, kind) {
                (true, "heavy") => &["KRAAK", "THOOM", "WHAM"],
                (true, "kick") => &["KRAK", "THWAK", "BAM"],
                (true, _) => &["TOK", "SHHK", "KRAK"],
                (false, _) => &["WHUMP", "KLANG", "FWIP"],
            };
            let text = words[r.below(words.len() as u64) as usize];
            let near = nearest(&objects, pos[foe], 2, &[]);
            if dmg > 0.0 {
                contacts.push(json!({ "t_ms": t_ms, "by": side_name(me), "limb": limb, "kind": kind, "on": side_name(foe), "at": zone,
                                      "impulse": (dmg / 4.0).clamp(0.1, 1.0), "text": text, "miss": false,
                                      "ranged": matches!(cls, "invoke" | "command") }));
            } else if let (Some(o), true) = (near.filter(|_| matches!(result, "missed" | "evaded")), r.below(2) == 0) {
                // The blow meant for the foe finds the scenery.
                contacts.push(json!({ "t_ms": t_ms, "by": side_name(me), "limb": limb, "kind": kind, "on": o["id"], "at": null,
                                      "impulse": 0.5, "text": text, "miss": false, "ranged": false }));
            } else {
                contacts.push(json!({ "t_ms": t_ms, "by": side_name(me), "limb": limb, "kind": kind, "on": side_name(foe), "at": zone,
                                      "impulse": 0.0, "text": text, "miss": true, "ranged": false }));
            }
        }
    }
    // The rules default for a fighter with nothing in the window: circle with the initiative, else guard.
    let mut default: [Value; 2] = [Value::Null, Value::Null];
    for s in 0..2 {
        if strikes.iter().any(|x| x.side == s) {
            continue;
        }
        let foe = 1 - s;
        if c.initiative == s && shown == 0 {
            default[s] = "circle".into();
            let step = if r.below(2) == 0 { 1 } else { -1 };
            let to = [pos[s][0], pos[s][1] + step];
            if hex_dist(to, pos[foe]) >= 1 {
                pos[s] = to;
            }
            segs[s] = vec![json!({ "clip": "walk", "ms": if b.moves { play.min(1200) } else { play }, "loop": true, "to": pos[s], "path": "dash", "face": "foe" })];
        } else if segs[s].is_empty() {
            default[s] = "guard".into();
            if !b.moves {
                segs[s] = vec![json!({ "clip": "idle_guard", "ms": play, "loop": true, "face": "foe" })];
            }
        } else {
            default[s] = "guard".into();
        }
    }
    // Pad each track to the panel's length. (Schema 12: with steps around the foe, guard shuffles and feints of the
    // feet, not only a held guard; a fighter knocked down gets up when there is time.)
    for s in 0..2 {
        let used: i64 = segs[s].iter().map(|x| x["ms"].as_i64().unwrap_or(0)).sum();
        if used < play {
            let last = segs[s].last().and_then(|x| x["clip"].as_str()).unwrap_or("").to_string();
            let downed = matches!(last.as_str(), "downed" | "defeat");
            if !b.moves {
                segs[s].push(json!({ "clip": if downed { "downed" } else { "idle_guard" }, "ms": play - used, "loop": !downed }));
            } else if last == "defeat" || (downed && play - used < 1400) {
                segs[s].push(json!({ "clip": "downed", "ms": play - used, "loop": false }));
            } else {
                let mut left = play - used;
                if downed {
                    segs[s].push(json!({ "clip": "downed", "ms": 500, "loop": false }));
                    segs[s].push(json!({ "clip": if r.below(2) == 0 { "rise" } else { "spin_rise" }, "ms": 800 }));
                    left -= 1300;
                }
                let more = filler(&mut r, &objects, &mut pos, s, left);
                segs[s].extend(more);
            }
        }
    }
    if let Some(last) = window.last() {
        let hp = if last["hp_after"].is_object() { &last["hp_after"] } else { &last["hp"] };
        c.hp = [hp["a"].as_f64().unwrap_or(c.hp[0]), hp["b"].as_f64().unwrap_or(c.hp[1])];
    }
    c.pos = pos;
    let line_of = |s: usize| strikes.iter().rev().find(|x| x.side == s && x.typed).and_then(|x| x.ev["text"].as_str());
    let fighter = |s: usize| {
        let line = line_of(s);
        let mut f = json!({ "line": line, "line_masked_len": line.map(|l| l.chars().count()).unwrap_or(0),
                "lines": strikes.iter().filter(|x| x.side == s && x.typed).count(), "move_class": class[s],
                "default": default[s], "auto": strikes.iter().any(|x| x.side == s && !x.typed), "segs": segs[s], "at": pos[s] });
        if ai_types {
            f["typing_script"] = scripts.iter().find(|x| x.side == s).map(|x| x.json(&duel_id)).unwrap_or(Value::Null);
        }
        f
    };
    let idle = ["They circle. Nobody blinks.", "A breath. The hex holds still.", "Feet shift on the stone. Not yet."];
    let caption = match decisive {
        Some(d) => d.ev["flavor"].as_str().unwrap_or("").to_string(),
        None => idle[r.below(idle.len() as u64) as usize].to_string(),
    };
    let shot = match (ko, decisive) {
        (Some(_), _) => "low",
        (_, None) => "side",
        (_, Some(d)) => match d.ev["choreo"]["camera"].as_str().unwrap_or("wide") {
            "over_a" => "over_a",
            "over_b" => "over_b",
            "close" => if d.side == 0 { "close_a" } else { "close_b" },
            _ => "wide",
        },
    };
    // No input after a knockout, nor when the next slot starts at or past the deadline.
    let closed = ko.is_some() || (deadline > 0 && starts + play >= deadline);
    let closes = starts + play - LEAD_MS;
    // (A knockout's outro comes with it: close 0 is always due.)
    c.closes_ms = if ko.is_some() { 0 } else if closed { starts + play } else { closes };
    c.next_ms = starts + play;
    let kind = if ko.is_some() { "ko" } else if shown == 0 { "idle" } else { "exchange" };
    let panel = json!({
        "event": "duel.panel", "duel_id": b.duel.id, "n": n, "kind": kind,
        "starts_at_ms": starts, "play_ms": play, "input_opens_ms": (!closed).then_some(opens), "input_closes_ms": (!closed).then_some(closes),
        "caption": caption, "shot": shot, "a": fighter(0), "b": fighter(1), "contacts": contacts,
        // The blows this panel stages (their `n`; the full `duel.blow` events are in the duel's `blows`).
        "blows": window.iter().map(|e| e["n"].clone()).collect::<Vec<_>>(),
        "flip": init_before != c.initiative, "hp": { "a": c.hp[0], "b": c.hp[1] }, "range": c.range, "initiative": side_name(c.initiative),
    });
    b.panels.push(panel);
    b.cycle = Some(c);
    note_scripts(b, scripts);
}

/// Schema 12: `ms` of idle time for fighter `s` as short chunks: a step to a neighbouring hex at the same distance
/// from the foe (circling), a guard shuffle, a feint of the feet, a held guard. Moves `pos[s]`; the chunks sum to `ms`.
fn filler(r: &mut Rng, objects: &[Value], pos: &mut [[i32; 2]; 2], s: usize, ms: i64) -> Vec<Value> {
    let mut out = vec![];
    let mut left = ms;
    let mut last = usize::MAX;
    while left > 0 {
        let mut take = if left < 1300 { left } else { 700 + r.below(500) as i64 };
        if left - take < 500 {
            take = left;
        }
        left -= take;
        let foe = pos[1 - s];
        let steps: Vec<[i32; 2]> = [[1, 0], [-1, 0], [0, 1], [0, -1], [1, -1], [-1, 1]]
            .iter()
            .map(|d| [pos[s][0] + d[0], pos[s][1] + d[1]])
            .filter(|h| hex_dist(*h, foe) == hex_dist(pos[s], foe).max(1) && hex_dist(*h, [0, 0]) <= 5 && !objects.iter().any(|o| obj_at(o) == *h))
            .collect();
        let mut kind = r.below(5) as usize;
        if kind == last || (kind <= 1 && (steps.is_empty() || take < 600)) {
            kind = 2 + (kind + 1) % 3;
        }
        last = kind;
        out.push(match kind {
            0 | 1 => {
                pos[s] = steps[r.below(steps.len() as u64) as usize];
                json!({ "clip": "walk", "ms": take, "loop": true, "to": pos[s], "path": "dash", "face": "foe" })
            }
            2 => json!({ "clip": "idle_guard", "ms": take, "loop": true, "face": "foe" }),
            3 => json!({ "clip": "evade_stand", "ms": take, "face": "foe" }),
            _ => json!({ "clip": "block", "ms": take, "face": "foe" }),
        });
    }
    out
}

/// The nearest object within `reach` hexes of `at` that affords one of `paths` (any object when `paths` is empty).
fn nearest<'a>(objects: &'a [Value], at: [i32; 2], reach: i32, paths: &[&str]) -> Option<&'a Value> {
    objects
        .iter()
        .filter(|o| paths.is_empty() || o["affords"].as_array().is_some_and(|a| a.iter().any(|p| paths.contains(&p.as_str().unwrap_or("")))))
        .filter_map(|o| {
            let at_o = [o["at"][0].as_i64()? as i32, o["at"][1].as_i64()? as i32];
            Some((hex_dist(at, at_o), o))
        })
        .filter(|(d, _)| *d <= reach)
        .min_by_key(|(d, o)| (*d, o["id"].as_str().unwrap_or("").to_string()))
        .map(|(_, o)| o)
}

fn obj_at(o: &Value) -> [i32; 2] {
    [o["at"][0].as_i64().unwrap_or(0) as i32, o["at"][1].as_i64().unwrap_or(0) as i32]
}

/// Stage one strike in a slot of `ms`: the attacker's and the defender's segments (each track sums to `ms`), moving
/// `pos`. The class and the matrix's result pick the clips; the range after it sets where they end; the scene's
/// objects give the evasions their vaults, slides and wall kicks.
#[allow(clippy::too_many_arguments)]
fn stage(r: &mut Rng, objects: &[Value], pos: &mut [[i32; 2]; 2], me: usize, cls: &str, foot: bool, result: &str, dmg: f64, down: bool,
         range_after: Range, ms: i64, mv: &Value) -> (Vec<Value>, Vec<Value>) {
    let foe = 1 - me;
    let mut mine: Vec<Value> = vec![];
    let mut theirs: Vec<Value> = vec![];
    // Schema 12 (`duel.blow.move`): the clip the line's words picked, its setups and its leap.
    let word_clip = mv["clip"].as_str();
    let leaps = mv["path"] == "leap";
    let want = sep(range_after);
    let dir = if pos[me][0] <= pos[foe][0] { -1 } else { 1 }; // my side of the foe along q
    let spot = |p: [i32; 2], k: i32| [p[0] + dir * k, p[1]];
    // The run-in (or the step back): end the slot at the range the rules left them at.
    let mut left = ms;
    // The setups first ("plead! jump": the kneel, then the jump): a short beat each.
    for c in mv["setup"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        let t = (ms / 5).clamp(250, 450);
        mine.push(json!({ "clip": c, "ms": t, "face": "foe" }));
        left -= t;
    }
    let mut leapt = false;
    let moving = hex_dist(pos[me], pos[foe]) != want && !matches!(cls, "invoke" | "command" | "taunt" | "guard" | "parry");
    if moving {
        let to = spot(pos[foe], want);
        let run = (ms * 3 / 10).max(300);
        let leap = cls == "heavy" && hex_dist(pos[me], pos[foe]) > want + 1;
        if leaps {
            // The words asked for a leap: the whole move is the jump across.
            mine.push(json!({ "clip": word_clip.unwrap_or("jump_attack"), "ms": left, "to": to, "path": "leap", "h": 1.6, "face": "foe" }));
            leapt = true;
        } else if leap {
            mine.push(json!({ "clip": "jump_attack", "ms": run, "to": to, "path": "leap", "h": 1.6, "face": "foe" }));
        } else {
            let clip = if matches!(cls, "retreat") { "walk" } else { "run" };
            mine.push(json!({ "clip": clip, "ms": run, "loop": true, "to": to, "path": "dash", "face": "foe" }));
        }
        pos[me] = to;
        if !leapt {
            left -= run;
        }
    }
    let pick = |r: &mut Rng, xs: &[&'static str]| xs[r.below(xs.len() as u64) as usize];
    let atk = match cls {
        "strike" if foot => "kick",
        "strike" => pick(r, &["strike_slash", "strike_side", "lunge"]),
        "heavy" => pick(r, &["heavy_hammer", "heavy_charged", "ground_slam"]),
        "guard" => "block",
        "parry" => "parry_high",
        "feint" => "evade_spin",
        "evade" => "evade_stand",
        "close" => "lunge",
        "retreat" => "evade_stand",
        "grapple" => "grapple_throw_down",
        "throw" => "throw_shoulder",
        "invoke" => "invoke",
        "command" => "command",
        "taunt" => "taunt",
        _ => "stagger_waist", // a penalty: the words knotted into string
    };
    let crit = dmg >= 3.0 && cls == "strike";
    match word_clip {
        _ if leapt => {}
        // A leap from where it stands: up and down on its own hex.
        Some(c) if leaps && !moving => mine.push(json!({ "clip": c, "ms": left, "to": pos[me], "path": "leap", "h": 1.2, "face": "foe" })),
        Some(c) => mine.push(json!({ "clip": c, "ms": left, "face": "foe" })),
        None => mine.push(json!({ "clip": if crit { "combo_triple" } else { atk }, "ms": left, "face": "foe" })),
    }
    // A move that attacks nobody (a guard, a step, a taunt, a failed dodge): the other just holds.
    if !matches!(cls, "strike" | "heavy" | "grapple" | "throw" | "invoke" | "command" | "feint") {
        theirs.push(json!({ "clip": "idle_guard", "ms": ms, "loop": true, "face": "foe" }));
        return (mine, theirs);
    }
    // The defender: wait for it, then react.
    let wait = (ms - left) + left * 45 / 100;
    let react = ms - wait;
    theirs.push(json!({ "clip": "idle_guard", "ms": wait, "loop": true, "face": "foe" }));
    let away = |p: [i32; 2], k: i32| [p[0] - dir * k, p[1]];
    match result {
        _ if down => {
            let to = away(pos[foe], 1);
            theirs.push(json!({ "clip": "airborne_hit", "ms": react / 2, "to": to, "path": "knock", "h": 1.2, "spin": true }));
            theirs.push(json!({ "clip": "defeat", "ms": react - react / 2 }));
            pos[foe] = to;
        }
        "thrown" => {
            let to = away(pos[foe], 2);
            theirs.push(json!({ "clip": "airborne_hit", "ms": react / 2, "to": to, "path": "knock", "h": 2.2, "spin": true }));
            theirs.push(json!({ "clip": "downed", "ms": react - react / 2 }));
            pos[foe] = to;
        }
        // Schema 12: a hard hit lifts it off its feet and it gets up again.
        "landed" if word_clip.is_some() && dmg >= 3.0 && react >= 900 => {
            let to = away(pos[foe], 1);
            let (fly, lie) = (react * 4 / 10, react * 25 / 100);
            theirs.push(json!({ "clip": "airborne_hit", "ms": fly, "to": to, "path": "knock", "h": 1.0, "spin": true }));
            theirs.push(json!({ "clip": "downed", "ms": lie }));
            theirs.push(json!({ "clip": if r.below(2) == 0 { "rise" } else { "spin_rise" }, "ms": react - fly - lie }));
            pos[foe] = to;
        }
        "landed" if dmg >= 2.0 => {
            let to = away(pos[foe], 1);
            theirs.push(json!({ "clip": "stagger_back", "ms": react, "to": to, "path": "knock", "h": 0.4 }));
            pos[foe] = to;
        }
        "landed" => theirs.push(json!({ "clip": if r.below(2) == 0 { "stagger_waist" } else { "stagger_back" }, "ms": react })),
        "absorbed" => theirs.push(json!({ "clip": "block_cross", "ms": react })),
        "parried" => theirs.push(json!({ "clip": if r.below(2) == 0 { "parry_back" } else { "counter" }, "ms": react })),
        "grappled" => theirs.push(json!({ "clip": "stagger_waist", "ms": react })),
        "evaded" | "missed" => {
            // Out of the way, over, under or off whatever stands near.
            const WAYS: &[&str] = &["vault", "slide", "wallkick"];
            match nearest(objects, pos[foe], 2, WAYS) {
                Some(o) => {
                    let at = obj_at(o);
                    let affords: Vec<&str> = o["affords"].as_array().map(|a| a.iter().filter_map(Value::as_str).filter(|p| WAYS.contains(p)).collect())
                        .unwrap_or_default();
                    let path = affords[r.below(affords.len().max(1) as u64) as usize % affords.len().max(1)];
                    // The far side of it (over or under), or back where it stood (off a wall).
                    let over = [2 * at[0] - pos[foe][0], 2 * at[1] - pos[foe][1]];
                    let id = o["id"].clone();
                    let seg = match path {
                        "vault" => json!({ "clip": "run", "ms": react, "loop": true, "speed": 0.6, "to": over, "path": "vault", "over": id }),
                        "slide" => json!({ "clip": "stagger_back", "ms": react, "to": over, "path": "slide", "under": id }),
                        _ => json!({ "clip": "jump_attack", "ms": react, "to": pos[foe], "path": "wallkick", "on": id, "h": 1.4 }),
                    };
                    if matches!(path, "vault" | "slide") && over != pos[me] {
                        pos[foe] = over;
                    }
                    theirs.push(seg);
                }
                None => {
                    let to = [pos[foe][0], pos[foe][1] + if r.below(2) == 0 { 1 } else { -1 }];
                    theirs.push(json!({ "clip": if r.below(2) == 0 { "evade_roll" } else { "evade_spin" }, "ms": react, "to": to, "path": "dash" }));
                    pos[foe] = to;
                }
            }
        }
        // wasted, moved, set, taunted, penalty: the other holds its guard
        _ => theirs.push(json!({ "clip": "idle_guard", "ms": react, "loop": true, "face": "foe" })),
    }
    (mine, theirs)
}

/// The live `also[]` of a duel action: [`since`], and in a duel whose AI types (schema 12) also the AI's blows entered
/// since `blows_from` (`duel.blow` with `ai: true`, each before the panel that stages it; one not staged yet comes
/// last) and, after each panel, the typing scripts of the window it opens (`duel.typing_script`).
pub fn extras(b: &Bout, scene_too: bool, panels_from: usize, blows_from: usize) -> Vec<Value> {
    if !b.moves {
        return since(b, scene_too, panels_from);
    }
    let mut out = vec![];
    if scene_too {
        out.extend(b.scene.clone());
    }
    let mut blows: Vec<&Value> = b.blows.iter().skip(blows_from).filter(|e| e["ai"] == true).collect();
    for p in b.panels.iter().skip(panels_from) {
        let staged = |e: &Value| p["blows"].as_array().is_some_and(|a| a.contains(&e["n"]));
        out.extend(blows.iter().filter(|e| staged(e)).map(|e| (*e).clone()));
        blows.retain(|e| !staged(e));
        out.push(p.clone());
        for s in ["a", "b"] {
            if p[s]["typing_script"].is_object() {
                out.push(p[s]["typing_script"].clone());
            }
        }
    }
    out.extend(blows.into_iter().cloned());
    // The outro came out with this action: the duel is closed, here is who won (the resolve sends the deed).
    if b.panels.iter().skip(panels_from).any(|p| p["kind"] == "outro") {
        out.push(crate::duel::end_live_json(b));
    }
    out
}

/// The events a duel action made beyond its own: the scene and the panels added since `from` (for `also[]`).
pub fn since(b: &Bout, scene_too: bool, from: usize) -> Vec<Value> {
    let mut out = vec![];
    if scene_too {
        out.extend(b.scene.clone());
    }
    out.extend(b.panels.iter().skip(from).cloned());
    out
}

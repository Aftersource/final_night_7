//! Words to moves (state schema 12): a typed line's WORDS pick its move class and the clip its figure plays.
//!
//! The designer, after reading the challenge logs, asked to see which moves are being used and whether some can be mapped to
//! actual animations". The players type short commands, typos included ("block!", "atack!!", "jimpkick!", "judo
//! throw!", "use teh force", "plead! jump"), and the 0.8.1 classifier read almost all of them as a plain strike, so
//! the figures walked and idled.
//!
//! [`read`] finds every move word of a line ([`TABLE`]): exactly, as the start of a longer word ("tumblefight"), in
//! an inflected form, or, in a short command (at most four words), within one or two edits ("attck", "jimpkick").
//! The LAST one is the move (its class, its clips, a leap); the earlier ones are setups the figure plays first
//! ("plead! jump": kneel, then the jump). An invoke or a command wins wherever it stands (they cost a bolt or need an
//! army). A line with no move word is classified as in 0.8.1 ([`crate::choreo::classify`]).
//!
//! This only chooses the class and the clips. The d20, the modifiers and the matrix still decide what happens.

use fn7_model::seed::Rng;

use crate::choreo::{self, MoveClass, Range};

/// One row: the words, the class they ask for, the clips that class may play for them, and its options.
pub struct Row {
    pub words: &'static [&'static str],
    pub class: MoveClass,
    pub clips: &'static [&'static str],
    /// The figure leaves the ground (a `leap` path).
    pub leap: bool,
    /// Only the move when no other row matched (a body part that is usually the target: "kick your knee").
    pub weak: bool,
    /// Typos may match (short commands only).
    pub fuzzy: bool,
}

const fn row(words: &'static [&'static str], class: MoveClass, clips: &'static [&'static str]) -> Row {
    Row { words, class, clips, leap: false, weak: false, fuzzy: true }
}

/// The move words. A word with a space is a phrase.
pub const TABLE: &[Row] = &[
    Row { leap: true, ..row(&["jumpkick", "flying kick", "jump kick", "dropkick"], MoveClass::Strike, &["flying_kick"]) },
    row(&["roundhouse"], MoveClass::Strike, &["roundhouse_kick"]),
    row(&["kick"], MoveClass::Strike, &["kick", "roundhouse_kick", "high_kick"]),
    row(&["sweep", "trip"], MoveClass::Strike, &["leg_sweep"]),
    Row { leap: true, ..row(&["jump", "leap", "vault", "pounce", "dive"], MoveClass::Strike, &["jump_attack"]) },
    row(&["punch", "attack", "strike", "fight", "hit", "bash", "pummel", "batter"], MoveClass::Strike, &["jab", "punch_combo", "strike_side"]),
    row(&["jab"], MoveClass::Strike, &["jab"]),
    row(&["uppercut"], MoveClass::Heavy, &["uppercut"]),
    Row { weak: true, ..row(&["elbow"], MoveClass::Strike, &["elbow_strike"]) },
    Row { weak: true, ..row(&["knee"], MoveClass::Strike, &["knee_strike"]) },
    row(&["flail", "frenzy", "combo", "flurry"], MoveClass::Strike, &["combo_triple", "punch_combo"]),
    row(&["spin", "whirl"], MoveClass::Strike, &["blade_spin", "axe_spin"]),
    row(&["slash", "slice", "cut", "lash", "whip", "claw", "rake"], MoveClass::Strike, &["strike_slash", "blade_spin"]),
    row(&["stab", "pierce", "impale", "skewer", "lunge", "thrust", "spear"], MoveClass::Strike, &["lunge"]),
    row(&["smash", "crush", "cleave", "overhead", "haymaker", "headbutt", "pulverize", "obliterate", "full force", "all my strength"],
        MoveClass::Heavy, &["heavy_hammer", "heavy_charged", "ground_slam", "sword_judgment"]),
    row(&["block", "guard", "shield", "brace", "defend"], MoveClass::Guard, &["block", "block_cross", "shield_push"]),
    row(&["parry", "deflect", "counter", "riposte"], MoveClass::Parry, &["parry_high", "parry_back", "counter"]),
    row(&["throw", "judo", "toss", "slam", "hurl", "suplex", "flip", "body-slam"], MoveClass::Throw, &["throw_shoulder", "grapple_throw_down"]),
    row(&["grab", "choke", "wrestle", "clinch", "seize", "headlock", "throttle", "hold you", "pin", "hug"], MoveClass::Grapple, &["grapple_throw_down"]),
    row(&["tumble", "roll", "dodge", "duck", "sidestep", "evade", "weave", "vanish"], MoveClass::Evade, &["evade_roll", "dodge_roll_back", "evade_spin"]),
    row(&["run", "charge", "rush", "advance", "approach", "dash", "close in", "step in"], MoveClass::CloseIn, &["lunge"]),
    row(&["retreat", "withdraw", "flee", "back off", "step back", "fall back", "back away"], MoveClass::Retreat, &["dodge_roll_back", "evade_stand"]),
    row(&["summon", "cast", "spell", "magic", "the force", "force", "curse", "conjure", "enchant", "invoke", "smite", "hex"], MoveClass::Invoke,
        &["invoke", "mage_cast", "ground_slam"]),
    Row { fuzzy: false, ..row(&["time", "nature", "consequence", "death", "chance", "memory", "entropy", "god", "gods", "divine", "heavens"],
                              MoveClass::Invoke, &["invoke", "mage_cast"]) },
    row(&["army", "armies", "legion", "troops", "soldiers", "followers", "host", "horde", "battalion", "faithful", "minions"], MoveClass::Command, &["command"]),
    row(&["plead", "beg", "pray", "mercy", "please"], MoveClass::Taunt, &["kneel", "bow"]),
    row(&["taunt", "mock", "laugh", "insult", "jeer", "sneer", "yawn", "scoff"], MoveClass::Taunt, &["taunt"]),
    row(&["feint", "fake", "trick", "bluff", "pretend"], MoveClass::Feint, &["evade_spin"]),
];

/// The clips a class plays when the line names none.
pub fn defaults(class: MoveClass) -> &'static [&'static str] {
    match class {
        MoveClass::Strike => &["strike_slash", "strike_side", "lunge", "jab", "kick", "elbow_strike", "punch_combo"],
        MoveClass::Heavy => &["heavy_hammer", "heavy_charged", "ground_slam", "sword_judgment", "axe_spin", "uppercut"],
        MoveClass::Guard => &["block", "block_cross", "shield_push"],
        MoveClass::Parry => &["parry_high", "parry_back"],
        MoveClass::Feint => &["evade_spin"],
        MoveClass::Evade => &["evade_stand", "evade_roll", "dodge_roll_back", "evade_spin"],
        MoveClass::CloseIn => &["lunge"],
        MoveClass::Retreat => &["evade_stand", "dodge_roll_back"],
        MoveClass::Grapple => &["grapple_throw_down"],
        MoveClass::Throw => &["throw_shoulder", "grapple_throw_down"],
        MoveClass::Invoke => &["invoke", "mage_cast"],
        MoveClass::Command => &["command"],
        MoveClass::Taunt => &["taunt"],
    }
}

/// What each clip is, for whoever (or whatever) picks from the list: `(clip, classes it serves, one line)`.
pub const CLIP_INFO: &[(&str, &str, &str)] = &[
    ("airborne_hit", "reaction", "knocked off the feet, flying back"),
    ("block", "guard", "a forearm block; also a guard shuffle between blows"),
    ("block_cross", "guard", "both arms crossed against a heavy blow"),
    ("bow", "taunt", "a bow: an appeal, a draw, respect"),
    ("combo_triple", "strike", "three fast hits in a row"),
    ("command", "command", "an arm thrown forward: the army is ordered in"),
    ("counter", "parry", "a parry that turns straight into a riposte"),
    ("defeat", "reaction", "knocked out, down for good"),
    ("downed", "reaction", "lying on the ground after a throw or a hard hit"),
    ("evade_roll", "evade", "a forward tumble out of the way"),
    ("evade_spin", "evade, feint", "a spin aside; a feint's turn"),
    ("evade_stand", "evade, retreat", "a standing lean or step out of reach"),
    ("grapple_throw_down", "grapple, throw", "taking hold and dragging the foe down"),
    ("ground_slam", "heavy, invoke", "both fists into the ground: a shockwave"),
    ("heavy_charged", "heavy", "a wound-up, charged heavy blow"),
    ("heavy_hammer", "heavy", "an overhead hammer blow"),
    ("idle_guard", "idle", "standing on guard"),
    ("invoke", "invoke", "arms raised: a god is called"),
    ("jump_attack", "strike, heavy", "a leap that comes down with a blow"),
    ("kick", "strike", "a front kick"),
    ("kneel", "taunt", "on one knee: pleading, begging, praying"),
    ("lunge", "strike, close", "a long step in with a thrust"),
    ("parry_back", "parry", "a parry stepping back"),
    ("parry_high", "parry", "a high parry"),
    ("rise", "reaction", "getting up from the ground"),
    ("run", "close", "running (with a dash path)"),
    ("shield_push", "guard", "a guard that shoves forward"),
    ("stagger_back", "reaction", "hit and stumbling back"),
    ("stagger_waist", "reaction", "hit in the body, doubling over"),
    ("strike_side", "strike", "a side strike with the hand"),
    ("strike_slash", "strike", "a slash"),
    ("taunt", "taunt", "a beckoning taunt"),
    ("throw_shoulder", "throw", "a shoulder throw (judo)"),
    ("victory", "outro", "the winner's pose"),
    ("walk", "idle, retreat", "walking (circling steps, stepping back)"),
    ("flying_kick", "strike", "a flying kick, both feet off the ground"),
    ("roundhouse_kick", "strike", "a roundhouse kick"),
    ("high_kick", "strike", "a kick to the head"),
    ("leg_sweep", "strike", "a low sweep at the legs"),
    ("elbow_strike", "strike", "an elbow"),
    ("uppercut", "heavy", "an uppercut"),
    ("jab", "strike", "a quick jab"),
    ("punch_combo", "strike", "a flurry of punches"),
    ("blade_spin", "strike", "a spinning slash"),
    ("axe_spin", "heavy", "a heavy spinning blow"),
    ("spin_rise", "reaction", "spinning up from the ground"),
    ("mage_cast", "invoke", "casting a spell with both hands"),
    ("dodge_roll_back", "evade, retreat", "a roll backwards"),
    ("knee_strike", "strike", "a knee"),
    ("sword_judgment", "heavy", "a great downward judgment blow"),
    ("bow_draw_shoot", "reserved: bow", "a full draw and a shot"),
    ("bow_quick_shot", "reserved: bow", "a snap shot"),
    ("bow_aim_walk", "reserved: bow", "walking with the bow drawn"),
    ("gun_side_shot", "reserved: rifle", "a shot from the hip, side on"),
    ("gun_run_shoot", "reserved: rifle", "shooting on the run"),
    ("gun_quick_draw", "reserved: rifle", "a quick draw and a shot"),
    ("gun_roll_fire", "reserved: rifle", "a roll that comes up firing"),
    ("gun_reload", "reserved: rifle", "reloading"),
    ("launcher_fire", "reserved: launcher", "a rocket launcher fired from the shoulder"),
    ("twohand_chop", "reserved: two_handed", "a two-handed chop"),
    ("twohand_reap", "reserved: two_handed", "a wide two-handed reaping swing"),
    ("twohand_stance", "reserved: two_handed", "a two-handed guard stance"),
    ("throw_object", "reserved: thrown", "throwing a held object"),
    ("draw_from_nowhere", "reserved: hammer-space", "pulling an item out of nowhere"),
];

/// Measured clip lengths (seconds; the Unreal import, read from the GLBs). The others are not measured yet.
pub const CLIP_LEN: &[(&str, f64)] = &[
    ("flying_kick", 4.67), ("roundhouse_kick", 2.67), ("high_kick", 2.03), ("leg_sweep", 2.47), ("knee_strike", 2.50), ("elbow_strike", 2.47),
    ("uppercut", 1.00), ("jab", 1.73), ("punch_combo", 2.47), ("blade_spin", 5.67), ("axe_spin", 2.50), ("spin_rise", 8.00), ("mage_cast", 3.00),
    ("dodge_roll_back", 1.27), ("sword_judgment", 4.33), ("bow_draw_shoot", 5.00), ("bow_quick_shot", 3.73), ("bow_aim_walk", 1.20),
    ("gun_side_shot", 4.00), ("gun_run_shoot", 0.67), ("gun_quick_draw", 7.30), ("gun_roll_fire", 3.33), ("gun_reload", 3.30), ("launcher_fire", 7.83),
    ("twohand_chop", 7.67), ("twohand_reap", 5.90), ("twohand_stance", 6.10), ("throw_object", 5.80), ("draw_from_nowhere", 5.33),
    ("idle_guard", 1.67), ("strike_slash", 1.50), ("run", 0.63), ("jump_attack", 5.87), ("evade_roll", 1.83),
];

/// A clip's first-guess trim from its length, by rule (nobody has looked at it: `trim: "estimated"`): a clip of 5 s
/// or more starts 35% in and uses 40% of its length (the wind-up and the recovery are skipped), a shorter one plays
/// whole; an attack's contact is 45% into the used part. `(len_s, at_ms, use_ms, hit_ms)`.
pub fn trim(clip: &str) -> Option<(f64, i64, i64, Option<i64>)> {
    let len = CLIP_LEN.iter().find(|c| c.0 == clip)?.1;
    let ms = (len * 1000.0).round() as i64;
    let (at, used) = if len >= 5.0 { (ms * 35 / 100, ms * 40 / 100) } else { (0, ms) };
    let about = CLIP_INFO.iter().find(|c| c.0 == clip).map(|c| c.1).unwrap_or("");
    let attack = ["strike", "heavy", "throw", "bow", "rifle", "launcher", "two_handed", "thrown", "invoke"].iter().any(|k| about.contains(k))
        && !["twohand_stance", "bow_aim_walk", "gun_reload"].contains(&clip);
    Some((len, at, used, attack.then_some(used * 45 / 100)))
}

/// What each move class means, in one line.
pub const CLASS_INFO: &[(&str, &str)] = &[
    ("strike", "a normal attack; from far it is a step in"),
    ("heavy", "a slow, strong attack: breaks guards and parries, leaves its user committed"),
    ("guard", "sets a guard: the next strike is absorbed"),
    ("parry", "sets a parry: the next strike is turned and the initiative flips"),
    ("feint", "wastes the defence the foe has set and takes the initiative"),
    ("evade", "sets an evasion: a heavy misses and its user is left open"),
    ("close", "steps one range band in"),
    ("retreat", "steps one range band out"),
    ("grapple", "takes hold (close range): the foe is off balance"),
    ("throw", "throws the foe down (from close range or a grapple)"),
    ("invoke", "a god's power: ignores range, costs a bolt (a taunt without one)"),
    ("command", "orders the army in: needs one near (a taunt without one)"),
    ("taunt", "words only: takes the initiative, unsettles the foe; also an appeal (plead, beg)"),
];

/// THE LIST (docs/DUEL_MOVES.md): everything a line can be mapped to, as data: the classes, the clips (with what
/// they are, the classes they serve and the fallback of a new one), the paths, and the move words. `version` changes
/// whenever any of it does.
pub fn list() -> serde_json::Value {
    use serde_json::json;
    let fallback = |c: &str| crate::panel::CLIP_FALLBACKS.iter().find(|f| f.0 == c).map(|f| f.1);
    let mut v = json!({
        "classes": CLASS_INFO.iter().map(|(c, about)| json!({ "class": c, "about": about })).collect::<Vec<_>>(),
        "clips": CLIP_INFO.iter().map(|(c, classes, about)| {
            let mut e = json!({ "clip": c, "for": classes, "about": about, "fallback": fallback(c) });
            // The trim a seg should play: start `at_ms` into the clip, play `use_ms` of it (speed = use_ms / seg.ms),
            // the contact `hit_ms` into the used part. Estimated by rule until someone corrects it by eye.
            if let Some((len, at, used, hit)) = trim(c) {
                e["len_s"] = len.into();
                e["at_ms"] = at.into();
                e["use_ms"] = used.into();
                e["hit_ms"] = hit.into();
                e["trim"] = "estimated".into();
            }
            e
        }).collect::<Vec<_>>(),
        "paths": [
            { "path": "dash", "about": "running or stepping along the ground" }, { "path": "leap", "about": "a jump to a hex (h: apex)" },
            { "path": "knock", "about": "thrown or knocked back" }, { "path": "vault", "about": "over an object (over: its id)" },
            { "path": "slide", "about": "under an object (under: its id)" }, { "path": "wallkick", "about": "off an object (on: its id)" },
            { "path": "flip", "about": "reserved" }, { "path": "wallrun", "about": "reserved" }, { "path": "mantle", "about": "reserved" }, { "path": "drop", "about": "reserved" }],
        "words": TABLE.iter().map(|r| json!({ "words": r.words, "class": r.class, "clips": r.clips, "leap": r.leap, "weak": r.weak, "typos": r.fuzzy })).collect::<Vec<_>>(),
    });
    v["version"] = format!("{:016x}", fn7_model::seed::hash_str(&v.to_string())).into();
    v
}

/// The list's version ([`list`]).
pub fn list_version() -> String {
    list()["version"].as_str().unwrap_or("").to_string()
}

/// A line as the rollups count it: lower case, words only, single spaces.
pub fn normalize(text: &str) -> String {
    text.to_lowercase().split(|c: char| !(c.is_alphanumeric() || c == '-')).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Real words one edit from a move word: never a typo of it.
const NOT_TYPOS: &[&str] = &[
    "fade", "face", "fame", "fate", "free", "flea", "play", "prey", "rash", "rust", "dish", "dove", "dime", "last", "lush", "slab", "star", "lean",
    "leaf", "span", "spit", "slim", "slum", "tons", "case", "cost", "cart", "lick", "kink", "dock", "deck", "trap", "trim", "sick", "pick", "grub",
    "black", "clock", "sweet", "sleep", "knew", "role", "rule", "spill", "sell", "flair", "frail", "change", "charm", "chart", "your", "you", "then",
    "that", "this", "with", "into", "from", "have", "just", "like", "them", "they", "what", "when", "will", "hand", "head", "back", "down", "again",
    "them", "here", "there", "left", "right", "under", "over", "pinch", "bunch", "lunch", "munch", "hunch", "junk", "bump", "dump", "lump", "pump",
    "temple", "ribs", "shin", "chin", "nose", "ankle", "stance", "footwork", "collar", "twice", "wrist", "skull", "spine", "throat", "thigh",
    "blood", "blade", "trick", "truck", "track", "stand", "still", "stay", "slow", "easy", "cute", "nice", "wait", "want", "mast", "mist", "must",
];

/// Optimal string alignment distance (insert, delete, substitute, swap neighbours), capped at 3.
fn edits(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len().abs_diff(b.len()) > 2 {
        return 3;
    }
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b.len() {
        d[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = (a[i - 1] != b[j - 1]) as usize;
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()].min(3)
}

/// How well `w` is the move word `k`: 0 = exactly (or inflected), 1 = it starts with it, 2 + n = within n edits
/// (`fuzzy`). `None`: it is not.
fn score(w: &str, k: &str, fuzzy: bool) -> Option<usize> {
    if w == k {
        return Some(0);
    }
    let n = k.chars().count();
    if let Some(rest) = w.strip_prefix(k) {
        let last = k.chars().last().unwrap_or(' ');
        let doubled = rest.strip_prefix(last).unwrap_or(rest);
        if ["s", "es", "ed", "ing", "d"].iter().any(|x| rest == *x || doubled == *x) {
            return Some(0);
        }
        if n >= 4 {
            return Some(1);
        }
    }
    let m = w.chars().count();
    if !fuzzy || n < 4 || m < 4 || w.chars().next() != k.chars().next() || NOT_TYPOS.contains(&w) {
        return None;
    }
    let max = if n <= 5 { 1 } else { 2 };
    let e = edits(w, k);
    (e <= max).then_some(2 + e)
}

/// What a line's words ask for.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub class: MoveClass,
    /// The clips the move may play.
    pub clips: &'static [&'static str],
    pub leap: bool,
    /// The move word as the table has it ("attack" for "attck").
    pub word: &'static str,
    /// The clips of the earlier move words, in order (at most two): played first, as setups.
    pub setup: Vec<&'static str>,
}

/// Every move word of `text` in order, then the move: see the module docs. `None`: no move word.
pub fn read(text: &str) -> Option<Read> {
    let t = text.to_lowercase();
    // The words with their byte offsets.
    let mut ws: Vec<(usize, &str)> = vec![];
    let mut start = None;
    for (i, c) in t.char_indices().chain(std::iter::once((t.len(), ' '))) {
        let inside = c.is_alphanumeric() || c == '-';
        match (start, inside) {
            (None, true) => start = Some(i),
            (Some(s), false) => {
                let w = t[s..i].trim_matches('-');
                if !w.is_empty() {
                    ws.push((s, w));
                }
                start = None;
            }
            _ => {}
        }
    }
    let short = ws.len() <= 4;
    // (offset, row, word of the table, score)
    let mut found: Vec<(usize, &Row, &'static str, usize)> = vec![];
    for (at, w) in &ws {
        let mut best: Option<(&Row, &'static str, usize)> = None;
        for r in TABLE {
            for k in r.words.iter().filter(|k| !k.contains(' ')) {
                if let Some(s) = score(w, k, short && r.fuzzy) {
                    // The best score; among equals the longest word ("jumpkick" before "jump").
                    if best.is_none_or(|b| s < b.2 || (s == b.2 && k.len() > b.1.len())) {
                        best = Some((r, k, s));
                    }
                }
            }
        }
        if let Some((r, k, s)) = best {
            found.push((*at, r, k, s));
        }
    }
    for r in TABLE {
        for k in r.words.iter().filter(|k| k.contains(' ')) {
            if let Some(at) = t.find(k) {
                // A phrase replaces the words inside it.
                found.retain(|f| f.0 < at || f.0 >= at + k.len());
                found.push((at, r, k, 0));
            }
        }
    }
    found.sort_by_key(|f| f.0);
    // A weak word (a body part that is usually the target) is a move only in a bare command ("knee!", "elbow him").
    if found.iter().any(|f| !f.1.weak) || ws.len() > 3 {
        found.retain(|f| !f.1.weak);
    }
    let k = found.iter().rposition(|f| matches!(f.1.class, MoveClass::Invoke | MoveClass::Command)).or(found.len().checked_sub(1))?;
    let (_, r, word, _) = found[k];
    let mut setup: Vec<&'static str> = vec![];
    for f in found.iter().take(k) {
        let c = f.1.clips[0];
        if !setup.contains(&c) && c != r.clips[0] && setup.len() < 2 {
            setup.push(c);
        }
    }
    Some(Read { class: r.class, clips: r.clips, leap: r.leap, word, setup })
}

/// The move class a line asks for (schema 12): its move word's, else the 0.8.1 reading.
pub fn classify(text: &str) -> MoveClass {
    read(text).map(|r| r.class).unwrap_or_else(|| choreo::classify(text))
}

/// What the class can be here (schema 12): as [`choreo::effective`], except that a throw works from close range (it
/// takes hold and throws in one move) and from farther away is a step in.
pub fn effective(class: MoveClass, range: Range, has_bolt: bool, has_army: bool) -> MoveClass {
    match class {
        MoveClass::Throw if range >= Range::Close => MoveClass::Throw,
        MoveClass::Throw => MoveClass::CloseIn,
        c => choreo::effective(c, range, has_bolt, has_army),
    }
}

/// The clip a strike plays: one of the line's (when what it asked for is what it became; a spell with no bolt behind
/// it still shows its casting), else one of the class's; never the clip this fighter played last when there is a choice.
pub fn clip(read: Option<&Read>, class: MoveClass, last: Option<&str>, r: &mut Rng) -> &'static str {
    let list = match read {
        Some(x) if x.class == class || (matches!(x.class, MoveClass::Invoke | MoveClass::Command) && class == MoveClass::Taunt) => x.clips,
        _ => defaults(class),
    };
    let fresh: Vec<&'static str> = list.iter().copied().filter(|c| Some(*c) != last).collect();
    let from = if fresh.is_empty() { list.to_vec() } else { fresh };
    from[r.below(from.len() as u64) as usize]
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use MoveClass::*;

    /// The designer's own lines from the challenge logs (typos included): the class and the clips each one picks.
    pub(crate) const LOGGED: &[(&str, MoveClass, &[&str], &[&str])] = &[
        ("block!", Guard, &["block", "block_cross", "shield_push"], &[]),
        ("jump!", Strike, &["jump_attack"], &[]),
        ("attack!", Strike, &["jab", "punch_combo", "strike_side"], &[]),
        ("atack!!", Strike, &["jab", "punch_combo", "strike_side"], &[]),
        ("attck!", Strike, &["jab", "punch_combo", "strike_side"], &[]),
        ("run!", CloseIn, &["lunge"], &[]),
        ("throw!", Throw, &["throw_shoulder", "grapple_throw_down"], &[]),
        ("judo throw!", Throw, &["throw_shoulder", "grapple_throw_down"], &[]),
        ("jumpkick!", Strike, &["flying_kick"], &[]),
        ("jimpkick!", Strike, &["flying_kick"], &[]),
        ("tumblefight!", Evade, &["evade_roll", "dodge_roll_back", "evade_spin"], &[]),
        ("flail!", Strike, &["combo_triple", "punch_combo"], &[]),
        ("summon undead!", Invoke, &["invoke", "mage_cast", "ground_slam"], &[]),
        ("use teh force", Invoke, &["invoke", "mage_cast", "ground_slam"], &[]),
        ("plead! jump", Strike, &["jump_attack"], &["kneel"]),
    ];

    #[test]
    fn the_logged_lines_pick_their_class_and_clips() {
        for (line, class, clips, setup) in LOGGED {
            let r = read(line).unwrap_or_else(|| panic!("{line:?} has a move word"));
            assert_eq!((r.class, r.clips, &r.setup[..]), (*class, *clips, *setup), "{line:?} ({})", r.word);
            assert_eq!(classify(line), *class);
            for c in r.clips.iter().chain(r.setup.iter()) {
                assert!(crate::panel::CLIPS.contains(c), "{c}");
            }
        }
        assert!(read("jumpkick!").unwrap().leap && read("jump!").unwrap().leap && !read("block!").unwrap().leap);
        // The lead's table.
        for (line, class, clip) in [
            ("parry", Parry, "parry_high"), ("gaurd!", Guard, "block"), ("toss him", Throw, "throw_shoulder"), ("slam", Throw, "throw_shoulder"),
            ("leap!", Strike, "jump_attack"), ("flying kick", Strike, "flying_kick"), ("kick", Strike, "kick"), ("sweep", Strike, "leg_sweep"),
            ("punch", Strike, "jab"), ("jab", Strike, "jab"), ("uppercut!", Heavy, "uppercut"), ("elbow", Strike, "elbow_strike"), ("knee!", Strike, "knee_strike"),
            ("frenzy", Strike, "combo_triple"), ("combo", Strike, "combo_triple"), ("spin", Strike, "blade_spin"), ("roll", Evade, "evade_roll"),
            ("dodge!", Evade, "evade_roll"), ("charge", CloseIn, "lunge"), ("rush him", CloseIn, "lunge"), ("retreat", Retreat, "dodge_roll_back"),
            ("back off", Retreat, "dodge_roll_back"), ("cast", Invoke, "invoke"), ("spell!", Invoke, "invoke"), ("magic", Invoke, "invoke"),
            ("curse you", Invoke, "invoke"), ("beg", Taunt, "kneel"), ("pray", Taunt, "kneel"), ("taunt", Taunt, "taunt"), ("mock", Taunt, "taunt"),
            ("laugh", Taunt, "taunt"), ("my army, charge!", Command, "command"), ("pary!", Parry, "parry_high"), ("thorw", Throw, "throw_shoulder"),
            ("doge!", Evade, "evade_roll"), ("sumon", Invoke, "invoke"), ("retrat!", Retreat, "dodge_roll_back"), ("punhc", Strike, "jab"),
        ] {
            let r = read(line).unwrap_or_else(|| panic!("{line:?}"));
            assert_eq!((r.class, r.clips[0]), (class, clip), "{line:?} ({})", r.word);
        }
    }

    #[test]
    fn sentences_read_as_before_unless_a_move_word_says_otherwise() {
        // The last move word is the move; a body part that is the target is not one; typos only count in short commands.
        assert_eq!((classify("I kick your knee into the mud"), read("I kick your knee into the mud").unwrap().clips[0]), (Strike, "kick"));
        assert_eq!(classify("then I smash your jaw with my elbow"), Heavy);
        assert_eq!(classify("I parry your blade"), Parry);
        assert_eq!(classify("Death, take him"), Invoke);
        assert_eq!(classify("I step back out of reach"), Retreat);
        assert_eq!(classify("you fight like a farmer, I mock you"), Taunt);
        assert_eq!(classify("I hook your chin"), Strike, "no move word: the 0.8.1 reading");
        assert!(read("My fist finds your knee.").is_none() && read("knee him!").is_some_and(|r| r.clips == ["knee_strike"]), "a target is not a move");
        assert!(read("I hook your chin").is_none());
        let r = read("I duck under it and stab your ribs").unwrap();
        assert_eq!((r.class, r.clips, &r.setup[..]), (Strike, &["lunge"][..], &["evade_roll"][..]), "duck, then the stab");
        assert_eq!(classify("I play my last card and face the black clock"), Strike, "real words are not typos");
        assert!(read("the sick old case went on and on").is_none(), "nor in a long line");
        assert_eq!(classify("I summon a storm and kick you"), Invoke, "an invoke wins wherever it stands");
        // A throw works from close range; from farther it is a step in.
        assert_eq!((effective(Throw, Range::Close, true, true), effective(Throw, Range::Grapple, true, true), effective(Throw, Range::Mid, true, true)),
                   (Throw, Throw, CloseIn));
        assert_eq!(effective(Invoke, Range::Far, false, true), Taunt);
        // The clip: the line's, never twice in a row when there is a choice; a spell with no bolt still casts.
        let rd = read("kick").unwrap();
        let mut r = Rng::new(4);
        let mut last = None;
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..40 {
            let c = clip(Some(&rd), Strike, last, &mut r);
            assert!(Some(c) != last && rd.clips.contains(&c));
            seen.insert(c);
            last = Some(c);
        }
        assert_eq!(seen.len(), 3);
        assert!(["invoke", "mage_cast", "ground_slam"].contains(&clip(read("summon undead!").as_ref(), Taunt, None, &mut r)));
        assert!(defaults(CloseIn).contains(&clip(read("throw!").as_ref(), CloseIn, None, &mut r)), "a throw that became a step in plays the step");
        // The list: every clip of the vocabulary is described, every new one has its fallback, the version is stable.
        let l = list();
        assert_eq!(l["clips"].as_array().unwrap().len(), crate::panel::CLIPS.len());
        assert!(crate::panel::CLIPS.iter().all(|c| CLIP_INFO.iter().any(|i| i.0 == *c)));
        assert!(l["clips"].as_array().unwrap().iter().filter(|c| c["fallback"].is_string()).count() == 29 && l["classes"].as_array().unwrap().len() == 13);
        assert_eq!((l["version"].as_str().map(str::len), list_version()), (Some(16), list_version()));
        assert_eq!(normalize("  Atack!!  the KNEE-cap, now"), "atack the knee-cap now");
        // Trims: a long clip skips its wind-up, a short one plays whole; attacks have a contact frame inside the used part.
        assert_eq!((trim("twohand_chop"), trim("jab"), trim("spin_rise"), trim("block")),
                   (Some((7.67, 2684, 3068, Some(1380))), Some((1.73, 0, 1730, Some(778))), Some((8.0, 2800, 3200, None)), None));
        assert!(CLIP_LEN.iter().all(|c| crate::panel::CLIPS.contains(&c.0)));
        // data/duel_moves.json is this list (regenerate: `fn7-duel-lines --list > data/duel_moves.json`).
        let file: serde_json::Value = serde_json::from_str(include_str!("../../../data/duel_moves.json")).expect("data/duel_moves.json");
        assert_eq!(file, l, "data/duel_moves.json is stale: fn7-duel-lines --list > data/duel_moves.json");
        for class in [Strike, Heavy, Guard, Parry, Feint, Evade, CloseIn, Retreat, Grapple, Throw, Invoke, Command, Taunt] {
            assert!(defaults(class).iter().all(|c| crate::panel::CLIPS.contains(c)));
        }
        assert!(TABLE.iter().all(|r| !r.clips.is_empty() && r.clips.iter().all(|c| crate::panel::CLIPS.contains(c))));
    }
}

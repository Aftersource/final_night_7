//! Duel words (state schema 21, docs/DUEL_MOVES.md §5): what a typed duel line NAMES reaches the stage.
//!
//! The designer asked that duel characters look like the player's main character fighting, with actions such as pulling out a sword visible
//! see it pulling out a sword or gun or whatever player comes up with" / "i should be able to say hide behind that
//! tree then pop up and slit his throat or duck roll to the left like i should be able to get as descriptive as
//! possible then see that play out in real time".
//!
//! Benito added that actions should be limited to items in the player's inventory, and that items on the board can also be used
//! see on the chest board there is trees and rocks u can use all whats in the board and in ur inventory so u might not
//! have a rock in ur inventory but there are rocks on the ground u could pick up dirt and throw in eyes etc".
//!
//! Pure rules on the words, read by the bout when a line is struck:
//!
//! - [`Usable`]: what a fighter can use: the items it holds (from the state at the lock), the duel board's objects
//!   (`duel.scene.objects`) and the hex's ground (dirt, sand, mud, snow, ash: [`ground_of`]).
//! - [`uses`]: what a line names and whether it is there: a held item ("draw my sword": THAT item is drawn, with its
//!   real name and colour), a board object ("hide behind that tree", "throw a rock"), the ground ("throw dirt in his
//!   eyes"). A thing named that is not there ("I draw a pistol" with none held, "that tree" with no tree) is a whiff:
//!   the blow fails ("you have no pistol"). There are no new hammer-space draws and nothing is summoned.
//! - [`plan`]: the line as up to [`MAX_STEPS`] staged steps (clip, path, the object, the body part), read clause by
//!   clause ("then", "and then", "and", "before"). The steps are presentation only: the blow is still one d20 and one
//!   matrix result. The plan's `class` turns a pure movement line ("roll left", "jump", "run in circles", "hide
//!   behind the cart") into an evade (a d20 for positioning) and a throw of a thing into a strike.

use fn7_model::manifest::ItemState;
use fn7_model::{DemigodId, ItemId, Ref, World};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::choreo::MoveClass;

/// Steps one line may stage.
pub const MAX_STEPS: usize = 4;
/// The step paths (schema 21; the panel turns them into the lab's `dash` / `leap` segs): to the named object's far
/// side, a side step or roll, back, in to the foe, a jump on the spot, a run around in a ring.
pub const STEP_PATHS: &[&str] = &["cover", "left", "right", "back", "in", "leap", "circle"];
/// The body parts a step may aim at (schema 21 adds `neck`).
pub const STEP_PARTS: &[&str] = &["head", "neck", "chest", "belly", "arms", "legs"];
/// The ground a line may pick up and throw.
pub const GROUND_KINDS: &[&str] = &["dirt", "sand", "mud", "snow", "ash", "gravel", "leaves", "water"];

fn words(s: &str) -> Vec<String> {
    s.to_lowercase().split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '\'')).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

/// A word of the line that names a cover kind (plurals and synonyms), as the kind.
fn cover_word(w: &str) -> Option<&'static str> {
    let w = w.trim_end_matches("'s");
    Some(match w {
        "tree" | "trees" | "oak" | "pine" | "trunk" => "tree",
        "rock" | "rocks" | "stone" | "stones" => "rock",
        "boulder" | "boulders" => "boulder",
        "cart" | "carts" | "wagon" | "wagons" | "car" | "cars" | "wheelbarrow" => "cart",
        "wall" | "walls" | "fence" => "wall",
        "pillar" | "pillars" | "column" | "columns" | "statue" => "pillar",
        "barrel" | "barrels" | "keg" | "cask" => "barrel",
        "table" | "tables" | "desk" => "table",
        "crate" | "crates" => "crate",
        "log" | "logs" => "log",
        "jar" | "jars" | "pot" | "urn" => "jar",
        "brazier" | "fire-pit" => "brazier",
        "altar" | "shrine" => "altar",
        "chest" | "chests" | "coffer" => "chest",
        "bones" | "bone" | "skeleton" => "bones",
        "cairn" | "cairns" => "cairn",
        "rubble" | "debris" => "rubble",
        "snowdrift" => "snowdrift",
        _ => return None,
    })
}

/// The material, colour and hex the line's words give a thing (`obsidian` → black), the first named.
pub fn colour_of(text: &str) -> Option<(&'static str, &'static str, &'static str)> {
    const TABLE: &[(&[&str], (&str, &str, &str))] = &[
        (&["obsidian"], ("obsidian", "black", "#141414")),
        (&["black", "onyx", "jet", "shadow", "ebony"], ("iron", "black", "#1c1c1c")),
        (&["golden", "gold", "gilded"], ("gold", "gold", "#d4af37")),
        (&["silver", "silvered", "moonsteel", "mithril"], ("iron", "silver", "#c0c4cc")),
        (&["iron", "steel"], ("iron", "steel", "#8a8f96")),
        (&["bronze", "brass", "copper"], ("gold", "bronze", "#b08d57")),
        (&["crimson", "red", "blood", "bloody", "ruby", "scarlet"], ("iron", "red", "#b3122e")),
        (&["bone", "ivory"], ("bone", "ivory", "#efe6cf")),
        (&["white", "pale"], ("marble", "white", "#f2f2f2")),
        (&["wooden", "wood", "oak", "oaken"], ("wood", "brown", "#8b5a2b")),
        (&["glass", "crystal", "diamond"], ("glass", "clear", "#cfe8ff")),
        (&["ice", "icy", "frost", "frozen"], ("ice", "ice blue", "#bfe6ff")),
        (&["jade", "emerald", "green"], ("stone", "green", "#2e8b57")),
        (&["sapphire", "blue"], ("glass", "blue", "#2459c8")),
        (&["purple", "amethyst", "violet"], ("glass", "purple", "#7b3fb5")),
        (&["flaming", "fire", "burning", "molten"], ("obsidian", "orange", "#ff7a1a")),
    ];
    words(text).iter().find_map(|w| TABLE.iter().find(|(ks, _)| ks.contains(&w.as_str())).map(|(_, c)| *c))
}

/// Objects a line may throw (a thrown prop, `kind: "object"`).
const THROWABLE: &[&str] = &["chair", "stool", "bottle", "barrel", "crate", "table", "bench", "mug", "plate", "pot", "lamp", "vase", "jar", "anvil",
                             "log", "bucket", "skull", "brick", "rock", "stone", "boulder", "cart", "shoe", "boot", "sandwich", "fish", "cat", "piano"];
const THROW_VERBS: &[&str] = &["throw", "throws", "hurl", "hurls", "toss", "tosses", "fling", "flings", "lob", "lobs", "chuck", "chucks", "pitch"];
const OWNERS: &[&str] = &["his", "her", "their", "your", "its", "the foe's", "enemy's"];

/// What a line names to hold: a weapon or a thrown object, with its words' colour.
#[derive(Debug, Clone, PartialEq)]
pub struct PropWords {
    /// The words of the thing ("obsidian blade").
    pub name: String,
    /// `weapon` or `object` (a thing thrown).
    pub kind: &'static str,
    pub weapon_class: &'static str,
    /// From the words (`None`: the words give none; the item rules pick one).
    pub material: Option<&'static str>,
    pub colour: Option<(&'static str, &'static str)>,
    /// A throw verb named it ("throw a chair"): the line is a thrown strike.
    pub thrown: bool,
}

/// The weapon or object a line names to hold or throw (schema 21), if any: a draw phrase's words
/// ([`crate::weapons::draw_words`]), else `my|a|an|with` + up to two words + a weapon noun ("my obsidian blade"), else
/// a throw verb + a throwable object ("throw a chair"). Never a weapon named as the foe's ("his sword").
pub fn prop_words(text: &str) -> Option<PropWords> {
    let colour = |name: &str| colour_of(name);
    if let Some((name, wc)) = crate::weapons::draw_words(text) {
        let c = colour(&name);
        return Some(PropWords { kind: "weapon", weapon_class: wc, material: c.map(|c| c.0), colour: c.map(|c| (c.1, c.2)), thrown: false, name });
    }
    let ws = words(text);
    // A throw verb and a throwable thing ("throw a chair", "hurl the barrel at him").
    for (i, w) in ws.iter().enumerate() {
        if !THROW_VERBS.contains(&w.as_str()) {
            continue;
        }
        let rest: Vec<&String> = ws[i + 1..].iter().take(5).collect();
        let mut name: Vec<&str> = vec![];
        for x in rest {
            if ["a", "an", "the", "my", "this", "that", "some"].contains(&x.as_str()) && name.is_empty() {
                continue;
            }
            if ["at", "to", "into", "and", "then", "him", "her", "them", "it"].contains(&x.as_str()) {
                break;
            }
            name.push(x);
            let joined = name.join(" ");
            if let Some(wc) = crate::weapons::class_of_words(&joined) {
                let c = colour(&joined);
                return Some(PropWords { name: joined, kind: "weapon", weapon_class: wc, material: c.map(|c| c.0), colour: c.map(|c| (c.1, c.2)), thrown: true });
            }
            if THROWABLE.contains(&x.trim_end_matches('s')) || THROWABLE.contains(&x.as_str()) {
                let c = colour(&joined);
                return Some(PropWords { name: joined, kind: "object", weapon_class: "thrown", material: c.map(|c| c.0), colour: c.map(|c| (c.1, c.2)), thrown: true });
            }
            if name.len() >= 3 {
                break;
            }
        }
    }
    // A weapon named as one's own ("my obsidian blade", "slash with a golden axe").
    for (i, w) in ws.iter().enumerate() {
        if !["my", "a", "an", "with"].contains(&w.as_str()) {
            continue;
        }
        if i > 0 && OWNERS.contains(&ws[i - 1].as_str()) {
            continue;
        }
        for k in 1..=3 {
            let Some(end) = ws.get(i + k) else { break };
            if OWNERS.contains(&end.as_str()) || ["and", "then", "at", "to"].contains(&end.as_str()) {
                break;
            }
            // The noun is the last word: it alone must name a weapon (not an adjective: "a sword-sharp wit").
            if crate::weapons::class_of_words(end).is_some() {
                let name = ws[i + 1..=i + k].iter().filter(|x| !["my", "a", "an", "the"].contains(&x.as_str())).cloned().collect::<Vec<_>>().join(" ");
                let wc = crate::weapons::class_of_words(&name)?;
                let c = colour(&name);
                return Some(PropWords { name, kind: "weapon", weapon_class: wc, material: c.map(|c| c.0), colour: c.map(|c| (c.1, c.2)), thrown: false });
            }
        }
    }
    None
}

/// A thrown "weapon" that is really a thing on the board ("throw a rock": the rocks lie on the ground).
fn board_thing(p: &PropWords) -> bool {
    p.thrown && p.name.split(' ').any(|w| cover_word(w).is_some())
}

/// The cover kind a line uses ("hide behind that tree" → `tree`, "kick the barrel" → `barrel`), unless the thing is
/// what it throws.
pub fn cover_kind(text: &str) -> Option<&'static str> {
    let thrown = prop_words(text).filter(|p| p.thrown && p.kind == "weapon").map(|p| p.name);
    words(text).iter().find_map(|w| cover_word(w).filter(|_| thrown.as_ref().is_none_or(|t| !t.split(' ').any(|x| x == w))))
}

/// The ground word of a line (`dust`, `earth` and `soil` are dirt), as one of [`GROUND_KINDS`].
pub fn ground_word(text: &str) -> Option<&'static str> {
    words(text).iter().find_map(|w| {
        Some(match w.as_str() {
            "dirt" | "dust" | "earth" | "soil" | "muck" | "grit" => "dirt",
            "sand" => "sand",
            "mud" => "mud",
            "snow" | "snowball" => "snow",
            "ash" | "ashes" | "soot" => "ash",
            "gravel" | "pebbles" | "pebble" => "gravel",
            "leaves" | "leaf" => "leaves",
            "water" | "puddle" => "water",
            _ => return None,
        })
    })
}

/// A line that throws a thing (not "I throw you into the crate": that is a judo throw).
fn tosses(ws: &[String]) -> bool {
    const PEOPLE: &[&str] = &["you", "him", "her", "them", "me", "yourself", "himself", "herself", "his", "your"];
    ws.iter().enumerate().any(|(i, w)| THROW_VERBS.contains(&w.as_str()) && !ws.get(i + 1).is_some_and(|n| PEOPLE.contains(&n.as_str())))
}

/// A line that throws or picks something up.
fn throws(ws: &[String], t: &str) -> bool {
    tosses(ws) || phrase(t, &["pick up", "picks up", "scoop", "grab a", "grab some", "kick up", "kicks up"])
}

/// Words that make a cover word a thing the line uses (not a figure of speech).
const USE_WORDS: &[&str] = &["behind", "under", "beneath", "cover", "hide", "to the", "toward", "towards", "over", "off the", "on the", "onto", "from the",
                             "against", "into the", "kick", "push", "shove", "smash", "topple", "climb", "vault", "use", "grab", "pick up",
                             "around the"];

/// One staged step of a line (`move.steps[]`, `duel.blow.stage[]`).
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub clip: &'static str,
    /// One of [`STEP_PATHS`], or none (in place).
    pub path: Option<&'static str>,
    /// The step uses the line's object (its cover).
    pub object: bool,
    pub target: Option<&'static str>,
    /// The step is the draw (the prop comes out here) or the throw.
    pub draw: bool,
}

impl Step {
    fn new(clip: &'static str) -> Step {
        Step { clip, path: None, object: false, target: None, draw: false }
    }
    fn path(mut self, p: &'static str) -> Step {
        self.path = Some(p);
        self
    }
    /// The step as JSON: `{clip, path?, object_id? | ground?, target_part?, prop?}` (`thing`: the board object or the
    /// ground the line uses, [`Thing`]).
    pub fn json(&self, thing: Option<&Thing>, prop: Option<&Value>) -> Value {
        let mut v = json!({ "clip": self.clip });
        if let Some(p) = self.path {
            v["path"] = p.into();
        }
        match thing.filter(|_| self.object) {
            Some(Thing::Board(id, _)) => v["object_id"] = id.clone().into(),
            Some(Thing::Ground(g)) => {
                v["ground"] = (*g).into();
                if v["path"] == "cover" {
                    v.as_object_mut().map(|o| o.remove("path"));
                }
            }
            _ => {}
        }
        if let Some(t) = self.target {
            v["target_part"] = t.into();
        }
        if let Some(p) = prop.filter(|p| !p.is_null()) {
            v["prop"] = p.clone();
        }
        v
    }
}

/// A line read as a plan: its steps, the class a movement line or a throw asks for, the part its attack aims at.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plan {
    pub steps: Vec<Step>,
    pub class: Option<MoveClass>,
    pub target: Option<&'static str>,
}

/// The body part the words aim at (schema 21: the throat is the `neck`).
pub fn part_of(text: &str) -> Option<&'static str> {
    words(text).iter().find_map(|w| {
        Some(match w.trim_end_matches("'s") {
            "throat" | "neck" | "jugular" | "windpipe" | "nape" => "neck",
            "head" | "face" | "jaw" | "chin" | "nose" | "eye" | "eyes" | "skull" | "temple" | "mouth" | "teeth" | "ear" | "forehead" => "head",
            "chest" | "heart" | "ribs" | "rib" | "sternum" | "back" | "spine" | "shoulder" => "chest",
            "belly" | "gut" | "guts" | "stomach" | "side" | "liver" | "kidney" | "groin" | "waist" => "belly",
            "arm" | "arms" | "hand" | "hands" | "wrist" | "elbow" | "fingers" => "arms",
            "leg" | "legs" | "knee" | "knees" | "shin" | "ankle" | "foot" | "feet" | "thigh" | "hamstring" => "legs",
            _ => return None,
        })
    })
}

fn has(ws: &[String], keys: &[&str]) -> bool {
    ws.iter().any(|w| keys.contains(&w.as_str()))
}

fn phrase(t: &str, keys: &[&str]) -> bool {
    keys.iter().any(|k| t.contains(k))
}

/// The line cut into its clauses ("then", "and then", "after that", "before", "and", commas, full stops).
fn clauses(text: &str) -> Vec<String> {
    let mut t = format!(" {} ", text.to_lowercase().replace([',', ';', '.', '!', '?'], " , "));
    for sep in [" and then ", " after that ", " afterwards ", " then ", " before ", " and ", " , "] {
        t = t.replace(sep, " | ");
    }
    t.split('|').map(|c| c.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|c| !c.is_empty()).collect()
}

/// The steps of one clause.
fn clause_steps(c: &str, has_prop: bool) -> Vec<Step> {
    let ws = words(c);
    let side = if has(&ws, &["left"]) {
        Some("left")
    } else if has(&ws, &["right"]) {
        Some("right")
    } else if has(&ws, &["back", "backward", "backwards", "away"]) {
        Some("back")
    } else {
        None
    };
    let cover = ws.iter().any(|w| cover_word(w).is_some());
    let attack = crate::moves::read_at(c, fn7_model::dm::DUEL_WORDS_SCHEMA).filter(|r| {
        matches!(r.class, MoveClass::Strike | MoveClass::Heavy | MoveClass::Throw | MoveClass::Grapple | MoveClass::Invoke | MoveClass::Command)
            // A bare jump is a jump, not a jump attack.
            && !matches!(r.word, "jump" | "leap" | "vault" | "dive")
    });
    let slit = has(&ws, &["slit", "slash", "slice", "cut", "slashes", "slits", "gash", "carve"]);
    let mut out = vec![];
    // The draw ("pull out a sword", "draw my pistol"): the prop comes out here.
    if has_prop && crate::weapons::draw_words(c).is_some() {
        out.push(Step { draw: true, ..Step::new("draw_from_nowhere") });
    }
    // A throw or a pick-up: the ground ("throw dirt in his eyes"), a board object ("throw a rock"), a held thing.
    if throws(&ws, c) {
        let tossed = tosses(&ws);
        let mut throw = Step { draw: true, ..Step::new("throw_object") };
        throw.target = part_of(c);
        let held_weapon = prop_words(c).is_some_and(|p| p.kind == "weapon" && !board_thing(&p));
        if ground_word(c).is_some() {
            out.push(Step { object: true, ..Step::new("kneel") });
            throw.object = true;
        } else if !held_weapon && (ws.iter().any(|w| cover_word(w).is_some()) || ws.iter().any(|w| THROWABLE.contains(&w.as_str()))) {
            out.push(Step { object: true, ..Step::new("run").path("cover") });
            out.push(Step { object: true, ..Step::new("kneel") });
            throw.object = true;
        }
        if tossed {
            out.push(throw);
        }
        if !out.is_empty() {
            return out;
        }
    }
    // To the cover ("hide behind that tree", "duck behind the cart", "get behind the wall").
    if cover && phrase(c, &["behind", "under", "beneath", "cover", "hide", "into the", "over to", "to the", "toward", "towards"]) {
        out.push(Step { object: true, ..Step::new("run").path("cover") });
        if has(&ws, &["hide", "hides", "hiding", "crouch", "crouches", "duck", "ducks", "cover", "wait", "lurk"]) {
            out.push(Step::new("kneel"));
        }
        return out;
    }
    // Up again ("pop up", "spring up", "jump out", "rise").
    if phrase(c, &["pop up", "pops up", "pop out", "spring up", "springs up", "jump out", "jumps out", "leap out", "leaps out", "stand up", "burst out", "come out"])
        || has(&ws, &["rise", "rises", "emerge", "emerges", "resurface"])
    {
        out.push(Step::new(if has(&ws, &["spin", "spinning", "twirl"]) { "spin_rise" } else { "rise" }));
        if attack.is_none() && !slit {
            return out;
        }
    }
    // A roll ("duck roll to the left", "roll right", "dive left", "tumble back").
    if has(&ws, &["roll", "rolls", "rolling", "tumble", "tumbles", "somersault", "cartwheel", "dive", "dives"]) {
        let s = side.unwrap_or("left");
        out.push(Step::new(if s == "back" { "dodge_roll_back" } else { "evade_roll" }).path(s));
        return out;
    }
    // Running in circles.
    if phrase(c, &["in circles", "in a circle", "around in circles", "run around", "runs around", "circle around", "running around"]) || has(&ws, &["circle", "circles"]) {
        out.push(Step::new("run").path("circle"));
        return out;
    }
    // A side step, a step back.
    if side.is_some() && has(&ws, &["step", "steps", "sidestep", "move", "moves", "strafe", "dodge", "dodges", "slide", "shift", "go", "jump", "hop", "lean"]) {
        let s = side.unwrap_or("left");
        let clip = if has(&ws, &["jump", "hop"]) { "jump_attack" } else if s == "back" { "walk" } else { "evade_stand" };
        out.push(Step::new(clip).path(if clip == "jump_attack" { "leap" } else { s }));
        if clip == "jump_attack" {
            out.last_mut().expect("pushed").path = Some(s);
        }
        return out;
    }
    if attack.is_none() && !slit {
        if has(&ws, &["jump", "jumps", "leap", "leaps", "hop", "hops", "bounce", "bounces"]) {
            out.push(Step::new("jump_attack").path("leap"));
        } else if has(&ws, &["duck", "ducks", "crouch", "crouches", "kneel", "kneels", "hide", "hides"]) {
            out.push(Step::new(if has(&ws, &["duck", "ducks"]) { "evade_stand" } else { "kneel" }));
        } else if has(&ws, &["block", "blocks", "guard", "brace"]) {
            out.push(Step::new("block"));
        } else if has(&ws, &["parry", "parries", "deflect"]) {
            out.push(Step::new("parry_high"));
        } else if has(&ws, &["run", "runs", "charge", "charges", "rush", "rushes", "sprint", "dash"]) {
            out.push(Step::new("run").path("in"));
        } else if has(&ws, &["dodge", "dodges", "sidestep", "weave", "evade"]) {
            out.push(Step::new("evade_spin"));
        }
        return out;
    }
    // The blow itself: a slash at the throat is `strike_slash` at the neck; else the mapper's first clip.
    let clip = if slit { "strike_slash" } else { attack.as_ref().map(|r| r.clips[0]).unwrap_or("strike_side") };
    let mut s = Step::new(clip);
    s.target = part_of(c);
    if cover && phrase(c, &["kick", "push", "shove", "smash", "topple", "knock", "punch", "hit"]) {
        // "kick the barrel": run to it and strike it.
        out.push(Step { object: true, ..Step::new("run").path("cover") });
        s.object = true;
    }
    out.push(s);
    out
}

/// The line as a plan (schema 21): its staged steps (at most [`MAX_STEPS`]), the class a movement-only line or a
/// throw asks for, and the part its attack aims at. `has_prop`: [`prop_words`] found a thing in it.
pub fn plan(text: &str) -> Plan {
    let has_prop = prop_words(text).is_some();
    let mut steps: Vec<Step> = vec![];
    for c in clauses(text) {
        steps.extend(clause_steps(&c, has_prop));
    }
    steps.truncate(MAX_STEPS);
    const ATTACKS: &[&str] = &["strike", "heavy", "throw", "grapple", "invoke", "command"];
    let attacks = |s: &Step| s.clip == "throw_object" || ATTACKS.iter().any(|c| crate::picker::serves(s.clip, c)) && !matches!(s.clip, "run" | "lunge") && s.path.is_none();
    let target = steps.iter().rev().find(|s| attacks(s)).and_then(|s| s.target).or_else(|| part_of(text));
    let thrown = steps.iter().any(|s| s.clip == "throw_object");
    let class = if thrown {
        Some(MoveClass::Strike)
    } else if !steps.is_empty() && !steps.iter().any(|s| attacks(s) || s.draw) && steps.iter().any(|s| s.path.is_some() || s.clip == "kneel" || s.clip == "evade_stand") {
        // Pure movement (a roll, a jump, a run in circles, hiding): a d20 for positioning.
        Some(MoveClass::Evade)
    } else {
        None
    };
    Plan { steps, class, target }
}

/// A held item a fighter can use (schema 21: `Fighter::held`, `usable.items[]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeldItem {
    pub item: ItemId,
    pub name: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weapon_class: Option<String>,
}

/// What demigod `d` holds (every held item, in id order).
pub fn held_items(w: &World, d: DemigodId) -> Vec<HeldItem> {
    let me = Ref::Demigod(d);
    w.items
        .iter()
        .filter(|i| i.holders.last() == Some(&me) && i.ext.state == ItemState::Held && i.ext.muster.is_none())
        .map(|i| HeldItem {
            item: i.id,
            name: i.name.clone(),
            kind: serde_json::to_value(i.ext.kind).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(),
            weapon_class: crate::weapons::weapon_class(i.ext.kind, &i.name).map(str::to_string),
        })
        .collect()
}

/// The ground of a duel's hex a fighter can pick up: by the scene's biome, and what the Earth simulator left there.
pub fn ground_of(scene: &Value) -> Vec<&'static str> {
    let mut g: Vec<&'static str> = match scene["biome"].as_str().unwrap_or("wild") {
        "harbour" => vec!["sand", "water", "gravel"],
        "forest" => vec!["dirt", "mud", "leaves"],
        "desert" => vec!["sand", "gravel"],
        "hills" => vec!["dirt", "gravel"],
        "town" => vec!["dirt", "gravel"],
        _ => vec!["dirt", "mud"],
    };
    let more: &[&'static str] = match scene["ground"]["floor"].as_str().unwrap_or("") {
        "snow" | "frozen" => &["snow"],
        "ash" | "burning" | "lava" => &["ash"],
        "flooded" => &["mud", "water"],
        "ruins" | "cracked" | "rift" | "shaken" => &["gravel"],
        _ => &[],
    };
    for m in more {
        if !g.contains(m) {
            g.push(m);
        }
    }
    g
}

/// The board's objects a line may name (not the flat hazard patches).
fn board(scene: &Value) -> Vec<&Value> {
    scene["objects"].as_array().into_iter().flatten().filter(|o| cover_word(o["kind"].as_str().unwrap_or("")).is_some()).collect()
}

/// `usable` (schema 21, on the lock and the scene per fighter): `{items: [{ref, name, kind, weapon_class}], board:
/// [{object_id, kind, name}], ground: [kinds]}`: what the tray can show as chips.
pub fn usable_json(items: &[HeldItem], scene: Option<&Value>) -> Value {
    let it: Vec<Value> = items.iter().map(|i| json!({ "ref": Ref::Item(i.item), "name": i.name, "kind": i.kind, "weapon_class": i.weapon_class })).collect();
    let (b, g): (Vec<Value>, Vec<&str>) = match scene {
        Some(sc) => (board(sc).iter().map(|o| json!({ "object_id": o["id"], "kind": o["kind"], "name": o["note"] })).collect(), ground_of(sc)),
        None => (vec![], vec![]),
    };
    json!({ "items": it, "board": b, "ground": g })
}

/// What a line uses.
#[derive(Debug, Clone, PartialEq)]
pub enum Thing {
    /// A held item (its index in the fighter's `held`).
    Item(usize),
    /// A board object: its id and kind.
    Board(String, String),
    /// The hex's ground.
    Ground(&'static str),
}

/// What a line names and whether it is there.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Uses {
    pub thing: Option<Thing>,
    /// Named but not there: the words, and where it was looked for (`held`, `board`, `ground`).
    pub missing: Option<(String, &'static str)>,
    /// The line throws it.
    pub throw: bool,
}

fn kind_matches(asked: &str, kind: &str) -> bool {
    asked == kind || (matches!(asked, "rock" | "boulder" | "rubble") && matches!(kind, "rock" | "boulder" | "rubble"))
}

fn hex_of(o: &Value) -> [i32; 2] {
    [o["at"][0].as_i64().unwrap_or(0) as i32, o["at"][1].as_i64().unwrap_or(0) as i32]
}

/// What `text` uses of what its fighter can use (schema 21): `held` (its items), the `scene` (the board and the
/// ground), `me` (its hex: the nearest board object of a kind is the one). See the module docs.
pub fn uses(text: &str, held: &[HeldItem], scene: Option<&Value>, me: [i32; 2]) -> Uses {
    let t = text.to_lowercase();
    let ws = words(&t);
    let throw = throws(&ws, &t);
    let tossed = tosses(&ws);
    let empty = json!({});
    let sc = scene.unwrap_or(&empty);
    let dist = |a: [i32; 2], b: [i32; 2]| ((a[0] - b[0]).abs() + (a[1] - b[1]).abs() + (a[0] - b[0] + a[1] - b[1]).abs()) / 2;
    let nearest = |kind: &str| {
        board(sc).into_iter().filter(|o| kind_matches(kind, o["kind"].as_str().unwrap_or(""))).min_by_key(|o| (dist(hex_of(o), me), o["id"].as_str().unwrap_or("").to_string()))
            .map(|o| Thing::Board(o["id"].as_str().unwrap_or("").to_string(), o["kind"].as_str().unwrap_or("").to_string()))
    };
    let named_held = |name: &str, class: Option<&str>| {
        let nw = words(name);
        held.iter().position(|h| words(&h.name).iter().any(|x| x.len() > 3 && nw.contains(x)))
            .or_else(|| class.and_then(|c| held.iter().position(|h| h.weapon_class.as_deref() == Some(c))))
    };
    // The ground ("throw dirt in his eyes", "scoop up some sand").
    if throw {
        if let Some(g) = ground_word(&t) {
            let there = ground_of(sc).contains(&g);
            return Uses { thing: there.then_some(Thing::Ground(g)), missing: (!there).then(|| (g.to_string(), "ground")), throw: tossed };
        }
    }
    // A weapon named as one's own, drawn or thrown ("draw my sword", "pull out a pistol", "throw my dagger").
    if let Some(p) = prop_words(&t).filter(|p| p.kind == "weapon" && !board_thing(p)) {
        return match named_held(&p.name, Some(p.weapon_class)) {
            Some(i) => Uses { thing: Some(Thing::Item(i)), missing: None, throw: tossed },
            None => Uses { thing: None, missing: Some((p.name, "held")), throw: tossed },
        };
    }
    // A held thing thrown by its name ("throw my lantern").
    if tossed {
        if let Some(i) = held.iter().position(|h| words(&h.name).iter().any(|x| x.len() > 3 && ws.contains(x))) {
            return Uses { thing: Some(Thing::Item(i)), missing: None, throw: true };
        }
    }
    // A board object: thrown or picked up ("throw a rock"), or used ("hide behind that tree", "kick the crate").
    let noun = ws.iter().find_map(|w| cover_word(w)).or_else(|| if throw { ws.iter().find(|w| THROWABLE.contains(&w.as_str())).map(|_| "") } else { None });
    if let Some(kind) = noun {
        let used = throw || USE_WORDS.iter().any(|k| t.contains(k));
        if used {
            let asked = if kind.is_empty() { ws.iter().find(|w| THROWABLE.contains(&w.as_str())).cloned().unwrap_or_default() } else { kind.to_string() };
            return match nearest(&asked) {
                Some(th) => Uses { thing: Some(th), missing: None, throw: tossed },
                None => Uses { thing: None, missing: Some((asked, "board")), throw: tossed },
            };
        }
    }
    Uses::default()
}

/// The caption of a whiff: `you have no pistol`, `no tree here`, `no sand here`.
pub fn whiff_caption(by: &str, named: &str, where_: &str) -> String {
    match where_ {
        "held" => format!("{by} reaches for a {named}: you have no {named}. The hand closes on air."),
        _ => format!("{by} looks for the {named}: no {named} here. The line is spent."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clips(p: &Plan) -> Vec<(&str, Option<&str>, bool, Option<&str>)> {
        p.steps.iter().map(|s| (s.clip, s.path, s.object, s.target)).collect()
    }

    #[test]
    fn props_from_the_words() {
        let p = prop_words("I pull out a sword").unwrap();
        assert_eq!((p.name.as_str(), p.kind, p.weapon_class, p.thrown), ("sword", "weapon", "one_handed", false));
        let p = prop_words("I slash him with my obsidian blade").unwrap();
        assert_eq!((p.name.as_str(), p.material, p.colour), ("obsidian blade", Some("obsidian"), Some(("black", "#141414"))));
        let p = prop_words("draw a golden pistol").unwrap();
        assert_eq!((p.weapon_class, p.colour.map(|c| c.0)), ("rifle", Some("gold")));
        let p = prop_words("I throw a chair at him").unwrap();
        assert_eq!((p.name.as_str(), p.kind, p.weapon_class, p.thrown), ("chair", "object", "thrown", true));
        assert_eq!(prop_words("I dodge his sword"), None, "the foe's sword is not mine to draw");
        assert_eq!(prop_words("I kick your knee"), None);
    }

    #[test]
    fn cover_and_steps() {
        assert_eq!(cover_kind("hide behind that tree then pop up and slit his throat"), Some("tree"));
        assert_eq!(cover_kind("duck behind the cart"), Some("cart"));
        let p = plan("hide behind that tree then pop up and slit his throat");
        assert_eq!(clips(&p), vec![("run", Some("cover"), true, None), ("kneel", None, false, None), ("rise", None, false, None),
                                   ("strike_slash", None, false, Some("neck"))]);
        assert_eq!((p.class, p.target), (None, Some("neck")));
        let p = plan("duck roll to the left");
        assert_eq!(clips(&p), vec![("evade_roll", Some("left"), false, None)]);
        assert_eq!(p.class, Some(MoveClass::Evade), "a roll is positioning: a d20 for it");
        assert_eq!(clips(&plan("roll right")), vec![("evade_roll", Some("right"), false, None)]);
        assert_eq!(clips(&plan("jump")), vec![("jump_attack", Some("leap"), false, None)]);
        assert_eq!(plan("jump").class, Some(MoveClass::Evade));
        assert_eq!(clips(&plan("run in circles")), vec![("run", Some("circle"), false, None)]);
        assert_eq!(clips(&plan("duck")), vec![("evade_stand", None, false, None)]);
        assert_eq!(clips(&plan("kick the barrel")), vec![("run", Some("cover"), true, None), ("kick", None, true, None)]);
        assert_eq!(plan("I pull out a sword and slash his neck").steps.iter().map(|s| s.clip).collect::<Vec<_>>(), vec!["draw_from_nowhere", "strike_slash"]);
        assert_eq!(plan("throw a chair").steps.last().unwrap().clip, "throw_object");
        assert_eq!(plan("throw a chair").class, Some(MoveClass::Strike));
        // Five clauses: four steps.
        assert_eq!(plan("roll left then roll right then jump then duck then roll left").steps.len(), MAX_STEPS);
        assert!(plan("I kick your knee").steps.len() == 1 && plan("I kick your knee").class.is_none());
    }

    fn item(n: u32, name: &str, wc: Option<&str>) -> HeldItem {
        HeldItem { item: ItemId(n), name: name.into(), kind: if wc.is_some() { "weapon" } else { "relic" }.into(), weapon_class: wc.map(str::to_string) }
    }

    #[test]
    fn only_what_is_held_or_on_the_board() {
        let scene = json!({ "biome": "forest", "objects": [
            { "id": "tree_a", "kind": "tree", "at": [2, -2], "note": "a tree" }, { "id": "tree_b", "kind": "tree", "at": [-2, -1], "note": "a tree" },
            { "id": "boulder_a", "kind": "boulder", "at": [0, 2], "note": "a boulder" }, { "id": "lava_a", "kind": "lava", "at": [1, 1], "note": "lava" } ] });
        let held = vec![item(4, "Obsidian Blade", Some("one_handed")), item(7, "Moon Lantern", None)];
        let u = |t: &str| uses(t, &held, Some(&scene), [-3, 0]);
        assert_eq!(u("I pull out my blade").thing, Some(Thing::Item(0)));
        assert_eq!(u("I pull out a sword").thing, Some(Thing::Item(0)), "a sword: the one-handed blade it holds");
        assert_eq!(u("I draw a pistol").missing, Some(("pistol".to_string(), "held")), "no pistol held: a whiff");
        assert_eq!(u("hide behind that tree").thing, Some(Thing::Board("tree_b".into(), "tree".into())), "the nearest tree");
        assert_eq!(u("duck behind the cart").missing, Some(("cart".to_string(), "board")));
        assert_eq!(u("throw a rock at his face").thing, Some(Thing::Board("boulder_a".into(), "boulder".into())), "rocks: the boulder on the board");
        let d = u("throw dirt in his eyes");
        assert_eq!((d.thing, d.throw), (Some(Thing::Ground("dirt")), true));
        assert_eq!(u("throw sand in his eyes").missing, Some(("sand".to_string(), "ground")), "a forest has no sand");
        assert_eq!(u("throw my lantern").thing, Some(Thing::Item(1)));
        assert_eq!(u("I kick your knee"), Uses::default());
        let us = usable_json(&held, Some(&scene));
        assert_eq!(us["board"].as_array().unwrap().len(), 3, "the lava patch is not a thing to use");
        assert_eq!(us["ground"], json!(["dirt", "mud", "leaves"]));
        assert_eq!(us["items"][0]["name"], "Obsidian Blade");
        assert_eq!(plan("throw dirt in his eyes").steps.iter().map(|s| (s.clip, s.target)).collect::<Vec<_>>(),
                   vec![("kneel", None), ("throw_object", Some("head"))]);
    }
}

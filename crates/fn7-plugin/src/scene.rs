//! Token play (state schema 19, docs/CLIENT_API.md "Token play", docs/DM_RULEBOOK.md chapter 30).
//!
//! Benito described typing an in-game action, such as going into the house to the left and approaching its occupant, and wanting to see it play out
//! outwiththelittlegametokens". When a demigod's typed will resolves into acts at a place, the people-level board
//! (3 m hexes) plays it out with the game tokens: a `scene.play` of actors and timed steps.
//!
//! **Rules first, and the scene decides nothing.** A scene is built after the merge from what already happened: the
//! demigod's intents of this beat and the ledger rows they produced, the act's recorded d20 (`act:<intent>`) or its
//! god's ruling. The intent's verb picks the act (talk, fight, search, ...); the hex grid gives the path (a few cells
//! on the people level); the place × time memory gives the building (`dm::cell_state`, or a `place.building` fact the
//! scene itself recorded); the rows' targets give the other actors (named persons the ledger names, or one person
//! the society brings forward the way it does for any mention, remembered ever after). The outcome comes from the
//! dice and the rows: the scene never changes it.
//!
//! **What stays hidden.** Captions are built from names and verb templates, the outcome text is the row's public
//! line; the order's words never appear (only "left" / "right" choose the side of the street). Acts of subagents and
//! possessions, and god rulings that are not public, make no scene.
//!
//! **Captions.** The rule captions always stand; with `token_play_llm` a small model may rewrite them later
//! ([`crate::polish`]), recorded as a side input so a replay never calls a model.

use fn7_model::dm::{DiceRoll, DiceVerdict, Scope, SCENES_KEPT};
use fn7_model::hexgrid::{cell_at, HexCell, HexLevel};
use fn7_model::*;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::settings::Settings;

/// A scene never runs longer than this (ms): it plays inside one beat's display (about 8 s per typed action).
pub const SCENE_MAX_MS: u32 = 9000;
/// A short scene is slowed toward this (about 8 s per typed action), never by more than half again.
pub const SCENE_TARGET_MS: u32 = 8000;
/// At most this many steps.
pub const MAX_STEPS: usize = 8;
/// A caption is at most this many characters.
pub const CAPTION_MAX: usize = 70;
/// The outcome text is at most this many characters.
pub const OUTCOME_MAX: usize = 140;
/// The farthest (in people-level cells) a real position is kept on the stage; anyone farther stands 2 cells away.
pub const STAGE_RADIUS: i64 = 4;

/// The verbs a token can play.
pub const VERBS: &[&str] = &[
    "walk", "enter", "exit", "talk", "give", "take", "fight", "cast", "flee", "kneel", "embrace", "search", "hide",
    "celebrate", "fall", "slap",
];
/// The moods of a step (the client's bubbles: hearts, anger marks, sweat drops, sparkles).
pub const MOODS: &[&str] = &["love", "anger", "fear", "joy", "neutral"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub scene_id: String,
    pub beat: u32,
    pub demigod: Ref,
    /// The seat whose will it is (absent for a built-in AI demigod).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seat: Option<u8>,
    pub place: ScenePlace,
    pub actors: Vec<Actor>,
    pub steps: Vec<Step>,
    /// The act's d20 as recorded (`dice.rolled`), when it was rolled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dice: Option<SceneDice>,
    pub outcome: SceneOutcome,
    pub total_ms: u32,
    /// The captions were rewritten by the polish model (`scene.captions`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub polished: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScenePlace {
    #[serde(rename = "ref")]
    pub place: Ref,
    pub name: String,
    /// The stage's anchor: the demigod's people-level cell when the beat began.
    pub cell: String,
    pub level: String,
    /// The building the scene goes into, if any (the client lifts its roof on `enter`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub building: Option<Building>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Building {
    pub cell: String,
    /// `left`, `right`, `ahead left`, `ahead right`, `behind left`, `behind right` (from the anchor).
    pub side: String,
    /// `observed`: the place × time memory has buildings on that hex; `houses`: the 30 m hex around it has houses
    /// and the scene recorded this one (a `place.building` fact, kept ever after).
    pub source: String,
    /// The `place.building` / `place.observed` memory entry that makes it a fact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Actor {
    /// `a0` (always the demigod), `a1`, ...
    pub id: String,
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub who: Option<Ref>,
    pub name: String,
    /// `demigod` | `person` | `crowd` | `creature` | `army`.
    pub kind: String,
    pub start_cell: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub t_ms: u32,
    pub dur_ms: u32,
    pub actor: String,
    pub verb: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_cell: Option<String>,
    /// A walk or a flee: every cell on the way, the last one is `to_cell`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_actor: Option<String>,
    pub caption: String,
    pub mood: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneDice {
    pub roll: u8,
    pub modifier: i32,
    pub total: i32,
    pub target_dc: i32,
    /// `crit_fail` | `fail` | `success` | `crit_success`.
    pub verdict: String,
    /// When the die tumbles (scene time, ms): the start of the act.
    pub at_ms: u32,
    /// The `dice.rolled` memory entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneOutcome {
    /// What happened, in one line: the ledger's line when its row is a public event, else the outcome step's caption
    /// (built from names: a row's own words may carry what only its witnesses know).
    pub text: String,
    /// `success` | `partial` | `failed` | `twisted`.
    pub result: String,
    /// The verb of the step that shows it (a heart burst on `embrace`, `slap`, `flee`, `fall`, `celebrate`, ...).
    pub verb: String,
}

impl Scene {
    /// The `scene.play` event.
    pub fn json(&self) -> Value {
        let mut v = serde_json::to_value(self).unwrap_or(Value::Null);
        v["event"] = "scene.play".into();
        v
    }
}

/// What the act is, for the tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Seduce,
    Talk,
    Threaten,
    Give,
    Take,
    Fight,
    Cast,
    Search,
    Hide,
    Flee,
    Kneel,
    Celebrate,
    Walk,
}

fn stem(verb: &str) -> String {
    let v = verb.trim().to_lowercase();
    let v = v.split_whitespace().next().unwrap_or("").to_string();
    for suf in ["ing", "es", "s"] {
        if let Some(x) = v.strip_suffix(suf) {
            if x.len() >= 3 {
                return x.to_string();
            }
        }
    }
    v
}

/// The act of an intent's verb (a god route is a cast). Deterministic.
pub fn act_of(verb: &str, god_route: bool) -> Act {
    if god_route {
        return Act::Cast;
    }
    let s = stem(verb);
    let raw = verb.trim().to_lowercase();
    let is = |ws: &[&str]| ws.iter().any(|w| s == *w || raw == *w || raw.starts_with(&format!("{w} ")));
    use fn7_agents::common::{classify, ActClass};
    if is(&["seduce", "seduc", "flirt", "court", "charm", "woo", "kiss", "romance", "romanc", "love", "marry", "marri"]) {
        Act::Seduce
    } else if is(&["pray", "kneel", "worship", "bow", "beg", "plead", "confess"]) {
        Act::Kneel
    } else if is(&["celebrate", "celebrat", "dance", "danc", "party", "parti", "feast", "sing", "drink", "toast", "cheer"]) {
        Act::Celebrate
    } else if is(&["give", "giv", "gift", "heal", "bless", "feed", "donate", "donat", "offer", "share", "shar", "pay", "tip"]) {
        Act::Give
    } else if is(&["steal", "take", "tak", "grab", "loot", "rob", "pickpocket", "buy", "snatch", "snatche", "pilfer"]) {
        Act::Take
    } else if is(&["cast", "curse", "hex", "enchant", "summon", "conjure", "smite", "zap", "storm"]) {
        Act::Cast
    } else if is(&["search", "searche", "scout", "look", "explore", "explor", "investigate", "investigat", "find", "seek", "dig", "examine", "examin", "spy", "inspect", "rummage"]) {
        Act::Search
    } else if is(&["hide", "hid", "sneak", "lurk", "skulk", "stalk"]) {
        Act::Hide
    } else if is(&["flee", "run", "escape", "escap", "retreat", "bolt"]) {
        Act::Flee
    } else if is(&["threaten", "threat", "intimidate", "intimidat", "menace", "menac", "scare", "terrify", "bully"]) {
        Act::Threaten
    } else if is(&["go", "walk", "travel", "head", "enter", "visit", "move", "mov", "march"]) {
        Act::Walk
    } else {
        match classify(verb) {
            ActClass::Attack => Act::Fight,
            ActClass::Social => Act::Talk,
            ActClass::Scare => Act::Threaten,
            ActClass::Move => Act::Walk,
            ActClass::Make => Act::Search,
            ActClass::Other => Act::Talk,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The hex grid (people level)
// ---------------------------------------------------------------------------------------------------------------------

/// The six axial directions, in [`HexCell::neighbors`] order, with the side they are from the anchor (west is left).
const DIRS: [((i64, i64), &str); 6] = [
    ((1, 0), "right"),
    ((1, -1), "ahead right"),
    ((0, -1), "ahead left"),
    ((-1, 0), "left"),
    ((-1, 1), "behind left"),
    ((0, 1), "behind right"),
];

pub fn hex_distance(a: &HexCell, b: &HexCell) -> i64 {
    let (dq, dr) = (a.q - b.q, a.r - b.r);
    (dq.abs() + dr.abs() + (dq + dr).abs()) / 2
}

/// The cells from `a` (excluded) to `b` (included) on a straight hex line.
pub fn hex_line(a: &HexCell, b: &HexCell) -> Vec<HexCell> {
    let n = hex_distance(a, b);
    let mut out = vec![];
    for i in 1..=n {
        let t = i as f64 / n as f64;
        // Cube lerp with a tiny nudge so ties break the same way everywhere.
        let (aq, ar) = (a.q as f64 + 1e-6, a.r as f64 + 2e-6);
        let (bq, br) = (b.q as f64 + 1e-6, b.r as f64 + 2e-6);
        let (q, r) = (aq + (bq - aq) * t, ar + (br - ar) * t);
        let s = -q - r;
        let (mut rq, mut rr, rs) = (q.round(), r.round(), s.round());
        let (dq, dr, ds) = ((rq - q).abs(), (rr - r).abs(), (rs - s).abs());
        if dq > dr && dq > ds {
            rq = -rr - rs;
        } else if dr > ds {
            rr = -rq - rs;
        }
        out.push(HexCell { q: rq as i64, r: rr as i64, ..*a });
    }
    out
}

fn step_toward(a: &HexCell, dir: usize, k: i64) -> HexCell {
    let (dq, dr) = DIRS[dir % 6].0;
    HexCell { q: a.q + dq * k, r: a.r + dr * k, ..*a }
}

fn side_of(a: &HexCell, b: &HexCell) -> String {
    let (dq, dr) = (b.q - a.q, b.r - a.r);
    let best = (0..6)
        .min_by_key(|k| {
            let (x, y) = DIRS[*k].0;
            let (ex, ey) = (dq - x * hex_distance(a, b).max(1), dr - y * hex_distance(a, b).max(1));
            (ex.abs() + ey.abs() + (ex + ey).abs(), *k)
        })
        .unwrap_or(0);
    DIRS[best].1.to_string()
}

/// The direction order the words ask for (left / right / behind / ahead), else the grid's own order.
fn preferred_dirs(words: &str) -> Vec<usize> {
    let w = words.to_lowercase();
    let has = |x: &str| w.split(|c: char| !c.is_alphanumeric()).any(|t| t == x);
    let first: &[usize] = if has("left") {
        &[3, 2, 4]
    } else if has("right") {
        &[0, 1, 5]
    } else if has("behind") || has("back") {
        &[4, 5, 3]
    } else if has("ahead") || has("front") || has("across") || has("opposite") {
        &[2, 1, 0]
    } else {
        &[]
    };
    let mut v: Vec<usize> = first.to_vec();
    for k in [3, 0, 2, 1, 4, 5] {
        if !v.contains(&k) {
            v.push(k);
        }
    }
    v
}

const HOUSE_WORDS: &[&str] = &[
    "house", "home", "inside", "indoors", "building", "shop", "store", "tavern", "inn", "bar", "pub", "room", "hut",
    "cottage", "hall", "temple", "church", "door", "cabin", "barn", "apartment", "flat", "mansion", "palace", "tower",
];

fn says_house(words: &str) -> bool {
    let w = words.to_lowercase();
    w.split(|c: char| !c.is_alphanumeric()).any(|t| HOUSE_WORDS.contains(&t))
}

/// Is `key` a building in the place × time memory at `beat`: a `place.building` fact, or buildings on the hex now.
fn building_fact(w: &World, key: &str, beat: u32) -> Option<(String, Option<u32>)> {
    if let Some(e) = w.dm.memory.iter().rev().find(|e| e.kind == "place.building" && e.place == key && e.beat <= beat) {
        return Some(("houses".into(), Some(e.id)));
    }
    let (state, _, _) = crate::dm::cell_state(w, key, beat).ok()?;
    (state["buildings"].as_u64().unwrap_or(0) > 0).then(|| ("observed".into(), None))
}

/// The building the scene goes into: the first cell (ring 1, then ring 2) in the preferred directions with buildings
/// on it; else, when the 30 m hex around the anchor has houses, the first preferred neighbour (recorded as a fact by
/// [`record_building`]). `None`: no house anywhere near (the scene stays outdoors).
fn find_building(w: &World, anchor: &HexCell, beat: u32, words: &str) -> Option<(HexCell, String, Option<u32>)> {
    let dirs = preferred_dirs(words);
    for k in 1..=2 {
        for d in &dirs {
            let c = step_toward(anchor, *d, k);
            if let Some((src, mem)) = building_fact(w, &c.key(), beat) {
                return Some((c, src, mem));
            }
        }
    }
    let houses = cell_at(anchor.center(), HexLevel::Houses).key();
    let (state, _, _) = crate::dm::cell_state(w, &houses, beat).ok()?;
    (state["buildings"].as_u64().unwrap_or(0) > 0).then(|| (step_toward(anchor, dirs[0], 1), "houses".into(), None))
}

/// Make the scene's building a fact of the place × time memory (once per hex: the same house ever after).
fn record_building(w: &mut World, beat: u32, cell: &HexCell, by: Ref) -> Option<u32> {
    let key = cell.key();
    if let Some(e) = w.dm.memory.iter().find(|e| e.kind == "place.building" && e.place == key) {
        return Some(e.id);
    }
    let wh = crate::dm::locate(w, cell.center(), HexLevel::People);
    let text = format!("a house stands on {key}");
    Some(crate::dm::remember(w, beat, &wh, crate::dm::Decision {
        scope: Scope::Cell, kind: "place.building", query: Some(format!("building:{key}")), public: true, text,
        data: json!({ "cell": key, "buildings": 1, "by": by, "why": "token play" }),
    }))
}

// ---------------------------------------------------------------------------------------------------------------------
// Building a scene
// ---------------------------------------------------------------------------------------------------------------------

/// Cut `s` to `n` characters (an ellipsis when cut).
pub fn clip(s: &str, n: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= n {
        return s;
    }
    let mut out: String = s.chars().take(n.saturating_sub(1)).collect();
    out = out.trim_end().to_string();
    out.push('…');
    out
}

struct Cast {
    actors: Vec<Actor>,
}

impl Cast {
    fn add(&mut self, who: Option<Ref>, name: String, kind: &str, start: &HexCell) -> String {
        let id = format!("a{}", self.actors.len());
        self.actors.push(Actor { id: id.clone(), who, name, kind: kind.into(), start_cell: start.key() });
        id
    }
}

/// The scene's steps as they are written (durations are fitted after).
struct Draft {
    steps: Vec<Step>,
}

impl Draft {
    #[allow(clippy::too_many_arguments)]
    fn push(&mut self, actor: &str, verb: &str, dur: u32, to: Option<&HexCell>, path: Vec<HexCell>, target: Option<&str>, caption: String, mood: &str) {
        if self.steps.len() >= MAX_STEPS {
            return;
        }
        self.steps.push(Step {
            t_ms: 0,
            dur_ms: dur,
            actor: actor.into(),
            verb: verb.into(),
            to_cell: to.map(|c| c.key()),
            path: path.iter().map(|c| c.key()).collect(),
            target_actor: target.map(str::to_string),
            caption: clip(&caption, CAPTION_MAX),
            mood: mood.into(),
        });
    }
}

/// Lay the steps end to end and fit them inside [`SCENE_MAX_MS`] (a short one slowed toward [`SCENE_TARGET_MS`]).
/// Returns the total.
pub fn fit(steps: &mut [Step]) -> u32 {
    let sum: u32 = steps.iter().map(|s| s.dur_ms).sum();
    if sum > 0 && sum < SCENE_TARGET_MS {
        let to = (sum as u64 * 3 / 2).min(SCENE_TARGET_MS as u64);
        for s in steps.iter_mut() {
            s.dur_ms = (s.dur_ms as u64 * to / sum as u64) as u32 / 10 * 10;
        }
    }
    let sum: u32 = steps.iter().map(|s| s.dur_ms).sum();
    if sum > SCENE_MAX_MS {
        for s in steps.iter_mut() {
            s.dur_ms = ((s.dur_ms as u64 * SCENE_MAX_MS as u64) / sum as u64).max(100) as u32;
        }
    }
    let mut t = 0u32;
    for s in steps.iter_mut() {
        s.t_ms = t;
        t += s.dur_ms;
    }
    if t > SCENE_MAX_MS {
        // Rounding up the 100 ms floors: trim the longest.
        let over = t - SCENE_MAX_MS;
        if let Some(s) = steps.iter_mut().max_by_key(|s| s.dur_ms) {
            s.dur_ms -= over.min(s.dur_ms - 100);
        }
        t = 0;
        for s in steps.iter_mut() {
            s.t_ms = t;
            t += s.dur_ms;
        }
    }
    t
}

/// The other actor of an act: who the rows (then the intent) aim at, never the demigod itself.
fn target_of(w: &World, d: DemigodId, rows: &[&LedgerRow], intent: &Intent) -> Option<Ref> {
    let ok = |r: &Ref| match r {
        Ref::Demigod(x) => *x != d && w.demigods.get(x.ix()).is_some(),
        Ref::Char(c) => w.characters.get(c.ix()).is_some(),
        Ref::Cluster(c) => w.clusters.get(c.ix()).is_some(),
        Ref::Mob(_) | Ref::Cell(_) => true,
        _ => false,
    };
    rows.iter()
        .flat_map(|r| r.targets.iter())
        .find(|r| ok(r))
        .copied()
        .or_else(|| intent.targets.iter().find_map(|t| match t {
            Target::Known(r) if ok(r) => Some(*r),
            _ => None,
        }))
}

fn kind_of(w: &World, r: Ref) -> &'static str {
    match r {
        Ref::Demigod(_) => "demigod",
        Ref::Char(_) => "person",
        Ref::Cluster(c) if w.clusters.get(c.ix()).is_some_and(|c| c.army.is_some()) => "army",
        _ => "crowd",
    }
}

fn pos_of(w: &World, r: Ref) -> Option<LonLat> {
    match r {
        Ref::Cluster(c) => w.clusters.get(c.ix()).and_then(|c| c.army.as_ref().map(|a| a.pos)),
        _ => fn7_society::geo::pos_of(w, r),
    }
}

/// Bring a person forward for a social act with nobody named: the nearest living person already known at the place,
/// else one the society materializes from a crowd there (the same `Memorize` every mention goes through), so the
/// same person is there next time.
fn someone_here(w: &mut World, beat: u32, d: DemigodId, indoors: bool, pantheon: &Pantheon, st: &Settings) -> Option<Ref> {
    let (place, pos) = (w.demigods[d.ix()].place, w.demigods[d.ix()].pos);
    let near = w
        .characters
        .iter()
        .filter(|c| c.alive && c.place == place && c.possessed_by.is_none())
        .map(|c| (fn7_society::geo::distance_km(c.pos, pos), c.id))
        .filter(|(km, _)| *km <= 0.05)
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|x| x.1);
    if let Some(c) = near {
        return Some(Ref::Char(c));
    }
    let cluster = w.clusters.iter().filter(|c| c.place == place && c.size > 0 && c.army.is_none()).map(|c| c.id).min()?;
    let hand = w.demigods[d.ix()].hands[0];
    let text = if indoors { "the one who lives in the house" } else { "a passer-by" };
    let p = Proposal {
        source: Source::Rules,
        intent: None,
        effect: Effect::Memorize { mention: Mention { text: text.into(), kind: "person".into(), near: Some(place) }, cluster: Some(cluster) },
        act: format!("{} meets {text}", w.demigods[d.ix()].name),
        actor: Ref::Agent(hand),
        magnitude: 1,
        valence: 0,
        witnesses: vec![],
        cause: None,
    };
    let (_, out) = crate::merge::run(w, vec![p], pantheon, beat, st.max_witnesses as usize);
    let c = *out.new_chars.first()?;
    crate::beat::record_mut(w, beat).memorized.push(c);
    // A person the scene met stands where the scene is.
    w.characters[c.ix()].pos = pos;
    Some(Ref::Char(c))
}

fn dice_of(w: &World, intent: IntentId) -> Option<(DiceRoll, u32)> {
    let e = w.dm.decided(&format!("act:{}", intent.0))?;
    Some((serde_json::from_value::<DiceRoll>(e.data.clone()).ok()?, e.id))
}

/// Who may get a scene this beat: `token_play` = `all` (every demigod), `humans` (seated, not a bot).
fn wanted(w: &World, d: &Demigod, st: &Settings) -> bool {
    match st.token_play.as_str() {
        "all" => true,
        "humans" => w.seats.iter().any(|s| s.seat == d.seat && !s.is_bot),
        _ => false,
    }
}

/// Is `d`'s seat a human's (the polish only ever runs for these).
pub fn is_human(w: &World, d: Ref) -> bool {
    match d {
        Ref::Demigod(d) => w.demigods.get(d.ix()).is_some_and(|x| w.seats.iter().any(|s| s.seat == x.seat && !s.is_bot)),
        _ => false,
    }
}

/// Build this beat's scenes (at most one per demigod), keep them in the state and return them, in demigod order.
/// Schema 19 only; nothing at all with `token_play: off`.
pub fn build(w: &mut World, beat: u32, row_start: usize, pos_at_start: &[LonLat], pantheon: &Pantheon, st: &Settings) -> Vec<Scene> {
    if !w.token_play_rules() || st.token_play == "off" {
        return vec![];
    }
    let ids: Vec<DemigodId> = w.demigods.iter().filter(|d| wanted(w, d, st)).map(|d| d.id).collect();
    let mut out = vec![];
    for d in ids {
        if let Some(s) = build_one(w, beat, d, row_start, pos_at_start, pantheon, st) {
            out.push(s);
        }
    }
    if !out.is_empty() {
        for s in &out {
            crate::world::event("scene.play", Some(s.demigod.to_string()), s.json());
        }
        w.dm.scenes.extend(out.iter().map(|s| serde_json::to_value(s).unwrap_or(Value::Null)));
        let n = w.dm.scenes.len();
        if n > SCENES_KEPT {
            w.dm.scenes.drain(..n - SCENES_KEPT);
        }
    }
    out
}

/// The intents of `d` this beat a scene may show: a hand's own (never a subagent's or a possession's), routed to the
/// world, or to a god whose ruling is public.
fn showable(w: &World, d: DemigodId, beat: u32) -> Vec<Intent> {
    w.intents
        .iter()
        .filter(|i| i.beat == beat && i.demigod == d)
        .filter(|i| w.agents.get(i.agent.ix()).is_some_and(|a| matches!(a.kind, AgentKind::Hand { .. })))
        .filter(|i| match &i.route {
            Route::World | Route::Pending => true,
            Route::God { .. } => w.rulings.iter().any(|r| r.intent == i.id && crate::beat::ruling_is_public(w, r)),
            _ => false,
        })
        .cloned()
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn build_one(w: &mut World, beat: u32, d: DemigodId, row_start: usize, pos_at_start: &[LonLat], pantheon: &Pantheon, st: &Settings) -> Option<Scene> {
    let intents = showable(w, d, beat);
    if intents.is_empty() {
        return None;
    }
    let rows_of = |w: &World, i: IntentId| -> Vec<LedgerRow> {
        w.ledger.rows[row_start.min(w.ledger.rows.len())..]
            .iter()
            .filter(|r| r.intent == Some(i) && !matches!(r.actor, Ref::Agent(_)))
            .cloned()
            .collect()
    };
    // The act shown: the intent with the biggest row (then the one that was rolled for, then the first).
    let mut best: Option<(u8, bool, std::cmp::Reverse<IntentId>, Intent, Vec<LedgerRow>)> = None;
    for i in &intents {
        let rows = rows_of(w, i.id);
        let rolled = dice_of(w, i.id).is_some() || w.rulings.iter().any(|r| r.intent == i.id);
        if rows.is_empty() && !rolled {
            continue;
        }
        let m = rows.iter().map(|r| r.magnitude).max().unwrap_or(0);
        let key = (m, rolled, std::cmp::Reverse(i.id));
        if best.as_ref().is_none_or(|b| (b.0, b.1, b.2) < key) {
            best = Some((key.0, key.1, key.2, i.clone(), rows));
        }
    }
    let (_, _, _, intent, rows) = best?;
    let rows_ref: Vec<&LedgerRow> = rows.iter().collect();
    let god_route = matches!(intent.route, Route::God { .. });
    // A will the item engine turned into a manifestation is a conjuring, whatever its verb.
    let conjured = rows.iter().find(|r| r.verb == "manifest").map(|r| r.act.clone());
    let act = if conjured.is_some() { Act::Cast } else { act_of(&intent.verb, god_route) };
    let me = &w.demigods[d.ix()];
    let (dname, place, seat) = (me.name.clone(), me.place, (me.seat < crate::SYNTHETIC_SEAT).then_some(me.seat));
    let start_pos = pos_at_start.get(d.ix()).copied().unwrap_or(me.pos);
    let anchor = cell_at(start_pos, HexLevel::People);
    // Only the side of the street is read from the order's words (never shown).
    let words = format!("{} {}", intent.how, intent.span);

    // ---- the outcome, as already decided ----
    let dice = dice_of(w, intent.id);
    let ruling = w.rulings.iter().find(|r| r.intent == intent.id).map(|r| r.verdict);
    let accident = rows.iter().any(|r| matches!(r.actor, Ref::Place(_)) && r.verb == "damage");
    let backfire = rows.iter().any(|r| r.verb == "damage" && r.targets.contains(&Ref::Demigod(d)) && r.actor != Ref::Demigod(d));
    let landed = rows.iter().any(|r| r.verb != "note" && !r.act.contains("comes to nothing"));
    let result = if accident || ruling == Some(Verdict::Twist) || (backfire && dice.as_ref().is_some_and(|x| x.0.verdict == DiceVerdict::CritFail)) {
        "twisted"
    } else if let Some((r, _)) = &dice {
        match r.verdict {
            DiceVerdict::CritSuccess | DiceVerdict::Success => "success",
            DiceVerdict::Fail if landed => "partial",
            _ => "failed",
        }
    } else {
        match ruling {
            Some(Verdict::Grant) | Some(Verdict::Bargain) => "success",
            Some(Verdict::Refuse) => "failed",
            _ if landed => "success",
            _ => "failed",
        }
    };
    // The ledger's line only when that row is a public event (a row's own words may carry what only its witnesses
    // know); otherwise the outcome step's caption, written from names below.
    let public_line = rows
        .iter()
        .filter(|r| !matches!(r.actor, Ref::Place(_)) && crate::payload::is_public_event(w, r))
        .max_by_key(|r| (r.magnitude, std::cmp::Reverse(r.id)))
        .map(|r| clip(r.act.trim().trim_end_matches('.'), OUTCOME_MAX));

    // ---- who is on the stage ----
    let mut target = target_of(w, d, &rows_ref, &intent);
    let needs_someone = matches!(act, Act::Seduce | Act::Talk | Act::Give | Act::Take | Act::Threaten);
    let house_act = matches!(act, Act::Seduce | Act::Search | Act::Hide) || says_house(&words);
    let target_outdoors = target.is_some_and(|t| matches!(kind_of(w, t), "demigod" | "army" | "crowd"));
    let building = if house_act || (needs_someone && !target_outdoors && matches!(act, Act::Talk | Act::Give | Act::Take)) {
        if target_outdoors && !says_house(&words) { None } else { find_building(w, &anchor, beat, &words) }
    } else {
        None
    };
    if target.is_none() && needs_someone {
        target = someone_here(w, beat, d, building.is_some(), pantheon, st);
    }
    let mut cast = Cast { actors: vec![] };
    let a0 = cast.add(Some(Ref::Demigod(d)), dname.clone(), "demigod", &anchor);
    let dirs = preferred_dirs(&words);
    // A creature: an uncovered beast on the stage, for a fight with nobody named.
    let creature = if target.is_none() && act == Act::Fight {
        w.dm.encounters
            .iter()
            .filter(|e| e.outcome.is_some() && e.faded_beat.is_none())
            .filter(|e| matches!(e.kind, fn7_model::dm::EncounterKind::Monster | fn7_model::dm::EncounterKind::Undead | fn7_model::dm::EncounterKind::Ghost))
            .map(|e| (hex_distance(&anchor, &cell_at(e.at, HexLevel::People)), e.id, e.kind))
            .filter(|(k, _, _)| *k <= STAGE_RADIUS)
            .min()
    } else {
        None
    };
    let mut building_json = None;
    let (other, other_cell): (Option<String>, Option<HexCell>) = {
        let cell_for = |w: &World, r: Option<Ref>| -> HexCell {
            if let Some((b, _, _)) = &building {
                return *b;
            }
            let real = r.and_then(|r| pos_of(w, r)).map(|p| cell_at(p, HexLevel::People));
            match real {
                Some(c) if c.face == anchor.face && c != anchor && hex_distance(&anchor, &c) <= STAGE_RADIUS => c,
                _ => step_toward(&anchor, dirs[0], 2),
            }
        };
        match (target, creature) {
            (Some(t), _) => {
                let c = cell_for(w, Some(t));
                let name = match w.name_of(t) {
                    n if n.trim().is_empty() || n == "someone" => match kind_of(w, t) {
                        "crowd" => "the crowd".to_string(),
                        "army" => "the army".to_string(),
                        _ => "a stranger".to_string(),
                    },
                    n => n,
                };
                (Some(cast.add(Some(t), name, kind_of(w, t), &c)), Some(c))
            }
            (None, Some((_, id, kind))) => {
                let c = cell_for(w, None);
                let name = format!("the {}", serde_json::to_value(kind).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or("beast".into()));
                let _ = id;
                (Some(cast.add(None, name, "creature", &c)), Some(c))
            }
            _ => (None, building.as_ref().map(|b| b.0)),
        }
    };
    if let Some((b, src, mem)) = &building {
        let memory = match src.as_str() {
            "houses" => record_building(w, beat, b, Ref::Demigod(d)),
            _ => {
                crate::dm::observe(w, beat, &b.key());
                w.dm.memory.iter().rev().find(|e| e.kind == "place.observed" && e.place == b.key()).map(|e| e.id).or(*mem)
            }
        };
        building_json = Some(Building { cell: b.key(), side: side_of(&anchor, b), source: src.clone(), memory });
    }
    let tname = other.as_ref().map(|id| cast.actors.iter().find(|a| &a.id == id).map(|a| a.name.clone()).unwrap_or_default());
    let tn = tname.clone().unwrap_or_else(|| "someone".into());
    let tkind = other.as_ref().and_then(|id| cast.actors.iter().find(|a| &a.id == id)).map(|a| a.kind.clone()).unwrap_or_default();

    // ---- the steps ----
    let mut dr = Draft { steps: vec![] };
    let mut entered = false;
    // Walk: to the building (then in), or up to the other actor.
    if let Some(bj) = &building_json {
        let b = HexCell::parse(&bj.cell).unwrap_or(anchor);
        let path = hex_line(&anchor, &b);
        dr.push(&a0, "walk", 450 * path.len().max(1) as u32 + 300, Some(&b), path, None, format!("{dname} heads for the house on the {}", bj.side), "neutral");
        dr.push(&a0, "enter", 600, Some(&b), vec![], None, format!("{dname} steps inside"), "neutral");
        entered = true;
    } else if let Some(c) = other_cell {
        let mut path = hex_line(&anchor, &c);
        path.pop();
        if let Some(last) = path.last().copied() {
            let n = path.len() as u32;
            let what = if act == Act::Fight { format!("{dname} closes in on {tn}") } else { format!("{dname} walks up to {tn}") };
            dr.push(&a0, "walk", 450 * n + 300, Some(&last), path, None, what, if act == Act::Fight { "anger" } else { "neutral" });
        }
    }
    let ot = other.as_deref();
    let act_at = dr.steps.len();
    match act {
        Act::Seduce => {
            dr.push(&a0, "talk", 1800, None, vec![], ot, format!("{dname} turns on the charm for {tn}"), "love");
            match result {
                "success" => dr.push(ot.unwrap_or(&a0), "embrace", 1600, None, vec![], Some(&a0), format!("{tn} falls into {dname}'s arms"), "love"),
                "partial" => dr.push(ot.unwrap_or(&a0), "talk", 1400, None, vec![], Some(&a0), format!("{tn} blushes, but keeps a little distance"), "joy"),
                _ => {
                    dr.push(ot.unwrap_or(&a0), "slap", 900, None, vec![], Some(&a0), format!("{tn} slaps {dname}"), "anger");
                    let out = step_toward(other_cell.as_ref().unwrap_or(&anchor), dirs[0], 2);
                    dr.push(ot.unwrap_or(&a0), "flee", 1200, Some(&out), hex_line(other_cell.as_ref().unwrap_or(&anchor), &out), None, format!("{tn} runs out"), "anger");
                }
            }
        }
        Act::Talk | Act::Threaten => {
            let threat = act == Act::Threaten;
            let line = if threat { format!("{dname} menaces {tn}") } else { format!("{dname} talks with {tn}") };
            dr.push(&a0, "talk", 1800, None, vec![], ot, line, if threat { "anger" } else { "neutral" });
            let t = ot.unwrap_or(&a0);
            match (result, threat) {
                ("success", false) => dr.push(t, "celebrate", 1300, None, vec![], Some(&a0), format!("{tn} is won over"), "joy"),
                ("success", true) => dr.push(t, "kneel", 1300, None, vec![], Some(&a0), format!("{tn} cowers before {dname}"), "fear"),
                ("partial", _) => dr.push(t, "talk", 1300, None, vec![], Some(&a0), format!("{tn} hears {dname} out, unsure"), "neutral"),
                _ => {
                    let out = step_toward(other_cell.as_ref().unwrap_or(&anchor), dirs[0], 2);
                    dr.push(t, "flee", 1200, Some(&out), hex_line(other_cell.as_ref().unwrap_or(&anchor), &out), None, format!("{tn} turns away from {dname}"), "anger")
                }
            }
        }
        Act::Give => {
            dr.push(&a0, "give", 1300, None, vec![], ot, format!("{dname} offers {tn} a gift"), "joy");
            let t = ot.unwrap_or(&a0);
            match result {
                "success" => dr.push(t, "celebrate", 1300, None, vec![], Some(&a0), format!("{tn} lights up"), "joy"),
                "partial" => dr.push(t, "take", 1000, None, vec![], Some(&a0), format!("{tn} takes it, warily"), "neutral"),
                _ => dr.push(t, "talk", 1200, None, vec![], Some(&a0), format!("{tn} wants none of it"), "anger"),
            }
        }
        Act::Take => {
            dr.push(&a0, "take", 1100, None, vec![], ot, format!("{dname} reaches for what {tn} has"), "neutral");
            let t = ot.unwrap_or(&a0);
            match result {
                "success" => dr.push(&a0, "celebrate", 1100, None, vec![], None, format!("{dname} makes off with it"), "joy"),
                "partial" => dr.push(t, "flee", 1100, None, vec![], None, format!("{tn} scrambles away"), "fear"),
                _ => dr.push(t, "fight", 1200, None, vec![], Some(&a0), format!("{tn} catches {dname} at it"), "anger"),
            }
        }
        Act::Fight => {
            dr.push(&a0, "fight", 1400, None, vec![], ot, format!("{dname} attacks {tn}"), "anger");
            let t = ot.unwrap_or(&a0);
            let crowd = matches!(tkind.as_str(), "crowd" | "army");
            match result {
                "success" if crowd => dr.push(t, "flee", 1200, None, vec![], None, format!("{tn} breaks and scatters"), "fear"),
                "success" => dr.push(t, "fall", 1100, None, vec![], None, format!("{tn} goes down"), "fear"),
                "partial" => dr.push(t, "fight", 1200, None, vec![], Some(&a0), format!("{tn} reels, and hits back"), "anger"),
                _ => dr.push(t, "fight", 1200, None, vec![], Some(&a0), format!("{tn} shrugs it off"), "anger"),
            }
        }
        Act::Cast => {
            let target_line = if conjured.is_some() {
                format!("{dname} conjures something out of thin air")
            } else if other.is_some() {
                format!("{dname} calls down a power on {tn}")
            } else {
                format!("{dname} calls on the gods")
            };
            dr.push(&a0, "cast", 1400, None, vec![], ot, target_line, "neutral");
            let harmful = rows.iter().any(|r| r.verb == "damage" && !r.targets.contains(&Ref::Demigod(d)));
            match (result, ot) {
                ("success" | "partial", Some(t)) if harmful => dr.push(t, "fall", 1100, None, vec![], None, format!("{tn} is struck"), "fear"),
                ("success" | "partial", Some(t)) => dr.push(t, "celebrate", 1100, None, vec![], None, format!("{tn} glows with it"), "joy"),
                ("success" | "partial", None) => dr.push(&a0, "celebrate", 1100, None, vec![], None, format!("the heavens answer {dname}"), "joy"),
                _ => dr.push(&a0, "kneel", 1100, None, vec![], None, "the power fizzles out".to_string(), "fear"),
            }
        }
        Act::Search => {
            let what = if entered { format!("{dname} searches the house") } else { format!("{dname} searches the ground") };
            dr.push(&a0, "search", 1700, None, vec![], None, what, "neutral");
            let found = w.ledger.rows[row_start.min(w.ledger.rows.len())..]
                .iter()
                .filter(|r| r.intent == Some(intent.id) && r.verb == "create_item")
                .find_map(|r| r.targets.iter().find_map(|t| match t {
                    Ref::Item(i) => w.items.get(i.ix()).map(|x| x.name.clone()),
                    _ => None,
                }));
            match result {
                "success" => dr.push(&a0, "take", 1100, None, vec![], None, match &found {
                    Some(n) => format!("{dname} finds {n}"),
                    None => format!("{dname} finds something"),
                }, "joy"),
                "partial" => dr.push(&a0, "take", 1000, None, vec![], None, format!("{dname} turns up a scrap"), "neutral"),
                _ => dr.push(&a0, "search", 1200, None, vec![], None, format!("{dname} finds nothing"), "neutral"),
            }
        }
        Act::Hide => {
            dr.push(&a0, "hide", 1400, None, vec![], None, format!("{dname} slips out of sight"), "fear");
            if !matches!(result, "success") {
                dr.push(&a0, "flee", 1100, None, vec![], None, format!("{dname} is spotted and bolts"), "fear");
            }
        }
        Act::Flee => {
            let to = step_toward(&anchor, (dirs[0] + 3) % 6, 3);
            dr.push(&a0, "flee", 1600, Some(&to), hex_line(&anchor, &to), None, format!("{dname} runs for it"), "fear");
            if !matches!(result, "success") {
                dr.push(&a0, "fall", 900, None, vec![], None, format!("{dname} trips"), "fear");
            }
        }
        Act::Kneel => {
            dr.push(&a0, "kneel", 1600, None, vec![], ot, format!("{dname} kneels and prays"), "neutral");
            if result == "success" {
                dr.push(&a0, "celebrate", 1100, None, vec![], None, format!("{dname} rises, answered"), "joy");
            }
        }
        Act::Celebrate => {
            dr.push(&a0, "celebrate", 1800, None, vec![], ot, format!("{dname} throws a little party"), "joy");
            if let (true, Some(t)) = (result == "success", ot) {
                dr.push(t, "celebrate", 1200, None, vec![], Some(&a0), format!("{tn} joins in"), "joy");
            }
        }
        Act::Walk => {
            if dr.steps.is_empty() {
                let to = step_toward(&anchor, dirs[0], 3);
                dr.push(&a0, "walk", 1700, Some(&to), hex_line(&anchor, &to), None, format!("{dname} sets off"), "neutral");
            }
            if matches!(result, "success" | "partial") {
                dr.push(&a0, "exit", 600, None, vec![], None, format!("{dname} heads on"), "neutral");
            }
        }
    }
    if result == "twisted" {
        dr.push(&a0, "fall", 1100, None, vec![], None, "a Final Destination moment".to_string(), "fear");
    }
    let demigod_left = dr.steps.iter().any(|s| s.actor == a0 && matches!(s.verb.as_str(), "fall" | "flee" | "exit"));
    if entered && !demigod_left {
        // Back out to the cell the walk came from (the client lowers the roof again).
        let b = HexCell::parse(&building_json.as_ref().map(|b| b.cell.clone()).unwrap_or_default()).unwrap_or(anchor);
        let back = hex_line(&b, &anchor).first().copied().unwrap_or(anchor);
        dr.push(&a0, "exit", 600, Some(&back), vec![], None, format!("{dname} steps back out"), "neutral");
    }
    let outcome_verb = dr.steps.iter().skip(act_at + 1).find(|s| !matches!(s.verb.as_str(), "exit" | "walk")).or(dr.steps.get(act_at)).map(|s| s.verb.clone()).unwrap_or_else(|| "walk".into());
    let outcome_text = public_line.unwrap_or_else(|| {
        let shown = dr.steps.iter().skip(act_at + 1).find(|s| !matches!(s.verb.as_str(), "exit" | "walk")).or(dr.steps.get(act_at));
        shown.map(|s| s.caption.clone()).unwrap_or_else(|| format!("{dname} tries, and it comes to nothing"))
    });
    let total_ms = fit(&mut dr.steps);
    let dice_json = dice.map(|(r, mem)| SceneDice {
        roll: r.roll, modifier: r.modifier, total: r.total, target_dc: r.target_dc, verdict: r.verdict.as_str().into(),
        at_ms: dr.steps.get(act_at).map(|s| s.t_ms + 200).unwrap_or(0), memory: Some(mem),
    });
    Some(Scene {
        scene_id: format!("sc:{beat}:{}", d.0),
        beat,
        demigod: Ref::Demigod(d),
        seat,
        place: ScenePlace { place: Ref::Place(place), name: w.name_of(Ref::Place(place)), cell: anchor.key(), level: "people".into(), building: building_json },
        actors: cast.actors,
        steps: dr.steps,
        dice: dice_json,
        outcome: SceneOutcome { text: outcome_text, result: result.into(), verb: outcome_verb },
        total_ms,
        polished: false,
    })
}

/// The kept scenes of `beat`.
pub fn scenes_of(w: &World, beat: u32) -> Vec<Scene> {
    scenes_in(&w.dm.scenes, beat)
}

/// The scenes of `beat` among `kept`.
pub fn scenes_in(kept: &[Value], beat: u32) -> Vec<Scene> {
    kept.iter().filter_map(|v| serde_json::from_value::<Scene>(v.clone()).ok()).filter(|s| s.beat == beat).collect()
}

// ---------------------------------------------------------------------------------------------------------------------
// Polished captions (applied at the next resolve from the recorded side input)
// ---------------------------------------------------------------------------------------------------------------------

/// Apply polished captions to a kept scene (once: a polished scene stays as it is). Returns the `scene.captions`
/// event, or `None` when the scene is gone, already polished, or the captions do not fit it.
pub fn apply_captions(w: &mut World, scene_id: &str, captions: &[String]) -> Option<Value> {
    apply_to(&mut w.dm.scenes, scene_id, captions)
}

/// [`apply_captions`] on a list of kept scenes.
pub fn apply_to(kept: &mut [Value], scene_id: &str, captions: &[String]) -> Option<Value> {
    let k = kept.iter().position(|v| v["scene_id"] == scene_id)?;
    let mut s: Scene = serde_json::from_value(kept[k].clone()).ok()?;
    if s.polished || captions.len() != s.steps.len() {
        return None;
    }
    for (st, c) in s.steps.iter_mut().zip(captions) {
        let c = clip(c, CAPTION_MAX);
        if !c.is_empty() {
            st.caption = c;
        }
    }
    s.polished = true;
    let caps: Vec<String> = s.steps.iter().map(|x| x.caption.clone()).collect();
    kept[k] = serde_json::to_value(&s).ok()?;
    Some(json!({ "event": "scene.captions", "scene_id": scene_id, "beat": s.beat, "captions": caps }))
}

//! Challenge mode (rules 0.8.1): a strategy board whose fights are decided by real-time play, as in Archon, except
//! the real-time play is TYPING. A demigod locks a rival's in a duel (`fields.challenge`, an instant action, forced),
//! both diorama portraits lock in (Mega Man X style), and every line either player types (`fields.duel`) is a blow
//! resolved at once by rules (a d20, a "wit" score of the line, the fighter's health and gear, a quick follow-up,
//! home turf and the night), with a rules-templated flavor line in the DM's Rube Goldberg voice (no LLM per blow).
//! The duel ends at a knockout or at the next resolve, which applies it to the world.
//!
//! **Live vs authoritative.** The Theatre's `on_instant` is read-only and sees only the last committed state, never
//! the other instant actions of the beat. So the plugin keeps a small per-match, per-beat in-memory cache ([`Live`],
//! keyed by match seed and beat) used ONLY to number and display blows live (`challenge.lock`, `duel.blow`) and to
//! refuse `busy` / `cooldown` / `duel_over` on arrival. The AUTHORITATIVE outcome is recomputed at the resolve from
//! the ordered challenge and duel actions the core delivers (arrival order = `seq`) with the same pure functions
//! ([`check_challenge`], [`Bout::step`]) on the same committed state, so the live events and the resolve agree and a
//! replay (which has no cache) reproduces every state byte for byte. Every blow's d20 is seeded by the match seed and
//! `"<duel_id>:<n>"` (counters `"<duel_id>:<n>:counter"`), never by wall time; timing only feeds the follow-up bonus,
//! the spam penalty and the counters, from the core's `received_at_ms` stamps (recorded with the actions).
//!
//! **Ids.** `duel_id = "duel:<beat>:<a>:<b>"` (the beat the state was in when the challenge arrived, the challenger's
//! and the target's demigod ids). A challenge that arrives while the previous beat is still resolving carries that
//! beat's number and is delivered to the next resolve: blows naming it are matched to it there (the bout adopts the id).
//!
//! **The built-in AI** challenges a rival in reach now and then (`duel_ai_chance`), decided at the round's start from
//! the state ([`ai_challenges`]; announced in `round.started.public.challenges` so a human can fight back) and
//! recomputed identically at the resolve. A duel nobody typed in is fought automatically (`duel_auto_rounds`) from the
//! AI phrase bank.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use fn7_agents::duel::DuelCtx;
use fn7_model::dm::{DiceRoll, DiceVerdict, Scope, Tale, TALES_KEPT};
use fn7_model::hexgrid::{cell_at, HexLevel};
use fn7_model::seed::{hash_str, Rng};
use fn7_model::*;
use fn7_society::geo::{distance_km, place_at, region_at};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theatre_plugin_api::Action;

use crate::actions::{BlowOrder, ChallengeOrder, Input};
use crate::dm::{self, Decision};
use crate::settings::Settings;

/// The board levels a challenge may be issued at, with their hexes (the lock's framing).
pub const LEVELS: [(&str, HexLevel); 4] =
    [("regions", HexLevel::Regions), ("villages", HexLevel::Villages), ("houses", HexLevel::Houses), ("people", HexLevel::People)];

/// Rules 0.8.1 design (Chris's point: everyone on the map should stop what they are doing and watch a contest of wills): an
/// accepted challenge should FREEZE THE WORLD through a room-wide engine hold (no beats resolve, timers pause, only the
/// duellists act; a knockout or `duel_deadline_ms` closes it). The Theatre has no hold window yet, so this capability
/// is off: the duel ends at the next resolve (or its deadline, whichever comes first), and the lock event advertises
/// the hold it would request (`freeze`). Flip it (and return the engine's control from `on_instant`) when the hold
/// API lands; everything else (blows, knockouts, the resolve) is the same either way.
pub const ENGINE_HOLDS: bool = false;

const PHRASE_SALT: u64 = 0xD0E1;
const AI_SALT: u64 = 0xD0E2;

/// A level word (`"regions"`, `"region"`, `"villages"`, `"houses"`, `"people"`...) → its canonical name.
pub fn level_of(s: &str) -> Option<&'static str> {
    match s.trim().to_lowercase().as_str() {
        "regions" | "region" => Some("regions"),
        "villages" | "village" => Some("villages"),
        "houses" | "house" => Some("houses"),
        "people" | "person" => Some("people"),
        _ => None,
    }
}

fn hex_level(level: &str) -> HexLevel {
    LEVELS.iter().find(|l| l.0 == level).map(|l| l.1).unwrap_or(HexLevel::Villages)
}

/// How far (km, great circle) a challenge issued at the client's board `level` reaches: max(`challenge_min_km` (120),
/// the contact distance at that level: regions 600, villages 15, houses 0.15, people 0.015). Two board cards touching
/// at the current zoom, or within 120 km at any zoom, may fight (Chris's concern: the challenge range felt too far);
/// direct combat must be very straightforward to engage).
pub fn reach_km(level: &str, st: &Settings) -> f64 {
    let contact = match level {
        "villages" => st.challenge_contact_km_villages,
        "houses" => st.challenge_contact_km_houses,
        "people" => st.challenge_contact_km_people,
        _ => st.challenge_contact_km_regions,
    };
    (st.challenge_min_km.max(contact) as f64).max(0.0)
}

/// A distance for a refusal, km at 3 significant decimals.
fn km3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// The finest board level whose hex still holds both duellists `km` apart (the lock's framing; regions at most).
pub fn frame_level(km: f64) -> &'static str {
    LEVELS.iter().rev().map(|l| l.0).find(|l| km * 1000.0 <= hex_level(l).size_m()).unwrap_or("regions")
}

/// A player's challenge: `"duel:s<seq>:<a>:<b>"` (the challenge action's room-wide `seq`, known both live and at the
/// resolve, so the id never changes across beats). A built-in AI's: `"duel:<beat>:<a>:<b>"`.
pub fn duel_id_seq(seq: u64, a: DemigodId, b: DemigodId) -> String {
    format!("duel:s{seq}:{}:{}", a.0, b.0)
}

/// Schema 8: a player's challenge id that no duel of this match has used. The core's `seq` restarts at 1 when the
/// server restarts (it is not in the room's snapshot), so `duel:s<seq>:<a>:<b>` could name an earlier duel of the same
/// pair: then the number steps by a million until it is free (checked against the DM's memory and `bouts`).
pub fn fresh_id(w: &World, bouts: &[Bout], seq: u64, a: DemigodId, b: DemigodId) -> String {
    let mut n = seq;
    loop {
        let id = duel_id_seq(n, a, b);
        let used = bouts.iter().any(|x| x.duel.id == id) || w.dm.decided(&format!("{id}:lock")).is_some() || ended(w, &id);
        if !used || !w.resume_rules() {
            return id;
        }
        n += 1_000_000;
    }
}

pub fn duel_id(beat: u32, a: DemigodId, b: DemigodId) -> String {
    format!("duel:{beat}:{}:{}", a.0, b.0)
}

/// `"duel:<beat>:<a>:<b>"` or `"duel:s<seq>:<a>:<b>"` → `(beat or seq, a, b)`.
pub fn parse_id(id: &str) -> Option<(u64, DemigodId, DemigodId)> {
    let mut it = id.strip_prefix("duel:")?.split(':');
    let first = it.next()?;
    let n = first.strip_prefix('s').unwrap_or(first).parse().ok()?;
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    it.next().is_none().then_some((n, DemigodId(a), DemigodId(b)))
}


fn round1(x: f32) -> f32 {
    (x * 10.0).round() / 10.0
}

/// An f32 for JSON at one decimal (no f32 noise like 7.300000190734863).
fn n1(x: f32) -> f64 {
    (x as f64 * 10.0).round() / 10.0
}

// ---------------------------------------------------------------------------------------------------------------------
// Words: the wit of a line
// ---------------------------------------------------------------------------------------------------------------------

const VERBS: &[&str] = &[
    "slash", "stab", "punch", "kick", "hurl", "throw", "parry", "feint", "grab", "bite", "smash", "sweep", "strike",
    "slam", "crush", "choke", "trip", "shove", "headbutt", "uppercut", "jab", "hook", "lunge", "dodge", "block", "swing",
    "cleave", "pierce", "burn", "freeze", "blast", "bash", "whip", "lash", "spin", "tackle", "toss", "fling", "rake",
    "gouge", "claw", "stomp", "wrestle", "flip", "twist", "snap", "break", "shatter", "curse", "hex", "mock", "taunt",
    "roast", "drown", "bury", "impale", "skewer", "sucker", "body-slam", "suplex", "backhand", "pummel", "batter",
];
const BODY: &[&str] = &[
    "jaw", "chin", "nose", "eye", "eyes", "ear", "ears", "teeth", "tooth", "throat", "neck", "shoulder", "arm", "elbow",
    "wrist", "hand", "fist", "finger", "fingers", "chest", "ribs", "rib", "gut", "belly", "stomach", "spine", "back",
    "hip", "knee", "knees", "shin", "ankle", "foot", "feet", "toe", "toes", "skull", "head", "temple", "brow", "heart",
    "lung", "kidney", "thigh", "beard", "hair", "face", "mouth", "tongue", "lip", "lips", "ego", "pride",
];
const TERRAIN: &[&str] = &[
    "river", "sea", "shore", "beach", "sand", "mud", "rock", "stone", "cliff", "hill", "mountain", "forest", "tree",
    "grass", "field", "road", "street", "wall", "roof", "bridge", "market", "rain", "snow", "fog", "wind", "storm",
    "dust", "ice", "ash", "moon", "sun", "shadow", "lantern", "gutter", "puddle", "ruins", "rubble", "temple", "shrine",
    "tower", "gate", "square", "fountain", "alley", "harbor", "harbour", "dock", "lake", "swamp", "desert", "dune",
];
const STOP: &[&str] = &[
    "the", "and", "with", "your", "you", "that", "this", "from", "into", "left", "right", "hand", "hands", "a", "an",
    "for", "his", "her", "their", "them", "then", "they", "what", "when", "will", "have", "just", "like", "over",
];

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !(c.is_alphanumeric() || c == '-'))
        .map(|w| w.trim_matches('-').to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Lore words a line can reference (len ≥ 4, not filler).
fn lore_words(texts: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for t in texts {
        for w in words(t) {
            if w.chars().count() >= 4 && !STOP.contains(&w.as_str()) && !out.contains(&w) {
                out.push(w);
            }
        }
    }
    out
}

fn has_vowel(w: &str) -> bool {
    w.chars().any(|c| "aeiouy".contains(c)) || !w.is_ascii()
}

/// The wit of a line: specificity scores (an action verb +1, two +1 more, a body part +1, an item the fighter holds
/// +1, a terrain or place word +1, a reference to the opponent's story or scars +1: at most +4), repetition and spam
/// cost (the same line again −2, nearly the same −1, under three words −1, gibberish −1): −3..+4. Returns the score
/// and its tags.
pub fn wit(text: &str, me: &Fighter, foe: &Fighter, duel: &Duel, said: &[String]) -> (i32, Vec<&'static str>) {
    let ws = words(text);
    if ws.is_empty() {
        return (-3, vec!["empty"]);
    }
    let mut tags = vec![];
    let mut verbs: Vec<&str> = vec![];
    for w in &ws {
        if let Some(v) = VERBS.iter().find(|v| w.starts_with(**v)) {
            if !verbs.contains(v) {
                verbs.push(v);
            }
        }
    }
    let mut up = 0;
    if !verbs.is_empty() {
        up += 1;
        tags.push("verb");
    }
    if verbs.len() >= 2 {
        up += 1;
        tags.push("combo");
    }
    if ws.iter().any(|w| BODY.contains(&w.as_str())) {
        up += 1;
        tags.push("body");
    }
    if ws.iter().any(|w| me.items.contains(w)) {
        up += 1;
        tags.push("item");
    }
    if ws.iter().any(|w| TERRAIN.contains(&w.as_str()) || duel.terrain.contains(w)) {
        up += 1;
        tags.push("terrain");
    }
    if ws.iter().any(|w| foe.lore.contains(w)) {
        up += 1;
        tags.push("story");
    }
    let mut down = 0;
    let norm = ws.join(" ");
    if said.contains(&norm) {
        down += 2;
        tags.push("repeat");
    } else if said.iter().any(|p| jaccard(p, &norm) >= 0.6) {
        down += 1;
        tags.push("echo");
    }
    if ws.len() < 3 {
        down += 1;
        tags.push("short");
    }
    let vowelless = ws.iter().filter(|w| !has_vowel(w)).count();
    let stutter = text.chars().collect::<Vec<_>>().windows(5).any(|x| x.iter().all(|c| *c == x[0] && !c.is_whitespace()));
    if vowelless * 2 > ws.len() || stutter {
        down += 1;
        tags.push("spam");
    }
    ((up.min(4) - down).clamp(-3, 4), tags)
}

fn jaccard(a: &str, b: &str) -> f64 {
    let x: std::collections::BTreeSet<&str> = a.split(' ').collect();
    let y: std::collections::BTreeSet<&str> = b.split(' ').collect();
    let u = x.union(&y).count();
    if u == 0 {
        return 0.0;
    }
    x.intersection(&y).count() as f64 / u as f64
}

// ---------------------------------------------------------------------------------------------------------------------
// The fighters and the duel
// ---------------------------------------------------------------------------------------------------------------------

/// Rules 0.8.1: a demigod summoned this beat before the challenge: it fights from its destination, at its health
/// minus the summoning's pending mishap ("teleporting into an opponent with a 2-3 life penalty is crazy dangerous").
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Moved {
    pub demigod: DemigodId,
    pub to: LonLat,
    pub lost: f32,
}

fn moved_of(moved: &[Moved], d: DemigodId) -> Option<&Moved> {
    moved.iter().rev().find(|m| m.demigod == d)
}

/// Where a demigod stands for a challenge (its pending summon's destination, else its position) and its health.
fn stance(w: &World, moved: &[Moved], d: DemigodId) -> (LonLat, f32) {
    let x = &w.demigods[d.ix()];
    match moved_of(moved, d) {
        Some(m) => (m.to, round1((x.health - m.lost).max(0.1))),
        None => (x.pos, round1(x.health)),
    }
}

/// A duellist as the duel saw it at the lock (from the committed state).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fighter {
    pub id: DemigodId,
    pub seat: u8,
    pub name: String,
    /// Built-in AI or a Theatre bot (`table::controller`): it is countered for, and its counters always fire.
    pub ai: bool,
    pub health: f32,
    pub max: f32,
    pub wardrobe: u8,
    /// Words of the items it holds (a line naming one is specific).
    pub items: Vec<String>,
    pub has_items: bool,
    /// Words of its story others can throw at it: its names, scars and tales.
    pub lore: Vec<String>,
    /// The duel is on its home turf (its starting region).
    pub home: bool,
    /// Its army is near (within 500 km) or its Following numbers 100+: its blows may call them in (animation cues).
    pub army: bool,
    /// Its player's bolts at the lock (an invoke costs one, paid at the resolve).
    pub bolts: u32,
    /// Rules 0.9.0: the duel bonus of its gear (`wills::arms`), on top of "any item +1". 0 (absent) before.
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub arms: i32,
}

fn is_zero_i32(x: &i32) -> bool {
    *x == 0
}

fn fighter(w: &World, d: DemigodId, region: PlaceId, moved: &[Moved]) -> Fighter {
    let x = &w.demigods[d.ix()];
    let (pos, health) = stance(w, moved, d);
    let me = Ref::Demigod(d);
    let items: Vec<&str> = w
        .items
        .iter()
        .filter(|i| i.holders.last() == Some(&me) && i.ext.state == fn7_model::manifest::ItemState::Held)
        .map(|i| i.name.as_str())
        .collect();
    let story = w.dm.story(d);
    let mut lore_src: Vec<&str> = vec![x.name.as_str(), x.player_name()];
    if let Some(s) = story {
        lore_src.extend(s.scars.iter().map(|s| s.cause.as_str()));
        lore_src.extend(s.tales.iter().map(|t| t.text.as_str()));
    }
    Fighter {
        id: d,
        seat: x.seat,
        name: x.name.clone(),
        ai: crate::table::controller(w, x) == "ai",
        health,
        max: x.max_health,
        wardrobe: story.map(|s| s.wardrobe).unwrap_or(0),
        items: lore_words(&items),
        has_items: !items.is_empty(),
        lore: lore_words(&lore_src),
        home: x.home == Some(region),
        army: crate::beat::own_army(w, d)
            .and_then(|c| w.clusters[c.ix()].army.as_ref().map(|a| distance_km(a.pos, pos) <= 500.0))
            .unwrap_or(false)
            || story.is_some_and(|s| s.followers >= 100),
        bolts: x.bolts,
        arms: crate::wills::arms::duel_bonus(w, d),
    }
}

/// A duel: two fighters locked at a place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Duel {
    pub id: String,
    pub beat: u32,
    pub f: [Fighter; 2],
    /// Where (the target's ground) and its hex at the challenge's level.
    pub at: LonLat,
    pub cell: String,
    pub level: String,
    pub place: String,
    pub phase: String,
    pub night: bool,
    /// Place and time words a line can use.
    pub terrain: Vec<String>,
    pub distance_km: f64,
    /// The challenger's seat (`None`: the built-in AI's challenge).
    pub by_seat: Option<u8>,
    pub seed: u64,
    /// The lock's receive stamp (epoch ms; 0 = unknown, e.g. the AI's challenges at the round's start) and the
    /// deadline it gives the duel (0 = none).
    pub started_ms: i64,
    pub deadline_ms: i64,
    /// The built-in AI's summoning next to its rival before the challenge (applied at the resolve).
    pub leap: Option<Moved>,
    /// The last beat whose resolve may still carry it (without an engine hold the world keeps beating: a duel lasts
    /// `ceil(duel_deadline_ms / flow_ms)` resolves from the beat it locked in, or ends sooner at a knockout or once a
    /// resolve's newest action stamp passes `deadline_ms`).
    #[serde(default)]
    pub ends_beat: u32,
}

impl Duel {
    #[allow(clippy::too_many_arguments)]
    pub fn new(w: &World, beat: u32, a: DemigodId, b: DemigodId, level: &'static str, by_seat: Option<u8>, started_ms: i64,
               moved: &[Moved]) -> Duel {
        Duel::with_id(w, duel_id(beat, a, b), beat, a, b, level, by_seat, started_ms, moved)
    }

    /// [`Duel::new`] under a given id (a player's challenge: [`duel_id_seq`]).
    #[allow(clippy::too_many_arguments)]
    pub fn with_id(w: &World, id: String, beat: u32, a: DemigodId, b: DemigodId, level: &'static str, by_seat: Option<u8>,
                   started_ms: i64, moved: &[Moved]) -> Duel {
        let st = crate::world::settings(w);
        let at = stance(w, moved, b).0;
        let region = region_at(w, at);
        let place = w.name_of(Ref::Place(place_at(w, at)));
        let (_, phase, _, _) = dm::phase_of(beat, st.beats_per_day);
        let region_name = w.name_of(Ref::Place(region));
        let mut terrain = lore_words(&[place.as_str(), region_name.as_str()]);
        terrain.push(phase.to_string());
        let resolves = (st.duel_deadline_ms as u64).div_ceil(st.flow_ms.max(500) as u64).max(1) as u32;
        Duel {
            id,
            beat,
            f: [fighter(w, a, region, moved), fighter(w, b, region, moved)],
            at,
            cell: cell_at(at, hex_level(level)).key(),
            level: level.to_string(),
            place,
            phase: phase.to_string(),
            night: phase == "night",
            terrain,
            distance_km: distance_km(stance(w, moved, a).0, at),
            by_seat,
            seed: w.meta.seed,
            started_ms,
            deadline_ms: if started_ms > 0 { started_ms + st.duel_deadline_ms as i64 } else { 0 },
            leap: None,
            ends_beat: beat + resolves - 1,
        }
    }

    /// The side (0 = the challenger `a`, 1 = the target `b`) a seat fights on.
    pub fn side_of(&self, seat: u8) -> Option<usize> {
        self.f.iter().position(|f| f.seat == seat)
    }
}

fn side_json(w: &World, f: &Fighter) -> Value {
    let d = &w.demigods[f.id.ix()];
    json!({ "demigod": Ref::Demigod(f.id), "name": f.name, "player": d.player_name(),
            "seat": (d.seat < crate::SYNTHETIC_SEAT).then_some(d.seat), "controller": crate::table::controller(w, d),
            "health": n1(f.health), "max_health": n1(f.max), "skin": crate::skin::of(w, d),
            // The media wrapper adds `portrait_image` / `diorama_image` next to these (the lock-in portraits).
            "portrait_prompt": crate::payload::portrait_prompt(w, d), "diorama_prompt": crate::payload::diorama_prompt(w, d) })
}

/// The public `challenge.lock` event.
pub fn lock_json(w: &World, d: &Duel) -> Value {
    let participants: Vec<u8> = d.f.iter().filter(|f| f.seat < crate::SYNTHETIC_SEAT).map(|f| f.seat).collect();
    json!({ "event": "challenge.lock", "duel_id": d.id, "a": side_json(w, &d.f[0]), "b": side_json(w, &d.f[1]),
            "at": d.at, "cell": d.cell, "level": d.level,
            // Every client centres the camera and spins the globe here.
            "focus": { "at": d.at, "lat": d.at[1], "lon": d.at[0], "cell": d.cell, "level": d.level },
            // A knockout or the deadline ends it. Without the engine's hold the world keeps beating meanwhile and the
            // duel lasts at most to `ends_beat`'s resolve (or the deadline's, whichever is first).
            "ends": "ko_or_deadline", "ends_beat": d.ends_beat, "deadline_ms": (d.deadline_ms > 0).then_some(d.deadline_ms),
            "world": if ENGINE_HOLDS { "held" } else { "beating" },
            "freeze": { "requested": true, "supported": ENGINE_HOLDS, "participants": participants,
                        "deadline_ms": (d.deadline_ms > 0).then_some(d.deadline_ms) },
            "leap": d.leap.map(|m| json!({ "demigod": Ref::Demigod(m.demigod), "to": m.to, "health_lost": n1(m.lost) })),
            "beat": d.beat, "place": d.place,
            "time_of_day": d.phase, "distance_km": (d.distance_km * 1000.0).round() / 1000.0, "by_seat": d.by_seat,
            "ai": d.by_seat.is_none() })
}

// ---------------------------------------------------------------------------------------------------------------------
// Flavor and the AI phrase bank
// ---------------------------------------------------------------------------------------------------------------------

const OMENS: &[&str] = &[
    "a heron standing on the wrong leg", "the third crow of the morning", "a bell that rang thirteen", "the baker's burnt loaf",
    "a moth circling a candle backwards", "the tide arriving a minute early", "a goat that licked the moonlight off a wall",
    "a coin that landed on its edge",
];
const LAND: &[&str] = &[
    "{by}'s words hit {foe}'s {part} only because {omen} had been washed in platinum dust at dawn.",
    "The line landed: {foe}'s {part} rang like a temple gong, as {omen} had foretold.",
    "{foe} reeled, because {omen} had promised {by} exactly one good sentence today.",
    "It struck home in {place}, where the cobbles still remember {omen}.",
];
const CRIT: &[&str] = &[
    "So precise that a sparrow three valleys over fainted in sympathy, and {foe}'s {part} filed a complaint with the gods.",
    "{by} said it so perfectly that {omen} reversed itself, and {foe} was struck twice by the same syllable.",
    "The sentence reached {foe}'s {part} before it was finished, which only happens when {omen} aligns.",
];
const MISS: &[&str] = &[
    "{foe} was saved by {omen}, which had been polished for exactly this occasion.",
    "The words slid off {foe} like rain off a freshly oiled shrine.",
    "{by}'s line got lost in {place}'s side streets, as lines do when {omen} is watching.",
    "{foe} ducked, not by skill, but because {omen} had owed them a favour since beat one.",
];
const FUMBLE: &[&str] = &[
    "{by}'s insult boomeranged off a weathervane and came back to bite its own author, as {omen} had warned.",
    "A natural one: {by} tripped over a vowel, and {omen} laughed for a week.",
];
const PENALTY: &str = "Too many words at once: they knotted into a ball of string, and {by} tripped over it.";
const AI_LINES: &[&str] = &[
    "I {verb} your {part} with {item}",
    "{verb}! Right in the {part}, here in the {terrain}",
    "Remember {lore}? I {verb} your {part} for it",
    "I {verb} you into the {terrain}, then {verb2} your {part}",
    "Your {part} again, {foe}, with {item} this time",
    "I {verb} your {part} and {verb2} your pride",
];
const AI_VERBS: &[&str] = &["kick", "punch", "slam", "hook", "jab", "sweep", "headbutt", "smash", "whip", "stomp"];
const AI_PARTS: &[&str] = &["jaw", "ribs", "knee", "nose", "shin", "chin", "ego", "temple"];

fn fill(t: &str, by: &str, foe: &str, part: &str, omen: &str, place: &str) -> String {
    t.replace("{by}", by).replace("{foe}", foe).replace("{part}", part).replace("{omen}", omen).replace("{place}", place)
}

fn pick<'a>(r: &mut Rng, xs: &[&'a str]) -> &'a str {
    xs[r.below(xs.len().max(1) as u64) as usize]
}

/// The flavor line of a strike (rules templates, seeded by the strike's query).
fn flavor(d: &Duel, side: usize, verdict: Option<DiceVerdict>, query: &str, text: &str) -> String {
    let mut r = Rng::derive(d.seed, &[PHRASE_SALT, hash_str(query), 1]);
    let (by, foe) = (&d.f[side].name, &d.f[1 - side].name);
    let part = words(text).into_iter().find(|w| BODY.contains(&w.as_str())).unwrap_or_else(|| pick(&mut r, AI_PARTS).into());
    let omen = pick(&mut r, OMENS);
    let t = match verdict {
        None => PENALTY,
        Some(DiceVerdict::CritSuccess) => pick(&mut r, CRIT),
        Some(DiceVerdict::Success) => pick(&mut r, LAND),
        Some(DiceVerdict::Fail) => pick(&mut r, MISS),
        Some(DiceVerdict::CritFail) => pick(&mut r, FUMBLE),
    };
    fill(t, by, foe, &part, omen, &d.place)
}

/// An automatic line for `side` (an AI's counter, or a duel nobody typed in), seeded by its query.
pub fn ai_line(d: &Duel, side: usize, query: &str) -> String {
    let mut r = Rng::derive(d.seed, &[PHRASE_SALT, hash_str(query), 2]);
    let (me, foe) = (&d.f[side], &d.f[1 - side]);
    let t = pick(&mut r, AI_LINES);
    let verb = pick(&mut r, AI_VERBS);
    let verb2 = pick(&mut r, AI_VERBS);
    let part = pick(&mut r, AI_PARTS);
    let item = me.items.get(r.below(me.items.len().max(1) as u64) as usize).cloned().unwrap_or_else(|| "my bare fists".into());
    let terrain = pick(&mut r, TERRAIN).to_string();
    let lore = foe.lore.get(r.below(foe.lore.len().max(1) as u64) as usize).cloned().unwrap_or_else(|| "your last mistake".into());
    let first = foe.name.split(['\'', ' ']).next().unwrap_or(&foe.name).to_string();
    t.replace("{verb2}", verb2)
        .replace("{verb}", verb)
        .replace("{part}", part)
        .replace("{item}", &item)
        .replace("{terrain}", &terrain)
        .replace("{lore}", &lore)
        .replace("{foe}", &first)
}

// ---------------------------------------------------------------------------------------------------------------------
// Animation cues (rules 0.8.1 v1.2): the 3D figures act out every blow
// ---------------------------------------------------------------------------------------------------------------------

/// The fixed move vocabulary of `anim.a` / `anim.b`.
pub const MOVES: &[&str] = &[
    "lunge", "slash", "parry", "dodge", "leap", "cast", "summon_army", "charge", "grapple", "taunt", "stagger", "fall",
    "block", "blast", "vanish", "rise",
];
/// `anim.fx` from the line's element words.
pub const FX: &[(&str, &[&str])] = &[
    ("fire", &["fire", "flame", "burn", "blaze", "lava", "ember", "inferno", "scorch", "torch"]),
    ("ice", &["ice", "frost", "freeze", "frozen", "snow", "icicle", "cold"]),
    ("lightning", &["lightning", "thunder", "shock", "spark", "bolt", "storm", "zap"]),
    ("earth", &["earth", "stone", "rock", "mud", "quake", "boulder", "sand", "dirt"]),
    ("water", &["water", "wave", "sea", "river", "flood", "drown", "rain", "tide"]),
    ("shadow", &["shadow", "dark", "darkness", "void", "shade", "night"]),
    ("light", &["light", "sun", "radiant", "holy", "dawn", "glow", "blinding"]),
    ("poison", &["poison", "venom", "toxic", "acid", "sting"]),
];
/// Attack verbs → moves (first match by prefix; checked in this order).
const MOVE_VERBS: &[(&str, &[&str])] = &[
    ("vanish", &["vanish", "disappear", "teleport", "blink"]),
    ("leap", &["leap", "jump", "vault", "pounce", "dive"]),
    ("charge", &["charge", "rush", "ram", "tackle", "stampede"]),
    ("grapple", &["grab", "choke", "wrestle", "suplex", "flip", "twist", "body-slam", "trip", "pin", "throttle", "hug"]),
    ("taunt", &["mock", "taunt", "roast", "insult", "laugh", "jeer", "sneer"]),
    ("cast", &["curse", "hex", "cast", "summon", "conjure", "enchant", "pray", "chant"]),
    ("blast", &["blast", "burn", "freeze", "drown", "shock", "zap", "explode", "fire"]),
    ("lunge", &["stab", "pierce", "impale", "skewer", "lunge", "jab", "thrust", "spear"]),
    ("slash", &["slash", "cleave", "whip", "lash", "rake", "claw", "gouge", "cut", "swing", "slice", "sweep", "kick", "backhand"]),
    ("lunge", &["punch", "hook", "uppercut", "smash", "slam", "headbutt", "stomp", "bash", "pummel", "batter", "strike", "hit", "elbow"]),
    ("parry", &["parry", "deflect", "counter"]),
    ("block", &["block", "shield", "guard"]),
    ("dodge", &["dodge", "duck", "sidestep", "evade", "weave"]),
];
const ARMY_WORDS: &[&str] = &["army", "armies", "legion", "troops", "soldiers", "followers", "host", "horde", "battalion", "faithful"];

/// The cues for one strike by `side`: `{a, b, fx, camera, army, attacker}` (`a` / `b` are the duel's sides: the
/// challenger's and the target's figures). Deterministic: the line's verbs and elements, the verdict, the items, the
/// army nearby, and a seeded pick of the defence on a miss.
fn anim(d: &Duel, side: usize, text: &str, verdict: Option<DiceVerdict>, ko: bool, query: &str) -> Value {
    let ws = words(text);
    let me = &d.f[side];
    let mut r = Rng::derive(d.seed, &[PHRASE_SALT, hash_str(query), 3]);
    let verb = ws.iter().find_map(|w| MOVE_VERBS.iter().find(|(_, vs)| vs.iter().any(|v| w.starts_with(v))).map(|x| x.0));
    let fx = ws.iter().find_map(|w| FX.iter().find(|(_, xs)| xs.contains(&w.as_str())).map(|x| x.0));
    let calls_army = me.army && (ws.iter().any(|w| ARMY_WORDS.contains(&w.as_str())) || verdict == Some(DiceVerdict::CritSuccess));
    let held = ws.iter().any(|w| me.items.contains(w));
    let (atk, def) = match verdict {
        None => ("stagger", "taunt"),
        Some(v) => {
            let atk = if calls_army {
                "summon_army"
            } else if let Some(m) = verb {
                m
            } else if me.army && v == DiceVerdict::Success {
                "charge"
            } else if fx.is_some() {
                "blast"
            } else if held {
                "slash"
            } else {
                "lunge"
            };
            let def = match v {
                _ if ko => "fall",
                DiceVerdict::CritSuccess | DiceVerdict::Success => "stagger",
                DiceVerdict::CritFail => "taunt",
                DiceVerdict::Fail => match atk {
                    "grapple" => "block",
                    "cast" | "blast" | "summon_army" => ["dodge", "vanish"][r.below(2) as usize],
                    _ => ["parry", "dodge", "block"][r.below(3) as usize],
                },
            };
            // A natural 1 trips its author.
            (if v == DiceVerdict::CritFail { "stagger" } else { atk }, def)
        }
    };
    let camera = match verdict {
        _ if ko => "close",
        Some(DiceVerdict::CritSuccess) => "close",
        None | Some(DiceVerdict::CritFail) => "wide",
        _ if side == 0 => "over_a",
        _ => "over_b",
    };
    let (a, b) = if side == 0 { (atk, def) } else { (def, atk) };
    json!({ "a": a, "b": b, "fx": fx, "camera": camera, "army": calls_army || atk == "charge" && me.army,
            "attacker": if side == 0 { "a" } else { "b" } })
}

// ---------------------------------------------------------------------------------------------------------------------
// The bout: the deterministic duel engine
// ---------------------------------------------------------------------------------------------------------------------

/// One die thrown in a duel (for `dice[]` at the resolve).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Strike {
    pub query: String,
    pub by: DemigodId,
    pub roll: DiceRoll,
}

/// A duel in progress: a pure function of the duel and the ordered blows (live and at the resolve alike). Kept in the
/// state between resolves (`World::dm.duels`) while it lasts; `log` and `strikes` are this resolve's only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bout {
    pub duel: Duel,
    pub hp: [f32; 2],
    /// Blows numbered so far (typed and automatic; counters ride on their blow's number).
    pub n: u32,
    pub last_ms: [Option<i64>; 2],
    pub last_seq: [Option<u64>; 2],
    pub said: [Vec<String>; 2],
    /// Every blow event since the last resolve, in order (with its counter).
    #[serde(skip)]
    pub log: Vec<Value>,
    #[serde(skip)]
    pub strikes: Vec<Strike>,
    /// The side knocked out.
    pub ko: Option<usize>,
    pub typed: u32,
    pub lines: Vec<String>,
    /// The formal state between blows (range, initiative, stances: [`crate::choreo`]).
    pub flow: crate::choreo::Flow,
    /// Bolts each side spent on invokes (paid at the resolve).
    pub invoked: [u32; 2],
    /// What earlier resolves already applied to the world: damage taken and bolts paid per side.
    #[serde(default)]
    pub applied: [f32; 2],
    #[serde(default)]
    pub paid: [u32; 2],
    /// A resolve already recorded its lock (and landed the AI's leap).
    #[serde(default)]
    pub opened: bool,
    /// The newest action `seq` a resolve delivered to it (the live cache replays only later lines on top).
    #[serde(default)]
    pub delivered_seq: u64,
    /// Schema 8 (exact resume): every blow event so far, in order, with its stamps (`at_ms`, `seq`): the duel's whole
    /// timeline (`log` is only this resolve's part of it).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blows: Vec<Value>,
    /// Schema 8: this bout keeps `blows` (set at its lock in a schema-8 state; older states serialize as before).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub full: bool,
    /// Schema 8: the spans `[from_ms, to_ms]` its clock stood still (the room was paused or the server was down);
    /// `duel.deadline_ms` already includes them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pauses: Vec<[i64; 2]>,
    /// Schema 9 (rules 0.8.2, [`crate::panel`]): the duel's scene (`duel.scene`), every panel so far (`duel.panel`)
    /// and where the panel cycle stands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub panels: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle: Option<crate::panel::Cycle>,
    /// Schema 12 ([`crate::moves`]): a line's words pick its move class and its clip (`duel.blow.move`), and the
    /// panels fill idle time with steps and shuffles.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub moves: bool,
    /// Schema 12 ([`crate::duel_ai`]): its AI fighters type a line in every input window: the scripts not yet entered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai: Option<crate::duel_ai::Plan>,
}

impl Bout {
    pub fn new(duel: Duel) -> Bout {
        let hp = [duel.f[0].health, duel.f[1].health];
        let flow = crate::choreo::Flow::new(&duel.level);
        Bout { duel, hp, n: 0, last_ms: [None; 2], last_seq: [None; 2], said: [vec![], vec![]], log: vec![], strikes: vec![],
               ko: None, typed: 0, lines: vec![], flow, invoked: [0; 2], applied: [0.0; 2], paid: [0; 2], opened: false, delivered_seq: 0,
               blows: vec![], full: false, pauses: vec![], scene: None, panels: vec![], cycle: None, moves: false, ai: None }
    }

    /// [`Bout::new`] in `w`'s rules: a schema-8 state keeps the duel's whole timeline.
    pub fn new_in(w: &World, duel: Duel) -> Bout {
        let mut b = Bout::new(duel);
        b.full = w.resume_rules();
        b.moves = w.ai_duel_rules();
        crate::panel::open(&mut b, w);
        b
    }

    fn keep(&mut self, ev: &Value) {
        if self.full {
            self.blows.push(ev.clone());
        }
    }

    /// Schema 8: the duel's clock stood still from `from_ms` to `to_ms`: its deadline moves by as much.
    pub fn pause(&mut self, from_ms: i64, to_ms: i64) {
        if to_ms <= from_ms || self.ko.is_some() {
            return;
        }
        // Only the part after its lock counts.
        let from = from_ms.max(self.duel.started_ms);
        if self.duel.deadline_ms > 0 && to_ms > from {
            self.duel.deadline_ms += to_ms - from;
            self.pauses.push([from, to_ms]);
            crate::panel::shift(self, to_ms - from);
        }
    }

    /// Ms of duel time elapsed at `at_ms` (wall time since the lock minus the pauses), and ms left to its deadline
    /// (`None`: it has no stamped lock).
    pub fn clock(&self, at_ms: i64) -> Option<(i64, i64)> {
        let d = &self.duel;
        if d.started_ms <= 0 || d.deadline_ms <= 0 {
            return None;
        }
        let paused: i64 = self.pauses.iter().map(|p| (at_ms.min(p[1]) - p[0]).max(0)).sum();
        Some(((at_ms - d.started_ms - paused).max(0), d.deadline_ms - at_ms.max(d.started_ms)))
    }

    fn hp_json(&self) -> Value {
        json!({ "a": n1(self.hp[0]), "b": n1(self.hp[1]) })
    }

    fn hurt(&mut self, side: usize, x: f32) {
        self.hp[side] = round1((self.hp[side] - x).max(0.0));
        if self.hp[side] <= 0.0 && self.ko.is_none() {
            self.ko = Some(side);
        }
    }

    /// One strike by `side` (a typed line, a counter or an automatic blow).
    #[allow(clippy::too_many_arguments)]
    fn strike(&mut self, w: &World, st: &Settings, side: usize, text: &str, query: String, followup: bool, auto: bool) -> Value {
        self.strike_capped(w, st, side, text, query, followup, auto, auto.then_some(2), 0)
    }

    /// [`Bout::strike`] with the line's wit capped at `cap` (an automatic line and an AI's typed line: 2) and
    /// `handicap` taken off the modifier (an AI's typed line: `duel_ai_handicap`; shown as `modifiers.handicap`).
    #[allow(clippy::too_many_arguments)]
    fn strike_capped(&mut self, w: &World, st: &Settings, side: usize, text: &str, query: String, followup: bool, auto: bool,
                     cap: Option<i32>, handicap: i32) -> Value {
        let (me, foe) = (self.duel.f[side].clone(), self.duel.f[1 - side].clone());
        let (mut wit, tags) = wit(text, &me, &foe, &self.duel, &self.said[side]);
        if let Some(cap) = cap {
            wit = wit.min(cap);
        }
        let health = if self.hp[side] >= 7.0 {
            1
        } else if self.hp[side] <= 3.0 {
            -1
        } else {
            0
        };
        let gear = me.wardrobe as i32 / 2 + me.has_items as i32 + me.arms;
        let follow = followup as i32;
        let home = me.home as i32;
        // The formal model: the line's move class, gated by range, bolts and armies; the situation's modifier.
        use crate::choreo::{self, MoveClass};
        let flow = self.flow.clone();
        // Schema 12: the line's words pick the class (typos and short commands included) and the clip.
        let read = if self.moves { crate::moves::read(text) } else { None };
        let (has_bolt, has_army) = (me.bolts > self.invoked[side], me.army);
        let class = if self.moves {
            crate::moves::effective(read.as_ref().map(|r| r.class).unwrap_or_else(|| choreo::classify(text)), flow.range, has_bolt, has_army)
        } else {
            choreo::effective(choreo::classify(text), flow.range, has_bolt, has_army)
        };
        let situ = choreo::situation(&flow, side);
        let modifier = wit + health + gear + follow + home + situ - handicap;
        let dc = 10 + self.duel.night as i32 + foe.wardrobe as i32 / 2;
        let roll = dm::roll_d20(w, &query, modifier, dc);
        let ok = roll.verdict.ok();
        let margin = roll.total - dc;
        let x = choreo::exchange(&flow, side, class, ok, margin, roll.verdict == DiceVerdict::CritSuccess);
        if class == MoveClass::Invoke {
            self.invoked[side] += 1;
        }
        let base = match roll.verdict {
            DiceVerdict::CritSuccess => 4.0,
            DiceVerdict::Success => 1.0 + (margin >= 5) as i32 as f32 + (wit >= 3) as i32 as f32,
            _ => 0.0,
        };
        let raw = if base > 0.0 && x.mult > 0.0 { (base * x.mult + x.bonus).clamp(0.0, 4.0) } else { 0.0 };
        let damage = round1(raw * st.duel_damage_scale);
        self.hurt(1 - side, damage);
        self.flow = x.after.clone();
        let ko = self.ko == Some(1 - side);
        if ko {
            self.flow.stance[1 - side] = choreo::Stance::Downed;
        }
        self.said[side].push(words(text).join(" "));
        let flavor = flavor(&self.duel, side, Some(roll.verdict), &query, text);
        let anim = anim(&self.duel, side, text, Some(roll.verdict), ko, &query);
        let camera = anim["camera"].as_str().unwrap_or("wide").to_string();
        let choreo = choreo::choreo_json(self.duel.seed, &query, &flow, &x, side, text, damage, ko, &camera);
        self.strikes.push(Strike { query, by: me.id, roll: roll.clone() });
        let mut ev = json!({ "by": Ref::Demigod(me.id), "by_name": me.name, "text": text, "roll": roll.roll, "modifier": modifier,
                "total": roll.total, "target_dc": dc, "verdict": roll.verdict, "damage": n1(damage), "hp": self.hp_json(),
                "flavor": flavor, "wit": wit, "tags": tags,
                "modifiers": { "wit": wit, "health": health, "gear": gear, "follow_up": follow, "home": home, "situation": situ },
                "ko": self.ko.is_some(), "auto": auto, "anim": anim, "choreo": choreo });
        if handicap != 0 {
            ev["modifiers"]["handicap"] = (-handicap).into();
        }
        if self.moves {
            let last = self.last_clip(side);
            let mut r = Rng::derive(self.duel.seed, &[PHRASE_SALT, hash_str(&self.duel.id), hash_str(text), self.n as u64, self.said[side].len() as u64, side as u64, 7]);
            let clip = crate::moves::clip(read.as_ref(), class, last.as_deref(), &mut r);
            ev["move"] = json!({ "clip": clip, "word": read.as_ref().map(|x| x.word), "setup": read.as_ref().map(|x| x.setup.clone()).unwrap_or_default(),
                                 "path": read.as_ref().filter(|x| x.leap && x.class == class).map(|_| "leap") });
        }
        ev
    }

    /// A typed line from `side` at `at_ms` (the core's receive stamp; 0 = unknown, then the arrival order `seq`
    /// stands in). Returns the public `duel.blow` event, or `None` once the duel is over.
    pub fn step(&mut self, w: &World, st: &Settings, side: usize, text: &str, at_ms: i64, seq: u64) -> Option<Value> {
        if self.ko.is_some() || side > 1 || self.past_deadline(at_ms) {
            return None;
        }
        self.n += 1;
        self.typed += 1;
        let n = self.n;
        let timed = at_ms > 0;
        let gap = self.last_ms[side].filter(|_| timed).map(|t| at_ms - t);
        let spam = gap.is_some_and(|g| g < st.duel_min_gap_ms as i64);
        let followup = !spam
            && if timed {
                gap.is_some_and(|g| g <= st.duel_followup_ms as i64)
            } else {
                self.last_seq[side].is_some_and(|s| s + 1 == seq)
            };
        let foe = 1 - side;
        // Schema 12: an AI that typed its own line for this window does not also counter in it.
        let foe_idle = !crate::duel_ai::typed_this_window(self, foe)
            && (self.duel.f[foe].ai
            || if timed {
                self.last_ms[foe].is_none_or(|t| at_ms - t > st.duel_counter_ms as i64)
            } else {
                self.last_seq[foe].is_none_or(|f| self.last_seq[side].is_some_and(|m| f < m))
            });
        self.last_ms[side] = Some(at_ms);
        self.last_seq[side] = Some(seq);
        let query = format!("{}:{n}", self.duel.id);
        let mut ev = if spam {
            // The rate limit (2 a second by default): no die, the words trip their author.
            let me = self.duel.f[side].clone();
            let self_damage = round1(0.5 * st.duel_damage_scale);
            self.hurt(side, self_damage);
            json!({ "by": Ref::Demigod(me.id), "by_name": me.name, "text": text, "roll": 0, "modifier": 0, "total": 0,
                    "target_dc": 0, "verdict": "penalty", "damage": 0.0, "self_damage": n1(self_damage),
                    "hp": self.hp_json(), "flavor": flavor(&self.duel, side, None, &query, text), "wit": 0,
                    "tags": ["rate_limit"], "ko": self.ko.is_some(), "auto": false,
                    "anim": anim(&self.duel, side, text, None, false, &query), "choreo": self.penalty_choreo(side) })
        } else {
            self.lines.push(text.to_string());
            self.strike(w, st, side, text, query.clone(), followup, false)
        };
        if self.ko.is_none() && foe_idle {
            let q = format!("{query}:counter");
            let line = ai_line(&self.duel, foe, &q);
            let c = self.strike(w, st, foe, &line, q, false, true);
            ev["counter"] = c;
            ev["ko"] = self.ko.is_some().into();
            ev["hp_after"] = self.hp_json();
        }
        let mut out = json!({ "event": "duel.blow", "duel_id": self.duel.id, "n": n });
        if let (Some(o), Value::Object(e)) = (out.as_object_mut(), ev) {
            o.extend(e);
        }
        out["at_ms"] = at_ms.into();
        out["seq"] = seq.into();
        self.log.push(out.clone());
        self.keep(&out);
        Some(out)
    }

    /// Schema 12: an AI's typed line is entered (its script's `ends_at_ms`): a blow like a human's line, by the same
    /// d20 and matrix, numbered in the duel's order, stamped at its Enter. It has no follow-up bonus, never trips the
    /// rate limit, its wit is capped at 2, `duel_ai_handicap` comes off its modifier (against a human who
    /// entered a line in the last 12 s), and it draws no automatic counter. `ai: true` marks it.
    pub fn ai_blow(&mut self, w: &World, st: &Settings, s: &crate::duel_ai::Script) -> Option<Value> {
        if self.ko.is_some() || s.side > 1 {
            return None;
        }
        self.n += 1;
        let n = self.n;
        let query = format!("{}:{n}", self.duel.id);
        self.lines.push(s.text.clone());
        // The handicap is the price of striking in every window: owed to a human, who cannot; not to another AI, and
        // not to a human silent for 12 s (since the lock or its last line).
        let foe = 1 - s.side;
        let silent = s.ends_at_ms - self.last_ms[foe].unwrap_or(self.duel.started_ms) > crate::duel_ai::IDLE_MS;
        let handicap = if self.duel.f[foe].ai || silent { 0 } else { st.duel_ai_handicap as i32 };
        self.last_ms[s.side] = Some(s.ends_at_ms);
        let ev = self.strike_capped(w, st, s.side, &s.text, query, false, false, Some(2), handicap);
        let mut out = json!({ "event": "duel.blow", "duel_id": self.duel.id, "n": n });
        if let (Some(o), Value::Object(e)) = (out.as_object_mut(), ev) {
            o.extend(e);
        }
        out["ai"] = true.into();
        out["window"] = s.window.into();
        out["at_ms"] = s.ends_at_ms.into();
        out["seq"] = Value::Null;
        if let Some(p) = self.ai.as_mut() {
            p.struck += 1;
            if self.ko.is_some() {
                p.pending.clear();
            }
        }
        self.log.push(out.clone());
        self.keep(&out);
        Some(out)
    }

    /// The clip `side` played for its last move (schema 12: the next one of the same list differs).
    fn last_clip(&self, side: usize) -> Option<String> {
        let who = json!(Ref::Demigod(self.duel.f[side].id));
        self.blows.iter().rev().find_map(|ev| {
            let own = if ev["counter"]["by"] == who { &ev["counter"] } else if ev["by"] == who { ev } else { return None };
            own["move"]["clip"].as_str().map(str::to_string)
        })
    }

    /// A penalty's choreography: its author stumbles, the other waits; the state is unchanged but its author is off
    /// balance and loses the initiative.
    fn penalty_choreo(&mut self, side: usize) -> Value {
        use crate::choreo::Stance;
        let before = self.flow.clone();
        self.flow.stance[side] = Stance::OffBalance;
        self.flow.initiative = 1 - side;
        let name = |s: usize| if s == 0 { "a" } else { "b" };
        let me = json!({ "move_class": null, "pose_keys": [{ "pose": "stumble", "t_ms": 0 }, { "pose": "idle_guard", "t_ms": 400 }],
                         "contact": null, "stance_after": Stance::OffBalance });
        let foe = json!({ "move_class": null, "pose_keys": [{ "pose": "taunt_beckon", "t_ms": 200 }], "contact": null,
                          "stance_after": self.flow.stance[1 - side] });
        let (a, b) = if side == 0 { (me, foe) } else { (foe, me) };
        json!({ "range_before": before.range, "range_after": self.flow.range, "initiative_before": name(before.initiative),
                "initiative_after": name(self.flow.initiative), "flip": before.initiative != self.flow.initiative,
                "result": "penalty", "a": a, "b": b, "camera": "wide", "duration_ms": 600 })
    }

    /// Is `at_ms` past the duel's deadline (`duel_deadline_ms` after the lock)?
    pub fn past_deadline(&self, at_ms: i64) -> bool {
        self.duel.deadline_ms > 0 && at_ms > self.duel.deadline_ms
    }

    /// A duel nobody typed in is fought automatically: `duel_auto_rounds` exchanges from the phrase bank.
    pub fn auto(&mut self, w: &World, st: &Settings) {
        for _ in 0..st.duel_auto_rounds {
            for side in [0, 1] {
                if self.ko.is_some() {
                    return;
                }
                self.n += 1;
                let q = format!("{}:{}", self.duel.id, self.n);
                let line = ai_line(&self.duel, side, &q);
                let ev = self.strike(w, st, side, &line, q, false, true);
                let mut out = json!({ "event": "duel.blow", "duel_id": self.duel.id, "n": self.n });
                if let (Some(o), Value::Object(e)) = (out.as_object_mut(), ev) {
                    o.extend(e);
                }
                self.keep(&out);
                self.log.push(out);
            }
        }
    }

    /// Damage each side took so far.
    pub fn taken(&self) -> [f32; 2] {
        [round1(self.duel.f[0].health - self.hp[0]), round1(self.duel.f[1].health - self.hp[1])]
    }

    /// `(winner, loser)` sides: the knockout, else the one hurt less (a draw within 0.05).
    pub fn result(&self) -> Option<(usize, usize)> {
        if let Some(k) = self.ko {
            return Some((1 - k, k));
        }
        let t = self.taken();
        if (t[0] - t[1]).abs() < 0.05 {
            None
        } else if t[0] > t[1] {
            Some((1, 0))
        } else {
            Some((0, 1))
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Checks
// ---------------------------------------------------------------------------------------------------------------------

/// Was the duel `id` already ended (and recorded) by a resolve?
pub fn ended(w: &World, id: &str) -> bool {
    w.dm.decided(&format!("{id}:end")).is_some()
}

/// The duels a resolve left under way (`World::dm.duels`), in order.
pub fn open_bouts(w: &World) -> Vec<Bout> {
    w.dm.duels.iter().filter_map(|v| serde_json::from_value::<Bout>(v.clone()).ok()).collect()
}

fn on_cooldown(w: &World, d: DemigodId, beat: u32, st: &Settings) -> bool {
    let cd = st.duel_cooldown_beats.max(1);
    if cd <= 1 {
        return false;
    }
    let me = json!(Ref::Demigod(d));
    w.dm.memory
        .iter()
        .rev()
        .take_while(|e| e.beat + cd > beat)
        .any(|e| e.kind == "duel.lock" && e.data["a"]["demigod"] == me && e.beat < beat)
}

/// May `seat` issue challenge `c` at `beat`, with `bouts` under way (the state's open duels and this beat's)? Refusal
/// words (the client's contract, in this order): `challenge_disabled`, `not_yours`, `fallen`, `bad_target`, `too_far`
/// (`too_far:<distance km>:<allowed km>`: beyond [`reach_km`] at the challenge's level), `busy` (either is already in a
/// duel), `cooldown` (its last challenge was under `duel_cooldown_beats` ago). A demigod summoned earlier (`moved`) is
/// checked at its destination.
pub fn check_challenge(w: &World, seat: u8, c: &ChallengeOrder, beat: u32, st: &Settings, bouts: &[Bout], moved: &[Moved]) -> Result<(), String> {
    if !st.challenge_enabled || !w.dm_rules() {
        return Err("challenge_disabled".into());
    }
    let Some(me) = w.demigods.get(c.demigod.ix()).filter(|d| d.seat == seat) else {
        return Err("not_yours".into());
    };
    let Some(foe) = w.demigods.get(c.target.ix()) else {
        return Err("bad_target".into());
    };
    if !me.alive || !foe.alive {
        return Err("fallen".into());
    }
    if foe.seat == seat {
        return Err("bad_target".into());
    }
    let km = distance_km(stance(w, moved, me.id).0, stance(w, moved, foe.id).0);
    let allowed = reach_km(c.level, st);
    if km > allowed {
        return Err(format!("too_far:{}:{}", km3(km), km3(allowed)));
    }
    if bouts.iter().any(|b| b.ko.is_none() && b.duel.f.iter().any(|f| f.id == me.id || f.id == foe.id)) {
        return Err("busy".into());
    }
    // Schema 12: a player fights one duel at a time (it can only type in one).
    if w.ai_duel_rules() && st.duel_one_per_player && player_busy(w, bouts, &[me.id, foe.id]) {
        return Err("busy:player".into());
    }
    if on_cooldown(w, me.id, beat, st) {
        return Err("cooldown".into());
    }
    Ok(())
}

/// Schema 12 (`duel_one_per_player`): is the player of any of `who` already in a duel under way (with any demigod)?
fn player_busy(w: &World, bouts: &[Bout], who: &[DemigodId]) -> bool {
    bouts.iter().filter(|b| b.ko.is_none()).flat_map(|b| b.duel.f.iter()).any(|f| who.iter().any(|d| w.same_player(f.id, *d)))
}

/// The built-in AI's challenges this beat (decided from the state at the round's start; recomputed identically at
/// the resolve): each standing AI-controlled demigod not already in a duel with a rival within `challenge_min_km` challenges
/// the nearest with chance `duel_ai_chance`, framed at the finest level whose hex holds both. `(a, b, level)`.
pub fn ai_challenges(w: &World, beat: u32, st: &Settings) -> Vec<AiChallenge> {
    if !st.challenge_enabled || !w.dm_rules() || st.duel_ai_chance <= 0.0 || st.society_only {
        return vec![];
    }
    let reach = st.challenge_min_km.max(0.0) as f64;
    let far = (st.duel_ai_reach_km as f64).max(reach);
    let fighting: Vec<DemigodId> = open_bouts(w).iter().flat_map(|b| b.duel.f.iter().map(|f| f.id)).collect();
    let mut out: Vec<AiChallenge> = vec![];
    // Schema 12: a player with a demigod in a duel (or about to be in one) is busy with all its demigods.
    let per_player = w.ai_duel_rules() && st.duel_one_per_player;
    let busy = |out: &[AiChallenge], d: DemigodId| {
        let same = |x: DemigodId| x == d || (per_player && w.same_player(x, d));
        fighting.iter().any(|x| same(*x)) || out.iter().any(|x| same(x.a) || same(x.b))
    };
    for d in w.alive_demigods() {
        if crate::table::controller(w, d) != "ai" || busy(&out, d.id) {
            continue;
        }
        let mut r = Rng::derive(w.meta.seed, &[AI_SALT, beat as u64, d.id.0 as u64]);
        if r.f64() >= st.duel_ai_chance as f64 || on_cooldown(w, d.id, beat, st) {
            continue;
        }
        let foe = w
            .alive_demigods()
            .filter(|x| x.seat != d.seat && !busy(&out, x.id))
            .map(|x| (distance_km(d.pos, x.pos), x.id))
            .filter(|(km, _)| *km <= far)
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let Some((km, foe)) = foe else { continue };
        if km <= reach {
            out.push(AiChallenge { a: d.id, b: foe, level: frame_level(km), leap: None });
            continue;
        }
        // Out of reach: it summons itself next to its rival first (a summoning, with its mishap), then challenges.
        let to = fn7_society::geo::destination(w.demigods[foe.ix()].pos, 90.0, 0.004);
        let order = crate::actions::SummonOrder { demigod: d.id, target: to, cell: None, seq: 0 };
        if !st.summon_enabled || crate::summon::check(w, d.seat, &order, beat, st).is_err() {
            continue;
        }
        let lost = round1(crate::summon::mishap(w, beat, &order, d.health, st).2);
        if d.health - lost <= 0.5 {
            continue;
        }
        out.push(AiChallenge { a: d.id, b: foe, level: "people", leap: Some(Moved { demigod: d.id, to, lost }) });
    }
    out
}

/// One of the built-in AI's challenges: `a` challenges `b` at `level`, after summoning itself next to `b` (`leap`)
/// when `b` was beyond reach (but within `duel_ai_reach_km`).
#[derive(Debug, Clone, PartialEq)]
pub struct AiChallenge {
    pub a: DemigodId,
    pub b: DemigodId,
    pub level: &'static str,
    pub leap: Option<Moved>,
}

pub fn ai_bout(w: &World, beat: u32, c: &AiChallenge) -> Bout {
    let moved: Vec<Moved> = c.leap.into_iter().collect();
    // Schema 8: its lock is stamped with the beat's Start on the server's clock when the state knows it (a recorded
    // side input; unknown in simulations), so the duel has a clock and a deadline like a player's.
    let started = w.dm.mark.map(|m| m.round_ms).filter(|_| w.resume_rules()).unwrap_or(0);
    let mut d = Duel::new(w, beat, c.a, c.b, c.level, None, started, &moved);
    d.leap = c.leap;
    Bout::new_in(w, d)
}

// ---------------------------------------------------------------------------------------------------------------------
// The live cache (display only)
// ---------------------------------------------------------------------------------------------------------------------

/// A duel line as it arrived live (replayed onto the state's bout when the cache rolls over to the next beat).
#[derive(Debug, Clone)]
pub struct Raw {
    pub duel_id: String,
    pub seat: u8,
    pub text: String,
    pub at_ms: i64,
    pub seq: u64,
}

#[derive(Debug, Default)]
pub struct BeatLive {
    pub bouts: Vec<Bout>,
    /// Summonings accepted this beat, in arrival order (rules 0.8.1).
    pub moved: Vec<Moved>,
    pub ai_seeded: bool,
    /// Duel lines accepted live this beat.
    pub raw: Vec<Raw>,
}

/// Per match (seed) and beat: the bouts shown live. Never read by the resolve.
#[derive(Debug, Default)]
pub struct Live {
    pub beats: BTreeMap<(u64, u32), BeatLive>,
}

pub type LiveCache = Arc<Mutex<Live>>;

impl Live {
    /// This beat's live bouts: the state's open duels, the AI's challenges, and (rolling over from the last beat) the
    /// lines and challenges that arrived while that beat was resolving (after the actions it was handed).
    fn at(&mut self, w: &World) -> &mut BeatLive {
        let (seed, beat) = (w.meta.seed, w.meta.beat);
        self.beats.retain(|k, _| k.0 != seed || k.1 + 3 >= beat);
        while self.beats.len() > 256 {
            let k = *self.beats.keys().next().expect("non-empty");
            self.beats.remove(&k);
        }
        if !self.beats.contains_key(&(seed, beat)) {
            let st = crate::world::settings(w);
            let mut e = BeatLive { ai_seeded: true, ..Default::default() };
            e.bouts = open_bouts(w);
            for c in ai_challenges(w, beat, &st) {
                e.bouts.push(ai_bout(w, beat, &c));
                if let Some(m) = c.leap {
                    e.moved.push(m);
                }
            }
            if let Some(prev) = beat.checked_sub(1).and_then(|b| self.beats.get(&(seed, b))) {
                for pb in &prev.bouts {
                    if ended(w, &pb.duel.id) {
                        continue;
                    }
                    if let Some(sb) = e.bouts.iter_mut().find(|b| b.duel.id == pb.duel.id) {
                        let since = sb.delivered_seq;
                        let id = sb.duel.id.clone();
                        for r in prev.raw.iter().filter(|r| r.duel_id == id && r.seq > since) {
                            if let Some(side) = sb.duel.side_of(r.seat) {
                                sb.step(w, &st, side, &r.text, r.at_ms, r.seq);
                            }
                        }
                    } else if pb.duel.by_seat.is_some() && !pb.opened {
                        // A challenge the last resolve wasn't handed yet: it stays live until the next one.
                        e.bouts.push(pb.clone());
                        e.raw.extend(prev.raw.iter().filter(|r| r.duel_id == pb.duel.id).cloned());
                    }
                }
            }
            self.beats.insert((seed, beat), e);
        }
        self.beats.get_mut(&(seed, beat)).expect("inserted")
    }
}

fn lock(live: &LiveCache) -> std::sync::MutexGuard<'_, Live> {
    live.lock().unwrap_or_else(|e| e.into_inner())
}

/// Note summonings accepted live (for a challenge right after them).
pub fn note_summons(live: &LiveCache, w: &World, moved: Vec<Moved>) {
    if moved.is_empty() {
        return;
    }
    let mut l = lock(live);
    l.at(w).moved.extend(moved);
}

/// The AI's challenges at the round's start, as `challenge.lock` events (seeded into the cache).
pub fn round_start(live: &LiveCache, w: &World) -> Vec<Value> {
    let mut l = lock(live);
    let e = l.at(w);
    e.bouts.iter().filter(|b| b.duel.by_seat.is_none() && !b.opened).map(|b| lock_json(w, &b.duel)).collect()
}

/// Find the live bout a blow names, and the sender's side.
fn find<'a>(bouts: &'a mut [Bout], w: &World, seat: u8, b: &BlowOrder) -> Result<(&'a mut Bout, usize), String> {
    if ended(w, &b.duel_id) {
        return Err("duel_over".into());
    }
    let bout = bouts.iter_mut().find(|x| x.duel.id == b.duel_id).ok_or("no_duel")?;
    let side = bout.duel.side_of(seat).ok_or("not_yours")?;
    // Schema 12: the first action past the deadline closes the duel (its outro and `challenge.end` are that action's
    // event): such a line is taken, though it is no blow. After that the duel is over for lines.
    let closing = bout.moves && bout.ko.is_none() && crate::panel::running(bout);
    if bout.ko.is_some() || (bout.past_deadline(theatre_trace::now_ms()) && !closing) {
        return Err("duel_over".into());
    }
    if !w.demigods[bout.duel.f[side].id.ix()].alive {
        return Err("fallen".into());
    }
    Ok((bout, side))
}

/// `validate_action` for a challenge or a duel line (on arrival, against the committed state and the live cache).
pub fn validate(live: &LiveCache, w: &World, seat: u8, i: &Input) -> Result<(), String> {
    let st = crate::world::settings(w);
    if !st.challenge_enabled || !w.dm_rules() {
        return Err("challenge_disabled".into());
    }
    let mut l = lock(live);
    let e = l.at(w);
    if let Some(c) = &i.challenge {
        return check_challenge(w, seat, c, w.meta.beat, &st, &e.bouts, &e.moved);
    }
    if let Some(b) = &i.blow {
        if parse_id(&b.duel_id).is_none() {
            return Err("no_duel".into());
        }
        find(&mut e.bouts, w, seat, b)?;
    }
    Ok(())
}

/// `on_instant` for a challenge (`challenge.lock`) or a duel line (`duel.blow`), shown at once.
pub fn on_instant(live: &LiveCache, w: &World, seat: u8, a: &Action, i: &Input) -> Option<Value> {
    let st = crate::world::settings(w);
    let mut l = lock(live);
    let e = l.at(w);
    let mut v = if let Some(c) = &i.challenge {
        check_challenge(w, seat, c, w.meta.beat, &st, &e.bouts, &e.moved).ok()?;
        let id = duel_id_seq(a.seq, c.demigod, c.target);
        let d = Duel::with_id(w, id, w.meta.beat, c.demigod, c.target, c.level, Some(seat), a.received_at_ms, &e.moved);
        let v = lock_json(w, &d);
        e.bouts.push(Bout::new(d));
        crate::world::event("duel_lock", Some(Ref::Demigod(c.demigod).to_string()), v.clone());
        v
    } else {
        let b = i.blow.as_ref()?;
        let (bout, side) = find(&mut e.bouts, w, seat, b).ok()?;
        let v = bout.step(w, &st, side, &b.text, a.received_at_ms, a.seq)?;
        e.raw.push(Raw { duel_id: b.duel_id.clone(), seat, text: b.text.clone(), at_ms: a.received_at_ms, seq: a.seq });
        crate::world::event("duel_blow", Some(b.duel_id.clone()), v.clone());
        v
    };
    v["seq"] = a.seq.into();
    v["received_at_ms"] = a.received_at_ms.into();
    Some(v)
}

// ---------------------------------------------------------------------------------------------------------------------
// The live view in a schema-8 state: the state's bouts plus the journal (crate::present), no cache
// ---------------------------------------------------------------------------------------------------------------------

/// Schema 8: the AI's challenges at the round's start, as `challenge.lock` events.
pub fn round_start_now(j: &crate::present::Journal, w: &World) -> Vec<Value> {
    let st = crate::world::settings(w);
    j.live(w, &st)
        .bouts
        .iter()
        .filter(|b| b.duel.by_seat.is_none() && !b.opened)
        .map(|b| {
            let mut lock = lock_json(w, &b.duel);
            // Rules 0.8.2: its scene and panel 0 come with it (schema 12: and the AI's first typing scripts).
            let also = crate::panel::extras(b, true, 0, 0);
            if !also.is_empty() {
                lock["also"] = also.into();
            }
            lock
        })
        .collect()
}

/// Schema 8: `validate_action` for a challenge or a duel line, against the duels as they stand now.
pub fn validate_now(j: &crate::present::Journal, w: &World, seat: u8, i: &Input) -> Result<(), String> {
    let st = crate::world::settings(w);
    if !st.challenge_enabled || !w.dm_rules() {
        return Err("challenge_disabled".into());
    }
    let mut lv = j.live(w, &st);
    if let Some(c) = &i.challenge {
        return check_challenge(w, seat, c, w.meta.beat, &st, &lv.bouts, &lv.moved);
    }
    if let Some(b) = &i.blow {
        if parse_id(&b.duel_id).is_none() {
            return Err("no_duel".into());
        }
        find(&mut lv.bouts, w, seat, b)?;
    }
    if let Some(id) = &i.tick {
        // Refusals: panels_off (a state before rules 0.8.2), no_duel (no such duel under way, or its panels are
        // done), not_yours (only its duellists tick).
        if !w.panel_rules() {
            return Err("panels_off".into());
        }
        let bout = lv.bouts.iter().find(|x| x.duel.id == *id).ok_or("no_duel")?;
        bout.duel.side_of(seat).ok_or("not_yours")?;
        if !crate::panel::running(bout) {
            return Err("no_duel".into());
        }
    }
    Ok(())
}

/// Schema 8: `on_instant` for a challenge (`challenge.lock`) or a duel line (`duel.blow`): the act applied on the duels
/// as they stand now, exactly as the resolve will apply it (the caller journals it).
pub fn on_instant_now(j: &crate::present::Journal, w: &World, seat: u8, a: &Action, i: &Input) -> Option<Value> {
    let st = crate::world::settings(w);
    let mut lv = j.live(w, &st);
    if let Some(b) = &i.blow {
        find(&mut lv.bouts, w, seat, b).ok()?;
    }
    let act = Act { seat, seq: a.seq, at_ms: a.received_at_ms, challenge: i.challenge.clone(), blow: i.blow.clone(), summons: vec![],
                    tick: i.tick.clone() };
    // Rules 0.8.2: the panels this action makes due go out with its event (`also[]`).
    let target = i.blow.as_ref().map(|b| b.duel_id.clone()).or(i.tick.clone());
    let had = target.as_ref().and_then(|id| lv.bouts.iter().find(|b| b.duel.id == *id)).map(|b| (b.panels.len(), b.blows.len()));
    let ev = apply_act(w, &st, w.meta.beat, &mut lv.bouts, &mut lv.moved, &act, false);
    let ev_was_none = ev.is_none();
    let id = target.or_else(|| ev.as_ref().and_then(|v| v["duel_id"].as_str().map(str::to_string)))?;
    let bout = lv.bouts.iter().find(|b| b.duel.id == id)?;
    // Schema 12: with the panels, the AI's lines this action's stamp entered (`duel.blow`, `ai: true`) and the
    // typing scripts of the windows it opened (`duel.typing_script`).
    let mut also = crate::panel::extras(bout, had.is_none(), had.map(|h| h.0).unwrap_or(0), had.map(|h| h.1).unwrap_or(0));
    let mut v = match ev {
        Some(v) => v,
        // A tick: the first panel it made due is the event (none due: nothing to show, unless it entered an AI's line).
        None if i.tick.is_some() && !also.is_empty() => {
            let k = also.iter().position(|x| x["event"] == "duel.panel").unwrap_or(0);
            also.remove(k)
        }
        // A line that came too late (the AI's line, entered first, ended the duel): what happened instead.
        None if i.blow.is_some() && bout.moves && !also.is_empty() => also.remove(0),
        None => return None,
    };
    match (&i.challenge, &i.blow) {
        (Some(c), _) => crate::world::event("duel_lock", Some(Ref::Demigod(c.demigod).to_string()), v.clone()),
        (_, Some(b)) => crate::world::event("duel_blow", Some(b.duel_id.clone()), v.clone()),
        _ => {}
    }
    let kind = |p: &Value| match p["event"].as_str() {
        Some("duel.scene") => "duel_scene",
        Some("duel.blow") => "duel_blow",
        Some("duel.typing_script") => "duel_typing_script",
        Some("challenge.end") => "duel_end_live",
        _ => "duel_panel",
    };
    for p in &also {
        crate::world::event(kind(p), Some(id.clone()), p.clone());
    }
    if i.challenge.is_none() && ev_was_none {
        crate::world::event(kind(&v), Some(id.clone()), v.clone());
    }
    if !also.is_empty() {
        v["also"] = also.into();
    }
    v["seq"] = a.seq.into();
    v["received_at_ms"] = a.received_at_ms.into();
    Some(v)
}

// ---------------------------------------------------------------------------------------------------------------------
// The resolve (authoritative)
// ---------------------------------------------------------------------------------------------------------------------

/// A challenge or a duel line as delivered to the resolve.
#[derive(Debug, Clone, PartialEq)]
pub struct Act {
    pub seat: u8,
    pub seq: u64,
    pub at_ms: i64,
    pub challenge: Option<ChallengeOrder>,
    pub blow: Option<BlowOrder>,
    /// Rules 0.8.1: summonings in the same stream (a later challenge fights from their destination).
    pub summons: Vec<crate::actions::SummonOrder>,
    /// Rules 0.8.2: a panel tick for this duel.
    pub tick: Option<String>,
}

/// This resolve's challenge and duel input: the acts in arrival order, plus the batch's newest receive stamp (the
/// resolve's only clock, recorded with the actions) and its newest `seq`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Acts {
    pub acts: Vec<Act>,
    pub now_ms: i64,
    pub max_seq: u64,
    /// Schema 8: the newest instant action of the batch, `(received_at_ms, seq)` (the state's delivery mark).
    pub newest: Option<(i64, u64)>,
}

/// An instant action as an [`Act`], when it is a challenge, a duel line or a summoning.
pub fn act_of(seat: u8, text: &str, fields: &Value, seq: u64, at_ms: i64) -> Option<Act> {
    let i = crate::actions::parse(text, fields).ok()?;
    (i.is_duel() || !i.summons.is_empty()).then_some(Act { seat, seq, at_ms, challenge: i.challenge, blow: i.blow, summons: i.summons, tick: i.tick })
}

/// This resolve's challenge and duel actions (instant, from real seats), in arrival order: by `seq`; in a schema-8
/// state (`by_stamp`) by receive stamp, then `seq` (the same order within one server run, and still the arrival order
/// when acts recovered from before a restart, whose `seq` counted from another origin, are in the batch).
pub fn acts_of(actions: &[Action], by_stamp: bool) -> Acts {
    let mut out: Vec<Act> = actions
        .iter()
        .filter(|a| a.instant && !a.defaulted && a.seat < crate::SYNTHETIC_SEAT)
        .filter_map(|a| {
            let i = crate::actions::parse(&a.text, &a.fields).ok()?;
            (i.is_duel() || !i.summons.is_empty()).then_some(Act {
                seat: a.seat, seq: a.seq, at_ms: a.received_at_ms, challenge: i.challenge, blow: i.blow, summons: i.summons, tick: i.tick,
            })
        })
        .collect();
    if by_stamp {
        out.sort_by_key(|a| (a.at_ms, a.seq));
    } else {
        out.sort_by_key(|a| a.seq);
    }
    Acts {
        acts: out,
        now_ms: actions.iter().map(|a| a.received_at_ms).max().unwrap_or(0),
        max_seq: actions.iter().map(|a| a.seq).max().unwrap_or(0),
        newest: actions.iter().filter(|a| a.instant && !a.defaulted).map(|a| (a.received_at_ms, a.seq)).max(),
    }
}

/// A duel fought this beat, applied, waiting for its deed.
#[derive(Debug, Clone)]
pub struct Pending {
    pub bout: Bout,
    pub ctx: DuelCtx,
    /// A human fought in it (its deed asks the LLM).
    pub human: bool,
    /// Why it ended: `ko`, `deadline`, `fallen` or `auto` (two built-in AIs, fought out at once).
    pub reason: &'static str,
    /// Rules 0.8.2: the panels it had before its last resolve (`challenge.end.panels` carries the rest).
    pub panels_from: usize,
}

/// What this resolve did with the duels.
#[derive(Debug, Clone, Default)]
pub struct Resolved {
    /// Duels that ended (their deeds are told in the act batch, then [`record`]).
    pub ended: Vec<Pending>,
    /// Duels still under way: public `challenge.open` entries (saved in `World::dm.duels`).
    pub open: Vec<Value>,
    /// Every duellist of this beat (their hands' travel this beat is void: they're locked in).
    pub pinned: Vec<DemigodId>,
    /// Built-in AI demigods that leapt to their duel this beat.
    pub leapt: Vec<DemigodId>,
}

/// Fight this beat's duels from the committed state (the open ones, the AI's challenges, then the actions in arrival
/// order) and apply them: every blow's d20 in the memory (`dice[]`, reason "a duel"), the damage taken since the last
/// resolve, the invokes' bolts, the lock in the memory at the hex. A duel ENDS at a knockout, when a duellist fell
/// elsewhere, when its last beat (`ends_beat`) resolves or the batch's newest stamp passes its deadline (the world
/// keeps beating: no engine hold yet), or at once between two built-in AIs; then the loser's loot drops on the duel's
/// hex. The others stay open in `World::dm.duels`.
pub fn process(w: &mut World, beat: u32, input: &Acts, pantheon: &Pantheon, st: &Settings) -> Resolved {
    if !st.challenge_enabled || !w.dm_rules() {
        return Resolved::default();
    }
    let mut bouts: Vec<Bout> = open_bouts(w);
    // Rules 0.8.2: the panels each duel had before this resolve (the outcome carries the ones it adds).
    let had: Vec<(String, usize)> = bouts.iter().map(|b| (b.duel.id.clone(), b.panels.len())).collect();
    let had = |id: &str| had.iter().find(|x| x.0 == id).map(|x| x.1).unwrap_or(0);
    bouts.extend(ai_challenges(w, beat, st).iter().map(|c| ai_bout(w, beat, c)));
    let mut moved: Vec<Moved> = bouts.iter().filter(|b| !b.opened).filter_map(|b| b.duel.leap).collect();
    for act in &input.acts {
        apply_act(w, st, beat, &mut bouts, &mut moved, act, true);
    }
    let mut out = Resolved::default();
    let mut still: Vec<Value> = vec![];
    for mut bout in bouts {
        bout.delivered_seq = bout.delivered_seq.max(input.max_seq);
        let fallen = bout.duel.f.iter().any(|f| !w.demigods[f.id.ix()].alive);
        let human = bout.duel.f.iter().any(|f| !f.ai);
        let time_up = beat >= bout.duel.ends_beat || (bout.duel.deadline_ms > 0 && input.now_ms > bout.duel.deadline_ms);
        // Schema 12: its AI fighters type. The lines entered by this batch's clock come first (one may be the
        // knockout), and two AIs fight their windows out to a knockout or the deadline instead of ending at once.
        let typing = bout.ai.is_some();
        if typing && input.now_ms > 0 && !fallen {
            crate::panel::advance(&mut bout, w, st, input.now_ms, false);
        }
        let reason = if bout.ko.is_some() {
            Some("ko")
        } else if fallen {
            Some("fallen")
        } else if !human && !typing {
            Some("auto")
        } else if time_up || ENGINE_HOLDS {
            Some("deadline")
        } else {
            None
        };
        // Rules 0.8.2: the panels due by this batch's clock; an ended duel plays out to its outro. (Schema 12: two
        // AIs keep typing through the windows left, so a duel no client ticked is still fought; against a human the
        // AI enters nothing more once the resolve has ended the duel: nobody could answer.)
        if typing && reason.is_some() && !fallen && !human {
            crate::panel::finish(&mut bout, w, st, true);
        }
        let struck = bout.ai.as_ref().map(|p| p.struck).unwrap_or(0);
        if reason.is_some() && !fallen && bout.typed == 0 && struck == 0 && bout.ko.is_none() {
            bout.auto(w, st);
        }
        let reason = if bout.ko.is_some() { Some("ko") } else { reason };
        if input.now_ms > 0 {
            crate::panel::advance(&mut bout, w, st, input.now_ms, false);
        }
        if reason.is_some() {
            crate::panel::finish(&mut bout, w, st, false);
        }
        let from = had(&bout.duel.id);
        out.pinned.extend(bout.duel.f.iter().map(|f| f.id));
        if let Some(m) = bout.duel.leap.filter(|_| !bout.opened) {
            out.leapt.push(m.demigod);
        }
        match reason {
            Some(why) => {
                let mut p = apply(w, beat, bout, pantheon, st, Some(why)).expect("final");
                p.panels_from = from;
                out.ended.push(p);
            }
            None => {
                let mut bout = bout;
                let log = std::mem::take(&mut bout.log);
                apply_segment(w, beat, &mut bout, pantheon, st, false);
                out.open.push(open_json(&bout, log, from));
                still.push(serde_json::to_value(&bout).unwrap_or(Value::Null));
            }
        }
    }
    w.dm.duels = still;
    out
}

/// One delivered act on the bouts under way (the resolve's loop, and the live view's replay of the acts not yet
/// delivered): a summoning moves a later challenge's ground, a challenge that passes [`check_challenge`] locks a new
/// bout, a duel line is a blow. Returns the public event it makes (`challenge.lock` or `duel.blow`).
pub fn apply_act(w: &World, st: &Settings, beat: u32, bouts: &mut Vec<Bout>, moved: &mut Vec<Moved>, act: &Act, trace: bool) -> Option<Value> {
    if st.rules_0_8_1 {
        for x in &act.summons {
            if crate::summon::check(w, act.seat, x, beat, st).is_ok() {
                let health = w.demigods[x.demigod.ix()].health;
                let lost = crate::summon::mishap(w, beat, x, health, st).2;
                moved.push(Moved { demigod: x.demigod, to: x.target, lost: round1(lost) });
            }
        }
    }
    let mut out = None;
    if let Some(c) = &act.challenge {
        match check_challenge(w, act.seat, c, beat, st, bouts, moved) {
            Ok(()) => {
                let id = fresh_id(w, bouts, act.seq, c.demigod, c.target);
                let d = Duel::with_id(w, id, beat, c.demigod, c.target, c.level, Some(act.seat), act.at_ms, moved);
                out = Some(lock_json(w, &d));
                bouts.push(Bout::new_in(w, d));
            }
            Err(why) if trace => crate::world::event("challenge_refused", Some(Ref::Demigod(c.demigod).to_string()),
                                                     json!({ "beat": beat, "seq": act.seq, "reason": why })),
            Err(_) => {}
        }
    }
    // Rules 0.8.2: a duellist's tick moves its duel's panel cycle on (and nothing else).
    if let Some(id) = &act.tick {
        if let Some(bout) = bouts.iter_mut().find(|x| x.duel.id == *id && x.duel.side_of(act.seat).is_some()) {
            crate::panel::advance(bout, w, st, act.at_ms, true);
        }
    }
    if let Some(b) = &act.blow {
        let bout = bouts.iter_mut().find(|x| x.duel.id == b.duel_id)?;
        let side = bout.duel.side_of(act.seat)?;
        if w.demigods[bout.duel.f[side].id.ix()].alive {
            // The windows this line's stamp has passed close first: it belongs to the one open now.
            crate::panel::advance(bout, w, st, act.at_ms, false);
            out = bout.step(w, st, side, &b.text, act.at_ms, act.seq);
        }
    }
    out
}

/// The public `challenge.open` entry of a duel still under way after this resolve.
pub fn open_json(b: &Bout, log: Vec<Value>, panels_from: usize) -> Value {
    let d = &b.duel;
    let mut v = json!({ "event": "challenge.open", "duel_id": d.id, "a": Ref::Demigod(d.f[0].id), "b": Ref::Demigod(d.f[1].id),
            "hp": b.hp_json(), "blows": b.n, "typed": b.typed, "ends_beat": d.ends_beat,
            "deadline_ms": (d.deadline_ms > 0).then_some(d.deadline_ms), "range": b.flow.range,
            "initiative": if b.flow.initiative == 0 { "a" } else { "b" }, "log": log });
    // Rules 0.8.2: the panels this resolve made due (by number; a client that missed one live has it here).
    if b.cycle.is_some() {
        v["panels"] = crate::panel::since(b, false, panels_from).into();
        v["panels_total"] = b.panels.len().into();
    }
    v
}

/// Apply what the duel did since the last resolve: its lock and the AI's leap (once), every die, the invokes' bolts
/// and the damage taken since (`fin`: a knockout takes everything left).
fn apply_segment(w: &mut World, beat: u32, bout: &mut Bout, pantheon: &Pantheon, st: &Settings, fin: bool) {
    let d = bout.duel.clone();
    if !bout.opened {
        bout.opened = true;
        let wh = dm::locate(w, d.at, HexLevel::People);
        let mut lock = lock_json(w, &d);
        for k in ["a", "b"] {
            if let Some(o) = lock[k].as_object_mut() {
                o.remove("portrait_prompt");
                o.remove("diorama_prompt");
                o.remove("skin"); // display only: never in the state
            }
        }
        dm::remember(w, beat, &wh, Decision { scope: Scope::Cell, kind: "duel.lock", query: Some(format!("{}:lock", d.id)), public: true,
                                              text: format!("{} challenged {} to a duel here", d.f[0].name, d.f[1].name), data: lock });
        // The AI's leap: the summoning lands first (its mishap's d20, the move, the health), as a summon would.
        if let Some(m) = d.leap {
            let order = crate::actions::SummonOrder { demigod: m.demigod, target: m.to, cell: None, seq: 0 };
            let health = w.demigods[m.demigod.ix()].health;
            let (q, roll, _) = crate::summon::mishap(w, beat, &order, health, st);
            dm::record_roll(w, beat, &q, Ref::Demigod(m.demigod), &roll, "a summoning", m.to);
            let name = d.f[0].name.clone();
            let to_place = place_at(w, m.to);
            let mut props = vec![Proposal {
                source: Source::Rules, intent: None,
                effect: Effect::Move { who: Ref::Demigod(m.demigod), to: m.to, place: Some(to_place) },
                act: format!("{name} summons itself to challenge {}", d.f[1].name), actor: Ref::Demigod(m.demigod),
                magnitude: 4, valence: 0, witnesses: vec![Ref::Demigod(m.demigod)], cause: None,
            }];
            if m.lost > 0.0 {
                props.push(Proposal {
                    source: Source::Rules, intent: None,
                    effect: Effect::Damage { who: Ref::Demigod(m.demigod), amount: m.lost, cause: "a reckless summoning".into() },
                    act: format!("{name} lands badly"), actor: Ref::Place(to_place), magnitude: 3, valence: -1,
                    witnesses: vec![Ref::Demigod(m.demigod)], cause: None,
                });
            }
            crate::merge::run(w, props, pantheon, beat, st.max_witnesses as usize);
            w.demigods[m.demigod.ix()].orders.clear();
            if let Some(s) = dm::story_mut(w, m.demigod) {
                s.last_summon = Some(beat);
            }
        }
    }
    for s in std::mem::take(&mut bout.strikes) {
        dm::record_roll(w, beat, &s.query, Ref::Demigod(s.by), &s.roll, "a duel", d.at);
    }
    // Invokes cost a bolt each from the purse (the god answered mid-duel).
    for i in 0..2 {
        let due = bout.invoked[i].saturating_sub(bout.paid[i]);
        if due > 0 {
            let id = d.f[i].id;
            let left = w.demigods[id.ix()].bolts.saturating_sub(due);
            w.set_bolts(id, left);
            bout.paid[i] = bout.invoked[i];
        }
    }
    let taken = bout.taken();
    let place = place_at(w, d.at);
    let mut props = vec![];
    #[allow(clippy::needless_range_loop)]
    for i in 0..2 {
        let (me, foe) = (&d.f[i], &d.f[1 - i]);
        let ko = fin && bout.ko == Some(i);
        let delta = round1(taken[i] - bout.applied[i]);
        if delta <= 0.0 && !ko {
            continue;
        }
        // A knockout takes everything left (the float never leaves a sliver of health).
        let now = w.demigods[me.id.ix()].health;
        let amount = if ko { now.max(me.health).max(taken[i]) + 1.0 } else { delta };
        props.push(Proposal {
            source: Source::Rules,
            intent: None,
            effect: Effect::Damage { who: Ref::Demigod(me.id), amount, cause: format!("a duel with {}", foe.name) },
            act: if ko {
                format!("{} is knocked out in a duel with {}", me.name, foe.name)
            } else {
                format!("{} takes {} in a duel with {}", me.name, delta, foe.name)
            },
            actor: Ref::Place(place),
            magnitude: if ko { 8 } else { (delta.round() as u8 + 3).min(7) },
            valence: -1,
            witnesses: vec![Ref::Demigod(me.id), Ref::Demigod(foe.id)],
            cause: None,
        });
    }
    bout.applied = taken;
    crate::merge::run(w, props, pantheon, beat, st.max_witnesses as usize);
}

/// End a duel: its last segment applied, then the loot and the deed's context.
fn apply(w: &mut World, beat: u32, mut bout: Bout, pantheon: &Pantheon, st: &Settings, reason: Option<&'static str>) -> Option<Pending> {
    let reason = reason?;
    apply_segment(w, beat, &mut bout, pantheon, st, true);
    let d = bout.duel.clone();
    let taken = bout.taken();
    // The loser's loot falls on the duel's hex: everything at a knockout; else, beaten by a clear margin, it rolls to
    // keep hold of its newest thing (DC 11, like a fight).
    let result = bout.result();
    if let Some((win, lose)) = result {
        let loser = d.f[lose].id;
        if bout.ko == Some(lose) {
            let items = dm::held_items(w, loser);
            dm::drop_loot_at(w, beat, loser, items, "a duel", d.at);
        } else if taken[lose] - taken[win] >= 1.0 {
            if let Some(newest) = dm::held_items(w, loser).last().copied() {
                let m = dm::demigod_modifier(w, loser);
                let r = dm::d20(w, beat, &format!("{}:hold", d.id), Ref::Demigod(loser), m, 11, "keeping hold in a duel", d.at);
                if !r.verdict.ok() {
                    dm::drop_loot_at(w, beat, loser, vec![newest], "a duel", d.at);
                }
            }
        }
        if let Some(s) = dm::story_mut(w, d.f[win].id) {
            s.deeds += if bout.ko.is_some() { 2.0 } else { 1.0 };
        }
    }
    let name = |s: Option<usize>| s.map(|i| d.f[i].name.clone());
    let mut lines = bout.lines.clone();
    lines.sort_by_key(|l| std::cmp::Reverse(l.chars().count()));
    lines.truncate(4);
    let ctx = DuelCtx {
        duel_id: d.id.clone(),
        a: d.f[0].name.clone(),
        b: d.f[1].name.clone(),
        winner: name(result.map(|r| r.0)),
        loser: name(result.map(|r| r.1)),
        ko: bout.ko.is_some(),
        damage: taken,
        blows: bout.n,
        lines,
        place: d.place.clone(),
        time: d.phase.to_string(),
        seed: Rng::derive(w.meta.seed, &[hash_str(&d.id), 0xDEED]).next_u64(),
    };
    let human = d.f.iter().any(|f| !f.ai);
    Some(Pending { bout, ctx, human, reason, panels_from: 0 })
}

/// The `challenge.end` payload (`log`: the blows this final resolve was handed; the earlier ones came live and in
/// earlier `challenge.open` entries).
pub fn end_json(p: &Pending, deed: &str) -> Value {
    let b = &p.bout;
    let d = &b.duel;
    let r = b.result();
    let t = b.taken();
    let mut v = json!({ "event": "challenge.end", "duel_id": d.id, "a": Ref::Demigod(d.f[0].id), "b": Ref::Demigod(d.f[1].id),
            "a_name": d.f[0].name, "b_name": d.f[1].name,
            "winner": r.map(|x| Ref::Demigod(d.f[x.0].id)), "loser": r.map(|x| Ref::Demigod(d.f[x.1].id)),
            "ko": b.ko.is_some(), "reason": p.reason, "damage": { "a": n1(t[0]), "b": n1(t[1]) }, "hp": { "a": n1(b.hp[0]), "b": n1(b.hp[1]) },
            "blows": b.n, "typed": b.typed, "deed": deed, "at": d.at, "pos": d.at, "cell": d.cell, "level": d.level, "place": d.place,
            "locked_beat": d.beat, "log": b.log });
    // Rules 0.8.2: the rest of its panels, through the outro (play them out; `starts_at_ms` may lie ahead).
    if b.cycle.is_some() {
        v["panels"] = crate::panel::since(b, false, p.panels_from).into();
        v["panels_total"] = b.panels.len().into();
    }
    v
}

/// Schema 12: the `challenge.end` shown at once when a duel action closes the duel (a knockout's outro, or the
/// first action stamped past the deadline): who won and the healths, `provisional: true`, no deed and no log yet.
/// The next resolve applies the result and sends the full `challenge.end` (the same `duel_id`) as before.
pub fn end_live_json(b: &Bout) -> Value {
    let d = &b.duel;
    let r = b.result();
    let t = b.taken();
    json!({ "event": "challenge.end", "duel_id": d.id, "provisional": true, "a": Ref::Demigod(d.f[0].id), "b": Ref::Demigod(d.f[1].id),
            "a_name": d.f[0].name, "b_name": d.f[1].name,
            "winner": r.map(|x| Ref::Demigod(d.f[x.0].id)), "loser": r.map(|x| Ref::Demigod(d.f[x.1].id)),
            "ko": b.ko.is_some(), "reason": if b.ko.is_some() { "ko" } else { "deadline" },
            "damage": { "a": n1(t[0]), "b": n1(t[1]) }, "hp": { "a": n1(b.hp[0]), "b": n1(b.hp[1]) },
            "blows": b.n, "typed": b.typed, "deed": null, "at": d.at, "pos": d.at, "cell": d.cell, "level": d.level, "place": d.place,
            "locked_beat": d.beat, "panels_total": b.panels.len() })
}

/// Record the told deeds: `duel.end` in the memory at the duel's hex, a tale for both duellists. Returns the public
/// `challenge.end` events (`round.outcome.public.duels[]`).
pub fn record(w: &mut World, beat: u32, pending: &[Pending], deeds: &[String]) -> Vec<Value> {
    let mut out = vec![];
    for (k, p) in pending.iter().enumerate() {
        let deed = deeds.get(k).filter(|x| !x.is_empty()).cloned().unwrap_or_else(|| fn7_agents::duel::fallback(&p.ctx).deed);
        let v = end_json(p, &deed);
        let mut data = v.clone();
        if let Some(o) = data.as_object_mut() {
            o.remove("log");
        }
        let wh = dm::locate(w, p.bout.duel.at, HexLevel::People);
        dm::remember(w, beat, &wh, Decision { scope: Scope::Cell, kind: "duel.end", query: Some(format!("{}:end", p.bout.duel.id)),
                                              public: true, text: deed.clone(), data });
        for f in &p.bout.duel.f {
            if let Some(s) = dm::story_mut(w, f.id) {
                s.tales.push(Tale { beat, kind: "duel".into(), text: deed.clone() });
                if s.tales.len() > TALES_KEPT {
                    s.tales.remove(0);
                }
            }
        }
        out.push(v);
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_levels_and_wit() {
        assert_eq!(parse_id(&duel_id(12, DemigodId(3), DemigodId(7))), Some((12, DemigodId(3), DemigodId(7))));
        assert_eq!(parse_id("duel:1:2"), None);
        assert_eq!((level_of("Region"), level_of("people"), level_of("galaxy")), (Some("regions"), Some("people"), None));
        let st = Settings::default();
        assert_eq!((reach_km("regions", &st), reach_km("villages", &st), reach_km("houses", &st), reach_km("people", &st)),
                   (600.0, 120.0, 120.0, 120.0), "touching cards at regions zoom, else 120 km");
        assert_eq!((frame_level(0.002), frame_level(2.0), frame_level(90.0), frame_level(500.0)), ("people", "villages", "regions", "regions"));
        let w = crate::testkit::world(4, 3);
        let d = Duel::new(&w, 1, DemigodId(0), DemigodId(2), "regions", None, 0, &[]);
        let (me, foe) = (&d.f[0], &d.f[1]);
        let (good, tags) = wit("I kick your knee then smash your jaw into the mud", me, foe, &d, &[]);
        assert!(good >= 3, "{good} {tags:?}");
        let (bad, tags) = wit("xxxxx", me, foe, &d, &[]);
        assert!(bad < 0 && tags.contains(&"spam"), "{bad} {tags:?}");
        let said = vec![words("I kick your knee then smash your jaw into the mud").join(" ")];
        let (again, tags) = wit("I kick your knee then smash your jaw into the mud", me, foe, &d, &said);
        assert!(again <= good - 2 && tags.contains(&"repeat"));
        assert!(wit("", me, foe, &d, &[]).0 == -3);
        // Animation cues: verbs, elements, the army, the verdict; deterministic.
        let mut d2 = d.clone();
        d2.f[0].army = true;
        let x = anim(&d2, 0, "my army charges through a wall of fire", Some(DiceVerdict::Success), false, "q");
        assert_eq!((x["a"].as_str(), x["b"].as_str(), x["fx"].as_str(), x["army"].as_bool()), (Some("summon_army"), Some("stagger"), Some("fire"), Some(true)));
        let y = anim(&d, 1, "I grab your throat", Some(DiceVerdict::Fail), false, "q");
        assert_eq!((y["b"].as_str(), y["a"].as_str(), y["camera"].as_str(), y["attacker"].as_str()), (Some("grapple"), Some("block"), Some("over_b"), Some("b")));
        let z = anim(&d, 0, "frost bites your ear", Some(DiceVerdict::CritSuccess), true, "q");
        assert_eq!((z["b"].as_str(), z["fx"].as_str(), z["camera"].as_str()), (Some("fall"), Some("ice"), Some("close")));
        assert_eq!(anim(&d, 0, "a b", Some(DiceVerdict::Fail), false, "q"), anim(&d, 0, "a b", Some(DiceVerdict::Fail), false, "q"));
    }
}

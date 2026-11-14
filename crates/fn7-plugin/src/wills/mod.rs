//! Wills that matter (rules 0.9.0; room setting `wills_enabled`, on top of the 0.8.1 rules; DM_RULEBOOK chapters
//! 23-27).
//!
//! The designer noted that healing and arming characters with god powers does not make them look any different, and asked why.
//! will translating to actual mega-changes like i should.. be able to revive the other demigods. a player even if
//! their demigods were wiped should be able to try to resurrect their demigods unless the other player cast a no
//! resurrect etc etc etc mystic quest wings of a butterfly to counter a curse" and "healing and arming do nothing".
//!
//! What happens to a WILL here, before the ordinary route (resolver / gods / item engine):
//!
//! 1. **It splits into its parts** ([`kinds_in`]): heal, bless, arm, raise, curse (or seal), ask, offer, gather. The
//!    hand mind may already have split it into intents (one per part); the rules read the player's words again and add
//!    any part the hand dropped, so the LLM path and the rules fallback agree. Each part × each target is resolved on
//!    its own, with its own visible ruling.
//! 2. **The rules and the dice decide** ([`resolve`]): a d20 against a low own-target DC with every modifier named,
//!    a price in sparks (a bolt breaks into `will_spark_per_bolt`; what the purse can't pay becomes a debt to the ruling
//!    god, or a quest), and an outcome that is never a bare "fail": a failed heal is a token heal plus a MYSTIC QUEST
//!    that completes it; a failed arming is gear a tier low plus a quest that tempers it.
//! 3. **It is told where the player looks**: `round.private.wills[]` (per part and per target: the roll against the
//!    DC with its modifiers, what was paid, what happened in plain words, and when it fell short, why and what would
//!    work), a public one-liner where the act is public, and the DM's memory.
//!
//! Everything is seeded and lives in the state (`World.dm.wills`, the [`Book`]), so a replay is exact. No LLM decides
//! anything here; the tellings are rules templates in the Lens's voice.

pub mod arms;
pub mod curses;
pub mod quests;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use fn7_agents::common::has_phrase;
use fn7_model::dm::{DiceRoll, DiceVerdict, Scope};
use fn7_model::hexgrid::HexLevel;
use fn7_model::manifest::{
    Cost, Flavor, ItemExt, ItemKind, ItemState, ManifestRequest, ManifestVerdict, Manifestation, ManifestationId, NewItem, Odds,
    Outcome, Tier,
};
use fn7_model::seed::{hash_str, Rng};
use fn7_model::*;
use fn7_society::geo::distance_km;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::beat::FallenWill;
use crate::dm::{self, Decision};
use crate::settings::Settings;
use crate::world::{event, open_debt};
use curses::{Curse, CurseKind};
use quests::{qref, Goal, Quest};

const WILL_SALT: u64 = 0x3111;

// ---------------------------------------------------------------------------------------------------------------------
// The book (state)
// ---------------------------------------------------------------------------------------------------------------------

/// One raising Death granted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Raised {
    pub demigod: DemigodId,
    pub beat: u32,
    /// The player it was granted to (the cap is theirs).
    pub by_seat: u8,
    /// `paid` | `on credit` | `quest q:N`.
    pub how: String,
}

/// A boon on a demigod: a d20 bonus until a beat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Boon {
    pub demigod: DemigodId,
    pub bonus: i32,
    pub until_beat: u32,
    pub from: String,
}

/// A once-per-match item power that was spent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Used {
    pub item: ItemId,
    pub beat: u32,
    pub power: String,
}

/// Everything "wills that matter" keeps in the state (`World.dm.wills`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Book {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quests: Vec<Quest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub curses: Vec<Curse>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raises: Vec<Raised>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub boons: Vec<Boon>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub used: Vec<Used>,
}

pub fn book(w: &World) -> Book {
    if w.dm.wills.is_null() {
        return Book::default();
    }
    serde_json::from_value(w.dm.wills.clone()).unwrap_or_default()
}

pub fn save(w: &mut World, b: &Book) {
    w.dm.wills = if *b == Book::default() { Value::Null } else { json!(b) };
}

/// The standing d20 modifiers on `d` at `beat`: boons (+) and hexes (−), named.
pub fn standing_mods(w: &World, d: DemigodId, beat: u32) -> Vec<(String, i32)> {
    let b = book(w);
    let mut out = vec![];
    let boon: i32 = b.boons.iter().filter(|x| x.demigod == d && x.until_beat >= beat).map(|x| x.bonus).max().unwrap_or(0);
    if boon != 0 {
        out.push(("boon".to_string(), boon));
    }
    if let Some(c) = curses::active(&b, d, CurseKind::Hex, beat) {
        out.push(("hexed".to_string(), -(c.strength as i32)));
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------------
// Paying
// ---------------------------------------------------------------------------------------------------------------------

/// What a part's price came to.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Paid {
    pub price: u32,
    /// Sparks paid (after any bolts were broken).
    pub sparks: u32,
    /// Bolts broken into sparks for it (the change stays in the purse as sparks).
    pub bolts_broken: u32,
    /// Sparks of the price nobody paid.
    pub short: u32,
}

/// Pay `price` sparks from `payer`'s player's purse: sparks first, then bolts broken into `will_spark_per_bolt` sparks
/// each (the change stays as sparks). `whole`: take nothing unless all of it can be paid.
pub fn pay(w: &mut World, payer: DemigodId, price: u32, whole: bool, st: &Settings) -> Paid {
    let me = &w.demigods[payer.ix()];
    let per = st.will_spark_per_bolt.max(1);
    let (mut sparks, mut bolts, mut broken) = (me.sparks, me.bolts, 0u32);
    while sparks < price && bolts > 0 {
        bolts -= 1;
        sparks += per;
        broken += 1;
    }
    let paid = price.min(sparks);
    if whole && paid < price {
        return Paid { price, sparks: 0, bolts_broken: 0, short: price };
    }
    w.set_sparks(payer, sparks - paid);
    w.set_bolts(payer, bolts);
    if paid > 0 || broken > 0 {
        event("will_paid", Some(Ref::Demigod(payer).to_string()), json!({ "price": price, "sparks": paid, "bolts_broken": broken, "short": price - paid }));
    }
    Paid { price, sparks: paid, bolts_broken: broken, short: price - paid }
}

fn paid_json(p: &Paid, debt: f32) -> Value {
    json!({ "price_sparks": p.price, "sparks": p.sparks, "bolts_broken": p.bolts_broken, "short_sparks": p.short, "debt": r2(debt) })
}

fn r1(x: f32) -> f32 {
    (x * 10.0).round() / 10.0
}
fn r2(x: f32) -> f32 {
    (x * 100.0).round() / 100.0
}

/// 2 when a curse of silence lies on any standing demigod of `seat` (its wills cost double), else 1.
fn price_factor(w: &World, b: &Book, seat: u8, beat: u32) -> u32 {
    if w.bodies_of_seat(seat).any(|d| curses::active(b, d.id, CurseKind::Silence, beat).is_some()) {
        2
    } else {
        1
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Reading a will: its parts
// ---------------------------------------------------------------------------------------------------------------------

/// A part of a will.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Heal,
    Bless,
    Arm,
    Raise,
    Curse,
    Ask,
    Offer,
    Gather,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Heal => "heal",
            Kind::Bless => "bless",
            Kind::Arm => "arm",
            Kind::Raise => "raise",
            Kind::Curse => "curse",
            Kind::Ask => "ask",
            Kind::Offer => "offer",
            Kind::Gather => "gather",
        }
    }
    /// The part a hand's canonical verb names.
    pub fn of_verb(verb: &str) -> Option<Kind> {
        let v = verb.trim().to_lowercase();
        let any = |ws: &[&str]| ws.iter().any(|w| v == *w || v.starts_with(w));
        if any(&["heal", "mend", "cure", "restore", "regenerate", "rejuvenate"]) {
            Some(Kind::Heal)
        } else if any(&["bless", "empower", "embolden", "anoint"]) {
            Some(Kind::Bless)
        } else if v == "arm" || any(&["arming", "armed", "equip", "outfit"]) {
            Some(Kind::Arm)
        } else if any(&["revive", "resurrect", "ressurect"]) {
            Some(Kind::Raise)
        } else if any(&["curse", "hex", "jinx", "wither", "seal"]) {
            Some(Kind::Curse)
        } else {
            None
        }
    }
}

fn any(t: &str, ws: &[&str]) -> bool {
    ws.iter().any(|w| has_phrase(t, w))
}

const OWN_WORDS: &[&str] = &["my", "me", "myself", "us", "our", "ours", "both", "demigod", "demigods", "mydemigods", "hands", "guys", "i", "we", "ourselves"];
const GEAR_NOUNS: &[(&str, &[&str])] = &[
    ("weapon", &["launcher", "launchers", "sword", "swords", "blade", "blades", "spear", "spears", "bow", "bows", "axe", "axes", "hammer",
                 "hammers", "gun", "guns", "rifle", "rifles", "cannon", "cannons", "staff", "staves", "dagger", "daggers", "lance",
                 "lances", "mace", "maces", "whip", "whips", "trident", "tridents", "scythe", "scythes", "club", "clubs", "halberd",
                 "pistol", "pistols", "bazooka", "bazookas", "railgun", "railguns", "crossbow", "crossbows", "katana", "katanas",
                 "glaive", "sabre", "saber", "weapon", "weapons", "rockets", "missiles", "flamethrower", "flamethrowers", "lasers"]),
    ("armor", &["armor", "armour", "shield", "shields", "helm", "helms", "helmet", "helmets", "plate", "mail", "cloak", "cloaks",
                "aegis", "gauntlets", "breastplate", "greaves", "wings"]),
    ("charm", &["relic", "relics", "amulet", "amulets", "talisman", "talismans", "ring", "rings", "crown", "crowns", "charm", "charms",
                "idol", "orb", "orbs", "chalice", "halo", "halos"]),
];
const MAKING: &[&str] = &["manifest", "conjure", "forge", "summon", "materialize", "materialise", "craft", "create", "build", "make", "spawn", "erect"];
const BLESS_WORDS: &[&str] = &["bless", "blesses", "blessed", "blessing", "empower", "empowers", "empowered", "embolden", "anoint",
                               "god power", "god powers", "godpower", "godpowers", "divine power", "divine powers", "holy", "divine", "sacred"];
const RAISE_STRONG: &[&str] = &["back to life", "come back", "comes back", "coming back", "revive", "revives", "revived", "resurrect",
                                "resurrects", "resurrected", "ressurect", "ressurrect", "rise again", "bring back", "bring us back",
                                "return from the dead", "live again", "alive again", "undie", "respawn", "raise the dead", "raise the fallen",
                                "raise my", "raise our", "raise his", "raise her", "raise their", "raise up my", "wake the dead", "undead me"];
const NOT_RAISE: &[&str] = &["army", "armies", "shrine", "temple", "banner", "banners", "fortress", "soldiers", "troops", "monument", "altar"];

/// The gear noun a will names, with the word before it when it reads as part of the name ("rocket launchers" →
/// `("weapon", "Rocket Launcher")`).
pub fn gear_noun(text: &str) -> Option<(&'static str, String)> {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    const SKIP: &[&str] = &["a", "an", "the", "my", "our", "some", "with", "and", "of", "gets", "get", "give", "gives", "new", "two", "both",
                            "their", "his", "her", "its", "fancy", "to", "is", "are", "in", "god", "holy", "blessed", "divine", "sacred",
                            "them", "all", "each", "every", "one", "me", "us", "legendary", "epic", "mythic", "great", "mighty", "magic",
                            "magical", "powerful", "awesome", "better", "best", "good", "big", "huge", "fully", "more", "real", "actual",
                            "proper", "cool"];
    for (i, wd) in words.iter().enumerate() {
        for (slot, nouns) in GEAR_NOUNS {
            if nouns.contains(wd) {
                let single = singular(wd);
                let name = match i.checked_sub(1).map(|j| words[j]).filter(|p| !SKIP.contains(p) && p.len() > 2 && !is_gear_noun(p)) {
                    Some(p) => format!("{} {}", title(p), title(&single)),
                    None => title(&single),
                };
                return Some((slot, name));
            }
        }
    }
    None
}

fn is_gear_noun(w: &str) -> bool {
    GEAR_NOUNS.iter().any(|(_, n)| n.contains(&w))
}

fn singular(w: &str) -> String {
    match w {
        "staves" => "staff".into(),
        "wings" | "gauntlets" | "greaves" | "lasers" => w.into(),
        _ if w.ends_with("ss") => w.into(),
        _ if w.ends_with('s') => w[..w.len() - 1].into(),
        _ => w.into(),
    }
}

fn title(s: &str) -> String {
    s.split([' ', '-'])
        .filter(|x| !x.is_empty())
        .map(|x| {
            let mut c = x.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The demigods a will names: by full name, or by the player's name (with "left"/"right" picking the hand).
pub fn named_demigods(w: &World, text: &str) -> Vec<DemigodId> {
    let t = text.to_lowercase();
    let mut out = vec![];
    for d in &w.demigods {
        let name = d.name.to_lowercase();
        let full = !name.is_empty() && t.contains(&name);
        let player = d.player_name().to_lowercase();
        let by_player = player.len() > 2 && has_phrase(&t, &player) && !d.name.eq_ignore_ascii_case(d.player_name());
        let side_ok = match d.side {
            Some(Hand::Left) => !has_phrase(&t, "right") || has_phrase(&t, "left"),
            Some(Hand::Right) => !has_phrase(&t, "left") || has_phrase(&t, "right"),
            None => true,
        };
        if full || (by_player && side_ok) {
            out.push(d.id);
        }
    }
    out
}

/// The parts the player's words ask for, in a fixed order (rules 0.9.0). `seat` is the speaker; `fallen_only`: every
/// demigod of theirs is down (they can only plead, ask and offer).
pub fn kinds_in(w: &World, b: &Book, seat: u8, text: &str) -> Vec<Kind> {
    let t = text.to_lowercase();
    let mut out = vec![];
    let named = named_demigods(w, &t);
    let own = |d: &DemigodId| w.demigods[d.ix()].seat == seat;
    let names_rival = named.iter().any(|d| !own(d));
    let names_own = named.iter().any(own);
    let speaks_of_own = names_own || any(&t, OWN_WORDS);
    let curse_words = ["curse", "seal", "hex", "wither", "withering", "silence", "binding", "jinx"];
    // Ask: about a quest, or how to break a curse.
    let ask = (has_phrase(&t, "quest") || has_phrase(&t, "quests"))
        && (any(&t, &["what", "how", "need", "needs", "status", "progress", "show", "tell", "steps", "which", "where"]) || t.contains('?'))
        || any(&t, &["break", "lift", "counter", "dispel", "undo", "remove", "cleanse"]) && any(&t, &curse_words)
        || any(&t, &["how do i break", "how do i lift", "what does it need", "what do i need"]);
    if ask {
        out.push(Kind::Ask);
    }
    if any(&t, &["offer", "offers", "pay", "pays", "tithe", "feed", "give", "gives", "spend"]) && any(&t, &["spark", "sparks"]) {
        out.push(Kind::Offer);
    }
    let wanted = quests::wanted(b, seat);
    if wanted.iter().any(|(_, word)| t.contains(word.as_str()))
        && any(&t, &["gather", "find", "catch", "collect", "fetch", "harvest", "wash", "bottle", "pluck", "make", "get", "forge", "craft",
                     "conjure", "seek", "hunt", "cut", "pull", "light", "steal", "take"])
    {
        out.push(Kind::Gather);
    }
    // Raise: a plea for the fallen, when someone has fallen.
    let any_fallen = w.demigods.iter().any(|d| !d.alive);
    let raise_named = has_phrase(&t, "raise") && named.iter().any(|d| !w.demigods[d.ix()].alive);
    if any_fallen && !ask && (any(&t, RAISE_STRONG) || raise_named) && !any(&t, NOT_RAISE) {
        out.push(Kind::Raise);
    }
    // Curse (or seal): strong words, or a weak one aimed at a named demigod.
    if !ask {
        if let Some(k) = CurseKind::from_text(&t) {
            let strong = any(&t, &["curse", "curses", "cursed", "hex", "hexes", "jinx", "no resurrection", "no resurrect", "no ressurect",
                                   "wither", "withers", "withering", "malediction", "evil eye", "stay dead", "stays dead", "never rise"]);
            let rivals = w.demigods.iter().any(|d| d.seat != seat);
            let _ = k;
            if rivals && (strong || names_rival) {
                out.push(Kind::Curse);
            }
        }
    }
    // Heal.
    let heal = any(&t, &["heal", "heals", "healed", "healing", "patch up", "regenerate", "rejuvenate"])
        || any(&t, &["restore", "restores", "restored", "mend", "mends", "mended", "cure", "cures", "cured", "soothe"])
            && (speaks_of_own || names_rival || any(&t, &["health", "wounds", "wounded"]));
    if heal && !ask {
        out.push(Kind::Heal);
    }
    // Arm: arming words, or "gets/give/with <gear>", about your own; never a making verb (that is the item engine's).
    let gear = gear_noun(&t).is_some();
    let arm_word = any(&t, &["arm", "arms", "armed", "arming", "equip", "equips", "equipped", "equipping", "outfit", "outfits", "outfitted",
                             "gear up", "suit up", "rearm"]);
    let give_gear = gear && any(&t, &["get", "gets", "give", "gives", "given", "grant", "grants", "hand", "wield", "wields", "carry", "carries", "bear", "bears", "armed with", "with"]);
    let crowd_arming = any(&t, &["crowd", "locals", "people", "workers", "army", "armies", "soldiers", "troops", "militia", "mob", "villagers", "citizens"]);
    // State schema 11 (the recorded live case: "my demigods need crazy powerful equipment! do that!" armed nobody):
    // kit words are an arming too, and so is needing or wanting a piece of gear.
    let kit = w.look_rules() && any(&t, &["equipment", "gear", "armament", "armaments", "weaponry", "arsenal"]);
    let want_gear = w.look_rules() && gear && any(&t, &["need", "needs", "want", "wants", "deserve", "deserves"]);
    let arm = (arm_word || give_gear || kit || want_gear) && !any(&t, MAKING) && speaks_of_own && !crowd_arming && !names_rival;
    if arm && !ask {
        out.push(Kind::Arm);
    }
    // Bless: on its own; with an arming, the blessing is the gear's.
    if any(&t, BLESS_WORDS) && !arm && !ask && !any(&t, MAKING) && (speaks_of_own || names_rival) {
        out.push(Kind::Bless);
    }
    out.sort();
    out.dedup();
    out
}

const BIG_WORDS: &[&str] = &[
    "fully", "completely", "entirely", "all the way", "to full", "max", "maximum", "totally", "legendary", "god power", "god powers",
    "best", "epic", "mythic", "mightiest", "greatest",
];
const SMALL_WORDS: &[&str] = &["a little", "slightly", "a bit", "somewhat", "small", "minor", "modest"];

/// How big a will is when no mind said: 8 with big words, 3 with small ones, else 5.
fn guess_magnitude(text: &str) -> u8 {
    let t = text.to_lowercase();
    if any(&t, BIG_WORDS) {
        8
    } else if any(&t, SMALL_WORDS) {
        3
    } else {
        5
    }
}

/// A hand's magnitude for a part, corrected by the words: big words lift a plain one to 8, small ones cap it at 3.
fn worded_magnitude(m: u8, text: &str) -> u8 {
    let t = text.to_lowercase();
    // Only where the hand gave its plain default (the rules hand's 4): a mind that weighed the words keeps its number.
    if m <= 4 && any(&t, BIG_WORDS) {
        8
    } else if any(&t, SMALL_WORDS) {
        m.min(3)
    } else {
        m
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The outcome of a beat's wills
// ---------------------------------------------------------------------------------------------------------------------

/// What became of one will (one hand's words this beat): the private `wills[]` entry.
#[derive(Debug, Clone)]
pub struct WillNote {
    pub seat: u8,
    pub text: String,
    pub by: DemigodId,
    pub parts: Vec<Value>,
    /// Arm parts whose item is known only after the merge: (part index, intent, holder).
    pub items: Vec<(usize, IntentId, DemigodId)>,
    /// The gods that ruled, in order.
    pub gods: Vec<GodId>,
    pub rulings: Vec<RulingId>,
}

/// What the beat's wills came to.
#[derive(Debug, Default)]
pub struct Out {
    /// Intents the wills resolved: the route skips them.
    pub handled: BTreeSet<IntentId>,
    /// Effects for the beat's merge.
    pub props: Vec<Proposal>,
    pub notes: Vec<WillNote>,
    /// Public one-liners (acts that are public).
    pub public: Vec<Value>,
}

impl Out {
    /// The private `wills[]` of `seat` (rules 0.9.0): `{text, by, routed_to, verdict, speech, reason, parts[]}`.
    pub fn wills_json(&self, w: &World, beat: u32, seat: u8) -> Vec<Value> {
        self.notes.iter().filter(|n| n.seat == seat).map(|n| n.json(w, beat)).collect()
    }
}

impl WillNote {
    pub fn json(&self, w: &World, beat: u32) -> Value {
        let mut parts = self.parts.clone();
        for (ix, intent, holder) in &self.items {
            let made = w
                .manifests
                .iter()
                .rev()
                .find(|m| m.beat == beat && m.intent == Some(*intent) && m.demigod == *holder)
                .and_then(|m| m.item)
                .and_then(|i| w.items.get(i.ix()));
            if let (Some(p), Some(it)) = (parts.get_mut(*ix), made) {
                let v = crate::items::item_json(w, it);
                if let Some(does) = v["does"].as_str() {
                    let tier = it.ext.tier.as_str();
                    p["result"] = format!("forged {} ({tier}): {does}", article(&it.name)).into();
                }
                p["item"] = v;
            }
        }
        let outcomes: Vec<&str> = parts.iter().filter_map(|p| p["outcome"].as_str()).collect();
        let verdict = if outcomes.is_empty() {
            "refused"
        } else if outcomes.iter().all(|o| matches!(*o, "success" | "crit" | "raised" | "cast" | "told" | "offered" | "gathered")) {
            "granted"
        } else if outcomes.iter().all(|o| matches!(*o, "refused" | "sealed" | "blocked" | "unpaid")) {
            "refused"
        } else {
            "partial"
        };
        let speech = self.rulings.iter().filter_map(|r| w.rulings.get(r.ix())).map(|r| r.speech.clone()).collect::<Vec<_>>().join(" ");
        let reason = parts.iter().filter_map(|p| p["result"].as_str()).collect::<Vec<_>>().join("; ");
        json!({
            "text": self.text, "by": Ref::Demigod(self.by), "by_name": w.name_of(Ref::Demigod(self.by)),
            "routed_to": self.gods.first().map(|g| w.name_of(Ref::God(*g))),
            "gods": self.gods.iter().map(|g| w.name_of(Ref::God(*g))).collect::<Vec<_>>(),
            "verdict": verdict, "speech": (!speech.is_empty()).then_some(speech), "reason": reason, "parts": parts,
        })
    }
}

fn article(name: &str) -> String {
    let first = name.chars().next().map(|c| c.to_ascii_lowercase()).unwrap_or('x');
    if "aeiou".contains(first) {
        format!("an {name}")
    } else {
        format!("a {name}")
    }
}

const FLOURISH: &[&str] = &[
    "a heron had been washed in platinum dust at dawn",
    "the third crow of the morning landed on the right side of the road",
    "a ferryman overpaid his toll in a year nobody remembers",
    "the tide went out one grain of sand early",
    "somebody finally returned a borrowed ladder",
    "the moths voted, and it was close",
    "a bell rang in a town that has no bell",
    "the dust on an unread ledger settled the right way up",
];
const SHORTFALL: &[&str] = &[
    "the candle was lit from the wrong end",
    "a butterfly somewhere declined to cross a strait",
    "the third crow of the morning landed on the wrong side of the road",
    "a stone was washed in ordinary dust",
    "the ferryman kept the change",
    "the moths voted, and it was not close",
];

fn pick<'a>(w: &World, salt: &str, xs: &[&'a str]) -> &'a str {
    xs[(hash_str(&format!("{}:{salt}", w.meta.seed)) % xs.len() as u64) as usize]
}

/// The shared context of one hand's will while its parts resolve.
struct Ctx<'a> {
    beat: u32,
    pantheon: &'a Pantheon,
    st: &'a Settings,
    /// Who acts and pays (the hand's demigod; a fallen player's first demigod).
    actor: DemigodId,
    seat: u8,
    /// The whole will, lower case.
    text: String,
    /// The carrier intent (targets, the ruling's intent, the die's key).
    intent: IntentId,
    magnitude: u8,
    targets: Vec<Ref>,
    /// A fallen player's plea.
    fallen: bool,
}

fn god_of(c: &Ctx, slug: &str) -> GodId {
    c.pantheon.id_of(slug).or_else(|| c.pantheon.id_of("entropy")).unwrap_or(GodId(0))
}

/// Push a rules-written ruling (never an LLM's): the god's visible verdict on a part of a will.
fn rule(w: &mut World, c: &Ctx, god: GodId, verdict: Verdict, speech: String) -> RulingId {
    let id = RulingId::from_ix(w.rulings.len());
    let r = Ruling {
        id,
        beat: c.beat,
        god,
        intent: c.intent,
        invoker: Ref::Demigod(c.actor),
        verdict,
        price: None,
        speech,
        dispatched: None,
        dispatch_mission: None,
        fallback: true,
        bolts: 0,
        at: Some(w.demigods[c.actor.ix()].pos),
    };
    event("ruling", Some(god.to_string()), json!(r));
    crate::beat::record_mut(w, c.beat).rulings.push(id);
    w.rulings.push(r);
    if let Some(g) = w.gods.get_mut(god.ix()) {
        g.attention = 1.0;
        g.invocations += 1;
    }
    id
}

#[allow(clippy::too_many_arguments)]
fn god_prop(c: &Ctx, god: GodId, ruling: RulingId, effect: Effect, act: String, magnitude: u8, valence: i8, witnesses: Vec<Ref>) -> Proposal {
    Proposal {
        source: Source::God { god, ruling: Some(ruling) },
        intent: Some(c.intent),
        effect,
        act,
        actor: Ref::God(god),
        magnitude,
        valence,
        witnesses,
        cause: None,
    }
}

/// Roll a part's d20: recorded, shown, and returned with its named modifiers.
fn roll(w: &mut World, c: &Ctx, part: &str, target: DemigodId, dc: i32, mut mods: Vec<(String, i32)>, reason: &str) -> (DiceRoll, Value) {
    mods.retain(|m| m.1 != 0);
    let total: i32 = mods.iter().map(|m| m.1).sum();
    let q = format!("will:{}:{part}:{}", c.intent.0, target.0);
    let pos = w.demigods[c.actor.ix()].pos;
    let r = dm::d20(w, c.beat, &q, Ref::Demigod(c.actor), total, dc, reason, pos);
    let v = json!({
        "die": "d20", "roll": r.roll, "modifier": r.modifier, "total": r.total, "target_dc": r.target_dc, "verdict": r.verdict,
        "mods": mods.iter().map(|(n, x)| json!({ "name": n, "value": x })).collect::<Vec<_>>(),
    });
    (r, v)
}

/// The modifiers every will of the actor carries: its wardrobe, its charm, boons and hexes, the Final Wish.
fn base_mods(w: &World, c: &Ctx) -> Vec<(String, i32)> {
    let mut m = vec![];
    let wardrobe = w.dm.story(c.actor).map(|s| s.wardrobe as i32 / 2).unwrap_or(0);
    m.push(("wardrobe".to_string(), wardrobe));
    if w.demigods[c.actor.ix()].alive {
        m.push(("charm".to_string(), arms::kit(w, c.actor, c.st).mods.deed));
    }
    m.extend(standing_mods(w, c.actor, c.beat));
    if crate::beat::is_final_wish(w, c.beat) {
        m.push(("the Final Wish".to_string(), 2));
    }
    m
}

fn favor_mod(w: &World, god: GodId, d: DemigodId) -> (String, i32) {
    let f = w.gods.get(god.ix()).and_then(|g| g.favor.get(d.ix())).copied().unwrap_or(0.0);
    let x = if f >= 2.0 {
        1
    } else if f <= -2.0 {
        -1
    } else {
        0
    };
    (format!("{}'s favor", w.name_of(Ref::God(god))), x)
}

/// What the purse could not pay, financed as a debt to `god` when the actor may still borrow. Returns the debt taken
/// (0 = none: nothing was short, or the gods lend no more).
fn finance(w: &World, c: &Ctx, god: GodId, short: u32, props: &mut Vec<Proposal>, ruling: RulingId) -> f32 {
    if short == 0 || c.fallen || !w.demigods[c.actor.ix()].alive {
        return 0.0;
    }
    let debt = r2(short as f32 * c.st.will_debt_per_spark);
    let pending: f32 = props
        .iter()
        .filter_map(|p| match &p.effect {
            Effect::Debt { owed_by, amount, .. } if *owed_by == Ref::Demigod(c.actor) => Some(*amount),
            _ => None,
        })
        .sum();
    if debt <= 0.0 || open_debt(w, Ref::Demigod(c.actor), None) + pending + debt > c.st.will_debt_cap {
        return 0.0;
    }
    props.push(god_prop(
        c,
        god,
        ruling,
        Effect::Debt { owed_by: Ref::Demigod(c.actor), god, amount: debt, due_in: c.st.debt_due_beats },
        format!("{} lends {} what the purse lacked", w.name_of(Ref::God(god)), w.name_of(Ref::Demigod(c.actor))),
        0,
        -1,
        vec![Ref::Demigod(c.actor)],
    ));
    debt
}

fn entropy_prop(c: &Ctx, god: GodId, ruling: RulingId, delta: f32, props: &mut Vec<Proposal>) {
    if delta > 0.0 {
        props.push(god_prop(c, god, ruling, Effect::Entropy { delta: r2(delta) }, "Reality strains around a demigod's will".into(), 0, 0, vec![]));
    }
}

fn target_json(w: &World, d: DemigodId) -> (Value, String) {
    (json!(Ref::Demigod(d)), w.name_of(Ref::Demigod(d)))
}

fn part_base(w: &World, kind: Kind, target: DemigodId, god: GodId) -> Value {
    let (t, n) = target_json(w, target);
    json!({ "part": kind.as_str(), "target": t, "target_name": n, "god": w.name_of(Ref::God(god)) })
}

fn quest_fields(w: &World, b: &Book, id: u32, p: &mut Value) {
    if let Some(q) = b.quests.iter().find(|q| q.id == id) {
        p["quest"] = quests::quest_json(w, q);
        let steps: Vec<String> = q.steps.iter().enumerate().map(|(i, s)| format!("{}. {}", i + 1, s.text)).collect();
        p["next"] = format!("quest {}: {}", qref(id), steps.join(" ")).into();
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Heal and bless
// ---------------------------------------------------------------------------------------------------------------------

/// The demigods an own-kind part (heal, bless, arm) is for: the named ones, else the speaker's own standing ones.
fn own_targets(w: &World, c: &Ctx) -> Vec<DemigodId> {
    let mut named: Vec<DemigodId> = named_demigods(w, &c.text);
    for t in &c.targets {
        if let Ref::Demigod(d) = t {
            if !named.contains(d) {
                named.push(*d);
            }
        }
    }
    // Rivals count only when the words name them (the 0.8.1 rule: "my demigods" never means the others).
    let by_name = named_demigods(w, &c.text);
    named.retain(|d| w.demigods[d.ix()].seat == c.seat || by_name.contains(d));
    let own_named: Vec<DemigodId> = named.iter().copied().filter(|d| w.demigods[d.ix()].seat == c.seat).collect();
    let all_words = any(&c.text, &["all", "every", "both", "everyone", "demigods", "mydemigods", "guys", "hands", "us", "our", "we"]);
    let mut out = named.clone();
    if own_named.is_empty() && named.is_empty() || all_words {
        for d in w.bodies_of_seat(c.seat) {
            if !out.contains(&d.id) {
                out.push(d.id);
            }
        }
    }
    out.sort();
    out
}

fn heal_part(w: &mut World, b: &mut Book, c: &Ctx, kind: Kind, note: &mut WillNote, out: &mut Out) -> bool {
    let god = god_of(c, "nature");
    let targets: Vec<DemigodId> = own_targets(w, c).into_iter().filter(|d| w.demigods[d.ix()].alive).collect();
    if targets.is_empty() {
        return false;
    }
    let ruling = rule(w, c, god, Verdict::Grant, String::new());
    let factor = price_factor(w, b, c.seat, c.beat);
    let mut said = vec![];
    let mut worst = Verdict::Grant;
    for t in targets {
        let mut p = part_base(w, kind, t, god);
        let own = w.demigods[t.ix()].seat == c.seat;
        let name = w.demigods[t.ix()].name.clone();
        let (health, max) = (w.demigods[t.ix()].health, w.demigods[t.ix()].max_health);
        if kind == Kind::Heal {
            if let Some(cu) = curses::active(b, t, CurseKind::Wither, c.beat) {
                p["outcome"] = "blocked".into();
                p["result"] = format!("{name} can't be healed: a curse of withering lies on it").into();
                p["why"] = "withered: no healing reaches it until the curse is countered".into();
                let q = cu.quest;
                quest_fields(w, b, q, &mut p);
                note.parts.push(p);
                worst = Verdict::Bargain;
                continue;
            }
            if health >= max {
                p["outcome"] = "success".into();
                p["result"] = format!("{name} is already whole ({}/{})", r1(health), r1(max)).into();
                p["paid"] = paid_json(&Paid::default(), 0.0);
                note.parts.push(p);
                continue;
            }
        }
        let price = (c.st.heal_price_sparks + c.magnitude as u32 / 4) * factor;
        let paid = pay(w, c.actor, price, false, c.st);
        let debt = finance(w, c, god, paid.short, &mut out.props, ruling);
        let covered = paid.sparks + if debt > 0.0 { paid.short } else { 0 };
        let pf = if price == 0 { 1.0 } else { covered as f32 / price as f32 };
        p["paid"] = paid_json(&paid, debt);
        entropy_prop(c, god, ruling, paid.sparks as f32 * c.st.will_entropy_per_spark, &mut out.props);
        let full_heal = r1((1.0 + c.magnitude as f32 / 4.0).clamp(1.0, 3.5) * c.st.heal_scale * (0.5 + 0.5 * pf));
        let goal_of = |amount: f32| match kind {
            Kind::Bless => Goal::Boon { bonus: c.st.boon_bonus as i32, beats: c.st.boon_beats },
            _ => Goal::Heal { amount },
        };
        let expires = (c.st.quest_expire_beats > 0).then(|| c.beat + c.st.quest_expire_beats);
        if price > 0 && covered == 0 {
            // Nothing to pay with and nothing more to borrow: a quest, never silence.
            let q = quests::give(w, b, c.beat, quests::Ask { seat: w.demigods[t.ix()].seat, demigod: t, goal: goal_of(full_heal), god, difficulty: 1 + (c.magnitude >= 7) as u8, curser: None, public: false, expires_beat: expires }, c.st);
            p["outcome"] = "quest".into();
            p["result"] = format!("nothing yet for {name}: the price is {price} sparks, the purse is empty and the gods lend no more").into();
            p["why"] = format!("price {price} sparks unpaid; open debt is at the cap ({})", c.st.will_debt_cap).into();
            quest_fields(w, b, q, &mut p);
            note.parts.push(p);
            said.push(format!("{name} must earn it"));
            worst = Verdict::Bargain;
            continue;
        }
        let dc = if own { c.st.will_own_dc as i32 + c.magnitude as i32 / 4 } else { 8 + c.magnitude as i32 / 2 };
        let mut mods = base_mods(w, c);
        mods.push(favor_mod(w, god, c.actor));
        mods.push(match (paid.short, debt > 0.0) {
            (0, _) => ("paid in full".to_string(), 1),
            (_, true) => ("on credit".to_string(), -1),
            _ => ("part paid".to_string(), -1),
        });
        let (r, rv) = roll(w, c, kind.as_str(), t, dc, mods, if kind == Kind::Bless { "a blessing" } else { "a mending" });
        p["roll"] = rv;
        let me = Ref::Demigod(t);
        let boon = |b: &mut Book, bonus: i32, beats: u32, from: &str| {
            b.boons.retain(|x| x.demigod != t);
            b.boons.push(Boon { demigod: t, bonus, until_beat: c.beat + beats, from: from.into() });
        };
        match (kind, r.verdict) {
            (Kind::Bless, v) if v.ok() => {
                let crit = v == DiceVerdict::CritSuccess;
                let (bonus, beats) = (c.st.boon_bonus as i32 + crit as i32, if crit { c.st.boon_beats * 3 / 2 } else { c.st.boon_beats });
                boon(b, bonus, beats, "a blessing");
                p["outcome"] = if crit { "crit" } else { "success" }.into();
                p["result"] = format!("blessed {name}: {bonus:+} on its d20s for {beats} beats").into();
                p["boon"] = json!({ "bonus": bonus, "until_beat": c.beat + beats });
                out.props.push(god_prop(c, god, ruling, Effect::Note { text: format!("{} blesses {name}", w.name_of(Ref::God(god))) }, format!("{} blesses {name}", w.name_of(Ref::God(god))), 0, 1, vec![me]));
                said.push(format!("{name} is blessed"));
            }
            (Kind::Bless, v) => {
                let fizzle = v == DiceVerdict::CritFail;
                if !fizzle {
                    boon(b, c.st.boon_bonus as i32, 2, "a faltering blessing");
                }
                let q = quests::give(w, b, c.beat, quests::Ask { seat: w.demigods[t.ix()].seat, demigod: t, goal: goal_of(0.0), god, difficulty: 1 + (c.magnitude >= 7) as u8, curser: None, public: false, expires_beat: expires }, c.st);
                p["outcome"] = if fizzle { "fizzle" } else { "partial" }.into();
                p["result"] = if fizzle {
                    format!("the blessing on {name} fizzled")
                } else {
                    format!("a faltering blessing on {name}: {:+} on its d20s for 2 beats", c.st.boon_bonus)
                }
                .into();
                p["why"] = format!("rolled {} against DC {}: {}", r.total, r.target_dc, pick(w, &format!("s{}", c.intent.0), SHORTFALL)).into();
                quest_fields(w, b, q, &mut p);
                said.push(format!("{name}'s blessing waits on a quest"));
                worst = Verdict::Bargain;
            }
            (_, v) if v.ok() => {
                let crit = v == DiceVerdict::CritSuccess;
                let amount = if crit { r1(full_heal * 1.5) } else { full_heal };
                let healed = r1(amount.min(max - health));
                out.props.push(god_prop(c, god, ruling, Effect::Heal { who: me, amount }, format!("{} mends {name}", w.name_of(Ref::God(god))), c.magnitude, 2, vec![me, Ref::Demigod(c.actor)]));
                p["outcome"] = if crit { "crit" } else { "success" }.into();
                p["healed"] = json!(healed);
                p["result"] = if crit {
                    boon(b, c.st.boon_bonus as i32, c.st.boon_beats, "a critical mending");
                    p["boon"] = json!({ "bonus": c.st.boon_bonus, "until_beat": c.beat + c.st.boon_beats });
                    format!("healed {name} {healed} (a natural 20), and a boon: {:+} on its d20s for {} beats", c.st.boon_bonus, c.st.boon_beats)
                } else {
                    format!("healed {name} {healed}")
                }
                .into();
                said.push(format!("{name} mends"));
            }
            (_, v) => {
                let fizzle = v == DiceVerdict::CritFail;
                let token = if fizzle { 0.0 } else { c.st.heal_token.min(full_heal) };
                let rest = r1((full_heal - token).max(0.5));
                if token > 0.0 {
                    out.props.push(god_prop(c, god, ruling, Effect::Heal { who: me, amount: token }, format!("{} mends {name} a little", w.name_of(Ref::God(god))), 2, 1, vec![me, Ref::Demigod(c.actor)]));
                }
                let q = quests::give(w, b, c.beat, quests::Ask { seat: w.demigods[t.ix()].seat, demigod: t, goal: goal_of(rest), god, difficulty: 1 + (c.magnitude >= 7) as u8, curser: None, public: false, expires_beat: expires }, c.st);
                let healed = r1(token.min(max - health));
                p["outcome"] = if fizzle { "fizzle" } else { "partial" }.into();
                p["healed"] = json!(healed);
                p["result"] = if fizzle {
                    format!("the mending of {name} fizzled (a natural 1); a quest will complete it (+{rest})")
                } else {
                    format!("healed {name} {healed} of {full_heal}; a quest will complete it (+{rest})")
                }
                .into();
                p["why"] = if fizzle {
                    format!("rolled a natural 1: {}", pick(w, &format!("s{}:{}", c.intent.0, t.0), SHORTFALL))
                } else {
                    format!("rolled {} against DC {}: {}", r.total, r.target_dc, pick(w, &format!("s{}:{}", c.intent.0, t.0), SHORTFALL))
                }
                .into();
                quest_fields(w, b, q, &mut p);
                said.push(format!("{name}'s mending waits on a quest"));
                worst = Verdict::Bargain;
            }
        }
        note.parts.push(p);
    }
    let g = w.name_of(Ref::God(god));
    let why = pick(w, &format!("f{}:{}", c.intent.0, c.beat), FLOURISH);
    let speech = if said.is_empty() {
        format!("{g}: \"Nothing here needs me.\"")
    } else if worst == Verdict::Grant {
        format!("{g}: \"Granted: {}, because {why}.\"", said.join(", "))
    } else {
        format!("{g}: \"{}. I never say no to my own; I say not yet, and I say how.\"", said.join("; "))
    };
    if let Some(r) = w.rulings.get_mut(ruling.ix()) {
        r.verdict = worst;
        r.speech = speech;
    }
    if !note.gods.contains(&god) {
        note.gods.push(god);
    }
    note.rulings.push(ruling);
    true
}

// ---------------------------------------------------------------------------------------------------------------------
// Arm
// ---------------------------------------------------------------------------------------------------------------------

/// The proposal that makes a piece of gear for `holder` (an `Effect::Manifest`: the `mf:` row and the item).
#[allow(clippy::too_many_arguments)]
fn forge_prop(
    w: &World,
    beat: u32,
    by: DemigodId,
    holder: DemigodId,
    intent: Option<IntentId>,
    kind: ItemKind,
    asked: Tier,
    made: Option<Tier>,
    name: &str,
    god: GodId,
    blessed: bool,
    die: Option<&DiceRoll>,
    mut reasons: Vec<String>,
    span: &str,
    magnitude: u8,
) -> Proposal {
    let h = &w.demigods[holder.ix()];
    if blessed {
        reasons.push(format!("{}: {}", arms::BLESSED, w.name_of(Ref::God(god))));
    }
    let outcome = match made {
        Some(t) if t < asked => Outcome::Partial,
        Some(_) => Outcome::Success,
        None => Outcome::Failed,
    };
    let m = Manifestation {
        id: ManifestationId::from_ix(w.manifests.len()),
        beat,
        by: Ref::Demigod(by),
        demigod: holder,
        hand: h.side,
        agent: Some(w.demigods[by.ix()].hands[0]),
        intent,
        request: ManifestRequest { kind, name: name.into(), count: 1.0, magnitude, span: span.chars().take(240).collect(), offer_bolts: 0, accept_debt: true, god: blessed.then_some(god) },
        place: Some(h.place),
        at: Some(h.pos),
        verdict: ManifestVerdict {
            outcome,
            tier: asked,
            odds: Odds::default(),
            roll: die.map(|d| (d.roll as f64 - 1.0) / 20.0).unwrap_or(0.0),
            cost: Cost::default(),
            god,
            favor: w.gods.get(god.ix()).and_then(|g| g.favor.get(by.ix())).copied().unwrap_or(0.0),
            stance: None,
            entropy: w.meta.entropy,
            reasons,
            flavor: Flavor::Rules,
        },
        item: None,
        rows: vec![],
        public: made.is_some_and(|t| t >= fn7_model::manifest::PUBLIC_TIER),
    };
    let item = made.map(|tier| {
        let material = arms::material_of(w.meta.seed, name).0;
        NewItem {
            name: name.into(),
            description: format!("Willed into being for {} at {} on beat {beat}.", h.name, w.name_of(Ref::Place(h.place))),
            ext: ItemExt {
                kind,
                tier,
                count: 1.0,
                unit: String::new(),
                state: ItemState::Held,
                manifestation: None,
                banner: None,
                image_prompt: Some(format!("{name}: {} gear of {material}, carried by a demigod", tier.as_str())),
                at: Some(h.pos),
                place: Some(h.place),
                muster: None,
                history: vec![],
            },
            muster: 0,
        }
    });
    let act = match (&item, outcome) {
        (Some(it), Outcome::Partial) => format!("{} is armed with {}, lesser than willed", h.name, article(&it.name)),
        (Some(it), _) => format!("{} is armed with {}", h.name, article(&it.name)),
        (None, _) => format!("{}'s arms refuse to appear", h.name),
    };
    Proposal {
        source: Source::Agent { agent: w.demigods[by.ix()].hands[0] },
        intent,
        effect: Effect::Manifest { manifestation: Box::new(m), item },
        act,
        actor: Ref::Demigod(holder),
        magnitude: (4 + made.unwrap_or(asked).index()).min(10) as u8,
        valence: 1,
        witnesses: vec![Ref::Demigod(holder), Ref::Demigod(by)],
        cause: None,
    }
}

/// Forge gear at once (a quest's reward): returns the item.
#[allow(clippy::too_many_arguments)]
pub fn forge_now(w: &mut World, beat: u32, holder: DemigodId, kind: ItemKind, tier: Tier, name: &str, god: Option<GodId>, why: &str, pantheon: &Pantheon, st: &Settings) -> Option<ItemId> {
    let g = god.unwrap_or_else(|| kind.patron());
    let g = if g.ix() < w.gods.len() { g } else { GodId(0) };
    let p = forge_prop(w, beat, holder, holder, None, kind, tier, Some(tier), name, g, god.is_some(), None, vec![why.to_string()], why, 5);
    let before = w.items.len();
    crate::merge::run(w, vec![p], pantheon, beat, st.max_witnesses as usize);
    (w.items.len() > before).then(|| ItemId::from_ix(before))
}

/// Rules 0.9.0: a god-tier power aimed at the caster's own standing demigod arms it: wonder-tier gear (a relic, a
/// tier up for the blessing) in the slot it lacks, carrying that god's signature. `None` when it already holds gear
/// that god blessed.
pub fn god_arms(w: &World, ruling: &Ruling, intent: &Intent, holder: DemigodId) -> Option<Proposal> {
    let me = Ref::Demigod(holder);
    let held: Vec<&Item> = w.items.iter().filter(|i| i.holders.last() == Some(&me) && i.ext.state == ItemState::Held).collect();
    if held.iter().any(|i| arms::blessed_by(w, i) == Some(ruling.god)) {
        return None;
    }
    let has = |slot: &str| held.iter().any(|i| arms::slot_of(i.ext.kind) == Some(slot));
    let (kind, noun) = if !has("weapon") {
        (ItemKind::Weapon, "Blade")
    } else if !has("armor") {
        (ItemKind::Armor, "Plate")
    } else {
        (ItemKind::Relic, "Talisman")
    };
    let mut rng = Rng::derive(w.meta.seed, &[WILL_SALT, intent.id.0 as u64, holder.0 as u64, 0x60D]);
    let m = arms::MATERIALS[rng.below(arms::MATERIALS.len() as u64) as usize].0;
    let name = format!("{} {noun} of {}", title(&m.replace('-', " ")), w.name_of(Ref::God(ruling.god)));
    Some(forge_prop(w, intent.beat, intent.demigod, holder, Some(intent.id), kind, Tier::Relic, Some(Tier::Wonder), &name, ruling.god, true, None,
                    vec!["willed: a god-tier power".to_string()], &intent.span, 8))
}

/// The gear a will asks for, for `holder`: its kind and name (the noun the words give, else the slot it lacks).
fn gear_for(w: &World, c: &Ctx, holder: DemigodId, n: usize) -> (ItemKind, String) {
    let me = Ref::Demigod(holder);
    let has = |slot: &str| {
        w.items.iter().any(|i| i.holders.last() == Some(&me) && i.ext.state == ItemState::Held && arms::slot_of(i.ext.kind) == Some(slot))
    };
    let (slot, noun) = match gear_noun(&c.text) {
        Some((s, n)) => (s, n),
        None if !has("weapon") => ("weapon", "Blade".to_string()),
        None if !has("armor") => ("armor", "Plate".to_string()),
        None => ("charm", "Talisman".to_string()),
    };
    let kind = match slot {
        "armor" => ItemKind::Armor,
        "charm" => ItemKind::Relic,
        _ => ItemKind::Weapon,
    };
    let already = arms::MATERIALS.iter().any(|m| noun.to_lowercase().contains(m.0));
    let name = if already {
        noun
    } else {
        let mut rng = Rng::derive(w.meta.seed, &[WILL_SALT, c.intent.0 as u64, holder.0 as u64, n as u64]);
        let m = arms::MATERIALS[rng.below(arms::MATERIALS.len() as u64) as usize].0;
        format!("{} {noun}", title(&m.replace('-', " ")))
    };
    (kind, name)
}

fn arm_part(w: &mut World, b: &mut Book, c: &Ctx, note: &mut WillNote, out: &mut Out) -> bool {
    let targets: Vec<DemigodId> = own_targets(w, c).into_iter().filter(|d| w.demigods[d.ix()].alive && w.demigods[d.ix()].seat == c.seat).collect();
    if targets.is_empty() {
        return false;
    }
    // A god's blessing: a god named, or blessing words.
    let named_god = c.pantheon.gods.iter().position(|g| has_phrase(&c.text, &g.slug) || has_phrase(&c.text, &g.name.to_lowercase())).map(GodId::from_ix);
    let wants_bless = named_god.is_some() || any(&c.text, BLESS_WORDS);
    let asked = match c.magnitude {
        0..=3 => Tier::Trinket,
        4..=6 => Tier::Artifact,
        _ => Tier::Relic,
    };
    let factor = price_factor(w, b, c.seat, c.beat);
    let tier_price = |t: Tier| match t.index() {
        0 => 0,
        1 => c.st.arm_price_artifact * factor,
        _ => c.st.arm_price_relic * factor,
    };
    let mut said = vec![];
    let mut worst = Verdict::Grant;
    let mut ruling: Option<(GodId, RulingId)> = None;
    for (n, t) in targets.into_iter().enumerate() {
        let (kind, name) = gear_for(w, c, t, n);
        let god = named_god.unwrap_or_else(|| kind.patron());
        let god = if god.ix() < w.gods.len() { god } else { god_of(c, "entropy") };
        let rid = match ruling {
            Some((g, r)) if g == god => r,
            _ => {
                let r = rule(w, c, god, Verdict::Grant, String::new());
                ruling = Some((god, r));
                if !note.gods.contains(&god) {
                    note.gods.push(god);
                }
                note.rulings.push(r);
                r
            }
        };
        let mut p = part_base(w, Kind::Arm, t, god);
        let holder = w.demigods[t.ix()].name.clone();
        // Pay: the tier's price, then the blessing's; what can't be paid is lent, or the tier comes down.
        let mut tier = asked;
        let mut blessed = wants_bless;
        let want = tier_price(tier) + if blessed { c.st.arm_bless_sparks * factor } else { 0 };
        let paid = pay(w, c.actor, want, false, c.st);
        let debt = finance(w, c, god, paid.short, &mut out.props, rid);
        let mut why = vec![];
        if paid.short > 0 && debt == 0.0 {
            // What was paid buys the best tier it covers; the blessing goes first.
            let mut have = paid.sparks;
            if blessed && have < tier_price(tier) + c.st.arm_bless_sparks * factor {
                blessed = false;
                why.push(format!("no blessing: it costs {} sparks more", c.st.arm_bless_sparks * factor));
            } else if blessed {
                have -= c.st.arm_bless_sparks * factor;
            }
            while tier.index() > 0 && tier_price(tier) > have {
                tier = tier.down(1);
            }
            if tier < asked {
                why.push(format!("{} tier costs {} sparks; {} paid: {} tier", asked.as_str(), tier_price(asked), paid.sparks, tier.as_str()));
            }
        }
        p["paid"] = paid_json(&paid, debt);
        entropy_prop(c, god, rid, paid.sparks as f32 * c.st.will_entropy_per_spark, &mut out.props);
        let dc = c.st.will_own_dc as i32 + 2 * tier.index() as i32;
        let mut mods = base_mods(w, c);
        mods.push(favor_mod(w, god, c.actor));
        if debt > 0.0 {
            mods.push(("on credit".to_string(), -1));
        }
        let (r, rv) = roll(w, c, "arm", t, dc, mods, "an arming");
        p["roll"] = rv;
        p["asked_tier"] = json!(asked);
        let bump = |t: Tier| if blessed { t.up(1) } else { t };
        let expires = (c.st.quest_expire_beats > 0).then(|| c.beat + c.st.quest_expire_beats);
        let slot = arms::slot_of(kind).unwrap_or("weapon").to_string();
        let mut reasons = vec![format!("willed: {} tier, {} sparks paid", asked.as_str(), paid.sparks)];
        reasons.extend(why.iter().cloned());
        match r.verdict {
            DiceVerdict::CritSuccess | DiceVerdict::Success => {
                let crit = r.verdict == DiceVerdict::CritSuccess;
                let made = bump(if crit { tier.up(1).min(Tier::Wonder) } else { tier });
                out.props.push(forge_prop(w, c.beat, c.actor, t, Some(c.intent), kind, asked, Some(made), &name, god, blessed, Some(&r), reasons, &c.text, c.magnitude));
                note.items.push((note.parts.len(), c.intent, t));
                p["outcome"] = if crit { "crit" } else if made < asked { "partial" } else { "success" }.into();
                p["result"] = format!("forged {} for {holder} ({} tier)", article(&name), made.as_str()).into();
                if !why.is_empty() {
                    p["why"] = why.join("; ").into();
                    p["next"] = format!("{} tier costs {} sparks (a bolt breaks into {})", asked.as_str(), tier_price(asked), c.st.will_spark_per_bolt).into();
                    worst = Verdict::Bargain;
                }
                said.push(format!("{holder} is armed"));
            }
            DiceVerdict::Fail => {
                // A partial: a tier low, and a quest that tempers it.
                let made = bump(tier.down(1));
                let goal_tier = bump(tier);
                out.props.push(forge_prop(w, c.beat, c.actor, t, Some(c.intent), kind, asked, Some(made), &name, god, blessed, Some(&r), reasons, &c.text, c.magnitude));
                note.items.push((note.parts.len(), c.intent, t));
                p["outcome"] = "partial".into();
                p["result"] = format!("forged {} for {holder}, but a tier low ({})", article(&name), made.as_str()).into();
                let mut ws = why.clone();
                ws.push(format!("rolled {} against DC {}: {}", r.total, r.target_dc, pick(w, &format!("a{}:{}", c.intent.0, t.0), SHORTFALL)));
                p["why"] = ws.join("; ").into();
                if goal_tier > made {
                    let q = quests::give(w, b, c.beat, quests::Ask { seat: c.seat, demigod: t, goal: Goal::Temper { item: None, intent: c.intent.0, tier: goal_tier }, god, difficulty: (goal_tier.index() as u8).clamp(1, 3), curser: None, public: false, expires_beat: expires }, c.st);
                    quest_fields(w, b, q, &mut p);
                }
                if p["next"].is_null() {
                    p["next"] = format!("{} tier costs {} sparks (a bolt breaks into {}); will it again with the purse for it", asked.as_str(), tier_price(asked), c.st.will_spark_per_bolt).into();
                }
                said.push(format!("{holder}'s arms want tempering"));
                worst = Verdict::Bargain;
            }
            DiceVerdict::CritFail => {
                out.props.push(forge_prop(w, c.beat, c.actor, t, Some(c.intent), kind, asked, None, &name, god, blessed, Some(&r), reasons, &c.text, c.magnitude));
                let q = quests::give(w, b, c.beat, quests::Ask { seat: c.seat, demigod: t, goal: Goal::Arm { slot, tier: bump(tier), name: name.clone(), god: blessed.then_some(god) }, god, difficulty: (bump(tier).index() as u8).clamp(1, 3), curser: None, public: false, expires_beat: expires }, c.st);
                p["outcome"] = "fizzle".into();
                p["result"] = format!("the {name} refused to appear for {holder} (a natural 1); a quest will forge it").into();
                p["why"] = format!("rolled a natural 1: {}", pick(w, &format!("a{}:{}", c.intent.0, t.0), SHORTFALL)).into();
                quest_fields(w, b, q, &mut p);
                said.push(format!("{holder}'s arms wait on a quest"));
                worst = Verdict::Bargain;
            }
        }
        p["blessed"] = blessed.into();
        note.parts.push(p);
    }
    if let Some((god, rid)) = ruling {
        let g = w.name_of(Ref::God(god));
        let speech = if worst == Verdict::Grant {
            format!("{g}: \"Granted: {}, because {}.\"", said.join(", "), pick(w, &format!("f{}:{}a", c.intent.0, c.beat), FLOURISH))
        } else {
            format!("{g}: \"{}. What I hand over is real; what is missing has a price or a road.\"", said.join("; "))
        };
        if let Some(r) = w.rulings.get_mut(rid.ix()) {
            r.verdict = worst;
            r.speech = speech;
        }
    }
    true
}

// ---------------------------------------------------------------------------------------------------------------------
// Raise
// ---------------------------------------------------------------------------------------------------------------------

/// Death's price to raise `target` now: `(sparks, years of max health)`.
pub fn raise_price(w: &World, b: &Book, target: DemigodId, beat: u32, st: &Settings) -> (u32, f32) {
    let n = b.raises.iter().filter(|r| r.demigod == target).count() as u32;
    let dead = beat.saturating_sub(w.demigods[target.ix()].eliminated_beat.unwrap_or(beat));
    (st.raise_price_sparks * (1 + n) + dead / st.raise_stale_beats.max(1), st.raise_years * (1 + n) as f32)
}

/// Raise `target` at once (Death's ruling was paid for, or its quest is done): it returns at
/// `raise_health_fraction` of its max health (less `years`), where it fell or at its shrine. `demigod.raised`.
#[allow(clippy::too_many_arguments)]
pub fn raise_now(w: &mut World, beat: u32, target: DemigodId, by: Option<DemigodId>, by_seat: u8, years: f32, how: &str, pantheon: &Pantheon, st: &Settings) -> bool {
    if w.demigods[target.ix()].alive {
        return false;
    }
    let death = pantheon.id_of("death").unwrap_or(GodId(0));
    let me = Ref::Demigod(target);
    let name = w.demigods[target.ix()].name.clone();
    let shrine = w.dm.shrines.iter().find(|s| s.demigod == target).map(|s| (s.at, s.name.clone()));
    let mut props = vec![Proposal {
        source: Source::God { god: death, ruling: None },
        intent: None,
        effect: Effect::Revive { who: me },
        act: format!("Death lets {name} get up"),
        actor: Ref::God(death),
        magnitude: 8,
        valence: 3,
        witnesses: vec![me],
        cause: None,
    }];
    if let Some((at, _)) = &shrine {
        props.push(Proposal {
            source: Source::God { god: death, ruling: None },
            intent: None,
            effect: Effect::Move { who: me, to: *at, place: Some(fn7_society::geo::place_at(w, *at)) },
            act: format!("{name} wakes on the steps of its shrine"),
            actor: Ref::God(death),
            magnitude: 3,
            valence: 1,
            witnesses: vec![me],
            cause: None,
        });
    }
    crate::merge::run(w, props, pantheon, beat, st.max_witnesses as usize);
    if !w.demigods[target.ix()].alive {
        return false;
    }
    {
        let x = &mut w.demigods[target.ix()];
        x.max_health = (x.max_health - years).max(1.0);
        x.health = (x.max_health * st.raise_health_fraction).clamp(0.5, x.max_health);
    }
    // Raised by a quest, nobody paid Death's sparks: the one who rose owes them as a debt.
    if how.starts_with("quest") {
        let owed = r2(st.raise_price_sparks as f32 * st.will_debt_per_spark);
        if owed > 0.0 {
            let p = Proposal {
                source: Source::God { god: death, ruling: None },
                intent: None,
                effect: Effect::Debt { owed_by: me, god: death, amount: owed, due_in: st.debt_due_beats },
                act: format!("Death writes {name}'s return into its ledger"),
                actor: Ref::God(death),
                magnitude: 2,
                valence: -1,
                witnesses: vec![me],
                cause: None,
            };
            crate::merge::run(w, vec![p], pantheon, beat, st.max_witnesses as usize);
        }
    }
    let mut b = book(w);
    b.raises.push(Raised { demigod: target, beat, by_seat, how: how.into() });
    // A raising quest for it is done with.
    save(w, &b);
    let pos = w.demigods[target.ix()].pos;
    let wh = dm::locate(w, pos, HexLevel::People);
    let by_name = by.map(|d| w.demigods[d.ix()].player_name().to_string());
    let text = match (&by_name, &shrine) {
        (Some(p), Some((_, s))) => format!("{name} rises at {s}, raised by {p}'s plea to Death ({how})"),
        (Some(p), None) => format!("{name} rises where it fell, raised by {p}'s plea to Death ({how})"),
        (None, Some((_, s))) => format!("{name} rises at {s} ({how})"),
        (None, None) => format!("{name} rises where it fell ({how})"),
    };
    event("demigod_raised", Some(me.to_string()), json!({ "beat": beat, "demigod": target, "by_seat": by_seat, "years": years, "how": how }));
    dm::remember(w, beat, &wh, Decision {
        scope: Scope::Cell,
        kind: "demigod.raised",
        query: None,
        public: true,
        text,
        data: json!({ "demigod": me, "name": name, "by": by.map(Ref::Demigod), "by_player": by_name, "by_seat": by_seat, "how": how,
                      "years": years, "at": pos, "cell": wh.cell, "at_shrine": shrine.is_some(), "beat": beat }),
    });
    true
}

fn raise_part(w: &mut World, b: &mut Book, c: &Ctx, note: &mut WillNote, out: &mut Out) -> bool {
    let death = god_of(c, "death");
    // Whom: the fallen the words name; "the dead / the others / everyone": every fallen demigod; else your own.
    let named: Vec<DemigodId> = named_demigods(w, &c.text)
        .into_iter()
        .chain(c.targets.iter().filter_map(|t| match t {
            Ref::Demigod(d) => Some(*d),
            _ => None,
        }))
        .filter(|d| !w.demigods[d.ix()].alive)
        .collect();
    let everyone = any(&c.text, &["the dead", "the fallen", "everyone", "everybody", "other", "others", "all the", "all demigods", "all of them"]);
    let mut targets: Vec<DemigodId> = if !named.is_empty() {
        named
    } else if everyone {
        w.demigods.iter().filter(|d| !d.alive).map(|d| d.id).collect()
    } else {
        w.bodies_of_seat(c.seat).filter(|d| !d.alive).map(|d| d.id).collect()
    };
    targets.sort();
    targets.dedup();
    // Your own first, then the longest dead; two a will.
    targets.sort_by_key(|d| (w.demigods[d.ix()].seat != c.seat, w.demigods[d.ix()].eliminated_beat.unwrap_or(0), d.0));
    targets.truncate(2);
    if targets.is_empty() {
        return false;
    }
    let ruling = rule(w, c, death, Verdict::Grant, String::new());
    let factor = price_factor(w, b, c.seat, c.beat);
    let mut said = vec![];
    let mut verdicts = vec![];
    for t in targets {
        let mut p = part_base(w, Kind::Raise, t, death);
        let name = w.demigods[t.ix()].name.clone();
        let (sparks, years) = raise_price(w, b, t, c.beat, c.st);
        let price = sparks * factor;
        p["price"] = json!({ "sparks": price, "years": years, "fallen_beat": w.demigods[t.ix()].eliminated_beat,
                             "raised_before": b.raises.iter().filter(|r| r.demigod == t).count() });
        if let Some(cu) = curses::active(b, t, CurseKind::Seal, c.beat).cloned() {
            let q = cu.quest;
            // Whoever pleads may take up the seal's counter-quest.
            if let Some(quest) = b.quests.iter_mut().find(|x| x.id == q) {
                if quest.seat != c.seat && !quest.helpers.contains(&c.seat) {
                    quest.helpers.push(c.seat);
                }
            }
            p["outcome"] = "sealed".into();
            p["result"] = format!("{name} can't be raised: a seal of no resurrection lies on it").into();
            p["why"] = "sealed: Death may not open this grave until the seal's counter-quest is done".into();
            p["curse"] = curses::public_json(w, &cu);
            quest_fields(w, b, q, &mut p);
            said.push(format!("{name} is sealed"));
            verdicts.push(Verdict::Refuse);
            note.parts.push(p);
            continue;
        }
        let granted = b.raises.iter().filter(|r| r.by_seat == c.seat).count() as u32;
        if granted >= c.st.raise_cap {
            p["outcome"] = "refused".into();
            p["result"] = format!("Death will raise no more for you this match ({granted} of {} granted)", c.st.raise_cap).into();
            p["why"] = "the per-match cap on raisings is reached".into();
            said.push("no more for you".to_string());
            verdicts.push(Verdict::Refuse);
            note.parts.push(p);
            continue;
        }
        if let Some(q) = b.quests.iter().find(|q| q.open() && q.demigod == t && matches!(q.goal, Goal::Raise { .. })).map(|q| q.id) {
            if let Some(quest) = b.quests.iter_mut().find(|x| x.id == q) {
                if quest.seat != c.seat && !quest.helpers.contains(&c.seat) {
                    quest.helpers.push(c.seat);
                }
            }
        }
        // All of it, or none of it: Death takes no deposits.
        let paid = pay(w, c.actor, price, true, c.st);
        let mut debt = 0.0;
        if paid.short > 0 {
            debt = finance(w, c, death, paid.short, &mut out.props, ruling);
        }
        p["paid"] = paid_json(&paid, debt);
        if paid.short == 0 || debt > 0.0 {
            let how = if debt > 0.0 { "on credit" } else { "paid" };
            entropy_prop(c, death, ruling, paid.sparks as f32 * c.st.will_entropy_per_spark, &mut out.props);
            save(w, b);
            let ok = raise_now(w, c.beat, t, Some(c.actor), c.seat, years, how, c.pantheon, c.st);
            *b = book(w);
            if ok {
                let x = &w.demigods[t.ix()];
                p["outcome"] = "raised".into();
                p["result"] = format!("{name} rises at {}/{} health ({how}: {price} sparks, {years} years of its life)", r1(x.health), r1(x.max_health)).into();
                out.public.push(json!({ "event": "will.raised", "by": Ref::Demigod(c.actor), "by_name": w.name_of(Ref::Demigod(c.actor)),
                                        "target": Ref::Demigod(t), "target_name": name, "text": format!("{name} rises: Death was paid") }));
                // An open raising quest for it is now moot (the quest step closes it).
                said.push(format!("{name} may get up"));
                verdicts.push(Verdict::Grant);
            } else {
                p["outcome"] = "refused".into();
                p["result"] = format!("{name} could not rise (no years left to return to)").into();
                verdicts.push(Verdict::Refuse);
            }
            note.parts.push(p);
            continue;
        }
        // It can't be paid: a quest (one per fallen demigod; a second plea shows the same one).
        let open = b.quests.iter().find(|q| q.open() && q.demigod == t && matches!(q.goal, Goal::Raise { .. })).map(|q| q.id);
        let q = match open {
            Some(q) => q,
            None => {
                let difficulty = (2 + b.raises.iter().filter(|r| r.demigod == t).count() as u8 + (w.demigods[t.ix()].seat != c.seat) as u8).min(3);
                quests::give(w, b, c.beat, quests::Ask { seat: c.seat, demigod: t, goal: Goal::Raise { years }, god: death, difficulty, curser: None, public: true, expires_beat: None }, c.st)
            }
        };
        p["outcome"] = "quest".into();
        p["result"] = format!("{name} stays down for now: Death's price is {price} sparks and {years} years; the purse can't pay it, so Death names a quest").into();
        p["why"] = format!("price {price} sparks (a bolt breaks into {}); you hold {} sparks and {} bolts", c.st.will_spark_per_bolt, w.demigods[c.actor.ix()].sparks, w.demigods[c.actor.ix()].bolts).into();
        quest_fields(w, b, q, &mut p);
        said.push(format!("{name} must be earned back"));
        verdicts.push(Verdict::Bargain);
        note.parts.push(p);
    }
    let worst = if verdicts.iter().all(|v| *v == Verdict::Grant) {
        Verdict::Grant
    } else if verdicts.iter().all(|v| *v == Verdict::Refuse) {
        Verdict::Refuse
    } else {
        Verdict::Bargain
    };
    let speech = match worst {
        Verdict::Grant => format!("Death: \"Paid. {}, because {}.\"", said.join(", "), pick(w, &format!("f{}:{}r", c.intent.0, c.beat), FLOURISH)),
        Verdict::Refuse => format!("Death: \"No: {}. I have said why, and what would change my mind.\"", said.join("; ")),
        _ => format!("Death: \"{}. I keep a price list and a list of errands; choose.\"", said.join("; ")),
    };
    if let Some(r) = w.rulings.get_mut(ruling.ix()) {
        r.verdict = worst;
        r.speech = speech;
    }
    if !note.gods.contains(&death) {
        note.gods.push(death);
    }
    note.rulings.push(ruling);
    true
}

// ---------------------------------------------------------------------------------------------------------------------
// Curse
// ---------------------------------------------------------------------------------------------------------------------

/// Remember a curse fact at its target's place. The public face never names the owner; the private seats are in
/// `data.seats` (the bearer's and the owner's).
pub fn remember_curse(w: &mut World, beat: u32, c: &Curse, kind: &'static str, text: String, public: bool) {
    let pos = w.demigods[c.target.ix()].pos;
    let wh = dm::locate(w, pos, HexLevel::People);
    let mut data = curses::public_json(w, c);
    data["at"] = json!(pos);
    data["cell"] = wh.cell.clone().into();
    data["beat"] = beat.into();
    // A public fact carries the public face only: never who cast it (not even by seat), never its counter.
    if !public {
        data["seats"] = json!([w.demigods[c.target.ix()].seat, c.owner_seat]);
        data["quest"] = qref(c.quest).into();
    }
    dm::remember(w, beat, &wh, Decision { scope: Scope::Cell, kind, query: None, public, text, data });
}

fn curse_part(w: &mut World, b: &mut Book, c: &Ctx, note: &mut WillNote, out: &mut Out) -> bool {
    let Some(kind) = CurseKind::from_text(&c.text) else { return false };
    // Whom: a rival the words name (or the hand aimed at); else the nearest standing rival (a seal: the newest grave).
    let named: Vec<DemigodId> = named_demigods(w, &c.text)
        .into_iter()
        .chain(c.targets.iter().filter_map(|t| match t {
            Ref::Demigod(d) => Some(*d),
            _ => None,
        }))
        .filter(|d| w.demigods[d.ix()].seat != c.seat)
        .collect();
    let here = w.demigods[c.actor.ix()].pos;
    let target = named.first().copied().or_else(|| {
        let rivals = w.demigods.iter().filter(|d| d.seat != c.seat);
        if kind == CurseKind::Seal {
            rivals.filter(|d| !d.alive).max_by_key(|d| (d.eliminated_beat, std::cmp::Reverse(d.id))).map(|d| d.id)
        } else {
            rivals.filter(|d| d.alive).min_by(|x, y| distance_km(x.pos, here).total_cmp(&distance_km(y.pos, here)).then(x.id.cmp(&y.id))).map(|d| d.id)
        }
    });
    let god_tier = any(&c.text, &["god-tier", "god tier", "godtier", "eternal", "forever", "until countered", "unbreakable", "greater"])
        || c.pantheon.gods.iter().any(|g| has_phrase(&c.text, &g.slug));
    let god = god_of(c, kind.god());
    let ruling = rule(w, c, god, Verdict::Grant, String::new());
    note.rulings.push(ruling);
    if !note.gods.contains(&god) {
        note.gods.push(god);
    }
    let gname = w.name_of(Ref::God(god));
    let set = |w: &mut World, v: Verdict, s: String| {
        if let Some(r) = w.rulings.get_mut(ruling.ix()) {
            r.verdict = v;
            r.speech = s;
        }
    };
    let Some(t) = target else {
        note.parts.push(json!({ "part": "curse", "curse": kind, "god": gname, "outcome": "refused",
                                "result": format!("no one to lay {} on", kind.title()),
                                "why": if kind == CurseKind::Seal { "no rival demigod has fallen: name the one to seal" } else { "no rival demigod stands" } }));
        set(w, Verdict::Refuse, format!("{gname}: \"On whom? Name one.\""));
        return true;
    };
    let mut p = part_base(w, Kind::Curse, t, god);
    p["curse"] = json!(kind);
    let tname = w.demigods[t.ix()].name.clone();
    let living_only = kind != CurseKind::Seal;
    let refuse = if living_only && !w.demigods[t.ix()].alive {
        Some((format!("{tname} has fallen: only a seal can be laid on the dead"), "say \"seal\" or \"no resurrection\" for a grave".to_string()))
    } else if curses::active(b, t, kind, c.beat).is_some() {
        Some((format!("{tname} already bears {}", kind.title()), "one of a kind at a time".to_string()))
    } else if b.curses.iter().filter(|x| x.owner_seat == c.seat && x.holds(c.beat)).count() as u32 >= c.st.curse_max_per_player {
        Some((format!("you already have {} curses standing", c.st.curse_max_per_player), "a player's standing curses are capped; one must break or fade first".to_string()))
    } else if !w.demigods[c.actor.ix()].alive {
        Some(("the fallen can't curse, only plead".to_string(), "plead with Death to come back first".to_string()))
    } else {
        None
    };
    if let Some((result, why)) = refuse {
        p["outcome"] = "refused".into();
        p["result"] = result.clone().into();
        p["why"] = why.into();
        note.parts.push(p);
        set(w, Verdict::Refuse, format!("{gname}: \"No: {result}.\""));
        return true;
    }
    let factor = price_factor(w, b, c.seat, c.beat);
    let price = (if god_tier { c.st.curse_god_price_sparks } else { c.st.curse_price_sparks }) * factor;
    let paid = pay(w, c.actor, price, true, c.st);
    let mut debt = 0.0;
    if paid.short > 0 {
        debt = finance(w, c, god, paid.short, &mut out.props, ruling);
    }
    p["paid"] = paid_json(&paid, debt);
    if paid.short > 0 && debt == 0.0 {
        p["outcome"] = "unpaid".into();
        p["result"] = format!("no curse: {} costs {price} sparks", if god_tier { "the god-tier variant" } else { kind.title() }).into();
        p["why"] = format!("you hold {} sparks and {} bolts (a bolt breaks into {}), and the gods lend no more", w.demigods[c.actor.ix()].sparks, w.demigods[c.actor.ix()].bolts, c.st.will_spark_per_bolt).into();
        p["next"] = format!("pay {price} sparks").into();
        note.parts.push(p);
        set(w, Verdict::Refuse, format!("{gname}: \"Malice is not free. {price} sparks.\""));
        return true;
    }
    let dc = c.st.curse_dc as i32
        + w.dm.story(t).map(|s| s.wardrobe as i32 / 2).unwrap_or(0)
        + if arms::kit(w, t, c.st).resists.iter().any(|r| r == "curse") { 2 } else { 0 };
    let mut mods = vec![("the caster".to_string(), dm::demigod_modifier(w, c.actor))];
    mods.extend(standing_mods(w, c.actor, c.beat));
    if god_tier {
        mods.push((format!("{gname}'s hand"), 2));
    }
    if debt > 0.0 {
        mods.push(("on credit".to_string(), -1));
    }
    if crate::beat::is_final_wish(w, c.beat) {
        mods.push(("the Final Wish".to_string(), 2));
    }
    let (r, rv) = roll(w, c, "curse", t, dc, mods, "a curse");
    p["roll"] = rv;
    let base_strength: u8 = if god_tier { 3 } else { 2 };
    let lasting = god_tier || kind == CurseKind::Seal;
    let (bearer, strength, until) = match r.verdict {
        DiceVerdict::CritSuccess => (t, (base_strength + 1).min(3), if lasting { None } else { Some(c.beat + c.st.curse_beats * 3 / 2) }),
        DiceVerdict::Success => (t, base_strength, if lasting { None } else { Some(c.beat + c.st.curse_beats) }),
        DiceVerdict::Fail => (t, 1, Some(c.beat + (c.st.curse_beats / 2).max(1))),
        // A natural 1: it turns on its caster (never a seal on the living caster: that one just fizzles weak).
        DiceVerdict::CritFail => (c.actor, 1, Some(c.beat + (c.st.curse_beats / 2).max(1))),
    };
    let backfired = bearer == c.actor;
    let id = b.curses.len() as u32;
    let bearer_seat = w.demigods[bearer.ix()].seat;
    let q = quests::give(w, b, c.beat, quests::Ask {
        seat: bearer_seat,
        demigod: bearer,
        goal: Goal::BreakCurse { curse: id, curse_kind: kind },
        god,
        difficulty: strength,
        curser: (!backfired).then_some(c.actor),
        public: kind == CurseKind::Seal,
        expires_beat: None,
    }, c.st);
    let cu = Curse {
        id,
        kind,
        owner: c.actor,
        owner_seat: c.seat,
        target: bearer,
        strength,
        cast_beat: c.beat,
        until_beat: until,
        paid_sparks: paid.sparks,
        god: god_tier.then_some(god),
        quest: q,
        broken_beat: None,
        broken_how: String::new(),
    };
    b.curses.push(cu.clone());
    event("curse_cast", Some(Ref::Demigod(c.actor).to_string()), json!({ "beat": c.beat, "curse": cu, "roll": r.roll, "backfired": backfired }));
    let bname = w.demigods[bearer.ix()].name.clone();
    let lasts = match until {
        Some(u) => format!("until beat {u}"),
        None => "until countered".to_string(),
    };
    remember_curse(w, c.beat, &cu, "curse.cast", format!("{} settles on {bname} ({}; {lasts}); it has a counter", cap(kind.title()), kind.does()), true);
    entropy_prop(c, god, ruling, if god_tier { c.st.entropy_per_mega } else { c.st.curse_entropy }, &mut out.props);
    out.props.push(god_prop(c, god, ruling, Effect::Note { text: format!("{} settles on {bname}", cap(kind.title())) }, format!("{} settles on {bname}", cap(kind.title())), 5, -2, vec![Ref::Demigod(bearer), Ref::Demigod(c.actor)]));
    out.public.push(json!({ "event": "will.curse", "target": Ref::Demigod(bearer), "target_name": bname, "curse": kind,
                            "text": format!("{} settles on {bname}: {}", cap(kind.title()), kind.does()) }));
    p["outcome"] = match (r.verdict, backfired) {
        (_, true) => "backfired",
        (DiceVerdict::Fail, _) => "partial",
        (DiceVerdict::CritSuccess, _) => "crit",
        _ => "cast",
    }
    .into();
    p["result"] = if backfired {
        format!("{} turned on its caster: it lies on {bname} (strength 1, {lasts})", kind.title())
    } else {
        format!("{} lies on {bname}: {} (strength {strength}, {lasts})", kind.title(), kind.does())
    }
    .into();
    if r.verdict == DiceVerdict::Fail {
        p["why"] = format!("rolled {} against DC {}: it took, but weakly", r.total, r.target_dc).into();
    }
    p["curse_ref"] = curses::cref(id).into();
    p["counter"] = "its bearer was given a mystic quest that breaks it".into();
    note.parts.push(p);
    let speech = if backfired {
        format!("{gname}: \"You held it by the wrong end. It is yours now.\"")
    } else {
        format!("{gname}: \"Done: {} on {bname}. And because I am fair, I told them how to be rid of it.\"", kind.title())
    };
    set(w, if backfired { Verdict::Twist } else { Verdict::Grant }, speech);
    true
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Ask, offer, gather
// ---------------------------------------------------------------------------------------------------------------------

fn ask_part(w: &World, b: &Book, c: &Ctx, note: &mut WillNote) {
    let mine: Vec<&Quest> = b.quests.iter().filter(|q| q.open() && q.takes(c.seat)).collect();
    let on_me: Vec<&Curse> = b.curses.iter().filter(|x| x.holds(c.beat) && w.demigods[x.target.ix()].seat == c.seat).collect();
    let mut lines = vec![];
    for q in &mine {
        let steps: Vec<String> = q
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| format!("{}. [{}] {}", i + 1, if s.done_beat.is_some() { "done" } else { "to do" }, s.text))
            .collect();
        lines.push(format!("quest {} for {} ({}): {}", qref(q.id), w.name_of(Ref::Demigod(q.demigod)), q.goal.kind(), steps.join(" ")));
    }
    for cu in &on_me {
        lines.push(format!("{} lies on {} ({}): broken by quest {}", cu.kind.title(), w.name_of(Ref::Demigod(cu.target)), cu.kind.does(), qref(cu.quest)));
    }
    let result = if lines.is_empty() { "no quest is open for you, and no curse lies on your demigods".to_string() } else { lines.join(" | ") };
    note.parts.push(json!({
        "part": "ask", "outcome": "told", "result": result,
        "quests": mine.iter().map(|q| quests::quest_json(w, q)).collect::<Vec<_>>(),
        "curses": on_me.iter().map(|cu| curses::private_json(w, b, cu, c.seat)).collect::<Vec<_>>(),
    }));
}

fn offer_part(w: &mut World, b: &mut Book, c: &Ctx, note: &mut WillNote) {
    match quests::offer_sparks(w, b, c.seat, c.actor, c.beat, c.st) {
        Ok((q, n)) => {
            let mut p = json!({ "part": "offer", "outcome": "offered", "result": format!("offered {n} sparks to quest {}", qref(q)),
                                "paid": { "price_sparks": n, "sparks": n, "bolts_broken": 0, "short_sparks": 0, "debt": 0.0 } });
            quest_fields(w, b, q, &mut p);
            note.parts.push(p);
        }
        Err(why) => note.parts.push(json!({ "part": "offer", "outcome": "refused", "result": "nothing was offered", "why": why })),
    }
}

/// Make the ingredient item for `word` in `d`'s hands, at once.
pub fn make_ingredient(w: &mut World, beat: u32, d: DemigodId, word: &str, how: &str, pantheon: &Pantheon, st: &Settings) -> Option<ItemId> {
    let name = quests::ingredient_name(word);
    let me = Ref::Demigod(d);
    let p = Proposal {
        source: Source::Rules,
        intent: None,
        effect: Effect::CreateItem { name: name.clone(), description: format!("An odd ingredient a mystic quest asked for, {how}."), holder: me },
        act: format!("{} comes by {}", w.demigods[d.ix()].name, article(&name)),
        actor: me,
        magnitude: 2,
        valence: 1,
        witnesses: vec![me],
        cause: None,
    };
    let before = w.items.len();
    crate::merge::run(w, vec![p], pantheon, beat, st.max_witnesses as usize);
    (w.items.len() > before).then(|| ItemId::from_ix(before))
}

fn gather_part(w: &mut World, b: &mut Book, c: &Ctx, note: &mut WillNote) {
    let Some((q, word)) = quests::wanted(b, c.seat).into_iter().find(|(_, word)| c.text.contains(word.as_str())) else { return };
    let name = quests::ingredient_name(&word);
    if !w.demigods[c.actor.ix()].alive {
        note.parts.push(json!({ "part": "gather", "outcome": "refused", "result": format!("the fallen can't gather the {name}"), "why": "your demigods have fallen" }));
        return;
    }
    let mods = base_mods(w, c);
    let (r, rv) = roll(w, c, "gather", c.actor, c.st.will_own_dc as i32, mods, "a foraging");
    let mut p = json!({ "part": "gather", "roll": rv, "target": Ref::Demigod(c.actor), "target_name": w.name_of(Ref::Demigod(c.actor)) });
    if r.verdict == DiceVerdict::CritFail {
        p["outcome"] = "fizzle".into();
        p["result"] = format!("the {name} slipped away (a natural 1)").into();
        p["why"] = "try again next beat, or win an encounter: they drop what a quest is waiting on".into();
    } else {
        save(w, b);
        let made = make_ingredient(w, c.beat, c.actor, &word, "gathered by its bearer's own hands", c.pantheon, c.st);
        *b = book(w);
        p["outcome"] = "gathered".into();
        p["result"] = format!("gathered the {name}").into();
        p["item"] = made.and_then(|i| w.items.get(i.ix())).map(|it| crate::items::item_json(w, it)).unwrap_or(Value::Null);
    }
    quest_fields(w, b, q, &mut p);
    note.parts.push(p);
}

// ---------------------------------------------------------------------------------------------------------------------
// The beat
// ---------------------------------------------------------------------------------------------------------------------

/// Every will's result goes into the DM's memory (`will.ruled`, private: its seat's), at the place of the demigod
/// that willed it.
fn remember_will(w: &mut World, beat: u32, note: &WillNote) {
    let pos = w.demigods[note.by.ix()].pos;
    let wh = dm::locate(w, pos, HexLevel::People);
    let parts: Vec<Value> = note
        .parts
        .iter()
        .map(|p| json!({ "part": p["part"], "target": p["target"], "outcome": p["outcome"], "result": p["result"], "quest": p["quest"]["ref"] }))
        .collect();
    let text = format!(
        "{} willed: {}",
        w.name_of(Ref::Demigod(note.by)),
        note.parts.iter().filter_map(|p| p["result"].as_str()).collect::<Vec<_>>().join("; ")
    );
    dm::remember(w, beat, &wh, Decision {
        scope: Scope::Cell,
        kind: "will.ruled",
        query: None,
        public: false,
        text,
        data: json!({ "seat": note.seat, "seats": [note.seat], "by": Ref::Demigod(note.by), "parts": parts, "at": pos, "cell": wh.cell, "beat": beat }),
    });
}

/// May this intent be taken wholly by the wills (it had no other meaning)? Never a journey, a strike, a parley, a
/// subagent, a possession, a zoom, a recall, a manifestation or an army act.
fn takeable(w: &World, i: &Intent) -> bool {
    use fn7_agents::common::{classify, ActClass};
    if !matches!(i.route, Route::World | Route::Pending | Route::God { .. }) {
        return false;
    }
    if i.how == crate::beat::STANDING_HOW || i.how == crate::godtier::HOW {
        return false;
    }
    if crate::items::is_manifest_act(w, i) || crate::beat::is_army_act(w, i) || crate::society::is_society_act(w, i) {
        return false;
    }
    // Verbs a plea or a gift is spoken with, whatever class the resolver would give them.
    const PLEA: &[&str] = &["raise", "rise", "bring", "return", "wake", "give", "gives", "grant", "hand", "get", "gets", "come", "plead", "beg"];
    Kind::of_verb(&i.verb).is_some() || matches!(classify(&i.verb), ActClass::Other) || PLEA.contains(&i.verb.trim().to_lowercase().as_str())
}

/// Is "wills that matter" on in this world (cheap: no settings parse)? The room setting, on a schema-10+ state.
pub fn on(w: &World) -> bool {
    w.wills_rules() && w.meta.settings.get("wills_enabled").and_then(Value::as_bool) == Some(true)
        && w.meta.settings.get("rules_0_8_1").and_then(Value::as_bool) == Some(true)
}

/// Is this intent one the wills resolve (so it never becomes a standing order)?
pub fn is_will(w: &World, i: &Intent, st: &Settings) -> bool {
    st.wills_enabled && takeable(w, i) && (Kind::of_verb(&i.verb).is_some() || !kinds_in(w, &book(w), w.demigods[i.demigod.ix()].seat, &i.span).is_empty())
}

/// Resolve this beat's wills (rules 0.9.0), before the route: the hands' intents from `from` on, and the fallen
/// players' pleas. Applies what is immediate (payments, raisings, curses, quests given, gathered ingredients, the
/// dice and the rulings) and returns the effects for the beat's merge, the intents it took, and what to tell.
pub fn resolve(
    w: &mut World,
    beat: u32,
    from: usize,
    hand_text: &BTreeMap<AgentId, Vec<String>>,
    fallen: &[FallenWill],
    pantheon: &Pantheon,
    st: &Settings,
) -> Out {
    let mut out = Out::default();
    if !st.wills_enabled {
        return out;
    }
    let mut b = book(w);
    // The hands that spoke, in agent order.
    let hands: Vec<(AgentId, String)> = hand_text.iter().map(|(a, lines)| (*a, lines.join(" "))).collect();
    for (agent, speech) in hands {
        let mine: Vec<usize> = (from..w.intents.len())
            .filter(|i| {
                let x = &w.intents[*i];
                x.agent == agent && x.beat == beat && x.how != crate::beat::STANDING_HOW && x.how != crate::godtier::HOW
            })
            .collect();
        let Some(&first) = mine.first() else { continue };
        let actor = w.intents[first].demigod;
        if !w.demigods[actor.ix()].alive {
            continue;
        }
        let seat = w.demigods[actor.ix()].seat;
        let mut kinds = kinds_in(w, &b, seat, &speech);
        // The hand's own split counts too (its verb says heal / arm / revive / curse though the words were unusual).
        for i in &mine {
            let x = &w.intents[*i];
            if let Some(k) = Kind::of_verb(&x.verb).filter(|_| takeable(w, x)) {
                let fine = match k {
                    Kind::Raise => w.demigods.iter().any(|d| !d.alive),
                    Kind::Curse => CurseKind::from_text(&format!("{} {}", x.verb, speech)).is_some(),
                    _ => true,
                };
                if fine && !kinds.contains(&k) {
                    kinds.push(k);
                }
            }
        }
        kinds.sort();
        if kinds.is_empty() {
            continue;
        }
        let mut note = WillNote { seat, text: speech.clone(), by: actor, parts: vec![], items: vec![], gods: vec![], rulings: vec![] };
        let mut claimed: Vec<usize> = vec![];
        for k in kinds {
            // The carrier: the intent whose verb names this part, else one whose words carry it, else the first.
            let carrier = mine
                .iter()
                .copied()
                .find(|i| !claimed.contains(i) && Kind::of_verb(&w.intents[*i].verb) == Some(k))
                .or_else(|| mine.iter().copied().find(|i| kinds_in(w, &b, seat, &w.intents[*i].span).contains(&k)))
                .unwrap_or(first);
            let x = w.intents[carrier].clone();
            let verb_named = Kind::of_verb(&x.verb) == Some(k);
            let mut text = speech.to_lowercase();
            if k == Kind::Curse && CurseKind::from_text(&text).is_none() {
                text = format!("{} {text}", x.verb.to_lowercase());
            }
            let wish = if crate::beat::is_final_wish(w, beat) && !verb_named { st.final_wish_magnitude as u8 } else { 0 };
            let c = Ctx {
                beat,
                pantheon,
                st,
                actor,
                seat,
                text,
                intent: x.id,
                magnitude: if verb_named { worded_magnitude(x.magnitude, &speech) } else { (guess_magnitude(&speech) + wish).min(10) },
                targets: if verb_named {
                    x.targets.iter().filter_map(|t| match t {
                        Target::Known(r) => Some(*r),
                        _ => None,
                    }).collect()
                } else {
                    vec![]
                },
                fallen: false,
            };
            let before = note.parts.len();
            let did = match k {
                Kind::Heal | Kind::Bless => heal_part(w, &mut b, &c, k, &mut note, &mut out),
                Kind::Arm => arm_part(w, &mut b, &c, &mut note, &mut out),
                Kind::Raise => raise_part(w, &mut b, &c, &mut note, &mut out),
                Kind::Curse => curse_part(w, &mut b, &c, &mut note, &mut out),
                Kind::Ask => {
                    ask_part(w, &b, &c, &mut note);
                    true
                }
                Kind::Offer => {
                    offer_part(w, &mut b, &c, &mut note);
                    true
                }
                Kind::Gather => {
                    gather_part(w, &mut b, &c, &mut note);
                    true
                }
            };
            if did || note.parts.len() > before {
                if verb_named {
                    claimed.push(carrier);
                }
                // Take the carrier when it had no other meaning; every will-verbed intent of this hand is taken.
                if takeable(w, &x) {
                    out.handled.insert(x.id);
                }
            }
        }
        if note.parts.is_empty() {
            continue;
        }
        for i in &mine {
            let x = &w.intents[*i];
            if takeable(w, x) && Kind::of_verb(&x.verb).is_some() {
                out.handled.insert(x.id);
            }
        }
        remember_will(w, beat, &note);
        event("will", Some(Ref::Demigod(actor).to_string()),
              json!({ "beat": beat, "seat": seat, "text": speech, "parts": note.parts, "handled": out.handled.iter().map(|i| i.0).collect::<Vec<_>>() }));
        out.notes.push(note);
    }
    // The fallen: they can plead with Death, ask about their quests and make offerings; nothing else.
    for will in fallen {
        let bodies: Vec<DemigodId> = w.bodies_of_seat(will.seat).map(|d| d.id).collect();
        let Some(&lead) = bodies.first() else { continue };
        let kinds: Vec<Kind> = kinds_in(w, &b, will.seat, &will.text).into_iter().filter(|k| matches!(k, Kind::Raise | Kind::Ask | Kind::Offer)).collect();
        let mut note = WillNote { seat: will.seat, text: will.text.clone(), by: lead, parts: vec![], items: vec![], gods: vec![], rulings: vec![] };
        if kinds.is_empty() {
            note.parts.push(json!({
                "part": "refused", "outcome": "refused", "result": "nothing happened: your demigods have fallen",
                "why": "the fallen can't act, only plead",
                "next": "plead with Death to come back (\"my demigods come back to life\"), ask what a quest needs, or offer sparks to a quest",
            }));
        } else {
            // A plea needs an intent for Death's ruling to hang on (as in 0.8.1).
            let id = IntentId::from_ix(w.intents.len());
            let d = &w.demigods[lead.ix()];
            let i = Intent {
                id,
                beat,
                agent: d.hands[0],
                demigod: lead,
                verb: if kinds.contains(&Kind::Raise) { "revive".into() } else { "plead".into() },
                targets: bodies.iter().map(|x| Target::Known(Ref::Demigod(*x))).collect(),
                how: String::new(),
                place: Some(d.place),
                magnitude: 6,
                span: will.text.chars().take(240).collect(),
                route: Route::God { god: pantheon.id_of("death").unwrap_or(GodId(0)), by: RouteBy::Keyword },
            };
            event("intent", Some(Ref::Agent(i.agent).to_string()), json!({ "beat": beat, "intent": i, "routed_by": "keyword", "fallback": null, "fallen_will": true }));
            w.intents.push(i);
            out.handled.insert(id);
            let c = Ctx { beat, pantheon, st, actor: lead, seat: will.seat, text: will.text.to_lowercase(), intent: id, magnitude: 6, targets: vec![], fallen: true };
            for k in kinds {
                match k {
                    Kind::Raise => {
                        raise_part(w, &mut b, &c, &mut note, &mut out);
                    }
                    Kind::Ask => ask_part(w, &b, &c, &mut note),
                    _ => offer_part(w, &mut b, &c, &mut note),
                }
            }
        }
        remember_will(w, beat, &note);
        event("fallen_will", Some(format!("seat:{}", will.seat)), json!({ "beat": beat, "seat": will.seat, "seq": will.seq, "text": will.text, "parts": note.parts }));
        out.notes.push(note);
    }
    save(w, &b);
    out
}

// ---------------------------------------------------------------------------------------------------------------------
// Payloads
// ---------------------------------------------------------------------------------------------------------------------

/// What a seat sees of the book: its quests (and the public ones), the curses on its demigods and the ones it cast,
/// its demigods' boons, the raisings so far.
pub fn seat_json(w: &World, seat: u8) -> Value {
    let b = book(w);
    let beat = w.meta.beat;
    let quests: Vec<Value> = b
        .quests
        .iter()
        .filter(|q| q.takes(seat) || q.public || b.curses.iter().any(|c| c.quest == q.id && c.owner_seat == seat))
        .filter(|q| q.open() || q.done_beat.or(q.failed_beat).is_some_and(|x| x + 8 >= beat))
        .map(|q| quests::quest_json(w, q))
        .collect();
    let curses: Vec<Value> = b
        .curses
        .iter()
        .filter(|c| c.holds(beat) || c.broken_beat.is_some_and(|x| x + 8 >= beat))
        .map(|c| {
            let on_me = w.demigods.get(c.target.ix()).map(|d| d.seat) == Some(seat);
            if on_me || c.owner_seat == seat {
                curses::private_json(w, &b, c, seat)
            } else {
                curses::public_json(w, c)
            }
        })
        .collect();
    let boons: Vec<Value> = b
        .boons
        .iter()
        .filter(|x| x.until_beat >= beat && w.demigods[x.demigod.ix()].seat == seat)
        .map(|x| json!({ "demigod": Ref::Demigod(x.demigod), "bonus": x.bonus, "until_beat": x.until_beat, "from": x.from }))
        .collect();
    let granted = b.raises.iter().filter(|r| r.by_seat == seat).count();
    json!({ "quests": quests, "curses": curses, "boons": boons,
            "raises": { "granted": granted, "cap": crate::world::settings(w).raise_cap } })
}

/// What everyone sees of the book: the curses' public faces and the public quests.
pub fn public_json(w: &World) -> Value {
    let b = book(w);
    let beat = w.meta.beat;
    json!({
        "curses": b.curses.iter().filter(|c| c.holds(beat)).map(|c| curses::public_json(w, c)).collect::<Vec<_>>(),
        "quests": b.quests.iter().filter(|q| q.public && q.open()).map(|q| quests::quest_json(w, q)).collect::<Vec<_>>(),
    })
}

/// The curse marks on a demigod's public piece.
pub fn demigod_fields(w: &World, d: &Demigod, v: &mut Value) {
    let b = book(w);
    let beat = w.meta.beat;
    v["curses"] = curses::on(&b, d.id, beat).map(|c| curses::public_json(w, c)).collect();
    let raised = b.raises.iter().filter(|r| r.demigod == d.id).count();
    if raised > 0 {
        v["raised"] = raised.into();
    }
}

/// The private events of this beat for `seat`: quest and curse facts the public does not get (same shapes as the
/// public `dm_events`).
pub fn private_events(w: &World, memory_start: usize, seat: u8) -> Vec<Value> {
    w.dm.memory[memory_start.min(w.dm.memory.len())..]
        .iter()
        .filter(|e| !e.public && (e.kind.starts_with("quest.") || e.kind.starts_with("curse.")))
        .filter(|e| e.data["seats"].as_array().is_some_and(|s| s.iter().any(|x| x.as_u64() == Some(seat as u64))))
        .map(|e| {
            let mut v = e.data.clone();
            v["event"] = e.kind.clone().into();
            v["text"] = e.text.clone().into();
            v["memory"] = e.id.into();
            v
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------------------------------
// The built-in AI
// ---------------------------------------------------------------------------------------------------------------------

/// A will the built-in AI demigod `d` makes this beat, if any (rules 0.9.0; its own seeded stream, so nothing else
/// the AI draws changes): heal when hurt, arm when bare, plead for a fallen sibling, curse a strong rival, pursue a
/// quest's current step.
pub fn ai_line(w: &World, d: DemigodId, beat: u32, st: &Settings) -> Option<String> {
    if !st.wills_enabled || st.wills_ai_chance <= 0.0 || crate::beat::is_final_wish(w, beat) {
        return None;
    }
    let me = &w.demigods[d.ix()];
    let mut rng = Rng::derive(w.meta.seed, &[hash_str("bot-wills"), beat as u64, d.0 as u64]);
    let p = st.wills_ai_chance as f64;
    let roll = |rng: &mut Rng, chance: f64| rng.f64() < chance * p;
    let b = book(w);
    let purse = me.sparks + me.bolts * st.will_spark_per_bolt;
    // A quest's current step, first.
    if let Some(q) = b.quests.iter().find(|q| q.open() && q.takes(me.seat)) {
        if let Some(c) = q.current() {
            let line = match &q.steps[c].kind {
                quests::StepKind::Sparks { n } if purse >= *n => Some("offer the sparks to the quest".to_string()),
                quests::StepKind::Hold { word } => Some(format!("gather the {} for the quest", quests::ingredient_name(word).to_lowercase())),
                quests::StepKind::Shrine => Some("raise a shrine to me here".to_string()),
                quests::StepKind::At { req: quests::PlaceReq::Shrine { of }, .. } => {
                    w.dm.shrines.iter().find(|s| s.demigod == *of).map(|s| format!("go to {}", w.name_of(Ref::Place(s.place))))
                }
                quests::StepKind::At { req: quests::PlaceReq::Point { at, .. }, .. } => {
                    Some(format!("go to {}", w.name_of(Ref::Place(fn7_society::geo::place_at(w, *at)))))
                }
                quests::StepKind::At { req: quests::PlaceReq::Settlement { level }, .. } => w
                    .dm
                    .settlements
                    .iter()
                    .filter(|s| s.level >= *level)
                    .min_by(|x, y| distance_km(w.places[x.place.ix()].center, me.pos).total_cmp(&distance_km(w.places[y.place.ix()].center, me.pos)).then(x.place.cmp(&y.place)))
                    .map(|s| format!("go to {}", w.places[s.place.ix()].name)),
                _ => None,
            };
            if let Some(l) = line {
                if roll(&mut rng, 1.6) {
                    return Some(l);
                }
            }
        }
    }
    let sibling_down = w.bodies_of_seat(me.seat).any(|x| !x.alive);
    if sibling_down && purse >= st.raise_price_sparks && roll(&mut rng, 1.0) {
        return Some("raise my fallen sibling: come back to life".into());
    }
    if me.health <= 4.0 && purse >= 1 && roll(&mut rng, 1.2) {
        return Some("heal me".into());
    }
    let k = arms::kit(w, d, st);
    if k.arms.is_empty() && purse >= st.arm_price_artifact && roll(&mut rng, 0.5) {
        return Some("arm me".into());
    }
    if purse >= st.curse_price_sparks + 2 && roll(&mut rng, 0.3) {
        let rival = w.demigods.iter().filter(|x| x.alive && x.seat != me.seat).min_by(|x, y| distance_km(x.pos, me.pos).total_cmp(&distance_km(y.pos, me.pos)).then(x.id.cmp(&y.id)))?;
        let kind = ["wither", "hex", "bind", "silence"][rng.below(4) as usize];
        return Some(format!("curse {} with a {kind}", rival.name));
    }
    None
}

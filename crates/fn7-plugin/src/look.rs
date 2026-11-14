//! Gear looks (state schema 11, room setting `looks_enabled`; docs/MEDIA.md "Looks").
//!
//! The designer asked where the equipment that demigods pick up is, and for portraits of each item to be made
//! idk major acquisition for the user's characters but not for the ai's? ... so it feels like ur getting more and more
//! stuff with proper promptcrafting, caching for items etc etc so the time travel looks perfect".
//!
//! - A demigod's **loadout** ([`loadout`]) is its notable gear right now: its best item per slot (weapon, armour,
//!   charm: the 0.9.0 arms rule) plus up to `look_extra_items` other major items (relic tier and up, god-blessed, or
//!   the first item of a kind it has), in a stable order. Its hash ([`loadout_hash`]) is over normalized item
//!   **descriptors** ([`Gear`]: kind, material, tier, name stem, blessing), never item ids, so the same gear is the
//!   same hash in every match.
//! - A **major acquisition** is a change of the loadout. For a human-controlled demigod it becomes a new **look**
//!   ([`Look`], in `World.dm.looks`): the portrait and diorama subjects with a gear clause ([`portrait_subject`],
//!   [`diorama_subject`]), which the media layer draws from the demigod's base images. At most
//!   `look_images_per_demigod` new pictures per demigod per match and one per `look_debounce_beats` beats (the newest
//!   loadout wins). The built-in AI's demigods keep their base images (their loadout is still listed).
//! - **Item art**: notable items a human's demigod holds (or that lie in a loot pile) are granted a picture
//!   ([`item_subject`], keyed by the descriptor: the same sword is the same picture everywhere), at most
//!   `item_art_per_match` per match; lesser items keep their `icon`.
//!
//! Everything is rules: the state stores the look that is current at each beat, so a snapshot, a replay, a fork and
//! the present all show the image that was current then, and the plugin never calls the image service.

use fn7_model::dm::Scope;
use fn7_model::hexgrid::HexLevel;
use fn7_model::manifest::{icon_of, ItemKind, ItemState, Tier};
use fn7_model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::settings::Settings;
use crate::wills::arms;
use crate::world::event;

/// Part of every loadout hash: bumped when the descriptor or the selection rule changes.
pub const LOADOUT_VERSION: &str = "fn7-loadout/1";
/// Part of every item art key.
pub const ART_VERSION: &str = "fn7-item-art/1";
/// Earlier looks kept per demigod (the newest).
pub const PAST_KEPT: usize = 8;
/// Earlier looks offered to the media layer as stand-ins while a new image renders.
pub const FALLBACKS: usize = 3;

// ---------------------------------------------------------------------------------------------------------------------
// Descriptors
// ---------------------------------------------------------------------------------------------------------------------

/// A normalized item descriptor: what an item IS, without its id, its match or its maker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gear {
    pub kind: ItemKind,
    /// The arms rule's material (gear only; `""` otherwise).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub material: String,
    pub tier: Tier,
    /// The name, normalized ([`stem_of`]).
    pub stem: String,
    /// The slug of the god whose blessing it carries (`""`: none).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub blessing: String,
    /// The name as shown (not part of the key).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

impl Gear {
    /// The descriptor as one line: `kind|material|tier|stem|blessing`.
    pub fn key(&self) -> String {
        format!("{}|{}|{}|{}|{}", kind_str(self.kind), self.material, self.tier.as_str(), self.stem, self.blessing)
    }

    /// The key of this item's picture: sha256 of [`ART_VERSION`] and [`Gear::key`].
    pub fn art_key(&self) -> String {
        sha(&[ART_VERSION, &self.key()])
    }
}

fn sha(parts: &[&str]) -> String {
    let mut h = Sha256::new();
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            h.update(b"\n");
        }
        h.update(p.as_bytes());
    }
    hex::encode(h.finalize())
}

pub fn kind_str(k: ItemKind) -> &'static str {
    match k {
        ItemKind::Artifact => "artifact",
        ItemKind::Weapon => "weapon",
        ItemKind::Armor => "armor",
        ItemKind::Relic => "relic",
        ItemKind::Structure => "structure",
        ItemKind::Army => "army",
        ItemKind::Fleet => "fleet",
        ItemKind::Creature => "creature",
        ItemKind::Phenomenon => "phenomenon",
        ItemKind::Other => "other",
    }
}

/// A name as a stem: lowercased, possessives and punctuation dropped, leading articles dropped, at most 8 words.
pub fn stem_of(name: &str) -> String {
    let lower = name.to_lowercase().replace("'s ", " ").replace("\u{2019}s ", " ");
    let clean: String = lower.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    clean
        .split_whitespace()
        .skip_while(|w| matches!(*w, "the" | "a" | "an"))
        .take(8)
        .collect::<Vec<_>>()
        .join(" ")
}

/// The descriptor of `it` in this world.
pub fn gear_of(w: &World, it: &Item) -> Gear {
    let slot = arms::slot_of(it.ext.kind);
    let material = if slot.is_some() { arms::material_of(w.meta.seed, &it.name).0.to_string() } else { String::new() };
    let blessing = arms::blessed_by(w, it).map(|g| w.name_of(Ref::God(g)).to_lowercase()).unwrap_or_default();
    Gear { kind: it.ext.kind, material, tier: it.ext.tier, stem: stem_of(&it.name), blessing, name: it.name.clone() }
}

// ---------------------------------------------------------------------------------------------------------------------
// The loadout
// ---------------------------------------------------------------------------------------------------------------------

/// One loadout line: the item, its role (`weapon` | `armor` | `charm`: the slot it fills; `extra`: another major item)
/// and its descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub item: ItemId,
    pub role: &'static str,
    pub gear: Gear,
}

impl Entry {
    fn line(&self) -> String {
        format!("{}|{}", self.role, self.gear.key())
    }
}

/// `d`'s notable gear right now, in a stable order: weapon, armour, charm (the best per slot: the higher tier, the
/// newer of equals), then up to `extras` other major items (relic tier and up or god-blessed, highest tier first; then
/// the first item of each kind not yet listed). Items lying in a loot pile and destroyed things never count.
pub fn loadout(w: &World, d: DemigodId, extras: usize) -> Vec<Entry> {
    let me = Ref::Demigod(d);
    let on_ground = |i: ItemId| w.dm.loot.iter().any(|l| l.taken_beat.is_none() && l.items.contains(&i));
    let mine: Vec<&Item> = w
        .items
        .iter()
        .filter(|i| i.holders.last() == Some(&me) && matches!(i.ext.state, ItemState::Held | ItemState::Deployed) && !on_ground(i.id))
        .collect();
    let mut best: Vec<(&'static str, &Item)> = vec![];
    for it in mine.iter().filter(|i| i.ext.state == ItemState::Held) {
        let Some(slot) = arms::slot_of(it.ext.kind) else { continue };
        match best.iter_mut().find(|b| b.0 == slot) {
            Some(b) => {
                if it.ext.tier >= b.1.ext.tier {
                    b.1 = it;
                }
            }
            None => best.push((slot, it)),
        }
    }
    let mut out: Vec<Entry> = ["weapon", "armor", "charm"]
        .into_iter()
        .filter_map(|s| best.iter().find(|b| b.0 == s))
        .map(|(slot, it)| Entry { item: it.id, role: slot, gear: gear_of(w, it) })
        .collect();
    let rest: Vec<&Item> = mine.iter().copied().filter(|i| !out.iter().any(|e| e.item == i.id)).collect();
    let mut major: Vec<&Item> = rest.iter().copied().filter(|i| i.ext.tier >= Tier::Relic || arms::blessed_by(w, i).is_some()).collect();
    major.sort_by(|a, b| b.ext.tier.cmp(&a.ext.tier).then(a.id.cmp(&b.id)));
    let mut picked: Vec<&Item> = major.into_iter().take(extras).collect();
    for it in &rest {
        if picked.len() >= extras {
            break;
        }
        let listed = out.iter().any(|e| e.gear.kind == it.ext.kind) || picked.iter().any(|p| p.ext.kind == it.ext.kind);
        if !listed {
            picked.push(it);
        }
    }
    let mut ex: Vec<Entry> = picked.into_iter().map(|it| Entry { item: it.id, role: "extra", gear: gear_of(w, it) }).collect();
    ex.sort_by(|a, b| b.gear.tier.cmp(&a.gear.tier).then_with(|| a.gear.key().cmp(&b.gear.key())).then(a.item.cmp(&b.item)));
    out.extend(ex);
    out
}

/// The hash of a loadout: sha256 of [`LOADOUT_VERSION`] and each line `role|kind|material|tier|stem|blessing`, in
/// order. `""` for an empty loadout (the base look).
pub fn loadout_hash(lo: &[Entry]) -> String {
    if lo.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = lo.iter().map(Entry::line).collect();
    let mut parts: Vec<&str> = vec![LOADOUT_VERSION];
    parts.extend(lines.iter().map(String::as_str));
    sha(&parts)
}

// ---------------------------------------------------------------------------------------------------------------------
// The prompt templates
// ---------------------------------------------------------------------------------------------------------------------

/// How each material looks (the arms rule's materials, in the house style's vocabulary).
fn material_look(m: &str) -> &'static str {
    match m {
        "obsidian" => "black volcanic obsidian glass with razor facets",
        "dragonscale" => "overlapping ember-red dragon scales",
        "butterfly-wing" => "iridescent butterfly-wing membrane veined like stained glass",
        "platinum" => "pale mirror-bright platinum",
        "struck-oak" => "lightning-charred oak with glowing orange cracks in the grain",
        "moonsteel" => "cold blue-white moonsteel",
        "whalebone" => "carved ivory whalebone lashed with tarred cord",
        "salt-glass" => "cloudy sea-green salt-glass",
        "gold" => "beaten gold",
        "heron-feather" => "long grey-white heron feathers bound in silver wire",
        _ => "",
    }
}

/// How each tier reads in a picture.
fn tier_glow(t: Tier) -> &'static str {
    match t {
        Tier::Trinket => "plain and battered",
        Tier::Artifact => "finely made, with a faint glow along its edges",
        Tier::Relic => "ornate, glowing steadily",
        Tier::Wonder => "radiant, wrapped in a halo of light",
        _ => "blazing with light that spills over everything near it",
    }
}

/// The mark a god's blessing leaves on gear (by god slug).
fn signature(slug: &str) -> &'static str {
    match slug {
        "" => "",
        "time" => "ringed by a thin halo of slow-turning clock hands of pale gold light",
        "nature" => "wound with living vines and small green leaves",
        "consequence" => "studded with barbed iron thorns that glow dull red",
        "death" => "inlaid with pale bone, a cold grey mist trailing from it",
        "chance" => "set with dice-pip studs that flicker gold",
        "memory" => "etched with mirror-bright lines that glow soft blue",
        "entropy" => "split by hairline cracks leaking violet static",
        _ => "marked with the glowing sigil of the god who blessed it",
    }
}

fn kind_noun(k: ItemKind) -> &'static str {
    match k {
        ItemKind::Weapon => "weapon",
        ItemKind::Armor => "armour",
        ItemKind::Relic => "relic",
        ItemKind::Artifact => "talisman",
        ItemKind::Structure => "monument",
        ItemKind::Army => "host",
        ItemKind::Fleet => "fleet",
        ItemKind::Creature => "beast",
        ItemKind::Phenomenon => "wonder",
        ItemKind::Other => "keepsake",
    }
}

fn article(s: &str) -> String {
    let first = s.chars().next().map(|c| c.to_ascii_lowercase()).unwrap_or('x');
    let last = s.split_whitespace().last().unwrap_or("");
    // Plurals and mass nouns ("gauntlets", "plate armour", "chain mail") take no article.
    let bare = (last.ends_with('s') && !last.ends_with("ss")) || matches!(last, "armour" | "armor" | "mail");
    if bare {
        s.to_string()
    } else if "aeiou".contains(first) {
        format!("an {s}")
    } else {
        format!("a {s}")
    }
}

/// The thing's own noun: its stem without the material's words and without a trailing "of <god>".
fn noun(g: &Gear) -> String {
    let mat: Vec<&str> = g.material.split('-').filter(|w| !w.is_empty()).collect();
    let mut words: Vec<&str> = g.stem.split_whitespace().filter(|w| !mat.contains(w)).collect();
    if !g.blessing.is_empty() && words.len() >= 3 && words[words.len() - 2] == "of" && words[words.len() - 1] == g.blessing {
        words.truncate(words.len() - 2);
    }
    while words.last().is_some_and(|w| matches!(*w, "of" | "the" | "and")) {
        words.pop();
    }
    let n = words.join(" ");
    match n.as_str() {
        "" => kind_noun(g.kind).to_string(),
        "plate" => "plate armour".to_string(),
        _ => n,
    }
}

/// One piece of gear in words: "a blade of iridescent butterfly-wing membrane veined like stained glass, ornate,
/// glowing steadily, wound with living vines and small green leaves" (a concrete noun, its material, its glow by
/// tier, its god's signature; never a number).
pub fn phrase(g: &Gear) -> String {
    let mut s = article(&noun(g));
    let look = material_look(&g.material);
    if !look.is_empty() {
        s.push_str(&format!(" of {look}"));
    }
    s.push_str(&format!(", {}", tier_glow(g.tier)));
    let sig = signature(&g.blessing);
    if !sig.is_empty() {
        s.push_str(&format!(", {sig}"));
    }
    s
}

fn has_word(n: &str, ws: &[&str]) -> bool {
    n.split_whitespace().any(|w| ws.iter().any(|x| w == *x || w.strip_suffix('s') == Some(*x)))
}

/// Where a piece sits on the figure in a portrait (the place first, then the piece).
fn worn(role: &str, g: &Gear) -> String {
    let n = noun(g);
    let p = phrase(g);
    match (role, g.kind) {
        ("weapon", _) => format!("gripped in the right hand, {p}"),
        ("armor", _) if has_word(&n, &["shield", "buckler"]) => format!("carried on the left forearm, {p}"),
        ("armor", _) if has_word(&n, &["gauntlet", "bracer", "vambrace"]) => format!("worn on the forearms, {p}"),
        ("armor", _) if has_word(&n, &["helm", "helmet", "mask"]) => format!("worn on the head, {p}"),
        ("armor", _) if has_word(&n, &["cloak", "shroud", "cape", "mantle"]) => format!("draped over the shoulders, {p}"),
        ("armor", _) => format!("strapped over the chest and shoulders, {p}"),
        ("charm", _) if has_word(&n, &["ring"]) => format!("worn on the left hand, {p}"),
        ("charm", _) if has_word(&n, &["crown", "circlet", "diadem"]) => format!("worn on the brow, {p}"),
        ("charm", _) if has_word(&n, &["horn", "compass", "lantern", "book", "kit"]) => format!("hanging from the belt, {p}"),
        ("charm", _) => format!("hanging on a cord at the throat, {p}"),
        (_, ItemKind::Weapon) => format!("slung across the back, {p}"),
        (_, ItemKind::Armor) => format!("buckled at the hip, {p}"),
        (_, ItemKind::Relic | ItemKind::Artifact) => format!("hanging from the belt, {p}"),
        (_, ItemKind::Army) => format!("planted behind the left shoulder, the tall war banner of {}, ranks of tiny spear points beneath it", article(&n)),
        (_, ItemKind::Fleet) => format!("on the horizon behind, the small dark sails of {}", article(&n)),
        (_, ItemKind::Creature) => format!("crouched at their side, {}, {}", article(&n), tier_glow(g.tier)),
        (_, ItemKind::Structure) => format!("on the horizon behind, the small silhouette of {}, {}", article(&n), tier_glow(g.tier)),
        (_, ItemKind::Phenomenon) => format!("churning in the sky behind, {}", article(&n)),
        (_, ItemKind::Other) => format!("tucked into the belt, {p}"),
    }
}

/// Where a piece sits on the miniature in a diorama.
fn placed(role: &str, g: &Gear) -> String {
    let n = noun(g);
    let p = phrase(g);
    match (role, g.kind) {
        ("weapon", _) => format!("raised in its right hand, {p}"),
        ("armor", _) => format!("as blocky plates over its torso and shoulders, {p}"),
        ("charm", _) => format!("as a small bright bead at its chest, {p}"),
        (_, ItemKind::Army) => format!("at the tile's edge, the banner of {}: a flat block on a thin pole over a row of tiny white pillar soldiers", article(&n)),
        (_, ItemKind::Fleet) => format!("at the tile's edge, {}: three tiny blocky ships", article(&n)),
        (_, ItemKind::Structure) => format!("at the back of the tile, {}: a small voxel building", article(&n)),
        (_, ItemKind::Creature) => format!("beside it, {}: a small faceted beast", article(&n)),
        (_, ItemKind::Phenomenon) => format!("above the tile, {}: a small swirl of light", article(&n)),
        _ => format!("as a small piece on the tile beside it, {p}"),
    }
}

/// A shown piece: its role and its descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shown {
    pub role: String,
    #[serde(flatten)]
    pub gear: Gear,
}

/// The portrait subject of `name` with `gear`: the identity sheet exactly as the base portrait has it, then the gear
/// clause (what is held in which hand, what is worn, materials and glow). No stats, no ids.
pub fn portrait_subject(name: &str, gear: &[Shown]) -> String {
    let base = crate::payload::portrait_prompt_for(name);
    let clause: Vec<String> = gear.iter().map(|s| worn(&s.role, &s.gear)).collect();
    format!("{base}; newly geared: {}; the same face, build, clothes and colours as before, with only this gear added", clause.join("; "))
}

/// The diorama subject of `name` with `gear`: the base diorama's words, then the gear as chunky miniature pieces.
pub fn diorama_subject(name: &str, gear: &[Shown]) -> String {
    let base = crate::payload::diorama_prompt_for(name);
    let clause: Vec<String> = gear.iter().map(|s| placed(&s.role, &s.gear)).collect();
    format!(
        "{base}; the miniature now carries its gear as chunky faceted low-poly pieces: {}; the same figure and tile as before, with only this gear added",
        clause.join("; ")
    )
}

/// The subject of an item's own picture, from its descriptor alone (so the same descriptor is the same picture).
pub fn item_subject(g: &Gear) -> String {
    let n = noun(g);
    let what = match g.kind {
        ItemKind::Weapon => "a demigod's weapon",
        ItemKind::Armor => "a demigod's armour",
        ItemKind::Relic => "a holy relic",
        ItemKind::Artifact => "a magic artifact",
        ItemKind::Structure => "a miniature voxel building",
        ItemKind::Army => "a flat banner block on a thin pole above a square of tiny white pillar soldiers",
        ItemKind::Fleet => "three tiny blocky ships on a ripple of blue tile",
        ItemKind::Creature => "a small faceted low-poly beast",
        ItemKind::Phenomenon => "a small swirl of weather and light held above the tile",
        ItemKind::Other => "a curious keepsake",
    };
    let mut s = article(&n);
    let look = material_look(&g.material);
    if !look.is_empty() {
        s.push_str(&format!(" of {look}"));
    }
    s.push_str(&format!(": {what}, {}", tier_glow(g.tier)));
    let sig = signature(&g.blessing);
    if !sig.is_empty() {
        s.push_str(&format!(", {sig}"));
    }
    s
}

// ---------------------------------------------------------------------------------------------------------------------
// The state
// ---------------------------------------------------------------------------------------------------------------------

/// An earlier look of a demigod.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Past {
    pub loadout: String,
    pub since_beat: u32,
    pub until_beat: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gear: Vec<Shown>,
}

/// A demigod's look record: the loadout it holds and the image identity it shows.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Look {
    pub demigod: DemigodId,
    /// The hash of the loadout it holds now (`""`: nothing notable).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub held: String,
    /// The hash of the loadout its images show (`""`: the base identity images).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub loadout: String,
    /// The beat its images became these.
    #[serde(default)]
    pub since_beat: u32,
    /// The pieces its images show (at most `look_items_shown`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gear: Vec<Shown>,
    /// sha256 of its portrait subject and of its diorama subject (`""`: the base images).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub portrait: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub diorama: String,
    /// Every line of the pictured loadout (what a later loadout is compared with to say what was gained).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<String>,
    /// The gear pictures made for it this match (portrait subject hashes; the cap counts these).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub made: Vec<String>,
    /// The beat of its newest new picture (the debounce counts from it; 0: none yet).
    #[serde(default, skip_serializing_if = "is0")]
    pub made_beat: u32,
    /// Its earlier looks, oldest first (the newest [`PAST_KEPT`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub past: Vec<Past>,
}

fn is0(x: &u32) -> bool {
    *x == 0
}

/// Everything gear looks keep in the state (`World.dm.looks`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LookBook {
    /// One record per demigod that ever held a loadout, in id order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub looks: Vec<Look>,
    /// The item pictures granted this match (art keys, in the order granted).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub art: Vec<String>,
}

pub fn book(w: &World) -> LookBook {
    if w.dm.looks.is_null() {
        return LookBook::default();
    }
    serde_json::from_value(w.dm.looks.clone()).unwrap_or_default()
}

pub fn save(w: &mut World, b: &LookBook) {
    w.dm.looks = if *b == LookBook::default() { Value::Null } else { json!(b) };
}

/// Are gear looks on in this world (cheap: no settings parse)? The room setting, on a schema-11+ state.
pub fn on(w: &World) -> bool {
    w.look_rules() && w.meta.settings.get("looks_enabled").and_then(Value::as_bool) == Some(true)
}

fn human(w: &World, d: &Demigod) -> bool {
    crate::table::controller(w, d) == "human"
}

// ---------------------------------------------------------------------------------------------------------------------
// The beat's step
// ---------------------------------------------------------------------------------------------------------------------

/// One look change of a beat (a `look.changed` event).
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub demigod: DemigodId,
    /// The best item newly in the loadout (`None`: something was lost, or only the picture caught up).
    pub gained: Option<ItemId>,
    /// Did its images change this beat?
    pub image_change: bool,
    /// Why the images stayed: `debounced` | `capped` (`None` when they changed).
    pub kept: Option<&'static str>,
    /// The DM memory entry.
    pub memory: u32,
}

/// The end of the beat for looks (after the DM's step and the gear's upkeep): every demigod's held loadout is noted;
/// a human-controlled, standing demigod whose loadout is not the one its images show takes a new look (within the cap
/// and the debounce) and the change is remembered (`look.changed`, public). Then item art is granted.
pub fn step(w: &mut World, beat: u32, st: &Settings) -> Vec<Change> {
    let mut out = vec![];
    if !st.looks_enabled || !w.look_rules() {
        return out;
    }
    let mut b = book(w);
    let before = b.clone();
    let ids: Vec<DemigodId> = w.demigods.iter().map(|d| d.id).collect();
    for d in ids {
        let lo = loadout(w, d, st.look_extra_items as usize);
        let h = loadout_hash(&lo);
        let ix = match b.looks.iter().position(|l| l.demigod == d) {
            Some(ix) => ix,
            None if h.is_empty() => continue,
            None => {
                let at = b.looks.iter().position(|l| l.demigod > d).unwrap_or(b.looks.len());
                b.looks.insert(at, Look { demigod: d, ..Default::default() });
                at
            }
        };
        let dg = &w.demigods[d.ix()];
        if !dg.alive {
            // A fallen demigod keeps the look it fell with (its gear lies on the ground; its picture is its memorial).
            continue;
        }
        let rec = &mut b.looks[ix];
        let held_changed = rec.held != h;
        rec.held = h.clone();
        if !human(w, dg) {
            continue;
        }
        if rec.loadout == h {
            continue;
        }
        let lines: Vec<String> = lo.iter().map(Entry::line).collect();
        let gained = lo
            .iter()
            .filter(|e| !rec.lines.contains(&e.line()))
            .max_by(|a, x| a.gear.tier.cmp(&x.gear.tier).then(x.item.cmp(&a.item)))
            .map(|e| e.item);
        let shown: Vec<Shown> =
            lo.iter().take(st.look_items_shown.max(1) as usize).map(|e| Shown { role: e.role.to_string(), gear: e.gear.clone() }).collect();
        let name = dg.name.clone();
        let (portrait, diorama) = if shown.is_empty() {
            (String::new(), String::new())
        } else {
            (sha(&[&portrait_subject(&name, &shown)]), sha(&[&diorama_subject(&name, &shown)]))
        };
        let mut kept = None;
        if !portrait.is_empty() && !rec.made.contains(&portrait) {
            if rec.made.len() >= st.look_images_per_demigod as usize {
                kept = Some("capped");
            } else if rec.made_beat != 0 && beat < rec.made_beat + st.look_debounce_beats {
                kept = Some("debounced");
            }
        }
        if kept.is_some() && !held_changed {
            // Still waiting (or capped) on a loadout already announced.
            continue;
        }
        if kept.is_none() {
            if !rec.loadout.is_empty() || !rec.past.is_empty() {
                rec.past.push(Past { loadout: rec.loadout.clone(), since_beat: rec.since_beat, until_beat: beat, gear: std::mem::take(&mut rec.gear) });
                if rec.past.len() > PAST_KEPT {
                    let cut = rec.past.len() - PAST_KEPT;
                    rec.past.drain(..cut);
                }
            }
            if !portrait.is_empty() && !rec.made.contains(&portrait) {
                rec.made.push(portrait.clone());
                rec.made_beat = beat;
            }
            rec.loadout = h.clone();
            rec.since_beat = beat;
            rec.gear = shown;
            rec.portrait = portrait;
            rec.diorama = diorama;
            rec.lines = lines;
        }
        let image_loadout = rec.loadout.clone();
        let pos = dg.pos;
        let wh = crate::dm::locate(w, pos, HexLevel::People);
        let gained_name = gained.and_then(|i| w.items.get(i.ix())).map(|i| i.name.clone());
        let text = match (&gained_name, kept) {
            (Some(g), None) => format!("{name} now carries the {g}"),
            (Some(g), Some(_)) => format!("{name} takes up the {g}"),
            (None, _) if h.is_empty() => format!("{name} stands bare-handed again"),
            (None, _) => format!("{name}'s gear has changed"),
        };
        event("look_changed", Some(Ref::Demigod(d).to_string()), json!({ "beat": beat, "loadout": h, "image_loadout": image_loadout, "kept": kept }));
        let memory = crate::dm::remember(w, beat, &wh, crate::dm::Decision {
            scope: Scope::Cell,
            kind: "look.changed",
            query: Some(format!("look:{}:{beat}", d.0)),
            public: true,
            text,
            data: json!({ "demigod": Ref::Demigod(d), "name": name, "loadout_hash": h, "image_loadout_hash": image_loadout,
                          "gained": gained.map(Ref::Item), "gained_name": gained_name, "image_change": kept.is_none(),
                          "kept": kept, "at": pos, "cell": wh.cell }),
        });
        out.push(Change { demigod: d, gained, image_change: kept.is_none(), kept, memory });
    }
    grant_art(w, &mut b, st);
    if b != before {
        save(w, &b);
    }
    out
}

/// Is `it` notable enough for a picture of its own: relic tier and up, god-blessed, or the first of its kind its
/// (human) player holds?
fn notable(w: &World, it: &Item) -> bool {
    if it.ext.tier >= Tier::Relic || arms::blessed_by(w, it).is_some() {
        return true;
    }
    let Some(Ref::Demigod(d)) = it.holders.last().copied() else { return false };
    let Some(dg) = w.demigods.get(d.ix()) else { return false };
    if !human(w, dg) {
        return false;
    }
    !w.items.iter().any(|x| {
        x.id < it.id
            && x.ext.kind == it.ext.kind
            && x.ext.state != ItemState::Destroyed
            && matches!(x.holders.last(), Some(Ref::Demigod(h)) if w.same_player(*h, d))
    })
}

/// Grant item pictures (in item order, up to `item_art_per_match`): notable items a human's demigod holds, and
/// relic-tier or blessed items lying in a loot pile.
fn grant_art(w: &World, b: &mut LookBook, st: &Settings) {
    let cap = st.item_art_per_match as usize;
    for it in &w.items {
        if b.art.len() >= cap {
            break;
        }
        if it.ext.state == ItemState::Destroyed {
            continue;
        }
        let on_ground = w.dm.loot.iter().any(|l| l.taken_beat.is_none() && l.items.contains(&it.id));
        let by_human = matches!(it.holders.last(), Some(Ref::Demigod(d)) if w.demigods.get(d.ix()).is_some_and(|x| human(w, x)));
        let wanted = if on_ground { it.ext.tier >= Tier::Relic || arms::blessed_by(w, it).is_some() } else { by_human && notable(w, it) };
        if !wanted {
            continue;
        }
        let key = gear_of(w, it).art_key();
        if !b.art.contains(&key) {
            b.art.push(key);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Payloads
// ---------------------------------------------------------------------------------------------------------------------

/// The art fields of an item in a payload: with a picture granted, `image_prompt` (the descriptor's subject),
/// `image_kind: "item"` and `art` (its key); without, no image fields at all (the client shows its `icon`).
pub fn art_fields(w: &World, b: &LookBook, it: &Item, v: &mut Value) {
    let g = gear_of(w, it);
    let key = g.art_key();
    let Some(o) = v.as_object_mut() else { return };
    if b.art.contains(&key) {
        o.insert("image_prompt".into(), item_subject(&g).into());
        o.insert("image_kind".into(), "item".into());
        o.insert("art".into(), key.into());
    } else {
        o.remove("image_prompt");
        o.remove("image_kind");
    }
}

fn entry_json(w: &World, b: &LookBook, e: &Entry) -> Value {
    let Some(it) = w.items.get(e.item.ix()) else { return Value::Null };
    let mut v = json!({
        "slot": e.role, "item": Ref::Item(it.id), "name": it.name, "icon": icon_of(it.ext.kind, &it.name),
        "tier": it.ext.tier, "kind": it.ext.kind,
        "material": (!e.gear.material.is_empty()).then(|| e.gear.material.clone()),
        "blessed_by": arms::blessed_by(w, it).map(|g| w.name_of(Ref::God(g))),
    });
    art_fields(w, b, it, &mut v);
    v
}

fn subjects(name: &str, loadout: &str, gear: &[Shown]) -> Value {
    json!({ "loadout_hash": loadout, "portrait_subject": portrait_subject(name, gear), "diorama_subject": diorama_subject(name, gear) })
}

/// The media layer's inputs for `d`'s look: the gear subjects (`null`: the base images), the base identity subjects
/// (the first reference image of a gear picture) and the earlier gear looks, newest first (stand-ins while the new
/// one renders).
fn image_fields(d: &Demigod, rec: Option<&Look>, v: &mut Value) {
    let name = &d.name;
    v["base_portrait"] = crate::payload::portrait_prompt_for(name).into();
    v["base_diorama"] = crate::payload::diorama_prompt_for(name).into();
    match rec.filter(|r| !r.gear.is_empty()) {
        Some(r) => {
            v["portrait_subject"] = portrait_subject(name, &r.gear).into();
            v["diorama_subject"] = diorama_subject(name, &r.gear).into();
        }
        None => {
            v["portrait_subject"] = Value::Null;
            v["diorama_subject"] = Value::Null;
        }
    }
    let fallback: Vec<Value> = rec
        .map(|r| r.past.iter().rev().filter(|p| !p.gear.is_empty()).take(FALLBACKS).map(|p| subjects(name, &p.loadout, &p.gear)).collect())
        .unwrap_or_default();
    v["fallback"] = fallback.into();
}

/// A demigod's `look`: `{loadout: [{slot, item, name, icon, tier, kind, material, blessed_by, image_prompt?}],
/// loadout_hash, image_loadout_hash, since_beat, gear, looks_made, looks_cap}` plus the media layer's inputs
/// ([`image_fields`]; the server replaces them with `portrait_image`, `diorama_image` and `pending`).
pub fn look_json(w: &World, d: &Demigod) -> Value {
    let st = crate::world::settings(w);
    let b = book(w);
    let lo = loadout(w, d.id, st.look_extra_items as usize);
    let rec = b.looks.iter().find(|l| l.demigod == d.id);
    let mut v = json!({
        "loadout": lo.iter().map(|e| entry_json(w, &b, e)).collect::<Vec<_>>(),
        "loadout_hash": loadout_hash(&lo),
        "image_loadout_hash": rec.map(|r| r.loadout.clone()).unwrap_or_default(),
        "since_beat": rec.map(|r| r.since_beat).unwrap_or(0),
        "gear": rec.is_some_and(|r| !r.gear.is_empty()),
        "looks_made": rec.map(|r| r.made.len()).unwrap_or(0),
        "looks_cap": st.look_images_per_demigod,
    });
    image_fields(d, rec, &mut v);
    v
}

/// The public `look.changed` events of this beat's changes, complete (the loadout, what was gained and the media
/// layer's inputs): `{event, demigod, name, player, beat, loadout, loadout_hash, image_loadout_hash, gained,
/// image_change, kept, text, memory, ...}`.
pub fn events_json(w: &World, beat: u32, changes: &[Change]) -> Vec<Value> {
    let b = book(w);
    changes
        .iter()
        .filter_map(|c| {
            let d = w.demigods.get(c.demigod.ix())?;
            let mut v = look_json(w, d);
            let o = v.as_object_mut()?;
            o.remove("looks_made");
            o.remove("looks_cap");
            v["event"] = "look.changed".into();
            v["demigod"] = json!(Ref::Demigod(d.id));
            v["name"] = d.name.clone().into();
            v["player"] = d.player_name().into();
            v["beat"] = beat.into();
            v["gained"] = match c.gained.and_then(|i| w.items.get(i.ix())) {
                Some(it) => {
                    let mut g = json!({ "item": Ref::Item(it.id), "name": it.name, "icon": icon_of(it.ext.kind, &it.name),
                                        "tier": it.ext.tier, "kind": it.ext.kind });
                    art_fields(w, &b, it, &mut g);
                    g
                }
                None => Value::Null,
            };
            v["image_change"] = c.image_change.into();
            v["kept"] = json!(c.kept);
            let e = w.dm.memory.iter().rev().find(|e| e.id == c.memory);
            v["text"] = e.map(|e| e.text.clone()).unwrap_or_default().into();
            v["memory"] = c.memory.into();
            v["at"] = json!(d.pos);
            Some(v)
        })
        .collect()
}

/// A demigod's looks so far, oldest first (the admin's and the profile's timeline): `[{loadout_hash, since_beat,
/// until_beat, gear: [names]}]`, ending with the current one (`until_beat: null`).
pub fn history_json(w: &World, d: DemigodId) -> Vec<Value> {
    let b = book(w);
    let Some(rec) = b.looks.iter().find(|l| l.demigod == d) else { return vec![] };
    let names = |g: &[Shown]| g.iter().map(|s| s.gear.name.clone()).collect::<Vec<_>>();
    let mut out: Vec<Value> = rec
        .past
        .iter()
        .map(|p| json!({ "loadout_hash": p.loadout, "since_beat": p.since_beat, "until_beat": p.until_beat, "gear": names(&p.gear) }))
        .collect();
    out.push(json!({ "loadout_hash": rec.loadout, "since_beat": rec.since_beat, "until_beat": null, "gear": names(&rec.gear) }));
    out
}

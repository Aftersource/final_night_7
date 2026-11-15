//! Duel weapons (state schema 14; docs/DUEL_MOVES.md §4.4-4.6): weapon classes, hammer-space draws and projectiles.
//!
//! - **Weapon classes.** Every wieldable item has a `weapon_class` (`unarmed | one_handed | two_handed | bow | rifle |
//!   launcher | thrown | staff | shield`), derived by rules from its kind, name and material ([`weapon_class`]:
//!   "rocket launchers" → launcher, "longbow" → bow, "greatsword" / "hammer" / "axe" → two_handed). A duellist fights
//!   with its best held weapon ([`best`]); a move class plays that weapon class's clips (the list's `weapons` sets,
//!   else the generic ones); its segs carry the weapon as a `prop` so the client puts a mesh in the hand. Ranged
//!   weapons are strong at far and mid range (a strike from far is a shot, not a step in; +0.5 damage) and weak in a
//!   grapple (half damage).
//! - **Hammer-space** ("players can grab from hammer-space"). Pulling out a HELD item mid-duel is free (a
//!   `draw_from_nowhere` setup seg; the fighter wields it from then on). Words that pull out something NOT held ("i
//!   pull out a rocket launcher") are a manifestation judged by the item engine's rules inside the duel, as a setup
//!   move ([`draw`]): its price in sparks (`duel_draw_sparks`: paid, the prop is artifact tier; unpaid, trinket tier),
//!   the engine's odds (tier, Death's favor, Entropy, the draws already made) turned into a d20's target, a seeded
//!   d20. Success: a duel prop that sets the weapon class for the rest of the duel. Failure: a fumble (the initiative
//!   flips to the foe before the blow). At most `duel_draws` hammer-space draws per fighter per duel.
//! - **Projectiles** ([`projectiles`]): a ranged blow adds a few dozen bytes to its panel: what flies, from whom, to
//!   whom (or to the scene object or hex a miss lands on), when it is fired and lands, its arc and speed, and what the
//!   hit does (`on_hit {blow, impulse, burst, aoe_hexes, objects}`). All decided by the blow's verdict and seeded. The
//!   duel-sim crate consumes `on_hit.impulse`, `aoe_hexes` and `objects[].impulse`: these names are stable.

use fn7_model::manifest::{ItemKind, ItemState};
use fn7_model::seed::{hash_str, Rng};
use fn7_model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::choreo::{MoveClass, Range};
use crate::duel::Bout;

/// Ranged weapon classes (strong at far and mid range, weak in a grapple).
pub const RANGED: &[&str] = &["bow", "rifle", "launcher", "thrown"];

fn words(s: &str) -> Vec<String> {
    s.to_lowercase().split(|c: char| !(c.is_alphanumeric() || c == '-')).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

/// A word list match: a word equal to one of `keys`, or ending with one marked `*` (`*bow`: "longbow").
fn any(ws: &[String], keys: &[&str]) -> bool {
    ws.iter().any(|w| keys.iter().any(|k| match k.strip_prefix('*') {
        Some(end) => w.ends_with(end) && w.len() > end.len() || w == end,
        None => w == k || (w.ends_with('s') && &w[..w.len() - 1] == *k),
    }))
}

/// The weapon class an item's words say, by rules: `None` when the words name no weapon.
pub fn class_of_words(text: &str) -> Option<&'static str> {
    let t = text.to_lowercase();
    let ws = words(&t);
    let phrase = |ps: &[&str]| ps.iter().any(|p| t.contains(p));
    if phrase(&["rocket launcher", "grenade launcher", "missile launcher"]) || any(&ws, &["launcher", "bazooka", "rpg", "cannon", "mortar", "*launcher"]) {
        return Some("launcher");
    }
    if any(&ws, &["bow", "longbow", "shortbow", "crossbow", "warbow", "greatbow", "recurve", "arbalest"]) {
        return Some("bow");
    }
    if any(&ws, &["rifle", "gun", "*gun", "pistol", "musket", "revolver", "shotgun", "blunderbuss", "carbine", "firearm", "flintlock", "blaster", "sniper", "uzi", "minigun"]) {
        return Some("rifle");
    }
    if phrase(&["throwing "]) || any(&ws, &["javelin", "shuriken", "dart", "bomb", "grenade", "sling", "boomerang", "chakram", "bolas", "rock", "stone", "brick", "kunai"]) {
        return Some("thrown");
    }
    if any(&ws, &["staff", "wand", "sceptre", "scepter", "rod", "stave", "quarterstaff"]) {
        return Some("staff");
    }
    if any(&ws, &["shield", "buckler", "aegis", "pavise"]) {
        return Some("shield");
    }
    if phrase(&["hand axe", "throwing axe"]) || any(&ws, &["hatchet", "tomahawk"]) {
        return Some("one_handed");
    }
    if phrase(&["great sword", "war hammer", "battle axe", "war axe"])
        || any(&ws, &["greatsword", "claymore", "zweihander", "*hammer", "maul", "axe", "*axe", "halberd", "glaive", "scythe", "polearm", "pike", "lance", "spear", "greataxe", "flamberge"])
    {
        return Some("two_handed");
    }
    if any(&ws, &["sword", "*sword", "blade", "dagger", "knife", "sabre", "saber", "rapier", "mace", "club", "cutlass", "katana", "scimitar", "whip", "dirk",
                  "flail", "cudgel", "stiletto", "machete", "trident", "sickle", "falchion", "gladius", "tanto", "cleaver"])
    {
        return Some("one_handed");
    }
    None
}

/// An item's weapon class (`None`: not wieldable): its words first; a weapon whose words name none is one-handed;
/// armour, relics and artifacts only when their words name a weapon (a shield, a staff, a cursed sword).
pub fn weapon_class(kind: ItemKind, name: &str) -> Option<&'static str> {
    match kind {
        ItemKind::Weapon => Some(class_of_words(name).unwrap_or("one_handed")),
        ItemKind::Armor | ItemKind::Relic | ItemKind::Artifact | ItemKind::Other => class_of_words(name),
        _ => None,
    }
}

/// How good a weapon class is to fight with (a duellist wields its best: tier first, then this).
fn rank(class: &str) -> usize {
    ["shield", "thrown", "one_handed", "staff", "two_handed", "bow", "rifle", "launcher"].iter().position(|c| *c == class).map(|i| i + 1).unwrap_or(0)
}

/// Something a duellist holds or wields: a held item, or a hammer-space prop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Wieldable {
    /// The world item (`None`: a hammer-space prop, which exists only in the duel).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<ItemId>,
    pub name: String,
    /// The item kind (`weapon`, `armor`, ...).
    pub kind: String,
    pub weapon_class: String,
    pub material: String,
    pub tier: String,
    /// Pulled out of hammer-space.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hammer: bool,
    /// Schema 21: the colour its name gives (`obsidian` → black: [`crate::duel_words::colour_of`]) and its hex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour_hex: Option<String>,
}

impl Wieldable {
    /// The seg `prop` (docs/CLIENT_API.md "Duel weapons"): what the client puts in the hand.
    pub fn prop(&self) -> Value {
        let mut v = json!({ "item": self.item.map(Ref::Item), "name": self.name, "kind": self.kind, "weapon_class": self.weapon_class,
                "material": self.material, "tier": self.tier, "hammer": self.hammer });
        if let Some(c) = &self.colour {
            v["colour"] = c.clone().into();
            v["colour_hex"] = self.colour_hex.clone().into();
        }
        v
    }
    pub fn ranged(&self) -> bool {
        RANGED.contains(&self.weapon_class.as_str())
    }
}

/// What a demigod holds that it could wield, best first.
pub fn held(w: &World, d: DemigodId) -> Vec<Wieldable> {
    let me = Ref::Demigod(d);
    let mut out: Vec<(usize, usize, Wieldable)> = w
        .items
        .iter()
        .filter(|i| i.holders.last() == Some(&me) && i.ext.state == ItemState::Held && i.ext.muster.is_none())
        .filter_map(|i| {
            let wc = weapon_class(i.ext.kind, &i.name)?;
            let kind = serde_json::to_value(i.ext.kind).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            let mut material = crate::wills::arms::material_of(w.meta.seed, &i.name).0.to_string();
            // Schema 21: the colour (and a lab material) its name gives.
            let c = w.duel_words_rules().then(|| crate::duel_words::colour_of(&i.name)).flatten();
            if let Some(c) = c {
                material = c.0.to_string();
            }
            Some((i.ext.tier.index(), rank(wc), Wieldable { item: Some(i.id), name: i.name.clone(), kind, weapon_class: wc.into(), material,
                                                            tier: i.ext.tier.as_str().into(), hammer: false,
                                                            colour: c.map(|c| c.1.to_string()), colour_hex: c.map(|c| c.2.to_string()) }))
        })
        .collect();
    out.sort_by(|a, b| (b.0, b.1).cmp(&(a.0, a.1)));
    out.into_iter().map(|x| x.2).collect()
}

/// The best of what it holds (what it wields at the lock).
pub fn best(held: &[Wieldable]) -> Option<Wieldable> {
    held.first().cloned()
}

/// A side's weapon class now (`unarmed` with nothing in hand).
pub fn class_of_side(b: &Bout, side: usize) -> &str {
    b.wield.get(side).and_then(|x| x.as_ref()).map(|x| x.weapon_class.as_str()).unwrap_or("unarmed")
}

/// Ranged weapons fight from far: a strike or heavy from far range stays what it is (a shot).
pub fn effective(class: MoveClass, asked: MoveClass, range: Range, weapon: &str) -> MoveClass {
    if range == Range::Far && RANGED.contains(&weapon) && matches!(asked, MoveClass::Strike | MoveClass::Heavy) && class == MoveClass::CloseIn {
        return asked;
    }
    class
}

/// The weapon's effect on a landed attack's damage: ranged +0.5 at far and mid, half in a grapple; a two-handed
/// heavy +0.5. `(mult, bonus)`.
pub fn damage(weapon: &str, class: MoveClass, range: Range) -> (f32, f32) {
    if !matches!(class, MoveClass::Strike | MoveClass::Heavy) {
        return (1.0, 0.0);
    }
    match weapon {
        w if RANGED.contains(&w) && range == Range::Grapple => (0.5, 0.0),
        w if RANGED.contains(&w) && range <= Range::Mid => (1.0, 0.5),
        "two_handed" if class == MoveClass::Heavy => (1.0, 0.5),
        _ => (1.0, 0.0),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Hammer-space
// ---------------------------------------------------------------------------------------------------------------------

const DRAW_WORDS: &[&str] = &[
    "pull out", "pulls out", "pulled out", "whip out", "whips out", "take out", "takes out", "bring out", "draw out", "out comes", "produce",
    "produces", "unsheathe", "unsheathes", "unsling", "unslings", "brandish", "brandishes", "from hammerspace", "from hammer-space",
    "from hammer space", "out of nowhere", "from nowhere", "conjure up", "reach behind", "draw", "draws", "drew", "ready my", "pull my", "grab my",
];

/// The thing a line pulls out, if it pulls one out: the words after a draw word (up to five), when they name a
/// weapon. `(the words, their weapon class)`.
pub fn draw_words(text: &str) -> Option<(String, &'static str)> {
    let t = text.to_lowercase();
    let mut best: Option<(usize, &str)> = None;
    for k in DRAW_WORDS {
        if let Some(at) = t.find(k) {
            // A whole word (not "drawer"), the earliest.
            let end = at + k.len();
            let ok_before = at == 0 || !t[..at].chars().last().is_some_and(char::is_alphanumeric);
            let ok_after = t[end..].chars().next().is_none_or(|c| !c.is_alphanumeric());
            if ok_before && ok_after && best.is_none_or(|b| at < b.0) {
                best = Some((at, k));
            }
        }
    }
    let (at, k) = best?;
    let rest: Vec<String> = words(&t[at + k.len()..]).into_iter().filter(|w| !["a", "an", "the", "my", "his", "her", "out", "of", "from", "nowhere", "hammerspace"].contains(&w.as_str())).take(5).collect();
    // The weapon words before any verb that follows ("a rocket launcher and fire").
    let mut name = vec![];
    for w in rest {
        if ["and", "then", "to", "at", "fire", "shoot", "strike", "swing", "hit"].contains(&w.as_str()) {
            break;
        }
        name.push(w);
    }
    let name = name.join(" ");
    let class = class_of_words(&name)?;
    Some((name, class))
}

/// Which held item the words mean: one whose name shares a word with them, else one of the same weapon class.
pub fn held_match<'a>(held: &'a [Wieldable], name: &str, class: &str) -> Option<&'a Wieldable> {
    let ws = words(name);
    held.iter()
        .find(|h| words(&h.name).iter().any(|x| x.len() > 3 && ws.contains(x)))
        .or_else(|| held.iter().find(|h| h.weapon_class == class))
}

/// A draw's outcome (in `duel.blow.move.draw`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Draw {
    /// The words drawn ("rocket launcher").
    pub thing: String,
    pub weapon_class: String,
    /// A held item: free, always works.
    pub held: bool,
    /// Hammer-space: the d20, the target it had to reach, the engine's probability, the sparks paid, the tier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roll: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub need: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p: Option<f64>,
    #[serde(default)]
    pub sparks: u32,
    pub tier: String,
    /// `drawn` (held), `success`, `fumble`, or `spent` (no draws left: the line is read without it).
    pub verdict: String,
}

/// The engine's odds for a hammer-space prop turned into a d20 target: the item engine's success probability for its
/// tier (Death's favor toward the drawer, the world's Entropy, the draws it already made this duel as repeats), and a
/// duel's pressure (+6: drawing under fire is harder than in a will).
pub fn draw_target(w: &World, d: DemigodId, paid: bool, recent: u32) -> (f64, i32, fn7_model::manifest::Tier) {
    use fn7_model::manifest::{odds, Cost, OddsInputs, Tier};
    let tier = if paid { Tier::Artifact } else { Tier::Trinket };
    let favor = w.gods.get(ItemKind::Weapon.patron().ix()).and_then(|g| g.favor.get(d.ix()).copied()).unwrap_or(0.0);
    let o = odds(&OddsInputs { tier, cost: Cost::default(), favor, stance: None, entropy: w.meta.entropy, recent });
    let need = ((20.0 * (1.0 - o.success)).ceil() as i32 + 6).clamp(2, 20);
    (o.success, need, tier)
}

/// The seeded d20 of a hammer-space draw.
pub fn draw_roll(seed: u64, query: &str) -> i32 {
    (Rng::derive(seed, &[0xD4A3, hash_str(query)]).below(20) + 1) as i32
}

// ---------------------------------------------------------------------------------------------------------------------
// Projectiles
// ---------------------------------------------------------------------------------------------------------------------

/// What a ranged class shoots, its speed (m/s) and its arc (apex above the line, m).
pub fn missile(kind: &str) -> (f64, f64) {
    match kind {
        "arrow" => (32.0, 0.5),
        "bullet" => (140.0, 0.0),
        "rocket" => (24.0, 0.3),
        "rock" => (14.0, 1.0),
        "orb" => (12.0, 0.2),
        // Schema 21: a handful of the ground (dirt, sand, mud, snow, ash) flung at the face.
        "dirt" => (10.0, 0.4),
        _ => (200.0, 0.0), // bolt
    }
}

/// Metres between two stage hexes (pointy-top, `x = 1.732 (q + r/2)`, `z = 1.5 r`).
pub fn metres(a: [i32; 2], b: [i32; 2]) -> f64 {
    let x = |h: [i32; 2]| (1.732 * (h[0] as f64 + h[1] as f64 / 2.0), 1.5 * h[1] as f64);
    let (pa, pb) = (x(a), x(b));
    ((pa.0 - pb.0).powi(2) + (pa.1 - pb.1).powi(2)).sqrt()
}

/// Everything [`projectile`] needs about one staged ranged blow.
pub struct Shot<'a> {
    pub id: String,
    pub kind: &'a str,
    pub by: usize,
    pub blow: u32,
    pub from: [i32; 2],
    pub foe: [i32; 2],
    pub t_fire_ms: i64,
    /// The blow's damage, its matrix result, its verdict's `ok`.
    pub damage: f64,
    pub result: &'a str,
    /// The defender wields a shield.
    pub foe_shield: bool,
    pub burst: &'a str,
}

fn side(s: usize) -> &'static str {
    if s == 0 { "a" } else { "b" }
}

fn hex_dist(a: [i32; 2], b: [i32; 2]) -> i32 {
    let (dq, dr) = (a[0] - b[0], a[1] - b[1]);
    (dq.abs() + dr.abs() + (dq + dr).abs()) / 2
}

fn obj_at(o: &Value) -> [i32; 2] {
    [o["at"][0].as_i64().unwrap_or(0) as i32, o["at"][1].as_i64().unwrap_or(0) as i32]
}

/// One projectile entry, decided by the blow's outcome and seeded: a hit flies to the foe; a parry or a shield's
/// guard deflects it (`deflected_by`, landing on a hex off to the side); a miss lands on a scene object near the foe
/// or a hex beyond it. A rocket bursts: the hexes around where it lands (`aoe_hexes`) and the scene objects in them
/// (`objects[].impulse`).
pub fn projectile(s: &Shot, objects: &[Value], r: &mut Rng) -> Value {
    let (speed, arc) = missile(s.kind);
    let hit = s.damage > 0.0;
    let deflected = !hit && (s.result == "parried" || (s.result == "absorbed" && s.foe_shield));
    let away = |p: [i32; 2], k: i32| {
        let dir = if s.from[0] <= p[0] { 1 } else { -1 };
        [p[0] + dir * k, p[1]]
    };
    let (to, land): (Value, [i32; 2]) = if hit {
        (json!(side(1 - s.by)), s.foe)
    } else if deflected {
        let h = [s.foe[0], s.foe[1] + if r.below(2) == 0 { 2 } else { -2 }];
        (json!(h), h)
    } else {
        let near: Vec<&Value> = objects.iter().filter(|o| hex_dist(obj_at(o), s.foe) <= 3).collect();
        if !near.is_empty() && r.below(3) > 0 {
            let o = near[r.below(near.len() as u64) as usize];
            (o["id"].clone(), obj_at(o))
        } else {
            let h = away(s.foe, 2 + r.below(2) as i32);
            (json!(h), h)
        }
    };
    let dist = metres(s.from, if deflected { s.foe } else { land }).max(1.0);
    let flight = ((dist / speed) * 1000.0).round().max(60.0) as i64;
    let impulse = if hit { (s.damage / 4.0).clamp(0.1, 1.0) } else { 0.5 };
    let mut on_hit = json!({ "blow": s.blow, "impulse": (impulse * 100.0).round() / 100.0, "burst": s.burst });
    if s.kind == "rocket" {
        // A small blast: the landing hex and its six neighbours; scene objects in them are knocked.
        let mut hexes = vec![land];
        for d in [[1, 0], [-1, 0], [0, 1], [0, -1], [1, -1], [-1, 1]] {
            hexes.push([land[0] + d[0], land[1] + d[1]]);
        }
        let knocked: Vec<Value> = objects.iter().filter(|o| hexes.contains(&obj_at(o)))
            .map(|o| json!({ "id": o["id"], "impulse": if obj_at(o) == land { 1.0 } else { 0.6 } })).collect();
        on_hit["aoe_hexes"] = json!(hexes);
        on_hit["objects"] = json!(knocked);
        on_hit["burst"] = "KRA-THOOM".into();
    }
    json!({ "id": s.id, "kind": s.kind, "by": side(s.by), "from": side(s.by), "from_hex": s.from, "to": to, "to_hex": land,
            "t_fire_ms": s.t_fire_ms, "t_hit_ms": s.t_fire_ms + flight, "arc_h": arc, "speed": speed,
            "on_hit": on_hit, "miss": !hit, "deflected_by": deflected.then(|| side(1 - s.by)) })
}

/// The projectile a staged blow makes, if any: its fighter's weapon is ranged and the class attacks (a strike or a
/// heavy), or it is an invoke (an `orb` with a staff, else the god's `bolt`).
pub fn kind_for(weapon: &str, class: &str) -> Option<&'static str> {
    match class {
        "strike" | "heavy" => match weapon {
            "bow" => Some("arrow"),
            "rifle" => Some("bullet"),
            "launcher" => Some("rocket"),
            "thrown" => Some("rock"),
            _ => None,
        },
        "invoke" => Some(if weapon == "staff" { "orb" } else { "bolt" }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_by_rules() {
        for (name, kind, want) in [
            ("Twin Rocket Launchers", ItemKind::Weapon, Some("launcher")), ("a yew longbow", ItemKind::Weapon, Some("bow")),
            ("Moonsteel Greatsword", ItemKind::Weapon, Some("two_handed")), ("Thor's hammer", ItemKind::Relic, Some("two_handed")),
            ("a woodcutter's axe", ItemKind::Weapon, Some("two_handed")), ("hand axe", ItemKind::Weapon, Some("one_handed")),
            ("Obsidian Dagger", ItemKind::Weapon, Some("one_handed")), ("flintlock pistol", ItemKind::Weapon, Some("rifle")),
            ("a sling of river stones", ItemKind::Weapon, Some("thrown")), ("Staff of Tides", ItemKind::Artifact, Some("staff")),
            ("Dragonscale Shield", ItemKind::Armor, Some("shield")), ("Dragonscale Mail", ItemKind::Armor, None),
            ("a strange blade-thing", ItemKind::Weapon, Some("one_handed")), ("Glowing Thing", ItemKind::Weapon, Some("one_handed")),
            ("a rainbow", ItemKind::Phenomenon, None), ("elbow pads", ItemKind::Armor, None),
        ] {
            assert_eq!(weapon_class(kind, name), want, "{name}");
        }
        assert_eq!(draw_words("i pull out a rocket launcher and fire"), Some(("rocket launcher".into(), "launcher")));
        assert_eq!(draw_words("I draw my bow!"), Some(("bow".into(), "bow")));
        assert_eq!(draw_words("i draw a breath and punch"), None);
        assert_eq!(draw_words("kick him"), None);
        assert_eq!(draw_words("from hammerspace: a greatsword"), Some(("greatsword".into(), "two_handed")));
    }
}

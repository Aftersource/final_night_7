//! Arms and their powers (rules 0.9.0, DM_RULEBOOK chapter 24): what an item DOES.
//!
//! Every held weapon, armour, relic or artifact has concrete modifiers derived by rules from its kind, tier, material
//! and blessing ([`arm_of`]): a d20 modifier on strikes, journeys or deeds, damage added or turned aside, a duel bonus
//! (used by the 0.8.1 duel rules), a damage type resisted, a little regeneration, and from relic tier up one
//! once-per-match power. Nothing is stored for them (the item is the record: its kind, tier, name and the
//! manifestation that made it), except the powers already spent (`Book::used`). A demigod fights with its best item
//! per slot (weapon, armour, charm), and every total is capped ([`Kit`]), so gear is bounded.
//!
//! Where the numbers are used: act d20s ([`act_mods`], called by `dm::resolver_dice`), every damage, heal and revive
//! proposal on its way into the merge ([`shape`], called by `merge::run`), the duel's gear bonus (`duel::fighter`),
//! and the end of the beat ([`upkeep`]: regeneration, Second Wind; [`last_stand`]).

use fn7_model::manifest::{ItemKind, ItemState, Tier};
use fn7_model::seed::hash_str;
use fn7_model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::settings::Settings;
use crate::world::event;

/// The materials gear is made of: (name, the damage type it turns aside, journey bonus).
pub const MATERIALS: &[(&str, &str, i32)] = &[
    ("obsidian", "fire", 0),
    ("dragonscale", "fire", 0),
    ("butterfly-wing", "storm", 1),
    ("platinum", "curse", 0),
    ("struck-oak", "lightning", 0),
    ("moonsteel", "cold", 0),
    ("whalebone", "water", 0),
    ("salt-glass", "poison", 0),
    ("gold", "blade", 0),
    ("heron-feather", "fall", 1),
];

/// Words in a damage's cause that mark its type (what a material resists).
const TYPES: &[(&str, &[&str])] = &[
    ("fire", &["fire", "burn", "flame", "lava", "volcan", "scorch", "blaze", "meteor"]),
    ("storm", &["storm", "wind", "hurricane", "tornado", "gale", "squall"]),
    ("curse", &["curse", "hex", "wither", "ward", "shrine"]),
    ("lightning", &["lightning", "bolt", "thunder"]),
    ("cold", &["cold", "ice", "frost", "snow", "blizzard", "frozen"]),
    ("water", &["flood", "wave", "drown", "tsunami", "sea", "river"]),
    ("poison", &["poison", "plague", "venom", "rot", "swarm"]),
    ("blade", &["sword", "blade", "stab", "slash", "spear", "arrow", "army", "soldier", "strike"]),
    ("fall", &["fall", "crash", "quake", "collapse", "summoning", "landslide"]),
];

/// Caps on a demigod's whole kit (bounded gear).
pub const CAP_D20: i32 = 3;
pub const CAP_DAMAGE: f32 = 1.5;
pub const CAP_GUARD: f32 = 1.5;
pub const CAP_REGEN: f32 = 0.4;
pub const CAP_DUEL: i32 = 3;
/// Armour never turns aside more than this share of one hit.
pub const GUARD_SHARE: f32 = 0.5;
/// Smite's extra damage, Last Stand's health, Second Wind's heal and trigger.
pub const SMITE: f32 = 2.0;
pub const LAST_STAND_HEALTH: f32 = 1.0;
pub const SECOND_WIND: f32 = 2.0;
pub const SECOND_WIND_AT: f32 = 3.0;
pub const THORNS: f32 = 0.5;

/// One item's modifiers. Zeroes are left out of the payload.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Mods {
    /// d20 modifier on the holder's strikes (attacks and threats).
    #[serde(default, skip_serializing_if = "is0")]
    pub strike: i32,
    /// d20 modifier on its journeys.
    #[serde(default, skip_serializing_if = "is0")]
    pub journey: i32,
    /// d20 modifier on its other deeds (parleys, makings, its own wills).
    #[serde(default, skip_serializing_if = "is0")]
    pub deed: i32,
    /// Damage added to each strike it lands on another body.
    #[serde(default, skip_serializing_if = "is0f")]
    pub damage: f32,
    /// Damage turned aside from each hit it takes (never more than half the hit).
    #[serde(default, skip_serializing_if = "is0f")]
    pub guard: f32,
    /// Health regained each beat (below its maximum).
    #[serde(default, skip_serializing_if = "is0f")]
    pub regen: f32,
    /// Added to its duel blows' d20 (challenge mode's gear term).
    #[serde(default, skip_serializing_if = "is0")]
    pub duel: i32,
    /// A damage type halved (after the guard).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resist: Option<String>,
    /// Damage a demigod takes for striking the holder.
    #[serde(default, skip_serializing_if = "is0f")]
    pub thorns: f32,
}

fn is0(x: &i32) -> bool {
    *x == 0
}
fn is0f(x: &f32) -> bool {
    *x == 0.0
}

/// What an item is and does.
#[derive(Debug, Clone, PartialEq)]
pub struct Arm {
    pub item: ItemId,
    /// `weapon` | `armor` | `charm`.
    pub slot: &'static str,
    pub tier: Tier,
    pub material: &'static str,
    pub mods: Mods,
    /// A once-per-match power: `smite` | `last_stand` | `second_wind`.
    pub power: Option<&'static str>,
    /// The god whose blessing it carries.
    pub blessed: Option<GodId>,
}

/// The slot an item kind fills, if it is gear at all (hosts, structures and phenomena are not).
pub fn slot_of(kind: ItemKind) -> Option<&'static str> {
    match kind {
        ItemKind::Weapon => Some("weapon"),
        ItemKind::Armor => Some("armor"),
        ItemKind::Relic | ItemKind::Artifact => Some("charm"),
        _ => None,
    }
}

/// The material of an item: the one its name says, else a seeded pick from its name.
pub fn material_of(seed: u64, name: &str) -> &'static (&'static str, &'static str, i32) {
    let n = name.to_lowercase();
    if let Some(m) = MATERIALS.iter().find(|m| n.contains(m.0) || n.contains(&m.0.replace('-', " "))) {
        return m;
    }
    let short = |m: &&(&str, &str, i32)| m.0.split('-').next().is_some_and(|w| w.len() > 3 && n.contains(w));
    if let Some(m) = MATERIALS.iter().find(short) {
        return m;
    }
    &MATERIALS[(hash_str(&format!("{seed}:{n}")) % MATERIALS.len() as u64) as usize]
}

/// The blessing marker in a manifestation's reasons ("god-blessed: Nature").
pub const BLESSED: &str = "god-blessed";

/// The god whose blessing made this item (rules 0.9.0: an arming a god blessed).
pub fn blessed_by(w: &World, it: &Item) -> Option<GodId> {
    let m = w.manifests.get(it.ext.manifestation?.ix())?;
    m.verdict.reasons.iter().any(|r| r.starts_with(BLESSED)).then_some(m.verdict.god)
}

/// The pure rule: modifiers of a `slot` item of `tier` made of `material`, blessed by the god with `slug`.
pub fn mods_for(slot: &str, tier: Tier, material: &(&str, &str, i32), blessed: Option<&str>) -> (Mods, Option<&'static str>) {
    let t = tier.index().min(3);
    let mut m = Mods::default();
    let mut power = None;
    match slot {
        "weapon" => {
            m.strike = [1, 1, 2, 2][t];
            m.damage = [0.0, 0.5, 0.5, 1.0][t];
            m.duel = [0, 1, 1, 2][t];
            if t >= 2 {
                power = Some("smite");
            }
        }
        "armor" => {
            m.guard = [0.25, 0.5, 0.75, 1.0][t];
            m.duel = [0, 0, 1, 1][t];
            if t >= 1 {
                m.resist = Some(material.1.to_string());
            }
            if t >= 2 {
                power = Some("last_stand");
            }
        }
        _ => {
            m.deed = [0, 1, 1, 2][t];
            m.regen = [0.0, 0.1, 0.15, 0.2][t];
            if t >= 2 {
                power = Some("second_wind");
            }
        }
    }
    m.journey += material.2;
    // The god's signature (DM_RULEBOOK 24.4).
    match blessed {
        Some("time") => m.journey += 2,
        Some("nature") => m.regen += 0.2,
        Some("consequence") => m.thorns = THORNS,
        Some("death") => power = Some("last_stand"),
        Some("chance") => {
            m.strike += 1;
            m.journey += 1;
            m.deed += 1;
        }
        Some("memory") => m.duel += 1,
        Some("entropy") => m.damage += 0.5,
        _ => {}
    }
    (m, power)
}

/// What `it` is and does as gear (`None`: not gear, or no longer held).
pub fn arm_of(w: &World, it: &Item, pantheon: Option<&Pantheon>) -> Option<Arm> {
    let slot = slot_of(it.ext.kind)?;
    if it.ext.state != ItemState::Held {
        return None;
    }
    let material = material_of(w.meta.seed, &it.name);
    let blessed = blessed_by(w, it);
    let slug = blessed.and_then(|g| god_slug(w, pantheon, g));
    let (mods, power) = mods_for(slot, it.ext.tier, material, slug.as_deref());
    Some(Arm { item: it.id, slot, tier: it.ext.tier, material: material.0, mods, power, blessed })
}

/// A god's slug: from the pantheon when at hand, else from its name in the state (the built-in seven).
fn god_slug(w: &World, pantheon: Option<&Pantheon>, g: GodId) -> Option<String> {
    match pantheon.and_then(|p| p.get(g)) {
        Some(def) => Some(def.slug.clone()),
        None => Some(w.name_of(Ref::God(g)).to_lowercase()),
    }
}

pub fn power_name(p: &str) -> &'static str {
    match p {
        "smite" => "Smite",
        "last_stand" => "Last Stand",
        "second_wind" => "Second Wind",
        _ => "Power",
    }
}

pub fn power_text(p: &str) -> &'static str {
    match p {
        "smite" => "once a match, the first strike it lands on a rival demigod deals 2 more",
        "last_stand" => "once a match, the first blow that would fell its bearer leaves it standing at 1 health",
        "second_wind" => "once a match, the first time its bearer ends a beat at 3 health or less it heals 2",
        _ => "",
    }
}

fn signed(x: i32) -> String {
    format!("{x:+}")
}

/// One line saying what the gear does ("+2 to strikes, +0.5 damage, +1 in duels; once a match: Smite ...").
pub fn describe(w: &World, a: &Arm, used: bool) -> String {
    let m = &a.mods;
    let mut parts = vec![];
    if m.strike != 0 {
        parts.push(format!("{} to strikes", signed(m.strike)));
    }
    if m.journey != 0 {
        parts.push(format!("{} to journeys", signed(m.journey)));
    }
    if m.deed != 0 {
        parts.push(format!("{} to deeds", signed(m.deed)));
    }
    if m.damage != 0.0 {
        parts.push(format!("+{} damage", m.damage));
    }
    if m.guard != 0.0 {
        parts.push(format!("turns aside {} of each hit", m.guard));
    }
    if let Some(r) = &m.resist {
        parts.push(format!("halves {r} damage"));
    }
    if m.regen != 0.0 {
        parts.push(format!("mends {} a beat", (m.regen * 100.0).round() / 100.0));
    }
    if m.duel != 0 {
        parts.push(format!("{} in duels", signed(m.duel)));
    }
    if m.thorns != 0.0 {
        parts.push(format!("{} back at whoever strikes its bearer", m.thorns));
    }
    let mut s = if parts.is_empty() { "a keepsake: it does nothing yet".to_string() } else { parts.join(", ") };
    if let Some(p) = a.power {
        s.push_str(&format!("; {}: {}{}", power_name(p), power_text(p), if used { " (spent)" } else { "" }));
    }
    if let Some(g) = a.blessed {
        s.push_str(&format!("; blessed by {}", w.name_of(Ref::God(g))));
    }
    s
}

/// The gear fields of an item's payload: `{slot, material, mods, power, blessed_by, does}` (nothing for non-gear).
pub fn item_fields(w: &World, it: &Item, v: &mut Value) {
    let Some(a) = arm_of(w, it, None) else { return };
    let used = super::book(w).used.iter().any(|u| u.item == it.id);
    v["slot"] = a.slot.into();
    v["material"] = a.material.into();
    v["mods"] = json!(a.mods);
    v["power"] = match a.power {
        Some(p) => json!({ "id": p, "name": power_name(p), "text": power_text(p), "used": used }),
        None => Value::Null,
    };
    v["blessed_by"] = json!(a.blessed.map(|g| w.name_of(Ref::God(g))));
    v["does"] = describe(w, &a, used).into();
}

/// A demigod's whole kit: its best item per slot, summed and capped.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Kit {
    pub mods: Mods,
    /// Every damage type it halves.
    pub resists: Vec<String>,
    /// The items that count (best per slot).
    pub arms: Vec<Arm>,
}

impl Kit {
    pub fn power(&self, p: &str) -> Option<ItemId> {
        self.arms.iter().find(|a| a.power == Some(p)).map(|a| a.item)
    }
}

/// The gear `d` fights with now (empty when the rules are off or it holds none).
pub fn kit(w: &World, d: DemigodId, st: &Settings) -> Kit {
    let mut k = Kit::default();
    if !st.wills_enabled {
        return k;
    }
    let me = Ref::Demigod(d);
    let mut best: Vec<Arm> = vec![];
    for it in w.items.iter().filter(|i| i.holders.last() == Some(&me)) {
        let Some(a) = arm_of(w, it, None) else { continue };
        match best.iter_mut().find(|b| b.slot == a.slot) {
            // The higher tier wins; the newer of equals.
            Some(b) => {
                if a.tier >= b.tier {
                    *b = a;
                }
            }
            None => best.push(a),
        }
    }
    let s = st.arms_scale;
    for a in &best {
        let m = &a.mods;
        k.mods.strike += m.strike;
        k.mods.journey += m.journey;
        k.mods.deed += m.deed;
        k.mods.duel += m.duel;
        k.mods.damage += m.damage * s;
        k.mods.guard += m.guard * s;
        k.mods.regen += m.regen * s;
        k.mods.thorns += m.thorns * s;
        if let Some(r) = &m.resist {
            k.resists.push(r.clone());
        }
    }
    if s <= 0.0 {
        k.mods.strike = 0;
        k.mods.journey = 0;
        k.mods.deed = 0;
        k.mods.duel = 0;
        k.resists.clear();
    }
    k.mods.strike = k.mods.strike.min(CAP_D20);
    k.mods.journey = k.mods.journey.min(CAP_D20);
    k.mods.deed = k.mods.deed.min(CAP_D20);
    k.mods.duel = k.mods.duel.min(CAP_DUEL);
    k.mods.damage = k.mods.damage.min(CAP_DAMAGE);
    k.mods.guard = k.mods.guard.min(CAP_GUARD);
    k.mods.regen = k.mods.regen.min(CAP_REGEN);
    k.arms = if s > 0.0 { best } else { vec![] };
    k
}

/// The named d20 modifiers a demigod's gear, boons and hexes give an act of `verb` (rules 0.9.0). Empty when off.
pub fn act_mods(w: &World, d: DemigodId, verb: &str, beat: u32, st: &Settings) -> Vec<(String, i32)> {
    use fn7_agents::common::{classify, ActClass};
    let mut out = vec![];
    if !st.wills_enabled {
        return out;
    }
    let k = kit(w, d, st);
    let (what, x) = match classify(verb) {
        ActClass::Attack | ActClass::Scare => ("arms", k.mods.strike),
        ActClass::Move => ("fleet gear", k.mods.journey),
        _ => ("charm", k.mods.deed),
    };
    if x != 0 {
        out.push((what.to_string(), x));
    }
    out.extend(super::standing_mods(w, d, beat));
    out
}

/// The sum of [`act_mods`].
pub fn act_mod(w: &World, d: DemigodId, verb: &str, beat: u32, st: &Settings) -> i32 {
    act_mods(w, d, verb, beat, st).iter().map(|x| x.1).sum()
}

/// The duel's gear term from arms (added to "any item +1"): the kit's duel bonus.
pub fn duel_bonus(w: &World, d: DemigodId) -> i32 {
    if !super::on(w) {
        return 0;
    }
    let st = crate::world::settings(w);
    kit(w, d, &st).mods.duel
}

fn round2(x: f32) -> f32 {
    (x * 100.0).round() / 100.0
}

fn typed(cause: &str, ty: &str) -> bool {
    let c = cause.to_lowercase();
    TYPES.iter().find(|t| t.0 == ty).is_some_and(|t| t.1.iter().any(|w| c.contains(w)))
}

/// Every proposal on its way into a merge, shaped by gear and curses (rules 0.9.0; a no-op when off):
/// - a heal on a withered demigod and a raising of a sealed one become narration (the curse holds);
/// - damage a demigod's strike deals another body grows by its weapon (and by Smite, once a match, on a rival demigod);
/// - damage a demigod takes shrinks by its armour's guard (at most half the hit), then halves if its gear resists
///   the type; thorns bite a demigod who struck it.
pub fn shape(w: &mut World, mut props: Vec<Proposal>, beat: u32) -> Vec<Proposal> {
    let st = crate::world::settings(w);
    if !st.wills_enabled || props.is_empty() {
        return props;
    }
    let touches = props.iter().any(|p| matches!(&p.effect, Effect::Damage { .. } | Effect::Heal { .. } | Effect::Revive { .. }));
    if !touches {
        return props;
    }
    let mut book = super::book(w);
    let mut dirty = false;
    let mut extra = vec![];
    for p in props.iter_mut() {
        match &mut p.effect {
            Effect::Heal { who: Ref::Demigod(d), .. } => {
                if let Some(c) = super::curses::active(&book, *d, super::curses::CurseKind::Wither, beat) {
                    event("curse_holds", Some(Ref::Demigod(*d).to_string()), json!({ "beat": beat, "curse": c.id, "kind": "wither", "blocked": "heal" }));
                    p.effect = Effect::Note { text: format!("{} (it withers away: a curse of withering lies on {})", p.act, w.name_of(Ref::Demigod(*d))) };
                }
            }
            Effect::Revive { who: Ref::Demigod(d) } => {
                if let Some(c) = super::curses::active(&book, *d, super::curses::CurseKind::Seal, beat) {
                    event("curse_holds", Some(Ref::Demigod(*d).to_string()), json!({ "beat": beat, "curse": c.id, "kind": "seal", "blocked": "revive" }));
                    p.effect = Effect::Note { text: format!("{} (the seal of no resurrection holds on {})", p.act, w.name_of(Ref::Demigod(*d))) };
                }
            }
            Effect::Damage { who, amount, cause } => {
                let before = *amount;
                let attacker = match p.actor {
                    Ref::Demigod(a) if *who != Ref::Demigod(a) && w.demigods.get(a.ix()).is_some_and(|x| x.alive) && p.source != Source::Rules => Some(a),
                    _ => None,
                };
                let mut notes = vec![];
                if let Some(a) = attacker {
                    let ka = kit(w, a, &st);
                    if ka.mods.damage > 0.0 {
                        *amount += ka.mods.damage;
                        notes.push(format!("weapon +{}", ka.mods.damage));
                    }
                    if let (Some(item), Ref::Demigod(t)) = (ka.power("smite"), *who) {
                        if !w.same_player(a, t) && !book.used.iter().any(|u| u.item == item) {
                            *amount += SMITE * st.arms_scale;
                            book.used.push(super::Used { item, beat, power: "smite".into() });
                            dirty = true;
                            notes.push("Smite".into());
                            event("arms_power", Some(Ref::Demigod(a).to_string()), json!({ "beat": beat, "item": item, "power": "smite", "on": t }));
                        }
                    }
                }
                if let Ref::Demigod(t) = *who {
                    let kt = kit(w, t, &st);
                    if kt.mods.guard > 0.0 && *amount > 0.0 {
                        let turned = kt.mods.guard.min(*amount * GUARD_SHARE);
                        *amount -= turned;
                        notes.push(format!("armour -{}", round2(turned)));
                    }
                    if kt.resists.iter().any(|r| typed(cause, r) || typed(&p.act, r)) {
                        *amount *= 0.5;
                        notes.push("resisted".into());
                    }
                    if let (Some(a), true) = (attacker, kt.mods.thorns > 0.0) {
                        extra.push(Proposal {
                            source: Source::Rules,
                            intent: p.intent,
                            effect: Effect::Damage { who: Ref::Demigod(a), amount: kt.mods.thorns, cause: format!("the thorns of {}'s gear", w.name_of(Ref::Demigod(t))) },
                            act: format!("{}'s gear bites back at {}", w.name_of(Ref::Demigod(t)), w.name_of(Ref::Demigod(a))),
                            actor: Ref::Demigod(t),
                            magnitude: 2,
                            valence: -1,
                            witnesses: vec![Ref::Demigod(a), Ref::Demigod(t)],
                            cause: None,
                        });
                    }
                }
                *amount = round2(amount.max(0.0));
                if !notes.is_empty() {
                    event("arms_shaped", Some(who.to_string()), json!({ "beat": beat, "who": who, "from": before, "to": *amount, "by": notes }));
                }
            }
            _ => {}
        }
    }
    if dirty {
        super::save(w, &book);
    }
    props.extend(extra);
    props
}

/// Last Stand (rules 0.9.0): a demigod that fell this beat while bearing unspent Last Stand gear stands again at 1
/// health (once a match per item). Runs after the beat's merge and the society, before the DM's step (so its gear
/// never drops). Returns who stood back up.
pub fn last_stand(w: &mut World, beat: u32, alive_at_start: &[DemigodId], pantheon: &Pantheon, st: &Settings) -> Vec<DemigodId> {
    let mut out = vec![];
    if !st.wills_enabled || st.arms_scale <= 0.0 {
        return out;
    }
    for &d in alive_at_start {
        if w.demigods[d.ix()].alive {
            continue;
        }
        let mut book = super::book(w);
        // The gear was "held" until it fell: read it as if it still stood.
        let Some(item) = kit(w, d, st).power("last_stand").filter(|i| !book.used.iter().any(|u| u.item == *i)) else { continue };
        if super::curses::active(&book, d, super::curses::CurseKind::Seal, beat).is_some() {
            continue;
        }
        book.used.push(super::Used { item, beat, power: "last_stand".into() });
        super::save(w, &book);
        let name = w.demigods[d.ix()].name.clone();
        let cause = w.demigods[d.ix()].cause.clone().unwrap_or_default();
        let p = Proposal {
            source: Source::Rules,
            intent: None,
            effect: Effect::Revive { who: Ref::Demigod(d) },
            act: format!("{name}'s {} holds: the Last Stand", w.name_of(Ref::Item(item))),
            actor: Ref::Item(item),
            magnitude: 6,
            valence: 2,
            witnesses: vec![Ref::Demigod(d)],
            cause: None,
        };
        crate::merge::run(w, vec![p], pantheon, beat, st.max_witnesses as usize);
        if !w.demigods[d.ix()].alive {
            continue;
        }
        let x = &mut w.demigods[d.ix()];
        x.health = LAST_STAND_HEALTH.min(x.max_health);
        let pos = x.pos;
        event("arms_power", Some(Ref::Demigod(d).to_string()), json!({ "beat": beat, "item": item, "power": "last_stand", "cause": cause }));
        let wh = crate::dm::locate(w, pos, fn7_model::hexgrid::HexLevel::People);
        crate::dm::remember(w, beat, &wh, crate::dm::Decision {
            scope: fn7_model::dm::Scope::Cell,
            kind: "arms.power",
            query: Some(format!("power:{}:last_stand", item.0)),
            public: true,
            text: format!("{name} should have fallen ({cause}), but the {} held: its Last Stand is spent", w.name_of(Ref::Item(item))),
            data: json!({ "demigod": Ref::Demigod(d), "name": name, "item": Ref::Item(item), "power": "last_stand", "at": pos, "cell": wh.cell }),
        });
        out.push(d);
    }
    out
}

/// The end of the beat for gear (after the DM's step): regeneration below the maximum, and Second Wind.
pub fn upkeep(w: &mut World, beat: u32, pantheon: &Pantheon, st: &Settings) {
    if !st.wills_enabled || st.arms_scale <= 0.0 {
        return;
    }
    let mut props = vec![];
    let mut book = super::book(w);
    let mut dirty = false;
    for d in w.demigods.iter().filter(|d| d.alive) {
        let k = kit(w, d.id, st);
        if k.arms.is_empty() {
            continue;
        }
        let me = Ref::Demigod(d.id);
        if k.mods.regen > 0.0 && d.health < d.max_health {
            props.push(Proposal {
                source: Source::Rules,
                intent: None,
                effect: Effect::Heal { who: me, amount: round2(k.mods.regen) },
                act: format!("{}'s charm mends it a little", d.name),
                actor: me,
                magnitude: 1,
                valence: 1,
                witnesses: vec![me],
                cause: None,
            });
        }
        if let Some(item) = k.power("second_wind").filter(|i| d.health <= SECOND_WIND_AT && !book.used.iter().any(|u| u.item == *i)) {
            book.used.push(super::Used { item, beat, power: "second_wind".into() });
            dirty = true;
            event("arms_power", Some(me.to_string()), json!({ "beat": beat, "item": item, "power": "second_wind" }));
            props.push(Proposal {
                source: Source::Rules,
                intent: None,
                effect: Effect::Heal { who: me, amount: SECOND_WIND * st.arms_scale },
                act: format!("{}'s {} breathes for it: the Second Wind", d.name, w.name_of(Ref::Item(item))),
                actor: Ref::Item(item),
                magnitude: 4,
                valence: 2,
                witnesses: vec![me],
                cause: None,
            });
        }
    }
    if dirty {
        super::save(w, &book);
    }
    if !props.is_empty() {
        crate::merge::run(w, props, pantheon, beat, st.max_witnesses as usize);
    }
}

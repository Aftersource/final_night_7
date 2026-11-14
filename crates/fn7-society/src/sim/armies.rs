//! Armies with a command chain: demigod → general → captains → units (squad cells). Orders go to the general, who
//! acts on them by their own judgment ([`judge`]: competence, loyalty, ambition and cruelty shape it; vague orders get
//! misread). Battles resolve by rules (size, morale, competence, supply, terrain, world mods, divine meddling) with
//! casualties, heroes, cowards and fallen officers. Strain makes decision points: mutiny, defection. The society also
//! raises volunteer hosts for the demigods it loves.

use fn7_model::seed::hash_str;
use fn7_model::*;
use serde_json::json;

use crate::geo::{bearing_deg, city_of, destination, distance_km, place_at, pos_of};
use crate::sim::people::{self, alive, emerge, kill, life, life_mut, set_role, Spec};
use crate::sim::{clamp01, r3, Ctx};
use crate::util::{bump, get};

/// Rival armies closer than this fight.
pub const CLASH_KM: f64 = 300.0;
/// An army this close to a rival demigod attacks it.
pub const STRIKE_KM: f64 = 200.0;
/// Strain above which a general faces a decision point (mutiny, defection).
pub const STRAIN_DECISION: f32 = 0.6;
/// Allegiance at which a crowd may raise a volunteer host for a demigod (members of its Following always may).
pub const HOST_ALLEGIANCE: f32 = 0.35;
/// Most live armies per demigod the society will raise volunteer hosts for.
pub const MAX_ARMIES_PER_DEMIGOD: usize = 4;
/// A crowd this close to a demigod with no army yet raises a first host for them with this extra chance a beat.
pub const HOST_NEAR_KM: f64 = 1200.0;
pub const FIRST_HOST_P: f32 = 0.2;
/// Generals on their own go after a hostile mob within this range (planet), when no hostile army is nearer.
pub const MOB_HUNT_KM: f64 = 1500.0;
/// With nothing nearer to do, a general seeks out a hostile army this far away (planet).
pub const FAR_SEEK_KM: f64 = 6000.0;

const VAGUE: &[&str] = &[
    "something", "deal with", "handle", "take care", "whatever", "do your thing", "sort out", "make it happen", "go",
    "be bold", "use your judgment", "somehow",
];
const RETREAT: &[&str] = &["retreat", "fall back", "withdraw", "pull back", "come home", "run"];
const HOLD: &[&str] = &["hold", "defend", "guard", "protect", "stay", "dig in", "garrison"];
const RAID: &[&str] = &["raid", "plunder", "sack", "burn", "loot", "pillage", "ravage"];
const SIEGE: &[&str] = &["besiege", "siege", "surround", "starve"];

fn has(text: &str, words: &[&str]) -> bool {
    let t = text.to_lowercase();
    words.iter().any(|w| t.contains(w))
}

/// Does this demigod still command an army (alive, not rogue)?
fn serves(w: &World, a: &Army) -> bool {
    !a.rogue && w.demigods.get(a.demigod.ix()).is_some_and(|d| d.alive)
}

/// A mob an army will put down: live, not rallying behind the army's demigod, and either rallying behind a rival or
/// looting or attacking.
pub fn hostile_mob(w: &World, a: &Army, m: &Mob) -> bool {
    !a.rogue
        && m.ended.is_none()
        && m.size > 0
        && m.side != Some(a.demigod)
        && (m.side.is_some_and(|d| w.demigods.get(d.ix()).is_some_and(|x| x.alive))
            || matches!(m.action, MobAction::Looting | MobAction::Attacking))
}

/// Live armies `d` commands (not rogue).
pub fn armies_of(w: &World, d: DemigodId) -> usize {
    w.clusters.iter().filter(|c| c.size > 0 && c.army.as_ref().is_some_and(|a| a.demigod == d && !a.rogue)).count()
}

fn hostile(w: &World, a: &Army, b: &Army) -> bool {
    a.rogue || b.rogue || (a.demigod != b.demigod && serves(w, a) && serves(w, b))
}

/// Create an army cluster split from crowd `from` (the body of `Effect::RaiseArmy`, and the society's volunteer
/// hosts). The general and captains are appointed by the next society step (same beat).
#[allow(clippy::too_many_arguments)]
pub fn create_army(
    w: &mut World,
    demigod: DemigodId,
    from: ClusterId,
    size: u32,
    name: &str,
    orders: &str,
    beat: u32,
    volunteer: bool,
) -> Result<ClusterId, String> {
    let src = w.clusters.get(from.ix()).ok_or("no such crowd")?;
    if src.army.is_some() {
        return Err("an army can't raise an army".into());
    }
    if size == 0 || size > src.size {
        return Err(format!("{from} has {} people, can't raise {size}", src.size));
    }
    let pos = w.places.get(src.place.ix()).map(|p| p.center).unwrap_or([0.0, 0.0]);
    let mut groups = src.groups.clone();
    groups.insert(w.demigods[demigod.ix()].following);
    let id = ClusterId::from_ix(w.clusters.len());
    let seed = fn7_model::seed::Rng::derive(w.meta.seed, &[hash_str("army"), id.0 as u64]).next_u64();
    let city = city_of(w, src.place).unwrap_or(src.place);
    let new = Cluster {
        id,
        label: name.to_string(),
        size,
        place: city,
        traits: src.traits.iter().take(2).cloned().chain(["armed".to_string()]).collect(),
        needs: vec!["orders".into(), "plunder".into()],
        mood: Mood { anger: 0.6, fear: 0.2, hope: 0.5, devotion: 0.7 },
        baseline: Mood { anger: 0.5, fear: 0.2, hope: 0.4, devotion: 0.6 },
        groups,
        worship: src.worship.clone(),
        allegiance: vec![(demigod, 1.0)],
        materialized: 0,
        seed,
        army: Some(Army {
            demigod,
            orders: orders.chars().take(200).collect(),
            pos,
            dest: None,
            target: None,
            raised_beat: beat,
            from,
            general: None,
            captains: vec![],
            units: vec![],
            morale: 0.8,
            supply: 1.0,
            stance: Stance::Holding,
            rogue: false,
            order: None,
            strain: 0.0,
            battles: 0,
            losses: 0,
            volunteer,
        }),
        need: Need::default(),
        stance: Stance::Holding,
        leader: None,
        cells: vec![],
        dead: 0,
    };
    w.clusters[from.ix()].size -= size;
    w.clusters.push(new);
    Ok(id)
}

/// The general's rules judgment of an order (`why` = "order"), an engagement ("engage", with `enemy`), strain
/// ("strain") or their own initiative ("initiative"). Deterministic per (seed, beat, army, words).
pub fn judge(
    w: &World,
    army: ClusterId,
    text: &str,
    order_target: Option<Ref>,
    by: DemigodId,
    why: &str,
    beat: u32,
) -> Judgment {
    let Some(cl) = w.clusters.get(army.ix()) else {
        return Judgment::default();
    };
    let Some(a) = cl.army.as_ref() else {
        return Judgment::default();
    };
    let g = alive(w, a.general);
    let gl = g.and_then(|g| life(w, g)).cloned();
    let (comp, ambition, cruelty, courage) = gl
        .as_ref()
        .map(|l| (l.competence, l.ambition, l.cruelty, l.courage))
        .unwrap_or((0.5, 0.3, 0.0, 0.5));
    let loyalty = g
        .map(|g| get(&w.characters[g.ix()].allegiance, by))
        .unwrap_or(0.6);
    let gname = g.map(|g| people::surname(&w.characters[g.ix()].name).to_string()).unwrap_or_else(|| "The army".into());
    let mut rng = fn7_model::seed::Rng::derive(
        w.meta.seed,
        &[hash_str("judge"), beat as u64, army.0 as u64, hash_str(text), hash_str(why)],
    );
    let home = w.clusters.get(a.from.ix()).map(|c| Ref::Place(city_of(w, c.place).unwrap_or(c.place)));
    let initiative = initiative_target(w, army);
    match why {
        "engage" => {
            let enemy = order_target;
            let mine = strength(w, army);
            let theirs = enemy
                .and_then(|e| match e {
                    Ref::Cluster(c) => Some(strength(w, c)),
                    Ref::Mob(m) => w.mobs.get(m.ix()).map(|m| m.size as f32 * 0.3),
                    _ => None,
                })
                .unwrap_or(0.0);
            let ratio = mine / theirs.max(1.0);
            let fight = ratio >= 0.8 || (courage > 0.7 && ratio >= 0.5) || (cruelty > 0.5 && ratio >= 0.6);
            if fight {
                Judgment {
                    reading: Reading::Obey,
                    target: enemy,
                    stance: Stance::Fighting,
                    speech: format!("{gname}: \"They came to us. Good. Form up.\""),
                    defect_to: None,
                }
            } else {
                Judgment {
                    reading: Reading::Obey,
                    target: home,
                    stance: Stance::Retreating,
                    speech: format!("{gname}: \"Not this fight. Fall back and live.\""),
                    defect_to: None,
                }
            }
        }
        "strain" => {
            let demigod_dead = !w.demigods.get(by.ix()).is_some_and(|d| d.alive);
            let rival = g.and_then(|g| {
                w.characters[g.ix()]
                    .allegiance
                    .iter()
                    .filter(|(d, x)| *d != by && *x >= 0.3 && w.demigods.get(d.ix()).is_some_and(|x| x.alive))
                    .max_by(|x, y| x.1.total_cmp(&y.1).then(y.0.cmp(&x.0)))
                    .map(|x| x.0)
            });
            if let Some(r) = rival.filter(|_| loyalty < 0.25 || demigod_dead) {
                let rn = w.demigods[r.ix()].name.clone();
                Judgment {
                    reading: Reading::Defect,
                    target: Some(Ref::Demigod(r)),
                    stance: Stance::Marching,
                    speech: format!("{gname}: \"We march for {rn} now. Anyone who disagrees can walk home.\""),
                    defect_to: Some(r),
                }
            } else if demigod_dead || (loyalty < 0.3 && ambition > 0.55) || (loyalty < 0.15 && rng.f64() < 0.5) {
                Judgment {
                    reading: Reading::Mutiny,
                    target: initiative,
                    stance: Stance::Rogue,
                    speech: format!("{gname}: \"No more gods. This army answers to me.\""),
                    defect_to: None,
                }
            } else {
                Judgment {
                    reading: Reading::Obey,
                    target: a.target.or(initiative),
                    stance: a.stance,
                    speech: format!("{gname}: \"Tighten your belts. We hold together.\""),
                    defect_to: None,
                }
            }
        }
        "initiative" => {
            let raid = cruelty > 0.3 && ambition > 0.5;
            Judgment {
                reading: Reading::Obey,
                target: initiative,
                stance: if initiative.is_none() {
                    Stance::Holding
                } else if raid {
                    Stance::Raiding
                } else {
                    Stance::Marching
                },
                speech: String::new(),
                defect_to: None,
            }
        }
        _ => {
            // An order from the demigod.
            let vague = order_target.is_none() || has(text, VAGUE);
            let p_misread = (if vague { 0.45 } else { 0.07 }) * (1.25 - comp) + 0.12 * (ambition - loyalty).max(0.0);
            let p_refuse = if loyalty < 0.1 { 0.3 + 0.4 * ambition } else { 0.0 };
            let x = rng.f64() as f32;
            let stance = if has(text, RETREAT) {
                Stance::Retreating
            } else if has(text, HOLD) {
                Stance::Holding
            } else if has(text, RAID) {
                Stance::Raiding
            } else if has(text, SIEGE) {
                Stance::Besieging
            } else {
                Stance::Marching
            };
            let tname = |t: Option<Ref>| t.map(|t| w.name_of(t)).unwrap_or_else(|| "wherever the fighting is".into());
            if x < p_refuse {
                Judgment {
                    reading: Reading::Refuse,
                    target: a.target,
                    stance: Stance::Holding,
                    speech: format!("{gname}: \"No. My people are not yours to waste.\""),
                    defect_to: None,
                }
            } else if x < p_refuse + p_misread {
                // Reads it as something else: the opposite stance for hold/retreat orders, else another target.
                let alt = [initiative, home, a.target]
                    .into_iter()
                    .flatten()
                    .find(|t| Some(*t) != order_target);
                let st = match stance {
                    Stance::Holding => Stance::Marching,
                    Stance::Retreating => Stance::Holding,
                    s => s,
                };
                Judgment {
                    reading: Reading::Misread,
                    target: alt.or(order_target),
                    stance: st,
                    speech: format!("{gname}: \"Understood: {}. Moving now.\"", tname(alt.or(order_target))),
                    defect_to: None,
                }
            } else {
                let target = match stance {
                    Stance::Retreating => order_target.or(home),
                    Stance::Holding => order_target.or(a.target),
                    _ => order_target.or(initiative),
                };
                let flourish = if cruelty > 0.4 {
                    " Leave nothing standing."
                } else if cruelty < -0.4 {
                    " Spare who you can."
                } else {
                    ""
                };
                Judgment {
                    reading: Reading::Obey,
                    target,
                    stance,
                    speech: format!("{gname}: \"{} — as you command.{flourish}\"", tname(target)),
                    defect_to: None,
                }
            }
        }
    }
}

/// Where a general goes on their own: the nearest hostile army within 3,000 km, else a hostile mob within
/// [`MOB_HUNT_KM`], else the nearest rival demigod within 4,000 km, else (planet) the nearest hostile army within
/// [`FAR_SEEK_KM`], else
/// the nearest city leaning to a rival within 1,500 km, else (outside Time's circle) the arena center.
pub fn initiative_target(w: &World, army: ClusterId) -> Option<Ref> {
    let cl = w.clusters.get(army.ix())?;
    let a = cl.army.as_ref()?;
    let mut best: Option<(f64, Ref)> = None;
    fn consider(best: &mut Option<(f64, Ref)>, d: f64, r: Ref, max: f64) {
        if d <= max && best.is_none_or(|b| d < b.0) {
            *best = Some((d, r));
        }
    }
    let far = if w.is_planet() { FAR_SEEK_KM } else { 3000.0 };
    let mut far_army: Option<(f64, Ref)> = None;
    for o in w.clusters.iter().filter(|o| o.id != army && o.size > 0) {
        if let Some(b) = o.army.as_ref() {
            if hostile(w, a, b) {
                consider(&mut best, distance_km(a.pos, b.pos), Ref::Cluster(o.id), 3000.0);
                consider(&mut far_army, distance_km(a.pos, b.pos), Ref::Cluster(o.id), far);
            }
        }
    }
    if best.is_some() {
        return best.map(|b| b.1);
    }
    // A hostile mob nearby: put it down.
    let hunt = if w.is_planet() { MOB_HUNT_KM } else { 3000.0 };
    for m in w.mobs.iter().filter(|m| hostile_mob(w, a, m)) {
        consider(&mut best, distance_km(a.pos, m.pos), Ref::Mob(m.id), hunt);
    }
    if best.is_some() {
        return best.map(|b| b.1);
    }
    for d in w.demigods.iter().filter(|d| d.alive && (a.rogue || d.id != a.demigod)) {
        consider(&mut best, distance_km(a.pos, d.pos), Ref::Demigod(d.id), 4000.0);
    }
    if best.is_some() {
        return best.map(|b| b.1);
    }
    // Nothing near: armies seek each other out across the map (the war comes to a head instead of idling).
    if far_army.is_some() {
        return far_army.map(|b| b.1);
    }
    for n in w.nodes.iter().filter(|n| n.level == Level::City || (!w.is_planet() && n.level == Level::District)) {
        let lean = n
            .allegiance
            .iter()
            .filter(|(d, x)| *d != a.demigod && *x > 0.25)
            .count();
        if lean > 0 || a.rogue {
            let c = w.places[n.place.ix()].center;
            consider(&mut best, distance_km(a.pos, c), Ref::Place(n.place), 1500.0);
        }
    }
    if best.is_some() {
        return best.map(|b| b.1);
    }
    let center = w.places.get(w.meta.arena.ix())?.center;
    (distance_km(a.pos, center) > w.meta.arena_radius_km * 0.6).then_some(Ref::Place(w.meta.arena))
}

/// Fighting strength: size × morale × the general's competence × supply.
pub fn strength(w: &World, army: ClusterId) -> f32 {
    let Some(cl) = w.clusters.get(army.ix()) else { return 0.0 };
    let Some(a) = cl.army.as_ref() else {
        return cl.size as f32 * 0.25;
    };
    let comp = alive(w, a.general).and_then(|g| life(w, g)).map(|l| l.competence).unwrap_or(0.4);
    cl.size as f32 * (0.4 + 0.6 * a.morale) * (0.6 + 0.8 * comp) * (0.7 + 0.3 * a.supply)
}

/// `Effect::Orders`: the army acts on this judgment. Mutiny sends it rogue (out of its demigod's Following), Defect
/// hands it and its general to `defect_to` (captains loyal to the old master walk out). Returns the row's targets.
#[allow(clippy::too_many_arguments)]
pub fn apply_orders(
    w: &mut World,
    army: ClusterId,
    by: DemigodId,
    text: &str,
    j: &Judgment,
    why: &str,
    llm: bool,
    beat: u32,
) -> Result<Vec<Ref>, String> {
    let cl = w.clusters.get(army.ix()).ok_or("no such army")?;
    let a = cl.army.as_ref().ok_or("not an army")?;
    if cl.size == 0 {
        return Err("the army is gone".into());
    }
    if let Some(t) = j.target {
        if !crate::util::exists(w, t) {
            return Err(format!("{t} does not exist"));
        }
    }
    if j.reading == Reading::Defect {
        let to = j.defect_to.ok_or("defect to whom?")?;
        if !w.demigods.get(to.ix()).is_some_and(|d| d.alive) || to == a.demigod {
            return Err("can't defect there".into());
        }
    }
    let general = a.general;
    let old = a.demigod;
    let dest = j.target.and_then(|t| pos_of(w, t));
    let mut refs = vec![Ref::Cluster(army)];
    refs.extend(general.map(Ref::Char));
    refs.extend(j.target);
    match j.reading {
        Reading::Mutiny => {
            let f = w.demigods[old.ix()].following;
            let c = &mut w.clusters[army.ix()];
            c.groups.remove(f);
            let a = c.army.as_mut().expect("army");
            a.rogue = true;
            w.soc.mutinies += 1;
            if let Some(g) = general {
                bump(&mut w.characters[g.ix()].allegiance, old, -0.6, -1.0, 1.0);
            }
        }
        Reading::Defect => {
            let to = j.defect_to.expect("checked");
            let (fo, fnew) = (w.demigods[old.ix()].following, w.demigods[to.ix()].following);
            let c = &mut w.clusters[army.ix()];
            c.groups.remove(fo);
            c.groups.insert(fnew);
            c.allegiance = vec![(to, 0.8)];
            let a = c.army.as_mut().expect("army");
            a.demigod = to;
            a.rogue = false;
            let captains = a.captains.clone();
            w.soc.defections += 1;
            if let Some(g) = general {
                let ch = &mut w.characters[g.ix()];
                bump(&mut ch.allegiance, to, 0.5, -1.0, 1.0);
                bump(&mut ch.allegiance, old, -0.6, -1.0, 1.0);
            }
            refs.push(Ref::Demigod(to));
            for cpt in captains {
                let loyal = get(&w.characters[cpt.ix()].allegiance, old) > 0.6;
                if loyal && w.characters[cpt.ix()].alive {
                    if let Some(a) = w.clusters[army.ix()].army.as_mut() {
                        a.captains.retain(|x| *x != cpt);
                    }
                    if let Some(l) = life_mut(w, cpt) {
                        l.role = Role::Deserter;
                        l.army = None;
                        l.rank.title = "a captain who would not turn traitor".into();
                    }
                    if let Some(g) = general {
                        people::grudge(w, cpt, Ref::Char(g));
                    }
                }
            }
        }
        _ => {}
    }
    let c = &mut w.clusters[army.ix()];
    let a = c.army.as_mut().expect("army");
    if j.target.is_some() {
        a.target = j.target;
        a.dest = dest;
    }
    if j.reading != Reading::Stall && j.reading != Reading::Refuse {
        a.stance = j.stance;
    }
    if !text.is_empty() {
        a.orders = text.chars().take(200).collect();
    }
    a.order = Some(ArmyOrder {
        text: text.chars().take(200).collect(),
        by,
        beat,
        judgment: Judgment {
            speech: j.speech.chars().take(200).collect(),
            ..j.clone()
        },
        why: why.to_string(),
        llm,
    });
    c.stance = a.stance;
    Ok(refs)
}

fn units_of(w: &World, army: ClusterId) -> Vec<CellId> {
    w.clusters
        .get(army.ix())
        .and_then(|c| c.army.as_ref())
        .map(|a| a.units.clone())
        .unwrap_or_default()
}

/// Give an army without a living general one: its best captain (a promotion), else a notable or a ringleader of its
/// home city who likes the demigod, else someone out of its crowd. New armies also get captains and squads, and the
/// general receives the order it was raised with.
fn appoint(w: &mut World, cx: &mut Ctx, army: ClusterId) {
    let cl = w.clusters[army.ix()].clone();
    let a = cl.army.clone().expect("army");
    let label = cl.label.clone();
    let title = format!("General of {label}");
    let fresh = a.units.is_empty() && a.captains.is_empty();
    // 1) The best living captain.
    let captain = a
        .captains
        .iter()
        .copied()
        .filter(|c| w.characters[c.ix()].alive)
        .max_by(|x, y| {
            let s = |c: &CharId| life(w, *c).map(|l| l.competence + l.courage).unwrap_or(0.0);
            s(x).total_cmp(&s(y)).then(y.cmp(x))
        });
    let kept = alive(w, a.general);
    let general = if kept.is_some() {
        kept
    } else if let Some(c) = captain {
        set_role(w, c, Role::General, Level::Crowd, Some(Ref::Cluster(army)), title.clone(), cx.beat);
        let name = w.characters[c.ix()].name.clone();
        w.soc.promotions += 1;
        cx.emit(
            w,
            SocKind::Promotion,
            format!("Captain {name} takes command of {label}"),
            Ref::Char(c),
            vec![Ref::Char(c), Ref::Cluster(army)],
            a.pos,
            Some(cl.place),
            5,
            0,
            json!({ "person": Ref::Char(c), "to": "general", "army": Ref::Cluster(army), "how": "succession" }),
        );
        Some(c)
    } else {
        // 2) A notable, hero or ringleader of the home city with no army, who doesn't hate the demigod.
        let home = w.clusters.get(a.from.ix()).and_then(|c| city_of(w, c.place));
        let local = w
            .characters
            .iter()
            .filter(|c| c.alive && c.serves.is_none() && c.possessed_by.is_none())
            .filter(|c| home.is_some() && city_of(w, c.place) == home)
            .filter(|c| get(&c.allegiance, a.demigod) >= -0.1)
            .filter(|c| match c.life.as_deref() {
                None => matches!(c.origin, Origin::Seeded),
                Some(l) => l.army.is_none() && l.mob.is_none() && matches!(l.role, Role::Ringleader | Role::Hero | Role::Notable),
            })
            .max_by(|x, y| {
                let s = |c: &Character| c.life.as_ref().map(|l| l.competence).unwrap_or(0.55) + get(&c.allegiance, a.demigod);
                s(x).total_cmp(&s(y)).then(y.id.cmp(&x.id))
            })
            .map(|c| c.id);
        match local {
            Some(c) => {
                set_role(w, c, Role::General, Level::Crowd, Some(Ref::Cluster(army)), title.clone(), cx.beat);
                Some(c)
            }
            None => {
                // 3) Someone out of the crowd it was raised from (or the army itself).
                let from = if w.clusters.get(a.from.ix()).is_some_and(|c| c.size >= 2) { a.from } else { army };
                let mut spec = Spec::new(
                    Role::General,
                    Level::Crowd,
                    Some(Ref::Cluster(army)),
                    title.clone(),
                    format!("the general of {label}"),
                );
                spec.competence = 0.3;
                spec.courage = 0.2;
                emerge(w, cx, from, spec, true)
            }
        }
    };
    let Some(g) = general else { return };
    let loyalty = 0.35 + 0.55 * cx.roll("loyalty", g.0 as u64);
    {
        let ch = &mut w.characters[g.ix()];
        let cur = get(&ch.allegiance, a.demigod);
        if cur < loyalty {
            bump(&mut ch.allegiance, a.demigod, loyalty - cur, -1.0, 1.0);
        }
        ch.pos = a.pos;
    }
    if let Some(l) = life_mut(w, g) {
        l.army = Some(army);
        l.goal = Goal::Serve;
    }
    let ar = w.clusters[army.ix()].army.as_mut().expect("army");
    ar.general = Some(g);
    ar.captains.retain(|c| *c != g);
    let gname = w.characters[g.ix()].name.clone();
    let dname = w.demigods[a.demigod.ix()].name.clone();
    cx.emit(
        w,
        SocKind::GeneralAppointed,
        format!("{gname} is appointed General of {label}, answering to {dname}"),
        Ref::Char(g),
        vec![Ref::Char(g), Ref::Cluster(army), Ref::Demigod(a.demigod)],
        a.pos,
        Some(cl.place),
        5,
        0,
        json!({ "army": Ref::Cluster(army), "general": Ref::Char(g), "demigod": Ref::Demigod(a.demigod),
                "loyalty": r3(get(&w.characters[g.ix()].allegiance, a.demigod)) }),
    );
    if fresh {
        // Captains (3, or 4 for big armies; a free veteran of the home city, a ringleader or a hero, takes the first
        // commission) and 3–5 squads.
        let n_capt = if cl.size > 20_000 { 4 } else { 3 };
        let mut captains = vec![];
        let home = w.clusters.get(a.from.ix()).and_then(|c| city_of(w, c.place));
        if let Some(v) = home.and_then(|h| people::veteran(w, h, &[Role::Ringleader, Role::Hero], Some(a.demigod))) {
            let before = life(w, v).map(|l| l.rank.title.clone()).unwrap_or_default();
            set_role(w, v, Role::Captain, Level::Cell, Some(Ref::Cluster(army)), format!("captain in {label}"), cx.beat);
            if let Some(l) = life_mut(w, v) {
                l.army = Some(army);
                l.deeds += 1;
            }
            bump(&mut w.characters[v.ix()].allegiance, a.demigod, 0.3, -1.0, 1.0);
            w.characters[v.ix()].pos = a.pos;
            captains.push(v);
            w.soc.promotions += 1;
            let vname = w.characters[v.ix()].name.clone();
            let was = if before.is_empty() { String::new() } else { format!(", once {before},") };
            cx.emit(
                w,
                SocKind::Promotion,
                format!("{vname}{was} takes a captain's commission in {label} under General {}", people::surname(&w.characters[g.ix()].name)),
                Ref::Char(v),
                vec![Ref::Char(v), Ref::Cluster(army)],
                a.pos,
                Some(cl.place),
                5,
                1,
                json!({ "person": Ref::Char(v), "to": "captain", "army": Ref::Cluster(army), "how": "veteran" }),
            );
        }
        for k in captains.len()..n_capt {
            let mut spec = Spec::new(
                Role::Captain,
                Level::Cell,
                Some(Ref::Cluster(army)),
                format!("captain in {label}"),
                format!("a captain of {label}"),
            );
            spec.courage = 0.1 * k as f32;
            if let Some(c) = emerge(w, cx, army, spec, true) {
                if let Some(l) = life_mut(w, c) {
                    l.army = Some(army);
                }
                bump(&mut w.characters[c.ix()].allegiance, a.demigod, 0.5, -1.0, 1.0);
                captains.push(c);
            }
        }
        let size = w.clusters[army.ix()].size;
        let n_units = (3 + (size / 15_000)).min(5) as usize;
        let mut units = vec![];
        for k in 0..n_units {
            let id = CellId::from_ix(w.cells.len());
            let leader = captains.get(k % captains.len().max(1)).copied();
            let share = size / n_units as u32 + u32::from(k < (size as usize % n_units));
            w.cells.push(Cell {
                id,
                kind: CellKind::Squad,
                name: format!("{} squad of {label}", ORDINALS[k % ORDINALS.len()]),
                crowd: army,
                size: share,
                mood: w.clusters[army.ix()].mood,
                leader,
                stance: Stance::Holding,
                mob: None,
                formed: cx.beat,
                ended: None,
                seed: fn7_model::seed::Rng::derive(cx.seed, &[hash_str("squad"), id.0 as u64]).next_u64(),
            });
            if let Some(l) = leader.and_then(|c| life_mut(w, c)) {
                l.cell.get_or_insert(id);
            }
            w.clusters[army.ix()].cells.push(id);
            units.push(id);
        }
        let ar = w.clusters[army.ix()].army.as_mut().expect("army");
        ar.captains.extend(captains);
        ar.units = units;
        // The order it was raised with goes to the new general.
        if ar.order.is_none() && !ar.orders.is_empty() {
            let text = ar.orders.clone();
            let t = ar.target;
            let j = judge(w, army, &text, t, a.demigod, "order", cx.beat);
            let _ = apply_orders(w, army, a.demigod, &text, &j, "order", false, cx.beat);
        }
    }
}

const ORDINALS: &[&str] = &["First", "Second", "Third", "Fourth", "Fifth"];

/// Unit sizes follow the army's size (proportional shares); dead captains leave their squads leaderless.
fn rescale_units(w: &mut World, army: ClusterId) {
    let size = w.clusters[army.ix()].size;
    let units = units_of(w, army);
    let total: u64 = units.iter().map(|u| w.cells[u.ix()].size as u64).sum();
    let n = units.len().max(1) as u64;
    let mut left = size as u64;
    for (k, u) in units.iter().enumerate() {
        let s = if total == 0 {
            size as u64 / n
        } else if k + 1 == units.len() {
            left
        } else {
            (w.cells[u.ix()].size as u64 * size as u64) / total
        };
        let s = s.min(left);
        left -= s;
        let cell = &mut w.cells[u.ix()];
        cell.size = s as u32;
        if cell.leader.is_some_and(|c| !w.characters[c.ix()].alive) {
            cell.leader = None;
        }
        if cell.size == 0 && cell.ended.is_none() {
            cell.ended = Some(0);
        }
    }
}

/// People of a dead army return to nothing; record deaths at the place.
fn deaths_at(w: &mut World, at: LonLat, n: u64) {
    let p = place_at(w, at);
    let city = city_of(w, p).unwrap_or(p);
    if let Some(node) = w.nodes.get_mut(city.ix()) {
        node.dead += n;
    }
    w.soc.deaths += n;
}

/// Volunteer hosts: a crowd devoted to a demigod (in their Following, big, fired up) raises an army in their name under
/// its own leader, or someone out of it (a few percent a beat, seeded; at most [`MAX_ARMIES_PER_DEMIGOD`] each).
fn volunteer_hosts(w: &mut World, cx: &mut Ctx) {
    let per: Vec<usize> = w
        .demigods
        .iter()
        .map(|d| w.clusters.iter().filter(|c| c.size > 0 && c.army.as_ref().is_some_and(|a| a.demigod == d.id && !a.rogue)).count())
        .collect();
    let mut raised = 0;
    let cands: Vec<(ClusterId, DemigodId)> = w
        .clusters
        .iter()
        .filter(|c| c.army.is_none() && c.size >= 1500)
        .filter_map(|c| {
            let d = w
                .demigods
                .iter()
                .filter(|d| d.alive)
                .find(|d| c.groups.contains(d.following) || get(&c.allegiance, d.id) >= HOST_ALLEGIANCE)?;
            ((c.mood.devotion >= 0.45 || c.mood.anger >= 0.5 || c.need.safety >= 0.4) && per[d.id.ix()] < MAX_ARMIES_PER_DEMIGOD)
                .then_some((c.id, d.id))
        })
        .collect();
    // At most one host a beat, plus one for each demigod who has no army yet: the people around a demigod who just
    // landed arm themselves early (a crowd near them is far likelier to), so armies are in the field in the first beats.
    let mut first_for: Vec<DemigodId> = vec![];
    for (c, d) in cands {
        let unarmed = per[d.ix()] == 0 && !first_for.contains(&d);
        if raised >= 1 && !unarmed {
            continue;
        }
        let (fervor, near) = {
            let k = &w.clusters[c.ix()];
            let at = w.places[k.place.ix()].center;
            let near = w.demigods.get(d.ix()).is_some_and(|x| distance_km(x.pos, at) <= HOST_NEAR_KM.min(w.meta.arena_radius_km * 0.3));
            ((k.mood.devotion + k.mood.anger + k.need.safety) / 3.0 + 0.5 * get(&k.allegiance, d), near)
        };
        let p = 0.01 + 0.1 * (fervor - 0.5).max(0.0) + if unarmed && near { FIRST_HOST_P } else { 0.0 };
        if cx.roll("host", c.0 as u64) >= p {
            continue;
        }
        if raise_host(w, cx, c, d, None).is_some() {
            if unarmed {
                first_for.push(d);
            } else {
                raised += 1;
            }
        }
    }
}

/// Raise a volunteer host for `d` out of crowd `c` (led by `leader` if given, else appointed). Emits `host_raised`.
pub fn raise_host(w: &mut World, cx: &mut Ctx, c: ClusterId, d: DemigodId, leader: Option<CharId>) -> Option<ClusterId> {
    let cl = w.clusters.get(c.ix())?.clone();
    let size = ((cl.size as f32) * (0.04 + 0.05 * cx.roll("host_size", c.0 as u64))).round() as u32;
    let size = size.clamp(200, 30_000).min(cl.size.saturating_sub(1));
    if size < 100 {
        return None;
    }
    let city = city_of(w, cl.place).unwrap_or(cl.place);
    let cname = w.name_of(Ref::Place(city));
    let dname = w.demigods[d.ix()].name.clone();
    // "the Volunteers of Lagos for Dune", then "the Second Volunteers of Lagos for Dune".
    let base = format!("Volunteers of {cname} for {dname}");
    let n = w.clusters.iter().filter(|x| x.army.is_some() && x.label.ends_with(&base)).count();
    const NTH: &[&str] = &["", "Second ", "Third ", "Fourth ", "Fifth ", "Sixth ", "Seventh ", "Eighth ", "Ninth ", "Tenth "];
    let name = match NTH.get(n) {
        Some(o) => format!("the {o}{base}"),
        None => format!("the {}th {base}", n + 1),
    };
    let army = create_army(w, d, c, size, &name, "", cx.beat, true).ok()?;
    w.soc.hosts += 1;
    if let Some(l) = leader.filter(|l| w.characters[l.ix()].alive) {
        let title = format!("General of {name}");
        set_role(w, l, Role::General, Level::Crowd, Some(Ref::Cluster(army)), title, cx.beat);
        if let Some(x) = life_mut(w, l) {
            x.army = Some(army);
            x.mob = None;
        }
        w.clusters[army.ix()].army.as_mut().expect("army").general = Some(l);
    }
    let pos = w.clusters[army.ix()].army.as_ref().expect("army").pos;
    cx.emit(
        w,
        SocKind::HostRaised,
        format!("The {} take up arms for {dname}: {size} volunteers", cl.label),
        Ref::Cluster(c),
        vec![Ref::Cluster(army), Ref::Cluster(c), Ref::Demigod(d)],
        pos,
        Some(city),
        6,
        1,
        json!({ "army": Ref::Cluster(army), "from": Ref::Cluster(c), "demigod": Ref::Demigod(d), "size": size,
                "leader": leader.map(Ref::Char) }),
    );
    appoint(w, cx, army);
    Some(army)
}

/// Emit the events of a judgment made this beat (by the general mind through `Effect::Orders`, or by the rules here).
fn order_events(w: &World, cx: &mut Ctx, army: ClusterId) {
    let cl = &w.clusters[army.ix()];
    let Some(a) = cl.army.as_ref() else { return };
    let Some(o) = a.order.as_ref().filter(|o| o.beat == cx.beat) else { return };
    let gname = alive(w, a.general).map(|g| w.characters[g.ix()].name.clone()).unwrap_or_else(|| "the officers".into());
    let label = cl.label.clone();
    let tname = o.judgment.target.map(|t| w.name_of(t)).unwrap_or_else(|| "nowhere in particular".into());
    let by = w.demigods.get(o.by.ix()).map(|d| d.name.clone()).unwrap_or_default();
    let mut refs = vec![Ref::Cluster(army)];
    refs.extend(a.general.map(Ref::Char));
    refs.extend(o.judgment.target);
    let data = json!({ "army": Ref::Cluster(army), "general": a.general.map(Ref::Char), "by": Ref::Demigod(o.by),
                       "order": o.text, "reading": o.judgment.reading, "target": o.judgment.target,
                       "stance": o.judgment.stance, "speech": o.judgment.speech, "why": o.why, "llm": o.llm,
                       "defect_to": o.judgment.defect_to.map(Ref::Demigod) });
    let actor = a.general.filter(|g| w.characters[g.ix()].alive).map(Ref::Char).unwrap_or(Ref::Cluster(army));
    let (kind, text, mag, val) = match o.judgment.reading {
        Reading::Obey if o.why == "order" => (
            SocKind::OrderReceived,
            format!("{gname} takes {by}'s order and turns {label} toward {tname}"),
            4,
            0,
        ),
        Reading::Obey if o.why == "engage" && o.judgment.stance == Stance::Retreating => (
            SocKind::OrderReceived,
            format!("{gname} refuses battle and pulls {label} back toward {tname}"),
            4,
            0,
        ),
        Reading::Obey => return,
        Reading::Misread => (
            SocKind::OrderMisread,
            format!("{gname} misreads {by}'s order and marches {label} on {tname} instead"),
            5,
            -1,
        ),
        Reading::Stall | Reading::Refuse => (
            SocKind::OrderReceived,
            format!("{gname} refuses {by}'s order and keeps {label} where it is"),
            5,
            -1,
        ),
        Reading::Mutiny => (
            SocKind::Mutiny,
            format!("{gname} turns {label} against {by}: the army answers to no god now"),
            7,
            -3,
        ),
        Reading::Defect => (
            SocKind::Defection,
            format!(
                "{gname} takes {label} over to {}",
                o.judgment.defect_to.map(|d| w.demigods[d.ix()].name.clone()).unwrap_or_default()
            ),
            7,
            -3,
        ),
    };
    cx.emit(w, kind, text, actor, refs, a.pos, Some(cl.place), mag, val, data);
}

/// Every beat, every army: volunteer hosts; generals appointed or replaced; strain and decision points; the
/// general's initiative; supply and morale; marches; battles (army vs army, army vs mob); raids; strikes on demigods;
/// desertion and recruitment.
pub fn step(w: &mut World, cx: &mut Ctx) {
    volunteer_hosts(w, cx);
    let ids: Vec<ClusterId> = w
        .clusters
        .iter()
        .filter(|c| c.size > 0 && c.army.is_some())
        .map(|c| c.id)
        .collect();
    for &id in &ids {
        let a = w.clusters[id.ix()].army.clone().expect("army");
        if a.rogue && w.demigods.iter().all(|d| !d.alive) {
            continue;
        }
        if alive(w, a.general).is_none() {
            appoint(w, cx, id);
        }
        rescale_units(w, id);
        let a = w.clusters[id.ix()].army.clone().expect("army");
        let decided = a.order.as_ref().is_some_and(|o| o.beat == cx.beat);
        // Strain: losses, hunger, low morale, a fallen master, low loyalty.
        let loyalty = alive(w, a.general).map(|g| get(&w.characters[g.ix()].allegiance, a.demigod)).unwrap_or(0.5);
        let master_dead = !a.rogue && !w.demigods.get(a.demigod.ix()).is_some_and(|d| d.alive);
        let raw = (1.0 - a.supply) * 0.6 + (1.0 - a.morale) * 0.6 + if master_dead { 0.8 } else { 0.0 } + if loyalty < 0.2 { 0.3 } else { 0.0 };
        let strain = clamp01(a.strain * 0.75 + 0.25 * raw);
        w.clusters[id.ix()].army.as_mut().expect("army").strain = strain;
        if !decided && !a.rogue && (strain > STRAIN_DECISION || master_dead) {
            let j = judge(w, id, "", None, a.demigod, "strain", cx.beat);
            if j.reading != Reading::Obey || master_dead {
                let _ = apply_orders(w, id, a.demigod, "", &j, "strain", false, cx.beat);
            }
        }
        // Initiative: no target, a dead target, or an order long done.
        let a = w.clusters[id.ix()].army.clone().expect("army");
        let target_ok = a.target.is_some_and(|t| crate::util::is_alive(w, t) && !matches!(t, Ref::Cluster(c) if w.clusters[c.ix()].size == 0));
        let stale = a.order.as_ref().is_none_or(|o| cx.beat > o.beat + 12 && o.why != "order")
            || a.dest.is_some_and(|d| distance_km(a.pos, d) < 5.0 && a.stance != Stance::Holding);
        // A hostile army coming within reach makes a general on their own initiative turn to face it.
        let threat = !matches!(a.target, Some(Ref::Cluster(_)))
            && a.order.as_ref().is_none_or(|o| o.why != "order")
            && w.clusters.iter().any(|o| {
                o.id != id && o.size > 0 && o.army.as_ref().is_some_and(|b| hostile(w, &a, b) && distance_km(a.pos, b.pos) <= 3000.0)
            });
        if a.order.as_ref().is_none_or(|o| o.beat != cx.beat)
            && (!target_ok || threat || (stale && a.order.as_ref().is_none_or(|o| o.why != "order")))
        {
            let j = judge(w, id, "", None, a.demigod, "initiative", cx.beat);
            if j.target != a.target || !target_ok {
                let _ = apply_orders(w, id, a.demigod, "", &j, "initiative", false, cx.beat);
            }
        }
        order_events(w, cx, id);
        supply_and_march(w, cx, id);
    }
    battles(w, cx, &ids);
    suppress_mobs(w, cx, &ids);
    let mut struck: Vec<DemigodId> = vec![];
    for &id in &ids {
        if w.clusters[id.ix()].size == 0 {
            continue;
        }
        strike(w, cx, id, &mut struck);
        raid(w, cx, id);
        desert_and_recruit(w, cx, id);
        rescale_units(w, id);
        // The general and captains ride with the army.
        let a = w.clusters[id.ix()].army.clone().expect("army");
        let place = w.clusters[id.ix()].place;
        for c in a.general.into_iter().chain(a.captains.iter().copied()) {
            let ch = &mut w.characters[c.ix()];
            if ch.alive {
                ch.pos = a.pos;
                ch.place = place;
            }
        }
    }
}

/// Supply and morale, then the march toward the target (speed by the general's competence and the supply).
fn supply_and_march(w: &mut World, cx: &mut Ctx, id: ClusterId) {
    let a = w.clusters[id.ix()].army.clone().expect("army");
    let place = place_at(w, a.pos);
    let city = city_of(w, place);
    let friendly = city
        .and_then(|c| w.nodes.get(c.ix()))
        .map(|n| get(&n.allegiance, a.demigod) >= 0.2)
        .unwrap_or(false);
    let center = w.places[w.meta.arena.ix()].center;
    let outside = distance_km(center, a.pos) > w.meta.arena_radius_km;
    let marching = matches!(a.stance, Stance::Marching | Stance::Raiding | Stance::Retreating | Stance::Rogue | Stance::Fighting | Stance::Besieging);
    let mut supply = a.supply + if friendly { 0.05 } else { 0.0 } - if marching { 0.025 } else { 0.01 } - if outside { 0.05 } else { 0.0 };
    if let Some(c) = city.filter(|_| friendly) {
        if let Some(n) = w.nodes.get_mut(c.ix()) {
            n.supply = r3((n.supply - 0.01).max(0.0));
        }
    }
    supply = clamp01(supply);
    let morale = clamp01(a.morale + if supply > 0.5 { 0.01 } else { -0.03 } + if a.rogue { -0.01 } else { 0.0 });
    let comp = alive(w, a.general).and_then(|g| life(w, g)).map(|l| l.competence).unwrap_or(0.4);
    let dest = match a.stance {
        Stance::Holding => None,
        _ => crate::world_rules::army_dest(w, &a),
    };
    let mut pos = a.pos;
    if let Some(d) = dest {
        let dist = distance_km(a.pos, d);
        let speed = ARMY_KM_PER_BEAT * (0.55 + 0.45 * comp as f64) * (0.6 + 0.4 * supply as f64);
        if dist > 5.0 {
            pos = if dist <= speed { d } else { destination(a.pos, bearing_deg(a.pos, d), speed) };
            pos = [(pos[0] * 1e6).round() / 1e6, (pos[1] * 1e6).round() / 1e6];
        }
    }
    let new_place = place_at(w, pos);
    let c = &mut w.clusters[id.ix()];
    c.place = new_place;
    let ar = c.army.as_mut().expect("army");
    ar.supply = supply;
    ar.morale = morale;
    ar.pos = pos;
    let _ = cx;
}

fn side_of(w: &World, army: ClusterId) -> BattleSide {
    let c = &w.clusters[army.ix()];
    let a = c.army.as_ref().expect("army");
    BattleSide {
        who: Ref::Cluster(army),
        demigod: (!a.rogue).then_some(a.demigod),
        general: alive(w, a.general),
        before: c.size,
        losses: 0,
    }
}

/// Hostile armies within [`CLASH_KM`] fight (each army at most once a beat). A side whose general chose to retreat
/// may slip away. Casualties, heroes, cowards and fallen officers by rules.
fn battles(w: &mut World, cx: &mut Ctx, ids: &[ClusterId]) {
    let mut pairs: Vec<(ClusterId, ClusterId)> = vec![];
    for (i, &x) in ids.iter().enumerate() {
        for &y in &ids[i + 1..] {
            let (cx_, cy) = (&w.clusters[x.ix()], &w.clusters[y.ix()]);
            if cx_.size == 0 || cy.size == 0 {
                continue;
            }
            let (ax, ay) = (cx_.army.as_ref().expect("army"), cy.army.as_ref().expect("army"));
            if hostile(w, ax, ay) && distance_km(ax.pos, ay.pos) <= CLASH_KM {
                pairs.push((x, y));
            }
        }
    }
    let mut fought: Vec<ClusterId> = vec![];
    for (x, y) in pairs {
        if fought.contains(&x) || fought.contains(&y) || w.clusters[x.ix()].size == 0 || w.clusters[y.ix()].size == 0 {
            continue;
        }
        // The engagement is a decision point: a general who hasn't decided this beat judges it by rules (the general
        // mind decided already through `Effect::Orders` when it was asked).
        let mut slipped = false;
        for (k, e) in [(x, y), (y, x)] {
            let a = w.clusters[k.ix()].army.clone().expect("army");
            if a.order.as_ref().is_none_or(|o| o.beat != cx.beat) {
                let j = judge(w, k, "", Some(Ref::Cluster(e)), a.demigod, "engage", cx.beat);
                if j.stance == Stance::Retreating {
                    let _ = apply_orders(w, k, a.demigod, "", &j, "engage", false, cx.beat);
                    order_events(w, cx, k);
                }
            }
            let a = w.clusters[k.ix()].army.clone().expect("army");
            if a.stance == Stance::Retreating {
                let comp = alive(w, a.general).and_then(|g| life(w, g)).map(|l| l.competence).unwrap_or(0.4);
                if cx.roll("slip", k.0 as u64) < 0.4 + 0.4 * comp {
                    slipped = true;
                }
            }
        }
        if slipped {
            continue;
        }
        // Armies that fought in the last few beats mostly regroup (bury the dead, gather stragglers) instead of
        // fighting again at once.
        let fought_last = |k: ClusterId| {
            w.battles.iter().rev().take_while(|b| b.beat + 3 >= cx.beat).any(|b| b.beat < cx.beat && b.sides.iter().any(|s| s.who == Ref::Cluster(k)))
        };
        if (fought_last(x) || fought_last(y)) && cx.roll("regroup", (x.0 as u64) << 20 | y.0 as u64) < 0.8 {
            continue;
        }
        fought.push(x);
        fought.push(y);
        battle(w, cx, x, y);
    }
}

#[allow(clippy::needless_range_loop)]
fn battle(w: &mut World, cx: &mut Ctx, x: ClusterId, y: ClusterId) {
    let at = {
        let (a, b) = (w.clusters[x.ix()].army.as_ref().unwrap().pos, w.clusters[y.ix()].army.as_ref().unwrap().pos);
        [((a[0] + b[0]) / 2.0 * 1e6).round() / 1e6, ((a[1] + b[1]) / 2.0 * 1e6).round() / 1e6]
    };
    let place = place_at(w, at);
    let city = city_of(w, place);
    // Terrain: holding a friendly city is worth a quarter more; world mods harm both; casts on an army this beat
    // (divine meddling) cost it; high Entropy swings it.
    let terrain = |w: &World, k: ClusterId| {
        let a = w.clusters[k.ix()].army.as_ref().unwrap();
        let holds = a.stance == Stance::Holding
            && city.and_then(|c| w.nodes.get(c.ix())).is_some_and(|n| get(&n.allegiance, a.demigod) > 0.2);
        let mods: f32 = w
            .mods
            .iter()
            .filter(|m| distance_km(m.at, at) <= m.radius_km as f64)
            .map(|m| 0.08 * m.per_beat.harm * m.stacks as f32)
            .sum();
        let meddled = w
            .ledger
            .rows
            .iter()
            .rev()
            .take_while(|r| r.beat == cx.beat)
            .any(|r| r.valence < 0 && r.targets.contains(&Ref::Cluster(k)) && matches!(r.source, Source::God { .. } | Source::Agent { .. }) || (r.verb == "cast" && r.targets.contains(&Ref::Cluster(k))));
        (if holds { 1.25 } else { 1.0 }) * (1.0 - mods.min(0.4)) * if meddled { 0.8 } else { 1.0 }
    };
    let swing = 0.15 + 0.05 * crate::world_rules::entropy_level(w.meta.entropy) as f32;
    let sx = strength(w, x) * terrain(w, x) * (1.0 - swing + 2.0 * swing * cx.roll("battle_x", x.0 as u64 ^ ((y.0 as u64) << 20)));
    let sy = strength(w, y) * terrain(w, y) * (1.0 - swing + 2.0 * swing * cx.roll("battle_y", y.0 as u64 ^ ((x.0 as u64) << 20)));
    let r = sx / (sx + sy).max(1.0);
    let winner: Option<u8> = if r > 0.55 { Some(0) } else if r < 0.45 { Some(1) } else { None };
    let margin = (r - 0.5).abs() * 2.0;
    let mut sides = [side_of(w, x), side_of(w, y)];
    let ks = [x, y];
    for k in 0..2 {
        let size = sides[k].before as f32;
        let frac = match winner {
            Some(win) if win as usize == k => 0.03 + 0.05 * (1.0 - margin),
            Some(_) => 0.08 + 0.15 * margin,
            None => 0.07,
        };
        let loss = ((size * frac).ceil() as u32).min(sides[k].before);
        sides[k].losses = loss;
        let home = w
            .clusters
            .get(w.clusters[ks[k].ix()].army.as_ref().unwrap().from.ix())
            .map(|f| f.place);
        let c = &mut w.clusters[ks[k].ix()];
        c.size -= loss;
        c.dead += loss;
        let a = c.army.as_mut().unwrap();
        a.losses += loss;
        a.battles += 1;
        let won = winner == Some(k as u8);
        a.morale = clamp01(a.morale + if won { 0.08 } else if winner.is_none() { -0.05 } else { -0.15 - 0.15 * margin });
        a.strain = clamp01(a.strain + if won { -0.05 } else { 0.15 });
        if !won && (a.morale < 0.25 || (winner.is_some() && margin > 0.5)) {
            a.stance = Stance::Retreating;
            if let Some(h) = home {
                a.target = Some(Ref::Place(h));
                a.dest = None;
            }
        }
    }
    let total: u64 = sides.iter().map(|s| s.losses as u64).sum();
    deaths_at(w, at, total);
    // Officers fall; heroes and cowards emerge.
    let mut heroes = vec![];
    let mut cowards = vec![];
    let mut fallen = vec![];
    for k in 0..2 {
        let won = winner == Some(k as u8);
        let a = w.clusters[ks[k].ix()].army.clone().unwrap();
        if let Some(g) = alive(w, a.general) {
            let courage = life(w, g).map(|l| l.courage).unwrap_or(0.5);
            let p = if won { 0.02 } else { 0.06 + 0.25 * margin * (1.0 - courage) };
            if cx.roll("general_falls", g.0 as u64) < p {
                fallen.push(g);
            }
        }
        for c in a.captains.iter().copied().filter(|c| w.characters[c.ix()].alive) {
            if cx.roll("captain_falls", c.0 as u64) < if won { 0.05 } else { 0.15 } {
                fallen.push(c);
            }
        }
        // A hero (usually on the winning side), a coward (usually on the losing side).
        // Big battles make legends (a skirmish rarely does); a captain is the likelier hero.
        let big = (sides[0].losses + sides[1].losses) >= 400;
        let p_hero = if !big { 0.05 } else if won { 0.35 } else if winner.is_none() { 0.15 } else { 0.05 };
        if cx.roll("hero", ks[k].0 as u64) < p_hero {
            let pick = a
                .captains
                .iter()
                .copied()
                .filter(|c| w.characters[c.ix()].alive && !fallen.contains(c))
                .find(|c| life(w, *c).is_some_and(|l| l.courage >= 0.6));
            let hero = match pick {
                Some(c) if cx.roll("hero_who", c.0 as u64) < 0.7 => Some(c),
                _ => {
                    let mut spec = Spec::new(
                        Role::Hero,
                        Level::Cell,
                        Some(Ref::Cluster(ks[k])),
                        format!("hero of {}", w.name_of(Ref::Place(city.unwrap_or(place)))),
                        format!("the hero of the battle near {}", w.name_of(Ref::Place(city.unwrap_or(place)))),
                    );
                    spec.courage = 0.9;
                    spec.competence = 0.3;
                    emerge(w, cx, ks[k], spec, false)
                }
            };
            if let Some(h) = hero {
                if let Some(l) = life_mut(w, h) {
                    l.deeds += 1;
                    l.kills += 3 + (l.courage * 10.0) as u32;
                    l.army = Some(ks[k]);
                    if l.role == Role::Soldier || l.role == Role::Commoner {
                        l.role = Role::Hero;
                    }
                }
                heroes.push(h);
            }
        }
        let p_coward = if won { 0.02 } else if big { 0.2 } else { 0.05 };
        if cx.roll("coward", ks[k].0 as u64) < p_coward {
            let pick = a
                .captains
                .iter()
                .copied()
                .filter(|c| w.characters[c.ix()].alive && !fallen.contains(c) && !heroes.contains(c))
                .min_by(|p, q| {
                    let s = |c: &CharId| life(w, *c).map(|l| l.courage).unwrap_or(0.5);
                    s(p).total_cmp(&s(q)).then(p.cmp(q))
                })
                .filter(|c| life(w, *c).is_some_and(|l| l.courage < 0.45));
            let coward = match pick {
                Some(c) => {
                    if let Some(ar) = w.clusters[ks[k].ix()].army.as_mut() {
                        ar.captains.retain(|x| *x != c);
                    }
                    let title = format!("the captain who ran at {}", w.places[place.ix()].name);
                    if let Some(l) = life_mut(w, c) {
                        l.role = Role::Coward;
                        l.rank.title = title;
                        l.goal = Goal::Flee;
                    }
                    if let Some(g) = alive(w, a.general) {
                        people::grudge(w, c, Ref::Char(g));
                    }
                    Some(c)
                }
                None => {
                    let mut spec = Spec::new(
                        Role::Coward,
                        Level::Cell,
                        Some(Ref::Cluster(ks[k])),
                        format!("the soldier who ran at {}", w.name_of(Ref::Place(city.unwrap_or(place)))),
                        format!("the deserter from the battle near {}", w.name_of(Ref::Place(city.unwrap_or(place)))),
                    );
                    spec.courage = -0.9;
                    emerge(w, cx, ks[k], spec, false)
                }
            };
            if let Some(c) = coward {
                cowards.push(c);
            }
        }
    }
    let names: Vec<String> = ks.iter().map(|k| w.clusters[k.ix()].label.clone()).collect();
    let gnames: Vec<String> = sides
        .iter()
        .map(|s| s.general.map(|g| format!(" under {}", w.characters[g.ix()].name)).unwrap_or_default())
        .collect();
    let where_ = crate::sim::near_name(w, at);
    let text = match winner {
        Some(k) => format!(
            "{}{} beats {}{} near {where_}: {} and {} fall",
            crate::sim::mobs::cap(&names[k as usize]), gnames[k as usize], names[1 - k as usize], gnames[1 - k as usize],
            sides[k as usize].losses, sides[1 - k as usize].losses
        ),
        None => format!(
            "{}{} and {}{} bleed each other near {where_}: {} and {} fall",
            crate::sim::mobs::cap(&names[0]), gnames[0], names[1], gnames[1], sides[0].losses, sides[1].losses
        ),
    };
    let id = w.soc.battles;
    w.soc.battles += 1;
    let b = Battle {
        id,
        beat: cx.beat,
        at,
        place: city.unwrap_or(place),
        sides: sides.clone(),
        winner,
        heroes: heroes.clone(),
        cowards: cowards.clone(),
        fallen: fallen.clone(),
        text: text.clone(),
    };
    let mut refs = vec![Ref::Cluster(x), Ref::Cluster(y)];
    refs.extend(sides.iter().filter_map(|s| s.general.map(Ref::Char)));
    let mag = (6 + (total > 2000) as u8 + (total > 10_000) as u8).min(9);
    cx.emit(
        w,
        SocKind::Battle,
        text,
        Ref::Place(city.unwrap_or(place)),
        refs,
        at,
        Some(place),
        mag,
        -4,
        json!({ "battle": b }),
    );
    for (k, s) in sides.iter().enumerate() {
        if s.losses > 0 {
            cx.emit(
                w,
                SocKind::Casualties,
                format!("{} loses {} dead near {where_}", crate::sim::mobs::cap(&names[k]), s.losses),
                Ref::Place(city.unwrap_or(place)),
                vec![Ref::Cluster(ks[k])],
                at,
                Some(place),
                if s.losses > 1000 { 5 } else { 3 },
                -3,
                json!({ "who": Ref::Cluster(ks[k]), "dead": s.losses, "battle": id }),
            );
        }
    }
    for h in &heroes {
        let n = w.characters[h.ix()].name.clone();
        cx.emit(
            w,
            SocKind::Hero,
            format!("{n} holds the line near {where_} and becomes a legend overnight"),
            Ref::Char(*h),
            vec![Ref::Char(*h)],
            at,
            Some(place),
            5,
            2,
            json!({ "person": Ref::Char(*h), "battle": id }),
        );
    }
    for c in &cowards {
        let n = w.characters[c.ix()].name.clone();
        cx.emit(
            w,
            SocKind::Coward,
            format!("{n} breaks and runs near {where_}; the army will not forget"),
            Ref::Char(*c),
            vec![Ref::Char(*c)],
            at,
            Some(place),
            4,
            -2,
            json!({ "person": Ref::Char(*c), "battle": id }),
        );
    }
    for f in fallen.clone() {
        kill(w, cx, f, &format!("cut down in the battle near {where_}"), None);
    }
    // A key witness of a big battle, out of the nearest city's biggest crowd.
    if total >= 3000 {
        if let Some(c) = city {
            let crowd = crate::sim::hier::crowds_under(w, c)
                .into_iter()
                .max_by_key(|k| (w.clusters[k.ix()].size, std::cmp::Reverse(k.0)));
            if let Some(k) = crowd {
                let spec = Spec::new(
                    Role::Witness,
                    Level::Person,
                    Some(Ref::Place(c)),
                    format!("witness to the battle near {where_}"),
                    format!("the one who saw the battle near {where_}"),
                );
                emerge(w, cx, k, spec, false);
            }
        }
    }
    w.battles.push(b);
    if w.battles.len() > BATTLES_KEPT {
        w.battles.remove(0);
    }
}

/// Armies put down hostile mobs in their city: the mob scatters with casualties (more under a cruel general, and the
/// city hates them for it); a surviving ringleader holds a grudge.
fn suppress_mobs(w: &mut World, cx: &mut Ctx, ids: &[ClusterId]) {
    for &id in ids {
        let Some(a) = w.clusters[id.ix()].army.clone() else { continue };
        if w.clusters[id.ix()].size == 0 || a.rogue {
            continue;
        }
        let here = city_of(w, place_at(w, a.pos));
        // A hostile mob (rallying behind a rival, looting, attacking) is put down; a mob that is only angry is put down
        // when the general hunted it, else now and then (an army in town doesn't always charge the crowd).
        let Some(mi) = w.mobs.iter().position(|m| {
            m.ended.is_none()
                && Some(m.city) == here
                && m.side != Some(a.demigod)
                && m.size > 0
                && (hostile_mob(w, &a, m)
                    || a.target == Some(Ref::Mob(m.id))
                    || cx.roll("suppress", (id.0 as u64) << 20 | m.id.0 as u64) < 0.25)
        }) else {
            continue;
        };
        let m = w.mobs[mi].clone();
        let g = alive(w, a.general);
        let cruelty = g.and_then(|g| life(w, g)).map(|l| l.cruelty).unwrap_or(0.0);
        let frac = 0.1 + 0.25 * cruelty.max(0.0);
        let dead = ((m.size as f32) * frac).ceil() as u32;
        crate::sim::mobs::casualties(w, mi, dead);
        let army_loss = ((m.size as f32) * 0.02).ceil() as u32;
        let c = &mut w.clusters[id.ix()];
        let army_loss = army_loss.min(c.size.saturating_sub(1));
        c.size -= army_loss;
        c.dead += army_loss;
        deaths_at(w, m.pos, (dead + army_loss) as u64);
        if let Some(city) = here {
            let d = Mood { anger: 0.04 + 0.08 * cruelty.max(0.0), fear: 0.08, hope: -0.03, devotion: 0.0 };
            crate::sim::hier::push_down(w, city, d, 1.0);
            for k in crate::sim::hier::crowds_under(w, city) {
                bump(&mut w.clusters[k.ix()].allegiance, a.demigod, -0.04 - 0.06 * cruelty.max(0.0), -1.0, 1.0);
            }
        }
        let gname = g.map(|g| w.characters[g.ix()].name.clone()).unwrap_or_else(|| w.clusters[id.ix()].label.clone());
        let massacre = cruelty > 0.4;
        let rl = alive(w, m.ringleader).map(|r| format!(" under {}", w.characters[r.ix()].name)).unwrap_or_default();
        let text = if massacre {
            format!("{gname} turns the soldiers loose on {}{rl}: {dead} rioters cut down in the streets, {army_loss} soldiers lost", m.name)
        } else {
            format!("{gname}'s soldiers break up {}{rl}: {dead} rioters dead, {army_loss} soldiers lost", m.name)
        };
        let id_b = w.soc.battles;
        w.soc.battles += 1;
        let b = Battle {
            id: id_b,
            beat: cx.beat,
            at: m.pos,
            place: m.city,
            sides: [
                BattleSide { who: Ref::Cluster(id), demigod: Some(a.demigod), general: g, before: w.clusters[id.ix()].size + army_loss, losses: army_loss },
                BattleSide { who: Ref::Mob(m.id), demigod: m.side, general: alive(w, m.ringleader), before: m.size, losses: dead },
            ],
            winner: Some(0),
            heroes: vec![],
            cowards: vec![],
            fallen: vec![],
            text: text.clone(),
        };
        let mut refs = vec![Ref::Cluster(id), Ref::Mob(m.id)];
        refs.extend(g.map(Ref::Char));
        refs.extend(alive(w, m.ringleader).map(Ref::Char));
        cx.emit(w, SocKind::Battle, text, Ref::Cluster(id), refs, m.pos, Some(m.place), if massacre { 7 } else { 6 }, -4, json!({ "battle": b }));
        w.battles.push(b);
        if w.battles.len() > BATTLES_KEPT {
            w.battles.remove(0);
        }
        if let (Some(r), Some(g)) = (alive(w, m.ringleader), g) {
            people::grudge(w, r, Ref::Char(g));
        }
        crate::sim::mobs::dissolve(w, cx, mi, crate::sim::mobs::SCATTERED);
    }
}

/// An army reaching a rival demigod (its target, or any within [`STRIKE_KM`]) storms them: a damage proposal.
fn strike(w: &mut World, cx: &mut Ctx, id: ClusterId, struck: &mut Vec<DemigodId>) {
    let c = &w.clusters[id.ix()];
    let a = c.army.as_ref().expect("army");
    for d in w.demigods.iter().filter(|d| d.alive && (a.rogue || d.id != a.demigod)) {
        // One army storms a demigod per beat (the others can't all reach them at once).
        if struck.contains(&d.id) {
            continue;
        }
        let aimed = a.target == Some(Ref::Demigod(d.id));
        if distance_km(a.pos, d.pos) > if aimed { STRIKE_KM } else { STRIKE_KM / 2.0 } {
            continue;
        }
        let comp = alive(w, a.general).and_then(|g| life(w, g)).map(|l| l.competence).unwrap_or(0.4);
        // A demigod is not easily killed by mortals: a big, well-led army takes a point or two a beat.
        let amount = ((0.5 + c.size as f32 / 40_000.0) * (0.6 + 0.6 * comp)).min(2.0);
        let gname = alive(w, a.general).map(|g| format!(" under {}", w.characters[g.ix()].name)).unwrap_or_default();
        cx.out.proposals.push(Proposal {
            source: Source::Rules,
            intent: None,
            effect: Effect::Damage {
                who: Ref::Demigod(d.id),
                amount: r3(amount),
                cause: format!("overrun by {}", c.label),
            },
            act: format!("{}{gname} storms {}", c.label, d.name),
            actor: Ref::Cluster(id),
            magnitude: 7,
            valence: -4,
            witnesses: vec![Ref::Demigod(d.id), Ref::Cluster(id)],
            cause: None,
        });
        struck.push(d.id);
    }
}

/// A raiding (or rogue) army in a city: it takes supply and lives, and the city remembers.
fn raid(w: &mut World, cx: &mut Ctx, id: ClusterId) {
    let a = w.clusters[id.ix()].army.clone().expect("army");
    if !matches!(a.stance, Stance::Raiding | Stance::Rogue) {
        return;
    }
    let Some(city) = city_of(w, place_at(w, a.pos)) else { return };
    if w.nodes.get(city.ix()).is_some_and(|n| !a.rogue && get(&n.allegiance, a.demigod) > 0.3) {
        return;
    }
    let crowds = crate::sim::hier::crowds_under(w, city);
    let Some(k) = crowds.iter().copied().max_by_key(|k| (w.clusters[k.ix()].size, std::cmp::Reverse(k.0))) else {
        return;
    };
    let cruelty = alive(w, a.general).and_then(|g| life(w, g)).map(|l| l.cruelty).unwrap_or(0.0);
    let dead = ((w.clusters[k.ix()].size as f32) * (0.002 + 0.004 * cruelty.max(0.0))).ceil() as u32;
    let crowd = &mut w.clusters[k.ix()];
    let dead = dead.min(crowd.size);
    crowd.size -= dead;
    crowd.dead += dead;
    crowd.mood.anger = clamp01(crowd.mood.anger + 0.08);
    crowd.mood.fear = clamp01(crowd.mood.fear + 0.1);
    if !a.rogue {
        bump(&mut crowd.allegiance, a.demigod, -0.1, -1.0, 1.0);
    }
    let label = crowd.label.clone();
    if let Some(n) = w.nodes.get_mut(city.ix()) {
        n.supply = r3((n.supply - 0.06).max(0.0));
    }
    let ar = w.clusters[id.ix()].army.as_mut().expect("army");
    ar.supply = clamp01(ar.supply + 0.1);
    deaths_at(w, a.pos, dead as u64);
    let gname = alive(w, a.general).map(|g| w.characters[g.ix()].name.clone()).unwrap_or_else(|| w.clusters[id.ix()].label.clone());
    let cname = w.name_of(Ref::Place(city));
    let mut refs = vec![Ref::Cluster(id), Ref::Cluster(k)];
    refs.extend(alive(w, a.general).map(Ref::Char));
    cx.emit(
        w,
        SocKind::Looting,
        format!("{gname}'s men strip {cname} bare; {dead} of the {label} die"),
        Ref::Cluster(id),
        refs,
        a.pos,
        Some(city),
        if dead > 500 { 5 } else { 4 },
        -3,
        json!({ "army": Ref::Cluster(id), "city": Ref::Place(city), "dead": dead }),
    );
}

/// Low morale bleeds deserters back to the home crowd; armies in cities that love their demigod draw recruits.
fn desert_and_recruit(w: &mut World, cx: &mut Ctx, id: ClusterId) {
    let a = w.clusters[id.ix()].army.clone().expect("army");
    let size = w.clusters[id.ix()].size;
    if a.morale < 0.4 && size > 50 {
        let n = ((size as f32) * (0.4 - a.morale) * 0.08).ceil() as u32;
        let n = n.min(size - 1);
        w.clusters[id.ix()].size -= n;
        if let Some(home) = w.clusters.get_mut(a.from.ix()) {
            home.size += n;
        }
        w.soc.deserters += n as u64;
        if n >= 200 {
            let label = w.clusters[id.ix()].label.clone();
            let mut refs = vec![Ref::Cluster(id)];
            refs.extend(alive(w, a.general).map(Ref::Char));
            cx.emit(
                w,
                SocKind::Desertion,
                format!("{n} slip away from {label} in the night"),
                Ref::Cluster(id),
                refs,
                a.pos,
                Some(w.clusters[id.ix()].place),
                if n >= 1000 { 5 } else { 3 },
                -1,
                json!({ "army": Ref::Cluster(id), "deserters": n, "morale": a.morale }),
            );
        }
    }
    if a.rogue {
        return;
    }
    let Some(city) = city_of(w, place_at(w, a.pos)) else { return };
    let mut got = 0u32;
    for k in crate::sim::hier::crowds_under(w, city) {
        let c = &w.clusters[k.ix()];
        if get(&c.allegiance, a.demigod) >= 0.4 && c.size > 500 {
            let n = ((c.size as f32) * 0.004).ceil() as u32;
            let n = n.min(2000 - got.min(2000));
            w.clusters[k.ix()].size -= n;
            got += n;
        }
    }
    if got > 0 {
        w.clusters[id.ix()].size += got;
        let ar = w.clusters[id.ix()].army.as_mut().expect("army");
        ar.morale = clamp01(ar.morale + 0.01);
    }
}

/// The plugin's decision points this beat: generals facing an engagement (a hostile army within twice the clash
/// range) or strain past [`STRAIN_DECISION`]. `(army, why, enemy)`.
pub fn decision_points(w: &World) -> Vec<(ClusterId, &'static str, Option<Ref>)> {
    let live: Vec<(ClusterId, Army)> = w
        .clusters
        .iter()
        .filter(|c| c.size > 0)
        .filter_map(|c| c.army.clone().map(|a| (c.id, a)))
        .collect();
    let mut out = vec![];
    for (id, a) in &live {
        if alive(w, a.general).is_none() {
            continue;
        }
        let enemy = live
            .iter()
            .filter(|(o, b)| o != id && hostile(w, a, b) && distance_km(a.pos, b.pos) <= CLASH_KM * 2.0)
            .min_by(|x, y| distance_km(a.pos, x.1.pos).total_cmp(&distance_km(a.pos, y.1.pos)).then(x.0.cmp(&y.0)))
            .map(|x| Ref::Cluster(x.0));
        if let Some(e) = enemy {
            out.push((*id, "engage", Some(e)));
        } else if !a.rogue && a.strain > STRAIN_DECISION {
            out.push((*id, "strain", None));
        }
    }
    out
}

//! Crowds by rules: supply and needs, mood dynamics, hardship, migration, cults and crowd-vs-crowd clashes, and
//! witnesses stepping out of the crowds after big acts.

use std::collections::BTreeMap;

use fn7_model::seed::hash_str;
use fn7_model::*;
use serde_json::json;

use crate::geo::{city_of, distance_km};
use crate::sim::hier::crowds_under;
use crate::sim::people::{self, emerge, life_mut, Spec};
use crate::sim::{clamp01, r3, Ctx};
use crate::util::{bump, get};

/// Per-city chance per beat of a hardship shock (a failed harvest, a fire, a plague).
pub const HARDSHIP_P: f32 = 0.004;
/// Share of a crowd that may migrate per beat (at most).
pub const MIGRATE_MAX: f32 = 0.015;
/// Clusters may grow to this multiple of the starting count through refugee crowds.
pub const REFUGEE_GROWTH: f32 = 1.5;

const HARDSHIPS: &[(&str, f32, f32)] = &[
    ("the harvest fails", 0.35, 0.0),
    ("fire guts the warehouses", 0.3, 0.001),
    ("a fever runs through the tenements", 0.15, 0.004),
    ("the wells go bad", 0.25, 0.002),
    ("the bridges are down and the roads washed out", 0.3, 0.0),
];

/// A demigod's landing (setup, after the nodes exist): the city they land in hoards and panics (its supply drops, its
/// crowds boil over: angry and afraid past the mob thresholds) and the streets around it stir, so the society's first
/// events happen in the first beats and around the demigods.
pub fn stir_homes(w: &mut World) {
    let reach = if w.is_planet() { crate::sim::mobs::NEAR_DEMIGOD_KM } else { w.meta.arena_radius_km * 0.2 };
    let homes: Vec<(LonLat, Option<PlaceId>)> = w
        .demigods
        .iter()
        .map(|d| {
            let p = crate::geo::place_at(w, d.pos);
            // The city they land in (or, off the map of cities, the nearest one).
            let city = city_of(w, p).or_else(|| {
                supply_homes(w)
                    .into_iter()
                    .min_by(|a, b| {
                        let (x, y) = (distance_km(w.places[a.ix()].center, d.pos), distance_km(w.places[b.ix()].center, d.pos));
                        x.total_cmp(&y).then(a.cmp(b))
                    })
            });
            (d.pos, city)
        })
        .collect();
    for (i, (pos, city)) in homes.into_iter().enumerate() {
        if let Some(n) = city.and_then(|c| w.nodes.get_mut(c.ix())) {
            n.supply = n.supply.min(0.55);
        }
        for k in 0..w.clusters.len() {
            let c = &w.clusters[k];
            if c.army.is_some() || c.size == 0 {
                continue;
            }
            let home = city.is_some() && city_of(w, c.place) == city;
            let near = distance_km(w.places[c.place.ix()].center, pos) <= reach;
            if !home && !near {
                continue;
            }
            let x = fn7_model::seed::Rng::derive(w.meta.seed, &[hash_str("stir"), i as u64, k as u64]).f64() as f32;
            let c = &mut w.clusters[k];
            let m = &mut c.mood;
            if home {
                m.anger = clamp01(m.anger.max(MOB_ANGER_STIR.max(c.baseline.anger + 0.24) + 0.08 * x));
                m.fear = clamp01(m.fear.max(1.04 - m.anger + 0.06 * x));
            } else {
                m.anger = clamp01(m.anger + 0.1 + 0.08 * x);
                m.fear = clamp01(m.fear + 0.1);
            }
            c.stance = crate::sim::hier::stance_of(&c.mood);
        }
    }
    crate::sim::hier::rollup(w);
}

/// The anger a demigod's landing raises their city's crowds to (at least; just past [`crate::sim::mobs::MOB_ANGER`]).
const MOB_ANGER_STIR: f32 = 0.66;

/// Supply-holding places: cities (planet) or districts (a city arena).
fn supply_homes(w: &World) -> Vec<PlaceId> {
    w.places.iter().filter(|p| city_of(w, p.id) == Some(p.id)).map(|p| p.id).collect()
}

/// Supply per city, then every crowd's needs and mood.
pub fn needs_and_moods(w: &mut World, cx: &mut Ctx) {
    let center = w.places[w.meta.arena.ix()].center;
    let radius = w.meta.arena_radius_km;
    let level = crate::world_rules::entropy_level(w.meta.entropy) as f32;
    // Armies by city (for load and danger).
    let armies: Vec<(LonLat, DemigodId, bool, u32)> = w
        .clusters
        .iter()
        .filter(|c| c.size > 0)
        .filter_map(|c| c.army.as_ref().map(|a| (a.pos, a.demigod, a.rogue, c.size)))
        .collect();
    let recent_battles: Vec<(LonLat, u32)> = w
        .battles
        .iter()
        .filter(|b| b.beat + 1 >= cx.beat)
        .map(|b| (b.at, b.sides[0].losses + b.sides[1].losses))
        .collect();
    let live_mob_cities: Vec<PlaceId> = w.mobs.iter().filter(|m| m.ended.is_none()).map(|m| m.city).collect();
    for h in supply_homes(w) {
        let p = w.places[h.ix()].clone();
        let Some(node) = w.nodes.get(h.ix()) else { continue };
        let load = node.population as f32 / node.base.max(1) as f32;
        let army_load: f32 = armies
            .iter()
            .filter(|(pos, ..)| distance_km(*pos, p.center) < p.radius_km.max(60.0))
            .map(|(.., s)| *s as f32)
            .sum::<f32>()
            / (node.population.max(1) as f32);
        let outside = distance_km(center, p.center) > radius;
        let mods: f32 = w
            .mods
            .iter()
            .filter(|m| distance_km(m.at, p.center) <= m.radius_km as f64 + p.radius_km)
            .map(|m| m.per_beat.harm * m.stacks.min(3) as f32)
            .sum();
        // Supply relaxes toward what the place can sustain: crowded (refugees), at war, under an army, blighted by
        // Time's circle or a world mod, it sustains less; shocks, looting and raids knock it down from there.
        let war = node.need.safety;
        let target = 1.15 / load.max(0.3).powf(0.7) - 0.25 * war - 0.4 * army_load.min(1.0) - if outside { 0.2 } else { 0.0 }
            - 0.15 * mods.min(3.0);
        let noise = (cx.roll("supply", h.0 as u64) - 0.5) * 0.03;
        let mut s = node.supply + 0.06 * (target - node.supply) + noise;
        if cx.roll("hardship", h.0 as u64) < HARDSHIP_P * (1.0 + level * 0.5) {
            let mut rng = cx.rng("hardship_kind", h.0 as u64);
            let (what, hit, kill) = *rng.pick(HARDSHIPS).expect("non-empty");
            s -= hit;
            let mut dead = 0u64;
            if kill > 0.0 {
                for k in crowds_under(w, h) {
                    let c = &mut w.clusters[k.ix()];
                    let d = ((c.size as f32) * kill).ceil() as u32;
                    let d = d.min(c.size);
                    c.size -= d;
                    c.dead += d;
                    dead += d as u64;
                }
            }
            if let Some(n) = w.nodes.get_mut(h.ix()) {
                n.dead += dead;
            }
            w.soc.deaths += dead;
            let pname = p.name.clone();
            cx.emit(
                w,
                SocKind::Hardship,
                if dead > 0 { format!("In {pname} {what}: {dead} dead") } else { format!("In {pname} {what}") },
                Ref::Place(h),
                vec![Ref::Place(h)],
                p.center,
                Some(h),
                if dead > 1000 { 6 } else { 5 },
                -3,
                json!({ "place": Ref::Place(h), "what": what, "supply_hit": hit, "dead": dead }),
            );
        }
        if let Some(n) = w.nodes.get_mut(h.ix()) {
            n.supply = r3(s.clamp(0.0, 1.5));
        }
    }
    // Crowds.
    let demigods: Vec<(DemigodId, LonLat)> = w.alive_demigods().map(|d| (d.id, d.pos)).collect();
    let cults_in: BTreeMap<PlaceId, usize> = {
        let mut m = BTreeMap::new();
        for c in &w.cells {
            if c.ended.is_none() && c.kind == CellKind::Congregation {
                if let Some(city) = w.clusters.get(c.crowd.ix()).and_then(|k| city_of(w, k.place)) {
                    *m.entry(city).or_insert(0) += 1;
                }
            }
        }
        m
    };
    for i in 0..w.clusters.len() {
        let c = &w.clusters[i];
        if c.army.is_some() || c.size == 0 {
            continue;
        }
        let city = city_of(w, c.place).unwrap_or(c.place);
        let cp = w.places[city.ix()].center;
        let supply = w.nodes.get(city.ix()).map(|n| n.supply).unwrap_or(1.0);
        let food = clamp01(1.1 - supply);
        let outside = distance_km(center, cp) > radius;
        let mods: Vec<&WorldMod> = w.mods.iter().filter(|m| distance_km(m.at, cp) <= m.radius_km as f64 + 30.0).collect();
        let mod_harm: f32 = mods.iter().map(|m| (m.per_beat.harm + m.per_beat.fear) * m.stacks.min(3) as f32).sum();
        let hostile_army = armies.iter().any(|(pos, d, rogue, _)| {
            distance_km(*pos, cp) < 300.0 && (*rogue || get(&c.allegiance, *d) < 0.1)
        });
        let battle = recent_battles.iter().any(|(at, _)| distance_km(*at, cp) < 400.0);
        let mob = live_mob_cities.contains(&city);
        let danger = (if outside { 0.35 } else { 0.0 })
            + 0.3 * mod_harm.min(2.0)
            + if hostile_army { 0.35 } else { 0.0 }
            + if battle { 0.3 } else { 0.0 }
            + if mob { 0.2 } else { 0.0 }
            + 0.05 * level;
        let safety = clamp01(c.need.safety * 0.6 + 0.4 * danger.min(1.0));
        let worship = c.worship.iter().map(|x| x.1).fold(0.0, f32::max);
        let cult = cults_in.get(&city).copied().unwrap_or(0) > 0;
        let hardship = food.max(safety);
        let faith = clamp01(0.2 + 0.5 * hardship - 0.5 * worship - if cult { 0.25 } else { 0.0 });
        let n = (cx.roll("crowd_noise", i as u64) - 0.5) * 0.03;
        let m = c.mood;
        // Pushes against the drift back to the crowd's baseline (4% of the gap a beat): at a hunger of 0.4 a crowd
        // settles about a quarter angrier than usual, at a danger of 0.5 about a third more afraid.
        let mut mood = Mood {
            anger: clamp01(m.anger + 0.028 * food * (1.0 - 0.4 * m.fear) + 0.01 * safety - 0.006 * m.hope + n),
            fear: clamp01(m.fear + 0.03 * safety - 0.004 + n * 0.5),
            hope: clamp01(m.hope + 0.02 * (supply - 1.0).clamp(-1.0, 0.4) - 0.01 * safety - n * 0.5),
            devotion: clamp01(m.devotion + 0.02 * faith * hardship),
        };
        // World mods flow down to every crowd under them (gently).
        for wm in &mods {
            let k = wm.stacks.min(3) as f32 * 0.25;
            let e = wm.per_beat;
            mood.anger = clamp01(mood.anger + e.anger * k);
            mood.fear = clamp01(mood.fear + e.fear * k);
            mood.hope = clamp01(mood.hope + e.hope * k);
            mood.devotion = clamp01(mood.devotion + e.devotion * k);
        }
        // Recruitment drift: a demigod nearby pulls the devout and the desperate toward them.
        let mut alleg = c.allegiance.clone();
        for (d, pos) in &demigods {
            let dist = distance_km(*pos, cp);
            let reach = if w.is_planet() { 1500.0 } else { 15.0 };
            if dist < reach {
                let pull = 0.015 * (1.0 - (dist / reach) as f32) * (0.5 + mood.devotion + 0.5 * hardship);
                bump(&mut alleg, *d, pull, -1.0, 1.0);
            }
        }
        for a in alleg.iter_mut() {
            a.1 = r3(a.1);
        }
        let c = &mut w.clusters[i];
        c.need = Need { food: r3(food), safety, faith };
        c.mood = mood;
        c.allegiance = alleg;
        c.stance = crate::sim::hier::stance_of(&mood);
    }
    witnesses(w, cx);
}

/// Big acts this beat (casts, eliminations, heavy harm on demigods, magnitude ≥ 7) get a witness out of the biggest
/// crowd of the city where they happened (at most one per beat).
fn witnesses(w: &mut World, cx: &mut Ctx) {
    let big: Option<(LedgerId, LonLat, String)> = w
        .ledger
        .rows
        .iter()
        .rev()
        .take_while(|r| r.beat == cx.beat)
        .filter(|r| r.magnitude >= 7 && SocKind::of(&r.verb).is_none() && r.at.is_some())
        .filter(|r| matches!(r.verb.as_str(), "cast" | "eliminate" | "damage" | "happen" | "revive" | "dispatch" | "raise_army"))
        .max_by_key(|r| (r.magnitude, std::cmp::Reverse(r.id)))
        .map(|r| (r.id, r.at.expect("checked"), r.act.clone()));
    let Some((row, at, act)) = big else { return };
    if !cx.can_emerge() {
        return;
    }
    let p = crate::geo::place_at(w, at);
    let Some(city) = city_of(w, p) else { return };
    let Some(k) = crowds_under(w, city).into_iter().max_by_key(|k| (w.clusters[k.ix()].size, std::cmp::Reverse(k.0))) else {
        return;
    };
    let short: String = act.chars().take(60).collect();
    let spec = Spec::new(
        Role::Witness,
        Level::Person,
        Some(Ref::Ledger(row)),
        format!("witness: {short}"),
        format!("the one who saw it: {short}"),
    );
    emerge(w, cx, k, spec, false);
}

/// People leave danger and hunger (a share per beat by pressure) for the best city within reach: safe, fed, inside
/// Time's circle, leaning to the demigod they love. Refugee crowds form at the destination (or join its biggest crowd
/// once the cluster table has grown by half). One `migration` event per origin city per beat for big flows.
pub fn migrate(w: &mut World, cx: &mut Ctx) {
    let center = w.places[w.meta.arena.ix()].center;
    let radius = w.meta.arena_radius_km;
    let reach = if w.is_planet() { 2500.0 } else { 30.0 };
    let homes: Vec<PlaceId> = supply_homes(w);
    let start_clusters = w.clusters.iter().filter(|c| c.army.is_none()).count().max(1);
    let mut flows: BTreeMap<(PlaceId, PlaceId), (u64, ClusterId, ClusterId)> = BTreeMap::new();
    for i in 0..w.clusters.len() {
        let c = &w.clusters[i];
        if c.army.is_some() || c.size < 300 {
            continue;
        }
        let Some(from) = city_of(w, c.place) else { continue };
        let fp = w.places[from.ix()].center;
        let outside = distance_km(center, fp) > radius;
        let refugee = c.label.starts_with("Refugees from");
        let pressure = (c.need.safety - if refugee { 0.7 } else { 0.55 })
            .max(c.need.food - if refugee { 0.75 } else { 0.6 })
            .max(if outside { 0.5 } else { 0.0 });
        if pressure <= 0.0 {
            continue;
        }
        let top = c
            .allegiance
            .iter()
            .filter(|x| x.1 > 0.2)
            .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
            .map(|x| x.0);
        let best = homes
            .iter()
            .filter(|h| **h != from)
            .filter_map(|h| {
                let p = &w.places[h.ix()];
                let d = distance_km(fp, p.center);
                if d > reach || distance_km(center, p.center) > radius {
                    return None;
                }
                let n = w.nodes.get(h.ix())?;
                let load = n.population as f32 / n.base.max(1) as f32;
                if load > 1.35 || n.need.food > 0.5 {
                    return None;
                }
                let lean = top.map(|t| get(&n.allegiance, t)).unwrap_or(0.0);
                let score = (1.0 - n.need.safety) + 0.5 * n.supply.min(1.2) + 0.5 * lean - (d / reach) as f32 * 0.6
                    - 1.2 * (load - 1.0).max(0.0);
                Some((score, *h))
            })
            .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)));
        let Some((_, to)) = best else { continue };
        let movers = ((c.size as f32) * (0.002 + 0.012 * pressure).min(MIGRATE_MAX)).round() as u32;
        if movers < 10 {
            continue;
        }
        // Destination crowd: refugees from this city there, else a new refugee crowd, else the biggest crowd there.
        let label = format!("Refugees from {}", w.places[from.ix()].name);
        let existing = w
            .clusters
            .iter()
            .find(|x| x.army.is_none() && x.label == label && city_of(w, x.place) == Some(to))
            .map(|x| x.id);
        let n_clusters = w.clusters.iter().filter(|c| c.army.is_none()).count();
        let dest = match existing {
            Some(d) => d,
            None if (n_clusters as f32) < start_clusters as f32 * REFUGEE_GROWTH
                || n_clusters < (w.places.len() / 2).max(8) =>
            {
                let src = w.clusters[i].clone();
                let place = w
                    .places
                    .iter()
                    .find(|p| p.kind == PlaceKind::Quarter && p.parent == Some(to))
                    .map(|p| p.id)
                    .unwrap_or(to);
                let id = ClusterId::from_ix(w.clusters.len());
                let seed = fn7_model::seed::Rng::derive(cx.seed, &[hash_str("refugees"), id.0 as u64]).next_u64();
                let hood = w
                    .groups
                    .iter()
                    .find(|g| g.kind == GroupKind::Neighborhood && src.groups.contains(g.id))
                    .map(|g| g.id);
                let groups: Vec<GroupId> = src.groups.as_slice().iter().copied().filter(|g| Some(*g) != hood).collect();
                w.clusters.push(Cluster {
                    id,
                    label: label.clone(),
                    size: 0,
                    place,
                    traits: vec!["displaced".into(), "wary".into(), src.traits.first().cloned().unwrap_or_else(|| "tired".into())],
                    needs: vec!["shelter".into(), "bread".into()],
                    mood: Mood { anger: src.mood.anger, fear: clamp01(src.mood.fear + 0.1), hope: src.mood.hope, devotion: src.mood.devotion },
                    baseline: Mood { anger: 0.35, fear: 0.4, hope: 0.2, devotion: 0.3 },
                    groups: GroupSet::new(groups),
                    worship: src.worship.clone(),
                    allegiance: src.allegiance.clone(),
                    materialized: 0,
                    seed,
                    army: None,
                    need: src.need,
                    stance: Stance::Fleeing,
                    leader: None,
                    cells: vec![],
                    dead: 0,
                });
                id
            }
            None => match crowds_under(w, to).into_iter().max_by_key(|k| (w.clusters[k.ix()].size, std::cmp::Reverse(k.0))) {
                Some(k) => k,
                None => continue,
            },
        };
        w.clusters[i].size -= movers;
        w.clusters[dest.ix()].size += movers;
        let e = flows.entry((from, to)).or_insert((0, ClusterId(i as u32), dest));
        e.0 += movers as u64;
    }
    // One event per origin city: its biggest flow.
    let mut by_origin: BTreeMap<PlaceId, (u64, PlaceId, ClusterId, ClusterId)> = BTreeMap::new();
    for ((from, to), (n, src, dst)) in flows {
        w.soc.migrants += n;
        let e = by_origin.entry(from).or_insert((0, to, src, dst));
        if n > e.0 {
            *e = (n, to, src, dst);
        }
    }
    for (from, (n, to, src, dst)) in by_origin {
        // A flow is news when it is big, and at most every fourth beat per city (people keep leaving in between).
        if n < 2000 || !(cx.beat + from.0).is_multiple_of(4) {
            continue;
        }
        w.soc.migrations += 1;
        let (fname, tname) = (w.places[from.ix()].name.clone(), w.places[to.ix()].name.clone());
        let label = w.clusters[src.ix()].label.clone();
        let why = {
            let c = &w.clusters[src.ix()];
            if distance_km(center, w.places[from.ix()].center) > radius {
                "Time's circle has passed them by"
            } else if c.need.food > c.need.safety {
                "there is no bread"
            } else {
                "it is not safe to stay"
            }
        };
        cx.emit(
            w,
            SocKind::Migration,
            format!("{n} of the {label} leave {fname} for {tname}: {why}"),
            Ref::Cluster(src),
            vec![Ref::Cluster(src), Ref::Cluster(dst), Ref::Place(to)],
            w.places[from.ix()].center,
            Some(from),
            if n >= 20_000 { 6 } else if n >= 3000 { 5 } else { 4 },
            -1,
            json!({ "from": Ref::Place(from), "to": Ref::Place(to), "people": n, "crowd": Ref::Cluster(src),
                    "into": Ref::Cluster(dst), "why": why }),
        );
    }
}

/// The cult group a congregation cell serves (its leader's rank, else the crowd's newest emergent group).
fn cult_group_of(w: &World, cell: &Cell) -> Option<GroupId> {
    cell.leader
        .and_then(|l| people::life(w, l))
        .and_then(|l| match l.rank.of {
            Some(Ref::Group(g)) => Some(g),
            _ => None,
        })
}

const CULT_NAMES: &[&str] = &["Burning Choir", "Ninth Vigil", "Open Hand", "Last Lamp", "Ash Psalm", "Quiet Flame", "Red Kneel", "Bright Wound"];

/// Worship movements: a devout, needy crowd founds a cult (of the demigod it loves, else of a god) around a prophet; cults
/// convert neighbors (worship, allegiance), and a big cult of a god may raise a paladin who hunts the demigod that god
/// favors least.
pub fn cults(w: &mut World, cx: &mut Ctx) {
    // Founding (at most one per beat).
    let cands: Vec<ClusterId> = w
        .clusters
        .iter()
        .filter(|c| c.army.is_none() && c.size >= 500)
        .filter(|c| (c.mood.devotion >= 0.7 && c.need.faith >= 0.35) || c.mood.devotion >= 0.85)
        .map(|c| c.id)
        .collect();
    let live_cults = w.cells.iter().filter(|x| x.ended.is_none() && x.kind == CellKind::Congregation).count();
    let mut founded = live_cults as u32 >= 3 + cx.beat / 10;
    for k in cands {
        if founded {
            break;
        }
        let c = w.clusters[k.ix()].clone();
        let Some(city) = city_of(w, c.place) else { continue };
        let has_cult = w.cells.iter().any(|x| {
            x.ended.is_none() && x.kind == CellKind::Congregation && city_of(w, w.clusters[x.crowd.ix()].place) == Some(city)
        });
        if has_cult || cx.roll("cult", k.0 as u64) >= 0.05 {
            continue;
        }
        founded = true;
        let demigod = c
            .allegiance
            .iter()
            .filter(|(d, x)| *x >= 0.45 && w.demigods.get(d.ix()).is_some_and(|d| d.alive))
            .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
            .map(|x| x.0);
        let god = c
            .worship
            .iter()
            .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
            .map(|x| x.0)
            .or_else(|| {
                let n = w.gods.len().max(1) as u64;
                Some(GodId(cx.rng("cult_god", k.0 as u64).below(n) as u32))
            })
            .filter(|g| g.ix() < w.gods.len());
        let mut rng = cx.rng("cult_name", k.0 as u64);
        let base = rng.pick(CULT_NAMES).copied().unwrap_or("Open Hand");
        let (name, patron, follows) = match demigod {
            Some(d) => (format!("the {base} of {}", w.demigods[d.ix()].name), None, Some(d)),
            None => match god {
                Some(g) => (format!("the {base} of {}", w.gods[g.ix()].name), Some(g), None),
                None => continue,
            },
        };
        let gid = GroupId::from_ix(w.groups.len());
        w.groups.push(Group {
            id: gid,
            name: name.clone(),
            kind: GroupKind::Emergent,
            permeability: 0.6,
            patron,
            follows,
            founded_beat: cx.beat,
            dissolved_beat: None,
        });
        w.clusters[k.ix()].groups.insert(gid);
        let size = ((c.size as f32) * 0.08).round().clamp(10.0, 500.0) as u32;
        let cell = CellId::from_ix(w.cells.len());
        w.cells.push(Cell {
            id: cell,
            kind: CellKind::Congregation,
            name: format!("the first congregation of {name}"),
            crowd: k,
            size,
            mood: c.mood,
            leader: None,
            stance: Stance::Worshipping,
            mob: None,
            formed: cx.beat,
            ended: None,
            seed: fn7_model::seed::Rng::derive(cx.seed, &[hash_str("cell"), cell.0 as u64]).next_u64(),
        });
        w.clusters[k.ix()].cells.push(cell);
        w.soc.cults += 1;
        let cname = w.name_of(Ref::Place(city));
        let at = w.places[c.place.ix()].center;
        let mut refs = vec![Ref::Group(gid), Ref::Cluster(k), Ref::Cell(cell)];
        refs.extend(follows.map(Ref::Demigod));
        refs.extend(patron.map(Ref::God));
        cx.emit(
            w,
            SocKind::CultFounded,
            format!("In {cname} the {} found {name}", c.label),
            Ref::Cluster(k),
            refs,
            at,
            Some(c.place),
            5,
            1,
            json!({ "group": Ref::Group(gid), "name": name, "crowd": Ref::Cluster(k), "cell": Ref::Cell(cell),
                    "demigod": follows.map(Ref::Demigod), "god": patron.map(Ref::God) }),
        );
        let mut spec = Spec::new(
            Role::Prophet,
            Level::Cell,
            Some(Ref::Group(gid)),
            format!("prophet of {name}"),
            format!("the prophet of {name}"),
        );
        spec.ambition = 0.3;
        spec.competence = 0.2;
        if let Some(p) = emerge(w, cx, k, spec, true) {
            w.cells[cell.ix()].leader = Some(p);
            w.clusters[k.ix()].leader.get_or_insert(p);
            if let Some(l) = life_mut(w, p) {
                l.cell = Some(cell);
            }
            if let Some(d) = follows {
                bump(&mut w.characters[p.ix()].allegiance, d, 0.5, -1.0, 1.0);
            }
        }
    }
    // Spread and paladins.
    let congs: Vec<CellId> = w
        .cells
        .iter()
        .filter(|x| x.ended.is_none() && x.kind == CellKind::Congregation)
        .map(|x| x.id)
        .collect();
    for cell in congs {
        let cl = w.cells[cell.ix()].clone();
        let Some(g) = cult_group_of(w, &cl).or_else(|| {
            w.clusters[cl.crowd.ix()]
                .groups
                .as_slice()
                .iter()
                .rev()
                .copied()
                .find(|g| w.groups[g.ix()].kind == GroupKind::Emergent)
        }) else {
            continue;
        };
        let grp = w.groups[g.ix()].clone();
        if grp.follows.is_some_and(|d| !w.demigods[d.ix()].alive) {
            continue;
        }
        let Some(city) = city_of(w, w.clusters[cl.crowd.ix()].place) else { continue };
        let prophet = people::alive(w, cl.leader);
        let skill = prophet.and_then(|p| people::life(w, p)).map(|l| l.competence).unwrap_or(0.3);
        let region = crate::geo::district_of(w, city);
        let targets: Vec<ClusterId> = w
            .clusters
            .iter()
            .filter(|c| c.army.is_none() && c.size > 0 && !c.groups.contains(g) && c.mood.devotion >= 0.4)
            .filter(|c| crate::sim::hier::under(w, c.place, region))
            .map(|c| c.id)
            .collect();
        let mut converted = vec![];
        for k in targets {
            let same_city = city_of(w, w.clusters[k.ix()].place) == Some(city);
            let p = (0.12 + 0.2 * skill) * if same_city { 1.0 } else { 0.25 };
            if cx.roll("convert", (cell.0 as u64) << 20 | k.0 as u64) < p {
                let c = &mut w.clusters[k.ix()];
                c.groups.insert(g);
                c.mood.devotion = clamp01(c.mood.devotion + 0.05);
                if let Some(d) = grp.follows {
                    bump(&mut c.allegiance, d, 0.08, -1.0, 1.0);
                }
                if let Some(god) = grp.patron {
                    bump(&mut c.worship, god, 0.06, 0.0, 1.0);
                }
                converted.push(k);
            }
        }
        if !converted.is_empty() {
            if let Some(p) = prophet {
                if let Some(l) = life_mut(w, p) {
                    l.deeds += 1;
                }
            }
            let at = w.places[w.clusters[cl.crowd.ix()].place.ix()].center;
            let who = prophet.map(|p| w.characters[p.ix()].name.clone()).unwrap_or_else(|| grp.name.clone());
            let names: Vec<String> = converted.iter().take(2).map(|k| w.clusters[k.ix()].label.clone()).collect();
            let mut refs = vec![Ref::Group(g)];
            refs.extend(prophet.map(Ref::Char));
            refs.extend(converted.iter().take(3).map(|k| Ref::Cluster(*k)));
            cx.emit(
                w,
                SocKind::Preaching,
                format!("{who} wins the {} over to {}", names.join(" and the "), grp.name),
                prophet.map(Ref::Char).unwrap_or(Ref::Place(city)),
                refs,
                at,
                Some(city),
                3,
                1,
                json!({ "group": Ref::Group(g), "converted": converted.iter().map(|k| Ref::Cluster(*k)).collect::<Vec<_>>() }),
            );
        }
        // A paladin: a big cult of a god, once.
        if let Some(god) = grp.patron {
            let members = w.clusters.iter().filter(|c| c.size > 0 && c.groups.contains(g)).count();
            let has = w.characters.iter().any(|c| {
                c.life.as_ref().is_some_and(|l| l.role == Role::Paladin && l.rank.of == Some(Ref::Group(g)))
            });
            if members >= 4 && !has && cx.roll("paladin", g.0 as u64) < 0.03 {
                let gname = w.gods[god.ix()].name.clone();
                let mut spec = Spec::new(
                    Role::Paladin,
                    Level::Cell,
                    Some(Ref::Group(g)),
                    format!("paladin of {gname} from {}", grp.name),
                    format!("the paladin of {}", grp.name),
                );
                spec.courage = 0.8;
                spec.cruelty = 0.4;
                if let Some(p) = emerge(w, cx, cl.crowd, spec, false) {
                    w.characters[p.ix()].serves = Some(god);
                    // They hunt the demigod their god favors least (if any is out of favor).
                    let worst = w.gods[god.ix()]
                        .favor
                        .iter()
                        .enumerate()
                        .filter(|(d, f)| **f < 0.0 && w.demigods.get(*d).is_some_and(|x| x.alive))
                        .min_by(|a, b| a.1.total_cmp(b.1).then(a.0.cmp(&b.0)))
                        .map(|x| DemigodId::from_ix(x.0));
                    if let Some(d) = worst {
                        people::grudge(w, p, Ref::Demigod(d));
                    }
                }
            }
        }
    }
}

/// Crowd-vs-crowd violence: in a city, an angry crowd and a rival one (another demigod's Following, or a rival faith
/// when both are fervent) come to blows: casualties on both sides, fear and anger up. At most one clash per city.
pub fn clashes(w: &mut World, cx: &mut Ctx) {
    let homes = supply_homes(w);
    for city in homes {
        // A city's streets erupt at most every third beat (the rest is bandaging and burying).
        if !(cx.beat + city.0).is_multiple_of(3) {
            continue;
        }
        let crowds: Vec<ClusterId> = crowds_under(w, city);
        if crowds.len() < 2 {
            continue;
        }
        let side = |c: &Cluster| -> Option<DemigodId> {
            w.demigods.iter().filter(|d| d.alive).find(|d| c.groups.contains(d.following)).map(|d| d.id).or_else(|| {
                c.allegiance
                    .iter()
                    .filter(|x| x.1 >= 0.35)
                    .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
                    .map(|x| x.0)
            })
        };
        let faith = |c: &Cluster| -> Option<GroupId> {
            c.groups.as_slice().iter().copied().find(|g| w.groups[g.ix()].kind == GroupKind::Faith || (w.groups[g.ix()].kind == GroupKind::Emergent && w.groups[g.ix()].patron.is_some()))
        };
        let mut pair = None;
        'outer: for (i, a) in crowds.iter().enumerate() {
            for b in &crowds[i + 1..] {
                let (x, y) = (&w.clusters[a.ix()], &w.clusters[b.ix()]);
                // Hot: angrier than usual (above its own baseline) and angry in absolute terms.
                let hot = |c: &Cluster| c.mood.anger >= 0.55 && c.mood.anger >= c.baseline.anger + 0.12;
                if !(hot(x) || hot(y)) || x.mood.anger < 0.45 || y.mood.anger < 0.45 {
                    continue;
                }
                let rival_side = matches!((side(x), side(y)), (Some(p), Some(q)) if p != q);
                let rival_faith = x.mood.devotion >= 0.62
                    && y.mood.devotion >= 0.62
                    && matches!((faith(x), faith(y)), (Some(p), Some(q)) if p != q);
                if rival_side || rival_faith {
                    pair = Some((*a, *b, rival_side));
                    break 'outer;
                }
            }
        }
        let Some((a, b, by_side)) = pair else { continue };
        let heat = (w.clusters[a.ix()].mood.anger + w.clusters[b.ix()].mood.anger - 0.9).max(0.0);
        if cx.roll("clash", city.0 as u64) >= 0.04 + 0.25 * heat {
            continue;
        }
        let mut dead = [0u32; 2];
        for (k, id) in [a, b].iter().enumerate() {
            let c = &mut w.clusters[id.ix()];
            let d = (((c.size as f32) * (0.0002 + 0.0008 * cx.roll("clash_dead", id.0 as u64))).ceil() as u32)
                .clamp(3, 400 + (2000.0 * heat) as u32)
                .min(c.size);
            c.size -= d;
            c.dead += d;
            // Blood spent: a little of the anger goes, fear comes.
            c.mood.anger = clamp01(c.mood.anger - 0.04);
            c.mood.fear = clamp01(c.mood.fear + 0.08);
            dead[k] = d;
        }
        let total = (dead[0] + dead[1]) as u64;
        if let Some(n) = w.nodes.get_mut(city.ix()) {
            n.dead += total;
        }
        w.soc.deaths += total;
        w.soc.clashes += 1;
        let (la, lb) = (w.clusters[a.ix()].label.clone(), w.clusters[b.ix()].label.clone());
        let cname = w.name_of(Ref::Place(city));
        let why = if by_side { "over whose demigod deserves the city" } else { "over whose god is real" };
        // Each side now holds the other's leader to account.
        if let (Some(x), Some(y)) = (w.clusters[a.ix()].leader, w.clusters[b.ix()].leader) {
            if w.characters[x.ix()].alive && w.characters[y.ix()].alive {
                people::grudge(w, x, Ref::Char(y));
            }
        }
        let at = w.places[w.clusters[a.ix()].place.ix()].center;
        cx.emit(
            w,
            SocKind::Clash,
            format!("The {la} and the {lb} fight in the streets of {cname} {why}: {total} dead"),
            Ref::Place(city),
            vec![Ref::Cluster(a), Ref::Cluster(b)],
            at,
            Some(city),
            if total >= 300 { 6 } else if total >= 60 { 5 } else { 4 },
            -3,
            json!({ "city": Ref::Place(city), "sides": [Ref::Cluster(a), Ref::Cluster(b)], "dead": dead, "why": why }),
        );
    }
}

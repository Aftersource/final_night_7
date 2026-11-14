//! The tree itself: city districts at setup, one [`Node`] per place, totals rolling up and effects flowing down.

use std::collections::BTreeMap;

use fn7_model::seed::{hash_str, Rng};
use fn7_model::*;

use crate::geo::{city_of, destination, prose};
use crate::sim::{clamp01, r3};
use crate::template::template;

/// City districts per planet city (at most).
pub const QUARTERS_PER_CITY: usize = 3;
/// A city district's radius.
pub const QUARTER_KM: f64 = 18.0;

/// The level of a place in the hierarchy.
pub fn level_of(w: &World, p: &Place) -> Level {
    match (w.is_planet(), p.kind) {
        (true, PlaceKind::Arena) => Level::Planet,
        (true, PlaceKind::District) => Level::Region,
        (true, PlaceKind::Site) => Level::City,
        (_, PlaceKind::Quarter) => Level::District,
        (false, PlaceKind::Arena) => Level::City,
        (false, _) => Level::District,
    }
}

/// The template district archetype a crowd belongs in (from its label), if any.
fn quarter_key_of(label: &str) -> Option<String> {
    let t = template();
    let low = label.to_lowercase();
    t.clusters
        .iter()
        .find(|a| a.labels.iter().any(|l| low.contains(&l.to_lowercase())))
        .and_then(|a| a.districts.first().cloned())
}

/// Planet setup: give every city up to [`QUARTERS_PER_CITY`] districts (Quarter places named from the society
/// template's district archetypes, chosen by the crowds that live there) and move each crowd into its district.
pub fn build_quarters(w: &mut World, seed: u64) {
    if !w.is_planet() {
        return;
    }
    let t = template();
    let cities: Vec<PlaceId> = w
        .places
        .iter()
        .filter(|p| p.kind == PlaceKind::Site)
        .map(|p| p.id)
        .collect();
    for city in cities {
        let cp = w.places[city.ix()].clone();
        let crowds: Vec<ClusterId> = w
            .clusters
            .iter()
            .filter(|c| c.place == city && c.army.is_none())
            .map(|c| c.id)
            .collect();
        // Archetype key per crowd (the city's own archetype when the crowd has none), most common first.
        let own = cp.archetype.clone().unwrap_or_else(|| "old_town".into());
        let mut keys: BTreeMap<String, Vec<ClusterId>> = BTreeMap::new();
        for c in &crowds {
            let k = quarter_key_of(&w.clusters[c.ix()].label).unwrap_or_else(|| own.clone());
            keys.entry(k).or_default().push(*c);
        }
        let mut groups: Vec<(String, Vec<ClusterId>)> = keys.into_iter().collect();
        groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
        if groups.is_empty() {
            groups.push((own.clone(), vec![]));
        }
        while groups.len() > QUARTERS_PER_CITY {
            let (_, extra) = groups.pop().expect("non-empty");
            groups[0].1.extend(extra);
        }
        let mut rng = Rng::derive(seed, &[hash_str("quarters"), city.0 as u64]);
        let n = groups.len();
        for (i, (key, members)) in groups.into_iter().enumerate() {
            let arch = t.districts.iter().find(|d| d.key == key);
            let base = arch
                .and_then(|a| rng.pick(&a.names).cloned())
                .unwrap_or_else(|| "Old Town".into());
            let bearing = 360.0 * i as f64 / n.max(1) as f64 + 30.0 * rng.f64();
            let dir = crate::geo::dir_word(bearing);
            let base = base.replace("{Dir}", &capitalize(dir));
            let center = if n == 1 {
                cp.center
            } else {
                destination(cp.center, bearing, 10.0 + 10.0 * rng.f64())
            };
            let id = PlaceId::from_ix(w.places.len());
            let mut tags = vec!["district".to_string()];
            if let Some(a) = arch {
                tags.extend(a.tags.iter().cloned());
            }
            w.places.push(Place {
                id,
                name: format!("{base}, {}", cp.name),
                kind: PlaceKind::Quarter,
                parent: Some(city),
                center: [round6(center[0]), round6(center[1])],
                radius_km: QUARTER_KM,
                terrain: prose(center, Some(cp.center)),
                tags,
                archetype: Some(key),
            });
            for c in members {
                w.clusters[c.ix()].place = id;
            }
        }
    }
}

fn round6(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// One node per place (supply 1, stance calm) and a first roll-up; `base` = the starting population.
pub fn init_nodes(w: &mut World) {
    let mut rng = Rng::derive(w.meta.seed, &[hash_str("nodes")]);
    w.nodes = w
        .places
        .iter()
        .map(|p| Node {
            place: p.id,
            level: level_of(w, p),
            population: 0,
            base: 0,
            mood: Mood::default(),
            need: Need::default(),
            allegiance: vec![],
            worship: vec![],
            supply: r3(0.9 + 0.25 * rng.f64() as f32),
            stance: Stance::Calm,
            leader: None,
            dead: 0,
        })
        .collect();
    rollup(w);
    for n in w.nodes.iter_mut() {
        n.base = n.population.max(1);
    }
}

/// Every crowd (not armies) under a node, at any level.
pub fn crowds_under(w: &World, place: PlaceId) -> Vec<ClusterId> {
    w.clusters
        .iter()
        .filter(|c| c.army.is_none() && c.size > 0 && under(w, c.place, place))
        .map(|c| c.id)
        .collect()
}

/// Is place `p` at or below `top`?
pub fn under(w: &World, mut p: PlaceId, top: PlaceId) -> bool {
    for _ in 0..8 {
        if p == top {
            return true;
        }
        match w.places.get(p.ix()).and_then(|x| x.parent) {
            Some(par) => p = par,
            None => return false,
        }
    }
    false
}

/// An effect on a node flows down: every crowd under it gets the mood push (scaled), its cells follow their crowd.
pub fn push_down(w: &mut World, place: PlaceId, d: Mood, scale: f32) {
    for c in crowds_under(w, place) {
        let m = &mut w.clusters[c.ix()].mood;
        m.anger = clamp01(m.anger + d.anger * scale);
        m.fear = clamp01(m.fear + d.fear * scale);
        m.hope = clamp01(m.hope + d.hope * scale);
        m.devotion = clamp01(m.devotion + d.devotion * scale);
    }
}

fn add_sparse<K: Ord + Copy>(acc: &mut BTreeMap<K, f64>, v: &[(K, f32)], wgt: f64) {
    for (k, x) in v {
        *acc.entry(*k).or_default() += *x as f64 * wgt;
    }
}

/// Totals roll up: crowds into their district (or city-arena district), districts into cities, cities into regions,
/// regions into the planet: population sums; mood, needs, allegiance and worship are size-weighted means; supply is
/// kept at the city (its districts share it) and averaged above; the stance is the dominant mood's; a node's leader is
/// the most prominent living person ranked there (kept if still alive).
#[allow(clippy::needless_range_loop)]
pub fn rollup(w: &mut World) {
    let n = w.places.len();
    if w.nodes.len() != n {
        return;
    }
    let mut pop = vec![0f64; n];
    let mut mood = vec![[0f64; 4]; n];
    let mut need = vec![[0f64; 3]; n];
    let mut alleg: Vec<BTreeMap<DemigodId, f64>> = vec![BTreeMap::new(); n];
    let mut wor: Vec<BTreeMap<GodId, f64>> = vec![BTreeMap::new(); n];
    // Leaves: crowds into the place they live in, then up the parent chain.
    for c in w.clusters.iter().filter(|c| c.army.is_none() && c.size > 0) {
        let s = c.size as f64;
        let mut p = Some(c.place);
        for _ in 0..8 {
            let Some(pi) = p else { break };
            let i = pi.ix();
            if i >= n {
                break;
            }
            pop[i] += s;
            for (k, x) in [c.mood.anger, c.mood.fear, c.mood.hope, c.mood.devotion].iter().enumerate() {
                mood[i][k] += *x as f64 * s;
            }
            for (k, x) in [c.need.food, c.need.safety, c.need.faith].iter().enumerate() {
                need[i][k] += *x as f64 * s;
            }
            add_sparse(&mut alleg[i], &c.allegiance, s);
            add_sparse(&mut wor[i], &c.worship, s);
            p = w.places[i].parent;
        }
    }
    // Supply: cities (planet) or districts (city arena) hold it; their districts copy it; upper nodes average.
    let supply_home: Vec<Option<PlaceId>> = w.places.iter().map(|p| city_of(w, p.id)).collect();
    let supplies: Vec<f32> = w.nodes.iter().map(|x| x.supply).collect();
    let mut sup_acc = vec![(0f64, 0f64); n];
    for (i, p) in w.places.iter().enumerate() {
        if supply_home[i] == Some(p.id) {
            let s = supplies[i] as f64;
            let wgt = pop[i].max(1.0);
            let mut q = p.parent;
            for _ in 0..8 {
                let Some(qi) = q else { break };
                sup_acc[qi.ix()].0 += s * wgt;
                sup_acc[qi.ix()].1 += wgt;
                q = w.places[qi.ix()].parent;
            }
        }
    }
    for i in 0..n {
        let s = pop[i].max(1e-9);
        let node = &mut w.nodes[i];
        node.population = pop[i].round() as u64;
        if pop[i] > 0.0 {
            node.mood = Mood {
                anger: r3((mood[i][0] / s) as f32),
                fear: r3((mood[i][1] / s) as f32),
                hope: r3((mood[i][2] / s) as f32),
                devotion: r3((mood[i][3] / s) as f32),
            };
            node.need = Need {
                food: r3((need[i][0] / s) as f32),
                safety: r3((need[i][1] / s) as f32),
                faith: r3((need[i][2] / s) as f32),
            };
            node.allegiance = alleg[i]
                .iter()
                .map(|(k, v)| (*k, r3((*v / s) as f32)))
                .filter(|x| x.1.abs() >= 0.005)
                .collect();
            node.worship = wor[i]
                .iter()
                .map(|(k, v)| (*k, r3((*v / s) as f32)))
                .filter(|x| x.1 >= 0.005)
                .collect();
        }
        match supply_home[i] {
            Some(h) if h.ix() != i => node.supply = supplies[h.ix()],
            Some(_) => {}
            None if sup_acc[i].1 > 0.0 => node.supply = r3((sup_acc[i].0 / sup_acc[i].1) as f32),
            None => {}
        }
        node.stance = stance_of(&node.mood);
    }
    // Leaders: keep a living one; else the highest-ranked living person whose rank is this node (or who stands here
    // leading a mob or army), by role weight then competence.
    let mut best: Vec<Option<(u8, f32, CharId)>> = vec![None; n];
    for c in w.characters.iter().filter(|c| c.alive) {
        let Some(l) = c.life.as_deref() else { continue };
        let weight = match l.role {
            Role::General => 5,
            Role::Ringleader => 4,
            Role::Prophet => 3,
            Role::Hero => 2,
            Role::Captain | Role::Notable => 1,
            _ => continue,
        };
        let home = match l.rank.of {
            Some(Ref::Place(p)) => Some(p),
            _ => city_of(w, c.place).or(Some(c.place)),
        };
        let Some(h) = home else { continue };
        if h.ix() >= n {
            continue;
        }
        let cand = (weight, l.competence, c.id);
        if best[h.ix()].is_none_or(|b| (cand.0, cand.1) > (b.0, b.1)) {
            best[h.ix()] = Some(cand);
        }
    }
    for i in 0..n {
        let keep = w.nodes[i]
            .leader
            .is_some_and(|c| w.characters.get(c.ix()).is_some_and(|x| x.alive));
        if !keep {
            w.nodes[i].leader = best[i].map(|b| b.2);
        }
    }
}

/// A node's or crowd's stance from its mood.
pub fn stance_of(m: &Mood) -> Stance {
    if m.anger >= 0.62 && m.anger + m.fear >= 1.0 {
        Stance::Rioting
    } else if m.fear >= 0.6 {
        Stance::Fleeing
    } else if m.devotion >= 0.65 {
        Stance::Worshipping
    } else if m.anger >= 0.45 || m.fear >= 0.45 {
        Stance::Restless
    } else {
        Stance::Calm
    }
}

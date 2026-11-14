//! Match setup: one real city (Natural Earth), its districts and sites, the groups of the Venn, a few hundred clusters,
//! seeded notables, the demigods with their hands, and the gods' runtime state.

use std::collections::BTreeMap;

use fn7_model::seed::{hash_str, Rng};
use fn7_model::*;
use theatre_toolkit::consensus::normalize;
use theatre_toolkit::spatial::{embedded, Place as GeoPlace};

use crate::geo::{
    destination, dir_word, distance_km, ll, on_land, place_at, prose, random_in, PLANET_RADIUS_KM,
};
use crate::rules::ARENA_END_FRACTION;
use crate::template::{template, ClusterArchetype};
use crate::util::{sample, weighted};

/// Find a Natural Earth city by name (normalized exact match, else Jaro-Winkler ≥ 0.9; ties go to the larger city).
pub fn find_city(name: &str) -> Option<&'static GeoPlace> {
    let n = normalize(name);
    if n.is_empty() {
        return None;
    }
    let all = embedded::places().all();
    let exact = all
        .iter()
        .filter(|p| normalize(&p.name) == n)
        .max_by_key(|p| p.population);
    exact.or_else(|| {
        all.iter()
            .map(|p| (strsim::jaro_winkler(&n, &normalize(&p.name)), p))
            .filter(|(s, _)| *s >= 0.9)
            .max_by(|a, b| {
                a.0.total_cmp(&b.0)
                    .then(a.1.population.cmp(&b.1.population))
            })
            .map(|x| x.1)
    })
}

fn pick_city(cfg: &SetupConfig) -> &'static GeoPlace {
    if let Some(c) = cfg.city.as_deref().and_then(find_city) {
        return c;
    }
    let all = embedded::places().all();
    let mut big: Vec<&GeoPlace> = all
        .iter()
        .filter(|p| p.population >= 1_000_000 && on_land(ll(p.point)))
        .collect();
    if big.is_empty() {
        big = all.iter().collect();
    }
    big.sort_by(|a, b| a.name.cmp(&b.name).then(a.country.cmp(&b.country)));
    let mut rng = Rng::derive(cfg.seed, &[hash_str("city")]);
    rng.pick(&big)
        .copied()
        .expect("embedded places are not empty")
}

fn default_radius(population: u64) -> f64 {
    (5.0 + (population as f64 / 1e6).sqrt() * 3.0).clamp(8.0, 20.0)
}

fn fill(tpl: &str, key: &str, val: &str) -> String {
    tpl.replace(key, val)
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().chain(c).collect(),
        None => String::new(),
    }
}

/// A hub: where crowds and notables live (a district of a city arena; a city of the planet), with its template
/// archetype and the neighborhood group of its district/region.
struct DistrictInfo {
    place: PlaceId,
    archetype: usize,
    neighborhood: GroupId,
}

/// The planet's Final Night radius around the convergence city.
pub const PLANET_END_RADIUS_KM: f64 = 1500.0;
/// Crowds this close to a demigod's starting point lean their way at setup.
pub const HOME_KM: f64 = 900.0;
/// Time's circle starts this far beyond the demigod farthest from the convergence city.
pub const PLANET_START_MARGIN_KM: f64 = 1200.0;
/// Major cities kept per planet region.
pub const CITIES_PER_REGION: usize = 3;
/// A planet city's radius.
pub const CITY_KM: f64 = 60.0;

/// Build the whole starting world. Deterministic for `(cfg, seats, pantheon)`.
pub fn setup(cfg: &SetupConfig, seats: &[(u8, String)], pantheon: &Pantheon) -> World {
    let t = template();
    let city = pick_city(cfg);
    let center = ll(city.point);
    let radius = cfg
        .arena_radius_km
        .unwrap_or_else(|| default_radius(city.population));
    let mut rng = Rng::derive(cfg.seed, &[hash_str("setup")]);

    let mut w = World {
        meta: Meta {
            schema: STATE_SCHEMA,
            rules_version: cfg.rules_version.clone(),
            pantheon_version: pantheon.version.clone(),
            seed: cfg.seed,
            beat: 0,
            beats_total: cfg.beats_total.max(1),
            beat_ms: cfg.beat_ms,
            arena: PlaceId(0),
            arena_radius_km: radius,
            arena_end_radius_km: radius * ARENA_END_FRACTION,
            settings: serde_json::Value::Null,
            arena_kind: cfg.arena,
            entropy: 0.0,
            heat_death: None,
        },
        places: vec![],
        groups: vec![],
        clusters: vec![],
        characters: vec![],
        demigods: vec![],
        items: vec![],
        relations: vec![],
        agents: vec![],
        gods: vec![],
        rulings: vec![],
        intents: vec![],
        ledger: Ledger::default(),
        memory: MemoryStore::default(),
        rewind: RewindRing {
            cap: cfg.rewind_cap,
            frames: vec![],
        },
        beats: vec![],
        seats: vec![],
        pending: vec![],
        slow: None,
        slow_settled: vec![],
        out_reported: vec![],
        mods: vec![],
        mod_seq: 0,
        nodes: vec![],
        cells: vec![],
        mobs: vec![],
        battles: vec![],
        soc: Default::default(),
    };

    // ---- places -------------------------------------------------------------------------------------------------
    let (mut districts, center, radius) = match cfg.arena {
        ArenaKind::City => city_places(&mut w, &mut rng, city, center, radius),
        ArenaKind::Planet => planet_places(&mut w, &mut rng, city, cfg),
    };
    w.meta.arena_radius_km = radius;
    w.meta.arena_end_radius_km = if cfg.arena == ArenaKind::Planet {
        PLANET_END_RADIUS_KM
    } else {
        radius * ARENA_END_FRACTION
    };

    // ---- groups -------------------------------------------------------------------------------------------------
    let mut keyed: BTreeMap<String, GroupId> = BTreeMap::new();
    let add_group = |w: &mut World,
                     name: String,
                     kind: GroupKind,
                     perm: Option<f32>,
                     patron: Option<GodId>,
                     follows: Option<DemigodId>| {
        let id = GroupId::from_ix(w.groups.len());
        w.groups.push(Group {
            id,
            name,
            kind,
            permeability: perm.unwrap_or_else(|| t.permeability.of(kind)),
            patron,
            follows,
            founded_beat: 0,
            dissolved_beat: None,
        });
        id
    };
    let mut hoods: Vec<(PlaceId, GroupId)> = vec![];
    let district_places: Vec<(PlaceId, String)> = w
        .places
        .iter()
        .filter(|p| p.kind == PlaceKind::District)
        .map(|p| (p.id, p.name.clone()))
        .collect();
    for (pid, dname) in district_places {
        let tpl = rng
            .pick(&t.neighborhood)
            .cloned()
            .unwrap_or_else(|| "{district} Folk".into());
        let g = add_group(
            &mut w,
            fill(&tpl, "{district}", &dname),
            GroupKind::Neighborhood,
            None,
            None,
            None,
        );
        hoods.push((pid, g));
    }
    for d in districts.iter_mut() {
        let dist = crate::geo::district_of(&w, d.place);
        d.neighborhood = hoods
            .iter()
            .find(|(p, _)| *p == dist)
            .map(|x| x.1)
            .unwrap_or(hoods[0].1);
    }
    for g in &t.groups {
        let id = add_group(&mut w, g.name.clone(), g.kind, g.permeability, None, None);
        keyed.insert(g.key.clone(), id);
    }
    let mut faith_of: Vec<(GodId, GroupId)> = vec![];
    for (gi, god) in pantheon.gods.iter().enumerate() {
        let tpl = rng
            .pick(&t.faiths)
            .cloned()
            .unwrap_or_else(|| "Faithful of {god}".into());
        let gid = GodId::from_ix(gi);
        let id = add_group(
            &mut w,
            fill(&tpl, "{god}", &god.name),
            GroupKind::Faith,
            None,
            Some(gid),
            None,
        );
        faith_of.push((gid, id));
    }
    let [fmin, fmax] = t.families.count;
    let n_fam = fmin + rng.below((fmax.saturating_sub(fmin) + 1) as u64) as u32;
    let mut families: Vec<(GroupId, String)> = vec![];
    for surname in sample(&mut rng, &t.names.surnames, n_fam as usize) {
        let tpl = rng
            .pick(&t.families.names)
            .cloned()
            .unwrap_or_else(|| "House {surname}".into());
        let id = add_group(
            &mut w,
            fill(&tpl, "{surname}", &surname),
            GroupKind::Family,
            None,
            None,
            None,
        );
        families.push((id, surname));
    }
    let mut followings = vec![];
    for (i, (_, name)) in seats.iter().enumerate() {
        let id = add_group(
            &mut w,
            format!("Followers of {name}"),
            GroupKind::Following,
            None,
            None,
            Some(DemigodId::from_ix(i)),
        );
        followings.push(id);
    }

    // ---- clusters -----------------------------------------------------------------------------------------------
    let total = cfg.clusters.max(1) as usize;
    let mut raw_sizes: Vec<f64> = vec![];
    for (di, d) in districts.iter().enumerate() {
        let dkey = &t.districts[d.archetype].key;
        let dname = w.places[d.place.ix()].name.clone();
        let count = total / districts.len() + usize::from(di < total % districts.len());
        let weights: Vec<f64> = t
            .clusters
            .iter()
            .map(|a| {
                if a.districts.iter().any(|k| k == dkey) {
                    3.0
                } else if a.districts.is_empty() {
                    0.6
                } else {
                    0.08
                }
            })
            .collect();
        let mut labels: Vec<String> = vec![];
        for _ in 0..count {
            let ai = weighted(&mut rng, &weights).unwrap_or(0);
            let a = &t.clusters[ai];
            let label = unique_label(&mut rng, a, &dname, &labels);
            labels.push(label.clone());
            let id = ClusterId::from_ix(w.clusters.len());
            let seed = Rng::derive(cfg.seed, &[hash_str("cluster"), id.0 as u64]).next_u64();
            let mut groups = vec![d.neighborhood];
            groups.extend(a.groups.iter().filter_map(|k| keyed.get(k).copied()));
            for (k, p) in &a.maybe {
                if (rng.f64() as f32) < *p {
                    if let Some(g) = keyed.get(k) {
                        groups.push(*g);
                    }
                }
            }
            let mut worship = vec![];
            if (rng.f64() as f32) < a.faith_chance && !faith_of.is_empty() {
                let fw: Vec<f64> = faith_of
                    .iter()
                    .map(|(g, _)| {
                        let slug = &pantheon.gods[g.ix()].slug;
                        0.25 + a
                            .faith
                            .iter()
                            .filter(|(s, _)| s == slug)
                            .map(|x| x.1 as f64)
                            .sum::<f64>()
                    })
                    .collect();
                if let Some(fi) = weighted(&mut rng, &fw) {
                    let (god, g) = faith_of[fi];
                    groups.push(g);
                    worship.push((god, round3(0.35 + 0.4 * rng.f64() as f32)));
                }
            }
            if (rng.f64() as f32) < a.family && !families.is_empty() {
                groups.push(rng.pick(&families).map(|f| f.0).unwrap_or(families[0].0));
            }
            let jitter = |rng: &mut Rng, x: f32| {
                round3((x + (rng.f64() as f32 - 0.5) * 0.2).clamp(0.0, 1.0))
            };
            let mood = Mood {
                anger: jitter(&mut rng, a.mood[0]),
                fear: jitter(&mut rng, a.mood[1]),
                hope: jitter(&mut rng, a.mood[2]),
                devotion: jitter(&mut rng, a.mood[3]),
            };
            let [lo, hi] = a.size;
            raw_sizes.push((lo + rng.below((hi.saturating_sub(lo) + 1) as u64) as u32) as f64);
            w.clusters.push(Cluster {
                id,
                label,
                size: 0,
                place: d.place,
                traits: sample(&mut rng, &a.traits, 3),
                needs: sample(&mut rng, &a.needs, 2),
                mood,
                baseline: Mood {
                    anger: round3(a.mood[0].clamp(0.0, 1.0)),
                    fear: round3(a.mood[1].clamp(0.0, 1.0)),
                    hope: round3(a.mood[2].clamp(0.0, 1.0)),
                    devotion: round3(a.mood[3].clamp(0.0, 1.0)),
                },
                groups: GroupSet::new(groups),
                worship,
                allegiance: vec![],
                materialized: 0,
                seed,
                army: None,
                need: Default::default(),
                stance: Default::default(),
                leader: None,
                cells: vec![],
                dead: 0,
            });
        }
    }
    let pop_base: u64 = match cfg.arena {
        ArenaKind::City => city.population,
        ArenaKind::Planet => districts
            .iter()
            .map(|d| {
                let p = &w.places[d.place.ix()];
                embedded::places()
                    .all()
                    .iter()
                    .filter(|x| x.name == p.name && distance_km(ll(x.point), p.center) < 1.0)
                    .map(|x| x.population)
                    .max()
                    .unwrap_or(1_000_000)
            })
            .sum(),
    };
    let target = pop_base as f64 * t.population_slice;
    let raw_total: f64 = raw_sizes.iter().sum::<f64>().max(1.0);
    let scale = (target / raw_total).max(0.01);
    for (c, raw) in w.clusters.iter_mut().zip(&raw_sizes) {
        c.size = ((raw * scale).round() as u32).max(20);
    }

    // ---- the hierarchy: city districts (planet) ------------------------------------------------------------------
    crate::sim::hier::build_quarters(&mut w, cfg.seed);

    // ---- notables -----------------------------------------------------------------------------------------------
    for d in &districts {
        let a = &t.districts[d.archetype];
        let dp = w.places[d.place.ix()].clone();
        let local: Vec<ClusterId> = w
            .clusters
            .iter()
            .filter(|c| c.place == d.place || w.places[c.place.ix()].parent == Some(d.place))
            .map(|c| c.id)
            .collect();
        let per_hub = if cfg.arena == ArenaKind::Planet {
            1
        } else {
            cfg.notables_per_district as usize
        };
        for n in 0..per_hub {
            let role = a
                .notables
                .get(n % a.notables.len().max(1))
                .cloned()
                .unwrap_or_else(|| "local figure".into());
            let given = rng.pick(&t.names.given).cloned().unwrap_or_default();
            let surname = rng.pick(&t.names.surnames).cloned().unwrap_or_default();
            let from = rng.pick(&local).copied();
            let (groups, mut traits) = match from {
                Some(c) => {
                    let cl = &w.clusters[c.ix()];
                    (cl.groups.clone(), sample(&mut rng, &cl.traits, 2))
                }
                None => (GroupSet::new(vec![d.neighborhood]), vec![]),
            };
            if let Some(q) = rng.pick(&t.names.quirks) {
                traits.push(q.clone());
            }
            let pos = random_in(&mut rng, dp.center, dp.radius_km * 0.6);
            let id = CharId::from_ix(w.characters.len());
            w.characters.push(Character {
                id,
                name: format!("{given} {surname}"),
                aliases: vec![format!("the {role} of {}", dp.name), format!("the {role}")],
                origin: Origin::Seeded,
                place: d.place,
                pos,
                traits,
                groups,
                health: DEFAULT_MAX_HEALTH,
                max_health: DEFAULT_MAX_HEALTH,
                alive: true,
                allegiance: vec![],
                serves: None,
                possessed_by: None,
                memorized_beat: 0,
                memorized_by: None,
                tempo: vec![],
                life: None,
            });
        }
    }
    // Notables' place: most specific place at their position (could be a site).
    for i in 0..w.characters.len() {
        let p = place_at(&w, w.characters[i].pos);
        w.characters[i].place = p;
    }

    // ---- demigods, hands, gods ----------------------------------------------------------------------------------
    let n = seats.len().max(1) as f64;
    let drops = start_cities(&w, &districts, seats.len(), cfg.seed);
    for (i, (seat, name)) in seats.iter().enumerate() {
        let id = DemigodId::from_ix(i);
        let pos = match cfg.arena {
            ArenaKind::City => destination(center, 45.0 + 360.0 * i as f64 / n, radius * 0.7),
            ArenaKind::Planet => drops[i % drops.len().max(1)],
        };
        let mut hands = [AgentId(0); 2];
        for (h, hand) in [Hand::Left, Hand::Right].into_iter().enumerate() {
            let aid = AgentId::from_ix(w.agents.len());
            hands[h] = aid;
            let side = if h == 0 { "Left" } else { "Right" };
            w.agents.push(Agent {
                id: aid,
                kind: AgentKind::Hand { demigod: id, hand },
                name: format!("{name}'s {side} Hand"),
                persona: format!("The {} hand of the demigod {name}: an orchestrator that plans, spawns subagents and acts.", lower_first(side)),
                goal: format!("Keep {name} alive and make them the last demigod standing."),
                scope: vec![],
                spawned_beat: 0,
                budget_beats: None,
                status: AgentStatus::Live,
                ended_beat: None,
                report: None,
                steps: 0,
                children: vec![],
            });
        }
        let place = place_at(&w, pos);
        w.demigods.push(Demigod {
            id,
            seat: *seat,
            name: name.clone(),
            pos,
            place,
            health: DEFAULT_MAX_HEALTH,
            max_health: DEFAULT_MAX_HEALTH,
            alive: true,
            eliminated_beat: None,
            cause: None,
            hands,
            following: followings[i],
            items: vec![],
            echo_charges: cfg.echo_charges,
            words_spoken: 0,
            tempo: vec![],
            bolts: cfg.start_bolts,
            slows: vec![],
            orders: vec![],
        });
    }
    for (gi, god) in pantheon.gods.iter().enumerate() {
        let gid = GodId::from_ix(gi);
        let aid = AgentId::from_ix(w.agents.len());
        w.agents.push(Agent {
            id: aid,
            kind: AgentKind::God { god: gid },
            name: god.name.clone(),
            persona: god.persona.clone(),
            goal: god.agenda.clone(),
            scope: vec![],
            spawned_beat: 0,
            budget_beats: None,
            status: AgentStatus::Live,
            ended_beat: None,
            report: None,
            steps: 0,
            children: vec![],
        });
        w.gods.push(GodState {
            id: gid,
            name: god.name.clone(),
            favor: vec![0.0; seats.len()],
            attention: 0.0,
            paladins: vec![],
            agent: aid,
            invocations: 0,
            health: 10.0,
        });
    }
    // Home crowds: the people around each demigod's starting point already lean their way (a base to build on).
    for (di, d) in w.demigods.clone().iter().enumerate() {
        let reach = if cfg.arena == ArenaKind::Planet { HOME_KM } else { radius * 0.4 };
        let mut rng = Rng::derive(cfg.seed, &[hash_str("home"), di as u64]);
        for c in w.clusters.iter_mut() {
            let at = w.places[c.place.ix()].center;
            if distance_km(at, d.pos) <= reach {
                let v = round3(0.2 + 0.25 * rng.f64() as f32);
                match c.allegiance.binary_search_by(|x| x.0.cmp(&d.id)) {
                    Ok(i) => c.allegiance[i].1 = v,
                    Err(i) => c.allegiance.insert(i, (d.id, v)),
                }
            }
        }
    }
    // Time's circle on the planet starts just wide enough to hold every demigod (plus a margin), so it presses from
    // the first beats instead of only at the end.
    if cfg.arena == ArenaKind::Planet && cfg.arena_radius_km.is_none() && !w.demigods.is_empty() {
        let c = w.places[w.meta.arena.ix()].center;
        let far = w.demigods.iter().map(|d| distance_km(c, d.pos)).fold(0.0, f64::max);
        let start = (far + PLANET_START_MARGIN_KM).clamp(PLANET_END_RADIUS_KM * 2.0, PLANET_RADIUS_KM);
        w.places[w.meta.arena.ix()].radius_km = start;
        w.meta.arena_radius_km = start;
    }
    crate::sim::hier::init_nodes(&mut w);
    w
}

fn unique_label(rng: &mut Rng, a: &ClusterArchetype, district: &str, taken: &[String]) -> String {
    let base = rng
        .pick(&a.labels)
        .cloned()
        .unwrap_or_else(|| a.key.clone());
    let plain = format!("{base} of {district}");
    if !taken.contains(&plain) {
        return plain;
    }
    let t = template();
    for q in sample(rng, &t.qualifiers, t.qualifiers.len()) {
        let l = format!("{q} {} of {district}", lower_first(&base));
        if !taken.contains(&l) {
            return l;
        }
    }
    (2..)
        .map(|i| format!("{plain} ({i})"))
        .find(|l| !taken.contains(l))
        .unwrap_or(plain)
}

pub(crate) fn round3(x: f32) -> f32 {
    (x * 1000.0).round() / 1000.0
}

/// One real city: the arena, 6-9 districts in a ring (templated archetypes) and one or two sites each. Returns the
/// hubs (where crowds live: the districts), the arena center and radius.
fn city_places(
    w: &mut World,
    rng: &mut Rng,
    city: &GeoPlace,
    center: LonLat,
    radius: f64,
) -> (Vec<DistrictInfo>, LonLat, f64) {
    let t = template();
    let mut arena_tags = vec!["arena".to_string()];
    if !city.country.is_empty() {
        arena_tags.push(city.country.clone());
    }
    w.places.push(Place {
        id: PlaceId(0),
        name: city.name.clone(),
        kind: PlaceKind::Arena,
        parent: None,
        center,
        radius_km: radius,
        terrain: prose(center, None),
        tags: arena_tags,
        archetype: None,
    });

    let n_districts = 6 + rng.below(4) as usize;
    let mut used = vec![false; t.districts.len()];
    let mut districts: Vec<DistrictInfo> = vec![];
    let ring = n_districts - 1;
    for i in 0..n_districts {
        let (pos, bearing) = if i == 0 {
            (center, 0.0)
        } else {
            let b = (i - 1) as f64 * 360.0 / ring as f64 + (rng.f64() - 0.5) * 20.0;
            (
                destination(center, b, radius * (0.45 + 0.2 * rng.f64())),
                b.rem_euclid(360.0),
            )
        };
        let wet = i > 0 && (!on_land(pos) || !on_land(destination(center, bearing, radius * 1.1)));
        let weights: Vec<f64> = t
            .districts
            .iter()
            .enumerate()
            .map(|(k, a)| {
                if used[k] {
                    return 0.0;
                }
                let mut s = if i == 0 {
                    if a.central {
                        3.0
                    } else {
                        0.0
                    }
                } else if a.central {
                    0.3
                } else {
                    1.0
                };
                if a.waterfront {
                    s = if wet { s + 4.0 } else { 0.0 };
                }
                s
            })
            .collect();
        let k = weighted(rng, &weights)
            .or_else(|| used.iter().position(|u| !u))
            .unwrap_or(i % t.districts.len());
        used[k] = true;
        let a = &t.districts[k];
        let dir = if i == 0 { "Central" } else { dir_word(bearing) };
        let name = fill(
            rng.pick(&a.names).map(String::as_str).unwrap_or(&a.key),
            "{Dir}",
            dir,
        );
        let id = PlaceId::from_ix(w.places.len());
        w.places.push(Place {
            id,
            name,
            kind: PlaceKind::District,
            parent: Some(PlaceId(0)),
            center: pos,
            radius_km: radius * 0.28,
            terrain: prose(pos, Some(center)),
            tags: a.tags.clone(),
            archetype: Some(a.key.clone()),
        });
        districts.push(DistrictInfo {
            place: id,
            archetype: k,
            neighborhood: GroupId(0),
        });
    }
    // Sites: one or two per district.
    for d in &districts {
        let a = &t.districts[d.archetype];
        let dp = w.places[d.place.ix()].clone();
        let n = 1 + rng.below(2) as usize;
        for name in sample(rng, &a.sites, n) {
            let pos = random_in(rng, dp.center, dp.radius_km * 0.5);
            let mut tags = a.tags.clone();
            tags.push("site".into());
            w.places.push(Place {
                id: PlaceId::from_ix(w.places.len()),
                name,
                kind: PlaceKind::Site,
                parent: Some(d.place),
                center: pos,
                radius_km: 0.4,
                terrain: prose(pos, Some(center)),
                tags,
                archetype: None,
            });
        }
    }

    (districts, center, radius)
}

/// Planet start positions: one city per demigod, spread over different regions (seeded), jittered a little.
fn start_cities(w: &World, hubs: &[DistrictInfo], n: usize, seed: u64) -> Vec<LonLat> {
    if !w.is_planet() || hubs.is_empty() {
        return vec![];
    }
    let mut rng = Rng::derive(seed, &[hash_str("drops")]);
    let mut by_region: BTreeMap<PlaceId, Vec<PlaceId>> = BTreeMap::new();
    for h in hubs {
        by_region
            .entry(crate::geo::district_of(w, h.place))
            .or_default()
            .push(h.place);
    }
    let mut regions: Vec<PlaceId> = by_region.keys().copied().collect();
    let mut out = vec![];
    while out.len() < n {
        if regions.is_empty() {
            regions = by_region.keys().copied().collect();
        }
        let i = rng.below(regions.len() as u64) as usize;
        let r = regions.remove(i);
        let cities = &by_region[&r];
        let c = cities[rng.below(cities.len() as u64) as usize];
        out.push(random_in(&mut rng, w.places[c.ix()].center, 20.0));
    }
    out
}

/// The whole planet: the arena (converging on `city` for the Final Night), every UN subregion with major cities as a
/// region (district), and its [`CITIES_PER_REGION`] largest cities as sites (each with a templated archetype). Returns
/// the hubs (the cities), the arena center (the convergence city) and the starting radius (everywhere).
fn planet_places(
    w: &mut World,
    rng: &mut Rng,
    city: &GeoPlace,
    cfg: &SetupConfig,
) -> (Vec<DistrictInfo>, LonLat, f64) {
    let t = template();
    let center = ll(city.point);
    let radius = cfg.arena_radius_km.unwrap_or(PLANET_RADIUS_KM);
    w.places.push(Place {
        id: PlaceId(0),
        name: "Earth".into(),
        kind: PlaceKind::Arena,
        parent: None,
        center,
        radius_km: radius,
        terrain: format!(
            "the whole planet; Time's clock closes in on {}{}",
            city.name,
            if city.country.is_empty() { String::new() } else { format!(", {}", city.country) }
        ),
        tags: vec!["arena".into(), "planet".into()],
        archetype: None,
    });
    // Cities by region: the region of the country under the city (or named as its country), largest first.
    let mut by_region: BTreeMap<String, Vec<&GeoPlace>> = BTreeMap::new();
    for p in embedded::places().all() {
        let sr = crate::geo::subregion_at(ll(p.point))
            .or_else(|| crate::geo::country_named(&p.country).map(|c| c.subregion.as_str()));
        if let Some(sr) = sr {
            by_region.entry(sr.to_string()).or_default().push(p);
        }
    }
    let mut hubs = vec![];
    for (region, mut cities) in by_region {
        cities.sort_by(|a, b| {
            b.population
                .cmp(&a.population)
                .then(a.name.cmp(&b.name))
        });
        cities.dedup_by(|a, b| a.name == b.name);
        cities.truncate(CITIES_PER_REGION);
        let pts: Vec<LonLat> = cities.iter().map(|c| ll(c.point)).collect();
        let n = pts.len() as f64;
        let rc = [
            pts.iter().map(|p| p[0]).sum::<f64>() / n,
            pts.iter().map(|p| p[1]).sum::<f64>() / n,
        ];
        let reach = pts
            .iter()
            .map(|p| distance_km(rc, *p))
            .fold(0.0, f64::max);
        let continent = crate::geo::countries()
            .iter()
            .find(|c| c.subregion == region)
            .map(|c| c.continent.clone())
            .unwrap_or_default();
        let rid = PlaceId::from_ix(w.places.len());
        let names: Vec<String> = cities.iter().map(|c| c.name.clone()).collect();
        w.places.push(Place {
            id: rid,
            name: region.clone(),
            kind: PlaceKind::District,
            parent: Some(PlaceId(0)),
            center: rc,
            radius_km: (reach + 400.0).clamp(600.0, 3500.0),
            terrain: format!("a region of {continent}; its great cities: {}", names.join(", ")),
            tags: vec!["region".into(), continent.to_lowercase()],
            archetype: None,
        });
        for c in cities {
            let pos = ll(c.point);
            let coastal = !on_land(destination(pos, 90.0, 40.0))
                || !on_land(destination(pos, 270.0, 40.0))
                || !on_land(destination(pos, 0.0, 40.0))
                || !on_land(destination(pos, 180.0, 40.0));
            let weights: Vec<f64> = t
                .districts
                .iter()
                .map(|a| {
                    if a.waterfront {
                        if coastal { 3.0 } else { 0.0 }
                    } else if a.central {
                        if c.capital { 3.0 } else { 1.0 }
                    } else {
                        1.0
                    }
                })
                .collect();
            let k = weighted(rng, &weights).unwrap_or(0);
            let mut tags = vec!["city".to_string()];
            if !c.country.is_empty() {
                tags.push(c.country.clone());
            }
            if c.capital {
                tags.push("capital".into());
            }
            tags.extend(t.districts[k].tags.iter().cloned());
            let id = PlaceId::from_ix(w.places.len());
            w.places.push(Place {
                id,
                name: c.name.clone(),
                kind: PlaceKind::Site,
                parent: Some(rid),
                center: pos,
                radius_km: CITY_KM,
                terrain: prose(pos, None),
                tags,
                archetype: Some(t.districts[k].key.clone()),
            });
            hubs.push(DistrictInfo {
                place: id,
                archetype: k,
                neighborhood: GroupId(0),
            });
        }
    }
    (hubs, center, radius)
}

//! The planet's per-beat rules, all deterministic (seeded by `meta.seed` and the beat) and all expressed as `Rules`
//! proposals the plugin merges with the beat's others: armies marching and fighting, Entropy's escalating wackiness,
//! the persistent world mods, and each demigod's influence per region (for the globe's colors).

use fn7_model::seed::{hash_str, Rng};
use fn7_model::*;

use crate::geo::{destination, distance_km, district_of, pos_of, random_in};
use crate::util::get;

// ---------------------------------------------------------------------------------------------------------------------
// Armies
// ---------------------------------------------------------------------------------------------------------------------

/// Rival armies closer than this fight.
pub const ARMY_CLASH_KM: f64 = 300.0;
/// An army this close to a rival demigod attacks it.
pub const ARMY_STRIKE_KM: f64 = 200.0;

fn rules(effect: Effect, act: String, actor: Ref, magnitude: u8, valence: i8, witnesses: Vec<Ref>) -> Proposal {
    Proposal {
        source: Source::Rules,
        intent: None,
        effect,
        act,
        actor,
        magnitude,
        valence,
        witnesses,
        cause: None,
    }
}

/// Live armies: size > 0 and their demigod alive, or rogue (answering to no demigod).
pub fn armies(w: &World) -> impl Iterator<Item = (&Cluster, &Army)> {
    w.clusters.iter().filter_map(move |c| {
        let a = c.army.as_ref()?;
        (c.size > 0 && (a.rogue || w.demigods.get(a.demigod.ix()).is_some_and(|d| d.alive))).then_some((c, a))
    })
}

/// Where an army is headed this beat: its target's current position (a demigod it hunts, a place), else its `dest`.
pub fn army_dest(w: &World, a: &Army) -> Option<LonLat> {
    match a.target {
        Some(t @ (Ref::Demigod(_) | Ref::Char(_))) if crate::util::is_alive(w, t) => pos_of(w, t),
        Some(t) => pos_of(w, t).or(a.dest),
        None => a.dest,
    }
}

/// Army proposals by the old flat rules. Since schema 4 armies are moved, fought and aimed by their generals in the
/// society step (`sim::armies`), which also produces the strikes on demigods; this returns nothing and is kept for
/// callers of the old API.
pub fn army_proposals(_w: &World, _beat: u32) -> Vec<Proposal> {
    vec![]
}

// ---------------------------------------------------------------------------------------------------------------------
// Entropy
// ---------------------------------------------------------------------------------------------------------------------

/// Entropy thresholds: (at, key, what it does to the world).
pub const ENTROPY_LEVELS: [(f32, &str, &str); 5] = [
    (0.0, "calm", "The world is holding together, mostly."),
    (
        20.0,
        "weather",
        "The weather has gone insane: freak storms, raining fish, snow in the desert.",
    ),
    (
        40.0,
        "physics",
        "Physics is unreliable: people fall sideways for hundreds of kilometers and powers land off target.",
    ),
    (
        60.0,
        "riots",
        "Crowds are rioting or worshipping whoever throws the most power around.",
    ),
    (
        80.0,
        "godfall",
        "Reality is tearing: the gods themselves are taking damage. Heat death is near.",
    ),
];

/// The level (0..=4) of an Entropy value.
pub fn entropy_level(e: f32) -> u8 {
    ENTROPY_LEVELS.iter().rposition(|l| e >= l.0).unwrap_or(0) as u8
}

pub fn entropy_key(level: u8) -> &'static str {
    ENTROPY_LEVELS[(level as usize).min(4)].1
}

/// One line for prompts: how wacky the world is right now.
pub fn entropy_prose(w: &World) -> String {
    let e = w.meta.entropy;
    let l = entropy_level(e);
    format!(
        "Entropy {:.0}/100 ({}): {}",
        e,
        entropy_key(l),
        ENTROPY_LEVELS[l as usize].2
    )
}

const WEATHER: &[&str] = &[
    "it rains fish",
    "a blizzard rolls in from nowhere",
    "hailstones the size of melons fall",
    "a warm wind smells of burnt sugar",
    "the clouds spell out rude words",
    "ball lightning bounces down the streets",
    "it snows upward",
    "a fog so thick the pigeons walk",
];

/// This beat's Entropy proposals, escalating with the level: freak weather (≥ 20), unreliable physics (≥ 40), riots and
/// worship (≥ 60), gods taking damage (≥ 80). Heat death itself is `raise_entropy` reaching the maximum.
pub fn entropy_proposals(w: &World, beat: u32, entropy_god: Option<GodId>) -> Vec<Proposal> {
    let l = entropy_level(w.meta.entropy);
    let mut out = vec![];
    if l == 0 {
        return out;
    }
    let actor = entropy_god.map(Ref::God).unwrap_or(Ref::Place(w.meta.arena));
    let mut rng = Rng::derive(w.meta.seed, &[hash_str("entropy"), beat as u64]);
    let regions: Vec<&Place> = w
        .places
        .iter()
        .filter(|p| p.kind == PlaceKind::District)
        .collect();
    // Weather insanity.
    if rng.f64() < if l >= 2 { 0.9 } else { 0.5 } {
        if let Some(r) = rng.pick(&regions) {
            let at = random_in(&mut rng, r.center, r.radius_km * 0.6);
            let what = rng.pick(WEATHER).copied().unwrap_or("it rains fish");
            out.push(rules(
                Effect::Happen {
                    text: format!("freak weather over {}: {what}", r.name),
                    at,
                },
                format!("Freak weather over {}: {what}", r.name),
                actor,
                5,
                -1,
                vec![],
            ));
            let mut local: Vec<&Cluster> = w
                .clusters
                .iter()
                .filter(|c| c.size > 0 && c.army.is_none() && district_of(w, c.place) == r.id)
                .collect();
            local.sort_by_key(|c| std::cmp::Reverse(c.size));
            if let Some(c) = local.first() {
                out.push(rules(
                    Effect::MoodShift {
                        cluster: c.id,
                        anger: 0.03,
                        fear: 0.1,
                        hope: 0.0,
                        devotion: 0.0,
                    },
                    format!("{} cower from the weather", c.label),
                    actor,
                    2,
                    -1,
                    vec![Ref::Cluster(c.id)],
                ));
            }
        }
    }
    // Unreliable physics.
    if l >= 2 && rng.f64() < 0.4 {
        let alive: Vec<&Demigod> = w.alive_demigods().collect();
        if let Some(d) = rng.pick(&alive) {
            let km = if w.is_planet() {
                300.0 + 1200.0 * rng.f64()
            } else {
                1.0 + 3.0 * rng.f64()
            };
            let to = destination(d.pos, rng.f64() * 360.0, km);
            out.push(rules(
                Effect::Move {
                    who: Ref::Demigod(d.id),
                    to,
                    place: None,
                },
                format!("Physics hiccups: {} falls sideways {:.0} km", d.name, km),
                actor,
                6,
                -1,
                vec![Ref::Demigod(d.id)],
            ));
        }
    }
    // Riots and worship.
    if l >= 3 {
        let crowds: Vec<&Cluster> = w.clusters.iter().filter(|c| c.size > 50 && c.army.is_none()).collect();
        if let Some(c) = crowds
            .iter()
            .max_by(|a, b| a.mood.anger.total_cmp(&b.mood.anger).then(b.id.cmp(&a.id)))
        {
            let n = ((c.size as f32) * 0.03).ceil() as u32;
            out.push(rules(
                Effect::Shrink {
                    cluster: c.id,
                    count: n.min(c.size),
                },
                format!("{} riot; {n} are trampled", c.label),
                actor,
                6,
                -3,
                vec![Ref::Cluster(c.id)],
            ));
        }
        if let Some(c) = crowds
            .iter()
            .max_by(|a, b| a.mood.devotion.total_cmp(&b.mood.devotion).then(b.id.cmp(&a.id)))
        {
            out.push(rules(
                Effect::MoodShift {
                    cluster: c.id,
                    anger: 0.0,
                    fear: 0.0,
                    hope: 0.05,
                    devotion: 0.1,
                },
                format!("{} fall to their knees and worship the chaos", c.label),
                actor,
                4,
                1,
                vec![Ref::Cluster(c.id)],
            ));
        }
    }
    // Gods take damage.
    if l >= 4 {
        let gods: Vec<&GodState> = w
            .gods
            .iter()
            .filter(|g| Some(g.id) != entropy_god && g.health > 0.0)
            .collect();
        if let Some(g) = rng.pick(&gods) {
            out.push(rules(
                Effect::Wound {
                    god: g.id,
                    amount: 1.0,
                },
                format!("Entropy tears a strip off {}", g.name),
                actor,
                7,
                -2,
                vec![Ref::God(g.id)],
            ));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------------
// World mods
// ---------------------------------------------------------------------------------------------------------------------

/// A world mod's effects shrink by this factor every beat of its age.
pub const MOD_DECAY: f32 = 0.9;
/// Below this (strongest per-beat effect × stacks × decay) a mod is spent and fades.
pub const MOD_SPENT: f32 = 0.03;
const MOD_VERBS: &[&str] = &["batters", "lashes", "grinds at", "rattles", "scours", "needles", "buffets", "harries"];

/// This beat's world-mod proposals: expiries (`Unmod`), then each mod's per-beat effect inside its radius × stacks:
/// harm and heal on demigods (and harm on armies), and every other beat a mood push on the nearest crowd.
pub fn mod_proposals(w: &World, beat: u32) -> Vec<Proposal> {
    let mut out = vec![];
    for m in &w.mods {
        let actor = Ref::Place(m.region);
        if m.expires_beat.is_some_and(|e| e <= beat) {
            out.push(rules(
                Effect::Unmod { id: m.id },
                format!("{} fades", m.name),
                actor,
                2,
                0,
                vec![],
            ));
            continue;
        }
        // Mods weaken with age (MOD_DECAY a beat) and fade out once they are spent; their bite lands every other beat
        // with varied words, so a quiet stretch isn't one line repeated.
        let age = beat.saturating_sub(m.since_beat);
        let decay = MOD_DECAY.powi(age as i32);
        let k = m.stacks.max(1) as f32 * decay;
        let strongest = [m.per_beat.harm, m.per_beat.heal, m.per_beat.anger.abs(), m.per_beat.fear.abs(), m.per_beat.hope.abs(), m.per_beat.devotion.abs()]
            .into_iter()
            .fold(0.0f32, f32::max);
        if age > 0 && strongest * k < MOD_SPENT {
            out.push(rules(Effect::Unmod { id: m.id }, format!("{} blows itself out", m.name), actor, 2, 0, vec![]));
            continue;
        }
        let bite = (beat + m.id).is_multiple_of(2);
        let r = m.radius_km as f64;
        for d in w.alive_demigods().filter(|d| bite && distance_km(d.pos, m.at) <= r) {
            if m.per_beat.harm > 0.0 {
                let amount = ((m.per_beat.harm * k).min(2.0) * 100.0).round() / 100.0;
                if amount < 0.05 {
                    continue;
                }
                let verb = MOD_VERBS[((beat as usize) + m.id as usize + d.id.ix()) % MOD_VERBS.len()];
                out.push(rules(
                    Effect::Damage {
                        who: Ref::Demigod(d.id),
                        amount,
                        cause: m.name.clone(),
                    },
                    format!("{} {verb} {}", m.name, d.name),
                    actor,
                    if amount >= 0.5 { 4 } else { 3 },
                    -2,
                    vec![Ref::Demigod(d.id)],
                ));
            }
            if m.per_beat.heal > 0.0 {
                out.push(rules(
                    Effect::Heal {
                        who: Ref::Demigod(d.id),
                        amount: (m.per_beat.heal * k).min(2.0),
                    },
                    format!("{} mends {}", m.name, d.name),
                    actor,
                    3,
                    2,
                    vec![Ref::Demigod(d.id)],
                ));
            }
        }
        if m.per_beat.harm > 0.0 {
            for (c, a) in armies(w).filter(|(_, a)| distance_km(a.pos, m.at) <= r) {
                let n = ((c.size as f32) * 0.02 * m.per_beat.harm * k).ceil() as u32;
                out.push(rules(
                    Effect::Shrink {
                        cluster: c.id,
                        count: n.min(c.size),
                    },
                    format!("{} thins out {}", m.name, c.label),
                    actor,
                    3,
                    -2,
                    vec![Ref::Cluster(c.id)],
                ));
                let _ = a;
            }
        }
        let e = m.per_beat;
        if (e.anger, e.fear, e.hope, e.devotion) != (0.0, 0.0, 0.0, 0.0) && (beat + m.id).is_multiple_of(2) {
            let near = w
                .clusters
                .iter()
                .filter(|c| c.size > 0 && c.army.is_none())
                .filter_map(|c| pos_of(w, Ref::Cluster(c.id)).map(|p| (distance_km(p, m.at), c)))
                .filter(|(d, _)| *d <= r)
                .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
            if let Some((_, c)) = near {
                let s = |x: f32| (x * k).clamp(-0.3, 0.3);
                out.push(rules(
                    Effect::MoodShift {
                        cluster: c.id,
                        anger: s(e.anger),
                        fear: s(e.fear),
                        hope: s(e.hope),
                        devotion: s(e.devotion),
                    },
                    format!("{} lives under {}", c.label, m.name),
                    actor,
                    2,
                    0,
                    vec![Ref::Cluster(c.id)],
                ));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------------
// Influence
// ---------------------------------------------------------------------------------------------------------------------

/// Each demigod's share of each region (planet) or district (city), for the globe's colors: crowds there weighted by
/// their allegiance to the demigod (plus everyone in its Following), its armies there at double weight, and the
/// demigod's own presence. Shares are of the region's people, so a region's shares sum to at most 1. Sorted by region,
/// then demigod; shares below 0.005 are left out.
pub fn influence(w: &World) -> Vec<(PlaceId, DemigodId, f32)> {
    let regions: Vec<PlaceId> = w
        .places
        .iter()
        .filter(|p| p.kind == PlaceKind::District)
        .map(|p| p.id)
        .collect();
    let n = w.demigods.len();
    let mut total = vec![0f64; w.places.len()];
    let mut score = vec![0f64; w.places.len() * n.max(1)];
    for c in w.clusters.iter().filter(|c| c.size > 0) {
        let r = match &c.army {
            Some(a) => crate::geo::region_at(w, a.pos),
            None => district_of(w, c.place),
        };
        let size = c.size as f64;
        total[r.ix()] += size;
        // Per player (rules 0.7.0: a player's two demigods share a Following; the player's credit is split evenly
        // between its standing demigods). One demigod per seat (schema 4): exactly the old per-demigod credit.
        let mut credit: Vec<(Vec<DemigodId>, f64)> = vec![];
        for seat in w.alive_seats() {
            let bodies: Vec<&Demigod> = w.bodies_of_seat(seat).filter(|d| d.alive).collect();
            let mut s = 0.0;
            if let Some(a) = &c.army {
                if bodies.iter().any(|d| a.demigod == d.id) && !a.rogue {
                    s = 2.0 * size;
                    total[r.ix()] += size;
                }
            } else if bodies.iter().any(|d| c.groups.contains(d.following)) {
                s = size;
            } else {
                let best = bodies.iter().map(|d| get(&c.allegiance, d.id).max(0.0)).fold(0.0f32, f32::max);
                s = size * best as f64;
            }
            credit.push((bodies.iter().map(|d| d.id).collect(), s));
        }
        // A crowd is never worth more than its people: shares over several players (two Followings, or allegiances
        // summing past 1) are scaled down to the crowd's size.
        let sum: f64 = credit.iter().map(|x| x.1).sum();
        let cap = if c.army.is_some() { 2.0 * size } else { size };
        let f = if sum > cap && sum > 0.0 { cap / sum } else { 1.0 };
        for (bodies, s) in credit {
            let k = bodies.len().max(1) as f64;
            for d in &bodies {
                score[r.ix() * n + d.ix()] += if bodies.len() == 1 { s * f } else { s * f / k };
            }
        }
    }
    for d in w.demigods.iter().filter(|d| d.alive) {
        let r = crate::geo::region_at(w, d.pos);
        let bonus = (total[r.ix()] * 0.05).max(1.0);
        total[r.ix()] += bonus;
        score[r.ix() * n + d.id.ix()] += bonus;
    }
    let mut out = vec![];
    for r in regions {
        let t = total[r.ix()];
        if t <= 0.0 {
            continue;
        }
        for d in &w.demigods {
            let s = (score[r.ix() * n + d.id.ix()] / t) as f32;
            if s >= 0.005 {
                out.push((r, d.id, s.min(1.0)));
            }
        }
    }
    out
}

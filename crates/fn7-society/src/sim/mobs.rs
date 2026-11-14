//! Mobs: when a crowd's anger and fear cross the thresholds it throws off a gang cell; gangs in a city merge into a mob
//! under a ringleader who steps out of the angriest crowd. Mobs swell, move between districts, riot, loot, attack rival
//! crowds or a hated demigod nearby, and dissolve (spent, scattered, beheaded). An ambitious ringleader may then offer
//! what is left to a demigod as a volunteer host.

use fn7_model::seed::hash_str;
use fn7_model::*;
use serde_json::json;

use crate::geo::{city_of, distance_km};
use crate::sim::hier::crowds_under;
use crate::sim::people::{self, alive, emerge, life_mut, Spec};
use crate::sim::{clamp01, r3, Ctx};
use crate::util::{bump, get};

/// A crowd riots (throws off a gang) at anger ≥ this with anger + fear ≥ [`MOB_SUM`].
pub const MOB_ANGER: f32 = 0.62;
pub const MOB_SUM: f32 = 1.0;
/// Most live mobs per city.
pub const MOBS_PER_CITY: usize = 1;
/// Biggest a mob grows.
pub const MOB_MAX: u32 = 6000;
/// Beats a city rests after a mob ends before the next one can form.
pub const MOB_COOLDOWN: u32 = 16;
/// Per-beat chance that a city's free gangs merge into a mob (and near a demigod, whose arrival stirs the streets).
pub const MOB_FORM_P: f32 = 0.08;
pub const MOB_FORM_P_NEAR: f32 = 0.6;
/// A mob is "near" a demigod within this distance (planet; a city arena uses a fifth of its radius).
pub const NEAR_DEMIGOD_KM: f64 = 900.0;
/// A mob older than this may tire (per beat, [`MOB_TIRED_P`]); below [`MOB_SPENT`] anger it breaks up.
pub const MOB_TIRED_AGE: u32 = 12;
pub const MOB_TIRED_P: f32 = 0.08;
pub const MOB_SPENT: f32 = 0.4;

/// Why a mob ends (the `mob_dissolved` event's `why`).
pub const FEW: &str = "too few are left to matter";
pub const SPENT: &str = "its anger is spent";
pub const LEADERLESS: &str = "nobody is left to follow";
pub const TIRED: &str = "the rioters are tired, hungry and going home";
pub const SCATTERED: &str = "scattered by soldiers";
pub const MARCHED_OFF: &str = "its ringleader marched it off to war";

/// Is `pos` near a living demigod (their arrival stirs the streets around them)?
pub fn near_demigod(w: &World, pos: LonLat) -> bool {
    let reach = if w.is_planet() { NEAR_DEMIGOD_KM } else { w.meta.arena_radius_km * 0.2 };
    w.demigods.iter().any(|d| d.alive && distance_km(d.pos, pos) <= reach)
}

/// "The Lagos bread riot", "the second Lagos bread riot": named by place and cause, numbered when the city has seen one
/// for the same cause before.
fn mob_name(w: &World, city: PlaceId, cause_word: &str) -> String {
    let cname = w.name_of(Ref::Place(city));
    let noun = match cause_word {
        "bread" => "bread riot",
        "panic" => "panic riot",
        "zealot" => "zealot riot",
        _ => "street riot",
    };
    let base = format!("{cname} {noun}");
    let before = w.mobs.iter().filter(|m| m.city == city && m.name.ends_with(&base)).count();
    let ord = match before {
        0 => "",
        1 => "second ",
        2 => "third ",
        3 => "fourth ",
        _ => "latest ",
    };
    format!("the {ord}{base}")
}

/// ", rioting in Dune's name" for a mob that rallies behind a living demigod (else empty).
fn banner(w: &World, side: Option<DemigodId>) -> String {
    side.and_then(|d| w.demigods.get(d.ix()))
        .filter(|d| d.alive)
        .map(|d| format!(", rioting in {}'s name", d.name))
        .unwrap_or_default()
}

/// Upper-case the first letter (mob names start lower-case: "the Lagos bread riot").
pub fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

const GANG_ADJ: &[&str] = &["Red", "Hungry", "Black", "Broken", "Lantern", "Iron", "Barefoot", "Ash", "Salt", "Rag"];
const GANG_NOUN: &[&str] = &["Boys", "Sisters", "Crew", "Hands", "Brigade", "Knives", "Choir", "Dogs", "Mothers", "Lot"];

/// Past the thresholds, and angrier than this crowd usually is (its baseline + 0.15).
fn angry(c: &Cluster) -> bool {
    c.mood.anger >= MOB_ANGER.max(c.baseline.anger + 0.18) && c.mood.anger + c.mood.fear >= MOB_SUM
}

/// The crowd's grievance, for the mob's name and cause.
fn grievance(w: &World, c: &Cluster) -> (&'static str, String) {
    let n = &c.need;
    if n.food >= 0.45 && n.food >= n.safety {
        ("bread", format!("empty bellies and empty granaries in {}", w.name_of(Ref::Place(city_of(w, c.place).unwrap_or(c.place)))))
    } else if n.safety >= 0.45 {
        ("panic", "fear of what is coming".into())
    } else if n.faith >= 0.5 {
        ("zealot", "a faith with no answer".into())
    } else {
        ("street", "old anger with nowhere to go".into())
    }
}

/// Members of a mob die: spread over its cells (and their crowds) by size.
pub fn casualties(w: &mut World, mi: usize, dead: u32) {
    let m = &w.mobs[mi];
    let dead = dead.min(m.size);
    let cells = m.cells.clone();
    let total: u64 = cells.iter().map(|c| w.cells[c.ix()].size as u64).sum::<u64>().max(1);
    let mut left = dead as u64;
    for (k, c) in cells.iter().enumerate() {
        let share = if k + 1 == cells.len() { left } else { (w.cells[c.ix()].size as u64 * dead as u64) / total };
        let share = share.min(left).min(w.cells[c.ix()].size as u64) as u32;
        left -= share as u64;
        let cell = &mut w.cells[c.ix()];
        cell.size -= share;
        let crowd = cell.crowd;
        let cr = &mut w.clusters[crowd.ix()];
        let d = share.min(cr.size);
        cr.size -= d;
        cr.dead += d;
    }
    let m = &mut w.mobs[mi];
    m.size = m.size.saturating_sub(dead);
    m.casualties += dead;
    m.fear = clamp01(m.fear + 0.05 + dead as f32 / (m.size as f32 + 1.0));
}

/// The mob breaks up: cells end, the ringleader goes home, is taken, or (ambitious, loved by a demigod's crowds) raises
/// a volunteer host. Emits `mob_dissolved`.
pub fn dissolve(w: &mut World, cx: &mut Ctx, mi: usize, why: &str) {
    if w.mobs[mi].ended.is_some() {
        return;
    }
    let m = w.mobs[mi].clone();
    w.mobs[mi].ended = Some(cx.beat);
    w.mobs[mi].action = MobAction::Dispersed;
    w.soc.mobs_dissolved += 1;
    for c in &m.cells {
        let cell = &mut w.cells[c.ix()];
        cell.ended = Some(cx.beat);
        cell.mob = None;
        cell.stance = Stance::Calm;
    }
    let mut refs = vec![Ref::Mob(m.id)];
    let leader = alive(w, m.ringleader);
    refs.extend(leader.map(Ref::Char));
    let age = cx.beat.saturating_sub(m.formed);
    let mut toll = format!(" after {age} beat{}", if age == 1 { "" } else { "s" });
    if m.casualties > 0 {
        toll += &format!(", {} of its own dead", m.casualties);
    }
    if m.kills > 0 {
        toll += &format!(", {} killed by it", m.kills);
    }
    cx.emit(
        w,
        SocKind::MobDissolved,
        format!("{} breaks up{toll}: {why}", cap(&m.name)),
        Ref::Place(m.city),
        refs,
        m.pos,
        Some(m.place),
        4,
        0,
        json!({ "mob": Ref::Mob(m.id), "why": why, "casualties": m.casualties, "kills": m.kills,
                "ringleader": leader.map(Ref::Char), "age": age }),
    );
    let Some(r) = leader else { return };
    if let Some(l) = life_mut(w, r) {
        l.mob = None;
    }
    if why == MARCHED_OFF {
        return;
    }
    let (ambition, comp) = people::life(w, r).map(|l| (l.ambition, l.competence)).unwrap_or((0.3, 0.3));
    let x = cx.roll("ringleader_fate", r.0 as u64);
    if why.contains("soldiers") && x < 0.3 {
        people::kill(w, cx, r, &format!("hanged in the square after {} was broken", m.name), None);
        return;
    }
    // A fall: a ringleader whose riot fizzled out is abandoned (the crowd remembers who led it nowhere).
    if (why == SPENT || why == TIRED) && x > 0.75 + 0.2 * comp {
        let rname = w.characters[r.ix()].name.clone();
        if let Some(l) = life_mut(w, r) {
            l.role = Role::Commoner;
            l.goal = Goal::Survive;
            l.rank = Rank { level: Level::Person, of: None, title: format!("once ringleader of {}", m.name) };
        }
        cx.emit(
            w,
            SocKind::MobDissolved,
            format!("{rname} is jeered off the cart and nobody follows them again: {} died with a whimper", m.name),
            Ref::Char(r),
            vec![Ref::Char(r), Ref::Mob(m.id)],
            m.pos,
            Some(m.place),
            4,
            -1,
            json!({ "mob": Ref::Mob(m.id), "ringleader": Ref::Char(r), "fall": true }),
        );
        return;
    }
    // Ambition: offer the mob's people to the demigod their crowds love.
    if ambition > 0.55 && x < 0.25 + 0.4 * comp {
        let side = m.side.filter(|d| w.demigods.get(d.ix()).is_some_and(|x| x.alive));
        let crowd = m.cells.first().map(|c| w.cells[c.ix()].crowd);
        if let (Some(d), Some(k)) = (side, crowd) {
            if w.clusters[k.ix()].size >= 1500 {
                crate::sim::armies::raise_host(w, cx, k, d, Some(r));
            }
        }
    }
}

/// Every beat: gangs out of angry crowds, mobs out of gangs (with a ringleader), and every live mob acts.
pub fn step(w: &mut World, cx: &mut Ctx) {
    form(w, cx);
    let live: Vec<usize> = (0..w.mobs.len()).filter(|i| w.mobs[*i].ended.is_none()).collect();
    for mi in live {
        act(w, cx, mi);
    }
}

fn live_mobs_in(w: &World, city: PlaceId) -> Vec<usize> {
    (0..w.mobs.len())
        .filter(|i| w.mobs[*i].ended.is_none() && w.mobs[*i].city == city)
        .collect()
}

fn form(w: &mut World, cx: &mut Ctx) {
    // Gangs: an angry crowd with no live gang throws one off (10–500 people).
    let angry_crowds: Vec<ClusterId> = w
        .clusters
        .iter()
        .filter(|c| c.army.is_none() && c.size >= 300 && angry(c))
        .map(|c| c.id)
        .collect();
    let mut cities: Vec<PlaceId> = vec![];
    for k in angry_crowds {
        let c = &w.clusters[k.ix()];
        let has_gang = c
            .cells
            .iter()
            .any(|x| w.cells[x.ix()].ended.is_none() && w.cells[x.ix()].kind == CellKind::Gang);
        let city = city_of(w, c.place).unwrap_or(c.place);
        let ended_gang = c
            .cells
            .iter()
            .copied()
            .find(|x| w.cells[x.ix()].ended.is_some() && w.cells[x.ix()].kind == CellKind::Gang && w.cells[x.ix()].mob.is_none());
        if !has_gang {
            let size = ((c.size as f32) * (c.mood.anger - 0.45) * 0.06).round().clamp(10.0, 500.0) as u32;
            // The same gang comes back (cells are never removed; an old one is revived rather than a new row).
            if let Some(g) = ended_gang {
                let mood = c.mood;
                let cell = &mut w.cells[g.ix()];
                cell.ended = None;
                cell.size = size;
                cell.mood = mood;
                cell.leader = cell.leader.filter(|l| w.characters[l.ix()].alive);
                cell.stance = Stance::Restless;
                cell.formed = cx.beat;
                if !cities.contains(&city) {
                    cities.push(city);
                }
                continue;
            }
            let mut rng = cx.rng("gang", k.0 as u64);
            let name = format!(
                "the {} {} of the {}",
                rng.pick(GANG_ADJ).copied().unwrap_or("Red"),
                rng.pick(GANG_NOUN).copied().unwrap_or("Crew"),
                c.label
            );
            let id = CellId::from_ix(w.cells.len());
            let mood = c.mood;
            w.cells.push(Cell {
                id,
                kind: CellKind::Gang,
                name,
                crowd: k,
                size,
                mood,
                leader: None,
                stance: Stance::Restless,
                mob: None,
                formed: cx.beat,
                ended: None,
                seed: fn7_model::seed::Rng::derive(cx.seed, &[hash_str("cell"), id.0 as u64]).next_u64(),
            });
            w.clusters[k.ix()].cells.push(id);
        }
        if !cities.contains(&city) {
            cities.push(city);
        }
    }
    // Gangs whose crowd calmed down go home.
    for i in 0..w.cells.len() {
        let cell = &w.cells[i];
        if cell.ended.is_none() && cell.kind == CellKind::Gang && cell.mob.is_none() {
            let c = &w.clusters[cell.crowd.ix()];
            if c.mood.anger < 0.5 || c.size == 0 {
                w.cells[i].ended = Some(cx.beat);
            }
        }
    }
    // Mobs: free gangs of a city join its mob, or merge into a new one.
    for city in cities {
        let free: Vec<CellId> = w
            .cells
            .iter()
            .filter(|x| x.ended.is_none() && x.kind == CellKind::Gang && x.mob.is_none())
            .filter(|x| city_of(w, w.clusters[x.crowd.ix()].place) == Some(city))
            .map(|x| x.id)
            .collect();
        if free.is_empty() {
            continue;
        }
        let mobs = live_mobs_in(w, city);
        if let Some(&mi) = mobs.first() {
            for c in free {
                if w.mobs[mi].size >= MOB_MAX || w.mobs[mi].cells.len() >= 8 {
                    break;
                }
                w.cells[c.ix()].mob = Some(w.mobs[mi].id);
                w.cells[c.ix()].stance = Stance::Rioting;
                w.mobs[mi].size += w.cells[c.ix()].size;
                w.mobs[mi].cells.push(c);
            }
            continue;
        }
        // A city that just saw a mob broken or spent needs a while before the next one; near a demigod the streets boil
        // over much sooner (their arrival is the story).
        let recent = w.mobs.iter().any(|m| m.city == city && m.ended.is_some_and(|e| e + MOB_COOLDOWN > cx.beat));
        let p = if near_demigod(w, w.places[city.ix()].center) { MOB_FORM_P_NEAR } else { MOB_FORM_P };
        if recent || mobs.len() >= MOBS_PER_CITY || cx.roll("mob", city.0 as u64) >= p {
            continue;
        }
        new_mob(w, cx, city, free);
    }
}

fn new_mob(w: &mut World, cx: &mut Ctx, city: PlaceId, cells: Vec<CellId>) {
    // The angriest cell's crowd gives the ringleader.
    let lead_cell = *cells
        .iter()
        .max_by(|a, b| {
            let (x, y) = (&w.cells[a.ix()], &w.cells[b.ix()]);
            x.mood.anger.total_cmp(&y.mood.anger).then(b.cmp(a))
        })
        .expect("non-empty");
    let crowd = w.cells[lead_cell.ix()].crowd;
    let cl = w.clusters[crowd.ix()].clone();
    let (cause_word, cause) = grievance(w, &cl);
    // Who they rally behind (if anyone): the demigod their crowds love most.
    let side = cl
        .allegiance
        .iter()
        .filter(|(d, x)| *x >= 0.4 && w.demigods.get(d.ix()).is_some_and(|d| d.alive))
        .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|x| x.0);
    let name = mob_name(w, city, cause_word);
    let id = MobId::from_ix(w.mobs.len());
    let place = cl.place;
    let pos = w.places[place.ix()].center;
    let size: u32 = cells.iter().map(|c| w.cells[c.ix()].size).sum();
    let anger = cells.iter().map(|c| w.cells[c.ix()].mood.anger).fold(0.0, f32::max);
    let fear = cl.mood.fear;
    for c in &cells {
        w.cells[c.ix()].mob = Some(id);
        w.cells[c.ix()].stance = Stance::Rioting;
    }
    w.mobs.push(Mob {
        id,
        name: name.clone(),
        city,
        place,
        pos,
        cells: cells.clone(),
        size,
        anger: r3(anger),
        fear: r3(fear),
        ringleader: None,
        target: None,
        action: MobAction::Gathering,
        cause: cause.clone(),
        formed: cx.beat,
        ended: None,
        casualties: 0,
        kills: 0,
        side,
    });
    w.soc.mobs_formed += 1;
    cx.emit(
        w,
        SocKind::MobFormed,
        format!(
            "{} begins: {size} of the {} take to the streets, driven by {cause}{}",
            cap(&name),
            cl.label,
            banner(w, side)
        ),
        Ref::Cluster(crowd),
        vec![Ref::Mob(id), Ref::Cluster(crowd)],
        pos,
        Some(place),
        5,
        -2,
        json!({ "mob": Ref::Mob(id), "name": name, "size": size, "cells": cells, "cause": cause,
                "side": side.map(Ref::Demigod), "city": Ref::Place(city) }),
    );
    // The ringleader: a veteran of the city's last riots takes the cart again if one is free, else someone steps out of
    // the angriest crowd (structural: a mob always has one).
    let veteran = people::veteran(w, city, &[Role::Ringleader], side);
    let (r, back) = match veteran {
        Some(r) => {
            let title = format!("ringleader of {name}");
            let before = people::life(w, r).map(|l| l.rank.title.clone()).unwrap_or_default();
            let ch = &mut w.characters[r.ix()];
            if !ch.aliases.contains(&title) {
                ch.aliases.push(title.clone());
            }
            if let Some(l) = life_mut(w, r) {
                l.rank = Rank { level: Level::Cell, of: Some(Ref::Mob(id)), title };
                l.deeds += 1;
            }
            (Some(r), Some(before))
        }
        None => {
            let mut spec = Spec::new(
                Role::Ringleader,
                Level::Cell,
                Some(Ref::Mob(id)),
                format!("ringleader of {name}"),
                format!("the ringleader of {name}"),
            );
            spec.ambition = 0.4;
            spec.courage = 0.3;
            spec.cruelty = 0.2;
            (emerge(w, cx, crowd, spec, true), None)
        }
    };
    let Some(r) = r else { return };
    w.mobs[id.ix()].ringleader = Some(r);
    w.cells[lead_cell.ix()].leader = Some(r);
    if let Some(l) = life_mut(w, r) {
        l.mob = Some(id);
        l.cell = Some(lead_cell);
    }
    if let Some(d) = side {
        bump(&mut w.characters[r.ix()].allegiance, d, 0.2, -1.0, 1.0);
    }
    w.clusters[crowd.ix()].leader = Some(r);
    let rname = w.characters[r.ix()].name.clone();
    let text = match back.filter(|t| t.starts_with("ringleader of ")) {
        Some(t) => format!("{rname}, who was {t}, climbs back onto the cart and takes charge of {name}"),
        None => format!("{rname} climbs onto a cart and takes charge of {name}"),
    };
    cx.emit(
        w,
        SocKind::RingleaderEmerged,
        text,
        Ref::Char(r),
        vec![Ref::Char(r), Ref::Mob(id)],
        pos,
        Some(place),
        5,
        -1,
        json!({ "mob": Ref::Mob(id), "ringleader": Ref::Char(r), "name": rname, "veteran": veteran.is_some() }),
    );
}

/// One beat of a live mob: swell, move, act (riot, loot, attack a rival crowd or a hated demigod nearby), dissolve.
fn act(w: &mut World, cx: &mut Ctx, mi: usize) {
    let m = w.mobs[mi].clone();
    let city = m.city;
    let crowds = crowds_under(w, city);
    // Mood follows its cells' crowds.
    let mut anger_sum = 0.0;
    let mut food = 0.0;
    let mut n = 0.0;
    for c in &m.cells {
        let cell = &w.cells[c.ix()];
        if cell.size == 0 {
            continue;
        }
        let cr = &w.clusters[cell.crowd.ix()];
        anger_sum += cr.mood.anger;
        food += cr.need.food;
        n += 1.0;
    }
    let (crowd_anger, food) = if n > 0.0 { (anger_sum / n, food / n) } else { (0.3, 0.0) };
    let mut anger = clamp01(m.anger * 0.7 + crowd_anger * 0.3 - 0.015);
    let age = cx.beat.saturating_sub(m.formed);
    // Dissolve: spent, too small, old and tired, or leaderless.
    let leaderless = alive(w, m.ringleader).is_none();
    if m.size < 20
        || anger < MOB_SPENT
        || (age >= MOB_TIRED_AGE && cx.roll("mob_tired", m.id.0 as u64) < MOB_TIRED_P)
        || (leaderless && cx.roll("mob_leaderless", m.id.0 as u64) < 0.3)
    {
        let why = if m.size < 20 {
            FEW
        } else if anger < MOB_SPENT {
            SPENT
        } else if leaderless {
            LEADERLESS
        } else {
            TIRED
        };
        dissolve(w, cx, mi, why);
        return;
    }
    // A rise: an ambitious, able ringleader of a mob that rallies behind a demigod marches it off to war as a
    // volunteer host, with themselves as its general.
    if let (Some(r), Some(d)) = (alive(w, m.ringleader), m.side.filter(|d| w.demigods.get(d.ix()).is_some_and(|x| x.alive))) {
        let (ambition, comp) = people::life(w, r).map(|l| (l.ambition, l.competence)).unwrap_or((0.0, 0.0));
        let crowd = m.cells.first().map(|c| w.cells[c.ix()].crowd);
        if age >= 3
            && ambition >= 0.5
            && crowd.is_some_and(|k| w.clusters[k.ix()].size >= 1500)
            && crate::sim::armies::armies_of(w, d) < crate::sim::armies::MAX_ARMIES_PER_DEMIGOD
            && cx.roll("mob_to_war", m.id.0 as u64) < 0.04 + 0.12 * comp
        {
            let k = crowd.expect("checked");
            if let Some(army) = crate::sim::armies::raise_host(w, cx, k, d, Some(r)) {
                let (rname, label) = (w.characters[r.ix()].name.clone(), w.clusters[army.ix()].label.clone());
                let dname = w.demigods[d.ix()].name.clone();
                w.soc.promotions += 1;
                cx.emit(
                    w,
                    SocKind::Promotion,
                    format!("{rname} marches {} off to war for {dname}: the ringleader is General of {label} now", m.name),
                    Ref::Char(r),
                    vec![Ref::Char(r), Ref::Mob(m.id), Ref::Cluster(army), Ref::Demigod(d)],
                    m.pos,
                    Some(m.place),
                    6,
                    1,
                    json!({ "person": Ref::Char(r), "from": "ringleader", "to": "general", "mob": Ref::Mob(m.id),
                            "army": Ref::Cluster(army), "how": "the mob went to war" }),
                );
                dissolve(w, cx, mi, MARCHED_OFF);
                return;
            }
        }
    }
    // Swell: angry crowds' free gangs were joined in `form`; a hot mob also pulls people straight off the street.
    if anger > 0.7 && m.size < MOB_MAX {
        let add = ((m.size as f32) * 0.08).ceil() as u32;
        if let Some(c) = m.cells.first() {
            w.cells[c.ix()].size += add;
            w.mobs[mi].size += add;
        }
    }
    let rname = alive(w, m.ringleader).map(|r| w.characters[r.ix()].name.clone());
    let who = match &rname {
        Some(r) => format!("{} under {r}", m.name),
        None => m.name.clone(),
    };
    let cname = w.name_of(Ref::Place(city));
    let mut refs = vec![Ref::Mob(m.id)];
    refs.extend(alive(w, m.ringleader).map(Ref::Char));
    // A hated demigod nearby?
    let mob_crowds: Vec<ClusterId> = m.cells.iter().map(|c| w.cells[c.ix()].crowd).collect();
    let hated = w
        .demigods
        .iter()
        .filter(|d| d.alive && distance_km(d.pos, m.pos) < 150.0)
        .filter(|d| {
            let a: f32 = mob_crowds.iter().map(|k| get(&w.clusters[k.ix()].allegiance, d.id)).sum::<f32>()
                / mob_crowds.len().max(1) as f32;
            a <= -0.3
        })
        .map(|d| d.id)
        .min();
    // A rival crowd in the city (in another demigod's Following, or the mob's side's rival).
    let rival = crowds.iter().copied().filter(|k| !mob_crowds.contains(k)).find(|k| {
        let c = &w.clusters[k.ix()];
        match m.side {
            Some(d) => w
                .demigods
                .iter()
                .any(|o| o.id != d && o.alive && (c.groups.contains(o.following) || get(&c.allegiance, o.id) >= 0.35)),
            None => w.demigods.iter().any(|o| o.alive && c.groups.contains(o.following)) && m.anger > 0.75,
        }
    });
    let (action, target) = if let Some(d) = hated {
        (MobAction::Attacking, Some(Ref::Demigod(d)))
    } else if food >= 0.45 {
        (MobAction::Looting, None)
    } else if let Some(k) = rival {
        (MobAction::Attacking, Some(Ref::Cluster(k)))
    } else {
        (MobAction::Rioting, None)
    };
    // Move to the district of what it goes after.
    let goto = match target {
        Some(Ref::Cluster(k)) => Some(w.clusters[k.ix()].place),
        _ => None,
    };
    if let Some(p) = goto.filter(|p| *p != m.place) {
        w.mobs[mi].place = p;
        w.mobs[mi].pos = w.places[p.ix()].center;
    }
    w.mobs[mi].action = action;
    w.mobs[mi].target = target;
    let pos = w.mobs[mi].pos;
    let place = w.mobs[mi].place;
    match action {
        MobAction::Attacking => match target {
            Some(Ref::Demigod(d)) => {
                let amount = r3((0.3 + m.size as f32 / 8000.0).min(1.5));
                let dname = w.demigods[d.ix()].name.clone();
                cx.out.proposals.push(Proposal {
                    source: Source::Rules,
                    intent: None,
                    effect: Effect::Damage { who: Ref::Demigod(d), amount, cause: format!("torn at by {}", m.name) },
                    act: format!("{} goes for {dname} with stones and torches", cap(&who)),
                    actor: Ref::Place(city),
                    magnitude: 6,
                    valence: -3,
                    witnesses: vec![Ref::Demigod(d)],
                    cause: None,
                });
                let dead = ((m.size as f32) * 0.03).ceil() as u32;
                casualties(w, mi, dead);
                refs.push(Ref::Demigod(d));
                cx.emit(w, SocKind::Riot, format!("{} storms {dname} in {cname} with stones and torches: {dead} rioters fall", cap(&who)), Ref::Place(city), refs, pos, Some(place), 6, -3,
                        json!({ "mob": Ref::Mob(m.id), "target": Ref::Demigod(d), "amount": amount, "dead": dead }));
            }
            Some(Ref::Cluster(k)) => {
                let theirs = ((w.clusters[k.ix()].size as f32) * 0.004 * anger).ceil() as u32;
                let theirs = theirs.min(w.clusters[k.ix()].size);
                let ours = ((m.size as f32) * 0.03).ceil() as u32;
                let c = &mut w.clusters[k.ix()];
                c.size -= theirs;
                c.dead += theirs;
                c.mood.anger = clamp01(c.mood.anger + 0.1);
                c.mood.fear = clamp01(c.mood.fear + 0.08);
                let label = c.label.clone();
                casualties(w, mi, ours);
                w.mobs[mi].kills += theirs;
                if let Some(n) = w.nodes.get_mut(city.ix()) {
                    n.dead += (theirs + ours) as u64;
                }
                w.soc.deaths += (theirs + ours) as u64;
                if let Some(r) = alive(w, m.ringleader) {
                    if let Some(l) = life_mut(w, r) {
                        l.kills += theirs.min(50);
                        l.deeds += 1;
                    }
                }
                refs.push(Ref::Cluster(k));
                cx.emit(w, SocKind::Riot, format!("{} falls on the {label}: {theirs} of them dead, {ours} rioters with them", cap(&who)),
                        Ref::Place(city), refs, pos, Some(place), if theirs + ours >= 100 { 5 } else { 4 }, -3,
                        json!({ "mob": Ref::Mob(m.id), "target": Ref::Cluster(k), "dead": theirs, "mob_dead": ours }));
            }
            _ => {}
        },
        MobAction::Looting => {
            let took = 0.02 + 0.02 * (m.size as f32 / 3000.0).min(1.0);
            if let Some(n) = w.nodes.get_mut(city.ix()) {
                n.supply = r3((n.supply - took).max(0.0));
            }
            anger = clamp01(anger - 0.05);
            // The looted (the richest crowd, if another) are furious.
            if let Some(k) = crowds.iter().copied().filter(|k| !mob_crowds.contains(k)).max_by_key(|k| (w.clusters[k.ix()].size, std::cmp::Reverse(k.0))) {
                let c = &mut w.clusters[k.ix()];
                c.mood.anger = clamp01(c.mood.anger + 0.05);
            }
            let dead = ((m.size as f32) * 0.005).ceil() as u32;
            casualties(w, mi, dead);
            if let Some(n) = w.nodes.get_mut(city.ix()) {
                n.dead += dead as u64;
            }
            w.soc.deaths += dead as u64;
            let news = m.action != MobAction::Looting || age.is_multiple_of(3);
            cx.emit(w, SocKind::Looting, format!("{} breaks open the granaries of {cname}: the city's stores fall to {}%", cap(&who), (100.0 * w.nodes.get(city.ix()).map(|n| n.supply).unwrap_or(0.0)).round()), Ref::Place(city), refs, pos,
                    Some(place), if m.size >= 1000 && news { 4 } else { 3 }, -2,
                    json!({ "mob": Ref::Mob(m.id), "supply_taken": r3(took), "dead": dead }));
        }
        _ => {
            let dead = ((m.size as f32) * 0.01).ceil() as u32;
            casualties(w, mi, dead);
            if let Some(n) = w.nodes.get_mut(city.ix()) {
                n.supply = r3((n.supply - 0.01).max(0.0));
                n.dead += dead as u64;
            }
            w.soc.deaths += dead as u64;
            crate::sim::hier::push_down(w, city, Mood { anger: 0.0, fear: 0.03, hope: -0.02, devotion: 0.0 }, 1.0);
            let news = m.action != MobAction::Rioting || age.is_multiple_of(3);
            cx.emit(w, SocKind::Riot, format!("{} tears through {cname}: {dead} trampled in the crush, shops burned", cap(&who)), Ref::Place(city), refs, pos,
                    Some(place), if m.size >= 1500 && news { 4 } else { 3 }, -2, json!({ "mob": Ref::Mob(m.id), "dead": dead }));
        }
    }
    w.mobs[mi].anger = anger;
}

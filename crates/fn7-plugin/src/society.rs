//! The hierarchical society inside the beat (schema 4): what is in focus (level of detail), hands acting on the
//! hierarchy (orders to generals, promotions, bribes), the general mind's calls, running the society step and turning
//! its events into ledger rows and trace events, and the hierarchy payloads for the zoomed views.

use std::time::Instant;

use fn7_agents::general::GeneralCtx;
use fn7_agents::Entity;
use fn7_model::*;
use fn7_society::geo::{city_of, distance_km};
use fn7_society::sim::armies::{decision_points, judge};
use serde_json::{json, Value};

use crate::merge;
use crate::settings::Settings;
use crate::world::*;

const ORDER_WORDS: &[&str] = &[
    "tell", "order", "command", "take", "attack", "march", "hold", "retreat", "defend", "raid", "go", "send", "hunt",
    "besiege", "storm", "capture", "seize", "guard", "fall back", "invade", "conquer", "move", "deal with", "crush",
];
const PROMOTE_WORDS: &[&str] = &["promote", "appoint", "elevate", "knight", "put in charge", "make him", "make her", "make them"];
const BRIBE_WORDS: &[&str] = &["bribe", "buy off", "buy", "pay off", "pay", "win over", "seduce", "flip", "turn"];

fn has(text: &str, words: &[&str]) -> bool {
    words.iter().any(|w| fn7_agents::common::has_phrase(text, w))
}

/// Refs in focus this beat (level of detail): each real seat's focus and the place it is zoomed on (city or region),
/// plus everything the hands addressed this beat (intents from `from`).
pub fn focus(w: &World, from: usize) -> Vec<Ref> {
    let mut v: Vec<Ref> = vec![];
    for s in &w.seats {
        let d = &w.demigods[s.demigod.ix()];
        if !d.alive {
            continue;
        }
        if let Some(f) = s.focus {
            v.push(f);
        }
        let place = s.focus.and_then(|f| fn7_society::place_of(w, f)).unwrap_or(d.place);
        match s.zoom {
            Zoom::World => {}
            Zoom::District => v.push(Ref::Place(district_of(w, place))),
            Zoom::City | Zoom::Character => v.push(Ref::Place(city_of(w, place).unwrap_or(place))),
        }
    }
    for i in w.intents.iter().skip(from) {
        for t in &i.targets {
            if let Target::Known(r) = t {
                if matches!(r, Ref::Char(_) | Ref::Cluster(_) | Ref::Mob(_) | Ref::Cell(_) | Ref::Place(_)) {
                    v.push(*r);
                }
            }
        }
    }
    v.sort_unstable();
    v.dedup();
    v
}

/// What a hand's world act does to the hierarchy, if anything.
pub enum SocietyAct {
    /// An order for the general of `army` (the general mind takes it).
    Order { army: ClusterId, target: Option<Ref>, text: String },
    /// Promotions and bribes, as proposals.
    Props(Vec<Proposal>),
}

fn known(i: &Intent) -> Vec<Ref> {
    i.targets
        .iter()
        .filter_map(|t| match t {
            Target::Known(r) => Some(*r),
            _ => None,
        })
        .collect()
}

fn role_of(w: &World, c: CharId) -> Option<Role> {
    w.characters.get(c.ix()).and_then(|x| x.life.as_ref()).map(|l| l.role)
}

fn army_of_general(w: &World, c: CharId) -> Option<ClusterId> {
    w.clusters
        .iter()
        .find(|k| k.size > 0 && k.army.as_ref().is_some_and(|a| a.general == Some(c)))
        .map(|k| k.id)
}

/// Is this world act about the hierarchy (orders to a general, a promotion, a bribe)? Standing orders never re-order
/// a general (the general keeps acting on the last order it understood).
pub fn society_intent(w: &World, i: &Intent) -> Option<SocietyAct> {
    if !matches!(i.route, Route::World | Route::Pending) {
        return None;
    }
    let d = i.demigod;
    let text = format!("{} {} {}", i.verb, i.span, i.how).to_lowercase();
    let refs = known(i);
    let people: Vec<CharId> = refs
        .iter()
        .filter_map(|r| match r {
            Ref::Char(c) if w.characters[c.ix()].alive && w.characters[c.ix()].serves.is_none() => Some(*c),
            _ => None,
        })
        .collect();
    let mk = |effect: Effect, act: String, witnesses: Vec<Ref>, magnitude: u8| Proposal {
        source: Source::Agent { agent: i.agent },
        intent: Some(i.id),
        effect,
        act,
        actor: Ref::Demigod(d),
        magnitude,
        valence: 0,
        witnesses,
        cause: None,
    };
    let dname = w.demigods[d.ix()].name.clone();
    // Promotions.
    if has(&text, PROMOTE_WORDS) {
        if let Some(&c) = people.first() {
            let role = if text.contains("general") {
                Role::General
            } else if text.contains("prophet") {
                Role::Prophet
            } else if text.contains("ringleader") && role_of(w, c) != Some(Role::Ringleader) {
                Role::Ringleader
            } else {
                Role::Captain
            };
            let army = refs.iter().find_map(|r| match r {
                Ref::Cluster(k) if w.clusters[k.ix()].army.as_ref().is_some_and(|a| a.demigod == d) => Some(*k),
                _ => None,
            });
            let n = w.characters[c.ix()].name.clone();
            return Some(SocietyAct::Props(vec![mk(
                Effect::Promote { who: c, by: d, role, army },
                format!("{dname} raises {n} to {}", format!("{role:?}").to_lowercase()),
                vec![Ref::Char(c), Ref::Demigod(d)],
                4,
            )]));
        }
    }
    // Bribes: the named people, and a general's captains when the words say so.
    if has(&text, BRIBE_WORDS) && !text.contains("tribute") {
        let mut who: Vec<CharId> = people.clone();
        for r in &refs {
            let army = match r {
                Ref::Cluster(k) if w.clusters[k.ix()].army.is_some() => Some(*k),
                Ref::Char(c) => army_of_general(w, *c),
                _ => None,
            };
            if let Some(a) = army.and_then(|k| w.clusters[k.ix()].army.as_ref()) {
                if text.contains("captain") || matches!(r, Ref::Cluster(_)) {
                    who.extend(a.captains.iter().filter(|c| w.characters[c.ix()].alive));
                }
                if matches!(r, Ref::Cluster(_)) {
                    who.extend(a.general.filter(|g| w.characters[g.ix()].alive));
                }
            }
        }
        who.sort_unstable();
        who.dedup();
        who.truncate(4);
        if !who.is_empty() {
            let paid = w.demigods[d.ix()].bolts >= 1;
            let delta = if paid { 0.35 } else { 0.15 };
            let mut props = vec![];
            if paid {
                props.push(mk(
                    Effect::Bolts { who: d, delta: -1, to: None, toll: false },
                    format!("{dname} spends a bolt on bribes"),
                    vec![Ref::Demigod(d)],
                    0,
                ));
            }
            for c in who {
                let n = w.characters[c.ix()].name.clone();
                props.push(mk(
                    Effect::Allegiance { who: Ref::Char(c), demigod: d, delta },
                    if paid { format!("{dname}'s gold finds {n}") } else { format!("{dname} sweet-talks {n}") },
                    vec![Ref::Char(c)],
                    3,
                ));
            }
            return Some(SocietyAct::Props(props));
        }
    }
    if i.how == crate::beat::STANDING_HOW {
        return None;
    }
    // Orders to a general: addressed by name, or the demigod's army addressed, or any army act when it has a general.
    let army = people
        .iter()
        .find_map(|c| army_of_general(w, *c).filter(|k| w.clusters[k.ix()].army.as_ref().is_some_and(|a| a.demigod == d && !a.rogue)))
        .or_else(|| {
            refs.iter().find_map(|r| match r {
                Ref::Cluster(k) if w.clusters[k.ix()].army.as_ref().is_some_and(|a| a.demigod == d && !a.rogue) => Some(*k),
                _ => None,
            })
        })
        .or_else(|| {
            (crate::beat::is_army_act(w, i) || text.contains("general"))
                .then(|| crate::beat::own_army(w, d))
                .flatten()
                .filter(|_| !has(&text, &["raise", "summon", "muster", "conscript", "form", "new army"]))
        })?;
    let a = w.clusters[army.ix()].army.as_ref()?;
    a.general.filter(|g| w.characters[g.ix()].alive)?;
    if !has(&text, ORDER_WORDS) && !crate::beat::is_army_act(w, i) && !text.contains("general") {
        return None;
    }
    let target = refs.iter().copied().find(|r| match r {
        Ref::Place(_) | Ref::Demigod(_) | Ref::Mob(_) => *r != Ref::Demigod(d),
        Ref::Cluster(k) => *k != army && w.clusters[k.ix()].army.is_some(),
        Ref::Char(c) => army_of_general(w, *c).is_some_and(|k| k != army),
        _ => false,
    });
    let target = match target {
        Some(Ref::Char(c)) => army_of_general(w, c).map(Ref::Cluster),
        t => t,
    };
    Some(SocietyAct::Order { army, target, text: i.span.chars().take(240).collect() })
}

/// Is this intent an act on the hierarchy (kept out of standing orders)?
pub fn is_society_act(w: &World, i: &Intent) -> bool {
    society_intent(w, i).is_some()
}

/// Who and why a general call was made (for its reply).
#[derive(Debug, Clone)]
pub struct GeneralMeta {
    pub army: ClusterId,
    pub by: DemigodId,
    pub why: &'static str,
    pub text: String,
    pub agent: Option<AgentId>,
    pub intent: Option<IntentId>,
}

/// A general mind call for `army` (the rules' judgment inside as the fallback).
pub fn general_call(w: &World, m: &GeneralMeta, target: Option<Ref>, beat: u32) -> Option<GeneralCtx> {
    let c = w.clusters.get(m.army.ix())?;
    let a = c.army.as_ref()?;
    let g = a.general.filter(|g| w.characters[g.ix()].alive)?;
    let gch = &w.characters[g.ix()];
    let l = gch.life.as_deref();
    let loyalty = gch.allegiance.iter().find(|x| x.0 == m.by).map(|x| x.1).unwrap_or(0.0);
    let sheet = match l {
        Some(l) => format!(
            "competence {:.1}, loyalty {:.1} to {}, ambition {:.1}, {} (cruelty {:.1}), courage {:.1}{}",
            l.competence,
            loyalty,
            w.demigods[m.by.ix()].name,
            l.ambition,
            if l.cruelty >= 0.3 { "cruel" } else if l.cruelty <= -0.3 { "merciful" } else { "hard" },
            l.cruelty,
            l.courage,
            if l.grudges.is_empty() {
                String::new()
            } else {
                format!("; grudges against {}", l.grudges.iter().map(|r| w.name_of(*r)).collect::<Vec<_>>().join(", "))
            }
        ),
        None => format!("loyalty {loyalty:.1}"),
    };
    let rules = judge(w, m.army, &m.text, target, m.by, m.why, beat);
    let mut opts: Vec<Ref> = vec![];
    opts.extend(target);
    opts.extend(rules.target);
    opts.extend(fn7_society::sim::armies::initiative_target(w, m.army));
    if let Some(h) = w.clusters.get(a.from.ix()).and_then(|f| city_of(w, f.place)) {
        opts.push(Ref::Place(h));
    }
    let mut cities: Vec<(f64, PlaceId)> = w
        .places
        .iter()
        .filter(|p| city_of(w, p.id) == Some(p.id))
        .map(|p| (distance_km(p.center, a.pos), p.id))
        .collect();
    cities.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
    opts.extend(cities.into_iter().take(4).map(|x| Ref::Place(x.1)));
    for d in w.demigods.iter().filter(|d| d.alive && d.id != m.by) {
        opts.push(Ref::Demigod(d.id));
    }
    for (k, b) in fn7_society::armies(w) {
        if k.id != m.army && (b.rogue || b.demigod != m.by) && distance_km(a.pos, b.pos) < 3000.0 {
            opts.push(Ref::Cluster(k.id));
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    opts.retain(|r| seen.insert(*r));
    opts.truncate(12);
    let options: Vec<Entity> = opts.iter().filter_map(|r| entity(w, *r)).collect();
    let rivals: Vec<Entity> = w
        .demigods
        .iter()
        .filter(|d| d.alive && d.id != m.by)
        .filter_map(|d| entity(w, Ref::Demigod(d.id)))
        .collect();
    let dg = &w.demigods[m.by.ix()];
    let mut situation = vec![
        format!("Your army: {}", army_blurb(w, c, a)),
        format!("Strain {:.1} (losses {}, battles {}). Your demigod {} has health {:.0}/{:.0}.", a.strain, a.losses, a.battles, dg.name, dg.health, dg.max_health),
        format!("Where you are: {}.", crate::payload::region_name(w, a.pos)),
    ];
    if let Some(o) = &a.order {
        situation.push(format!("Your last orders: {}", if o.text.is_empty() { "your own initiative".into() } else { o.text.clone() }));
    }
    let captains: Vec<String> = a.captains.iter().filter(|x| w.characters[x.ix()].alive).map(|x| w.characters[x.ix()].name.clone()).collect();
    if !captains.is_empty() {
        situation.push(format!("Your captains: {}.", captains.join(", ")));
    }
    Some(GeneralCtx {
        army: m.army,
        army_name: c.label.clone(),
        general: gch.name.clone(),
        sheet,
        demigod: m.by,
        demigod_name: dg.name.clone(),
        why: m.why.to_string(),
        order: m.text.clone(),
        situation,
        options,
        rivals,
        rules,
    })
}

/// Decision points worth the general mind this beat: engagements and strain of armies in focus (or any army when
/// nothing is in focus), not already ordered this beat, at most `max`.
pub fn decision_calls(w: &World, focus: &[Ref], ordered: &[ClusterId], max: usize) -> Vec<(GeneralMeta, Option<Ref>)> {
    let focus_cities = fn7_society::sim::focus_cities(w, focus);
    let mut out = vec![];
    for (army, why, enemy) in decision_points(w) {
        if out.len() >= max || ordered.contains(&army) {
            continue;
        }
        let c = &w.clusters[army.ix()];
        let a = c.army.as_ref().expect("army");
        let here = city_of(w, fn7_society::geo::place_at(w, a.pos));
        let in_focus = focus.contains(&Ref::Cluster(army))
            || a.general.is_some_and(|g| focus.contains(&Ref::Char(g)))
            || here.is_some_and(|h| focus_cities.contains(&h))
            || focus.iter().any(|f| matches!(f, Ref::Place(p) if fn7_society::sim::hier::under(w, c.place, *p)));
        // Engagements are big moments: asked even out of focus; strain only in focus.
        if why == "strain" && !in_focus {
            continue;
        }
        out.push((
            GeneralMeta { army, by: a.demigod, why, text: String::new(), agent: None, intent: None },
            enemy,
        ));
    }
    out
}

/// The `Orders` proposal from a general's judgment.
pub fn orders_prop(w: &World, m: &GeneralMeta, j: Judgment, llm: bool) -> Proposal {
    let a = w.clusters[m.army.ix()].army.as_ref();
    let g = a.and_then(|a| a.general).filter(|g| w.characters[g.ix()].alive);
    let actor = g.map(Ref::Char).unwrap_or(Ref::Cluster(m.army));
    let label = w.clusters[m.army.ix()].label.clone();
    let act = if j.speech.is_empty() {
        format!("{} takes {}'s order", g.map(|g| w.characters[g.ix()].name.clone()).unwrap_or(label), w.demigods[m.by.ix()].name)
    } else {
        j.speech.clone()
    };
    let mut witnesses = vec![actor, Ref::Cluster(m.army)];
    if m.why == "order" {
        witnesses.push(Ref::Demigod(m.by));
    }
    Proposal {
        source: match m.agent {
            Some(agent) => Source::Agent { agent },
            None => Source::Rules,
        },
        intent: m.intent,
        effect: Effect::Orders { army: m.army, by: m.by, text: m.text.clone(), judgment: j, why: m.why.to_string(), llm },
        act,
        actor,
        magnitude: if m.why == "order" { 4 } else { 3 },
        valence: 0,
        witnesses,
        cause: None,
    }
}

/// What the society step produced, for the beat.
pub struct SocietyRun {
    pub ms: f64,
    pub events: usize,
    pub dropped: u32,
    pub emerged: usize,
}

/// Run the society step for `beat`, record its events as `Effect::Society` rows (plus demigod-touching proposals),
/// emit a trace event per society event (its kind), write the new people's `memorized` events, and store the level of
/// detail on the beat's record.
pub fn run_step(w: &mut World, beat: u32, focus: &[Ref], llm_refs: Vec<Ref>, pantheon: &Pantheon, st: &Settings) -> SocietyRun {
    let t0 = Instant::now();
    let out = fn7_society::sim::step(w, beat, focus);
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let n_events = out.events.len();
    let mut props: Vec<Proposal> = vec![];
    for e in &out.events {
        let actor = if fn7_society::is_alive(w, e.actor) { e.actor } else { e.place.map(Ref::Place).unwrap_or(Ref::Place(w.meta.arena)) };
        let mut witnesses: Vec<Ref> = e
            .refs
            .iter()
            .copied()
            .filter(|r| matches!(r, Ref::Char(c) if w.characters[c.ix()].alive) || matches!(r, Ref::Cluster(_) | Ref::Demigod(_)))
            .take(3)
            .collect();
        if matches!(actor, Ref::Char(_)) && !witnesses.contains(&actor) {
            witnesses.insert(0, actor);
        }
        props.push(Proposal {
            source: Source::Rules,
            intent: None,
            effect: Effect::Society { kind: e.kind, refs: e.refs.clone(), at: e.at, place: e.place },
            act: e.text.clone(),
            actor,
            magnitude: e.magnitude,
            valence: e.valence,
            witnesses,
            cause: None,
        });
    }
    props.extend(out.proposals);
    let (_, applied) = merge::run(w, props, pantheon, beat, st.max_witnesses as usize);
    for (i, row) in &applied.rows {
        if let Some(e) = out.events.get(*i).filter(|e| e.kind != SocKind::PersonEmerged) {
            let mut data = json!({ "beat": beat, "row": row, "text": e.text, "refs": e.refs, "at": e.at, "place": e.place,
                                   "magnitude": e.magnitude });
            if let (Some(o), Value::Object(extra)) = (data.as_object_mut(), &e.data) {
                for (k, v) in extra {
                    o.insert(k.clone(), v.clone());
                }
            }
            event(e.kind.as_str(), Some(e.actor.to_string()), data);
        }
    }
    // Every person who entered the story: a `person_emerged` event with their sheet (their ledger row, a small one,
    // is kept only in focus), then the `memorized` event and their inherited memories.
    for c in &out.new_people {
        let x = &w.characters[c.ix()];
        let from = match x.origin {
            Origin::Materialized { cluster, .. } => Some(Ref::Cluster(cluster)),
            _ => None,
        };
        let mut data = person_json(w, *c);
        data["beat"] = beat.into();
        data["from"] = json!(from);
        event(SocKind::PersonEmerged.as_str(), Some(Ref::Char(*c).to_string()), data);
        merge::memorized(w, *c, beat);
    }
    let mut fid = out.fidelity;
    fid.llm = llm_refs;
    event(
        "society_step",
        None,
        json!({ "beat": beat, "ms": ms, "events": n_events, "dropped": out.dropped, "emerged": out.new_people.len(),
                "fidelity": fid, "totals": w.soc }),
    );
    if let Some(r) = w.beats.iter_mut().rev().find(|r| r.beat == beat) {
        r.fidelity = Some(fid);
    }
    SocietyRun { ms, events: n_events, dropped: out.dropped, emerged: out.new_people.len() }
}

// ---------------------------------------------------------------------------------------------------------------------
// Payloads (data contract only)
// ---------------------------------------------------------------------------------------------------------------------

pub(crate) fn mood_json(m: &Mood) -> Value {
    json!({ "anger": m.anger, "fear": m.fear, "hope": m.hope, "devotion": m.devotion })
}

pub(crate) fn alleg_json(w: &World, v: &[(DemigodId, f32)]) -> Vec<Value> {
    v.iter()
        .filter_map(|(d, x)| w.demigods.get(d.ix()).map(|dg| json!({ "demigod": dg.name, "demigod_ref": Ref::Demigod(*d), "value": x })))
        .collect()
}

pub(crate) fn who_json(w: &World, c: Option<CharId>) -> Value {
    match c.and_then(|c| w.characters.get(c.ix())) {
        Some(x) => json!({ "ref": Ref::Char(x.id), "name": x.name, "alive": x.alive,
                            "role": x.life.as_ref().map(|l| l.role), "title": x.life.as_ref().map(|l| l.rank.title.clone()),
                            "pos": x.pos, "cell": people_cell(x.pos) }),
        None => Value::Null,
    }
}

/// One upper node: `{ref, name, level, population, base, mood, need, allegiance, worship, supply, stance, leader, dead,
/// at, region}`.
pub fn node_json(w: &World, p: PlaceId) -> Value {
    let Some(n) = w.nodes.get(p.ix()) else { return Value::Null };
    let pl = &w.places[p.ix()];
    json!({ "ref": Ref::Place(p), "name": pl.name, "level": n.level, "population": n.population, "base": n.base,
            "mood": mood_json(&n.mood), "need": n.need, "allegiance": alleg_json(w, &n.allegiance),
            "worship": n.worship.iter().map(|(g, x)| json!({ "god": w.name_of(Ref::God(*g)), "god_ref": Ref::God(*g), "value": x })).collect::<Vec<_>>(),
            "supply": n.supply, "stance": n.stance, "leader": who_json(w, n.leader), "dead": n.dead, "at": pl.center,
            "parent": pl.parent.map(Ref::Place) })
}

/// One crowd: `{ref, name, size, place, at, mood, need, stance, allegiance, leader, dead, cells}`.
pub fn crowd_json(w: &World, k: ClusterId) -> Value {
    let c = &w.clusters[k.ix()];
    let cells: Vec<Value> = c.cells.iter().filter(|x| w.cells[x.ix()].ended.is_none()).map(|x| cell_json(w, *x)).collect();
    let pos = piece_pos(w, k);
    json!({ "ref": Ref::Cluster(k), "name": c.label, "size": c.size, "place": Ref::Place(c.place),
            "at": fn7_society::geo::pos_of(w, Ref::Cluster(k)), "mood": mood_json(&c.mood), "need": c.need, "stance": c.stance,
            "allegiance": alleg_json(w, &c.allegiance), "leader": who_json(w, c.leader), "dead": c.dead, "cells": cells,
            "pos": pos, "cell": people_cell(pos),
            "village": fn7_model::hexgrid::cell_at(pos, fn7_model::hexgrid::HexLevel::Villages).key() })
}

/// Where a crowd's piece stands on the table (rules 0.7.0): crowds live at their place's centre (`at`), so each gets a
/// fixed spot around it (seeded by the match and the crowd: within a third of the place's radius, at most 3 km), so
/// the pieces of a city's crowds don't stack on one hex.
pub fn piece_pos(w: &World, k: ClusterId) -> LonLat {
    let c = &w.clusters[k.ix()];
    let (center, radius) = w.places.get(c.place.ix()).map(|p| (p.center, p.radius_km)).unwrap_or(([0.0, 0.0], 1.0));
    let mut r = fn7_model::seed::Rng::derive(w.meta.seed, &[fn7_model::seed::hash_str("piece"), k.0 as u64]);
    let km = (radius / 3.0).clamp(0.05, 3.0) * (0.2 + 0.8 * r.f64());
    fn7_society::geo::destination(center, r.f64() * 360.0, km)
}

pub fn cell_json(w: &World, id: CellId) -> Value {
    let x = &w.cells[id.ix()];
    json!({ "ref": Ref::Cell(id), "name": x.name, "kind": x.kind, "size": x.size, "crowd": Ref::Cluster(x.crowd),
            "leader": who_json(w, x.leader), "stance": x.stance, "mob": x.mob.map(Ref::Mob),
            "at": fn7_society::geo::pos_of(w, Ref::Cell(id)) })
}

/// One person: `{ref, name, alive, role, rank, competence, ambition, cruelty, courage, loyalty, grudges, need, goal,
/// emerged_as, emerged_beat, deeds, kills, died_beat, army, mob, cell, place, at}`.
pub fn person_json(w: &World, c: CharId) -> Value {
    let x = &w.characters[c.ix()];
    let mut v = json!({ "ref": Ref::Char(c), "name": x.name, "alive": x.alive, "place": Ref::Place(x.place), "at": x.pos,
                        "loyalty": alleg_json(w, &x.allegiance), "traits": x.traits });
    if let Some(l) = x.life.as_deref() {
        let o = v.as_object_mut().expect("object");
        o.insert("role".into(), json!(l.role));
        o.insert("rank".into(), json!(l.rank));
        o.insert("competence".into(), json!(l.competence));
        o.insert("ambition".into(), json!(l.ambition));
        o.insert("cruelty".into(), json!(l.cruelty));
        o.insert("courage".into(), json!(l.courage));
        o.insert("grudges".into(), json!(l.grudges));
        o.insert("need".into(), json!(l.need));
        o.insert("goal".into(), json!(l.goal));
        o.insert("emerged_as".into(), json!(l.emerged_as));
        o.insert("emerged_beat".into(), json!(l.emerged));
        o.insert("deeds".into(), json!(l.deeds));
        o.insert("kills".into(), json!(l.kills));
        o.insert("died_beat".into(), json!(l.died));
        o.insert("army".into(), json!(l.army.map(Ref::Cluster)));
        o.insert("mob".into(), json!(l.mob.map(Ref::Mob)));
        o.insert("cell".into(), json!(l.cell.map(Ref::Cell)));
    }
    v
}

pub fn mob_json(w: &World, m: &Mob) -> Value {
    json!({ "ref": Ref::Mob(m.id), "name": m.name, "size": m.size, "action": m.action, "target": m.target,
            "pos": m.pos, "cell": people_cell(m.pos), "ringleader_ref": m.ringleader.map(Ref::Char),
            "ringleader": who_json(w, m.ringleader), "cause": m.cause, "at": m.pos, "city": Ref::Place(m.city),
            "place": Ref::Place(m.place), "region": crate::payload::region_name(w, m.pos), "side": m.side.map(Ref::Demigod),
            "anger": m.anger, "fear": m.fear, "cells": m.cells.iter().map(|c| Ref::Cell(*c)).collect::<Vec<_>>(), "formed_beat": m.formed, "ended_beat": m.ended,
            "casualties": m.casualties, "kills": m.kills })
}

/// An army's chain of command: general, captains, units, morale, supply, stance, strain, the order as understood
/// (`order.text` only for its owner).
pub fn chain_json(w: &World, a: &Army, owner: bool) -> Value {
    json!({
        "general": a.general.map(|g| person_json(w, g)),
        "captains": a.captains.iter().map(|c| person_json(w, *c)).collect::<Vec<_>>(),
        "units": a.units.iter().filter(|u| w.cells[u.ix()].ended.is_none()).map(|u| cell_json(w, *u)).collect::<Vec<_>>(),
        "morale": a.morale, "supply": a.supply, "stance": a.stance, "strain": a.strain, "rogue": a.rogue,
        "battles": a.battles, "losses": a.losses, "volunteer": a.volunteer,
        "order": a.order.as_ref().map(|o| json!({
            "text": if owner { Some(o.text.clone()) } else { None }, "by": Ref::Demigod(o.by), "beat": o.beat,
            "reading": o.judgment.reading, "target": o.judgment.target, "stance": o.judgment.stance,
            "speech": o.judgment.speech, "why": o.why, "llm": o.llm })),
    })
}

/// Recent battles: `[{id, beat, at, place, sides: [{who, demigod, general, before, losses}], winner, heroes, cowards,
/// fallen, text}]`.
pub fn battles_json(w: &World, within: Option<PlaceId>, n: usize) -> Vec<Value> {
    w.battles
        .iter()
        .rev()
        .filter(|b| within.is_none_or(|p| fn7_society::sim::hier::under(w, b.place, p)))
        .take(n)
        .map(|b| json!(b))
        .collect()
}

/// The hierarchy for a seat's zoom: the node in view and its path up to the planet, its children (regions, cities,
/// districts), its crowds and cells (city and district views), the people ranked there, the armies with their chains
/// of command, mobs and recent battles. Character zoom adds the person's own chain (cell → crowd → district → city →
/// region). Every entry is geotagged (`at`).
pub fn hierarchy_json(w: &World, d: DemigodId, zoom: Zoom, focus: Option<Ref>) -> Value {
    if w.nodes.is_empty() {
        return Value::Null;
    }
    let me = &w.demigods[d.ix()];
    let place = focus.and_then(|f| fn7_society::place_of(w, f)).unwrap_or(me.place);
    let node = match zoom {
        Zoom::World => w.meta.arena,
        Zoom::District => district_of(w, place),
        Zoom::City | Zoom::Character => city_of(w, place).unwrap_or(place),
    };
    let mut path = vec![];
    let mut p = Some(node);
    while let Some(x) = p {
        path.push(json!({ "ref": Ref::Place(x), "name": w.places[x.ix()].name, "level": w.nodes.get(x.ix()).map(|n| n.level) }));
        p = w.places[x.ix()].parent;
    }
    path.reverse();
    let children: Vec<Value> = w
        .places
        .iter()
        .filter(|c| c.parent == Some(node))
        .map(|c| node_json(w, c.id))
        .collect();
    let inside = |p: PlaceId| fn7_society::sim::hier::under(w, p, node);
    let crowds: Vec<Value> = if matches!(zoom, Zoom::City | Zoom::Character) {
        w.clusters.iter().filter(|c| c.army.is_none() && c.size > 0 && inside(c.place)).map(|c| crowd_json(w, c.id)).collect()
    } else {
        vec![]
    };
    let weight = |r: Role| match r {
        Role::General => 0,
        Role::Ringleader => 1,
        Role::Prophet => 2,
        Role::Captain => 3,
        Role::Hero => 4,
        Role::Paladin => 5,
        _ => 6,
    };
    let mut ppl: Vec<&Character> = w
        .characters
        .iter()
        .filter(|c| c.alive && c.life.is_some() && inside(c.place))
        .collect();
    ppl.sort_by_key(|c| (weight(c.life.as_ref().map(|l| l.role).unwrap_or_default()), c.id));
    let people: Vec<Value> = ppl.into_iter().take(40).map(|c| person_json(w, c.id)).collect();
    let armies: Vec<Value> = fn7_society::armies(w)
        .filter(|(c, _)| zoom == Zoom::World || inside(c.place))
        .map(|(c, a)| {
            let mut v = json!({ "ref": Ref::Cluster(c.id), "name": c.label, "size": c.size, "at": a.pos,
                                "demigod_ref": (!a.rogue).then_some(Ref::Demigod(a.demigod)), "target": a.target });
            v["chain"] = chain_json(w, a, a.demigod == d);
            v
        })
        .collect();
    let mobs: Vec<Value> = w.mobs.iter().filter(|m| m.ended.is_none() && inside(m.place)).map(|m| mob_json(w, m)).collect();
    let mut v = json!({
        "level": w.nodes.get(node.ix()).map(|n| n.level),
        "node": node_json(w, node),
        "path": path,
        "children": children,
        "crowds": crowds,
        "people": people,
        "armies": armies,
        "mobs": mobs,
        "battles": battles_json(w, (zoom != Zoom::World).then_some(node), 6),
    });
    if let (Zoom::Character, Some(Ref::Char(c))) = (zoom, focus) {
        if let Some(x) = w.characters.get(c.ix()) {
            let mut chain = vec![];
            if let Some(cell) = x.life.as_ref().and_then(|l| l.cell) {
                chain.push(json!({ "ref": Ref::Cell(cell), "name": w.cells[cell.ix()].name, "level": Level::Cell }));
            }
            if let Origin::Materialized { cluster, .. } = x.origin {
                chain.push(json!({ "ref": Ref::Cluster(cluster), "name": w.clusters[cluster.ix()].label, "level": Level::Crowd }));
            }
            v["person"] = person_json(w, c);
            v["person_chain"] = json!(chain);
        }
    }
    v
}

/// The society for the public view: running totals, live mobs, the latest battles, who the simulation has brought
/// into the story (counts by role).
pub fn public_json(w: &World) -> Value {
    let mut roles: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    for c in w.characters.iter().filter(|c| c.alive) {
        if let Some(l) = c.life.as_deref() {
            *roles.entry(format!("{:?}", l.role).to_lowercase()).or_default() += 1;
        }
    }
    json!({
        "totals": w.soc,
        "people_by_role": roles,
        "mobs": w.mobs.iter().filter(|m| m.ended.is_none()).map(|m| mob_json(w, m)).collect::<Vec<_>>(),
        "battles": battles_json(w, None, 5),
        "population": w.nodes.get(w.meta.arena.ix()).map(|n| n.population),
    })
}

/// The society's own action in beat `b` for the narrator: its rows by magnitude, one line each.
pub fn narrator_lines(w: &World, b: u32, n: usize) -> Vec<String> {
    let mut rows: Vec<&LedgerRow> = w
        .ledger
        .rows
        .iter()
        .rev()
        .take_while(|r| r.beat >= b)
        .filter(|r| r.beat == b && SocKind::of(&r.verb).is_some() && r.magnitude >= 4)
        .collect();
    rows.sort_by(|x, y| y.magnitude.cmp(&x.magnitude).then(x.id.cmp(&y.id)));
    rows.into_iter().take(n).map(|r| r.act.clone()).collect()
}

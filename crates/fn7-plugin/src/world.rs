//! Lookups over the `World` the plugin shares between hooks: state (de)serialization, place hierarchy, who acts for whom,
//! what a seat knows, and the entity lists minds and seats see.

use fn7_agents::Entity;
use fn7_model::*;
use fn7_society::geo::distance_km;
use serde_json::Value;

use crate::settings::Settings;

pub fn load(state: &Value) -> anyhow::Result<World> {
    Ok(<World as serde::Deserialize>::deserialize(state)?)
}

pub fn save(w: &World) -> Value {
    serde_json::to_value(w).expect("World serializes")
}

pub fn settings(w: &World) -> Settings {
    let mut s = Settings::from_value(&w.meta.settings);
    // Rules 0.8.1 is gated on the state schema too: a schema-6 (or older) state never duels and keeps the 0.8.0
    // summon, heal and rename rules, whatever its settings say.
    if !w.challenge_rules() {
        s.challenge_enabled = false;
        s.rules_0_8_1 = false;
    }
    // Rules 0.9.0 ("wills that matter") needs the 0.8.1 rules under it and its own room setting.
    if !s.rules_0_8_1 || !w.wills_rules() {
        s.wills_enabled = false;
    }
    // Gear looks need a schema-11 state (an older state never tracks loadouts, whatever its settings say).
    if !w.look_rules() {
        s.looks_enabled = false;
    }
    // The Earth simulator needs a schema-13 state and the whole planet.
    if !w.earth_rules() || !w.is_planet() {
        s.earth_sim_enabled = false;
    }
    s
}

/// Emit a trace event (no-op when nothing records).
pub fn event(kind: &str, actor: Option<String>, data: Value) {
    theatre_trace::event(kind, actor.as_deref(), data);
}

/// The district a place lies in (itself for a district; the arena for the arena).
pub fn district_of(w: &World, p: PlaceId) -> PlaceId {
    let mut cur = p;
    for _ in 0..8 {
        let Some(pl) = w.places.get(cur.ix()) else {
            return w.meta.arena;
        };
        if pl.kind != PlaceKind::Site {
            return cur;
        }
        match pl.parent {
            Some(par) => cur = par,
            None => return cur,
        }
    }
    cur
}

pub fn in_district(w: &World, p: PlaceId, d: PlaceId) -> bool {
    district_of(w, p) == d
}

/// The hand agent an agent works under (itself for a hand).
pub fn hand_of(w: &World, a: AgentId) -> Option<AgentId> {
    let mut cur = a;
    for _ in 0..32 {
        let ag = w.agents.get(cur.ix())?;
        match ag.kind {
            AgentKind::Hand { .. } => return Some(cur),
            AgentKind::Subagent { parent } | AgentKind::Possession { parent, .. } => cur = parent,
            _ => return None,
        }
    }
    None
}

/// Live subagents and possessions under a hand.
pub fn live_children(w: &World, hand: AgentId) -> Vec<AgentId> {
    w.agents
        .iter()
        .filter(|a| {
            a.is_live() && a.parent().is_some() && hand_of(w, a.id) == Some(hand) && a.id != hand
        })
        .map(|a| a.id)
        .collect()
}

/// Everything that is "you" for a demigod's player: the demigod and its sibling (schema 5: the player's two demigods
/// know what either knows), their hands, every agent under them, and the bodies they steer. Sorted. One body's own
/// deeds use [`body_selves`].
pub fn selves(w: &World, d: DemigodId) -> Vec<Ref> {
    let mut v = vec![];
    for s in w.siblings(d) {
        v.extend(body_selves(w, s));
    }
    if v.is_empty() {
        v = body_selves(w, d);
    }
    v.sort_unstable();
    v.dedup();
    v
}

/// One demigod body's own "you": itself, its hand(s), every agent under them, and the bodies they steer. Sorted.
pub fn body_selves(w: &World, d: DemigodId) -> Vec<Ref> {
    let mut v = vec![Ref::Demigod(d)];
    for a in &w.agents {
        if fn7_society::demigod_of(w, Ref::Agent(a.id)) == Some(d) && a.parent().is_some()
            || matches!(a.kind, AgentKind::Hand { demigod, .. } if demigod == d)
        {
            v.push(Ref::Agent(a.id));
            if let AgentKind::Possession { body, .. } = a.kind {
                if a.is_live() {
                    v.push(Ref::Char(body));
                }
            }
        }
    }
    v.sort_unstable();
    v.dedup();
    v
}

/// Does demigod `d` know ledger row `r`? It (or one of its agents/bodies) did it, saw it, was told, was its target,
/// or its Following has heard it. Memory's erasure of the demigod wins.
pub fn knows(me: &[Ref], following: GroupId, r: &LedgerRow) -> bool {
    if me
        .iter()
        .any(|x| r.erased_from.contains(x) && matches!(x, Ref::Demigod(_)))
    {
        return false;
    }
    me.contains(&r.actor)
        || r.witnessed_by.iter().any(|x| me.contains(x))
        || r.told_to.iter().any(|x| me.contains(x))
        || r.targets.iter().any(|x| me.contains(x))
        || r.known_by.contains(following)
}

/// Open debt a ref owes (optionally to one god).
pub fn open_debt(w: &World, who: Ref, god: Option<GodId>) -> f32 {
    w.ledger
        .open_debts()
        .filter_map(|r| r.debt)
        .filter(|d| d.owed_by == who && god.is_none_or(|g| d.owed_to == g))
        .map(|d| d.amount)
        .sum()
}

pub fn mood_word(m: &Mood) -> &'static str {
    let xs = [
        (m.anger, "angry"),
        (m.fear, "afraid"),
        (m.hope, "hopeful"),
        (m.devotion, "devoted"),
    ];
    let mut best = xs[0];
    for x in xs {
        if x.0 > best.0 {
            best = x;
        }
    }
    if best.0 < 0.3 {
        "calm"
    } else {
        best.1
    }
}

/// An entity for a prompt (with condition). Seat payloads use [`entity_json`] instead (no rival condition).
pub fn entity(w: &World, r: Ref) -> Option<Entity> {
    Some(match r {
        Ref::Demigod(d) => {
            let x = w.demigods.get(d.ix())?;
            let cond = if x.alive {
                format!("demigod, health {:.0}/{:.0}", x.health, x.max_health)
            } else {
                "demigod, eliminated".into()
            };
            Entity::new(r, x.name.clone(), cond).at(x.pos, Some(x.place))
        }
        Ref::Char(c) => {
            let x = w.characters.get(c.ix())?;
            let mut b = x.traits.join(", ");
            if let Some(l) = x.life.as_deref() {
                b = format!("{}; {}", person_blurb(w, x, l), b);
            }
            if let Some(g) = x.serves {
                b = format!("paladin of {}, {b}", w.name_of(Ref::God(g)));
            }
            if !x.alive {
                b.push_str(", dead");
            } else if x.health < x.max_health {
                b.push_str(&format!(", health {:.0}", x.health));
            }
            Entity::new(r, x.name.clone(), b).at(x.pos, Some(x.place))
        }
        Ref::Cluster(c) => {
            let x = w.clusters.get(c.ix())?;
            let center = fn7_society::geo::pos_of(w, r).unwrap_or([0.0, 0.0]);
            let blurb = match &x.army {
                Some(a) => army_blurb(w, x, a),
                None => {
                    let lead = x
                        .leader
                        .filter(|l| w.characters[l.ix()].alive)
                        .map(|l| format!(", led by {}", w.characters[l.ix()].name))
                        .unwrap_or_default();
                    format!("~{} people, {}, {}{lead}", x.size, x.traits.join(", "), mood_word(&x.mood))
                }
            };
            Entity::new(r, x.label.clone(), blurb).at(center, Some(x.place))
        }
        Ref::Cell(c) => {
            let x = w.cells.get(c.ix())?;
            let lead = x
                .leader
                .filter(|l| w.characters[l.ix()].alive)
                .map(|l| format!(", led by {}", w.characters[l.ix()].name))
                .unwrap_or_default();
            Entity::new(
                r,
                x.name.clone(),
                format!("{:?} of ~{} out of the {}{lead}", x.kind, x.size, w.name_of(Ref::Cluster(x.crowd))).to_lowercase(),
            )
            .at(fn7_society::geo::pos_of(w, r).unwrap_or([0.0, 0.0]), fn7_society::place_of(w, r))
        }
        Ref::Mob(m) => {
            let x = w.mobs.get(m.ix())?;
            let lead = x
                .ringleader
                .filter(|l| w.characters[l.ix()].alive)
                .map(|l| format!(", ringleader {}", w.characters[l.ix()].name))
                .unwrap_or_default();
            Entity::new(
                r,
                x.name.clone(),
                format!("mob of ~{}, {:?}, {}{lead}", x.size, x.action, x.cause).to_lowercase(),
            )
            .at(x.pos, Some(x.place))
        }
        Ref::Place(p) => {
            let x = w.places.get(p.ix())?;
            Entity::new(
                r,
                x.name.clone(),
                format!("{}; {}", x.terrain, x.tags.join(", ")),
            )
            .at(x.center, Some(p))
        }
        Ref::Group(g) => {
            let x = w.groups.get(g.ix())?;
            Entity::new(
                r,
                x.name.clone(),
                format!("{:?} group", x.kind).to_lowercase(),
            )
        }
        Ref::Item(i) => {
            let x = w.items.get(i.ix())?;
            Entity::new(r, x.name.clone(), x.description.clone())
        }
        Ref::God(g) => Entity::new(
            r,
            w.name_of(r),
            format!(
                "god, {}",
                w.gods.get(g.ix()).map(|_| "watching").unwrap_or("")
            ),
        ),
        Ref::Agent(a) => {
            let x = w.agents.get(a.ix())?;
            Entity::new(r, x.name.clone(), x.goal.clone())
        }
        Ref::Ledger(_) => return None,
    })
}

/// Refs present in a district: alive demigods, alive characters, clusters, sites, and items held there.
pub fn present_in(w: &World, d: PlaceId) -> Vec<Ref> {
    let mut v: Vec<Ref> = vec![];
    for x in w.mobs.iter().filter(|m| m.ended.is_none() && in_district(w, m.place, d)) {
        v.push(Ref::Mob(x.id));
    }
    for x in w
        .demigods
        .iter()
        .filter(|x| x.alive && in_district(w, x.place, d))
    {
        v.push(Ref::Demigod(x.id));
    }
    for x in w
        .characters
        .iter()
        .filter(|x| x.alive && in_district(w, x.place, d))
    {
        v.push(Ref::Char(x.id));
    }
    for x in w
        .clusters
        .iter()
        .filter(|x| x.size > 0 && in_district(w, x.place, d))
    {
        v.push(Ref::Cluster(x.id));
    }
    for x in w
        .places
        .iter()
        .filter(|x| x.kind == PlaceKind::Site && x.parent == Some(d))
    {
        v.push(Ref::Place(x.id));
    }
    let holders: Vec<Ref> = v.clone();
    for it in &w.items {
        if it.holders.last().is_some_and(|h| holders.contains(h)) {
            v.push(Ref::Item(it.id));
        }
    }
    v
}

/// What demigod `d` sees at its zoom: always itself, the rival demigods (positions are public), the districts and the
/// gods; then the focus district (district zoom) or the focused person and their district (character zoom), or the
/// biggest crowds (world zoom). Its own items and live subagents' bodies. Order is deterministic.
pub fn visible(w: &World, d: DemigodId, zoom: Zoom, focus: Option<Ref>, cap: usize) -> Vec<Ref> {
    let me = &w.demigods[d.ix()];
    let mut v: Vec<Ref> = vec![Ref::Demigod(d)];
    for x in w.demigods.iter().filter(|x| x.alive && x.id != d) {
        v.push(Ref::Demigod(x.id));
    }
    for it in &me.items {
        v.push(Ref::Item(*it));
    }
    v.extend(society_people(w, d, me.place).into_iter().take(16));
    let focus_place = focus
        .and_then(|f| fn7_society::place_of(w, f))
        .unwrap_or(me.place);
    let district = district_of(w, focus_place);
    match zoom {
        Zoom::World => {
            for p in w.places.iter().filter(|p| p.kind == PlaceKind::District) {
                v.push(Ref::Place(p.id));
            }
            let mut cl: Vec<&Cluster> = w.clusters.iter().filter(|c| c.size > 0 && c.army.is_none()).collect();
            cl.sort_by(|a, b| b.size.cmp(&a.size).then(a.id.cmp(&b.id)));
            v.extend(cl.into_iter().take(10).map(|c| Ref::Cluster(c.id)));
            v.extend(fn7_society::armies(w).map(|(c, _)| Ref::Cluster(c.id)));
        }
        Zoom::City => {
            let city = match w.places.get(focus_place.ix()) {
                Some(p) if p.kind == PlaceKind::Site => Some(p.id),
                _ => w
                    .places
                    .iter()
                    .filter(|p| p.kind == PlaceKind::Site && p.parent == Some(district))
                    .min_by(|a, b| {
                        distance_km(a.center, me.pos)
                            .total_cmp(&distance_km(b.center, me.pos))
                            .then(a.id.cmp(&b.id))
                    })
                    .map(|p| p.id),
            };
            if let Some(f) = focus {
                v.push(f);
            }
            if let Some(c) = city {
                v.push(Ref::Place(c));
            }
            v.push(Ref::Place(district));
            let here = present_in(w, district);
            v.extend(here.iter().copied().filter(|r| {
                city.is_some()
                    && fn7_society::place_of(w, *r).and_then(|p| fn7_society::geo::city_of(w, p).or(Some(p))) == city
            }));
            v.extend(here.into_iter().filter(|r| matches!(r, Ref::Place(_))));
        }
        Zoom::District | Zoom::Character => {
            if let Some(f) = focus {
                v.push(f);
            }
            v.push(Ref::Place(district));
            v.extend(present_in(w, district));
            // Neighboring districts, so the hands can send people places.
            for p in w
                .places
                .iter()
                .filter(|p| p.kind == PlaceKind::District && p.id != district)
            {
                v.push(Ref::Place(p.id));
            }
        }
    }
    // Bodies steered by its agents.
    for a in w.agents.iter().filter(|a| a.is_live()) {
        if let AgentKind::Possession { body, .. } = a.kind {
            if fn7_society::demigod_of(w, Ref::Agent(a.id)) == Some(d) {
                v.push(Ref::Char(body));
            }
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    v.retain(|r| seen.insert(*r));
    v.truncate(cap);
    v
}

/// Entities near a point (alive demigods and characters within `km`), nearest first.
pub fn near(w: &World, at: LonLat, km: f64) -> Vec<Ref> {
    let mut v: Vec<(f64, Ref)> = vec![];
    for x in w.demigods.iter().filter(|x| x.alive) {
        v.push((distance_km(at, x.pos), Ref::Demigod(x.id)));
    }
    for x in w.characters.iter().filter(|x| x.alive) {
        v.push((distance_km(at, x.pos), Ref::Char(x.id)));
    }
    v.retain(|(d, _)| *d <= km);
    v.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    v.into_iter().map(|x| x.1).collect()
}

/// Seat payload for one entity: ref, name, kind (no condition for anything but yourself).
pub fn entity_json(w: &World, r: Ref) -> Value {
    let kind = match r {
        Ref::Demigod(_) => "demigod",
        Ref::Char(_) => "character",
        Ref::Cluster(_) => "cluster",
        Ref::Group(_) => "group",
        Ref::God(_) => "god",
        Ref::Place(_) => "place",
        Ref::Item(_) => "item",
        Ref::Agent(_) => "agent",
        Ref::Ledger(_) => "ledger",
        Ref::Cell(_) => "cell",
        Ref::Mob(_) => "mob",
    };
    let pos = fn7_society::geo::pos_of(w, r);
    let mut v = serde_json::json!({ "ref": r, "name": w.name_of(r), "kind": kind });
    if let Some(p) = pos {
        v["pos"] = serde_json::json!(p);
    }
    if let Ref::Cluster(c) = r {
        if let Some(x) = w.clusters.get(c.ix()) {
            v["size"] = x.size.into();
            v["mood"] = mood_word(&x.mood).into();
            if let Some(a) = &x.army {
                v["army_of"] = serde_json::json!(Ref::Demigod(a.demigod));
            }
        }
    }
    if let Ref::Char(c) = r {
        if let Some(x) = w.characters.get(c.ix()) {
            v["alive"] = x.alive.into();
            if let Some(l) = x.life.as_deref() {
                v["role"] = serde_json::json!(l.role);
                v["rank"] = serde_json::json!(l.rank);
            }
        }
    }
    if let Ref::Mob(m) = r {
        if let Some(x) = w.mobs.get(m.ix()) {
            v["size"] = x.size.into();
            v["action"] = serde_json::json!(x.action);
            v["ringleader"] = serde_json::json!(x.ringleader.map(Ref::Char));
        }
    }
    if let Ref::Cell(c) = r {
        if let Some(x) = w.cells.get(c.ix()) {
            v["size"] = x.size.into();
            v["cell_kind"] = serde_json::json!(x.kind);
            v["leader"] = serde_json::json!(x.leader.map(Ref::Char));
        }
    }
    v
}

/// A simulated person in one line for prompts: title, the numbers that drive them (competence, loyalty to their
/// master, ambition, cruelty or mercy, courage), grudges and goal. The story is about these people, so minds see them.
pub fn person_blurb(w: &World, x: &Character, l: &Life) -> String {
    let word = |v: f32, hi: &'static str, lo: &'static str| if v >= 0.65 { Some(hi) } else if v <= 0.3 { Some(lo) } else { None };
    let mut bits: Vec<String> = vec![if l.rank.title.is_empty() { format!("{:?}", l.role).to_lowercase() } else { l.rank.title.clone() }];
    bits.extend(word(l.competence, "competent", "inept").map(String::from));
    bits.extend(word(l.ambition, "ambitious", "content").map(String::from));
    bits.extend(word(l.courage, "brave", "timid").map(String::from));
    if l.cruelty >= 0.4 {
        bits.push("cruel".into());
    } else if l.cruelty <= -0.4 {
        bits.push("merciful".into());
    }
    if let Some((d, a)) = x.allegiance.iter().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()).then(b.0.cmp(&a.0))) {
        if a.abs() >= 0.2 {
            let n = w.demigods.get(d.ix()).map(|d| d.name.as_str()).unwrap_or("someone");
            bits.push(if *a > 0.0 { format!("loyal to {n} ({a:.1})") } else { format!("hates {n}") });
        }
    }
    if !l.grudges.is_empty() {
        let g: Vec<String> = l.grudges.iter().take(2).map(|r| w.name_of(*r)).collect();
        bits.push(format!("grudge against {}", g.join(", ")));
    }
    if !x.alive {
        bits.push("dead".into());
    }
    bits.join(", ")
}

/// An army in one line for prompts: size, its general and captains, morale, supply, what it is doing.
pub fn army_blurb(w: &World, c: &Cluster, a: &Army) -> String {
    let g = a
        .general
        .filter(|g| w.characters[g.ix()].alive)
        .map(|g| format!("General {}", w.characters[g.ix()].name))
        .unwrap_or_else(|| "no general".into());
    let caps = a.captains.iter().filter(|x| w.characters[x.ix()].alive).count();
    let master = if a.rogue {
        "rogue (answers to no demigod)".to_string()
    } else {
        format!("army of {}", w.demigods.get(a.demigod.ix()).map(|d| d.name.as_str()).unwrap_or("?"))
    };
    let target = a.target.map(|t| format!(", heading for {}", w.name_of(t))).unwrap_or_default();
    format!(
        "{master}, ~{} strong under {g} and {caps} captains, morale {:.1}, supply {:.1}, {:?}{target}",
        c.size, a.morale, a.supply, a.stance
    )
    .replace("Marching", "marching")
}

/// The society people a demigod's hands should be able to address: its generals and captains, the ringleaders,
/// prophets and heroes near it, rival generals within reach. Deterministic order.
pub fn society_people(w: &World, d: DemigodId, near_place: PlaceId) -> Vec<Ref> {
    let mut v = vec![];
    for (c, a) in fn7_society::armies(w).filter(|(_, a)| a.demigod == d && !a.rogue) {
        v.push(Ref::Cluster(c.id));
        v.extend(a.general.filter(|g| w.characters[g.ix()].alive).map(Ref::Char));
        v.extend(a.captains.iter().filter(|x| w.characters[x.ix()].alive).take(3).map(|x| Ref::Char(*x)));
    }
    let region = district_of(w, near_place);
    for m in w.mobs.iter().filter(|m| m.ended.is_none() && district_of(w, m.place) == region) {
        v.push(Ref::Mob(m.id));
        v.extend(m.ringleader.filter(|r| w.characters[r.ix()].alive).map(Ref::Char));
    }
    for x in w.characters.iter().filter(|x| x.alive && x.serves.is_none() && district_of(w, x.place) == region) {
        if x.life.as_ref().is_some_and(|l| matches!(l.role, Role::Prophet | Role::Hero | Role::Ringleader | Role::General)) {
            v.push(Ref::Char(x.id));
        }
    }
    let me = w.demigods[d.ix()].pos;
    for (c, a) in fn7_society::armies(w).filter(|(_, a)| a.demigod != d || a.rogue) {
        if distance_km(a.pos, me) < 3000.0 {
            v.push(Ref::Cluster(c.id));
            v.extend(a.general.filter(|g| w.characters[g.ix()].alive).map(Ref::Char));
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    v.retain(|r| seen.insert(*r));
    v
}

/// The seat state for `seat`, if any.
pub fn seat_state(w: &World, seat: u8) -> Option<&SeatState> {
    w.seats.iter().find(|s| s.seat == seat)
}

/// The seat state of the player a demigod belongs to (either of its two demigods).
pub fn seat_of(w: &World, d: DemigodId) -> Option<&SeatState> {
    let seat = w.demigods.get(d.ix())?.seat;
    seat_state(w, seat)
}

/// A demigod's hand agents without repeats (schema 5: its own hand; before: both of the player's hands).
pub fn hands_of(dg: &Demigod) -> Vec<AgentId> {
    if dg.hands[0] == dg.hands[1] {
        vec![dg.hands[0]]
    } else {
        dg.hands.to_vec()
    }
}

/// The hand agents a seat speaks through (both hands, Left then Right), with the demigod each belongs to.
pub fn seat_hands(w: &World, seat: u8) -> Vec<(AgentId, DemigodId)> {
    let mut v = vec![];
    for d in w.bodies_of_seat(seat) {
        for h in hands_of(d) {
            if !v.iter().any(|(x, _)| *x == h) {
                v.push((h, d.id));
            }
        }
    }
    v
}

pub fn words(text: &str) -> u32 {
    text.split_whitespace().count() as u32
}

//! The client-facing pieces every payload shares, built one way: omens `{god, god_ref, speech}`, subagents (one flat
//! object shape), hands, news (newest first, one line per ledger row, refs deduplicated), favor and item origins with
//! `Ref` ids, the god table, and [`tidy`], which rounds the floats of an outgoing payload.
//!
//! Everything here reads the state and builds JSON for the client; nothing writes the state, so the state (and a
//! replay of it) is untouched by how payloads look.

use std::collections::BTreeMap;

use fn7_agents::common::strip_speaker;
use fn7_model::*;
use serde_json::{json, Value};

use crate::world::live_children;

/// Round the floats of an outgoing payload: positions (`pos`, `center`) to 6 decimals, every other non-integer number
/// to 2, and `-0.0` to `0.0`. Integers are untouched. Deterministic (plain f64 arithmetic), and only ever applied to a
/// payload, never to the state.
pub fn tidy(v: &mut Value) {
    tidy_at(v, 2);
}

fn tidy_at(v: &mut Value, places: i32) {
    match v {
        Value::Number(n) if n.is_f64() => {
            if let Some(f) = n.as_f64() {
                *v = json!(round_to(f, places));
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| tidy_at(x, places)),
        Value::Object(o) => {
            for (k, x) in o.iter_mut() {
                let p = if matches!(k.as_str(), "pos" | "center") { 6 } else { 2 };
                tidy_at(x, p);
            }
        }
        _ => {}
    }
}

/// `x` rounded to `places` decimals; `-0.0` becomes `0.0`; non-finite values become 0.
pub fn round_to(x: f64, places: i32) -> f64 {
    if !x.is_finite() {
        return 0.0;
    }
    let k = 10f64.powi(places);
    let r = (x * k).round() / k;
    if r == 0.0 {
        0.0
    } else {
        r
    }
}

/// A ruling as an omen: the god's name and ref, and the speech without a leading speaker tag.
pub fn omen(w: &World, r: &Ruling) -> Omen {
    let god = w.name_of(Ref::God(r.god));
    let at = r.at.or_else(|| fn7_society::geo::pos_of(w, r.invoker));
    Omen {
        speech: strip_speaker(&r.speech, &god),
        god,
        god_ref: Ref::God(r.god),
        region: at.map(|p| region_name(w, p)),
        at,
    }
}

/// The region (planet) or district (city) name of a point.
pub fn region_name(w: &World, at: LonLat) -> String {
    w.name_of(Ref::Place(fn7_society::geo::region_at(w, at)))
}

/// A settlement this near (km, planet) names a point that lies outside it ("near Moscow").
pub const NEAR_KM: f64 = 300.0;

/// Schema 22 (clear words): a point in plain words, never coordinates: "in Moscow" (a settlement it lies in), "near
/// Moscow" (one within [`NEAR_KM`]), "in Eastern Europe" (the region), "at sea off Eastern Europe" (no country under
/// it); in a city arena, "in <the most specific place>". Returns (the phrase, the bare name).
pub fn where_words(w: &World, at: LonLat) -> (String, String) {
    use fn7_society::geo::{country_at, distance_km, place_at};
    let here = place_at(w, at);
    let name = w.name_of(Ref::Place(here));
    if !w.is_planet() || w.places.get(here.ix()).is_some_and(|p| p.kind == PlaceKind::Site) {
        return (format!("in {name}"), name);
    }
    let near = w
        .places
        .iter()
        .filter(|p| p.kind == PlaceKind::Site)
        .map(|p| (distance_km(p.center, at), p))
        .filter(|(d, _)| *d <= NEAR_KM)
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
    if let Some((_, p)) = near {
        return (format!("near {}", p.name), p.name.clone());
    }
    let region = region_name(w, at);
    if country_at(at).is_none() {
        return (format!("at sea off {region}"), region);
    }
    (format!("in {region}"), region)
}

/// A ledger row's geotag: where it happened and the region (planet) or district (city) it was in.
pub fn geotag(w: &World, r: &LedgerRow) -> (Option<LonLat>, Option<String>) {
    let (at, region) = geotag_place(w, r);
    (at, region.map(|p| w.name_of(Ref::Place(p))))
}

/// [`geotag`] with the region as a place id.
pub fn geotag_place(w: &World, r: &LedgerRow) -> (Option<LonLat>, Option<PlaceId>) {
    let at = r
        .at
        .or_else(|| r.place.and_then(|p| w.places.get(p.ix()).map(|x| x.center)));
    let region = match (r.place, at) {
        (Some(p), _) if w.places.get(p.ix()).is_some_and(|x| x.kind != PlaceKind::Arena) => {
            Some(fn7_society::geo::district_of(w, p))
        }
        (_, Some(a)) => Some(fn7_society::geo::region_at(w, a)),
        _ => None,
    };
    (at, region)
}

/// The Entropy meter: value (0..max), level (0..4) and its key, the next threshold, heat death; `delta` = change this
/// beat when given.
pub fn entropy_json(w: &World, delta: Option<f32>) -> Value {
    let e = w.meta.entropy;
    let l = fn7_society::entropy_level(e);
    let next = fn7_society::ENTROPY_LEVELS
        .get(l as usize + 1)
        .map(|x| x.0)
        .unwrap_or(ENTROPY_MAX);
    let mut v = json!({
        "value": e, "max": ENTROPY_MAX, "level": l, "key": fn7_society::entropy_key(l),
        "text": fn7_society::ENTROPY_LEVELS[l as usize].2, "next_at": next, "heat_death": w.meta.heat_death,
    });
    if let Some(d) = delta {
        v["delta"] = d.into();
    }
    v
}

/// Verbs that are public world events (on everyone's globe), whoever caused them.
const PUBLIC_VERBS: &[&str] = &[
    "cast", "happen", "raise_army", "march", "eliminate", "revive", "wound", "mod", "unmod",
];

/// Is this row a public world event? Casts and their ramifications, armies raised, marching and fighting, freak
/// weather, world mods, gods wounded, eliminations and revivals.
pub fn is_public_event(w: &World, r: &LedgerRow) -> bool {
    is_public_head(w, &r.verb, r.magnitude, &r.source, r.actor)
}

/// [`is_public_event`] on the fields it reads (so a query can test a row without deserializing all of it).
pub fn is_public_head(w: &World, verb: &str, magnitude: u8, source: &Source, actor: Ref) -> bool {
    if SocKind::of(verb).is_some() {
        return magnitude >= 4;
    }
    if PUBLIC_VERBS.contains(&verb) {
        return true;
    }
    // Rules 0.7.0: a manifestation is public from relic tier up (its row's magnitude is 4 + tier).
    if verb == "manifest" {
        return magnitude >= 6;
    }
    let entropy = w.gods.iter().find(|g| g.name == "Entropy").map(|g| g.id);
    match *source {
        Source::God { god, .. } if Some(god) == entropy => true,
        Source::Rules => {
            magnitude >= 4
                && match actor {
                    Ref::Cluster(c) => w.clusters.get(c.ix()).is_some_and(|c| c.army.is_some()),
                    Ref::Place(_) => true,
                    Ref::God(g) => Some(g) == entropy,
                    _ => false,
                }
        }
        _ => false,
    }
}

/// The public, geotagged events of `rows` (in order): `{row, kind, text, at, region, refs, magnitude, image_prompt,
/// image_kind}`. Every public event carries its image prompt (a candidate moment card); the media server attaches an
/// `image` only to those the style config's `event_selection` picks. At most 40.
pub fn events(w: &World, rows: &[LedgerRow]) -> Vec<Value> {
    rows.iter()
        .filter(|r| is_public_event(w, r))
        .take(40)
        .map(|r| event_json(w, r))
        .collect()
}

/// One public event: `{row, kind, text, at, region, refs, magnitude, image_prompt, image_kind}`.
pub fn event_json(w: &World, r: &LedgerRow) -> Value {
    let (at, region) = geotag(w, r);
    let mut refs = vec![r.actor];
    refs.extend(r.targets.iter().copied().filter(|t| !matches!(t, Ref::Ledger(_))));
    let mut seen = std::collections::BTreeSet::new();
    refs.retain(|x| seen.insert(*x));
    json!({ "row": Ref::Ledger(r.id), "kind": r.verb, "text": r.act, "at": at,
            "region": region, "refs": refs, "magnitude": r.magnitude,
            "image_prompt": event_subject(w, r, region.as_deref()), "image_kind": "event" })
}

/// An event image's subject: what happened, where, and who is in it with their visual sheets (so a character looks the
/// same in every image). The subject only (the media server adds framing and house style), never an engine id. Fixed
/// for the row (rows never change), so every payload and query that shows the row asks for the same image.
pub fn event_subject(w: &World, r: &LedgerRow, region: Option<&str>) -> String {
    let place = r
        .place
        .map(|p| w.name_of(Ref::Place(p)))
        .filter(|n| Some(n.as_str()) != region && n != "Earth");
    let wh = match (place, region) {
        (Some(p), Some(g)) => format!("{p}, {g}"),
        (Some(p), None) => p,
        (None, Some(g)) => g.to_string(),
        _ => "somewhere on Earth".into(),
    };
    let mut s = format!("{}, in {wh}", r.act.trim().trim_end_matches('.'));
    let mut cast: Vec<Ref> = vec![r.actor];
    cast.extend(r.targets.iter().copied());
    let mut seen = std::collections::BTreeSet::new();
    let starring: Vec<String> = cast
        .into_iter()
        .filter(|x| matches!(x, Ref::Demigod(_) | Ref::Char(_)) && seen.insert(*x))
        .filter_map(|x| crate::visual::of(w, x).map(|v| format!("{} ({})", w.name_of(x), v.brief())))
        .take(2)
        .collect();
    if !starring.is_empty() {
        s.push_str(&format!("; starring {}", starring.join("; and ")));
    }
    s
}

/// A demigod's portrait prompt: its name and visual sheet plus the demigod pose, stable for the whole match (no
/// health, place or state words), so it is one image per demigod.
pub fn portrait_prompt(w: &World, d: &Demigod) -> String {
    portrait_prompt_in(w, &d.name)
}

/// [`portrait_prompt_for`] in a schema-7 state; before, the name and sheet as they were (a `" (n)"` suffix kept).
pub fn portrait_prompt_in(w: &World, name: &str) -> String {
    if w.challenge_rules() {
        portrait_prompt_for(name)
    } else {
        portrait_prompt_with(name, &crate::visual::demigod_by_name_unstripped(name))
    }
}

/// A demigod's portrait prompt from its name alone (identity only: the name and its name-keyed visual sheet), so the
/// same demigod is the same image in every match.
pub fn portrait_prompt_for(name: &str) -> String {
    portrait_prompt_with(crate::visual::image_name(name), &crate::visual::demigod_by_name(name))
}

fn portrait_prompt_with(name: &str, v: &crate::visual::Visual) -> String {
    format!(
        "{}, a demigod: {}; a larger-than-life figure with two glowing orchestrator hands floating at their sides and \
         lightning crackling between their fingers",
        name,
        v.brief()
    )
}

/// A demigod's diorama prompt: the demigod as a tiny geometric miniature on a single hex diorama tile with its
/// signature prop, in its colors. Identity only (the name and its visual sheet; no home region, beat or health), so it
/// is one image per demigod across every match.
pub fn diorama_prompt(w: &World, d: &Demigod) -> String {
    diorama_prompt_in(w, &d.name)
}

/// [`diorama_prompt_for`] in a schema-7 state; before, the name and sheet as they were.
pub fn diorama_prompt_in(w: &World, name: &str) -> String {
    if w.challenge_rules() {
        diorama_prompt_for(name)
    } else {
        diorama_prompt_with(name, &crate::visual::demigod_by_name_unstripped(name))
    }
}

/// A hand's name as setup gives it (`"<demigod>'s Left Hand"` / `"<demigod>'s Right Hand"`), for prewarming.
pub fn hand_names(demigod: &str) -> [String; 2] {
    [format!("{demigod}'s Left Hand"), format!("{demigod}'s Right Hand")]
}

/// [`diorama_prompt`] from the name alone.
pub fn diorama_prompt_for(name: &str) -> String {
    diorama_prompt_with(crate::visual::image_name(name), &crate::visual::demigod_by_name(name))
}

fn diorama_prompt_with(name: &str, v: &crate::visual::Visual) -> String {
    format!(
        "{}, a demigod: {}; as a tiny geometric miniature figure standing on a single raised hexagonal diorama tile, its \
         signature prop, {}, built into the scene at its feet, trimmed in {} and {}",
        name,
        v.brief(),
        v.traits.prop,
        v.traits.colors[0],
        v.traits.colors[1],
    )
}

/// A named person's portrait prompt: name and visual sheet (stable all match).
pub fn person_portrait_prompt(w: &World, c: &Character) -> String {
    format!(
        "{}, a mortal caught up in a war of demigods: {}",
        c.name,
        crate::visual::person(w, c).brief()
    )
}

/// A place's image prompt (`game.media` kind `place`): its name, what it is and its terrain (all fixed at setup).
pub fn place_prompt(w: &World, p: &Place) -> String {
    let what = match p.kind {
        PlaceKind::Arena if w.is_planet() => "the whole planet Earth as a battle royale arena".to_string(),
        PlaceKind::Arena => format!("the city of {} as a battle royale arena", p.name),
        PlaceKind::District if w.is_planet() => format!("the region {}", p.name),
        PlaceKind::District => format!("the district {}", p.name),
        _ => p.name.clone(),
    };
    let parent = p
        .parent
        .and_then(|x| w.places.get(x.ix()))
        .filter(|x| x.kind != PlaceKind::Arena)
        .map(|x| format!(" in {}", x.name))
        .unwrap_or_default();
    let terrain = p.terrain.trim();
    if terrain.is_empty() {
        format!("{what}{parent}, under a sky crackling with divine lightning")
    } else {
        format!("{what}{parent} ({terrain}), under a sky crackling with divine lightning")
    }
}

/// An army's image prompt (`game.media` kind `army`): its name and the banners of the demigod it was raised for (the
/// demigod's sheet colors and emblem; fixed for the army).
pub fn army_prompt(w: &World, c: &Cluster, a: &Army) -> String {
    match w.demigods.get(a.demigod.ix()) {
        Some(d) => format!(
            "the army \"{}\" raised for the demigod {}, a marching host with {}",
            c.label,
            d.name,
            crate::visual::demigod(w, d).banners()
        ),
        None => format!("the army \"{}\", a marching host under ragged banners", c.label),
    }
}

/// Each demigod's share of each region: `[{region, region_ref, demigod, demigod_ref, share}]`.
pub fn influence_json(w: &World) -> Vec<Value> {
    fn7_society::influence(w)
        .into_iter()
        .map(|(r, d, s)| {
            json!({ "region": w.name_of(Ref::Place(r)), "region_ref": Ref::Place(r),
                    "demigod": w.demigods[d.ix()].name, "demigod_ref": Ref::Demigod(d), "share": s })
        })
        .collect()
}

/// The regions (planet) or districts (city): `[{ref, name, center, radius_km, cities: [{ref, name, center}]}]`.
pub fn regions_json(w: &World) -> Vec<Value> {
    w.places
        .iter()
        .filter(|p| p.kind == PlaceKind::District)
        .map(|p| {
            let cities: Vec<Value> = w
                .places
                .iter()
                .filter(|c| c.kind == PlaceKind::Site && c.parent == Some(p.id))
                .map(|c| json!({ "ref": Ref::Place(c.id), "name": c.name, "center": c.center }))
                .collect();
            json!({ "ref": Ref::Place(p.id), "name": p.name, "center": p.center, "radius_km": p.radius_km,
                    "cities": cities })
        })
        .collect()
}

/// Live armies: `[{ref, name, demigod, demigod_ref, size, pos, region, dest, target}]` (+ `orders` when `owner` is
/// its demigod).
pub fn armies_json(w: &World, owner: Option<DemigodId>) -> Vec<Value> {
    fn7_society::armies(w)
        .filter(|(_, a)| owner.is_none_or(|o| o == a.demigod))
        .map(|(c, a)| {
            let mut v = json!({ "ref": Ref::Cluster(c.id), "name": c.label, "demigod": w.demigods[a.demigod.ix()].name,
                "demigod_ref": Ref::Demigod(a.demigod), "size": c.size, "pos": a.pos,
                "region": region_name(w, a.pos), "dest": fn7_society::army_dest(w, a), "target": a.target,
                "rogue": a.rogue, "stance": a.stance,
                "general": a.general.filter(|g| w.characters[g.ix()].alive).map(|g| json!({
                    "ref": Ref::Char(g), "name": w.characters[g.ix()].name })),
                "chain": crate::society::chain_json(w, a, owner == Some(a.demigod)) });
            if owner.is_some() {
                v["orders"] = a.orders.clone().into();
            }
            v
        })
        .collect()
}

/// Live world mods: `[{id, name, description, at, radius_km, region, region_ref, tags, stacks, since_beat,
/// expires_beat, power, by}]`.
pub fn mods_json(w: &World) -> Vec<Value> {
    w.mods
        .iter()
        .map(|m| {
            json!({ "id": m.id, "name": m.name, "description": m.description, "at": m.at, "radius_km": m.radius_km,
                    "region": w.name_of(Ref::Place(m.region)), "region_ref": Ref::Place(m.region), "tags": m.tags,
                    "stacks": m.stacks, "since_beat": m.since_beat, "expires_beat": m.expires_beat,
                    "power": m.power, "by": m.by.map(Ref::Demigod) })
        })
        .collect()
}

/// The power palette: `[{slug, name, icon, cost, radius_km, target, magnitude, damage, tags}]` (+ the custom entry).
pub fn powers_json() -> Vec<Value> {
    let mut v: Vec<Value> = crate::powers()
        .powers
        .iter()
        .map(|p| {
            let var = |s: CastScale| {
                let v = p.variant(s);
                // cost: 1 bolt (mega) or 1 spark (mini).
                let _ = s;
                json!({ "radius_km": v.radius_km, "magnitude": v.magnitude, "damage": v.damage, "cost": 1 })
            };
            json!({ "slug": p.slug, "name": p.name, "icon": p.icon, "cost": p.cost, "radius_km": p.radius_km,
                    "target": p.target, "magnitude": p.magnitude, "damage": p.damage, "tags": p.tags,
                    "mega": var(CastScale::Mega), "mini": var(CastScale::Mini) })
        })
        .collect();
    v.push(json!({ "slug": "custom", "name": "Custom power", "icon": "custom", "cost": null, "radius_km": 200.0,
                   "target": "point", "magnitude": null, "damage": null, "tags": ["custom"] }));
    v
}

/// Per-player aura colours (rules 0.7.0), by player index (the order of each player's first demigod; stable for the
/// match). Chosen to stay distinct for the common colour-vision deficiencies (Okabe-Ito based, brightened) and readable
/// on the table's dark cobalt and violet; no cyan (the AI flicker's) and no grey (the fallen's).
pub const AURAS: [&str; 8] = ["#E69F00", "#F0E442", "#009E73", "#F28CC0", "#D55E00", "#A6D854", "#FFFFFF", "#C9A27E"];

/// A seat's aura colour (both of its demigods share it).
pub fn aura(w: &World, seat: u8) -> &'static str {
    let i = w.player_seats().iter().position(|s| *s == seat).unwrap_or(0);
    AURAS[i % AURAS.len()]
}

/// Time's bill for a slow window opened now by demigod `d`'s player (rules 0.7.0, the live bill): `debt(t) =
/// min(debt_cap, debt_base + debt_per_second × t)` after `t` seconds open, plus one bolt; `debt_base = 0.5 ×
/// repeat_factor` (`1 + 0.5 × slows in the last 20 beats`), doubled when the purse has no bolt.
pub fn time_bill(w: &World, d: DemigodId) -> Value {
    let x = &w.demigods[d.ix()];
    let recent = x.slows.iter().filter(|b| **b + crate::flow::SLOW_REPEAT_BEATS > w.meta.beat.max(1)).count() as u32;
    let repeat = 1.0 + 0.5 * recent as f32;
    let pay = x.bolts >= 1;
    let base = crate::flow::SLOW_DEBT_BASE * repeat * if pay { 1.0 } else { 2.0 };
    json!({ "bolts": 1, "can_pay_bolt": pay, "recent_slows": recent, "repeat_factor": repeat,
            "debt_base": base, "debt_per_second": base / 10.0, "debt_per_10s": base, "debt_cap": 10.0,
            "instant_order_debt": crate::world::settings(w).instant_order_debt * if pay { 1.0 } else { 2.0 } })
}

/// The seat list (rules 0.7.0): one entry per player, `{seat, player, aura, controller, connected, alive, bolts,
/// demigods: [ref], time_bill}` (the built-in AI's `seat` is null; `index` is the player's stable index).
pub fn players_json(w: &World) -> Vec<Value> {
    w.player_seats()
        .into_iter()
        .enumerate()
        .filter_map(|(i, seat)| {
            let first = w.demigod_of_seat(seat)?;
            let lead = w.lead_of_seat(seat).unwrap_or(first.id);
            let mut v = json!({
                "index": i, "seat": (seat < crate::SYNTHETIC_SEAT).then_some(seat), "player": first.player_name(),
                "aura": aura(w, seat), "alive": w.seat_alive(seat), "bolts": first.bolts, "sparks": first.sparks,
                "charges": { "bolts": first.bolts, "sparks": first.sparks },
                "demigods": w.bodies_of_seat(seat).map(|d| Ref::Demigod(d.id)).collect::<Vec<_>>(),
                "time_bill": time_bill(w, lead),
            });
            crate::table::add_table_fields(w, first, &mut v);
            Some(v)
        })
        .collect()
}

/// The open slow-time window as of the last resolve, publicly: `{by, by_ref, window_id, opened_ms, deadline_ms,
/// opened_beat}` or null. Live window state comes from the Theatre (`window.opened` / `window.closed`,
/// `match.snapshot.window`); this is the game's record of it.
pub fn slow_json(w: &World) -> Value {
    match &w.slow {
        Some(s) => json!({ "by": w.demigods[s.initiator.ix()].name, "by_ref": Ref::Demigod(s.initiator),
                           "window_id": s.window_id, "opened_ms": s.opened_ms, "deadline_ms": s.deadline_ms,
                           "opened_beat": s.opened_beat }),
        None => Value::Null,
    }
}

/// The flat client form of a live subagent, possession or paladin. `line` is what it said this beat when given,
/// else its goal.
pub fn subagent_line(w: &World, a: AgentId, said: Option<&String>) -> SubagentLine {
    let x = &w.agents[a.ix()];
    let (kind, parent, body) = match x.kind {
        AgentKind::Possession { parent, body } => (
            SubagentKind::Possession,
            Some(Ref::Agent(parent)),
            Some(Ref::Char(body)),
        ),
        AgentKind::Paladin { god, body } => (
            SubagentKind::Paladin,
            w.gods.get(god.ix()).map(|g| Ref::Agent(g.agent)),
            Some(Ref::Char(body)),
        ),
        AgentKind::Subagent { parent } => (SubagentKind::Subagent, Some(Ref::Agent(parent)), None),
        _ => (SubagentKind::Subagent, None, None),
    };
    SubagentLine {
        agent: Ref::Agent(a),
        name: x.name.clone(),
        goal: x.goal.clone(),
        line: said
            .filter(|s| !s.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| x.goal.clone()),
        beats_left: x
            .budget_beats
            .map(|b| (x.spawned_beat + b).saturating_sub(w.meta.beat)),
        kind,
        parent,
        body,
    }
}

/// A hand's live subagents, in the flat form.
pub fn subagents(w: &World, hand: AgentId, said: &BTreeMap<AgentId, String>) -> Vec<SubagentLine> {
    live_children(w, hand)
        .into_iter()
        .map(|a| subagent_line(w, a, said.get(&a)))
        .collect()
}

/// The seat view's hand.
pub fn seat_hand(w: &World, h: AgentId) -> SeatHand {
    let a = &w.agents[h.ix()];
    let mut h = SeatHand {
        agent: Ref::Agent(h),
        name: a.name.clone(),
        live: a.is_live(),
        subagents: subagents(w, h, &BTreeMap::new()),
        portrait_prompt: portrait_prompt_in(w, &a.name),
        diorama_prompt: diorama_prompt_in(w, &a.name),
        visual: Some(crate::visual::demigod_by_name_in(w, &a.name).to_json()),
        demigod: None,
        side: None,
        health: None,
        alive: None,
    };
    // Rules 0.7.0: the hand is a demigod in its own right.
    if let AgentKind::Hand { demigod, hand } = a.kind {
        if let Some(d) = w.demigods.get(demigod.ix()).filter(|d| d.side.is_some()) {
            h.demigod = Some(Ref::Demigod(d.id));
            h.side = Some(hand);
            h.health = Some(d.health);
            h.alive = Some(d.alive);
        }
    }
    h
}

/// News lines as a client gets them: newest first (by row), at most one line per ledger row, refs without repeats.
pub fn news_lines(mut v: Vec<NewsLine>) -> Vec<NewsLine> {
    v.sort_by_key(|n| std::cmp::Reverse(n.row));
    v.dedup_by(|a, b| a.row == b.row);
    for n in v.iter_mut() {
        let mut seen: Vec<Ref> = Vec::with_capacity(n.refs.len());
        n.refs.retain(|r| {
            if seen.contains(r) {
                false
            } else {
                seen.push(*r);
                true
            }
        });
    }
    v
}

/// Every god's favor toward `d`, as `{god, god_ref, favor}`.
pub fn favor(w: &World, d: DemigodId) -> Vec<FavorLine> {
    w.gods
        .iter()
        .map(|g| FavorLine {
            god: w.name_of(Ref::God(g.id)),
            god_ref: Ref::God(g.id),
            favor: g.favor.get(d.ix()).copied().unwrap_or(0.0),
        })
        .collect()
}

/// An item's origin as `(origin, origin_ref)`: `"echoed"` with the source item, `"granted"` with the god.
pub fn item_origin(o: &ItemOrigin) -> (&'static str, Option<Ref>) {
    match *o {
        ItemOrigin::Starting => ("starting", None),
        ItemOrigin::Summoned => ("summoned", None),
        ItemOrigin::Found => ("found", None),
        ItemOrigin::Echoed { from } => ("echoed", Some(Ref::Item(from))),
        ItemOrigin::Granted { god } => ("granted", Some(Ref::God(god))),
    }
}

/// The god table: `[{ref, name, slug, domain}]` in id order.
pub fn gods(pantheon: &Pantheon) -> Vec<Value> {
    pantheon
        .gods
        .iter()
        .enumerate()
        .map(|(i, g)| {
            json!({ "ref": Ref::God(GodId::from_ix(i)), "name": g.name, "slug": g.slug, "domain": g.domain })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidy_rounds_floats_but_not_positions_or_integers() {
        let mut v = json!({
            "health": 3.3333333f32 as f64, "debt": 0.05f32 as f64, "zero": -0.0, "n": 7, "big": 1000001.0,
            "pos": [106.8274921f64, -6.1724719f64], "arena": { "center": [1.23456789, 2.0], "radius_km": 12.0712345 },
            "favor": [{ "favor": 0.03999999910593033 }],
        });
        tidy(&mut v);
        assert_eq!(
            v,
            json!({
                "health": 3.33, "debt": 0.05, "zero": 0.0, "n": 7, "big": 1000001.0,
                "pos": [106.827492, -6.172472], "arena": { "center": [1.234568, 2.0], "radius_km": 12.07 },
                "favor": [{ "favor": 0.04 }],
            })
        );
        assert_eq!(serde_json::to_string(&v["zero"]).unwrap(), "0.0");
    }

    #[test]
    fn news_is_newest_first_one_line_per_row_and_refs_are_unique() {
        let n = |row: u32, refs: Vec<Ref>| NewsLine {
            row: Ref::Ledger(LedgerId(row)),
            text: format!("row {row}"),
            refs,
            how: "you saw it".into(),
            at: None,
            region: None,
        };
        let g = Ref::God(GodId(2));
        let d = Ref::Demigod(DemigodId(1));
        let out = news_lines(vec![n(3, vec![g, d, g]), n(5, vec![d]), n(3, vec![g])]);
        assert_eq!(out.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(), ["row 5", "row 3"]);
        assert_eq!(out[1].refs, vec![g, d]);
    }
}

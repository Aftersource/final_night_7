//! Casts: god powers thrown from the UX at any point of the globe (`fields.cast` / `casts`; `bolt` / `bolts` are
//! lightning). Separate from text entry: a cast never carries orders and never slows time.
//!
//! Clients send casts as Theatre instant actions (`action.submit { instant: true, fields: { cast | casts | bolt |
//! bolts } }`, any phase). `validate_action` checks them on arrival against the last committed state (known powers,
//! enough bolts for the action's total); [`streak`] is the public `phase.event` everyone sees at once (`on_instant`).
//! Nothing is spent then (hooks are read-only): each cast is settled at the next Resolve in arrival order (the plugin's
//! AI first, then by the core's `seq`, then demigod; a cast its caster can no longer pay for fizzles): it costs its
//! price in bolts, every other live demigod gains 2 per bolt, Entropy rises, and it strikes its point at once (rules).
//! Every cast gets Entropy's check, which always says yes: one ramifications call per region with casts this beat
//! ("N ramifications in <region> with <context>", N up to 10) runs in the act batch and becomes geotagged rows, news
//! and persistent world mods.

use std::collections::BTreeMap;
use std::sync::Arc;

use fn7_agents::ramify::{ramifications_for, CastLine, RamifyCtx};
use fn7_agents::Entity;
use fn7_model::seed::{hash_str, Rng};
use fn7_model::*;
use fn7_society::geo::{distance_km, place_at, pos_of, region_at};
use serde_json::{json, Value};

use crate::actions::{CastOrder, CastTarget};
use crate::beat::Turn;
use crate::merge;
use crate::settings::Settings;
use crate::world::{entity, event, present_in};
use crate::SYNTHETIC_SEAT;

/// What this beat's casts produced.
#[derive(Debug, Default)]
pub struct CastOut {
    /// One ramifications call per region (ruling ids are set by the beat).
    pub ramify: Vec<RamifyCtx>,
    /// The intent of the first cast in each ramify call (the Entropy ruling's intent).
    pub ramify_intents: Vec<(IntentId, DemigodId, LonLat)>,
    /// Public lines: every cast this beat, landed or fizzled.
    pub lines: Vec<Value>,
}

/// A cast's price in bolts.
pub fn cost_of(o: &CastOrder) -> u32 {
    power_of(o, crate::powers()).cost
}

/// A resolved power: the palette entry, or a custom one priced by magnitude.
fn power_of(o: &CastOrder, powers: &fn7_pantheon::Powers) -> PowerDef {
    if let Some(p) = powers.get(&o.power) {
        return p.clone();
    }
    let mag = o.magnitude.unwrap_or(5).min(10);
    let text = o.text.clone().unwrap_or_default();
    PowerDef {
        slug: "custom".into(),
        name: text.chars().take(40).collect::<String>().trim().to_string(),
        icon: "custom".into(),
        cost: fn7_pantheon::custom_cost(mag),
        radius_km: 200.0,
        target: PowerTarget::Point,
        magnitude: mag,
        damage: (mag as f32 * 0.3 * 10.0).round() / 10.0,
        tags: vec!["custom".into()],
        blurb: text,
    }
}

/// A name → a ref: demigods, places, crowds and people by exact name (case-insensitive), then places and demigods by
/// shared words.
pub fn resolve_name(w: &World, s: &str) -> Option<Ref> {
    let n = s.trim();
    let eq = |x: &str| x.eq_ignore_ascii_case(n);
    if let Some(d) = w.demigods.iter().find(|d| eq(&d.name)) {
        return Some(Ref::Demigod(d.id));
    }
    if let Some(p) = w.places.iter().find(|p| eq(&p.name) && p.kind != PlaceKind::Arena) {
        return Some(Ref::Place(p.id));
    }
    if let Some(c) = w.clusters.iter().find(|c| eq(&c.label) && c.size > 0) {
        return Some(Ref::Cluster(c.id));
    }
    if let Some(c) = w.characters.iter().find(|c| eq(&c.name) && c.alive) {
        return Some(Ref::Char(c.id));
    }
    let want = fn7_society::tokens(n);
    if want.is_empty() {
        return None;
    }
    let score = |name: &str| {
        let t = fn7_society::tokens(name);
        want.iter().filter(|x| t.contains(x)).count()
    };
    w.places
        .iter()
        .filter(|p| p.kind != PlaceKind::Arena)
        .map(|p| (score(&p.name), Ref::Place(p.id)))
        .chain(w.demigods.iter().map(|d| (score(&d.name), Ref::Demigod(d.id))))
        .filter(|(s, _)| *s > 0)
        .max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)))
        .map(|x| x.1)
}

/// A cast's aim → (strike point, the thing it hits directly).
fn aim(w: &World, t: &CastTarget, power: &PowerDef) -> Option<(LonLat, Option<Ref>)> {
    let r = match t {
        CastTarget::At(p) => return Some((*p, None)),
        CastTarget::Ref(r) => *r,
        CastTarget::Name(n) => resolve_name(w, n)?,
    };
    if !fn7_society::exists(w, r) {
        return None;
    }
    let at = pos_of(w, r)?;
    let hits = match r {
        Ref::Demigod(_) | Ref::Char(_) | Ref::Cluster(_) => fn7_society::is_alive(w, r).then_some(r),
        _ => None,
    };
    let _ = power;
    Some((at, hits))
}

/// The public streak of one instant action's casts (`on_instant`, shown to everyone at once): who, and per cast the
/// power, its icon key, the aim (`target`, `at` [lon, lat], region). Read-only: nothing is paid until Resolve, and at
/// Entropy 40+ a cast may still land off target there.
pub fn streak(w: &World, d: DemigodId, seat: u8, casts: &[CastOrder]) -> Option<Value> {
    let powers = crate::powers();
    let name = w.demigods[d.ix()].name.clone();
    let lines: Vec<Value> = casts
        .iter()
        .map(|o| {
            let p = power_of(o, powers);
            let mut v = json!({ "power": p.slug, "power_name": p.name, "icon": p.icon, "cost": p.cost,
                                "radius_km": p.radius_km });
            if let Some(t) = o.text.as_ref().filter(|_| p.slug == "custom") {
                v["text"] = t.clone().into();
            }
            match aim(w, &o.target, &p) {
                Some((at, target)) => {
                    let (region, rname) = region_name(w, at);
                    v["at"] = json!(at);
                    v["region"] = rname.into();
                    v["region_ref"] = json!(Ref::Place(region));
                    v["place"] = w.name_of(Ref::Place(place_at(w, at))).into();
                    v["target"] = json!(target);
                    v["target_name"] = json!(target.map(|r| w.name_of(r)));
                }
                None => v["fizzled"] = "no such target".into(),
            }
            v
        })
        .collect();
    let first = lines.first()?.clone();
    let mut out = json!({ "event": "streak", "by": name, "by_ref": Ref::Demigod(d), "by_seat": seat,
                          "casts": lines });
    for k in ["power", "power_name", "icon", "cost", "at", "region", "region_ref", "place", "target", "target_name"] {
        if let Some(x) = first.get(k) {
            out[k] = x.clone();
        }
    }
    Some(out)
}

fn region_name(w: &World, p: LonLat) -> (PlaceId, String) {
    let r = region_at(w, p);
    (r, w.name_of(Ref::Place(r)))
}

/// Settle this beat's casts (spend, gifts, Entropy, the strike), record them, and prepare the ramifications calls.
pub fn process(
    w: &mut World,
    beat: u32,
    turns: &[Turn],
    pantheon: &Arc<Pantheon>,
    st: &Settings,
) -> CastOut {
    let powers = crate::powers();
    let entropy = pantheon.id_of("entropy");
    let mut out = CastOut::default();
    // Arrival order: the plugin's AI first, then the core's `seq`, then demigod, then position in the action.
    let mut order: Vec<(&Turn, &CastOrder)> = turns
        .iter()
        .flat_map(|t| t.input.casts.iter().map(move |o| (t, o)))
        .collect();
    order.sort_by_key(|(t, o)| (t.seat < SYNTHETIC_SEAT, o.seq, t.demigod.0));
    let mut by_region: BTreeMap<PlaceId, Vec<(CastLine, IntentId)>> = BTreeMap::new();
    let mut k = 0u64;
    {
        for (t, o) in order {
            k += 1;
            let d = t.demigod;
            let p = power_of(o, powers);
            let name = w.demigods[d.ix()].name.clone();
            let have = w.demigods[d.ix()].bolts;
            let base = json!({ "by": name, "by_ref": Ref::Demigod(d), "power": p.slug, "icon": p.icon,
                               "power_name": p.name, "cost": p.cost, "seq": o.seq, "instant": o.instant });
            let Some((mut at, mut target)) = aim(w, &o.target, &p) else {
                let mut v = base;
                v["fizzled"] = "no such target".into();
                event("cast_fizzled", Some(Ref::Demigod(d).to_string()),
                      json!({ "beat": beat, "by": d, "power": p.slug, "reason": "no such target", "seq": o.seq,
                              "instant": o.instant }));
                out.lines.push(v);
                continue;
            };
            if !w.demigods[d.ix()].alive || have < p.cost {
                let mut v = base;
                v["fizzled"] = format!("needs {} bolts, has {have}", p.cost).into();
                v["at"] = json!(at);
                event("cast_fizzled", Some(Ref::Demigod(d).to_string()),
                      json!({ "beat": beat, "by": d, "power": p.slug, "cost": p.cost, "have": have,
                              "reason": "not enough bolts", "seq": o.seq, "instant": o.instant }));
                out.lines.push(v);
                continue;
            }
            // Unreliable physics: at Entropy 40+ a third of casts land off target.
            let mut off = false;
            if fn7_society::entropy_level(w.meta.entropy) >= 2 {
                let mut rng = Rng::derive(w.meta.seed, &[hash_str("off_target"), beat as u64, k]);
                if rng.f64() < 0.33 {
                    let km = if w.is_planet() { 50.0 + 350.0 * rng.f64() } else { 1.0 + 2.0 * rng.f64() };
                    at = fn7_society::geo::destination(at, rng.f64() * 360.0, km);
                    target = None;
                    off = true;
                }
            }
            let place = place_at(w, at);
            let (region, rname) = region_name(w, at);
            let place_name = w.name_of(Ref::Place(place));
            let intent = IntentId::from_ix(w.intents.len());
            let span = match &o.text {
                Some(x) if p.slug == "custom" => format!("cast {x} on {place_name}"),
                _ => format!("cast {} on {place_name}", p.name.to_lowercase()),
            };
            let it = Intent {
                id: intent,
                beat,
                agent: w.demigods[d.ix()].hands[1],
                demigod: d,
                verb: p.slug.clone(),
                targets: target.iter().map(|r| Target::Known(*r)).collect(),
                how: String::new(),
                place: Some(place),
                magnitude: p.magnitude,
                span,
                route: Route::God {
                    god: entropy.unwrap_or(GodId(0)),
                    by: RouteBy::Keyword,
                },
            };
            event("intent", Some(Ref::Demigod(d).to_string()),
                  json!({ "beat": beat, "intent": it, "routed_by": "keyword", "fallback": false, "cast": true }));
            w.intents.push(it);
            let mut witnesses: Vec<Ref> = target.iter().copied().collect();
            witnesses.extend(
                w.alive_demigods()
                    .filter(|x| distance_km(x.pos, at) <= p.radius_km as f64)
                    .map(|x| Ref::Demigod(x.id)),
            );
            witnesses.dedup();
            let tname = target.map(|r| w.name_of(r));
            let act = match &tname {
                Some(n) => format!("{name} casts {} on {n} in {place_name}", p.name.to_lowercase()),
                None => format!("{name} casts {} on {place_name}", p.name.to_lowercase()),
            };
            let prop = Proposal {
                source: Source::Rules,
                intent: Some(intent),
                effect: Effect::Cast {
                    by: d,
                    power: p.slug.clone(),
                    at,
                    target,
                    cost: p.cost,
                    damage: p.damage,
                },
                act,
                actor: Ref::Demigod(d),
                magnitude: p.magnitude,
                valence: if p.damage > 0.0 { -3 } else { 1 },
                witnesses,
                cause: None,
            };
            let (_, applied) = merge::run(w, vec![prop], pantheon, beat, st.max_witnesses as usize);
            let row = applied.rows.first().map(|x| x.1);
            let mut v = base;
            v["at"] = json!(at);
            v["region"] = rname.clone().into();
            v["region_ref"] = json!(Ref::Place(region));
            v["place"] = place_name.clone().into();
            v["target"] = json!(target);
            v["target_name"] = json!(tname);
            v["off_target"] = off.into();
            v["row"] = json!(row.map(Ref::Ledger));
            event("cast", Some(Ref::Demigod(d).to_string()),
                  json!({ "beat": beat, "by": d, "power": p.slug, "cost": p.cost, "at": at, "region": region,
                          "target": target, "damage": p.damage, "off_target": off, "row": row,
                          "bolts_left": w.demigods[d.ix()].bolts, "custom": o.text, "entropy": w.meta.entropy,
                          "seq": o.seq, "instant": o.instant }));
            if row.is_none() {
                v["fizzled"] = "the strike failed".into();
                out.lines.push(v);
                continue;
            }
            out.lines.push(v);
            by_region.entry(region).or_default().push((
                CastLine {
                    by: d,
                    by_name: name,
                    power: p.slug.clone(),
                    power_name: p.name.clone(),
                    blurb: p.blurb.clone(),
                    custom: p.slug == "custom",
                    at,
                    place_name,
                    target,
                    cost: p.cost,
                    magnitude: p.magnitude,
                    radius_km: p.radius_km,
                    tags: p.tags.clone(),
                },
                intent,
            ));
        }
    }
    let world = fn7_society::entropy_prose(w);
    for (region, casts) in by_region {
        let n = ramifications_for(&casts.iter().map(|(c, _)| (c.cost, c.magnitude)).collect::<Vec<_>>());
        let first = &casts[0];
        let mut refs: Vec<Ref> = casts.iter().filter_map(|(c, _)| c.target).collect();
        let mut near: Vec<(f64, Ref)> = present_in(w, region)
            .into_iter()
            .filter(|r| matches!(r, Ref::Demigod(_) | Ref::Char(_) | Ref::Cluster(_) | Ref::Place(_)))
            .filter_map(|r| pos_of(w, r).map(|p| (distance_km(p, first.0.at), r)))
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        refs.extend(near.into_iter().map(|x| x.1).take(16));
        let mut seen = std::collections::BTreeSet::new();
        refs.retain(|r| seen.insert(*r));
        let present: Vec<Entity> = refs.iter().filter_map(|r| entity(w, *r)).collect();
        let mods: Vec<String> = w
            .mods
            .iter()
            .filter(|m| m.region == region)
            .take(6)
            .map(|m| format!("- {} (x{}): {}", m.name, m.stacks, m.description))
            .collect();
        out.ramify_intents.push((first.1, first.0.by, first.0.at));
        out.ramify.push(RamifyCtx {
            region,
            region_name: w.name_of(Ref::Place(region)),
            beat,
            seed: w.meta.seed,
            god: entropy.unwrap_or(GodId(0)),
            ruling: RulingId(0),
            casts: casts.into_iter().map(|x| x.0).collect(),
            n,
            present,
            mods,
            world: world.clone(),
        });
    }
    out
}

//! God-tier powers (rules 0.8.0): one signature power per god, cast from the power ring
//! (`fields.cast {power: "god_<slug>", tier: "god", target}`, an instant action). Paid at the next resolve:
//! `god_power_cost` bolts (3) from the purse, every OTHER player with a demigod standing gains `god_power_gift` (1) bolt
//! (like a mega), Entropy rises `entropy_per_mega`. Then it is **that god's ruling**: an invocation routed to the god's
//! mind (verdict, price from its table, precedence in the merge; its invocation price in bolts is already paid). Unless
//! the god refuses, its signature effect lands as the god's own proposals:
//!
//! | God | Signature |
//! |---|---|
//! | Time | rewind one target 2 beats; aimed at a point, rival demigods within 1,500 km are slowed a beat |
//! | Nature | a wild surge of growth and beasts: 1.2 damage to rival demigods within 400 km |
//! | Consequence | a rival's biggest open debt is collected now |
//! | Death | raise a fallen target, or strike a standing one for 3 |
//! | Chance | a coin-flip storm: each demigod within 600 km heals 1.5 or takes 1.5, by a seeded coin |
//! | Memory | the region forgets: every crowd's allegiance there resets to nothing |
//! | Entropy | a chaos burst: Entropy +15 |

use fn7_model::seed::{hash_str, Rng};
use fn7_model::*;
use fn7_society::geo::{distance_km, place_at};
use serde_json::{json, Value};

use crate::actions::GodPowerOrder;
use crate::beat::Turn;
use crate::settings::Settings;

/// The `Intent::how` that marks a god-tier invocation (its bolts are paid; the signature effect follows the ruling).
pub const HOW: &str = "god-tier power";

/// `(slug, power name, verb, words, aimed at a unit, radius km)`.
pub const POWERS: &[(&str, &str, &str, &str, bool, f32)] = &[
    ("time", "Time Stands Still", "rewind", "rewind my target and slow the world around it", true, 1500.0),
    ("nature", "The Wild Surge", "unleash", "unleash a wild surge of growth and beasts", false, 400.0),
    ("consequence", "The Reckoning", "collect", "collect my rival's debt, now", true, 0.0),
    ("death", "The Pale Verdict", "strike", "raise the fallen or strike the living", true, 0.0),
    ("chance", "The Coin-Flip Storm", "gamble", "a storm of coin flips: luck for some, ruin for others", false, 600.0),
    ("memory", "The Great Forgetting", "forget", "make the whole region forget whom it loves", false, 0.0),
    ("entropy", "The Chaos Burst", "unleash chaos", "unleash a burst of pure chaos", false, 0.0),
];

fn power(slug: &str) -> Option<&'static (&'static str, &'static str, &'static str, &'static str, bool, f32)> {
    POWERS.iter().find(|p| p.0 == slug)
}

/// The god-tier entries of the power palette (`powers[]` with `tier: "god"`).
pub fn powers_json(pantheon: &Pantheon, st: &Settings) -> Vec<Value> {
    POWERS
        .iter()
        .map(|(slug, name, _, words, unit, radius)| {
            json!({ "slug": format!("god_{slug}"), "name": name, "icon": format!("god_{slug}"), "cost": st.god_power_cost,
                    "radius_km": radius, "target": if *unit { "unit" } else { "point" }, "magnitude": 8, "damage": null,
                    "tags": ["god", *slug], "tier": "god", "god": slug, "god_ref": pantheon.id_of(slug).map(Ref::God),
                    "blurb": words })
        })
        .collect()
}

/// Settle this beat's god-tier powers (arrival order): pay, gift, Entropy, and the invocation to the god (pushed as an
/// intent the route sends to its mind). Returns the public lines.
pub fn process(w: &mut World, beat: u32, turns: &[Turn], pantheon: &Pantheon, st: &Settings) -> Vec<Value> {
    let mut order: Vec<(&Turn, &GodPowerOrder)> = turns.iter().flat_map(|t| t.input.god_powers.iter().map(move |g| (t, g))).collect();
    order.sort_by_key(|(t, g)| (g.seq, t.demigod.0));
    let mut out = vec![];
    for (t, g) in order {
        let d = t.demigod;
        let Some(&(slug, name, verb, words, unit, radius)) = power(&g.god) else { continue };
        let Some(god) = pantheon.id_of(slug) else { continue };
        let me = &w.demigods[d.ix()];
        let mut line = json!({ "by": me.player_name(), "by_ref": Ref::Demigod(d), "power": format!("god_{slug}"),
                               "power_name": name, "tier": "god", "god": slug, "god_ref": Ref::God(god),
                               "cost": st.god_power_cost, "seq": g.seq });
        if !me.alive || me.bolts < st.god_power_cost {
            line["fizzled"] = format!("needs {} bolts, has {}", st.god_power_cost, me.bolts).into();
            out.push(line);
            continue;
        }
        let pd = PowerDef {
            slug: format!("god_{slug}"), name: name.into(), icon: format!("god_{slug}"), cost: st.god_power_cost,
            radius_km: radius, target: if unit { PowerTarget::Unit } else { PowerTarget::Point }, magnitude: 8,
            damage: 0.0, tags: vec![], blurb: words.into(), mega: None, mini: None,
        };
        let Some((at, target)) = crate::casts::aim(w, &g.target, &pd) else {
            line["fizzled"] = "no such target".into();
            out.push(line);
            continue;
        };
        // Pay; gift every other player once; Entropy as a mega.
        let bolts = me.bolts - st.god_power_cost;
        w.set_bolts(d, bolts);
        let seat = w.demigods[d.ix()].seat;
        for other in w.alive_seats().into_iter().filter(|s| *s != seat) {
            let x = w.bodies_of_seat(other).next().map(|x| (x.id, x.bolts));
            if let Some((id, b)) = x {
                w.set_bolts(id, b + st.god_power_gift);
            }
        }
        let place = place_at(w, at);
        let entropy = Proposal {
            source: Source::Rules,
            intent: None,
            effect: Effect::Entropy { delta: st.entropy_per_mega * st.entropy_scale },
            act: format!("{} calls on {}: {name}", w.demigods[d.ix()].name, w.name_of(Ref::God(god))),
            actor: Ref::Demigod(d),
            magnitude: 6,
            valence: 0,
            witnesses: vec![Ref::Demigod(d)],
            cause: None,
        };
        crate::merge::run(w, vec![entropy], pantheon, beat, st.max_witnesses as usize);
        let intent = Intent {
            id: IntentId::from_ix(w.intents.len()),
            beat,
            agent: w.demigods[d.ix()].hands[1],
            demigod: d,
            verb: verb.into(),
            targets: target.iter().map(|r| Target::Known(*r)).collect(),
            how: HOW.into(),
            place: Some(place),
            magnitude: 8,
            span: format!("{words} ({name}) at {}", w.name_of(Ref::Place(place))),
            route: Route::God { god, by: RouteBy::Keyword },
        };
        crate::world::event("intent", Some(Ref::Demigod(d).to_string()),
                            json!({ "beat": beat, "intent": intent, "routed_by": "keyword", "fallback": false, "god_tier": true }));
        w.intents.push(intent);
        line["at"] = json!(at);
        line["target"] = json!(target);
        line["place"] = w.name_of(Ref::Place(place)).into();
        line["region_ref"] = json!(Ref::Place(fn7_society::geo::region_at(w, at)));
        out.push(line);
    }
    out
}

/// The signature effect of a god-tier ruling that wasn't refused, as the god's proposals.
pub fn signature(w: &World, pantheon: &Pantheon, ruling: &Ruling, intent: &Intent) -> Vec<Proposal> {
    let Some(def) = pantheon.get(ruling.god) else { return vec![] };
    let d = intent.demigod;
    let target = intent.targets.iter().find_map(|t| match t {
        Target::Known(r) => Some(*r),
        _ => None,
    });
    let center = target
        .and_then(|r| fn7_society::geo::pos_of(w, r))
        .or_else(|| intent.place.and_then(|p| w.places.get(p.ix()).map(|x| x.center)))
        .unwrap_or(w.demigods[d.ix()].pos);
    let rivals_within = |km: f64| -> Vec<DemigodId> {
        w.alive_demigods().filter(|x| !w.same_player(x.id, d) && distance_km(x.pos, center) <= km).map(|x| x.id).collect()
    };
    let p = |effect: Effect, act: String| crate::beat::god_prop(ruling, intent, effect, act);
    let god = def.name.clone();
    let mut out = vec![];
    // Rules 0.9.0: aimed at one of the caster's own standing demigods, a god-tier power also ARMS it: god-blessed
    // gear, a tier up, with that god's signature (once per god per demigod). Death arms instead of striking its own.
    let own = match target {
        Some(Ref::Demigod(t)) if crate::wills::on(w) && w.same_player(t, d) && w.demigods[t.ix()].alive => Some(t),
        _ => None,
    };
    if let Some(t) = own {
        out.extend(crate::wills::god_arms(w, ruling, intent, t));
        if def.slug == "death" {
            return out;
        }
    }
    match def.slug.as_str() {
        "time" => match target.filter(|r| r.is_actor()) {
            Some(r) => out.push(p(Effect::Rewind { target: r, beats: 2 }, format!("{god} winds {} back two beats", w.name_of(r)))),
            None => {
                for x in rivals_within(1500.0) {
                    out.push(p(Effect::Tempo { who: Ref::Demigod(x), factor: 0.5, beats: 1 },
                               format!("{god} holds {} still for a beat", w.demigods[x.ix()].name)));
                }
            }
        },
        "nature" => {
            out.push(p(Effect::Happen { text: "a wild surge of growth and beasts tears through the land".into(), at: center },
                       format!("{god} unleashes a wild surge")));
            for x in rivals_within(400.0) {
                out.push(p(Effect::Damage { who: Ref::Demigod(x), amount: 1.2, cause: "a beast surge".into() },
                           format!("beasts of {god} maul {}", w.demigods[x.ix()].name)));
            }
        }
        "consequence" => {
            if let Some(Ref::Demigod(x)) = target.filter(|r| matches!(r, Ref::Demigod(t) if !w.same_player(*t, d))) {
                let biggest = w
                    .ledger
                    .open_debts()
                    .filter(|r| r.debt.is_some_and(|b| b.owed_by == Ref::Demigod(x)))
                    .max_by(|a, b| a.debt.map(|x| x.amount).unwrap_or(0.0).total_cmp(&b.debt.map(|x| x.amount).unwrap_or(0.0)).then(b.id.cmp(&a.id)));
                if let Some(r) = biggest {
                    out.push(p(Effect::Collect { row: r.id }, format!("{god} collects {}'s debt early", w.demigods[x.ix()].name)));
                }
            }
        }
        "death" => {
            if let Some(r @ (Ref::Demigod(_) | Ref::Char(_))) = target {
                let alive = match r {
                    Ref::Demigod(x) => w.demigods[x.ix()].alive,
                    Ref::Char(c) => w.characters[c.ix()].alive,
                    _ => true,
                };
                if alive {
                    out.push(p(Effect::Damage { who: r, amount: 3.0, cause: format!("{god}'s pale verdict") },
                               format!("{god} strikes {}", w.name_of(r))));
                } else {
                    out.push(p(Effect::Revive { who: r }, format!("{god} sends {} back", w.name_of(r))));
                }
            }
        }
        "chance" => {
            let mut rng = Rng::derive(w.meta.seed, &[hash_str("coin_storm"), intent.id.0 as u64]);
            for x in w.alive_demigods().filter(|x| distance_km(x.pos, center) <= 600.0).map(|x| x.id).collect::<Vec<_>>() {
                let n = w.demigods[x.ix()].name.clone();
                if rng.below(2) == 0 {
                    out.push(p(Effect::Heal { who: Ref::Demigod(x), amount: 1.5 }, format!("heads: {god} smiles on {n}")));
                } else {
                    out.push(p(Effect::Damage { who: Ref::Demigod(x), amount: 1.5, cause: "a coin-flip storm".into() },
                               format!("tails: {god} turns on {n}")));
                }
            }
        }
        "memory" => {
            let region = fn7_society::geo::region_at(w, center);
            for c in w.clusters.iter().filter(|c| c.army.is_none() && fn7_society::geo::district_of(w, c.place) == region) {
                for (dg, v) in c.allegiance.iter().filter(|(_, v)| *v != 0.0) {
                    out.push(p(Effect::Allegiance { who: Ref::Cluster(c.id), demigod: *dg, delta: -*v },
                               format!("the {} forget whom they loved", c.label)));
                }
            }
        }
        "entropy" => out.push(p(Effect::Entropy { delta: 15.0 }, format!("{god} bursts: the world gets much stranger"))),
        _ => {}
    }
    out
}

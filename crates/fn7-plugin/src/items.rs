//! The item engine in the beat (rules 0.7.0, state schema 5; designed in docs/DEMIGODS_AND_ITEMS.md §4).
//!
//! A demigod's order to manifest something ("raise banners of 100,000,000 soldiers", "forge a sword of stars") is read
//! by rules ([`is_manifest_act`], then `fn7_model::manifest::request_from_text`), judged by the item engine's pure
//! rules (tier, cost in bolts with the shortfall as Consequence's debt, odds, one seeded roll: `Rng::derive(seed,
//! [MANIFEST_SALT, beat, mf id])`), and becomes proposals: the payment, the debt, a backfire's damage, Entropy, and
//! `Effect::Manifest` (the `mf:` row, and the item if one appeared). The LLM only names and describes a made thing
//! (the `manifest` mind, for wonders and up and twists, at most `manifest_calls_per_beat`); the rules name the rest.
//! The verdict never comes from an LLM, so a recording replays exactly.
//!
//! Who pays: the purse is the player's (both demigods share it); a debt and a backfire land on the demigod who acted.
//! A made host (army, fleet, creatures) of 100 or more takes the field as an army cluster of
//! `muster_size(count) × manifest_muster_scale` under its maker; its losses scale the item's narrative count
//! ([`sync_hosts`]).

use std::collections::BTreeMap;

use fn7_agents::manifest::{ManifestCtx, ManifestOut};
use fn7_model::manifest::*;
use fn7_model::seed::Rng;
use fn7_model::*;
use serde_json::{json, Value};

use crate::settings::Settings;
use crate::world::event;

/// Verbs that make an act a manifestation (with a thing to make).
const VERBS: &[&str] = &[
    "manifest", "conjure", "forge", "summon", "materialize", "materialise", "craft", "create", "raise", "build",
    "make", "call forth", "bring forth", "spawn", "erect", "unleash",
];
/// Verbs strong enough that an army asked for with them is conjured, not recruited from a crowd.
const STRONG: &[&str] = &["manifest", "conjure", "summon", "materialize", "materialise", "call forth", "bring forth"];

fn has(text: &str, words: &[&str]) -> bool {
    words.iter().any(|w| fn7_agents::common::has_phrase(text, w))
}

/// Is this act an order to manifest something (schema 5)? A making verb and a thing the engine knows (an army,
/// fleet, creature, structure, weapon, armor, relic, artifact or phenomenon). An army is conjured only with a count
/// of 1,000 or more, banners, or a strong verb ("summon", "conjure", "manifest"); "raise an army from the dockworkers"
/// stays a recruitment from a crowd (the army rules).
///
/// World acts qualify, and so do acts the rules (not the LLM) routed to a god by keyword ("raise" reads as Death's
/// raise-the-dead, "summon" as Nature's beasts), except phenomena ("summon a storm over Vex" stays Nature's).
pub fn is_manifest_act(w: &World, i: &Intent) -> bool {
    if !w.two_bodies() {
        return false;
    }
    let by_keyword = matches!(i.route, Route::God { by: RouteBy::Keyword, .. });
    if !(matches!(i.route, Route::World | Route::Pending) || by_keyword) {
        return false;
    }
    let text = format!("{} {}", i.verb, i.span).to_lowercase();
    if !has(&text, VERBS) {
        return false;
    }
    let kind = classify_kind(&text);
    match kind {
        ItemKind::Other => false,
        ItemKind::Phenomenon if by_keyword => false,
        ItemKind::Army => {
            parse_count(&text).is_some_and(|c| c >= 1000.0) || has(&text, &["banner", "banners"]) || has(&text, STRONG)
        }
        _ => true,
    }
}

/// One judged attempt, waiting for its words (the manifest mind or the rules).
#[derive(Debug, Clone)]
pub struct Judged {
    pub m: Manifestation,
    /// What appeared (kind, tier, count after a partial), if anything did.
    pub made: Option<(ItemKind, Tier, f64)>,
    /// Payment, debt, backfire and Entropy (rules proposals).
    pub props: Vec<Proposal>,
    /// The words call (present only when something appeared).
    pub call: Option<ManifestCtx>,
    /// Whether the call may use the LLM (big tiers and twists, within the beat's budget).
    pub ask: bool,
    /// Rules 0.8.0 (schema 6): the attempt's d20 (`mf:N`), recorded and shown once the attempt is judged.
    pub dice: Option<fn7_model::dm::DiceRoll>,
}

/// Judge `intent` as manifestation number `k` of this beat (its id is `w.manifests.len() + k`). `spent` tracks bolts
/// already committed per seat this beat (the purse is shared by the player's demigods).
pub fn judge(
    w: &World,
    intent: &Intent,
    beat: u32,
    k: usize,
    spent: &mut BTreeMap<u8, u32>,
    pantheon: &Pantheon,
    st: &Settings,
) -> Option<Judged> {
    let d = intent.demigod;
    let me = w.demigods.get(d.ix()).filter(|x| x.alive)?;
    let req = request_from_text(&format!("{} {}", intent.verb, intent.span), intent.magnitude);
    let tier = classify(req.kind, req.count, req.magnitude);
    let used = spent.get(&me.seat).copied().unwrap_or(0);
    let have = me.bolts.saturating_sub(used);
    let cost = match cost_scaled(tier, have, req.offer_bolts, req.accept_debt, st.manifest_scale) {
        Ok(c) => c,
        Err(why) => {
            event(
                "manifest_refused",
                Some(Ref::Demigod(d).to_string()),
                json!({ "beat": beat, "intent": intent.id, "tier": tier, "reason": format!("{why:?}") }),
            );
            return None;
        }
    };
    *spent.entry(me.seat).or_default() += cost.bolts;
    let god = req.god.unwrap_or_else(|| req.kind.patron());
    let god = if god.ix() < w.gods.len() { god } else { pantheon.id_of("entropy").unwrap_or(GodId(0)) };
    let favor = w.gods.get(god.ix()).and_then(|g| g.favor.get(d.ix())).copied().unwrap_or(0.0);
    let recent = w.manifests.iter().filter(|m| m.demigod == d && m.beat + REPEAT_WINDOW > beat).count() as u32;
    let odds = odds(&OddsInputs { tier, cost, favor, stance: None, entropy: w.meta.entropy, recent });
    let id = ManifestationId::from_ix(w.manifests.len() + k);
    let (u, dice) = if w.dm_rules() {
        // Rules 0.8.0: the d20 decides (the DC carries the odds; see `dm::manifest_d20`).
        let (r, u) = crate::dm::manifest_d20(w, id.0, odds.success, fn7_model::manifest::TIER_BASE_P[tier.index()]);
        (u, Some(r))
    } else {
        (Rng::derive(w.meta.seed, &[MANIFEST_SALT, beat as u64, id.0 as u64]).f64(), None)
    };
    let outcome = roll(&odds, u);
    let made = outcome.made().then(|| {
        let (c, t) = if outcome == Outcome::Partial { partial_of(req.kind, req.count, tier) } else { (req.count, tier) };
        (req.kind, t, c)
    });
    let mut reasons = vec![format!("{} tier: {} bolt{}", tier.as_str(), cost.required, if cost.required == 1 { "" } else { "s" })];
    if cost.missing > 0 {
        reasons.push(format!("{} bolt(s) short: {:.1} debt to Consequence", cost.missing, cost.debt));
    }
    if favor.abs() >= 1.0 {
        reasons.push(format!("{} {} you", w.name_of(Ref::God(god)), if favor > 0.0 { "favors" } else { "dislikes" }));
    }
    if tier >= Tier::Wonder && w.meta.entropy >= 10.0 {
        reasons.push("Entropy loves this".into());
    }
    if recent > 0 {
        reasons.push(format!("{recent} recent attempt(s)"));
    }
    let public = made.is_some_and(|(_, t, _)| t >= PUBLIC_TIER) || tier >= Tier::Legend;
    let m = Manifestation {
        id,
        beat,
        by: Ref::Demigod(d),
        demigod: d,
        hand: me.side,
        agent: Some(intent.agent),
        intent: Some(intent.id),
        request: req.clone(),
        place: Some(me.place),
        at: Some(me.pos),
        verdict: ManifestVerdict {
            outcome,
            tier,
            odds,
            roll: u,
            cost,
            god,
            favor,
            stance: None,
            entropy: w.meta.entropy,
            reasons,
            flavor: Flavor::Rules,
        },
        item: None,
        rows: vec![],
        public,
    };
    let rules = |effect: Effect, act: String, mag: u8, val: i8| Proposal {
        source: Source::Rules,
        intent: Some(intent.id),
        effect,
        act,
        actor: Ref::Demigod(d),
        magnitude: mag,
        valence: val,
        witnesses: vec![Ref::Demigod(d)],
        cause: None,
    };
    let mut props = vec![];
    if cost.bolts > 0 {
        props.push(rules(
            Effect::Bolts { who: d, delta: -(cost.bolts as i32), to: Some(god), toll: false },
            format!("{} spends {} bolt{} to manifest", me.name, cost.bolts, if cost.bolts == 1 { "" } else { "s" }),
            2,
            0,
        ));
    }
    if cost.debt > 0.0 {
        if let Some(cg) = pantheon.id_of("consequence") {
            props.push(rules(
                Effect::Debt { owed_by: Ref::Demigod(d), god: cg, amount: (cost.debt * 100.0).round() / 100.0, due_in: st.debt_due_beats },
                format!("Consequence finances {}'s manifestation", me.name),
                2,
                -1,
            ));
        }
    }
    if outcome == Outcome::Backfired {
        props.push(rules(
            Effect::Damage { who: Ref::Demigod(d), amount: backfire_damage(tier), cause: "a manifestation backfired".into() },
            format!("{}'s manifestation backfires", me.name),
            (3 + tier.index()).min(10) as u8,
            -3,
        ));
    }
    let rise = entropy_rise(tier);
    if rise > 0.0 {
        props.push(rules(Effect::Entropy { delta: rise }, "Reality strains around a demigod's will".into(), 2, 0));
    }
    let call = made.map(|(kind, t, count)| ManifestCtx {
        demigod: d,
        by_name: me.name.clone(),
        look: crate::visual::demigod(w, me).brief(),
        kind,
        tier: t,
        count,
        unit: kind.unit().to_string(),
        outcome,
        place_name: w.name_of(Ref::Place(me.place)),
        span: intent.span.clone(),
        asked_name: req.name.clone(),
        rules_name: rules_name_for(kind, t, count, &intent.span),
    });
    let ask = made.is_some_and(|(_, t, _)| t >= Tier::Wonder) || outcome == Outcome::Twisted;
    Some(Judged { m, made, props, call, ask, dice })
}

/// The `Effect::Manifest` proposal for a judged attempt, with its words (`None`: nothing appeared).
pub fn proposal(w: &World, j: &Judged, words: Option<&ManifestOut>, llm: bool, st: &Settings) -> Proposal {
    let d = j.m.demigod;
    let me = &w.demigods[d.ix()];
    let mut m = j.m.clone();
    m.verdict.flavor = match (words.is_some(), llm) {
        (true, true) => Flavor::Llm,
        _ => Flavor::Rules,
    };
    let item = j.made.map(|(kind, tier, count)| {
        let fb = j.call.as_ref().map(fn7_agents::manifest::fallback);
        let w_ = words.cloned().or(fb).unwrap_or(ManifestOut {
            name: rules_name(kind, tier, count),
            description: String::new(),
            motto: None,
            twist: None,
        });
        let v = crate::visual::demigod(w, me);
        let banner = kind.is_host().then(|| Banner {
            device: v.traits.emblem.clone(),
            colors: v.traits.colors.clone(),
            motto: w_.motto.clone(),
        });
        let muster = if kind.is_host() && count >= 100.0 {
            ((muster_size(count) as f32) * st.manifest_muster_scale).round() as u32
        } else {
            0
        };
        let image_prompt = match &banner {
            Some(b) => format!(
                "{}: {}; a host under banners in {} and {} bearing {}",
                w_.name, w_.description, b.colors[0], b.colors[1], b.device
            ),
            None => format!("{}: {}", w_.name, w_.description),
        };
        NewItem {
            name: w_.name.clone(),
            description: match &w_.twist {
                Some(t) => format!("{} (Twisted: {t})", w_.description),
                None => w_.description.clone(),
            },
            ext: ItemExt {
                kind,
                tier,
                count,
                unit: kind.unit().to_string(),
                state: if muster > 0 { ItemState::Deployed } else { ItemState::Held },
                manifestation: None,
                banner,
                image_prompt: Some(image_prompt),
                at: Some(me.pos),
                place: Some(me.place),
                muster: None,
                history: vec![],
            },
            muster,
        }
    });
    let tier = j.made.map(|x| x.1).unwrap_or(m.verdict.tier);
    let act = match (&item, m.verdict.outcome) {
        (Some(it), Outcome::Partial) => format!("{} manifests {}, smaller than asked", me.name, it.name),
        (Some(it), Outcome::Twisted) => format!("{} manifests {}, but not as intended", me.name, it.name),
        (Some(it), _) => format!("{} manifests {}", me.name, it.name),
        (None, Outcome::Backfired) => format!("{} tries to manifest something and it blows up in their face", me.name),
        (None, _) => format!("{} tries to manifest something and nothing appears", me.name),
    };
    Proposal {
        source: Source::Agent { agent: j.m.agent.unwrap_or(me.hands[0]) },
        intent: j.m.intent,
        effect: Effect::Manifest { manifestation: Box::new(m), item },
        act,
        actor: Ref::Demigod(d),
        magnitude: (4 + tier.index()).min(10) as u8,
        valence: 1,
        witnesses: vec![Ref::Demigod(d)],
        cause: None,
    }
}

/// Manifested hosts in the field: the item's narrative count follows its army's losses (the same ratio), and an army
/// that is gone leaves the item `destroyed` (kept, as every item is). Deterministic; runs after the society step.
pub fn sync_hosts(w: &mut World, beat: u32) {
    for i in 0..w.items.len() {
        let it = &w.items[i];
        let Some(c) = it.ext.muster.filter(|_| it.ext.state == ItemState::Deployed) else {
            continue;
        };
        // The army's strength now against what it mustered (its size plus its losses so far).
        let (size, mustered) = match w.clusters.get(c.ix()) {
            Some(x) => match &x.army {
                Some(a) if !a.rogue => (x.size, (x.size + a.losses).max(1)),
                _ => (0, 1),
            },
            None => (0, 1),
        };
        let made = it.ext.history.first().and_then(|e| e.count).unwrap_or(it.ext.count);
        let it = &mut w.items[i];
        let holder = it.holders.last().copied();
        if size == 0 {
            it.ext.state = ItemState::Destroyed;
            it.ext.history.push(ItemEvent { beat, state: ItemState::Destroyed, holder, by: None, row: None, count: Some(0.0) });
            event("item_state", Some(Ref::Item(it.id).to_string()), json!({ "beat": beat, "item": it.id, "state": "destroyed" }));
            continue;
        }
        let count = (made * size as f64 / mustered as f64).floor().max(1.0);
        if count + 1.0 <= it.ext.count {
            it.ext.count = count;
            it.ext.history.push(ItemEvent { beat, state: ItemState::Deployed, holder, by: None, row: None, count: Some(count) });
        }
    }
}

/// An item for a payload: `{ref, name, description, kind, icon, tier, count, unit, state, holder, created_beat,
/// manifestation, at, banner, image_prompt, image_kind}` (counts rounded to integers).
pub fn item_json(w: &World, it: &Item) -> Value {
    let e = &it.ext;
    let mut v = json!({
        "ref": Ref::Item(it.id), "name": it.name, "description": it.description,
        "kind": e.kind, "icon": icon_of(e.kind, &it.name), "tier": e.tier, "count": e.count.round(), "unit": e.unit,
        "state": e.state, "holder": it.holders.last(), "holder_name": it.holders.last().map(|h| w.name_of(*h)),
        "created_beat": it.created_beat,
        "manifestation": e.manifestation.map(|m| m.to_string()), "at": e.at, "banner": e.banner,
        "image_prompt": e.image_prompt.clone().unwrap_or_else(|| format!("{}: {}", it.name, it.description)),
        "image_kind": "item",
    });
    // Schema 14: how it is wielded in a duel, for what can be wielded.
    if w.duel_weapon_rules() {
        if let Some(wc) = crate::weapons::weapon_class(e.kind, &it.name) {
            v["weapon_class"] = wc.into();
        }
    }
    // Rules 0.9.0: what the item DOES (`slot`, `material`, `mods`, `power`, `blessed_by`, `does`), for gear.
    if crate::wills::on(w) {
        crate::wills::arms::item_fields(w, it, &mut v);
    }
    // Gear looks (schema 11): only an item granted a picture carries `image_prompt` (the descriptor's subject, so the
    // same item is the same picture everywhere); the rest keep their `icon`.
    if crate::look::on(w) {
        crate::look::art_fields(w, &crate::look::book(w), it, &mut v);
    }
    v
}

/// A manifestation for a payload. `full` (the maker's own seat) adds the verdict's odds, roll, cost and reasons.
pub fn manifest_json(w: &World, m: &Manifestation, full: bool) -> Value {
    let item = m.item.and_then(|i| w.items.get(i.ix()));
    let (kind, tier, count) = match item {
        Some(i) => (i.ext.kind, i.ext.tier, i.ext.count),
        None => (m.request.kind, m.verdict.tier, m.request.count),
    };
    let mut v = json!({
        "ref": m.id.to_string(), "beat": m.beat, "by": m.by, "by_name": w.name_of(m.by), "side": m.hand,
        "kind": kind, "icon": icon_of(kind, item.map(|i| i.name.as_str()).unwrap_or("")), "tier": tier,
        "count": count.round(), "unit": kind.unit(), "outcome": m.verdict.outcome,
        "item": m.item.map(Ref::Item), "name": item.map(|i| i.name.clone()), "at": m.at,
        "region": m.at.map(|p| crate::payload::region_name(w, p)), "public": m.public,
        "image_prompt": item.and_then(|i| i.ext.image_prompt.clone()), "image_kind": "item",
    });
    if crate::look::on(w) {
        match item {
            Some(it) => crate::look::art_fields(w, &crate::look::book(w), it, &mut v),
            None => {
                v["image_prompt"] = Value::Null;
            }
        }
    }
    if full {
        v["verdict"] = json!(m.verdict);
        v["request"] = json!({ "kind": m.request.kind, "count": m.request.count.round(), "magnitude": m.request.magnitude });
    }
    v
}

/// Items a demigod holds now (its manifestations and anything else it carries), for payloads.
pub fn items_of(w: &World, d: DemigodId) -> Vec<Value> {
    let me = Ref::Demigod(d);
    w.items.iter().filter(|i| i.holders.last() == Some(&me)).map(|i| item_json(w, i)).collect()
}

/// The public item pieces of a demigod (made things of tier ≥ relic, or deployed): what rivals see on the table.
pub fn public_items_of(w: &World, d: DemigodId) -> Vec<Value> {
    let me = Ref::Demigod(d);
    w.items
        .iter()
        .filter(|i| i.holders.last() == Some(&me))
        .filter(|i| i.ext.manifestation.is_some() && (i.ext.tier >= PUBLIC_TIER || i.ext.state == ItemState::Deployed))
        .map(|i| item_json(w, i))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent(span: &str) -> Intent {
        Intent {
            id: IntentId(0),
            beat: 1,
            agent: AgentId(0),
            demigod: DemigodId(0),
            verb: span.split_whitespace().next().unwrap_or("").into(),
            targets: vec![],
            how: String::new(),
            place: None,
            magnitude: 7,
            span: span.into(),
            route: Route::World,
        }
    }

    #[test]
    fn what_counts_as_a_manifestation() {
        let mut w = crate::testkit::world(4, 7);
        assert!(w.two_bodies());
        for yes in [
            "raise a banner army of 100,000,000",
            "raise banners of 10,000 soldiers at Lagos",
            "forge a sword of stars",
            "summon an army of the dead",
            "conjure a fortress overnight",
            "manifest an armada of airships",
        ] {
            assert!(is_manifest_act(&w, &intent(yes)), "{yes}");
        }
        for no in ["raise an army from the dockworkers", "search for a weapon", "take the bridge", "go to Paris", "build trust"] {
            assert!(!is_manifest_act(&w, &intent(no)), "{no}");
        }
        w.meta.schema = 4;
        assert!(!is_manifest_act(&w, &intent("forge a sword of stars")), "schema 4 keeps the old rules");
    }
}

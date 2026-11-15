//! Applying merged effects: validate invariants, mutate the tables, append exactly one ledger row.

use fn7_model::seed::{hash_str, Rng};
use fn7_model::*;

use crate::geo::{place_at, pos_of, random_in};
use crate::mention::{materialize, resolve_mention, Resolution};
use crate::template::template;
use crate::util::{bump, demigod_of, end_agents_for, exists, groups_incl, is_alive, place_of};

/// Starting maximum health of a new body (Final Night's scale). Each body's own cap is its `max_health`.
pub const MAX_HEALTH: f32 = DEFAULT_MAX_HEALTH;
/// `Revive` brings a body back at this fraction of its `max_health`.
pub const REVIVE_FRACTION: f32 = 0.5;
/// Bounds on a `Tempo` factor (quarter speed .. four times as fast) and its length in beats.
pub const TEMPO_MIN: f32 = 0.25;
pub const TEMPO_MAX: f32 = 4.0;
pub const TEMPO_MAX_BEATS: u32 = 10;
/// Beats until a `Price { kind: Debt }` comes due.
pub const PRICE_DEBT_DUE: u32 = 10;
/// Time's delay grows a debt by this factor.
pub const DELAY_GROWTH: f32 = 1.5;

/// Consequence's bill for an act: rises with magnitude, and softly (logarithmically) with the words the player spoke this
/// beat (the speed counterweight). 0..10.
pub fn debt_for(magnitude: u8, words_this_beat: u32) -> f32 {
    let m = magnitude.min(10) as f32 / 10.0;
    let speed = 1.0 + (1.0 + words_this_beat as f32 / 40.0).ln();
    (m * 1.5 * speed).min(10.0)
}

/// Canonical verb of an effect (its tag).
pub fn verb_of(e: &Effect) -> &'static str {
    match e {
        Effect::Move { .. } => "move",
        Effect::Damage { .. } => "damage",
        Effect::Heal { .. } => "heal",
        Effect::Shrink { .. } => "shrink",
        Effect::MoodShift { .. } => "mood_shift",
        Effect::Allegiance { .. } => "allegiance",
        Effect::Relation { .. } => "relation",
        Effect::Join { .. } => "join",
        Effect::Leave { .. } => "leave",
        Effect::FoundGroup { .. } => "found_group",
        Effect::Memorize { .. } => "memorize",
        Effect::CreateItem { .. } => "create_item",
        Effect::Echo { .. } => "echo",
        Effect::Favor { .. } => "favor",
        Effect::Debt { .. } => "debt",
        Effect::Delay { .. } => "delay",
        Effect::Collect { .. } => "collect",
        Effect::Rewind { .. } => "rewind",
        Effect::Price { .. } => "price",
        Effect::Forget { .. } => "forget",
        Effect::Plant { .. } => "plant",
        Effect::Tell { .. } => "tell",
        Effect::Dispatch { .. } => "dispatch",
        Effect::Eliminate { .. } => "eliminate",
        Effect::Revive { .. } => "revive",
        Effect::Tempo { .. } => "tempo",
        Effect::Note { .. } => "note",
        Effect::Cast { .. } => "cast",
        Effect::Bolts { .. } => "bolts",
        Effect::Entropy { .. } => "entropy",
        Effect::RaiseArmy { .. } => "raise_army",
        Effect::March { .. } => "march",
        Effect::Wound { .. } => "wound",
        Effect::Manifest { .. } => "manifest",
        Effect::Mod { .. } => "mod",
        Effect::Unmod { .. } => "unmod",
        Effect::Happen { .. } => "happen",
        Effect::Society { kind, .. } => kind.as_str(),
        Effect::Orders { .. } => "orders",
        Effect::Promote { .. } => "promote",
    }
}

fn need(w: &World, r: Ref) -> Result<(), String> {
    if exists(w, r) {
        Ok(())
    } else {
        Err(format!("{r} does not exist"))
    }
}

fn need_alive(w: &World, r: Ref) -> Result<(), String> {
    need(w, r)?;
    if is_alive(w, r) {
        Ok(())
    } else {
        Err(format!("{r} is dead"))
    }
}

fn finite(x: f32, what: &str) -> Result<(), String> {
    if x.is_finite() {
        Ok(())
    } else {
        Err(format!("{what} is not finite"))
    }
}

fn nonneg(x: f32, what: &str) -> Result<(), String> {
    finite(x, what)?;
    if x >= 0.0 {
        Ok(())
    } else {
        Err(format!("{what} is negative"))
    }
}

fn need_member(r: Ref) -> Result<(), String> {
    match r {
        Ref::Char(_) | Ref::Cluster(_) => Ok(()),
        _ => Err(format!("{r} can't belong to groups")),
    }
}

/// Set someone's health, capped at their `max_health`; at 0 they die (demigods are eliminated). Assumes validated.
fn set_health(w: &mut World, who: Ref, h: f32, beat: u32, cause: &str) {
    let h = h.clamp(0.0, max_health_of(w, who).unwrap_or(MAX_HEALTH).max(0.0));
    match who {
        Ref::Demigod(d) => w.demigods[d.ix()].health = h,
        Ref::Char(c) => w.characters[c.ix()].health = h,
        _ => return,
    }
    if h <= 0.0 {
        kill(w, who, beat, cause);
    }
}

fn kill(w: &mut World, who: Ref, beat: u32, cause: &str) {
    match who {
        Ref::Demigod(d) => {
            let x = &mut w.demigods[d.ix()];
            x.health = 0.0;
            x.alive = false;
            x.eliminated_beat = Some(beat);
            x.cause = Some(cause.to_string());
            settle_debts_of(w, who);
        }
        Ref::Char(c) => {
            let x = &mut w.characters[c.ix()];
            x.health = 0.0;
            x.alive = false;
        }
        _ => return,
    }
    end_agents_for(w, who, beat);
}

/// A demigod who falls owes nothing more: every open or delayed debt of theirs is closed as Settled (Consequence never
/// collects from the dead).
pub fn settle_debts_of(w: &mut World, who: Ref) {
    for r in w.ledger.rows.iter_mut() {
        if let Some(d) = r.debt.as_mut() {
            if d.owed_by == who && matches!(d.status, DebtStatus::Open | DebtStatus::Delayed) {
                d.status = DebtStatus::Settled;
            }
        }
    }
}

fn health_of(w: &World, r: Ref) -> Option<f32> {
    match r {
        Ref::Demigod(d) => w.demigods.get(d.ix()).map(|x| x.health),
        Ref::Char(c) => w.characters.get(c.ix()).map(|x| x.health),
        _ => None,
    }
}

fn max_health_of(w: &World, r: Ref) -> Option<f32> {
    match r {
        Ref::Demigod(d) => w.demigods.get(d.ix()).map(|x| x.max_health),
        Ref::Char(c) => w.characters.get(c.ix()).map(|x| x.max_health),
        _ => None,
    }
}

fn need_body(w: &World, r: Ref) -> Result<(), String> {
    need_alive(w, r)?;
    match r {
        Ref::Demigod(_) | Ref::Char(_) => Ok(()),
        _ => Err(format!("{r} has no body")),
    }
}

/// Push a debt back by `beats`: the amount grows by 50%, status becomes Delayed. Validates the row has an open debt.
pub fn delay_debt(w: &mut World, row: LedgerId, beats: u32) -> Result<(), String> {
    let r = w
        .ledger
        .rows
        .get_mut(row.ix())
        .ok_or_else(|| format!("{row} does not exist"))?;
    let d = r
        .debt
        .as_mut()
        .ok_or_else(|| format!("{row} has no debt"))?;
    if !matches!(d.status, DebtStatus::Open | DebtStatus::Delayed) {
        return Err(format!("{row}'s debt is already settled"));
    }
    d.amount = (d.amount * DELAY_GROWTH).min(10.0);
    d.due_beat = d.due_beat.saturating_add(beats.max(1));
    d.delays = d.delays.saturating_add(1);
    d.status = DebtStatus::Delayed;
    Ok(())
}

/// Consequence collects a due debt: appends a `Collect` row (source Rules, actor the god it's owed to) that damages the
/// debtor by the amount and marks the debt Collected. Returns the new row.
pub fn collect_debt(w: &mut World, row: LedgerId, beat: u32) -> Result<LedgerId, String> {
    let r = w
        .ledger
        .get(row)
        .ok_or_else(|| format!("{row} does not exist"))?;
    let d = r.debt.ok_or_else(|| format!("{row} has no debt"))?;
    let act = format!(
        "The debt of {} comes due and is collected.",
        w.name_of(d.owed_by)
    );
    let p = Proposal {
        source: Source::Rules,
        intent: None,
        effect: Effect::Collect { row },
        act,
        actor: Ref::God(d.owed_to),
        magnitude: d.amount.round().clamp(0.0, 10.0) as u8,
        valence: -2,
        witnesses: vec![d.owed_by],
        cause: Some(row),
    };
    apply(w, &p, beat)
}

/// Apply one merged proposal: validate, mutate, and append exactly one ledger row. On `Err` nothing changed.
/// `Forget`/`Plant` belong to the memory crate: only their row is written here.
pub fn apply(w: &mut World, p: &Proposal, beat: u32) -> Result<LedgerId, String> {
    need(w, p.actor)?;
    if !is_alive(w, p.actor) {
        return Err(format!("{} is dead and can't act", p.actor));
    }
    for x in &p.witnesses {
        need(w, *x)?;
    }
    if let Some(c) = p.cause {
        need(w, Ref::Ledger(c))?;
    }
    let id = w.ledger.next_id();
    let mut known = GroupSet::default();
    for x in &p.witnesses {
        known = known.union(&groups_incl(w, *x));
    }
    let known_at = known.as_slice().iter().map(|g| (*g, beat)).collect();
    let mut row = LedgerRow {
        id,
        beat,
        source: p.source,
        actor: p.actor,
        verb: verb_of(&p.effect).into(),
        act: p.act.clone(),
        targets: vec![],
        place: None,
        magnitude: p.magnitude.min(10),
        valence: p.valence.clamp(-5, 5),
        witnessed_by: p.witnesses.clone(),
        known_by: known,
        known_at,
        told_to: vec![],
        erased_from: vec![],
        debt: None,
        cause: p.cause,
        intent: p.intent,
        at: None,
    };
    let items_before = w.items.len();
    effect(w, p, beat, &mut row)?;
    debug_assert!(
        w.items.len() >= items_before && w.items.iter().all(|i| !i.holders.is_empty()),
        "items are never lost"
    );
    if row.place.is_none() {
        row.place = row
            .targets
            .iter()
            .find_map(|t| place_of(w, *t))
            .or_else(|| place_of(w, p.actor));
    }
    if row.at.is_none() {
        row.at = row
            .targets
            .iter()
            .find_map(|t| pos_of(w, *t))
            .or_else(|| pos_of(w, p.actor))
            .or_else(|| row.place.and_then(|pl| w.places.get(pl.ix()).map(|x| x.center)));
    }
    w.ledger.rows.push(row);
    Ok(id)
}

fn effect(w: &mut World, p: &Proposal, beat: u32, row: &mut LedgerRow) -> Result<(), String> {
    match &p.effect {
        Effect::Move { who, to, place } => {
            need_alive(w, *who)?;
            if !(to[0].is_finite() && to[1].is_finite() && (-90.0..=90.0).contains(&to[1])) {
                return Err("bad position".into());
            }
            if let Some(pl) = place {
                need(w, Ref::Place(*pl))?;
            }
            let pl = place.unwrap_or_else(|| place_at(w, *to));
            match *who {
                Ref::Demigod(d) => {
                    w.demigods[d.ix()].pos = *to;
                    w.demigods[d.ix()].place = pl;
                }
                Ref::Char(c) => {
                    w.characters[c.ix()].pos = *to;
                    w.characters[c.ix()].place = pl;
                }
                Ref::Cluster(c) => w.clusters[c.ix()].place = pl,
                r => return Err(format!("{r} can't move")),
            }
            row.targets = vec![*who];
            row.place = Some(pl);
        }
        Effect::Damage { who, amount, cause } => {
            nonneg(*amount, "damage")?;
            need_alive(w, *who)?;
            match *who {
                Ref::Demigod(_) | Ref::Char(_) => {
                    let h = health_of(w, *who).unwrap_or(0.0) - amount;
                    set_health(w, *who, h, beat, cause);
                }
                Ref::Cluster(c) => {
                    let cl = &mut w.clusters[c.ix()];
                    let deaths = ((cl.size as f32) * (amount / 10.0).min(1.0) * 0.02).ceil() as u32;
                    cl.size = cl.size.saturating_sub(deaths);
                    cl.mood.fear = (cl.mood.fear + amount * 0.03).min(1.0);
                    cl.mood.anger = (cl.mood.anger + amount * 0.02).min(1.0);
                }
                r => return Err(format!("{r} can't be damaged")),
            }
            row.targets = vec![*who];
        }
        Effect::Heal { who, amount } => {
            nonneg(*amount, "heal")?;
            need_body(w, *who)?;
            let h = health_of(w, *who).unwrap_or(0.0) + amount;
            set_health(w, *who, h, beat, "");
            row.targets = vec![*who];
        }
        Effect::Shrink { cluster, count } => {
            need(w, Ref::Cluster(*cluster))?;
            let cl = &mut w.clusters[cluster.ix()];
            if *count > cl.size {
                return Err(format!(
                    "{cluster} has {} left, can't lose {count}",
                    cl.size
                ));
            }
            cl.size -= count;
            row.targets = vec![Ref::Cluster(*cluster)];
        }
        Effect::MoodShift {
            cluster,
            anger,
            fear,
            hope,
            devotion,
        } => {
            need(w, Ref::Cluster(*cluster))?;
            for (x, n) in [
                (anger, "anger"),
                (fear, "fear"),
                (hope, "hope"),
                (devotion, "devotion"),
            ] {
                finite(*x, n)?;
            }
            let m = &mut w.clusters[cluster.ix()].mood;
            m.anger = (m.anger + anger).clamp(0.0, 1.0);
            m.fear = (m.fear + fear).clamp(0.0, 1.0);
            m.hope = (m.hope + hope).clamp(0.0, 1.0);
            m.devotion = (m.devotion + devotion).clamp(0.0, 1.0);
            row.targets = vec![Ref::Cluster(*cluster)];
        }
        Effect::Allegiance {
            who,
            demigod,
            delta,
        } => {
            finite(*delta, "delta")?;
            need(w, Ref::Demigod(*demigod))?;
            need_alive(w, *who)?;
            match *who {
                Ref::Char(c) => {
                    bump(
                        &mut w.characters[c.ix()].allegiance,
                        *demigod,
                        *delta,
                        -1.0,
                        1.0,
                    );
                }
                Ref::Cluster(c) => {
                    bump(
                        &mut w.clusters[c.ix()].allegiance,
                        *demigod,
                        *delta,
                        -1.0,
                        1.0,
                    );
                }
                r => return Err(format!("{r} has no allegiance")),
            }
            row.targets = vec![*who, Ref::Demigod(*demigod)];
        }
        Effect::Relation {
            a,
            b,
            trust,
            fear,
            loyalty,
        } => {
            need(w, *a)?;
            need(w, *b)?;
            if a == b {
                return Err("a relation needs two parties".into());
            }
            for (x, n) in [(trust, "trust"), (fear, "fear"), (loyalty, "loyalty")] {
                finite(*x, n)?;
            }
            let i = match w.relations.binary_search_by(|r| (r.a, r.b).cmp(&(*a, *b))) {
                Ok(i) => i,
                Err(i) => {
                    w.relations.insert(
                        i,
                        Relation {
                            a: *a,
                            b: *b,
                            trust: 0.0,
                            fear: 0.0,
                            debt: 0.0,
                            loyalty: 0.0,
                            updated_beat: beat,
                        },
                    );
                    i
                }
            };
            let r = &mut w.relations[i];
            r.trust = (r.trust + trust).clamp(-1.0, 1.0);
            r.fear = (r.fear + fear).clamp(-1.0, 1.0);
            r.loyalty = (r.loyalty + loyalty).clamp(-1.0, 1.0);
            r.updated_beat = beat;
            row.targets = vec![*a, *b];
        }
        Effect::Join { member, group } | Effect::Leave { member, group } => {
            let join = matches!(p.effect, Effect::Join { .. });
            need_member(*member)?;
            need_alive(w, *member)?;
            need(w, Ref::Group(*group))?;
            if join && w.groups[group.ix()].dissolved_beat.is_some() {
                return Err(format!("{group} is dissolved"));
            }
            let gs = match *member {
                Ref::Char(c) => &mut w.characters[c.ix()].groups,
                Ref::Cluster(c) => &mut w.clusters[c.ix()].groups,
                _ => unreachable!(),
            };
            if join {
                gs.insert(*group);
            } else if !gs.remove(*group) {
                return Err(format!("{member} is not in {group}"));
            }
            row.targets = vec![*member, Ref::Group(*group)];
        }
        Effect::FoundGroup {
            name,
            kind,
            members,
        } => {
            for m in members {
                need_member(*m)?;
                need_alive(w, *m)?;
            }
            let k = match kind.to_lowercase().as_str() {
                "faction" => GroupKind::Faction,
                "trade" => GroupKind::Trade,
                "faith" => GroupKind::Faith,
                "neighborhood" => GroupKind::Neighborhood,
                "family" => GroupKind::Family,
                _ => GroupKind::Emergent,
            };
            let gid = GroupId::from_ix(w.groups.len());
            w.groups.push(Group {
                id: gid,
                name: name.clone(),
                kind: k,
                permeability: template().permeability.of(k),
                patron: None,
                follows: None,
                founded_beat: beat,
                dissolved_beat: None,
            });
            for m in members {
                match *m {
                    Ref::Char(c) => w.characters[c.ix()].groups.insert(gid),
                    Ref::Cluster(c) => w.clusters[c.ix()].groups.insert(gid),
                    _ => false,
                };
            }
            row.targets = std::iter::once(Ref::Group(gid))
                .chain(members.iter().copied())
                .collect();
        }
        Effect::Memorize { mention, cluster } => {
            let cl = match cluster {
                Some(c) => {
                    need(w, Ref::Cluster(*c))?;
                    *c
                }
                None => match resolve_mention(w, mention, mention.near) {
                    Resolution::Existing { char, .. } => {
                        let ch = &mut w.characters[char.ix()];
                        let txt = mention.text.trim().to_string();
                        if !txt.is_empty() && txt != ch.name && !ch.aliases.contains(&txt) {
                            ch.aliases.push(txt);
                        }
                        row.targets = vec![Ref::Char(char)];
                        return Ok(());
                    }
                    Resolution::Materialize { cluster, .. } => cluster,
                    Resolution::None => return Err(format!("nobody fits {:?}", mention.text)),
                },
            };
            if w.clusters[cl.ix()].size == 0 {
                return Err(format!("{cl} is empty"));
            }
            let ch = materialize(w, cl, mention, beat, Some(p.actor));
            row.targets = vec![Ref::Char(ch), Ref::Cluster(cl)];
        }
        Effect::CreateItem {
            name,
            description,
            holder,
        } => {
            need(w, *holder)?;
            let origin = match p.source {
                Source::God { god, .. } => ItemOrigin::Granted { god },
                _ => ItemOrigin::Summoned,
            };
            // Schema 15: never an entity's name (a resolver's "Bot Kestrel's Right Hand: finds a rusted knife ...").
            let name = if w.item_name_rules() { thing_name(w, name, description) } else { name.clone() };
            let iid = new_item(
                w,
                name.clone(),
                description.clone(),
                origin,
                beat,
                p.actor,
                *holder,
            );
            // Rules 0.7.0: things made outside the item engine (a god's gift, a resolver's find) still get a kind; they
            // are trinkets.
            if w.two_bodies() {
                let kind = fn7_model::manifest::classify_kind(&format!("{name} {description}"));
                let it = &mut w.items[iid.ix()];
                it.ext.kind = kind;
                it.ext.unit = kind.unit().to_string();
            }
            row.targets = vec![Ref::Item(iid), *holder];
        }
        Effect::Echo { item, holder } => {
            need(w, Ref::Item(*item))?;
            need(w, *holder)?;
            let payer = demigod_of(w, p.actor)
                .or(match holder {
                    Ref::Demigod(d) => Some(*d),
                    _ => None,
                })
                .ok_or("echo needs a demigod's charge")?;
            need_alive(w, Ref::Demigod(payer))?;
            if w.demigods[payer.ix()].echo_charges == 0 {
                return Err(format!("{payer} has no echo charges"));
            }
            w.demigods[payer.ix()].echo_charges -= 1;
            let src = &mut w.items[item.ix()];
            src.uses += 1;
            let name = format!("{} (echo)", src.name.trim_end_matches(" (echo)"));
            let desc = src.description.clone();
            let iid = new_item(
                w,
                name,
                desc,
                ItemOrigin::Echoed { from: *item },
                beat,
                p.actor,
                *holder,
            );
            row.targets = vec![Ref::Item(iid), Ref::Item(*item), *holder];
        }
        Effect::Favor {
            god,
            demigod,
            delta,
        } => {
            finite(*delta, "delta")?;
            need(w, Ref::God(*god))?;
            need(w, Ref::Demigod(*demigod))?;
            let n = w.demigods.len();
            let f = &mut w.gods[god.ix()].favor;
            if f.len() < n {
                f.resize(n, 0.0);
            }
            f[demigod.ix()] = (f[demigod.ix()] + delta).clamp(-10.0, 10.0);
            row.targets = vec![Ref::God(*god), Ref::Demigod(*demigod)];
        }
        Effect::Debt {
            owed_by,
            god,
            amount,
            due_in,
        } => {
            nonneg(*amount, "debt")?;
            need(w, *owed_by)?;
            need(w, Ref::God(*god))?;
            row.debt = Some(new_debt(*owed_by, *god, *amount, beat + due_in));
            row.targets = vec![*owed_by];
        }
        Effect::Delay { row: r, beats } => {
            delay_debt(w, *r, *beats)?;
            let owed = w.ledger.rows[r.ix()].debt.map(|d| d.owed_by);
            row.targets = std::iter::once(Ref::Ledger(*r)).chain(owed).collect();
            row.cause = row.cause.or(Some(*r));
        }
        Effect::Collect { row: r } => {
            let d = w
                .ledger
                .get(*r)
                .ok_or_else(|| format!("{r} does not exist"))?
                .debt
                .ok_or_else(|| format!("{r} has no debt"))?;
            if !matches!(d.status, DebtStatus::Open | DebtStatus::Delayed) {
                return Err(format!("{r}'s debt is already settled"));
            }
            if is_alive(w, d.owed_by) {
                if let Some(h) = health_of(w, d.owed_by) {
                    set_health(
                        w,
                        d.owed_by,
                        h - d.amount,
                        beat,
                        "a debt collected by Consequence",
                    );
                } else if let Ref::Cluster(c) = d.owed_by {
                    let cl = &mut w.clusters[c.ix()];
                    cl.size = cl
                        .size
                        .saturating_sub(((cl.size as f32) * d.amount / 100.0).ceil() as u32);
                }
            }
            let debt = w.ledger.rows[r.ix()].debt.as_mut().expect("checked");
            debt.status = if beat >= w.meta.beats_total {
                DebtStatus::Settled
            } else {
                DebtStatus::Collected
            };
            debt.collected_by = Some(row.id);
            row.targets = vec![d.owed_by, Ref::Ledger(*r)];
            row.cause = row.cause.or(Some(*r));
        }
        Effect::Rewind { target, beats } => {
            need_alive(w, *target)?;
            let frame = w
                .rewind
                .at(beat.saturating_sub(*beats))
                .ok_or("nothing to rewind to")?;
            let e = frame
                .entries
                .iter()
                .find(|e| e.who == *target)
                .cloned()
                .ok_or("target not in the rewind frame")?;
            if let Some(pos) = e.pos {
                let pl = place_at(w, pos);
                match *target {
                    Ref::Demigod(d) => {
                        w.demigods[d.ix()].pos = pos;
                        w.demigods[d.ix()].place = pl;
                    }
                    Ref::Char(c) => {
                        w.characters[c.ix()].pos = pos;
                        w.characters[c.ix()].place = pl;
                    }
                    _ => {}
                }
            }
            if let Some(h) = e.health.filter(|h| *h > 0.0) {
                set_health(w, *target, h, beat, "");
            }
            row.targets = vec![*target];
        }
        Effect::Price {
            who,
            god,
            kind,
            amount,
        } => {
            nonneg(*amount, "price")?;
            need_alive(w, *who)?;
            need(w, Ref::God(*god))?;
            match kind {
                // Years of life: the cap drops (health clamps to it; a cap of 0 kills).
                PriceKind::Life => {
                    if let (Some(h), Some(m)) = (health_of(w, *who), max_health_of(w, *who)) {
                        let m = (m - amount).max(0.0);
                        match *who {
                            Ref::Demigod(d) => w.demigods[d.ix()].max_health = m,
                            Ref::Char(c) => w.characters[c.ix()].max_health = m,
                            _ => {}
                        }
                        set_health(w, *who, h.min(m), beat, "a price paid in years");
                    }
                }
                PriceKind::Debt => {
                    row.debt = Some(new_debt(*who, *god, *amount, beat + PRICE_DEBT_DUE))
                }
                // Soul, Forget, Wild, Clause: the god (or memory crate) settles them later; the row is the record.
                _ => {}
            }
            row.targets = vec![*who, Ref::God(*god)];
        }
        Effect::Forget { owner, about } | Effect::Plant { owner, about, .. } => {
            need(w, *owner)?;
            for a in about {
                need(w, *a)?;
            }
            row.targets = std::iter::once(*owner)
                .chain(about.iter().copied())
                .collect();
        }
        Effect::Tell { to, row: r } => {
            need(w, *to)?;
            need(w, Ref::Ledger(*r))?;
            let told = &mut w.ledger.rows[r.ix()].told_to;
            if !told.contains(to) {
                told.push(*to);
            }
            row.told_to = vec![*to];
            row.targets = vec![*to, Ref::Ledger(*r)];
            row.cause = row.cause.or(Some(*r));
        }
        Effect::Dispatch {
            god,
            mission,
            target,
            paladin,
        } => {
            need(w, Ref::God(*god))?;
            need(w, *target)?;
            let ch = match paladin.and_then(|c| retask_paladin(w, *god, c, mission, *target, beat))
            {
                Some(c) => c,
                None => spawn_paladin(w, *god, mission, *target, beat),
            };
            row.targets = vec![Ref::Char(ch), *target];
        }
        Effect::Eliminate { who, cause } => {
            need_body(w, *who)?;
            kill(w, *who, beat, cause);
            row.targets = vec![*who];
        }
        Effect::Revive { who } => {
            need(w, *who)?;
            if !matches!(who, Ref::Demigod(_) | Ref::Char(_)) {
                return Err(format!("{who} has no body"));
            }
            if is_alive(w, *who) {
                return Err(format!("{who} is alive"));
            }
            let m = max_health_of(w, *who).unwrap_or(MAX_HEALTH);
            if m <= 0.0 {
                return Err(format!("{who} has no years left to return to"));
            }
            let h = (m * REVIVE_FRACTION).max(0.1).min(m);
            match *who {
                Ref::Demigod(d) => {
                    let x = &mut w.demigods[d.ix()];
                    x.alive = true;
                    x.health = h;
                    x.eliminated_beat = None;
                    x.cause = None;
                    // The hands come back with their demigod (subagents stay ended).
                    for a in x.hands {
                        if let Some(ag) = w.agents.get_mut(a.ix()) {
                            if ag.status == AgentStatus::Ended {
                                ag.status = AgentStatus::Live;
                                ag.ended_beat = None;
                            }
                        }
                    }
                }
                Ref::Char(c) => {
                    let x = &mut w.characters[c.ix()];
                    x.alive = true;
                    x.health = h;
                }
                _ => unreachable!(),
            }
            row.targets = vec![*who];
        }
        Effect::Tempo { who, factor, beats } => {
            finite(*factor, "tempo factor")?;
            if !(TEMPO_MIN..=TEMPO_MAX).contains(factor) {
                return Err(format!(
                    "tempo factor {factor} is outside {TEMPO_MIN}..{TEMPO_MAX}"
                ));
            }
            if *beats == 0 || *beats > TEMPO_MAX_BEATS {
                return Err(format!(
                    "tempo lasts 1..{TEMPO_MAX_BEATS} beats, not {beats}"
                ));
            }
            need_body(w, *who)?;
            let t = Tempo {
                factor: *factor,
                until_beat: beat + beats,
            };
            let v = match *who {
                Ref::Demigod(d) => &mut w.demigods[d.ix()].tempo,
                Ref::Char(c) => &mut w.characters[c.ix()].tempo,
                _ => unreachable!(),
            };
            v.retain(|x| x.until_beat > beat);
            v.push(t);
            row.targets = vec![*who];
        }
        Effect::Note { .. } => {}
        Effect::Society { refs, at, place, .. } => {
            for r in refs {
                need(w, *r)?;
            }
            if !(at[0].is_finite() && at[1].is_finite() && (-90.0..=90.0).contains(&at[1])) {
                return Err("bad position".into());
            }
            if let Some(pl) = place {
                need(w, Ref::Place(*pl))?;
            }
            row.targets = refs.iter().take(6).copied().collect();
            row.at = Some(*at);
            row.place = *place;
        }
        Effect::Orders { army, by, text, judgment, why, llm } => {
            need(w, Ref::Cluster(*army))?;
            need(w, Ref::Demigod(*by))?;
            if w.clusters[army.ix()].army.as_ref().is_none_or(|a| !w.same_player(a.demigod, *by) || a.rogue) {
                return Err(format!("{army} does not take orders from {by}"));
            }
            row.targets = crate::sim::armies::apply_orders(w, *army, *by, text, judgment, why, *llm, beat)?;
        }
        Effect::Promote { who, by, role, army } => {
            need_alive(w, Ref::Demigod(*by))?;
            row.targets = crate::sim::people::promote(w, *who, *by, *role, *army, beat)?;
        }
        Effect::Happen { at, .. } => {
            if !(at[0].is_finite() && at[1].is_finite() && (-90.0..=90.0).contains(&at[1])) {
                return Err("bad position".into());
            }
            row.at = Some(*at);
            row.place = Some(place_at(w, *at));
        }
        Effect::Cast {
            by,
            power: _,
            at,
            target,
            cost,
            damage,
            scale,
            radius_km,
        } => {
            need_alive(w, Ref::Demigod(*by))?;
            if !(at[0].is_finite() && at[1].is_finite() && (-90.0..=90.0).contains(&at[1])) {
                return Err("bad position".into());
            }
            nonneg(*damage, "damage")?;
            match scale {
                // Rules 0.7.0: the 1:5 economy (a bolt per mega, a spark per mini).
                Some(s) => pay_charge(w, *by, *s, beat)?,
                None => {
                    let have = w.demigods[by.ix()].bolts;
                    if have < *cost {
                        return Err(format!("{by} has {have} bolts, the power costs {cost}"));
                    }
                    spend_bolts(w, *by, *cost, beat, true);
                }
            }
            // A mega's area: every other rival demigod within its radius takes a share of the damage.
            if *scale == Some(CastScale::Mega) && *damage > 0.0 {
                let hit: Vec<DemigodId> = w
                    .demigods
                    .iter()
                    .filter(|d| d.alive && !w.same_player(d.id, *by) && Some(Ref::Demigod(d.id)) != *target)
                    .filter(|d| crate::geo::distance_km(d.pos, *at) <= *radius_km as f64)
                    .map(|d| d.id)
                    .collect();
                for d in hit {
                    let h = w.demigods[d.ix()].health - damage * MEGA_AREA_SHARE;
                    set_health(w, Ref::Demigod(d), h, beat, "caught in a mega power's blast");
                    row.targets.push(Ref::Demigod(d));
                }
            }
            if let Some(t) = target {
                need(w, *t)?;
                if is_alive(w, *t) {
                    match *t {
                        Ref::Demigod(_) | Ref::Char(_) => {
                            let h = health_of(w, *t).unwrap_or(0.0) - damage;
                            set_health(w, *t, h, beat, "struck down by a god power");
                        }
                        Ref::Cluster(c) => {
                            let cl = &mut w.clusters[c.ix()];
                            let share = if cl.army.is_some() { 0.04 } else { 0.01 } * damage;
                            cl.size = cl
                                .size
                                .saturating_sub(((cl.size as f32) * share.min(0.6)).ceil() as u32);
                            cl.mood.fear = (cl.mood.fear + 0.05 * damage).min(1.0);
                        }
                        _ => {}
                    }
                }
            }
            let mut t: Vec<Ref> = target.iter().copied().collect();
            t.append(&mut row.targets);
            row.targets = t;
            row.at = Some(*at);
            row.place = Some(place_at(w, *at));
        }
        Effect::Bolts { who, delta, to, toll } => {
            need(w, Ref::Demigod(*who))?;
            if let Some(g) = to {
                need(w, Ref::God(*g))?;
            }
            if *delta < 0 {
                let n = delta.unsigned_abs();
                let have = w.demigods[who.ix()].bolts;
                if have < n {
                    return Err(format!("{who} has {have} bolts, not {n}"));
                }
                spend_bolts(w, *who, n, beat, !*toll);
            } else if *delta > 0 {
                let b = (w.demigods[who.ix()].bolts + *delta as u32).min(MAX_BOLTS);
                w.set_bolts(*who, b);
                raise_entropy(w, *delta as f32 * ENTROPY_PER_GRANTED_BOLT, beat);
            }
            row.targets = std::iter::once(Ref::Demigod(*who))
                .chain(to.map(Ref::God))
                .collect();
        }
        Effect::Entropy { delta } => {
            finite(*delta, "entropy")?;
            raise_entropy(w, *delta, beat);
        }
        Effect::RaiseArmy {
            demigod,
            from,
            size,
            name,
            orders,
        } => {
            need_alive(w, Ref::Demigod(*demigod))?;
            need(w, Ref::Cluster(*from))?;
            let id = crate::sim::armies::create_army(w, *demigod, *from, *size, name, orders, beat, false)?;
            row.targets = vec![Ref::Cluster(id), Ref::Cluster(*from)];
        }
        Effect::March {
            army,
            to,
            dest,
            target,
            orders,
        } => {
            need(w, Ref::Cluster(*army))?;
            if !(to[0].is_finite() && to[1].is_finite() && (-90.0..=90.0).contains(&to[1])) {
                return Err("bad position".into());
            }
            let cl = &w.clusters[army.ix()];
            let Some(a) = &cl.army else {
                return Err(format!("{army} is not an army"));
            };
            if cl.size == 0 {
                return Err(format!("{army} has no one left"));
            }
            if crate::geo::distance_km(a.pos, *to) > ARMY_KM_PER_BEAT * 1.05 {
                return Err("an army marches at most ARMY_KM_PER_BEAT a beat".into());
            }
            let pl = place_at(w, *to);
            let cl = &mut w.clusters[army.ix()];
            let a = cl.army.as_mut().expect("checked");
            a.pos = *to;
            if dest.is_some() || target.is_some() {
                a.dest = *dest;
                a.target = *target;
            }
            if let Some(o) = orders {
                a.orders = o.chars().take(200).collect();
            }
            cl.place = pl;
            row.targets = std::iter::once(Ref::Cluster(*army)).chain(*target).collect();
            row.at = Some(*to);
            row.place = Some(pl);
        }
        Effect::Manifest { manifestation, item } => {
            let by = manifestation.demigod;
            need(w, Ref::Demigod(by))?;
            let id = fn7_model::manifest::ManifestationId::from_ix(w.manifests.len());
            let mut m = (**manifestation).clone();
            m.id = id;
            m.rows.insert(0, row.id);
            row.targets = vec![Ref::Demigod(by)];
            row.at = m.at;
            row.place = m.place;
            if let Some(new) = item.as_ref().filter(|_| m.verdict.outcome.made()) {
                let holder = Ref::Demigod(by);
                let iid = new_item(w, new.name.clone(), new.description.clone(), ItemOrigin::Summoned, beat, p.actor, holder);
                let mut ext = new.ext.clone();
                ext.manifestation = Some(id);
                ext.history.push(fn7_model::manifest::ItemEvent {
                    beat,
                    state: ext.state,
                    holder: Some(holder),
                    by: Some(p.actor),
                    row: Some(row.id),
                    count: Some(ext.count),
                });
                if new.muster > 0 && ext.kind.is_host() && w.demigods[by.ix()].alive {
                    let army = crate::sim::armies::muster_army(w, by, new.muster, &new.name, beat);
                    ext.muster = army;
                }
                w.items[iid.ix()].ext = ext;
                m.item = Some(iid);
                row.targets.insert(0, Ref::Item(iid));
            }
            w.manifests.push(m);
        }
        Effect::Wound { god, amount } => {
            need(w, Ref::God(*god))?;
            nonneg(*amount, "wound")?;
            let g = &mut w.gods[god.ix()];
            g.health = (g.health - amount).max(0.0);
            row.targets = vec![Ref::God(*god)];
        }
        Effect::Mod { m } => {
            if !(m.at[0].is_finite() && m.at[1].is_finite() && m.radius_km.is_finite()) {
                return Err("bad mod position".into());
            }
            need(w, Ref::Place(m.region))?;
            let key = m.name.trim().to_lowercase();
            if let Some(x) = w
                .mods
                .iter_mut()
                .find(|x| x.region == m.region && x.name.trim().to_lowercase() == key)
            {
                x.stacks = (x.stacks + 1).min(MOD_MAX_STACKS);
                x.expires_beat = match (x.expires_beat, m.expires_beat) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    _ => None,
                };
                row.at = Some(x.at);
            } else {
                if w.mods.len() >= MAX_MODS {
                    let oldest = w
                        .mods
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, x)| (x.since_beat, x.id))
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    w.mods.remove(oldest);
                }
                let mut m = m.clone();
                m.id = w.mod_seq;
                m.since_beat = beat;
                m.stacks = m.stacks.max(1);
                m.radius_km = m.radius_km.clamp(10.0, 3000.0);
                w.mod_seq += 1;
                row.at = Some(m.at);
                w.mods.push(m);
            }
            row.targets = vec![Ref::Place(m.region)];
        }
        Effect::Unmod { id } => {
            let i = w
                .mods
                .iter()
                .position(|x| x.id == *id)
                .ok_or_else(|| format!("no world mod {id}"))?;
            let m = w.mods.remove(i);
            row.targets = vec![Ref::Place(m.region)];
            row.at = Some(m.at);
        }
    }
    Ok(())
}

/// Bolts every other live demigod gains per bolt spent.
pub const BOLT_GIFT: u32 = 2;
/// Entropy per bolt spent anywhere.
pub const ENTROPY_PER_BOLT: f32 = 0.4;
/// Entropy per bolt Entropy hands out.
pub const ENTROPY_PER_GRANTED_BOLT: f32 = 0.2;
/// Most bolts one demigod can hold.
pub const MAX_BOLTS: u32 = 99;
/// Most stacks of one world mod.
pub const MOD_MAX_STACKS: u32 = 5;

/// `who` pays `n` bolts: every other live demigod gains [`BOLT_GIFT`] each (unless `gift` is false: Time's toll for
/// an order or a slow window), Entropy rises either way. Assumes validated.
///
/// Bolts are a per-player purse (rules 0.7.0): every demigod of a seat holds the same count ([`World::set_bolts`]),
/// and the gift goes once to every *other player* with a demigod standing. With one demigod per seat (schema 4) that
/// is exactly "every other live demigod".
fn spend_bolts(w: &mut World, who: DemigodId, n: u32, beat: u32, gift: bool) {
    let left = w.demigods[who.ix()].bolts - n;
    w.set_bolts(who, left);
    if gift {
        let payer = w.demigods[who.ix()].seat;
        let mut done: Vec<u8> = vec![payer];
        for i in 0..w.demigods.len() {
            let d = &w.demigods[i];
            if !d.alive || done.contains(&d.seat) {
                continue;
            }
            done.push(d.seat);
            let b = (d.bolts + BOLT_GIFT * n).min(MAX_BOLTS);
            w.set_bolts(d.id, b);
        }
    }
    raise_entropy(w, n as f32 * ENTROPY_PER_BOLT, beat);
}

/// Rules 0.7.0: pay one power charge. A mega costs a bolt and every other player (with a demigod standing) gains the
/// room's `bolt_gift` bolts (1) and `spark_gift` sparks (0), once; Entropy rises `entropy_per_mega` (10). A mini costs
/// a spark, gifts nothing, and raises Entropy `entropy_per_mini` (1).
fn pay_charge(w: &mut World, by: DemigodId, scale: CastScale, beat: u32) -> Result<(), String> {
    let set = |w: &World, k: &str, d: f64| crate::rules::setting_f64(w, k, d);
    match scale {
        CastScale::Mega => {
            let have = w.demigods[by.ix()].bolts;
            if have < 1 {
                return Err(format!("{by} has no bolt for a mega power"));
            }
            let (bg, sg, e) =
                (set(w, "bolt_gift", 1.0) as u32, set(w, "spark_gift", 0.0) as u32, set(w, "entropy_per_mega", 10.0) as f32);
            w.set_bolts(by, have - 1);
            let payer = w.demigods[by.ix()].seat;
            let mut done = vec![payer];
            for i in 0..w.demigods.len() {
                let d = &w.demigods[i];
                if !d.alive || done.contains(&d.seat) {
                    continue;
                }
                done.push(d.seat);
                let (id, b, s) = (d.id, (d.bolts + bg).min(MAX_BOLTS), (d.sparks + sg).min(MAX_BOLTS));
                w.set_bolts(id, b);
                w.set_sparks(id, s);
            }
            raise_entropy(w, e, beat);
        }
        CastScale::Mini => {
            let have = w.demigods[by.ix()].sparks;
            if have < 1 {
                return Err(format!("{by} has no spark for a mini power"));
            }
            w.set_sparks(by, have - 1);
            let e = set(w, "entropy_per_mini", 1.0) as f32;
            raise_entropy(w, e, beat);
        }
    }
    Ok(())
}

/// Move the Entropy meter (clamped 0..=ENTROPY_MAX). Reaching the maximum is heat death: this beat becomes the Final
/// Night (`beats_total` = `beat`), once.
pub fn raise_entropy(w: &mut World, delta: f32, beat: u32) {
    // The room's `entropy_scale` (a playtest dial, default 1) multiplies every rise; cooling is never scaled.
    let scale = w
        .meta
        .settings
        .get("entropy_scale")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(1.0) as f32;
    let delta = if delta > 0.0 { delta * scale } else { delta };
    w.meta.entropy = ((w.meta.entropy + delta).clamp(0.0, ENTROPY_MAX) * 100.0).round() / 100.0;
    if w.meta.entropy >= ENTROPY_MAX && w.meta.heat_death.is_none() {
        w.meta.heat_death = Some(beat);
        w.meta.beats_total = w.meta.beats_total.min(beat.max(1));
    }
}

fn new_debt(owed_by: Ref, god: GodId, amount: f32, due: u32) -> Debt {
    Debt {
        owed_by,
        owed_to: god,
        amount: amount.min(10.0),
        due_beat: due,
        delays: 0,
        status: DebtStatus::Open,
        collected_by: None,
    }
}

/// The name a made thing takes (schema 15): `name` itself, unless it is empty, names an entity
/// ([`World::names_entity`]) or is only the start of its own description (a step's act cut to 40 characters); then the
/// object its description describes ([`fn7_model::manifest::object_name`]), else `"Curious Keepsake"`.
pub fn thing_name(w: &World, name: &str, description: &str) -> String {
    let n = name.trim();
    let d = description.trim();
    // A name cut from its own description (`line` ends a cut with "…").
    let head = n.trim_end_matches('…').trim_end();
    let sentence = !head.is_empty() && (d.len() > head.len() && d.starts_with(head) || (n == d && n.split_whitespace().count() > 5));
    if !w.names_entity(n) && !sentence {
        return n.to_string();
    }
    [d, n]
        .iter()
        .filter_map(|t| fn7_model::manifest::object_name(t))
        .find(|o| !w.names_entity(o))
        .unwrap_or_else(|| "Curious Keepsake".to_string())
}

fn new_item(
    w: &mut World,
    name: String,
    description: String,
    origin: ItemOrigin,
    beat: u32,
    by: Ref,
    holder: Ref,
) -> ItemId {
    let iid = ItemId::from_ix(w.items.len());
    w.items.push(Item {
        id: iid,
        name,
        description,
        origin,
        created_beat: beat,
        created_by: Some(by),
        holders: vec![holder],
        uses: 0,
        ext: Default::default(),
    });
    if let Ref::Demigod(d) = holder {
        w.demigods[d.ix()].items.push(iid);
    }
    iid
}

/// Send an existing paladin on a new mission: it must be alive and serve `god`. Its mind (re)lives with the new goal
/// and the target as its scope. `None` when it can't serve (dead, someone else's, no mind): a new one is raised instead.
fn retask_paladin(
    w: &mut World,
    god: GodId,
    c: CharId,
    mission: &str,
    target: Ref,
    beat: u32,
) -> Option<CharId> {
    let ch = w.characters.get(c.ix())?;
    if !ch.alive || ch.serves != Some(god) {
        return None;
    }
    let a = w
        .agents
        .iter_mut()
        .find(|a| matches!(a.kind, AgentKind::Paladin { god: g, body } if g == god && body == c))?;
    a.goal = mission.to_string();
    a.scope = vec![target];
    if a.status != AgentStatus::Live {
        a.status = AgentStatus::Live;
        a.ended_beat = None;
        a.spawned_beat = beat;
    }
    Some(c)
}

/// A god's paladin: a memorized body (Origin::Paladin) plus its mind (AgentKind::Paladin), near the target.
fn spawn_paladin(w: &mut World, god: GodId, mission: &str, target: Ref, beat: u32) -> CharId {
    let t = template();
    let gs = &w.gods[god.ix()];
    let n = gs.paladins.len() as u64;
    let god_agent = gs.agent;
    let god_name = w.name_of(Ref::God(god));
    let mut rng = Rng::derive(w.meta.seed, &[hash_str("paladin"), god.0 as u64, n]);
    let given = rng
        .pick(&t.names.paladin)
        .cloned()
        .unwrap_or_else(|| "Warden".into());
    let center = pos_of(w, target).unwrap_or_else(|| w.places[w.meta.arena.ix()].center);
    let pos = random_in(&mut rng, center, 0.5);
    let place = place_at(w, pos);
    let cid = CharId::from_ix(w.characters.len());
    let aid = AgentId::from_ix(w.agents.len());
    let name = format!("{given}, Paladin of {god_name}");
    w.characters.push(Character {
        id: cid,
        name: name.clone(),
        aliases: vec![format!("{god_name}'s paladin"), given.clone()],
        origin: Origin::Paladin { god },
        place,
        pos,
        traits: vec!["relentless".into(), "divine".into()],
        groups: GroupSet::default(),
        health: MAX_HEALTH,
        max_health: MAX_HEALTH,
        alive: true,
        allegiance: vec![],
        serves: Some(god),
        possessed_by: None,
        memorized_beat: beat,
        memorized_by: Some(Ref::God(god)),
        tempo: vec![],
        life: None,
    });
    w.agents.push(Agent {
        id: aid,
        kind: AgentKind::Paladin { god, body: cid },
        name,
        persona: format!("A paladin of {god_name}."),
        goal: mission.to_string(),
        scope: vec![target],
        spawned_beat: beat,
        budget_beats: None,
        status: AgentStatus::Live,
        ended_beat: None,
        report: None,
        steps: 0,
        children: vec![],
    });
    w.gods[god.ix()].paladins.push(cid);
    if let Some(a) = w.agents.get_mut(god_agent.ix()) {
        a.children.push(aid);
    }
    cid
}

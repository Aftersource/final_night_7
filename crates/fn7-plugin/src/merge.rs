//! The merge: every proposal of the beat → one decision, then the survivors are applied (one ledger row each), with
//! every decision and side effect recorded.
//!
//! Rules, in the order they run (the first rule to decide a proposal wins):
//! 1. `time_delay_beats_collect`: a Time `Delay` and a Consequence `Collect` on the same debt row → the collection is
//!    overruled; the delay applies and the debt grows by `fn7_society::DELAY_GROWTH`.
//! 2. `rewind_undoes`: a Time `Rewind` on a target overrules this beat's Nature and Chance proposals on that target,
//!    except debts and prices (a rewind never removes a debt).
//! 3. `precedence`: god proposals that pull one target opposite ways (heal/revive vs damage/eliminate) from different
//!    gods → the lower pantheon `precedence` is overruled; on equal precedence `chance_breaks_tie` (a seeded coin).
//! 4. `damage_median`: several non-rules `Damage` proposals on one target → one hit at the median amount (kept on the
//!    first proposal; the others are `Merged` into it).
//!
//! Before applying, `out_of_reach`: a body's damage to a body in another district is invalid (go there first).
//! Applying: `fn7_society::apply`; `Err` → `Invalid` with the reason. Memory's `Forget`/`Plant` run through
//! `fn7_memory`. Every proposal yields `proposal` + `resolution` events; applied ones a `ledger_row` (with the effect),
//! plus `rewind`, `debt_delayed`, `debt_collected`, `memorized`, `memory_write`, `memory_forget`, `agent_spawned`,
//! `agent_ended` as they happen.

use fn7_model::seed::{hash_str, Rng};
use fn7_model::*;
use serde_json::json;

use crate::world::event;

#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Apply,
    Merged { into: usize },
    Overruled { by: usize, rule: &'static str },
}

/// The body or thing an effect lands on (for conflicts).
pub fn target_of(e: &Effect) -> Option<Ref> {
    Some(match e {
        Effect::Damage { who, .. }
        | Effect::Heal { who, .. }
        | Effect::Revive { who }
        | Effect::Eliminate { who, .. }
        | Effect::Tempo { who, .. }
        | Effect::Move { who, .. }
        | Effect::Price { who, .. }
        | Effect::Allegiance { who, .. } => *who,
        Effect::Rewind { target, .. } => *target,
        Effect::Forget { owner, .. } | Effect::Plant { owner, .. } => *owner,
        Effect::Shrink { cluster, .. } | Effect::MoodShift { cluster, .. } => {
            Ref::Cluster(*cluster)
        }
        Effect::Relation { a, .. } => *a,
        Effect::Echo { holder, .. } | Effect::CreateItem { holder, .. } => *holder,
        Effect::Debt { owed_by, .. } => *owed_by,
        Effect::Tell { to, .. } => *to,
        Effect::Dispatch { target, .. } => *target,
        Effect::Join { member, .. } | Effect::Leave { member, .. } => *member,
        _ => return None,
    })
}

fn direction(e: &Effect) -> i8 {
    match e {
        Effect::Heal { .. } | Effect::Revive { .. } => 1,
        Effect::Damage { .. } | Effect::Eliminate { .. } | Effect::Shrink { .. } => -1,
        _ => 0,
    }
}

fn god_of(p: &Proposal) -> Option<GodId> {
    match p.source {
        Source::God { god, .. } => Some(god),
        _ => None,
    }
}

/// Decide every proposal (pure).
pub fn decide(props: &[Proposal], pantheon: &Pantheon, seed: u64, beat: u32) -> Vec<Decision> {
    let n = props.len();
    let mut d = vec![Decision::Apply; n];
    let slug = |p: &Proposal| {
        god_of(p)
            .and_then(|g| pantheon.get(g))
            .map(|g| g.slug.as_str())
    };
    // 1. Time's delay beats Consequence's collection.
    for i in 0..n {
        if let Effect::Collect { row } = props[i].effect {
            if let Some(j) = (0..n)
                .find(|&j| matches!(props[j].effect, Effect::Delay { row: r, .. } if r == row))
            {
                d[i] = Decision::Overruled {
                    by: j,
                    rule: "time_delay_beats_collect",
                };
            }
        }
    }
    // 2. A rewind undoes Nature and Chance on its target this beat (never debts or prices).
    for j in 0..n {
        let Effect::Rewind { target, .. } = props[j].effect else {
            continue;
        };
        if slug(&props[j]) != Some("time") {
            continue;
        }
        for i in 0..n {
            if d[i] != Decision::Apply || i == j {
                continue;
            }
            let undoable = matches!(slug(&props[i]), Some("nature") | Some("chance"))
                && !matches!(props[i].effect, Effect::Debt { .. } | Effect::Price { .. })
                && target_of(&props[i].effect) == Some(target);
            if undoable {
                d[i] = Decision::Overruled {
                    by: j,
                    rule: "rewind_undoes",
                };
            }
        }
    }
    // 3. Gods pulling one target opposite ways: precedence, then Chance's coin.
    for i in 0..n {
        for j in (i + 1)..n {
            if d[i] != Decision::Apply || d[j] != Decision::Apply {
                continue;
            }
            let (Some(gi), Some(gj)) = (god_of(&props[i]), god_of(&props[j])) else {
                continue;
            };
            if gi == gj {
                continue;
            }
            let (di, dj) = (direction(&props[i].effect), direction(&props[j].effect));
            if di == 0
                || dj == 0
                || di == dj
                || target_of(&props[i].effect) != target_of(&props[j].effect)
            {
                continue;
            }
            let pi = pantheon.get(gi).map(|g| g.precedence).unwrap_or(0);
            let pj = pantheon.get(gj).map(|g| g.precedence).unwrap_or(0);
            let (win, lose, rule) = if pi > pj {
                (i, j, "precedence")
            } else if pj > pi {
                (j, i, "precedence")
            } else if Rng::derive(seed, &[hash_str("tie"), beat as u64, i as u64, j as u64])
                .below(2)
                == 0
            {
                (i, j, "chance_breaks_tie")
            } else {
                (j, i, "chance_breaks_tie")
            };
            d[lose] = Decision::Overruled { by: win, rule };
        }
    }
    // 4. Damage median per target.
    let mut groups: Vec<(Ref, Vec<usize>)> = vec![];
    for i in 0..n {
        if d[i] != Decision::Apply || props[i].source == Source::Rules {
            continue;
        }
        if let Effect::Damage { who, .. } = props[i].effect {
            match groups.iter_mut().find(|g| g.0 == who) {
                Some(g) => g.1.push(i),
                None => groups.push((who, vec![i])),
            }
        }
    }
    for (_, ix) in groups.iter().filter(|g| g.1.len() > 1) {
        for &i in &ix[1..] {
            d[i] = Decision::Merged { into: ix[0] };
        }
    }
    d
}

/// The median damage of a merged group (applied to the kept proposal).
pub fn median_amounts(props: &[Proposal], decisions: &[Decision]) -> Vec<Option<f32>> {
    let mut out = vec![None; props.len()];
    for k in 0..props.len() {
        if decisions[k] != Decision::Apply {
            continue;
        }
        let mut xs: Vec<f32> = (0..props.len())
            .filter(|&i| i == k || decisions[i] == Decision::Merged { into: k })
            .filter_map(|i| match props[i].effect {
                Effect::Damage { amount, .. } => Some(amount),
                _ => None,
            })
            .collect();
        if xs.len() > 1 {
            xs.sort_by(f32::total_cmp);
            let m = xs.len() / 2;
            let med = if xs.len() % 2 == 1 {
                xs[m]
            } else {
                (xs[m - 1] + xs[m]) / 2.0
            };
            out[k] = Some((med * 100.0).round() / 100.0);
        }
    }
    out
}

/// What applying produced beyond the row (for the beat to follow up).
#[derive(Debug, Default)]
pub struct Applied {
    pub resolutions: Vec<MergeResolution>,
    pub new_chars: Vec<CharId>,
    pub new_agents: Vec<AgentId>,
    /// Proposals (by index) that were applied, with their row.
    pub rows: Vec<(usize, LedgerId)>,
}

fn rank(r: &Ref) -> u8 {
    match r {
        Ref::Demigod(_) => 0,
        Ref::Char(_) => 1,
        Ref::Agent(_) => 2,
        Ref::God(_) => 3,
        Ref::Cluster(_) => 4,
        _ => 5,
    }
}

/// Keep at most `max` witnesses: the effect's target and the actor first, then demigods, people, crowds.
pub fn cap_witnesses(p: &mut Proposal, max: usize) {
    if p.witnesses.len() <= max {
        return;
    }
    let t = target_of(&p.effect);
    let actor = p.actor;
    p.witnesses
        .sort_by_key(|r| (Some(*r) != t, *r != actor, rank(r)));
    p.witnesses.truncate(max);
}

/// Apply one proposal (already decided `Apply`) and record everything about it.
pub fn apply_one(
    w: &mut World,
    p: &Proposal,
    idx: usize,
    beat: u32,
    out: &mut Applied,
) -> MergeResolution {
    let chars_before = w.characters.len();
    let agents_before = w.agents.len();
    let live_before: Vec<AgentId> = w
        .agents
        .iter()
        .filter(|a| a.is_live())
        .map(|a| a.id)
        .collect();
    let mod_seq = w.mod_seq;
    let mod_ids: Vec<u32> = w.mods.iter().map(|m| m.id).collect();
    let rewind_frame = match p.effect {
        Effect::Rewind { target, beats } => w
            .rewind
            .at(beat.saturating_sub(beats))
            .and_then(|f| f.entries.iter().find(|e| e.who == target).cloned()),
        _ => None,
    };
    let res = match fn7_society::apply(w, p, beat) {
        Ok(row) => {
            out.rows.push((idx, row));
            let lr = w.ledger.get(row).cloned();
            event(
                "ledger_row",
                Some(p.actor.to_string()),
                json!({ "beat": beat, "row": lr, "effect": p.effect, "proposal": idx }),
            );
            match &p.effect {
                Effect::Forget { owner, about } => {
                    let ids = fn7_memory::forget(w, *owner, about);
                    event(
                        "memory_forget",
                        Some(owner.to_string()),
                        json!({ "beat": beat, "owner": owner, "about": about, "ids": ids, "row": row }),
                    );
                }
                Effect::Plant { owner, text, about } => {
                    let id = fn7_memory::plant(w, *owner, text, about, beat);
                    event(
                        "memory_write",
                        Some(owner.to_string()),
                        json!({ "beat": beat, "items": [w.memory.items[id.ix()]], "row": row }),
                    );
                }
                Effect::Rewind { target, beats } => event(
                    "rewind",
                    Some(target.to_string()),
                    json!({ "beat": beat, "target": target, "beats": beats, "frame": rewind_frame, "row": row }),
                ),
                Effect::Delay { row: r, beats } => event(
                    "debt_delayed",
                    w.ledger
                        .get(*r)
                        .and_then(|x| x.debt)
                        .map(|d| d.owed_by.to_string()),
                    json!({ "beat": beat, "row": r, "beats": beats, "debt": w.ledger.get(*r).and_then(|x| x.debt), "by": row }),
                ),
                Effect::Mod { m } => {
                    let new = w.mod_seq > mod_seq;
                    let live = if new {
                        w.mods.last().cloned()
                    } else {
                        w.mods
                            .iter()
                            .find(|x| x.region == m.region && x.name.trim().eq_ignore_ascii_case(m.name.trim()))
                            .cloned()
                    };
                    for gone in mod_ids.iter().filter(|i| !w.mods.iter().any(|x| x.id == **i)) {
                        event(
                            "mod_expired",
                            None,
                            json!({ "beat": beat, "id": gone, "row": row, "reason": "pushed out by a newer mod" }),
                        );
                    }
                    event(
                        "mod_created",
                        Some(Ref::Place(m.region).to_string()),
                        json!({ "beat": beat, "mod": live, "stacked": !new, "row": row }),
                    );
                }
                Effect::Unmod { id } => event(
                    "mod_expired",
                    None,
                    json!({ "beat": beat, "id": id, "row": row }),
                ),
                Effect::Collect { row: r } => event(
                    "debt_collected",
                    w.ledger
                        .get(*r)
                        .and_then(|x| x.debt)
                        .map(|d| d.owed_by.to_string()),
                    json!({ "beat": beat, "row": r, "debt": w.ledger.get(*r).and_then(|x| x.debt), "by": row }),
                ),
                _ => {}
            }
            for c in chars_before..w.characters.len() {
                let id = CharId::from_ix(c);
                out.new_chars.push(id);
                memorized(w, id, beat);
            }
            for a in agents_before..w.agents.len() {
                let id = AgentId::from_ix(a);
                out.new_agents.push(id);
                event(
                    "agent_spawned",
                    Some(id.to_string()),
                    json!({ "beat": beat, "agent": w.agents[a], "by": p.actor }),
                );
            }
            MergeResolution {
                proposal: idx,
                outcome: MergeOutcome::Applied,
                row: Some(row),
            }
        }
        Err(reason) => MergeResolution {
            proposal: idx,
            outcome: MergeOutcome::Invalid { reason },
            row: None,
        },
    };
    for a in live_before {
        let ag = &w.agents[a.ix()];
        if !ag.is_live() {
            event(
                "agent_ended",
                Some(a.to_string()),
                json!({ "beat": beat, "agent": ag, "status": ag.status, "report": ag.report }),
            );
        }
    }
    res
}

/// A character just entered the world: the `memorized` event and its inherited memories.
pub fn memorized(w: &mut World, id: CharId, beat: u32) {
    let ch = w.characters[id.ix()].clone();
    let (cluster, index, seed) = match ch.origin {
        Origin::Materialized { cluster, index } => (
            Some(cluster),
            Some(index),
            w.clusters.get(cluster.ix()).map(|c| c.seed),
        ),
        _ => (None, None, None),
    };
    event(
        "memorized",
        Some(Ref::Char(id).to_string()),
        json!({ "beat": beat, "character": ch, "cluster": cluster, "index": index, "seed": seed }),
    );
    if let Some(c) = cluster {
        let rows = fn7_society::inheritable_rows(w, c);
        let news = fn7_memory::inherit(w, id, &rows, beat);
        if !news.is_empty() {
            let ids: Vec<MemoryId> = news
                .into_iter()
                .map(|m| fn7_memory::write(&mut w.memory, m))
                .collect();
            let items: Vec<&MemoryItem> = ids.iter().map(|i| &w.memory.items[i.ix()]).collect();
            event(
                "memory_write",
                Some(Ref::Char(id).to_string()),
                json!({ "beat": beat, "items": items, "inherited": true }),
            );
        }
    }
}

/// A body can only hurt a body in its own district (minds and gods reach anywhere).
pub fn out_of_reach(w: &World, p: &Proposal) -> bool {
    let Effect::Damage { who, .. } = p.effect else {
        return false;
    };
    if !matches!(p.source, Source::Resolver | Source::Agent { .. }) {
        return false;
    }
    let body = |r: Ref| matches!(r, Ref::Demigod(_) | Ref::Char(_));
    if !body(p.actor) || !body(who) || p.actor == who {
        return false;
    }
    match (
        fn7_society::place_of(w, p.actor),
        fn7_society::place_of(w, who),
    ) {
        (Some(a), Some(b)) => crate::world::district_of(w, a) != crate::world::district_of(w, b),
        _ => false,
    }
}

/// Decide, then apply every proposal in order, recording `proposal`/`resolution` events.
pub fn run(
    w: &mut World,
    mut props: Vec<Proposal>,
    pantheon: &Pantheon,
    beat: u32,
    max_witnesses: usize,
) -> (Vec<Proposal>, Applied) {
    // Rules 0.9.0: gear and curses shape what goes in (weapons add, armour turns aside, a withered body takes no
    // healing, a sealed one no raising). A no-op unless "wills that matter" is on.
    if crate::wills::on(w) {
        props = crate::wills::arms::shape(w, props, beat);
    }
    for p in props.iter_mut() {
        cap_witnesses(p, max_witnesses);
    }
    let decisions = decide(&props, pantheon, w.meta.seed, beat);
    let medians = median_amounts(&props, &decisions);
    let mut out = Applied::default();
    for i in 0..props.len() {
        event(
            "proposal",
            Some(props[i].actor.to_string()),
            json!({ "beat": beat, "index": i, "proposal": props[i] }),
        );
        let res = match &decisions[i] {
            Decision::Apply if out_of_reach(w, &props[i]) => MergeResolution {
                proposal: i,
                outcome: MergeOutcome::Invalid {
                    reason: "out of reach: the target is in another district".into(),
                },
                row: None,
            },
            Decision::Apply => {
                let mut p = props[i].clone();
                if let (Some(m), Effect::Damage { amount, .. }) = (medians[i], &mut p.effect) {
                    *amount = m;
                }
                apply_one(w, &p, i, beat, &mut out)
            }
            Decision::Merged { into } => MergeResolution {
                proposal: i,
                outcome: MergeOutcome::Merged { into: *into },
                row: None,
            },
            Decision::Overruled { by, rule } => MergeResolution {
                proposal: i,
                outcome: MergeOutcome::Overruled {
                    by: *by,
                    rule: rule.to_string(),
                },
                row: None,
            },
        };
        let rule = match &decisions[i] {
            Decision::Apply if medians[i].is_some() => "damage_median",
            Decision::Apply => match res.outcome {
                MergeOutcome::Invalid { .. } => "invalid",
                _ => "applied",
            },
            Decision::Merged { .. } => "damage_median",
            Decision::Overruled { rule, .. } => rule,
        };
        event(
            "resolution",
            Some(props[i].actor.to_string()),
            json!({ "beat": beat, "resolution": res, "rule": rule }),
        );
        out.resolutions.push(res);
    }
    (props, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prop(source: Source, effect: Effect) -> Proposal {
        Proposal {
            source,
            intent: None,
            effect,
            act: "x".into(),
            actor: Ref::Demigod(DemigodId(0)),
            magnitude: 3,
            valence: 0,
            witnesses: vec![],
            cause: None,
        }
    }

    #[test]
    fn rules() {
        let p = fn7_pantheon::load();
        let g = |s: &str| p.id_of(s).unwrap();
        let god = |s: &str| Source::God {
            god: g(s),
            ruling: None,
        };
        let vex = Ref::Demigod(DemigodId(1));
        let dmg = |a: f32| Effect::Damage {
            who: vex,
            amount: a,
            cause: "c".into(),
        };
        let props = vec![
            prop(Source::Rules, Effect::Collect { row: LedgerId(3) }),
            prop(
                god("time"),
                Effect::Delay {
                    row: LedgerId(3),
                    beats: 2,
                },
            ),
            prop(
                god("time"),
                Effect::Rewind {
                    target: vex,
                    beats: 1,
                },
            ),
            prop(god("nature"), dmg(5.0)),
            prop(
                god("nature"),
                Effect::Debt {
                    owed_by: vex,
                    god: g("nature"),
                    amount: 1.0,
                    due_in: 2,
                },
            ),
            prop(Source::Resolver, dmg(2.0)),
            prop(Source::Resolver, dmg(4.0)),
            prop(Source::Resolver, dmg(9.0)),
            prop(
                god("death"),
                Effect::Heal {
                    who: Ref::Char(CharId(1)),
                    amount: 2.0,
                },
            ),
            prop(
                god("chance"),
                Effect::Damage {
                    who: Ref::Char(CharId(1)),
                    amount: 2.0,
                    cause: "c".into(),
                },
            ),
        ];
        let d = decide(&props, &p, 1, 1);
        assert_eq!(
            d[0],
            Decision::Overruled {
                by: 1,
                rule: "time_delay_beats_collect"
            }
        );
        assert_eq!(d[1], Decision::Apply);
        assert_eq!(
            d[3],
            Decision::Overruled {
                by: 2,
                rule: "rewind_undoes"
            }
        );
        assert_eq!(d[4], Decision::Apply, "a rewind never removes a debt");
        assert_eq!(
            (d[6].clone(), d[7].clone()),
            (Decision::Merged { into: 5 }, Decision::Merged { into: 5 })
        );
        assert_eq!(median_amounts(&props, &d)[5], Some(4.0));
        assert_eq!(
            d[9],
            Decision::Overruled {
                by: 8,
                rule: "precedence"
            },
            "Death (45) outranks Chance (10)"
        );
    }
}

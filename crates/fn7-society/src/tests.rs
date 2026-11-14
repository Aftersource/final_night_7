use std::time::Instant;

use fn7_model::seed::Rng;
use fn7_model::*;

use crate::*;

const PANTHEON: &str = include_str!("../../../data/pantheon.json");

fn pantheon() -> Pantheon {
    serde_json::from_str(PANTHEON).unwrap()
}

fn seats(n: u8) -> Vec<(u8, String)> {
    ["Vex", "Ondine", "Kael", "Mirth", "Sable", "Toma"]
        .iter()
        .take(n as usize)
        .enumerate()
        .map(|(i, s)| (i as u8, s.to_string()))
        .collect()
}

fn world(seed: u64) -> World {
    setup(
        &SetupConfig {
            seed,
            ..Default::default()
        },
        &seats(4),
        &pantheon(),
    )
}

fn mention(text: &str, near: Option<PlaceId>) -> Mention {
    Mention {
        text: text.into(),
        kind: "person".into(),
        near,
    }
}

fn prop(actor: Ref, effect: Effect, witnesses: Vec<Ref>) -> Proposal {
    Proposal {
        source: Source::Resolver,
        intent: None,
        effect,
        act: "something happens".into(),
        actor,
        magnitude: 5,
        valence: -2,
        witnesses,
        cause: None,
    }
}

fn districts(w: &World) -> Vec<&Place> {
    w.places
        .iter()
        .filter(|p| p.kind == PlaceKind::District)
        .collect()
}

#[test]
fn template_parses() {
    let t = template::template();
    assert!(t.districts.len() >= 9 && t.clusters.len() >= 20 && t.names.given.len() >= 30);
    for c in &t.clusters {
        for g in c.groups.iter().chain(c.maybe.iter().map(|m| &m.0)) {
            assert!(
                t.groups.iter().any(|x| &x.key == g),
                "{} refers to unknown group {g}",
                c.key
            );
        }
        for d in &c.districts {
            assert!(
                t.districts.iter().any(|x| &x.key == d),
                "{} refers to unknown district {d}",
                c.key
            );
        }
    }
}

#[test]
fn setup_is_deterministic_and_sane() {
    let a = serde_json::to_string(&world(42)).unwrap();
    let b = serde_json::to_string(&world(42)).unwrap();
    assert_eq!(a, b);
    assert_ne!(a, serde_json::to_string(&world(43)).unwrap());

    let w = world(42);
    let nd = districts(&w).len();
    assert!((6..=9).contains(&nd), "{nd} districts");
    assert!(
        w.places
            .iter()
            .filter(|p| p.kind == PlaceKind::Site)
            .count()
            >= nd
    );
    assert_eq!(w.clusters.len(), 200);
    assert_eq!(w.demigods.len(), 4);
    assert_eq!(w.gods.len(), 7);
    assert_eq!(w.agents.len(), 4 * 2 + 7);
    assert!(
        w.agents
            .iter()
            .filter(|a| matches!(a.kind, AgentKind::Hand { .. }))
            .count()
            == 8
    );
    assert_eq!(w.characters.len(), nd * 2);
    assert!(w.groups.len() >= 30, "{} groups", w.groups.len());
    assert!(w
        .groups
        .iter()
        .filter(|g| g.kind == GroupKind::Faith)
        .all(|g| g.patron.is_some()));
    assert_eq!(
        w.groups
            .iter()
            .filter(|g| g.kind == GroupKind::Following)
            .count(),
        4
    );
    assert_eq!(w.meta.schema, STATE_SCHEMA);
    let pop: u64 = w.clusters.iter().map(|c| c.size as u64).sum();
    let city_pop = embedded_pop(&w.places[0].name);
    assert!(
        pop as f64 > city_pop as f64 * 0.2 && (pop as f64) < city_pop as f64 * 0.6,
        "{pop} of {city_pop}"
    );
    // Labels unique, every cluster has a neighborhood.
    let mut labels: Vec<&str> = w.clusters.iter().map(|c| c.label.as_str()).collect();
    labels.sort();
    labels.dedup();
    assert_eq!(labels.len(), 200);
    for c in &w.clusters {
        assert!(c
            .groups
            .as_slice()
            .iter()
            .any(|g| w.groups[g.ix()].kind == GroupKind::Neighborhood));
        assert!(c.size >= 20);
    }
    println!(
        "city {} · {nd} districts · {} groups · pop {pop}",
        w.places[0].name,
        w.groups.len()
    );
}

fn embedded_pop(name: &str) -> u64 {
    find_city(name).unwrap().population
}

#[test]
fn paris_by_name() {
    let cfg = SetupConfig {
        city: Some("paris".into()),
        ..Default::default()
    };
    let w = setup(&cfg, &seats(2), &pantheon());
    assert_eq!(w.places[0].name, "Paris");
    assert!(
        w.places[0].terrain.contains("Paris"),
        "{}",
        w.places[0].terrain
    );
    for d in districts(&w) {
        assert!(geo::distance_km(d.center, w.places[0].center) < w.places[0].radius_km);
    }
    println!(
        "{:?}",
        districts(&w)
            .iter()
            .map(|d| (&d.name, &d.terrain))
            .collect::<Vec<_>>()
    );
}

#[test]
fn materialize_is_replayable() {
    let mut a = world(5);
    let mut b = world(5);
    let m = mention("a tired dockworker", None);
    let ca = materialize(&mut a, ClusterId(17), &m, 3, None);
    let cb = materialize(&mut b, ClusterId(17), &m, 3, None);
    assert_eq!(a.characters[ca.ix()], b.characters[cb.ix()]);
    assert_eq!(a.clusters[17].materialized, 1);
    let ca2 = materialize(&mut a, ClusterId(17), &m, 4, None);
    assert_ne!(
        a.characters[ca.ix()].name,
        a.characters[ca2.ix()].name.clone() + "x"
    );
    assert_eq!(
        a.characters[ca2.ix()].origin,
        Origin::Materialized {
            cluster: ClusterId(17),
            index: 1
        }
    );
    assert_eq!(a.clusters[17].size, b.clusters[17].size - 1);
    assert_eq!(a.characters[ca.ix()].groups, a.clusters[17].groups);
}

#[test]
fn resolve_existing_and_materialize() {
    let mut w = world(9);
    let d = districts(&w)[1].id;
    // A mention with no match materializes; the same words then resolve to that person.
    let m = mention("the baker who saw the fire", Some(d));
    let cl = match resolve_mention(&w, &m, Some(d)) {
        Resolution::Materialize { cluster, .. } => cluster,
        r => panic!("{r:?}"),
    };
    let ch = materialize(&mut w, cl, &m, 1, None);
    match resolve_mention(&w, &m, Some(d)) {
        Resolution::Existing { char, score } => {
            assert_eq!(char, ch);
            assert!(score >= EXISTING_MIN);
        }
        r => panic!("{r:?}"),
    }
    // By first name alone, and a slightly different phrasing of the alias.
    let name = w.characters[ch.ix()].name.clone();
    let first = name.split(' ').next().unwrap().to_string();
    assert!(
        matches!(resolve_mention(&w, &mention(&name, None), None), Resolution::Existing { char, .. } if char == ch)
    );
    let _ = first;
    assert!(matches!(
        resolve_mention(&w, &mention("The baker who saw the fire!", None), None),
        Resolution::Existing { char, .. } if char == ch
    ));
    // A seeded notable by role alias.
    let notable = w.characters[0].clone();
    let alias = notable.aliases[0].clone();
    assert!(
        matches!(resolve_mention(&w, &mention(&alias, None), None), Resolution::Existing { char, .. } if char == notable.id)
    );
    // Word overlap picks a fitting cluster.
    if let Resolution::Materialize { cluster, .. } =
        resolve_mention(&w, &mention("a student", None), None)
    {
        let c = &w.clusters[cluster.ix()];
        let text = format!(
            "{} {:?}",
            c.label,
            c.groups
                .as_slice()
                .iter()
                .map(|g| &w.groups[g.ix()].name)
                .collect::<Vec<_>>()
        );
        assert!(
            text.to_lowercase().contains("student") || text.contains("Scholars"),
            "{text}"
        );
    }
    assert_eq!(
        resolve_mention(
            &w,
            &Mention {
                text: "the cathedral".into(),
                kind: "place".into(),
                near: None
            },
            None
        ),
        Resolution::None
    );
    // Inheritable rows: a row known by the cluster's groups.
    let g = w.clusters[cl.ix()].groups.as_slice()[0];
    let row = apply(
        &mut w,
        &prop(
            Ref::Demigod(DemigodId(0)),
            Effect::Note { text: "x".into() },
            vec![Ref::Group(g)],
        ),
        1,
    )
    .unwrap();
    assert_eq!(inheritable_rows(&w, cl), vec![row]);
}

#[test]
fn apply_writes_rows_and_enforces_invariants() {
    let mut w = world(11);
    let dg0 = Ref::Demigod(DemigodId(0));
    let ch = Ref::Char(CharId(0));
    let cl = ClusterId(3);
    let wit = vec![Ref::Cluster(cl)];

    // Damage
    let r = apply(
        &mut w,
        &prop(
            dg0,
            Effect::Damage {
                who: ch,
                amount: 4.0,
                cause: "punch".into(),
            },
            wit.clone(),
        ),
        1,
    )
    .unwrap();
    assert_eq!(w.characters[0].health, 6.0);
    let row = &w.ledger.rows[r.ix()];
    assert_eq!(row.verb, "damage");
    assert_eq!(row.targets, vec![ch]);
    assert_eq!(row.known_by, w.clusters[cl.ix()].groups);
    assert!(row.place.is_some());

    // Move
    let to = geo::destination(w.places[0].center, 90.0, 1.0);
    apply(
        &mut w,
        &prop(
            dg0,
            Effect::Move {
                who: dg0,
                to,
                place: None,
            },
            vec![],
        ),
        1,
    )
    .unwrap();
    assert_eq!(w.demigods[0].pos, to);

    // Join (and Leave twice: second fails)
    let g = w
        .groups
        .iter()
        .find(|g| g.kind == GroupKind::Faction && !w.characters[0].groups.contains(g.id))
        .unwrap()
        .id;
    apply(
        &mut w,
        &prop(
            dg0,
            Effect::Join {
                member: ch,
                group: g,
            },
            vec![],
        ),
        1,
    )
    .unwrap();
    assert!(w.characters[0].groups.contains(g));
    apply(
        &mut w,
        &prop(
            dg0,
            Effect::Leave {
                member: ch,
                group: g,
            },
            vec![],
        ),
        1,
    )
    .unwrap();
    assert!(apply(
        &mut w,
        &prop(
            dg0,
            Effect::Leave {
                member: ch,
                group: g
            },
            vec![]
        ),
        1
    )
    .is_err());

    // Debt
    let r = apply(
        &mut w,
        &prop(
            dg0,
            Effect::Debt {
                owed_by: dg0,
                god: GodId(2),
                amount: 3.0,
                due_in: 5,
            },
            vec![],
        ),
        2,
    )
    .unwrap();
    let d = w.ledger.rows[r.ix()].debt.unwrap();
    assert_eq!((d.due_beat, d.amount, d.status), (7, 3.0, DebtStatus::Open));

    // Echo: needs an item and charges
    apply(
        &mut w,
        &prop(
            dg0,
            Effect::CreateItem {
                name: "Lantern".into(),
                description: "a brass lantern".into(),
                holder: dg0,
            },
            vec![],
        ),
        2,
    )
    .unwrap();
    let item = w.demigods[0].items[0];
    for _ in 0..3 {
        apply(
            &mut w,
            &prop(dg0, Effect::Echo { item, holder: dg0 }, vec![]),
            2,
        )
        .unwrap();
    }
    assert_eq!(w.demigods[0].echo_charges, 0);
    assert_eq!(w.items.len(), 4);
    assert!(matches!(w.items[3].origin, ItemOrigin::Echoed { from } if from == item));
    let n = w.ledger.rows.len();
    assert!(apply(
        &mut w,
        &prop(dg0, Effect::Echo { item, holder: dg0 }, vec![]),
        2
    )
    .unwrap_err()
    .contains("echo"));
    assert_eq!(w.ledger.rows.len(), n, "a rejected proposal writes no row");

    // Eliminate, then the dead can't be hurt, moved, or act
    let dg1 = Ref::Demigod(DemigodId(1));
    apply(
        &mut w,
        &prop(
            dg0,
            Effect::Eliminate {
                who: dg1,
                cause: "thrown from the bell tower".into(),
            },
            vec![],
        ),
        3,
    )
    .unwrap();
    assert!(!w.demigods[1].alive && w.demigods[1].eliminated_beat == Some(3));
    assert!(w.agents[w.demigods[1].hands[0].ix()].status == AgentStatus::Ended);
    let n = w.ledger.rows.len();
    assert!(apply(
        &mut w,
        &prop(
            dg0,
            Effect::Damage {
                who: dg1,
                amount: 1.0,
                cause: "x".into()
            },
            vec![]
        ),
        3
    )
    .is_err());
    assert!(apply(
        &mut w,
        &prop(
            dg0,
            Effect::Move {
                who: dg1,
                to,
                place: None
            },
            vec![]
        ),
        3
    )
    .is_err());
    assert!(apply(
        &mut w,
        &prop(dg1, Effect::Note { text: "boo".into() }, vec![]),
        3
    )
    .is_err());
    let hand1 = Ref::Agent(w.demigods[1].hands[0]);
    assert!(apply(
        &mut w,
        &prop(hand1, Effect::Note { text: "boo".into() }, vec![]),
        3
    )
    .is_err());
    // Missing targets, cluster below zero
    assert!(apply(
        &mut w,
        &prop(
            dg0,
            Effect::Damage {
                who: Ref::Char(CharId(9999)),
                amount: 1.0,
                cause: "x".into()
            },
            vec![]
        ),
        3
    )
    .is_err());
    let size = w.clusters[cl.ix()].size;
    assert!(apply(
        &mut w,
        &prop(
            dg0,
            Effect::Shrink {
                cluster: cl,
                count: size + 1
            },
            vec![]
        ),
        3
    )
    .is_err());
    assert_eq!(w.ledger.rows.len(), n);
    // Lethal damage kills
    apply(
        &mut w,
        &prop(
            dg0,
            Effect::Damage {
                who: ch,
                amount: 50.0,
                cause: "fall".into(),
            },
            vec![],
        ),
        4,
    )
    .unwrap();
    assert!(!w.characters[0].alive);
    // Memorize materializes; Forget only writes its row
    let before = w.characters.len();
    let r = apply(
        &mut w,
        &prop(
            dg0,
            Effect::Memorize {
                mention: mention("a gambler", None),
                cluster: None,
            },
            vec![],
        ),
        4,
    )
    .unwrap();
    assert_eq!(w.characters.len(), before + 1);
    assert!(matches!(w.ledger.rows[r.ix()].targets[0], Ref::Char(_)));
    apply(
        &mut w,
        &prop(
            dg0,
            Effect::Forget {
                owner: dg0,
                about: vec![ch],
            },
            vec![],
        ),
        4,
    )
    .unwrap();
    // Dispatch makes a paladin body + mind
    let r = apply(
        &mut w,
        &Proposal {
            source: Source::God {
                god: GodId(0),
                ruling: None,
            },
            ..prop(
                Ref::God(GodId(0)),
                Effect::Dispatch {
                    god: GodId(0),
                    mission: "collect".into(),
                    target: dg0,
                    paladin: None,
                },
                vec![],
            )
        },
        4,
    )
    .unwrap();
    let Ref::Char(p) = w.ledger.rows[r.ix()].targets[0] else {
        panic!()
    };
    assert_eq!(w.characters[p.ix()].serves, Some(GodId(0)));
    assert_eq!(w.gods[0].paladins, vec![p]);
    assert!(w.agents.iter().any(|a| a.kind
        == AgentKind::Paladin {
            god: GodId(0),
            body: p
        }));
    // Dispatching that paladin again retasks it (no new body); another god's paladin can't be borrowed.
    let (chars, agents) = (w.characters.len(), w.agents.len());
    let dispatch = |god: GodId, paladin| Proposal {
        source: Source::God { god, ruling: None },
        ..prop(
            Ref::God(god),
            Effect::Dispatch {
                god,
                mission: "again".into(),
                target: dg0,
                paladin: Some(paladin),
            },
            vec![],
        )
    };
    apply(&mut w, &dispatch(GodId(0), p), 5).unwrap();
    assert_eq!((w.characters.len(), w.agents.len()), (chars, agents));
    let mind = w
        .agents
        .iter()
        .find(|a| {
            a.kind
                == AgentKind::Paladin {
                    god: GodId(0),
                    body: p,
                }
        })
        .unwrap();
    assert_eq!(
        (mind.goal.as_str(), mind.scope.clone()),
        ("again", vec![dg0])
    );
    apply(&mut w, &dispatch(GodId(1), p), 5).unwrap();
    assert_eq!(
        w.characters.len(),
        chars + 1,
        "a new paladin for the other god"
    );
    // ids are row indices
    assert!(w
        .ledger
        .rows
        .iter()
        .enumerate()
        .all(|(i, r)| r.id.ix() == i));
}

#[test]
fn setup_fills_schema_2_fields() {
    let w = world(18);
    let p = pantheon();
    for (i, g) in w.gods.iter().enumerate() {
        assert_eq!(g.name, p.gods[i].name);
        assert_eq!(w.name_of(Ref::God(g.id)), p.gods[i].name);
    }
    assert_eq!(w.name_of(Ref::Ledger(LedgerId(0))), "an earlier event");
    let t = template::template();
    for pl in &w.places {
        assert!(pl.tags.iter().all(|t| !t.starts_with("archetype:")));
        match pl.kind {
            PlaceKind::District => {
                let k = pl
                    .archetype
                    .as_deref()
                    .expect("districts carry their archetype");
                assert!(t.districts.iter().any(|d| d.key == k), "{k}");
            }
            _ => assert!(pl.archetype.is_none()),
        }
    }
    let r0 = w.places[0].radius_km;
    assert!((w.meta.arena_end_radius_km - r0 * ARENA_END_FRACTION).abs() < 1e-9);
    assert!(w.clusters.iter().any(|c| c.baseline != Mood::REST));
    for c in &w.clusters {
        let b = c.baseline;
        assert!([b.anger, b.fear, b.hope, b.devotion]
            .iter()
            .all(|x| (0.0..=1.0).contains(x)));
    }
    assert!(w
        .demigods
        .iter()
        .all(|d| d.max_health == MAX_HEALTH && d.tempo.is_empty()));
    assert!(w.characters.iter().all(|c| c.max_health == MAX_HEALTH));
}

#[test]
fn drift_relaxes_toward_the_baseline() {
    let mut w = world(19);
    let b = Mood {
        anger: 0.9,
        fear: 0.1,
        hope: 0.5,
        devotion: 0.0,
    };
    w.clusters[0].baseline = b;
    w.clusters[0].mood = Mood {
        anger: 0.1,
        fear: 0.9,
        hope: 0.5,
        devotion: 0.5,
    };
    for beat in 1..=200 {
        drift(&mut w, beat);
    }
    let m = w.clusters[0].mood;
    for (x, y) in [
        (m.anger, b.anger),
        (m.fear, b.fear),
        (m.hope, b.hope),
        (m.devotion, b.devotion),
    ] {
        assert!((x - y).abs() < 0.02, "{m:?} vs {b:?}");
    }
}

#[test]
fn arena_end_radius_comes_from_meta() {
    let mut w = world(20);
    w.meta.arena_end_radius_km = 1.0;
    assert!((arena_radius_at(&w, w.meta.beats_total) - 1.0).abs() < 1e-9);
    // Schema-1 states (no end radius) keep the old rule.
    w.meta.arena_end_radius_km = 0.0;
    let r0 = w.places[0].radius_km;
    assert!((arena_radius_at(&w, w.meta.beats_total) - r0 * ARENA_END_FRACTION).abs() < 1e-9);
}

#[test]
fn known_at_tracks_who_learned_when() {
    let mut w = world(13);
    let dg0 = Ref::Demigod(DemigodId(0));
    let wit = Ref::Cluster(ClusterId(0));
    let row = apply(
        &mut w,
        &prop(
            dg0,
            Effect::Note {
                text: "a fire".into(),
            },
            vec![wit],
        ),
        2,
    )
    .unwrap();
    let r = &w.ledger.rows[row.ix()];
    let at0: Vec<(GroupId, u32)> = r.known_by.as_slice().iter().map(|g| (*g, 2)).collect();
    assert_eq!(r.known_at, at0);
    for beat in 3..=12 {
        for h in spread(&mut w, beat) {
            if h.row == row {
                assert_eq!(w.ledger.rows[row.ix()].known_since(h.group), Some(beat));
            }
        }
    }
    let r = &w.ledger.rows[row.ix()];
    assert!(r.known_at.len() > at0.len(), "news spread");
    let mut groups: Vec<GroupId> = r.known_at.iter().map(|x| x.0).collect();
    assert!(
        r.known_at.windows(2).all(|x| x[0].1 <= x[1].1),
        "in the order learned"
    );
    groups.sort();
    assert_eq!(groups, r.known_by.as_slice(), "one entry per known group");
}

#[test]
fn life_price_revive_and_tempo() {
    let mut w = world(22);
    let dg0 = Ref::Demigod(DemigodId(0));
    let dg1 = Ref::Demigod(DemigodId(1));
    let ch = Ref::Char(CharId(0));
    let god = |e: Effect| Proposal {
        source: Source::God {
            god: GodId(0),
            ruling: None,
        },
        ..prop(Ref::God(GodId(0)), e, vec![])
    };
    // Years of life lower the cap; health clamps to it and heals stop there.
    apply(
        &mut w,
        &god(Effect::Damage {
            who: dg0,
            amount: 1.0,
            cause: "x".into(),
        }),
        1,
    )
    .unwrap();
    apply(
        &mut w,
        &god(Effect::Price {
            who: dg0,
            god: GodId(0),
            kind: PriceKind::Life,
            amount: 3.0,
        }),
        1,
    )
    .unwrap();
    assert_eq!((w.demigods[0].health, w.demigods[0].max_health), (7.0, 7.0));
    apply(
        &mut w,
        &god(Effect::Damage {
            who: dg0,
            amount: 4.0,
            cause: "x".into(),
        }),
        1,
    )
    .unwrap();
    apply(
        &mut w,
        &god(Effect::Heal {
            who: dg0,
            amount: 9.0,
        }),
        1,
    )
    .unwrap();
    assert_eq!(w.demigods[0].health, 7.0);
    // A cap of 0 is death.
    apply(
        &mut w,
        &god(Effect::Price {
            who: ch,
            god: GodId(0),
            kind: PriceKind::Life,
            amount: 20.0,
        }),
        1,
    )
    .unwrap();
    assert!(!w.characters[0].alive);
    assert!(apply(&mut w, &god(Effect::Revive { who: ch }), 2)
        .unwrap_err()
        .contains("years"));

    // Revive: only the dead, back at a fraction of their max, hands live again.
    assert!(apply(&mut w, &god(Effect::Revive { who: dg1 }), 2)
        .unwrap_err()
        .contains("alive"));
    assert!(apply(
        &mut w,
        &god(Effect::Revive {
            who: Ref::Cluster(ClusterId(0))
        }),
        2
    )
    .is_err());
    apply(
        &mut w,
        &god(Effect::Price {
            who: dg1,
            god: GodId(0),
            kind: PriceKind::Life,
            amount: 2.0,
        }),
        2,
    )
    .unwrap();
    apply(
        &mut w,
        &god(Effect::Eliminate {
            who: dg1,
            cause: "fell".into(),
        }),
        2,
    )
    .unwrap();
    assert_eq!(
        w.agents[w.demigods[1].hands[0].ix()].status,
        AgentStatus::Ended
    );
    let r = apply(&mut w, &god(Effect::Revive { who: dg1 }), 3).unwrap();
    assert_eq!(w.ledger.rows[r.ix()].verb, "revive");
    let d = &w.demigods[1];
    assert!(d.alive && d.eliminated_beat.is_none() && d.cause.is_none());
    assert_eq!(d.health, 8.0 * REVIVE_FRACTION);
    assert!(d
        .hands
        .iter()
        .all(|a| w.agents[a.ix()].status == AgentStatus::Live));

    // Tempo: stored on the body until it runs out; bad factors, lengths and the dead are invalid.
    let r = apply(
        &mut w,
        &god(Effect::Tempo {
            who: dg0,
            factor: 0.5,
            beats: 3,
        }),
        4,
    )
    .unwrap();
    assert_eq!(w.ledger.rows[r.ix()].verb, "tempo");
    assert_eq!(
        w.demigods[0].tempo,
        vec![Tempo {
            factor: 0.5,
            until_beat: 7
        }]
    );
    assert_eq!(tempo_factor(&w.demigods[0].tempo, 6), 0.5);
    assert_eq!(tempo_factor(&w.demigods[0].tempo, 7), 1.0);
    apply(
        &mut w,
        &god(Effect::Tempo {
            who: dg0,
            factor: 2.0,
            beats: 1,
        }),
        8,
    )
    .unwrap();
    assert_eq!(
        w.demigods[0].tempo,
        vec![Tempo {
            factor: 2.0,
            until_beat: 9
        }],
        "expired entries are dropped"
    );
    let n = w.ledger.rows.len();
    for e in [
        Effect::Tempo {
            who: dg0,
            factor: 0.0,
            beats: 2,
        },
        Effect::Tempo {
            who: dg0,
            factor: f32::NAN,
            beats: 2,
        },
        Effect::Tempo {
            who: dg0,
            factor: 9.0,
            beats: 2,
        },
        Effect::Tempo {
            who: dg0,
            factor: 0.5,
            beats: 0,
        },
        Effect::Tempo {
            who: ch,
            factor: 0.5,
            beats: 2,
        },
        Effect::Tempo {
            who: Ref::Cluster(ClusterId(0)),
            factor: 0.5,
            beats: 2,
        },
    ] {
        assert!(apply(&mut w, &god(e.clone()), 8).is_err(), "{e:?}");
    }
    assert_eq!(w.ledger.rows.len(), n);
}

#[test]
fn rewind_restores() {
    let mut w = world(12);
    let dg0 = Ref::Demigod(DemigodId(0));
    let start = w.demigods[0].pos;
    w.meta.beat = 1;
    capture_rewind(&mut w);
    let to = geo::destination(start, 0.0, 2.0);
    apply(
        &mut w,
        &prop(
            dg0,
            Effect::Move {
                who: dg0,
                to,
                place: None,
            },
            vec![],
        ),
        2,
    )
    .unwrap();
    apply(
        &mut w,
        &prop(
            dg0,
            Effect::Damage {
                who: dg0,
                amount: 3.0,
                cause: "x".into(),
            },
            vec![],
        ),
        2,
    )
    .unwrap();
    apply(
        &mut w,
        &prop(
            dg0,
            Effect::Rewind {
                target: dg0,
                beats: 1,
            },
            vec![],
        ),
        2,
    )
    .unwrap();
    assert_eq!((w.demigods[0].pos, w.demigods[0].health), (start, 10.0));
}

#[test]
fn spread_follows_overlaps_only() {
    let mut w = world(13);
    let dg0 = Ref::Demigod(DemigodId(0));
    // An empty emergent group overlaps nobody.
    apply(
        &mut w,
        &prop(
            dg0,
            Effect::FoundGroup {
                name: "The Hollow".into(),
                kind: "cult".into(),
                members: vec![],
            },
            vec![],
        ),
        0,
    )
    .unwrap();
    let hollow = GroupId::from_ix(w.groups.len() - 1);
    let dockish = w
        .groups
        .iter()
        .find(|g| g.kind == GroupKind::Trade)
        .unwrap()
        .id;
    let row = apply(
        &mut w,
        &prop(
            dg0,
            Effect::Note {
                text: "a fire".into(),
            },
            vec![Ref::Group(dockish)],
        ),
        0,
    )
    .unwrap();
    let neighbors: Vec<GroupId> = overlap_graph(&w)
        .into_iter()
        .filter_map(|(a, b, _)| {
            if a == dockish {
                Some(b)
            } else if b == dockish {
                Some(a)
            } else {
                None
            }
        })
        .collect();
    assert!(!neighbors.is_empty());
    let mut first_hop = None;
    for beat in 1..=30 {
        let hops = spread(&mut w, beat);
        for h in &hops {
            assert_ne!(h.group, hollow);
            assert!(h.p > 0.0);
        }
        if first_hop.is_none()
            && w.ledger.rows[row.ix()]
                .known_by
                .as_slice()
                .iter()
                .any(|g| neighbors.contains(g))
        {
            first_hop = Some(beat);
        }
    }
    let fh = first_hop.expect("reached a neighbor");
    assert!(fh <= 3, "first hop at beat {fh}");
    assert!(!w.ledger.rows[row.ix()].known_by.contains(hollow));
    // Deterministic
    let mut a = world(13);
    let mut b = world(13);
    for x in [&mut a, &mut b] {
        apply(
            x,
            &prop(
                dg0,
                Effect::Note {
                    text: "a fire".into(),
                },
                vec![Ref::Group(dockish)],
            ),
            0,
        )
        .unwrap();
    }
    for beat in 1..10 {
        assert_eq!(spread(&mut a, beat), spread(&mut b, beat));
    }
}

#[test]
fn debts_delay_and_collect() {
    let mut w = world(14);
    let dg0 = Ref::Demigod(DemigodId(0));
    let r = apply(
        &mut w,
        &prop(
            dg0,
            Effect::Debt {
                owed_by: dg0,
                god: GodId(2),
                amount: 2.0,
                due_in: 3,
            },
            vec![],
        ),
        1,
    )
    .unwrap();
    assert!(due_debts(&w, 3).is_empty());
    assert_eq!(due_debts(&w, 4), vec![r]);
    // Time delays it (via an effect): +50%, due moves.
    let d = apply(
        &mut w,
        &prop(dg0, Effect::Delay { row: r, beats: 5 }, vec![]),
        4,
    )
    .unwrap();
    let debt = w.ledger.rows[r.ix()].debt.unwrap();
    assert_eq!(
        (debt.amount, debt.due_beat, debt.status, debt.delays),
        (3.0, 9, DebtStatus::Delayed, 1)
    );
    assert_eq!(w.ledger.rows[d.ix()].cause, Some(r));
    assert!(due_debts(&w, 8).is_empty());
    assert_eq!(due_debts(&w, 9), vec![r]);
    delay_debt(&mut w, r, 1).unwrap();
    assert_eq!(w.ledger.rows[r.ix()].debt.unwrap().amount, 4.5);
    let c = collect_debt(&mut w, r, 10).unwrap();
    let debt = w.ledger.rows[r.ix()].debt.unwrap();
    assert_eq!(
        (debt.status, debt.collected_by),
        (DebtStatus::Collected, Some(c))
    );
    assert_eq!(w.demigods[0].health, 5.5);
    assert!(collect_debt(&mut w, r, 11).is_err());
    assert!(due_debts(&w, 100).is_empty());
    // On the Final Night everything outstanding is due.
    let r2 = apply(
        &mut w,
        &prop(
            dg0,
            Effect::Debt {
                owed_by: dg0,
                god: GodId(2),
                amount: 1.0,
                due_in: 500,
            },
            vec![],
        ),
        11,
    )
    .unwrap();
    assert_eq!(due_debts(&w, w.meta.beats_total), vec![r2]);
}

#[test]
fn debt_curve() {
    assert_eq!(debt_for(0, 500), 0.0);
    assert!(debt_for(5, 0) < debt_for(5, 60) && debt_for(5, 60) < debt_for(5, 300));
    assert!(debt_for(10, 0) < debt_for(10, 1000) && debt_for(10, 1000) <= 10.0);
    // Soft: tripling the words far less than triples the bill.
    assert!(debt_for(6, 300) < 1.6 * debt_for(6, 100));
}

#[test]
fn venn_queries() {
    let w = world(15);
    let c = &w.clusters[0];
    let gs = c.groups.as_slice().to_vec();
    let m = members(&w, &gs, &[]);
    assert!(m.contains(&Ref::Cluster(c.id)));
    let excl = members(&w, &gs[..1], &gs[1..]);
    assert!(!excl.contains(&Ref::Cluster(c.id)));
    let g = overlap_graph(&w);
    assert!(g.windows(2).all(|x| (x[0].0, x[0].1) < (x[1].0, x[1].1)));
    assert!(g.iter().all(|(a, b, x)| a < b && *x > 0.0 && *x <= 1.0));
    assert!(headcount(&w, gs[0]) >= c.size as u64);
}

#[test]
fn arena_shrinks() {
    let mut w = world(16);
    let r0 = w.places[0].radius_km;
    w.meta.beat = 50;
    let _ = shrink_arena(&mut w);
    let eased = 1.0 - 0.5f64.powf(ARENA_EASE);
    assert!((w.meta.arena_radius_km - (r0 + (r0 * ARENA_END_FRACTION - r0) * eased)).abs() < 1e-9);
    w.meta.beat = 100;
    let out = shrink_arena(&mut w);
    // Demigods start at 0.7 R, so they're all outside the final 0.15 R.
    assert_eq!(
        out.iter().filter(|r| matches!(r, Ref::Demigod(_))).count(),
        4
    );
}

#[test]
fn state_is_small() {
    let w = world(17);
    let n = serde_json::to_vec(&w).unwrap().len();
    println!("state for 4 seats: {n} bytes");
    assert!(n < 1_000_000, "{n}");
}

fn smoke(seed: u64) -> (World, usize) {
    let mut w = world(seed);
    let dg0 = Ref::Demigod(DemigodId(0));
    apply(
        &mut w,
        &prop(
            dg0,
            Effect::CreateItem {
                name: "Bell".into(),
                description: "a cracked bell".into(),
                holder: dg0,
            },
            vec![],
        ),
        0,
    )
    .unwrap();
    let texts = [
        "the baker who saw the fire",
        "a dockworker",
        "an angry student",
        "a nurse",
        "the gravedigger",
        "a gambler",
        "a smuggler",
    ];
    let mut applied = 0;
    for beat in 1..=100u32 {
        w.meta.beat = beat;
        let mut rng = Rng::derive(seed, &[beat as u64]);
        for _ in 0..12 {
            let alive: Vec<DemigodId> = w.alive_demigods().map(|d| d.id).collect();
            let actor_dg = rng.pick(&alive).copied().unwrap_or(DemigodId(0));
            let actor = match rng.below(3) {
                _ if alive.is_empty() => Ref::God(GodId(rng.below(6) as u32)),
                0 => Ref::Demigod(actor_dg),
                1 => Ref::Agent(w.demigods[actor_dg.ix()].hands[rng.below(2) as usize]),
                _ => Ref::God(GodId(rng.below(6) as u32)),
            };
            let nc = w.characters.len() as u64;
            let ncl = w.clusters.len() as u64;
            let ch = Ref::Char(CharId(rng.below(nc) as u32));
            let cl = ClusterId(rng.below(ncl) as u32);
            let dg = Ref::Demigod(DemigodId(rng.below(w.demigods.len() as u64) as u32));
            let grp = GroupId(rng.below(w.groups.len() as u64) as u32);
            let d = districts(&w)[rng.below(districts(&w).len() as u64) as usize].id;
            let effect = match rng.below(14) {
                0 => Effect::Move {
                    who: dg,
                    to: geo::random_in(&mut rng, w.places[0].center, w.places[0].radius_km),
                    place: None,
                },
                1 => Effect::Damage {
                    who: if rng.below(3) == 0 { dg } else { ch },
                    amount: rng.below(4) as f32,
                    cause: "brawl".into(),
                },
                2 => Effect::Heal {
                    who: if rng.below(2) == 0 { dg } else { ch },
                    amount: 1.0,
                },
                3 => Effect::Memorize {
                    mention: mention(texts[rng.below(texts.len() as u64) as usize], Some(d)),
                    cluster: None,
                },
                4 => Effect::Memorize {
                    mention: mention("someone in the crowd", None),
                    cluster: Some(cl),
                },
                5 => Effect::Join {
                    member: ch,
                    group: grp,
                },
                6 => Effect::Debt {
                    owed_by: Ref::Demigod(actor_dg),
                    god: GodId(2),
                    amount: debt_for(rng.below(10) as u8, rng.below(200) as u32) / 4.0,
                    due_in: 3 + rng.below(10) as u32,
                },
                7 => Effect::Echo {
                    item: ItemId(0),
                    holder: Ref::Demigod(actor_dg),
                },
                8 => Effect::MoodShift {
                    cluster: cl,
                    anger: 0.05,
                    fear: 0.0,
                    hope: -0.02,
                    devotion: 0.0,
                },
                9 => Effect::Allegiance {
                    who: Ref::Cluster(cl),
                    demigod: actor_dg,
                    delta: 0.2,
                },
                10 => Effect::Relation {
                    a: ch,
                    b: Ref::Demigod(actor_dg),
                    trust: 0.1,
                    fear: 0.1,
                    loyalty: 0.0,
                },
                11 if !w.ledger.rows.is_empty() => Effect::Tell {
                    to: ch,
                    row: LedgerId(rng.below(w.ledger.rows.len() as u64) as u32),
                },
                12 => Effect::Favor {
                    god: GodId(rng.below(6) as u32),
                    demigod: actor_dg,
                    delta: 0.5,
                },
                _ => Effect::Note {
                    text: "a miracle of light".into(),
                },
            };
            let mut witnesses = vec![Ref::Cluster(cl)];
            if rng.below(2) == 0 {
                witnesses.push(Ref::Cluster(ClusterId(rng.below(ncl) as u32)));
            }
            let valence = rng.below(11) as i8 - 5;
            let p = Proposal {
                valence,
                magnitude: rng.below(11) as u8,
                ..prop(actor, effect, witnesses)
            };
            if apply(&mut w, &p, beat).is_ok() {
                applied += 1;
            }
        }
        spread(&mut w, beat);
        drift(&mut w, beat);
        for r in due_debts(&w, beat) {
            let _ = collect_debt(&mut w, r, beat);
        }
        capture_rewind(&mut w);
        let _ = shrink_arena(&mut w);
    }
    (w, applied)
}

#[test]
fn smoke_100_beats() {
    let t = Instant::now();
    let (w, applied) = smoke(21);
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let json = serde_json::to_string(&w).unwrap();
    let known: usize = w.ledger.rows.iter().map(|r| r.known_by.len()).sum();
    println!(
        "smoke: {ms:.0} ms, {applied} applied, {} rows, {} characters, avg known_by {:.1}, state {} bytes",
        w.ledger.rows.len(),
        w.characters.len(),
        known as f64 / w.ledger.rows.len() as f64,
        json.len()
    );
    for (lo, hi) in [(0u32, 0u32), (1, 2), (3, 5), (6, 10), (11, 30), (31, 100)] {
        let v: Vec<usize> = w
            .ledger
            .rows
            .iter()
            .filter(|r| (lo..=hi).contains(&(100 - r.beat)))
            .map(|r| r.known_by.len())
            .collect();
        println!(
            "age {lo}-{hi}: avg known_by {:.1} of {} groups",
            v.iter().sum::<usize>() as f64 / v.len().max(1) as f64,
            w.groups.len()
        );
    }
    println!(
        "alive demigods {:?}",
        w.demigods
            .iter()
            .map(|d| (d.alive, d.eliminated_beat))
            .collect::<Vec<_>>()
    );
    assert!(json.len() < 4_000_000, "{}", json.len());
    assert!(applied > 800, "{applied}");
    assert!(w.clusters.iter().all(
        |c| [c.mood.anger, c.mood.fear, c.mood.hope, c.mood.devotion]
            .iter()
            .all(|x| (0.0..=1.0).contains(x))
    ));
    if !cfg!(debug_assertions) {
        assert!(ms < 1000.0, "{ms} ms");
    }
    // Replays exactly.
    let (w2, _) = smoke(21);
    assert_eq!(json, serde_json::to_string(&w2).unwrap());
}


#[test]
fn planet_setup_regions_cities_and_rules() {
    let p = pantheon();
    let cfg = SetupConfig {
        seed: 9,
        arena: ArenaKind::Planet,
        ..SetupConfig::default()
    };
    let seats: Vec<(u8, String)> = (0..4).map(|i| (i, format!("D{i}"))).collect();
    let w = setup(&cfg, &seats, &p);
    assert_eq!(serde_json::to_string(&w).unwrap(), serde_json::to_string(&setup(&cfg, &seats, &p)).unwrap());
    assert!(w.is_planet());
    let regions: Vec<&Place> = w.places.iter().filter(|x| x.kind == PlaceKind::District).collect();
    assert!(regions.len() >= 15, "{} regions", regions.len());
    assert!(regions.iter().any(|r| r.name == "Western Europe"));
    // Demigods start in different regions, each at (near) a city.
    let homes: std::collections::BTreeSet<PlaceId> =
        w.demigods.iter().map(|d| crate::geo::region_at(&w, d.pos)).collect();
    assert_eq!(homes.len(), 4);
    assert!(w.demigods.iter().all(|d| d.bolts == 3));
    // Paris is a city of Western Europe.
    let paris = crate::geo::place_at(&w, [2.35, 48.86]);
    assert_eq!(w.places[paris.ix()].kind, PlaceKind::Site);
    assert_eq!(w.name_of(Ref::Place(crate::geo::district_of(&w, paris))), "Western Europe");
    // Time's circle starts just wide enough to hold every demigod (plus the margin), not at the whole planet.
    let center = w.places[0].center;
    let far = w.demigods.iter().map(|d| crate::geo::distance_km(center, d.pos)).fold(0.0, f64::max);
    assert!(w.meta.arena_radius_km <= crate::geo::PLANET_RADIUS_KM);
    assert!((w.meta.arena_radius_km - (far + PLANET_START_MARGIN_KM).clamp(3000.0, crate::geo::PLANET_RADIUS_KM)).abs() < 1e-6);
    assert!(w.demigods.iter().all(|d| crate::geo::distance_km(center, d.pos) < w.meta.arena_radius_km));
    // The hierarchy: every city has districts (Quarters) holding its crowds; one node per place.
    assert_eq!(w.nodes.len(), w.places.len());
    assert!(w.clusters.iter().all(|c| w.places[c.place.ix()].kind == PlaceKind::Quarter));
    assert!(w.nodes[0].population > 1_000_000 && w.nodes[0].level == Level::Planet);
    // Influence: every demigod holds a share of its home region; shares per region ≤ 1.
    let inf = influence(&w);
    for d in &w.demigods {
        assert!(inf.iter().any(|x| x.1 == d.id && x.0 == crate::geo::region_at(&w, d.pos)));
    }
    // Rules: entropy is calm at 0; with Entropy at 85 every level fires, deterministically.
    assert!(entropy_proposals(&w, 1, Some(GodId(6))).is_empty());
    let mut hot = w.clone();
    hot.meta.entropy = 85.0;
    let a = entropy_proposals(&hot, 3, Some(GodId(6)));
    assert_eq!(a, entropy_proposals(&hot, 3, Some(GodId(6))));
    assert!(a.iter().any(|x| matches!(x.effect, Effect::Wound { .. })));
    // Raise an army and march it on Paris: it moves at most ARMY_KM_PER_BEAT a beat.
    let mut w2 = w.clone();
    let src = w2.clusters.iter().find(|c| c.size > 10).unwrap().id;
    let raise = Proposal {
        source: Source::Rules,
        intent: None,
        effect: Effect::RaiseArmy { demigod: DemigodId(0), from: src, size: 5, name: "Host".into(), orders: "march".into() },
        act: "raise".into(),
        actor: Ref::Demigod(DemigodId(0)),
        magnitude: 5,
        valence: 0,
        witnesses: vec![],
        cause: None,
    };
    apply(&mut w2, &raise, 1).unwrap();
    let army = ClusterId::from_ix(w2.clusters.len() - 1);
    w2.clusters[army.ix()].army.as_mut().unwrap().target = Some(Ref::Place(paris));
    assert!(army_proposals(&w2, 2).is_empty(), "armies move in the society step now");
    // The society step appoints the command chain (general, captains, squads) and marches it at most a beat's march.
    let from = w2.clusters[army.ix()].army.as_ref().unwrap().pos;
    w2.meta.beat = 2;
    let chars_before = w2.characters.len();
    let out = crate::sim::step(&mut w2, 2, &[]);
    let a = w2.clusters[army.ix()].army.clone().unwrap();
    let g = a.general.expect("a general");
    let gl = w2.characters[g.ix()].life.as_deref().expect("a sheet");
    assert_eq!(gl.role, Role::General);
    assert!(w2.characters[g.ix()].aliases.iter().any(|x| x.starts_with("General ")));
    assert!(a.captains.len() >= 2 && a.units.len() >= 3, "{} captains, {} units", a.captains.len(), a.units.len());
    assert_eq!(a.units.iter().map(|u| w2.cells[u.ix()].size).sum::<u32>(), w2.clusters[army.ix()].size);
    assert!(a.order.is_some(), "the order it was raised with reached the general");
    assert!(out.events.iter().any(|e| e.kind == SocKind::GeneralAppointed));
    // The general is either a local notable/hero/ringleader of the home city (appoint step 2, an existing person)
    // or someone who emerged from the crowd this step (step 3, a new person).
    assert!(out.new_people.contains(&g) || g.ix() < chars_before, "the general is neither new nor an existing local");
    assert!(crate::geo::distance_km(from, a.pos) <= ARMY_KM_PER_BEAT + 1.0);
}

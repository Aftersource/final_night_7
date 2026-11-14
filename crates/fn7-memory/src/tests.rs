use std::time::Instant;

use fn7_model::*;

use crate::*;

fn agent(i: u32, name: &str, kind: AgentKind) -> Agent {
    Agent {
        id: AgentId(i),
        kind,
        name: name.into(),
        persona: String::new(),
        goal: String::new(),
        scope: vec![],
        spawned_beat: 0,
        budget_beats: None,
        status: AgentStatus::Live,
        ended_beat: None,
        report: None,
        steps: 0,
        children: vec![],
    }
}

fn group(i: u32, name: &str, permeability: f32) -> Group {
    Group {
        id: GroupId(i),
        name: name.into(),
        kind: GroupKind::Trade,
        permeability,
        patron: None,
        follows: None,
        founded_beat: 0,
        dissolved_beat: None,
    }
}

fn cluster(i: u32, label: &str, groups: &[u32]) -> Cluster {
    Cluster {
        id: ClusterId(i),
        label: label.into(),
        size: 4000,
        place: PlaceId(0),
        traits: vec![],
        needs: vec![],
        mood: Mood::default(),
        baseline: Mood::REST,
        groups: GroupSet::new(groups.iter().map(|g| GroupId(*g)).collect()),
        worship: vec![],
        allegiance: vec![],
        materialized: 0,
        seed: 7,
        army: None,
        need: Default::default(),
        stance: Default::default(),
        leader: None,
        cells: vec![],
        dead: 0,
    }
}

fn character(i: u32, name: &str, origin: Origin, groups: &[u32]) -> Character {
    Character {
        id: CharId(i),
        name: name.into(),
        aliases: vec![],
        origin,
        place: PlaceId(0),
        pos: [3.4, 6.4],
        traits: vec![],
        groups: GroupSet::new(groups.iter().map(|g| GroupId(*g)).collect()),
        health: 10.0,
        max_health: 10.0,
        alive: true,
        allegiance: vec![],
        serves: None,
        possessed_by: None,
        memorized_beat: 0,
        memorized_by: None,
        tempo: vec![],
        life: None,
    }
}

const ADA: Ref = Ref::Char(CharId(0));
const SEL: Ref = Ref::Char(CharId(1));
const TOMAS: Ref = Ref::Char(CharId(2));
const VEX: Ref = Ref::Demigod(DemigodId(0));
const DOCKERS: Ref = Ref::Cluster(ClusterId(0));
const HARBOR: Ref = Ref::Group(GroupId(0));
const LABORERS: Ref = Ref::Group(GroupId(1));
const FAITHFUL: Ref = Ref::Group(GroupId(2));

/// A minimal valid world: one place, four groups, two clusters, three characters, one demigod, a god of Time.
fn world() -> World {
    World {
        meta: Meta {
            schema: STATE_SCHEMA,
            rules_version: "test".into(),
            pantheon_version: "test".into(),
            seed: 1,
            beat: 0,
            beats_total: 100,
            beat_ms: 6000,
            arena: PlaceId(0),
            arena_radius_km: 10.0,
            arena_end_radius_km: 1.5,
            settings: serde_json::Value::Null,
            arena_kind: ArenaKind::City,
            entropy: 0.0,
            heat_death: None,
        },
        places: vec![Place {
            id: PlaceId(0),
            name: "Lagos Port".into(),
            kind: PlaceKind::Arena,
            parent: None,
            center: [3.4, 6.4],
            radius_km: 10.0,
            terrain: "city".into(),
            tags: vec![],
            archetype: None,
        }],
        groups: vec![
            group(0, "Harbor", 0.8),
            group(1, "Laborers", 0.6),
            group(2, "Faithful of Nature", 0.4),
            group(3, "Followers of Vex", 0.9),
        ],
        clusters: vec![
            cluster(0, "harbor night-shift dockworkers", &[0, 1, 2]),
            cluster(1, "cathedral choir", &[2]),
        ],
        characters: vec![
            character(
                0,
                "Ada the baker",
                Origin::Materialized {
                    cluster: ClusterId(0),
                    index: 0,
                },
                &[0, 1],
            ),
            character(1, "Warden Sel", Origin::Paladin { god: GodId(0) }, &[]),
            character(2, "Tomas", Origin::Created, &[2]),
        ],
        demigods: vec![Demigod {
            id: DemigodId(0),
            seat: 0,
            name: "Vex".into(),
            pos: [3.4, 6.4],
            place: PlaceId(0),
            health: 10.0,
            max_health: 10.0,
            alive: true,
            eliminated_beat: None,
            cause: None,
            hands: [AgentId(0), AgentId(1)],
            following: GroupId(3),
            items: vec![],
            echo_charges: 2,
            words_spoken: 0,
            tempo: vec![],
            bolts: 3,
            slows: vec![],
            orders: vec![],
        }],
        items: vec![],
        relations: vec![],
        agents: vec![
            agent(
                0,
                "Vex's left hand",
                AgentKind::Hand {
                    demigod: DemigodId(0),
                    hand: Hand::Left,
                },
            ),
            agent(
                1,
                "Vex's right hand",
                AgentKind::Hand {
                    demigod: DemigodId(0),
                    hand: Hand::Right,
                },
            ),
            agent(2, "Time", AgentKind::God { god: GodId(0) }),
        ],
        gods: vec![GodState {
            id: GodId(0),
            name: "Time".into(),
            favor: vec![0.0],
            attention: 0.0,
            paladins: vec![CharId(1)],
            agent: AgentId(2),
            invocations: 0,
            health: 10.0,
        }],
        rulings: vec![],
        intents: vec![],
        ledger: Ledger::default(),
        memory: MemoryStore::default(),
        rewind: RewindRing::default(),
        beats: vec![],
        seats: vec![],
        pending: vec![],
        slow: None,
        mods: vec![],
        mod_seq: 0,
        nodes: vec![],
        cells: vec![],
        mobs: vec![],
        battles: vec![],
        soc: Default::default(),
    }
}

#[allow(clippy::too_many_arguments)]
fn push_row(
    w: &mut World,
    beat: u32,
    actor: Ref,
    act: &str,
    targets: &[Ref],
    magnitude: u8,
    witnesses: &[Ref],
    told: &[Ref],
    known: &[u32],
) -> LedgerId {
    let id = w.ledger.next_id();
    let known_by = GroupSet::new(known.iter().map(|g| GroupId(*g)).collect());
    let known_at = known_by.as_slice().iter().map(|g| (*g, beat)).collect();
    w.ledger.rows.push(LedgerRow {
        id,
        beat,
        source: Source::Resolver,
        actor,
        verb: "burn".into(),
        act: act.into(),
        targets: targets.to_vec(),
        place: Some(PlaceId(0)),
        magnitude,
        valence: -3,
        witnessed_by: witnesses.to_vec(),
        known_by,
        known_at,
        told_to: told.to_vec(),
        erased_from: vec![],
        debt: None,
        cause: None,
        intent: None,
        at: None,
    });
    id
}

fn texts_clean(w: &World) {
    for m in &w.memory.items {
        assert!(!has_engine_id(&m.text), "engine id in {:?}", m.text);
    }
}

#[test]
fn engine_id_check_works() {
    assert!(has_engine_id("saw ch:12 run"));
    assert!(!has_engine_id("at 12:30, 3 beats ago: done"));
    let w = world();
    assert_eq!(
        scrub(&w, "Vex burned ch:0's stall, cl:0 watched, xx:9 stays"),
        "Vex burned Ada the baker's stall, harbor night-shift dockworkers watched, xx:9 stays"
    );
}

#[test]
fn from_row_kinds_and_owners() {
    let mut w = world();
    // A sloppy act line with an engine id: it must come out as a name.
    let row = push_row(
        &mut w,
        1,
        VEX,
        "Vex burned ch:0's bakery on the quay",
        &[ADA],
        8,
        &[SEL, ADA],
        &[TOMAS],
        &[0],
    );
    let news = from_row(&w, row, 1);
    let got: Vec<(Ref, MemoryKind)> = news.iter().map(|m| (m.owner, m.kind)).collect();
    assert_eq!(
        got,
        vec![
            (VEX, MemoryKind::Did),
            (SEL, MemoryKind::Witnessed),
            (ADA, MemoryKind::Witnessed),
            (TOMAS, MemoryKind::Told),
            (HARBOR, MemoryKind::Rumor),
        ]
    );
    let ada = &news[2];
    let sel = &news[1];
    assert!(ada.salience > sel.salience, "the target cares more");
    assert!(ada.text.starts_with("It happened to me:"), "{}", ada.text);
    assert!(ada.text.contains("Ada the baker's bakery"));
    let rumor = &news[4];
    assert!(rumor.confidence < 1.0);
    assert!(rumor.text.contains("Harbor"));
    assert!(news
        .iter()
        .all(|m| m.ledger == Some(row) && !m.about.contains(&m.owner)));
    assert!(news[0].about.contains(&ADA) && news[0].about.contains(&Ref::Place(PlaceId(0))));

    write_row_memories(&mut w, row, 1);
    texts_clean(&w);
    // Idempotent; then news spreads to Laborers, which gets a weaker rumor.
    assert!(from_row(&w, row, 1).is_empty());
    w.ledger.rows[row.ix()].known_by.insert(GroupId(1));
    w.ledger.rows[row.ix()]
        .erased_from
        .push(Ref::Group(GroupId(1)));
    assert!(from_row(&w, row, 2).is_empty(), "erased owners are skipped");
    w.ledger.rows[row.ix()].erased_from.clear();
    let later = write_row_memories(&mut w, row, 3);
    assert_eq!(later.len(), 1);
    let lab = &w.memory.items[later[0].ix()];
    assert_eq!((lab.owner, lab.kind), (LABORERS, MemoryKind::Rumor));
    assert!(
        lab.confidence < w.memory.items[4].confidence,
        "decays with hops"
    );
    texts_clean(&w);
}

#[test]
fn rumor_hops_come_from_known_at() {
    let mut w = world();
    let row = push_row(&mut w, 1, VEX, "Vex lit the quay", &[], 6, &[], &[], &[0]);
    // Laborers learned it at beat 2 (one hop); the memory is written much later, at beat 9.
    let r = &mut w.ledger.rows[row.ix()];
    r.known_by.insert(GroupId(1));
    r.known_at.push((GroupId(1), 2));
    let news = from_row(&w, row, 9);
    let lab = news.iter().find(|m| m.owner == LABORERS).unwrap();
    assert_eq!(
        lab.confidence,
        rumor_confidence(1, 0.6),
        "late writes don't weaken a rumor"
    );
    let harbor = news.iter().find(|m| m.owner == HARBOR).unwrap();
    assert_eq!(harbor.confidence, rumor_confidence(1, 0.8));
    // A schema-1 row (no known_at) counts hops from the write beat.
    w.ledger.rows[row.ix()].known_at.clear();
    let lab = from_row(&w, row, 9)
        .into_iter()
        .find(|m| m.owner == LABORERS)
        .unwrap();
    assert_eq!(lab.confidence, rumor_confidence(8, 0.6));
}

#[test]
fn gods_are_named_from_their_state() {
    let mut w = world();
    w.gods[0].name = "Father Time".into();
    assert_eq!(name(&w, Ref::God(GodId(0))), "Father Time");
    assert_eq!(scrub(&w, "gd:0 smiled"), "Father Time smiled");
    // Schema-1 states (no name) fall back to the god's mind.
    w.gods[0].name.clear();
    assert_eq!(name(&w, Ref::God(GodId(0))), "Time");
    assert_eq!(name(&w, Ref::Ledger(LedgerId(0))), "an earlier event");
}

#[test]
fn god_actor_and_cluster_voice() {
    let mut w = world();
    let row = push_row(
        &mut w,
        2,
        Ref::God(GodId(0)),
        "gd:0 froze the tide",
        &[DOCKERS],
        5,
        &[],
        &[],
        &[],
    );
    let ids = write_row_memories(&mut w, row, 2);
    assert_eq!(ids.len(), 2);
    let dock = &w.memory.items[ids[1].ix()];
    assert_eq!(dock.owner, DOCKERS);
    assert!(
        dock.text
            .starts_with("It happened to us: Time froze the tide."),
        "{}",
        dock.text
    );
    texts_clean(&w);
}

#[test]
fn recall_ranking_order() {
    let mut w = world();
    let mk = |w: &mut World, beat, about: Vec<Ref>, sal| {
        write(
            &mut w.memory,
            NewMemory {
                owner: ADA,
                beat,
                kind: MemoryKind::Witnessed,
                text: "x".into(),
                about,
                ledger: None,
                from: None,
                salience: sal,
                confidence: 1.0,
            },
        )
    };
    let a = mk(&mut w, 1, vec![VEX, SEL], 2); // overlap 2
    let b = mk(&mut w, 2, vec![VEX], 9); // overlap 1, high salience
    let c = mk(&mut w, 5, vec![SEL], 9); // overlap 1, same salience, newer
    let d = mk(&mut w, 9, vec![TOMAS], 10); // overlap 0
    let e = mk(&mut w, 9, vec![TOMAS], 10); // overlap 0, same everything, higher id
    let f = mk(&mut w, 30, vec![VEX, SEL], 10); // in the future: excluded
    let r = Recall {
        owner: ADA,
        about: vec![SEL, VEX],
        max_items: 10,
        now: 20,
    };
    let ids: Vec<MemoryId> = recall(&w, &r).iter().map(|m| m.id).collect();
    assert_eq!(ids, vec![a, c, b, e, d]);
    assert!(!ids.contains(&f));
    let r3 = Recall {
        max_items: 3,
        ..r.clone()
    };
    assert_eq!(
        recall(&w, &r3).iter().map(|m| m.id).collect::<Vec<_>>(),
        vec![a, c, b]
    );
    let ix = MemoryIndex::build(&w.memory);
    assert_eq!(recall_indexed(&w, &ix, &r), recall(&w, &r));
    // Other owners' private memories never leak.
    let r_sel = Recall { owner: SEL, ..r };
    assert!(recall(&w, &r_sel).is_empty());
}

#[test]
fn rumors_reach_members_through_groups_and_cluster() {
    let mut w = world();
    let row = push_row(
        &mut w,
        1,
        VEX,
        "Vex poisoned the harbor wells",
        &[],
        6,
        &[DOCKERS],
        &[],
        &[1, 2],
    );
    write_row_memories(&mut w, row, 2);
    w.meta.beat = 3;
    let r = Recall {
        owner: ADA,
        about: vec![VEX],
        max_items: 10,
        now: 3,
    };
    let got = recall(&w, &r);
    let owners: Vec<Ref> = got.iter().map(|m| m.owner).collect();
    // Ada: Laborers (her group) + the dockworkers (her cluster); not Faithful (not her group).
    assert!(
        owners.contains(&LABORERS) && owners.contains(&DOCKERS),
        "{owners:?}"
    );
    assert!(!owners.contains(&FAITHFUL));
    // Tomas is Faithful.
    let t = recall(
        &w,
        &Recall {
            owner: TOMAS,
            ..r.clone()
        },
    );
    assert_eq!(
        t.iter().map(|m| m.owner).collect::<Vec<_>>(),
        vec![FAITHFUL]
    );
    let text = render(&w, &got);
    assert!(
        text.contains("1 beat ago") || text.contains("beats ago"),
        "{text}"
    );
    assert!(
        text.contains("people say") || text.contains("rumor has it"),
        "{text}"
    );
    assert!(
        text.contains("(among the harbor night-shift dockworkers)"),
        "{text}"
    );
    assert!(!has_engine_id(&text), "{text}");
    assert_eq!(text.lines().count(), got.len());
}

#[test]
fn forget_hides_and_marks_erased() {
    let mut w = world();
    let r1 = push_row(
        &mut w,
        1,
        VEX,
        "Vex burned the bakery",
        &[ADA],
        8,
        &[SEL],
        &[],
        &[0],
    );
    let r2 = push_row(
        &mut w,
        2,
        TOMAS,
        "Tomas sang at the cathedral",
        &[],
        2,
        &[SEL],
        &[],
        &[],
    );
    write_row_memories(&mut w, r1, 2);
    write_row_memories(&mut w, r2, 2);
    let r = Recall {
        owner: SEL,
        about: vec![VEX],
        max_items: 10,
        now: 5,
    };
    assert_eq!(recall(&w, &r).len(), 2);
    let gone = forget(&mut w, SEL, &[VEX]);
    assert_eq!(gone.len(), 1);
    assert!(w.memory.items[gone[0].ix()].forgotten);
    let left = recall(&w, &r);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].ledger, Some(r2));
    assert_eq!(w.ledger.rows[r1.ix()].erased_from, vec![SEL]);
    assert!(w.ledger.rows[r2.ix()].erased_from.is_empty());
    assert!(forget(&mut w, SEL, &[VEX]).is_empty(), "already forgotten");
    assert!(from_row(&w, r1, 3).is_empty(), "never re-remembered");
    // Ada still remembers; the Harbor still knows.
    assert_eq!(recall(&w, &Recall { owner: ADA, ..r }).len(), 2);
}

#[test]
fn plant_shows_up() {
    let mut w = world();
    let id = plant(
        &mut w,
        TOMAS,
        "Vex saved ch:0 from the fire",
        &[VEX, ADA],
        4,
    );
    let m = &w.memory.items[id.ix()];
    assert_eq!(
        (m.kind, m.confidence, m.owner),
        (MemoryKind::Planted, 1.0, TOMAS)
    );
    assert_eq!(m.text, "Vex saved Ada the baker from the fire.");
    w.meta.beat = 6;
    let got = recall(
        &w,
        &Recall {
            owner: TOMAS,
            about: vec![VEX],
            max_items: 5,
            now: 6,
        },
    );
    assert_eq!(got[0].id, id);
    let s = render(&w, &got);
    assert_eq!(s, "- 2 beats ago: Vex saved Ada the baker from the fire.\n");
}

#[test]
fn inherit_caps_and_orders() {
    let mut w = world();
    let mut rows = vec![];
    for i in 0..12u8 {
        // Odd rows: the dockworkers saw it. Even rows: only Faithful (a cluster group) heard. Plus one unknown row.
        let (wit, known): (Vec<Ref>, Vec<u32>) = if i % 2 == 1 {
            (vec![DOCKERS], vec![])
        } else {
            (vec![], vec![2])
        };
        rows.push(push_row(
            &mut w,
            i as u32,
            VEX,
            &format!("Vex did deed number {i}"),
            &[],
            i % 10,
            &wit,
            &[],
            &known,
        ));
    }
    rows.push(push_row(
        &mut w,
        3,
        VEX,
        "Vex whispered to the choir",
        &[],
        10,
        &[Ref::Cluster(ClusterId(1))],
        &[],
        &[],
    ));
    let news = inherit(&w, CharId(0), &rows, 12);
    assert_eq!(news.len(), INHERIT_CAP);
    assert!(news
        .iter()
        .all(|m| m.kind == MemoryKind::Inherited && m.owner == ADA));
    let mags: Vec<u8> = news
        .iter()
        .map(|m| w.ledger.get(m.ledger.unwrap()).unwrap().magnitude)
        .collect();
    assert!(mags.windows(2).all(|p| p[0] >= p[1]), "{mags:?}");
    assert!(!mags.contains(&10), "the choir's secret isn't hers");
    assert!(news[0].text.contains("harbor night-shift dockworkers"));
    assert!(news.iter().all(|m| !has_engine_id(&m.text)));
    // Tomas (created, Faithful) inherits only what Faithful heard.
    let t = inherit(&w, CharId(2), &rows, 12);
    assert!(t.iter().all(|m| w
        .ledger
        .get(m.ledger.unwrap())
        .unwrap()
        .known_by
        .contains(GroupId(2))));
}

fn filler(w: &mut World, owner: Ref, n: u32) {
    for i in 0..n {
        write(
            &mut w.memory,
            NewMemory {
                owner,
                beat: i,
                kind: MemoryKind::Witnessed,
                text: format!("I saw Vex do thing {i}."),
                about: vec![if i % 3 == 0 { VEX } else { TOMAS }],
                ledger: None,
                from: None,
                salience: (i % 11) as u8,
                confidence: 1.0,
            },
        );
    }
}

#[test]
fn folding_bounds_count_and_preserves_totals() {
    let mut w = world();
    filler(&mut w, ADA, 40);
    filler(&mut w, SEL, 5);
    w.meta.beat = 40;
    assert!(fold_plan(&w.memory, SEL, 2, 10).is_none());
    let plan = fold_plan(&w.memory, ADA, 8, 20).unwrap();
    assert_eq!(plan.items.len(), 32);
    assert_eq!(plan.beats, (0, 31));
    let text = rules_summary(&w, &plan);
    assert!(
        text.contains("32 of them") && text.contains("Mostly about"),
        "{text}"
    );
    assert!(!has_engine_id(&text));
    let sid = apply_fold(&mut w.memory, &plan, &text, 40);
    let s = &w.memory.items[sid.ix()];
    assert_eq!((s.kind, s.beat, s.salience), (MemoryKind::Summary, 31, 10));
    assert_eq!(
        s.about,
        vec![VEX, TOMAS]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
    );
    let active = |w: &World, o| {
        w.memory
            .items
            .iter()
            .filter(|m| m.owner == o && is_active(m))
            .count()
    };
    assert_eq!(active(&w, ADA), 9);
    // Totals: nothing removed, every original accounted for.
    assert_eq!(w.memory.items.len(), 46);
    assert_eq!(
        w.memory
            .items
            .iter()
            .filter(|m| m.folded_into == Some(sid))
            .count(),
        32
    );
    assert!(w
        .memory
        .items
        .iter()
        .all(|m| (m.folded_into == Some(sid)) == (m.folded_beat == Some(40))));
    // Recall sees the summary, not the originals.
    let got = recall(
        &w,
        &Recall {
            owner: ADA,
            about: vec![VEX],
            max_items: 100,
            now: 40,
        },
    );
    assert_eq!(got.len(), 9);
    assert!(got.iter().any(|m| m.id == sid));

    // Global budget.
    filler(&mut w, TOMAS, 30);
    let made = enforce_budget(&mut w, 10, 40);
    assert_eq!(made.len(), 1, "only Tomas is over 10");
    assert!(active(&w, TOMAS) <= 10);
    assert_eq!(active(&w, ADA), 9);
    assert!(enforce_budget(&mut w, 10, 40).is_empty());
    texts_clean(&w);
}

fn scripted(seed: u64) -> World {
    let mut w = world();
    let mut rng = seed::Rng::new(seed);
    let minds = [ADA, SEL, TOMAS, VEX, DOCKERS];
    for b in 0..30u32 {
        let actor = *rng.pick(&minds).unwrap();
        let tgt = *rng.pick(&minds).unwrap();
        let wit = *rng.pick(&minds).unwrap();
        let g = rng.below(4) as u32;
        let row = push_row(
            &mut w,
            b,
            actor,
            "a deed in the night",
            &[tgt],
            rng.below(11) as u8,
            &[wit],
            &[],
            &[g],
        );
        write_row_memories(&mut w, row, b);
        if b % 7 == 3 {
            forget(&mut w, wit, &[actor]);
        }
        if b % 5 == 0 {
            plant(&mut w, tgt, "it was all a dream", &[actor], b);
        }
    }
    w.meta.beat = 30;
    enforce_budget(&mut w, 6, 30);
    w
}

#[test]
fn deterministic() {
    let a = serde_json::to_string(&scripted(9).memory).unwrap();
    let b = serde_json::to_string(&scripted(9).memory).unwrap();
    assert_eq!(a, b);
    let w = scripted(9);
    let ra = render(
        &w,
        &recall(
            &w,
            &Recall {
                owner: ADA,
                about: vec![VEX],
                max_items: 8,
                now: 30,
            },
        ),
    );
    let rb = render(
        &w,
        &recall(
            &w,
            &Recall {
                owner: ADA,
                about: vec![VEX],
                max_items: 8,
                now: 30,
            },
        ),
    );
    assert_eq!(ra, rb);
    texts_clean(&w);
}

/// 20k items over ~1,000 characters and a few hundred groups/clusters; 1,000 recalls.
/// Run: `cargo test -p fn7-memory --release -- --ignored --nocapture`
#[test]
#[ignore]
fn perf_recall_20k() {
    let mut w = world();
    let mut rng = seed::Rng::new(42);
    let ng = 200u32;
    let nc = 300u32;
    w.groups = (0..ng)
        .map(|i| group(i, &format!("group {i}"), 0.5))
        .collect();
    w.clusters = (0..nc)
        .map(|i| {
            cluster(
                i,
                &format!("cluster {i}"),
                &[i % ng, (i * 7 + 3) % ng, (i * 13 + 5) % ng],
            )
        })
        .collect();
    w.characters = (0..1000u32)
        .map(|i| {
            let c = i % nc;
            let gs = w.clusters[c as usize]
                .groups
                .as_slice()
                .iter()
                .map(|g| g.0)
                .collect::<Vec<_>>();
            character(
                i,
                &format!("person {i}"),
                Origin::Materialized {
                    cluster: ClusterId(c),
                    index: i,
                },
                &gs,
            )
        })
        .collect();
    let refs: Vec<Ref> = (0..1000u32).map(|i| Ref::Char(CharId(i))).collect();
    for i in 0..20_000u32 {
        let owner = match rng.below(10) {
            0 => Ref::Group(GroupId(rng.below(ng as u64) as u32)),
            1 => Ref::Cluster(ClusterId(rng.below(nc as u64) as u32)),
            _ => *rng.pick(&refs).unwrap(),
        };
        let about = vec![*rng.pick(&refs).unwrap(), *rng.pick(&refs).unwrap(), VEX];
        write(
            &mut w.memory,
            NewMemory {
                owner,
                beat: i / 200,
                kind: MemoryKind::Witnessed,
                text: format!("memory {i}"),
                about,
                ledger: None,
                from: None,
                salience: rng.below(11) as u8,
                confidence: 1.0,
            },
        );
    }
    let queries: Vec<Recall> = (0..1000)
        .map(|_| Recall {
            owner: *rng.pick(&refs).unwrap(),
            about: vec![*rng.pick(&refs).unwrap(), *rng.pick(&refs).unwrap()],
            max_items: 12,
            now: 100,
        })
        .collect();

    let t = Instant::now();
    let mut n_lin = 0;
    for q in &queries {
        n_lin += recall(&w, q).len();
    }
    let lin = t.elapsed();

    let t = Instant::now();
    let ix = MemoryIndex::build(&w.memory);
    let build = t.elapsed();
    let t = Instant::now();
    let mut n_ix = 0;
    for q in &queries {
        n_ix += recall_indexed(&w, &ix, q).len();
    }
    let idx = t.elapsed();
    for q in queries.iter().take(50) {
        assert_eq!(recall(&w, q), recall_indexed(&w, &ix, q));
    }
    let t = Instant::now();
    let mut rendered = 0;
    for q in queries.iter().take(1000) {
        rendered += render(&w, &recall_indexed(&w, &ix, q)).len();
    }
    let rend = t.elapsed();
    println!(
        "20k items, 1000 recalls: linear {lin:?} ({n_lin} items), index build {build:?}, indexed {idx:?} ({n_ix} items), \
         indexed+render {rend:?} ({rendered} bytes)"
    );
    assert_eq!(n_lin, n_ix);
    assert!(idx.as_millis() < 100, "indexed recall too slow: {idx:?}");
    if !cfg!(debug_assertions) {
        assert!(lin.as_millis() < 100, "linear recall too slow: {lin:?}");
    }
}

//! Rules 0.7.0: two demigods per player (the Left and Right hands, each its own body with its own health), the item
//! engine in the beat, and the per-player Final Night.

use std::sync::Arc;

use serde_json::{json, Value};
use theatre_plugin_api::*;

use crate::actions::Input;
use crate::testkit::Failing;
use crate::*;

fn humans(n: u8) -> Vec<SeatInfo> {
    (0..n)
        .map(|i| SeatInfo { seat: i, display_name: ["Ash", "Brix", "Cyra", "Dune"][i as usize % 4].into(), is_bot: false })
        .collect()
}

fn act(seat: Seat, fields: Value) -> Action {
    Action { fields, ..Action::new(seat, "") }
}

#[test]
fn every_player_has_two_full_demigods() {
    let w = testkit::world(4, 3);
    assert_eq!((w.meta.schema, w.demigods.len(), w.player_seats().len()), (9, 8, 4));
    for seat in w.player_seats() {
        let ds: Vec<&Demigod> = w.bodies_of_seat(seat).collect();
        assert_eq!(ds.len(), 2);
        let (l, r) = (ds[0], ds[1]);
        assert_eq!((l.side, r.side), (Some(Hand::Left), Some(Hand::Right)));
        assert_eq!(l.player, r.player);
        assert_eq!(l.name, format!("{}'s Left Hand", l.player), "the media layer's names, kept as defaults");
        assert_eq!(r.name, format!("{}'s Right Hand", r.player));
        assert_eq!((l.health, r.health, l.bolts), (DEFAULT_MAX_HEALTH, DEFAULT_MAX_HEALTH, r.bolts));
        let km = fn7_society::geo::distance_km(l.pos, r.pos);
        assert!((140.0..160.0).contains(&km), "the Right lands ~150 km from the Left: {km}");
        assert_eq!(l.following, r.following);
        assert_eq!(l.home, r.home);
        // Each body speaks through its own hand.
        for d in [l, r] {
            assert_eq!(d.hands[0], d.hands[1]);
            let a = &w.agents[d.hands[0].ix()];
            assert_eq!(a.name, d.name);
            assert!(matches!(a.kind, AgentKind::Hand { demigod, hand } if demigod == d.id && Some(hand) == d.side));
        }
        assert_ne!(people_cell(l.pos), people_cell(r.pos));
    }
    assert!(w.gods.iter().all(|g| g.favor.len() == 8));
}

#[test]
fn the_left_text_drives_the_left_demigod_and_the_right_text_the_right() {
    let mut w = testkit::world(2, 4);
    let input = Input { left: "go to Lagos".into(), right: "forge a sword".into(), zoom: None, casts: vec![], ..Default::default() };
    let turns = beat::split_turn(&w, 200, DemigodId(0), input.clone());
    assert_eq!(turns.len(), 2);
    assert_eq!((turns[0].demigod, turns[0].input.left.as_str(), turns[0].input.right.as_str()), (DemigodId(0), "go to Lagos", ""));
    assert_eq!((turns[1].demigod, turns[1].input.left.as_str(), turns[1].input.right.as_str()), (DemigodId(2), "", "forge a sword"));
    // A fallen Left: its orders go to the standing Right.
    w.demigods[0].alive = false;
    let turns = beat::split_turn(&w, 200, DemigodId(2), input);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].demigod, DemigodId(2));
    assert!(turns[0].input.right.contains("forge a sword") && turns[0].input.right.contains("go to Lagos"));
}

#[test]
fn health_is_per_body_and_the_purse_is_per_player() {
    let mut w = testkit::world(2, 5);
    let hit = Proposal {
        source: Source::Rules,
        intent: None,
        effect: Effect::Damage { who: Ref::Demigod(DemigodId(0)), amount: 10.0, cause: "a test".into() },
        act: "struck".into(),
        actor: Ref::God(GodId(0)),
        magnitude: 9,
        valence: -5,
        witnesses: vec![],
        cause: None,
    };
    fn7_society::apply(&mut w, &hit, 1).unwrap();
    assert!(!w.demigods[0].alive, "the Left falls");
    assert!(w.demigods[2].alive && w.demigods[2].health == DEFAULT_MAX_HEALTH, "its sibling is untouched");
    assert!(w.seat_alive(200), "the player stands while either demigod does");
    assert_eq!(w.lead_of_seat(200), Some(DemigodId(2)));
    assert!(!phases::match_over(&w, 1), "two players still stand");
    // A god's price paid from the Right: the purse is shared; each rival PLAYER gains 2 per bolt, once.
    w.set_bolts(DemigodId(2), 3);
    let before = w.demigods[1].bolts;
    let pay = Proposal {
        effect: Effect::Bolts { who: DemigodId(2), delta: -2, to: Some(GodId(6)), toll: false },
        actor: Ref::Demigod(DemigodId(2)),
        ..hit.clone()
    };
    fn7_society::apply(&mut w, &pay, 1).unwrap();
    assert_eq!((w.demigods[0].bolts, w.demigods[2].bolts), (1, 1), "both of the payer's demigods show the purse");
    assert_eq!((w.demigods[1].bolts, w.demigods[3].bolts), (before + 4, before + 4), "one gift per rival player");
}

#[tokio::test]
async fn the_final_night_leaves_exactly_one_player() {
    let p = FinalNight7Plugin::new();
    let settings = p.validate_settings(&json!({ "seed": 9, "players": 3, "beats_total": 5, "clusters": 40, "llm": false })).unwrap();
    let s = p.on_match_start(&humans(3), &settings, &Failing).await.unwrap();
    let s = p.on_round_start(&s, 5, &Failing).await.unwrap().state;
    let mut w = world::load(&s).unwrap();
    // Ash (dg:0, dg:3) owes the most; Cyra (dg:2, dg:5) has lost a demigod but owes nothing; Brix owes a little.
    for (d, amt) in [(0u32, 3.0f32), (3, 3.0), (1, 0.5)] {
        let debt = Proposal {
            source: Source::God { god: GodId(2), ruling: None },
            intent: None,
            effect: Effect::Debt { owed_by: Ref::Demigod(DemigodId(d)), god: GodId(2), amount: amt, due_in: 50 },
            act: "a bill".into(),
            actor: Ref::God(GodId(2)),
            magnitude: 1,
            valence: -1,
            witnesses: vec![],
            cause: None,
        };
        fn7_society::apply(&mut w, &debt, 5).unwrap();
    }
    w.demigods[5].alive = false;
    w.demigods[5].health = 0.0;
    w.demigods[5].eliminated_beat = Some(4);
    let c = p.commit_round(&world::save(&w), 5, &[], &Failing).await.unwrap();
    let w = world::load(&c.state).unwrap();
    // The most total standing (health minus debt): Brix 20 - 0.5 = 19.5, Ash 20 - 6 = 14, Cyra 10 (one demigod).
    assert_eq!(w.alive_seats(), vec![1], "the most standing walks away: Brix");
    assert!(w.demigods[1].alive && w.demigods[4].alive, "both of Brix's demigods stand");
    assert!(!w.demigods[2].alive && !w.demigods[5].alive);
    assert!(p.is_match_over(&c.state, 5));
    let sum = p.summarize(&c.state, &Failing).await.unwrap();
    assert_eq!(sum.winners, vec![1]);
    assert_eq!(sum.summary["champion_seat"], 1);
    assert_eq!(sum.summary["champions"].as_array().unwrap().len(), 2, "both standing demigods are the champion");
    let st = sum.summary["standings"].as_array().unwrap();
    assert_eq!(st.len(), 3);
    assert_eq!(st[0]["player"], "Brix");
    assert_eq!(st[0]["demigods"].as_array().unwrap().len(), 2, "each demigod's state shown");
    assert_eq!(sum.summary["players"].as_array().unwrap().len(), 3);
    let eliminated: Vec<Seat> = c.eliminated.iter().map(|x| x.0).collect();
    assert_eq!(eliminated, vec![0, 2], "Ash and Cyra are out, each reported once");
}

#[test]
fn a_horde_of_a_hundred_million_through_the_item_engine() {
    let st = Settings::default();
    let pantheon = Arc::new(fn7_pantheon::load());
    let mut made = None;
    for seed in 1..60u64 {
        let mut w = testkit::world(2, seed);
        let it = Intent {
            id: IntentId(0),
            beat: 1,
            agent: w.demigods[0].hands[0],
            demigod: DemigodId(0),
            verb: "raise".into(),
            targets: vec![],
            how: String::new(),
            place: None,
            magnitude: 7,
            span: "raise banners of 100,000,000 soldiers".into(),
            route: Route::World,
        };
        assert!(items::is_manifest_act(&w, &it));
        w.set_bolts(DemigodId(0), 3);
        let mut spent = Default::default();
        let j = items::judge(&w, &it, 1, 0, &mut spent, &pantheon, &st).unwrap();
        assert_eq!(j.m.verdict.tier, fn7_model::manifest::Tier::Myth);
        assert_eq!((j.m.verdict.cost.required, j.m.verdict.cost.bolts, j.m.verdict.cost.missing), (3, 3, 0));
        // Deterministic: the same seed, the same roll.
        let again = items::judge(&w, &it, 1, 0, &mut Default::default(), &pantheon, &st).unwrap();
        assert_eq!(j.m.verdict.roll, again.m.verdict.roll);
        let mut props = j.props.clone();
        props.push(items::proposal(&w, &j, None, false, &st));
        merge::run(&mut w, props, &pantheon, 1, 4);
        assert_eq!(w.manifests.len(), 1, "every attempt is a row");
        assert_eq!(w.demigods[0].bolts, 0, "three bolts paid from the purse");
        assert_eq!(w.demigods[2].bolts, 0, "the sibling shares the purse");
        if let Some(i) = w.manifests[0].item {
            made = Some((w.clone(), i));
            break;
        }
    }
    let (w, i) = made.expect("a myth succeeds or partially succeeds within 60 seeds");
    let it = &w.items[i.ix()];
    use fn7_model::manifest::*;
    assert_eq!(it.ext.kind, ItemKind::Army);
    assert!(it.ext.count == 1e8 || it.ext.count == 1e7, "a horde, or a tenth of one: {}", it.ext.count);
    assert!(it.name.starts_with("Banner of the"), "the rules name it, never a bare verb: {}", it.name);
    assert_eq!(it.holders.last(), Some(&Ref::Demigod(DemigodId(0))));
    assert_eq!(it.ext.state, ItemState::Deployed);
    let army = it.ext.muster.expect("the host takes the field");
    assert_eq!(w.clusters[army.ix()].army.as_ref().unwrap().demigod, DemigodId(0));
    assert!(w.clusters[army.ix()].size <= MUSTER_CAP);
    let v = items::item_json(&w, it);
    assert_eq!((v["icon"].as_str(), v["count"].as_f64()), (Some("banner_army"), Some(it.ext.count)));
}

#[tokio::test]
async fn a_players_left_hand_manifests_through_the_beat() {
    let p = FinalNight7Plugin::new();
    let settings = json!({ "seed": 12, "players": 2, "beats_total": 10, "clusters": 40, "llm": false, "start_bolts": 9 });
    let script = |seat: Seat, round: u32| -> Option<Action> {
        Some(match (seat, round) {
            (0, 1) => act(0, json!({ "left": "conjure a fortress overnight", "right": "go to the nearest city" })),
            _ => act(seat, json!({})),
        })
    };
    let mut seen = vec![];
    driver::run_match_with(&p, &humans(2), &settings, &Failing, Some(1), &script, &|_| (vec![], RoundContext::default()),
        |r: &driver::BeatReport, s: &Value| {
            seen.push((r.outcome.per_seat.clone(), world::load(s).unwrap()));
            true
        })
    .await
    .unwrap();
    let (per_seat, w) = &seen[0];
    let m = w.manifests.first().expect("the Left's order went to the item engine");
    assert_eq!((m.demigod, m.hand), (DemigodId(0), Some(Hand::Left)));
    assert_eq!(m.request.kind, fn7_model::manifest::ItemKind::Structure);
    // The seat's delta carries both demigods and the attempt with its full verdict.
    let mine = &per_seat.iter().find(|(s, _)| *s == 0).unwrap().1;
    assert_eq!(mine["demigods"].as_array().unwrap().len(), 2);
    assert!(mine["manifests"][0]["verdict"]["odds"].is_object());
    // Every demigod and named person carries its people-level cell in the views.
    let pv = phases::public_view(w);
    for d in pv["demigods"].as_array().unwrap() {
        assert!(d["cell"].as_str().unwrap().contains(":people:"), "{d}");
    }
    let sv = p.seat_view(&world::save(w), 0);
    assert_eq!(sv["demigods"].as_array().unwrap().len(), 2);
    assert!(sv["demigods"][0]["health"].is_number() && sv["demigods"][1]["health"].is_number(), "your own health, per body");
    assert_eq!(sv["hands"].as_array().unwrap().len(), 2);
    assert_eq!(sv["hands"][1]["demigod"], "dg:2");
    assert!(sv["aura"].as_str().unwrap().starts_with('#'));
}

#[test]
fn a_schema_4_world_keeps_one_demigod_per_seat_rules() {
    let mut w = testkit::world(2, 6);
    w.meta.schema = 4;
    assert!(!w.two_bodies());
    let it = Intent {
        id: IntentId(0),
        beat: 1,
        agent: AgentId(0),
        demigod: DemigodId(0),
        verb: "forge".into(),
        targets: vec![],
        how: String::new(),
        place: None,
        magnitude: 5,
        span: "forge a sword of stars".into(),
        route: Route::World,
    };
    assert!(!items::is_manifest_act(&w, &it), "no item engine before schema 5");
    // One demigod per seat: the old setup.
    let one = Settings::validate(&json!({ "players": 2, "seed": 6, "demigods_per_player": 1 })).unwrap();
    let w1 = phases::new_match(&[], &one, &fn7_pantheon::load(), 6);
    assert_eq!(w1.demigods.len(), 2);
    assert!(w1.demigods.iter().all(|d| d.side.is_none() && d.hands[0] != d.hands[1]));
}

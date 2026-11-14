//! The table: mid-match takeovers (`on_seat_takeover`), the `controller` / `connected` / `seat` fields on every public
//! demigod, and the diorama prompt.

use serde_json::{json, Value};
use theatre_plugin_api::*;

use crate::driver::run_match_joins;
use crate::testkit::Failing;
use crate::*;

/// Two Theatre bot seats (0, 1) and two built-in AI demigods (synthetic seats 200, 201).
async fn start(seed: u64) -> (FinalNight7Plugin, Value) {
    let p = FinalNight7Plugin::new();
    let seats: Vec<SeatInfo> = (0..2)
        .map(|i| SeatInfo { seat: i, display_name: ["Ash", "Brix"][i as usize].into(), is_bot: true })
        .collect();
    let settings = p.validate_settings(&json!({ "seed": seed, "players": 4, "beats_total": 10, "clusters": 40 })).unwrap();
    let s = p.on_match_start(&seats, &settings, &Failing).await.unwrap();
    (p, s)
}

fn takeover(seat: Seat, new_seat: bool) -> SeatTakeover {
    SeatTakeover {
        seat,
        display_name: "Benito".into(),
        round: 3,
        new_seat,
        previous_name: (!new_seat).then(|| "Brix".into()),
    }
}

fn demigods(v: &Value) -> Vec<Value> {
    v["demigods"].as_array().cloned().unwrap()
}

fn by_ref<'a>(ds: &'a [Value], r: &str) -> &'a Value {
    ds.iter().find(|d| d["ref"] == r).unwrap()
}

fn with_health(s: &Value, f: impl Fn(&mut Demigod)) -> Value {
    let mut w = world::load(s).unwrap();
    w.demigods.iter_mut().for_each(f);
    world::save(&w)
}

#[tokio::test]
async fn a_taken_over_bot_seat_keeps_its_demigod_and_turns_human() {
    let (p, s) = start(5).await;
    let before = p.public_view(&s);
    let d1 = by_ref(&demigods(&before), "dg:1").clone();
    assert_eq!((d1["seat"].as_u64(), d1["controller"].as_str(), d1["connected"].as_bool()), (Some(1), Some("ai"), Some(true)));
    let s2 = p.on_seat_takeover(&s, &takeover(1, false)).expect("state changes");
    let w = world::load(&s2).unwrap();
    let st = world::seat_state(&w, 1).unwrap();
    assert_eq!((st.demigod, st.is_bot), (DemigodId(1), false), "same demigod, now human");
    // Rules 0.8.0: the bot's demigods take the joiner's names (refs stable; hand agents follow; remembered).
    assert_eq!(w.demigods[1].name, "Benito's Left Hand", "renamed for the joiner");
    let right = w.siblings(DemigodId(1)).into_iter().find(|x| *x != DemigodId(1)).unwrap();
    assert_eq!(w.demigods[right.ix()].name, "Benito's Right Hand");
    assert_eq!(w.agents[w.demigods[1].hands[0].ix()].name, "Benito's Left Hand");
    assert!(w.dm.memory.iter().any(|e| e.kind == "seat.takeover" && e.text == "Benito took up the mantle of Brix's Left Hand"));
    assert_eq!((w.demigods[1].player.as_str(), w.seats[1].display_name.as_str()), ("Benito", "Benito"), "the joiner is the player now");
    let after = by_ref(&demigods(&p.public_view(&s2)), "dg:1").clone();
    assert_eq!(after["controller"], "human");
    assert!(after["connected"].is_null(), "presence is the core's: null for a human");
    assert_eq!(after["seat"], 1);
    assert_eq!(after["name"], "Benito's Left Hand");
    // Nothing else moved.
    for r in ["dg:0", "dg:2", "dg:3", "dg:4", "dg:6", "dg:7"] {
        assert_eq!(by_ref(&demigods(&before), r), by_ref(&demigods(&p.public_view(&s2)), r));
    }
    // Deterministic, and idempotent: the seat is already human.
    assert_eq!(p.on_seat_takeover(&s, &takeover(1, false)), Some(s2.clone()));
    assert_eq!(p.on_seat_takeover(&s2, &takeover(1, false)), None);
}

#[tokio::test]
async fn a_new_seat_takes_over_the_healthiest_live_ai_demigod() {
    let (p, s) = start(6).await;
    // dg:2 (synthetic 200) at 4 health, dg:3 (synthetic 201) at 7: the new seat gets dg:3.
    let s = with_health(&s, |d| {
        d.health = match d.id.0 {
            2 => 4.0,
            3 => 7.0,
            _ => d.health,
        }
    });
    assert!(p.seat_view(&s, 2).is_null());
    let s2 = p.on_seat_takeover(&s, &takeover(2, true)).expect("bound");
    let w = world::load(&s2).unwrap();
    assert_eq!(w.demigods[3].seat, 2);
    assert_eq!(world::seat_state(&w, 2).map(|x| (x.demigod, x.is_bot)), Some((DemigodId(3), false)));
    assert!(w.seats.windows(2).all(|x| x[0].seat < x[1].seat), "seats stay sorted");
    assert_eq!(w.seats.iter().filter(|x| x.seat >= SYNTHETIC_SEAT).count(), 1, "one AI demigod left");
    assert_eq!(p.seat_view(&s2, 2)["demigod"], "dg:3");
    let d3 = by_ref(&demigods(&p.public_view(&s2)), "dg:3").clone();
    assert_eq!((d3["seat"].as_u64(), d3["controller"].as_str()), (Some(2), Some("human")));
    // The joiner gets a brief, and their demigod's eliminations and score now reach the Theatre.
    let b = p.brief(&s2, 3, &Failing).await.unwrap();
    assert!(b.per_seat.iter().any(|(seat, v)| *seat == 2 && v["demigod"] == "dg:3"));
    // Deterministic: same state, same takeover, same bytes.
    assert_eq!(serde_json::to_string(&p.on_seat_takeover(&s, &takeover(2, true))).unwrap(), serde_json::to_string(&Some(s2.clone())).unwrap());
    // Ties: the lowest id.
    let tie = with_health(&s, |d| d.health = 6.0);
    let w = world::load(&p.on_seat_takeover(&tie, &takeover(2, true)).unwrap()).unwrap();
    assert_eq!(w.demigods[2].seat, 2);
    // A second new seat gets the last AI demigod; a third has none left.
    let s3 = p.on_seat_takeover(&s2, &takeover(3, true)).expect("the other one");
    assert_eq!(world::load(&s3).unwrap().demigods[2].seat, 3);
    assert_eq!(p.on_seat_takeover(&s3, &takeover(4, true)), None);
}

#[tokio::test]
async fn with_no_live_ai_demigod_the_joiner_spectates() {
    let (p, s) = start(7).await;
    let s = with_health(&s, |d| {
        if d.id.0 >= 2 {
            d.alive = false;
            d.health = 0.0;
        }
    });
    assert_eq!(p.on_seat_takeover(&s, &takeover(2, true)), None, "no state change");
    assert!(p.seat_view(&s, 2).is_null(), "no demigod: a spectator");
    assert!(demigods(&p.public_view(&s)).iter().all(|d| d["seat"] != 2));
    // Their orders are accepted and ignored.
    assert!(p.validate_action(&s, 2, "l: hello", &Value::Null, SubmitKind::Compose).is_ok());
}

#[tokio::test]
async fn every_public_demigod_carries_the_table_fields() {
    let p = FinalNight7Plugin::new();
    let seats = vec![
        SeatInfo { seat: 0, display_name: "Chris".into(), is_bot: false },
        SeatInfo { seat: 1, display_name: "Bot".into(), is_bot: true },
    ];
    let settings = p.validate_settings(&json!({ "seed": 8, "players": 3, "beats_total": 10, "clusters": 40 })).unwrap();
    let s = p.on_match_start(&seats, &settings, &Failing).await.unwrap();
    let want = [("dg:0", json!(0), "human", Value::Null), ("dg:1", json!(1), "ai", json!(true)), ("dg:2", Value::Null, "ai", json!(true))];
    let check = |what: &str, ds: &[Value], key: &str| {
        // Rules 0.7.0: two demigods per player; the Right ones are dg:3..5, with their player's table fields.
        assert_eq!(ds.len(), 6, "{what}");
        let right: Vec<(String, Value, &str, Value)> = want
            .iter()
            .map(|(r, s, c, n)| (format!("dg:{}", r[3..].parse::<u32>().unwrap() + 3), s.clone(), *c, n.clone()))
            .collect();
        let all: Vec<(String, Value, &str, Value)> =
            want.iter().map(|(r, s, c, n)| (r.to_string(), s.clone(), *c, n.clone())).chain(right).collect();
        for (r, seat, controller, connected) in &all {
            let d = ds.iter().find(|d| d[key] == r.as_str()).unwrap_or_else(|| panic!("{what}: no {r}"));
            assert_eq!(&d["seat"], seat, "{what} {r} seat");
            assert_eq!(d["controller"], *controller, "{what} {r} controller");
            assert_eq!(&d["connected"], connected, "{what} {r} connected");
        }
    };
    check("public_view", &demigods(&p.public_view(&s)), "ref");
    check("globe", &demigods(&p.query(&s, None, "globe", &json!({})).unwrap()), "ref");
    let st = p.query(&s, Some(0), "standings", &json!({})).unwrap();
    check("standings", st["standings"].as_array().unwrap(), "demigod");
    assert_eq!(st["players"].as_array().unwrap().len(), 3, "the standings' players: one row each");
    // The region query lists the live demigods in a region with the same fields.
    let home = world::load(&s).unwrap().demigods[0].home.expect("home region");
    let reg = p.query(&s, None, "region", &json!({ "ref": Ref::Place(home).to_string() })).unwrap();
    let d0 = by_ref(reg["demigods"].as_array().unwrap(), "dg:0").clone();
    assert_eq!((d0["controller"].as_str(), d0["seat"].as_u64()), (Some("human"), Some(0)));
}

#[tokio::test]
async fn the_diorama_prompt_is_stable_and_drawable_by_ref() {
    let (p, s) = start(9).await;
    let w = world::load(&s).unwrap();
    let ds = demigods(&p.public_view(&s));
    for d in &w.demigods {
        let v = by_ref(&ds, &Ref::Demigod(d.id).to_string());
        let prompt = v["diorama_prompt"].as_str().unwrap();
        assert_eq!(v["diorama_image_kind"], "diorama");
        assert_eq!(v["image_kind"], "portrait", "the portrait's kind is unchanged");
        let sheet = visual::demigod(&w, d);
        let home = &w.places[d.home.unwrap().ix()].name;
        assert!(prompt.contains(&d.name) && prompt.contains(&sheet.traits.prop), "{prompt}");
        assert!(!prompt.contains(home.as_str()), "identity only, no home region: {prompt}");
        assert!(prompt.contains("hexagonal diorama tile"), "{prompt}");
        assert!(!prompt.contains("dg:"), "no engine ids: {prompt}");
        // game.media {kind: diorama} answers the same subject.
        let m = p.query(&s, None, "media", &json!({ "kind": "diorama", "ref": Ref::Demigod(d.id).to_string() })).unwrap();
        assert_eq!((m["image_kind"].as_str(), m["image_prompt"].as_str()), (Some("diorama"), Some(prompt)));
        assert_eq!(m["visual"]["sheet_hash"], sheet.sheet_hash);
    }
    assert!(p.query(&s, None, "media", &json!({ "kind": "diorama", "ref": "ch:0" })).unwrap_err().contains("dg:"));
    // Stable for the match: the same prompt after beats of moving, fighting and a takeover.
    let first: Vec<Value> = ds.iter().map(|d| d["diorama_prompt"].clone()).collect();
    let mut seen = vec![];
    let none = |_: Seat, _: u32| None;
    let joins = |round: u32| if round == 2 { vec![takeover(1, false)] } else { vec![] };
    let settings = json!({ "seed": 9, "players": 4, "beats_total": 6, "clusters": 40 });
    let seats: Vec<SeatInfo> = (0..2)
        .map(|i| SeatInfo { seat: i, display_name: ["Ash", "Brix"][i as usize].into(), is_bot: true })
        .collect();
    run_match_joins(&p, &seats, &settings, &Failing, None, &none, &|_| (vec![], RoundContext::default()), &joins, |_, st| {
        seen.push(demigods(&p.public_view(st)).iter().map(|d| d["diorama_prompt"].clone()).collect::<Vec<_>>());
        true
    })
    .await
    .unwrap();
    assert!(!seen.is_empty());
    // Rules 0.8.0: only the taken-over seat's two demigods change, to the joiner's names (and their prewarmed images).
    let seat1: Vec<usize> = ds.iter().enumerate().filter(|(_, d)| d["seat"] == 1).map(|(i, _)| i).collect();
    assert_eq!(seat1.len(), 2);
    for x in &seen {
        for (i, (a, b)) in x.iter().zip(&first).enumerate() {
            if !seat1.contains(&i) {
                assert_eq!(a, b, "diorama prompts never change during a match, except a takeover's");
            }
        }
    }
    let last = seen.last().unwrap();
    assert!(seat1.iter().all(|i| last[*i].as_str().unwrap().contains("Benito's")), "the joiner's demigods");
}

#[tokio::test]
async fn a_joiner_drives_their_demigod_from_the_round_they_join() {
    let p = FinalNight7Plugin::new();
    let seats: Vec<SeatInfo> = (0..2)
        .map(|i| SeatInfo { seat: i, display_name: ["Ash", "Brix"][i as usize].into(), is_bot: true })
        .collect();
    let settings = json!({ "seed": 11, "players": 4, "beats_total": 12, "clusters": 40 });
    // Seat 2 is new at round 3: silent except at round 4.
    let script = |seat: Seat, round: u32| match (seat, round) {
        (2, 4) => Some(actions::action(2, "rally every crowd in sight to my banner", "", None)),
        _ => None,
    };
    let no_extras = |_: u32| (vec![], RoundContext::default());
    // (round, seat 2's demigod, every demigod's words_spoken) after each commit.
    type Words = Vec<(u32, Option<DemigodId>, Vec<u64>)>;
    async fn run(p: &FinalNight7Plugin, seats: &[SeatInfo], settings: &Value, script: &driver::Script<'_>,
                 extras: &driver::Extras<'_>, joins: &driver::Joins<'_>) -> Words {
        let mut words: Words = vec![];
        run_match_joins(p, seats, settings, &Failing, Some(10), script, extras, joins, |r, st| {
            let w = world::load(st).unwrap();
            let mine = world::seat_state(&w, 2).map(|x| x.demigod);
            words.push((r.round, mine, w.demigods.iter().map(|d| d.words_spoken).collect()));
            true
        })
        .await
        .unwrap();
        words
    }
    let joins = |round: u32| if round == 3 { vec![takeover(2, true)] } else { vec![] };
    let joined = run(&p, &seats, &settings, &script, &no_extras, &joins).await;
    let alone = run(&p, &seats, &settings, &script, &no_extras, &|_| vec![]).await;
    assert!(joined[1].1.is_none(), "no seat 2 before the join");
    let d = joined[2].1.expect("seat 2 has a demigod from round 3").ix();
    // Without the join the built-in AI speaks for that demigod after round 3; with it, only the joiner does: silent at
    // round 3, its 8 words at round 4, silent after.
    assert!(alone[9].2[d] > alone[2].2[d], "the AI would have spoken for it");
    let at_join = joined[1].2[d];
    assert_eq!(joined[2].2[d], at_join, "round 3: the joiner was silent and the AI no longer speaks for it");
    assert_eq!(joined[3].2[d], at_join + 8, "round 4: the joiner's 8 words reached its hands");
    assert!(joined[3..].iter().all(|x| x.2[d] == at_join + 8), "only the joiner speaks for it: {joined:?}");
}

#[tokio::test]
async fn portraits_and_dioramas_are_identity_only_across_matches() {
    // Two matches, different seeds; the AI demigods take the same names (in different slots is fine too).
    let (p, a) = start(3).await;
    let (_, b) = start(4242).await;
    let (wa, wb) = (world::load(&a).unwrap(), world::load(&b).unwrap());
    assert_ne!(wa.meta.seed, wb.meta.seed);
    let (da, db) = (demigods(&p.public_view(&a)), demigods(&p.public_view(&b)));
    let mut shared = 0;
    for x in &da {
        let Some(y) = db.iter().find(|y| y["name"] == x["name"]) else { continue };
        shared += 1;
        for k in ["portrait_prompt", "diorama_prompt"] {
            assert_eq!(x[k], y[k], "{k} of {} is the same in both matches", x["name"]);
        }
        assert_eq!(x["visual"]["sheet_hash"], y["visual"]["sheet_hash"]);
        let name = x["name"].as_str().unwrap();
        assert_eq!(x["portrait_prompt"].as_str().unwrap(), payload::portrait_prompt_for(name));
        assert_eq!(x["diorama_prompt"].as_str().unwrap(), payload::diorama_prompt_for(name));
    }
    assert_eq!(shared, 8, "the seats and the AI roster give the same eight demigod names (two per player)");
}

#[tokio::test]
async fn each_hand_is_a_demigod_with_its_own_identity_images() {
    let (p, a) = start(3).await;
    let (_, b) = start(77).await;
    let ha = p.seat_view(&a, 0)["hands"].clone();
    let hb = p.seat_view(&b, 0)["hands"].clone();
    let ha = ha.as_array().unwrap();
    assert_eq!(ha.len(), 2);
    assert_eq!(ha[0]["name"], "Ash's Left Hand");
    for (i, h) in ha.iter().enumerate() {
        let name = h["name"].as_str().unwrap();
        assert_eq!(h["portrait_prompt"].as_str().unwrap(), payload::portrait_prompt_for(name));
        assert_eq!(h["diorama_prompt"].as_str().unwrap(), payload::diorama_prompt_for(name));
        assert_eq!(h["visual"]["sheet_hash"].as_str().unwrap(), visual::demigod_by_name(name).sheet_hash);
        assert_eq!(h["portrait_prompt"], hb[i]["portrait_prompt"], "the same hand in another match: the same prompt");
    }
    assert_ne!(ha[0]["visual"]["sheet_hash"], ha[1]["visual"]["sheet_hash"], "left and right are different figures");
    // Rules 0.7.0: each hand IS one of the player's two demigods: the same name, portrait and diorama.
    let ds = demigods(&p.public_view(&a));
    for h in ha {
        let d = by_ref(&ds, h["demigod"].as_str().unwrap());
        assert_eq!((&d["name"], &d["portrait_prompt"], &d["diorama_prompt"]), (&h["name"], &h["portrait_prompt"], &h["diorama_prompt"]));
        assert_eq!(d["side"], h["side"]);
    }
    assert_eq!((ds[0]["side"].as_str(), ha[1]["side"].as_str()), (Some("left"), Some("right")));
    assert_eq!(payload::hand_names("Ash"), ["Ash's Left Hand".to_string(), "Ash's Right Hand".to_string()]);
}

/// Schema 5 (rules 0.7.0) keeps the old behaviour: a taken-over bot's demigods keep their names, so old recordings replay.
#[tokio::test]
async fn a_schema_5_takeover_keeps_the_names() {
    let (p, s) = start(5).await;
    let mut w = world::load(&s).unwrap();
    w.meta.schema = 5;
    let s5 = world::save(&w);
    let s2 = p.on_seat_takeover(&s5, &takeover(1, false)).expect("state changes");
    let w2 = world::load(&s2).unwrap();
    assert_eq!(w2.demigods[1].name, "Brix's Left Hand");
    assert!(w2.dm.memory.is_empty());
}

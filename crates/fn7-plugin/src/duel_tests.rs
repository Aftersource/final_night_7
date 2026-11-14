//! Rules 0.8.1: challenge mode (typed duels).

use serde_json::{json, Value};
use theatre_plugin_api::*;

use crate::testkit::Failing;
use crate::*;

fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    futures::executor::block_on(f)
}

/// Seat 0 is a human; seats 1..3 are Theatre bots.
fn seats() -> Vec<SeatInfo> {
    (0..4)
        .map(|i| SeatInfo { seat: i, display_name: ["Ash", "Brix", "Cyra", "Dune"][i as usize].into(), is_bot: i != 0 })
        .collect()
}

struct Table {
    p: FinalNight7Plugin,
    state: Value,
    me: DemigodId,
    foe: DemigodId,
}

/// A match at beat 2's start with seat 0's Left demigod and seat 1's Left demigod 1 km apart, both at full health.
fn table(seed: u64, extra: Value) -> Table {
    let p = FinalNight7Plugin::new();
    let mut s = json!({ "seed": seed, "beats_total": 30, "llm": false, "dm_encounters_per_beat": 0, "duel_ai_chance": 0.0 });
    if let (Some(o), Some(x)) = (s.as_object_mut(), extra.as_object()) {
        o.extend(x.clone());
    }
    let settings = p.validate_settings(&s).unwrap();
    let state = block(p.on_match_start(&seats(), &settings, &Failing)).unwrap();
    let state = block(p.on_round_start(&state, 1, &Failing)).unwrap().state;
    let o = block(p.resolve_round(&state, 1, &bot_actions(&p, &state), &RoundContext::default(), &Failing, &NoProgress)).unwrap();
    let state = block(p.commit_round(&o.state, 1, &[], &Failing)).unwrap().state;
    let mut w = world::load(&state).unwrap();
    let me = w.bodies_of_seat(0).next().unwrap().id;
    let foe = w.bodies_of_seat(1).next().unwrap().id;
    let at = w.demigods[me.ix()].pos;
    w.demigods[foe.ix()].pos = fn7_society::geo::destination(at, 90.0, 1.0);
    for d in w.demigods.iter_mut() {
        d.health = 10.0;
        d.alive = true;
        d.eliminated_beat = None;
    }
    let state = world::save(&w);
    let state = block(p.on_round_start(&state, 2, &Failing)).unwrap().state;
    Table { p, state, me, foe }
}

fn bot_actions(p: &FinalNight7Plugin, state: &Value) -> Vec<Action> {
    let mut v = vec![p.default_action(state, 0)];
    for seat in 1..4u8 {
        v.push(block(p.bot_action(state, seat, &Failing)).unwrap());
    }
    v
}

/// The test's wall clock origin: stamps are offsets from now (the live deadline check reads the real clock).
fn t0() -> i64 {
    static T: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *T.get_or_init(theatre_trace::now_ms)
}

/// An instant action stamped `at_ms` after [`t0`] (as the core stamps `seq` and `received_at_ms`).
fn instant(seat: u8, fields: Value, seq: u64, at_ms: i64) -> Action {
    Action { fields, instant: true, seq, received_at_ms: t0() + at_ms, ..Action::new(seat, "") }
}

fn challenge(t: &Table, level: &str) -> Value {
    json!({ "challenge": { "demigod": Ref::Demigod(t.me), "target": Ref::Demigod(t.foe), "level": level } })
}

fn line(id: &str, text: &str) -> Value {
    json!({ "duel": { "duel_id": id, "text": text } })
}

/// Submit as the core does: validate, then on_instant; returns the public event.
fn submit(t: &Table, a: &Action) -> Result<Value, String> {
    t.p.validate_action(&t.state, a.seat, &a.text, &a.fields, SubmitKind::Instant)?;
    t.p.on_instant(&t.state, a.seat, a).ok_or_else(|| "no event".to_string())
}

fn resolve(t: &Table, extra: &[Action]) -> RoundOutcome {
    resolve_with(&FinalNight7Plugin::new(), t, extra)
}

fn resolve_with(p: &FinalNight7Plugin, t: &Table, extra: &[Action]) -> RoundOutcome {
    let mut actions = bot_actions(p, &t.state);
    actions.extend(extra.iter().cloned());
    block(p.resolve_round(&t.state, 2, &actions, &RoundContext::default(), &Failing, &NoProgress)).unwrap()
}

#[test]
fn challenges_are_refused_with_the_clients_reasons() {
    let t = table(21, json!({}));
    let v = |f: Value, seat: u8| t.p.validate_action(&t.state, seat, "", &f, SubmitKind::Instant);
    let c = |a: DemigodId, b: DemigodId, level: &str| json!({ "challenge": { "demigod": Ref::Demigod(a), "target": Ref::Demigod(b), "level": level } });
    assert_eq!(v(c(t.foe, t.me, "villages"), 0), Err("not_yours".into()));
    assert_eq!(v(c(t.me, t.foe, "people"), 0), Err("too_far".into()), "1 km is beyond 4.5 m");
    let sib = world::load(&t.state).unwrap().siblings(t.me).into_iter().find(|x| *x != t.me).unwrap();
    assert_eq!(v(c(t.me, sib, "villages"), 0), Err("bad_target".into()), "your own sibling is no rival");
    assert_eq!(v(c(t.me, t.foe, "galaxy"), 0), Err("bad_level".into()));
    assert!(t.p.validate_action(&t.state, 0, "", &challenge(&t, "villages"), SubmitKind::Compose).is_err(), "instant only");
    assert!(v(json!({ "challenge": { "demigod": "dg:0", "target": "dg:1" }, "left": "go" }), 0).is_err(), "travels alone");
    // Fallen.
    let mut w = world::load(&t.state).unwrap();
    w.demigods[t.foe.ix()].alive = false;
    assert_eq!(t.p.validate_action(&world::save(&w), 0, "", &challenge(&t, "villages"), SubmitKind::Instant), Err("fallen".into()));
    // Accepted, then busy (neither may duel twice in a beat).
    submit(&t, &instant(0, challenge(&t, "villages"), 1, 1_000)).expect("accepted");
    assert_eq!(v(challenge(&t, "villages"), 0), Err("busy".into()));
    // Cooldown (duel_cooldown_beats 2): last beat's challenge still counts.
    let t2 = table(21, json!({ "duel_cooldown_beats": 2 }));
    let o = resolve(&t2, &[instant(0, challenge(&t2, "villages"), 1, 1_000)]);
    let s = block(t2.p.commit_round(&o.state, 2, &[], &Failing)).unwrap().state;
    let s = block(t2.p.on_round_start(&s, 3, &Failing)).unwrap().state;
    let mut w = world::load(&s).unwrap();
    for d in w.demigods.iter_mut() {
        d.health = 10.0;
        d.alive = true;
    }
    let pos = w.demigods[t2.me.ix()].pos;
    w.demigods[t2.foe.ix()].pos = fn7_society::geo::destination(pos, 90.0, 1.0);
    assert_eq!(t2.p.validate_action(&world::save(&w), 0, "", &challenge(&t2, "villages"), SubmitKind::Instant), Err("cooldown".into()));
    // A 0.8.0 match (settings without challenge_enabled) refuses: old recordings never see challenge mode.
    let mut old = world::load(&t.state).unwrap();
    old.meta.settings.as_object_mut().unwrap().remove("challenge_enabled");
    assert_eq!(FinalNight7Plugin::new().validate_action(&world::save(&old), 0, "", &challenge(&t, "villages"), SubmitKind::Instant),
               Err("challenge_disabled".into()));
}

#[test]
fn the_lock_shows_both_portraits_and_the_place() {
    let t = table(22, json!({}));
    let v = submit(&t, &instant(0, challenge(&t, "villages"), 7, 5_000)).unwrap();
    assert_eq!(v["event"], "challenge.lock");
    assert_eq!(v["duel_id"], format!("duel:2:{}:{}", t.me.0, t.foe.0));
    assert_eq!((v["a"]["demigod"].clone(), v["b"]["demigod"].clone()), (json!(Ref::Demigod(t.me)), json!(Ref::Demigod(t.foe))));
    for k in ["a", "b"] {
        assert!(v[k]["name"].as_str().unwrap().contains("Hand"));
        assert_eq!(v[k]["health"], 10.0);
        assert!(v[k]["portrait_prompt"].as_str().unwrap().len() > 20 && v[k]["diorama_prompt"].is_string());
    }
    assert_eq!((v["level"].as_str(), v["ends"].as_str(), v["seq"].as_u64()), (Some("villages"), Some("resolve"), Some(7)));
    assert!(v["cell"].as_str().unwrap().contains(":villages:"));
    assert_eq!((v["focus"]["at"].clone(), v["focus"]["cell"].clone(), v["focus"]["level"].clone()), (v["at"].clone(), v["cell"].clone(), v["level"].clone()));
    assert_eq!(v["at"], json!(world::load(&t.state).unwrap().demigods[t.foe.ix()].pos));
    assert_eq!(v["freeze"]["deadline_ms"], t0() + 5_000 + 30_000);
}

/// A duel typed live and the same actions resolved agree blow for blow; the dice land in `dice[]`; the duel ends.
#[test]
fn live_blows_and_the_resolve_agree() {
    let t = table(23, json!({}));
    let lock = submit(&t, &instant(0, challenge(&t, "villages"), 1, 10_000)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let texts = ["I kick your knee into the mud", "then I smash your jaw with my elbow", "your ribs, again, under this moon",
                 "I sweep your ankle and stomp your pride"];
    let mut acts = vec![instant(0, challenge(&t, "villages"), 1, 10_000)];
    let mut live = vec![];
    for (k, text) in texts.iter().enumerate() {
        let a = instant(0, line(&id, text), 2 + k as u64, 11_000 + 2_000 * k as i64);
        match submit(&t, &a) {
            Ok(v) => live.push(v),
            Err(e) => assert_eq!(e, "duel_over", "only a knockout stops it"),
        }
        acts.push(a);
    }
    assert!(!live.is_empty());
    for (k, b) in live.iter().enumerate() {
        assert_eq!((b["event"].as_str(), b["n"].as_u64()), (Some("duel.blow"), Some(k as u64 + 1)));
        assert!((1..=20).contains(&b["roll"].as_u64().unwrap()));
        let dmg = b["damage"].as_f64().unwrap();
        assert!([0.0, 1.0, 2.0, 3.0, 4.0].contains(&dmg), "{dmg}");
        assert!(b["flavor"].as_str().unwrap().len() > 20);
        assert!(b["counter"].is_object(), "the AI opponent counters every blow");
        for x in [&b["anim"], &b["counter"]["anim"]] {
            assert!(duel::MOVES.contains(&x["a"].as_str().unwrap()) && duel::MOVES.contains(&x["b"].as_str().unwrap()), "{x}");
            assert!(["wide", "close", "over_a", "over_b"].contains(&x["camera"].as_str().unwrap()));
            assert!(x["army"].is_boolean());
        }
        assert_eq!(b["counter"]["anim"]["attacker"], "b");
        for c in [&b["choreo"], &b["counter"]["choreo"]] {
            for s in ["a", "b"] {
                for k in c[s]["pose_keys"].as_array().unwrap() {
                    assert!(choreo::POSES.contains(&k["pose"].as_str().unwrap()), "{k}");
                }
            }
            assert!(["far", "mid", "close", "grapple"].contains(&c["range_after"].as_str().unwrap()));
            assert!((600..=1400).contains(&c["duration_ms"].as_u64().unwrap()));
        }
        assert_eq!(b["counter"]["by"], json!(Ref::Demigod(t.foe)));
    }
    let o = resolve(&t, &acts);
    let duels = o.public["duels"].as_array().expect("duels[]");
    assert_eq!(duels.len(), 1);
    let end = &duels[0];
    assert_eq!((end["event"].as_str(), end["duel_id"].as_str()), (Some("duel.end"), Some(id.as_str())));
    assert!(end["deed"].as_str().unwrap().len() > 20);
    let log = end["log"].as_array().unwrap();
    assert_eq!(log.len(), live.len());
    for (a, b) in log.iter().zip(&live) {
        for k in ["n", "roll", "modifier", "total", "target_dc", "verdict", "damage", "hp", "flavor", "counter", "choreo"] {
            assert_eq!(a[k], b[k], "{k} agrees live and at the resolve");
        }
    }
    // The world took the damage, the dice are public, both have a tale, the hex remembers.
    let w = world::load(&o.state).unwrap();
    let w0 = world::load(&t.state).unwrap();
    let taken = |d: DemigodId| w0.demigods[d.ix()].health - if w.demigods[d.ix()].alive { w.demigods[d.ix()].health } else { 0.0 };
    assert!((taken(t.foe) as f64 - end["damage"]["b"].as_f64().unwrap()).abs() < 1e-3 || !w.demigods[t.foe.ix()].alive);
    assert!(o.public["dice"].as_array().unwrap().iter().any(|x| x["reason"] == "a duel"));
    assert!(w.dm.story(t.me).unwrap().tales.iter().any(|x| x.kind == "duel"));
    assert!(w.dm.memory.iter().any(|e| e.kind == "duel.end" && e.query.as_deref() == Some(&format!("{id}:end"))));
    assert!(w.dm.memory.iter().any(|e| e.kind == "duel.lock"));
    // Over: the next beat refuses its lines.
    assert_eq!(t.p.validate_action(&o.state, 0, "", &line(&id, "one more"), SubmitKind::Instant), Err("duel_over".into()));
    // A fresh plugin (no live cache: a replay) resolves the same actions to the same state.
    assert_eq!(resolve(&t, &acts).state, o.state, "deterministic and cache-free");
}

#[test]
fn a_knockout_eliminates_and_drops_the_loot_on_the_duel_hex() {
    let t = table(24, json!({ "duel_damage_scale": 10.0 }));
    let mut w = world::load(&t.state).unwrap();
    w.demigods[t.foe.ix()].health = 1.0;
    let t = Table { state: world::save(&w), ..t };
    let lock = submit(&t, &instant(0, challenge(&t, "villages"), 1, 10_000)).unwrap();
    assert_eq!(lock["b"]["health"], 1.0);
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let mut acts = vec![instant(0, challenge(&t, "villages"), 1, 10_000)];
    let mut ko = false;
    for k in 0..30 {
        let a = instant(0, line(&id, &format!("I kick your knee into the mud, round {k}")), 2 + k, 11_000 + 2_000 * k as i64);
        match submit(&t, &a) {
            Ok(v) => {
                acts.push(a);
                if v["ko"] == true {
                    ko = true;
                    break;
                }
            }
            Err(e) => panic!("{e}"),
        }
    }
    assert!(ko, "someone was knocked out");
    assert_eq!(submit(&t, &instant(0, line(&id, "and stay down"), 99, 99_000)), Err("duel_over".into()));
    let o = resolve(&t, &acts);
    let end = &o.public["duels"][0];
    assert_eq!(end["ko"], true);
    let w2 = world::load(&o.state).unwrap();
    let loser = if end["loser"] == json!(Ref::Demigod(t.foe)) { t.foe } else { t.me };
    let l = &w2.demigods[loser.ix()];
    assert!(!l.alive, "a knockout is an elimination");
    assert!(l.cause.as_deref().unwrap_or("").contains("duel"), "{:?}", l.cause);
    assert!(o.public["eliminated"].as_array().unwrap().iter().any(|n| n == &json!(l.name)));
}

#[test]
fn spam_is_penalised_and_idle_humans_are_countered() {
    let t = table(25, json!({}));
    let lock = submit(&t, &instant(0, challenge(&t, "villages"), 1, 10_000)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let a = submit(&t, &instant(0, line(&id, "I jab your nose by the fountain"), 2, 11_000)).unwrap();
    let b = submit(&t, &instant(0, line(&id, "jab jab jab"), 3, 11_100)).unwrap();
    assert_ne!(a["verdict"], "penalty");
    assert_eq!((b["verdict"].as_str(), b["roll"].as_u64()), (Some("penalty"), Some(0)), "two lines in 100 ms");
    assert_eq!(b["self_damage"], 0.5);
    let c = submit(&t, &instant(0, line(&id, "now I hook your chin with a feint"), 4, 12_200)).unwrap();
    assert_eq!(c["modifiers"]["follow_up"], 1, "a quick follow-up within 1.5 s");
    // Wit: a specific line beats a lazy one.
    assert!(a["wit"].as_i64().unwrap() > b["wit"].as_i64().unwrap_or(0) - 1);
    // Against a human: countered only while they're idle (3 s).
    let t = table(26, json!({}));
    let mut w = world::load(&t.state).unwrap();
    w.seats.iter_mut().find(|s| s.seat == 1).unwrap().is_bot = false;
    let t = Table { state: world::save(&w), ..t };
    let lock = submit(&t, &instant(0, challenge(&t, "villages"), 1, 10_000)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let x = submit(&t, &instant(0, line(&id, "I kick your knee"), 2, 11_000)).unwrap();
    assert!(x["counter"].is_object(), "the human hasn't blown yet");
    let y = submit(&t, &instant(1, line(&id, "I bite your ear in the rain"), 3, 11_500)).unwrap();
    assert_eq!(y["by"], json!(Ref::Demigod(t.foe)));
    assert!(y.get("counter").is_none(), "seat 0 blew 0.5 s ago: no automatic counter for it");
    let z = submit(&t, &instant(0, line(&id, "I slam your ribs on the road"), 4, 12_000)).unwrap();
    assert!(z.get("counter").is_none(), "the human answered 0.5 s ago: no automatic counter");
    assert_eq!(t.p.validate_action(&t.state, 2, "", &line(&id, "me too"), SubmitKind::Instant), Err("not_yours".into()));
}

#[test]
fn the_built_in_ai_challenges_and_its_duels_fight_themselves() {
    let t = table(27, json!({ "duel_ai_chance": 1.0 }));
    // Seat 0 is human; the AI (seat 1) may challenge it or another rival: everyone near enough is AI-eligible.
    let rs = block(t.p.on_round_start(&{
        let mut w = world::load(&t.state).unwrap();
        w.meta.beat = 1;
        world::save(&w)
    }, 2, &Failing)).unwrap();
    let locks = rs.public["challenges"].as_array().cloned().unwrap_or_default();
    assert!(!locks.is_empty(), "an AI demigod with a rival in reach challenged");
    assert!(locks.iter().all(|l| l["ai"] == true && l["event"] == "challenge.lock"));
    let o = resolve(&Table { state: rs.state.clone(), ..t }, &[]);
    let duels = o.public["duels"].as_array().unwrap();
    assert_eq!(duels.len(), locks.len());
    assert_eq!(duels[0]["duel_id"], locks[0]["duel_id"]);
    assert!(duels[0]["blows"].as_u64().unwrap() >= 1, "fought automatically");
    assert!(duels[0]["log"][0]["auto"] == true);
}

#[test]
fn old_settings_never_duel() {
    // A 0.8.0 match: no AI challenges even at chance 1 (the key is absent), so its recordings replay unchanged.
    let t = table(28, json!({ "duel_ai_chance": 1.0 }));
    let mut w = world::load(&t.state).unwrap();
    w.meta.settings.as_object_mut().unwrap().remove("challenge_enabled");
    assert!(duel::ai_challenges(&w, 2, &world::settings(&w)).is_empty());
    let t = Table { state: world::save(&w), ..t };
    let o = resolve(&t, &[instant(0, challenge(&t, "villages"), 1, 10_000)]);
    assert!(o.public.get("duels").is_none());
}

/// Prints one real lock, blow and end (for docs/CLIENT_API.md). `cargo test -p fn7-plugin print_duel_examples -- --ignored --nocapture`
#[test]
#[ignore]
fn print_duel_examples() {
    let t = table(23, json!({}));
    let lock = submit(&t, &instant(0, challenge(&t, "villages"), 41, 10_000)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let blow = submit(&t, &instant(0, line(&id, "I sweep your ankle into the harbour mud"), 42, 11_000)).unwrap();
    let o = resolve(&t, &[instant(0, challenge(&t, "villages"), 41, 10_000), instant(0, line(&id, "I sweep your ankle into the harbour mud"), 42, 11_000)]);
    println!("LOCK {}\nBLOW {}\nEND {}", serde_json::to_string_pretty(&lock).unwrap(), serde_json::to_string_pretty(&blow).unwrap(),
             serde_json::to_string_pretty(&o.public["duels"][0]).unwrap());
}

/// Rules 0.8.1: the summoning's damage check comes at the accept (the resolve applies the same), and a challenge right
/// after a teleport fights from the destination at the reduced health.
#[test]
fn a_challenge_right_after_a_teleport_fights_at_the_reduced_health() {
    let t = table(29, json!({}));
    // Pull the rival 500 km away: out of villages reach until the summon lands next to it.
    let mut w = world::load(&t.state).unwrap();
    let here = w.demigods[t.me.ix()].pos;
    let far = fn7_society::geo::destination(here, 0.0, 500.0);
    w.demigods[t.foe.ix()].pos = far;
    let t = Table { state: world::save(&w), ..t };
    assert_eq!(t.p.validate_action(&t.state, 0, "", &challenge(&t, "villages"), SubmitKind::Instant), Err("too_far".into()));
    let dest = fn7_society::geo::destination(far, 90.0, 0.5);
    let summon = json!({ "summon": { "demigod": Ref::Demigod(t.me), "target": dest } });
    let s = submit(&t, &instant(0, summon.clone(), 1, 1_000)).unwrap();
    assert_eq!(s["event"], "summoning");
    let lost = s["health_lost"].as_f64().unwrap();
    assert!((1..=20).contains(&s["roll"].as_u64().unwrap()) && s["target_dc"].as_i64().is_some() && s["verdict"].is_string(), "{s}");
    assert_eq!(s["summons"][0]["health_lost"], s["health_lost"]);
    // Now the challenge is in reach, and the lock shows the reduced health.
    let lock = submit(&t, &instant(0, challenge(&t, "villages"), 2, 2_000)).expect("valid from the destination");
    assert!((lock["a"]["health"].as_f64().unwrap() - (10.0 - lost)).abs() < 1e-6, "{} vs {lost}", lock["a"]["health"]);
    assert_eq!(lock["b"]["health"], 10.0);
    let o = resolve(&t, &[instant(0, summon, 1, 1_000), instant(0, challenge(&t, "villages"), 2, 2_000)]);
    // The resolve applied the same mishap and fought the same duel.
    let summoned = o.public["summons"].as_array().unwrap().first().cloned().expect("summoned");
    assert_eq!((summoned["roll"].clone(), summoned["health_lost"].as_f64()), (s["roll"].clone(), Some(lost)));
    let end = &o.public["duels"][0];
    assert_eq!(end["duel_id"], lock["duel_id"]);
    let w2 = world::load(&o.state).unwrap();
    let d = &w2.demigods[t.me.ix()];
    if d.alive {
        let expect = 10.0 - lost - end["damage"]["a"].as_f64().unwrap();
        assert!((d.health as f64 - expect).abs() < 0.11, "{} vs {expect}", d.health);
        assert!(fn7_society::geo::distance_km(d.pos, dest) < 0.1, "it landed");
    }
}

/// Rules 0.8.1: an AI demigod whose rival is out of reach summons itself next to it, then challenges (the leap).
#[test]
fn the_ai_leaps_to_a_far_rival_before_it_challenges() {
    let t = table(30, json!({ "duel_ai_chance": 1.0, "duel_ai_reach_km": 20000.0 }));
    let w0 = world::load(&t.state).unwrap();
    let mut w = w0.clone();
    w.meta.beat = 1;
    let rs = block(t.p.on_round_start(&world::save(&w), 2, &Failing)).unwrap();
    let locks = rs.public["challenges"].as_array().cloned().unwrap_or_default();
    let leap = locks.iter().find(|l| l["leap"].is_object()).cloned().expect("someone leapt");
    let a: Ref = leap["a"]["demigod"].as_str().unwrap().parse().unwrap();
    let Ref::Demigod(a) = a else { panic!() };
    let lost = leap["leap"]["health_lost"].as_f64().unwrap();
    let start = world::load(&rs.state).unwrap().demigods[a.ix()].health as f64;
    assert!((leap["a"]["health"].as_f64().unwrap() - ((start - lost) * 10.0).round() / 10.0).abs() < 1e-6);
    assert_eq!(leap["level"], "people");
    let o = resolve(&Table { state: rs.state.clone(), ..t }, &[]);
    let w2 = world::load(&o.state).unwrap();
    assert!(o.public["duels"].as_array().unwrap().iter().any(|d| d["duel_id"] == leap["duel_id"]));
    assert!(o.public["dice"].as_array().unwrap().iter().any(|x| x["reason"] == "a summoning" && x["who"] == json!(Ref::Demigod(a))));
    let to: LonLat = serde_json::from_value(leap["leap"]["to"].clone()).unwrap();
    if w2.demigods[a.ix()].alive {
        assert!(fn7_society::geo::distance_km(w2.demigods[a.ix()].pos, to) < 50.0, "it leapt");
    }
    assert_eq!(w2.dm.story(a).unwrap().last_summon, Some(2));
}

// ---- Rules 0.8.1 live fix: heals and intents that must never vanish (match 79fd73bf) ------------------------------

/// "heal all my demigods" reaches Nature, is ruled on, and heals the player's own two demigods (not the rivals).
#[test]
fn heal_all_my_demigods_heals_your_own_two() {
    let t = table(31, json!({}));
    let mut w = world::load(&t.state).unwrap();
    let mine: Vec<DemigodId> = w.bodies_of_seat(0).map(|d| d.id).collect();
    for d in &mine {
        w.demigods[d.ix()].health = 5.0;
    }
    w.demigods[t.foe.ix()].health = 5.0;
    w.set_bolts(mine[0], 10);
    let t = Table { state: world::save(&w), ..t };
    let p = FinalNight7Plugin::new();
    let mut actions = bot_actions(&p, &t.state);
    actions[0] = actions::action(0, "heal all my demigods", "", None);
    let o = block(p.resolve_round(&t.state, 2, &actions, &RoundContext::default(), &Failing, &NoProgress)).unwrap();
    let w2 = world::load(&o.state).unwrap();
    let nature = p.pantheon().id_of("nature").unwrap();
    let ruling = w2.rulings.iter().find(|r| r.beat == 2 && r.god == nature).expect("Nature ruled on the heal");
    let healed: Vec<DemigodId> = w2
        .ledger
        .rows
        .iter()
        .filter(|r| r.beat == 2 && r.verb == "heal")
        .flat_map(|r| r.targets.iter().filter_map(|t| match t { Ref::Demigod(d) => Some(*d), _ => None }))
        .collect();
    assert_ne!(ruling.verdict, Verdict::Refuse, "Nature's rules fallback grants a paid heal");
    for d in &mine {
        assert!(healed.contains(d), "{d} healed: {healed:?} ({:?})", ruling.verdict);
    }
    assert!(!healed.contains(&t.foe), "rivals are not healed by 'my demigods'");
}

/// A 0.8.1 match routes a god-routed intent that names no god to Entropy (a visible ruling), never to silence.
#[test]
fn an_unroutable_god_intent_gets_a_ruling() {
    // The hand's routing is unit-tested in fn7-agents (minds.rs); here the settings gate it reaches the hand with.
    let t = table(32, json!({}));
    let w = world::load(&t.state).unwrap();
    assert!(world::settings(&w).rules_0_8_1);
    let mut old = w.clone();
    old.meta.settings.as_object_mut().unwrap().remove("rules_0_8_1");
    assert!(!world::settings(&old).rules_0_8_1, "0.8.0 recordings keep the old routing");
}

/// An invoke mid-duel costs its player a bolt at the resolve (and works from any range).
#[test]
fn an_invoke_costs_a_bolt() {
    let t = table(33, json!({}));
    let mut w = world::load(&t.state).unwrap();
    w.set_bolts(t.me, 3);
    let t = Table { state: world::save(&w), ..t };
    let lock = submit(&t, &instant(0, challenge(&t, "villages"), 1, 10_000)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let b = submit(&t, &instant(0, line(&id, "Death, take him"), 2, 11_000)).unwrap();
    assert_eq!(b["choreo"]["a"]["move_class"], "invoke");
    let o = resolve(&t, &[instant(0, challenge(&t, "villages"), 1, 10_000), instant(0, line(&id, "Death, take him"), 2, 11_000)]);
    let w2 = world::load(&o.state).unwrap();
    assert!(w2.demigods[t.me.ix()].bolts < 3, "the invoke's bolt was paid ({} left)", w2.demigods[t.me.ix()].bolts);
}

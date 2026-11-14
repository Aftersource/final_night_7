//! Whole matches through the `GamePlugin` trait, offline.

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theatre_plugin_api::*;

use crate::driver::{run_match, BeatReport};
use crate::testkit::{Failing, Scripted, Slow};
use crate::*;

fn bots(n: u8) -> Vec<SeatInfo> {
    (0..n)
        .map(|i| SeatInfo {
            seat: i,
            display_name: ["Ash", "Brix", "Cyra", "Dune"][i as usize % 4].into(),
            is_bot: true,
        })
        .collect()
}

fn none(_: Seat, _: u32) -> Option<Action> {
    None
}

fn load(s: &Value) -> World {
    world::load(s).unwrap()
}

struct Run {
    states: Vec<Value>,
    reports: Vec<(Value, Vec<(Seat, Value)>)>,
    summary: Summary,
    last: Value,
}

async fn play(
    settings: Value,
    llm: &dyn LlmProvider,
    seats: Vec<SeatInfo>,
    stop: Option<u32>,
) -> Run {
    let p = FinalNight7Plugin::new();
    let mut states = vec![];
    let mut reports = vec![];
    let (last, summary) = run_match(
        &p,
        &seats,
        &settings,
        llm,
        stop,
        &none,
        |r: &BeatReport, s: &Value| {
            states.push(s.clone());
            reports.push((r.outcome.public.clone(), r.outcome.per_seat.clone()));
            true
        },
    )
    .await
    .unwrap();
    Run {
        states,
        reports,
        summary,
        last,
    }
}

fn one_winner(r: &Run) {
    let w = load(&r.last);
    // Rules 0.7.0: exactly one PLAYER stands (one or both of their demigods).
    assert_eq!(w.alive_seats().len(), 1, "exactly one player standing at the end");
    assert!((1..=2).contains(&w.alive_demigods().count()));
    assert_eq!(
        r.summary.winners.len(),
        1,
        "{:?}",
        r.summary.summary["champions"]
    );
    let champ = w.alive_demigods().next().unwrap();
    assert_eq!(r.summary.winners[0], champ.seat);
    assert_eq!(r.summary.summary["piles"].as_array().unwrap().len(), w.demigods.len());
    assert_eq!(r.summary.summary["players"].as_array().unwrap().len(), 4, "one row per player");
    assert_eq!(r.summary.summary["standings"].as_array().unwrap().len(), 4, "standings are per player");
    assert!(r.summary.summary["piles"][0]["statue_prompt"]
        .as_str()
        .unwrap()
        .contains("amiibo"));
}

#[tokio::test(flavor = "multi_thread")]
async fn full_match_on_rules_fallbacks() {
    let r = play(
        json!({"seed": 11, "beats_total": 30, "clusters": 80}),
        &Failing,
        bots(4),
        None,
    )
    .await;
    one_winner(&r);
    let w = load(&r.last);
    assert!(w.meta.beat <= 30);
    assert!(
        w.ledger.rows.len() > 50,
        "the world moved: {} rows",
        w.ledger.rows.len()
    );
    assert!(
        w.beats
            .iter()
            .all(|b| b.timing.llm_fallbacks == b.timing.llm_calls),
        "every call fell back"
    );
    assert!(
        w.agents
            .iter()
            .any(|a| matches!(a.kind, AgentKind::Subagent { .. })),
        "bots spawn subagents"
    );
    assert!(!w.rulings.is_empty(), "bots invoke gods");
    assert!(
        w.ledger.rows.iter().any(|r| r.verb == "collect"),
        "debts get collected"
    );
    // Per-stage timings are reported on every beat.
    for (public, _) in &r.reports {
        let stages: Vec<&str> = public["timing"]["stages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["stage"].as_str().unwrap())
            .collect();
        assert_eq!(
            stages,
            [
                "intake",
                "route",
                "act",
                "merge",
                "consequence",
                "narrate",
                "broadcast"
            ]
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_final_night_leaves_exactly_one() {
    // Too short for anyone to fall before the Final Night: the survivor rule picks one.
    let r = play(
        json!({"seed": 2, "beats_total": 4, "clusters": 40}),
        &Failing,
        bots(4),
        None,
    )
    .await;
    let w = load(&r.last);
    assert_eq!(w.meta.beat, 4, "the Final Night is played");
    one_winner(&r);
    assert!(w
        .demigods
        .iter()
        .filter(|d| !d.alive)
        .all(|d| d.eliminated_beat == Some(4) || d.eliminated_beat.is_some()));
    assert!(
        w.ledger
            .rows
            .iter()
            .any(|r| r.verb == "eliminate" && r.beat == 4),
        "the others fall on the Final Night"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn full_match_with_scripted_llm() {
    let llm = Scripted::default();
    let r = play(
        json!({"seed": 5, "beats_total": 20, "clusters": 80}),
        &llm,
        bots(4),
        None,
    )
    .await;
    one_winner(&r);
    let w = load(&r.last);
    let calls: u32 = w.beats.iter().map(|b| b.timing.llm_calls).sum();
    let fb: u32 = w.beats.iter().map(|b| b.timing.llm_fallbacks).sum();
    assert!(calls > 0);
    assert!(
        fb * 10 <= calls,
        "scripted replies parse: {fb} fallbacks of {calls}"
    );
    assert!(
        w.rulings.iter().any(|r| !r.fallback),
        "god rulings from the LLM"
    );
    assert!(
        w.agents
            .iter()
            .any(|a| matches!(a.kind, AgentKind::Possession { .. })),
        "possessions"
    );
    assert!(
        w.agents
            .iter()
            .any(|a| matches!(a.kind, AgentKind::Paladin { .. })),
        "paladins"
    );
    assert!(
        w.beats.iter().any(|b| b.caption.starts_with("Beat chaos")),
        "narrator captions"
    );
    assert_eq!(r.summary.summary["final_cut"]["title"], "The Last Bill");
    // Beat N's caption is narrated during beat N+1 and arrives in its outcome; the last one in the summary.
    for (i, (public, _)) in r.reports.iter().enumerate().skip(1) {
        let w = load(&r.states[i]);
        let prev = &w.beats.iter().find(|b| b.beat == i as u32).unwrap().caption;
        assert!(!prev.is_empty(), "beat {i} narrated during beat {}", i + 1);
        assert_eq!(public["previous_caption"].as_str().unwrap(), prev);
        assert_eq!(public["caption_beat"], json!(i));
    }
    assert!(r.reports[0].0["previous_caption"] == "", "nothing to narrate before beat 1");
    let last = w.beats.last().unwrap().beat;
    assert_eq!(r.summary.summary["last_caption"]["beat"], json!(last));
    assert!(r.summary.summary["last_caption"]["caption"]
        .as_str()
        .unwrap()
        .starts_with("Beat chaos"));
    let per_beat = calls as f64 / w.beats.len() as f64;
    eprintln!(
        "scripted LLM: {calls} calls over {} beats = {per_beat:.1} per beat ({fb} fallbacks)",
        w.beats.len()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn deterministic_given_seed_and_replies() {
    let s = json!({"seed": 21, "beats_total": 12, "clusters": 60});
    let a = play(s.clone(), &Scripted::default(), bots(4), None).await;
    let b = play(s.clone(), &Scripted::default(), bots(4), None).await;
    assert_eq!(a.states.len(), b.states.len());
    for (i, (x, y)) in a.states.iter().zip(&b.states).enumerate() {
        assert!(x == y, "state after beat {} differs", i + 1);
    }
    let c = play(s, &Failing, bots(4), None).await;
    let d = play(
        json!({"seed": 21, "beats_total": 12, "clusters": 60}),
        &Failing,
        bots(4),
        None,
    )
    .await;
    assert_eq!(c.states, d.states);
    let e = play(
        json!({"seed": 22, "beats_total": 12, "clusters": 60}),
        &Failing,
        bots(4),
        None,
    )
    .await;
    assert_ne!(
        c.states.last(),
        e.states.last(),
        "another seed, another match"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn hidden_information() {
    let p = FinalNight7Plugin::new();
    let mut seats = bots(4);
    seats[0].is_bot = false;
    seats[0].display_name = "Secret".into();
    const SECRET: &str = "zebra-seventeen";
    let script = |seat: Seat, _round: u32| {
        (seat == 0).then(|| {
            actions::action(
                0,
                &format!("whisper {SECRET} to the baker who saw the fire"),
                &format!("send a scout to spy on {SECRET}"),
                None,
            )
        })
    };
    let mut checked = 0;
    let settings = json!({"seed": 3, "beats_total": 10, "clusters": 60});
    run_match(
        &p,
        &seats,
        &settings,
        &Failing,
        None,
        &script,
        |r: &BeatReport, s: &Value| {
            let w = load(s);
            for (seat, v) in &r.outcome.per_seat {
                let text = v.to_string();
                if *seat != 0 {
                    assert!(
                        !text.contains(SECRET),
                        "seat {seat}'s round.private leaks seat 0's words"
                    );
                    assert!(
                        !p.seat_view(s, *seat).to_string().contains(SECRET),
                        "seat {seat}'s view leaks seat 0's words"
                    );
                }
                let d: SeatDelta = serde_json::from_value(v.clone()).unwrap();
                let dg = world::seat_state(&w, *seat).unwrap().demigod;
                let me = world::selves(&w, dg);
                let following = w.demigods[dg.ix()].following;
                for n in &d.news {
                    let Ref::Ledger(id) = n.row else {
                        panic!("news row {} is not a ledger ref", n.row)
                    };
                    let row = &w.ledger.rows[id.ix()];
                    assert!(
                        world::knows(&me, following, row),
                        "seat {seat} got row {} it doesn't know",
                        n.row
                    );
                    checked += 1;
                }
            }
            assert!(
                !p.public_view(s).to_string().contains(SECRET),
                "the public view leaks seat 0's words"
            );
            // Rows the seat doesn't know never reach it.
            for (seat, v) in &r.outcome.per_seat {
                let d: SeatDelta = serde_json::from_value(v.clone()).unwrap();
                let dg = world::seat_state(&w, *seat).unwrap().demigod;
                let me = world::selves(&w, dg);
                let following = w.demigods[dg.ix()].following;
                let known: Vec<LedgerId> = w
                    .ledger
                    .rows
                    .iter()
                    .filter(|x| x.beat == r.round && world::knows(&me, following, x))
                    .map(|x| x.id)
                    .collect();
                let got: Vec<LedgerId> = d
                    .news
                    .iter()
                    .filter_map(|n| match n.row {
                        Ref::Ledger(id) => Some(id),
                        _ => None,
                    })
                    .collect();
                assert_eq!(got.len(), d.news.len());
                assert!(got.iter().all(|g| known.contains(g)));
            }
            true
        },
    )
    .await
    .unwrap();
    assert!(checked > 0, "some news was checked");
}

#[tokio::test(flavor = "multi_thread")]
async fn slow_llm_falls_back_within_the_timeout() {
    let timeout_ms = 250u64;
    let llm = Slow(Duration::from_secs(3));
    let p = FinalNight7Plugin::new();
    let settings =
        json!({"seed": 9, "beats_total": 50, "clusters": 60, "mind_timeout_ms": timeout_ms});
    let mut worst = 0f64;
    let t = Instant::now();
    run_match(
        &p,
        &bots(4),
        &settings,
        &llm,
        Some(3),
        &none,
        |r: &BeatReport, _s: &Value| {
            let ms = r.outcome.public["timing"]["total_ms"].as_f64().unwrap();
            worst = worst.max(ms);
            // Beats where someone spoke or cast ask the LLM (and fall back); quiet flow beats ask nothing.
            let t = &r.outcome.public["timing"];
            assert_eq!(t["llm_fallbacks"], t["llm_calls"]);
            true
        },
    )
    .await
    .unwrap();
    // Two sequential mind stages (hands, act batch; last beat's narrator runs alongside them), each capped by the
    // per-call timeout; the bound keeps the old three-stage margin.
    let bound = 3.0 * timeout_ms as f64 + 750.0;
    eprintln!(
        "slow LLM: worst beat {worst:.0} ms (bound {bound:.0} ms); 3 beats + summary in {:?}",
        t.elapsed()
    );
    assert!(worst < bound, "worst beat {worst} ms ≥ {bound} ms");
}

/// Answers like `Scripted`, after a delay that depends on the mind (the narrator is the slow one).
struct Delayed {
    narrator: Duration,
    other: Duration,
}

#[async_trait::async_trait]
impl LlmProvider for Delayed {
    async fn complete(&self, req: LlmRequest) -> anyhow::Result<LlmResponse> {
        let d = if req.system.contains("NARRATOR of FINAL NIGHT 7, a battle royale") {
            self.narrator
        } else {
            self.other
        };
        tokio::time::sleep(d).await;
        Scripted::default().complete(req).await
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_narrator_is_off_the_critical_path() {
    // Hands and the act batch take 300 ms each, the narrator 500 ms: run in sequence a beat would take ~1100 ms; with
    // last beat's narrator alongside the hands and the act batch, ~600 ms.
    let llm = Delayed {
        narrator: Duration::from_millis(500),
        other: Duration::from_millis(300),
    };
    let p = FinalNight7Plugin::new();
    let settings = json!({"seed": 4, "beats_total": 50, "clusters": 60, "mind_timeout_ms": 5000});
    let mut worst = 0f64;
    run_match(
        &p,
        &bots(4),
        &settings,
        &llm,
        Some(4),
        &none,
        |r: &BeatReport, _s: &Value| {
            let t = &r.outcome.public["timing"];
            assert_eq!(t["llm_fallbacks"], 0, "beat {}", r.round);
            if r.round > 1 {
                worst = worst.max(t["total_ms"].as_f64().unwrap());
                assert!(!r.outcome.public["previous_caption"].as_str().unwrap().is_empty());
            }
            true
        },
    )
    .await
    .unwrap();
    eprintln!("narrator overlapped: worst beat {worst:.0} ms");
    assert!(worst < 950.0, "worst beat {worst} ms: the narrator is on the critical path");
}

#[tokio::test(flavor = "multi_thread")]
async fn settings_and_actions() {
    let p = FinalNight7Plugin::new();
    assert_eq!(p.info().min_active, 0);
    let settings = p
        .validate_settings(&json!({"rounds": 5, "seed": 1, "players": 4, "clusters": 40}))
        .unwrap();
    let seats = bots(2);
    let s = p.on_match_start(&seats, &settings, &Failing).await.unwrap();
    let w = load(&s);
    assert_eq!(w.demigods.len(), 8, "AI players fill the empty places; two demigods per player (rules 0.7.0)");
    assert_eq!(w.player_seats().len(), 4);
    assert_eq!(
        w.seats.iter().filter(|x| x.seat >= SYNTHETIC_SEAT).count(),
        2
    );
    assert_eq!(w.meta.beats_total, 5);
    assert!(p.validate_action(&s, 0, "l: hi", &Value::Null, SubmitKind::Compose).is_ok());
    assert!(p
        .validate_action(
            &s,
            0,
            "",
            &json!({"left": "go", "zoom": {"level": "district", "focus": "pl:1"}}), SubmitKind::Compose
        )
        .is_ok());
    assert!(p
        .validate_action(&s, 0, "", &json!({"zoom": "nowhere"}), SubmitKind::Compose)
        .is_err());
    let rs = p.on_round_start(&s, 1, &Failing).await.unwrap();
    assert_eq!(load(&rs.state).meta.beat, 1, "round = beat");
    let brief = p.brief(&rs.state, 1, &Failing).await.unwrap();
    assert_eq!(brief.per_seat.len(), 2, "no briefs for AI demigods");
    assert!(brief.per_seat[0].1["visible"].as_array().unwrap().len() > 3);
    // The core's fallback path (resolve failed twice): rules only, synchronous, still a full beat.
    let acts = vec![
        actions::action(0, "recruit the dockworkers", "slow time", None),
        p.default_action(&rs.state, 1),
    ];
    let fo = p.fallback_outcome(&rs.state, 1, &acts, &RoundContext::default());
    assert_eq!(fo.public["timing"]["llm_calls"], 0);
    assert!(load(&fo.state).ledger.rows.len() > load(&rs.state).ledger.rows.len());
    let pv = p.public_view(&rs.state);
    assert!(
        pv["demigods"][0].get("health").is_none(),
        "rival health is private"
    );
}

/// Drive `beats` beats through the trait, ignoring `is_match_over` (the world keeps turning after the last demigod
/// stands), returning per-beat (total ms, state bytes, LLM calls) and the last state.
/// `immortal`: after each commit every demigod is set back on its feet (a stress test: four active demigods for all
/// 100 beats, the worst case for state growth).
async fn drive(
    settings: Value,
    llm: &dyn LlmProvider,
    beats: u32,
    immortal: bool,
) -> (Vec<(f64, usize, u64)>, Value) {
    let p = FinalNight7Plugin::new();
    let settings = p.validate_settings(&settings).unwrap();
    let mut state = p.on_match_start(&bots(4), &settings, llm).await.unwrap();
    let mut out = vec![];
    for round in 1..=beats {
        state = p.on_round_start(&state, round, llm).await.unwrap().state;
        let mut actions = vec![];
        for s in 0..4u8 {
            actions.push(p.bot_action(&state, s, llm).await.unwrap());
        }
        let o = p
            .resolve_round(&state, round, &actions, &RoundContext::default(), llm, &NoProgress)
            .await
            .unwrap();
        state = p
            .commit_round(&o.state, round, &[], llm)
            .await
            .unwrap()
            .state;
        if immortal && round < beats {
            let mut w = load(&state);
            for d in w.demigods.iter_mut() {
                d.max_health = d.max_health.max(5.0);
                d.health = d.max_health;
                d.alive = true;
                d.eliminated_beat = None;
                d.cause = None;
                for h in d.hands {
                    w.agents[h.ix()].status = AgentStatus::Live;
                    w.agents[h.ix()].ended_beat = None;
                }
            }
            state = world::save(&w);
        }
        out.push((
            o.public["timing"]["total_ms"].as_f64().unwrap(),
            serde_json::to_vec(&state).unwrap().len(),
            o.public["timing"]["llm_calls"].as_u64().unwrap(),
        ));
    }
    (out, state)
}

/// The heavy one: 100 beats in release; state stays under 6 MB (schema 4: the hierarchy's nodes, cells, mobs and the
/// simulated people with their sheets; DATA_MODEL §2 "Size budget"); rules-path beat time and scripted-LLM calls per beat.
/// Run with `cargo test --release -p fn7-plugin -- --ignored --nocapture`.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn hundred_beats_state_size_and_speed() {
    let scripted = Scripted::default();
    for (name, llm, settings, immortal) in [
        (
            "rules",
            &Failing as &dyn LlmProvider,
            json!({"seed": 7, "beats_total": 100, "llm": false}),
            false,
        ),
        (
            "rules, 4 immortal demigods",
            &Failing as &dyn LlmProvider,
            json!({"seed": 8, "beats_total": 100, "llm": false}),
            true,
        ),
        (
            "scripted LLM",
            &scripted as &dyn LlmProvider,
            json!({"seed": 9, "beats_total": 100}),
            false,
        ),
        (
            "scripted LLM, 4 immortal demigods",
            &scripted as &dyn LlmProvider,
            json!({"seed": 9, "beats_total": 100}),
            true,
        ),
    ] {
        let t = Instant::now();
        let (beats, last) = drive(settings, llm, 100, immortal).await;
        let w = load(&last);
        let n = beats.len() as f64;
        let mean = beats.iter().map(|b| b.0).sum::<f64>() / n;
        let max = beats.iter().map(|b| b.0).fold(0.0, f64::max);
        let peak = beats.iter().map(|b| b.1).max().unwrap();
        let calls = beats.iter().map(|b| b.2).sum::<u64>() as f64 / n;
        let alive_until = w
            .demigods
            .iter()
            .filter_map(|d| d.eliminated_beat)
            .max()
            .unwrap_or(0);
        eprintln!(
            "{name}: 100 beats in {:?}; beat mean {mean:.1} ms, max {max:.1} ms; LLM calls {calls:.1}/beat; state {:.2} MB at beat 100 \
             (peak {:.2} MB); {} rows, {} memories, {} characters, {} agents; {} alive, last fall at beat {alive_until}",
            t.elapsed(),
            beats.last().unwrap().1 as f64 / 1e6,
            peak as f64 / 1e6,
            w.ledger.rows.len(),
            w.memory.items.len(),
            w.characters.len(),
            w.agents.len(),
            w.alive_demigods().count(),
        );
        let mut tables: Vec<(String, usize)> = last
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::to_vec(v).unwrap().len()))
            .collect();
        tables.sort_by_key(|t| std::cmp::Reverse(t.1));
        eprintln!(
            "    largest tables: {}",
            tables
                .iter()
                .take(5)
                .map(|(k, n)| format!("{k} {:.0} KB", *n as f64 / 1e3))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let mut kinds: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
        for m in &w.memory.items {
            let k = format!(
                "{:?}/{}",
                m.kind,
                m.owner.to_string().split(':').next().unwrap_or("")
            );
            let e = kinds.entry(k).or_default();
            e.0 += 1;
            e.1 += (m.folded_into.is_some() || m.forgotten) as usize;
        }
        eprintln!("    memories (kind/owner: n, folded): {kinds:?}");
        let mut verbs: std::collections::BTreeMap<&str, usize> = Default::default();
        for r in &w.ledger.rows {
            *verbs.entry(r.verb.as_str()).or_default() += 1;
        }
        eprintln!("    rows by verb: {verbs:?}");
        assert!(peak < 6_000_000, "{name}: state {peak} bytes");
        assert_eq!(
            w.alive_demigods().count(),
            1,
            "{name}: exactly one stands after the Final Night"
        );
    }
}

fn humans(n: u8) -> Vec<SeatInfo> {
    (0..n)
        .map(|i| SeatInfo {
            seat: i,
            display_name: ["Ash", "Brix", "Cyra", "Dune"][i as usize % 4].into(),
            is_bot: false,
        })
        .collect()
}

fn act(seat: Seat, fields: Value) -> Action {
    Action {
        fields,
        ..Action::new(seat, "")
    }
}

/// A windowed action as the core delivers it when its window closes.
fn windowed(seat: Seat, window_id: u64, seq: u64, at_ms: i64, fields: Value) -> Action {
    Action {
        windowed: true,
        window_id: Some(window_id),
        seq,
        received_at_ms: at_ms,
        ..act(seat, fields)
    }
}

/// An instant action as the core delivers it at the next resolve.
fn instant(seat: Seat, seq: u64, fields: Value) -> Action {
    Action {
        instant: true,
        seq,
        received_at_ms: 1_000 + seq as i64,
        ..act(seat, fields)
    }
}

fn time_debt(w: &World, d: u32, beat: u32) -> f32 {
    w.ledger
        .rows
        .iter()
        .filter(|r| r.beat == beat)
        .filter_map(|r| r.debt)
        .filter(|x| x.owed_to == GodId(0) && x.owed_by == Ref::Demigod(DemigodId(d)))
        .map(|x| x.amount)
        .sum()
}

#[allow(clippy::too_many_arguments)]
fn closed(id: u64, by: Seat, opened: i64, deadline: i64, closed_ms: i64, reason: &str, submitted: Vec<Seat>,
          engaged: Vec<Seat>, dropped: Vec<Seat>) -> ClosedWindow {
    ClosedWindow {
        info: WindowInfo { window_id: id, by_seat: by, opened_ms: opened, deadline_ms: deadline, engaged },
        closed_ms,
        reason: reason.into(),
        by_seat: (reason == "submitted").then_some(by),
        submitted,
        dropped,
    }
}

/// Text entry is the God of Time interaction on the core's action windows. Ash's `action.begin` opens a window with
/// the core's exact next id (refused for a demigod with no bolt); Brix engages unseen; the world keeps resolving while
/// it's open (beat 2, nothing settled); the core closes it on Ash's Enter and delivers Brix's and Ash's held orders
/// with the `ClosedWindow` (beat 3): both go through, and Time bills Ash for the real 8 seconds open (a bolt, a debt of
/// 0.5 × 1.8, Entropy). Cyra was cut (dropped) and never arrives; nobody is told who else typed. A window that timed
/// out with nobody submitting still bills its opener the full slow_ms (beat 4); orders sent without a window are an
/// instant Enter (beat 5, a zero-second slow).
#[tokio::test(flavor = "multi_thread")]
async fn slow_windows_bill_time_from_real_seconds() {
    let p = FinalNight7Plugin::new();
    let settings = json!({"seed": 3, "beats_total": 30, "clusters": 40, "players": 3, "llm": false, "start_bolts": 3});
    let t0: i64 = 1_000_000;
    let script = |seat: Seat, round: u32| -> Option<Action> {
        Some(match (round, seat) {
            (5, 2) => act(2, json!({"left": "go to the market"})),
            _ => act(seat, json!({})),
        })
    };
    let extras = |round: u32| -> (Vec<Action>, RoundContext) {
        match round {
            3 => (
                vec![
                    windowed(1, 1, 10, t0 + 5_000, json!({"left": "recruit the locals"})),
                    windowed(0, 1, 11, t0 + 8_000, json!({"left": "attack Brix", "right": "go to the market"})),
                ],
                RoundContext {
                    windows_closed: vec![closed(1, 0, t0, t0 + 30_000, t0 + 8_000, "submitted", vec![1, 0],
                                                vec![0, 1, 2], vec![2])],
                },
            ),
            4 => (
                vec![],
                RoundContext {
                    windows_closed: vec![closed(2, 1, 1_900_000, 1_930_000, 1_930_004, "deadline", vec![],
                                                vec![1], vec![])],
                },
            ),
            _ => (vec![], RoundContext::default()),
        }
    };
    let mut seen = vec![];
    crate::driver::run_match_with(&p, &humans(3), &settings, &Failing, Some(5), &script, &extras,
        |r: &BeatReport, s: &Value| {
            if r.round == 1 {
                // Ash starts typing: a window, closed by Ash, others dropped; everyone told who.
                let c = p.on_action_begin(s, 2, 0, None, 1);
                let wc = c.window.clone().expect("a window");
                assert_eq!((wc.deadline_in_ms, wc.close_on_submit_of, wc.missing), (30_000, Some(0), MissingActions::Drop));
                assert_eq!(c.public.as_ref().unwrap()["by"], "Ash");
                assert_eq!(c.public.as_ref().unwrap()["text"], "Ash slowed time");
                // Brix engages: nothing public, no window.
                let info = WindowInfo { window_id: 1, by_seat: 0, opened_ms: t0, deadline_ms: t0 + 30_000,
                                        engaged: vec![0, 1] };
                assert_eq!(p.on_action_begin(s, 2, 1, Some(&info), 2), PhaseControl::default());
                // A demigod with no bolt can't slow time.
                let mut w = load(s);
                w.demigods[2].bolts = 0;
                assert_eq!(p.on_action_begin(&world::save(&w), 2, 2, None, 1), PhaseControl::default());
            }
            seen.push((r.round, r.outcome.public.clone(), r.outcome.per_seat.clone(), load(s)));
            true
        })
    .await
    .unwrap();
    let (_, pub2, _, w2) = &seen[1];
    assert!(pub2["slow"].as_array().is_none_or(|a| a.is_empty()), "nothing settles while it's open: {}", pub2["slow"]);
    assert!(w2.demigods[0].slows.is_empty());
    let (_, pub3, per3, w3) = &seen[2];
    let closed = &pub3["slow"][0];
    assert_eq!((closed["event"].as_str(), closed["how"].as_str()), (Some("closed"), Some("enter")), "{closed}");
    assert_eq!(closed["after_ms"], 8000);
    assert!(closed.get("engaged").is_none() && closed.get("dropped").is_none() && closed.get("submitted").is_none());
    assert!(!pub3["slow"].to_string().contains("Brix") && !pub3["slow"].to_string().contains("Cyra"),
            "nobody learns who else typed");
    assert!(!serde_json::to_string(per3).unwrap().contains("\"dropped\""), "no payload names the dropped");
    assert!(w3.slow.is_none());
    assert_eq!(w3.demigods[0].bolts, 2, "Time took a bolt");
    assert_eq!(w3.demigods[1].bolts, 3, "Time's bolt is a toll: no rival gains from it (rules 0.6.0)");
    let words = |d: usize| w3.demigods[d].words_spoken;
    assert!(words(0) > 0 && words(1) > 0 && words(2) == 0, "{} {} {}", words(0), words(1), words(2));
    // Time's debt for 8 seconds (opened_ms to the core's closed_ms), no repeats: 0.5 × (1 + 0.8) = 0.9.
    // Rules 0.7.0: Ash's orders went to both of Ash's demigods (dg:0 left, dg:3 right), so they owe half each.
    assert!((time_debt(w3, 0, 3) - 0.45).abs() < 1e-4, "{}", time_debt(w3, 0, 3));
    assert!((time_debt(w3, 3, 3) - 0.45).abs() < 1e-4, "{}", time_debt(w3, 3, 3));
    assert_eq!(w3.demigods[0].slows.len(), 1, "one window, one slow");
    assert!(w3.meta.entropy > seen[0].3.meta.entropy);
    assert_eq!(w3.slow_settled.last().map(|s| s.window_id), Some(1));
    // Beat 4: Brix's window timed out at its deadline with nobody submitting: the full 30 s → 0.5 × 4 = 2.0.
    let (_, pub4, _, w4) = &seen[3];
    assert_eq!(pub4["slow"][0]["how"], "timeout");
    assert_eq!(pub4["slow"][0]["after_ms"], 30_000);
    assert!((time_debt(w4, 1, 4) - 2.0).abs() < 1e-4, "{}", time_debt(w4, 1, 4));
    // Beat 5: Cyra's orders without a window: an instant order (the flat instant_order_debt, 0.5).
    let (_, pub5, _, w5) = &seen[4];
    assert_eq!(pub5["slow"][0]["event"], "instant", "{}", pub5["slow"]);
    assert!((time_debt(w5, 2, 5) - 0.5).abs() < 1e-4);
}

/// Rules 0.6.0 (R1, R3): an instant order (no window) costs Time's flat `instant_order_debt`, with no repeat factor,
/// doubled when the bolt can't be paid; the bolt is a toll, so no rival gains from it (Entropy still rises).
#[tokio::test(flavor = "multi_thread")]
async fn instant_orders_pay_a_flat_toll() {
    let p = FinalNight7Plugin::new();
    let script = |seat: Seat, round: u32| -> Option<Action> {
        Some(match (round, seat) {
            (1..=4, 0) => act(0, json!({"left": "go to the market"})),
            _ => act(seat, json!({})),
        })
    };
    let none = |_: u32| -> (Vec<Action>, RoundContext) { (vec![], RoundContext::default()) };
    for (extra, debt) in [(json!({}), 0.5f32), (json!({"instant_order_debt": 0.3}), 0.3), (json!({"start_bolts": 0}), 0.5)] {
        let mut settings = json!({"seed": 3, "beats_total": 30, "clusters": 40, "players": 3, "llm": false});
        for (k, v) in extra.as_object().unwrap() {
            settings[k] = v.clone();
        }
        let start = settings.get("start_bolts").and_then(Value::as_u64).unwrap_or(crate::settings::DEFAULT_BOLT_START as u64) as u32;
        let mut seen = vec![];
        crate::driver::run_match_with(&p, &humans(3), &settings, &Failing, Some(4), &script, &none,
            |_: &BeatReport, s: &Value| {
                seen.push(load(s));
                true
            })
        .await
        .unwrap();
        for (i, w) in seen.iter().enumerate() {
            let beat = i as u32 + 1;
            // The same bill every beat, however many slows came before (no repeat factor); doubled once the bolts run
            // out (3 to start: beat 4 is unpaid).
            let want = if start as usize > i { debt } else { 2.0 * debt };
            assert!((time_debt(w, 0, beat) - want).abs() < 1e-4, "{extra} beat {beat}: {}", time_debt(w, 0, beat));
            assert_eq!(w.demigods[0].slows.len(), i + 1);
            // Ash's tolls never reach the rivals.
            assert_eq!((w.demigods[1].bolts, w.demigods[2].bolts), (start, start), "{extra} beat {beat}");
        }
        assert_eq!(seen[3].demigods[0].bolts, start.saturating_sub(4), "{extra}: one bolt a beat while there is one");
        assert!(seen[3].meta.entropy > 0.0);
    }
}

/// Death's revive: a seat reported eliminated comes back (`Committed.revived`) once its demigod lives again.
#[tokio::test(flavor = "multi_thread")]
async fn revive_returns_the_seat_to_play() {
    let p = FinalNight7Plugin::new();
    let settings = json!({"seed": 4, "beats_total": 30, "clusters": 40, "players": 2, "llm": false});
    let r = play(settings, &Failing, humans(2), Some(1)).await;
    let mut w = load(&r.last);
    // Rules 0.7.0: Brix (seat 1) has two demigods, dg:1 (Left) and dg:3 (Right). One falling isn't the seat's end.
    assert_eq!(w.siblings(DemigodId(1)), vec![DemigodId(1), DemigodId(3)]);
    let fall = |w: &mut World, d: usize, beat: u32| {
        w.demigods[d].alive = false;
        w.demigods[d].health = 0.0;
        w.demigods[d].eliminated_beat = Some(beat);
        w.demigods[d].cause = Some("struck down".into());
    };
    fall(&mut w, 3, 2);
    let c = p.commit_round(&world::save(&w), 2, &[], &Failing).await.unwrap();
    assert!(c.eliminated.is_empty(), "Brix still stands on his Left: {:?}", c.eliminated);
    let mut w = load(&c.state);
    assert!(w.out_reported.is_empty());
    fall(&mut w, 1, 2);
    let c = p.commit_round(&world::save(&w), 2, &[], &Failing).await.unwrap();
    assert_eq!(c.eliminated.len(), 1, "{:?}", c.eliminated);
    assert_eq!(c.eliminated[0].0, 1);
    assert!(c.revived.is_empty());
    let mut w = load(&c.state);
    assert_eq!(w.out_reported, vec![DemigodId(1)]);
    let death = p.pantheon().id_of("death").unwrap();
    let raise = Proposal {
        source: Source::God { god: death, ruling: None },
        intent: None,
        effect: Effect::Revive { who: Ref::Demigod(DemigodId(1)) },
        act: "Death relents and Brix breathes again".into(),
        actor: Ref::God(death),
        magnitude: 8,
        valence: 4,
        witnesses: vec![Ref::Demigod(DemigodId(1))],
        cause: None,
    };
    fn7_society::apply(&mut w, &raise, 3).unwrap();
    let c = p.commit_round(&world::save(&w), 3, &[], &Failing).await.unwrap();
    assert_eq!(c.revived, vec![(1, "Death relents and Brix breathes again".to_string())]);
    assert!(load(&c.state).out_reported.is_empty());
}

/// Casts (rules 0.7.0, the 1:5 economy): instant actions with a public streak at once, paid at the next resolve in `seq`
/// order (overspend fizzles): a MEGA costs a bolt and gives every rival player a bolt, a MINI costs a spark and gives
/// nothing; Entropy's yes and its ramifications, every row geotagged; typed text can never cast.
#[tokio::test(flavor = "multi_thread")]
async fn casts_pay_gift_ramify_and_geotag() {
    let p = FinalNight7Plugin::new();
    let settings = json!({"seed": 5, "beats_total": 30, "clusters": 40, "players": 2, "llm": true});
    let script = |seat: Seat, round: u32| -> Option<Action> {
        Some(match (round, seat) {
            (2, 0) => act(0, json!({"left": "throw a lightning bolt at Brix"})),
            _ => act(seat, json!({})),
        })
    };
    let extras = |round: u32| -> (Vec<Action>, RoundContext) {
        let a = match round {
            1 => vec![
                instant(1, 5, json!({"bolt": {"target": "Ash's Left Hand"}})),
                instant(0, 6, json!({"cast": {"power": "volcano", "target": "dg:1", "tier": "mega"}})),
                instant(0, 7, json!({"bolts": [[2.35, 48.85]]})),
            ],
            // Ash has no bolt left (the mega took it): this mega is refused on arrival.
            2 => vec![instant(0, 19, json!({"cast": {"power": "lightning", "target": "Brix's Left Hand", "tier": "mega"}})),
                      instant(1, 20, json!({"cast": {"power": "meteor", "target": [0.0, 0.0], "tier": "mega"}}))],
            _ => vec![],
        };
        (a, RoundContext::default())
    };
    let mut seen = vec![];
    crate::driver::run_match_with(&p, &humans(2), &settings, &Scripted::default(), Some(2), &script, &extras,
        |r: &BeatReport, s: &Value| {
            seen.push((r.outcome.public.clone(), load(s)));
            true
        })
    .await
    .unwrap();
    let (pub1, w1) = &seen[0];
    let casts = pub1["casts"].as_array().unwrap();
    assert_eq!(casts.len(), 3, "{casts:?}");
    assert_eq!(casts[0]["by"], "Brix", "arrival order (seq); `by` is the player");
    assert_eq!((casts[0]["tier"].as_str(), casts[1]["tier"].as_str()), (Some("mini"), Some("mega")), "absent = mini");
    assert_eq!(casts[0]["instant"], true);
    assert!(casts.iter().all(|c| c.get("fizzled").is_none()), "{casts:?}");
    // Ash: 1 bolt - 1 (the mega) = 0, 5 sparks - 1 = 4. Brix: 1 bolt + 1 (Ash's mega's gift) = 2, 5 - 1 = 4 sparks.
    assert_eq!((w1.demigods[0].bolts, w1.demigods[0].sparks), (0, 4));
    assert_eq!((w1.demigods[1].bolts, w1.demigods[1].sparks), (2, 4));
    assert_eq!((w1.demigods[2].bolts, w1.demigods[3].sparks), (0, 4), "the purse is the player's: both demigods show it");
    assert!(w1.meta.entropy >= 10.0, "a mega adds entropy_per_mega: {}", w1.meta.entropy);
    let entropy = GodId(6);
    assert!(w1.rulings.iter().any(|r| r.god == entropy && r.verdict == Verdict::Grant), "Entropy says yes");
    let events = pub1["events"].as_array().unwrap();
    assert!(!events.is_empty());
    for e in events {
        assert!(e["at"].is_array() && e["region"].is_string(), "{e}");
    }
    assert!(events.iter().any(|e| e.get("image_prompt").is_some()));
    assert!(w1.ledger.rows.iter().all(|r| r.at.is_some() || r.place.is_none()));
    // Beat 2: typed text never casts; Ash's mega never arrives (validated on arrival against the last commit: no
    // bolt); Brix's mega lands.
    let (pub2, w2) = &seen[1];
    let c2 = pub2["casts"].as_array().unwrap();
    assert_eq!(c2.len(), 1, "{c2:?}");
    assert_eq!(c2[0]["by"], "Brix");
    assert!(c2[0].get("fizzled").is_none());
    // The mega row: Brix paid it; its gift is a bolt for Ash (god invocations still gift 2 per bolt; casts don't).
    assert!(w2.ledger.rows.iter().any(|r| r.beat == 2 && r.verb == "cast" && r.actor == Ref::Demigod(DemigodId(1))));
    assert!(w2.demigods[1].bolts < 2 + 2 * 4, "no cast gifts 2 per bolt any more");
    // Validation (read-only, against the last commit): unknown powers, overspend, casts carrying orders.
    let st = world::save(w2);
    assert!(p.validate_action(&st, 0, "", &json!({"cast": {"power": "nuke", "target": "dg:1"}}), SubmitKind::Instant).is_err());
    assert!(p.validate_action(&st, 0, "", &json!({"cast": {"power": "custom", "target": "dg:1"}}), SubmitKind::Instant).is_err());
    assert!(p.validate_action(&st, 0, "", &json!({"cast": {"power": "lightning", "target": "dg:1", "tier": "huge"}}), SubmitKind::Instant).is_err());
    let rich = { let mut w = w2.clone(); w.set_bolts(DemigodId(0), 9); world::save(&w) };
    assert!(p
        .validate_action(&rich, 0, "", &json!({"cast": {"power": "custom", "text": "rain of frogs", "target": "Lagos"}}), SubmitKind::Instant)
        .is_ok());
    assert!(p.validate_action(&rich, 0, "", &json!({"left": "go", "bolt": "Brix"}), SubmitKind::Instant).unwrap_err().contains("carry casts, not orders"));
    assert!(p.validate_action(&rich, 0, "", &json!({"left": "go", "bolt": "Brix"}), SubmitKind::Compose).unwrap_err().contains("never carry orders"));
    // Instant actions never carry orders (enforced on arrival, faa7220's SubmitKind); text entry may.
    assert!(p.validate_action(&rich, 0, "", &json!({"left": "march on Lagos"}), SubmitKind::Instant).is_err());
    assert!(p.validate_action(&rich, 0, "", &json!({"left": "march on Lagos"}), SubmitKind::Window { window_id: 3 }).is_ok());
    let broke = { let mut w = w2.clone(); w.set_bolts(DemigodId(0), 0); w.set_sparks(DemigodId(0), 0); world::save(&w) };
    let mega = json!({"cast": {"power": "meteor", "target": "dg:1", "tier": "mega"}});
    assert!(p.validate_action(&broke, 0, "", &mega, SubmitKind::Instant).unwrap_err().contains("not enough bolts"));
    assert!(p.validate_action(&broke, 0, "", &json!({"bolt": "dg:1"}), SubmitKind::Instant).unwrap_err().contains("not enough sparks"));
    // The streak: shown at once, read-only (nothing paid yet).
    let streak = p.on_instant(&rich, 0, &instant(0, 30, mega)).unwrap();
    assert_eq!((streak["event"].as_str(), streak["by"].as_str(), streak["power"].as_str()), (Some("streak"), Some("Ash"), Some("meteor")));
    assert_eq!(streak["casts"][0]["tier"], "mega");
    assert!(streak["casts"][0]["radius_km"].as_f64().unwrap() >= 600.0, "a mega is map scale");
    assert!(streak["icon"].is_string() && streak["at"].is_array() && streak["region"].is_string(), "{streak}");
    assert_eq!(streak["target"], "dg:1");
    assert_eq!(load(&rich).demigods[0].bolts, 9);
}

/// "Everyone casting everything ends the world in about 30 seconds" (BALANCE_REPORT_1 §8b, re-checked after rules
/// 0.6.0's R3, which takes the gift off Time's toll but not off casts): four seats casting at the client's maximum rate
/// (2 a second, burst 4: 28 casts in the first 12 s beat, 24 after) at `entropy_scale` 1 reach heat death at the 2nd or
/// 3rd resolve (24–36 s at `flow_ms` 12000), never the 1st, with the mixed palette or with only the 1-bolt powers.
#[tokio::test(flavor = "multi_thread")]
async fn frantic_casting_ends_the_world_in_about_30_seconds() {
    let p = FinalNight7Plugin::new();
    let palette: Vec<String> = crate::powers().powers.iter().map(|x| x.slug.clone()).collect();
    let cheap: Vec<String> = crate::powers().powers.iter().filter(|x| x.cost == 1).map(|x| x.slug.clone()).collect();
    assert!(palette.len() >= 10 && !cheap.is_empty());
    // Rules 0.7.0, the 1:5 economy: frantic MEGA casting (a bolt each; every mega gives every rival a bolt) still ends
    // the world at the 2nd or 3rd resolve; frantic MINI casting (a spark each, no gifts) can't.
    for (label, powers, scale) in [("mixed mega", &palette, "mega"), ("cheapest mega", &cheap, "mega"), ("mini", &palette, "mini")] {
        let settings = json!({"seed": 6, "beats_total": 30, "clusters": 40, "players": 4, "llm": false,
                              "flow_ms": 12000, "entropy_scale": 1.0});
        let script = |seat: Seat, _: u32| -> Option<Action> { Some(act(seat, json!({}))) };
        let extras = |round: u32| -> (Vec<Action>, RoundContext) {
            let per_seat = if round == 1 { 28 } else { 24 };
            let mut v = vec![];
            for i in 0..per_seat {
                for seat in 0..4u8 {
                    let k = (i * 4 + seat as usize + round as usize) % powers.len();
                    let seq = (round as u64) * 1000 + (i * 4) as u64 + seat as u64;
                    // Open ocean, so nobody is hurt and every seat keeps casting.
                    v.push(instant(seat, seq, json!({"cast": {"power": powers[k], "target": [-30.0, 0.0], "tier": scale}})));
                }
            }
            (v, RoundContext::default())
        };
        let mut entropy = vec![];
        let (last, _) = crate::driver::run_match_with(&p, &humans(4), &settings, &Failing, Some(6), &script, &extras,
            |_: &BeatReport, s: &Value| {
                entropy.push(load(s).meta.entropy);
                true
            })
        .await
        .unwrap();
        let w = load(&last);
        if scale == "mini" {
            assert!(w.meta.heat_death.is_none() && w.meta.entropy < 50.0, "{label}: minis are contained: {entropy:?}");
            continue;
        }
        let hd = w.meta.heat_death.unwrap_or_else(|| panic!("{label}: no heat death; Entropy by resolve {entropy:?}"));
        eprintln!("    {label}: heat death at resolve {hd}; Entropy by resolve {entropy:?}");
        assert!((2..=3).contains(&hd), "{label}: heat death at resolve {hd} ({entropy:?}), expected the 2nd or 3rd");
    }
}

/// The planet: regions with cities, demigods in different regions, influence shares per region summing to ≤ 1, and
/// state well under 4 MB.
#[tokio::test(flavor = "multi_thread")]
async fn the_planet_arena() {
    let r = play(json!({"seed": 8, "beats_total": 40, "clusters": 200}), &Failing, bots(4), Some(20)).await;
    let w = load(&r.last);
    assert!(w.is_planet());
    let regions = w.places.iter().filter(|p| p.kind == PlaceKind::District).count();
    let cities = w.places.iter().filter(|p| p.kind == PlaceKind::Site).count();
    assert!(regions >= 15 && cities >= 40, "{regions} regions, {cities} cities");
    let pv = phases::public_view(&w);
    let mut sums: std::collections::BTreeMap<String, f64> = Default::default();
    for x in pv["influence"].as_array().unwrap() {
        *sums.entry(x["region"].as_str().unwrap().into()).or_default() += x["share"].as_f64().unwrap();
    }
    assert!(sums.values().all(|s| *s <= 1.0001), "{sums:?}");
    assert!(pv["demigods"][0]["portrait_prompt"].is_string());
    let size = serde_json::to_vec(&r.last).unwrap().len();
    assert!(size < 4_000_000, "state {size} bytes");
}

/// Heat death: Entropy at its maximum makes that beat the Final Night: every debt is collected, exactly one survives.
#[tokio::test(flavor = "multi_thread")]
async fn heat_death_brings_the_final_night() {
    let r = play(
        json!({"seed": 4, "beats_total": 100, "clusters": 60, "entropy_scale": 10, "start_bolts": 20}),
        &Failing,
        bots(4),
        None,
    )
    .await;
    let w = load(&r.last);
    let hd = w.meta.heat_death.expect("heat death struck");
    assert_eq!(w.meta.beat, hd, "the match ended on the heat-death beat");
    assert_eq!(w.meta.beats_total, hd);
    assert!(w.meta.entropy >= ENTROPY_MAX);
    one_winner(&r);
    assert_eq!(w.ledger.open_debts().count(), 0, "all debts fell due");
    assert_eq!(r.summary.summary["heat_death"], json!(hd));
}

/// The world-only simulation (no orders, no casts): the society acts by itself every beat (mobs with ringleaders,
/// migration, cults, people emerging), replays exactly, records its level of detail, and the payloads carry the
/// hierarchy.
#[tokio::test(flavor = "multi_thread")]
async fn society_acts_on_its_own() {
    let settings = json!({ "seed": 7, "beats_total": 100, "llm": false, "society_only": true });
    let a = play(settings.clone(), &Failing, bots(4), Some(60)).await;
    let w = load(&a.last);
    assert!(w.intents.is_empty(), "nobody gave an order");
    let s = &w.soc;
    assert!(s.mobs_formed >= 1 && s.migrations >= 1 && s.cults >= 1 && s.people_emerged >= 5, "{s:?}");
    assert!(w.mobs.iter().all(|m| m.ringleader.is_some()), "every mob has a ringleader");
    assert!(w.characters.iter().filter(|c| c.life.is_some()).count() >= 5);
    // Every level ran by rules every beat; the record says so.
    assert!(w.beats.iter().filter(|b| b.beat >= 1).all(|b| b.fidelity.as_ref().is_some_and(|f| f.rules > 0)));
    // Totals roll up: the planet's population is the sum of its crowds.
    let crowds: u64 = w.clusters.iter().filter(|c| c.army.is_none()).map(|c| c.size as u64).sum();
    assert_eq!(w.nodes[w.meta.arena.ix()].population, crowds);
    // Society rows are geotagged ledger rows.
    let soc_rows: Vec<&LedgerRow> = w.ledger.rows.iter().filter(|r| SocKind::of(&r.verb).is_some()).collect();
    assert!(!soc_rows.is_empty() && soc_rows.iter().all(|r| r.at.is_some()));
    // Payloads: the seat view and the per-seat delta carry the hierarchy; the public outcome carries the society.
    let view = FinalNight7Plugin::new().seat_view(&a.last, 0);
    assert!(view["hierarchy"]["node"]["population"].as_u64().unwrap_or(0) > 0, "{}", view["hierarchy"]);
    let (public, per_seat) = a.reports.last().unwrap();
    assert!(public["society"]["totals"]["mobs_formed"].as_u64().is_some());
    assert!(per_seat.iter().any(|(_, v)| v["hierarchy"]["path"].is_array()));
    // Deterministic: the same seed is the same society.
    let b = play(settings, &Failing, bots(4), Some(60)).await;
    assert_eq!(a.last, b.last);
}

/// Hands act on the hierarchy: an order goes to the army's general (the general mind's call, with the rules'
/// judgment as its fallback), "buy off the captains" turns their loyalty, "promote" raises someone.
#[tokio::test(flavor = "multi_thread")]
async fn hands_order_generals_bribe_and_promote() {
    let p = FinalNight7Plugin::new();
    let settings = p.validate_settings(&json!({ "seed": 11, "llm": false, "society_only": true })).unwrap();
    let state = p.on_match_start(&bots(4), &settings, &Failing).await.unwrap();
    let mut w = load(&state);
    let pantheon = p.pantheon().clone();
    let d = DemigodId(0);
    let src = w
        .clusters
        .iter()
        .filter(|c| c.army.is_none())
        .max_by_key(|c| c.size)
        .unwrap()
        .id;
    let raise = Proposal {
        source: Source::Rules,
        intent: None,
        effect: Effect::RaiseArmy { demigod: d, from: src, size: 5000, name: "Ash's Host".into(), orders: String::new() },
        act: "raise".into(),
        actor: Ref::Demigod(d),
        magnitude: 5,
        valence: 0,
        witnesses: vec![],
        cause: None,
    };
    w.meta.beat = 1;
    crate::merge::run(&mut w, vec![raise], &pantheon, 1, 4);
    let army = ClusterId::from_ix(w.clusters.len() - 1);
    let st = Settings::from_value(&w.meta.settings);
    crate::society::run_step(&mut w, 1, &[], vec![], &pantheon, &st);
    let a = w.clusters[army.ix()].army.clone().unwrap();
    let g = a.general.expect("appointed with the army");
    let city = w.places.iter().find(|x| x.kind == PlaceKind::Site).unwrap().id;
    let hand = w.demigods[0].hands[0];
    let intent = |verb: &str, span: &str, targets: Vec<Ref>| Intent {
        id: IntentId(0),
        beat: 2,
        agent: hand,
        demigod: d,
        verb: verb.into(),
        targets: targets.into_iter().map(Target::Known).collect(),
        how: String::new(),
        place: None,
        magnitude: 5,
        span: span.into(),
        route: Route::World,
    };
    // An order addressed to the general.
    let i = intent("tell", "tell the general to take the city", vec![Ref::Char(g), Ref::Place(city)]);
    let Some(crate::society::SocietyAct::Order { army: got, target, text }) = crate::society::society_intent(&w, &i) else {
        panic!("an order for the general");
    };
    assert_eq!((got, target), (army, Some(Ref::Place(city))));
    let m = crate::society::GeneralMeta { army, by: d, why: "order", text, agent: Some(i.agent), intent: None };
    let ctx = crate::society::general_call(&w, &m, target, 2).expect("a general mind call");
    assert!(ctx.options.iter().any(|e| e.r == Ref::Place(city)));
    let prop = crate::society::orders_prop(&w, &m, ctx.rules.clone(), false);
    let (_, applied) = crate::merge::run(&mut w, vec![prop], &pantheon, 2, 4);
    assert_eq!(applied.rows.len(), 1);
    let o = w.clusters[army.ix()].army.as_ref().unwrap().order.clone().unwrap();
    assert_eq!((o.why.as_str(), o.beat), ("order", 2));
    // Buy off the captains of a rival's army: their loyalty to us rises.
    let captains = a.captains.clone();
    assert!(!captains.is_empty());
    let mut rival = w.clone();
    rival.clusters[army.ix()].army.as_mut().unwrap().demigod = DemigodId(1);
    for c in &captains {
        rival.characters[c.ix()].allegiance = vec![(DemigodId(1), 0.6)];
    }
    let i = intent("buy off", "buy off the general's captains", vec![Ref::Char(g)]);
    let Some(crate::society::SocietyAct::Props(props)) = crate::society::society_intent(&rival, &i) else {
        panic!("a bribe");
    };
    let before = rival.characters[captains[0].ix()].allegiance.iter().find(|x| x.0 == d).map(|x| x.1).unwrap_or(0.0);
    crate::merge::run(&mut rival, props, &pantheon, 2, 4);
    let after = rival.characters[captains[0].ix()].allegiance.iter().find(|x| x.0 == d).map(|x| x.1).unwrap_or(0.0);
    assert!(after > before, "{before} -> {after}");
    // Promote a captain to general: the old general is demoted and holds a grudge.
    let c = captains[0];
    let i = intent("promote", "promote him to general", vec![Ref::Char(c)]);
    let Some(crate::society::SocietyAct::Props(props)) = crate::society::society_intent(&w, &i) else {
        panic!("a promotion");
    };
    crate::merge::run(&mut w, props, &pantheon, 2, 4);
    assert_eq!(w.clusters[army.ix()].army.as_ref().unwrap().general, Some(c));
    let old = w.characters[g.ix()].life.as_deref().unwrap();
    assert_eq!(old.role, fn7_model::Role::Captain);
    assert!(old.grudges.contains(&Ref::Char(c)));
}

/// Fix: an eliminated demigod's debts are settled at once; Consequence never collects from the dead.
#[test]
fn debts_settle_at_elimination() {
    let p = FinalNight7Plugin::new();
    let settings = p.validate_settings(&json!({ "seed": 3, "llm": false })).unwrap();
    let state = futures::executor::block_on(p.on_match_start(&bots(4), &settings, &Failing)).unwrap();
    let mut w = load(&state);
    let pantheon = p.pantheon().clone();
    let cg = pantheon.id_of("consequence").unwrap();
    let mk = |effect: Effect| Proposal {
        source: Source::Rules,
        intent: None,
        effect,
        act: "x".into(),
        actor: Ref::God(cg),
        magnitude: 3,
        valence: -1,
        witnesses: vec![],
        cause: None,
    };
    let d = DemigodId(2);
    crate::merge::run(&mut w, vec![mk(Effect::Debt { owed_by: Ref::Demigod(d), god: cg, amount: 2.0, due_in: 3 })], &pantheon, 1, 4);
    assert_eq!(w.ledger.open_debts().count(), 1);
    crate::merge::run(&mut w, vec![mk(Effect::Eliminate { who: Ref::Demigod(d), cause: "test".into() })], &pantheon, 2, 4);
    assert_eq!(w.ledger.open_debts().count(), 0, "settled at elimination");
    assert!(fn7_society::due_debts(&w, 10).is_empty());
}

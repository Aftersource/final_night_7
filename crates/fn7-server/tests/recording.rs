//! Recorded matches (SQLite in a temp dir, offline): every DATA_MODEL §3 event kind appears, LLM steps are annotated,
//! and replay verification reproduces every snapshot byte for byte.

use std::path::PathBuf;
use std::sync::Arc;

use fn7_plugin::driver::run_match;
use fn7_plugin::testkit::{Failing, Scripted};
use fn7_plugin::FinalNight7Plugin;
use serde_json::{json, Value};
use theatre_plugin_api::*;
use theatre_recorder::{RecordingPlugin, SqliteSink};

/// Every event kind of DATA_MODEL §3.
const KINDS: &[&str] = &[
    "utterance",
    "intent",
    "mention_resolved",
    "memorized",
    "agent_spawned",
    "agent_step",
    "agent_ended",
    "ruling",
    "proposal",
    "resolution",
    "ledger_row",
    "spread",
    "debt_due",
    "debt_delayed",
    "debt_collected",
    "rewind",
    "memory_write",
    "memory_forget",
    "memory_fold",
    "hand_said",
    "zoom",
    "beat",
    "elimination",
];

/// Every LLM step of DATA_MODEL §3 the plugin makes (`route` has no mind: hands route themselves).
const STEPS: &[&str] = &[
    "hand",
    "subagent",
    "possession",
    "god",
    "paladin",
    "resolver",
    "materialize",
    "fold",
    "narrator",
];

fn temp_db(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fn7-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("t.sqlite")
}

fn seats() -> Vec<SeatInfo> {
    ["Ash", "Brix", "Cyra", "Dune"]
        .iter()
        .enumerate()
        .map(|(i, n)| SeatInfo {
            seat: i as u8,
            display_name: n.to_string(),
            is_bot: true,
        })
        .collect()
}

async fn record(db: &PathBuf, settings: Value, llm: &dyn LlmProvider) -> String {
    let sink = Arc::new(SqliteSink::open(db).unwrap());
    let p = RecordingPlugin::new(FinalNight7Plugin::new(), sink.clone());
    let none = |_: Seat, _: u32| None;
    let (state, _) = run_match(&p, &seats(), &settings, llm, None, &none, |_, _| true)
        .await
        .unwrap();
    sink.close();
    theatre_recorder::unwrap_state(&state).0.unwrap()
}

fn count(db: &PathBuf, sql: &str, run: &str) -> i64 {
    let c = rusqlite::Connection::open(db).unwrap();
    c.query_row(sql, [run], |r| r.get(0)).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn every_event_kind_and_step_is_recorded() {
    let db = temp_db("events");
    let run = record(
        &db,
        json!({"seed": 4, "beats_total": 30, "clusters": 80, "memory_per_owner": 12}),
        &Scripted::default(),
    )
    .await;
    let mut missing = vec![];
    for k in KINDS {
        let n = count(
            &db,
            &format!("SELECT COUNT(*) FROM events WHERE run_id = ?1 AND kind = '{k}'"),
            &run,
        );
        if n == 0 {
            missing.push(*k);
        }
    }
    assert!(missing.is_empty(), "event kinds never emitted: {missing:?}");
    let mut steps_missing = vec![];
    for s in STEPS {
        let n = count(
            &db,
            &format!("SELECT COUNT(*) FROM llm_calls WHERE run_id = ?1 AND step = '{s}'"),
            &run,
        );
        if n == 0 {
            steps_missing.push(*s);
        }
    }
    assert!(
        steps_missing.is_empty(),
        "LLM steps never annotated: {steps_missing:?}"
    );
    // Payloads the projection reads.
    assert!(count(&db, "SELECT COUNT(*) FROM events WHERE run_id = ?1 AND kind = 'ledger_row' AND json_extract(data, '$.effect.fx') IS NOT NULL AND json_extract(data, '$.row.id') IS NOT NULL", &run) > 0);
    assert!(count(&db, "SELECT COUNT(*) FROM events WHERE run_id = ?1 AND kind = 'memorized' AND json_extract(data, '$.character.groups') IS NOT NULL", &run) > 0);
    assert!(count(&db, "SELECT COUNT(*) FROM events WHERE run_id = ?1 AND kind = 'elimination' AND json_extract(data, '$.chain') IS NOT NULL AND json_extract(data, '$.demigod') IS NOT NULL", &run) > 0);
    assert!(count(&db, "SELECT COUNT(*) FROM events WHERE run_id = ?1 AND kind = 'beat' AND json_array_length(json_extract(data, '$.record.timing.ms')) = 7", &run) > 0, "stage timings in the beat event");
    assert!(count(&db, "SELECT COUNT(*) FROM events WHERE run_id = ?1 AND kind LIKE 'debt_%' AND json_extract(data, '$.debt') IS NOT NULL", &run) > 0);
    // Round = beat.
    assert_eq!(count(&db, "SELECT COUNT(*) FROM events WHERE run_id = ?1 AND kind = 'beat' AND round != json_extract(data, '$.beat')", &run), 0);
    // The ruleset, every snapshot label.
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM runs WHERE id = ?1 AND ruleset_id IS NOT NULL",
            &run
        ),
        1
    );
    for l in [
        "match_start",
        "round_start",
        "resolved",
        "committed",
        "final",
    ] {
        assert!(
            count(
                &db,
                &format!("SELECT COUNT(*) FROM snapshots WHERE run_id = ?1 AND label = '{l}'"),
                &run
            ) > 0,
            "{l}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn replay_reproduces_every_snapshot() {
    for (name, llm) in [
        ("scripted", &Scripted::default() as &dyn LlmProvider),
        ("failing", &Failing),
    ] {
        let db = temp_db(&format!("replay-{name}"));
        let run = record(
            &db,
            json!({"seed": 12, "beats_total": 15, "clusters": 60}),
            llm,
        )
        .await;
        let rep = fn7_server::replay::verify(&db, &run, |l| eprintln!("{l}"))
            .await
            .unwrap();
        eprintln!(
            "{name}: {} snapshots, {} identical, hits {} misses {}",
            rep.snapshots, rep.identical, rep.llm_hits, rep.llm_misses
        );
        assert!(rep.snapshots > 10);
        assert!(rep.differences.is_empty(), "{:?}", rep.differences);
        assert_eq!(rep.identical, rep.snapshots);
        if name == "scripted" {
            assert!(rep.llm_hits > 0);
            assert_eq!(
                rep.llm_misses, 0,
                "every request was answered from the recording"
            );
        }
    }
}

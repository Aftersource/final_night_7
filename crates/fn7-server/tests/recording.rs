//! Recorded matches in the Theatre core's JSONL format (fn7-sim's recorder, temp dir, offline): every DATA_MODEL §3
//! event kind appears, LLM steps are annotated, the core's snapshot labels and ruleset are there, and replay
//! verification (the engine's `ReplayLlm`) reproduces every snapshot byte for byte.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fn7_plugin::driver::run_match;
use fn7_plugin::testkit::{Failing, Scripted};
use fn7_plugin::FinalNight7Plugin;
use fn7_server::record::{recorder, traced, Recorded};
use serde_json::{json, Value};
use theatre_plugin_api::*;

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

fn temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("fn7-{name}-{}", uuid::Uuid::new_v4()))
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

/// Record one match into `dir` at full level; returns the recording's path.
async fn record(dir: &Path, settings: Value, llm: Arc<dyn LlmProvider>) -> PathBuf {
    let rec = recorder(dir, theatre_trace::Level::Full).unwrap();
    let run = uuid::Uuid::new_v4().to_string();
    let p = Recorded::new(FinalNight7Plugin::new(), rec.run(run.clone()));
    let llm = traced(llm);
    let none = |_: Seat, _: u32| None;
    run_match(&p, &seats(), &settings, llm.as_ref(), None, &none, |_, _| true)
        .await
        .unwrap();
    rec.flush();
    dir.join(format!("{run}.jsonl"))
}

fn lines(path: &Path) -> Vec<Value> {
    fn7_server::replay::read_records(path).unwrap()
}

fn of<'a>(ls: &'a [Value], ty: &str) -> impl Iterator<Item = &'a Value> + 'a {
    let ty = ty.to_string();
    ls.iter().filter(move |l| l["type"] == ty.as_str())
}

#[tokio::test(flavor = "multi_thread")]
async fn every_event_kind_and_step_is_recorded() {
    let dir = temp_dir("events");
    let path = record(
        &dir,
        json!({"seed": 4, "beats_total": 30, "clusters": 80, "memory_per_owner": 12}),
        Arc::new(Scripted::default()),
    )
    .await;
    let ls = lines(&path);
    let events: Vec<&Value> = of(&ls, "event").collect();
    let kind = |k: &str| events.iter().filter(|e| e["kind"] == k).count();
    let missing: Vec<&str> = KINDS.iter().copied().filter(|k| kind(k) == 0).collect();
    assert!(missing.is_empty(), "event kinds never emitted: {missing:?}");
    let steps_missing: Vec<&str> = STEPS
        .iter()
        .copied()
        .filter(|s| !of(&ls, "llm_call").any(|c| c["step"] == *s))
        .collect();
    assert!(steps_missing.is_empty(), "LLM steps never annotated: {steps_missing:?}");
    // Payloads the projection reads.
    let has = |k: &str, f: &dyn Fn(&Value) -> bool| events.iter().any(|e| e["kind"] == k && f(&e["data"]));
    assert!(has("ledger_row", &|d| !d["effect"]["fx"].is_null() && !d["row"]["id"].is_null()));
    assert!(has("memorized", &|d| !d["character"]["groups"].is_null()));
    assert!(has("elimination", &|d| !d["chain"].is_null() && !d["demigod"].is_null()));
    assert!(
        has("beat", &|d| d["record"]["timing"]["ms"].as_array().map(Vec::len) == Some(7)),
        "stage timings in the beat event"
    );
    assert!(events
        .iter()
        .any(|e| e["kind"].as_str().unwrap_or("").starts_with("debt_") && !e["data"]["debt"].is_null()));
    // Round = beat.
    assert!(events
        .iter()
        .filter(|e| e["kind"] == "beat")
        .all(|e| e["ctx"]["round"] == e["data"]["beat"]));
    // The core's ruleset (version, rules_version as label) plus the plugin's rich one; every core snapshot label.
    assert!(of(&ls, "ruleset").any(|r| r["label"] == fn7_plugin::RULES_VERSION && r["version"] == "0.1.0"));
    assert!(of(&ls, "ruleset").any(|r| r["body"]["mechanics"].is_array()));
    for l in ["on_match_start", "on_round_start", "resolve_round", "commit_round"] {
        assert!(of(&ls, "snapshot").any(|s| s["label"] == l), "{l}");
    }
    assert_eq!(of(&ls, "run_start").count(), 1);
    assert_eq!(of(&ls, "run_end").count(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn replay_reproduces_every_snapshot() {
    for (name, llm) in [
        ("scripted", Arc::new(Scripted::default()) as Arc<dyn LlmProvider>),
        ("failing", Arc::new(Failing)),
    ] {
        let dir = temp_dir(&format!("replay-{name}"));
        let path = record(&dir, json!({"seed": 12, "beats_total": 15, "clusters": 60}), llm).await;
        let rep = fn7_server::replay::verify(&path, |l| eprintln!("{l}")).await.unwrap();
        eprintln!(
            "{name}: {} snapshots, {} identical, hits {} misses {}",
            rep.snapshots, rep.identical, rep.llm_hits, rep.llm_misses
        );
        assert!(rep.snapshots > 10);
        assert!(rep.differences.is_empty(), "{:?}", rep.differences);
        assert_eq!(rep.identical, rep.snapshots);
        assert_eq!(rep.summary_identical, Some(true));
        if name == "scripted" {
            assert!(rep.llm_hits > 0);
            assert_eq!(rep.llm_misses, 0, "every request was answered from the recording");
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}

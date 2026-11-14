//! Replay verification of a core recording (`<match_id>.jsonl`, from the Theatre server with `THEATRE_RECORD` set, or
//! from `fn7-sim`): re-execute the match from its `on_match_start` snapshot (a fork's run: its `fork` snapshot) with every LLM request answered by the
//! engine's [`ReplayLlm`] (keyed by round, actor, step and request content; a request the recording lacks fails, so its
//! mind takes the same rules fallback it took live), feeding each round the actions and `RoundContext` (closed action
//! windows) recorded in its `resolve_round` hook input, and check that every recorded snapshot
//! (`on_round_start` and `resolve_round` at `full`, `commit_round` always) serializes byte for byte the same, plus the
//! summary in `run_end`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use fn7_plugin::FinalNight7Plugin;
use serde_json::Value;
use theatre_plugin_api::*;
use theatre_server::replay::ReplayLlm;
use theatre_trace::{Ctx, Record, Sink};

#[derive(Debug, Default)]
pub struct Report {
    pub run: String,
    pub snapshots: usize,
    pub identical: usize,
    pub differences: Vec<String>,
    pub llm_hits: u64,
    pub llm_misses: u64,
    pub beats: u32,
    /// Whether the replayed summary equals the recorded one (`None`: the recording has no `run_end`).
    pub summary_identical: Option<bool>,
}

/// The first place two JSON values differ (path: a vs b), if any.
pub fn first_diff(a: &Value, b: &Value, path: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys()) {
                let (va, vb) = (
                    x.get(k).unwrap_or(&Value::Null),
                    y.get(k).unwrap_or(&Value::Null),
                );
                if let Some(d) = first_diff(va, vb, &format!("{path}.{k}")) {
                    return Some(d);
                }
            }
            None
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                return Some(format!("{path}: length {} vs {}", x.len(), y.len()));
            }
            x.iter()
                .zip(y)
                .enumerate()
                .find_map(|(i, (va, vb))| first_diff(va, vb, &format!("{path}[{i}]")))
        }
        _ if a == b => None,
        _ => Some(format!("{path}: {} vs {}", trunc(a), trunc(b))),
    }
}

fn trunc(v: &Value) -> String {
    let s = v.to_string();
    if s.chars().count() > 120 {
        format!("{}…", s.chars().take(120).collect::<String>())
    } else {
        s
    }
}

/// Discards everything: the replay runs hooks inside a trace context (so the LLM replay sees round and step) but
/// records nothing.
struct NullSink(AtomicU64);

impl Sink for NullSink {
    fn next_id(&self) -> i64 {
        self.0.fetch_add(1, Ordering::Relaxed) as i64
    }
    fn write(&self, _: Record) {}
}

/// Counts answered calls (the engine's `ReplayLlm` counts only misses).
struct Counting {
    inner: ReplayLlm,
    calls: AtomicU64,
}

#[async_trait]
impl LlmProvider for Counting {
    async fn complete(&self, req: LlmRequest) -> anyhow::Result<LlmResponse> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.inner.complete(req).await
    }
}

/// Read a JSONL recording (unparseable lines are skipped, like the engine's readers do).
pub fn read_records(path: &Path) -> anyhow::Result<Vec<Value>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
}

fn round_of(r: &Value) -> Option<u32> {
    r["round"]
        .as_u64()
        .or_else(|| r["ctx"]["round"].as_u64())
        .map(|n| n as u32)
}

struct ResolveIn {
    hook_id: Option<i64>,
    actions: Vec<Action>,
    ctx: RoundContext,
    ok: bool,
}

pub async fn verify(path: impl AsRef<Path>, mut log: impl FnMut(&str)) -> anyhow::Result<Report> {
    let records = read_records(path.as_ref())?;
    let run = records
        .iter()
        .find(|r| r["type"] == "run_start")
        .and_then(|r| r["run"].as_str())
        .or_else(|| records.iter().find_map(|r| r["run"].as_str().or(r["ctx"]["run"].as_str())))
        .ok_or_else(|| anyhow!("empty recording"))?
        .to_string();
    let snaps: Vec<(Option<u32>, &str, &Value)> = records
        .iter()
        .filter(|r| r["type"] == "snapshot")
        .map(|r| (round_of(r), r["label"].as_str().unwrap_or(""), &r["state"]))
        .collect();
    let snap = |round: u32, label: &str| {
        snaps
            .iter()
            .rev()
            .find(|s| s.0 == Some(round) && s.1 == label)
            .map(|s| s.2)
    };
    // Each round's resolve: the last attempt that succeeded, else (fallback) the last attempt.
    let mut resolves: BTreeMap<u32, ResolveIn> = BTreeMap::new();
    for h in records
        .iter()
        .filter(|r| r["type"] == "hook" && r["hook"] == "resolve_round")
    {
        let Some(round) = round_of(h) else { continue };
        let ok = h["error"].is_null();
        if resolves.get(&round).is_some_and(|p| p.ok && !ok) {
            continue;
        }
        resolves.insert(
            round,
            ResolveIn {
                hook_id: h["id"].as_i64(),
                actions: serde_json::from_value(h["input"]["actions"].clone()).unwrap_or_default(),
                ctx: serde_json::from_value(h["input"]["ctx"].clone()).unwrap_or_default(),
                ok,
            },
        );
    }
    // A match starts from its on_match_start snapshot; a fork (admin API) from its `fork` snapshot after round N.
    let (first, start) = snaps
        .iter()
        .find(|s| s.1 == "on_match_start" || s.1 == "fork")
        .map(|s| (s.0.unwrap_or(0) + 1, s.2.clone()))
        .ok_or_else(|| anyhow!("no on_match_start or fork snapshot in run {run}"))?;
    let last_commit = snaps
        .iter()
        .filter(|s| s.1 == "commit_round")
        .filter_map(|s| s.0)
        .max()
        .unwrap_or(0);
    let last = resolves.keys().max().copied().unwrap_or(0).max(last_commit);

    let replay = Counting {
        inner: ReplayLlm::from_records(&records),
        calls: AtomicU64::new(0),
    };
    let sink: Arc<dyn Sink> = Arc::new(NullSink(AtomicU64::new(1)));
    let ctx = |round: u32| {
        let mut c = Ctx::new(sink.clone(), run.clone());
        c.round = Some(round);
        c
    };
    let p = FinalNight7Plugin::new();
    let mut rep = Report {
        run: run.clone(),
        ..Default::default()
    };
    let mut compare = |round: u32, label: &str, got: &Value, rep: &mut Report| {
        if let Some(theirs) = snap(round, label) {
            rep.snapshots += 1;
            if serde_json::to_string(theirs).ok() == serde_json::to_string(got).ok() {
                rep.identical += 1;
            } else {
                let d = first_diff(theirs, got, "state").unwrap_or_else(|| "byte-level difference".into());
                log(&format!("  beat {round:>3} {label:<15} DIFFERS at {d}"));
                rep.differences.push(format!("beat {round} {label}: {d}"));
            }
        }
    };
    let mut state = start;
    for round in first..=last {
        let rs = theatre_trace::within(ctx(round), p.on_round_start(&state, round, &replay)).await?;
        compare(round, "on_round_start", &rs.state, &mut rep);
        state = rs.state;
        let Some(input) = resolves.get(&round) else {
            break;
        };
        let out = if input.ok {
            let mut c = ctx(round);
            c.hook_id = input.hook_id;
            theatre_trace::within(c, p.resolve_round(&state, round, &input.actions, &input.ctx, &replay, &NoProgress))
                .await?
        } else {
            // The core passes no context to fallback_outcome; the plugin uses the one its failed attempts got.
            p.remember_round_ctx(&state, round, input.ctx.clone());
            p.fallback_outcome(&state, round, &input.actions)
        };
        compare(round, "resolve_round", &out.state, &mut rep);
        state = out.state;
        let committed = theatre_trace::within(ctx(round), p.commit_round(&state, round, &[], &replay)).await?;
        compare(round, "commit_round", &committed.state, &mut rep);
        state = committed.state;
        rep.beats = round;
    }
    // A next beat's Start the driver ran before learning the match was over.
    if snap(last + 1, "on_round_start").is_some() {
        let rs = theatre_trace::within(ctx(last + 1), p.on_round_start(&state, last + 1, &replay)).await?;
        compare(last + 1, "on_round_start", &rs.state, &mut rep);
        state = rs.state;
    }
    if let Some(end) = records.iter().rev().find(|r| r["type"] == "run_end") {
        let summarize = records
            .iter()
            .rev()
            .find(|r| r["type"] == "hook" && r["hook"] == "summarize" && r["error"].is_null());
        if let Some(h) = summarize {
            let round = round_of(h).unwrap_or(rep.beats);
            let s = theatre_trace::within(ctx(round), p.summarize(&state, &replay)).await?;
            let same = s.summary == end["summary"]["summary"];
            if !same {
                let d = first_diff(&end["summary"]["summary"], &s.summary, "summary").unwrap_or_default();
                log(&format!("  summary DIFFERS at {d}"));
                rep.differences.push(format!("summary: {d}"));
            }
            rep.summary_identical = Some(same);
        }
    }
    let misses = *replay.inner.misses.lock();
    rep.llm_misses = misses;
    rep.llm_hits = replay.calls.load(Ordering::Relaxed).saturating_sub(misses);
    Ok(rep)
}

//! Replay verification: re-execute a recorded match from its `match_start` snapshot with every LLM request answered
//! from the recording (`theatre_recorder::ReplayProvider`; a request the recording lacks is an error, so its mind takes
//! the same rules fallback it took live), feeding each beat the recorded actions, and check every snapshot
//! (`round_start`, `resolved` / `resolved_fallback`, `committed`, `final`) is byte-identical.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::Ordering;

use fn7_plugin::FinalNight7Plugin;
use rusqlite::{params, Connection};
use serde_json::Value;
use theatre_plugin_api::*;
use theatre_recorder::ReplayProvider;

#[derive(Debug, Default)]
pub struct Report {
    pub snapshots: usize,
    pub identical: usize,
    pub differences: Vec<String>,
    pub llm_hits: u64,
    pub llm_misses: u64,
    pub beats: u32,
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

struct Snap {
    round: Option<u32>,
    label: String,
    raw: String,
}

/// The most recent run in a recording.
pub fn latest_run(db: impl AsRef<Path>) -> anyhow::Result<String> {
    let c = Connection::open(db)?;
    Ok(c.query_row(
        "SELECT id FROM runs ORDER BY started_at DESC, rowid DESC LIMIT 1",
        [],
        |r| r.get(0),
    )?)
}

pub async fn verify(
    db: impl AsRef<Path>,
    run: &str,
    mut log: impl FnMut(&str),
) -> anyhow::Result<Report> {
    let db = db.as_ref();
    let c = Connection::open(db)?;
    let snaps: Vec<Snap> = c
        .prepare("SELECT round, label, state FROM snapshots WHERE run_id = ?1 ORDER BY id")?
        .query_map(params![run], |r| {
            Ok(Snap {
                round: r.get(0)?,
                label: r.get(1)?,
                raw: r.get(2)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    let inputs = |hook: &str| -> anyhow::Result<BTreeMap<u32, Value>> {
        let mut m = BTreeMap::new();
        let mut st = c.prepare(
            "SELECT round, input FROM hooks WHERE run_id = ?1 AND hook = ?2 ORDER BY id",
        )?;
        for row in st.query_map(params![run, hook], |r| {
            Ok((r.get::<_, Option<u32>>(0)?, r.get::<_, String>(1)?))
        })? {
            let (round, input) = row?;
            if let Some(round) = round {
                m.insert(round, serde_json::from_str(&input)?);
            }
        }
        Ok(m)
    };
    let resolves = inputs("resolve_round")?;
    let fallbacks = inputs("fallback_outcome")?;
    let start = snaps
        .iter()
        .find(|s| s.label == "match_start")
        .ok_or_else(|| anyhow::anyhow!("no match_start snapshot for run {run}"))?;
    let snap = |round: u32, label: &str| {
        snaps
            .iter()
            .find(|s| s.round == Some(round) && s.label == label)
    };
    let actions_of = |v: &Value| -> Vec<Action> {
        serde_json::from_value(v["actions"].clone()).unwrap_or_default()
    };
    let replay = ReplayProvider::open(db, run, None)?;
    let p = FinalNight7Plugin::new();
    let mut rep = Report::default();
    let mut state: Value = serde_json::from_str(&start.raw)?;
    let last = resolves
        .keys()
        .chain(fallbacks.keys())
        .max()
        .copied()
        .unwrap_or(0);
    let mut compare =
        |round: u32, label: &str, got: &Value, rep: &mut Report| match snap(round, label) {
            Some(s) => {
                rep.snapshots += 1;
                // Byte for byte: the recorded text against this run's serialization.
                if s.raw == serde_json::to_string(got).unwrap_or_default() {
                    rep.identical += 1;
                } else {
                    let theirs: Value = serde_json::from_str(&s.raw).unwrap_or(Value::Null);
                    let d = first_diff(&theirs, got, "state")
                        .unwrap_or_else(|| "byte-level difference".into());
                    log(&format!("  beat {round:>3} {label:<17} DIFFERS at {d}"));
                    rep.differences.push(format!("beat {round} {label}: {d}"));
                }
            }
            None => log(&format!(
                "  beat {round:>3} {label:<17} (no snapshot recorded)"
            )),
        };
    for round in 1..=last {
        let rs = p.on_round_start(&state, round, &replay).await?;
        compare(round, "round_start", &rs.state, &mut rep);
        state = rs.state;
        let (out, label) = if let Some(i) = resolves.get(&round) {
            (
                p.resolve_round(&state, round, &actions_of(i), &replay, &NoProgress)
                    .await?,
                "resolved",
            )
        } else if let Some(i) = fallbacks.get(&round) {
            (
                p.fallback_outcome(&state, round, &actions_of(i)),
                "resolved_fallback",
            )
        } else {
            break;
        };
        compare(round, label, &out.state, &mut rep);
        state = out.state;
        let committed = p.commit_round(&state, round, &[], &replay).await?;
        compare(round, "committed", &committed.state, &mut rep);
        state = committed.state;
        rep.beats = round;
    }
    // The core runs the next beat's Start before it learns the match is over.
    if snap(last + 1, "round_start").is_some() {
        let rs = p.on_round_start(&state, last + 1, &replay).await?;
        compare(last + 1, "round_start", &rs.state, &mut rep);
        state = rs.state;
    }
    if let Some(f) = snaps.iter().rev().find(|s| s.label == "final") {
        rep.snapshots += 1;
        if f.raw == serde_json::to_string(&state)? {
            rep.identical += 1;
        } else {
            let theirs: Value = serde_json::from_str(&f.raw)?;
            let d = first_diff(&theirs, &state, "state").unwrap_or_default();
            log(&format!("  final DIFFERS at {d}"));
            rep.differences.push(format!("final: {d}"));
        }
    }
    rep.llm_hits = replay.hits.load(Ordering::Relaxed);
    rep.llm_misses = replay.misses.load(Ordering::Relaxed);
    Ok(rep)
}

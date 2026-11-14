//! Core recordings (JSONL, the store of record) → SQLite for analysis.
//!
//! The Theatre core records every match as `<THEATRE_RECORD_DIR>/<match_id>.jsonl` (one `theatre_trace::Record` per
//! line, tagged by `"type"`); `fn7-sim` and `fn7-agents` write the same format. [`ingest`] loads one such file into
//! generic tables (`runs`, `rulesets`, `hooks`, `llm_calls`, `http_exchanges`, `events`, `snapshots`,
//! `protocol_frames`), which the typed `fn7_*` projection ([`crate::project`]) reads. Re-ingesting a run replaces its
//! rows. Record ids are unique only within one server process, so each file's ids are shifted past the ids already in
//! the database (references between rows shift with them).

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection, Transaction};
use serde_json::Value;

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS rulesets (
    id TEXT PRIMARY KEY, plugin TEXT NOT NULL, version TEXT NOT NULL, label TEXT, created_at INTEGER NOT NULL,
    body TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS runs (
    id TEXT PRIMARY KEY, plugin TEXT NOT NULL, started_at INTEGER NOT NULL, ended_at INTEGER, settings TEXT, meta TEXT,
    summary TEXT, ruleset_id TEXT, rules_version TEXT, plugin_version TEXT, source TEXT, file TEXT
);
CREATE TABLE IF NOT EXISTS hooks (
    id INTEGER PRIMARY KEY, run_id TEXT NOT NULL, round INTEGER, hook TEXT NOT NULL, seat INTEGER,
    started_at INTEGER NOT NULL, duration_ms REAL NOT NULL, input TEXT, output TEXT, error TEXT
);
CREATE TABLE IF NOT EXISTS llm_calls (
    id INTEGER PRIMARY KEY, run_id TEXT NOT NULL, hook_id INTEGER, round INTEGER, step TEXT, actor TEXT,
    started_at INTEGER NOT NULL, duration_ms REAL NOT NULL, request TEXT NOT NULL, choices TEXT, cached INTEGER,
    error TEXT
);
CREATE TABLE IF NOT EXISTS http_exchanges (
    id INTEGER PRIMARY KEY, run_id TEXT NOT NULL, llm_call_id INTEGER, hook_id INTEGER, round INTEGER, step TEXT,
    actor TEXT, sample INTEGER, attempt INTEGER, started_at INTEGER NOT NULL, duration_ms REAL NOT NULL, url TEXT,
    status INTEGER, request TEXT, response TEXT, input_tokens INTEGER, output_tokens INTEGER, stop_reason TEXT,
    error TEXT, request_headers TEXT, response_headers TEXT
);
CREATE TABLE IF NOT EXISTS protocol_frames (
    id INTEGER PRIMARY KEY, run_id TEXT NOT NULL, actor TEXT, direction TEXT NOT NULL, at INTEGER NOT NULL,
    frame_type TEXT, raw TEXT NOT NULL, recipients TEXT
);
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY, run_id TEXT NOT NULL, hook_id INTEGER, llm_call_id INTEGER, round INTEGER, step TEXT,
    kind TEXT NOT NULL, actor TEXT, at INTEGER NOT NULL, data TEXT
);
CREATE TABLE IF NOT EXISTS snapshots (
    id INTEGER PRIMARY KEY, run_id TEXT NOT NULL, hook_id INTEGER, round INTEGER, label TEXT NOT NULL,
    at INTEGER NOT NULL, state TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS hooks_run    ON hooks (run_id, hook, round);
CREATE INDEX IF NOT EXISTS calls_run    ON llm_calls (run_id, step, actor, round);
CREATE INDEX IF NOT EXISTS calls_hook   ON llm_calls (hook_id);
CREATE INDEX IF NOT EXISTS http_call    ON http_exchanges (llm_call_id);
CREATE INDEX IF NOT EXISTS http_run     ON http_exchanges (run_id, step, round);
CREATE INDEX IF NOT EXISTS events_kind  ON events (run_id, kind, round, id);
CREATE INDEX IF NOT EXISTS events_actor ON events (run_id, actor, round, id);
CREATE INDEX IF NOT EXISTS events_hook  ON events (hook_id);
CREATE INDEX IF NOT EXISTS snaps_label  ON snapshots (run_id, label, round);
CREATE INDEX IF NOT EXISTS frames_type  ON protocol_frames (run_id, frame_type, at);
CREATE INDEX IF NOT EXISTS runs_started ON runs (started_at);

-- Spend and latency per run / round / step.
CREATE VIEW IF NOT EXISTS v_http_by_step AS
    SELECT run_id, round, step, COUNT(*) AS requests,
           SUM(input_tokens) AS input_tokens, SUM(output_tokens) AS output_tokens,
           ROUND(AVG(duration_ms)) AS avg_ms, MAX(duration_ms) AS max_ms,
           SUM(error IS NOT NULL) AS errors, SUM(stop_reason = 'refusal') AS refusals
    FROM http_exchanges GROUP BY run_id, round, step;
"#;

const ID_TABLES: &[&str] = &["hooks", "llm_calls", "http_exchanges", "events", "snapshots", "protocol_frames"];

/// Create the generic tables (idempotent).
pub fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA)?;
    Ok(())
}

/// What one ingest loaded.
#[derive(Debug, Default)]
pub struct Ingested {
    pub run: String,
    pub records: usize,
    pub skipped_lines: usize,
}

fn js(v: &Value) -> Option<String> {
    (!v.is_null()).then(|| v.to_string())
}

fn s(v: &Value) -> Option<&str> {
    v.as_str()
}

fn i(v: &Value) -> Option<i64> {
    v.as_i64()
}

/// Load one JSONL recording (one run) into `conn`, replacing that run's rows.
pub fn ingest(conn: &mut Connection, path: &Path) -> Result<Ingested> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut out = Ingested::default();
    let mut recs = vec![];
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<Value>(line) {
            Ok(v) => recs.push(v),
            Err(_) => out.skipped_lines += 1,
        }
    }
    out.run = recs
        .iter()
        .find_map(|r| s(&r["run"]).or(s(&r["ctx"]["run"])))
        .ok_or_else(|| anyhow!("{}: no records", path.display()))?
        .to_string();
    ensure_schema(conn)?;
    let tx = conn.transaction()?;
    for t in ID_TABLES {
        tx.execute(&format!("DELETE FROM {t} WHERE run_id = ?1"), params![out.run])?;
    }
    tx.execute("DELETE FROM runs WHERE id = ?1", params![out.run])?;
    let mut offset = 0i64;
    for t in ID_TABLES {
        let m: Option<i64> = tx.query_row(&format!("SELECT MAX(id) FROM {t}"), [], |r| r.get(0))?;
        offset = offset.max(m.unwrap_or(0));
    }
    let file = path.display().to_string();
    for r in &recs {
        insert(&tx, r, offset, &file)?;
        out.records += 1;
    }
    tx.commit()?;
    Ok(out)
}

fn insert(tx: &Transaction, r: &Value, off: i64, file: &str) -> Result<()> {
    let id = |v: &Value| v.as_i64().map(|n| n + off);
    let ctx = &r["ctx"];
    match r["type"].as_str().unwrap_or("") {
        "run_start" => {
            tx.execute(
                "INSERT OR REPLACE INTO runs (id, plugin, started_at, settings, meta, source, file) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![s(&r["run"]), s(&r["plugin"]), i(&r["at"]), js(&r["settings"]), js(&r["meta"]),
                        s(&r["meta"]["source"]).unwrap_or("theatre-server"), file],
            )?;
        }
        "run_end" => {
            tx.execute(
                "UPDATE runs SET ended_at = ?2, summary = ?3 WHERE id = ?1",
                params![s(&r["run"]), i(&r["at"]), js(&r["summary"])],
            )?;
        }
        "ruleset" => {
            tx.execute(
                "INSERT OR IGNORE INTO rulesets (id, plugin, version, label, created_at, body) VALUES (?1,?2,?3,?4,?5,?6)",
                params![s(&r["id"]), s(&r["plugin"]), s(&r["version"]), s(&r["label"]), i(&r["at"]), r["body"].to_string()],
            )?;
            // The core's ruleset carries the plugin version and rules_version; a plugin's richer body (trace::ruleset)
            // becomes the run's ruleset_id.
            if r["body"].get("settings").is_some() && r["body"].get("mechanics").is_none() {
                tx.execute(
                    "UPDATE runs SET plugin_version = ?2, rules_version = ?3, ruleset_id = COALESCE(ruleset_id, ?4) WHERE id = ?1",
                    params![s(&r["run"]), s(&r["version"]), s(&r["label"]), s(&r["id"])],
                )?;
            } else {
                tx.execute("UPDATE runs SET ruleset_id = ?2 WHERE id = ?1", params![s(&r["run"]), s(&r["id"])])?;
            }
        }
        "hook" => {
            tx.execute(
                "INSERT INTO hooks (id, run_id, round, hook, seat, started_at, duration_ms, input, output, error) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![id(&r["id"]), s(&r["run"]), i(&r["round"]), s(&r["hook"]), i(&r["seat"]), i(&r["started_at"]),
                        r["duration_ms"].as_f64().unwrap_or(0.0), r["input"].to_string(), r["output"].to_string(), s(&r["error"])],
            )?;
        }
        "llm_call" => {
            tx.execute(
                "INSERT INTO llm_calls (id, run_id, hook_id, round, step, actor, started_at, duration_ms, request, choices, cached, error) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                params![id(&r["id"]), s(&r["run"]), id(&r["hook_id"]), i(&r["round"]), s(&r["step"]), s(&r["actor"]),
                        i(&r["started_at"]), r["duration_ms"].as_f64().unwrap_or(0.0), r["request"].to_string(),
                        js(&r["choices"]), r["cached"].as_bool(), s(&r["error"])],
            )?;
        }
        "http" => {
            tx.execute(
                "INSERT INTO http_exchanges (id, run_id, llm_call_id, hook_id, round, step, actor, sample, attempt, started_at, duration_ms, url, status, request, response, input_tokens, output_tokens, stop_reason, error, request_headers, response_headers) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)",
                params![id(&ctx["id"]), s(&ctx["run"]), id(&ctx["llm_call_id"]), id(&ctx["hook_id"]), i(&ctx["round"]),
                        s(&ctx["step"]), s(&ctx["actor"]), i(&r["sample"]), i(&r["attempt"]), i(&r["started_at"]),
                        r["duration_ms"].as_f64().unwrap_or(0.0), s(&r["url"]), i(&r["status"]), js(&r["request"]),
                        js(&r["response"]), i(&r["input_tokens"]), i(&r["output_tokens"]), s(&r["stop_reason"]),
                        s(&r["error"]), js(&r["request_headers"]), js(&r["response_headers"])],
            )?;
        }
        "event" => {
            tx.execute(
                "INSERT INTO events (id, run_id, hook_id, llm_call_id, round, step, kind, actor, at, data) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![id(&r["id"]), s(&ctx["run"]), id(&ctx["hook_id"]), id(&ctx["llm_call_id"]), i(&ctx["round"]),
                        s(&ctx["step"]), s(&r["kind"]), s(&r["actor"]), i(&r["at"]), r["data"].to_string()],
            )?;
        }
        "snapshot" => {
            tx.execute(
                "INSERT INTO snapshots (id, run_id, hook_id, round, label, at, state) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![id(&r["id"]), s(&ctx["run"]), id(&ctx["hook_id"]), i(&ctx["round"]), s(&r["label"]), i(&r["at"]),
                        r["state"].to_string()],
            )?;
        }
        "frame" => {
            tx.execute(
                "INSERT INTO protocol_frames (id, run_id, actor, direction, at, frame_type, raw, recipients) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![id(&r["id"]), s(&r["run"]), s(&r["actor"]), s(&r["direction"]), i(&r["at"]), s(&r["frame_type"]),
                        s(&r["raw"]), js(&r["recipients"])],
            )?;
        }
        _ => {}
    }
    Ok(())
}

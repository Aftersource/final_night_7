//! The duel data store (docs/DUEL_MOVES.md §2): `<data>/fn7/duel_lines.sqlite`, one row per typed duel line (state
//! schema 14) and the picker's cache, plus the key-gated rollups `GET /api/duel/lines?by=line|clip|class|disagreement|
//! unmatched`. The recording's `duel_line` events stay the record of truth; this is the queryable copy the plugin
//! writes as each duel ends ([`fn7_plugin::picker::DuelData`]), and the cache that lets a repeated line be picked the
//! moment it arrives.
//!
//! Configuration: `FN7_DUEL_LINES_DB` (a path), else `<THEATRE_DATA_DIR>/fn7/duel_lines.sqlite`; `FN7_DUEL_LINES=off`
//! turns it off. The routes need `?key=` or `Authorization: Bearer` equal to `FN7_DUEL_LINES_KEY`, else
//! `FN7_PROFILE_KEY`, else the admin token; with none configured they answer 403.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use fn7_agents::duel_pick::Pick;
use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::{json, Value};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS duel_lines (
  match_id TEXT NOT NULL, duel_id TEXT NOT NULL, n INTEGER NOT NULL,
  beat INTEGER, at_ms INTEGER, seat INTEGER, account TEXT, player TEXT, demigod TEXT, ai INTEGER NOT NULL DEFAULT 0,
  line TEXT, normalized TEXT, list_version TEXT,
  mapper_word TEXT, mapper_class TEXT, mapper_clip TEXT,
  picker_class TEXT, picker_clips TEXT, picker_path TEXT, picker_object TEXT, picker_target TEXT,
  picker_latency_ms INTEGER, picker_tokens_in INTEGER, picker_tokens_out INTEGER, picker_cost_usd REAL,
  picker_used INTEGER, picker_pre INTEGER, picker_cached INTEGER, picker_error TEXT,
  class TEXT, result TEXT, verdict TEXT, damage REAL, range TEXT,
  staged_clip TEXT, staged_setup TEXT, staged_path TEXT, weapon_class TEXT, draw TEXT,
  row TEXT NOT NULL, written_ms INTEGER NOT NULL,
  PRIMARY KEY (match_id, duel_id, n));
CREATE INDEX IF NOT EXISTS duel_lines_norm ON duel_lines(normalized);
CREATE TABLE IF NOT EXISTS pick_cache (key TEXT PRIMARY KEY, pick TEXT NOT NULL, model TEXT, at_ms INTEGER NOT NULL);
";

/// The store (one connection behind a mutex: rows are few and small).
pub struct DuelLines {
    db: Mutex<Connection>,
    pub path: PathBuf,
}

impl DuelLines {
    pub fn open(path: &Path) -> anyhow::Result<DuelLines> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let c = Connection::open(path)?;
        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=2000;")?;
        c.execute_batch(SCHEMA)?;
        Ok(DuelLines { db: Mutex::new(c), path: path.to_path_buf() })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Upsert one `duel_line` record.
    pub fn put(&self, run: &str, r: &Value) -> rusqlite::Result<()> {
        let s = |v: &Value| v.as_str().map(str::to_string);
        let j = |v: &Value| (!v.is_null()).then(|| v.to_string());
        let p = &r["picker"];
        let pick = &p["pick"];
        self.conn().execute(
            "INSERT OR REPLACE INTO duel_lines VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33,?34,?35,?36,?37,?38,?39,?40,?41)",
            params![
                run, s(&r["duel_id"]).unwrap_or_default(), r["n"].as_i64().unwrap_or(0), r["beat"].as_i64(), r["at_ms"].as_i64(), r["seat"].as_i64(),
                s(&r["account"]), s(&r["player"]), s(&r["demigod"]), (r["ai"] == true) as i64,
                s(&r["line"]), s(&r["normalized"]), s(&r["list_version"]),
                s(&r["mapper"]["word"]), s(&r["mapper"]["class"]), s(&r["mapper"]["clips"][0]),
                s(&pick["move_class"]), j(&pick["clips"]), s(&pick["path"]), s(&pick["object_id"]), s(&pick["target_part"]),
                p["latency_ms"].as_i64(), p["tokens_in"].as_i64(), p["tokens_out"].as_i64(), p["cost_usd"].as_f64(),
                p["used"].as_bool().map(|b| b as i64), p["pre"].as_bool().map(|b| b as i64), p["cached"].as_bool().map(|b| b as i64), s(&p["error"]),
                s(&r["class"]), s(&r["result"]), s(&r["verdict"]), r["damage"].as_f64(), s(&r["range"]),
                s(&r["staged"]["clip"]), j(&r["staged"]["setup"]), s(&r["staged"]["path"]), s(&r["staged"]["weapon_class"]), j(&r["staged"]["draw"]),
                r.to_string(), theatre_trace::now_ms(),
            ],
        )?;
        Ok(())
    }

    pub fn count(&self) -> i64 {
        self.conn().query_row("SELECT COUNT(*) FROM duel_lines", [], |r| r.get(0)).unwrap_or(0)
    }

    /// A rollup (`by`: line, clip, class, disagreement, unmatched), most frequent first.
    pub fn rollup(&self, by: &str, ai: bool, limit: u32) -> Result<Value, String> {
        let who = if ai { "1=1" } else { "ai = 0" };
        let sql = match by {
            "line" => format!("SELECT normalized, COUNT(*), GROUP_CONCAT(DISTINCT class), GROUP_CONCAT(DISTINCT staged_clip), SUM(mapper_word IS NULL), SUM(picker_used)
                               FROM duel_lines WHERE {who} GROUP BY normalized ORDER BY COUNT(*) DESC, normalized LIMIT ?1"),
            "clip" => format!("SELECT staged_clip, COUNT(*), GROUP_CONCAT(DISTINCT class), GROUP_CONCAT(DISTINCT weapon_class), SUM(picker_used), AVG(damage)
                               FROM duel_lines WHERE {who} GROUP BY staged_clip ORDER BY COUNT(*) DESC, staged_clip LIMIT ?1"),
            "class" => format!("SELECT class, COUNT(*), GROUP_CONCAT(DISTINCT mapper_class), GROUP_CONCAT(DISTINCT picker_class), SUM(verdict IN ('success','crit_success')), AVG(damage)
                                FROM duel_lines WHERE {who} GROUP BY class ORDER BY COUNT(*) DESC, class LIMIT ?1"),
            "disagreement" => format!("SELECT normalized, COUNT(*), mapper_class, picker_class, GROUP_CONCAT(DISTINCT mapper_clip), GROUP_CONCAT(DISTINCT json_extract(picker_clips, '$[0]'))
                                       FROM duel_lines WHERE {who} AND picker_class IS NOT NULL AND (mapper_class IS NULL OR picker_class != mapper_class)
                                       GROUP BY normalized, mapper_class, picker_class ORDER BY COUNT(*) DESC, normalized LIMIT ?1"),
            "unmatched" => format!("SELECT normalized, COUNT(*), GROUP_CONCAT(DISTINCT class), GROUP_CONCAT(DISTINCT picker_class), GROUP_CONCAT(DISTINCT staged_clip), NULL
                                    FROM duel_lines WHERE {who} AND mapper_word IS NULL GROUP BY normalized ORDER BY COUNT(*) DESC, normalized LIMIT ?1"),
            _ => return Err("by must be line, clip, class, disagreement or unmatched".into()),
        };
        let cols: [&str; 6] = match by {
            "line" => ["line", "count", "classes", "clips", "no_move_word", "picker_used"],
            "clip" => ["clip", "count", "classes", "weapon_classes", "picker_used", "avg_damage"],
            "class" => ["class", "count", "mapper_classes", "picker_classes", "successes", "avg_damage"],
            "disagreement" => ["line", "count", "mapper_class", "picker_class", "mapper_clips", "picker_clips"],
            _ => ["line", "count", "classes", "picker_classes", "clips", "_"],
        };
        let c = self.conn();
        let mut st = c.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = st
            .query_map(params![limit.clamp(1, 1000)], |r| {
                let mut o = serde_json::Map::new();
                for (i, name) in cols.iter().enumerate().filter(|(_, n)| **n != "_") {
                    let v: rusqlite::types::Value = r.get(i)?;
                    o.insert((*name).into(), match v {
                        rusqlite::types::Value::Null => Value::Null,
                        rusqlite::types::Value::Integer(x) => json!(x),
                        rusqlite::types::Value::Real(x) => json!((x * 100.0).round() / 100.0),
                        rusqlite::types::Value::Text(t) => json!(t),
                        rusqlite::types::Value::Blob(_) => Value::Null,
                    });
                }
                Ok(Value::Object(o))
            })
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        let total: i64 = c.query_row(&format!("SELECT COUNT(*) FROM duel_lines WHERE {who}"), [], |r| r.get(0)).unwrap_or(0);
        Ok(json!({ "by": by, "lines": total, "ai": ai, "rows": rows }))
    }
}

impl fn7_plugin::picker::DuelData for DuelLines {
    fn line(&self, run: &str, row: &Value) {
        if let Err(e) = self.put(run, row) {
            tracing::warn!(error = %e, "duel lines: a row was not written");
        }
    }
    fn cached(&self) -> Vec<(String, Pick)> {
        let c = self.conn();
        let Ok(mut st) = c.prepare("SELECT key, pick FROM pick_cache") else { return vec![] };
        st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map(|it| it.filter_map(Result::ok).filter_map(|(k, p)| Some((k, serde_json::from_str(&p).ok()?))).collect())
            .unwrap_or_default()
    }
    fn cache(&self, key: &str, pick: &Pick, model: &str) {
        let _ = self.conn().execute("INSERT OR REPLACE INTO pick_cache VALUES (?1, ?2, ?3, ?4)",
                                    params![key, serde_json::to_string(pick).unwrap_or_default(), model, theatre_trace::now_ms()]);
    }
}

/// Open the store from the environment (`None`: turned off).
pub fn open(data_dir: &Path, get: impl Fn(&str) -> Option<String>) -> anyhow::Result<Option<Arc<DuelLines>>> {
    if get("FN7_DUEL_LINES").is_some_and(|v| matches!(v.trim(), "off" | "0" | "false")) {
        return Ok(None);
    }
    let path = get("FN7_DUEL_LINES_DB").filter(|p| !p.trim().is_empty()).map(PathBuf::from)
        .unwrap_or_else(|| data_dir.join("fn7").join("duel_lines.sqlite"));
    Ok(Some(Arc::new(DuelLines::open(&path)?)))
}

#[derive(Clone)]
struct St {
    store: Arc<DuelLines>,
    key: Option<String>,
}

#[derive(Deserialize, Default)]
struct Q {
    key: Option<String>,
    by: Option<String>,
    ai: Option<String>,
    limit: Option<u32>,
}

fn eq_ct(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn finish(code: StatusCode, v: Value) -> Response {
    let mut r = (code, Json(v)).into_response();
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

async fn lines(State(st): State<St>, Query(q): Query<Q>, headers: HeaderMap) -> Response {
    let given = q.key.clone().or_else(|| {
        headers.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Bearer ").map(|t| t.trim().to_string())
    });
    match (&st.key, given) {
        (Some(k), Some(g)) if eq_ct(k, &g) => {}
        (None, _) => return finish(StatusCode::FORBIDDEN, json!({ "error": "no_key", "detail": "set FN7_DUEL_LINES_KEY (or FN7_PROFILE_KEY)" })),
        _ => return finish(StatusCode::FORBIDDEN, json!({ "error": "key" })),
    }
    let by = q.by.clone().unwrap_or_else(|| "line".into());
    let ai = q.ai.as_deref().is_some_and(|v| matches!(v, "1" | "true" | "yes"));
    let store = st.store.clone();
    let limit = q.limit.unwrap_or(40);
    match tokio::task::spawn_blocking(move || store.rollup(&by, ai, limit)).await {
        Ok(Ok(v)) => finish(StatusCode::OK, v),
        Ok(Err(e)) => finish(StatusCode::BAD_REQUEST, json!({ "error": "by", "detail": e })),
        Err(e) => finish(StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": "store", "detail": e.to_string() })),
    }
}

/// `GET /api/duel/lines?by=…&ai=1&limit=N&key=…`.
pub fn router(store: Arc<DuelLines>, key: Option<String>) -> Router {
    Router::new().route("/api/duel/lines", get(lines)).with_state(St { store, key })
}

/// The routes' key from the environment.
pub fn key_from(get: impl Fn(&str) -> Option<String>, admin: Option<String>) -> Option<String> {
    get("FN7_DUEL_LINES_KEY").or_else(|| get("FN7_PROFILE_KEY")).or(admin).filter(|k| !k.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fn7_plugin::picker::DuelData;

    #[test]
    fn rows_upsert_and_roll_up() {
        let dir = std::env::temp_dir().join(format!("fn7-duel-lines-{}-{}", std::process::id(), theatre_trace::now_ms()));
        let s = DuelLines::open(&dir.join("duel_lines.sqlite")).unwrap();
        let row = |n: u32, line: &str, mapper: Option<&str>, picked: Option<&str>| json!({
            "duel_id": "duel:s1:0:1", "n": n, "beat": 2, "line": line, "normalized": line, "ai": false, "class": "strike", "verdict": "success",
            "damage": 1.0, "mapper": mapper.map(|c| json!({ "word": "x", "class": c, "clips": ["jab"] })),
            "picker": picked.map(|c| json!({ "pick": { "move_class": c, "clips": ["kick"] }, "used": true, "latency_ms": 420, "tokens_in": 900 })),
            "staged": { "clip": "kick", "weapon_class": "unarmed" } });
        s.line("m1", &row(1, "kick him", Some("strike"), Some("strike")));
        s.line("m1", &row(1, "kick him", Some("strike"), Some("strike")));
        s.line("m1", &row(2, "dance at him", None, Some("taunt")));
        s.line("m1", &row(3, "smash", Some("heavy"), Some("strike")));
        assert_eq!(s.count(), 3, "an upsert by match, duel and blow");
        assert_eq!(s.rollup("unmatched", false, 10).unwrap()["rows"][0]["line"], "dance at him");
        let d = s.rollup("disagreement", false, 10).unwrap();
        assert_eq!(d["rows"].as_array().unwrap().len(), 2, "{d}");
        assert_eq!(s.rollup("clip", false, 10).unwrap()["rows"][0]["clip"], "kick");
        assert!(s.rollup("vibes", false, 10).is_err());
        let p: Pick = serde_json::from_value(json!({ "move_class": "strike", "clips": ["kick"], "path": null, "object_id": null, "target_part": "legs" })).unwrap();
        s.cache("kick him|mid|v", &p, "m");
        assert_eq!(s.cached(), vec![("kick him|mid|v".to_string(), p)]);
        let _ = std::fs::remove_dir_all(dir);
    }
}

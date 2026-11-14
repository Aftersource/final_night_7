//! # fn7-assets
//!
//! The asset registry: the single record of every generated asset of Final Night 7 (docs/ASSETS.md).
//!
//! - **Files** ([`files`]): content-addressed under `<THEATRE_DATA_DIR>/fn7/assets/` in subdirectories by kind
//!   (`images/portrait/<sha256>.png`, `models/statue/<sha256>.glb`, `textures/skin/…`, `animations/clip/…`).
//! - **SQL** ([`schema`]): `assets.sqlite` beside them: `assets`, `generations` (one row per provider call, with the
//!   exact prompt, parameters, timing, cost and who asked), `generation_outputs`, `lineage`, `uses`, and the
//!   `current_assets` view. Nothing is overwritten: a new version is a new row.
//! - **Ingestion**: [`live`] (the media layer's observer and the payload walker: every image as it is requested,
//!   finished, failed or refused), [`ingest`] (backfill from a media cache, the profile library and recordings),
//!   [`provenance`] (the Unreal repo's Meshy provenance files and the JSON-lines job records).
//! - **API** ([`http`]): `/api/assets…`.

pub mod files;
pub mod http;
pub mod ingest;
pub mod live;
pub mod provenance;
pub mod query;
pub mod schema;
pub mod subject;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub use files::{Link, Stored};
pub use subject::Subject;

pub type Result<T> = anyhow::Result<T>;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// The assets root: `FN7_ASSETS_DIR`, else `<data_dir>/fn7/assets`.
pub fn default_root(data_dir: &Path, env: impl Fn(&str) -> Option<String>) -> PathBuf {
    env("FN7_ASSETS_DIR")
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir.join("fn7").join("assets"))
}

fn short_id(prefix: &str, identity: &str) -> String {
    let h = Sha256::digest(identity.as_bytes());
    format!("{prefix}_{}", &hex::encode(h)[..24])
}

/// The id of the asset behind a media hash (`GET /media/<hash>.<ext>`): a pure function of the hash.
pub fn asset_id_for_media(media_hash: &str) -> String {
    short_id("ast", &format!("media:{media_hash}"))
}

/// The id of an asset identified by a free key (a file's hash and role, a provider task and output).
pub fn asset_id_for_key(key: &str) -> String {
    short_id("ast", key)
}

pub fn generation_id_for_key(key: &str) -> String {
    short_id("gen", key)
}

/// How much a source knows: a richer source's values win over a poorer one's, never the other way.
fn rank(source: &str) -> u8 {
    match source {
        "sidecar" | "backfill" => 1,
        _ => 2,
    }
}

/// One asset as an ingester describes it. `None` fields are left as they are in an existing row.
#[derive(Debug, Clone, Default)]
pub struct AssetIn {
    pub id: String,
    pub kind: String,
    pub subkind: String,
    pub subject: Option<Subject>,
    /// Replace an existing row's subject (a provenance file names it); otherwise only an unknown subject is filled.
    pub subject_strong: bool,
    pub variant: String,
    pub file: Option<Stored>,
    pub created_at: i64,
    /// `pending | ready | failed | refused | external`.
    pub status: String,
    pub status_reason: Option<String>,
    pub style_version: Option<String>,
    pub prompt_version: Option<String>,
    pub media_hash: Option<String>,
    pub source: String,
    pub origin_path: Option<String>,
    pub meta: Option<Value>,
}

/// One provider call as an ingester describes it. `None` fields are left as they are in an existing row.
#[derive(Debug, Clone, Default)]
pub struct GenerationIn {
    pub id: String,
    pub asset_id: String,
    pub provider: String,
    pub model: Option<String>,
    pub operation: Option<String>,
    pub endpoint: Option<String>,
    pub prompt: Option<String>,
    pub negative_prompt: Option<String>,
    pub params: Option<Value>,
    pub reference_asset_ids: Option<Vec<String>>,
    pub provider_task_id: Option<String>,
    pub requested_at: Option<i64>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub duration_ms: Option<f64>,
    pub cost_usd: Option<f64>,
    pub cost_credits: Option<f64>,
    pub cost_basis: Option<String>,
    pub usage: Option<Value>,
    /// `pending | ok | failed | refused`.
    pub status: String,
    pub error: Option<String>,
    pub requested_by: Option<String>,
    pub requested_match: Option<String>,
    pub requested_beat: Option<i64>,
    pub requested_seat: Option<i64>,
    pub requested_account: Option<String>,
    pub source: String,
}

/// One sighting of an asset in a match.
#[derive(Debug, Clone, Default)]
pub struct UseIn {
    pub asset_id: String,
    pub match_id: String,
    /// `None` = a match-level record (no beat known).
    pub beat: Option<i64>,
    pub role: String,
    pub subject_ref: String,
    pub subject_kind: Option<String>,
    pub subject_key: Option<String>,
    pub loadout_hash: Option<String>,
    pub seat: Option<i64>,
    pub at_ms: i64,
    pub source: String,
}

/// The registry: the files root and its database. Cheap to share behind an `Arc`; every call takes the one
/// connection's lock (SQLite in WAL mode; other processes may read, and the CLI may write, while the server runs).
pub struct Registry {
    root: PathBuf,
    conn: Mutex<Connection>,
}

impl Registry {
    /// Open (creating and migrating) the registry rooted at `root`: `<root>/assets.sqlite` and the file tree.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        let mut conn = Connection::open(root.join("assets.sqlite"))?;
        conn.busy_timeout(std::time::Duration::from_secs(30))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        schema::migrate(&mut conn)?;
        Ok(Self { root, conn: Mutex::new(conn) })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn db_path(&self) -> PathBuf {
        self.root.join("assets.sqlite")
    }

    /// The absolute path of a stored file (`path` as in `assets.path`); `None` if it would leave the root.
    pub fn file_path(&self, rel: &str) -> Option<PathBuf> {
        let ok = !rel.is_empty()
            && !rel.starts_with('/')
            && rel.split('/').all(|p| !p.is_empty() && p != "." && p != ".." && !p.contains('\\'));
        ok.then(|| self.root.join(rel))
    }

    /// Run `f` with the connection (one lock; no transaction).
    pub fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let c = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        f(&c)
    }

    /// Run `f` in one transaction (rolled back on error).
    pub fn tx<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let mut c = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = c.transaction()?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }

    pub fn store_file(&self, src: &Path, kind: &str, subkind: &str, link: Link) -> Result<Stored> {
        Ok(files::store_file(&self.root, src, kind, subkind, link)?)
    }

    pub fn store_bytes(&self, data: &[u8], kind: &str, subkind: &str, ext: &str) -> Result<Stored> {
        Ok(files::store_bytes(&self.root, data, kind, subkind, ext)?)
    }

    pub fn upsert_asset(&self, a: &AssetIn) -> Result<String> {
        self.with(|c| upsert_asset(c, a))
    }
    pub fn upsert_generation(&self, g: &GenerationIn) -> Result<String> {
        self.with(|c| upsert_generation(c, g))
    }
    pub fn add_lineage(&self, parent: &str, child: &str, role: &str, note: Option<&str>) -> Result<()> {
        self.with(|c| add_lineage(c, parent, child, role, note))
    }
    pub fn add_output(&self, generation: &str, asset: &str, role: &str) -> Result<()> {
        self.with(|c| add_output(c, generation, asset, role))
    }
    pub fn note_use(&self, u: &UseIn, extend: bool) -> Result<()> {
        self.with(|c| note_use(c, u, extend))
    }
    pub fn asset_id_by_media(&self, hash: &str) -> Result<Option<String>> {
        self.with(|c| {
            Ok(c.query_row("SELECT id FROM assets WHERE media_hash = ?1", [hash], |r| r.get(0)).optional()?)
        })
    }
    pub fn schema_version(&self) -> Result<i64> {
        self.with(|c| Ok(c.query_row("PRAGMA user_version", [], |r| r.get(0))?))
    }
    pub fn log_run(&self, kind: &str, source: &str, started_at: i64, report: &Value) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO ingest_runs (kind, source, started_at, finished_at, report) VALUES (?1,?2,?3,?4,?5)",
                params![kind, source, started_at, now_ms(), report.to_string()],
            )?;
            Ok(())
        })
    }
}

fn json_text(v: &Option<Value>) -> Option<String> {
    v.as_ref().map(|v| v.to_string())
}

/// Insert the asset, or merge it into the row with its id. A file, once set, is never replaced; `ready` is final;
/// other fields are filled where the row has none.
pub fn upsert_asset(c: &Connection, a: &AssetIn) -> Result<String> {
    anyhow::ensure!(schema::KINDS.contains(&a.kind.as_str()), "asset kind {:?}", a.kind);
    anyhow::ensure!(schema::STATUSES.contains(&a.status.as_str()), "asset status {:?}", a.status);
    let now = now_ms();
    type Old = (String, Option<String>, String, String, String);
    let old: Option<Old> = c
        .query_row("SELECT status, sha256, subject_key, subject_kind, display_name FROM assets WHERE id = ?1", [&a.id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .optional()?;
    let subj = a.subject.clone().unwrap_or_else(Subject::unknown);
    let f = a.file.as_ref();
    match old {
        None => {
            c.execute(
                "INSERT INTO assets (id, sha256, kind, subkind, subject_kind, subject_key, variant, display_name, path, bytes,
                    mime, width, height, tris, created_at, updated_at, status, status_reason, style_version, prompt_version,
                    media_hash, source, origin_path, meta)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24)",
                params![
                    a.id, f.map(|f| &f.sha256), a.kind, a.subkind, subj.kind, subj.key, a.variant, subj.name,
                    f.map(|f| &f.path), f.map(|f| f.bytes as i64), f.map(|f| f.mime), f.and_then(|f| f.width),
                    f.and_then(|f| f.height), f.and_then(|f| f.tris.map(|t| t as i64)), a.created_at, now, a.status,
                    a.status_reason, a.style_version, a.prompt_version, a.media_hash, a.source, a.origin_path,
                    json_text(&a.meta),
                ],
            )?;
        }
        Some((old_status, old_sha, old_key, old_kind, old_name)) => {
            let was_ready = old_status == "ready";
            // `ready` is final; a `pending` report never downgrades a row that already has an outcome... except a
            // fresh request after a failure or refusal (another match retries it).
            let status = if was_ready { "ready".to_string() } else { a.status.clone() };
            let becomes_ready = !was_ready && status == "ready";
            let take_file = old_sha.is_none() && f.is_some();
            if take_file {
                let f = f.unwrap();
                c.execute(
                    "UPDATE assets SET sha256=?2, path=?3, bytes=?4, mime=?5, width=?6, height=?7, tris=?8 WHERE id=?1",
                    params![a.id, f.sha256, f.path, f.bytes as i64, f.mime, f.width, f.height, f.tris.map(|t| t as i64)],
                )?;
            }
            let set_subject = a.subject.is_some()
                && (a.subject_strong || old_key == "unknown")
                && subj.key != "unknown"
                && (subj.key != old_key || subj.kind != old_kind || subj.name != old_name);
            if set_subject {
                c.execute(
                    "UPDATE assets SET subject_kind=?2, subject_key=?3, display_name=?4 WHERE id=?1",
                    params![a.id, subj.kind, subj.key, subj.name],
                )?;
            }
            let n = c.execute(
                "UPDATE assets SET status=?2,
                    status_reason = CASE WHEN ?2 = 'ready' THEN NULL WHEN ?3 IS NOT NULL THEN ?3 ELSE status_reason END,
                    created_at = CASE WHEN ?4 THEN ?5 ELSE created_at END,
                    style_version = COALESCE(style_version, ?6), prompt_version = COALESCE(prompt_version, ?7),
                    media_hash = COALESCE(media_hash, ?8), origin_path = COALESCE(origin_path, ?9),
                    meta = COALESCE(meta, ?10)
                 WHERE id=?1 AND (status IS NOT ?2 OR (?3 IS NOT NULL AND status_reason IS NOT ?3) OR ?4
                    OR (style_version IS NULL AND ?6 IS NOT NULL) OR (prompt_version IS NULL AND ?7 IS NOT NULL)
                    OR (media_hash IS NULL AND ?8 IS NOT NULL) OR (origin_path IS NULL AND ?9 IS NOT NULL)
                    OR (meta IS NULL AND ?10 IS NOT NULL))",
                params![
                    a.id, status, if status == "ready" { None } else { a.status_reason.clone() }, becomes_ready,
                    a.created_at, a.style_version, a.prompt_version, a.media_hash, a.origin_path, json_text(&a.meta),
                ],
            )?;
            if n > 0 || take_file || set_subject {
                c.execute("UPDATE assets SET updated_at=?2 WHERE id=?1", params![a.id, now])?;
            }
        }
    }
    Ok(a.id.clone())
}

/// Insert the generation, or merge it into the row with its id: `ok` is final, a richer source's values replace a
/// poorer one's, and a value is never replaced by nothing. Also records the asset as the call's primary output.
pub fn upsert_generation(c: &Connection, g: &GenerationIn) -> Result<String> {
    let old: Option<(String, String)> = c
        .query_row("SELECT status, source FROM generations WHERE id = ?1", [&g.id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let refs = g.reference_asset_ids.as_ref().map(|r| serde_json::to_string(r).unwrap_or_default());
    match old {
        None => {
            c.execute(
                "INSERT INTO generations (id, asset_id, provider, model, operation, endpoint, prompt, negative_prompt, params,
                    reference_asset_ids, provider_task_id, requested_at, started_at, finished_at, duration_ms, cost_usd,
                    cost_credits, cost_basis, usage, status, error, requested_by, requested_match, requested_beat,
                    requested_seat, requested_account, source, recorded_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28)",
                params![
                    g.id, g.asset_id, g.provider, g.model, g.operation, g.endpoint, g.prompt, g.negative_prompt,
                    json_text(&g.params), refs, g.provider_task_id, g.requested_at, g.started_at, g.finished_at,
                    g.duration_ms, g.cost_usd, g.cost_credits, g.cost_basis, json_text(&g.usage), g.status, g.error,
                    g.requested_by, g.requested_match, g.requested_beat, g.requested_seat, g.requested_account,
                    g.source, now_ms(),
                ],
            )?;
        }
        Some((old_status, old_source)) => {
            // The outcome: `ok` stays; a `pending` report never replaces an outcome.
            let status = match (old_status.as_str(), g.status.as_str()) {
                ("ok", _) => "ok",
                (o, "pending") => o,
                (_, n) => n,
            }
            .to_string();
            // richer: the new values win where both have one; poorer: the old ones do.
            let richer = rank(&g.source) >= rank(&old_source);
            // Parameters are merged key by key (the request knows the reserved price, the exchange the rest).
            let old_params: Option<String> = c.query_row("SELECT params FROM generations WHERE id = ?1", [&g.id], |r| r.get(0))?;
            let merged_params = match (old_params.and_then(|p| serde_json::from_str::<Value>(&p).ok()), &g.params) {
                (Some(Value::Object(mut o)), Some(Value::Object(n))) => {
                    for (k, v) in n {
                        if richer || !o.contains_key(k) {
                            o.insert(k.clone(), v.clone());
                        }
                    }
                    Some(Value::Object(o))
                }
                (_, new) => new.clone(),
            };
            let sql = if richer {
                "UPDATE generations SET status=?2, provider=?3,
                    model=COALESCE(?4,model), operation=COALESCE(?5,operation), endpoint=COALESCE(?6,endpoint),
                    prompt=COALESCE(prompt,?7), negative_prompt=COALESCE(negative_prompt,?8), params=COALESCE(?9,params),
                    reference_asset_ids=COALESCE(?10,reference_asset_ids), provider_task_id=COALESCE(?11,provider_task_id),
                    requested_at=COALESCE(requested_at,?12), started_at=COALESCE(?13,started_at),
                    finished_at=COALESCE(?14,finished_at), duration_ms=COALESCE(?15,duration_ms),
                    cost_usd=COALESCE(?16,cost_usd), cost_credits=COALESCE(?17,cost_credits),
                    cost_basis=COALESCE(?18,cost_basis), usage=COALESCE(?19,usage), error=COALESCE(?20,error),
                    requested_by=COALESCE(requested_by,?21), requested_match=COALESCE(requested_match,?22),
                    requested_beat=COALESCE(requested_beat,?23), requested_seat=COALESCE(requested_seat,?24),
                    requested_account=COALESCE(requested_account,?25), source=?26
                 WHERE id=?1"
            } else {
                "UPDATE generations SET status=?2, provider=COALESCE(provider,?3),
                    model=COALESCE(model,?4), operation=COALESCE(operation,?5), endpoint=COALESCE(endpoint,?6),
                    prompt=COALESCE(prompt,?7), negative_prompt=COALESCE(negative_prompt,?8), params=COALESCE(params,?9),
                    reference_asset_ids=COALESCE(reference_asset_ids,?10), provider_task_id=COALESCE(provider_task_id,?11),
                    requested_at=COALESCE(requested_at,?12), started_at=COALESCE(started_at,?13),
                    finished_at=COALESCE(finished_at,?14), duration_ms=COALESCE(duration_ms,?15),
                    cost_usd=COALESCE(cost_usd,?16), cost_credits=COALESCE(cost_credits,?17),
                    cost_basis=COALESCE(cost_basis,?18), usage=COALESCE(usage,?19), error=COALESCE(error,?20),
                    requested_by=COALESCE(requested_by,?21), requested_match=COALESCE(requested_match,?22),
                    requested_beat=COALESCE(requested_beat,?23), requested_seat=COALESCE(requested_seat,?24),
                    requested_account=COALESCE(requested_account,?25), source=COALESCE(source,?26)
                 WHERE id=?1"
            };
            c.execute(
                sql,
                params![
                    g.id, status, g.provider, g.model, g.operation, g.endpoint, g.prompt, g.negative_prompt,
                    json_text(&merged_params), refs, g.provider_task_id, g.requested_at, g.started_at, g.finished_at,
                    g.duration_ms, g.cost_usd, g.cost_credits, g.cost_basis, json_text(&g.usage), g.error,
                    g.requested_by, g.requested_match, g.requested_beat, g.requested_seat, g.requested_account,
                    g.source,
                ],
            )?;
            if status == "ok" {
                c.execute("UPDATE generations SET error=NULL WHERE id=?1 AND status='ok' AND ?2", params![g.id, g.status == "ok"])?;
            }
        }
    }
    add_output(c, &g.id, &g.asset_id, "primary")?;
    Ok(g.id.clone())
}

pub fn add_output(c: &Connection, generation: &str, asset: &str, role: &str) -> Result<()> {
    c.execute(
        "INSERT OR IGNORE INTO generation_outputs (generation_id, asset_id, output_role) VALUES (?1,?2,?3)",
        params![generation, asset, role],
    )?;
    Ok(())
}

pub fn add_lineage(c: &Connection, parent: &str, child: &str, role: &str, note: Option<&str>) -> Result<()> {
    anyhow::ensure!(schema::LINEAGE_ROLES.contains(&role), "lineage role {role:?}");
    anyhow::ensure!(parent != child, "an asset cannot be its own parent");
    c.execute(
        "INSERT OR IGNORE INTO lineage (parent_id, child_id, role, note, created_at) VALUES (?1,?2,?3,?4,?5)",
        params![parent, child, role, note, now_ms()],
    )?;
    Ok(())
}

/// Record a sighting. With `extend`, the newest row of the same asset, match, role, subject ref and loadout that
/// starts at or before this beat is extended to it (the live walker: one row per run of beats); without, only the row
/// of exactly this beat is touched (backfills). Otherwise a new row starts at this beat.
pub fn note_use(c: &Connection, u: &UseIn, extend: bool) -> Result<()> {
    let beat = u.beat.unwrap_or(-1);
    let n = c.execute(
        "UPDATE uses SET last_beat = MAX(last_beat, ?5), last_seen_at = MAX(last_seen_at, ?6), seat = COALESCE(seat, ?8)
         WHERE id = (SELECT id FROM uses WHERE asset_id=?1 AND match_id=?2 AND role=?3 AND subject_ref=?4
                       AND beat <= ?5 AND (?9 OR beat = ?5) AND loadout_hash IS ?7
                     ORDER BY beat DESC LIMIT 1)",
        params![u.asset_id, u.match_id, u.role, u.subject_ref, beat, u.at_ms, u.loadout_hash, u.seat, extend],
    )?;
    if n == 0 {
        c.execute(
            "INSERT OR IGNORE INTO uses (asset_id, match_id, beat, last_beat, role, subject_ref, subject_kind, subject_key,
                loadout_hash, seat, first_seen_at, last_seen_at, source)
             VALUES (?1,?2,?3,?3,?4,?5,?6,?7,?8,?9,?10,?10,?11)",
            params![
                u.asset_id, u.match_id, beat, u.role, u.subject_ref, u.subject_kind, u.subject_key, u.loadout_hash,
                u.seat, u.at_ms, u.source,
            ],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

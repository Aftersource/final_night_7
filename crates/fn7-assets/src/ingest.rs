//! Backfill: bring what already exists into the registry. Every step only reads its source, is idempotent, and can
//! be re-run at any time (also while the server runs: SQLite serializes the writers).
//!
//! 1. [`media_cache`]: a media cache directory (`<hash>.<ext>` + `<hash>.json` sidecars). Each image becomes an asset
//!    (its file hard-linked into the layout, so the old path keeps working and no disk is used twice) with one
//!    generation from its sidecar: the exact prompt, provider, model, when, how long, what it cost, which match.
//! 2. [`profile_library`]: the profile store's library (`library`, `library_matches`): which player's match showed
//!    which image, as `uses` rows.
//! 3. [`recordings`]: core recordings (`<match>.jsonl`): the `media_*` events and the image `http` records add what
//!    sidecars do not hold: the beat that asked, the reference images sent, the request parameters, the provider's
//!    request id and the token usage; and refusals and failures, which leave no file.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};

use crate::live::{apply, Call, MediaRecord, RefFile, What};
use crate::subject::name_key;
use crate::{asset_id_for_media, note_use, now_ms, Registry, Result, UseIn};

#[derive(Debug, Default, Clone, Serialize)]
pub struct CacheReport {
    pub sidecars: u64,
    pub ingested: u64,
    /// Sidecars whose image file is gone.
    pub missing_file: u64,
    /// Sidecars that do not parse.
    pub bad_sidecar: u64,
    /// Asset rows that did not exist before this run.
    pub new_assets: u64,
    pub errors: u64,
}

fn count(c: &Connection, sql: &str) -> Result<u64> {
    Ok(c.query_row(sql, [], |r| r.get::<_, i64>(0))? as u64)
}

/// Step 1: every image of a media cache directory.
pub fn media_cache(reg: &Registry, dir: &Path) -> Result<CacheReport> {
    let t0 = now_ms();
    let mut rep = CacheReport::default();
    let before = reg.with(|c| count(c, "SELECT COUNT(*) FROM assets"))?;
    let mut names: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    names.sort();
    let root = reg.root().to_path_buf();
    for chunk in names.chunks(200) {
        reg.tx(|c| {
            for p in chunk {
                rep.sidecars += 1;
                let side: fn7_media::Sidecar = match std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
                    Some(s) => s,
                    None => {
                        rep.bad_sidecar += 1;
                        continue;
                    }
                };
                let file = dir.join(format!("{}.{}", side.key, side.ext));
                if !file.is_file() {
                    rep.missing_file += 1;
                    continue;
                }
                let mut r = MediaRecord::new(What::Sidecar, &side.key, side.kind.as_str(), &side.provider, "sidecar");
                r.prompt = Some(side.prompt.clone());
                r.at_ms = side.created_at;
                r.file = Some(file);
                r.created_at = Some(side.created_at);
                r.cost_usd = Some(side.cost_usd);
                r.latency_ms = Some(side.latency_ms);
                r.model = Some(side.model.clone());
                r.first_match = side.match_id.clone();
                match apply(c, &root, &r) {
                    Ok(()) => rep.ingested += 1,
                    Err(e) => {
                        rep.errors += 1;
                        eprintln!("media cache: {}: {e}", p.display());
                    }
                }
            }
            Ok(())
        })?;
    }
    rep.new_assets = reg.with(|c| count(c, "SELECT COUNT(*) FROM assets"))? - before;
    reg.log_run("media-cache", &dir.display().to_string(), t0, &serde_json::to_value(&rep)?)?;
    Ok(rep)
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct LibraryReport {
    /// (profile, image) rows in the library.
    pub library_rows: u64,
    /// (profile, image, match) links read.
    pub links: u64,
    pub uses_written: u64,
    /// Distinct library hashes with no asset (the image is no longer in the media cache that was ingested).
    pub hashes_without_asset: u64,
}

/// Step 2: the profile store's library. The store is opened read-only.
pub fn profile_library(reg: &Registry, profiles_db: &Path) -> Result<LibraryReport> {
    let t0 = now_ms();
    let src = Connection::open_with_flags(profiles_db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI)?;
    let mut rep = LibraryReport::default();
    let mut st = src.prepare(
        "SELECT l.hash, l.kind, l.subject, l.subject_name, m.match_id, COALESCE(m.at, l.first_seen)
         FROM library l LEFT JOIN library_matches m ON m.profile_key = l.profile_key AND m.hash = l.hash
         ORDER BY l.hash",
    )?;
    type Row = (String, String, Option<String>, Option<String>, Option<String>, i64);
    let rows: Vec<Row> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?
        .collect::<rusqlite::Result<_>>()?;
    rep.library_rows = src.query_row("SELECT COUNT(*) FROM library", [], |r| r.get::<_, i64>(0))? as u64;
    let mut missing = std::collections::BTreeSet::new();
    reg.tx(|c| {
        let before = count(c, "SELECT COUNT(*) FROM uses")?;
        for (hash, _kind, subject, subject_name, match_id, at) in &rows {
            let Some(m) = match_id else { continue };
            rep.links += 1;
            let found: Option<(String, String, String)> = c
                .query_row("SELECT id, subject_kind, subject_key FROM assets WHERE media_hash=?1", [hash], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })
                .optional()?;
            let Some((id, skind, skey)) = found else {
                missing.insert(hash.clone());
                continue;
            };
            let sref = subject.clone().unwrap_or_default();
            let named = subject_name.as_deref().and_then(name_key).zip(crate::live::kind_of_ref(&sref));
            let (skind, skey) = match named {
                Some((k, kind)) => (kind.to_string(), k),
                None => (skind, skey),
            };
            note_use(c, &UseIn {
                asset_id: id,
                match_id: m.clone(),
                beat: None,
                role: "library".into(),
                subject_ref: sref,
                subject_kind: Some(skind),
                subject_key: Some(skey),
                loadout_hash: None,
                seat: None,
                at_ms: *at,
                source: "library".into(),
            }, false)?;
        }
        rep.uses_written = count(c, "SELECT COUNT(*) FROM uses")? - before;
        Ok(())
    })?;
    rep.hashes_without_asset = missing.len() as u64;
    reg.log_run("profile-library", &profiles_db.display().to_string(), t0, &serde_json::to_value(&rep)?)?;
    Ok(rep)
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct RecordingsReport {
    pub files: u64,
    pub bytes: u64,
    pub media_events: u64,
    pub http_records: u64,
    pub requests: u64,
    pub ready: u64,
    pub refused: u64,
    pub failed: u64,
    pub cached: u64,
    /// Finished generations that got their provider exchange (usage, request id, parameters) attached.
    pub with_http: u64,
    /// Requests with no outcome in their recording (the server stopped first, or the match is still running).
    pub no_outcome: u64,
    pub errors: u64,
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Step 3: one recording, or every `*.jsonl` (and archived `*.jsonl.zst`) of a directory. `media_dir`, when given, is where a `media_ready`
/// event's file is looked up by name (an image that is in a recording but was never in the ingested cache).
pub fn recordings(reg: &Registry, path: &Path, media_dir: Option<&Path>) -> Result<RecordingsReport> {
    let t0 = now_ms();
    let mut files: Vec<PathBuf> = if path.is_dir() {
        std::fs::read_dir(path)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "jsonl") || p.to_string_lossy().ends_with(".jsonl.zst"))
            .collect()
    } else {
        vec![path.to_path_buf()]
    };
    files.sort();
    let mut rep = RecordingsReport::default();
    let root = reg.root().to_path_buf();
    for f in &files {
        rep.files += 1;
        let mut records: Vec<MediaRecord> = vec![];
        // hash → prompt (from its request), and (prompt) → exchanges not yet claimed by an outcome.
        let mut prompts: HashMap<String, String> = HashMap::new();
        let mut calls: HashMap<String, Vec<Call>> = HashMap::new();
        let mut open: std::collections::HashSet<String> = Default::default();
        // An archived recording (`<match>.jsonl.zst`, runs/lan/recordings_tool.sh) is read through `zstd -dc`.
        let mut child = None;
        let src: Box<dyn std::io::Read> = if f.to_string_lossy().ends_with(".zst") {
            let mut c = std::process::Command::new("zstd")
                .args(["-dc", "--long=27"])
                .arg(f)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|e| anyhow::anyhow!("{}: zstd is needed to read an archived recording: {e}", f.display()))?;
            let out = c.stdout.take().expect("piped");
            child = Some(c);
            Box::new(out)
        } else {
            Box::new(std::fs::File::open(f)?)
        };
        let mut rd = BufReader::with_capacity(1 << 20, src);
        let mut line: Vec<u8> = Vec::new();
        loop {
            line.clear();
            let n = rd.read_until(b'\n', &mut line)?;
            if n == 0 {
                break;
            }
            rep.bytes += n as u64;
            // The records we want are short and say what they are near their start; a snapshot line can be megabytes.
            let head = &line[..line.len().min(if line.len() <= 1 << 16 { line.len() } else { 400 })];
            let is_event = contains(head, b"\"kind\":\"media_");
            let is_http = !is_event && contains(head, b"\"type\":\"http\"") && contains(head, b"/images/");
            if !is_event && !is_http {
                continue;
            }
            let Ok(v) = serde_json::from_slice::<Value>(&line) else { continue };
            let run = v.pointer("/ctx/run").and_then(Value::as_str).map(str::to_string);
            let round = v.pointer("/ctx/round").and_then(Value::as_u64).map(|r| r as u32);
            if is_http {
                rep.http_records += 1;
                let Some(prompt) = v.pointer("/request/prompt").and_then(Value::as_str) else { continue };
                calls.entry(prompt.to_string()).or_default().push(Call {
                    attempt: v["attempt"].as_u64().unwrap_or(0) as u32,
                    started_at: v["started_at"].as_i64().unwrap_or(0),
                    duration_ms: v["duration_ms"].as_f64().unwrap_or(0.0),
                    url: v["url"].as_str().unwrap_or_default().to_string(),
                    status: v["status"].as_u64().map(|s| s as u16),
                    request: v["request"].clone(),
                    usage: v.pointer("/response/usage").filter(|u| !u.is_null()).cloned(),
                    request_id: v.pointer("/response_headers/x-request-id").and_then(Value::as_str).map(str::to_string),
                    error: v["error"].as_str().map(str::to_string),
                });
                continue;
            }
            rep.media_events += 1;
            let d = &v["data"];
            let (Some(kind), Some(hash)) = (v["kind"].as_str(), d["hash"].as_str()) else { continue };
            let what = match kind {
                "media_request" => What::Requested,
                "media_cached" => What::Cached,
                "media_refused" => What::Refused,
                "media_ready" => What::Ready,
                "media_failed" => What::Failed,
                _ => continue,
            };
            let mut r = MediaRecord::new(what, hash, d["kind"].as_str().unwrap_or("event"), d["provider"].as_str().unwrap_or("openai"), "recording");
            r.match_id = run;
            r.beat = round;
            r.at_ms = v["at"].as_i64().unwrap_or(0);
            r.prompt = d["prompt"].as_str().map(str::to_string).or_else(|| prompts.get(hash).cloned());
            match what {
                What::Requested => {
                    rep.requests += 1;
                    r.label = d["model"].as_str().map(str::to_string);
                    r.price_usd = d["price_usd"].as_f64();
                    r.refs = d["refs"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|x| x.as_str())
                        .map(|n| RefFile { name: n.to_string(), ..Default::default() })
                        .collect();
                    if let Some(p) = &r.prompt {
                        prompts.insert(hash.to_string(), p.clone());
                    }
                    open.insert(hash.to_string());
                }
                What::Cached => rep.cached += 1,
                What::Refused => {
                    rep.refused += 1;
                    r.reason = d["reason"].as_str().map(str::to_string);
                }
                What::Ready | What::Failed => {
                    open.remove(hash);
                    if what == What::Ready { rep.ready += 1 } else { rep.failed += 1 }
                    r.reason = d["error"].as_str().map(str::to_string);
                    r.cost_usd = d["cost_usd"].as_f64();
                    r.latency_ms = d["latency_ms"].as_f64();
                    r.model = d["model"].as_str().map(str::to_string);
                    r.file = match (media_dir, d["path"].as_str().and_then(|p| Path::new(p).file_name())) {
                        (Some(dir), Some(name)) => Some(dir.join(name)),
                        _ => None,
                    };
                    if let Some(p) = &r.prompt {
                        r.calls = calls.remove(p).unwrap_or_default();
                        // The exchange names the reference files with their hashes.
                        if let Some(imgs) = r.calls.last().and_then(|c| c.request.get("image[]")).and_then(Value::as_array) {
                            r.refs = imgs
                                .iter()
                                .map(|i| RefFile {
                                    name: i["file"].as_str().unwrap_or_default().to_string(),
                                    sha256: i["sha256"].as_str().map(str::to_string),
                                    path: None,
                                    bytes: i["bytes"].as_u64(),
                                })
                                .collect();
                        }
                        if !r.calls.is_empty() && what == What::Ready {
                            rep.with_http += 1;
                        }
                        r.size = r.calls.last().and_then(|c| c.request["size"].as_str()).map(str::to_string);
                    }
                }
                What::Sidecar => {}
            }
            records.push(r);
        }
        if let Some(mut c) = child {
            let _ = c.wait();
        }
        rep.no_outcome += open.len() as u64;
        for chunk in records.chunks(500) {
            reg.tx(|c| {
                for r in chunk {
                    if let Err(e) = apply(c, &root, r) {
                        rep.errors += 1;
                        eprintln!("recording {}: {}: {e}", f.display(), r.hash);
                    }
                }
                Ok(())
            })?;
        }
    }
    reg.log_run("recordings", &path.display().to_string(), t0, &serde_json::to_value(&rep)?)?;
    Ok(rep)
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct RefsReport {
    /// Reference assets known only by hash (`external`) before this run.
    pub wanted: u64,
    pub files_seen: u64,
    pub attached: u64,
}

/// Give reference images that are known only by name and hash (from a recording) their files: every file under
/// `dir` whose sha256 a waiting reference asset expects is copied into the layout.
pub fn attach_references(reg: &Registry, dir: &Path) -> Result<RefsReport> {
    let t0 = now_ms();
    let mut rep = RefsReport::default();
    let wanted: HashMap<String, (String, String)> = reg.with(|c| {
        let mut st = c.prepare("SELECT id, display_name, meta FROM assets WHERE kind='image' AND subkind='reference' AND path IS NULL AND meta IS NOT NULL")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
        let mut m = HashMap::new();
        for (id, name, meta) in rows.flatten() {
            if let Some(sha) = serde_json::from_str::<Value>(&meta).ok().and_then(|v| v["sha256_expected"].as_str().map(str::to_string)) {
                m.insert(sha, (id, name));
            }
        }
        Ok(m)
    })?;
    rep.wanted = wanted.len() as u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for p in rd.filter_map(|e| e.ok()).map(|e| e.path()) {
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if !p.extension().is_some_and(|x| ["jpg", "jpeg", "png", "webp"].contains(&x.to_string_lossy().to_lowercase().as_str())) {
                continue;
            }
            rep.files_seen += 1;
            let Ok((sha, _)) = crate::files::sha256_file(&p) else { continue };
            let Some((id, name)) = wanted.get(&sha) else { continue };
            let file = reg.store_file(&p, "image", "reference", crate::Link::Copy)?;
            reg.upsert_asset(&crate::AssetIn {
                id: id.clone(),
                kind: "image".into(),
                subkind: "reference".into(),
                subject: Some(crate::Subject::shared(&format!("style_ref:{}", name.to_lowercase()), name)),
                variant: "style".into(),
                created_at: std::fs::metadata(&p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or(t0),
                file: Some(file),
                status: "ready".into(),
                source: "backfill".into(),
                origin_path: Some(p.display().to_string()),
                ..Default::default()
            })?;
            rep.attached += 1;
        }
    }
    reg.log_run("style-refs", &dir.display().to_string(), t0, &serde_json::to_value(&rep)?)?;
    Ok(rep)
}

/// What the registry holds, for the CLI's `stats` and the backfill report.
pub fn stats(reg: &Registry) -> Result<Value> {
    reg.with(|c| {
        let table = |sql: &str| -> Result<Vec<Value>> {
            let mut st = c.prepare(sql)?;
            let n = st.column_count();
            let names: Vec<String> = (0..n).map(|i| st.column_name(i).unwrap_or("").to_string()).collect();
            let rows = st.query_map([], |r| {
                let mut o = serde_json::Map::new();
                for (i, name) in names.iter().enumerate() {
                    let v: rusqlite::types::Value = r.get(i)?;
                    o.insert(name.clone(), crate::query::sql_json(v));
                }
                Ok(Value::Object(o))
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        };
        let one = |sql: &str| -> Result<Value> { Ok(table(sql)?.into_iter().next().unwrap_or(Value::Null)) };
        Ok(json!({
            "schema": c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
            "assets": one("SELECT COUNT(*) AS total, COALESCE(SUM(bytes),0) AS bytes, COUNT(DISTINCT sha256) AS distinct_files FROM assets")?,
            "by_kind_status": table("SELECT kind, subkind, status, COUNT(*) AS n, COALESCE(SUM(bytes),0) AS bytes FROM assets GROUP BY 1,2,3 ORDER BY 1,2,3")?,
            "by_status": table("SELECT status, COUNT(*) AS n FROM assets GROUP BY 1 ORDER BY 1")?,
            "generations": table("SELECT provider, status, COALESCE(cost_basis,'unknown') AS cost_basis, COUNT(*) AS n, ROUND(COALESCE(SUM(cost_usd),0),4) AS cost_usd, COALESCE(SUM(cost_credits),0) AS cost_credits FROM generations GROUP BY 1,2,3 ORDER BY 1,2,3")?,
            "cost": one("SELECT ROUND(COALESCE(SUM(cost_usd),0),4) AS usd, COALESCE(SUM(cost_credits),0) AS credits FROM generations")?,
            "gaps": one("SELECT
                (SELECT COUNT(*) FROM assets a WHERE NOT EXISTS (SELECT 1 FROM generation_outputs o WHERE o.asset_id = a.id)) AS assets_without_generation,
                (SELECT COUNT(*) FROM assets a WHERE a.status='ready' AND NOT EXISTS (SELECT 1 FROM generation_outputs o JOIN generations g ON g.id=o.generation_id WHERE o.asset_id = a.id AND g.prompt IS NOT NULL)) AS ready_without_prompt,
                (SELECT COUNT(*) FROM assets a WHERE a.status='ready' AND NOT EXISTS (SELECT 1 FROM generation_outputs o JOIN generations g ON g.id=o.generation_id WHERE o.asset_id = a.id AND (g.cost_usd IS NOT NULL OR g.cost_credits IS NOT NULL))) AS ready_without_cost,
                (SELECT COUNT(*) FROM generations WHERE status='ok' AND usage IS NULL AND provider='openai') AS openai_ok_without_usage,
                (SELECT COUNT(*) FROM generations WHERE status='ok' AND requested_match IS NULL) AS ok_without_match,
                (SELECT COUNT(*) FROM generations WHERE status='ok' AND requested_beat IS NULL) AS ok_without_beat,
                (SELECT COUNT(*) FROM assets a WHERE NOT EXISTS (SELECT 1 FROM uses u WHERE u.asset_id = a.id)) AS assets_without_use")?,
            "uses": one("SELECT COUNT(*) AS rows, COUNT(DISTINCT match_id) AS matches, COUNT(DISTINCT asset_id) AS assets FROM uses")?,
            "lineage": table("SELECT role, COUNT(*) AS n FROM lineage GROUP BY 1 ORDER BY 1")?,
        }))
    })
}

/// The asset id of a media hash, if registered (the documented mapping; see [`asset_id_for_media`]).
pub fn asset_of_media(reg: &Registry, hash: &str) -> Result<Option<String>> {
    let id = asset_id_for_media(hash);
    reg.with(|c| Ok(c.query_row("SELECT id FROM assets WHERE id=?1", [&id], |r| r.get(0)).optional()?))
}

//! Reads: everything the API and the CLI answer, as JSON.

use rusqlite::types::Value as Sql;
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Map, Value};

use crate::subject::name_key;
use crate::{Registry, Result};

/// An account's accepted god, as the accounts layer knows it (the server implements [`crate::http::Gods`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AccountGod {
    pub account: String,
    /// The fn7-media key of the accepted reference image.
    pub media_hash: String,
    /// `/media/<hash>.<ext>` once it is on disk.
    pub url: Option<String>,
    /// `pending` | `ready`.
    pub status: String,
    /// The registry's model asset of its ready statue, if any.
    pub statue_asset: Option<String>,
    /// `queued` | `ready` | `failed` of its newest statue, if any.
    pub statue_status: Option<String>,
    pub description: String,
    pub stone: String,
    pub pose: String,
    pub accepted_at: i64,
}

pub const MAX_LIMIT: u32 = 500;
pub const DEFAULT_LIMIT: u32 = 50;

/// Columns holding JSON text, returned parsed.
const JSON_COLUMNS: [&str; 5] = ["meta", "params", "usage", "reference_asset_ids", "report"];

pub fn sql_json(v: Sql) -> Value {
    match v {
        Sql::Null => Value::Null,
        Sql::Integer(i) => json!(i),
        Sql::Real(f) => json!(f),
        Sql::Text(t) => Value::String(t),
        Sql::Blob(b) => json!(format!("<{} bytes>", b.len())),
    }
}

/// Run a query and return each row as an object keyed by column name (JSON columns parsed).
pub fn rows(c: &Connection, sql: &str, params: &[&dyn rusqlite::ToSql]) -> Result<Vec<Value>> {
    let mut st = c.prepare(sql)?;
    let names: Vec<String> = (0..st.column_count()).map(|i| st.column_name(i).unwrap_or("").to_string()).collect();
    let out = st
        .query_map(params, |r| {
            let mut o = Map::new();
            for (i, n) in names.iter().enumerate() {
                let v = sql_json(r.get::<_, Sql>(i)?);
                let v = match (&v, JSON_COLUMNS.contains(&n.as_str())) {
                    (Value::String(s), true) => serde_json::from_str(s).unwrap_or(v),
                    _ => v,
                };
                o.insert(n.clone(), v);
            }
            Ok(Value::Object(o))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(out)
}

/// Add what a client needs to fetch the file: `file_url` (and `media_url` for a media-layer image).
fn with_urls(mut a: Value) -> Value {
    let id = a["id"].as_str().unwrap_or_default().to_string();
    let has_file = a["path"].is_string();
    let media = match (a["media_hash"].as_str(), a["path"].as_str().and_then(|p| p.rsplit('.').next())) {
        (Some(h), Some(ext)) => json!(format!("/media/{h}.{ext}")),
        _ => Value::Null,
    };
    if let Some(o) = a.as_object_mut() {
        o.insert("file_url".into(), if has_file { json!(format!("/api/assets/{id}/file")) } else { Value::Null });
        o.insert("media_url".into(), media);
    }
    a
}

#[derive(Debug, Default, Clone)]
pub struct Filter {
    pub subject_kind: Option<String>,
    /// A name, descriptor or match id; normalized the way subject keys are.
    pub subject: Option<String>,
    /// `image | model | texture | animation`, a subkind (`portrait`, `statue`, …), or `kind/subkind`.
    pub kind: Option<String>,
    pub match_id: Option<String>,
    pub beat: Option<i64>,
    pub status: Option<String>,
    pub media_hash: Option<String>,
    pub sha256: Option<String>,
    /// Only the newest ready asset per (kind, subkind, subject, variant).
    pub current: bool,
    /// Subject keys (an account's names) or an account id that asked: any of them matches.
    pub mine: Option<(Vec<String>, String)>,
    pub limit: Option<u32>,
    /// The `next` cursor of the previous page.
    pub after: Option<String>,
}

/// The key a subject filter matches: a name's normalized key, or the text as given (a match id, `style_ref:…`).
fn subject_keys(s: &str) -> Vec<String> {
    let mut v = vec![s.trim().to_string(), s.trim().to_lowercase()];
    v.extend(name_key(s));
    v.dedup();
    v
}

impl Registry {
    /// A page of assets, newest first. `next` is the cursor for the following page (`null` at the end).
    pub fn list(&self, f: &Filter) -> Result<Value> {
        let limit = f.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT) as i64;
        let mut wh: Vec<String> = vec![];
        let mut args: Vec<Sql> = vec![];
        let arg = |v: Sql, args: &mut Vec<Sql>| {
            args.push(v);
            format!("?{}", args.len())
        };
        if let Some(k) = &f.subject_kind {
            wh.push(format!("a.subject_kind = {}", arg(Sql::Text(k.clone()), &mut args)));
        }
        if let Some(s) = &f.subject {
            let ks: Vec<String> = subject_keys(s).into_iter().map(|k| arg(Sql::Text(k), &mut args)).collect();
            wh.push(format!("a.subject_key IN ({})", ks.join(",")));
        }
        if let Some(k) = &f.kind {
            match k.split_once('/') {
                Some((a, b)) => wh.push(format!("(a.kind = {} AND a.subkind = {})", arg(Sql::Text(a.into()), &mut args), arg(Sql::Text(b.into()), &mut args))),
                None if crate::schema::KINDS.contains(&k.as_str()) => wh.push(format!("a.kind = {}", arg(Sql::Text(k.clone()), &mut args))),
                None => wh.push(format!("a.subkind = {}", arg(Sql::Text(k.clone()), &mut args))),
            }
        }
        if let Some(s) = &f.status {
            wh.push(format!("a.status = {}", arg(Sql::Text(s.clone()), &mut args)));
        }
        if let Some(h) = &f.media_hash {
            wh.push(format!("a.media_hash = {}", arg(Sql::Text(h.clone()), &mut args)));
        }
        if let Some(h) = &f.sha256 {
            wh.push(format!("a.sha256 = {}", arg(Sql::Text(h.clone()), &mut args)));
        }
        if let Some(m) = &f.match_id {
            let p = arg(Sql::Text(m.clone()), &mut args);
            let beat = match f.beat {
                Some(b) => {
                    let bp = arg(Sql::Integer(b), &mut args);
                    (format!(" AND u.beat <= {bp} AND u.last_beat >= {bp}"), format!(" AND g.requested_beat = {bp}"))
                }
                None => (String::new(), String::new()),
            };
            wh.push(format!(
                "(EXISTS (SELECT 1 FROM uses u WHERE u.asset_id = a.id AND u.match_id = {p}{}) OR EXISTS (SELECT 1 FROM generations g WHERE g.asset_id = a.id AND g.requested_match = {p}{}))",
                beat.0, beat.1
            ));
        }
        if let Some((keys, account)) = &f.mine {
            let ks: Vec<String> = keys.iter().map(|k| arg(Sql::Text(k.clone()), &mut args)).collect();
            let acc = arg(Sql::Text(account.clone()), &mut args);
            let by_key = if ks.is_empty() { "0".to_string() } else { format!("a.subject_key IN ({})", ks.join(",")) };
            wh.push(format!("({by_key} OR EXISTS (SELECT 1 FROM generations g WHERE g.asset_id = a.id AND g.requested_account = {acc}))"));
        }
        if let Some((t, id)) = f.after.as_deref().and_then(|c| c.split_once('_')).and_then(|(t, id)| Some((t.parse::<i64>().ok()?, id.to_string()))) {
            let tp = arg(Sql::Integer(t), &mut args);
            let ip = arg(Sql::Text(id), &mut args);
            wh.push(format!("(a.created_at < {tp} OR (a.created_at = {tp} AND a.id < {ip}))"));
        }
        let table = if f.current { "current_assets" } else { "assets" };
        let filter = if wh.is_empty() { String::new() } else { format!("WHERE {}", wh.join(" AND ")) };
        self.with(|c| {
            let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|a| a as &dyn rusqlite::ToSql).collect();
            let total: i64 = match f.after {
                Some(_) => -1,
                None => c.query_row(&format!("SELECT COUNT(*) FROM {table} a {filter}"), refs.as_slice(), |r| r.get(0))?,
            };
            let mut items = rows(c, &format!("SELECT a.* FROM {table} a {filter} ORDER BY a.created_at DESC, a.id DESC LIMIT {}", limit + 1), refs.as_slice())?;
            let more = items.len() as i64 > limit;
            items.truncate(limit as usize);
            let next = match (more, items.last()) {
                (true, Some(l)) => json!(format!("{}_{}", l["created_at"], l["id"].as_str().unwrap_or_default())),
                _ => Value::Null,
            };
            let items: Vec<Value> = items.into_iter().map(with_urls).collect();
            let mut out = json!({ "items": items, "next": next, "limit": limit });
            if total >= 0 {
                out["total"] = json!(total);
            }
            Ok(out)
        })
    }

    /// The id an `{id}` path segment names: an asset id, or a media hash (64 hex) of a registered image.
    pub fn resolve_id(&self, id_or_hash: &str) -> Result<Option<String>> {
        self.with(|c| {
            Ok(c.query_row("SELECT id FROM assets WHERE id = ?1 OR media_hash = ?1 LIMIT 1", [id_or_hash], |r| r.get(0)).optional()?)
        })
    }

    /// One asset with everything about it: the row, its generations (exact prompts), lineage both ways, uses.
    pub fn detail(&self, id: &str) -> Result<Option<Value>> {
        self.with(|c| {
            let Some(asset) = rows(c, "SELECT * FROM assets WHERE id = ?1", &[&id])?.into_iter().next() else { return Ok(None) };
            let generations = rows(
                c,
                "SELECT g.*, o.output_role FROM generation_outputs o JOIN generations g ON g.id = o.generation_id
                 WHERE o.asset_id = ?1 ORDER BY COALESCE(g.started_at, g.requested_at, g.finished_at, g.recorded_at), g.id",
                &[&id],
            )?;
            let brief = "a.id, a.kind, a.subkind, a.variant, a.subject_kind, a.subject_key, a.display_name, a.status, a.sha256, a.path, a.bytes, a.mime, a.media_hash, a.created_at";
            let parents: Vec<Value> = rows(c, &format!("SELECT l.role, l.note, {brief} FROM lineage l JOIN assets a ON a.id = l.parent_id WHERE l.child_id = ?1 ORDER BY a.created_at, a.id"), &[&id])?
                .into_iter().map(with_urls).collect();
            let children: Vec<Value> = rows(c, &format!("SELECT l.role, l.note, {brief} FROM lineage l JOIN assets a ON a.id = l.child_id WHERE l.parent_id = ?1 ORDER BY a.created_at, a.id"), &[&id])?
                .into_iter().map(with_urls).collect();
            let uses = rows(c, "SELECT match_id, beat, last_beat, role, subject_ref, subject_kind, subject_key, loadout_hash, seat, first_seen_at, last_seen_at, source FROM uses WHERE asset_id = ?1 ORDER BY first_seen_at, id LIMIT 1000", &[&id])?;
            let uses_total: i64 = c.query_row("SELECT COUNT(*) FROM uses WHERE asset_id = ?1", [id], |r| r.get(0))?;
            let current: bool = c.query_row("SELECT EXISTS (SELECT 1 FROM current_assets WHERE id = ?1)", [id], |r| r.get(0))?;
            Ok(Some(json!({
                "asset": with_urls(asset), "current": current, "generations": generations,
                "lineage": { "parents": parents, "children": children },
                "uses": uses, "uses_total": uses_total,
            })))
        })
    }

    /// The whole chain an asset belongs to, oldest ancestor first (`portrait → heroic image → statue → preview`):
    /// every ancestor and descendant with the role of each link.
    pub fn chain(&self, id: &str) -> Result<Vec<Value>> {
        self.with(|c| {
            rows(
                c,
                "WITH RECURSIVE up(id, depth) AS (SELECT ?1, 0 UNION SELECT l.parent_id, up.depth - 1 FROM lineage l JOIN up ON l.child_id = up.id WHERE up.depth > -16),
                      down(id, depth) AS (SELECT ?1, 0 UNION SELECT l.child_id, down.depth + 1 FROM lineage l JOIN down ON l.parent_id = down.id WHERE down.depth < 16),
                      allof(id, depth) AS (SELECT id, MIN(depth) FROM up GROUP BY id UNION SELECT id, MAX(depth) FROM down WHERE depth > 0 GROUP BY id)
                 SELECT x.depth, a.id, a.kind, a.subkind, a.variant, a.display_name, a.status, a.path, a.sha256,
                        (SELECT group_concat(l.role || ':' || l.parent_id) FROM lineage l WHERE l.child_id = a.id) AS parents
                 FROM allof x JOIN assets a ON a.id = x.id ORDER BY x.depth, a.created_at, a.id",
                &[&id],
            )
        })
    }

    /// What a client needs to put a figure on a seat: the current statue and rigged figure of a demigod or god by
    /// name, and the shared mannequin (rig, clip bundle, skins) to fall back on. Without a name: every subject that
    /// has a statue or figure.
    pub fn figures(&self, name: Option<&str>, subject_kind: Option<&str>) -> Result<Value> {
        let cols = "id, kind, subkind, variant, subject_kind, subject_key, display_name, sha256, path, bytes, mime, tris, created_at, meta";
        self.with(|c| {
            let shape = |v: Vec<Value>| -> Vec<Value> { v.into_iter().map(with_urls).collect() };
            let mannequin = || -> Result<Value> {
                let rig = shape(rows(c, &format!("SELECT {cols} FROM current_assets WHERE kind='model' AND subkind='mannequin' ORDER BY variant"), &[])?);
                let clips = shape(rows(c, &format!("SELECT {cols} FROM current_assets WHERE kind='animation' AND subject_kind='shared' ORDER BY variant"), &[])?);
                let skins = shape(rows(c, &format!("SELECT {cols} FROM current_assets WHERE kind='texture' AND subkind='skin' AND subject_kind='shared' ORDER BY variant"), &[])?);
                Ok(json!({ "models": rig, "clips": clips, "skins": skins }))
            };
            match name {
                Some(n) => {
                    let key = name_key(n).unwrap_or_default();
                    let models = shape(rows(
                        c,
                        &format!("SELECT {cols} FROM current_assets WHERE kind='model' AND subkind IN ('statue','figure') AND subject_kind IN ('demigod','god') AND subject_key = ?1 ORDER BY subkind, variant"),
                        &[&key],
                    )?);
                    // A demigod and a god may share a name ("Chris"): `subject_kind` says which; without it the
                    // demigod's comes first and the god's is also given as `god_statue`.
                    let models: Vec<Value> = models.into_iter().filter(|m| subject_kind.is_none_or(|k| m["subject_kind"] == k)).collect();
                    let pick = |kind: Option<&str>, sub: &str, variant: Option<&str>| {
                        models
                            .iter()
                            .find(|m| m["subkind"] == sub && kind.is_none_or(|k| m["subject_kind"] == k) && variant.is_none_or(|v| m["variant"] == v))
                            .cloned()
                            .unwrap_or(Value::Null)
                    };
                    // The default file of each: the GLB (variant ''), else whatever there is.
                    let best = |kind: Option<&str>, sub: &str| match pick(kind, sub, Some("")) { Value::Null => pick(kind, sub, None), v => v };
                    let first = |sub: &str| match best(Some("demigod"), sub) { Value::Null => best(None, sub), v => v };
                    let (statue, figure, god_statue) = (first("statue"), first("figure"), best(Some("god"), "statue"));
                    let previews = shape(rows(c, &format!("SELECT {cols} FROM current_assets WHERE kind='image' AND subkind='preview' AND subject_key = ?1"), &[&key])?);
                    Ok(json!({
                        "name": n, "key": key, "found": !models.is_empty(),
                        "statue": statue, "figure": figure, "god_statue": god_statue, "models": models, "previews": previews,
                        "fallback": if models.is_empty() { json!("mannequin") } else { Value::Null },
                        "mannequin": mannequin()?,
                    }))
                }
                None => {
                    let all = shape(rows(c, &format!("SELECT {cols} FROM current_assets WHERE kind='model' AND subkind IN ('statue','figure') ORDER BY subject_key, subkind, variant"), &[])?);
                    Ok(json!({ "figures": all, "mannequin": mannequin()? }))
                }
            }
        })
    }

    /// The god at a seat (docs/ACCOUNTS.md § 14), for `GET /api/assets/figures?name=`: the account's accepted god
    /// portrait (and its statue when one is ready) when `account` is given, else the house's marble god of that name
    /// (the marble reference image and the statue imported from the Unreal repo's `figures.json`), else `null`.
    pub fn god_figure(&self, name: &str, account: Option<&AccountGod>) -> Result<Value> {
        let cols = "id, kind, subkind, variant, subject_kind, subject_key, display_name, sha256, path, bytes, mime, tris, created_at, media_hash, status";
        let key = name_key(name).unwrap_or_default();
        self.with(|c| {
            let one = |sql: &str, p: &[&dyn rusqlite::ToSql]| -> Result<Value> {
                Ok(rows(c, sql, p)?.into_iter().next().map(with_urls).unwrap_or(Value::Null))
            };
            if let Some(g) = account {
                let asset = one(&format!("SELECT {cols} FROM assets WHERE id = ?1"), &[&crate::asset_id_for_media(&g.media_hash)])?;
                let statue = match &g.statue_asset {
                    Some(id) => one(&format!("SELECT {cols} FROM assets WHERE id = ?1 AND status = 'ready'"), &[id])?,
                    None => Value::Null,
                };
                return Ok(json!({
                    "source": "account", "account": g.account, "status": g.status,
                    "image": { "hash": g.media_hash, "url": g.url, "status": g.status,
                               "sha256": asset["sha256"], "asset_id": asset["id"], "file_url": asset["file_url"] },
                    "statue": statue, "statue_status": g.statue_status,
                    "description": g.description, "stone": g.stone, "pose": g.pose, "accepted_at": g.accepted_at,
                }));
            }
            let image = one(
                &format!("SELECT {cols} FROM current_assets WHERE kind='image' AND subkind='reference' AND variant='heroic' AND subject_kind='god' AND subject_key = ?1"),
                &[&key],
            )?;
            let statue = one(
                &format!("SELECT {cols} FROM current_assets WHERE kind='model' AND subkind='statue' AND subject_kind='god' AND subject_key = ?1 ORDER BY variant LIMIT 1"),
                &[&key],
            )?;
            if image.is_null() && statue.is_null() {
                return Ok(Value::Null);
            }
            let url = image["file_url"].clone();
            Ok(json!({
                "source": "house", "account": null, "status": if image.is_null() { "none" } else { "ready" },
                "image": if image.is_null() { Value::Null } else {
                    json!({ "hash": null, "url": url, "status": "ready", "sha256": image["sha256"], "asset_id": image["id"], "file_url": url })
                },
                "statue": statue, "statue_status": if statue.is_null() { Value::Null } else { json!("ready") },
            }))
        })
    }

    /// Cost roll-ups over the generations: `by` = `day | kind | provider | match | subject | basis | model` (UTC days).
    /// Every row: `calls`, `ok`, `failed`, `refused`, `cost_usd`, `cost_credits`, `duration_ms` (sum),
    /// `avg_duration_ms`, `first_at`, `last_at`; `usd_measured` (priced from usage or a provider's reported number)
    /// vs `usd_estimated`.
    pub fn costs(&self, by: &str, usd_per_credit: Option<f64>) -> Result<Value> {
        let key = match by {
            "day" => "strftime('%Y-%m-%d', COALESCE(g.finished_at, g.started_at, g.requested_at, g.recorded_at) / 1000, 'unixepoch')",
            "kind" => "a.kind || '/' || a.subkind",
            "provider" => "g.provider",
            "model" => "g.provider || ':' || COALESCE(g.model, g.operation, '?')",
            "match" => "COALESCE(g.requested_match, g.requested_by, 'unknown')",
            "subject" => "a.subject_kind || ':' || a.subject_key",
            "basis" => "COALESCE(g.cost_basis, 'unknown')",
            other => anyhow::bail!("by={other}: expected day | kind | provider | match | subject | basis | model"),
        };
        let agg = "COUNT(*) AS calls,
                   SUM(g.status='ok') AS ok, SUM(g.status='failed') AS failed, SUM(g.status='refused') AS refused, SUM(g.status='pending') AS pending,
                   ROUND(COALESCE(SUM(g.cost_usd),0), 6) AS cost_usd,
                   COALESCE(SUM(g.cost_credits),0) AS cost_credits,
                   ROUND(COALESCE(SUM(CASE WHEN g.cost_basis IN ('usage','price_table','fixed_price','sidecar') THEN g.cost_usd END),0), 6) AS usd_recorded,
                   ROUND(COALESCE(SUM(CASE WHEN g.cost_basis = 'estimated' THEN g.cost_usd END),0), 6) AS usd_estimated,
                   SUM(g.status='ok' AND g.cost_usd IS NULL AND g.cost_credits IS NULL) AS ok_without_cost,
                   ROUND(COALESCE(SUM(g.duration_ms),0), 1) AS duration_ms,
                   ROUND(AVG(g.duration_ms), 1) AS avg_duration_ms,
                   MIN(COALESCE(g.started_at, g.requested_at, g.finished_at)) AS first_at,
                   MAX(COALESCE(g.finished_at, g.started_at, g.requested_at)) AS last_at";
        self.with(|c| {
            let mut items = rows(c, &format!("SELECT {key} AS key, {agg} FROM generations g JOIN assets a ON a.id = g.asset_id GROUP BY 1 ORDER BY 1"), &[])?;
            let mut total = rows(c, &format!("SELECT {agg} FROM generations g JOIN assets a ON a.id = g.asset_id"), &[])?.into_iter().next().unwrap_or(Value::Null);
            if let Some(rate) = usd_per_credit {
                for r in items.iter_mut().chain(std::iter::once(&mut total)) {
                    let cr = r["cost_credits"].as_f64().unwrap_or(0.0);
                    r["credits_usd_estimate"] = json!((cr * rate * 1e6).round() / 1e6);
                }
            }
            Ok(json!({ "by": by, "items": items, "total": total, "usd_per_credit": usd_per_credit }))
        })
    }

    /// Time travel: what was shown at `beat` of `match_id` (every image in effect at that beat), or with `demigod`
    /// (a name, or an engine ref like `dg:0`) what that demigod looked like: per role the asset whose run of beats
    /// covers the beat, else the newest one first seen at or before it, else the earliest. Without `beat`: the latest.
    pub fn at(&self, match_id: &str, beat: Option<i64>, demigod: Option<&str>) -> Result<Value> {
        let cols = "u.role, u.subject_ref, u.subject_kind, u.subject_key, u.beat AS first_beat, u.last_beat, u.loadout_hash, u.source AS use_source,
                    a.id, a.kind, a.subkind, a.variant, a.display_name, a.status, a.sha256, a.path, a.bytes, a.mime, a.width, a.height, a.media_hash, a.created_at";
        self.with(|c| {
            let b = beat.unwrap_or(i64::MAX);
            let (filter, key): (String, String) = match demigod {
                Some(d) if d.contains(':') && d.split(':').nth(1).is_some_and(|n| n.chars().all(|c| c.is_ascii_digit())) => {
                    // An engine ref: the name it had in this match, then everything under that name.
                    let k: Option<String> = c
                        .query_row("SELECT subject_key FROM uses WHERE match_id=?1 AND subject_ref=?2 AND subject_key IS NOT NULL LIMIT 1", [match_id, d], |r| r.get(0))
                        .optional()?;
                    ("AND (u.subject_ref = ?3 OR u.subject_key = ?4)".into(), k.unwrap_or_default())
                }
                Some(d) => ("AND u.subject_key = ?4 AND ?3 = ?3".into(), name_key(d).unwrap_or_default()),
                None => ("AND ?3 = ?3 AND ?4 = ?4".into(), String::new()),
            };
            let dref = demigod.unwrap_or("");
            let all = rows(
                c,
                &format!("SELECT {cols} FROM uses u JOIN assets a ON a.id = u.asset_id WHERE u.match_id = ?1 AND ?2 = ?2 {filter} ORDER BY u.beat, u.id"),
                &[&match_id, &b, &dref, &key],
            )?;
            // Per (subject, picture kind): the rows covering the beat, the newest start first; else the newest that
            // started at or before it; else (asked for a beat before anything was seen) the earliest.
            let mut slots: std::collections::BTreeMap<(String, String), Vec<&Value>> = Default::default();
            for u in &all {
                // Not what was on screen: a refusal, a picture still rendering, an announcement.
                if u["role"].as_str().is_some_and(|r| r == "refused" || r.starts_with("pending.") || r.starts_with("event.")) {
                    continue;
                }
                let subject = u["subject_key"].as_str().unwrap_or("").to_string();
                slots.entry((subject, u["subkind"].as_str().unwrap_or("").to_string())).or_default().push(u);
            }
            let mut items: Vec<Value> = vec![];
            for ((_, subkind), us) in &slots {
                let first = |u: &Value| u["first_beat"].as_i64().unwrap_or(-1);
                let last = |u: &Value| u["last_beat"].as_i64().unwrap_or(-1);
                // Identity pictures (one per subject at a time) answer with one; moment cards and items with all in effect.
                let single = demigod.is_some() || matches!(subkind.as_str(), "portrait" | "diorama" | "statue");
                let covering: Vec<&&Value> = us.iter().filter(|u| beat.is_none() || (first(u) <= b && last(u) >= b)).collect();
                let pick_one = |cands: Vec<&&Value>, how: &str| -> Option<Value> {
                    // A look (what the demigod looked like, gear and all) before its identity picture.
                    let is_look = |u: &Value| u["role"].as_str().is_some_and(|r| r.starts_with("look."));
                    let best = cands.into_iter().max_by_key(|u| (is_look(u), first(u), last(u), u["created_at"].as_i64().unwrap_or(0)))?;
                    let mut v = with_urls((**best).clone());
                    v["matched"] = json!(how);
                    Some(v)
                };
                if single {
                    let exact = pick_one(covering, if beat.is_some() { "covers_beat" } else { "latest" });
                    let before = || pick_one(us.iter().filter(|u| first(u) <= b).collect(), "latest_before");
                    let earliest = || {
                        let e = us.iter().min_by_key(|u| first(u))?;
                        let mut v = with_urls((**e).clone());
                        v["matched"] = json!("earliest_seen");
                        Some(v)
                    };
                    items.extend(exact.or_else(before).or_else(earliest));
                } else {
                    let mut seen = std::collections::HashSet::new();
                    for u in covering {
                        if seen.insert(u["id"].as_str().unwrap_or("").to_string()) {
                            let mut v = with_urls((**u).clone());
                            v["matched"] = json!("covers_beat");
                            items.push(v);
                        }
                    }
                }
            }
            // The 3D pieces for the same name (they are per identity, not per match): as they are now.
            let models = match demigod {
                Some(_) if !key.is_empty() => rows(
                    c,
                    "SELECT id, kind, subkind, variant, display_name, sha256, path, bytes, mime, tris, created_at FROM current_assets WHERE kind='model' AND subject_key = ?1 ORDER BY subkind, variant",
                    &[&key],
                )?.into_iter().map(with_urls).collect(),
                _ => vec![],
            };
            Ok(json!({ "match": match_id, "beat": beat, "demigod": demigod, "key": if key.is_empty() { Value::Null } else { json!(key) },
                       "items": items, "models": models, "uses_in_match": all.len() }))
        })
    }
}

//! Live ingestion: what the media layer does, written to the registry as it happens.
//!
//! - [`MediaRecord`]: one thing the media layer recorded about one image (requested, served from its cache, refused,
//!   ready, failed), as plain data. The live observer builds it from a [`fn7_media::MediaEvent`]; the backfills build
//!   the same record from a cache sidecar or from a recording's `media_*` events and `http` records, so all three
//!   write identical rows ([`apply`]) and re-running any of them changes nothing.
//! - [`LiveHook`]: the [`fn7_media::MediaObserver`]. It copies the event and hands it to a writer thread; the hook
//!   or queue worker that reported it never waits on SQLite or the disk.
//! - [`images_in`]: finds every image object in an outgoing payload (with the demigod or item it sits on), which the
//!   server's wrapper turns into `uses` rows.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use fn7_media::{MediaEvent, MediaObserver, Stage};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::subject::{self, Subject};
use crate::{
    add_lineage, asset_id_for_key, asset_id_for_media, generation_id_for_key, note_use, upsert_asset,
    upsert_generation, AssetIn, GenerationIn, Link, Registry, Result, UseIn,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum What {
    Requested,
    /// Served from the media cache (the file is there; nothing was generated now).
    Cached,
    Refused,
    Ready,
    Failed,
    /// A cache sidecar read by the backfill (ready; nobody asked just now).
    Sidecar,
}

/// A reference image sent with a request.
#[derive(Debug, Clone, Default)]
pub struct RefFile {
    /// The file name (`IMG_3751.jpg`).
    pub name: String,
    pub sha256: Option<String>,
    pub path: Option<PathBuf>,
    pub bytes: Option<u64>,
}

/// One provider exchange.
#[derive(Debug, Clone, Default)]
pub struct Call {
    pub attempt: u32,
    pub started_at: i64,
    pub duration_ms: f64,
    pub url: String,
    pub status: Option<u16>,
    pub request: Value,
    pub usage: Option<Value>,
    pub request_id: Option<String>,
    pub error: Option<String>,
}

/// Who asked (set by the server's plugin wrapper around each hook).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Asker {
    pub seat: Option<u8>,
    pub account: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MediaRecord {
    pub what: What,
    pub hash: String,
    /// `portrait | diorama | event | item | statue | place | army`.
    pub kind: String,
    /// The exact prompt text (`None` when the record does not carry it: a recording's cached/ready/failed event).
    pub prompt: Option<String>,
    pub size: Option<String>,
    pub refs: Vec<RefFile>,
    /// The media key of the image this one is drawn on (a gear look on its base portrait), when it has one.
    pub base: Option<String>,
    /// `mock | openai`.
    pub provider: String,
    /// The generator label (`openai:<model>:<quality>`) or the model alone.
    pub label: Option<String>,
    pub match_id: Option<String>,
    pub beat: Option<u32>,
    pub at_ms: i64,
    pub price_usd: Option<f64>,
    pub reason: Option<String>,
    /// The image file to store (the media cache's), when this record has it.
    pub file: Option<PathBuf>,
    /// The sidecar's: when it was stored, what it cost, how long the generator took, the model, who asked first.
    pub created_at: Option<i64>,
    pub cost_usd: Option<f64>,
    pub latency_ms: Option<f64>,
    pub model: Option<String>,
    pub first_match: Option<String>,
    pub calls: Vec<Call>,
    pub asker: Option<Asker>,
    /// `live | sidecar | recording`.
    pub source: &'static str,
    pub style_version: Option<String>,
    /// The style's avoid text, when the writer knows it.
    pub avoid: Option<String>,
    /// How `cost_usd` was arrived at, when the writer knows better than [`cost_basis`].
    pub cost_basis: Option<&'static str>,
}

impl MediaRecord {
    pub fn new(what: What, hash: &str, kind: &str, provider: &str, source: &'static str) -> Self {
        Self {
            what, hash: hash.into(), kind: kind.into(), prompt: None, size: None, refs: vec![], base: None, provider: provider.into(),
            label: None, match_id: None, beat: None, at_ms: 0, price_usd: None, reason: None, file: None,
            created_at: None, cost_usd: None, latency_ms: None, model: None, first_match: None, calls: vec![],
            asker: None, source, style_version: None, avoid: None, cost_basis: None,
        }
    }
}

fn real_match(m: Option<&str>) -> Option<&str> {
    m.filter(|m| !m.is_empty() && !matches!(*m, "-" | "prewarm" | "library"))
}

fn requested_by(m: Option<&str>) -> String {
    match m {
        Some("prewarm") => "prewarm".into(),
        // The people-and-houses library (sprites and buildings, asked for lazily or by the library prewarm).
        Some("library") => "library".into(),
        Some(m) if !m.is_empty() && m != "-" => format!("match:{m}"),
        _ => "unknown".into(),
    }
}

/// `openai:<model>:<quality>` → (model, quality).
fn split_label(label: Option<&str>) -> (Option<String>, Option<String>) {
    match label.map(|l| l.split(':').collect::<Vec<_>>()) {
        Some(p) if p.len() >= 3 => (Some(p[1].to_string()), Some(p[2].to_string())),
        Some(p) if p.len() == 1 && !p[0].is_empty() => (Some(p[0].to_string()), None),
        _ => (None, None),
    }
}

/// How the recorded cost of a finished image was arrived at:
/// `free` (the mock), `usage` (an edits call priced from the response's token usage: the per-image output price from
/// the price table plus the measured input tokens at the table's rates), `price_table` (a flat table price), or
/// `sidecar` (only the media cache's sidecar is left: the number the media layer wrote, by one of the rules above).
pub fn cost_basis(provider: &str, calls: &[Call], source: &str) -> &'static str {
    if provider == "mock" {
        return "free";
    }
    match calls.last() {
        Some(c) if c.url.ends_with("/images/edits") && c.usage.as_ref().is_some_and(|u| u.pointer("/input_tokens_details/image_tokens").is_some()) => "usage",
        Some(_) => "price_table",
        None if source == "sidecar" => "sidecar",
        None => "price_table",
    }
}

fn operation_of(url: &str) -> Option<String> {
    let i = url.find("/images/")?;
    Some(url[i + 1..].trim_end_matches('/').to_string())
}

/// The request's parameters without the prompt (kept in its own column) and with the reference images as recorded.
fn params_of(call: &Call) -> Value {
    let mut p = call.request.clone();
    if let Some(o) = p.as_object_mut() {
        o.remove("prompt");
    }
    p
}

/// A reference named after a media key (`base/<64 hex>.<ext>`, or `<64 hex>.<ext>` as a recorded exchange lists its
/// file) → the key: the base picture a gear look was drawn on.
fn base_key(name: &str) -> Option<&str> {
    name.strip_prefix("base/").unwrap_or(name).split('.').next().filter(|k| is_hash(k))
}

/// Make sure a reference image is an asset (`image/reference`, shared), storing its file when we have it.
fn reference_asset(c: &Connection, reg_root: &std::path::Path, r: &RefFile, at_ms: i64, source: &str) -> Result<Option<String>> {
    // A base image (`base/<media key>.<ext>` in a recorded exchange: the picture a gear look was drawn on) is the
    // asset of that key.
    if let Some(key) = base_key(&r.name) {
        let id = asset_id_for_media(key);
        let known: bool = c.query_row("SELECT 1 FROM assets WHERE id=?1", [&id], |_| Ok(true)).optional()?.unwrap_or(false);
        if known {
            return Ok(Some(id));
        }
    }
    // Bytes the registry already holds under another role (a generated picture sent back in as a reference).
    if let Some(sha) = &r.sha256 {
        let held: Option<String> = c
            .query_row("SELECT id FROM assets WHERE sha256=?1 AND subkind != 'reference' ORDER BY created_at, id LIMIT 1", [sha], |row| row.get(0))
            .optional()?;
        if held.is_some() {
            return Ok(held);
        }
    }
    // By content when known, else by name (a recording's request lists names; its http record adds the hashes).
    if let Some(sha) = &r.sha256 {
        let id = asset_id_for_key(&format!("file:{sha}:image/reference"));
        let have: Option<Option<String>> =
            c.query_row("SELECT path FROM assets WHERE id=?1", [&id], |row| row.get(0)).optional()?;
        if matches!(have, Some(Some(_))) {
            return Ok(Some(id));
        }
        let file = match &r.path {
            Some(p) if p.is_file() => Some(crate::files::store_file(reg_root, p, "image", "reference", Link::Copy)?),
            _ => None,
        };
        let status = if file.is_some() { "ready" } else { "external" };
        upsert_asset(c, &AssetIn {
            id: id.clone(),
            kind: "image".into(),
            subkind: "reference".into(),
            subject: Some(Subject::shared(&format!("style_ref:{}", r.name.to_lowercase()), &r.name)),
            variant: "style".into(),
            file,
            created_at: at_ms,
            status: status.into(),
            status_reason: (status == "external").then(|| "the reference file was not available when this was recorded".to_string()),
            source: source.into(),
            origin_path: r.path.as_ref().map(|p| p.display().to_string()),
            meta: Some(json!({ "sha256_expected": sha, "bytes_expected": r.bytes })),
            ..Default::default()
        })?;
        return Ok(Some(id));
    }
    Ok(c.query_row(
        "SELECT id FROM assets WHERE kind='image' AND subkind='reference' AND subject_key=?1 ORDER BY created_at DESC LIMIT 1",
        [format!("style_ref:{}", r.name.to_lowercase())],
        |row| row.get(0),
    )
    .optional()?)
}

/// Write one media record: the asset row (and its file), the generation rows, reference lineage, and a `uses` row
/// for the match that asked. Idempotent.
pub fn apply(c: &Connection, root: &std::path::Path, r: &MediaRecord) -> Result<()> {
    let id = asset_id_for_media(&r.hash);
    let asking = r.match_id.as_deref();
    let gen_match = match r.what {
        What::Sidecar | What::Cached => r.first_match.as_deref(),
        _ => asking,
    };
    let exists: Option<(String, String, String)> = c
        .query_row("SELECT status, subject_key, subkind FROM assets WHERE id=?1", [&id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .optional()?;
    // A record without the prompt (a recording's ready/failed/cached) can only add to an asset we already have.
    if exists.is_none() && r.prompt.is_none() {
        return Ok(());
    }
    let subj = r.prompt.as_deref().map(|p| subject::from_prompt(&r.kind, p, real_match(gen_match.or(asking))));
    let file = match (&r.file, r.what) {
        (Some(p), What::Ready | What::Cached | What::Sidecar) if p.is_file() => {
            // Under the row's own subkind when it was claimed first (a god preview: `images/god_preview/`).
            let sub = exists.as_ref().map(|e| e.2.as_str()).unwrap_or(&r.kind);
            Some(crate::files::store_file(root, p, "image", sub, Link::Hard)?)
        }
        _ => None,
    };
    let (status, reason) = match r.what {
        What::Requested => ("pending", None),
        What::Refused => ("refused", r.reason.clone()),
        What::Failed => ("failed", r.reason.clone()),
        What::Ready | What::Cached | What::Sidecar if file.is_some() => ("ready", None),
        // Ready in a recording, but the file is no longer in the cache we were given.
        What::Ready => ("external", Some("generated, but its file was not in the media cache when this was ingested".to_string())),
        What::Cached | What::Sidecar => ("pending", None),
    };
    // A request, failure or refusal whose outcome is already on record is a re-read (a backfill run again): it
    // changes nothing about the asset.
    let concluded: bool = match r.what {
        What::Requested | What::Failed | What::Refused => {
            let cycle = if r.what == What::Refused { "refused" } else { "req" };
            c.query_row(
                "SELECT 1 FROM generations WHERE id=?1 AND status != 'pending'",
                [generation_id_for_key(&format!("{cycle}:{}:{}", r.hash, gen_match.unwrap_or("-")))],
                |_| Ok(true),
            )
            .optional()?
            .unwrap_or(false)
        }
        _ => false,
    };
    // Never let a report without a file disturb a row that has one, and never downgrade on a re-read.
    let keep = concluded
        || matches!(exists.as_ref().map(|e| e.0.as_str()), Some("ready"))
        || (status == "external" && exists.as_ref().is_some_and(|e| e.0 != "pending"))
        || (status == "pending" && r.what != What::Requested && exists.is_some());
    let created_at = r.created_at.unwrap_or(r.at_ms);
    if !keep || file.is_some() {
        upsert_asset(c, &AssetIn {
            id: id.clone(),
            kind: "image".into(),
            subkind: r.kind.clone(),
            subject: subj.clone(),
            file,
            created_at,
            status: status.into(),
            status_reason: reason.clone(),
            style_version: r.style_version.clone(),
            prompt_version: Some(fn7_media::PROMPT_VERSION.to_string()),
            media_hash: Some(r.hash.clone()),
            source: match r.source { "sidecar" => "backfill".into(), s => s.into() },
            origin_path: r.file.as_ref().map(|p| p.display().to_string()),
            meta: r.size.as_ref().map(|s| json!({ "size": s })),
            ..Default::default()
        })?;
    }

    // References: assets of their own, and lineage into this image.
    let mut ref_ids: Vec<String> = vec![];
    // (The mock sends nothing: its references are only named in the parameters.)
    for rf in r.refs.iter().filter(|_| r.provider != "mock") {
        if let Some(rid) = reference_asset(c, root, rf, r.at_ms, r.source)? {
            if rid == id {
                continue;
            }
            if base_key(&rf.name).is_some() {
                add_lineage(c, &rid, &id, "variant_of", Some("drawn on this base image (the same character, with its gear)"))?;
                add_lineage(c, &rid, &id, "reference", Some("the base image, sent as the first reference"))?;
            } else {
                add_lineage(c, &rid, &id, "reference", Some("style reference sent with the request"))?;
            }
            ref_ids.push(rid);
        }
    }
    // A picture drawn on a base image (a gear look on the demigod's own portrait): the base is its first reference
    // and its parent.
    if let Some(base) = r.base.as_deref() {
        let bid = asset_id_for_media(base);
        let known: bool = c.query_row("SELECT 1 FROM assets WHERE id=?1", [&bid], |_| Ok(true)).optional()?.unwrap_or(false);
        if known && bid != id {
            add_lineage(c, &bid, &id, "variant_of", Some("drawn on this base image (the same character, with its gear)"))?;
            if r.provider != "mock" {
                add_lineage(c, &bid, &id, "reference", Some("the base image, sent as the first reference"))?;
            }
            ref_ids.insert(0, bid);
        }
    }
    let refs_known = (!r.refs.is_empty() || r.base.is_some()) && r.provider != "mock" && ref_ids.len() == r.refs.len() + r.base.is_some() as usize;

    // The generation of this request cycle: one per (image, match that had it generated).
    // A refusal is a row of its own (a server restarted mid-match may refuse what it made before, or the reverse).
    let cycle = if r.what == What::Refused { "refused" } else { "req" };
    let gid = generation_id_for_key(&format!("{cycle}:{}:{}", r.hash, gen_match.unwrap_or("-")));
    let (label_model, quality) = split_label(r.label.as_deref());
    let last = r.calls.last();
    let model = r.model.clone().or(label_model).or_else(|| (r.provider == "mock").then(|| "mock".to_string()));
    let mut params = last.map(params_of).unwrap_or_else(|| json!({}));
    if let Some(o) = params.as_object_mut() {
        if let (Some(s), false) = (&r.size, o.contains_key("size")) {
            o.insert("size".into(), json!(s));
        }
        if let (Some(q), false) = (&quality, o.contains_key("quality")) {
            o.insert("quality".into(), json!(q));
        }
        if !r.refs.is_empty() && !o.contains_key("image[]") {
            o.insert("references".into(), json!(r.refs.iter().map(|x| x.name.clone()).collect::<Vec<_>>()));
        }
        if let Some(p) = r.price_usd {
            o.insert("price_reserved_usd".into(), json!(p));
        }
        if let Some(b) = &r.base {
            o.insert("base_media_hash".into(), json!(b));
        }
    }
    let params = params.as_object().is_some_and(|o| !o.is_empty()).then_some(params);
    let finished = match r.what {
        What::Ready | What::Sidecar | What::Cached => r.created_at.or(Some(r.at_ms)),
        What::Failed => Some(r.at_ms),
        _ => None,
    };
    let duration = last.map(|c| c.duration_ms).or(r.latency_ms);
    let started = last.map(|c| c.started_at).or_else(|| match (finished, r.latency_ms) {
        (Some(f), Some(l)) => Some(f - l.round() as i64),
        _ => None,
    });
    let outcome = match r.what {
        What::Requested => "pending",
        What::Refused => "refused",
        What::Failed => "failed",
        _ => "ok",
    };
    let has_cost = matches!(r.what, What::Ready | What::Sidecar | What::Cached) && r.cost_usd.is_some();
    let g = GenerationIn {
        id: gid.clone(),
        asset_id: id.clone(),
        provider: r.provider.clone(),
        model,
        operation: last.and_then(|c| operation_of(&c.url)).or_else(|| match (r.provider.as_str(), r.what) {
            ("mock", _) => Some("mock".to_string()),
            (_, What::Refused) => None,
            _ if !r.refs.is_empty() => Some("images/edits".to_string()),
            _ => None,
        }),
        endpoint: last.map(|c| c.url.clone()),
        prompt: r.prompt.clone(),
        negative_prompt: r.prompt.as_deref().and_then(|p| subject::avoid_tail(p, r.avoid.as_deref())),
        params,
        reference_asset_ids: refs_known.then_some(ref_ids),
        provider_task_id: last.and_then(|c| c.request_id.clone()),
        requested_at: matches!(r.what, What::Requested | What::Refused).then_some(r.at_ms),
        started_at: if outcome == "ok" || outcome == "failed" { started } else { None },
        finished_at: finished,
        duration_ms: if outcome == "ok" || outcome == "failed" { duration } else { None },
        cost_usd: match r.what {
            What::Refused | What::Failed => Some(0.0),
            _ if has_cost => r.cost_usd,
            _ => None,
        },
        cost_credits: None,
        cost_basis: match r.what {
            What::Refused | What::Failed => Some("free".to_string()),
            _ if has_cost => Some(r.cost_basis.unwrap_or_else(|| cost_basis(&r.provider, &r.calls, r.source)).to_string()),
            _ => None,
        },
        usage: last.and_then(|c| c.usage.clone()),
        status: outcome.into(),
        error: match r.what {
            What::Refused | What::Failed => r.reason.clone(),
            _ => None,
        },
        requested_by: Some(requested_by(gen_match)),
        requested_match: real_match(gen_match).map(str::to_string),
        requested_beat: match r.what {
            What::Sidecar | What::Cached => None,
            _ => r.beat.map(|b| b as i64),
        },
        requested_seat: r.asker.as_ref().and_then(|a| a.seat).map(|s| s as i64),
        requested_account: r.asker.as_ref().and_then(|a| a.account.clone()),
        source: r.source.into(),
    };
    // A cache hit says nothing new about a generation we already hold; it only fills a missing one.
    let known_ok: bool = c
        .query_row("SELECT 1 FROM generations WHERE asset_id=?1 AND status='ok' LIMIT 1", [&id], |_| Ok(true))
        .optional()?
        .unwrap_or(false);
    let skip = match r.what {
        // A cache hit read from a recording carries no sidecar: nothing to say about the generation.
        What::Cached | What::Sidecar if r.created_at.is_none() => true,
        What::Cached | What::Sidecar => known_ok && r.calls.is_empty() && {
            // …unless it is that very generation (a re-run of the same backfill): then merging is a no-op anyway.
            let same: Option<i64> = c.query_row("SELECT 1 FROM generations WHERE id=?1", [&gid], |row| row.get(0)).optional()?;
            same.is_none()
        },
        _ => false,
    };
    if !skip {
        upsert_generation(c, &g)?;
        // The same generation may already be here from a sidecar that did not know its match: that row is this one.
        let stray = generation_id_for_key(&format!("req:{}:-", r.hash));
        if r.what == What::Ready && stray != gid {
            let is_stray: bool = c
                .query_row("SELECT 1 FROM generations WHERE id=?1 AND status='ok' AND source='sidecar'", [&stray], |_| Ok(true))
                .optional()?
                .unwrap_or(false);
            if is_stray {
                c.execute("DELETE FROM generation_outputs WHERE generation_id=?1", [&stray])?;
                c.execute("DELETE FROM generations WHERE id=?1", [&stray])?;
            }
        }
    }
    // Earlier exchanges of the same generation (a refused transparent background, a 429): rows of their own.
    if r.calls.len() > 1 {
        for call in &r.calls[..r.calls.len() - 1] {
            upsert_generation(c, &GenerationIn {
                id: generation_id_for_key(&format!("http:{}:{}:{}", r.hash, call.started_at, call.attempt)),
                asset_id: id.clone(),
                provider: r.provider.clone(),
                model: g.model.clone(),
                operation: operation_of(&call.url),
                endpoint: Some(call.url.clone()),
                prompt: r.prompt.clone().or_else(|| call.request.get("prompt").and_then(Value::as_str).map(str::to_string)),
                params: Some(params_of(call)),
                provider_task_id: call.request_id.clone(),
                started_at: Some(call.started_at),
                finished_at: Some(call.started_at + call.duration_ms.round() as i64),
                duration_ms: Some(call.duration_ms),
                cost_usd: Some(0.0),
                cost_basis: Some("free".into()),
                usage: call.usage.clone(),
                status: if call.error.is_some() || call.status.is_some_and(|s| s >= 400) { "failed".into() } else { "ok".into() },
                error: call.error.clone(),
                requested_by: Some(requested_by(gen_match)),
                requested_match: real_match(gen_match).map(str::to_string),
                requested_beat: r.beat.map(|b| b as i64),
                source: r.source.into(),
                ..Default::default()
            })?;
        }
    }
    // The match that asked saw it at this beat.
    if let (Some(m), true) = (real_match(asking), matches!(r.what, What::Requested | What::Cached | What::Refused)) {
        let s = match subj {
            Some(s) => Some((s.kind.to_string(), s.key)),
            None => c
                .query_row("SELECT subject_kind, subject_key FROM assets WHERE id=?1", [&id], |row| Ok((row.get(0)?, row.get(1)?)))
                .optional()?,
        };
        note_use(c, &UseIn {
            asset_id: id,
            match_id: m.to_string(),
            beat: r.beat.map(|b| b as i64),
            role: match r.what { What::Cached => "cached".into(), What::Refused => "refused".into(), _ => "requested".into() },
            subject_ref: String::new(),
            subject_kind: s.as_ref().map(|s| s.0.clone()),
            subject_key: s.map(|s| s.1),
            loadout_hash: None,
            seat: r.asker.as_ref().and_then(|a| a.seat).map(|s| s as i64),
            at_ms: r.at_ms,
            source: if r.source == "sidecar" { "live".into() } else { r.source.into() },
        }, false)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// Images in payloads
// ---------------------------------------------------------------------------------------------------------------------

/// An image object found in a payload, with what it sits on.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Seen {
    pub hash: String,
    /// The key it was under: `portrait_image`, `diorama_image`, `statue_image`, `image`, …
    pub role: String,
    /// The owner object's `ref` (`dg:0`, `hd:1`, `it:4`), if any.
    pub subject_ref: String,
    /// The owner object's `name`, if any.
    pub name: Option<String>,
    /// The gear the picture shows (`loadout_hash` on the image object or its owner), if the payload says.
    pub loadout_hash: Option<String>,
}

fn is_hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Every image object (`{hash, kind, status, …}` under a key ending in `image`) in `v` that is or will be a picture
/// (`ready` or `pending`), with what it sits on.
///
/// - **Whose it is**: the nearest object above it with a `ref` (or a `demigod` ref) and a `name`.
/// - **The role** is the key, prefixed by where its owner sits: `portrait_image` (on a demigod: its identity
///   picture), `look.portrait_image` (in a demigod's `look`: what it looks like NOW, gear and all),
///   `pending.portrait_image` (in a look's `pending`: asked for, not shown yet), `event.portrait_image` (in an event
///   object such as `look.changed`: an announcement).
/// - **The loadout**: the image's own `loadout_hash`, else its owner's `image_loadout_hash` / `loadout_hash` /
///   `gear_hash`; none when the owner has a `pending` picture (what is shown then is a stand-in for the new gear).
pub fn images_in(v: &Value) -> Vec<Seen> {
    #[derive(Clone, Default)]
    struct Above {
        r: Option<String>,
        name: Option<String>,
    }
    fn text(o: &serde_json::Map<String, Value>, k: &str) -> Option<String> {
        o.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
    }
    fn is_ref(s: &str) -> bool {
        s.split_once(':').is_some_and(|(a, b)| a.len() <= 3 && !a.is_empty() && !b.is_empty() && b.bytes().all(|c| c.is_ascii_digit()))
    }
    fn walk(v: &Value, key: &str, above: &Above, out: &mut Vec<Seen>, depth: u32) {
        if depth > 24 {
            return;
        }
        match v {
            Value::Array(a) => a.iter().for_each(|x| walk(x, key, above, out, depth + 1)),
            Value::Object(o) => {
                let here = Above {
                    r: text(o, "ref").filter(|r| is_ref(r)).or_else(|| text(o, "demigod").filter(|r| is_ref(r))).or_else(|| above.r.clone()),
                    name: text(o, "name").or_else(|| above.name.clone()),
                };
                let prefix = match key {
                    "look" | "pending" => key,
                    _ if o.get("event").is_some_and(Value::is_string) => "event",
                    _ => "",
                };
                for (k, x) in o {
                    let img = x.as_object().filter(|_| k == "image" || k.ends_with("_image"));
                    let hash = img.and_then(|i| i.get("hash")).and_then(Value::as_str).filter(|h| is_hash(h));
                    match (img, hash) {
                        (Some(i), Some(h)) => {
                            if matches!(i.get("status").and_then(Value::as_str), Some("ready" | "pending")) {
                                let owners = if o.contains_key("pending") {
                                    None
                                } else {
                                    text(o, "image_loadout_hash").or_else(|| text(o, "loadout_hash")).or_else(|| text(o, "gear_hash"))
                                };
                                out.push(Seen {
                                    hash: h.to_string(),
                                    role: if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") },
                                    subject_ref: here.r.clone().unwrap_or_default(),
                                    name: here.name.clone(),
                                    loadout_hash: text(i, "loadout_hash").or(owners),
                                });
                            }
                        }
                        _ => walk(x, k, &here, out, depth + 1),
                    }
                }
            }
            _ => {}
        }
    }
    let mut out = vec![];
    walk(v, "", &Above::default(), &mut out, 0);
    out
}

/// The subject kind an engine ref names.
pub fn kind_of_ref(r: &str) -> Option<&'static str> {
    match r.split(':').next()? {
        "dg" | "hd" => Some("demigod"),
        "it" => Some("item"),
        "ch" => Some("person"),
        "pl" => Some("place"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The observer and its writer thread
// ---------------------------------------------------------------------------------------------------------------------

enum Msg {
    Media(Box<MediaRecord>),
    Seen { match_id: String, beat: u32, seat: Option<u8>, at_ms: i64, images: Vec<Seen> },
    Flush(mpsc::SyncSender<()>),
}

tokio::task_local! {
    /// Who is asking, for the hook running in this task ([`LiveHook::asking`], [`LiveHook::asking_sync`]).
    static ASKER: Asker;
}

/// The live writer. Clone-free: share it behind an `Arc`.
pub struct LiveHook {
    tx: Mutex<mpsc::Sender<Msg>>,
    style_version: Option<String>,
    avoid: Option<String>,
    fixed_price: bool,
    /// (match → (beat, what was already reported this beat)): keeps repeated views of one beat off the channel.
    seen: Mutex<HashMap<String, (u32, std::collections::HashSet<Seen>)>>,
}

impl LiveHook {
    /// Start the writer thread on `registry`. `style_version` and `avoid` come from the media style in use;
    /// `fixed_price` says `FN7_IMAGE_PRICE_USD` overrides the price table (then costs are that number).
    pub fn start(registry: Arc<Registry>, style_version: Option<String>, avoid: Option<String>, fixed_price: bool) -> Arc<Self> {
        let (tx, rx) = mpsc::channel::<Msg>();
        std::thread::Builder::new()
            .name("fn7-assets".into())
            .spawn(move || writer(registry, rx))
            .expect("the asset writer thread starts");
        Arc::new(Self { tx: Mutex::new(tx), style_version, avoid, fixed_price, seen: Mutex::new(HashMap::new()) })
    }

    /// For a media service: its style's version and avoid text, and whether its price is fixed.
    pub fn for_media(registry: Arc<Registry>, media: &fn7_media::Media) -> Arc<Self> {
        let style = media.style();
        let fixed = media.config().openai.as_ref().is_some_and(|o| o.price_override.is_some());
        Self::start(registry, Some(style.version.clone()), Some(style.avoid.clone()).filter(|a| !a.is_empty()), fixed)
    }

    fn send(&self, m: Msg) {
        let _ = self.tx.lock().unwrap_or_else(|e| e.into_inner()).send(m);
    }

    /// Run an async hook with its asker known to the observer.
    pub async fn asking<F: std::future::Future>(asker: Asker, fut: F) -> F::Output {
        ASKER.scope(asker, fut).await
    }

    /// Run a sync hook with its asker known to the observer.
    pub fn asking_sync<R>(asker: Asker, f: impl FnOnce() -> R) -> R {
        ASKER.sync_scope(asker, f)
    }

    /// The images of an outgoing payload were shown at `beat` of `match_id` (to `seat`, or to everyone).
    pub fn saw(&self, match_id: &str, beat: u32, seat: Option<u8>, payload: &Value) {
        let mut images = images_in(payload);
        if images.is_empty() {
            return;
        }
        {
            let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
            if seen.len() > 512 {
                seen.clear();
            }
            let e = seen.entry(match_id.to_string()).or_insert_with(|| (beat, Default::default()));
            if e.0 != beat {
                *e = (beat, Default::default());
            }
            images.retain(|i| e.1.insert(i.clone()));
        }
        if !images.is_empty() {
            self.send(Msg::Seen { match_id: match_id.to_string(), beat, seat, at_ms: crate::now_ms(), images });
        }
    }

    /// Wait until everything handed over so far is written (tests, shutdown).
    pub fn flush(&self) {
        let (tx, rx) = mpsc::sync_channel(1);
        self.send(Msg::Flush(tx));
        let _ = rx.recv_timeout(std::time::Duration::from_secs(30));
    }
}

impl MediaObserver for LiveHook {
    fn on_media(&self, ev: &MediaEvent<'_>) {
        let what = match ev.stage {
            Stage::Requested => What::Requested,
            Stage::Cached => What::Cached,
            Stage::Refused => What::Refused,
            Stage::Ready => What::Ready,
            Stage::Failed => What::Failed,
        };
        let mut r = MediaRecord::new(what, ev.hash, ev.prompt.kind.as_str(), ev.provider, "live");
        r.prompt = Some(ev.prompt.text.clone());
        r.size = Some(ev.prompt.size.clone());
        r.refs = ev
            .prompt
            .refs
            .iter()
            .map(|x| RefFile { name: x.file_name(), sha256: Some(x.sha256.clone()), path: Some(x.path.clone()), bytes: None })
            .collect();
        r.base = ev.prompt.base.clone();
        r.label = Some(ev.label.to_string());
        r.match_id = ev.match_id.map(str::to_string);
        r.beat = ev.round;
        r.at_ms = ev.at_ms;
        r.price_usd = Some(ev.price_usd);
        r.reason = ev.reason.map(str::to_string);
        r.file = ev.path.map(|p| p.to_path_buf());
        r.latency_ms = ev.latency_ms;
        if let Some(s) = ev.sidecar {
            r.created_at = Some(s.created_at);
            r.cost_usd = Some(s.cost_usd);
            r.latency_ms = r.latency_ms.or(Some(s.latency_ms));
            r.model = Some(s.model.clone());
            r.first_match = s.match_id.clone();
            if what == What::Cached {
                // The sidecar is the record of the generation; what made it then is its provider, not today's.
                r.provider = s.provider.clone();
                r.label = None;
                r.source = "sidecar";
            }
        }
        r.calls = ev
            .calls
            .iter()
            .map(|c| Call {
                attempt: c.attempt, started_at: c.started_at, duration_ms: c.duration_ms, url: c.url.clone(),
                status: c.status, request: c.request.clone(), usage: c.usage.clone(), request_id: c.request_id.clone(),
                error: c.error.clone(),
            })
            .collect();
        r.asker = ASKER.try_with(|a| a.clone()).ok();
        r.style_version = self.style_version.clone();
        r.avoid = self.avoid.clone();
        if self.fixed_price && what == What::Ready && ev.provider != "mock" {
            r.cost_basis = Some("fixed_price");
        }
        self.send(Msg::Media(Box::new(r)));
    }
}

fn writer(reg: Arc<Registry>, rx: mpsc::Receiver<Msg>) {
    // (match, role, subject ref or the asset itself) → the asset last seen there: a different one starts a new row.
    let mut current: HashMap<(String, String, String), String> = HashMap::new();
    while let Ok(m) = rx.recv() {
        match m {
            Msg::Media(r) => {
                let root = reg.root().to_path_buf();
                if let Err(e) = reg.tx(|c| apply(c, &root, &r)) {
                    tracing::warn!(hash = %r.hash, error = %e, "assets: could not record a media event");
                }
            }
            Msg::Seen { match_id, beat, seat, at_ms, images } => {
                let res = reg.tx(|c| {
                    for i in &images {
                        let Some((id, kind, key)): Option<(String, String, String)> = c
                            .query_row("SELECT id, subject_kind, subject_key FROM assets WHERE media_hash=?1", [&i.hash], |r| {
                                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                            })
                            .optional()?
                        else {
                            continue;
                        };
                        // What the payload says it shows; fills an asset whose prompt named nobody.
                        let named = i.name.as_deref().and_then(|n| {
                            let k = kind_of_ref(&i.subject_ref)?;
                            let s = Subject::named(k, n);
                            (s.key != "unknown").then_some(s)
                        });
                        if let (Some(s), true) = (&named, key == "unknown") {
                            c.execute(
                                "UPDATE assets SET subject_kind=?2, subject_key=?3, display_name=?4, updated_at=?5 WHERE id=?1",
                                params![id, s.kind, s.key, s.name, at_ms],
                            )?;
                        }
                        let (skind, skey) = match &named {
                            Some(s) => (s.kind.to_string(), s.key.clone()),
                            None => (kind, key),
                        };
                        let slot = (match_id.clone(), i.role.clone(), if i.subject_ref.is_empty() { id.clone() } else { i.subject_ref.clone() });
                        let extend = current.get(&slot) == Some(&id);
                        note_use(c, &UseIn {
                            asset_id: id.clone(),
                            match_id: match_id.clone(),
                            beat: Some(beat as i64),
                            role: i.role.clone(),
                            subject_ref: i.subject_ref.clone(),
                            subject_kind: Some(skind),
                            subject_key: Some(skey),
                            loadout_hash: i.loadout_hash.clone(),
                            seat: seat.map(|s| s as i64),
                            at_ms,
                            source: "live".into(),
                        }, extend)?;
                        if current.len() > 20_000 {
                            current.clear();
                        }
                        current.insert(slot, id);
                    }
                    Ok(())
                });
                if let Err(e) = res {
                    tracing::warn!(match_id = %match_id, error = %e, "assets: could not record uses");
                }
            }
            Msg::Flush(done) => {
                let _ = done.send(());
            }
        }
    }
}

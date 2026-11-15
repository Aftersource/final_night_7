//! External assets: the 3D work done with Meshy (and the reference images made for it) outside the server.
//!
//! - [`import_provenance`]: reads a Meshy production directory as the Unreal repo keeps it
//!   (`production_assets/meshy/`): `figures/figures.json`, `figures/<slug>/provenance.json`,
//!   `mannequin/provenance.json`. Registers every step as assets, generations (exact prompts, task ids, credits),
//!   outputs and lineage, and copies the files into the layout. Where the Meshy CLI's project folders are still there
//!   (`<dir>/<date>_<name>_<id>/metadata.json`, `task_<id>.json`; git-ignored in the Unreal repo), each task's
//!   measured times and consumed credits are read from them.
//! - [`import_jobs`]: reads a JSON-lines **job record** file (the format future tool runs append to; docs/ASSETS.md).
//!
//! Both are idempotent: ids derive from the task id (or the file's hash and role), files are content-addressed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::OptionalExtension;
use serde::Serialize;
use serde_json::{json, Value};

use crate::subject::{name_key, Subject};
use crate::{
    add_lineage, add_output, asset_id_for_key, asset_id_for_media, generation_id_for_key, now_ms, upsert_asset,
    upsert_generation, AssetIn, GenerationIn, Link, Registry, Result, Stored,
};

#[derive(Debug, Default, Clone, Serialize)]
pub struct ImportReport {
    pub provenance_files: u64,
    pub jobs: u64,
    pub assets: u64,
    pub generations: u64,
    pub files_stored: u64,
    pub bytes_stored: u64,
    /// Files a record names that are not on disk (the asset is registered as `external`).
    pub missing_files: Vec<String>,
    /// Files whose sha256 differs from the one recorded.
    pub hash_mismatches: Vec<String>,
    /// Tasks whose times and credits were read from the Meshy CLI's task files.
    pub tasks_with_measured_times: u64,
    pub credits: f64,
    pub notes: Vec<String>,
}

/// What the Meshy CLI's project folders know about each task.
#[derive(Default)]
struct Tasks {
    /// task id → its `task_<id>.json`.
    by_id: HashMap<String, Value>,
    /// stage name (`anim-idle_guard`, `retexture-gold`) → task id.
    by_stage: HashMap<String, String>,
    /// task id → API endpoint.
    endpoint: HashMap<String, String>,
}

fn load_tasks(dir: &Path) -> Tasks {
    let mut t = Tasks::default();
    let Ok(rd) = std::fs::read_dir(dir) else { return t };
    for sub in rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()) {
        let Some(meta) = read_json(&sub.join("metadata.json")) else { continue };
        for task in meta["tasks"].as_array().into_iter().flatten() {
            let (Some(id), Some(stage)) = (task["task_id"].as_str(), task["stage"].as_str()) else { continue };
            if stage != "delivered" {
                t.by_stage.insert(stage.to_string(), id.to_string());
            }
            if let Some(e) = task["endpoint"].as_str() {
                t.endpoint.insert(id.to_string(), e.to_string());
            }
            if !t.by_id.contains_key(id) {
                if let Some(j) = read_json(&sub.join(format!("task_{id}.json"))) {
                    t.by_id.insert(id.to_string(), j);
                }
            }
        }
    }
    t
}

fn read_json(p: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(p).ok()?).ok()
}

/// `2026-11-14` or an RFC 3339 instant → unix ms (UTC; a bare date is its midnight). No date crate: civil arithmetic.
pub fn parse_time(s: &str) -> Option<i64> {
    let s = s.trim();
    let num = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, m, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    if s.as_bytes().get(4) != Some(&b'-') || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let (yy, mm) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = yy.div_euclid(400);
    let yoe = yy.rem_euclid(400);
    let doy = (153 * mm + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let mut ms = days * 86_400_000;
    if s.len() >= 19 && matches!(s.as_bytes()[10], b'T' | b' ') {
        ms += num(11, 13)? * 3_600_000 + num(14, 16)? * 60_000 + num(17, 19)? * 1000;
        if s.as_bytes().get(19) == Some(&b'.') {
            let frac: String = s[20..].chars().take_while(|c| c.is_ascii_digit()).take(3).collect();
            ms += format!("{frac:0<3}").parse::<i64>().unwrap_or(0);
        }
    }
    Some(ms)
}

struct Ctx<'a> {
    reg: &'a Registry,
    c: &'a rusqlite::Connection,
    /// The Meshy production directory: paths in the records are relative to it.
    base: PathBuf,
    tasks: Tasks,
    rep: ImportReport,
    source: &'static str,
    /// "No time known" for an asset: replaced by its file's modification time.
    now: i64,
}

/// The subject a provenance file or a job names.
fn subject_of(kind: &str, name: &str, slug: &str) -> Subject {
    match kind {
        "demigod" => Subject::named("demigod", name),
        "god" => Subject::named("god", name),
        "item" => Subject::named("item", name),
        "place" => Subject::named("place", name),
        _ => Subject::shared(slug, name),
    }
}

impl Ctx<'_> {
    /// Store a file the record names (copying it in) and check it against the recorded hash.
    fn store(&mut self, rel: &str, kind: &str, subkind: &str, expect_sha: Option<&str>) -> Option<Stored> {
        let p = self.base.join(rel);
        if !p.is_file() {
            if !self.rep.missing_files.iter().any(|m| m == rel) {
                self.rep.missing_files.push(rel.to_string());
            }
            return None;
        }
        match self.reg.store_file(&p, kind, subkind, Link::Copy) {
            Ok(s) => {
                if expect_sha.is_some_and(|e| e != s.sha256) {
                    self.rep.hash_mismatches.push(rel.to_string());
                }
                self.rep.files_stored += 1;
                self.rep.bytes_stored += s.bytes;
                Some(s)
            }
            Err(e) => {
                self.rep.notes.push(format!("{rel}: {e}"));
                None
            }
        }
    }

    /// Register one file as an asset. Its id comes from the bytes and the role (or, without a file, from `fallback`).
    #[allow(clippy::too_many_arguments)]
    fn asset(
        &mut self,
        kind: &str,
        subkind: &str,
        subject: &Subject,
        variant: &str,
        rel: Option<&str>,
        expect_sha: Option<&str>,
        fallback: &str,
        created_at: i64,
        meta: Value,
    ) -> Result<String> {
        let file = rel.and_then(|r| self.store(r, kind, subkind, expect_sha));
        // No time on record: the file's own (so a re-import, or an import elsewhere, says the same).
        let created_at = match (created_at == self.now, rel) {
            (true, Some(r)) => std::fs::metadata(self.base.join(r)).and_then(|m| m.modified()).ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or(created_at),
            _ => created_at,
        };
        let id = match &file {
            Some(f) => asset_id_for_key(&format!("file:{}:{kind}/{subkind}:{}:{}:{variant}", f.sha256, subject.kind, subject.key)),
            None => asset_id_for_key(&format!("nofile:{fallback}")),
        };
        let status = if file.is_some() { "ready" } else { "external" };
        upsert_asset(self.c, &AssetIn {
            id: id.clone(),
            kind: kind.into(),
            subkind: subkind.into(),
            subject: Some(subject.clone()),
            subject_strong: true,
            variant: variant.into(),
            file,
            created_at,
            status: status.into(),
            status_reason: (status == "external").then(|| match rel {
                Some(r) => format!("recorded, but {r} is not kept"),
                None => "an intermediate result that was never downloaded".to_string(),
            }),
            source: self.source.into(),
            origin_path: rel.map(str::to_string),
            meta: Some(meta).filter(|m| !m.is_null()),
            ..Default::default()
        })?;
        self.rep.assets += 1;
        Ok(id)
    }

    /// A generation for a Meshy task (or a call without a task id), with measured times when the task file is there.
    fn generation(&mut self, key: &str, asset: &str, mut g: GenerationIn) -> Result<String> {
        g.id = generation_id_for_key(key);
        g.asset_id = asset.to_string();
        g.source = self.source.into();
        g.status = if g.status.is_empty() { "ok".into() } else { g.status };
        g.requested_by.get_or_insert_with(|| "operator".into());
        if let Some(t) = g.provider_task_id.as_deref().and_then(|id| self.tasks.by_id.get(id)) {
            g.requested_at = g.requested_at.or(t["created_at"].as_i64());
            g.started_at = t["started_at"].as_i64().or(g.started_at);
            g.finished_at = t["finished_at"].as_i64().or(g.finished_at);
            if let (Some(a), Some(b)) = (g.started_at, g.finished_at) {
                g.duration_ms = Some((b - a) as f64);
            }
            if let Some(cr) = t["consumed_credits"].as_f64() {
                g.cost_credits = Some(cr);
                g.cost_basis = Some("credits_reported".into());
            }
            if let Some(seed) = t["seed"].as_u64() {
                if let Some(o) = g.params.get_or_insert_with(|| json!({})).as_object_mut() {
                    o.insert("seed".into(), json!(seed));
                }
            }
            self.rep.tasks_with_measured_times += 1;
        }
        if let Some(e) = g.provider_task_id.as_deref().and_then(|id| self.tasks.endpoint.get(id)) {
            g.endpoint.get_or_insert_with(|| e.clone());
        }
        if g.cost_credits.is_some() && g.cost_basis.is_none() {
            g.cost_basis = Some("credits_recorded".into());
        }
        self.rep.credits += g.cost_credits.unwrap_or(0.0);
        self.rep.generations += 1;
        upsert_generation(self.c, &g)
    }
}

/// `"… about $0.05 by the repo's price estimate"` → 0.05.
fn dollars_in(text: &str) -> Option<f64> {
    let i = text.find('$')?;
    let n: String = text[i + 1..].chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    n.trim_end_matches('.').parse().ok()
}

/// A number written as a number or as text (`30`, `"30"`).
fn num(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

/// What an OpenAI image call cost by the media layer's own rule: the per-image price of the repo's table plus the
/// usage's input tokens at the table's rates. `None` when the table does not know the model, size or quality.
fn openai_cost(model: &str, size: &str, quality: &str, usage: &Value) -> Option<f64> {
    let image = fn7_media::generator::price_per_image(model, size, quality)?;
    // (text, image) USD per input token.
    let (txt_rate, img_rate) = fn7_media::generator::input_rates(model)?;
    let img = usage.pointer("/input_tokens_details/image_tokens")?.as_f64()?;
    let txt = usage.pointer("/input_tokens_details/text_tokens").and_then(Value::as_f64).unwrap_or(0.0);
    Some(image + img * img_rate + txt * txt_rate)
}

fn file_of(step: &Value) -> Vec<(String, Option<String>)> {
    let one = |f: &Value| Some((f["path"].as_str()?.to_string(), f["sha256"].as_str().map(str::to_string)));
    let mut v: Vec<_> = step["file"].as_object().and_then(|_| one(&step["file"])).into_iter().collect();
    v.extend(step["files"].as_array().into_iter().flatten().filter_map(one));
    v
}

/// One figure's `provenance.json` (a demigod or god: portrait → heroic image → statue, and the A-pose image).
fn import_figure(x: &mut Ctx<'_>, prov: &Value, index: Option<&Value>, media_portraits_only: bool) -> Result<()> {
    let slug = prov["slug"].as_str().unwrap_or("figure");
    let name = prov["name"].as_str().unwrap_or(slug);
    let subject = subject_of(prov["kind"].as_str().unwrap_or("demigod"), name, slug);
    // path (as the record names it) → asset id, to resolve `reference_images` and `input`.
    let mut by_path: HashMap<String, String> = HashMap::new();
    let mut last_image: Option<String> = None;
    for step in prov["lineage"].as_array().into_iter().flatten() {
        let what = step["asset"].as_str().unwrap_or("").to_lowercase();
        let made = step["made_at"].as_str().and_then(parse_time);
        let provider = step["provider"].as_str().unwrap_or("").to_lowercase();
        let files = file_of(step);
        let label = format!("{slug}:{}", step["step"]);
        if let Some(mh) = step["media_hash"].as_str() {
            // The portrait the media layer made: the same asset as the media cache's, found by its media hash.
            let Some((rel, sha)) = files.first().cloned() else { continue };
            let id = asset_id_for_media(mh);
            let have: Option<Option<String>> = x.c.query_row("SELECT sha256 FROM assets WHERE id=?1", [&id], |r| r.get(0)).optional()?;
            let file = if matches!(have, Some(Some(_))) { None } else { x.store(&rel, "image", "portrait", sha.as_deref()) };
            if have.is_none() || file.is_some() {
                upsert_asset(x.c, &AssetIn {
                    id: id.clone(),
                    kind: "image".into(),
                    subkind: "portrait".into(),
                    subject: Some(subject.clone()),
                    status: if file.is_some() { "ready".into() } else { "external".into() },
                    file,
                    created_at: made.unwrap_or(x.now),
                    media_hash: Some(mh.to_string()),
                    source: x.source.into(),
                    origin_path: Some(rel.clone()),
                    meta: Some(json!({ "note": step["provider"], "from": step["source"] })),
                    ..Default::default()
                })?;
                x.rep.assets += 1;
            }
            by_path.insert(rel.clone(), id);
            last_image = Some(rel);
            continue;
        }
        if media_portraits_only {
            continue;
        }
        let mut inputs: Vec<String> = step["reference_images"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
            .chain(step["input"].as_str())
            .map(str::to_string)
            .collect();
        // A model step that names no input was made from the index's source image, else from the image before it.
        if inputs.is_empty() && provider.contains("meshy") {
            let from_index = index.and_then(|i| i["source_image"].as_str()).map(|p| format!("figures/{p}")).filter(|p| by_path.contains_key(p));
            inputs.extend(from_index.or_else(|| last_image.clone()));
        }
        let input_ids: Vec<String> = inputs.iter().filter_map(|p| by_path.get(p).cloned()).collect();
        if provider.is_empty() && step["prompt"].is_null() {
            // A picture taken as it is (a likeness: someone's portrait): the asset that already has these bytes, else
            // a reference image with no generation of its own.
            let Some((rel, sha)) = files.first().cloned() else { continue };
            let actual = crate::files::sha256_file(&x.base.join(&rel)).ok().map(|s| s.0);
            let held: Option<String> = match &actual {
                Some(a) => x.c.query_row("SELECT id FROM assets WHERE sha256=?1 ORDER BY (media_hash IS NULL), created_at, id LIMIT 1", [a], |r| r.get(0)).optional()?,
                None => None,
            };
            let id = match held {
                Some(id) => id,
                None => x.asset("image", "reference", &subject, "likeness", Some(&rel), sha.as_deref(), &rel, made.unwrap_or(x.now), json!({ "slug": slug, "note": step["asset"] }))?,
            };
            by_path.insert(rel.clone(), id);
            last_image = Some(rel);
            continue;
        }
        if provider.contains("meshy") {
            // A model task: the model file is the asset; a preview and other formats are further outputs.
            let subkind = if what.contains("statue") { "statue" } else { "figure" };
            let task = step["task_id"].as_str();
            let (model, others): (Vec<_>, Vec<_>) = files.iter().cloned().partition(|(p, _)| !p.ends_with(".png"));
            let primary = model.first().cloned();
            let created = task.and_then(|t| x.tasks.by_id.get(t)).and_then(|t| t["finished_at"].as_i64()).or(made).unwrap_or_else(now_ms);
            let meta = json!({ "slug": slug, "cli": step["cli"], "note": step["asset"], "fbx": step["fbx"] });
            let id = x.asset("model", subkind, &subject, "", primary.as_ref().map(|p| p.0.as_str()),
                             primary.as_ref().and_then(|p| p.1.as_deref()), &format!("task:{}", task.unwrap_or(&label)), created, meta)?;
            let gid = x.generation(&format!("task:meshy:{}", task.unwrap_or(&label)), &id, GenerationIn {
                provider: "meshy".into(),
                operation: step["operation"].as_str().map(str::to_string),
                params: Some(step["params"].clone()).filter(|p| !p.is_null()),
                reference_asset_ids: Some(input_ids.clone()),
                provider_task_id: task.map(str::to_string),
                finished_at: made,
                cost_credits: num(&step["credits"]),
                ..Default::default()
            })?;
            for p in &input_ids {
                add_lineage(x.c, p, &id, "source", Some("the image the model was made from"))?;
            }
            for (rel, sha) in others.iter().chain(model.iter().skip(1)) {
                let (k, sk, role, variant) = if rel.ends_with(".png") { ("image", "preview", "preview", "") } else { ("model", subkind, "variant", "alt") };
                let oid = x.asset(k, sk, &subject, variant, Some(rel), sha.as_deref(), rel, created, json!({ "slug": slug }))?;
                add_output(x.c, &gid, &oid, role)?;
                add_lineage(x.c, &id, &oid, if role == "preview" { "preview_of" } else { "variant_of" }, None)?;
            }
            // The index may name a second format that exists on disk (the FBX is usually not committed).
            if let Some(fbx) = index.and_then(|i| i["statue_fbx"].as_str()).map(|p| format!("figures/{p}")) {
                if x.base.join(&fbx).is_file() && !files.iter().any(|(p, _)| *p == fbx) {
                    let oid = x.asset("model", subkind, &subject, "fbx", Some(&fbx), None, &fbx, created, json!({ "slug": slug }))?;
                    add_output(x.c, &gid, &oid, "fbx")?;
                    add_lineage(x.c, &id, &oid, "variant_of", Some("the same model as FBX"))?;
                }
            }
            if let Some((rel, _)) = primary {
                by_path.insert(rel, id);
            }
        } else {
            // A reference image made for the 3D step (heroic pose, A-pose).
            let variant = if what.contains("a-pose") || what.contains("apose") { "apose" } else if what.contains("heroic") || what.contains("statue") { "heroic" } else { "reference" };
            let Some((rel, sha)) = files.first().cloned() else { continue };
            let meta = json!({ "slug": slug, "size": step["size"], "note": step["asset"] });
            let id = x.asset("image", "reference", &subject, variant, Some(&rel), sha.as_deref(), &rel, made.unwrap_or(x.now), meta)?;
            let cost_text = step["cost"].as_str();
            let usage = Some(step["usage"].clone()).filter(|u| u.is_object());
            // The response's usage, when the file kept it: priced by the media layer's own rule.
            let from_usage = usage.as_ref().and_then(|u| {
                openai_cost(step["model"].as_str()?, step["size"].as_str()?, step["quality"].as_str()?, u)
            });
            let (cost_usd, basis) = match (from_usage, cost_text.and_then(dollars_in)) {
                (Some(c), _) => (Some((c * 1e6).round() / 1e6), "usage"),
                (None, Some(c)) => (Some(c), "estimated"),
                (None, None) => (None, "unknown"),
            };
            x.generation(&format!("prov:{label}:{variant}"), &id, GenerationIn {
                provider: if provider.contains("openai") { "openai".into() } else { provider.clone() },
                model: step["model"].as_str().map(str::to_string),
                operation: step["endpoint"].as_str().and_then(|e| e.split("/v1/").nth(1)).map(str::to_string),
                endpoint: step["endpoint"].as_str().map(str::to_string),
                prompt: step["prompt"].as_str().map(str::to_string),
                params: Some(json!({ "size": step["size"], "quality": step["quality"], "reference_images": inputs })),
                reference_asset_ids: Some(input_ids.clone()),
                finished_at: made,
                cost_usd,
                cost_basis: Some(basis.to_string()),
                usage: usage.or_else(|| cost_text.map(|t| json!({ "note": t }))),
                ..Default::default()
            })?;
            for p in &input_ids {
                add_lineage(x.c, p, &id, "reference", Some("identity reference sent with the request"))?;
            }
            by_path.insert(rel.clone(), id);
            last_image = Some(rel);
        }
    }
    Ok(())
}

/// The mannequin's `provenance.json`: mesh → wood texture → rig → four skins → 35 clips.
fn import_mannequin(x: &mut Ctx<'_>, prov: &Value, dir_rel: &str) -> Result<()> {
    let slug = prov["slug"].as_str().unwrap_or("mannequin");
    let subject = Subject::shared(slug, prov["name"].as_str().unwrap_or(slug));
    let t0 = x.now;
    let finished = |x: &Ctx<'_>, task: Option<&str>| task.and_then(|t| x.tasks.by_id.get(t)).and_then(|t| t["finished_at"].as_i64()).unwrap_or(t0);
    let (mut mesh, mut rig): (Option<String>, Option<String>) = (None, None);
    let mut bundle: Option<(String, Option<String>)> = None;
    let mut rig_gen: Option<String> = None;
    // One skin: its maps and preview from `<dir>/skins/<name>/`, one generation, retexture lineage from the mesh.
    #[allow(clippy::too_many_arguments)]
    fn skin(x: &mut Ctx<'_>, subject: &Subject, dir_rel: &str, name: &str, task: Option<&str>, prompt: Option<&str>, op: &str, params: Value,
            credits: Option<f64>, mesh: Option<&str>, created: i64) -> Result<()> {
        let sdir = format!("{dir_rel}/skins/{name}");
        let mut maps: Vec<String> = std::fs::read_dir(x.base.join(&sdir))
            .map(|rd| rd.filter_map(|e| e.ok()).filter_map(|e| e.file_name().into_string().ok()).collect())
            .unwrap_or_default();
        maps.sort();
        let map_of = |f: &str| f.trim_end_matches(".png").trim_start_matches("texture_0_").to_string();
        let mut primary: Option<String> = None;
        let mut outs: Vec<(String, String)> = vec![];
        for f in maps.iter().filter(|f| f.ends_with(".png")) {
            let rel = format!("{sdir}/{f}");
            let m = map_of(f);
            let id = if m == "preview" {
                x.asset("image", "preview", subject, name, Some(&rel), None, &rel, created, json!({ "skin": name }))?
            } else {
                x.asset("texture", "skin", subject, &format!("{name}/{m}"), Some(&rel), None, &rel, created, json!({ "skin": name, "map": m }))?
            };
            if m == "base_color" {
                primary = Some(id.clone());
            }
            outs.push((id, m));
        }
        let primary = match primary.or_else(|| outs.first().map(|o| o.0.clone())) {
            Some(p) => p,
            None => x.asset("texture", "skin", subject, &format!("{name}/base_color"), None, None, &format!("skin:{name}"), created, json!({ "skin": name }))?,
        };
        let gid = x.generation(&format!("task:meshy:{}", task.unwrap_or(&format!("skin:{name}"))), &primary, GenerationIn {
            provider: "meshy".into(),
            operation: Some(op.to_string()),
            prompt: prompt.map(str::to_string),
            params: Some(params).filter(|p| !p.is_null()),
            reference_asset_ids: mesh.map(|m| vec![m.to_string()]),
            provider_task_id: task.map(str::to_string),
            cost_credits: credits,
            ..Default::default()
        })?;
        for (id, m) in &outs {
            add_output(x.c, &gid, id, m)?;
            if let Some(mesh) = mesh {
                add_lineage(x.c, mesh, id, if m == "preview" { "preview_of" } else { "retexture_of" }, Some(name))?;
            }
        }
        Ok(())
    }
    for step in prov["lineage"].as_array().into_iter().flatten() {
        let op = step["operation"].as_str().unwrap_or("");
        let task = step["task_id"].as_str();
        let what = step["asset"].as_str().unwrap_or("").to_lowercase();
        if let Some(skins) = step["skins"].as_object() {
            for (name, s) in skins {
                let t = s["task_id"].as_str();
                let created = finished(x, t);
                skin(x, &subject, dir_rel, name, t, s["prompt"].as_str(), op, step["params"].clone(), num(&step["credits_each"]), mesh.as_deref(), created)?;
            }
        } else if let Some(clips) = step["clip_action_ids"].as_object() {
            // The bundle GLB every clip lives in: this step's file, else the one an earlier clips step named (later
            // clips are added to the same file).
            let first = bundle.is_none();
            let (bundle_id, bundle_rel) = match (step["file"]["path"].as_str(), &bundle) {
                (None, Some(b)) => b.clone(),
                (rel, _) => {
                    let id = x.asset("animation", "clip", &subject, "all_clips", rel, step["file"]["sha256"].as_str(), "mannequin:all_clips", t0,
                                     json!({ "bundle": true, "note": step["asset"] }))?;
                    if let Some(r) = &rig {
                        add_lineage(x.c, r, &id, "clip_of", Some("every clip in one GLB"))?;
                    }
                    (id, rel.map(str::to_string))
                }
            };
            bundle = Some((bundle_id.clone(), bundle_rel.clone()));
            let bundle_rel = bundle_rel.as_deref();
            let each = num(&step["credits"]).map(|c| c / clips.len().max(1) as f64);
            let mut names: Vec<(String, Option<u64>, bool)> = clips.iter().map(|(n, a)| (n.clone(), a.as_u64(), false)).collect();
            // The two that came with the rig (with the first set of clips).
            for n in ["walk", "run"] {
                if first && !clips.contains_key(n) {
                    names.push((n.to_string(), None, true));
                }
            }
            for (name, action, bundled) in names {
                let fbx = format!("{dir_rel}/anims/{name}.fbx");
                let own = x.base.join(&fbx).is_file();
                let t = x.tasks.by_stage.get(&format!("anim-{name}")).cloned();
                let created = finished(x, t.as_deref());
                let meta = json!({ "clip": name, "action_id": action, "in_bundle": bundle_rel, "came_with_the_rig": bundled });
                let id = if own {
                    x.asset("animation", "clip", &subject, &name, Some(&fbx), None, &fbx, created, meta)?
                } else {
                    // No file of its own: the clip lives in the bundle, so it points at the bundle's file.
                    let id = asset_id_for_key(&format!("clip:{slug}:{name}"));
                    let file: Option<Stored> = x.c.query_row(
                        "SELECT sha256, path, bytes, mime FROM assets WHERE id=?1 AND path IS NOT NULL", [&bundle_id],
                        |r| Ok(Stored { sha256: r.get(0)?, path: r.get(1)?, bytes: r.get::<_, i64>(2)? as u64, mime: "model/gltf-binary", width: None, height: None, tris: None }),
                    ).optional()?;
                    upsert_asset(x.c, &AssetIn {
                        id: id.clone(), kind: "animation".into(), subkind: "clip".into(), subject: Some(subject.clone()), subject_strong: true,
                        variant: name.clone(), status: if file.is_some() { "ready".into() } else { "external".into() }, file,
                        created_at: created, source: x.source.into(), origin_path: bundle_rel.map(str::to_string), meta: Some(meta), ..Default::default()
                    })?;
                    x.rep.assets += 1;
                    id
                };
                add_lineage(x.c, &bundle_id, &id, "variant_of", Some("a clip of the bundle")).ok();
                if let Some(r) = &rig {
                    add_lineage(x.c, r, &id, "clip_of", None)?;
                }
                if bundled {
                    if let Some(g) = &rig_gen {
                        add_output(x.c, g, &id, "member")?;
                    }
                } else {
                    x.generation(&format!("task:meshy:{}", t.clone().unwrap_or_else(|| format!("clip:{slug}:{name}"))), &id, GenerationIn {
                        provider: "meshy".into(),
                        operation: Some("animate".into()),
                        params: Some(json!({ "action_id": action })),
                        reference_asset_ids: rig.clone().map(|r| vec![r]),
                        provider_task_id: t,
                        cost_credits: each,
                        ..Default::default()
                    })?;
                }
            }
        } else if op.contains("rigging") || what == "rig" {
            let rel = step["file"]["path"].as_str();
            let id = x.asset("model", "mannequin", &subject, "rigged", rel, step["file"]["sha256"].as_str(), "mannequin:rig", finished(x, task),
                             json!({ "note": step["asset"] }))?;
            let gid = x.generation(&format!("task:meshy:{}", task.unwrap_or("mannequin:rig")), &id, GenerationIn {
                provider: "meshy".into(),
                operation: Some(op.to_string()),
                params: Some(step["params"].clone()).filter(|p| !p.is_null()),
                reference_asset_ids: mesh.clone().map(|m| vec![m]),
                provider_task_id: task.map(str::to_string),
                cost_credits: num(&step["credits"]),
                ..Default::default()
            })?;
            if let Some(m) = &mesh {
                add_lineage(x.c, m, &id, "rig_of", None)?;
            }
            rig = Some(id);
            rig_gen = Some(gid);
        } else if step["texture_prompt"].is_string() {
            // The first texture (text-to-3d refine): the skin named by the step ("wood texture …").
            let name = what.split_whitespace().next().unwrap_or("base").to_string();
            let created = finished(x, task);
            skin(x, &subject, dir_rel, &name, task, step["texture_prompt"].as_str(), op, step["params"].clone(), step["credits"].as_f64(), mesh.as_deref(), created)?;
        } else {
            // The untextured mesh (text-to-3d preview): recorded, its file never kept.
            let id = x.asset("model", "mannequin", &subject, "mesh", step["file"]["path"].as_str(), step["file"]["sha256"].as_str(), "mannequin:mesh",
                             finished(x, task), json!({ "note": step["asset"] }))?;
            x.generation(&format!("task:meshy:{}", task.unwrap_or("mannequin:mesh")), &id, GenerationIn {
                provider: "meshy".into(),
                operation: Some(op.to_string()),
                prompt: step["prompt"].as_str().map(str::to_string),
                params: Some(step["params"].clone()).filter(|p| !p.is_null()),
                provider_task_id: task.map(str::to_string),
                cost_credits: num(&step["credits"]),
                ..Default::default()
            })?;
            mesh = Some(id);
        }
    }
    if let Some(total) = prov["total_credits"].as_f64() {
        x.rep.notes.push(format!("{slug}: provenance total_credits = {total}"));
    }
    Ok(())
}

/// Import a Meshy production directory (see the module doc). `dir` is only read.
pub fn import_provenance(reg: &Registry, dir: &Path) -> Result<ImportReport> {
    let t0 = now_ms();
    anyhow::ensure!(dir.is_dir(), "{} is not a directory", dir.display());
    let tasks = load_tasks(dir);
    let index = read_json(&dir.join("figures").join("figures.json")).unwrap_or(Value::Null);
    let mut found: Vec<(PathBuf, String)> = vec![];
    for sub in ["figures", "."] {
        let Ok(rd) = std::fs::read_dir(dir.join(sub)) else { continue };
        let mut dirs: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.join("provenance.json").is_file()).collect();
        dirs.sort();
        for d in dirs {
            let rel = d.strip_prefix(dir).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
            found.push((d.join("provenance.json"), rel.trim_start_matches("./").to_string()));
        }
    }
    anyhow::ensure!(!found.is_empty(), "no provenance.json under {} (expected figures/<slug>/provenance.json, mannequin/provenance.json)", dir.display());
    let rep = reg.tx(|c| {
        let mut x = Ctx { reg, c, base: dir.to_path_buf(), tasks, rep: ImportReport::default(), source: "provenance", now: now_ms() };
        // First the portraits the media layer made (found by their media hash), in every file: a later file may
        // use one as a likeness, and must find the same asset whatever order the files come in.
        for (file, _) in &found {
            if let Some(prov) = read_json(file) {
                let slug = prov["slug"].as_str().unwrap_or("");
                if prov["lineage"].as_array().is_some_and(|l| l.iter().any(|s| s["media_hash"].is_string())) {
                    let before = x.rep.assets;
                    import_figure(&mut x, &prov, index.pointer(&format!("/figures/{slug}")), true)?;
                    x.rep.assets = before;
                }
            }
        }
        for (file, rel) in &found {
            let Some(prov) = read_json(file) else {
                x.rep.notes.push(format!("{}: not JSON", file.display()));
                continue;
            };
            x.rep.provenance_files += 1;
            let slug = prov["slug"].as_str().unwrap_or("");
            let is_shared = prov["kind"].as_str().is_some_and(|k| k.contains("shared")) || prov["lineage"].as_array().is_some_and(|l| l.iter().any(|s| s["skins"].is_object() || s["clip_action_ids"].is_object()));
            if is_shared {
                import_mannequin(&mut x, &prov, rel)?;
            } else {
                import_figure(&mut x, &prov, index.pointer(&format!("/figures/{slug}")), false)?;
            }
        }
        // Figures the index lists without a provenance file: the statue alone, from the index line.
        for (slug, f) in index["figures"].as_object().into_iter().flatten() {
            if found.iter().any(|(_, rel)| rel.ends_with(&format!("/{slug}")) || rel == slug) {
                continue;
            }
            let subject = subject_of(f["kind"].as_str().unwrap_or("demigod"), f["name"].as_str().unwrap_or(slug), slug);
            let task = f["meshy_task"].as_str();
            let glb = f["statue_glb"].as_str().map(|p| format!("figures/{p}"));
            let id = x.asset("model", "statue", &subject, "", glb.as_deref(), None, &format!("task:{}", task.unwrap_or(slug)), x.now, json!({ "slug": slug, "from": "figures.json" }))?;
            let credits = f["result"].as_str().and_then(|r| r.split_whitespace().nth(1)).and_then(|n| n.parse::<f64>().ok());
            x.generation(&format!("task:meshy:{}", task.unwrap_or(slug)), &id, GenerationIn {
                provider: "meshy".into(), operation: Some("image-to-3d".into()), provider_task_id: task.map(str::to_string), cost_credits: credits, ..Default::default()
            })?;
        }
        Ok(x.rep)
    })?;
    reg.log_run("provenance", &dir.display().to_string(), t0, &serde_json::to_value(&rep)?)?;
    Ok(rep)
}

/// Import a JSON-lines job record file (docs/ASSETS.md, "Job records"). Paths in it are relative to `root` (default:
/// the file's directory). One line = one provider call:
///
/// ```json
/// {"v":1,"provider":"meshy","operation":"image-to-3d","task_id":"01a1…","status":"SUCCEEDED","credits":30,
///  "requested_at":1794698221145,"started_at":1794698221157,"finished_at":1794698319490,
///  "prompt":null,"negative_prompt":null,"model":null,"endpoint":"/openapi/v1/image-to-3d",
///  "params":{"model_type":"standard","texture_resolution":"2k"},
///  "subject":{"kind":"demigod","name":"Chris's Left Hand","slug":"chris_left_hand"},
///  "asset":{"kind":"model","subkind":"statue","variant":""},
///  "inputs":[{"path":"figures/chris_left_hand/heroic.png","role":"source"}],
///  "outputs":[{"path":"figures/chris_left_hand/chris_left_hand_statue.glb","role":"primary"},
///             {"path":"figures/chris_left_hand/preview.png","role":"preview"}],
///  "requested_by":"operator","tool":"tools/meshy/make_statue.sh"}
/// ```
pub fn import_jobs(reg: &Registry, file: &Path, root: Option<&Path>) -> Result<ImportReport> {
    let t0 = now_ms();
    let text = std::fs::read_to_string(file)?;
    let base = root.map(Path::to_path_buf).or_else(|| file.parent().map(Path::to_path_buf)).unwrap_or_default();
    let rep = reg.tx(|c| {
        let mut x = Ctx { reg, c, base: base.clone(), tasks: Tasks::default(), rep: ImportReport::default(), source: "job", now: now_ms() };
        for (n, line) in text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#')) {
            let j: Value = match serde_json::from_str(line) {
                Ok(j) => j,
                Err(e) => {
                    x.rep.notes.push(format!("line {}: {e}", n + 1));
                    continue;
                }
            };
            if let Err(e) = import_job(&mut x, &j, n + 1) {
                x.rep.notes.push(format!("line {}: {e}", n + 1));
            }
        }
        Ok(x.rep)
    })?;
    reg.log_run("jobs", &file.display().to_string(), t0, &serde_json::to_value(&rep)?)?;
    Ok(rep)
}

fn time_of(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_str().and_then(parse_time))
}

fn import_job(x: &mut Ctx<'_>, j: &Value, line: usize) -> Result<()> {
    let provider = j["provider"].as_str().ok_or_else(|| anyhow::anyhow!("no provider"))?.to_lowercase();
    let op = j["operation"].as_str().ok_or_else(|| anyhow::anyhow!("no operation"))?;
    let task = j["task_id"].as_str().filter(|t| !t.is_empty());
    let s = &j["subject"];
    let slug = s["slug"].as_str().unwrap_or("");
    let subject = subject_of(s["kind"].as_str().unwrap_or("shared"), s["name"].as_str().unwrap_or(slug), if slug.is_empty() { "unknown" } else { slug });
    let kind = j["asset"]["kind"].as_str().unwrap_or("model");
    let subkind = j["asset"]["subkind"].as_str().unwrap_or("figure");
    let variant = j["asset"]["variant"].as_str().unwrap_or("");
    let finished = time_of(&j["finished_at"]).or_else(|| time_of(&j["at"]));
    let created = finished.unwrap_or(x.now);
    let ok = matches!(j["status"].as_str().map(str::to_uppercase).as_deref(), None | Some("SUCCEEDED" | "OK" | "READY"));
    x.rep.jobs += 1;
    // Inputs: an asset we already hold with the same bytes, else registered as a reference image.
    let mut inputs: Vec<(String, String)> = vec![];
    for i in j["inputs"].as_array().into_iter().flatten() {
        let Some(rel) = i["path"].as_str() else { continue };
        let role = i["role"].as_str().unwrap_or("source").to_string();
        let p = x.base.join(rel);
        let known: Option<String> = match crate::files::sha256_file(&p) {
            Ok((sha, _)) => x.c.query_row("SELECT id FROM assets WHERE sha256=?1 ORDER BY created_at LIMIT 1", [&sha], |r| r.get(0)).optional()?,
            Err(_) => None,
        };
        let id = match known {
            Some(id) => id,
            None => x.asset("image", "reference", &subject, i["variant"].as_str().unwrap_or("input"), Some(rel), None, rel, created, json!({ "job_line": line }))?,
        };
        inputs.push((id, role));
    }
    let outs: Vec<&Value> = j["outputs"].as_array().into_iter().flatten().collect();
    let primary = outs.iter().find(|o| o["role"].as_str().unwrap_or("primary") == "primary").copied();
    let fallback = format!("task:{}", task.map(str::to_string).unwrap_or_else(|| format!("job:{provider}:{op}:{}:{variant}:{}", subject.key, created)));
    let meta = json!({ "tool": j["tool"], "slug": s["slug"], "note": j["note"] });
    let id = match primary {
        Some(o) if ok => x.asset(kind, subkind, &subject, variant, o["path"].as_str(), o["sha256"].as_str(), &fallback, created, meta)?,
        _ => {
            // No primary output (a failed job, or a result that was never downloaded): the asset is the record of it.
            let id = asset_id_for_key(&format!("nofile:{fallback}"));
            upsert_asset(x.c, &AssetIn {
                id: id.clone(), kind: kind.into(), subkind: subkind.into(), subject: Some(subject.clone()), subject_strong: true, variant: variant.into(),
                created_at: created, status: if ok { "external".into() } else { "failed".into() },
                status_reason: j["error"].as_str().map(str::to_string).or_else(|| ok.then(|| "the job's result was not downloaded".to_string())),
                source: "job".into(), meta: Some(meta), ..Default::default()
            })?;
            x.rep.assets += 1;
            id
        }
    };
    let started = time_of(&j["started_at"]);
    let gid = x.generation(&format!("task:{provider}:{}", task.map(str::to_string).unwrap_or_else(|| fallback.clone())), &id, GenerationIn {
        provider,
        model: j["model"].as_str().map(str::to_string),
        operation: Some(op.to_string()),
        endpoint: j["endpoint"].as_str().map(str::to_string),
        prompt: j["prompt"].as_str().map(str::to_string),
        negative_prompt: j["negative_prompt"].as_str().map(str::to_string),
        params: Some(j["params"].clone()).filter(|p| !p.is_null()),
        reference_asset_ids: Some(inputs.iter().map(|i| i.0.clone()).collect()),
        provider_task_id: task.map(str::to_string),
        requested_at: time_of(&j["requested_at"]).or_else(|| time_of(&j["at"])),
        started_at: started,
        finished_at: finished,
        duration_ms: match (started, finished) { (Some(a), Some(b)) => Some((b - a) as f64), _ => j["duration_ms"].as_f64() },
        cost_usd: j["cost_usd"].as_f64(),
        cost_credits: j["credits"].as_f64(),
        cost_basis: j["cost_basis"].as_str().map(str::to_string).or_else(|| j["credits"].as_f64().map(|_| "credits_reported".to_string())),
        usage: Some(j["usage"].clone()).filter(|u| !u.is_null()),
        status: if ok { "ok".into() } else { "failed".into() },
        error: j["error"].as_str().map(str::to_string),
        requested_by: Some(j["requested_by"].as_str().unwrap_or("operator").to_string()),
        requested_match: j["match"].as_str().map(str::to_string),
        ..Default::default()
    })?;
    for (pid, role) in &inputs {
        let role = if crate::schema::LINEAGE_ROLES.contains(&role.as_str()) { role.as_str() } else { "source" };
        add_lineage(x.c, pid, &id, role, None).ok();
    }
    // What the child is to its parent, when the job says (a rig of a model, a retexture, a clip).
    if let Some(rel) = j["lineage"].as_array() {
        for l in rel {
            let (Some(role), Some(parent)) = (l["role"].as_str(), l["parent_task_id"].as_str()) else { continue };
            let pid: Option<String> = x.c.query_row("SELECT asset_id FROM generations WHERE provider_task_id=?1 LIMIT 1", [parent], |r| r.get(0)).optional()?;
            if let Some(pid) = pid {
                add_lineage(x.c, &pid, &id, role, None).ok();
            }
        }
    }
    if ok {
        for o in outs.iter().filter(|o| primary.map(|p| !std::ptr::eq(p, **o)).unwrap_or(true)) {
            let Some(rel) = o["path"].as_str() else { continue };
            let role = o["role"].as_str().unwrap_or("output");
            let (k, sk, v) = if role == "preview" { ("image", "preview", variant.to_string()) } else {
                (o["kind"].as_str().unwrap_or(kind), o["subkind"].as_str().unwrap_or(subkind), o["variant"].as_str().unwrap_or(role).to_string())
            };
            if !x.base.join(rel).is_file() && o["optional"].as_bool().unwrap_or(false) {
                continue;
            }
            let oid = x.asset(k, sk, &subject, &v, Some(rel), o["sha256"].as_str(), rel, created, json!({ "job_line": line }))?;
            add_output(x.c, &gid, &oid, role)?;
            add_lineage(x.c, &id, &oid, if role == "preview" { "preview_of" } else { "variant_of" }, None).ok();
        }
    }
    let _ = name_key;
    Ok(())
}

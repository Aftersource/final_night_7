//! Registry tests: schema and migrations, idempotent ingestion, byte-exact prompts, lineage chains, cost roll-ups,
//! the at-beat lookup, the live observer (mock generator only: nothing here calls a provider), recordings,
//! provenance and job imports, and the HTTP routes.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use fn7_media::{ImageGenerator, ImageKind, Media, MediaCache, MediaConfig, MockGenerator, Mode, Sidecar};
use http_body_util::BodyExt;
use rusqlite::Connection;
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::http::{HttpConfig, Me, Sessions};
use crate::live::LiveHook;
use crate::query::Filter;
use crate::*;

struct Temp(PathBuf);
impl Temp {
    fn new(tag: &str) -> Self {
        let d = std::env::temp_dir().join(format!("fn7-assets-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        Temp(d)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A 1×1 PNG header's worth of bytes with the given size written into IHDR, plus a tail to make files differ.
fn png(w: u32, h: u32, tail: &str) -> Vec<u8> {
    let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R'];
    v.extend(w.to_be_bytes());
    v.extend(h.to_be_bytes());
    v.extend(tail.as_bytes());
    v
}

const PORTRAIT: &str = "Waist-up, three-quarter view, dramatic rim light: Vex, a demigod: wiry and long-limbed; a larger-than-life figure. Style: a dark-fantasy world at night. No words or letters in the picture, no gore, no real people.";

fn hash_of(n: u8) -> String {
    format!("{n:02x}").repeat(32)
}

/// A media cache with three images: two OpenAI portraits/events with costs and a free mock statue.
fn fake_cache(dir: &Path) -> Vec<Sidecar> {
    let cache = MediaCache::new(dir);
    let mk = |n: u8, kind: ImageKind, prompt: &str, provider: &str, cost: f64, m: Option<&str>, ext: &str| Sidecar {
        key: hash_of(n), kind, prompt: prompt.into(), provider: provider.into(),
        model: if provider == "mock" { "mock".into() } else { "gpt-image-1-mini".into() }, ext: ext.into(), bytes: 0,
        created_at: 1_794_600_000_000 + n as i64 * 86_400_000, cost_usd: cost, latency_ms: 1500.5, match_id: m.map(str::to_string),
    };
    let sides = vec![
        mk(1, ImageKind::Portrait, PORTRAIT, "openai", 0.0285, None, "png"),
        mk(2, ImageKind::Event, "Seen from high above the hex-tile table: Vex casts volcano on Mara in Oslo; starring Vex (round), rendered with blocky geometry. Style: x. No words.", "openai", 0.027, Some("match-a"), "png"),
        mk(3, ImageKind::Statue, "A figure on a plinth: a faceted low-poly figure of the demigod Mara (tall), mid-fall. Style: x.", "mock", 0.0, Some("match-a"), "svg"),
    ];
    for (i, s) in sides.iter().enumerate() {
        let bytes = if s.ext == "svg" { format!("<svg xmlns=\"x\" width=\"256\" height=\"256\">{i}</svg>").into_bytes() } else { png(1024, 1536, &s.key) };
        cache.store(&bytes, s).unwrap();
    }
    sides
}

/// Every table as text, for "nothing changed" comparisons (the run log aside).
fn dump(reg: &Registry) -> String {
    reg.with(|c| {
        let mut out = String::new();
        for (t, order) in [("assets", "id"), ("generations", "id"), ("generation_outputs", "generation_id, asset_id"), ("lineage", "parent_id, child_id, role"), ("uses", "asset_id, match_id, beat, role, subject_ref")] {
            let cols = if t == "uses" { "asset_id, match_id, beat, last_beat, role, subject_ref, subject_kind, subject_key, loadout_hash, seat, first_seen_at, last_seen_at, source" } else if t == "lineage" { "parent_id, child_id, role, note" } else { "*" };
            for r in crate::query::rows(c, &format!("SELECT {cols} FROM {t} ORDER BY {order}"), &[])? {
                out.push_str(&format!("{t} {r}\n"));
            }
        }
        Ok(out)
    })
    .unwrap()
}

fn one(reg: &Registry, sql: &str) -> Value {
    reg.with(|c| Ok(crate::query::rows(c, sql, &[])?.into_iter().next().unwrap_or(Value::Null))).unwrap()
}

fn n(reg: &Registry, sql: &str) -> i64 {
    reg.with(|c| Ok(c.query_row(sql, [], |r| r.get(0))?)).unwrap()
}

// ---------------------------------------------------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn schema_is_created_migrated_and_guarded() {
    let t = Temp::new("schema");
    // A fresh registry is at the newest version with every table and the view.
    let reg = Registry::open(t.path().join("a")).unwrap();
    assert_eq!(reg.schema_version().unwrap(), schema::VERSION);
    for name in ["assets", "generations", "generation_outputs", "lineage", "uses", "ingest_runs", "current_assets"] {
        assert_eq!(n(&reg, &format!("SELECT COUNT(*) FROM sqlite_master WHERE name = '{name}'")), 1, "{name}");
    }
    // The constraints hold: an unknown status, kind, subject kind or lineage role is refused.
    reg.with(|c| {
        let bad = |sql: &str| assert!(c.execute(sql, []).is_err(), "{sql}");
        bad("INSERT INTO assets (id, kind, subkind, subject_kind, subject_key, display_name, created_at, updated_at, status, source) VALUES ('x','image','p','demigod','k','n',1,1,'done','t')");
        bad("INSERT INTO assets (id, kind, subkind, subject_kind, subject_key, display_name, created_at, updated_at, status, source) VALUES ('x','video','p','demigod','k','n',1,1,'ready','t')");
        bad("INSERT INTO assets (id, kind, subkind, subject_kind, subject_key, display_name, created_at, updated_at, status, source) VALUES ('x','image','p','robot','k','n',1,1,'ready','t')");
        bad("INSERT INTO generations (id, asset_id, provider, status, source, recorded_at) VALUES ('g','nope','openai','ok','t',1)");
        Ok(())
    })
    .unwrap();
    drop(reg);

    // A version-1 database (before ingest_runs) with a row in it migrates in place and keeps the row.
    let dir = t.path().join("old");
    std::fs::create_dir_all(&dir).unwrap();
    {
        let c = Connection::open(dir.join("assets.sqlite")).unwrap();
        c.execute_batch(schema::MIGRATIONS[0]).unwrap();
        c.pragma_update(None, "user_version", 1).unwrap();
        c.execute("INSERT INTO assets (id, kind, subkind, subject_kind, subject_key, display_name, created_at, updated_at, status, source) VALUES ('ast_old','image','portrait','demigod','vex','Vex',5,5,'pending','live')", []).unwrap();
        assert!(c.execute("SELECT 1 FROM ingest_runs", []).is_err());
    }
    let reg = Registry::open(&dir).unwrap();
    assert_eq!(reg.schema_version().unwrap(), schema::VERSION);
    assert_eq!(n(&reg, "SELECT COUNT(*) FROM assets WHERE id = 'ast_old'"), 1);
    assert_eq!(n(&reg, "SELECT COUNT(*) FROM ingest_runs"), 0);
    drop(reg);
    // Opening again changes nothing; a database from a newer build is refused rather than guessed at.
    Registry::open(&dir).unwrap();
    Connection::open(dir.join("assets.sqlite")).unwrap().pragma_update(None, "user_version", schema::VERSION + 1).unwrap();
    assert!(Registry::open(&dir).is_err());
}

// ---------------------------------------------------------------------------------------------------------------------
// Backfill
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn media_cache_backfill_is_idempotent_and_keeps_the_old_paths() {
    let t = Temp::new("backfill");
    let media = t.path().join("media");
    let sides = fake_cache(&media);
    // A sidecar without its image, and one that is not JSON: counted, not ingested.
    std::fs::write(media.join(format!("{}.json", hash_of(9))), serde_json::to_vec(&Sidecar { key: hash_of(9), ..sides[0].clone() }).unwrap()).unwrap();
    std::fs::write(media.join("broken.json"), b"{").unwrap();
    let reg = Registry::open(t.path().join("assets")).unwrap();

    let r1 = ingest::media_cache(&reg, &media).unwrap();
    assert_eq!((r1.sidecars, r1.ingested, r1.missing_file, r1.bad_sidecar, r1.new_assets, r1.errors), (5, 3, 1, 1, 3, 0));
    let before = dump(&reg);
    let r2 = ingest::media_cache(&reg, &media).unwrap();
    assert_eq!((r2.ingested, r2.new_assets), (3, 0));
    assert_eq!(dump(&reg), before, "a second run changes no row");
    assert_eq!(n(&reg, "SELECT COUNT(*) FROM ingest_runs"), 2);

    // The portrait: its row, its file in the layout by sha256, and the generation from the sidecar.
    let a = one(&reg, &format!("SELECT * FROM assets WHERE media_hash = '{}'", hash_of(1)));
    assert_eq!(a["id"], asset_id_for_media(&hash_of(1)));
    assert_eq!((a["kind"].as_str(), a["subkind"].as_str(), a["status"].as_str()), (Some("image"), Some("portrait"), Some("ready")));
    assert_eq!((a["subject_kind"].as_str(), a["subject_key"].as_str(), a["display_name"].as_str()), (Some("demigod"), Some("vex"), Some("Vex")));
    let old = media.join(format!("{}.png", hash_of(1)));
    let bytes = std::fs::read(&old).unwrap();
    let sha = files::sha256_bytes(&bytes);
    assert_eq!(a["sha256"], sha);
    assert_eq!(a["path"], format!("images/portrait/{sha}.png"));
    assert_eq!((a["bytes"].as_u64(), a["mime"].as_str(), a["width"].as_u64(), a["height"].as_u64()), (Some(bytes.len() as u64), Some("image/png"), Some(1024), Some(1536)));
    assert_eq!(a["created_at"], sides[0].created_at);
    assert_eq!(a["prompt_version"], fn7_media::PROMPT_VERSION);
    let stored = reg.file_path(a["path"].as_str().unwrap()).unwrap();
    assert_eq!(std::fs::read(&stored).unwrap(), bytes);
    assert!(old.is_file(), "the media cache still serves the old path");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(std::fs::metadata(&stored).unwrap().ino(), std::fs::metadata(&old).unwrap().ino(), "hard-linked, not copied");
    }
    let g = one(&reg, &format!("SELECT * FROM generations WHERE asset_id = '{}'", a["id"].as_str().unwrap()));
    assert_eq!((g["provider"].as_str(), g["model"].as_str(), g["status"].as_str()), (Some("openai"), Some("gpt-image-1-mini"), Some("ok")));
    assert_eq!(g["prompt"], PORTRAIT);
    assert_eq!(g["negative_prompt"], "No words or letters in the picture, no gore, no real people.");
    assert_eq!((g["cost_usd"].as_f64(), g["cost_basis"].as_str()), (Some(0.0285), Some("sidecar")));
    assert_eq!((g["finished_at"].as_i64(), g["duration_ms"].as_f64()), (Some(sides[0].created_at), Some(1500.5)));
    assert_eq!(g["started_at"], sides[0].created_at - 1501);
    assert_eq!((g["requested_by"].as_str(), g["requested_match"].as_str()), (Some("unknown"), None));
    // The event belongs to its match; the mock statue is free.
    let e = one(&reg, &format!("SELECT a.subject_kind, a.subject_key, g.requested_match, g.requested_by FROM assets a JOIN generations g ON g.asset_id = a.id WHERE a.media_hash = '{}'", hash_of(2)));
    assert_eq!(e, json!({ "subject_kind": "match", "subject_key": "match-a", "requested_match": "match-a", "requested_by": "match:match-a" }));
    let s = one(&reg, &format!("SELECT a.path, a.width, a.subject_key, g.cost_usd, g.cost_basis, g.provider FROM assets a JOIN generations g ON g.asset_id = a.id WHERE a.media_hash = '{}'", hash_of(3)));
    assert!(s["path"].as_str().unwrap().starts_with("images/statue/") && s["path"].as_str().unwrap().ends_with(".svg"));
    assert_eq!((s["width"].as_u64(), s["subject_key"].as_str(), s["cost_usd"].as_f64(), s["cost_basis"].as_str(), s["provider"].as_str()), (Some(256), Some("mara"), Some(0.0), Some("free"), Some("mock")));
    let st = ingest::stats(&reg).unwrap();
    assert_eq!(st["assets"]["total"], 3);
    assert_eq!(st["cost"]["usd"], 0.0555);
    assert_eq!(st["gaps"]["ready_without_prompt"], 0);
}

#[test]
fn the_exact_prompt_round_trips_byte_for_byte() {
    let t = Temp::new("prompt");
    let media = t.path().join("media");
    let cache = MediaCache::new(&media);
    // Everything a prompt could carry that a careless layer would mangle.
    let prompt = "  Leading spaces: Zoë “the Blade” O'Hara, a demigod: line one\nline two\r\n\ttabbed; 100% {subject} \\n not-a-newline; emoji 🗡️; 日本語; trailing spaces.  Style: x.  ";
    let side = Sidecar { key: hash_of(7), kind: ImageKind::Portrait, prompt: prompt.into(), provider: "openai".into(), model: "gpt-image-1-mini".into(),
                         ext: "png".into(), bytes: 0, created_at: 1_794_600_000_000, cost_usd: 0.03, latency_ms: 10.0, match_id: None };
    cache.store(&png(8, 8, "p"), &side).unwrap();
    let reg = Registry::open(t.path().join("assets")).unwrap();
    ingest::media_cache(&reg, &media).unwrap();
    let id = asset_id_for_media(&hash_of(7));
    let stored: Vec<u8> = reg.with(|c| Ok(c.query_row("SELECT CAST(prompt AS BLOB) FROM generations WHERE asset_id = ?1", [&id], |r| r.get(0))?)).unwrap();
    assert_eq!(stored, prompt.as_bytes(), "the bytes in SQL are the bytes that were sent");
    // And through the API's JSON.
    let d = reg.detail(&id).unwrap().unwrap();
    assert_eq!(d["generations"][0]["prompt"].as_str().unwrap().as_bytes(), prompt.as_bytes());
    let text = serde_json::to_string(&d).unwrap();
    let back: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(back["generations"][0]["prompt"].as_str().unwrap().as_bytes(), prompt.as_bytes());
    // A later, poorer or richer, report of the same generation never rewrites it.
    reg.upsert_generation(&GenerationIn { id: d["generations"][0]["id"].as_str().unwrap().into(), asset_id: id.clone(), provider: "openai".into(),
                                          prompt: Some("something else".into()), status: "ok".into(), source: "live".into(), ..Default::default() }).unwrap();
    assert_eq!(reg.detail(&id).unwrap().unwrap()["generations"][0]["prompt"].as_str().unwrap().as_bytes(), prompt.as_bytes());
    assert_eq!(d["asset"]["subject_key"], "zoë “the blade” o'hara");
}

#[test]
fn nothing_is_overwritten_and_current_is_the_newest_ready_version() {
    let t = Temp::new("versions");
    let reg = Registry::open(t.path()).unwrap();
    let subject = Subject::named("demigod", "Vex");
    let mut ids = vec![];
    for (i, status) in ["ready", "ready", "failed"].iter().enumerate() {
        let f = reg.store_bytes(&png(4, 4, &format!("v{i}")), "model", "statue", "glb").unwrap();
        let id = asset_id_for_key(&format!("file:{}", f.sha256));
        reg.upsert_asset(&AssetIn { id: id.clone(), kind: "model".into(), subkind: "statue".into(), subject: Some(subject.clone()), file: Some(f),
                                    created_at: 100 + i as i64, status: status.to_string(), source: "operator".into(), ..Default::default() }).unwrap();
        ids.push(id);
    }
    assert_eq!(n(&reg, "SELECT COUNT(*) FROM assets"), 3, "three versions, three rows");
    assert_eq!(one(&reg, "SELECT id FROM current_assets")["id"], ids[1], "the newest ready one is current");
    assert_eq!(reg.list(&Filter { current: true, ..Default::default() }).unwrap()["total"], 1);
    // A ready asset's file and status are final: a later report with other bytes or a failure changes nothing.
    let other = reg.store_bytes(b"other bytes", "model", "statue", "glb").unwrap();
    let before = one(&reg, &format!("SELECT sha256, path, status, updated_at FROM assets WHERE id = '{}'", ids[0]));
    reg.upsert_asset(&AssetIn { id: ids[0].clone(), kind: "model".into(), subkind: "statue".into(), file: Some(other), created_at: 999,
                                status: "failed".into(), status_reason: Some("late".into()), source: "live".into(), ..Default::default() }).unwrap();
    assert_eq!(one(&reg, &format!("SELECT sha256, path, status, updated_at FROM assets WHERE id = '{}'", ids[0])), before);
    // Both stored files are still on disk.
    assert_eq!(std::fs::read_dir(t.path().join("models/statue")).unwrap().count(), 4);
}

// ---------------------------------------------------------------------------------------------------------------------
// Lineage, provenance, jobs
// ---------------------------------------------------------------------------------------------------------------------

/// A Meshy production directory shaped like the Unreal repo's, small.
fn fake_production(dir: &Path, media_hash: &str) {
    let fig = dir.join("figures/vex");
    std::fs::create_dir_all(&fig).unwrap();
    std::fs::create_dir_all(dir.join("mannequin/skins/wood")).unwrap();
    std::fs::create_dir_all(dir.join("mannequin/skins/gold")).unwrap();
    std::fs::create_dir_all(dir.join("mannequin/anims")).unwrap();
    let w = |p: &str, b: &[u8]| {
        std::fs::write(dir.join(p), b).unwrap();
        files::sha256_bytes(b)
    };
    let portrait = w("figures/vex/portrait.png", &png(1024, 1024, "portrait"));
    let heroic = w("figures/vex/heroic.png", &png(1024, 1536, "heroic"));
    let glb = w("figures/vex/vex_statue.glb", &glb_with_tris(12));
    let preview = w("figures/vex/preview.png", &png(256, 256, "preview"));
    w("figures/figures.json", json!({ "version": 1, "figures": { "vex": { "kind": "demigod", "name": "Vex", "meshy_task": "task-statue",
        "statue_glb": "vex/vex_statue.glb", "statue_fbx": "vex/vex_statue.fbx", "preview": "vex/preview.png", "source_image": "vex/heroic.png", "result": "SUCCEEDED 30" } } }).to_string().as_bytes());
    w("figures/vex/provenance.json", json!({ "slug": "vex", "kind": "demigod", "name": "Vex", "lineage": [
        { "step": 1, "asset": "portrait", "provider": "fn7-media (OpenAI gpt-image-1-mini)", "media_hash": media_hash,
          "file": { "path": "figures/vex/portrait.png", "sha256": portrait, "bytes": 1 } },
        { "step": 2, "asset": "heroic reference image", "provider": "OpenAI", "model": "gpt-image-1-mini", "endpoint": "POST /v1/images/edits",
          "size": "1024x1536", "quality": "high", "reference_images": ["figures/vex/portrait.png"], "prompt": "A single full-body figure in a HEROIC POSE: Vex.",
          "made_at": "2026-11-14", "cost": "not recorded per call; about $0.05 by the repo's price estimate",
          "file": { "path": "figures/vex/heroic.png", "sha256": heroic, "bytes": 1 } },
        { "step": 3, "asset": "heroic statue (static textured mesh)", "provider": "Meshy", "cli": "meshy-cli 0.4.0", "operation": "image-to-3d",
          "params": { "model_type": "standard", "texture_resolution": "2k" }, "input": "figures/vex/heroic.png", "task_id": "task-statue", "credits": 30,
          "made_at": "2026-11-14", "files": [ { "path": "figures/vex/vex_statue.glb", "sha256": glb, "bytes": 1 }, { "path": "figures/vex/preview.png", "sha256": preview, "bytes": 1 } ] } ] }).to_string().as_bytes());
    for (skin, maps) in [("wood", ["base_color", "normal"]), ("gold", ["base_color", "roughness"])] {
        for m in maps {
            w(&format!("mannequin/skins/{skin}/texture_0_{m}.png"), &png(2048, 2048, &format!("{skin}{m}")));
        }
        w(&format!("mannequin/skins/{skin}/preview.png"), &png(64, 64, skin));
    }
    let rig = w("mannequin/mannequin_rigged.fbx", b"rigged fbx");
    let clips = w("mannequin/mannequin_all_clips.glb", b"all clips glb");
    w("mannequin/anims/idle_guard.fbx", b"idle guard fbx");
    w("mannequin/anims/walk.fbx", b"walk fbx");
    w("mannequin/provenance.json", json!({ "slug": "mannequin", "kind": "shared figure", "name": "the duel mannequin", "total_credits": 51, "lineage": [
        { "step": 1, "asset": "mesh (T-pose)", "provider": "Meshy", "operation": "text-to-3d preview", "params": { "pose_mode": "t-pose" },
          "prompt": "A classic wooden artist's mannequin", "task_id": "task-mesh", "credits": 20 },
        { "step": 2, "asset": "wood texture (PBR, 4K)", "provider": "Meshy", "operation": "text-to-3d refine", "texture_prompt": "pale natural beech wood", "task_id": "task-wood", "credits": 10 },
        { "step": 3, "asset": "rig", "provider": "Meshy", "operation": "rigging", "params": { "height_meters": 1.7 }, "task_id": "task-rig", "credits": 5,
          "file": { "path": "mannequin/mannequin_rigged.fbx", "sha256": rig, "bytes": 1 } },
        { "step": 4, "asset": "skins", "provider": "Meshy", "operation": "retexture", "params": { "enable_pbr": true }, "credits_each": 10,
          "skins": { "gold": { "task_id": "task-gold", "prompt": "solid burnished antique gold" } } },
        { "step": 5, "asset": "4 clips", "provider": "Meshy", "operation": "rig's bundled walk/run + 2 animation-library clips", "credits": 6,
          "clip_action_ids": { "idle_guard": 89, "lunge": 240 }, "file": { "path": "mannequin/mannequin_all_clips.glb", "sha256": clips, "bytes": 1 } } ] }).to_string().as_bytes());
    // What the Meshy CLI leaves behind (git-ignored in the Unreal repo): measured times and credits per task.
    let proj = dir.join("20261114_statue_vex");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(proj.join("metadata.json"), json!({ "tasks": [
        { "task_id": "task-statue", "stage": "model", "endpoint": "/openapi/v1/image-to-3d" },
        { "task_id": "task-lunge", "stage": "anim-lunge", "endpoint": "/openapi/v1/animations" } ] }).to_string()).unwrap();
    std::fs::write(proj.join("task_task-statue.json"), json!({ "id": "task-statue", "created_at": 1_794_698_221_145i64, "started_at": 1_794_698_221_157i64, "finished_at": 1_794_698_319_490i64, "consumed_credits": 30 }).to_string()).unwrap();
    std::fs::write(proj.join("task_task-lunge.json"), json!({ "id": "task-lunge", "created_at": 10, "started_at": 20, "finished_at": 520, "consumed_credits": 3 }).to_string()).unwrap();
}

/// A minimal GLB: header + a JSON chunk with one triangle-mode primitive of `tris` triangles.
fn glb_with_tris(tris: u64) -> Vec<u8> {
    let mut j = json!({ "accessors": [ { "count": tris * 3 } ], "meshes": [ { "primitives": [ { "indices": 0 } ] } ] }).to_string().into_bytes();
    while j.len() % 4 != 0 {
        j.push(b' ');
    }
    let mut v = b"glTF".to_vec();
    v.extend(2u32.to_le_bytes());
    v.extend((20 + j.len() as u32).to_le_bytes());
    v.extend((j.len() as u32).to_le_bytes());
    v.extend(b"JSON");
    v.extend(j);
    v
}

#[test]
fn provenance_import_builds_the_chain_portrait_to_statue() {
    let t = Temp::new("prov");
    let prod = t.path().join("meshy");
    let mh = hash_of(0xab);
    fake_production(&prod, &mh);
    // The portrait is already known from the media cache (same bytes): the import must link to it, not copy it.
    let media = t.path().join("media");
    let portrait_bytes = std::fs::read(prod.join("figures/vex/portrait.png")).unwrap();
    MediaCache::new(&media).store(&portrait_bytes, &Sidecar { key: mh.clone(), kind: ImageKind::Portrait, prompt: PORTRAIT.into(), provider: "openai".into(),
        model: "gpt-image-1-mini".into(), ext: "png".into(), bytes: 0, created_at: 1_794_600_000_000, cost_usd: 0.0285, latency_ms: 9000.0, match_id: None }).unwrap();
    let reg = Registry::open(t.path().join("assets")).unwrap();
    ingest::media_cache(&reg, &media).unwrap();

    let rep = provenance::import_provenance(&reg, &prod).unwrap();
    assert_eq!(rep.provenance_files, 2);
    assert!(rep.hash_mismatches.is_empty(), "{:?}", rep.hash_mismatches);
    assert!(rep.missing_files.is_empty(), "{:?}", rep.missing_files);
    assert_eq!(rep.credits, 30.0 + 20.0 + 10.0 + 5.0 + 10.0 + 6.0);
    let before = dump(&reg);
    provenance::import_provenance(&reg, &prod).unwrap();
    assert_eq!(dump(&reg), before, "importing again changes no row");

    // One portrait row (the media layer's), not two.
    assert_eq!(n(&reg, "SELECT COUNT(*) FROM assets WHERE subkind = 'portrait'"), 1);
    let portrait = asset_id_for_media(&mh);
    let statue = one(&reg, "SELECT * FROM assets WHERE kind = 'model' AND subkind = 'statue'");
    assert_eq!((statue["subject_kind"].as_str(), statue["subject_key"].as_str(), statue["status"].as_str(), statue["tris"].as_u64(), statue["mime"].as_str()),
               (Some("demigod"), Some("vex"), Some("ready"), Some(12), Some("model/gltf-binary")));
    assert!(statue["path"].as_str().unwrap().starts_with("models/statue/"));
    assert_eq!(statue["created_at"], 1_794_698_319_490i64, "the task's measured finish time");
    let sid = statue["id"].as_str().unwrap();
    // The chain, oldest first: portrait → heroic image → statue → preview, with the role of each link.
    let chain = reg.chain(sid).unwrap();
    let names: Vec<(i64, String, String)> = chain.iter().map(|c| (c["depth"].as_i64().unwrap(), c["subkind"].as_str().unwrap().to_string(), c["variant"].as_str().unwrap().to_string())).collect();
    assert_eq!(names, vec![(-2, "portrait".into(), "".into()), (-1, "reference".into(), "heroic".into()), (0, "statue".into(), "".into()), (1, "preview".into(), "".into())]);
    assert_eq!(chain[0]["id"], portrait);
    assert_eq!(chain[1]["parents"], format!("reference:{portrait}"));
    assert_eq!(chain[2]["parents"], format!("source:{}", chain[1]["id"].as_str().unwrap()));
    assert_eq!(chain[3]["parents"], format!("preview_of:{sid}"));
    // From the other end, the same chain.
    assert_eq!(reg.chain(&portrait).unwrap().len(), 4);
    // The statue's generation: Meshy, the task id, the credits, the exact params, measured times from the task file.
    let d = reg.detail(sid).unwrap().unwrap();
    let g = &d["generations"][0];
    assert_eq!((g["provider"].as_str(), g["operation"].as_str(), g["provider_task_id"].as_str(), g["cost_credits"].as_f64(), g["cost_basis"].as_str()),
               (Some("meshy"), Some("image-to-3d"), Some("task-statue"), Some(30.0), Some("credits_reported")));
    assert_eq!(g["params"], json!({ "model_type": "standard", "texture_resolution": "2k" }));
    assert_eq!((g["started_at"].as_i64(), g["finished_at"].as_i64(), g["duration_ms"].as_f64()), (Some(1_794_698_221_157), Some(1_794_698_319_490), Some(98_333.0)));
    assert_eq!(g["endpoint"], "/openapi/v1/image-to-3d");
    assert_eq!(g["reference_asset_ids"], json!([chain[1]["id"]]));
    assert_eq!(d["lineage"]["parents"][0]["role"], "source");
    assert_eq!(d["lineage"]["children"][0]["role"], "preview_of");
    // The heroic image: OpenAI, the exact prompt, an estimated cost marked as such.
    let h = reg.detail(chain[1]["id"].as_str().unwrap()).unwrap().unwrap();
    let hg = &h["generations"][0];
    assert_eq!((hg["provider"].as_str(), hg["operation"].as_str(), hg["prompt"].as_str(), hg["cost_usd"].as_f64(), hg["cost_basis"].as_str()),
               (Some("openai"), Some("images/edits"), Some("A single full-body figure in a HEROIC POSE: Vex."), Some(0.05), Some("estimated")));
    assert_eq!(hg["finished_at"], 1_794_614_400_000i64, "2026-11-14 UTC");
    // The portrait kept the media layer's generation and gained no second one.
    assert_eq!(n(&reg, &format!("SELECT COUNT(*) FROM generations WHERE asset_id = '{portrait}'")), 1);

    // The mannequin: an external mesh, the rig from it, skins as retextures (every map an asset), clips of the rig.
    let mesh = one(&reg, "SELECT a.id, a.status, a.path, g.prompt, g.cost_credits, g.operation FROM assets a JOIN generations g ON g.asset_id = a.id WHERE a.subkind = 'mannequin' AND a.variant = 'mesh'");
    assert_eq!((mesh["status"].as_str(), mesh["path"].as_str(), mesh["prompt"].as_str(), mesh["cost_credits"].as_f64(), mesh["operation"].as_str()),
               (Some("external"), None, Some("A classic wooden artist's mannequin"), Some(20.0), Some("text-to-3d preview")));
    let mid = mesh["id"].as_str().unwrap();
    assert_eq!(n(&reg, &format!("SELECT COUNT(*) FROM lineage WHERE parent_id = '{mid}' AND role = 'rig_of'")), 1);
    assert_eq!(n(&reg, &format!("SELECT COUNT(*) FROM lineage WHERE parent_id = '{mid}' AND role = 'retexture_of'")), 4, "two maps each of two skins");
    let gold = one(&reg, "SELECT g.prompt, g.provider_task_id, g.cost_credits, g.operation, (SELECT COUNT(*) FROM generation_outputs o WHERE o.generation_id = g.id) AS outputs FROM generations g JOIN assets a ON a.id = g.asset_id WHERE a.variant = 'gold/base_color'");
    assert_eq!(gold, json!({ "prompt": "solid burnished antique gold", "provider_task_id": "task-gold", "cost_credits": 10.0, "operation": "retexture", "outputs": 3 }));
    let wood = one(&reg, "SELECT g.prompt, g.operation FROM generations g JOIN assets a ON a.id = g.asset_id WHERE a.variant = 'wood/base_color'");
    assert_eq!(wood, json!({ "prompt": "pale natural beech wood", "operation": "text-to-3d refine" }));
    assert_eq!(n(&reg, "SELECT COUNT(*) FROM assets WHERE kind = 'texture' AND subkind = 'skin' AND path LIKE 'textures/skin/%'"), 4);
    // Clips: the bundle, two library clips (3 credits each: 6 / 2), walk and run that came with the rig (no cost).
    assert_eq!(n(&reg, "SELECT COUNT(*) FROM assets WHERE kind = 'animation'"), 5);
    let lunge = one(&reg, "SELECT a.path, a.status, g.provider_task_id, g.cost_credits, g.duration_ms, g.params FROM assets a JOIN generations g ON g.asset_id = a.id WHERE a.kind = 'animation' AND a.variant = 'lunge'");
    assert_eq!(lunge["path"], one(&reg, "SELECT path FROM assets WHERE variant = 'all_clips'")["path"], "a clip without its own file points into the bundle");
    assert_eq!((lunge["provider_task_id"].as_str(), lunge["cost_credits"].as_f64(), lunge["duration_ms"].as_f64(), lunge["params"]["action_id"].as_u64()), (Some("task-lunge"), Some(3.0), Some(500.0), Some(240)));
    let idle = one(&reg, "SELECT path FROM assets WHERE kind = 'animation' AND variant = 'idle_guard'");
    assert!(idle["path"].as_str().unwrap().starts_with("animations/clip/") && idle["path"].as_str().unwrap().ends_with(".fbx"));
    assert_eq!(n(&reg, "SELECT COUNT(*) FROM generations g JOIN assets a ON a.id = g.asset_id WHERE a.kind = 'animation' AND a.variant IN ('walk','run')"), 0);
    assert_eq!(n(&reg, "SELECT COUNT(*) FROM lineage WHERE role = 'clip_of'"), 5);
    // Credits add up to what the files say.
    assert_eq!(one(&reg, "SELECT SUM(cost_credits) AS c FROM generations WHERE provider = 'meshy'")["c"], 81.0);

    // What a client asks to seat the statue.
    let f = reg.figures(Some("  VEX ")).unwrap();
    assert_eq!((f["found"].as_bool(), f["key"].as_str()), (Some(true), Some("vex")));
    assert_eq!(f["statue"]["id"], sid);
    assert_eq!(f["statue"]["file_url"], format!("/api/assets/{sid}/file"));
    assert_eq!(f["statue"]["sha256"], statue["sha256"]);
    assert!(f["figure"].is_null());
    assert_eq!(f["mannequin"]["models"][0]["variant"], "rigged");
    assert_eq!(f["mannequin"]["skins"].as_array().unwrap().len(), 4);
    let none = reg.figures(Some("Nobody")).unwrap();
    assert_eq!((none["found"].as_bool(), none["fallback"].as_str()), (Some(false), Some("mannequin")));
    assert_eq!(reg.figures(None).unwrap()["figures"].as_array().unwrap().len(), 1);
}

/// The real thing: a copy of the Unreal repo's provenance files (read-only source; skipped where the repo is absent).
#[test]
fn the_unreal_repos_provenance_imports() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../final_night_7_unreal/production_assets/meshy");
    if !src.join("figures/figures.json").is_file() {
        eprintln!("skipped: {} is not here", src.display());
        return;
    }
    let t = Temp::new("unreal");
    let prod = t.path().join("meshy");
    // Copy what is committed there: figures/ and mannequin/ (on APFS a clone: no disk is used).
    fn copy_tree(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().filter_map(|e| e.ok()) {
            let (p, d) = (e.path(), to.join(e.file_name()));
            if p.is_dir() { copy_tree(&p, &d) } else { std::fs::copy(&p, &d).unwrap(); }
        }
    }
    copy_tree(&src.join("figures"), &prod.join("figures"));
    copy_tree(&src.join("mannequin"), &prod.join("mannequin"));
    let reg = Registry::open(t.path().join("assets")).unwrap();
    let rep = provenance::import_provenance(&reg, &prod).unwrap();
    assert_eq!(rep.provenance_files, 3, "two figures and the mannequin");
    assert!(rep.hash_mismatches.is_empty(), "every committed file matches its recorded sha256: {:?}", rep.hash_mismatches);
    assert!(rep.missing_files.is_empty(), "{:?}", rep.missing_files);
    let before = dump(&reg);
    provenance::import_provenance(&reg, &prod).unwrap();
    assert_eq!(dump(&reg), before);
    // Two statues with their exact task ids and 30 credits each.
    for (name, task, glb_sha) in [("Chris's Left Hand", "01a1dc59-9087-77f1-b365-e64fbf5e001f", "c358e92dbd22eff3a9d5eeff70873941893a6eff0f41a121e475a613fe7ab87f"),
                                  ("Benito's Left Hand", "01a1dc5b-3b55-7484-9921-7a1ba88eb055", "83b918161246b36c668d4db01e4559731e1da74c12bd74631b38755676ae7589")] {
        let f = reg.figures(Some(name)).unwrap();
        assert_eq!(f["statue"]["sha256"], glb_sha, "{name}");
        assert!(f["statue"]["tris"].as_u64().unwrap() > 1000, "{name}: {}", f["statue"]["tris"]);
        let d = reg.detail(f["statue"]["id"].as_str().unwrap()).unwrap().unwrap();
        assert_eq!((d["generations"][0]["provider_task_id"].as_str(), d["generations"][0]["cost_credits"].as_f64()), (Some(task), Some(30.0)));
        let chain = reg.chain(f["statue"]["id"].as_str().unwrap()).unwrap();
        let kinds: Vec<&str> = chain.iter().filter(|c| c["depth"].as_i64().unwrap() <= 0).map(|c| c["subkind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["portrait", "reference", "statue"], "{name}");
        let heroic = reg.detail(chain[1]["id"].as_str().unwrap()).unwrap().unwrap();
        assert!(heroic["generations"][0]["prompt"].as_str().unwrap().starts_with("A single full-body figure of this same character as a collectible hand-painted tabletop miniature statue in a HEROIC POSE: "));
    }
    // The mannequin: 174 credits in all, 35 clips and the bundle, 5 skins (wood and four retextures) of 4 maps each.
    assert_eq!(one(&reg, "SELECT SUM(g.cost_credits) AS c FROM generations g JOIN assets a ON a.id = g.asset_id WHERE a.subject_key = 'mannequin'")["c"], 174.0);
    assert_eq!(n(&reg, "SELECT COUNT(*) FROM assets WHERE kind = 'animation' AND variant != 'all_clips'"), 35);
    assert_eq!(n(&reg, "SELECT COUNT(*) FROM assets WHERE kind = 'texture' AND subkind = 'skin'"), 20);
    assert_eq!(n(&reg, "SELECT COUNT(*) FROM generations WHERE operation = 'retexture' AND prompt IS NOT NULL AND provider_task_id IS NOT NULL"), 4);
    assert_eq!(one(&reg, "SELECT g.prompt FROM generations g WHERE g.operation = 'text-to-3d preview'")["prompt"].as_str().map(|p| p.starts_with("A classic wooden artist's figure-drawing mannequin")), Some(true));
    assert_eq!(one(&reg, "SELECT SUM(cost_credits) AS c FROM generations")["c"], 234.0, "174 + two statues at 30");
    let v = reg.costs("provider", Some(0.02)).unwrap();
    let meshy = v["items"].as_array().unwrap().iter().find(|i| i["key"] == "meshy").unwrap();
    assert_eq!((meshy["cost_credits"].as_f64(), meshy["credits_usd_estimate"].as_f64()), (Some(234.0), Some(4.68)));
}

#[test]
fn job_records_import_and_are_idempotent() {
    let t = Temp::new("jobs");
    let prod = t.path().join("meshy");
    std::fs::create_dir_all(prod.join("figures/mara")).unwrap();
    std::fs::write(prod.join("figures/mara/apose.png"), png(1024, 1536, "apose")).unwrap();
    std::fs::write(prod.join("figures/mara/mara_rigged.glb"), glb_with_tris(7)).unwrap();
    std::fs::write(prod.join("figures/mara/mara_model.glb"), glb_with_tris(9)).unwrap();
    std::fs::write(prod.join("figures/mara/preview.png"), png(64, 64, "pv")).unwrap();
    let lines = [
        json!({ "v": 1, "at": "2026-11-15T10:00:00Z", "provider": "meshy", "operation": "image-to-3d", "task_id": "t-model", "status": "SUCCEEDED", "credits": 30,
                "requested_at": 1000, "started_at": 1100, "finished_at": 61100, "endpoint": "/openapi/v1/image-to-3d", "params": { "pose_mode": "a-pose" },
                "subject": { "kind": "demigod", "name": "Mara", "slug": "mara" }, "asset": { "kind": "model", "subkind": "figure", "variant": "mesh" },
                "inputs": [ { "path": "figures/mara/apose.png", "role": "source", "variant": "apose" } ],
                "outputs": [ { "path": "figures/mara/mara_model.glb", "role": "primary" }, { "path": "figures/mara/preview.png", "role": "preview" },
                             { "path": "figures/mara/mara_model.fbx", "role": "fbx", "optional": true } ],
                "requested_by": "operator", "tool": "tools/meshy/make_figure.sh" }),
        json!({ "v": 1, "provider": "meshy", "operation": "rigging", "task_id": "t-rig", "status": "SUCCEEDED", "credits": 5, "started_at": 70000, "finished_at": 90000,
                "params": { "height_meters": 1.7, "skeleton_template": "ue5_mannequin" }, "subject": { "kind": "demigod", "name": "Mara", "slug": "mara" },
                "asset": { "kind": "model", "subkind": "figure", "variant": "" }, "lineage": [ { "role": "rig_of", "parent_task_id": "t-model" } ],
                "outputs": [ { "path": "figures/mara/mara_rigged.glb", "role": "primary" } ] }),
        json!({ "v": 1, "provider": "meshy", "operation": "image-to-3d", "task_id": "t-bad", "status": "FAILED", "credits": 0, "error": "moderation",
                "subject": { "kind": "god", "name": "Consequence" }, "asset": { "kind": "model", "subkind": "statue" }, "outputs": [] }),
    ];
    let file = prod.join("jobs.jsonl");
    std::fs::write(&file, format!("# job records\n{}\n\nnot json\n", lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n"))).unwrap();
    let reg = Registry::open(t.path().join("assets")).unwrap();
    let rep = provenance::import_jobs(&reg, &file, None).unwrap();
    assert_eq!((rep.jobs, rep.credits, rep.notes.len()), (3, 35.0, 1), "{:?}", rep.notes);
    assert!(rep.missing_files.is_empty(), "an optional output that is absent is not an error: {:?}", rep.missing_files);
    let before = dump(&reg);
    provenance::import_jobs(&reg, &file, Some(&prod)).unwrap();
    assert_eq!(dump(&reg), before);
    // The rigged figure is what a client gets; it is the rig of the model, which came from the A-pose image.
    let f = reg.figures(Some("Mara")).unwrap();
    assert_eq!((f["figure"]["tris"].as_u64(), f["figure"]["variant"].as_str()), (Some(7), Some("")));
    let chain = reg.chain(f["figure"]["id"].as_str().unwrap()).unwrap();
    let got: Vec<(&str, &str)> = chain.iter().map(|c| (c["subkind"].as_str().unwrap(), c["variant"].as_str().unwrap())).collect();
    assert_eq!(got, [("reference", "apose"), ("figure", "mesh"), ("figure", "")]);
    assert!(chain[2]["parents"].as_str().unwrap().starts_with("rig_of:"));
    let g = one(&reg, "SELECT requested_at, started_at, finished_at, duration_ms, cost_credits, cost_basis, requested_by, provider_task_id FROM generations WHERE provider_task_id = 't-model'");
    assert_eq!(g, json!({ "requested_at": 1000, "started_at": 1100, "finished_at": 61100, "duration_ms": 60000.0, "cost_credits": 30.0, "cost_basis": "credits_reported", "requested_by": "operator", "provider_task_id": "t-model" }));
    // The failed job is on record, with no file and its error.
    let bad = one(&reg, "SELECT a.status, a.path, a.subject_kind, g.status AS gstatus, g.error FROM assets a JOIN generations g ON g.asset_id = a.id WHERE g.provider_task_id = 't-bad'");
    assert_eq!(bad, json!({ "status": "failed", "path": null, "subject_kind": "god", "gstatus": "failed", "error": "moderation" }));
    assert!(reg.figures(Some("Consequence")).unwrap()["statue"].is_null(), "a failed statue is never current");
}

// ---------------------------------------------------------------------------------------------------------------------
// Costs and time travel
// ---------------------------------------------------------------------------------------------------------------------

fn seed_costs(reg: &Registry) {
    let day = 86_400_000i64;
    let d0 = 1_794_614_400_000i64; // 2026-11-14 00:00 UTC
    let mk = |i: usize, subkind: &str, subject: Subject, provider: &str, usd: Option<f64>, credits: Option<f64>, basis: &str, status: &str, m: Option<&str>, at: i64, dur: f64| {
        let id = asset_id_for_key(&format!("cost:{i}"));
        reg.upsert_asset(&AssetIn { id: id.clone(), kind: if provider == "meshy" { "model".into() } else { "image".into() }, subkind: subkind.into(), subject: Some(subject),
                                    created_at: at, status: "pending".into(), source: "live".into(), ..Default::default() }).unwrap();
        reg.upsert_generation(&GenerationIn { id: generation_id_for_key(&format!("cost:{i}")), asset_id: id, provider: provider.into(), cost_usd: usd, cost_credits: credits,
                                              cost_basis: Some(basis.into()), status: status.into(), requested_match: m.map(str::to_string), started_at: Some(at),
                                              finished_at: Some(at + dur as i64), duration_ms: Some(dur), source: "live".into(), ..Default::default() }).unwrap();
    };
    mk(0, "portrait", Subject::named("demigod", "Vex"), "openai", Some(0.0285), None, "usage", "ok", Some("m1"), d0 + 1000, 9000.0);
    mk(1, "diorama", Subject::named("demigod", "Vex"), "openai", Some(0.0229), None, "usage", "ok", Some("m1"), d0 + 2000, 8000.0);
    mk(2, "event", Subject::new("match", "m1", "x"), "openai", Some(0.0269), None, "usage", "ok", Some("m1"), d0 + day, 12000.0);
    mk(3, "event", Subject::new("match", "m2", "x"), "openai", Some(0.0), None, "free", "refused", Some("m2"), d0 + day, 0.0);
    mk(4, "event", Subject::new("match", "m2", "x"), "openai", Some(0.0), None, "free", "failed", Some("m2"), d0 + day, 500.0);
    mk(5, "reference", Subject::named("demigod", "Vex"), "openai", Some(0.05), None, "estimated", "ok", None, d0 + day, 20000.0);
    mk(6, "statue", Subject::named("demigod", "Vex"), "meshy", None, Some(30.0), "credits_reported", "ok", None, d0 + 2 * day, 98000.0);
    mk(7, "portrait", Subject::named("demigod", "Mara"), "mock", Some(0.0), None, "free", "ok", Some("m2"), d0 + 2 * day, 1.0);
}

#[test]
fn costs_roll_up_by_day_kind_provider_match_and_subject() {
    let t = Temp::new("costs");
    let reg = Registry::open(t.path()).unwrap();
    seed_costs(&reg);
    let by = |b: &str| reg.costs(b, None).unwrap();
    let row = |v: &Value, k: &str| v["items"].as_array().unwrap().iter().find(|i| i["key"] == k).cloned().unwrap_or_else(|| panic!("no {k} in {v}"));
    let v = by("provider");
    let o = row(&v, "openai");
    assert_eq!((o["calls"].as_i64(), o["ok"].as_i64(), o["failed"].as_i64(), o["refused"].as_i64()), (Some(6), Some(4), Some(1), Some(1)));
    assert_eq!((o["cost_usd"].as_f64(), o["usd_recorded"].as_f64(), o["usd_estimated"].as_f64()), (Some(0.1283), Some(0.0783), Some(0.05)));
    assert_eq!((o["duration_ms"].as_f64(), o["avg_duration_ms"].as_f64()), (Some(49500.0), Some(8250.0)));
    let m = row(&v, "meshy");
    assert_eq!((m["cost_usd"].as_f64(), m["cost_credits"].as_f64(), m["calls"].as_i64()), (Some(0.0), Some(30.0), Some(1)));
    assert_eq!((v["total"]["calls"].as_i64(), v["total"]["cost_usd"].as_f64(), v["total"]["cost_credits"].as_f64()), (Some(8), Some(0.1283), Some(30.0)));
    let v = by("day");
    let keys: Vec<&str> = v["items"].as_array().unwrap().iter().map(|i| i["key"].as_str().unwrap()).collect();
    assert_eq!(keys, ["2026-11-14", "2026-11-15", "2026-11-16"]);
    assert_eq!(row(&v, "2026-11-14")["cost_usd"], 0.0514);
    assert_eq!(row(&v, "2026-11-15")["cost_usd"], 0.0769);
    assert_eq!(row(&v, "2026-11-16")["cost_credits"], 30.0);
    let v = by("kind");
    assert_eq!((row(&v, "image/event")["calls"].as_i64(), row(&v, "image/event")["cost_usd"].as_f64()), (Some(3), Some(0.0269)));
    assert_eq!(row(&v, "model/statue")["cost_credits"], 30.0);
    let v = by("match");
    assert_eq!(row(&v, "m1")["cost_usd"], 0.0783);
    assert_eq!((row(&v, "m2")["cost_usd"].as_f64(), row(&v, "m2")["calls"].as_i64()), (Some(0.0), Some(3)));
    assert_eq!(row(&v, "unknown")["calls"], 2);
    let v = by("subject");
    let vex = row(&v, "demigod:vex");
    assert_eq!((vex["cost_usd"].as_f64(), vex["cost_credits"].as_f64(), vex["calls"].as_i64()), (Some(0.1014), Some(30.0), Some(4)));
    assert_eq!(row(&by("basis"), "estimated")["cost_usd"], 0.05);
    assert_eq!(reg.costs("provider", Some(0.02)).unwrap()["total"]["credits_usd_estimate"], 0.6);
    assert!(reg.costs("colour", None).is_err());
}

#[test]
fn the_at_beat_lookup_answers_what_a_demigod_looked_like() {
    let t = Temp::new("at");
    let reg = Registry::open(t.path()).unwrap();
    let vex = Subject::named("demigod", "Vex");
    let img = |tag: &str, subkind: &str, subject: &Subject| -> String {
        let f = reg.store_bytes(&png(8, 8, tag), "image", subkind, "png").unwrap();
        let id = asset_id_for_media(&files::sha256_bytes(tag.as_bytes()));
        reg.upsert_asset(&AssetIn { id: id.clone(), kind: "image".into(), subkind: subkind.into(), subject: Some(subject.clone()), file: Some(f), created_at: 1,
                                    status: "ready".into(), media_hash: Some(files::sha256_bytes(tag.as_bytes())), source: "live".into(), ..Default::default() }).unwrap();
        id
    };
    let (a, b, dio) = (img("plain", "portrait", &vex), img("armoured", "portrait", &vex), img("dio", "diorama", &vex));
    let other = img("mara", "portrait", &Subject::named("demigod", "Mara"));
    let card = img("card", "event", &Subject::new("match", "m1", "Vex casts volcano"));
    let seen = |asset: &str, m: &str, beat: i64, role: &str, sref: &str, key: &str, loadout: Option<&str>, extend: bool| {
        reg.note_use(&UseIn { asset_id: asset.into(), match_id: m.into(), beat: Some(beat), role: role.into(), subject_ref: sref.into(), subject_kind: Some("demigod".into()),
                              subject_key: Some(key.into()), loadout_hash: loadout.map(str::to_string), seat: None, at_ms: 1000 + beat, source: "live".into() }, extend).unwrap();
    };
    // Beats 1–5 the plain portrait, 6–9 armoured (a new loadout), 10–12 plain again; the diorama throughout.
    for beat in 1..=12 {
        let (p, l, first) = match beat { 1..=5 => (&a, None, beat == 1), 6..=9 => (&b, Some("gear-1"), beat == 6), _ => (&a, None, beat == 10) };
        seen(p, "m1", beat, "portrait_image", "dg:0", "vex", l, !first);
        seen(&dio, "m1", beat, "diorama_image", "dg:0", "vex", None, beat != 1);
        seen(&other, "m1", beat, "portrait_image", "dg:1", "mara", None, beat != 1);
    }
    reg.note_use(&UseIn { asset_id: card.clone(), match_id: "m1".into(), beat: Some(7), role: "image".into(), subject_kind: Some("match".into()), subject_key: Some("m1".into()), at_ms: 1, source: "live".into(), ..Default::default() }, false).unwrap();
    seen(&b, "m2", 3, "portrait_image", "dg:2", "vex", Some("gear-1"), false);
    // Three runs of the portrait, not twelve rows; one run of the diorama.
    assert_eq!(n(&reg, "SELECT COUNT(*) FROM uses WHERE match_id = 'm1' AND subject_ref = 'dg:0' AND role = 'portrait_image'"), 3);
    assert_eq!(one(&reg, "SELECT beat, last_beat FROM uses WHERE match_id = 'm1' AND role = 'diorama_image'"), json!({ "beat": 1, "last_beat": 12 }));

    let portrait_at = |m: &str, beat: Option<i64>, who: &str| -> (String, String) {
        let v = reg.at(m, beat, Some(who)).unwrap();
        let p = v["items"].as_array().unwrap().iter().find(|i| i["subkind"] == "portrait").cloned().unwrap_or(Value::Null);
        (p["id"].as_str().unwrap_or("").to_string(), p["matched"].as_str().unwrap_or("").to_string())
    };
    assert_eq!(portrait_at("m1", Some(3), "Vex"), (a.clone(), "covers_beat".into()));
    assert_eq!(portrait_at("m1", Some(7), "vex"), (b.clone(), "covers_beat".into()));
    assert_eq!(portrait_at("m1", Some(11), " VEX "), (a.clone(), "covers_beat".into()));
    assert_eq!(portrait_at("m1", Some(7), "dg:0").0, b, "by the engine ref of that match");
    assert_eq!(portrait_at("m1", Some(7), "dg:1").0, other);
    assert_eq!(portrait_at("m1", Some(40), "Vex"), (a.clone(), "latest_before".into()), "after the last beat seen: the last look");
    assert_eq!(portrait_at("m1", Some(0), "Vex"), (a.clone(), "earliest_seen".into()));
    assert_eq!(portrait_at("m1", None, "Vex").0, a, "no beat: the latest");
    assert_eq!(portrait_at("m2", Some(3), "Vex").0, b, "another match, another answer");
    assert_eq!(portrait_at("m9", Some(3), "Vex").0, "", "a match with no record answers nothing");
    // The answer carries what a client needs to draw it: the file path, the URL, the hash, the loadout.
    let v = reg.at("m1", Some(7), Some("Vex")).unwrap();
    assert_eq!(v["items"].as_array().unwrap().len(), 2, "portrait and diorama: {v}");
    let p = v["items"].as_array().unwrap().iter().find(|i| i["subkind"] == "portrait").unwrap();
    assert_eq!((p["loadout_hash"].as_str(), p["first_beat"].as_i64(), p["last_beat"].as_i64()), (Some("gear-1"), Some(6), Some(9)));
    assert!(p["path"].as_str().unwrap().starts_with("images/portrait/"));
    assert_eq!(p["file_url"], format!("/api/assets/{b}/file"));
    assert!(p["media_url"].as_str().unwrap().starts_with("/media/"));
    // Without a demigod: everything in effect at that beat (both demigods' pictures and the moment card of beat 7).
    let all = reg.at("m1", Some(7), None).unwrap();
    let mut kinds: Vec<&str> = all["items"].as_array().unwrap().iter().map(|i| i["subkind"].as_str().unwrap()).collect();
    kinds.sort();
    assert_eq!(kinds, ["diorama", "event", "portrait", "portrait"]);
    assert_eq!(reg.at("m1", Some(8), None).unwrap()["items"].as_array().unwrap().len(), 3, "the card was only up at beat 7");
    // The list route's match and beat filters agree.
    assert_eq!(reg.list(&Filter { match_id: Some("m1".into()), beat: Some(7), ..Default::default() }).unwrap()["total"], 4);
    assert_eq!(reg.list(&Filter { match_id: Some("m2".into()), ..Default::default() }).unwrap()["total"], 1);
}

// ---------------------------------------------------------------------------------------------------------------------
// Live
// ---------------------------------------------------------------------------------------------------------------------

fn media_with(dir: &Path, gen: MockGenerator, budget: f64) -> Media {
    let cfg = MediaConfig { mode: Mode::Mock, dir: dir.join("media"), budget_usd: budget, match_budget_usd: budget, ..Default::default() };
    let g: Arc<dyn ImageGenerator> = Arc::new(gen);
    Media::start(cfg, Some(g))
}

#[tokio::test(flavor = "multi_thread")]
async fn the_live_observer_writes_every_request_outcome_and_refusal() {
    let t = Temp::new("live");
    let reg = Arc::new(Registry::open(t.path().join("assets")).unwrap());
    let media = media_with(t.path(), MockGenerator { price_usd: 0.25, delay: Duration::from_millis(600), ..Default::default() }, 0.6);
    let hook = LiveHook::for_media(reg.clone(), &media);
    media.set_observer(hook.clone());

    let p1 = media.prompt(ImageKind::Portrait, "Vex, a demigod: wiry and long-limbed").unwrap();
    let p2 = media.prompt(ImageKind::Diorama, "Vex, a demigod: wiry; as a tiny geometric miniature").unwrap();
    let p3 = media.prompt(ImageKind::Item, "Struck Oak Blade: a blade of oak").unwrap();
    let r1 = LiveHook::asking(live::Asker { seat: Some(2), account: Some("acc_1".into()) }, async { media.request(&p1, Some("m1")) }).await;
    assert_eq!(r1.status, "pending");
    hook.flush();
    // At the moment of the request: a pending asset and a pending generation with the prompt exactly as sent.
    let id1 = asset_id_for_media(&r1.hash);
    let pending = one(&reg, &format!("SELECT a.status, a.path, g.status AS g, g.prompt, g.requested_at IS NOT NULL AS asked, g.started_at, g.requested_seat, g.requested_account, g.requested_match, g.requested_by FROM assets a JOIN generations g ON g.asset_id = a.id WHERE a.id = '{id1}'"));
    assert_eq!(pending, json!({ "status": "pending", "path": null, "g": "pending", "prompt": p1.text, "asked": 1, "started_at": null, "requested_seat": 2, "requested_account": "acc_1", "requested_match": "m1", "requested_by": "match:m1" }));
    let r2 = media.request(&p2, Some("m1"));
    let r3 = media.request(&p3, Some("m1")); // over the budget (0.25 + 0.25 + 0.25 > 0.6)
    assert_eq!((r2.status, r3.status), ("pending", "off"));
    assert!(media.wait_idle(Duration::from_secs(10)).await);
    hook.flush();

    for (r, p, sub) in [(&r1, &p1, "portrait"), (&r2, &p2, "diorama")] {
        let d = reg.detail(&asset_id_for_media(&r.hash)).unwrap().unwrap();
        let (a, g) = (&d["asset"], &d["generations"][0]);
        assert_eq!((a["status"].as_str(), a["subkind"].as_str(), a["subject_key"].as_str(), a["media_hash"].as_str()), (Some("ready"), Some(sub), Some("vex"), Some(r.hash.as_str())));
        let on_disk = std::fs::read(reg.file_path(a["path"].as_str().unwrap()).unwrap()).unwrap();
        assert_eq!(on_disk, std::fs::read(media.cache().image_path(&r.hash, "svg")).unwrap(), "the registry file is the media file");
        assert_eq!(a["sha256"], files::sha256_bytes(&on_disk));
        assert_eq!(a["media_url"], format!("/media/{}.svg", r.hash));
        assert_eq!(d["generations"].as_array().unwrap().len(), 1, "requested and ready are one generation");
        assert_eq!(g["prompt"].as_str().unwrap().as_bytes(), p.text.as_bytes());
        assert_eq!((g["status"].as_str(), g["provider"].as_str(), g["model"].as_str(), g["operation"].as_str()), (Some("ok"), Some("mock"), Some("mock"), Some("mock")));
        assert_eq!((g["cost_usd"].as_f64(), g["cost_basis"].as_str()), (Some(0.25), Some("free")));
        let (req, start, fin, dur) = (g["requested_at"].as_i64().unwrap(), g["started_at"].as_i64().unwrap(), g["finished_at"].as_i64().unwrap(), g["duration_ms"].as_f64().unwrap());
        assert!(req <= fin && start <= fin && dur >= 500.0 && dur < 20000.0, "timing: {req} {start} {fin} {dur}");
        assert_eq!(g["params"]["size"], p.size);
        assert_eq!(g["params"]["price_reserved_usd"], 0.25);
        assert_eq!(a["style_version"], media.style().version);
        assert_eq!(d["uses"][0]["match_id"], "m1");
        assert_eq!(d["uses"][0]["role"], "requested");
    }
    // The refusal: an asset with no file, a generation with the reason, nothing spent.
    let d = reg.detail(&asset_id_for_media(&r3.hash)).unwrap().unwrap();
    assert_eq!((d["asset"]["status"].as_str(), d["asset"]["path"].as_str(), d["asset"]["subject_kind"].as_str(), d["asset"]["subject_key"].as_str()), (Some("refused"), None, Some("item"), Some("struck oak blade")));
    assert!(d["asset"]["status_reason"].as_str().unwrap().contains("budget"));
    assert_eq!((d["generations"][0]["status"].as_str(), d["generations"][0]["cost_usd"].as_f64(), d["generations"][0]["prompt"].as_str()), (Some("refused"), Some(0.0), Some(p3.text.as_str())));
    assert!(d["generations"][0]["error"].as_str().unwrap().contains("budget"));
    assert_eq!(reg.costs("match", None).unwrap()["items"][0], json!({ "key": "m1", "calls": 3, "ok": 2, "failed": 0, "refused": 1, "pending": 0, "cost_usd": 0.5, "cost_credits": 0,
        "usd_recorded": 0.0, "usd_estimated": 0.0, "ok_without_cost": 0, "duration_ms": reg.costs("match", None).unwrap()["items"][0]["duration_ms"], "avg_duration_ms": reg.costs("match", None).unwrap()["items"][0]["avg_duration_ms"],
        "first_at": reg.costs("match", None).unwrap()["items"][0]["first_at"], "last_at": reg.costs("match", None).unwrap()["items"][0]["last_at"] }));

    // The payload walker: the same images shown on a demigod at beats 4 and 5, then a new look at beat 6.
    let payload = |hash: &str, loadout: Option<&str>| json!({ "demigods": [ { "ref": "dg:0", "name": "Vex (2)", "portrait_prompt": "…",
        "portrait_image": { "hash": hash, "kind": "portrait", "status": "ready", "url": "/media/x.svg", "prompt": "…", "loadout_hash": loadout } },
        { "ref": "dg:1", "name": "Mara", "portrait_image": { "hash": hash_of(0xee), "kind": "portrait", "status": "off", "url": null } } ] });
    hook.saw("m1", 4, None, &payload(&r1.hash, None));
    hook.saw("m1", 4, Some(0), &payload(&r1.hash, None));
    hook.saw("m1", 5, None, &payload(&r1.hash, None));
    hook.saw("m1", 6, None, &payload(&r2.hash, Some("gear-9")));
    hook.flush();
    let uses = reg.with(|c| crate::query::rows(c, "SELECT a.media_hash, u.beat, u.last_beat, u.role, u.subject_ref, u.subject_key, u.loadout_hash FROM uses u JOIN assets a ON a.id = u.asset_id WHERE u.role = 'portrait_image' ORDER BY u.beat", &[])).unwrap();
    assert_eq!(uses, vec![
        json!({ "media_hash": r1.hash, "beat": 4, "last_beat": 5, "role": "portrait_image", "subject_ref": "dg:0", "subject_key": "vex", "loadout_hash": null }),
        json!({ "media_hash": r2.hash, "beat": 6, "last_beat": 6, "role": "portrait_image", "subject_ref": "dg:0", "subject_key": "vex", "loadout_hash": "gear-9" }),
    ]);
    let at5 = reg.at("m1", Some(5), Some("Vex")).unwrap();
    assert!(at5["items"].as_array().unwrap().iter().any(|i| i["media_hash"] == r1.hash.as_str() && i["role"] == "portrait_image"), "{at5}");

    // A second process over the same cache: the image is served from disk, the registry gains a use, not a generation.
    let media2 = media_with(t.path(), MockGenerator { price_usd: 0.25, ..Default::default() }, 5.0);
    media2.set_observer(hook.clone());
    assert_eq!(media2.request(&p1, Some("m2")).status, "ready");
    hook.flush();
    let d = reg.detail(&id1).unwrap().unwrap();
    assert_eq!(d["generations"].as_array().unwrap().len(), 1);
    assert_eq!(d["generations"][0]["requested_match"], "m1", "the match that paid for it");
    assert!(d["uses"].as_array().unwrap().iter().any(|u| u["match_id"] == "m2" && u["role"] == "cached"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_generation_is_recorded_and_a_later_success_replaces_nothing() {
    let t = Temp::new("fail");
    let reg = Arc::new(Registry::open(t.path().join("assets")).unwrap());
    let media = media_with(t.path(), MockGenerator { fail: true, ..Default::default() }, 1.0);
    let hook = LiveHook::for_media(reg.clone(), &media);
    media.set_observer(hook.clone());
    let p = media.prompt(ImageKind::Statue, "a faceted low-poly figure of the demigod Mara (tall), mid-fall").unwrap();
    let r = media.request(&p, Some("m1"));
    assert!(media.wait_idle(Duration::from_secs(10)).await);
    hook.flush();
    let id = asset_id_for_media(&r.hash);
    let d = reg.detail(&id).unwrap().unwrap();
    assert_eq!((d["asset"]["status"].as_str(), d["asset"]["path"].as_str(), d["asset"]["status_reason"].as_str()), (Some("failed"), None, Some("mock generator set to fail")));
    assert_eq!((d["generations"][0]["status"].as_str(), d["generations"][0]["error"].as_str(), d["generations"][0]["cost_usd"].as_f64()), (Some("failed"), Some("mock generator set to fail"), Some(0.0)));
    assert!(d["generations"][0]["finished_at"].is_i64() && d["generations"][0]["duration_ms"].is_number(), "{}", d["generations"][0]);
    // Another match asks again with a generator that works: the same asset becomes ready; both attempts stay on record.
    let media2 = media_with(t.path(), MockGenerator::new(), 1.0);
    media2.set_observer(hook.clone());
    assert_eq!(media2.request(&p, Some("m2")).hash, r.hash);
    assert!(media2.wait_idle(Duration::from_secs(10)).await);
    hook.flush();
    let d = reg.detail(&id).unwrap().unwrap();
    assert_eq!((d["asset"]["status"].as_str(), d["asset"]["status_reason"].as_str()), (Some("ready"), None));
    let mut outcomes: Vec<(&str, &str)> = d["generations"].as_array().unwrap().iter().map(|g| (g["requested_match"].as_str().unwrap(), g["status"].as_str().unwrap())).collect();
    outcomes.sort();
    assert_eq!(outcomes, [("m1", "failed"), ("m2", "ok")]);
}

// ---------------------------------------------------------------------------------------------------------------------
// Recordings
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn recordings_add_beats_references_usage_and_refusals() {
    let t = Temp::new("rec");
    let media = t.path().join("media");
    let cache = MediaCache::new(&media);
    let prompt = "Seen from high above the hex-tile table: Vex casts volcano on Mara; starring Vex (round). Style: x. Match the reference images. No words.";
    let (h1, h2, h3) = (hash_of(0x11), hash_of(0x22), hash_of(0x33));
    cache.store(&png(1536, 1024, "card"), &Sidecar { key: h1.clone(), kind: ImageKind::Event, prompt: prompt.into(), provider: "openai".into(), model: "gpt-image-1-mini".into(),
        ext: "png".into(), bytes: 0, created_at: 1_794_694_253_527, cost_usd: 0.02704, latency_ms: 15006.0, match_id: Some("run-1".into()) }).unwrap();
    // Lines in the core's field order (type first), as its recorder writes them.
    let ev = |id: u64, kind: &str, round: u32, at: i64, data: Value| {
        format!(r#"{{"type":"event","id":{id},"ctx":{{"id":{id},"run":"run-1","hook_id":1,"llm_call_id":null,"round":{round},"step":null,"actor":null}},"kind":"{kind}","actor":null,"at":{at},"data":{data}}}"#)
    };
    let usage = json!({ "input_tokens": 4892, "input_tokens_details": { "image_tokens": 4512, "text_tokens": 380 }, "output_tokens": 1568, "total_tokens": 6460 });
    let http = |attempt: u32, started: i64, status: u16, error: Value, response: Value| {
        let request = json!({ "encoding": "multipart/form-data", "image[]": [ { "bytes": 166511, "file": "IMG_3751.jpg", "sha256": "16".repeat(32) }, { "bytes": 261057, "file": "IMG_3754.jpg", "sha256": "17".repeat(32) } ],
                              "model": "gpt-image-1-mini", "n": 1, "output_format": "png", "prompt": prompt, "quality": "medium", "size": "1536x1024", "background": "transparent" });
        format!(r#"{{"type":"http","sample":0,"attempt":{attempt},"started_at":{started},"duration_ms":14988.9,"url":"https://api.openai.com/v1/images/edits","status":{status},"request":{request},"response":{response},"input_tokens":null,"output_tokens":null,"stop_reason":null,"error":{error},"request_headers":{{"authorization":"Bearer <redacted>"}},"response_headers":{{"x-request-id":"req_{started}"}},"ctx":{{"id":9,"run":"run-1","round":2}}}}"#)
    };
    let big = format!("{{\"type\":\"snapshot\",\"state\":\"{}\"}}", "x".repeat(300_000));
    let lines = [
        big.clone(),
        ev(1, "media_request", 2, 1_794_694_238_514, json!({ "hash": h1, "kind": "event", "model": "openai:gpt-image-1-mini:medium", "price_usd": 0.0282, "prompt": prompt, "provider": "openai", "refs": ["IMG_3751.jpg", "IMG_3754.jpg"] })),
        http(0, 1_794_694_238_535, 400, json!("400 Bad Request: background"), json!({ "error": { "message": "background" } })),
        http(0, 1_794_694_238_900, 200, Value::Null, json!({ "data": [ { "b64_json": "<omitted>" } ], "usage": usage })),
        ev(2, "media_ready", 2, 1_794_694_253_527, json!({ "bytes": 99, "cost_usd": 0.02704, "hash": h1, "kind": "event", "latency_ms": 15006.0, "model": "gpt-image-1-mini", "path": format!("runs/media/{h1}.png"), "provider": "openai", "status": "ready" })),
        ev(3, "media_refused", 3, 1_794_694_260_000, json!({ "hash": h2, "kind": "event", "provider": "openai", "reason": "event image cap reached (3 per match)", "prompt": "Seen from above: another moment. Style: x." })),
        ev(4, "media_cached", 5, 1_794_694_270_000, json!({ "hash": h1, "kind": "event", "provider": "openai", "path": "runs/media/x.png" })),
        ev(5, "media_request", 6, 1_794_694_280_000, json!({ "hash": h3, "kind": "portrait", "model": "openai:gpt-image-1-mini:medium", "price_usd": 0.03, "prompt": "Waist-up: Oko, a demigod: tall. Style: x.", "provider": "openai", "refs": [] })),
        "{ torn line".to_string(),
    ];
    let recs = t.path().join("recordings");
    std::fs::create_dir_all(&recs).unwrap();
    std::fs::write(recs.join("run-1.jsonl"), lines.join("\n") + "\n").unwrap();

    let reg = Registry::open(t.path().join("assets")).unwrap();
    ingest::media_cache(&reg, &media).unwrap();
    let id = asset_id_for_media(&h1);
    assert_eq!(one(&reg, &format!("SELECT cost_basis, usage, requested_beat FROM generations WHERE asset_id = '{id}'")), json!({ "cost_basis": "sidecar", "usage": null, "requested_beat": null }));

    let rep = ingest::recordings(&reg, &recs, Some(&media)).unwrap();
    assert_eq!((rep.files, rep.media_events, rep.http_records, rep.requests, rep.ready, rep.refused, rep.cached, rep.with_http, rep.no_outcome, rep.errors), (1, 5, 2, 2, 1, 1, 1, 1, 1, 0));
    let before = dump(&reg);
    ingest::recordings(&reg, &recs, Some(&media)).unwrap();
    ingest::media_cache(&reg, &media).unwrap();
    assert_eq!(dump(&reg), before, "re-running either backfill, in any order, changes no row");

    // The sidecar's generation, now with what only the recording knew.
    let d = reg.detail(&id).unwrap().unwrap();
    let gens = d["generations"].as_array().unwrap();
    assert_eq!(gens.len(), 2, "the refused-background attempt and the call that made the picture");
    let (first, g) = (&gens[0], &gens[1]);
    assert_eq!((first["status"].as_str(), first["error"].as_str(), first["cost_usd"].as_f64(), first["started_at"].as_i64()), (Some("failed"), Some("400 Bad Request: background"), Some(0.0), Some(1_794_694_238_535)));
    assert_eq!(g["prompt"].as_str().unwrap().as_bytes(), prompt.as_bytes());
    assert_eq!((g["status"].as_str(), g["cost_usd"].as_f64(), g["cost_basis"].as_str(), g["requested_beat"].as_i64(), g["requested_match"].as_str()), (Some("ok"), Some(0.02704), Some("usage"), Some(2), Some("run-1")));
    assert_eq!(g["usage"], usage);
    assert_eq!((g["provider_task_id"].as_str(), g["operation"].as_str(), g["endpoint"].as_str()), (Some("req_1794694238900"), Some("images/edits"), Some("https://api.openai.com/v1/images/edits")));
    assert_eq!((g["requested_at"].as_i64(), g["started_at"].as_i64(), g["duration_ms"].as_f64()), (Some(1_794_694_238_514), Some(1_794_694_238_900), Some(14988.9)));
    assert_eq!((g["params"]["size"].as_str(), g["params"]["quality"].as_str(), g["params"]["background"].as_str(), g["params"]["price_reserved_usd"].as_f64()), (Some("1536x1024"), Some("medium"), Some("transparent"), Some(0.0282)));
    assert!(g["params"].get("prompt").is_none(), "the prompt lives in its own column");
    // The two style references: assets (their files were not at hand: external), lineage, and the ids on the call.
    let refs = g["reference_asset_ids"].as_array().unwrap();
    assert_eq!(refs.len(), 2);
    assert_eq!(d["lineage"]["parents"].as_array().unwrap().len(), 2);
    assert_eq!((d["lineage"]["parents"][0]["role"].as_str(), d["lineage"]["parents"][0]["subkind"].as_str(), d["lineage"]["parents"][0]["status"].as_str()), (Some("reference"), Some("reference"), Some("external")));
    // Uses: asked for at beat 2, served from the cache at beat 5.
    let uses: Vec<(i64, &str)> = d["uses"].as_array().unwrap().iter().map(|u| (u["beat"].as_i64().unwrap(), u["role"].as_str().unwrap())).collect();
    assert_eq!(uses, [(2, "requested"), (5, "cached")]);
    // The refusal left no file, but it is on record with its reason and beat.
    let r = reg.detail(&asset_id_for_media(&h2)).unwrap().unwrap();
    assert_eq!((r["asset"]["status"].as_str(), r["generations"][0]["status"].as_str(), r["generations"][0]["error"].as_str(), r["generations"][0]["requested_beat"].as_i64()),
               (Some("refused"), Some("refused"), Some("event image cap reached (3 per match)"), Some(3)));
    // The request with no outcome stays pending.
    assert_eq!(one(&reg, &format!("SELECT status FROM assets WHERE id = '{}'", asset_id_for_media(&h3)))["status"], "pending");

    // A server restarted mid-match may refuse (a fresh cap count) an image the same match already made: the refusal
    // is a row of its own and never touches what the generation cost.
    std::fs::write(recs.join("run-1.jsonl"), lines.join("\n") + "\n" + &ev(9, "media_refused", 9, 1_794_694_300_000, json!({ "hash": h1, "kind": "event", "provider": "openai", "reason": "event image cap reached (3 per match)", "prompt": prompt })) + "\n").unwrap();
    ingest::recordings(&reg, &recs, Some(&media)).unwrap();
    let d = reg.detail(&id).unwrap().unwrap();
    assert_eq!(d["asset"]["status"], "ready");
    let mut by: Vec<(&str, f64)> = d["generations"].as_array().unwrap().iter().map(|g| (g["status"].as_str().unwrap(), g["cost_usd"].as_f64().unwrap())).collect();
    by.sort_by(|a, b| a.0.cmp(b.0));
    assert_eq!(by, [("failed", 0.0), ("ok", 0.02704), ("refused", 0.0)]);
    assert!(d["generations"].as_array().unwrap().iter().find(|g| g["status"] == "ok").unwrap()["error"].is_null());
    assert_eq!(one(&reg, "SELECT ROUND(SUM(cost_usd), 5) AS usd FROM generations")["usd"], 0.02704);

    // The style references a recording names by hash get their files from a directory that has them.
    let refs_dir = t.path().join("style_refs");
    std::fs::create_dir_all(refs_dir.join("nested")).unwrap();
    std::fs::write(refs_dir.join("nested/IMG_3751.jpg"), b"not the expected bytes").unwrap();
    let rep = ingest::attach_references(&reg, &refs_dir).unwrap();
    assert_eq!((rep.wanted, rep.files_seen, rep.attached), (2, 1, 0), "a file with other bytes is not the reference");
    assert_eq!(n(&reg, "SELECT COUNT(*) FROM assets WHERE subkind = 'reference' AND status = 'external'"), 2);
}

#[test]
fn a_sidecar_that_did_not_know_its_match_is_not_counted_twice() {
    let t = Temp::new("stray");
    let media = t.path().join("media");
    let h = hash_of(0x44);
    let jpg = b"reference bytes".to_vec();
    let ref_sha = files::sha256_bytes(&jpg);
    MediaCache::new(&media).store(&png(8, 8, "x"), &Sidecar { key: h.clone(), kind: ImageKind::Portrait, prompt: PORTRAIT.into(), provider: "openai".into(),
        model: "gpt-image-1-mini".into(), ext: "png".into(), bytes: 0, created_at: 5000, cost_usd: 0.03, latency_ms: 900.0, match_id: None }).unwrap();
    let reg = Registry::open(t.path().join("assets")).unwrap();
    ingest::media_cache(&reg, &media).unwrap();
    let recs = t.path().join("rec");
    std::fs::create_dir_all(&recs).unwrap();
    let ctx = r#"{"id":1,"run":"run-9","hook_id":1,"llm_call_id":null,"round":3,"step":null,"actor":null}"#;
    let lines = [
        format!(r#"{{"type":"event","id":1,"ctx":{ctx},"kind":"media_request","actor":null,"at":4000,"data":{}}}"#, json!({ "hash": h, "kind": "portrait", "model": "openai:gpt-image-1-mini:medium", "price_usd": 0.0302, "prompt": PORTRAIT, "provider": "openai", "refs": ["IMG_1.jpg"] })),
        format!(r#"{{"type":"http","sample":0,"attempt":0,"started_at":4100,"duration_ms":890.0,"url":"https://api.openai.com/v1/images/edits","status":200,"request":{},"response":{{"usage":{{"input_tokens_details":{{"image_tokens":1128,"text_tokens":300}}}}}},"error":null,"request_headers":{{}},"response_headers":{{}},"ctx":{ctx}}}"#,
                json!({ "prompt": PORTRAIT, "size": "1024x1024", "image[]": [ { "file": "IMG_1.jpg", "bytes": jpg.len(), "sha256": ref_sha } ] })),
        format!(r#"{{"type":"event","id":2,"ctx":{ctx},"kind":"media_ready","actor":null,"at":5001,"data":{}}}"#, json!({ "bytes": 1, "cost_usd": 0.03, "hash": h, "kind": "portrait", "latency_ms": 900.0, "model": "gpt-image-1-mini", "path": "runs/media/x.png", "provider": "openai", "status": "ready" })),
    ];
    std::fs::write(recs.join("run-9.jsonl"), lines.join("\n") + "\n").unwrap();
    for _ in 0..2 {
        ingest::recordings(&reg, &recs, None).unwrap();
        ingest::media_cache(&reg, &media).unwrap();
        let g = reg.with(|c| crate::query::rows(c, "SELECT status, cost_usd, cost_basis, requested_match, requested_beat, source FROM generations", &[])).unwrap();
        assert_eq!(g, vec![json!({ "status": "ok", "cost_usd": 0.03, "cost_basis": "usage", "requested_match": "run-9", "requested_beat": 3, "source": "recording" })],
                   "one generation, now with its match, beat and usage; the cost counted once");
    }
    // The reference it was made with: known by hash, then given its file.
    let refs_dir = t.path().join("style_refs");
    std::fs::create_dir_all(&refs_dir).unwrap();
    std::fs::write(refs_dir.join("IMG_1.jpg"), &jpg).unwrap();
    let rep = ingest::attach_references(&reg, &refs_dir).unwrap();
    assert_eq!((rep.wanted, rep.attached), (1, 1));
    let r = one(&reg, "SELECT status, sha256, path, subject_key FROM assets WHERE subkind = 'reference'");
    assert_eq!(r, json!({ "status": "ready", "sha256": ref_sha, "path": format!("images/reference/{ref_sha}.jpg"), "subject_key": "style_ref:img_1.jpg" }));
    assert_eq!(ingest::attach_references(&reg, &refs_dir).unwrap().wanted, 0);
}

// ---------------------------------------------------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------------------------------------------------

struct OneSession;
impl Sessions for OneSession {
    fn me(&self, token: &str) -> Option<Me> {
        (token == "fn7s_good").then(|| Me { account: "acc_1".into(), names: vec!["Mara".into()] })
    }
}

async fn call(app: &axum::Router, req: Request<Body>) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let r = app.clone().oneshot(req).await.unwrap();
    let (parts, body) = r.into_parts();
    (parts.status, parts.headers, body.collect().await.unwrap().to_bytes().to_vec())
}

async fn get(app: &axum::Router, uri: &str) -> (StatusCode, Value) {
    let (s, _, b) = call(app, Request::get(uri).body(Body::empty()).unwrap()).await;
    (s, serde_json::from_slice(&b).unwrap_or(Value::Null))
}

#[tokio::test(flavor = "multi_thread")]
async fn the_routes_list_page_detail_serve_files_and_register() {
    let t = Temp::new("http");
    let media = t.path().join("media");
    let sides = fake_cache(&media);
    let reg = Arc::new(Registry::open(t.path().join("assets")).unwrap());
    ingest::media_cache(&reg, &media).unwrap();
    let cfg = HttpConfig { key: Some("k1".into()), admin_token: Some("admin-1".into()), sessions: Some(Arc::new(OneSession)) };
    let app = crate::http::router(reg.clone(), cfg);

    // The key gates every read.
    for uri in ["/api/assets", "/api/assets?key=wrong", "/api/assets/figures", "/api/assets/costs", "/api/assets/at?match=m", "/api/assets/stats", "/api/assets/x", "/api/assets/x/file"] {
        assert_eq!(call(&app, Request::get(uri).body(Body::empty()).unwrap()).await.0, StatusCode::FORBIDDEN, "{uri}");
    }
    // The list: newest first, paged by cursor, no asset twice or missing.
    let (s, page1) = get(&app, "/api/assets?key=k1&limit=2").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!((page1["total"].as_i64(), page1["items"].as_array().unwrap().len()), (Some(3), 2));
    let next = page1["next"].as_str().unwrap();
    let (_, page2) = get(&app, &format!("/api/assets?key=k1&limit=2&after={next}")).await;
    assert_eq!((page2["items"].as_array().unwrap().len(), page2["next"].is_null()), (1, true));
    let hashes: Vec<String> = page1["items"].as_array().unwrap().iter().chain(page2["items"].as_array().unwrap()).map(|i| i["media_hash"].as_str().unwrap().to_string()).collect();
    assert_eq!(hashes, [hash_of(3), hash_of(2), hash_of(1)]);
    // Filters.
    let total = |v: &Value| v["total"].as_i64().unwrap();
    assert_eq!(total(&get(&app, "/api/assets?key=k1&kind=portrait").await.1), 1);
    assert_eq!(total(&get(&app, "/api/assets?key=k1&kind=image").await.1), 3);
    assert_eq!(total(&get(&app, "/api/assets?key=k1&kind=image/statue&status=ready").await.1), 1);
    assert_eq!(total(&get(&app, "/api/assets?key=k1&subject_kind=demigod&subject=%20VEX").await.1), 1);
    assert_eq!(total(&get(&app, "/api/assets?key=k1&match=match-a").await.1), 2);
    assert_eq!(total(&get(&app, "/api/assets?key=k1&status=failed").await.1), 0);
    assert_eq!(total(&get(&app, &format!("/api/assets?key=k1&media_hash={}", hash_of(2))).await.1), 1);
    // Detail by asset id and by media hash: the same answer, with the exact prompt.
    let id = asset_id_for_media(&hash_of(1));
    let (s, d) = get(&app, &format!("/api/assets/{id}?key=k1")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(d["generations"][0]["prompt"], sides[0].prompt);
    assert_eq!(get(&app, &format!("/api/assets/{}?key=k1", hash_of(1))).await.1, d);
    assert_eq!(get(&app, "/api/assets/ast_nope?key=k1").await.0, StatusCode::NOT_FOUND);
    assert_eq!(get(&app, &format!("/api/assets/{id}/chain?key=k1")).await.1["chain"].as_array().unwrap().len(), 1);

    // The file: whole, by range, conditional; the same bytes the media route serves.
    let want = std::fs::read(media.join(format!("{}.png", hash_of(1)))).unwrap();
    let sha = files::sha256_bytes(&want);
    let uri = format!("/api/assets/{id}/file?key=k1");
    let (s, h, body) = call(&app, Request::get(&uri).body(Body::empty()).unwrap()).await;
    assert_eq!((s, body == want), (StatusCode::OK, true));
    assert_eq!(h["content-type"], "image/png");
    assert_eq!(h["cache-control"], "public, max-age=31536000, immutable");
    assert_eq!(h["etag"], format!("\"{sha}\""));
    assert_eq!(h["accept-ranges"], "bytes");
    assert_eq!(h["content-length"], want.len().to_string());
    assert_eq!(h["x-asset-sha256"], sha);
    let (s, h, body) = call(&app, Request::get(&uri).header("range", "bytes=4-11").body(Body::empty()).unwrap()).await;
    assert_eq!((s, body.as_slice()), (StatusCode::PARTIAL_CONTENT, &want[4..12]));
    assert_eq!(h["content-range"], format!("bytes 4-11/{}", want.len()));
    let (s, _, body) = call(&app, Request::get(&uri).header("range", "bytes=-5").body(Body::empty()).unwrap()).await;
    assert_eq!((s, body.as_slice()), (StatusCode::PARTIAL_CONTENT, &want[want.len() - 5..]));
    let (s, _, body) = call(&app, Request::get(&uri).header("range", format!("bytes={}-", want.len() - 3)).body(Body::empty()).unwrap()).await;
    assert_eq!((s, body.as_slice()), (StatusCode::PARTIAL_CONTENT, &want[want.len() - 3..]));
    let (s, h, _) = call(&app, Request::get(&uri).header("range", "bytes=999999-").body(Body::empty()).unwrap()).await;
    assert_eq!((s, h["content-range"].to_str().unwrap()), (StatusCode::RANGE_NOT_SATISFIABLE, format!("bytes */{}", want.len()).as_str()));
    let (s, _, body) = call(&app, Request::get(&uri).header("if-none-match", format!("\"{sha}\"")).body(Body::empty()).unwrap()).await;
    assert_eq!((s, body.len()), (StatusCode::NOT_MODIFIED, 0));
    // The media hash maps to the same file.
    let (s, _, body) = call(&app, Request::get(format!("/api/assets/{}/file?key=k1", hash_of(1))).body(Body::empty()).unwrap()).await;
    assert_eq!((s, body == want), (StatusCode::OK, true));

    // Costs, stats, at.
    let (_, c) = get(&app, "/api/assets/costs?key=k1&by=provider").await;
    assert_eq!(c["total"]["cost_usd"], 0.0555);
    assert_eq!(get(&app, "/api/assets/costs?key=k1&by=colour").await.0, StatusCode::BAD_REQUEST);
    assert_eq!(get(&app, "/api/assets/costs?key=k1").await.1["by"], "day");
    assert_eq!(get(&app, "/api/assets/stats?key=k1").await.1["assets"]["total"], 3);
    assert_eq!(get(&app, "/api/assets/at?key=k1").await.0, StatusCode::BAD_REQUEST);
    assert_eq!(get(&app, "/api/assets/at?key=k1&match=match-a&beat=3").await.0, StatusCode::OK);

    // Mine: a session, and only that account's names.
    assert_eq!(get(&app, "/api/assets/mine?key=k1").await.0, StatusCode::UNAUTHORIZED);
    let bad = call(&app, Request::get("/api/assets/mine?key=k1").header("authorization", "Bearer nope").body(Body::empty()).unwrap()).await;
    assert_eq!(bad.0, StatusCode::UNAUTHORIZED);
    let (s, _, b) = call(&app, Request::get("/api/assets/mine?key=k1").header("authorization", "Bearer fn7s_good").body(Body::empty()).unwrap()).await;
    let mine: Value = serde_json::from_slice(&b).unwrap();
    assert_eq!((s, mine["total"].as_i64(), mine["items"][0]["subject_key"].as_str(), mine["account"].as_str()), (StatusCode::OK, Some(1), Some("mara"), Some("acc_1")));

    // Register: the admin token, not the key. A statue for Vex, made from the portrait.
    let glb = glb_with_tris(40);
    let body = json!({ "kind": "model", "subkind": "statue", "subject_kind": "demigod", "subject": "Vex", "ext": "glb", "data_base64": b64(&glb),
                       "meta": { "note": "sculpted by hand" }, "lineage": [ { "parent": id, "role": "source" } ],
                       "generation": { "provider": "meshy", "operation": "image-to-3d", "task_id": "t-9", "credits": 30, "prompt": "exact words" } });
    let post = |auth: Option<&str>, body: &Value| {
        let mut r = Request::post("/api/assets/register").header("content-type", "application/json");
        if let Some(a) = auth {
            r = r.header("authorization", format!("Bearer {a}"));
        }
        r.body(Body::from(body.to_string())).unwrap()
    };
    assert_eq!(call(&app, post(None, &body)).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(call(&app, post(Some("k1"), &body)).await.0, StatusCode::UNAUTHORIZED);
    let (s, _, b) = call(&app, post(Some("admin-1"), &body)).await;
    let made: Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(s, StatusCode::CREATED, "{made}");
    assert_eq!((made["asset"]["status"].as_str(), made["asset"]["tris"].as_u64(), made["asset"]["source"].as_str(), made["current"].as_bool()), (Some("ready"), Some(40), Some("operator"), Some(true)));
    assert_eq!((made["generations"][0]["prompt"].as_str(), made["generations"][0]["cost_credits"].as_f64(), made["generations"][0]["requested_by"].as_str()), (Some("exact words"), Some(30.0), Some("operator")));
    assert_eq!(made["lineage"]["parents"][0]["id"], id);
    // Registering the same bytes again is the same asset; other bytes are a new version, and the newest is current.
    let (_, _, b) = call(&app, post(Some("admin-1"), &body)).await;
    assert_eq!(serde_json::from_slice::<Value>(&b).unwrap()["asset"]["id"], made["asset"]["id"]);
    let mut v2 = body.clone();
    v2["data_base64"] = json!(b64(&glb_with_tris(41)));
    v2["lineage"] = json!([]);
    let (_, _, b) = call(&app, post(Some("admin-1"), &v2)).await;
    let second: Value = serde_json::from_slice(&b).unwrap();
    assert_ne!(second["asset"]["id"], made["asset"]["id"]);
    let (_, fig) = get(&app, "/api/assets/figures?key=k1&name=vex").await;
    assert_eq!((fig["statue"]["id"].clone(), fig["statue"]["tris"].as_u64()), (second["asset"]["id"].clone(), Some(41)));
    assert_eq!(total(&get(&app, "/api/assets?key=k1&kind=model/statue").await.1), 2, "the older version is still a row");
    assert_eq!(total(&get(&app, "/api/assets?key=k1&kind=model/statue&current=1").await.1), 1);
    // Bad requests are refused with a reason.
    for (bad, why) in [(json!({ "kind": "video", "subkind": "x", "subject_kind": "demigod", "subject": "Vex", "data_base64": "AA==" }), "kind"),
                       (json!({ "kind": "model", "subkind": "statue", "subject_kind": "demigod", "subject": "Vex" }), "data_base64"),
                       (json!({ "kind": "model", "subkind": "statue", "subject_kind": "demigod", "subject": "Vex", "path": "../../etc/passwd" }), "incoming")] {
        let (s, _, b) = call(&app, post(Some("admin-1"), &bad)).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad}");
        assert!(String::from_utf8_lossy(&b).contains(why), "{}", String::from_utf8_lossy(&b));
    }
    // No admin token configured: the route does not exist.
    let open = crate::http::router(reg.clone(), HttpConfig::default());
    assert_eq!(call(&open, post(Some("admin-1"), &body)).await.0, StatusCode::NOT_FOUND);
    assert_eq!(get(&open, "/api/assets").await.0, StatusCode::OK, "no key configured: open on the LAN, like the profile routes");
}

fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for ch in data.chunks(3) {
        let n = (ch[0] as u32) << 16 | (*ch.get(1).unwrap_or(&0) as u32) << 8 | *ch.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if ch.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if ch.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

//! `fn7-cutout`: remove the backgrounds of the people-and-houses library pictures (sprites, buildings; optionally
//! other kinds) with fal's BiRefNet. Each cutout is a new media entry `<cutout_key(original)>.png` + sidecar in the
//! media dir, and an asset-registry row with its lineage (source = the original). Idempotent; failures are listed
//! and the run goes on. The server serves a cutout instead of its original (`fn7_media::with_cutout`).
//!
//! `fn7-cutout [--env runs/lan/fal.env] [--media runs/media] [--assets runs/lan/fn7/assets] [--kinds sprite,building]
//! [--concurrency 4] [--limit N] [--dry-run]`

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use clap::Parser;
use fn7_assets::live::{apply, MediaRecord, What};
use fn7_assets::{asset_id_for_media, generation_id_for_key, GenerationIn, Registry};
use fn7_media::{cutout_key, MediaCache, Sidecar, CUTOUT_RECIPE};
use futures::StreamExt;
use serde_json::{json, Value};

const MODEL: &str = "fal-ai/birefnet";

#[derive(Parser)]
#[command(about = "Remove the backgrounds of the library pictures with fal BiRefNet (new media entries + registry rows)")]
struct Args {
    /// dotenv file holding FAL_KEY.
    #[arg(long, default_value = "runs/lan/fal.env")]
    env: PathBuf,
    /// The media cache directory.
    #[arg(long, default_value = "runs/media")]
    media: PathBuf,
    /// The asset registry root (holds assets.sqlite).
    #[arg(long, default_value = "runs/lan/fn7/assets")]
    assets: PathBuf,
    /// Image kinds to cut (sprite, building, item, army, …).
    #[arg(long, value_delimiter = ',', default_value = "sprite,building")]
    kinds: Vec<String>,
    #[arg(long, default_value_t = 4)]
    concurrency: usize,
    /// Cut at most this many (0 = all).
    #[arg(long, default_value_t = 0)]
    limit: usize,
    /// List what would be cut, call nothing.
    #[arg(long)]
    dry_run: bool,
    /// fal endpoint base.
    #[arg(long, default_value = "https://fal.run")]
    fal_url: String,
}

struct Done {
    side: Sidecar,
    original: Sidecar,
    request_id: Option<String>,
    started: i64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let a = Args::parse();
    let cache = MediaCache::new(&a.media);
    let todo: Vec<Sidecar> = cache
        .scan()
        .into_iter()
        .filter(|s| a.kinds.iter().any(|k| k == s.kind.as_str()) && s.provider != "fal")
        .filter(|s| cache.lookup(&cutout_key(&s.key)).is_none())
        .take(if a.limit == 0 { usize::MAX } else { a.limit })
        .collect();
    println!("{} to cut ({}) in {}", todo.len(), a.kinds.join(","), a.media.display());
    if a.dry_run || todo.is_empty() {
        for s in &todo {
            println!("  {} {} -> {}", s.kind.as_str(), s.key, cutout_key(&s.key));
        }
        return Ok(());
    }
    dotenvy::from_path(&a.env).with_context(|| format!("loading {}", a.env.display()))?;
    let key = std::env::var("FAL_KEY").map_err(|_| anyhow!("FAL_KEY missing from {}", a.env.display()))?;
    let http = reqwest::Client::builder().timeout(std::time::Duration::from_secs(180)).build()?;
    let reg = Arc::new(Registry::open(&a.assets)?);
    let ctx = Arc::new((http, key, a.fal_url.clone(), cache.clone()));

    let results: Vec<(String, Result<Done>)> = futures::stream::iter(todo)
        .map(|s| {
            let ctx = ctx.clone();
            async move {
                let k = s.key.clone();
                (k, cut(&ctx.0, &ctx.1, &ctx.2, &ctx.3, s).await)
            }
        })
        .buffer_unordered(a.concurrency.max(1))
        .collect()
        .await;

    let (mut ok, mut failed) = (0, vec![]);
    for (k, r) in results {
        match r.and_then(|d| register(&reg, &a.media, &d).map(|_| d)) {
            Ok(d) => {
                ok += 1;
                println!("  ok   {} {} -> {}", d.original.kind.as_str(), k, d.side.key);
            }
            Err(e) => {
                println!("  FAIL {k}: {e:#}");
                failed.push(k);
            }
        }
    }
    println!("done {ok}, failed {} (cost not reported by fal; see the fal dashboard)", failed.len());
    if !failed.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}

async fn cut(http: &reqwest::Client, key: &str, base: &str, cache: &MediaCache, s: Sidecar) -> Result<Done> {
    let started = fn7_assets::now_ms();
    let t0 = Instant::now();
    let bytes = std::fs::read(cache.image_path(&s.key, &s.ext))?;
    let mime = fn7_media::cache::content_type(&s.ext);
    let uri = format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes));
    let resp = http
        .post(format!("{base}/{MODEL}"))
        .header("Authorization", format!("Key {key}"))
        .json(&json!({ "image_url": uri, "output_format": "png" }))
        .send()
        .await?;
    let status = resp.status();
    let request_id = resp.headers().get("x-fal-request-id").and_then(|v| v.to_str().ok()).map(str::to_string);
    let body: Value = resp.json().await.context("fal answer is not JSON")?;
    if !status.is_success() {
        return Err(anyhow!("fal {status}: {}", body.to_string().chars().take(300).collect::<String>()));
    }
    let url = body["image"]["url"].as_str().ok_or_else(|| anyhow!("no image.url in fal answer"))?;
    let png = if let Some(d) = url.strip_prefix("data:") {
        let b64 = d.split_once(',').map(|x| x.1).unwrap_or("");
        base64::engine::general_purpose::STANDARD.decode(b64)?
    } else {
        http.get(url).send().await?.error_for_status()?.bytes().await?.to_vec()
    };
    if png.len() < 8 || &png[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err(anyhow!("fal output is not a PNG"));
    }
    let side = Sidecar {
        key: cutout_key(&s.key),
        kind: s.kind,
        prompt: format!("{CUTOUT_RECIPE} of {}: {}", s.key, s.prompt),
        provider: "fal".into(),
        model: MODEL.into(),
        ext: "png".into(),
        bytes: png.len() as u64,
        created_at: fn7_assets::now_ms(),
        cost_usd: 0.0,
        latency_ms: t0.elapsed().as_secs_f64() * 1000.0,
        match_id: Some("cutout".into()),
    };
    cache.store(&png, &side)?;
    Ok(Done { side, original: s, request_id, started })
}

/// The cutout's asset row (from its sidecar), its generation (fal, requested_by `cutout`) and its lineage.
fn register(reg: &Registry, media: &Path, d: &Done) -> Result<()> {
    let s = &d.side;
    let mut r = MediaRecord::new(What::Sidecar, &s.key, s.kind.as_str(), "fal", "sidecar");
    r.prompt = Some(s.prompt.clone());
    r.at_ms = s.created_at;
    r.file = Some(media.join(format!("{}.{}", s.key, s.ext)));
    r.created_at = Some(s.created_at);
    r.latency_ms = Some(s.latency_ms);
    r.model = Some(MODEL.into());
    r.base = Some(d.original.key.clone());
    let root = reg.root().to_path_buf();
    reg.tx(|c| apply(c, &root, &r))?;
    let child = asset_id_for_media(&s.key);
    let parent = asset_id_for_media(&d.original.key);
    reg.upsert_generation(&GenerationIn {
        id: generation_id_for_key(&format!("cutout:{}", s.key)),
        asset_id: child.clone(),
        provider: "fal".into(),
        model: Some(MODEL.into()),
        operation: Some("remove-background".into()),
        endpoint: Some(format!("https://fal.run/{MODEL}")),
        params: Some(json!({ "recipe": CUTOUT_RECIPE, "output_format": "png" })),
        reference_asset_ids: Some(vec![parent.clone()]),
        provider_task_id: d.request_id.clone(),
        requested_at: Some(d.started),
        started_at: Some(d.started),
        finished_at: Some(s.created_at),
        duration_ms: Some(s.latency_ms),
        cost_basis: Some("not reported by fal".into()),
        status: "ok".into(),
        requested_by: Some("cutout".into()),
        source: "cutout".into(),
        ..Default::default()
    })?;
    if reg.asset_id_by_media(&d.original.key)?.is_some() {
        reg.add_lineage(&parent, &child, "source", Some(CUTOUT_RECIPE))?;
    }
    Ok(())
}

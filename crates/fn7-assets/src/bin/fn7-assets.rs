//! `fn7-assets`: fill and inspect the asset registry (docs/ASSETS.md).
//!
//! ```text
//! fn7-assets [--assets-dir <dir>] ingest [--data-dir <dir>] [--media-dir <dir>] [--profiles <db>] [--recordings <dir>]
//! fn7-assets [--assets-dir <dir>] import-provenance <meshy production dir>
//! fn7-assets [--assets-dir <dir>] import-jobs <jobs.jsonl> [--root <dir>]
//! fn7-assets [--assets-dir <dir>] stats | costs [--by day|kind|provider|match|subject|basis|model] | show <id or media hash> | verify
//! ```
//!
//! The registry is `--assets-dir`, else `FN7_ASSETS_DIR`, else `<THEATRE_DATA_DIR or ./data>/fn7/assets`. Every
//! command only reads its sources; all of them can be re-run. Nothing here calls a provider.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use fn7_assets::{ingest, provenance, Registry};

#[derive(Parser)]
#[command(name = "fn7-assets", about = "The Final Night 7 asset registry: backfill, import and inspect")]
struct Cli {
    /// The assets root (holds assets.sqlite and the file tree).
    #[arg(long, global = true)]
    assets_dir: Option<PathBuf>,
    /// Print JSON only.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Backfill from a data dir: the media cache, the profile library, and (optionally) the recordings.
    Ingest {
        /// The server's THEATRE_DATA_DIR (default: THEATRE_DATA_DIR or ./data). Gives the default profile store.
        #[arg(long)]
        data_dir: Option<PathBuf>,
        /// The media cache (default: FN7_MEDIA_DIR or runs/media).
        #[arg(long)]
        media_dir: Option<PathBuf>,
        /// The profile store (default: <data-dir>/fn7/profiles.sqlite). Opened read-only.
        #[arg(long)]
        profiles: Option<PathBuf>,
        /// A recordings directory or one recording: adds beats, references, parameters, usage, refusals, failures.
        #[arg(long)]
        recordings: Option<PathBuf>,
        /// Skip the media cache step.
        #[arg(long)]
        no_media: bool,
        /// Skip the profile library step.
        #[arg(long)]
        no_profiles: bool,
    },
    /// Import the Unreal repo's Meshy provenance (figures/figures.json, */provenance.json).
    ImportProvenance { dir: PathBuf },
    /// Import a JSON-lines job record file.
    ImportJobs {
        file: PathBuf,
        /// What the paths in the file are relative to (default: the file's directory).
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Counts by kind and status, cost totals, known gaps.
    Stats,
    /// Cost roll-ups.
    Costs {
        #[arg(long, default_value = "provider")]
        by: String,
        #[arg(long)]
        usd_per_credit: Option<f64>,
    },
    /// One asset with its generations, lineage and uses (by asset id or media hash).
    Show { id: String },
    /// Check every stored file against its row: present, the recorded size, the recorded sha256.
    Verify,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let env = |k: &str| std::env::var(k).ok();
    let data = || PathBuf::from(env("THEATRE_DATA_DIR").unwrap_or_else(|| "./data".into()));
    let (data_dir, cmd) = match cli.cmd {
        Cmd::Ingest { data_dir, media_dir, profiles, recordings, no_media, no_profiles } => {
            let d = data_dir.unwrap_or_else(data);
            (d, Cmd::Ingest { data_dir: None, media_dir, profiles, recordings, no_media, no_profiles })
        }
        other => (data(), other),
    };
    let root = cli.assets_dir.unwrap_or_else(|| fn7_assets::default_root(&data_dir, env));
    let reg = Registry::open(&root)?;
    let say = |v: serde_json::Value| println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
    if !cli.json {
        eprintln!("registry: {}", reg.db_path().display());
    }
    match cmd {
        Cmd::Ingest { media_dir, profiles, recordings, no_media, no_profiles, .. } => {
            let mut out = serde_json::Map::new();
            if !no_media {
                let dir = media_dir.clone().or_else(|| env("FN7_MEDIA_DIR").map(PathBuf::from)).unwrap_or_else(|| "runs/media".into());
                if dir.is_dir() {
                    eprintln!("media cache: {}", dir.display());
                    out.insert("media_cache".into(), serde_json::to_value(ingest::media_cache(&reg, &dir)?)?);
                } else {
                    eprintln!("media cache: {} is not a directory, skipped", dir.display());
                }
            }
            if let Some(rec) = &recordings {
                eprintln!("recordings: {}", rec.display());
                let dir = media_dir.clone().or_else(|| env("FN7_MEDIA_DIR").map(PathBuf::from));
                out.insert("recordings".into(), serde_json::to_value(ingest::recordings(&reg, rec, dir.as_deref())?)?);
            }
            if !no_profiles {
                let db = profiles.unwrap_or_else(|| fn7_profile::default_path(&data_dir, env));
                if db.is_file() {
                    eprintln!("profile library: {}", db.display());
                    out.insert("profile_library".into(), serde_json::to_value(ingest::profile_library(&reg, &db)?)?);
                } else {
                    eprintln!("profile library: {} not found, skipped", db.display());
                }
            }
            out.insert("stats".into(), ingest::stats(&reg)?);
            say(out.into());
        }
        Cmd::ImportProvenance { dir } => {
            let rep = provenance::import_provenance(&reg, &dir)?;
            say(serde_json::json!({ "import": rep, "stats": ingest::stats(&reg)? }));
        }
        Cmd::ImportJobs { file, root } => {
            let rep = provenance::import_jobs(&reg, &file, root.as_deref())?;
            say(serde_json::json!({ "import": rep }));
        }
        Cmd::Stats => say(ingest::stats(&reg)?),
        Cmd::Costs { by, usd_per_credit } => say(reg.costs(&by, usd_per_credit)?),
        Cmd::Show { id } => match reg.resolve_id(&id)? {
            Some(id) => say(serde_json::json!({ "detail": reg.detail(&id)?, "chain": reg.chain(&id)? })),
            None => anyhow::bail!("no asset {id}"),
        },
        Cmd::Verify => {
            let rows: Vec<(String, String, i64, String)> = reg.with(|c| {
                let mut st = c.prepare("SELECT id, path, COALESCE(bytes, -1), COALESCE(sha256, '') FROM assets WHERE path IS NOT NULL")?;
                let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<_>>()?;
                Ok(v)
            })?;
            let (mut ok, mut bad) = (0u64, vec![]);
            for (id, path, bytes, sha) in &rows {
                let Some(p) = reg.file_path(path) else {
                    bad.push(format!("{id}: bad path {path}"));
                    continue;
                };
                match fn7_assets::files::sha256_file(&p) {
                    Ok((s, n)) if s == *sha && n as i64 == *bytes => ok += 1,
                    Ok(_) => bad.push(format!("{id}: {path} does not match its row")),
                    Err(e) => bad.push(format!("{id}: {path}: {e}")),
                }
            }
            say(serde_json::json!({ "files": rows.len(), "ok": ok, "bad": bad }));
            if !bad.is_empty() {
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

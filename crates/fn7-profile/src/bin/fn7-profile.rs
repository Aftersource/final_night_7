//! `fn7-profile`: backfill and inspect the profile store.
//!
//! ```text
//! fn7-profile --db <profiles.sqlite> ingest <recording.jsonl | dir>...   # backfill from core recordings (read-only)
//! fn7-profile --db <profiles.sqlite> list                                # every profile with its match count
//! fn7-profile --db <profiles.sqlite> show <name> [--json]                # profile, history and library
//! ```
//!
//! Without `--db`: `FN7_PROFILE_DB`, else `<THEATRE_DATA_DIR or ./data>/fn7/profiles.sqlite`.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use fn7_profile::{default_path, ingest, ProfileStore};

fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut db: Option<PathBuf> = None;
    if let Some(i) = args.iter().position(|a| a == "--db") {
        if i + 1 >= args.len() {
            bail!("--db needs a path");
        }
        db = Some(PathBuf::from(args.remove(i + 1)));
        args.remove(i);
    }
    let json = args.iter().any(|a| a == "--json");
    args.retain(|a| a != "--json");
    let db = db.unwrap_or_else(|| {
        let data = std::env::var("THEATRE_DATA_DIR").unwrap_or_else(|_| "./data".into());
        default_path(Path::new(&data), |k| std::env::var(k).ok())
    });
    let store = ProfileStore::open(&db).with_context(|| format!("open {}", db.display()))?;
    match args.first().map(String::as_str) {
        Some("ingest") => {
            let mut files: Vec<PathBuf> = vec![];
            for a in &args[1..] {
                let p = PathBuf::from(a);
                if p.is_dir() {
                    let mut v: Vec<PathBuf> = std::fs::read_dir(&p)?
                        .filter_map(|e| e.ok().map(|e| e.path()))
                        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
                        .collect();
                    v.sort();
                    files.extend(v);
                } else {
                    files.push(p);
                }
            }
            let (mut ended, mut unfinished, mut skipped) = (0, 0, 0);
            for f in &files {
                let t = std::time::Instant::now();
                let (input, st) = ingest::read_recording(f).with_context(|| format!("read {}", f.display()))?;
                let Some(m) = input else {
                    println!("{}: not a Final Night 7 recording", f.display());
                    skipped += 1;
                    continue;
                };
                let rep = store.record_match(&m)?;
                if m.ended() {
                    ended += 1;
                } else {
                    unfinished += 1;
                }
                println!(
                    "{} {:>9} {:>6.1} MB {:>6} lines {:>4} parsed {:>5} ms  players {:?} images {}{}",
                    &m.match_id[..m.match_id.len().min(8)],
                    if m.ended() { "ended" } else { "unfinished" },
                    st.bytes as f64 / 1e6,
                    st.lines,
                    st.parsed,
                    t.elapsed().as_millis(),
                    rep.players,
                    rep.images,
                    if rep.skipped { " (kept the ended record)" } else { "" }
                );
            }
            println!("{} files: {ended} ended, {unfinished} unfinished, {skipped} skipped -> {}", files.len(), db.display());
        }
        Some("list") => {
            for (key, name, n) in store.list()? {
                println!("{n:>4}  {name}  ({key})");
            }
        }
        Some("show") => {
            let name = args.get(1).context("show <name>")?;
            let Some(p) = store.profile(name)? else {
                bail!("no profile {name:?}");
            };
            let h = store.history(name, 0, Some(100))?;
            let l = store.library(name, 0, Some(100), None)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "profile": p, "history": h, "library": l }))?);
                return Ok(());
            }
            println!("{} ({})", p.display_name, p.key);
            println!("  auto: {}", p.description.auto);
            let t = &p.totals;
            println!(
                "  matches {} (finished {}, unfinished {}), champion {}, best {:?}, avg {:?}, falls {}, nemesis {:?}, images {}",
                t.matches, t.finished, t.unfinished, t.champion, t.best_placement, t.avg_placement, t.falls, t.nemesis, t.images
            );
            for d in &p.demigods {
                println!(
                    "  demigod {} sheet {} portrait {} matches {}",
                    d.name,
                    d.sheet_hash.as_deref().map(|h| &h[..12.min(h.len())]).unwrap_or("-"),
                    d.portrait_hash.as_deref().map(|h| &h[..12.min(h.len())]).unwrap_or("-"),
                    d.matches
                );
            }
            for m in &h.items {
                println!(
                    "  {} {:<10} seat {} {:>2}/{:<2} {} rounds {:?} champions {:?} fell {:?} {:?} items {} deeds {}",
                    &m.match_id[..8],
                    m.outcome,
                    m.seat,
                    m.placement.map(|p| p.to_string()).unwrap_or("-".into()),
                    m.field.map(|p| p.to_string()).unwrap_or("-".into()),
                    if m.champion { "CHAMPION" } else { "" },
                    m.rounds,
                    m.champions,
                    m.eliminated_beat,
                    m.cause,
                    m.items.len(),
                    m.deeds.len()
                );
            }
            let mut kinds: std::collections::BTreeMap<String, usize> = Default::default();
            for i in &l.items {
                *kinds.entry(i.kind.clone()).or_default() += 1;
            }
            println!("  library: {} images {:?}", l.total, kinds);
        }
        _ => bail!("usage: fn7-profile [--db <path>] ingest <files|dirs>... | list | show <name> [--json]"),
    }
    Ok(())
}

//! fn7-project: load core recordings (JSONL, the store of record) into SQLite and project their events into the typed
//! `fn7_*` tables (docs/DATA_MODEL.md §4).
//!
//!   fn7-project --db runs/fn7.sqlite runs/recordings/<match_id>.jsonl   # ingest + project these runs
//!   fn7-project --db runs/fn7.sqlite runs/recordings/                   # every *.jsonl in a directory
//!   fn7-project --db runs/fn7.sqlite                                    # re-project the most recent ingested run
//!   fn7-project --db runs/fn7.sqlite --run <id> | --all

use std::path::PathBuf;

use clap::Parser;
use fn7_tools::{ingest, project};

#[derive(Parser)]
#[command(
    name = "fn7-project",
    about = "Project a Final Night 7 recording into typed SQL tables and views"
)]
struct Args {
    /// The analysis database (SQLite; created if missing). Generic tables + fn7_* projection + views.
    #[arg(long, default_value = "runs/fn7.sqlite")]
    db: String,
    /// Core recordings to ingest first (`<match_id>.jsonl` files, or directories of them); their runs are projected.
    recordings: Vec<PathBuf>,
    /// Project only this run (default: the most recent run).
    #[arg(long, conflicts_with = "all")]
    run: Option<String>,
    /// Project every run in the file.
    #[arg(long)]
    all: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    if let Some(dir) = std::path::Path::new(&args.db).parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let mut conn = rusqlite::Connection::open(&args.db)?;
    conn.busy_timeout(std::time::Duration::from_secs(30))?;
    let mut files = vec![];
    for p in &args.recordings {
        if p.is_dir() {
            let mut in_dir: Vec<PathBuf> = std::fs::read_dir(p)?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|f| f.extension().is_some_and(|x| x == "jsonl"))
                .collect();
            in_dir.sort();
            files.extend(in_dir);
        } else {
            files.push(p.clone());
        }
    }
    let mut ingested = vec![];
    for f in &files {
        let i = ingest::ingest(&mut conn, f)?;
        println!(
            "ingested {} ({} records{}) from {}",
            i.run,
            i.records,
            if i.skipped_lines > 0 { format!(", {} unreadable lines skipped", i.skipped_lines) } else { String::new() },
            f.display()
        );
        ingested.push(i.run);
    }
    let known = project::all_runs(&conn)?;
    let runs: Vec<String> = match (&args.run, args.all) {
        (Some(r), _) => vec![r.clone()],
        (None, true) => known,
        (None, false) if !ingested.is_empty() => ingested,
        (None, false) => known.last().cloned().into_iter().collect(),
    };
    if runs.is_empty() {
        anyhow::bail!("no runs in {} (pass a recording to ingest)", args.db);
    }
    for s in project::project(&mut conn, &runs)? {
        println!(
            "run {}  ({} events; snapshots: {})",
            s.run,
            s.events,
            if s.snapshots.is_empty() {
                "none".into()
            } else {
                s.snapshots.join(", ")
            }
        );
        for (t, n) in &s.counts {
            println!("  {t:<16} {n:>8}");
        }
    }
    println!(
        "views: v_fn7_chain, v_fn7_agent_tree, v_fn7_rumor_path{}",
        if has_beat_cost(&conn) {
            ", v_fn7_beat_cost"
        } else {
            ""
        }
    );
    Ok(())
}

fn has_beat_cost(conn: &rusqlite::Connection) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE name = 'v_fn7_beat_cost'",
        [],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .unwrap_or(false)
}

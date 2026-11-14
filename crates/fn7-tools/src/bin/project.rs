//! fn7-project: project a recording's events into the typed `fn7_*` tables (docs/DATA_MODEL.md §4).
//!
//!   fn7-project --db runs/match.sqlite            # the most recent run
//!   fn7-project --db runs/match.sqlite --run <id>
//!   fn7-project --db runs/match.sqlite --all

use clap::Parser;
use fn7_tools::project;

#[derive(Parser)]
#[command(
    name = "fn7-project",
    about = "Project a Final Night 7 recording into typed SQL tables and views"
)]
struct Args {
    /// The recording (a theatre-recorder SQLite file). Tables are written into the same file.
    #[arg(long)]
    db: String,
    /// Project only this run (default: the most recent run).
    #[arg(long, conflicts_with = "all")]
    run: Option<String>,
    /// Project every run in the file.
    #[arg(long)]
    all: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    if !std::path::Path::new(&args.db).exists() {
        anyhow::bail!("no such recording: {}", args.db);
    }
    let mut conn = rusqlite::Connection::open(&args.db)?;
    conn.busy_timeout(std::time::Duration::from_secs(30))?;
    let known = project::all_runs(&conn)?;
    let runs: Vec<String> = match (&args.run, args.all) {
        (Some(r), _) => vec![r.clone()],
        (None, true) => known,
        (None, false) => known.last().cloned().into_iter().collect(),
    };
    if runs.is_empty() {
        anyhow::bail!("no runs in {}", args.db);
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

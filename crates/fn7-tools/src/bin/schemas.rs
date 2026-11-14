//! fn7-schemas: write the client payload JSON Schemas (docs/schemas), extract real example frames from a recording
//! (docs/examples), and validate recorded frames against the schemas. See docs/CLIENT_API.md.
//!
//!   fn7-schemas                                     # write docs/schemas/*.schema.json
//!   fn7-schemas --check                             # exit 1 if docs/schemas is stale
//!   fn7-schemas --examples-from runs/docs-agents.sqlite --actor Juniper   # write docs/examples/<t>.json
//!   fn7-schemas --validate-db runs/docs-agents.sqlite                      # validate every recorded frame

use std::collections::BTreeMap;
use std::path::PathBuf;

use clap::Parser;
use fn7_tools::schemas;
use serde_json::Value;

#[derive(Parser)]
#[command(
    name = "fn7-schemas",
    about = "Final Night 7 client payload schemas and examples"
)]
struct Args {
    /// Where the schemas go.
    #[arg(long, default_value_os_t = docs().join("schemas"))]
    out: PathBuf,
    /// Only compare the generated schemas with the files in --out.
    #[arg(long)]
    check: bool,
    /// A recording with `protocol_frames` (fn7-agents --record) to extract one example frame per message type from.
    #[arg(long)]
    examples_from: Option<PathBuf>,
    /// The player (protocol_frames.actor) whose frames become the examples; other players fill message types it lacks.
    #[arg(long, default_value = "Juniper")]
    actor: String,
    /// Where the examples go.
    #[arg(long, default_value_os_t = docs().join("examples"))]
    examples_out: PathBuf,
    /// Validate every recorded frame in this recording against its schema (strictly: undocumented keys are errors).
    #[arg(long)]
    validate_db: Option<PathBuf>,
}

fn docs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs")
}

/// Arrays longer than this are cut to `KEEP` items in the examples.
const LONG: usize = 8;
const KEEP: usize = 6;

fn trim(v: &mut Value) {
    match v {
        Value::Array(a) => {
            if a.len() > LONG {
                a.truncate(KEEP);
            }
            a.iter_mut().for_each(trim);
        }
        Value::Object(o) => o.values_mut().for_each(trim),
        _ => {}
    }
}

fn frames(db: &PathBuf) -> anyhow::Result<Vec<(String, String, String, String)>> {
    let conn = rusqlite::Connection::open(db)?;
    let mut st = conn
        .prepare("SELECT actor, direction, frame_type, raw FROM protocol_frames ORDER BY at, id")?;
    let rows = st
        .query_map([], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                r.get(1)?,
                r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                r.get(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let all = schemas::all();

    if args.check {
        let mut stale = vec![];
        for (stem, s) in &all {
            let p = args.out.join(format!("{stem}.schema.json"));
            if std::fs::read_to_string(&p).ok() != Some(schemas::pretty(s)) {
                stale.push(p.display().to_string());
            }
        }
        if !stale.is_empty() {
            eprintln!("stale schemas (run fn7-schemas): {}", stale.join(", "));
            std::process::exit(1);
        }
        println!("{} schemas up to date", all.len());
    } else if args.examples_from.is_none() && args.validate_db.is_none() {
        std::fs::create_dir_all(&args.out)?;
        for (stem, s) in &all {
            let p = args.out.join(format!("{stem}.schema.json"));
            std::fs::write(&p, schemas::pretty(s))?;
            println!("wrote {}", p.display());
        }
    }

    if let Some(db) = &args.examples_from {
        // The biggest frame of each type for the actor (the richest example), then other actors for missing types.
        let mut best: BTreeMap<String, (bool, usize, String)> = BTreeMap::new();
        for (actor, _dir, t, raw) in frames(db)? {
            if t.is_empty() {
                continue;
            }
            let mine = actor == args.actor;
            let better = match best.get(&t) {
                None => true,
                Some((m, len, _)) => (mine && !m) || (mine == *m && raw.len() > *len),
            };
            if better {
                best.insert(t, (mine, raw.len(), raw));
            }
        }
        std::fs::create_dir_all(&args.examples_out)?;
        for (t, (_, _, raw)) in best {
            let mut v: Value = serde_json::from_str(&raw)?;
            trim(&mut v);
            let p = args.examples_out.join(format!("{t}.json"));
            std::fs::write(&p, schemas::pretty(&v))?;
            println!("wrote {}", p.display());
        }
    }

    if let Some(db) = &args.validate_db {
        let by_t: BTreeMap<&str, Vec<(&str, &Value)>> =
            schemas::TARGETS
                .iter()
                .fold(BTreeMap::new(), |mut m, (stem, t, ptr)| {
                    let s = &all.iter().find(|(x, _)| x == stem).expect("schema").1;
                    m.entry(*t).or_insert_with(Vec::new).push((*ptr, s));
                    m
                });
        let (mut n, mut bad) = (0usize, 0usize);
        for (actor, _dir, t, raw) in frames(db)? {
            let Some(targets) = by_t.get(t.as_str()) else {
                continue;
            };
            let v: Value = serde_json::from_str(&raw)?;
            for (ptr, s) in targets {
                n += 1;
                let Some(x) = v.pointer(ptr) else {
                    bad += 1;
                    println!("{actor} {t}: missing {ptr}");
                    continue;
                };
                let errs = schemas::validate(s, x, true);
                if !errs.is_empty() {
                    bad += 1;
                    println!("{actor} {t} {ptr}:\n  {}", errs.join("\n  "));
                }
            }
        }
        println!("validated {n} payloads, {bad} with errors");
        if bad > 0 {
            std::process::exit(1);
        }
    }
    Ok(())
}

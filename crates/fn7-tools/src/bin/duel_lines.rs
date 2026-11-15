//! fn7-duel-lines: what players type in duels, rolled up from recordings (the `duel_line` events of state schema 12;
//! docs/DUEL_MOVES.md), so new animations can be ordered from what the list lacks.
//!
//!   fn7-duel-lines runs/lan/recordings                 # every *.jsonl in a directory
//!   fn7-duel-lines a.jsonl b.jsonl --top 30            # or files
//!   fn7-duel-lines runs/lan/recordings --json          # the rollups as JSON
//!   fn7-duel-lines --list                              # THE LIST (classes, clips, paths, words, version)
//!
//! Reads only. AI fighters' lines are counted apart (`--ai` includes them in the rollups).

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};

use clap::Parser;
use serde_json::{json, Value};

#[derive(Parser)]
#[command(name = "fn7-duel-lines", about = "Roll up the lines typed in duels from recordings")]
struct Args {
    /// Recording files (*.jsonl) or directories of them.
    paths: Vec<String>,
    /// Rows per rollup.
    #[arg(long, default_value_t = 20)]
    top: usize,
    /// Count the AI fighters' own lines too.
    #[arg(long)]
    ai: bool,
    #[arg(long)]
    json: bool,
    /// Print the list the lines are mapped to and exit.
    #[arg(long)]
    list: bool,
}

fn top(m: &BTreeMap<String, u64>, n: usize) -> Vec<(String, u64)> {
    let mut v: Vec<(String, u64)> = m.iter().map(|(k, c)| (k.clone(), *c)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v.truncate(n);
    v
}

fn main() -> anyhow::Result<()> {
    let a = Args::parse();
    if a.list {
        println!("{}", serde_json::to_string_pretty(&fn7_plugin::moves::list())?);
        return Ok(());
    }
    let mut files: Vec<std::path::PathBuf> = vec![];
    for p in &a.paths {
        let p = std::path::PathBuf::from(p);
        if p.is_dir() {
            files.extend(std::fs::read_dir(&p)?.flatten().map(|e| e.path()).filter(|f| f.extension().is_some_and(|x| x == "jsonl")));
        } else {
            files.push(p);
        }
    }
    files.sort();
    let (mut lines, mut unread, mut clips, mut classes, mut words, mut downgraded): (BTreeMap<String, u64>, BTreeMap<String, u64>,
        BTreeMap<String, u64>, BTreeMap<String, u64>, BTreeMap<String, u64>, BTreeMap<String, u64>) = Default::default();
    let (mut human, mut ai, mut matches) = (0u64, 0u64, 0u64);
    let mut seen: std::collections::BTreeSet<(String, String, u64)> = Default::default();
    for f in &files {
        let mut any = false;
        for line in BufReader::new(std::fs::File::open(f)?).lines() {
            let line = line?;
            if !line.contains("\"duel_line\"") {
                continue;
            }
            let Ok(r) = serde_json::from_str::<Value>(&line) else { continue };
            if r["type"] != "event" || r["kind"] != "duel_line" {
                continue;
            }
            let d = &r["data"];
            // A retried resolve records its lines again: count each (match, duel, n) once.
            let key = (f.display().to_string(), d["duel_id"].as_str().unwrap_or("").to_string(), d["n"].as_u64().unwrap_or(0));
            if !seen.insert(key) {
                continue;
            }
            any = true;
            if d["ai"] == true {
                ai += 1;
                if !a.ai {
                    continue;
                }
            } else {
                human += 1;
            }
            let norm = d["normalized"].as_str().unwrap_or("").to_string();
            *lines.entry(norm.clone()).or_default() += 1;
            match d["mapper"]["word"].as_str() {
                Some(w) => *words.entry(w.to_string()).or_default() += 1,
                None => *unread.entry(norm.clone()).or_default() += 1,
            }
            if let (Some(asked), Some(got)) = (d["mapper"]["class"].as_str(), d["class"].as_str()) {
                if asked != got {
                    *downgraded.entry(format!("{asked} -> {got}")).or_default() += 1;
                }
            }
            if let Some(c) = d["class"].as_str() {
                *classes.entry(c.to_string()).or_default() += 1;
            }
            if let Some(c) = d["staged"]["clip"].as_str() {
                *clips.entry(c.to_string()).or_default() += 1;
            }
            for c in d["staged"]["setup"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                *clips.entry(c.to_string()).or_default() += 1;
            }
        }
        matches += any as u64;
    }
    let never: Vec<&str> = fn7_plugin::panel::CLIPS.iter().copied().filter(|c| !clips.contains_key(*c)).collect();
    if a.json {
        let obj = |m: &BTreeMap<String, u64>| top(m, a.top).into_iter().map(|(k, c)| json!({ "key": k, "count": c })).collect::<Vec<_>>();
        println!("{}", serde_json::to_string_pretty(&json!({
            "files": files.len(), "matches_with_lines": matches, "human_lines": human, "ai_lines": ai, "list_version": fn7_plugin::moves::list_version(),
            "most_typed_lines": obj(&lines), "lines_with_no_move_word": obj(&unread), "move_words": obj(&words), "classes": obj(&classes),
            "clip_usage": obj(&clips), "asked_but_became": obj(&downgraded), "clips_never_played_by_a_line": never }))?);
        return Ok(());
    }
    println!("{} recordings, {matches} with duel lines: {human} human lines, {ai} AI lines (list {})", files.len(), fn7_plugin::moves::list_version());
    for (title, m) in [("most typed lines", &lines), ("lines with no move word (what the list lacks)", &unread), ("move words read", &words),
                       ("resolved classes", &classes), ("clip usage (move and setup clips)", &clips), ("asked for, but became (range, bolts, armies)", &downgraded)] {
        println!("\n{title}");
        for (k, c) in top(m, a.top) {
            println!("  {c:6}  {k}");
        }
    }
    println!("\nclips no line has played: {}", never.join(", "));
    Ok(())
}

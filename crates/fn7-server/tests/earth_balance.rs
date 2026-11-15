//! The Earth simulator's balance check (state schema 13; docs/EARTH_SIM.md §9): the same seeds, rules only (no
//! LLM), with `earth_sim_enabled` off and on, through `simapi::stats::run_seed` exactly as `POST /sim/compare` runs
//! them. Ignored (minutes in release):
//!
//! ```sh
//! FN7_BALANCE_SEEDS=128 FN7_BALANCE_BEATS=60 cargo test --release -p fn7-server --test earth_balance -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use fn7_plugin::settings::Settings;
use fn7_server::simapi::stats;
use serde_json::{json, Value};

const EARTH_CAUSES: [&str; 7] = ["wildfire", "lava", "flood", "quake", "storm", "blizzard", "swarm"];

fn env(k: &str, d: u64) -> u64 {
    std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

fn arm(earth: bool, seeds: &[u64], beats: u64, workers: usize) -> Vec<Value> {
    let queue = Arc::new(Mutex::new(seeds.to_vec()));
    let out = Arc::new(Mutex::new(vec![]));
    let threads: Vec<_> = (0..workers)
        .map(|_| {
            let (queue, out) = (queue.clone(), out.clone());
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
                loop {
                    let Some(seed) = queue.lock().unwrap().pop() else { break };
                    let s = Settings::validate(&json!({ "seed": seed, "beats_total": beats, "llm": false, "earth_sim_enabled": earth })).unwrap();
                    let r = rt.block_on(stats::run_seed(&s, 4, Arc::new(fn7_plugin::testkit::Failing), None))
                        .unwrap_or_else(|e| json!({ "seed": seed, "error": format!("{e:#}") }));
                    out.lock().unwrap().push(r);
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let mut v = std::mem::take(&mut *out.lock().unwrap());
    v.sort_by_key(|r| r["seed"].as_u64());
    v
}

struct Sum {
    n: usize,
    beats: f64,
    early_end: usize,
    first_elim: Vec<f64>,
    elims: f64,
    heat_death: usize,
    champs: BTreeMap<String, usize>,
    earth_kills: usize,
    errors: usize,
}

fn sum(rs: &[Value]) -> Sum {
    let mut s = Sum { n: 0, beats: 0.0, early_end: 0, first_elim: vec![], elims: 0.0, heat_death: 0, champs: BTreeMap::new(), earth_kills: 0, errors: 0 };
    for r in rs {
        if r.get("error").is_some() {
            s.errors += 1;
            eprintln!("seed {}: {}", r["seed"], r["error"]);
            continue;
        }
        s.n += 1;
        let played = r["beats_played"].as_f64().unwrap_or(0.0);
        s.beats += played;
        if r["ended"] == "last_standing" && played < r["beats_total"].as_f64().unwrap_or(0.0) {
            s.early_end += 1;
        }
        if let Some(f) = r["first_elimination_beat"].as_f64() {
            s.first_elim.push(f);
        }
        s.elims += r["eliminations_count"].as_f64().unwrap_or(0.0);
        if !r["entropy"]["heat_death_beat"].is_null() {
            s.heat_death += 1;
        }
        let c = match &r["champion"] {
            Value::Null => "none".to_string(),
            c => c["seat"].as_u64().map(|x| format!("seat {x}")).unwrap_or_else(|| "ai".into()),
        };
        *s.champs.entry(c).or_default() += 1;
        for e in r["eliminations"].as_array().into_iter().flatten() {
            let cause = e["cause"].as_str().unwrap_or("");
            if EARTH_CAUSES.iter().any(|k| cause.contains(k)) {
                s.earth_kills += 1;
            }
        }
    }
    s
}

#[test]
#[ignore]
fn earth_on_and_off_over_many_seeds() {
    let (n, beats) = (env("FN7_BALANCE_SEEDS", 128), env("FN7_BALANCE_BEATS", 60));
    let workers = env("FN7_BALANCE_WORKERS", 8) as usize;
    let seeds: Vec<u64> = (1..=n).collect();
    let t = std::time::Instant::now();
    let off = arm(false, &seeds, beats, workers);
    let t_off = t.elapsed().as_secs_f64();
    let on = arm(true, &seeds, beats, workers);
    let t_on = t.elapsed().as_secs_f64() - t_off;
    let mut out = String::new();
    for (name, rs, secs) in [("earth off", &off, t_off), ("earth on", &on, t_on)] {
        let s = sum(rs);
        let k = s.n.max(1) as f64;
        let fe = if s.first_elim.is_empty() { f64::NAN } else { s.first_elim.iter().sum::<f64>() / s.first_elim.len() as f64 };
        out += &format!("{name}: {} seeds x {beats} beats ({} errors, {secs:.0} s): mean beats played {:.1}; world ended early (last standing) {} ({:.0}%); \
                         first elimination beat mean {fe:.1} ({} seeds had one); eliminations per match {:.2}; heat death in {} seeds; \
                         eliminations the field caused {}; champions {:?}\n",
                        s.n, s.errors, s.beats / k, s.early_end, 100.0 * s.early_end as f64 / k, s.first_elim.len(), s.elims / k, s.heat_death, s.earth_kills, s.champs);
    }
    // Same seeds, paired: how often the champion changed.
    let changed = off.iter().zip(&on).filter(|(a, b)| a["champion"]["seat"] != b["champion"]["seat"]).count();
    out += &format!("champion differs on {changed} of {} paired seeds\n", seeds.len());
    eprintln!("{out}");
    let (a, b) = (sum(&off), sum(&on));
    assert_eq!((a.errors, b.errors), (0, 0));
    // The field must not end the world early much more often, nor shorten matches by more than a tenth.
    assert!(b.early_end <= a.early_end + seeds.len() / 10, "{out}");
    assert!(b.beats >= a.beats * 0.9, "{out}");
}

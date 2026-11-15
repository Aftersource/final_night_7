//! Fault junctions and tiers' balance check (state schema 16; DM_RULEBOOK §13): the same seeds, rules only (no LLM),
//! under the live house fault curve, with `fault_junctions_enabled` off (the schema-12 rule) and on, through
//! `simapi::stats::run_seed` exactly as `POST /sim/compare` runs them. `FN7_FAULT_EXTRA` (a JSON object) adds settings
//! to both arms (e.g. a counterweight). Ignored (minutes in release):
//!
//! ```sh
//! FN7_BALANCE_SEEDS=128 FN7_BALANCE_BEATS=60 cargo test --release -p fn7-server --test fault_balance -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use fn7_plugin::settings::Settings;
use fn7_server::simapi::stats;
use serde_json::{json, Value};

fn env(k: &str, d: u64) -> u64 {
    std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

/// The live house settings (players 4, beats 60, the steep fault curve).
fn house(seed: u64, beats: u64, junctions: bool) -> Value {
    let mut s = json!({ "seed": seed, "beats_total": beats, "llm": false, "players": 4,
        "fault_near_km": 200, "fault_far_km": 1000, "fault_near_magnitude": 5, "fault_far_magnitude": -4,
        "fault_near_radius": 2.2, "fault_far_radius": 0.4, "fault_near_damage": 2.5, "fault_far_damage": 0.25,
        "fault_junctions_enabled": junctions });
    if let Ok(x) = std::env::var("FN7_FAULT_EXTRA") {
        for (k, v) in serde_json::from_str::<Value>(&x).expect("FN7_FAULT_EXTRA json").as_object().unwrap() {
            s[k] = v.clone();
        }
    }
    Settings::validate(&s).unwrap()
}

fn arm(junctions: bool, seeds: &[u64], beats: u64, workers: usize) -> Vec<Value> {
    let queue = Arc::new(Mutex::new(seeds.to_vec()));
    let out = Arc::new(Mutex::new(vec![]));
    let threads: Vec<_> = (0..workers)
        .map(|_| {
            let (queue, out) = (queue.clone(), out.clone());
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
                loop {
                    let Some(seed) = queue.lock().unwrap().pop() else { break };
                    let r = rt.block_on(stats::run_seed(&house(seed, beats, junctions), 4, Arc::new(fn7_plugin::testkit::Failing), None))
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

#[derive(Default)]
struct Sum {
    n: usize,
    errors: usize,
    beats: f64,
    early_end: usize,
    heat_death: usize,
    early_heat_death: usize,
    heat_beats: Vec<f64>,
    first_elim: Vec<f64>,
    elims: f64,
    champs: BTreeMap<String, usize>,
    tiers: BTreeMap<String, u64>,
    megas: f64,
}

fn sum(rs: &[Value], beats: u64) -> Sum {
    let mut s = Sum::default();
    for r in rs {
        if r.get("error").is_some() {
            s.errors += 1;
            eprintln!("seed {}: {}", r["seed"], r["error"]);
            continue;
        }
        s.n += 1;
        let played = r["beats_played"].as_f64().unwrap_or(0.0);
        s.beats += played;
        if played < beats as f64 {
            s.early_end += 1;
        }
        if let Some(h) = r["entropy"]["heat_death_beat"].as_f64() {
            s.heat_death += 1;
            s.heat_beats.push(h);
            if h < beats as f64 * 0.75 {
                s.early_heat_death += 1;
            }
        }
        if let Some(f) = r["first_elimination_beat"].as_f64() {
            s.first_elim.push(f);
        }
        s.elims += r["eliminations_count"].as_f64().unwrap_or(0.0);
        let c = match &r["champion"] {
            Value::Null => "none".to_string(),
            c => c["seat"].as_u64().map(|x| format!("seat {x}")).unwrap_or_else(|| "ai".into()),
        };
        *s.champs.entry(c).or_default() += 1;
        for (k, v) in r["casts"]["by_fault_tier"].as_object().into_iter().flatten() {
            *s.tiers.entry(k.clone()).or_default() += v.as_u64().unwrap_or(0);
        }
        s.megas += r["casts"]["count"].as_f64().unwrap_or(0.0);
    }
    s
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() { f64::NAN } else { v.iter().sum::<f64>() / v.len() as f64 }
}

/// Seat fairness: the largest share of wins any seat (or "none"/"ai") took, and a chi-square over seats 0..3.
fn fairness(c: &BTreeMap<String, usize>) -> (f64, f64) {
    let seats: Vec<f64> = (0..4).map(|i| *c.get(&format!("seat {i}")).unwrap_or(&0) as f64).collect();
    let n: f64 = seats.iter().sum();
    let e = n / 4.0;
    let chi = if e > 0.0 { seats.iter().map(|x| (x - e).powi(2) / e).sum() } else { 0.0 };
    (seats.iter().copied().fold(0.0, f64::max) / n.max(1.0), chi)
}

#[test]
#[ignore]
fn junctions_off_and_on_over_many_seeds() {
    let (n, beats) = (env("FN7_BALANCE_SEEDS", 128), env("FN7_BALANCE_BEATS", 60));
    let workers = env("FN7_BALANCE_WORKERS", 8) as usize;
    let seeds: Vec<u64> = (1..=n).collect();
    let t = std::time::Instant::now();
    let off = arm(false, &seeds, beats, workers);
    let t_off = t.elapsed().as_secs_f64();
    let on = arm(true, &seeds, beats, workers);
    let t_on = t.elapsed().as_secs_f64() - t_off;
    let mut out = String::new();
    for (name, rs, secs) in [("junctions off (house curve)", &off, t_off), ("junctions on (schema 16)", &on, t_on)] {
        let s = sum(rs, beats);
        let k = s.n.max(1) as f64;
        let (top, chi) = fairness(&s.champs);
        out += &format!(
            "{name}: {} seeds x {beats} beats ({} errors, {secs:.0} s): mean beats played {:.1}; ended early {} ({:.0}%); \
             heat death {} (mean beat {:.1}; before beat {}: {}); first elimination beat mean {:.1} ({} seeds); eliminations/match {:.2}; \
             casts/match {:.1}; mega tiers {:?}; champions {:?} (top seat share {:.0}%, chi2 over seats {:.2}, 3 dof: 7.81 = p .05)\n",
            s.n, s.errors, s.beats / k, s.early_end, 100.0 * s.early_end as f64 / k, s.heat_death, mean(&s.heat_beats),
            beats * 3 / 4, s.early_heat_death, mean(&s.first_elim), s.first_elim.len(), s.elims / k, s.megas / k, s.tiers, s.champs,
            100.0 * top, chi);
    }
    let changed = off.iter().zip(&on).filter(|(a, b)| a["champion"]["seat"] != b["champion"]["seat"]).count();
    out += &format!("champion differs on {changed} of {} paired seeds\n", seeds.len());
    eprintln!("{out}");
    let (a, b) = (sum(&off, beats), sum(&on, beats));
    assert_eq!((a.errors, b.errors), (0, 0));
    // The off arm never tiers a mega. (At the house settings the built-in AI rarely holds the 2 bolts it wants before
    // a mega, so it may cast none: `FN7_FAULT_EXTRA='{"bolt_start": 6}'` stresses the megas.)
    assert!(a.tiers.is_empty(), "{out}");
}

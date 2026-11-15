//! One simulated match (one seed) through the `GamePlugin` trait, as `fn7-sim` runs it, reduced to balance numbers;
//! and the aggregates over seeds (`mean` / `median` / `min` / `max` of every numeric field) for `/sim/run` and
//! `/sim/compare`.
//!
//! Everything in a seed's result except `timing` and `recording` is a pure function of (settings, seed, players)
//! when the LLM is off: the determinism test compares exactly that.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use fn7_model::DebtStatus;
use fn7_plugin::driver::{run_match, BeatReport};
use fn7_plugin::FinalNight7Plugin;
use serde_json::{json, Map, Value};
use theatre_plugin_api::*;
use theatre_server::recording::Recorder;

/// Seat names (all seats are bots), as in `fn7-sim`.
const SEAT_NAMES: &[&str] = &["Ash", "Brix", "Cyra", "Dune", "Ember", "Fable", "Grit", "Hex"];

/// Most points kept in a seed's `entropy.curve`.
pub const CURVE_POINTS: usize = 40;

/// Counts the calls one seed makes (its share of a job's LLM use).
pub struct Counted {
    pub inner: Arc<dyn LlmProvider>,
    pub calls: AtomicU64,
}

#[async_trait]
impl LlmProvider for Counted {
    async fn complete(&self, req: LlmRequest) -> anyhow::Result<LlmResponse> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.inner.complete(req).await
    }
}

fn dist(mut v: Vec<f64>) -> Value {
    if v.is_empty() {
        return json!({ "n": 0 });
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    let mean = v.iter().sum::<f64>() / n as f64;
    let pct = |p: f64| v[(((n as f64) * p) as usize).min(n - 1)];
    json!({ "n": n, "mean": round3(mean), "p50": round3(pct(0.5)), "p95": round3(pct(0.95)), "max": round3(v[n - 1]) })
}

/// A coarse kind for an elimination cause (causes are prose that names people): `debt`, `arena_clock`, `accident`,
/// `army`, `final_night`, `god_power`, `bolt`, `entropy`, `mod` or `other`. The prose stays in `eliminations[].cause`.
pub fn cause_kind(cause: &str) -> &'static str {
    let c = cause.to_lowercase();
    let has = |k: &[&str]| k.iter().any(|x| c.contains(x));
    if has(&["debt", "consequence"]) {
        "debt"
    } else if has(&["outside the arena", "time's clock"]) {
        "arena_clock"
    } else if has(&["final destination", "accident"]) {
        "accident"
    } else if has(&["overrun", "army", "host of", "volunteers"]) {
        "army"
    } else if has(&["final night"]) {
        "final_night"
    } else if has(&["god power"]) {
        "god_power"
    } else if has(&["lightning", "bolt"]) {
        "bolt"
    } else if has(&["entropy", "heat death"]) {
        "entropy"
    } else if has(&["storm", "flood", "quake", "fire", "plague"]) {
        "mod"
    } else {
        "other"
    }
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// Run one match. `settings` are validated room settings with `seed` set; `recorder` records it in the core's JSONL
/// format (returns the run id in `recording.run_id`).
pub async fn run_seed(
    settings: &Value,
    players: u8,
    llm: Arc<dyn LlmProvider>,
    recorder: Option<&Recorder>,
) -> anyhow::Result<Value> {
    let wall = Instant::now();
    let seats: Vec<SeatInfo> = (0..players)
        .map(|i| SeatInfo {
            seat: i,
            display_name: SEAT_NAMES[i as usize % SEAT_NAMES.len()].into(),
            is_bot: true,
        })
        .collect();
    let counted = Arc::new(Counted {
        inner: llm,
        calls: AtomicU64::new(0),
    });
    let run_id = uuid::Uuid::new_v4().to_string();
    let (p, llm): (Box<dyn GamePlugin>, Arc<dyn LlmProvider>) = match recorder {
        Some(r) => (
            Box::new(crate::record::Recorded::new(FinalNight7Plugin::new(), r.run(run_id.clone()))),
            crate::record::traced(counted.clone()),
        ),
        None => (Box::new(FinalNight7Plugin::new()), counted.clone()),
    };

    let mut entropy: Vec<(u32, f64)> = vec![];
    let mut heat_death: Option<u32> = None;
    let mut beat_ms: Vec<f64> = vec![];
    let mut society_ms: Vec<f64> = vec![];
    let mut plugin_llm_calls = 0u64;
    let mut plugin_llm_fallbacks = 0u64;
    let mut casts = 0u64;
    let mut fizzled = 0u64;
    let mut bolts_spent = 0f64;
    let mut by_power: BTreeMap<String, u64> = BTreeMap::new();
    // State schema 16: megas by fault tier (absent before it).
    let mut by_fault_tier: BTreeMap<String, u64> = BTreeMap::new();
    let mut windows = 0u64;
    let mut open_ms: Vec<f64> = vec![];
    let mut timeouts = 0u64;
    let mut instant = 0u64;
    let mut slow_debt = 0f64;
    let mut paid_bolt = 0u64;
    let mut beats = 0u32;

    let (state, summary) = run_match(p.as_ref(), &seats, settings, llm.as_ref(), None, &|_, _| None, |r: &BeatReport, _s: &Value| {
        beats = r.round;
        let pubv = &r.outcome.public;
        if let Some(v) = pubv["entropy"]["value"].as_f64() {
            entropy.push((r.round, v));
        }
        if heat_death.is_none() && !pubv["entropy"]["heat_death"].is_null() && pubv["entropy"].is_object() {
            heat_death = Some(r.round);
        }
        let tm = &pubv["timing"];
        beat_ms.push(tm["total_ms"].as_f64().unwrap_or(0.0));
        society_ms.push(tm["society_ms"].as_f64().unwrap_or(0.0));
        plugin_llm_calls += tm["llm_calls"].as_u64().unwrap_or(0);
        plugin_llm_fallbacks += tm["llm_fallbacks"].as_u64().unwrap_or(0);
        for c in pubv["casts"].as_array().into_iter().flatten() {
            if c.get("fizzled").is_some_and(|f| !f.is_null() && f != &Value::Bool(false)) {
                fizzled += 1;
                continue;
            }
            casts += 1;
            bolts_spent += c["cost"].as_f64().unwrap_or(0.0);
            *by_power.entry(c["power"].as_str().unwrap_or("?").to_string()).or_default() += 1;
            if let Some(t) = c["fault_tier"].as_str() {
                *by_fault_tier.entry(t.to_string()).or_default() += 1;
            }
        }
        for s in pubv["slow"].as_array().into_iter().flatten() {
            match s["event"].as_str() {
                Some("closed") => {
                    windows += 1;
                    open_ms.push(s["after_ms"].as_f64().unwrap_or(0.0));
                    if s["how"] == "timeout" {
                        timeouts += 1;
                    }
                }
                Some("instant") => instant += 1,
                _ => {}
            }
            slow_debt += s["debt"].as_f64().unwrap_or(0.0);
            if s["paid_bolt"] == true {
                paid_bolt += 1;
            }
        }
        true
    })
    .await?;

    let w = fn7_plugin::world::load(&state)?;

    // Eliminations (from the final world: beat and cause per demigod).
    let mut elims: Vec<Value> = vec![];
    let mut by_cause: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_kind: BTreeMap<&str, u64> = BTreeMap::new();
    let mut by_beat: BTreeMap<String, u64> = BTreeMap::new();
    let mut ds: Vec<&fn7_model::Demigod> = w.demigods.iter().filter(|d| !d.alive).collect();
    ds.sort_by_key(|d| (d.eliminated_beat, d.id));
    for d in &ds {
        let cause = d.cause.clone().unwrap_or_else(|| "unknown".into());
        *by_cause.entry(cause.clone()).or_default() += 1;
        *by_kind.entry(cause_kind(&cause)).or_default() += 1;
        if let Some(b) = d.eliminated_beat {
            *by_beat.entry(b.to_string()).or_default() += 1;
        }
        elims.push(json!({ "beat": d.eliminated_beat, "name": d.name, "seat": (d.seat < fn7_plugin::SYNTHETIC_SEAT).then_some(d.seat), "cause_kind": cause_kind(&cause), "cause": cause }));
    }
    let first_elim = ds.iter().filter_map(|d| d.eliminated_beat).min();

    // Debts (Consequence's bills and the gods' prices) by status.
    let (mut paid, mut paid_amount, mut open, mut delayed, mut owed) = (0u64, 0f64, 0u64, 0u64, 0f64);
    for d in w.ledger.rows.iter().filter_map(|r| r.debt.as_ref()) {
        match d.status {
            DebtStatus::Collected | DebtStatus::Settled => {
                paid += 1;
                paid_amount += d.amount as f64;
            }
            DebtStatus::Open => {
                open += 1;
                owed += d.amount as f64;
            }
            DebtStatus::Delayed => {
                delayed += 1;
                owed += d.amount as f64;
            }
        }
    }

    // Society.
    let mut emerged: BTreeMap<String, u64> = BTreeMap::new();
    let mut emerged_dead = 0u64;
    for c in &w.characters {
        if let Some(l) = c.life.as_deref() {
            let role = serde_json::to_value(l.emerged_as)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_else(|| "unknown".into());
            *emerged.entry(role).or_default() += 1;
            if !c.alive {
                emerged_dead += 1;
            }
        }
    }
    let mut society = match serde_json::to_value(&w.soc)? {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    society.insert("mobs_ever".into(), w.mobs.len().into());
    society.insert("people_emerged_by_role".into(), json!(emerged));
    society.insert("people_emerged_dead".into(), emerged_dead.into());
    let arena_ix = w.meta.arena.ix();
    let (base, pop) = w
        .nodes
        .get(arena_ix)
        .map(|n| (n.base, n.population))
        .unwrap_or((0, 0));
    society.insert("arena_population_start".into(), base.into());
    society.insert("arena_population_end".into(), pop.into());

    // Entropy curve, sampled to at most CURVE_POINTS points (always keeping the last).
    let step = entropy.len().div_ceil(CURVE_POINTS).max(1);
    let curve: Vec<Value> = entropy
        .iter()
        .enumerate()
        .filter(|(i, _)| i % step == 0 || *i + 1 == entropy.len())
        .map(|(_, (b, v))| json!([b, round3(*v)]))
        .collect();
    let e_final = entropy.last().map(|x| round3(x.1));
    let e_max = entropy.iter().map(|x| x.1).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))));

    let champions = summary.summary["champions"].clone();
    let champion = champions.get(0).cloned().unwrap_or(Value::Null);
    let alive_end = w.alive_demigods().count();
    let ended = if alive_end <= 1 {
        "last_standing"
    } else if beats >= w.meta.beats_total {
        "final_night"
    } else {
        "stopped"
    };
    let bolts_end: u64 = w.demigods.iter().map(|d| d.bolts as u64).sum();

    let mut out = json!({
        "seed": w.meta.seed,
        "arena": w.places.get(arena_ix).map(|p| p.name.clone()),
        "champion": champion,
        "champions": champions,
        "beats_played": beats,
        "beats_total": w.meta.beats_total,
        "ended": ended,
        "alive_end": alive_end,
        "eliminations": elims,
        "eliminations_count": ds.len(),
        "first_elimination_beat": first_elim,
        "eliminations_by_cause_kind": by_kind,
        "eliminations_by_cause": by_cause,
        "eliminations_by_beat": by_beat,
        "casts": { "count": casts, "fizzled": fizzled, "bolts_spent": bolts_spent, "by_power": by_power },
        "bolts_end_total": bolts_end,
        "entropy": { "final": e_final, "max": e_max.map(round3), "heat_death_beat": heat_death, "curve": curve },
        "slow": { "windows": windows, "mean_open_ms": if open_ms.is_empty() { Value::Null } else { json!(round3(open_ms.iter().sum::<f64>() / open_ms.len() as f64)) },
                  "timeouts": timeouts, "instant_orders": instant, "debt_billed": round3(slow_debt), "paid_with_bolt": paid_bolt },
        "debts": { "paid": paid, "paid_amount": round3(paid_amount), "open": open, "delayed": delayed, "owed_amount": round3(owed) },
        "society": society,
        "llm": { "plugin_calls": plugin_llm_calls, "plugin_fallbacks": plugin_llm_fallbacks, "provider_calls": counted.calls.load(Ordering::Relaxed) },
        "timing": { "wall_ms": round3(wall.elapsed().as_secs_f64() * 1000.0), "beat_ms": dist(beat_ms), "society_ms": dist(society_ms) },
    });
    if !by_fault_tier.is_empty() {
        out["casts"]["by_fault_tier"] = json!(by_fault_tier);
    }
    if let Some(r) = recorder {
        r.flush();
        out["recording"] = json!({ "run_id": run_id, "file": format!("{run_id}.jsonl") });
    }
    Ok(out)
}

/// A seed's result without the fields that depend on wall-clock time or file names (what determinism compares).
pub fn deterministic_part(r: &Value) -> Value {
    let mut r = r.clone();
    if let Some(o) = r.as_object_mut() {
        o.remove("timing");
        o.remove("recording");
    }
    r
}

/// Numeric leaves as dotted paths. Count maps (`*by_*`) are flattened with their keys; arrays are skipped.
fn leaves(prefix: &str, v: &Value, out: &mut BTreeMap<String, f64>) {
    match v {
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                out.insert(prefix.to_string(), f);
            }
        }
        Value::Bool(b) => {
            out.insert(prefix.to_string(), if *b { 1.0 } else { 0.0 });
        }
        Value::Object(m) => {
            for (k, x) in m {
                let p = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                leaves(&p, x, out);
            }
        }
        _ => {}
    }
}

fn skip_key(k: &str) -> bool {
    // Raw causes name people, so they don't aggregate as stats (see `eliminations_by_cause_total`).
    k == "seed" || k.starts_with("eliminations_by_cause.") || k.starts_with("champion.") || k.starts_with("recording") || k.starts_with("eliminations_by_beat")
}

/// Count-map keys: a seed without the key counted zero of it.
fn zero_when_missing(k: &str) -> bool {
    k.contains("by_")
}

/// Aggregate stats over seed results (errors skipped): every numeric field's `{n, mean, median, min, max}`, plus
/// who won (`champion_seats`), how matches ended and total eliminations by cause.
pub fn aggregate(results: &[Value]) -> Value {
    let ok: Vec<&Value> = results.iter().filter(|r| r.get("error").is_none()).collect();
    let flat: Vec<BTreeMap<String, f64>> = ok
        .iter()
        .map(|r| {
            let mut m = BTreeMap::new();
            leaves("", r, &mut m);
            m
        })
        .collect();
    let mut keys: Vec<String> = flat.iter().flat_map(|m| m.keys().cloned()).collect();
    keys.sort();
    keys.dedup();
    let mut stats = Map::new();
    for k in keys.iter().filter(|k| !skip_key(k)) {
        let mut v: Vec<f64> = flat
            .iter()
            .filter_map(|m| m.get(k).copied().or(zero_when_missing(k).then_some(0.0)))
            .collect();
        if v.is_empty() {
            continue;
        }
        v.sort_by(|a, b| a.total_cmp(b));
        let n = v.len();
        let median = if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 };
        stats.insert(
            k.clone(),
            json!({ "n": n, "mean": round3(v.iter().sum::<f64>() / n as f64), "median": round3(median), "min": round3(v[0]), "max": round3(v[n - 1]) }),
        );
    }
    let mut champs: BTreeMap<String, u64> = BTreeMap::new();
    let mut ended: BTreeMap<String, u64> = BTreeMap::new();
    let mut causes: BTreeMap<String, u64> = BTreeMap::new();
    for r in &ok {
        let c = match &r["champion"] {
            Value::Null => "none".to_string(),
            c => c["seat"].as_u64().map(|s| format!("seat {s}")).unwrap_or_else(|| "ai".into()),
        };
        *champs.entry(c).or_default() += 1;
        *ended.entry(r["ended"].as_str().unwrap_or("?").to_string()).or_default() += 1;
        for (k, n) in r["eliminations_by_cause"].as_object().into_iter().flatten() {
            *causes.entry(k.clone()).or_default() += n.as_u64().unwrap_or(0);
        }
    }
    json!({
        "seeds_ok": ok.len(),
        "seeds_failed": results.len() - ok.len(),
        "champion_seats": champs,
        "ended": ended,
        "eliminations_by_cause_total": causes,
        "stats": stats,
    })
}

/// `b − a` of every stat both aggregates have (`mean` and `median` deltas, and the relative change of the mean).
pub fn delta(a: &Value, b: &Value) -> Value {
    let mut out = Map::new();
    let (Some(sa), Some(sb)) = (a["stats"].as_object(), b["stats"].as_object()) else {
        return Value::Object(out);
    };
    let zero = json!({ "mean": 0.0, "median": 0.0 });
    let mut keys: Vec<&String> = sa.keys().chain(sb.keys()).collect();
    keys.sort();
    keys.dedup();
    for k in keys {
        // A count-map key only one arm saw counted zero in the other.
        let pick = |s: &Map<String, Value>| s.get(k.as_str()).or(zero_when_missing(k).then_some(&zero)).cloned();
        let (Some(x), Some(y)) = (pick(sa), pick(sb)) else { continue };
        let (Some(ma), Some(mb)) = (x["mean"].as_f64(), y["mean"].as_f64()) else { continue };
        let (da, db) = (x["median"].as_f64().unwrap_or(0.0), y["median"].as_f64().unwrap_or(0.0));
        if ma == mb && da == db {
            continue;
        }
        out.insert(
            k.clone(),
            json!({ "a": ma, "b": mb, "mean": round3(mb - ma), "median": round3(db - da),
                    "pct": if ma != 0.0 { json!(round3((mb - ma) / ma.abs() * 100.0)) } else { Value::Null } }),
        );
    }
    Value::Object(out)
}

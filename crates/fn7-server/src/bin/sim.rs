//! `fn7-sim`: a whole Final Night 7 match in the terminal, no server. Every step goes through the `GamePlugin` trait
//! exactly as the Theatre core drives it, recorded in the core's own JSONL format (`fn7_server::record`: the engine's
//! `JsonlSink` and `RunRec`), so the file holds what a server recording holds (hooks, snapshots, LLM calls, raw HTTP,
//! events). Prints each beat's caption and stage timings.
//!
//!   fn7-sim --seed 7 --beats 100                       # rules fallbacks only (no LLM), instant
//!   fn7-sim --scripted --beats 20                      # offline scripted replies for every mind
//!   fn7-sim --claude --beats 10 --budget 0.50          # Claude via ANTHROPIC_API_KEY (spends money)
//!   fn7-sim --replay runs/recordings/<id>.jsonl        # re-execute a recording (server or sim) and check every snapshot
//!
//! Recording goes to `runs/recordings/<run>.jsonl` by default (`--record DIR`, `--level events|full`, or `--no-record`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Parser;
use fn7_model::World;
use fn7_plugin::driver::{run_match, BeatReport};
use fn7_plugin::testkit::{Failing, Scripted};
use fn7_plugin::FinalNight7Plugin;
use fn7_server::anthropic::{price_per_mtok, DEFAULT_MODEL};
use serde_json::{json, Value};
use theatre_plugin_api::*;

#[derive(Parser)]
#[command(about = "Run (or replay) a Final Night 7 match in-process")]
struct Args {
    #[arg(long)]
    seed: Option<u64>,
    #[arg(long)]
    city: Option<String>,
    /// beats_total (the Final Night).
    #[arg(long, default_value_t = 100)]
    beats: u32,
    /// Seats (all bots); the plugin fills up to 4 demigods with its own AI.
    #[arg(long, default_value_t = 4)]
    players: u8,
    #[arg(long, default_value_t = 200)]
    clusters: u32,
    #[arg(long)]
    mind_timeout_ms: Option<u32>,
    /// Stop after this many beats (the match keeps its full length for pacing).
    #[arg(long)]
    stop_after: Option<u32>,
    /// Claude via ANTHROPIC_API_KEY (Messages API).
    #[arg(long)]
    claude: bool,
    #[arg(long, default_value = DEFAULT_MODEL)]
    model: String,
    /// Offline scripted replies for every mind (no network).
    #[arg(long)]
    scripted: bool,
    /// Stop when estimated Claude spend passes this many dollars.
    #[arg(long, default_value_t = 1.0)]
    budget: f64,
    #[arg(long)]
    verbose: bool,
    /// Recording directory (`<dir>/<run>.jsonl`, the core's format).
    #[arg(long, default_value = "runs/recordings")]
    record: String,
    /// Recording level: events | full (full adds per-phase snapshots, sync hooks and raw HTTP).
    #[arg(long, default_value = "full")]
    level: String,
    #[arg(long)]
    no_record: bool,
    /// Replay-verify a core recording (`<match_id>.jsonl`, from fn7-server or fn7-sim) instead of playing.
    #[arg(long)]
    replay: Option<String>,
    /// Extra room settings, merged last (JSON object), e.g. '{"entropy_scale": 4, "start_bolts": 6}'.
    #[arg(long)]
    settings: Option<String>,
    /// World-only simulation: no player orders and no casts (the AI demigods stay silent); only the society acts.
    #[arg(long)]
    society_only: bool,
    /// Print this many example society events at the end (0 = none).
    #[arg(long, default_value_t = 15)]
    examples: usize,
}

const SEAT_NAMES: &[&str] = &[
    "Ash", "Brix", "Cyra", "Dune", "Ember", "Fable", "Grit", "Hex",
];

fn inner(state: &Value) -> World {
    fn7_plugin::world::load(state).expect("fn7 state")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    let a = Args::parse();

    if let Some(path) = &a.replay {
        println!("replaying {path}");
        let t = Instant::now();
        let rep = fn7_server::replay::verify(path, |l| println!("{l}")).await?;
        println!(
            "run {}: verified {} snapshots over {} beats ({} via fallback_outcome, {} mid-match joins): {} identical, {} different; summary {}; LLM replay {} hits, {} misses ({:?})",
            rep.run,
            rep.snapshots,
            rep.beats,
            rep.fallbacks,
            rep.takeovers,
            rep.identical,
            rep.differences.len(),
            match rep.summary_identical {
                Some(true) => "identical",
                Some(false) => "DIFFERENT",
                None => "not recorded",
            },
            rep.llm_hits,
            rep.llm_misses,
            t.elapsed()
        );
        if !rep.differences.is_empty() {
            std::process::exit(1);
        }
        return Ok(());
    }

    // ---- LLM
    let mut usage = None;
    let claude: Option<Arc<dyn LlmProvider>> = if a.claude {
        let b = fn7_server::llm::claude(&a.model, Duration::from_secs(60), 24, 2)?
            .ok_or_else(|| anyhow::anyhow!("ANTHROPIC_API_KEY is not set"))?;
        usage = b.usage;
        Some(b.llm)
    } else {
        None
    };
    let base: Arc<dyn LlmProvider> = match claude {
        Some(c) => c,
        None if a.scripted => Arc::new(Scripted::default()),
        None => Arc::new(Failing),
    };
    let use_llm = a.claude || a.scripted;

    // ---- Plugin (recorded in the core's format)
    let recorder = if a.no_record {
        None
    } else {
        let level: theatre_trace::Level = a.level.parse().map_err(anyhow::Error::msg)?;
        Some(fn7_server::record::recorder(&a.record, level)?)
    };
    let run_id = uuid::Uuid::new_v4().to_string();
    let (p, llm): (Box<dyn GamePlugin>, Arc<dyn LlmProvider>) = match &recorder {
        Some(r) => (
            Box::new(fn7_server::record::Recorded::new(FinalNight7Plugin::new(), r.run(run_id.clone()))),
            fn7_server::record::traced(base),
        ),
        None => (Box::new(FinalNight7Plugin::new()), base),
    };
    let llm: &dyn LlmProvider = llm.as_ref();
    let recording_note = recorder
        .as_ref()
        .map(|r| format!("\nrecording run {run_id} to {}", r.dir.join(format!("{run_id}.jsonl")).display()))
        .unwrap_or_default();
    let mut settings = json!({ "beats_total": a.beats, "clusters": a.clusters, "llm": use_llm });
    if let Some(s) = a.seed {
        settings["seed"] = s.into();
    }
    if let Some(c) = &a.city {
        settings["city"] = c.clone().into();
    }
    if let Some(t) = a.mind_timeout_ms {
        settings["mind_timeout_ms"] = t.into();
    }
    if a.society_only {
        settings["society_only"] = true.into();
    }
    if let Some(extra) = &a.settings {
        let v: Value = serde_json::from_str(extra)?;
        for (k, x) in v.as_object().into_iter().flatten() {
            settings[k] = x.clone();
        }
    }
    let seats: Vec<SeatInfo> = (0..a.players)
        .map(|i| SeatInfo {
            seat: i,
            display_name: SEAT_NAMES[i as usize % SEAT_NAMES.len()].into(),
            is_bot: true,
        })
        .collect();

    let mut printed_header = false;
    let mut soc_ms: Vec<f64> = vec![];
    let mut times: Vec<f64> = vec![];
    let mut calls: Vec<u64> = vec![];
    let t = Instant::now();
    let (state, summary) = run_match(p.as_ref(), &seats, &settings, llm, a.stop_after, &|_, _| None, |r: &BeatReport, s: &Value| {
        let w = inner(s);
        if !printed_header {
            printed_header = true;
            let arena = &w.places[w.meta.arena.ix()];
            println!(
                "FINAL NIGHT 7 in {} (seed {}): {} demigods, {} beats, {} clusters, LLM: {}{}",
                arena.name,
                w.meta.seed,
                w.demigods.len(),
                w.meta.beats_total,
                w.clusters.len(),
                if a.claude { a.model.as_str() } else if a.scripted { "scripted (offline)" } else { "off (rules fallbacks)" },
                recording_note
            );
        }
        if std::env::var("FN7_SOC_DEBUG").is_ok() {
            let crowds: Vec<&fn7_model::Cluster> = w.clusters.iter().filter(|c| c.army.is_none() && c.size > 0).collect();
            let n = crowds.len().max(1) as f32;
            let avg = |f: &dyn Fn(&fn7_model::Cluster) -> f32| crowds.iter().map(|c| f(c)).sum::<f32>() / n;
            let angry = crowds.iter().filter(|c| c.mood.anger >= 0.62f32.max(c.baseline.anger + 0.15) && c.mood.anger + c.mood.fear >= 1.0).count();
            let supply: f32 = w.nodes.iter().filter(|x| x.level == fn7_model::Level::City).map(|x| x.supply).sum::<f32>()
                / w.nodes.iter().filter(|x| x.level == fn7_model::Level::City).count().max(1) as f32;
            let follow = crowds.iter().filter(|c| w.demigods.iter().any(|d| c.groups.contains(d.following))).count();
            let alleg = crowds.iter().map(|c| c.allegiance.iter().map(|x| x.1).fold(0.0f32, f32::max)).fold(0.0f32, f32::max);
            println!("    dbg anger {:.2} fear {:.2} hope {:.2} dev {:.2} food {:.2} safety {:.2} faith {:.2} angry {angry} supply {supply:.2} following {follow} max_alleg {alleg:.2} armies {} mobs {}",
                avg(&|c| c.mood.anger), avg(&|c| c.mood.fear), avg(&|c| c.mood.hope), avg(&|c| c.mood.devotion),
                avg(&|c| c.need.food), avg(&|c| c.need.safety), avg(&|c| c.need.faith),
                w.clusters.iter().filter(|c| c.army.is_some() && c.size > 0).count(),
                w.mobs.iter().filter(|m| m.ended.is_none()).count());
        }
        let pubv = &r.outcome.public;
        let tm = &pubv["timing"];
        let total = tm["total_ms"].as_f64().unwrap_or(0.0);
        times.push(total);
        soc_ms.push(tm["society_ms"].as_f64().unwrap_or(0.0));
        calls.push(tm["llm_calls"].as_u64().unwrap_or(0));
        let stages: Vec<String> = tm["stages"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|x| format!("{} {:.0}", &x["stage"].as_str().unwrap_or("")[..3.min(x["stage"].as_str().unwrap_or("").len())], x["ms"].as_f64().unwrap_or(0.0)))
            .collect();
        println!(
            "beat {:>3} | {} alive | r={:.0}km | {:.0} ms [{}] soc {:.1} ms/{} ev | llm {}/{} fb | {}",
            r.round,
            w.alive_demigods().count(),
            w.meta.arena_radius_km,
            total,
            stages.join(" "),
            tm["society_ms"].as_f64().unwrap_or(0.0),
            tm["society_events"],
            tm["llm_calls"],
            tm["llm_fallbacks"],
            pubv["caption"].as_str().unwrap_or("")
        );
        if a.verbose {
            for c in pubv["callouts"].as_array().into_iter().flatten() {
                println!("           > {}", c.as_str().unwrap_or(""));
            }
            for o in pubv["omens"].as_array().into_iter().flatten() {
                println!("           ~ {}: {}", o["god"].as_str().unwrap_or(""), o["speech"].as_str().unwrap_or(""));
            }
        }
        for d in w.demigods.iter().filter(|d| d.eliminated_beat == Some(r.round)) {
            println!("           x {} — {}", d.name, d.cause.clone().unwrap_or_default());
        }
        if let Some(u) = &usage {
            let (req, i, o, refusals) = u.snapshot();
            let (pi, po) = price_per_mtok(&a.model).unwrap_or((0.0, 0.0));
            let cost = i as f64 / 1e6 * pi + o as f64 / 1e6 * po;
            println!("           [claude: {req} requests, {i} in / {o} out tokens, {refusals} refusals, ~${cost:.3}]");
            if cost > a.budget {
                println!("           budget of ${} reached, stopping", a.budget);
                return false;
            }
        }
        true
    })
    .await?;

    let w = inner(&state);
    println!("\n==== FINAL CUT ====");
    for c in summary.summary["champions"]
        .as_array()
        .into_iter()
        .flatten()
    {
        println!(
            "Champion: {} (health {})",
            c["name"].as_str().unwrap_or(""),
            c["health"]
        );
    }
    println!(
        "{}\n{}",
        summary.summary["final_cut"]["title"].as_str().unwrap_or(""),
        summary.summary["final_cut"]["story"]
            .as_str()
            .unwrap_or("")
            .lines()
            .rev()
            .take(5)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n")
    );
    let n = times.len().max(1) as f64;
    let size = serde_json::to_vec(&state)?.len();
    println!(
        "\n{} beats in {:?}: beat mean {:.1} ms, max {:.1} ms; LLM calls {:.1}/beat; state {:.2} MB; {} rows, {} memories, {} characters, {} agents",
        times.len(),
        t.elapsed(),
        times.iter().sum::<f64>() / n,
        times.iter().cloned().fold(0.0, f64::max),
        calls.iter().sum::<u64>() as f64 / n,
        size as f64 / 1e6,
        w.ledger.rows.len(),
        w.memory.items.len(),
        w.characters.len(),
        w.agents.len()
    );
    if w.dm_rules() {
        // Rules 0.8.0: what the Dungeon Master decided.
        let dm = &w.dm;
        let kind = |k: &str| dm.memory.iter().filter(|e| e.kind == k).count();
        println!(
            "dm: {} decisions ({} d20 rolls); encounters {} marked, {} revealed, {} faded; loot {} piles ({} taken); \
             roads {}; shrines {}; settlements grew {} / shrank {}; scars {}",
            dm.memory.len(),
            kind("dice.rolled"),
            dm.encounters.len(),
            dm.encounters.iter().filter(|e| e.outcome.is_some()).count(),
            dm.encounters.iter().filter(|e| e.faded_beat.is_some()).count(),
            dm.loot.len(),
            dm.loot.iter().filter(|l| l.taken_beat.is_some()).count(),
            dm.roads().count(),
            dm.shrines.len(),
            kind("settlement.grew"),
            kind("settlement.shrank"),
            kind("story.scar"),
        );
    }
    society_report(&w, &soc_ms, a.examples);
    if let Some(r) = recorder {
        r.flush();
        println!("recorded to {}", r.dir.join(format!("{run_id}.jsonl")).display());
    }
    Ok(())
}

/// The society's own story: totals, named people by role, population shift, and example events naming humans.
fn society_report(w: &World, soc_ms: &[f64], examples: usize) {
    use fn7_model::{Role, SocKind};
    let n = soc_ms.len().max(1) as f64;
    let mut sorted = soc_ms.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let p95 = sorted.get(((sorted.len() as f64) * 0.95) as usize).copied().unwrap_or(0.0);
    println!(
        "\n==== SOCIETY ====\nsociety step: mean {:.2} ms, p95 {:.2} ms, max {:.2} ms ({} crowds, {} places, {} cells, {} mobs ever)",
        soc_ms.iter().sum::<f64>() / n,
        p95,
        sorted.last().copied().unwrap_or(0.0),
        w.clusters.iter().filter(|c| c.army.is_none()).count(),
        w.places.len(),
        w.cells.len(),
        w.mobs.len()
    );
    println!("totals: {}", serde_json::to_string(&w.soc).unwrap_or_default());
    let mut roles: std::collections::BTreeMap<String, (u32, u32)> = Default::default();
    for c in &w.characters {
        if let Some(l) = c.life.as_deref() {
            let e = roles.entry(format!("{:?}", l.emerged_as).to_lowercase()).or_default();
            e.0 += 1;
            if !c.alive {
                e.1 += 1;
            }
        }
    }
    let named = w.characters.iter().filter(|c| c.life.is_some()).count();
    println!(
        "named people from the simulation: {named} (by how they emerged, (total, dead)): {:?}; characters overall {}",
        roles,
        w.characters.len()
    );
    let now: Vec<(Role, usize)> = [Role::General, Role::Captain, Role::Ringleader, Role::Prophet, Role::Hero, Role::Coward, Role::Paladin, Role::Witness, Role::Deserter, Role::Avenger]
        .into_iter()
        .map(|r| (r, w.characters.iter().filter(|c| c.alive && c.life.as_ref().is_some_and(|l| l.role == r)).count()))
        .collect();
    println!("alive now by role: {now:?}");
    let base: u64 = w.nodes.get(w.meta.arena.ix()).map(|x| x.base).unwrap_or(0);
    let pop: u64 = w.nodes.get(w.meta.arena.ix()).map(|x| x.population).unwrap_or(0);
    println!("population: {base} -> {pop} ({:+.2}%)", (pop as f64 - base as f64) / base.max(1) as f64 * 100.0);
    let mut shifts: Vec<(i64, String)> = w
        .nodes
        .iter()
        .filter(|x| x.level == fn7_model::Level::City || (!w.is_planet() && x.level == fn7_model::Level::District))
        .map(|x| (x.population as i64 - x.base as i64, w.places[x.place.ix()].name.clone()))
        .collect();
    shifts.sort();
    let lost: Vec<String> = shifts.iter().take(4).map(|(d, n)| format!("{n} {d:+}")).collect();
    let gained: Vec<String> = shifts.iter().rev().take(4).map(|(d, n)| format!("{n} {d:+}")).collect();
    println!("biggest losses: {}\nbiggest gains: {}", lost.join(", "), gained.join(", "));
    let mut counts: std::collections::BTreeMap<&str, u32> = Default::default();
    for r in &w.ledger.rows {
        if let Some(k) = SocKind::of(&r.verb) {
            *counts.entry(k.as_str()).or_default() += 1;
        }
    }
    println!("society rows in the ledger by kind: {counts:?}");
    if examples == 0 {
        return;
    }
    println!("examples:");
    let want = [
        SocKind::MobFormed, SocKind::RingleaderEmerged, SocKind::GeneralAppointed, SocKind::Battle, SocKind::Hero,
        SocKind::Coward, SocKind::Mutiny, SocKind::Defection, SocKind::Migration, SocKind::CultFounded, SocKind::Revenge,
        SocKind::Promotion, SocKind::Death, SocKind::HostRaised, SocKind::OrderMisread, SocKind::Clash, SocKind::Riot,
        SocKind::Looting, SocKind::Desertion, SocKind::OrderReceived,
    ];
    let mut shown = 0;
    for k in want {
        if shown >= examples {
            break;
        }
        if let Some(r) = w.ledger.rows.iter().find(|r| r.verb == k.as_str()) {
            println!("  beat {:>3} [{}] {}", r.beat, k.as_str(), r.act);
            shown += 1;
        }
    }
}

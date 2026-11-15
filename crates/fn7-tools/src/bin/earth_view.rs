//! `fn7-earth-view`: a tiny viewer of the Earth simulator (state schema 13). It reads a recording (the core's JSONL:
//! the newest snapshot that carries `dm.earth` is used) or a state JSON file, and prints an ASCII (or ANSI colour) map
//! of a region's top materials for a range of beats. Past beats are rebuilt from the state's keyframes and input log
//! (time travel), so one snapshot shows the whole match.
//!
//! ```text
//! fn7-earth-view runs/sim/<id>.jsonl                       # the busiest phenomenon's surroundings, the last 4 beats
//! fn7-earth-view <file> --from 10 --to 40 --step 10        # a beat range
//! fn7-earth-view <file> --bbox 90,-15,150,25 --ansi        # a box [west, south, east, north] in colour
//! fn7-earth-view <file> --region pl:12 --span 30           # a place's surroundings (± span degrees of longitude)
//! ```

use std::io::{BufRead, BufReader};

use anyhow::{anyhow, bail, Context, Result};
use clap::Parser;
use fn7_earth::sim::{kind_name, Sim, TIERS};
use fn7_plugin::earth::view;
use serde_json::Value;

#[derive(Parser)]
#[command(about = "Print an ASCII map of the Earth simulator's field from a recording or a state file")]
struct Args {
    /// A core JSONL recording, or a JSON file holding a match state (or a snapshot record with `state`).
    file: String,
    /// First beat (default: the last beat − 3 × step).
    #[arg(long)]
    from: Option<u32>,
    /// Last beat (default: the last beat stepped).
    #[arg(long)]
    to: Option<u32>,
    #[arg(long, default_value_t = 1)]
    step: u32,
    /// [west, south, east, north] in degrees.
    #[arg(long, value_delimiter = ',', allow_hyphen_values = true)]
    bbox: Option<Vec<f64>>,
    /// A place ref (`pl:<n>`): centre the map on it.
    #[arg(long)]
    region: Option<String>,
    /// Centre the map on lon,lat.
    #[arg(long, value_delimiter = ',', allow_hyphen_values = true)]
    around: Option<Vec<f64>>,
    /// Half-width of a centred map, degrees of longitude (the height follows the aspect).
    #[arg(long, default_value_t = 30.0)]
    span: f64,
    #[arg(long, default_value_t = 72)]
    cols: usize,
    #[arg(long, default_value_t = 24)]
    rows: usize,
    /// ANSI colours.
    #[arg(long)]
    ansi: bool,
}

/// The newest state in the file that has `dm.earth`.
fn read_state(path: &str) -> Result<Value> {
    let f = std::fs::File::open(path).with_context(|| format!("open {path}"))?;
    let mut r = BufReader::new(f);
    let mut first = String::new();
    r.read_line(&mut first)?;
    // A plain JSON file (one value over many lines, or one line).
    if !path.ends_with(".jsonl") {
        let mut rest = first;
        std::io::Read::read_to_string(&mut r, &mut rest)?;
        let v: Value = serde_json::from_str(&rest)?;
        return Ok(v.get("state").cloned().unwrap_or(v));
    }
    let mut best: Option<String> = None;
    let mut line = first;
    loop {
        if line.contains("\"type\":\"snapshot\"") && line.contains("\"earth\":{") {
            best = Some(std::mem::take(&mut line));
        }
        line.clear();
        if r.read_line(&mut line)? == 0 {
            break;
        }
    }
    let best = best.ok_or_else(|| anyhow!("no snapshot with an Earth field in {path} (state schema 13 with earth_sim_enabled)"))?;
    let v: Value = serde_json::from_str(&best)?;
    v.get("state").cloned().ok_or_else(|| anyhow!("the snapshot has no state"))
}

fn main() -> Result<()> {
    let a = Args::parse();
    let state = read_state(&a.file)?;
    let (now, es) = view::field_at(&state, None).map_err(|e| anyhow!(e))?;
    let to = a.to.unwrap_or(es.beat).min(es.beat);
    let from = a.from.unwrap_or(to.saturating_sub(3 * a.step)).max(es.first_beat.max(1));
    if from > to {
        bail!("no beats in {from}..={to} (the field runs {}..={})", es.first_beat, es.beat);
    }
    let aspect = a.rows as f64 / a.cols as f64 * 2.0;
    let centred = |c: [f64; 2]| [c[0] - a.span, (c[1] - a.span * aspect).max(-89.0), c[0] + a.span, (c[1] + a.span * aspect).min(89.0)];
    let bbox: [f64; 4] = if let Some(b) = &a.bbox {
        let [w, s, e, n] = b[..] else { bail!("--bbox is west,south,east,north") };
        [w, s, e, n]
    } else if let Some(c) = &a.around {
        centred([c[0], c[1]])
    } else if let Some(r) = &a.region {
        let ix: usize = r.strip_prefix("pl:").and_then(|x| x.parse().ok()).ok_or_else(|| anyhow!("--region is pl:<n>"))?;
        let c = state.pointer(&format!("/places/{ix}/center")).and_then(|c| Some([c.get(0)?.as_f64()?, c.get(1)?.as_f64()?]));
        centred(c.ok_or_else(|| anyhow!("no place {r}"))?)
    } else if let Some(p) = now.phen.iter().max_by_key(|p| (p.power, std::cmp::Reverse(p.id))) {
        centred(now.grid().center[p.cell as usize])
    } else {
        [-180.0, -70.0, 180.0, 80.0]
    };
    println!("{}", view::LEGEND);
    println!("match seed {}, field beats {}..={}, keyframes at {:?}, {} inputs; box {:?}",
             state.pointer("/meta/seed").and_then(Value::as_u64).unwrap_or(0), es.first_beat, es.beat,
             es.keyframes.iter().map(|k| k.beat).collect::<Vec<_>>(), es.inputs.len(), bbox.map(|x| (x * 10.0).round() / 10.0));
    let mut b = from;
    while b <= to {
        let (sim, _): (Sim, _) = view::field_at(&state, Some(b)).map_err(|e| anyhow!(e))?;
        println!("\n-- beat {b}: {} dirty cells, hash {:016x}", sim.list.len(), sim.hash());
        for p in &sim.phen {
            let c = sim.grid().center[p.cell as usize];
            println!("   #{} {} ({}) at [{:.1}, {:.1}] power {} radius {} beats left {}", p.id, kind_name(p.kind), TIERS[p.tier.min(3) as usize], c[0], c[1], p.power, p.radius, p.dur);
        }
        print!("{}", view::ascii(&sim, bbox, a.cols, a.rows, a.ansi));
        b += a.step.max(1);
    }
    Ok(())
}

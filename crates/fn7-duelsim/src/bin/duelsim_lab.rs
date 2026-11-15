//! duelsim_lab: bake a lab duel file (`duel_lab/duel.json`) with this crate and print what happened, the sizes and
//! the compute time. With `--lab <export.json>` (from `tools/lab_export.mjs`) it prints the parity table against the
//! lab's own engine. See docs/DUEL_SIM.md.
//!
//!   cargo run --release -p fn7-duelsim --bin duelsim_lab -- duel_lab/duel.json [--a gold] [--b obsidian] [--lab export.json] [--json out.json] [--events]

use fn7_duelsim::bake::{FighterSpec, Sim};
use fn7_duelsim::lab_panels;
use fn7_duelsim::physics::{Ev, With};
use serde_json::{json, Value};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let file = args.iter().find(|a| a.ends_with(".json") && Some(*a) != opt("--lab").as_ref() && Some(*a) != opt("--json").as_ref()).cloned().unwrap_or("duel_lab/duel.json".into());
    let d: Value = serde_json::from_str(&std::fs::read_to_string(&file).expect("duel file")).expect("json");
    let lab: Option<Value> = opt("--lab").map(|f| serde_json::from_str(&std::fs::read_to_string(f).expect("lab export")).expect("json"));
    let skin = |k: &str| opt(&format!("--{k}")).or_else(|| lab.as_ref().and_then(|l| l["skins"][if k == "a" { 0 } else { 1 }].as_str().map(|s| s.to_string()))).unwrap_or(d[k]["skin"].as_str().unwrap_or("wood").to_string());
    let start = |k: &str| [d[k]["start"][0].as_f64().unwrap(), d[k]["start"][1].as_f64().unwrap()];
    let fighters = [FighterSpec::new(&skin("a"), start("a")), FighterSpec::new(&skin("b"), start("b"))];
    let panels = lab_panels(&d);
    let seed = 7;

    // --repeat N: bake the whole duel N more times first and print the fastest (for timing and profiling)
    if let Some(n) = opt("--repeat").and_then(|n| n.parse::<usize>().ok()) {
        let mut best = vec![u128::MAX; panels.len()];
        for _ in 0..n { let mut s = Sim::new(&d["scene"], &fighters, seed); s.world.support_wake = !args.iter().any(|a| a == "--lab-exact"); for (i, p) in panels.iter().enumerate() { let t = Instant::now(); let _ = s.panel(p); best[i] = best[i].min(t.elapsed().as_micros()); } }
        println!("fastest of {n}: {:?} us per panel, {} us the duel", best, best.iter().sum::<u128>());
    }
    let t = Instant::now();
    let mut sim = Sim::new(&d["scene"], &fighters, seed);
    sim.keep_raw = lab.is_some();
    // --lab-exact: the lab's engine to the letter (no support waking), to show the port is faithful
    if args.iter().any(|a| a == "--lab-exact") { sim.world.support_wake = false; }
    // per body: where it started, its up axis then, the farthest it got, the most it tilted
    let mut track: Vec<Option<([f64; 3], [f64; 3], f64, f64)>> = vec![];
    let setup_us = t.elapsed().as_micros();
    println!("fighters: a {} {:.0} kg (jump x{:.2}, speed x{:.2}), b {} {:.0} kg (jump x{:.2}, speed x{:.2})", sim.fb[0].skin, sim.fb[0].mass, sim.fb[0].jump, sim.fb[0].speed, sim.fb[1].skin, sim.fb[1].mass, sim.fb[1].jump, sim.fb[1].speed);
    println!("scene: {} objects, {} affordances, derived in {} us", sim.scene.objects.len() - 1, sim.scene.affordances.len(), setup_us);
    let intro = d["intro_ms"].as_f64().unwrap_or(0.0);
    let mut all = vec![]; let mut rows = vec![]; let mut out_panels = vec![];
    for (i, p) in panels.iter().enumerate() {
        let t = Instant::now();
        let b = sim.panel(p);
        let us = t.elapsed().as_micros();
        let js = b.to_json(&sim.scene);
        let bytes = serde_json::to_string(&js).unwrap().len();
        let keys: usize = b.bodies.iter().map(|x| x.2.len()).sum();
        rows.push(format!("panel {i:>2} {:<8} {:>5} ms  {:>4} steps  {:>7} us  {:>6} B  bodies {:>2} keys {:>4}  events {:>3}  root keys {}+{}  limbs {}+{}  hash {:08x}",
            p["kind"].as_str().unwrap_or("panel"), b.play_ms, (b.len as u64) * 2, us, bytes, b.bodies.len(), keys, js["events"].as_array().unwrap().len(),
            b.fighters[0].root.len(), b.fighters[1].root.len(), b.fighters[0].limbs.len(), b.fighters[1].limbs.len(), b.hash));
        for w in &b.warnings { println!("warning: {w}"); }
        for e in &b.events { all.push((e.clone(), sim.scene.objects[e.obj].id.clone())); }
        for (bi, keys) in &b.raw {
            if track.len() <= *bi { track.resize(*bi + 1, None); }
            for k in keys {
                let up = fn7_duelsim::math::qrot(k.q, [0.0, 1.0, 0.0]);
                let t = track[*bi].get_or_insert((k.p, up, 0.0, 0.0));
                t.2 = t.2.max(((k.p[0] - t.0[0]).powi(2) + (k.p[2] - t.0[2]).powi(2)).sqrt());
                t.3 = t.3.max((up[0] * t.1[0] + up[1] * t.1[1] + up[2] * t.1[2]).clamp(-1.0, 1.0).acos().to_degrees());
            }
        }
        out_panels.push(js);
    }
    for r in &rows { println!("{r}"); }
    println!("hash {:08x}  state {:016x}", sim.hash, sim.world.state_hash(fn7_duelsim::math::FNV64_0));
    if args.iter().any(|a| a == "--events") {
        for (e, obj) in &all {
            if (e.kind == Ev::Hit || e.kind == Ev::Push) && e.j <= 60.0 { continue; }
            let with = match e.with { With::Floor => "floor".to_string(), With::Body(b) => format!("body {b}"), With::Fighter(s) => format!("fighter {}", if s == 0 { "a" } else { "b" }), With::None => e.why.to_string() };
            println!("{:>6.2} {:<7} {:<10} {:<12} {} {}", (e.step as f64 * 1000.0 / 120.0 - intro) / 1000.0, e.kind.name(), obj, with, if e.j > 0.0 { format!("{:.0} N.s", e.j) } else { String::new() }, if e.made.is_empty() { String::new() } else { e.made.len().to_string() });
        }
    }
    if let Some(path) = opt("--json") {
        let evs: Vec<Value> = all.iter().map(|(e, obj)| json!({ "t": e.step as f64 / 120.0, "type": e.kind.name(), "obj": obj, "J": e.j, "n": e.made.len(), "why": e.why, "sim": e.sim })).collect();
        let finals: Vec<Value> = sim.world.bodies.iter().filter(|b| !b.hidden).map(|b| json!({ "id": b.id, "obj": sim.scene.objects[b.obj].id, "frag": b.frag, "piece": b.piece, "dead": b.dead, "p": b.origin(), "q": b.q, "up": fn7_duelsim::math::qrot(b.q, [0.0, 1.0, 0.0])[1] })).collect();
        let cracked: Vec<Value> = sim.world.bodies.iter().flat_map(|b| b.parts.iter().filter(|p| p.cracked || p.dead).map(|p| json!({ "obj": sim.scene.objects[b.obj].id, "part": p.src, "cracked": p.cracked, "dead": p.dead, "damage": p.damage }))).collect();
        std::fs::write(&path, serde_json::to_string(&json!({ "events": evs, "bodies": finals, "parts": cracked, "panels": out_panels, "hash": format!("{:08x}", sim.hash) })).unwrap()).unwrap();
        println!("wrote {path}");
    }
    if let Some(lab) = lab { parity(&lab, &all, &sim, &track); }
}

/// The parity table: the same things must happen in both engines, at about the same times.
fn parity(lab: &Value, mine: &[(fn7_duelsim::physics::Event, String)], sim: &Sim, track: &[Option<([f64; 3], [f64; 3], f64, f64)>]) {
    let intro = lab["intro_ms"].as_f64().unwrap_or(0.0) / 1000.0;
    println!("\nPARITY with duel_lab (a {} / b {}; lab bake {:.0} ms in node, hash {})", lab["skins"][0].as_str().unwrap_or("?"), lab["skins"][1].as_str().unwrap_or("?"), lab["bake_ms"].as_f64().unwrap_or(0.0), lab["hash"].as_str().unwrap_or("?"));
    println!("| what | lab | this crate | delta | ok |\n|---|---|---|---|---|");
    let mut bad = 0;
    let mut row = |what: String, l: String, m: String, d: String, ok: bool| { if !ok { bad += 1; } println!("| {what} | {l} | {m} | {d} | {} |", if ok { "yes" } else { "NO" }); };
    let lev: Vec<&Value> = lab["events"].as_array().unwrap().iter().collect();
    // every crack, break, shatter and scripted strike, in order of each kind on each object
    for kind in ["hit", "crack", "break", "shatter", "touch"] {
        let l: Vec<&&Value> = lev.iter().filter(|e| e["type"] == kind && match kind { "hit" => e["sim"] == false, "touch" => e["obj"] == "crate_e" || e["obj"] == "screen", _ => true }).collect();
        let m: Vec<&(fn7_duelsim::physics::Event, String)> = mine.iter().filter(|(e, o)| e.kind.name() == kind && match kind { "hit" => !e.sim, "touch" => o == "crate_e" || o == "screen", _ => true }).collect();
        let mut used = vec![false; m.len()];
        for e in &l {
            let (obj, t) = (e["obj"].as_str().unwrap_or(""), e["t"].as_f64().unwrap());
            let found = m.iter().enumerate().filter(|(i, x)| !used[*i] && x.1 == obj).min_by(|a, b| ((a.1 .0.step as f64 / 120.0) - t).abs().partial_cmp(&((b.1 .0.step as f64 / 120.0) - t).abs()).unwrap());
            let label = format!("{kind} {obj}{}", if kind == "hit" { format!(" ({})", if e["why"].as_str().unwrap_or("").is_empty() { "strike" } else { e["why"].as_str().unwrap() }) } else { String::new() });
            let lv = |t: f64, j: f64, n: f64| format!("{:.2} s{}{}", t - intro, if j > 0.0 { format!(", {j:.0} N.s") } else { String::new() }, if n > 0.0 { format!(", {n:.0} pieces") } else { String::new() });
            match found {
                Some((i, x)) => { used[i] = true; let mt = x.0.step as f64 / 120.0; let (lj, ln) = (e["J"].as_f64().unwrap_or(0.0), e["n"].as_f64().unwrap_or(0.0));
                    let mj = if kind == "hit" { x.0.j } else { 0.0 }; let lj = if kind == "hit" { lj } else { 0.0 };
                    let ok = (mt - t).abs() <= 0.1 && ln == x.0.made.len() as f64 && (lj - mj).abs() <= 0.05 * lj.max(1.0);
                    row(label, lv(t, lj, ln), lv(mt, mj, x.0.made.len() as f64), format!("{:+.2} s", mt - t), ok); }
                None => row(label, lv(t, 0.0, e["n"].as_f64().unwrap_or(0.0)), "missing".into(), "".into(), false),
            }
        }
        for (i, x) in m.iter().enumerate() { if !used[i] { row(format!("{kind} {}", x.1), "missing".into(), format!("{:.2} s", x.0.step as f64 / 120.0 - intro), "".into(), false); } }
    }
    // where the loose things ended up
    for lb in lab["bodies"].as_array().unwrap() {
        if lb["frag"] == true || lb["samples"].as_u64().unwrap_or(0) < 2 { continue; }
        let (obj, src, piece) = (lb["obj"].as_str().unwrap(), lb["src"].as_u64().unwrap() as usize, lb["piece"] == true);
        let mb = sim.world.bodies.iter().find(|b| !b.frag && b.piece == piece && sim.scene.objects[b.obj].id == obj && (!piece || b.parts[0].src == src));
        let Some(mb) = mb else { row(format!("body {obj}"), "there".into(), "missing".into(), "".into(), false); continue };
        let Some(t) = track.get(mb.id).copied().flatten() else { if lb["far"].as_f64().unwrap() > 0.3 { row(format!("body {obj}"), "moved".into(), "never moved".into(), "".into(), false); } continue };
        let (lfar, ltilt, mfar, mtilt) = (lb["far"].as_f64().unwrap(), lb["max_tilt"].as_f64().unwrap(), t.2, t.3);
        if lfar < 0.3 && mfar < 0.3 && ltilt < 8.0 && mtilt < 8.0 { continue; }
        let p1 = mb.origin(); let lp1 = &lb["p1"];
        let end = ((p1[0] - lp1[0].as_f64().unwrap()).powi(2) + (p1[2] - lp1[2].as_f64().unwrap()).powi(2)).sqrt();
        let name = if piece { format!("{obj} piece (part {src})") } else { obj.to_string() };
        row(format!("{name}: farthest from its start"), format!("{lfar:.2} m"), format!("{mfar:.2} m"), format!("{:+.2} m", mfar - lfar), (mfar - lfar).abs() <= 0.35f64.max(0.25 * lfar));
        row(format!("{name}: most it tilted"), format!("{ltilt:.0} deg"), format!("{mtilt:.0} deg"), format!("{:+.0} deg", mtilt - ltilt), (mtilt - ltilt).abs() <= 25.0);
        row(format!("{name}: where it came to rest"), format!("({:.2}, {:.2})", lp1[0].as_f64().unwrap(), lp1[2].as_f64().unwrap()), format!("({:.2}, {:.2})", p1[0], p1[2]), format!("{end:.2} m apart"), end <= 0.6f64.max(0.3 * lfar));
    }
    println!("{}", if bad == 0 { "PARITY: every row within tolerance".to_string() } else { format!("PARITY: {bad} rows out of tolerance") });
}

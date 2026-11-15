//! duelsim_lab: bake a lab duel file (`duel_lab/duel.json`) with this crate and print what happened, the sizes and
//! the compute time. With `--lab <export.json>` (from `tools/lab_export.mjs`) it prints the parity table against the
//! lab's own engine. See docs/DUEL_SIM.md.
//!
//!   cargo run --release -p fn7-duelsim --bin duelsim_lab -- duel_lab/duel.json [--a gold] [--b obsidian] [--lab export.json] [--json out.json] [--events]

use fn7_duelsim::bake::{FighterSpec, Sim};
use fn7_duelsim::physics::{Ev, With};
use serde_json::{json, Value};
use std::time::Instant;

/// A lab duel as the server would stream it: an intro panel (the lab's `intro_ms` of standing), its panels, an outro
/// (the lab's `outro_ms`; the settle follows it).
pub fn lab_panels(d: &Value) -> Vec<Value> {
    let mut v = vec![json!({ "kind": "intro", "play_ms": d["intro_ms"], "a": { "segs": [] }, "b": { "segs": [] } })];
    v.extend(d["panels"].as_array().unwrap().iter().cloned());
    v.push(json!({ "kind": "outro", "play_ms": d["outro_ms"], "a": { "segs": [] }, "b": { "segs": [] } }));
    v
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let file = args.iter().find(|a| a.ends_with(".json") && Some(*a) != opt("--lab").as_ref() && Some(*a) != opt("--json").as_ref()).cloned().unwrap_or("duel_lab/duel.json".into());
    let d: Value = serde_json::from_str(&std::fs::read_to_string(&file).expect("duel file")).expect("json");
    let skin = |k: &str| opt(&format!("--{k}")).unwrap_or(d[k]["skin"].as_str().unwrap_or("wood").to_string());
    let start = |k: &str| [d[k]["start"][0].as_f64().unwrap(), d[k]["start"][1].as_f64().unwrap()];
    let fighters = [FighterSpec::new(&skin("a"), start("a")), FighterSpec::new(&skin("b"), start("b"))];
    let panels = lab_panels(&d);
    let seed = 7;

    let t = Instant::now();
    let mut sim = Sim::new(&d["scene"], &fighters, seed);
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
}

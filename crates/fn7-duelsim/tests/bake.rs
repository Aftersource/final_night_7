//! The bake as a whole on the lab's duel: determinism (two runs, incremental = from scratch, every entry point
//! agrees), the pinned cross-platform hash, the wire format round trip, the fighters' limb targets on real surfaces,
//! the effect of mass, and the size and time budgets.

use fn7_duelsim::bake::{FighterSpec, Sim};
use fn7_duelsim::math::*;
use fn7_duelsim::physics::Ev;
use fn7_duelsim::wire::*;
use fn7_duelsim::{bake, lab_panels, panel_sim, present_sim, with_sims, SimCache};
use serde_json::Value;

fn lab() -> Value { serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../duel_lab/duel.json")).unwrap()).unwrap() }
fn fighters(d: &Value, a: &str, b: &str) -> [FighterSpec; 2] {
    let st = |k: &str| [d[k]["start"][0].as_f64().unwrap(), d[k]["start"][1].as_f64().unwrap()];
    [FighterSpec::new(a, st("a")), FighterSpec::new(b, st("b"))]
}
fn sims(d: &Value, f: &[FighterSpec; 2]) -> Vec<String> {
    let mut s = Sim::new(&d["scene"], f, 7);
    lab_panels(d).iter().map(|p| { let b = s.panel(p); serde_json::to_string(&b.to_json(&s.scene)).unwrap() }).collect()
}

/// The whole lab duel (gold vs obsidian, seed 7). Measured identical on aarch64-apple-darwin (debug and release) and
/// x86_64-apple-darwin under Rosetta (SSE2). If an intended engine change moves it, re-pin it here and re-run the
/// cross-target check in docs/DUEL_SIM.md; an unintended change is a determinism bug.
const PINNED: (u32, u64) = (0x9f677152, 0xbfc407b3a54f5c96);

#[test]
fn the_lab_duel_bakes_to_the_pinned_hash() {
    let d = lab();
    let r = bake(&d["scene"], &fighters(&d, "gold", "obsidian"), &lab_panels(&d), 7);
    assert_eq!((r.hash, r.state), PINNED, "hash {:08x} state {:016x}", r.hash, r.state);
}

#[test]
fn two_runs_give_the_same_bytes_and_skins_and_seeds_matter() {
    let d = lab();
    let f = fighters(&d, "gold", "obsidian");
    let (x, y) = (sims(&d, &f), sims(&d, &f));
    assert_eq!(x, y);
    let w = sims(&d, &fighters(&d, "wood", "obsidian"));
    assert_ne!(x, w, "a different skin is a different bake");
    let s8 = bake(&d["scene"], &f, &lab_panels(&d), 8);
    assert_ne!(s8.state, PINNED.1, "the seed reaches the shards");
}

#[test]
fn incremental_equals_from_scratch_by_every_entry_point() {
    let d = lab();
    let f = fighters(&d, "gold", "obsidian");
    let panels = lab_panels(&d);
    let scratch = sims(&d, &f);
    // panel by panel, keeping the state (the cheap path)
    let mut state = None;
    for (i, p) in panels.iter().enumerate() {
        let r = panel_sim(&d["scene"], &f, 7, &panels[..i], p, state.take());
        assert_eq!(serde_json::to_string(&r.sim).unwrap(), scratch[i], "panel {i}, incremental");
        state = Some(r.state);
    }
    // panel by panel, dropping the state (a restart: the earlier panels are re-baked)
    for i in [1, 6, panels.len() - 1] {
        let r = panel_sim(&d["scene"], &f, 7, &panels[..i], &panels[i], None);
        assert_eq!(serde_json::to_string(&r.sim).unwrap(), scratch[i], "panel {i}, from scratch");
    }
    // a stale state (wrong panel count) is ignored, not trusted
    let mut early = Sim::new(&d["scene"], &f, 7); early.panel(&panels[0]);
    let r = panel_sim(&d["scene"], &f, 7, &panels[..5], &panels[5], Some(early));
    assert_eq!(serde_json::to_string(&r.sim).unwrap(), scratch[5]);
    // a resumed checkpoint (a clone of the state) continues to the same bytes
    let mut a = Sim::new(&d["scene"], &f, 7);
    for p in &panels[..6] { a.panel(p); }
    let mut b = a.clone();
    assert_eq!(serde_json::to_string(&b.panel(&panels[6]).to_json(&b.scene)).unwrap(), scratch[6]);
    // with_sims, present_sim and the cache agree
    let ws = with_sims(&d["scene"], &f, 7, &panels, 3);
    assert_eq!(ws.len(), panels.len() - 3);
    for (k, p) in ws.iter().enumerate() { assert_eq!(serde_json::to_string(&p["sim"]).unwrap(), scratch[k + 3]); }
    let pr = present_sim(&d["scene"], &f, 7, &panels[..7]);
    assert_eq!(serde_json::to_string(&pr["sim"]).unwrap(), scratch[6]);
    let mut cache = SimCache::new(4);
    for n in 1..=panels.len() {
        let got = cache.with_sims("duel-1", &d["scene"], &f, 7, &panels[..n], n - 1);
        assert_eq!(serde_json::to_string(&got[0]["sim"]).unwrap(), scratch[n - 1], "cache, panel {}", n - 1);
    }
    assert_eq!(cache.present("duel-1", &d["scene"], &f, 7, &panels[..7]), pr, "the cache rewinds to an earlier panel count");
    assert_eq!(cache.len(), 1);
}

#[test]
fn the_wire_round_trips_within_its_quantisation() {
    let d = lab();
    let mut s = Sim::new(&d["scene"], &fighters(&d, "gold", "obsidian"), 7);
    s.keep_raw = true;
    let mut checked = 0;
    for p in lab_panels(&d) {
        let b = s.panel(&p);
        for (bi, _, keys) in &b.bodies {
            let back = unpack_body(&pack_body(keys));
            assert_eq!(back.len(), keys.len());
            for (k, r) in keys.iter().zip(&back) {
                assert_eq!(k.i, r.i);
                assert!(dist(k.p, r.p) < 0.0011, "body {bi}: {:?} vs {:?}", k.p, r.p);
                assert!(qdot(k.q, r.q).abs() > 0.99999 - 0.0005, "quaternion {:?} vs {:?}", k.q, r.q);
            }
            // playback of the reduced track stays within tolerance of every raw sample
            let raw = &b.raw.iter().find(|r| r.0 == *bi).unwrap().1;
            let tol = if s.world.bodies[*bi].frag { 0.02 } else { 0.008 };
            for m in raw { let (p, _) = body_at(keys, m.i as f64).unwrap(); assert!(dist(p, m.p) <= tol + 1e-9, "body {bi} sample {}: off by {}", m.i, dist(p, m.p)); checked += 1; }
        }
        for f in &b.fighters {
            let back = unpack_root(&pack_root(&f.root));
            for (k, r) in f.root.iter().zip(&back) { assert!(dist(k.p, r.p) < 0.0011 && (k.yaw - r.yaw).abs() < 0.0011 && k.flags == r.flags); }
        }
    }
    assert!(checked > 500);
}

#[test]
fn limb_targets_land_on_real_surfaces() {
    let d = lab();
    let mut s = Sim::new(&d["scene"], &fighters(&d, "gold", "obsidian"), 7);
    let sc = s.scene.clone();
    let id = |o: usize| sc.objects[o].id.clone();
    let mut kinds = vec![];
    for p in lab_panels(&d) {
        let b = s.panel(&p);
        for f in &b.fighters {
            for l in &f.limbs {
                kinds.push(l.kind);
                assert!(l.t1_ms > l.t0_ms && l.in_ms >= 0.0 && l.out_ms >= 0.0 && l.in_ms + l.out_ms <= l.t1_ms - l.t0_ms + 1e-6, "{l:?}");
                assert!((len(l.n) - 1.0).abs() < 1e-6, "unit normal {l:?}");
                if let fn7_duelsim::bake::On::Obj(o, _) = l.on {
                    let ob = &sc.objects[o];
                    match l.kind {
                        // a hand on the desk's top for a vault, on the sacks' top for a mantle: at the top's height, normal up
                        "vault" | "mantle" => { assert!(l.n[1] > 0.99 && (l.p[1] - ob.aabb[1][1]).abs() < 0.02, "{} on {}: {:?} top {}", l.kind, id(o), l.p, ob.aabb[1][1]); assert!(l.limb.starts_with("hand")); }
                        // a foot on a wall: on its side (normal level), between the floor and its top
                        "wallrun" | "wallkick" => { assert!(l.n[1].abs() < 0.05 && l.p[1] > 0.3 && l.p[1] < ob.aabb[1][1] + 0.01, "{} on {}: {:?} n {:?}", l.kind, id(o), l.p, l.n); assert!(l.limb.starts_with("foot")); }
                        _ => {}
                    }
                }
            }
        }
    }
    for k in ["plant", "strike", "vault", "mantle", "wallrun", "wallkick"] { assert!(kinds.contains(&k), "the lab duel has {k} targets: {kinds:?}"); }
}

#[test]
fn mass_decides_what_a_blow_breaks() {
    // the boosted final blow on the pillar breaks it with a gold fighter and only cracks it with a wooden one
    let d = lab();
    let outcome = |skin: &str| {
        let r = bake(&d["scene"], &fighters(&d, skin, "obsidian"), &lab_panels(&d), 7);
        let pillar = r.sim.scene.index("pillar").unwrap();
        let n = |k: Ev| r.panels.iter().flat_map(|p| &p.events).filter(|e| e.obj == pillar && e.kind == k).count();
        (n(Ev::Crack), n(Ev::Break))
    };
    let (gc, gb) = outcome("gold");
    let (wc, wb) = outcome("wood");
    assert!(gb == 1 && gc >= 1, "gold breaks the pillar: {gc} cracks, {gb} breaks");
    assert!(wb == 0 && wc >= 1, "wood only cracks it: {wc} cracks, {wb} breaks");
}

#[test]
fn heavier_fighters_jump_lower_and_hit_harder() {
    let d = lab();
    let s = |skin: &str| Sim::new(&d["scene"], &fighters(&d, skin, "wood"), 7);
    let (g, w, b) = (s("gold"), s("wood"), s("butterfly"));
    assert!(g.fb[0].jump < w.fb[0].jump && w.fb[0].jump < b.fb[0].jump);
    // the arcs a heavy fighter flies are lower: the lab duel's leap by a, as gold and as wood
    let apex = |skin: &str| { let mut s = s(skin); let mut best = 0.0f64; for p in lab_panels(&d) { for a in &s.panel(&p).fighters[0].arcs { best = best.max(a.2); } } best };
    let (ag, aw) = (apex("gold"), apex("wood"));
    assert!(ag > 0.0 && aw > ag, "wood flies higher than gold: {aw} vs {ag}");
}

#[cfg_attr(debug_assertions, ignore = "budgets are measured in release: cargo test --release -p fn7-duelsim")]
#[test]
fn size_and_time_budgets() {
    let d = lab();
    let panels = lab_panels(&d);
    let mut s = Sim::new(&d["scene"], &fighters(&d, "gold", "obsidian"), 7);
    let t = std::time::Instant::now();
    let mut worst = 0u128;
    for p in &panels {
        let t1 = std::time::Instant::now();
        let b = s.panel(p);
        worst = worst.max(t1.elapsed().as_micros());
        let bytes = serde_json::to_string(&b.to_json(&s.scene)).unwrap().len();
        let destructive = b.events.iter().any(|e| matches!(e.kind, Ev::Break | Ev::Shatter));
        let calm = b.events.is_empty() && b.bodies.len() <= 1 && b.fighters.iter().all(|f| f.arcs.is_empty());
        assert!(bytes <= 12 * 1024, "panel {}: {bytes} B", b.n);
        if calm { assert!(bytes <= 1536, "calm panel {}: {bytes} B", b.n); }
        if !destructive { assert!(bytes <= 6 * 1024, "panel {} without breakage: {bytes} B", b.n); }
    }
    let total = t.elapsed().as_millis();
    assert!(total < 1500 && worst < 250_000, "the whole duel {total} ms, worst panel {worst} us");
}

/// A duel in the shapes the rules stream today (`duel.scene` with `a_start` / `b_start`, `duel.panel` with
/// `contacts[].on = "b"`, a zone in `at`, `impulse` 0..1), through the live seam.
#[test]
fn a_server_shaped_duel_through_the_live_seam() {
    let scene = serde_json::json!({ "event": "duel.scene", "duel_id": "duel:t:1", "a_start": [-1, 0], "b_start": [2, 0], "objects": [
        { "id": "crate_a", "kind": "crate", "at": [2, 1], "parts": [{ "shape": "box", "size": [0.8, 0.8, 0.8], "offset": [0, 0.4, 0], "material": "wood", "fill": 0.15 }] }] });
    let fighter = |segs: serde_json::Value| serde_json::json!({ "at": [0, 0], "segs": segs });
    let panels = vec![
        serde_json::json!({ "event": "duel.panel", "n": 0, "kind": "intro", "play_ms": 2600, "a": fighter(serde_json::json!([{ "clip": "idle_guard", "ms": 2600 }])), "b": fighter(serde_json::json!([{ "clip": "idle_guard", "ms": 2600 }])), "contacts": [] }),
        serde_json::json!({ "event": "duel.panel", "n": 1, "kind": "exchange", "play_ms": 2600,
            "a": fighter(serde_json::json!([{ "clip": "run", "loop": true, "ms": 390, "path": "dash", "to": [0, 0], "face": "foe" }, { "clip": "strike_side", "ms": 910, "face": "foe" }, { "clip": "idle_guard", "ms": 1300 }])),
            "b": fighter(serde_json::json!([{ "clip": "idle_guard", "ms": 2600 }])),
            "contacts": [{ "at": "belly", "by": "a", "impulse": 0.25, "kind": "hand", "limb": "hand_r", "miss": false, "on": "b", "ranged": false, "t_ms": 715 },
                         { "at": null, "by": "b", "impulse": 0.5, "kind": "kick", "limb": "foot_r", "miss": false, "on": "crate_a", "ranged": false, "t_ms": 1800 }] }),
        serde_json::json!({ "event": "duel.panel", "n": 2, "kind": "outro", "play_ms": 2600, "a": fighter(serde_json::json!([])), "b": fighter(serde_json::json!([])), "contacts": [] }),
    ];
    let skins = ["obsidian", "gold"];
    let live = fn7_duelsim::live_with_sims("duel:t:1", &scene, skins, 42, &panels, 0);
    let f = fn7_duelsim::fighters_of(&scene, skins[0], skins[1]);
    let plain = with_sims(&scene, &f, fn7_duelsim::seed32(42), &panels, 0);
    assert_eq!(live, plain, "the cache gives the same bytes");
    let sim = &live[1]["sim"];
    // a's punch: a strike target on b's belly anchor (1.04 m up), on "b", from 200 ms before the contact
    let la = &sim["fighters"]["a"]["limbs"].as_array().unwrap().iter().find(|l| l[5] == "strike").expect("a strike target").clone();
    assert_eq!((la[0].as_i64(), la[4].as_str(), la[12].as_str(), la[13].as_str()), (Some(515), Some("hand_r"), Some("b"), Some("belly")));
    assert_eq!(la[7].as_i64(), Some(1040), "y in mm");
    // b's kick on the crate: the crate moves, so it has a keyframe track and a hit event by b
    assert!(sim["events"].as_array().unwrap().iter().any(|e| e["e"] == "hit" && e["obj"] == "crate_a" && e["by"] == "b"));
    assert!(sim["bodies"].as_array().unwrap().iter().any(|b| b["obj"] == "crate_a"));
    let pr = fn7_duelsim::live_present("duel:t:1", &scene, skins, 42, &panels[..2]);
    assert_eq!(pr["sim"], live[1]["sim"]);
    assert_eq!(pr["at"]["n"], 1);
}

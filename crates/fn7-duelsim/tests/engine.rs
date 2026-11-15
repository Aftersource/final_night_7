//! Each interaction of the engine on a small stage: skid, tumble, topple, break by strength, shatter, stacks,
//! projectiles, a blast, a fighter's body and a fighter's landing.

use fn7_duelsim::bake::{FighterSpec, Sim};
use fn7_duelsim::math::*;
use fn7_duelsim::physics::{Ev, Tag, With, World};
use fn7_duelsim::props::Scene;
use serde_json::{json, Value};

fn world(objects: Value) -> (Scene, World) { let sc = Scene::derive(&json!({ "objects": objects })); let w = World::new(&sc, 7); (sc, w) }
fn run(w: &mut World, secs: f64) { for _ in 0..(secs * 120.0) as usize { w.step(); } }
fn tilt(w: &World, b: usize) -> f64 { qrot(w.bodies[b].q, UP)[1].clamp(-1.0, 1.0).acos().to_degrees() }
fn crate_() -> Value { json!({ "id": "crate", "at": [0, 0], "parts": [{ "shape": "box", "size": [0.8, 0.8, 0.8], "offset": [0, 0.4, 0], "material": "wood", "fill": 0.15 }] }) }
fn count(w: &World, k: Ev) -> usize { w.events.iter().filter(|e| e.kind == k).count() }

#[test]
fn a_body_at_rest_stays_at_rest_and_a_dropped_one_lands() {
    let (_, mut w) = world(json!([crate_(), { "id": "ball", "at": [2, 0], "parts": [{ "shape": "sphere", "size": [0.25], "offset": [0, 0.25, 0], "material": "iron" }] }]));
    let p0 = w.bodies[0].p;
    run(&mut w, 1.0);
    assert_eq!(w.bodies[0].p, p0, "nothing touched it: not one bit moved");
    // lift the crate 1 m and let go
    w.bodies[0].p[1] += 1.0; w.bodies[0].asleep = false;
    run(&mut w, 0.3);
    assert!(w.bodies[0].v[1] < -2.0, "falling under g: {:?}", w.bodies[0].v);
    run(&mut w, 2.5);
    assert!(w.bodies[0].asleep && (w.bodies[0].p[1] - 0.4).abs() < 0.01, "landed and asleep on the floor: y {}", w.bodies[0].p[1]);
    assert!(w.events.iter().any(|e| e.kind == Ev::Hit && e.with == With::Floor && e.sim), "the landing is a hit on the floor");
}

#[test]
fn a_low_push_skids_a_high_one_tumbles() {
    // 50 kg crate, mu = sqrt(0.5 * 0.6) = 0.55
    let (_, mut w) = world(json!([crate_()]));
    let m = w.bodies[0].m;
    w.strike(0, [m * 3.0, 0.0, 0.0], [-0.4, 0.4, 0.0], Tag::default());
    let mut worst = 0.0f64;
    for _ in 0..360 { w.step(); worst = worst.max(tilt(&w, 0)); }
    let d = w.bodies[0].p[0];
    // v^2 / (2 mu g) = 9 / (2 * 0.548 * 9.81) = 0.84 m
    assert!(d > 0.55 && d < 1.0, "skid distance {d}");
    println!("skid {d} tilt {worst}");
    assert!(worst < 12.0 && w.bodies[0].asleep, "it slid upright (most tilt {worst})");

    let (_, mut w) = world(json!([crate_()]));
    w.strike(0, [m * 5.0, 0.0, 0.0], [-0.4, 0.78, 0.0], Tag::default());
    let mut worst = 0.0f64;
    for _ in 0..480 { w.step(); worst = worst.max(tilt(&w, 0)); }
    assert!(worst > 60.0, "hit at the top edge it goes over: most tilt {worst}");
    assert!(w.bodies[0].asleep, "and comes to rest");
}

#[test]
fn a_tall_thing_topples_a_squat_one_does_not() {
    let tall = json!({ "id": "post", "at": [0, 0], "parts": [{ "shape": "box", "size": [0.3, 2.0, 0.3], "offset": [0, 1.0, 0], "material": "wood" }] });
    let squat = json!({ "id": "block", "at": [3, 0], "parts": [{ "shape": "box", "size": [1.2, 0.6, 1.2], "offset": [0, 0.3, 0], "material": "wood", "fill": 0.21 }] });
    let (sc, mut w) = world(json!([tall, squat]));
    assert!(sc.objects[0].tip_angle < 9.0 && sc.objects[1].tip_angle > 60.0, "tipping angles {} {}", sc.objects[0].tip_angle, sc.objects[1].tip_angle);
    let (m0, m1) = (w.bodies[0].m, w.bodies[1].m);
    assert!((m0 - m1).abs() < 15.0, "about the same mass: {m0} {m1}");
    let (p0, p1) = (w.bodies[0].p, w.bodies[1].p);
    w.strike(0, [m0 * 1.2, 0.0, 0.0], [p0[0] - 0.15, 1.9, p0[2]], Tag::default());
    w.strike(1, [m1 * 1.2, 0.0, 0.0], [p1[0] - 0.6, 0.55, p1[2]], Tag::default());
    run(&mut w, 5.0);
    assert!(tilt(&w, 0) > 80.0, "the post is down: {}", tilt(&w, 0));
    assert!(tilt(&w, 1) < 3.0, "the block stands: {}", tilt(&w, 1));
}

fn pillar() -> Value { json!({ "id": "pillar", "at": [0, 0], "fixed": true, "parts": [
    { "shape": "box", "size": [0.9, 0.35, 0.9], "offset": [0, 0.175, 0], "material": "stone" }, { "shape": "cylinder", "size": [0.3, 1.5], "offset": [0, 1.1, 0], "material": "stone" },
    { "shape": "cylinder", "size": [0.3, 1.5], "offset": [0, 2.6, 0], "material": "stone" }, { "shape": "box", "size": [0.8, 0.28, 0.8], "offset": [0, 3.49, 0], "material": "stone" }] }) }

#[test]
fn a_part_breaks_by_its_strength() {
    // a stone drum of radius 0.3: section pi r^2 = 0.283 m^2, strength 5200 -> it takes 1470 N s to break, 588 to crack, under 221 nothing
    let jb = 5200.0 * PI * 0.09;
    let (_, mut w) = world(json!([pillar()]));
    w.strike(0, [0.14 * jb, 0.0, 0.0], [-0.3, 1.3, 0.0], Tag::default());
    assert_eq!(w.bodies[0].parts[1].damage, 0.0, "a blow under 15 % leaves no mark");
    w.strike(0, [0.5 * jb, 0.0, 0.0], [-0.3, 1.3, 0.0], Tag::default());
    assert!(w.bodies[0].parts[1].cracked && count(&w, Ev::Crack) == 1 && count(&w, Ev::Break) == 0, "half its strength cracks it");
    assert!((w.bodies[0].parts[1].damage - 0.5).abs() < 1e-9);
    // the next blow (damage adds up past 1) breaks it: the drum and everything above it come loose
    w.strike(0, [2.5 * jb, 0.0, 0.0], [-0.3, 1.3, 0.0], Tag::default());
    let e = w.events.iter().find(|e| e.kind == Ev::Break).expect("damage adds up: it breaks");
    assert_eq!(e.made.len(), 3, "the struck drum and everything above it come off");
    assert!(w.bodies[0].parts[0].dead == false && w.bodies[0].parts[1].dead && w.bodies[0].parts[2].dead && w.bodies[0].parts[3].dead, "the base stays");
    assert!(!w.bodies[0].dead && w.bodies[0].fixed);
    let made = e.made.clone();
    assert!(made.iter().all(|b| w.bodies[*b].piece && !w.bodies[*b].fixed));
    let v0 = w.bodies[made[0]].v[0];
    assert!((v0 - 2.5 * jb / w.bodies[made[0]].m).abs() < 1e-9, "the blow moves the loose drum: {v0}");
    let before: Vec<V3> = made.iter().map(|b| w.bodies[*b].p).collect();
    run(&mut w, 6.0);
    // Coulomb: the drum is pinned between the base and the 1.5 t stack above it, mu = 0.65 on both faces, so it
    // decelerates at mu g (m_below_load + m_above_load) / m_drum and slides only v^2 / 2a before it stops
    let (md, mab) = (w.bodies[made[0]].m, w.bodies[made[1]].m + w.bodies[made[2]].m);
    let a = 0.65 * G * ((md + mab) + mab) / md;
    let want = v0 * v0 / (2.0 * a);
    let slid = hypot2(w.bodies[made[0]].p[0] - before[0][0], w.bodies[made[0]].p[2] - before[0][2]);
    assert!(slid > 0.4 * want && slid < 2.5 * want, "a pinned drum slides about v^2/2a = {want:.3} m: {slid:.3} m");
    assert!(w.bodies[made[0]].p[0] - before[0][0] > 0.02, "along the blow");
    assert!((w.bodies[made[0]].p[1] - before[0][1]).abs() < 0.02, "the struck drum still stands on the base");
    // the shear at the foot of the tall, slender drum above may tip it off (it is only 0.6 m wide and 1.5 m tall);
    // whatever happens, nothing rises and everything comes to rest
    for (i, b) in made.iter().enumerate() { assert!(w.bodies[*b].p[1] < before[i][1] + 0.02 && len(w.bodies[*b].v) < 0.5, "piece {b}: {:?}", w.bodies[*b].p); }
    // one tough block on its own only cracks
    let (_, mut w) = world(json!([{ "id": "rock", "at": [0, 0], "parts": [{ "shape": "box", "size": [0.5, 0.5, 0.5], "offset": [0, 0.25, 0], "material": "stone" }] }]));
    w.strike(0, [9000.0, 0.0, 0.0], [-0.25, 0.25, 0.0], Tag::default());
    assert!(count(&w, Ev::Crack) == 1 && count(&w, Ev::Break) == 0 && w.bodies.len() == 2, "a single tough block cracks, it does not come apart");
}

#[test]
fn brittle_things_shatter_into_their_shards() {
    let (sc, mut w) = world(json!([{ "id": "pane", "at": [0, 0], "parts": [
        { "shape": "box", "size": [1.3, 1.5, 0.008], "offset": [0, 0.9, 0], "material": "glass" }, { "shape": "box", "size": [1.45, 0.14, 0.34], "offset": [0, 0.07, 0], "material": "wood", "fill": 0.4 }] }]));
    let glass = sc.objects[0].parts[0].prim.vol;
    // glass: strength 450 x the smallest section (1.3 x 0.008) = 4.7 N s
    w.strike(0, [0.0, 0.0, 30.0], [0.0, 1.0, -0.004], Tag::default());
    let e = w.events.iter().find(|e| e.kind == Ev::Shatter).expect("the pane shatters");
    assert_eq!(e.made.len(), 12);
    let vol: f64 = e.made.iter().map(|b| w.bodies[*b].parts[0].prim.vol).sum();
    assert!((vol - glass).abs() < 1e-9, "the shards are the pane: {vol} of {glass}");
    assert!(e.made.iter().all(|b| w.bodies[*b].frag));
    assert!(w.bodies[0].parts[0].dead && !w.bodies[0].dead, "the pane is gone, the wooden foot is still a body");
    let made = e.made.clone();
    run(&mut w, 5.0);
    assert!(made.iter().all(|b| w.bodies[*b].p[1] < 0.2), "the shards are on the floor");
    assert_eq!(count(&w, Ev::Shatter), 1, "shards do not shatter again");
    // a clay jar on its own is gone entirely
    let (_, mut w) = world(json!([{ "id": "jar", "at": [0, 0], "parts": [{ "shape": "cylinder", "size": [0.25, 0.6], "offset": [0, 0.3, 0], "material": "clay" }] }]));
    w.strike(0, [400.0, 0.0, 0.0], [-0.25, 0.3, 0.0], Tag::default());
    assert!(w.bodies[0].dead && count(&w, Ev::Shatter) == 1 && w.bodies.len() == 2 + 6, "6 clay shards and the jar is no more");
}

#[test]
fn a_stack_stands_and_falls_when_its_support_goes() {
    let c = |id: &str, on: Option<&str>, s: f64| { let mut v = json!({ "id": id, "at": [0, 0], "parts": [{ "shape": "box", "size": [s, 0.7, s], "offset": [0, 0.35, 0], "material": "wood", "fill": 0.12 }] }); if let Some(o) = on { v["on"] = o.into(); } v };
    let (_, mut w) = world(json!([c("a", None, 0.85), c("b", Some("a"), 0.75), c("c", Some("b"), 0.65)]));
    for b in &mut w.bodies { b.asleep = false; }
    run(&mut w, 3.0);
    assert!((w.bodies[1].p[1] - 1.05).abs() < 0.01 && (w.bodies[2].p[1] - 1.75).abs() < 0.015, "the stack holds: {} {}", w.bodies[1].p[1], w.bodies[2].p[1]);
    assert!(w.bodies.iter().take(3).all(|b| b.asleep && tilt(&w, b.id) < 1.0));
    // knock the bottom one out
    let m = w.bodies[0].m;
    w.strike(0, [m * 9.0, 0.0, 0.0], [-0.42, 0.3, 0.0], Tag::default());
    run(&mut w, 4.0);
    assert!(w.bodies[2].p[1] < 1.2, "the top crate came down: y {}", w.bodies[2].p[1]);
}

fn duel(objects: Value, a: [f64; 2], b: [f64; 2], skins: [&str; 2]) -> Sim {
    Sim::new(&json!({ "objects": objects }), &[FighterSpec::new(skins[0], a), FighterSpec::new(skins[1], b)], 7)
}
fn evs(p: &fn7_duelsim::PanelBake, k: Ev) -> Vec<&fn7_duelsim::physics::Event> { p.events.iter().filter(|e| e.kind == k).collect() }

#[test]
fn a_strike_on_an_object_lands_on_its_surface_with_the_fighters_mass() {
    let stage = || json!([{ "id": "crate", "at": [0, 0], "parts": [{ "shape": "box", "size": [0.8, 0.8, 0.8], "offset": [0, 0.4, 0], "material": "wood", "fill": 0.15 }] }]);
    let panel = json!({ "play_ms": 2400, "a": { "segs": [{ "clip": "kick", "ms": 2400 }] }, "b": { "segs": [{ "clip": "idle_guard", "ms": 2400 }] },
        "contacts": [{ "t_ms": 600, "by": "a", "limb": "foot_r", "kind": "kick", "on": "crate", "power": 1.0, "y": 0.4 }] });
    let mut far = vec![];
    for skin in ["wood", "gold"] {
        let mut s = duel(stage(), [-0.8, 0.0], [4.0, 0.0], [skin, "wood"]);
        let p = s.panel(&panel);
        let hit = evs(&p, Ev::Hit).into_iter().find(|e| !e.sim).expect("the kick lands");
        let f = &s.fb[0];
        assert!((hit.j - 0.25 * f.mass * 11.0 * f.speed).abs() < 1e-6, "a kick is a quarter of the body at 11 m/s: {}", hit.j);
        assert!((hit.at[0] + 0.4).abs() < 1e-6 && (hit.at[1] - 0.4).abs() < 1e-9, "on the crate's near face at the height of the kick: {:?}", hit.at);
        // the limb target is that very point, with the face's normal, 200 ms before to 180 ms after
        let t = p.fighters[0].limbs.iter().find(|l| l.kind == "strike").expect("a limb target");
        assert_eq!(t.limb, "foot_r"); assert_eq!(t.p, hit.at); assert!(t.n[0] < -0.99); assert_eq!((t.t0_ms, t.t1_ms), (400.0, 780.0));
        far.push(s.world.bodies[0].p[0]);
    }
    assert!(far[0] > 0.3 && far[1] > far[0] * 1.8, "gold kicks the same crate much farther: {far:?}");
}

#[test]
fn a_fighters_body_pushes_what_it_runs_into_and_is_not_pushed_back() {
    let stage = json!([{ "id": "crate", "at": [0, 0], "parts": [{ "shape": "box", "size": [0.8, 0.8, 0.8], "offset": [0, 0.4, 0], "material": "wood", "fill": 0.15 }] }]);
    let mut s = duel(stage, [-3.0, 0.0], [5.0, 0.0], ["gold", "wood"]);
    let p = s.panel(&json!({ "play_ms": 2400, "a": { "segs": [{ "clip": "run", "ms": 1200, "to": [2, 0], "path": "dash" }, { "clip": "idle_guard", "ms": 1200 }] }, "b": { "segs": [] } }));
    let t = evs(&p, Ev::Touch);
    assert!(!t.is_empty() && t[0].with == With::Fighter(0), "the runner touches the crate");
    assert!(s.world.bodies[0].p[0] > 1.0, "and shoves it ahead: x {}", s.world.bodies[0].p[0]);
    assert_eq!(s.cur[0].p, hex_to_world([2.0, 0.0]), "the fighter ends exactly where the script says");
}

#[test]
fn a_knocked_fighter_landing_on_an_object_hits_it_with_its_weight() {
    let couch = json!([{ "id": "couch", "at": [3, 0], "parts": [{ "shape": "box", "size": [1.9, 0.5, 0.9], "offset": [0, 0.25, 0], "material": "cloth", "fill": 0.25 }] }]);
    let panel = json!({ "play_ms": 2400, "a": { "segs": [] }, "b": { "segs": [{ "clip": "airborne_hit", "ms": 900, "to": [3, 0], "path": "knock", "onto": "couch", "h": 0.4 }, { "clip": "downed", "ms": 1500 }] } });
    let mut js = vec![];
    for skin in ["butterfly", "gold"] {
        let mut s = duel(couch.clone(), [-3.0, 0.0], [0.0, 0.0], ["wood", skin]);
        let p = s.panel(&panel);
        let land = p.events.iter().find(|e| e.why == "landing").expect("the landing is an impulse on the couch");
        assert_eq!(land.by, Some(1));
        assert!((s.cur[1].p[1] - 0.5).abs() < 0.02, "it lies on the couch's top: y {}", s.cur[1].p[1]);
        js.push(land.j);
    }
    assert!(js[1] > js[0] * 5.0, "a gold body lands far harder than a paper one: {js:?}");
}

#[test]
fn projectiles_hit_what_is_in_the_way_and_a_rocket_blasts() {
    let stage = |jar: [f64; 2]| json!([
        { "id": "jar", "at": jar, "parts": [{ "shape": "cylinder", "size": [0.25, 0.9], "offset": [0, 0.45, 0], "material": "clay" }] },
        { "id": "crate_l", "at": [3, -1], "parts": [{ "shape": "box", "size": [0.7, 0.7, 0.7], "offset": [0, 0.35, 0], "material": "wood", "fill": 0.14 }] },
        { "id": "crate_r", "at": [2, 1], "parts": [{ "shape": "box", "size": [0.7, 0.7, 0.7], "offset": [0, 0.35, 0], "material": "wood", "fill": 0.14 }] }]);
    // an arrow at the jar: 0.05 kg at 60 m/s = 3 N s, far under what a clay jar this thick takes
    let mut s = duel(stage([0.0, 0.0]), [-3.0, 0.0], [5.0, 0.0], ["wood", "wood"]);
    let p = s.panel(&json!({ "play_ms": 1200, "a": { "segs": [] }, "b": { "segs": [] }, "projectiles": [{ "t_ms": 100, "by": "a", "kind": "arrow", "on": "jar", "y": 0.6 }] }));
    assert_eq!(p.shots.len(), 1);
    assert!(p.shots[0].t1_ms > p.shots[0].t0_ms && (p.shots[0].to[0] - (-0.25)).abs() < 0.05, "the arrow stops at the jar's near side: {:?}", p.shots[0].to);
    let hit = p.events.iter().find(|e| e.why == "shot").expect("it arrives as a hit");
    assert!((hit.j - 3.0).abs() < 1e-9 && evs(&p, Ev::Shatter).is_empty());
    // a rocket (2 kg at 40 m/s, a 320 N s blast) at the same jar: it bursts on the jar, the jar is gone, and the crates
    // either side of the line (1.7 m off, inside its 2.5 m radius) are thrown outward, away from each other
    let mut s = duel(stage([2.0, 0.0]), [-3.0, 0.0], [5.0, 2.0], ["wood", "wood"]);
    let (l0, r0) = (s.world.bodies[1].p, s.world.bodies[2].p);
    let p = s.panel(&json!({ "play_ms": 2400, "a": { "segs": [] }, "b": { "segs": [] }, "projectiles": [{ "t_ms": 100, "by": "a", "kind": "rocket", "to": [2.5, 0], "y": 0.6 }] }));
    assert_eq!(p.shots[0].kind, "rocket");
    assert_eq!(evs(&p, Ev::Shatter).len(), 1, "the jar in its way is gone");
    let (l1, r1) = (s.world.bodies[1].p, s.world.bodies[2].p);
    println!("crates {l0:?} -> {l1:?}, {r0:?} -> {r1:?}");
    assert!(l1[2] < l0[2] - 0.1 && r1[2] > r0[2] + 0.1 && l1[0] > l0[0] && r1[0] > r0[0], "the blast throws the crates away from the jar: {l0:?} -> {l1:?}, {r0:?} -> {r1:?}");
}

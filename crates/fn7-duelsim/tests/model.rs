//! The model against hand-computed values: primitives, compound bodies, tipping, the fighters, the affordances.

use fn7_duelsim::materials::{fighter_body, is_material, mat, table};
use fn7_duelsim::math::*;
use fn7_duelsim::props::Scene;
use fn7_duelsim::shapes::primitive;
use serde_json::{json, Value};

fn near(a: f64, b: f64, tol: f64) -> bool { (a - b).abs() <= tol }
fn lab() -> Value { serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../duel_lab/duel.json")).unwrap()).unwrap() }

#[test]
fn our_own_trig_is_accurate() {
    for i in -2000..2000 {
        let x = i as f64 * 0.01;
        assert!(near(sin(x), x.sin(), 1e-14) && near(cos(x), x.cos(), 1e-14), "sin/cos at {x}");
        let y = (i as f64 * 0.37).sin() * 3.0;
        assert!(near(atan2(y, x), y.atan2(x), 1e-13) || x == 0.0, "atan2({y}, {x})");
    }
    for x in [0.001, 0.5, 0.7853981633974483, 1.0, 8.0, 27.0, 1000.0] { assert!(near(cbrt(x), x.cbrt(), 1e-12 * x.max(1.0)), "cbrt {x}"); }
    assert_eq!(sincos(0.0), (0.0, 1.0));
}

#[test]
fn rng_is_the_labs_mulberry32() {
    // node: const r = rng(7); [r(), r(), r()]
    let mut r = Rng(7);
    let got = [r.next(), r.next(), r.next()];
    let want: [f64; 3] = [0.011704753153026104, 0.06195825757458806, 0.97690763277933];
    for i in 0..3 { assert_eq!(got[i].to_bits(), want[i].to_bits(), "draw {i}: {}", got[i]); }
}

#[test]
fn primitive_volumes_and_centres() {
    let b = primitive("box", &[0.85, 0.75, 0.85]);
    assert!(near(b.vol, 0.541875, 1e-12) && b.com == [0.0; 3]);
    assert!(near(b.i[4], (0.85 * 0.85 + 0.85 * 0.85) / 12.0, 1e-12) && near(b.i[0], (0.75 * 0.75 + 0.85 * 0.85) / 12.0, 1e-12));
    assert!(near(b.section, 0.75 * 0.85, 1e-12), "the smallest cross-section of a box is its two shortest sides");
    assert_eq!(b.verts.len(), 20, "8 corners and 12 edge midpoints");
    let c = primitive("cylinder", &[0.3, 0.75]);
    assert!(near(c.vol, PI * 0.09 * 0.75, 1e-12) && near(c.i[4], 0.045, 1e-12) && near(c.i[0], (3.0 * 0.09 + 0.5625) / 12.0, 1e-12));
    assert!(near(c.section, PI * 0.09, 1e-12));
    let s = primitive("sphere", &[0.2]);
    assert!(near(s.vol, 4.0 / 3.0 * PI * 0.008, 1e-12) && near(s.i[0], 0.4 * 0.04, 1e-12));
    // a cone on a regular 10-gon: V = base * h / 3, centre of mass a quarter of the way up
    let k = primitive("cone", &[0.8, 1.2]);
    let base = 5.0 * 0.64 * (2.0 * PI / 10.0).sin();
    assert!(near(k.vol, base * 1.2 / 3.0, 1e-9), "cone volume {}", k.vol);
    assert!(near(k.com[1], -0.3, 1e-9) && near(k.com[0], 0.0, 1e-9) && near(k.com[2], 0.0, 1e-9), "cone centre {:?}", k.com);
    // the platonic solids of circumradius 1
    let a = (8.0f64 / 3.0).sqrt();
    assert!(near(primitive("tetra", &[1.0]).vol, a * a * a / (6.0 * 2.0f64.sqrt()), 1e-9));
    assert!(near(primitive("octa", &[1.0]).vol, 4.0 / 3.0, 1e-9));
    let a = 1.0 / (2.0 * PI / 5.0).sin();
    assert!(near(primitive("ico", &[1.0]).vol, 5.0 * (3.0 + 5.0f64.sqrt()) / 12.0 * a * a * a, 1e-9));
    let a = 4.0 / (3.0f64.sqrt() * (1.0 + 5.0f64.sqrt()));
    assert!(near(primitive("dodeca", &[1.0]).vol, (15.0 + 7.0 * 5.0f64.sqrt()) / 4.0 * a * a * a, 1e-9));
    for (shape, faces) in [("tetra", 4), ("octa", 8), ("ico", 20), ("dodeca", 12)] {
        let p = primitive(shape, &[0.5]);
        assert_eq!(p.planes.len(), faces, "{shape}");
        assert!(len(p.com) < 1e-9, "{shape} is centred");
        // a platonic solid's inertia is isotropic
        assert!(near(p.i[0], p.i[4], 1e-9) && near(p.i[4], p.i[8], 1e-9) && p.i[1].abs() < 1e-9, "{shape} inertia {:?}", p.i);
    }
}

#[test]
fn compound_mass_centre_inertia() {
    let d = lab();
    let sc = Scene::derive(&d["scene"]);
    let o = |id: &str| &sc.objects[sc.index(id).unwrap()];
    // the pillar: a base, four drums, a capital, all stone (2500 kg/m^3)
    let drum = PI * 0.09 * 0.75;
    let vol = 0.9 * 0.35 * 0.9 + 4.0 * drum + 0.8 * 0.28 * 0.8;
    let p = o("pillar");
    assert!(near(p.mass, 2500.0 * vol, 1e-6), "pillar mass {}", p.mass);
    let y = (0.2835 * 0.175 + drum * (0.725 + 1.475 + 2.225 + 2.975) + 0.1792 * 3.49) / vol;
    assert!(near(p.com_local[1], y, 1e-9) && near(p.com_local[0], 0.0, 1e-12), "pillar centre {:?} want y {y}", p.com_local);
    assert!(near(p.aabb[1][1], 3.63, 1e-9));
    // a crate: wood, 12 % solid
    let c = o("crate_a");
    assert!(near(c.mass, 650.0 * 0.85 * 0.75 * 0.85 * 0.12, 1e-9), "crate mass {}", c.mass);
    assert!(near(c.com[1], 0.375, 1e-9));
    assert!(near(c.tip_angle, (0.425f64 / 0.375).atan().to_degrees(), 1e-6), "a crate tips when its centre passes its edge: {}", c.tip_angle);
    assert!(near(c.inertia[4], c.mass * (0.85 * 0.85 * 2.0) / 12.0, 1e-9), "Iyy of a box");
    // stacked: crate_c sits on crate_a's top, crate_f on crate_c
    assert!(near(o("crate_c").p[1], 0.75, 1e-9) && near(o("crate_f").p[1], 1.45, 1e-9));
    assert!(near(o("jar_a").p[1], 0.94, 1e-9), "the jar stands on the desk top");
    // the desk stands on four legs: the support polygon is their footprint, and it tips later along its length
    let k = o("desk");
    assert!(k.support.len() >= 4 && near(k.base_y, 0.0, 1e-9));
    let top = 650.0 * 2.1 * 0.08 * 0.85 * 0.5; let leg = 650.0 * 0.09 * 0.86 * 0.09;
    assert!(near(k.mass, top + 4.0 * leg, 1e-9));
    assert!(near(k.com_local[1], (top * 0.9 + 4.0 * leg * 0.43) / (top + 4.0 * leg), 1e-9));
    let want = ((0.34f64 + 0.045) / k.com_local[1]).atan().to_degrees();
    assert!(near(k.tip_angle, want, 1e-6), "desk tips at {} want {want}", k.tip_angle);
    assert!(k.tip_energy > 0.0);
}

#[test]
fn parallel_axis_two_halves_make_the_whole() {
    let two = Scene::derive(&json!({ "objects": [{ "id": "x", "at": [0, 0], "parts": [
        { "shape": "box", "size": [0.5, 0.4, 0.6], "offset": [-0.25, 0.2, 0], "material": "stone" }, { "shape": "box", "size": [0.5, 0.4, 0.6], "offset": [0.25, 0.2, 0], "material": "stone" }] }] }));
    let one = Scene::derive(&json!({ "objects": [{ "id": "x", "at": [0, 0], "parts": [{ "shape": "box", "size": [1.0, 0.4, 0.6], "offset": [0, 0.2, 0], "material": "stone" }] }] }));
    let (a, b) = (&two.objects[0], &one.objects[0]);
    assert!(near(a.mass, b.mass, 1e-9));
    for i in 0..9 { assert!(near(a.inertia[i], b.inertia[i], 1e-9), "inertia[{i}] {} vs {}", a.inertia[i], b.inertia[i]); }
    for i in 0..3 { assert!(near(a.com[i], b.com[i], 1e-12)); }
}

#[test]
fn fighters_weigh_what_their_skin_is_made_of() {
    let kg = |s: &str| fighter_body(s, "mannequin").mass;
    // the lab's numbers
    assert_eq!((kg("gold").round(), kg("obsidian").round(), kg("wood").round(), kg("butterfly").round(), kg("dragonscale").round()), (178.0, 63.0, 43.0, 16.0, 95.0));
    assert!(near(kg("gold"), 19300.0 * 0.14 * 0.066, 1e-9));
    assert_eq!(kg("no such skin"), kg("wood"));
    let (g, w, b) = (fighter_body("gold", "mannequin"), fighter_body("wood", "mannequin"), fighter_body("butterfly", "mannequin"));
    assert!(near(g.jump, (43.0 / g.mass).sqrt(), 1e-12) && near(g.jump, 0.49, 0.005), "gold jumps half as high: {}", g.jump);
    assert!(near(w.jump, 1.0, 0.002) && near(w.speed, 1.0, 0.001));
    assert_eq!(b.jump, 1.25); assert_eq!(b.speed, 1.15);
    // a heavy blow: half the body's mass at 12 m/s times the speed scale: gold hits about three times as hard as wood
    let j = |f: &fn7_duelsim::materials::FighterBody| 0.5 * f.mass * 12.0 * f.speed;
    assert!(near(j(&g) / j(&w), 2.9, 0.1), "gold / wood strike impulse {}", j(&g) / j(&w));
    // per-segment masses add up; the centre of mass of the standing figure is at the hips
    let seg = g.segment_masses();
    assert_eq!(seg.len(), 22);
    assert!(near(seg.iter().map(|s| s.1).sum::<f64>(), g.mass, 1e-9));
    let c = g.com_rest();
    assert!(c[0].abs() < 1e-9 && c[1] > 0.9 && c[1] < 1.02, "standing centre of mass {c:?}");
    // every skin (those to come too) names a material in the table
    for s in &table().skins { assert!(is_material(&s.material), "{}", s.name); assert!(kg(&s.name) > 5.0 && kg(&s.name) < 400.0); }
}

#[test]
fn the_scene_generators_materials_are_in_the_table() {
    // every material name fn7-plugin's scene generator writes must be in data/materials.json
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../fn7-plugin/src/panel.rs")).unwrap();
    let mut seen = vec![];
    for (i, _) in src.match_indices("\"material\": \"") { let rest = &src[i + 13..]; seen.push(rest[..rest.find('"').unwrap()].to_string()); }
    for line in src.lines().filter(|l| l.contains("part(\"")) {
        for chunk in line.split("part(\"").skip(1) { if let Some(end) = chunk.find(")") { let args = &chunk[..end]; if let Some(q) = args.rfind(", \"") { seen.push(args[q + 3..].trim_end_matches('"').to_string()); } } }
    }
    seen.sort(); seen.dedup();
    assert!(seen.len() >= 5, "found the generator's materials: {seen:?}");
    for m in &seen { assert!(is_material(m), "scene generator material {m:?} is not in data/materials.json"); }
    assert_eq!(mat("unknown stuff").name, "wood");
    assert!(mat("glass").brittle && mat("glass").shards == 12 && !mat("stone").brittle);
}

#[test]
fn affordances_of_the_lab_scene() {
    let d = lab();
    let sc = Scene::derive(&d["scene"]);
    let has = |kind: &str, id: &str| sc.affordances.iter().any(|a| a.kind == kind && sc.objects[a.obj].id == id);
    assert!(has("vault", "desk") && has("slide", "desk") && has("vault", "crate_e") && has("vault", "low_wall"));
    assert!(has("mantle", "sacks") && has("perch", "sacks"));
    assert!(has("wallrun", "pillar") && has("wallkick", "pillar") && has("wallrun", "godown_wall") && has("wallkick", "godown_wall"));
    assert!(has("swing", "tree"), "the low branch is a bar");
    assert!(!has("vault", "pillar") && !has("slide", "couch"));
    let up = sc.affordances.iter().find(|a| a.kind == "wallrun" && sc.objects[a.obj].id == "pillar").unwrap();
    assert_eq!(up.mode, "up");
    assert!(sc.affordances.iter().any(|a| a.kind == "wallrun" && sc.objects[a.obj].id == "godown_wall" && a.mode == "along"), "the long face of the godown wall is run along");
    // positions: the desk's vault point is the centre of its top, 0.94 m up
    let v = sc.affordances.iter().find(|a| a.kind == "vault" && sc.objects[a.obj].id == "desk").unwrap();
    let desk = &sc.objects[v.obj];
    assert!(near(v.pos[1], 0.94, 1e-9) && near(v.pos[0], desk.p[0], 1e-9) && near(v.pos[2], desk.p[2], 1e-9) && near(v.height, 0.94, 1e-9));
    let s = sc.affordances.iter().find(|a| a.kind == "slide").unwrap();
    assert!(near(s.height, 0.86, 1e-9), "clearance under the desk top: {}", s.height);
    // the ground under a point: the steps rise 0.25 at a time
    let st = &sc.objects[sc.index("steps").unwrap()];
    assert!(near(sc.ground_at(st.p[0], st.p[2] + 1.5, 9.0), 0.25, 1e-9) && near(sc.ground_at(st.p[0], st.p[2] + 1.0, 9.0), 0.5, 1e-9) && near(sc.ground_at(st.p[0], st.p[2] - 0.75, 9.0), 1.0, 1e-9));
    assert_eq!(sc.ground_at(0.0, 0.0, 9.0), 0.0);
}

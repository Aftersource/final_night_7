//! What is derived once from a scene's `objects[].parts[]`: mass, centre of mass, inertia, bounds, the support polygon
//! and the tipping angle of every object; the scene's surfaces (walkable tops, vertical faces, undersides, ledges,
//! bars) and from those the parkour affordances. A port of `duel_lab/lib/props.js`.

use crate::materials::{mat, Mat};
use crate::math::*;
use crate::shapes::{primitive, Prim};
use serde_json::Value;
use std::sync::Arc;

/// Half extents of the playable floor inside the box rails (metres).
pub const STAGE_HX: f64 = 9.9;
pub const STAGE_HZ: f64 = 7.9;

#[derive(Debug, Clone)]
pub struct SPart {
    pub pi: usize,
    pub prim: Arc<Prim>,
    pub mat: &'static Mat,
    pub fill: f64,
    pub lp: V3,
    pub lq: Q,
    pub wp: V3,
    pub wq: Q,
    pub wv: Vec<V3>,
    pub lo: f64,
    pub hi: f64,
}
#[derive(Debug, Clone)]
pub struct SObj {
    pub id: String,
    pub kind: String,
    pub fixed: bool,
    pub hidden: bool,
    pub p: V3,
    pub q: Q,
    pub parts: Vec<SPart>,
    pub aabb: [V3; 2],
    pub mass: f64,
    pub volume: f64,
    pub com_local: V3,
    pub inertia: M3,
    pub com: V3,
    pub radius: f64,
    pub support: Vec<[f64; 2]>,
    pub base_y: f64,
    /// Degrees of tilt at which the centre of mass passes the edge of the support polygon.
    pub tip_angle: f64,
    pub tip_dir: V3,
    /// Joules to push it over.
    pub tip_energy: f64,
}
#[derive(Debug, Clone)]
pub struct Top { pub obj: usize, pub part: usize, pub c: V3, pub u: V3, pub v: V3, pub round: f64, pub y: f64, pub area: f64, pub walkable: bool }
#[derive(Debug, Clone)]
pub struct Face { pub obj: usize, pub part: usize, pub c: V3, pub n: V3, pub t: V3, pub w: f64, pub h: f64, pub y0: f64, pub y1: f64 }
#[derive(Debug, Clone)]
pub struct Round { pub obj: usize, pub part: usize, pub c: V3, pub r: f64, pub y0: f64, pub y1: f64, pub parts_all: Vec<usize> }
#[derive(Debug, Clone)]
pub struct Under { pub obj: usize, pub part: usize, pub c: V3, pub u: V3, pub v: V3, pub y: f64 }
#[derive(Debug, Clone)]
pub struct Bar { pub obj: usize, pub part: usize, pub c: V3, pub dir: V3, pub half: f64, pub y: f64, pub r: f64 }
#[derive(Debug, Clone)]
pub struct Edge { pub obj: usize, pub part: usize, pub round: f64, pub c: V3, pub p0: V3, pub p1: V3, pub mid: V3, pub out: V3, pub y: f64, pub drop: f64 }
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum On { Top(usize), Edge(usize), Face(usize), Round(usize), Under(usize), Bar(usize) }
#[derive(Debug, Clone)]
pub struct Aff {
    /// vault, perch, mantle, wallrun, wallkick, slide, swing
    pub kind: &'static str,
    pub obj: usize,
    pub part: usize,
    pub pos: V3,
    pub hex: [f64; 2],
    pub on: On,
    /// wallrun: "up" or "along"
    pub mode: &'static str,
    pub height: f64,
}
/// A scene part: (object index, part index).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ref { pub obj: usize, pub part: usize }

#[derive(Debug, Clone)]
pub struct Scene {
    pub objects: Vec<SObj>,
    pub tops: Vec<Top>,
    pub faces: Vec<Face>,
    pub rounds: Vec<Round>,
    pub unders: Vec<Under>,
    pub bars: Vec<Bar>,
    pub edges: Vec<Edge>,
    pub affordances: Vec<Aff>,
}

/// Mass, centre of mass (in the frame the parts are given in) and the inertia tensor about it.
pub fn mass_of(parts: &[(&Prim, &Mat, f64, V3, Q)]) -> (f64, V3, M3) {
    let (mut m, mut c) = (0.0, Z3);
    let mut pm = Vec::with_capacity(parts.len());
    for (prim, mt, fill, lp, lq) in parts {
        let mass = mt.density * prim.vol * fill; let pc = add(*lp, qrot(*lq, prim.com));
        m += mass; c = adds(c, pc, mass); pm.push((mass, pc));
    }
    c = mul(c, 1.0 / if m == 0.0 { 1.0 } else { m });
    let mut i = [0.0; 9];
    for (k, (prim, _, _, _, lq)) in parts.iter().enumerate() {
        let r = m3q(*lq);
        i = m3add(&i, &m3add(&m3mul(&m3mul(&r, &m3scale(&prim.i, pm[k].0)), &m3t(&r)), &m3point(pm[k].0, sub(pm[k].1, c))));
    }
    (m, c, i)
}
fn in_top(t: &Top, x: f64, z: f64) -> bool {
    let d = [x - t.c[0], 0.0, z - t.c[2]];
    if t.round > 0.0 { return hypot2(d[0], d[2]) <= t.round; }
    dot(d, t.u).abs() <= dot(t.u, t.u) && dot(d, t.v).abs() <= dot(t.v, t.v)
}
fn fv(v: &Value, d: f64) -> f64 { v.as_f64().unwrap_or(d) }
fn v3(v: &Value) -> V3 { [fv(&v[0], 0.0), fv(&v[1], 0.0), fv(&v[2], 0.0)] }

fn rails() -> Value {
    serde_json::json!({ "id": "_rails", "kind": "rails", "at": [0, 0], "fixed": true, "hidden": true, "parts": [
        { "shape": "box", "size": [22, 3, 1], "offset": [0, 1.5, STAGE_HZ + 0.5], "material": "wood" }, { "shape": "box", "size": [22, 3, 1], "offset": [0, 1.5, -STAGE_HZ - 0.5], "material": "wood" },
        { "shape": "box", "size": [1, 3, 18], "offset": [STAGE_HX + 0.5, 1.5, 0], "material": "wood" }, { "shape": "box", "size": [1, 3, 18], "offset": [-STAGE_HX - 0.5, 1.5, 0], "material": "wood" }] })
}

impl Scene {
    pub fn index(&self, id: &str) -> Option<usize> { self.objects.iter().position(|o| o.id == id) }
    pub fn in_top(&self, t: &Top, x: f64, z: f64) -> bool { in_top(t, x, z) }
    /// Height of the static scene under a point (the highest top not above max_y).
    pub fn ground_at(&self, x: f64, z: f64, max_y: f64) -> f64 { ground_at(&self.tops, x, z, max_y) }
    pub fn top_at(&self, x: f64, z: f64, max_y: f64) -> Option<usize> {
        let mut b: Option<usize> = None;
        for (i, t) in self.tops.iter().enumerate() { if t.y <= max_y && b.map_or(true, |j| t.y > self.tops[j].y) && in_top(t, x, z) { b = Some(i); } }
        b
    }
    pub fn afford<'a>(&'a self, kind: &'a str, obj: usize) -> impl Iterator<Item = &'a Aff> + 'a { self.affordances.iter().filter(move |a| a.kind == kind && a.obj == obj) }

    /// Derive everything from a `duel.scene` (or the lab's `scene`): `{ objects: [...] }`.
    pub fn derive(spec: &Value) -> Scene {
        let mut sc = Scene { objects: vec![], tops: vec![], faces: vec![], rounds: vec![], unders: vec![], bars: vec![], edges: vec![], affordances: vec![] };
        let mut raw_rounds: Vec<Round> = vec![];
        let mut specs: Vec<Value> = spec["objects"].as_array().cloned().unwrap_or_default();
        specs.push(rails());
        for (oi, s) in specs.iter().enumerate() {
            let q = qaxis(UP, fv(&s["yaw"], 0.0) * DEG);
            let mut p = hex_to_world([fv(&s["at"][0], 0.0), fv(&s["at"][1], 0.0)]);
            let fixed = s["fixed"].as_bool().unwrap_or(false);
            let mut parts: Vec<SPart> = s["parts"].as_array().map(|a| a.iter().enumerate().map(|(pi, ps)| {
                let size: Vec<f64> = match &ps["size"] { Value::Array(a) => a.iter().map(|x| fv(x, 0.5)).collect(), x => vec![fv(x, 0.5)] };
                let rot = if ps["rot"].is_array() { v3(&ps["rot"]) } else { Z3 };
                SPart { pi, prim: Arc::new(primitive(ps["shape"].as_str().unwrap_or("box"), &size)), mat: mat(ps["material"].as_str().unwrap_or("wood")), fill: fv(&ps["fill"], 1.0),
                    lp: if ps["offset"].is_array() { v3(&ps["offset"]) } else { Z3 }, lq: qeuler(rot), wp: Z3, wq: QI, wv: vec![], lo: 0.0, hi: 0.0 }
            }).collect()).unwrap_or_default();
            if parts.is_empty() { continue; }
            let verts = |p: V3, pt: &SPart| -> Vec<V3> { pt.prim.verts.iter().map(|v| add(p, qrot(q, add(pt.lp, qrot(pt.lq, *v))))).collect() };
            // settle: loose things rest exactly on the floor, or on the top of the object named by `on`
            let mut lo = f64::INFINITY;
            for pt in &parts { for v in verts(p, pt) { lo = lo.min(v[1]); } }
            let on = s["on"].as_str().and_then(|id| sc.index(id));
            let under = match on {
                Some(oj) => {
                    let mut y = 0.0f64;
                    for t in &sc.tops { if t.obj == oj && in_top(t, p[0], p[2]) { y = y.max(t.y); } }
                    if y == 0.0 { sc.objects[oj].aabb[1][1] } else { y }
                }
                None => 0.0,
            };
            if !fixed || lo < 0.0 || on.is_some() { p = [p[0], under - lo, p[2]]; }
            let (mut mn, mut mx): (V3, V3) = ([1e9; 3], [-1e9; 3]);
            for pt in &mut parts {
                pt.wp = add(p, qrot(q, pt.lp)); pt.wq = qmul(q, pt.lq);
                pt.wv = pt.prim.verts.iter().map(|v| add(p, qrot(q, add(pt.lp, qrot(pt.lq, *v))))).collect();
                pt.lo = f64::INFINITY; pt.hi = f64::NEG_INFINITY;
                for v in &pt.wv { pt.lo = pt.lo.min(v[1]); pt.hi = pt.hi.max(v[1]); for i in 0..3 { mn[i] = mn[i].min(v[i]); mx[i] = mx[i].max(v[i]); } }
            }
            let (mass, com_local, inertia) = mass_of(&parts.iter().map(|pt| (&*pt.prim, pt.mat, pt.fill, pt.lp, pt.lq)).collect::<Vec<_>>());
            let com = add(p, qrot(q, com_local));
            let (mut radius, mut volume, mut low) = (0.0f64, 0.0, vec![]);
            for pt in &parts { volume += pt.prim.vol * pt.fill; for v in &pt.wv { radius = radius.max(len(sub(*v, com))); if v[1] < mn[1] + 0.05 { low.push([v[0], v[2]]); } } }
            // the support polygon (hull of everything touching the base) and the tilt at which the centre of mass passes its edge
            let support = hull2(&low);
            let (mut tip, mut tip_dir) = (f64::INFINITY, [1.0, 0.0, 0.0]);
            let (n, h) = (support.len(), (com[1] - mn[1]).max(1e-6));
            if n >= 3 { for i in 0..n {
                let (a, b) = (support[i], support[(i + 1) % n]);
                let e = [b[0] - a[0], b[1] - a[1]]; let mut el = hypot2(e[0], e[1]); if el == 0.0 { el = 1e-9; }
                let d = ((com[0] - a[0]) * e[1] - (com[2] - a[1]) * e[0]).abs() / el;
                if d < tip { tip = d; tip_dir = [e[1] / el, 0.0, -e[0] / el]; }
            } } else { tip = 0.0; }
            let o = SObj { id: s["id"].as_str().map(|x| x.to_string()).unwrap_or_else(|| format!("obj{oi}")), kind: s["kind"].as_str().unwrap_or("thing").into(), fixed, hidden: s["hidden"].as_bool().unwrap_or(false),
                p, q, parts, aabb: [mn, mx], mass, volume, com_local, inertia, com, radius, support, base_y: mn[1],
                tip_angle: atan2(tip, h) / DEG, tip_dir, tip_energy: mass * G * (hypot2(tip, h) - h) };
            let hidden = o.hidden;
            sc.objects.push(o);
            if !hidden { let k = sc.objects.len() - 1; sc.surfaces_of(k, &mut raw_rounds); }
        }
        // merge stacked drums of one object into tall runs (a pillar of drums is one wall-run)
        for oi in 0..sc.objects.len() {
            let mut rs: Vec<&Round> = raw_rounds.iter().filter(|r| r.obj == oi).collect();
            rs.sort_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap());
            let mut cur: Option<usize> = None;
            for r in rs {
                if let Some(ci) = cur { let c = &mut sc.rounds[ci];
                    if (r.y0 - c.y1).abs() < 0.05 && hypot2(r.c[0] - c.c[0], r.c[2] - c.c[2]) < 0.05 { c.y1 = r.y1; c.r = c.r.min(r.r); c.parts_all.push(r.part); continue; } }
                sc.rounds.push(r.clone()); cur = Some(sc.rounds.len() - 1);
            }
        }
        // free headroom above a top, and ledges (top edges with a drop beside them)
        for ti in 0..sc.tops.len() {
            let t = sc.tops[ti].clone();
            let blocked = sc.objects.iter().enumerate().any(|(oi, o)| o.parts.iter().any(|p| !(oi == t.obj && p.pi == t.part) && p.lo > t.y - 0.02 && p.lo < t.y + 1.5
                && hypot2(p.wp[0] - t.c[0], p.wp[2] - t.c[2]) < p.prim.half[0].max(p.prim.half[2]) * 0.9));
            let walkable = t.area >= 0.1 && !blocked;
            sc.tops[ti].walkable = walkable;
            if !walkable { continue; }
            if t.round > 0.0 { sc.edges.push(Edge { obj: t.obj, part: t.part, round: t.round, c: t.c, p0: Z3, p1: Z3, mid: Z3, out: Z3, y: t.y, drop: 0.0 }); continue; }
            for (e, f) in [(t.u, t.v), (mul(t.u, -1.0), t.v), (t.v, t.u), (mul(t.v, -1.0), t.u)] {
                let out = norm([e[0], 0.0, e[2]]); let mid = add(t.c, e); let q = adds(mid, out, 0.35);
                let g = ground_at(&sc.tops, q[0], q[2], t.y + 0.3);
                if g < t.y - 0.4 { sc.edges.push(Edge { obj: t.obj, part: t.part, round: 0.0, c: Z3, p0: sub(mid, f), p1: add(mid, f), mid, out, y: t.y, drop: t.y - g }); }
            }
        }
        // ---- affordances: what a body can do with each surface ----
        let mut aff: Vec<Aff> = vec![];
        let mut push = |kind: &'static str, obj: usize, part: usize, pos: V3, on: On, mode: &'static str, height: f64| {
            let h = world_to_hex(pos);
            aff.push(Aff { kind, obj, part, pos, hex: [(h[0] * 100.0 + 0.5).floor() / 100.0, (h[1] * 100.0 + 0.5).floor() / 100.0], on, mode, height });
        };
        for (i, t) in sc.tops.iter().enumerate() {
            if !t.walkable { continue; }
            let thin = if t.round > 0.0 { 2.0 * t.round } else { 2.0 * len(t.u).min(len(t.v)) };
            if t.y >= 0.5 && t.y <= 1.2 && thin <= 1.7 { push("vault", t.obj, t.part, t.c, On::Top(i), "", t.y); }
            if t.y >= 1.0 { push("perch", t.obj, t.part, t.c, On::Top(i), "", t.y); }
        }
        for (i, e) in sc.edges.iter().enumerate() { if e.y > 1.2 && e.y <= 2.6 { push("mantle", e.obj, e.part, if e.round > 0.0 { e.c } else { [e.mid[0], e.y, e.mid[2]] }, On::Edge(i), "", e.y); } }
        for (i, f) in sc.faces.iter().enumerate() {
            if f.y1 < 1.0 || sc.objects[f.obj].hidden { continue; }
            if f.w >= 2.0 && f.h >= 1.5 { push("wallrun", f.obj, f.part, f.c, On::Face(i), "along", 0.0); } else if f.h >= 2.0 && f.w >= 0.4 { push("wallrun", f.obj, f.part, f.c, On::Face(i), "up", 0.0); }
            if f.h >= 0.9 && f.w >= 0.4 { push("wallkick", f.obj, f.part, f.c, On::Face(i), "", 0.0); }
        }
        for (i, r) in sc.rounds.iter().enumerate() {
            if r.y1 - r.y0 >= 2.0 { push("wallrun", r.obj, r.part, [r.c[0], (r.y0 + r.y1) / 2.0, r.c[2]], On::Round(i), "up", 0.0); }
            if r.y1 >= 1.0 && r.y1 - r.y0 >= 0.9 && r.r >= 0.15 { push("wallkick", r.obj, r.part, [r.c[0], r.y1.min(1.2), r.c[2]], On::Round(i), "", 0.0); }
        }
        for (i, u) in sc.unders.iter().enumerate() {
            if u.y >= 0.5 && u.y <= 1.15 && 2.0 * len(u.u).min(len(u.v)) >= 0.6 && ground_at(&sc.tops, u.c[0], u.c[2], u.y + 0.02) < 0.15 { push("slide", u.obj, u.part, u.c, On::Under(i), "", u.y); }
        }
        for (i, b) in sc.bars.iter().enumerate() { push("swing", b.obj, b.part, b.c, On::Bar(i), "", b.y); }
        sc.affordances = aff;
        sc
    }

    fn surfaces_of(&mut self, oi: usize, rounds: &mut Vec<Round>) {
        let o = self.objects[oi].clone();
        for p in &o.parts {
            let pr = &p.prim; let ax = |v: V3| qrot(p.wq, v); let up = ax(UP);
            if pr.shape == "box" {
                let h = pr.half; let a = [ax([1.0, 0.0, 0.0]), ax(UP), ax([0.0, 0.0, 1.0])];
                let mut order = [0usize, 1, 2]; order.sort_by(|x, y| h[*y].partial_cmp(&h[*x]).unwrap());
                let l = order[0];
                if h[l] >= 0.4 && h[order[1]] <= 0.11 && a[l][1].abs() < 0.2 && p.wp[1] >= 1.9 && p.wp[1] <= 3.3 { self.bars.push(Bar { obj: oi, part: p.pi, c: p.wp, dir: a[l], half: h[l], y: p.wp[1], r: h[order[1]] }); }
                for i in 0..3 { for s in [1.0, -1.0] {
                    let n = mul(a[i], s); let c = adds(p.wp, n, h[i]); let (j, k) = ((i + 1) % 3, (i + 2) % 3); let (u, v) = (mul(a[j], h[j]), mul(a[k], h[k]));
                    if n[1] > 0.9 { self.tops.push(Top { obj: oi, part: p.pi, c, u, v, round: 0.0, y: c[1], area: 4.0 * h[j] * h[k], walkable: false }); }
                    else if n[1] < -0.9 { self.unders.push(Under { obj: oi, part: p.pi, c, u, v, y: c[1] }); }
                    else if n[1].abs() < 0.15 {
                        let (hu, vu) = if u[1].abs() < v[1].abs() { (u, v) } else { (v, u) };
                        self.faces.push(Face { obj: oi, part: p.pi, c, n: norm([n[0], 0.0, n[2]]), t: norm([hu[0], 0.0, hu[2]]), w: 2.0 * len(hu), h: 2.0 * len(vu), y0: c[1] - len(vu), y1: c[1] + len(vu) });
                    }
                } }
            } else if pr.shape == "cylinder" {
                if up[1] > 0.95 {
                    self.tops.push(Top { obj: oi, part: p.pi, c: adds(p.wp, up, pr.hh), u: Z3, v: Z3, round: pr.rad, y: p.wp[1] + pr.hh, area: PI * pr.rad * pr.rad, walkable: false });
                    rounds.push(Round { obj: oi, part: p.pi, c: p.wp, r: pr.rad, y0: p.wp[1] - pr.hh, y1: p.wp[1] + pr.hh, parts_all: vec![p.pi] });
                } else if up[1].abs() < 0.2 && pr.rad <= 0.11 && pr.hh >= 0.4 && p.wp[1] >= 1.9 && p.wp[1] <= 3.3 { self.bars.push(Bar { obj: oi, part: p.pi, c: p.wp, dir: up, half: pr.hh, y: p.wp[1], r: pr.rad }); }
            }
        }
    }
}
fn ground_at(tops: &[Top], x: f64, z: f64, max_y: f64) -> f64 { let mut y = 0.0; for t in tops { if t.y <= max_y && t.y > y && in_top(t, x, z) { y = t.y; } } y }

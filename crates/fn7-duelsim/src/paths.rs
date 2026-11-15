//! The script resolver: turns a panel's `segs[]` into, for each fighter, segments on the duel's timeline, each with
//! a body trajectory and timed hand / foot contact targets on real surfaces of the scene. Everything here is worked
//! out ahead of time from the script, the derived scene and the fighters' masses. A port of `duel_lab/lib/paths.js`.

use crate::materials::{ref_mass, FighterBody};
use crate::math::*;
use crate::props::{Ref, Scene, Top};
use serde_json::Value;

fn bump(x: f64) -> f64 { let s = sin(PI * clamp01(x)); s * s }
fn pulse(x: f64, a: f64, b: f64, f: f64) -> f64 { smooth((x - a) / f) * (1.0 - smooth((x - b) / f)) }

/// A ballistic arc under real gravity: apex h above the higher end.
#[derive(Debug, Clone, Copy)]
pub struct Arc { pub from: V3, pub t: f64, pub h: f64, pub peak: f64, pub v0: V3, pub v1: V3, pub squeezed: bool }
impl Arc {
    pub fn new(from: V3, to: V3, h: f64) -> Arc {
        let peak = from[1].max(to[1]) + h.max(0.01);
        let (tu, td) = ((2.0 * (peak - from[1]) / G).sqrt(), (2.0 * (peak - to[1]) / G).sqrt());
        let t = tu + td; let vh = mul(flat(sub(to, from)), 1.0 / t); let vy = G * tu;
        Arc { from, t, h, peak, v0: [vh[0], vy, vh[2]], v1: [vh[0], vy - G * t, vh[2]], squeezed: false }
    }
    pub fn at(&self, t: f64) -> V3 { [self.from[0] + self.v0[0] * t, self.from[1] + self.v0[1] * t - 0.5 * G * t * t, self.from[2] + self.v0[2] * t] }
    /// Lower the apex until the flight fits the time the script gives it.
    pub fn fit(from: V3, to: V3, h: f64, avail: f64) -> Arc {
        let a = Arc::new(from, to, h);
        if a.t <= avail { return a; }
        let (mut lo, mut hi) = (0.01, h);
        for _ in 0..24 { let m = (lo + hi) / 2.0; if Arc::new(from, to, m).t > avail { hi = m; } else { lo = m; } }
        let mut a = Arc::new(from, to, lo); a.squeezed = a.t > avail + 0.02; a
    }
}

#[derive(Debug, Clone)]
struct Wall { n: V3, t: V3, base: V3, y0: f64, y1: f64, d: f64, w: f64, face: Option<usize>, r: Ref }
/// The nearest wall of an object, as seen from a point.
fn wall_of(w: &Scene, obj: usize, from: V3) -> Option<Wall> {
    let mut best: Option<Wall> = None;
    for a in &w.affordances {
        if a.obj != obj || (a.kind != "wallkick" && a.kind != "wallrun") { continue; }
        let c = match a.on {
            crate::props::On::Round(i) => { let r = &w.rounds[i]; let n = norm([from[0] - r.c[0], 0.0, from[2] - r.c[2]]);
                Wall { n, t: [-n[2], 0.0, n[0]], base: [r.c[0] + n[0] * r.r, 0.0, r.c[2] + n[2] * r.r], y0: r.y0, y1: r.y1, d: hypot2(from[0] - r.c[0], from[2] - r.c[2]) - r.r, w: 2.0 * r.r, face: None, r: Ref { obj, part: r.part } } }
            crate::props::On::Face(i) => { let f = &w.faces[i]; let rel = sub(from, f.c); let d = dot(rel, f.n);
                if d <= 0.0 { continue; }
                let s = clamp(dot(rel, f.t), -f.w / 2.0 + 0.15, f.w / 2.0 - 0.15);
                Wall { n: f.n, t: f.t, base: [f.c[0] + f.t[0] * s, 0.0, f.c[2] + f.t[2] * s], y0: f.y0, y1: f.y1, d: hypot2(d, dot(rel, f.t) - s), w: f.w, face: Some(i), r: Ref { obj, part: f.part } } }
            _ => continue,
        };
        let better = match &best { None => true, Some(b) => c.d < b.d - 1e-6 || ((c.d - b.d).abs() < 1e-6 && c.y1 > b.y1) };
        if better { best = Some(c); }
    }
    best
}
fn top_of(w: &Scene, obj: usize, near: V3) -> Option<usize> {
    let (mut best, mut bd) = (None, 1e9);
    for (i, t) in w.tops.iter().enumerate() { if t.obj != obj || !t.walkable { continue; } let d = len(flat(sub(t.c, near))) - t.y * 0.01; if d < bd { bd = d; best = Some(i); } }
    best
}
fn ride_of(t: &Top) -> Ref { Ref { obj: t.obj, part: t.part } }

/// A limb target the path asks for at this instant: weight 0..1.
#[derive(Debug, Clone, Copy)]
pub struct Ik { pub limb: &'static str, pub id: u8, pub p: V3, pub n: V3, pub r: Option<Ref>, pub w: f64 }
/// Where a fighter is at one instant of a segment.
#[derive(Debug, Clone)]
pub struct St {
    pub p: V3,
    pub air: bool,
    /// How much the body turns about its centre of mass (0..1), in the air.
    pub aw: f64,
    pub hung: bool,
    pub low: bool,
    pub pitch: f64,
    pub roll: f64,
    pub face: Option<V3>,
    pub rides: [(Option<Ref>, f64); 2],
    pub ik: Vec<Ik>,
}
#[derive(Debug, Clone)]
enum F {
    Air { a: Arc, lead: f64, k: f64, flip: bool, knock: bool, back: bool, dir: V3, spin: f64, face_path: bool },
    Dash { sm: [f64; 25], dir: V3, face_foe: bool },
    Vault { uc: f64, y_peak: f64, plant: V3, r: Ref, side: f64, dir: V3 },
    Slide { dir: V3 },
    WallKick { c: V3, foot: V3, t_a: f64, a: Arc, n: V3, d_out: V3, wall: Ref },
    WallAlong { fc: V3, ft: V3, n: V3, s0: f64, s1: f64, e0: V3, e1: V3, steps: usize, tdir: V3, roll_s: f64, hi: f64, u0: f64, u1: f64, wall: Ref },
    WallUp { base: V3, n: V3, t: V3, y0: f64, rise: f64, u0: f64, steps: usize, wall: Ref },
    Mantle { hang: V3, e: V3, o: V3, tg: V3, h: f64, r: Ref },
    Stay,
}
#[derive(Debug, Clone)]
pub struct Seg {
    /// stay, dash, leap, flip, knock, drop, vault, slide, wallrun, wallkick, mantle, perch
    pub kind: String,
    /// Start on the duel's timeline (ms) and length.
    pub t0: f64,
    pub ms: f64,
    pub from: V3,
    pub to: V3,
    pub ride0: Option<Ref>,
    pub ride1: Option<Ref>,
    pub pitch0: f64,
    /// The scene object the fighter faces (`aim`).
    pub aim: Option<usize>,
    pub arc: Option<Arc>,
    pub lead: f64,
    f: F,
}
/// What the fighter's momentum does to the scenery at a moment of the path (a take-off, a landing, a wall step).
#[derive(Debug, Clone)]
pub struct Impulse { pub t: f64, pub obj: usize, pub at: V3, pub j: V3, pub by: usize, pub soft: bool, pub why: &'static str }
/// Where a fighter stands between segments.
#[derive(Debug, Clone, PartialEq)]
pub struct Cur { pub p: V3, pub ride: Option<Ref>, pub pitch: f64 }
pub struct Out { pub impulses: Vec<Impulse>, pub warnings: Vec<String> }

fn num(v: &Value) -> Option<f64> { v.as_f64() }
fn hex(v: &Value) -> Option<[f64; 2]> { if v.is_array() { Some([v[0].as_f64()?, v[1].as_f64()?]) } else { None } }
pub const AIR: [&str; 4] = ["leap", "flip", "knock", "drop"];

/// Resolve one segment. `cur` is where the fighter is; it is advanced to the segment's end.
#[allow(clippy::too_many_arguments)]
pub fn resolve_seg(s: &Value, ms: f64, t0: f64, cur: &mut Cur, fb: &FighterBody, foe: &FighterBody, w: &Scene, out: &mut Out, side: usize) -> Seg {
    let (from, sec) = (cur.p, ms / 1000.0);
    let to_hex = hex(&s["to"]);
    let kind: String = s["path"].as_str().map(|x| x.to_string()).unwrap_or_else(|| if to_hex.is_some() { "dash".into() } else { "stay".into() });
    let (ride0, pitch0) = (cur.ride, cur.pitch);
    let to_xz = to_hex.map(hex_to_world).unwrap_or([from[0], 0.0, from[2]]);
    let mut to = [to_xz[0], w.ground_at(to_xz[0], to_xz[2], from[1] + 0.45), to_xz[2]];
    let (mut ride1, mut pitch1): (Option<Ref>, f64) = (None, 0.0);
    let (mut arc, mut lead_out) = (None, 0.0);
    let name = |i: usize| w.objects[i].id.clone();
    let obj_of = |k: &str| s[k].as_str().and_then(|id| w.index(id));
    let mut ev = |dt: f64, obj: usize, at: V3, j: V3, soft: bool, why: &'static str| out.impulses.push(Impulse { t: (t0 + dt * 1000.0) / 1000.0, obj, at, j, by: side, soft, why });
    let mut warns: Vec<String> = vec![];
    let mut warn = |m: String| warns.push(format!("{}@{:.1}s {}: {}", if side == 0 { "a" } else { "b" }, t0 / 1000.0, kind, m));
    let fallback = |s2: Value, cur: &mut Cur, out: &mut Out| resolve_seg(&s2, ms, t0, cur, fb, foe, w, out, side);
    let with = |k: &str, v: Value| { let mut c = s.clone(); c[k] = v; c };
    let f: F;

    if AIR.contains(&kind.as_str()) {
        let onto = obj_of("onto");
        if s["onto"].is_string() {
            match onto.and_then(|o| top_of(w, o, if to_hex.is_some() { to_xz } else { from })) {
                Some(ti) => { let top = &w.tops[ti];
                    if to_hex.is_none() || !w.in_top(top, to_xz[0], to_xz[2]) { to = [top.c[0], top.y, top.c[2]]; } else { to[1] = top.y; }
                    ride1 = Some(ride_of(top)); }
                None => warn(format!("no top on {}", s["onto"].as_str().unwrap_or(""))),
            }
        } else {
            match w.top_at(to_xz[0], to_xz[2], from[1] + 3.0) { Some(ti) => { to[1] = w.tops[ti].y; ride1 = Some(ride_of(&w.tops[ti])); } None => to[1] = 0.0 }
        }
        let (knock, drop, flip) = (kind == "knock", kind == "drop", kind == "flip");
        let scale = if knock { fb.jump * root4(foe.mass / ref_mass()).min(1.5) } else { fb.jump };
        let h = if drop { 0.12 } else { num(&s["h"]).unwrap_or(1.2) * scale };
        let lead = if knock || drop { 0.0 } else if let Some(l) = num(&s["lead"]) { l / 1000.0 } else { clamp((sec - Arc::new(from, to, h).t) * 0.35, 0.0, 0.22) };
        let a = Arc::fit(from, to, h, sec - lead - 0.02);
        let k = if a.squeezed { a.t / (sec - lead) } else { 1.0 };
        if a.squeezed { warn(format!("flight squeezed to fit {ms} ms")); }
        let back = s["back"].as_bool().unwrap_or(pitch0 < -0.5);
        let dir = norm(flat(sub(to, from)));
        arc = Some(a); lead_out = lead;
        if let Some(r) = ride0 { ev(lead, r.obj, from, mul(a.v0, -fb.mass), true, "take-off"); }
        if let Some(r) = ride1 { ev(lead + a.t / k, r.obj, to, mul([a.v1[0] * 0.5, a.v1[1], a.v1[2] * 0.5], fb.mass), false, "landing"); }
        f = F::Air { a, lead, k, flip, knock, back, dir, spin: num(&s["spin"]).unwrap_or(1.0), face_path: s["face"] == "path" };
    } else if kind == "dash" {
        // a ground move: the feet follow the walkable tops under the line (steps, a quay), a pace at a time
        const N: usize = 24;
        let mut ys = [from[1]; 25];
        for i in 1..=N { let p = mix(from, to, i as f64 / N as f64); ys[i] = w.ground_at(p[0], p[2], ys[i - 1] + 0.45); }
        let mut sm = [0.0; 25];
        for i in 0..=N { sm[i] = (ys[i.max(1) - 1] + ys[i] + ys[(i + 1).min(N)]) / 3.0; }
        sm[0] = ys[0]; sm[N] = ys[N]; to[1] = ys[N];
        ride1 = if to[1] > 0.01 { w.top_at(to[0], to[2], to[1] + 0.01).map(|t| ride_of(&w.tops[t])) } else { None };
        f = F::Dash { sm, dir: norm(flat(sub(to, from))), face_foe: s["face"] == "foe" };
    } else if kind == "vault" {
        let o = obj_of("over").or_else(|| obj_of("on"));
        let Some(ti) = o.and_then(|o| top_of(w, o, mix(from, to, 0.5))) else {
            warn(format!("nothing to vault: {}", s["over"].as_str().or(s["on"].as_str()).unwrap_or("")));
            out.warnings.append(&mut warns);
            let mut c = with("path", "leap".into()); c["h"] = 1.0.into(); return fallback(c, cur, out);
        };
        let top = &w.tops[ti];
        let d = flat(sub(to, from)); let l = if len(d) == 0.0 { 1.0 } else { len(d) }; let dir = mul(d, 1.0 / l);
        let uc = clamp(dot(sub(top.c, from), dir) / l, 0.2, 0.8); let sd = if s["hand"] == "l" { -1.0 } else { 1.0 };
        let y_peak = (top.y - 0.5).max(0.1);
        let plant = add([from[0] + d[0] * uc, top.y, from[2] + d[2] * uc], mul([-dir[2], 0.0, dir[0]], 0.22 * sd));
        ev(sec * (uc - 0.08), top.obj, plant, add(mul(UP, -fb.mass * 2.2), mul(dir, fb.mass * 0.6)), true, "vault hand");
        f = F::Vault { uc, y_peak, plant, r: ride_of(top), side: sd, dir };
    } else if kind == "slide" {
        f = F::Slide { dir: norm(flat(sub(to, from))) };
    } else if kind == "wallrun" || kind == "wallkick" {
        let Some(wall) = obj_of("on").and_then(|o| wall_of(w, o, from)) else {
            warn(format!("no wall on {}", s["on"].as_str().unwrap_or("")));
            out.warnings.append(&mut warns);
            let c = with("path", if to_hex.is_some() { "leap".into() } else { Value::Null }); return fallback(c, cur, out);
        };
        let _ = name;
        let n = wall.n;
        let along = kind == "wallrun" && to_hex.is_some() && wall.face.is_some() && wall.w >= 2.0;
        if kind == "wallkick" {
            // up to the wall, one foot on it, off again on a ballistic arc
            let hk = clamp(wall.y0 + num(&s["kick_h"]).unwrap_or(1.0), wall.y0 + 0.3, wall.y1 - 0.12);
            let c = add(adds(wall.base, n, 0.5), [0.0, from[1].max(hk - 0.45), 0.0]); let foot = [wall.base[0], hk, wall.base[2]];
            let t_a = clamp(len(flat(sub(c, from))) / 7.0, 0.18, sec * 0.45);
            let a = Arc::fit(c, to, num(&s["h"]).unwrap_or(1.0) * fb.jump, sec - t_a - 0.02);
            let vin = mul(sub(c, from), 1.0 / t_a);
            if to[1] > 0.01 { if let Some(t) = w.top_at(to[0], to[2], c[1] + 3.0) { ride1 = Some(ride_of(&w.tops[t])); } }
            ev(t_a, wall.r.obj, foot, mul(sub(a.v0, vin), -fb.mass), true, "wall-kick");
            arc = Some(a); lead_out = t_a;
            f = F::WallKick { c, foot, t_a, a, n, d_out: norm(flat(sub(to, c))), wall: wall.r };
        } else if along {
            // along a wide wall: three or four strides on the face, the body rolled toward it
            let fc_ = &w.faces[wall.face.unwrap()]; let (fc, ft, fw) = (fc_.c, fc_.t, fc_.w);
            let s0 = clamp(dot(sub(from, fc), ft), -fw / 2.0 + 0.3, fw / 2.0 - 0.3); let s1 = clamp(dot(sub(to, fc), ft), -fw / 2.0 + 0.3, fw / 2.0 - 0.3);
            let sg = if s1 - s0 < 0.0 { -1.0 } else { 1.0 };
            let pt = |sv: f64, off: f64, y: f64| -> V3 { [fc[0] + ft[0] * sv + n[0] * off, y, fc[2] + ft[2] * sv + n[2] * off] };
            let (e0, e1) = (pt(s0, 0.8, 0.0), pt(s1, 0.8, 0.0)); let steps = s["steps"].as_u64().unwrap_or(4).max(1) as usize; let tdir = mul(ft, sg);
            let roll_s = if dot([-tdir[2], 0.0, tdir[0]], n) > 0.0 { 1.0 } else { -1.0 };
            let hi = (wall.y1 - 1.2).min(num(&s["h"]).unwrap_or(0.9) * (fb.jump * 1.2).min(1.0)); let (u0, u1) = (0.22, 0.8);
            for i in 0..steps { let k = (i as f64 + 0.5) / steps as f64;
                ev(sec * lerp(u0, u1, k), wall.r.obj, pt(lerp(s0, s1, k), 0.0, hi + 0.3), add(mul(n, -fb.mass * 1.2), mul(UP, -fb.mass * G * sec * (u1 - u0) / steps as f64)), true, "wall-run step"); }
            f = F::WallAlong { fc, ft, n, s0, s1, e0, e1, steps, tdir, roll_s, hi, u0, u1, wall: wall.r };
        } else {
            // straight up a tall face: a run-in, then strides up the wall leaning back, until the legs run out of speed
            let steps = s["steps"].as_u64().unwrap_or(3).max(1) as usize;
            let rise = (wall.y1 - wall.y0 - 1.2).min(num(&s["rise"]).unwrap_or(2.3) * fb.jump);
            let u0 = clamp(len(flat(sub(adds(wall.base, n, 0.35), from))) / 6.5 / sec, 0.1, 0.5);
            to = add(adds(wall.base, n, 0.3 + 0.62), [0.0, wall.y0 + rise, 0.0]); pitch1 = -1.05; ride1 = Some(wall.r);
            for i in 0..steps { let k = (i as f64 + 0.5) / steps as f64; let kk = 1.0 - (1.0 - k) * (1.0 - k);
                ev(sec * lerp(u0, 1.0, k), wall.r.obj, [wall.base[0], wall.y0 + rise * kk + 0.55, wall.base[2]], add(mul(n, -fb.mass * 1.0), mul(UP, -fb.mass * G * sec * (1.0 - u0) / steps as f64)), true, "wall-run step"); }
            f = F::WallUp { base: wall.base, n, t: wall.t, y0: wall.y0, rise, u0, steps, wall: wall.r };
        }
    } else if kind == "mantle" {
        // grab the ledge, hang, pull up, stand on top
        let on = obj_of("on");
        let mut best: Option<(V3, V3, V3, usize)> = None; let mut bd = 1e9;
        for a in &w.affordances {
            let crate::props::On::Edge(ei) = a.on else { continue };
            if Some(a.obj) != on { continue; }
            let e = &w.edges[ei];
            let (ee, o, tg);
            if e.round > 0.0 { o = norm([from[0] - e.c[0], 0.0, from[2] - e.c[2]]); ee = [e.c[0] + o[0] * e.round, e.y, e.c[2] + o[2] * e.round]; tg = [-o[2], 0.0, o[0]]; }
            else { let ed = sub(e.p1, e.p0); let l = len(ed); tg = mul(ed, 1.0 / l);
                let sv = clamp(dot(sub(from, e.p0), tg), (0.25f64).min(l / 2.0), (l - 0.25).max(l / 2.0));
                let mut x = adds(e.p0, tg, sv); x[1] = e.y; ee = x; o = e.out; }
            let d = len(flat(sub(ee, from))) - 2.0 * dot(norm(flat(sub(from, ee))), o);
            if d < bd { bd = d; best = Some((ee, o, tg, ei)); }
        }
        let Some((ee, o, tg, ei)) = best else {
            warn(format!("no ledge on {}", s["on"].as_str().unwrap_or("")));
            out.warnings.append(&mut warns);
            let mut c = with("path", "leap".into()); c["onto"] = s["on"].clone(); return fallback(c, cur, out);
        };
        let e = &w.edges[ei]; let h = e.y;
        let hang = add(adds(ee, o, 0.3), [0.0, from[1].max(h - 1.72) - h, 0.0]); let r = Ref { obj: e.obj, part: e.part };
        let inset = if e.round > 0.0 { (0.35f64).min(e.round * 0.9) } else { 0.38 };
        to = adds(ee, o, -inset); ride1 = Some(r);
        ev(sec * 0.26, e.obj, ee, mul(UP, -fb.mass * 2.5), true, "ledge grab");
        f = F::Mantle { hang, e: ee, o, tg, h, r };
    } else { // stay / perch: hold the spot (and ride whatever it is on)
        to = from; ride1 = ride0;
        if kind == "perch" && ride0.is_none() { warn("perch with nothing under the feet".into()); }
        f = F::Stay;
    }
    out.warnings.append(&mut warns);
    cur.p = to; cur.ride = ride1; cur.pitch = pitch1;
    Seg { kind, t0, ms, from, to, ride0, ride1, pitch0, aim: s["aim"].as_str().and_then(|id| w.index(id)), arc, lead: lead_out, f }
}

fn rd(a: Option<Ref>, w: f64) -> (Option<Ref>, f64) { (a, if a.is_some() { w } else { 0.0 }) }

impl Seg {
    /// A segment that holds a spot (the wait before the first panel, or after the last).
    pub fn hold(cur: &Cur, t0: f64) -> Seg {
        Seg { kind: "stay".into(), t0, ms: 0.0, from: cur.p, to: cur.p, ride0: cur.ride, ride1: cur.ride, pitch0: cur.pitch, aim: None, arc: None, lead: 0.0, f: F::Stay }
    }
    /// The state `tau` ms into the segment, before anything it rides has moved.
    pub fn eval(&self, tau: f64) -> St {
        let (from, to, ms, pitch0) = (self.from, self.to, self.ms, self.pitch0);
        let none = (None, 0.0);
        match &self.f {
            F::Air { a, lead, k, flip, knock, back, dir, spin, face_path } => {
                let ta = (tau / 1000.0 - lead) * k; let u = clamp01(ta / a.t); let air = ta >= 0.0 && ta < a.t;
                let pitch = if *flip { if air || ta < 0.0 { lerp(pitch0, if *back { -2.0 * PI } else { 2.0 * PI }, if ta < 0.0 { 0.0 } else { u }) } else { 0.0 } }
                    else if *knock { -1.0 * sin(PI * u) * spin } else { pitch0 * (1.0 - smooth(u * 3.0)) };
                St { p: if ta <= 0.0 { from } else if air { a.at(ta) } else { to }, air, aw: if air { pulse(ta, 0.0, a.t, 0.07) } else { 0.0 }, hung: false, low: false, pitch, roll: 0.0,
                    face: if *back && air { Some(mul(*dir, -1.0)) } else if *knock { Some(mul(*dir, -1.0)) } else if air || *face_path { Some(*dir) } else { None },
                    rides: [rd(self.ride0, 1.0 - smooth(u / 0.25)), rd(self.ride1, smooth((u - 0.6) / 0.4))], ik: vec![] }
            }
            F::Dash { sm, dir, face_foe } => {
                let u = smooth(tau / ms); let x = u * 24.0; let i = (x.floor() as usize).min(23);
                let mut p = mix(from, to, u); p[1] = lerp(sm[i], sm[i + 1], x - i as f64);
                St { p, air: false, aw: 0.0, hung: false, low: false, pitch: pitch0 * (1.0 - smooth(tau / 200.0)), roll: 0.0, face: if *face_foe { None } else { Some(*dir) },
                    rides: [rd(self.ride0, 1.0 - u), rd(self.ride1, u)], ik: vec![] }
            }
            F::Vault { uc, y_peak, plant, r, side, dir } => {
                let u = tau / ms; let b = bump((u - uc) / 0.7 + 0.5);
                let mut p = mix(from, to, u); p[1] = lerp(from[1], to[1], u) + y_peak * b;
                St { p, air: b > 0.25, aw: 0.0, hung: true, low: false, pitch: 0.25 * b, roll: 1.2 * b * side, face: Some(*dir), rides: [none, none],
                    ik: vec![Ik { limb: if *side > 0.0 { "hand_r" } else { "hand_l" }, id: 0, p: *plant, n: UP, r: Some(*r), w: pulse(u, uc - 0.24, uc + 0.14, 0.1) }] }
            }
            F::Slide { dir } => {
                let u = tau / ms; let k = pulse(u, 0.02, 0.86, 0.14);
                let mut p = mix(from, to, u); p[1] = lerp(from[1], to[1], u) - 0.74 * k;
                St { p, air: false, aw: 0.0, hung: k > 0.2, low: k > 0.5, pitch: -1.38 * k, roll: 0.0, face: Some(*dir), rides: [none, none], ik: vec![] }
            }
            F::WallKick { c, foot, t_a, a, n, d_out, wall } => {
                let t = tau / 1000.0; let k = clamp01(t / t_a); let ta = t - t_a; let air = ta < a.t;
                let p = if ta < 0.0 { let mut p = mix(from, *c, k); p[1] = lerp(from[1], c[1], 2.0 * k - k * k); p } else if air { a.at(ta) } else { to };
                St { p, air: air && (ta > 0.0 || k > 0.3), aw: pulse(t, 0.04, t_a + a.t, 0.08), hung: ta < 0.08, low: false, pitch: -0.5 * pulse(t, t_a - 0.15, t_a + 0.05, 0.12), roll: 0.0,
                    face: Some(if ta < 0.0 { mul(*n, -1.0) } else { *d_out }), rides: [rd(self.ride1, smooth((ta / a.t - 0.6) / 0.4)), none],
                    ik: vec![Ik { limb: "foot_r", id: 0, p: *foot, n: *n, r: Some(*wall), w: pulse(t, t_a - 0.13, t_a + 0.07, 0.07) }] }
            }
            F::WallAlong { fc, ft, n, s0, s1, e0, e1, steps, tdir, roll_s, hi, u0, u1, wall } => {
                let (u0, u1) = (*u0, *u1);
                let u = tau / ms; let k = clamp01((u - u0) / (u1 - u0)); let on = pulse(u, u0, u1, 0.1); let mut ik = vec![];
                let mut p = if u < u0 { mix(from, *e0, u / u0) } else if u > u1 { mix(*e1, to, (u - u1) / (1.0 - u1)) } else { mix(*e0, *e1, k) };
                p[1] = lerp(from[1], to[1], u) + hi * sin(PI * clamp01((u - u0 * 0.5) / (1.0 - u0 * 0.5 - (1.0 - u1) * 0.5)));
                let st = k * *steps as f64; let i = (st.floor() as usize).min(steps - 1);
                if on > 0.0 { let sv = lerp(*s0, *s1, (i as f64 + 0.5) / *steps as f64);
                    ik.push(Ik { limb: if i % 2 == 1 { "foot_l" } else { "foot_r" }, id: i as u8, p: [fc[0] + ft[0] * sv, p[1] + 0.32, fc[2] + ft[2] * sv], n: *n, r: Some(*wall), w: on * bump(st - i as f64) }); }
                St { p, air: on > 0.5, aw: 0.0, hung: on > 0.2, low: false, pitch: 0.0, roll: 0.62 * on * roll_s,
                    face: Some(if u < u0 { norm(flat(sub(*e0, from))) } else if u > u1 { norm(flat(sub(to, *e1))) } else { *tdir }), rides: [none, none], ik }
            }
            F::WallUp { base, n, t, y0, rise, u0, steps, wall } => {
                let u0 = *u0; let sp = |pf: f64| adds(*base, *n, 0.3 + 0.62 * pf);
                let u = tau / ms; let k = clamp01((u - u0) / (1.0 - u0)); let ke = 1.0 - (1.0 - k) * (1.0 - k); let pf = smooth((u - u0 * 0.6) / (u0 * 0.6 + 0.08)); let mut ik = vec![];
                let mut p = if u < u0 { mix(from, sp(0.0), smooth(u / u0)) } else { sp(pf) };
                if u < u0 { let q = sp(pf); let wgt = smooth(u / u0); p[0] = lerp(from[0], q[0], wgt); p[2] = lerp(from[2], q[2], wgt); }
                p[1] = lerp(from[1], *y0, clamp01(u / u0)) + rise * ke;
                let st = k * *steps as f64; let i = (st.floor() as usize).min(steps - 1); let sgn = if i % 2 == 1 { -0.09 } else { 0.09 };
                if u >= u0 { ik.push(Ik { limb: if i % 2 == 1 { "foot_l" } else { "foot_r" }, id: i as u8, p: [base[0] + t[0] * sgn, p[1] + 0.62, base[2] + t[2] * sgn], n: *n, r: Some(*wall), w: bump(st - i as f64) }); }
                St { p, air: k > 0.05, aw: 0.0, hung: pf > 0.3, low: false, pitch: -1.05 * pf, roll: 0.0, face: Some(mul(*n, -1.0)), rides: [rd(Some(*wall), pf), none], ik }
            }
            F::Mantle { hang, e, o, tg, h, r } => {
                let u = tau / ms; let pull = smooth((u - 0.3) / 0.42); let over = smooth((u - 0.5) / 0.32); let j = clamp01(u / 0.26);
                let p = if u < 0.26 { let mut p = mix(from, *hang, j); p[1] = lerp(from[1], hang[1], 2.0 * j - j * j); p } else { let mut p = mix(*hang, to, over); p[1] = lerp(hang[1], *h, pull); p };
                let wg = pulse(u, 0.16, 0.74, 0.1);
                St { p, air: u > 0.05 && u < 0.7, aw: 0.0, hung: u < 0.85, low: false, pitch: 0.45 * pulse(u, 0.34, 0.8, 0.14), roll: 0.0, face: Some(mul(*o, -1.0)), rides: [rd(Some(*r), smooth((u - 0.15) / 0.15)), none],
                    ik: vec![Ik { limb: "hand_l", id: 0, p: adds(*e, *tg, 0.2), n: UP, r: Some(*r), w: wg }, Ik { limb: "hand_r", id: 0, p: adds(*e, *tg, -0.2), n: UP, r: Some(*r), w: wg }] }
            }
            F::Stay => St { p: from, air: false, aw: 0.0, hung: pitch0.abs() > 0.3 && tau < 150.0, low: false, pitch: pitch0 * (1.0 - smooth(tau / 220.0)), roll: 0.0, face: None, rides: [rd(self.ride0, 1.0), none], ik: vec![] },
        }
    }
}

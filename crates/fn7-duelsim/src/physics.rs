//! A tiny deterministic rigid-body engine: compound bodies of convex primitives, gravity, ground / static / body
//! contacts found at vertices ("feature contacts": a box also carries its edge midpoints), Coulomb friction,
//! restitution, sequential impulses, sleeping, damage per part, and breaking: compounds come apart at their joints,
//! brittle parts shatter into pre-cut shards. Fixed 120 Hz step, no randomness except the seeded rng, no clock.
//! A port of `duel_lab/lib/physics.js`, statement for statement where it matters.

use crate::materials::{crack_at, Mat};
use crate::math::*;
use crate::props::{mass_of, Scene};
use crate::shapes::{fragments, primitive, ray, sd, Kind, Prim};
use std::sync::Arc;

pub const HZ: u64 = 120;
pub const DT: f64 = 1.0 / 120.0;
const FLOOR_FRICTION: f64 = 0.6;
const FLOOR_RESTITUTION: f64 = 0.1;
const ITER: usize = 10;
const SLOP: f64 = 0.004;
const BETA: f64 = 0.2;

fn tangents(n: V3) -> (V3, V3) { let a = if n[1].abs() < 0.9 { UP } else { [1.0, 0.0, 0.0] }; let t1 = norm(cross(n, a)); (t1, cross(n, t1)) }

#[derive(Debug, Clone)]
pub struct Part {
    pub prim: Arc<Prim>,
    pub mat: &'static Mat,
    pub fill: f64,
    pub lp: V3,
    pub lq: Q,
    /// The scene part it was (index into the object's `parts`).
    pub src: usize,
    pub frag: bool,
    pub damage: f64,
    pub cracked: bool,
    pub dead: bool,
    pub jbreak: f64,
    pub born: u64,
    pub died: u64,
    // world-space caches, refreshed by `pose`
    pub wp: V3,
    pub wq: Q,
    pub wqi: Q,
    pub wv: Vec<V3>,
}
impl Part {
    pub fn new(prim: Arc<Prim>, mat: &'static Mat, fill: f64, lp: V3, lq: Q, src: usize, frag: bool) -> Part {
        Part { prim, mat, fill, lp, lq, src, frag, damage: 0.0, cracked: false, dead: false, jbreak: 0.0, born: 0, died: u64::MAX, wp: Z3, wq: QI, wqi: QI, wv: vec![] }
    }
}
#[derive(Debug, Clone)]
pub struct Body {
    pub id: usize,
    /// The scene object it is (or came from).
    pub obj: usize,
    pub fixed: bool,
    pub hidden: bool,
    pub frag: bool,
    pub piece: bool,
    pub p: V3,
    pub q: Q,
    pub v: V3,
    pub w: V3,
    pub parts: Vec<Part>,
    pub asleep: bool,
    pub still: f64,
    pub moves: u64,
    stamp: u64,
    pub born: u64,
    pub died: u64,
    pub dead: bool,
    /// The frame the parts were authored in, seen from the centre of mass: origin = p + q * refv.
    pub refv: V3,
    pub m: f64,
    pub inv_m: f64,
    pub ib: M3,
    pub inv_ib: M3,
    pub inv_i: M3,
    pub radius: f64,
}
impl Body {
    /// Where the body's authored origin is now.
    pub fn origin(&self) -> V3 { add(self.p, qrot(self.q, self.refv)) }
    pub fn vel_at(&self, r: V3) -> V3 { add(self.v, cross(self.w, r)) }
}
/// A fighter's sphere: on a scripted path, carrying real mass into a collision, never pushed back.
#[derive(Debug, Clone, Copy)]
pub struct Kin { pub id: u8, pub who: usize, pub c: V3, pub r: f64, pub v: V3, pub m: f64 }
#[derive(Debug, Clone, Copy)]
pub struct Load { pub body: usize, pub f: V3, pub at: V3 }

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ev { Hit, Push, Touch, Crack, Break, Shatter, Whiff, Blast }
impl Ev { pub fn name(self) -> &'static str { match self { Ev::Hit => "hit", Ev::Push => "push", Ev::Touch => "touch", Ev::Crack => "crack", Ev::Break => "break", Ev::Shatter => "shatter", Ev::Whiff => "whiff", Ev::Blast => "blast" } } }
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum With { None, Floor, Body(usize), Fighter(usize) }
#[derive(Debug, Clone)]
pub struct Event {
    pub step: u64,
    pub kind: Ev,
    pub obj: usize,
    pub body: usize,
    pub part: usize,
    /// The scene part index of `part` (what a client knows it as).
    pub src: usize,
    pub at: V3,
    pub j: f64,
    pub dir: V3,
    /// The side (0 = a, 1 = b) whose act it was.
    pub by: Option<usize>,
    pub why: &'static str,
    pub with: With,
    pub speed: f64,
    /// Bodies made by a break or a shatter.
    pub made: Vec<usize>,
    pub damage: f64,
    pub fixed: bool,
    pub mass: f64,
    /// From the solver (a collision), not from the script.
    pub sim: bool,
    pub word: &'static str,
    pub sound: &'static str,
}
#[derive(Debug, Clone, Copy, Default)]
pub struct Tag { pub by: Option<usize>, pub why: &'static str }

struct Contact {
    a: usize, b: Option<usize>, kin: Option<usize>, pa: usize, pb: usize, p: V3, n: V3, depth: f64, ra: V3, rb: V3, vn0: f64, mu: f64, e: f64, max_n: Option<f64>,
    t1: V3, t2: V3, jn: f64, j1: f64, j2: f64, ia: f64, ib: f64, kn: f64, k1: f64, k2: f64, target: f64,
}

#[derive(Debug, Clone)]
pub struct World {
    pub bodies: Vec<Body>,
    pub events: Vec<Event>,
    /// Steps taken: time = step / 120 s.
    pub step: u64,
    pub kin: Vec<Kin>,
    pub loads: Vec<Load>,
    touching: Vec<(u8, usize)>,
    pub rnd: Rng,
}

impl World {
    pub fn new(scene: &Scene, seed: u32) -> World {
        let mut w = World { bodies: vec![], events: vec![], step: 0, kin: vec![], loads: vec![], touching: vec![], rnd: Rng(seed) };
        for (oi, o) in scene.objects.iter().enumerate() {
            let parts = o.parts.iter().map(|p| Part::new(p.prim.clone(), p.mat, p.fill, p.lp, p.lq, p.pi, false)).collect();
            w.spawn(oi, o.fixed, o.hidden, o.p, o.q, parts, Z3, Z3, true, false);
        }
        w
    }
    pub fn t(&self) -> f64 { self.step as f64 / HZ as f64 }
    /// Make a body from parts given in the frame (p, q); its own frame is moved to the centre of mass.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(&mut self, obj: usize, fixed: bool, hidden: bool, p: V3, q: Q, mut parts: Vec<Part>, v: V3, w: V3, asleep: bool, frag: bool) -> usize {
        for pt in &mut parts {
            pt.damage = 0.0; pt.cracked = false; pt.dead = false; pt.born = self.step; pt.died = u64::MAX;
            pt.jbreak = pt.mat.strength * if pt.mat.brittle { pt.prim.section * pt.fill.sqrt() } else { pt.prim.section.max(0.1) };
        }
        let id = self.bodies.len();
        self.bodies.push(Body { id, obj, fixed, hidden, frag, piece: false, p, q, v, w, parts, asleep, still: 0.0, moves: 0, stamp: u64::MAX, born: self.step, died: u64::MAX, dead: false,
            refv: Z3, m: 0.0, inv_m: 0.0, ib: [0.0; 9], inv_ib: [0.0; 9], inv_i: [0.0; 9], radius: 0.0 });
        self.remass(id);
        id
    }
    fn remass(&mut self, bi: usize) {
        let b = &mut self.bodies[bi];
        let live: Vec<(&Prim, &Mat, f64, V3, Q)> = b.parts.iter().filter(|p| !p.dead).map(|p| (&*p.prim, p.mat, p.fill, p.lp, p.lq)).collect();
        let (m, com, i) = mass_of(&live);
        b.p = add(b.p, qrot(b.q, com));
        for p in &mut b.parts { p.lp = sub(p.lp, com); }
        b.refv = sub(b.refv, com);
        b.m = m; b.inv_m = if b.fixed { 0.0 } else { 1.0 / m }; b.ib = i; b.inv_ib = if b.fixed { [0.0; 9] } else { m3inv(&i) };
        let mut r = 0.0f64;
        for p in b.parts.iter().filter(|p| !p.dead) { r = r.max(len(p.lp) + p.prim.r); }
        b.radius = r; b.stamp = u64::MAX;
    }
    /// World-space caches for a body's parts (centres, orientations, vertices), refreshed when it has moved.
    pub fn pose(&mut self, bi: usize) {
        let b = &mut self.bodies[bi];
        if b.stamp == b.moves { return; }
        b.stamp = b.moves;
        let r = m3q(b.q);
        b.inv_i = if b.fixed { b.inv_ib } else { m3mul(&m3mul(&r, &b.inv_ib), &m3t(&r)) };
        let (bp, bq) = (b.p, b.q);
        for p in &mut b.parts {
            if p.dead { continue; }
            p.wp = add(bp, qrot(bq, p.lp)); p.wq = qmul(bq, p.lq); p.wqi = qconj(p.wq);
            let (wp, wq) = (p.wp, p.wq);
            p.wv.clear(); p.wv.extend(p.prim.verts.iter().map(|v| add(wp, qrot(wq, *v))));
        }
    }
    fn wake(&mut self, bi: usize, deep: bool) {
        let b = &mut self.bodies[bi];
        if b.asleep { b.asleep = false; b.still = 0.0; }
        if deep { self.wake_near(bi); }
    }
    /// Whatever rests against a disturbed body has to wake too, or it would hang in the air when its support leaves.
    fn wake_near(&mut self, bi: usize) {
        let (bp, br) = (self.bodies[bi].p, self.bodies[bi].radius);
        for o in &mut self.bodies { if o.asleep && !o.dead && !o.fixed && len(sub(o.p, bp)) < o.radius + br + 0.05 { o.asleep = false; o.still = 0.0; } }
    }
    pub fn push(&mut self, bi: usize, j: V3, at: V3) {
        if self.bodies[bi].fixed { return; }
        self.wake(bi, true); self.pose(bi);
        let b = &mut self.bodies[bi];
        b.v = adds(b.v, j, b.inv_m); b.w = add(b.w, m3v(&b.inv_i, cross(sub(at, b.p), j)));
    }
    /// The live part of a body nearest a world point.
    pub fn part_at(&mut self, bi: usize, at: V3) -> usize {
        self.pose(bi);
        let (mut best, mut bd) = (0, f64::INFINITY);
        for (i, p) in self.bodies[bi].parts.iter().enumerate() { if p.dead { continue; } let d = sd(&p.prim, qrot(p.wqi, sub(at, p.wp))).0; if d < bd { bd = d; best = i; } }
        best
    }
    /// The live main body of a scene object (not a shard).
    pub fn body(&self, obj: usize) -> Option<usize> { self.bodies.iter().position(|b| b.obj == obj && !b.dead && !b.frag) }
    fn event(&mut self, kind: Ev, bi: usize, part: usize, at: V3, j: f64, dir: V3, tag: Tag) -> usize {
        let b = &self.bodies[bi]; let m = b.parts[part].mat;
        self.events.push(Event { step: self.step, kind, obj: b.obj, body: bi, part, src: b.parts[part].src, at, j, dir, by: tag.by, why: tag.why, with: With::None, speed: 0.0, made: vec![], damage: b.parts[part].damage,
            fixed: b.fixed, mass: b.m, sim: false, word: m.word.as_str(), sound: m.sound.as_str() });
        self.events.len() - 1
    }
    /// A blow from outside the simulation (a fighter's strike, a landing, a projectile): damages the part, may break
    /// it, then moves what is left.
    pub fn strike(&mut self, bi: usize, j: V3, at: V3, tag: Tag) {
        let p = self.part_at(bi, at); let jl = len(j);
        self.event(Ev::Hit, bi, p, at, jl, norm(j), tag);
        let target = self.harm(bi, p, jl, at, Some(j)).unwrap_or(bi);
        if !self.bodies[target].dead { self.push(target, j, at); }
    }
    /// A push that cannot hurt (a take-off, a hand on a top).
    pub fn shove(&mut self, bi: usize, j: V3, at: V3, tag: Tag) {
        let p = self.part_at(bi, at);
        self.event(Ev::Push, bi, p, at, len(j), norm(j), tag);
        self.push(bi, j, at);
    }
    /// A small radial blast (a rocket): every body within `radius` of `at` takes an impulse falling off as (1 - d/radius)^2.
    pub fn blast(&mut self, at: V3, j0: f64, radius: f64, tag: Tag) {
        let n = self.bodies.len();
        for bi in 0..n {
            if self.bodies[bi].dead || self.bodies[bi].hidden { continue; }
            if len(sub(self.bodies[bi].p, at)) > self.bodies[bi].radius + radius { continue; }
            let pi = self.part_at(bi, at);
            let (d, nrm, wp) = { let p = &self.bodies[bi].parts[pi]; let s = sd(&p.prim, qrot(p.wqi, sub(at, p.wp))); (s.0.max(0.0), qrot(p.wq, s.1), p.wp) };
            if d >= radius { continue; }
            let k = 1.0 - d / radius; let j = j0 * k * k;
            let hit = adds(at, nrm, -d); // the nearest point of the part
            let mut dir = norm(add(sub(wp, at), [0.0, 0.25 * len(sub(wp, at)) + 0.05, 0.0]));
            if len(dir) == 0.0 { dir = UP; }
            if self.bodies[bi].frag { self.push(bi, mul(dir, j), hit); } else { self.strike(bi, mul(dir, j), hit, tag); }
        }
    }
    /// Add damage to a part; returns the body now carrying it if the part broke off.
    fn harm(&mut self, bi: usize, pi: usize, j: f64, at: V3, jv: Option<V3>) -> Option<usize> {
        let p = &mut self.bodies[bi].parts[pi];
        if p.frag || j < 0.15 * p.jbreak { return None; }
        p.damage += j / p.jbreak;
        let (dmg, was) = (p.damage, p.cracked);
        if dmg >= crack_at() && !was { self.bodies[bi].parts[pi].cracked = true; self.event(Ev::Crack, bi, pi, at, j, Z3, Tag::default()); }
        if dmg >= 1.0 { self.break_part(bi, pi, at, jv) } else { None }
    }
    fn break_part(&mut self, bi: usize, pi: usize, at: V3, jv: Option<V3>) -> Option<usize> {
        self.pose(bi);
        let alive: Vec<usize> = (0..self.bodies[bi].parts.len()).filter(|i| !self.bodies[bi].parts[*i].dead).collect();
        let solo = alive.len() == 1;
        let (fixed, obj, bw, bm) = { let b = &self.bodies[bi]; (b.fixed, b.obj, b.w, b.m) };
        let v_at = |w: &World, c: V3| -> V3 { let b = &w.bodies[bi]; if b.fixed { Z3 } else { b.vel_at(sub(c, b.p)) } };
        let p = self.bodies[bi].parts[pi].clone();
        if !p.mat.brittle && solo { return None; } // one tough block: it cracks, it does not come apart
        if p.mat.brittle { // shatter into pre-cut shards
            if p.frag || p.mat.shards < 2 { return None; }
            let fr = fragments(&p.prim, p.mat.shards, &mut self.rnd);
            let mut made = vec![];
            for (size, offset) in &fr {
                let v = v_at(self, p.wp);
                let part = Part::new(Arc::new(primitive("box", size)), p.mat, p.fill, Z3, QI, p.src, true);
                made.push(self.spawn(obj, false, false, add(p.wp, qrot(p.wq, *offset)), p.wq, vec![part], v, bw, false, true));
            }
            // share the breaking impulse among the shards, nearest the blow most, and scatter them a little
            let mut sum = 0.0;
            let wt: Vec<f64> = made.iter().map(|f| { let f = &self.bodies[*f]; let x = len(sub(f.p, at)) / 0.3; let k = 1.0 / (1.0 + x * x); sum += k * f.m; k }).collect();
            let jl = jv.map(len).unwrap_or(0.0);
            for (i, f) in made.iter().enumerate() {
                if let (Some(j), true) = (jv, fixed) { let b = &mut self.bodies[*f]; b.v = adds(b.v, j, wt[i] / sum); }
                let s = (0.4 + jl / if bm == 0.0 { 1.0 } else { bm }).min(1.5);
                let (r0, r1, r2) = (self.rnd.next(), self.rnd.next(), self.rnd.next());
                let (r3, r4, r5) = (self.rnd.next(), self.rnd.next(), self.rnd.next());
                let b = &mut self.bodies[*f];
                b.v = add(b.v, [(r0 - 0.5) * s, r1 * s, (r2 - 0.5) * s]); b.w = [(r3 - 0.5) * 8.0, (r4 - 0.5) * 8.0, (r5 - 0.5) * 8.0];
            }
            self.wake_near(bi);
            { let pt = &mut self.bodies[bi].parts[pi]; pt.dead = true; pt.died = self.step; }
            let e = self.event(Ev::Shatter, bi, pi, at, jl, Z3, Tag::default()); self.events[e].made = made;
            if solo { let b = &mut self.bodies[bi]; b.dead = true; b.died = self.step; } else { self.remass(bi); self.wake(bi, true); }
            return None;
        }
        // a joint gives way: a fixed thing loses the struck segment and everything above it, a loose thing comes fully apart
        let off: Vec<usize> = alive.iter().copied().filter(|x| !fixed || self.bodies[bi].parts[*x].wp[1] >= p.wp[1] - 0.02).collect();
        let (mut hit, mut made) = (None, vec![]);
        for x in &off {
            let xp = self.bodies[bi].parts[*x].clone();
            let v = v_at(self, xp.wp);
            let nb = self.spawn(obj, false, false, xp.wp, xp.wq, vec![Part::new(xp.prim.clone(), xp.mat, xp.fill, Z3, QI, xp.src, false)], v, if fixed { Z3 } else { bw }, false, false);
            { let n = &mut self.bodies[nb]; n.parts[0].damage = if *x == pi { 0.5 } else { xp.damage }; n.parts[0].cracked = xp.cracked; n.piece = true; }
            { let pt = &mut self.bodies[bi].parts[*x]; pt.dead = true; pt.died = self.step; }
            if *x == pi { hit = Some(nb); }
            made.push(nb);
        }
        self.wake_near(bi);
        let e = self.event(Ev::Break, bi, pi, at, jv.map(len).unwrap_or(0.0), Z3, Tag::default()); self.events[e].made = made;
        if off.len() == alive.len() { let b = &mut self.bodies[bi]; b.dead = true; b.died = self.step; } else { self.remass(bi); }
        hit
    }

    fn contact(&mut self, c: &mut Vec<Contact>, a: usize, b: usize, pa: usize, pb: usize, p: V3, n: V3, depth: f64) { // n points from b's solid toward a's vertex
        let (ra, rb) = (sub(p, self.bodies[a].p), sub(p, self.bodies[b].p));
        let vn = dot(sub(self.bodies[a].vel_at(ra), self.bodies[b].vel_at(rb)), n);
        if self.bodies[a].asleep && !self.bodies[b].fixed && !self.bodies[b].asleep && vn.abs() > 0.3 { self.wake(a, false); }
        if self.bodies[b].asleep && !self.bodies[a].asleep && vn.abs() > 0.3 { self.wake(b, false); }
        let (ma, mb) = (self.bodies[a].parts[pa].mat, self.bodies[b].parts[pb].mat);
        c.push(Contact { a, b: Some(b), kin: None, pa, pb, p, n, depth, ra, rb, vn0: vn, mu: (ma.friction * mb.friction).sqrt(), e: ma.restitution.min(mb.restitution), max_n: None,
            t1: Z3, t2: Z3, jn: 0.0, j1: 0.0, j2: 0.0, ia: 0.0, ib: 0.0, kn: 0.0, k1: 0.0, k2: 0.0, target: 0.0 });
    }

    pub fn step(&mut self) {
        let dt = DT; let nb = self.bodies.len();
        let mut cs: Vec<Contact> = vec![];
        for i in 0..nb { let b = &mut self.bodies[i]; if b.dead { continue; } if !b.fixed && !b.asleep { b.v = [b.v[0], b.v[1] - G * dt, b.v[2]]; } self.pose(i); }
        // a body on the move wakes the sleepers it overlaps: a stack must not hang in the air when its support leaves
        // (the lab wakes only the neighbours of a struck body; this goes one step further, every step)
        for i in 0..nb {
            let a = &self.bodies[i];
            if a.dead || a.fixed || a.asleep || (dot(a.v, a.v) < 0.09 && dot(a.w, a.w) < 0.25) { continue; }
            let (ap, ar) = (a.p, a.radius);
            for o in &mut self.bodies { if o.asleep && !o.dead && !o.fixed && len(sub(o.p, ap)) < o.radius + ar + 0.05 { o.asleep = false; o.still = 0.0; } }
        }
        for l in self.loads.clone() { let b = &self.bodies[l.body]; if !b.dead && !b.fixed && !b.asleep { self.push(l.body, mul(l.f, dt), l.at); } }
        // ---- contacts ----
        let floor = |b: &Body, pa: usize, p: V3, depth: f64| -> Contact {
            let m = b.parts[pa].mat; let ra = sub(p, b.p);
            Contact { a: b.id, b: None, kin: None, pa, pb: 0, p, n: UP, depth, ra, rb: Z3, vn0: b.vel_at(ra)[1], mu: (m.friction * FLOOR_FRICTION).sqrt(), e: m.restitution.min(FLOOR_RESTITUTION), max_n: None,
                t1: Z3, t2: Z3, jn: 0.0, j1: 0.0, j2: 0.0, ia: 0.0, ib: 0.0, kn: 0.0, k1: 0.0, k2: 0.0, target: 0.0 }
        };
        let mut found: Vec<(bool, usize, usize, V3, V3, f64)> = vec![];
        for i in 0..nb {
            if self.bodies[i].dead { continue; }
            { let a = &self.bodies[i];
              if !a.fixed && !a.asleep { for (pi, pa) in a.parts.iter().enumerate() { // the floor
                if pa.dead || pa.wp[1] - pa.prim.r > 0.0 { continue; }
                if pa.prim.kind == Kind::Sphere { let y = pa.wp[1] - pa.prim.rad; if y < 0.0 { cs.push(floor(a, pi, [pa.wp[0], y, pa.wp[2]], -y)); } continue; }
                for v in &pa.wv { if v[1] < 0.0 { cs.push(floor(a, pi, *v, -v[1])); } }
              } } }
            for j in i + 1..nb {
                { let (a, b) = (&self.bodies[i], &self.bodies[j]);
                  if b.dead || ((a.fixed || a.asleep) && (b.fixed || b.asleep)) { continue; }
                  let rr = a.radius + b.radius; let d = sub(a.p, b.p);
                  if dot(d, d) > rr * rr { continue; }
                  found.clear();
                  for (ia, pa) in a.parts.iter().enumerate() { if pa.dead { continue; } for (ib, pb) in b.parts.iter().enumerate() { if pb.dead { continue; }
                    let r2 = pa.prim.r + pb.prim.r; let e = sub(pa.wp, pb.wp);
                    if dot(e, e) > r2 * r2 { continue; }
                    // a vertex outside the other part's bounding sphere cannot be inside it: skip the exact test (same result, less work)
                    let (ra2, rb2) = (pa.prim.r * pa.prim.r, pb.prim.r * pb.prim.r);
                    for v in &pa.wv { let d = sub(*v, pb.wp); if dot(d, d) > rb2 { continue; } let s = sd(&pb.prim, qrot(pb.wqi, d)); if s.0 < 0.0 { found.push((false, ia, ib, *v, qrot(pb.wq, s.1), -s.0)); } }
                    for v in &pb.wv { let d = sub(*v, pa.wp); if dot(d, d) > ra2 { continue; } let s = sd(&pa.prim, qrot(pa.wqi, d)); if s.0 < 0.0 { found.push((true, ia, ib, *v, qrot(pa.wq, s.1), -s.0)); } }
                  } } }
                for k in 0..found.len() { let (swap, ia, ib, v, n, depth) = found[k]; if swap { self.contact(&mut cs, j, i, ib, ia, v, n, depth); } else { self.contact(&mut cs, i, j, ia, ib, v, n, depth); } }
            }
        }
        // the fighters: spheres on scripted paths that carry real mass into a collision but are never pushed back
        let mut now: Vec<(u8, usize)> = vec![];
        for ki in 0..self.kin.len() { let k = self.kin[ki]; for ai in 0..nb {
            { let a = &self.bodies[ai]; if a.dead || a.fixed { continue; } let d = sub(a.p, k.c); let rr = a.radius + k.r; if dot(d, d) > rr * rr { continue; } }
            for pi in 0..self.bodies[ai].parts.len() {
                let (s, wq) = { let pa = &self.bodies[ai].parts[pi]; if pa.dead { continue; } (sd(&pa.prim, qrot(pa.wqi, sub(k.c, pa.wp))), pa.wq) };
                if s.0 >= k.r { continue; }
                let nw = qrot(wq, s.1); let p = adds(k.c, nw, -s.0); let n = mul(nw, -1.0);
                let a = &self.bodies[ai]; let ra = sub(p, a.p); let vn = dot(sub(a.vel_at(ra), k.v), n);
                let key = (k.id, ai);
                let first = !self.touching.contains(&key) && !now.contains(&key);
                if !now.contains(&key) { now.push(key); }
                if first && vn < -0.3 { let e = self.event(Ev::Touch, ai, pi, p, 0.0, n, Tag { by: Some(k.who), why: "" }); self.events[e].with = With::Fighter(k.who); self.events[e].speed = -vn; }
                if self.bodies[ai].asleep { if vn < -0.3 { self.wake(ai, false); } else { continue; } }
                cs.push(Contact { a: ai, b: None, kin: Some(ki), pa: pi, pb: 0, p, n, depth: (k.r - s.0).min(0.05), ra, rb: Z3, vn0: vn, mu: 0.4, e: 0.1,
                    max_n: Some(k.m * (G * dt * 1.5 + if first { (-vn).max(0.0) * 1.1 } else { 0.0 })), t1: Z3, t2: Z3, jn: 0.0, j1: 0.0, j2: 0.0, ia: 0.0, ib: 0.0, kn: 0.0, k1: 0.0, k2: 0.0, target: 0.0 });
            }
        } }
        self.touching = now;
        // ---- solve: sequential impulses ----
        for c in &mut cs {
            let (t1, t2) = tangents(c.n); c.t1 = t1; c.t2 = t2;
            let a = &self.bodies[c.a];
            let ia = if a.asleep { 0.0 } else { a.inv_m };
            let ib = match (c.kin, c.b) { (Some(k), _) => 1.0 / self.kin[k].m, (None, Some(b)) if !self.bodies[b].asleep => self.bodies[b].inv_m, _ => 0.0 };
            c.ia = ia; c.ib = ib;
            let (ra, rb, kin, cb) = (c.ra, c.rb, c.kin, c.b);
            let k = |d: V3| -> f64 {
                let mut s = ia + ib;
                if ia != 0.0 { s += dot(cross(m3v(&a.inv_i, cross(ra, d)), ra), d); }
                if ib != 0.0 && kin.is_none() { let b = &self.bodies[cb.unwrap()]; s += dot(cross(m3v(&b.inv_i, cross(rb, d)), rb), d); }
                if s > 1e-12 { 1.0 / s } else { 0.0 }
            };
            c.kn = k(c.n); c.k1 = k(t1); c.k2 = k(t2);
            c.target = (1.5f64).min((BETA / dt) * (c.depth - SLOP).max(0.0)).max(if c.vn0 < -1.0 { -c.e * c.vn0 } else { 0.0 });
        }
        for _ in 0..ITER { for c in &mut cs {
            let d = (c.target - dot(self.rel(c), c.n)) * c.kn; let j0 = c.jn;
            c.jn = (j0 + d).max(0.0); if let Some(mx) = c.max_n { c.jn = c.jn.min(mx); }
            self.apply(c, mul(c.n, c.jn - j0));
            let lim = c.mu * c.jn;
            let d = -dot(self.rel(c), c.t1) * c.k1; let j0 = c.j1; c.j1 = (-lim).max(lim.min(j0 + d)); self.apply(c, mul(c.t1, c.j1 - j0));
            let d = -dot(self.rel(c), c.t2) * c.k2; let j0 = c.j2; c.j2 = (-lim).max(lim.min(j0 + d)); self.apply(c, mul(c.t2, c.j2 - j0));
        } }
        // ---- impacts: damage, cracks, breaks (collected first: breaking changes the body list) ----
        let mut imp: Vec<((usize, usize, u8, usize, usize), usize, f64)> = vec![];
        for (ci, c) in cs.iter().enumerate() {
            if c.vn0 > -1.0 || c.jn <= 0.0 { continue; }
            let key = match (c.kin, c.b) { (Some(k), _) => (c.a, c.pa, 1u8, self.kin[k].id as usize, 0), (None, Some(b)) => (c.a, c.pa, 2, b, c.pb), _ => (c.a, c.pa, 0, 0, 0) };
            match imp.iter_mut().find(|e| e.0 == key) { Some(e) => e.2 += c.jn, None => imp.push((key, ci, c.jn)) }
        }
        for (_, ci, j) in imp {
            let c = &cs[ci];
            if j > 25.0 {
                let e = self.event(Ev::Hit, c.a, c.pa, c.p, j, c.n, Tag::default());
                self.events[e].sim = true;
                self.events[e].with = match (c.kin, c.b) { (Some(k), _) => With::Fighter(self.kin[k].who), (None, Some(b)) => With::Body(b), _ => With::Floor };
            }
            if !self.bodies[c.a].parts[c.pa].dead && !self.bodies[c.a].dead { self.harm(c.a, c.pa, j, c.p, Some(mul(c.n, j))); }
            if let (Some(b), None) = (c.b, c.kin) { if !self.bodies[b].parts[c.pb].dead && !self.bodies[b].dead { self.harm(b, c.pb, j, c.p, Some(mul(c.n, -j))); } }
        }
        // ---- integrate, damp, sleep ----
        let step = self.step;
        for b in &mut self.bodies {
            if b.dead || b.fixed || b.asleep { continue; }
            b.p = adds(b.p, b.v, dt); b.q = qstep(b.q, b.w, dt); b.v = mul(b.v, 0.9995); b.w = mul(b.w, 0.994); b.moves += 1;
            if dot(b.v, b.v) < 0.006 && dot(b.w, b.w) < 0.02 { b.still += dt; if b.still > 0.5 { b.asleep = true; b.v = Z3; b.w = Z3; } } else { b.still = 0.0; }
            if b.p[1] < -5.0 { b.dead = true; b.died = step; }
        }
        self.step += 1;
    }
    fn rel(&self, c: &Contact) -> V3 {
        let v = self.bodies[c.a].vel_at(c.ra);
        match (c.kin, c.b) { (Some(k), _) => sub(v, self.kin[k].v), (None, Some(b)) => sub(v, self.bodies[b].vel_at(c.rb)), _ => v }
    }
    fn apply(&mut self, c: &Contact, p: V3) {
        if c.ia != 0.0 { let a = &mut self.bodies[c.a]; a.v = adds(a.v, p, c.ia); a.w = add(a.w, m3v(&a.inv_i, cross(c.ra, p))); }
        if c.ib != 0.0 && c.kin.is_none() { let b = &mut self.bodies[c.b.unwrap()]; b.v = adds(b.v, p, -c.ib); b.w = sub(b.w, m3v(&b.inv_i, cross(c.rb, p))); }
    }
    /// True when nothing loose is awake.
    pub fn at_rest(&self) -> bool { self.bodies.iter().all(|b| b.dead || b.fixed || b.asleep) }

    /// Ray against the live bodies that pass `keep`: (t, normal, body, part) of the nearest hit.
    pub fn cast_ray(&mut self, keep: &dyn Fn(&Body) -> bool, o: V3, dir: V3, tmax: f64, pad: f64) -> Option<(f64, V3, usize, usize)> {
        let mut best: Option<(f64, V3, usize, usize)> = None;
        for bi in 0..self.bodies.len() {
            { let b = &self.bodies[bi]; if b.dead || !keep(b) { continue; }
              let oc = sub(b.p, o); let along = dot(oc, dir); let r = b.radius + pad;
              if along < -r || along > tmax + r || dot(oc, oc) - along * along > r * r { continue; } }
            self.pose(bi);
            let b = &self.bodies[bi]; let qi = qconj(b.q); let (lo, ld) = (qrot(qi, sub(o, b.p)), qrot(qi, dir));
            for (pi, p) in b.parts.iter().enumerate() {
                if p.dead { continue; }
                let pq = qconj(p.lq);
                if let Some((t, n)) = ray(&p.prim, qrot(pq, sub(lo, p.lp)), qrot(pq, ld), best.map_or(tmax, |x| x.0)) { best = Some((t, qrot(b.q, qrot(p.lq, n)), bi, pi)); }
            }
        }
        best
    }
    /// Every f64 of the simulation, bit for bit, into a 64-bit FNV-1a.
    pub fn state_hash(&self, mut h: u64) -> u64 {
        h = fnv64(h, &self.step.to_le_bytes()); h = fnv64(h, &self.rnd.0.to_le_bytes());
        for b in &self.bodies {
            h = fnv64(h, &[b.dead as u8, b.asleep as u8, b.fixed as u8, b.frag as u8, b.piece as u8]);
            for x in b.p.iter().chain(b.q.iter()).chain(b.v.iter()).chain(b.w.iter()).chain(b.refv.iter()) { h = fnv64(h, &x.to_bits().to_le_bytes()); }
            h = fnv64(h, &b.still.to_bits().to_le_bytes());
            for p in &b.parts { h = fnv64(h, &[p.dead as u8, p.cracked as u8]); h = fnv64(h, &p.damage.to_bits().to_le_bytes()); }
        }
        h
    }
}

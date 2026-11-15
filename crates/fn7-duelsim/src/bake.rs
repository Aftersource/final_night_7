//! The bake: run the engine over one panel at a fixed 120 Hz, driven by the panel's resolved script (strikes,
//! landings, wall-kicks, projectiles, the fighters' bodies), and record what a client needs to play it back: for each
//! body that moved a compact keyframe track, the break / shatter / hit events, and for each fighter its root track
//! and the timed limb targets. A port of `duel_lab/lib/bake.js`, made incremental: [`Sim`] is the state after panel
//! n - 1, [`Sim::panel`] bakes panel n from it. Nothing here reads a clock or anything but its arguments.

use crate::materials::{fighter_body, figure, limb, projectile, FighterBody};
use crate::math::*;
use crate::paths::{resolve_seg, Cur, Impulse, Out, Seg, St};
use crate::physics::{Ev, Event, Kin, Load, Tag, With, World, DT, HZ};
use crate::props::{Ref, Scene};
use crate::wire::*;
use serde_json::Value;
use std::sync::Arc;

/// Record every 2nd step: 60 samples a second.
pub const SAMPLE: u64 = 2;
/// After the outro the stage is simulated on until everything is asleep, at most this long.
pub const SETTLE_MAX_MS: f64 = 4000.0;
/// Limb-target ramps of a strike (the lab's strike IK): in from -200 ms over 140 ms, out from +30 ms over 150 ms.
const STRIKE_LEAD_MS: f64 = 200.0;
const STRIKE_IN_MS: f64 = 140.0;
const STRIKE_TAIL_MS: f64 = 180.0;
const STRIKE_OUT_MS: f64 = 150.0;
const FOOT_EVERY: u64 = 6;
/// Keyframe tolerances: (metres, half-angle radians) a dropped sample may be off the line between its keys.
pub const TOL_BODY: (f64, f64) = (0.008, 0.02);
pub const TOL_SHARD: (f64, f64) = (0.02, 0.06);
/// The fighters' roots: (metres, radians of yaw / pitch / roll, radians of lean).
pub const TOL_ROOT: (f64, f64, f64) = (0.02, 0.05, 0.08);

#[derive(Debug, Clone, PartialEq)]
pub struct FighterSpec { pub skin: String, pub figure: String, pub start: [f64; 2] }
impl FighterSpec { pub fn new(skin: &str, start: [f64; 2]) -> FighterSpec { FighterSpec { skin: skin.into(), figure: "mannequin".into(), start } } }

/// What a limb target is on.
#[derive(Debug, Clone, PartialEq)]
pub enum On { Floor, Obj(usize, usize), Fighter(usize, String) }
/// A timed target for one limb: the client's two-bone IK pulls the limb's end to `p` with a weight that eases in
/// over `in_ms` from `t0_ms` and out over `out_ms` up to `t1_ms` (panel-local).
#[derive(Debug, Clone, PartialEq)]
pub struct LimbTarget { pub t0_ms: f64, pub t1_ms: f64, pub in_ms: f64, pub out_ms: f64, pub limb: &'static str, pub p: V3, pub n: V3, pub on: On, pub kind: &'static str }
#[derive(Debug, Clone, PartialEq)]
pub struct Shot { pub t0_ms: f64, pub t1_ms: f64, pub kind: String, pub from: V3, pub to: V3, pub by: usize, pub hit: On, pub blast: f64, pub radius: f64 }
#[derive(Debug, Clone, Default)]
pub struct FighterBake {
    pub root: Vec<RootKey>,
    pub limbs: Vec<LimbTarget>,
    /// (t_ms, what the head looks at: "foe", "path" or an object id)
    pub look: Vec<(f64, String)>,
    /// Flights: (t0_ms, t1_ms, apex height above the stage, take-off impulse N s).
    pub arcs: Vec<(f64, f64, f64, f64)>,
}
#[derive(Debug, Clone)]
pub struct PanelBake {
    pub n: usize,
    /// The panel's start on the duel's timeline (ms): the sum of the earlier panels' `play_ms`.
    pub t0_ms: f64,
    pub play_ms: f64,
    /// The 120 Hz step of sample 0. Sample i is at duel time (step0 + 2 i) / 120 s.
    pub step0: u64,
    /// The last sample index (the panel's end, or the end of the settle after an outro).
    pub len: u32,
    /// (body, its object, reduced keys) for every body that moved.
    pub bodies: Vec<(usize, usize, Vec<BodyKey>)>,
    pub events: Vec<Event>,
    /// Bodies made in this panel: (body, object, scene part, is a shard, the shard's box size).
    pub pieces: Vec<(usize, usize, usize, bool, V3)>,
    pub fighters: [FighterBake; 2],
    pub shots: Vec<Shot>,
    pub warnings: Vec<String>,
    /// The chained FNV-1a 32 of the wire bytes of every panel so far.
    pub hash: u32,
    /// FNV-1a 64 of every f64 of the simulation after this panel, bit for bit.
    pub state: u64,
    /// Full-rate samples (only with `keep_raw`): for tools.
    pub raw: Vec<(usize, Vec<BodyKey>)>,
    pub raw_roots: [Vec<RootKey>; 2],
}

#[derive(Debug, Clone)]
enum Act { Path(Impulse), Shot { at: V3, j: V3, body: Option<usize>, blast: f64, radius: f64, by: usize } }
#[derive(Debug, Clone)]
struct OpenIk { side: usize, limb: &'static str, id: u8, seg: usize, samples: Vec<(u64, f64)>, wmax: f64, p: V3, n: V3, r: Option<Ref>, seen: u64, why: &'static str }
#[derive(Debug, Clone)]
struct OpenFoot { step0: u64, step1: u64, p: V3, n: V3, obj: Option<usize>, part: usize }

/// The state of a duel's simulation after some panels. Clone it to keep a checkpoint.
#[derive(Debug, Clone)]
pub struct Sim {
    pub scene: Arc<Scene>,
    pub world: World,
    pub fb: [FighterBody; 2],
    pub cur: [Cur; 2],
    prev_piv: [Option<V3>; 2],
    yaw: [f64; 2],
    raw_yaw: [f64; 2],
    yaw_from: [f64; 2],
    hist: [[V3; 2]; 2],
    hist_ok: [u8; 2],
    lean: [[f64; 2]; 2],
    pending: Vec<(f64, Act)>,
    seen: Vec<(u64, V3, Q)>,
    /// Duel time at the end of the newest panel (ms).
    pub t_ms: f64,
    /// Panels baked.
    pub n: usize,
    pub hash: u32,
    pub seed: u32,
    pub keep_raw: bool,
}

fn wrap(x: f64) -> f64 { x - 2.0 * PI * (x / (2.0 * PI) + 0.5).floor() }
/// How a scene part has moved since the start: where the start-of-duel world point `p` of that part is now.
pub fn moved(scene: &Scene, world: &World, r: Ref, p: V3) -> Option<V3> {
    let o = &scene.objects[r.obj]; let sp = o.parts.get(r.part).unwrap_or(&o.parts[0]);
    for b in &world.bodies {
        if b.dead || b.obj != r.obj || b.frag { continue; }
        if let Some(pt) = b.parts.iter().find(|x| x.src == sp.pi && !x.dead) {
            let (wq, wp) = (qmul(b.q, pt.lq), add(b.p, qrot(b.q, pt.lp)));
            return Some(add(wp, qrot(wq, qrot(qconj(sp.wq), sub(p, sp.wp)))));
        }
    }
    None
}
fn side_of(v: &Value) -> Option<usize> { match v.as_str() { Some("a") => Some(0), Some("b") => Some(1), _ => None } }
/// The strike power of a contact: the lab's `power` if the panel gives one, else from the rules' `impulse` (0..1,
/// damage / 4): 0.5 + 2.6 impulse, so a light blow is the lab's 1.0 and a full one its boosted 3.1.
pub fn contact_power(c: &Value) -> f64 { c["power"].as_f64().unwrap_or_else(|| c["impulse"].as_f64().map(|i| 0.5 + 2.6 * i).unwrap_or(1.0)) }

impl Sim {
    pub fn new(scene_spec: &Value, fighters: &[FighterSpec; 2], seed: u32) -> Sim {
        let scene = Arc::new(Scene::derive(scene_spec));
        let world = World::new(&scene, seed);
        let fb = [fighter_body(&fighters[0].skin, &fighters[0].figure), fighter_body(&fighters[1].skin, &fighters[1].figure)];
        let start = |f: &FighterSpec| { let s = hex_to_world(f.start); Cur { p: [s[0], scene.ground_at(s[0], s[2], 1e9), s[2]], ride: None, pitch: 0.0 } };
        let cur = [start(&fighters[0]), start(&fighters[1])];
        let d = sub(cur[1].p, cur[0].p);
        let y0 = atan2(d[0], d[2]); let y1 = atan2(-d[0], -d[2]);
        let seen = world.bodies.iter().map(|b| (b.moves, b.origin(), b.q)).collect();
        Sim { scene, world, fb, cur, prev_piv: [None, None], yaw: [y0, y1], raw_yaw: [y0, y1], yaw_from: [y0, y1], hist: [[Z3; 2]; 2], hist_ok: [0, 0], lean: [[0.0; 2]; 2], pending: vec![],
            seen, t_ms: 0.0, n: 0, hash: FNV0, seed, keep_raw: false }
    }

    /// A fighter's state at duel time `tms`, riding whatever has moved under it, and its facing.
    fn fighter(&self, segs: &[Vec<Seg>; 2], tms: f64) -> [(St, usize, f64, f64); 2] {
        let one = |side: usize| -> (St, usize, f64) {
            let tr = &segs[side];
            let k = tr.iter().position(|s| tms < s.t0 + s.ms).unwrap_or(tr.len() - 1);
            let tau = (tms - tr[k].t0).max(0.0);
            let mut st = tr[k].eval(tau);
            let p0 = st.p; let mut p = p0;
            for (r, w) in st.rides { if let (Some(r), true) = (r, w > 0.0) { if let Some(q) = moved(&self.scene, &self.world, r, p0) { p = adds(p, sub(q, p0), w); } } }
            st.p = p;
            for ik in &mut st.ik { if let Some(r) = ik.r { if let Some(q) = moved(&self.scene, &self.world, r, ik.p) { ik.p = q; } } }
            (st, k, tau)
        };
        let (a, b) = (one(0), one(1));
        let raw = |me: &(St, usize, f64), foe: &(St, usize, f64), side: usize| -> f64 {
            let d = me.0.face.or_else(|| segs[side][me.1].aim.map(|o| sub(self.scene.objects[o].com, me.0.p))).unwrap_or_else(|| sub(foe.0.p, me.0.p));
            if hypot2(d[0], d[2]) > 0.05 { atan2(d[0], d[2]) } else { self.raw_yaw[side] }
        };
        let (ya, yb) = (raw(&a, &b, 0), raw(&b, &a, 1));
        [(a.0, a.1, a.2, ya), (b.0, b.1, b.2, yb)]
    }

    /// Bake the next panel. `panel` is a `duel.panel` (or a lab panel): `play_ms`, `a.segs`, `b.segs`, `contacts`,
    /// optional `projectiles`. An `outro` is followed by the settle: the stage runs on until everything is asleep.
    pub fn panel(&mut self, panel: &Value) -> PanelBake {
        let play_ms = panel["play_ms"].as_f64().unwrap_or(0.0).max(0.0);
        let settle = panel["kind"] == "outro" || panel["settle"] == true;
        let (t0, t1) = (self.t_ms, self.t_ms + play_ms);
        let s_begin = self.world.step;
        let s_end = ((t1 * HZ as f64 / 1000.0) - 1e-9).ceil().max(s_begin as f64) as u64;
        let step0 = s_begin + s_begin % SAMPLE;
        let scene = self.scene.clone();
        for y in &mut self.yaw { *y = wrap(*y); }

        // ---- resolve the script against the scene and the fighters' masses ----
        let mut out = Out { impulses: vec![], warnings: vec![] };
        let mut segs: [Vec<Seg>; 2] = [vec![], vec![]];
        let mut fighters: [FighterBake; 2] = Default::default();
        for side in 0..2 {
            let mut used = 0.0;
            if let Some(arr) = panel[if side == 0 { "a" } else { "b" }]["segs"].as_array() {
                for (i, s) in arr.iter().enumerate() {
                    let ms0 = s["ms"].as_f64().unwrap_or(0.0);
                    let ms = if i + 1 == arr.len() { ms0.max(play_ms - used) } else { ms0 };
                    let seg = resolve_seg(s, ms, t0 + used, &mut self.cur[side], &self.fb[side], &self.fb[1 - side], &scene, &mut out, side);
                    if let Some(a) = seg.arc { fighters[side].arcs.push((used + seg.lead * 1000.0, used + (seg.lead + a.t) * 1000.0, a.peak, len(a.v0) * self.fb[side].mass)); }
                    let look = if let Some(o) = seg.aim { scene.objects[o].id.clone() } else if s["face"] != "foe" && ["dash", "vault", "slide", "wallrun", "mantle", "drop"].contains(&seg.kind.as_str()) { "path".to_string() } else { "foe".to_string() };
                    if fighters[side].look.last().map(|l| &l.1) != Some(&look) { fighters[side].look.push((used, look)); }
                    segs[side].push(seg); used += ms;
                }
            }
            if segs[side].is_empty() { let mut h = Seg::hold(&self.cur[side], t0); h.ms = play_ms; segs[side].push(h); fighters[side].look.push((0.0, "foe".into())); }
            let mut h = Seg::hold(&self.cur[side], t0 + used.max(play_ms)); h.ms = 1e12; segs[side].push(h); // what it does once the script is over
        }
        for i in out.impulses.drain(..) { self.pending.push((i.t, Act::Path(i))); }
        self.pending.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        // strikes and projectiles, by time
        let mut strikes: Vec<(f64, &Value)> = panel["contacts"].as_array().map(|a| a.iter().map(|c| ((t0 + c["t_ms"].as_f64().unwrap_or(0.0)) / 1000.0, c)).collect()).unwrap_or_default();
        strikes.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let mut shots_in: Vec<(f64, &Value)> = panel["projectiles"].as_array().map(|a| a.iter().map(|c| ((t0 + c["t_ms"].as_f64().unwrap_or(0.0)) / 1000.0, c)).collect()).unwrap_or_default();
        shots_in.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let (mut si, mut pi) = (0, 0);

        let ev0 = self.world.events.len();
        let mut tracks: Vec<Vec<BodyKey>> = vec![vec![]; self.world.bodies.len()];
        let mut roots: [Vec<RootKey>; 2] = [vec![], vec![]];
        let mut open: Vec<OpenIk> = vec![];
        let mut feet: [[Option<OpenFoot>; 2]; 2] = Default::default();
        let mut shots: Vec<Shot> = vec![];
        let mut last_seg = [usize::MAX; 2];
        let pivot = [figure(&self.fb[0].figure).pivot, figure(&self.fb[1].figure).pivot];
        let local_ms = |step: u64| step as f64 * 1000.0 / HZ as f64 - t0;
        let close_ik = |o: OpenIk, lim: &mut [FighterBake; 2]| {
            let (s0, s1) = (o.samples[0].0, o.samples[o.samples.len() - 1].0);
            let full0 = o.samples.iter().find(|s| s.1 >= 0.9 * o.wmax).map(|s| s.0).unwrap_or(s0);
            let full1 = o.samples.iter().rev().find(|s| s.1 >= 0.9 * o.wmax).map(|s| s.0).unwrap_or(s1);
            lim[o.side].limbs.push(LimbTarget { t0_ms: local_ms(s0), t1_ms: local_ms(s1), in_ms: local_ms(full0) - local_ms(s0), out_ms: local_ms(s1) - local_ms(full1), limb: o.limb, p: o.p, n: o.n,
                on: o.r.map(|r| On::Obj(r.obj, r.part)).unwrap_or(On::Floor), kind: o.why });
        };
        let close_foot = |f: OpenFoot, limb: &'static str, fb: &mut FighterBake| {
            fb.limbs.push(LimbTarget { t0_ms: local_ms(f.step0), t1_ms: local_ms(f.step1 + FOOT_EVERY), in_ms: 100.0, out_ms: 100.0, limb, p: f.p, n: f.n, on: f.obj.map(|o| On::Obj(o, f.part)).unwrap_or(On::Floor), kind: "plant" });
        };

        let max_end = s_end + if settle { (SETTLE_MAX_MS * HZ as f64 / 1000.0) as u64 } else { 0 };
        let mut step = s_begin;
        loop {
            // the last pass only samples: the state at the panel's end is the next panel's sample 0
            let closing = step >= s_end && (!settle || step >= max_end || (self.world.at_rest() && self.pending.is_empty()));
            let t = step as f64 / HZ as f64; let tms = t * 1000.0;
            let sts = self.fighter(&segs, tms);
            if !closing { self.world.kin.clear(); self.world.loads.clear(); }
            let mut yaws = [0.0; 2];
            for side in 0..2 {
                let (st, k, tau, raw) = (&sts[side].0, sts[side].1, sts[side].2, sts[side].3);
                // facing: eased over 220 ms from where the last segment left it
                let from = if last_seg[side] != k { self.raw_yaw[side] } else { self.yaw_from[side] };
                let y = if tau < 220.0 && (k > 0 || self.n > 0) { from + wrap(raw - from) * smooth(tau / 220.0) } else { raw };
                let yaw = self.yaw[side] + wrap(y - self.yaw[side]);
                yaws[side] = yaw;
                if closing { continue; }
                self.yaw_from[side] = from; last_seg[side] = k; self.raw_yaw[side] = raw; self.yaw[side] = yaw;
                let (f, piv) = (&self.fb[side], add(st.p, [0.0, pivot[side], 0.0]));
                let v = self.prev_piv[side].map(|p| mul(sub(piv, p), 1.0 / DT)).unwrap_or(Z3);
                self.prev_piv[side] = Some(piv);
                let up = cos(st.pitch) * cos(st.roll); let sp = figure(&f.figure).spheres;
                if !st.hung { self.world.kin.push(Kin { id: (side * 2) as u8, who: side, c: piv, r: sp[0][1], v, m: f.mass }); }
                if !st.low && !st.hung { self.world.kin.push(Kin { id: (side * 2 + 1) as u8, who: side, c: add(piv, [0.0, (sp[1][0] - sp[0][0]) * up, 0.0]), r: sp[1][1], v, m: f.mass }); }
                for (r, w) in st.rides { if let (Some(r), true) = (r, w >= 0.99 && !st.air) { // standing weight
                    if let Some(b) = self.world.body(r.obj) { if !self.world.bodies[b].fixed { self.world.loads.push(Load { body: b, f: [0.0, -f.mass * G, 0.0], at: st.p }); } } } }
            }
            // ---- the fighters' record: root samples, path targets, feet ----
            if step >= step0 && (step - step0) % SAMPLE == 0 {
                let i = ((step - step0) / SAMPLE) as u32;
                for side in 0..2 {
                    let st = &sts[side].0;
                    let flags = st.air as u8 | (st.hung as u8) << 1 | (st.low as u8) << 2;
                    // lean: the body tilts into its acceleration, so the pull of gravity and of the move pass through the feet
                    let h = self.hist[side]; let dts = SAMPLE as f64 * DT;
                    let mut target = [0.0, 0.0];
                    if flags == 0 && self.hist_ok[side] >= 2 {
                        let a = [(st.p[0] - 2.0 * h[1][0] + h[0][0]) / (dts * dts), (st.p[2] - 2.0 * h[1][2] + h[0][2]) / (dts * dts)];
                        let l = hypot2(a[0], a[1]) / G; let k = if l > 0.3 { 0.3 / l } else { 1.0 };
                        target = [a[0] / G * k, a[1] / G * k];
                    }
                    let lean = [self.lean[side][0] * 0.75 + target[0] * 0.25, self.lean[side][1] * 0.75 + target[1] * 0.25];
                    if !closing { self.lean[side] = lean; self.hist[side] = [h[1], st.p]; self.hist_ok[side] = if flags == 0 { (self.hist_ok[side] + 1).min(2) } else { 0 }; }
                    roots[side].push(RootKey { i, p: st.p, yaw: yaws[side], pitch: st.pitch, roll: st.roll, lean, flags });
                }
            }
            if closing { break; }
            for side in 0..2 {
                let (st, k) = (&sts[side].0, sts[side].1);
                for ik in &st.ik {
                    if ik.w <= 0.01 { continue; }
                    match open.iter_mut().find(|o| o.side == side && o.limb == ik.limb && o.id == ik.id && o.seg == k) {
                        Some(o) => { o.samples.push((step, ik.w)); o.seen = step; if ik.w > o.wmax { o.wmax = ik.w; o.p = ik.p; o.n = ik.n; } }
                        None => open.push(OpenIk { side, limb: ik.limb, id: ik.id, seg: k, samples: vec![(step, ik.w)], wmax: ik.w, p: ik.p, n: ik.n, r: ik.r, seen: step,
                            why: match segs[side][k].kind.as_str() { "vault" => "vault", "mantle" => "mantle", "wallkick" => "wallkick", _ => "wallrun" } }),
                    }
                }
                // feet: the real surface under each foot when it is not simply the plane the root stands on
                if (step - s_begin) % FOOT_EVERY == 0 {
                    let planted = !st.air && !st.hung && st.pitch.abs() < 0.3 && st.roll.abs() < 0.3;
                    let (sy, cy) = sincos(yaws[side]); let left = [cy, 0.0, -sy]; let hs = figure(&self.fb[side].figure).half_stance;
                    for (fi, sg) in [(0usize, 1.0), (1, -1.0)] {
                        let mut found: Option<OpenFoot> = None;
                        if planted {
                            let x = adds(st.p, left, sg * hs); let py = st.p[1];
                            let hit = self.world.cast_ray(&|b| !b.hidden, [x[0], py + 0.55, x[2]], [0.0, -1.0, 0.0], 1.5, 0.0);
                            let g = hit.map(|h| py + 0.55 - h.0).unwrap_or(0.0f64.max(py - 0.95));
                            let dy = if g - py < -0.45 { 0.0 } else { g - py };
                            let n = hit.map(|h| h.1).unwrap_or(UP);
                            if dy.abs() > 0.012 || n[1] < 0.995 { found = Some(OpenFoot { step0: step, step1: step, p: [x[0], py + dy, x[2]], n, obj: hit.map(|h| self.world.bodies[h.2].obj), part: hit.map(|h| self.world.bodies[h.2].parts[h.3].src).unwrap_or(0) }); }
                        }
                        let same = match (&feet[side][fi], &found) { (Some(o), Some(f)) => o.obj == f.obj && (o.p[1] - f.p[1]).abs() < 0.015 && dot(o.n, f.n) > 0.999 && hypot2(o.p[0] - f.p[0], o.p[2] - f.p[2]) < 0.12, _ => false };
                        if same { feet[side][fi].as_mut().unwrap().step1 = step; }
                        else { if let Some(o) = feet[side][fi].take() { close_foot(o, if fi == 0 { "foot_l" } else { "foot_r" }, &mut fighters[side]); } feet[side][fi] = found; }
                    }
                }
            }
            let mut k = 0;
            while k < open.len() { if open[k].seen != step { let o = open.remove(k); close_ik(o, &mut fighters); } else { k += 1; } }

            // ---- parkour reactions and landings resolved by the script, projectiles arriving ----
            while !self.pending.is_empty() && self.pending[0].0 <= t + 1e-9 {
                match self.pending.remove(0).1 {
                    Act::Path(e) => {
                        let mut best: Option<(usize, f64)> = None;
                        for b in &self.world.bodies { if !b.dead && !b.frag && b.obj == e.obj { let d = len(sub(b.p, e.at)); if best.map_or(true, |x| d < x.1) { best = Some((b.id, d)); } } }
                        let Some((b, _)) = best else { continue };
                        let tag = Tag { by: Some(e.by), why: e.why };
                        let pt = self.world.part_at(b, e.at);
                        if e.soft && !self.world.bodies[b].parts[pt].mat.brittle { self.world.shove(b, e.j, e.at, tag); } else { self.world.strike(b, e.j, e.at, tag); }
                    }
                    Act::Shot { at, j, body, blast, radius, by } => {
                        let tag = Tag { by: Some(by), why: "shot" };
                        if let Some(b) = body { if !self.world.bodies[b].dead { self.world.strike(b, j, at, tag); } }
                        if blast > 0.0 { self.world.blast(at, blast, radius, Tag { by: Some(by), why: "blast" }); }
                    }
                }
            }
            // ---- strikes: aim from the fighter at the thing, find the real surface point, deliver limb mass x speed ----
            while si < strikes.len() && strikes[si].0 <= t + 1e-9 {
                let c = strikes[si].1; si += 1;
                let Some(by) = side_of(&c["by"]) else { continue };
                let (st, kind) = (&sts[by].0, c["kind"].as_str().unwrap_or("hand"));
                let lm = limb(kind);
                let limb_name: Option<&'static str> = match c["limb"].as_str() { Some("hand_l") => Some("hand_l"), Some("hand_r") => Some("hand_r"), Some("foot_l") => Some("foot_l"), Some("foot_r") => Some("foot_r"), _ => None };
                let ranged = c["ranged"] == true; let miss = c["miss"] == true;
                let ct = c["t_ms"].as_f64().unwrap_or(0.0);
                let target = |p: V3, n: V3, on: On| LimbTarget { t0_ms: ct - STRIKE_LEAD_MS, t1_ms: ct + STRIKE_TAIL_MS, in_ms: STRIKE_IN_MS, out_ms: STRIKE_OUT_MS, limb: limb_name.unwrap_or("hand_r"), p, n, on, kind: "strike" };
                let obj = c["on"].as_str().filter(|s| *s != "a" && *s != "b" && *s != "foe").and_then(|id| scene.index(id));
                if let Some(oi) = obj {
                    let o = add(st.p, [0.0, c["y"].as_f64().unwrap_or(lm.y), 0.0]);
                    let mut near: Option<(usize, f64)> = None;
                    for b in &self.world.bodies { if !b.dead && !b.frag && b.obj == oi { let d = len(flat(sub(b.p, o))); if near.map_or(true, |x| d < x.1) { near = Some((b.id, d)); } } }
                    let Some((nb, _)) = near else { continue };
                    let bp = self.world.bodies[nb].p;
                    let keep = |b: &crate::physics::Body| !b.frag && b.obj == oi;
                    let mut aim = norm(flat(sub(bp, o)));
                    let mut h = self.world.cast_ray(&keep, o, aim, 2.4, 0.0);
                    if h.is_none() { aim = norm(sub(bp, o)); h = self.world.cast_ray(&keep, o, aim, 2.4, 0.0); }
                    let Some((ht, hn, hb, hp)) = h else {
                        let e = Event { step, kind: Ev::Whiff, obj: oi, body: nb, part: 0, src: 0, at: adds(o, aim, 1.0), j: 0.0, dir: aim, by: Some(by), why: "", with: With::None, speed: 0.0, made: vec![], damage: 0.0, fixed: false, mass: 0.0, sim: false, word: "", sound: "" };
                        self.world.events.push(e); continue;
                    };
                    let at = adds(o, aim, ht);
                    let j = mul(norm(flat(aim)), lm.m * self.fb[by].mass * lm.v * self.fb[by].speed * contact_power(c));
                    if limb_name.is_some() && !ranged { let src = self.world.bodies[hb].parts[hp].src; fighters[by].limbs.push(target(at, hn, On::Obj(oi, src))); }
                    self.world.strike(hb, j, at, Tag { by: Some(by), why: "strike" });
                } else if limb_name.is_some() && !ranged && !miss {
                    // a blow on the foe: the target is the body part's anchor on the foe as it stands at that moment
                    let foe = side_of(&c["on"]).filter(|s| *s != by).unwrap_or(1 - by);
                    let part = c["at"].as_str().filter(|a| ["head", "chest", "belly", "legs"].contains(a)).unwrap_or("chest");
                    let anchor = figure(&self.fb[foe].figure).anchors.iter().find(|a| a.0 == part).map(|a| a.1).unwrap_or([0.0, 1.3, 0.1]);
                    let (sy, cy) = sincos(yaws[foe]); let fp = sts[foe].0.p;
                    let p = [fp[0] + cy * anchor[0] + sy * anchor[2], fp[1] + anchor[1], fp[2] - sy * anchor[0] + cy * anchor[2]];
                    let n = norm(flat(sub(st.p, fp)));
                    fighters[by].limbs.push(target(p, if len(n) == 0.0 { UP } else { n }, On::Fighter(foe, part.to_string())));
                }
            }
            while pi < shots_in.len() && shots_in[pi].0 <= t + 1e-9 {
                let c = shots_in[pi].1; pi += 1;
                let Some(by) = side_of(&c["by"]) else { continue };
                let pk = projectile(c["kind"].as_str().unwrap_or("stone"));
                let (mass, speed) = (c["mass"].as_f64().unwrap_or(pk.mass), c["speed"].as_f64().unwrap_or(pk.speed).max(1.0));
                let (blast, radius) = (c["blast"].as_f64().unwrap_or(pk.blast), c["radius"].as_f64().unwrap_or(pk.radius).max(0.1));
                let (sy, cy) = sincos(yaws[by]);
                let from = add(sts[by].0.p, [sy * 0.4, c["y"].as_f64().unwrap_or(1.3), cy * 0.4]);
                let (goal, mut hit) = if let Some(oi) = c["on"].as_str().and_then(|id| scene.index(id)) {
                    (self.world.body(oi).map(|b| self.world.bodies[b].p).unwrap_or(scene.objects[oi].com), On::Floor)
                } else if c["to"].is_array() { let w = hex_to_world([c["to"][0].as_f64().unwrap_or(0.0), c["to"][1].as_f64().unwrap_or(0.0)]); ([w[0], 0.0, w[2]], On::Floor) }
                else { let foe = side_of(&c["on"]).filter(|s| *s != by).unwrap_or(1 - by); (add(sts[foe].0.p, [0.0, 1.3, 0.0]), On::Fighter(foe, "chest".into())) };
                let d = sub(goal, from); let (dl, dir) = (len(d), norm(d));
                let h = self.world.cast_ray(&|b| !b.hidden, from, dir, dl + 0.3, 0.0);
                let (to, body) = match h { Some((ht, _, hb, hp)) => { hit = On::Obj(self.world.bodies[hb].obj, self.world.bodies[hb].parts[hp].src); (adds(from, dir, ht), Some(hb)) } None => (goal, None) };
                let tf = len(sub(to, from)) / speed;
                let ct = c["t_ms"].as_f64().unwrap_or(0.0);
                shots.push(Shot { t0_ms: ct, t1_ms: ct + tf * 1000.0, kind: pk.kind.clone(), from, to, by, hit, blast, radius });
                if body.is_some() || blast > 0.0 {
                    self.pending.push((t + tf, Act::Shot { at: to, j: mul(dir, mass * speed), body, blast, radius, by }));
                    self.pending.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                }
            }
            // ---- the bodies' record ----
            if step >= step0 && (step - step0) % SAMPLE == 0 { self.sample(((step - step0) / SAMPLE) as u32, &mut tracks); }
            self.world.step();
            step += 1;
        }
        // the closing sample: the state at the panel's end is the next panel's sample 0
        if step >= step0 && (step - step0) % SAMPLE == 0 { self.sample(((step - step0) / SAMPLE) as u32, &mut tracks); }
        for o in open.drain(..) { close_ik(o, &mut fighters); }
        for side in 0..2 { for fi in 0..2 { if let Some(o) = feet[side][fi].take() { close_foot(o, if fi == 0 { "foot_l" } else { "foot_r" }, &mut fighters[side]); } } }
        for f in &mut fighters { f.limbs.sort_by(|a, b| a.t0_ms.partial_cmp(&b.t0_ms).unwrap()); }

        // ---- pack ----
        let len_samples = if step >= step0 { ((step - step0) / SAMPLE) as u32 } else { 0 };
        let mut bodies = vec![]; let mut raw = vec![];
        for (bi, tr) in tracks.into_iter().enumerate() {
            if tr.is_empty() { continue; }
            let b = &self.world.bodies[bi];
            let (tp, tq) = if b.frag { TOL_SHARD } else { TOL_BODY };
            bodies.push((bi, b.obj, reduce_body(&tr, tp, tq)));
            if self.keep_raw { raw.push((bi, tr)); }
        }
        for side in 0..2 { fighters[side].root = reduce_root(&roots[side], TOL_ROOT.0, TOL_ROOT.1, TOL_ROOT.2); }
        let events: Vec<Event> = self.world.events.drain(ev0..).collect();
        self.world.events.clear();
        let mut pieces = vec![];
        for e in &events { for m in &e.made { let b = &self.world.bodies[*m]; pieces.push((*m, b.obj, b.parts[0].src, b.frag, mul(b.parts[0].prim.half, 2.0))); } }
        let mut h = self.hash;
        for (bi, _, k) in &bodies { h = fnv(h, &(*bi as u32).to_le_bytes()); h = fnv(h, &pack_body(k)); }
        for f in &fighters { h = fnv(h, &pack_root(&f.root)); for l in &f.limbs { h = fnv(h, l.limb.as_bytes()); for c in [l.t0_ms / 1000.0, l.t1_ms / 1000.0, l.p[0], l.p[1], l.p[2]] { h = fnv(h, &((c * 1000.0 + 0.5).floor() as i64).to_le_bytes()); } } }
        for e in &events { h = fnv(h, e.kind.name().as_bytes()); h = fnv(h, &e.step.to_le_bytes()); h = fnv(h, &(e.body as u32).to_le_bytes()); h = fnv(h, &((e.j * 1000.0 + 0.5).floor() as i64).to_le_bytes()); }
        self.hash = h;
        self.t_ms = if settle { t1.max(step as f64 * 1000.0 / HZ as f64) } else { t1 };
        self.n += 1;
        PanelBake { n: self.n - 1, t0_ms: t0, play_ms, step0, len: len_samples, bodies, events, pieces, fighters, shots, warnings: out.warnings, hash: h, state: self.world.state_hash(FNV64_0),
            raw, raw_roots: if self.keep_raw { roots } else { [vec![], vec![]] } }
    }

    fn sample(&mut self, i: u32, tracks: &mut Vec<Vec<BodyKey>>) {
        for bi in 0..self.world.bodies.len() {
            let b = &self.world.bodies[bi];
            if b.hidden { if bi >= self.seen.len() { self.seen.push((b.moves, b.origin(), b.q)); } continue; }
            if bi >= tracks.len() { tracks.resize(bi + 1, vec![]); }
            if bi >= self.seen.len() { // just made by a break: its first key is where it appears
                self.seen.push((b.moves, b.origin(), b.q)); tracks[bi].push(BodyKey { i, p: b.origin(), q: b.q }); continue;
            }
            if b.dead || b.moves == self.seen[bi].0 { continue; }
            let tr = &mut tracks[bi];
            if i > 0 && tr.last().map(|k| k.i) != Some(i - 1) { tr.push(BodyKey { i: i - 1, p: self.seen[bi].1, q: self.seen[bi].2 }); } // where it rested until now
            tr.push(BodyKey { i, p: b.origin(), q: b.q });
            self.seen[bi] = (b.moves, b.origin(), b.q);
        }
    }
}

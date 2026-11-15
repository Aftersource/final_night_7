//! Vectors, quaternions, 3x3 matrices, the hex grid, the seeded RNG and the hash: a port of `duel_lab/lib/m.js`.
//!
//! **Determinism.** Everything is `f64` and uses only `+ - * /` and `sqrt`, which IEEE 754 defines to the last bit
//! and which rustc compiles as written (no fast-math, no fused multiply-add unless `mul_add` is called: it never is
//! here). The platform's libm is never called: `sin`, `cos`, `atan2` and `cbrt` below are our own, built from the
//! same exact operations, so the same inputs give the same bits on every 64-bit target.

pub type V3 = [f64; 3];
pub type Q = [f64; 4];
pub type M3 = [f64; 9];

pub const G: f64 = 9.81;
pub const PI: f64 = 3.141592653589793;
pub const DEG: f64 = PI / 180.0;
pub const S3: f64 = 1.7320508075688772;
pub const QI: Q = [0.0, 0.0, 0.0, 1.0];
pub const UP: V3 = [0.0, 1.0, 0.0];
pub const Z3: V3 = [0.0, 0.0, 0.0];

pub fn clamp(x: f64, a: f64, b: f64) -> f64 { if x < a { a } else if x > b { b } else { x } }
pub fn clamp01(x: f64) -> f64 { clamp(x, 0.0, 1.0) }
pub fn smooth(x: f64) -> f64 { let x = clamp01(x); x * x * (3.0 - 2.0 * x) }
pub fn lerp(a: f64, b: f64, t: f64) -> f64 { a + (b - a) * t }
pub fn add(a: V3, b: V3) -> V3 { [a[0] + b[0], a[1] + b[1], a[2] + b[2]] }
pub fn sub(a: V3, b: V3) -> V3 { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
pub fn mul(a: V3, s: f64) -> V3 { [a[0] * s, a[1] * s, a[2] * s] }
pub fn adds(a: V3, b: V3, s: f64) -> V3 { [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s] }
pub fn dot(a: V3, b: V3) -> f64 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
pub fn cross(a: V3, b: V3) -> V3 { [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]] }
pub fn len(a: V3) -> f64 { dot(a, a).sqrt() }
pub fn dist(a: V3, b: V3) -> f64 { len(sub(a, b)) }
pub fn norm(a: V3) -> V3 { let l = len(a); if l > 1e-12 { mul(a, 1.0 / l) } else { Z3 } }
pub fn mix(a: V3, b: V3, t: f64) -> V3 { [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)] }
pub fn flat(a: V3) -> V3 { [a[0], 0.0, a[2]] }
pub fn hypot2(a: f64, b: f64) -> f64 { (a * a + b * b).sqrt() }

// ---- our own elementary functions (never the platform's libm) ----

/// sin and cos together. Reduction to |r| <= pi/4 by quadrant (Cody-Waite, two-part pi/2), then fixed Taylor
/// polynomials in Horner form. Good to ~1e-16 for the |x| < 1e4 this crate ever passes.
pub fn sincos(x: f64) -> (f64, f64) {
    const P1: f64 = 1.5707963267341256; // pi/2, the top 33 bits
    const P2: f64 = 6.077100506506192e-11; // the rest
    let k = (x * (2.0 / PI) + if x >= 0.0 { 0.5 } else { -0.5 }) as i64;
    let kf = k as f64;
    let r = (x - kf * P1) - kf * P2;
    let z = r * r;
    let s = r * (1.0 + z * (-1.0 / 6.0 + z * (1.0 / 120.0 + z * (-1.0 / 5040.0 + z * (1.0 / 362880.0 + z * (-1.0 / 39916800.0
        + z * (1.0 / 6227020800.0 + z * (-1.0 / 1307674368000.0 + z * (1.0 / 355687428096000.0)))))))));
    let c = 1.0 + z * (-0.5 + z * (1.0 / 24.0 + z * (-1.0 / 720.0 + z * (1.0 / 40320.0 + z * (-1.0 / 3628800.0
        + z * (1.0 / 479001600.0 + z * (-1.0 / 87178291200.0 + z * (1.0 / 20922789888000.0))))))));
    match k.rem_euclid(4) { 0 => (s, c), 1 => (c, -s), 2 => (-s, -c), _ => (-c, s) }
}
pub fn sin(x: f64) -> f64 { sincos(x).0 }
pub fn cos(x: f64) -> f64 { sincos(x).1 }

/// atan for any x: fold to [0, 1], halve the angle twice, then the alternating series.
fn atan(x: f64) -> f64 {
    let neg = x < 0.0;
    let mut a = if neg { -x } else { x };
    let inv = a > 1.0;
    if inv { a = 1.0 / a; }
    // atan(a) = 2 atan(a / (1 + sqrt(1 + a^2))), twice: a <= 1 -> <= 0.4143 -> <= 0.1990
    a = a / (1.0 + (1.0 + a * a).sqrt());
    a = a / (1.0 + (1.0 + a * a).sqrt());
    let z = a * a;
    let mut s = 0.0;
    let mut n = 27i32;
    while n >= 1 { s = 1.0 / (n as f64) - z * s; n -= 2; }
    let mut r = 4.0 * a * s;
    if inv { r = PI / 2.0 - r; }
    if neg { -r } else { r }
}
pub fn atan2(y: f64, x: f64) -> f64 {
    if x > 0.0 { atan(y / x) }
    else if x < 0.0 { if y >= 0.0 { atan(y / x) + PI } else { atan(y / x) - PI } }
    else if y > 0.0 { PI / 2.0 } else if y < 0.0 { -PI / 2.0 } else { 0.0 }
}
/// Cube root of a positive number: 80 Newton steps from max(x, 1) (fixed count, so fixed bits).
pub fn cbrt(x: f64) -> f64 {
    if x <= 0.0 { return 0.0; }
    let mut y = if x > 1.0 { x } else { 1.0 };
    for _ in 0..80 { y = y - (y * y * y - x) / (3.0 * y * y); }
    y
}
/// x^(1/4), as the lab's `Math.pow(k, 0.25)`.
pub fn root4(x: f64) -> f64 { x.sqrt().sqrt() }

// ---- the hex grid: pointy-top axial [q, r] (fractions allowed) <-> metres on the stage floor (y = 0) ----
pub fn hex_to_world(h: [f64; 2]) -> V3 { [S3 * (h[0] + h[1] / 2.0), 0.0, 1.5 * h[1]] }
pub fn world_to_hex(p: V3) -> [f64; 2] { let r = p[2] / 1.5; [p[0] / S3 - r / 2.0, r] }

// ---- quaternions [x, y, z, w] ----
pub fn qmul(a: Q, b: Q) -> Q {
    [a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
     a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
     a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
     a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2]]
}
pub fn qconj(q: Q) -> Q { [-q[0], -q[1], -q[2], q[3]] }
pub fn qnorm(q: Q) -> Q {
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    let l = if l == 0.0 { 1.0 } else { l };
    [q[0] / l, q[1] / l, q[2] / l, q[3] / l]
}
pub fn qaxis(ax: V3, ang: f64) -> Q { let (s, c) = sincos(ang / 2.0); [ax[0] * s, ax[1] * s, ax[2] * s, c] }
pub fn qrot(q: Q, v: V3) -> V3 { let u = [q[0], q[1], q[2]]; let t = cross(u, add(cross(u, v), mul(v, q[3]))); adds(v, t, 2.0) }
/// Euler degrees, three.js 'XYZ' order.
pub fn qeuler(e: V3) -> Q { qmul(qmul(qaxis([1.0, 0.0, 0.0], e[0] * DEG), qaxis([0.0, 1.0, 0.0], e[1] * DEG)), qaxis([0.0, 0.0, 1.0], e[2] * DEG)) }
pub fn qstep(q: Q, w: V3, dt: f64) -> Q {
    let d = qmul([w[0], w[1], w[2], 0.0], q); let h = 0.5 * dt;
    qnorm([q[0] + d[0] * h, q[1] + d[1] * h, q[2] + d[2] * h, q[3] + d[3] * h])
}
pub fn qdot(a: Q, b: Q) -> f64 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3] }
pub fn qnlerp(a: Q, b: Q, t: f64) -> Q {
    let s = if qdot(a, b) < 0.0 { -1.0 } else { 1.0 };
    qnorm([lerp(a[0], s * b[0], t), lerp(a[1], s * b[1], t), lerp(a[2], s * b[2], t), lerp(a[3], s * b[3], t)])
}

// ---- 3x3 matrices, row-major ----
pub fn m3q(q: Q) -> M3 {
    let [x, y, z, w] = q;
    [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w),
     2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w),
     2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)]
}
pub fn m3v(m: &M3, v: V3) -> V3 { [m[0] * v[0] + m[1] * v[1] + m[2] * v[2], m[3] * v[0] + m[4] * v[1] + m[5] * v[2], m[6] * v[0] + m[7] * v[1] + m[8] * v[2]] }
pub fn m3mul(a: &M3, b: &M3) -> M3 {
    let mut o = [0.0; 9];
    for i in 0..3 { for j in 0..3 { o[i * 3 + j] = a[i * 3] * b[j] + a[i * 3 + 1] * b[3 + j] + a[i * 3 + 2] * b[6 + j]; } }
    o
}
pub fn m3t(m: &M3) -> M3 { [m[0], m[3], m[6], m[1], m[4], m[7], m[2], m[5], m[8]] }
pub fn m3add(a: &M3, b: &M3) -> M3 { let mut o = [0.0; 9]; for i in 0..9 { o[i] = a[i] + b[i]; } o }
pub fn m3scale(a: &M3, s: f64) -> M3 { let mut o = [0.0; 9]; for i in 0..9 { o[i] = a[i] * s; } o }
pub fn m3inv(m: &M3) -> M3 {
    let [a, b, c, d, e, f, g, h, i] = *m;
    let (aa, bb, cc) = (e * i - f * h, -(d * i - f * g), d * h - e * g);
    let mut det = a * aa + b * bb + c * cc;
    if det == 0.0 { det = 1e-12; }
    [aa / det, -(b * i - c * h) / det, (b * f - c * e) / det, bb / det, (a * i - c * g) / det, -(a * f - c * d) / det, cc / det, -(a * h - b * g) / det, (a * e - b * d) / det]
}
/// Inertia of a point mass m at offset d (the parallel-axis term).
pub fn m3point(m: f64, d: V3) -> M3 {
    let dd = dot(d, d);
    [m * (dd - d[0] * d[0]), -m * d[0] * d[1], -m * d[0] * d[2], -m * d[1] * d[0], m * (dd - d[1] * d[1]), -m * d[1] * d[2], -m * d[2] * d[0], -m * d[2] * d[1], m * (dd - d[2] * d[2])]
}

/// mulberry32: the only source of "randomness" in the simulation. Its whole state is one u32.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rng(pub u32);
impl Rng {
    pub fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6D2B79F5);
        let s = self.0;
        let mut t = (s ^ (s >> 15)).wrapping_mul(1 | s);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        ((t ^ (t >> 14)) as f64) / 4294967296.0
    }
}
/// FNV-1a, 32 bits.
pub fn fnv(mut h: u32, bytes: &[u8]) -> u32 { for b in bytes { h ^= *b as u32; h = h.wrapping_mul(16777619); } h }
pub const FNV0: u32 = 2166136261;
/// FNV-1a, 64 bits (the state hash: every f64 of the simulation, bit for bit).
pub fn fnv64(mut h: u64, bytes: &[u8]) -> u64 { for b in bytes { h ^= *b as u64; h = h.wrapping_mul(0x100000001b3); } h }
pub const FNV64_0: u64 = 0xcbf29ce484222325;

/// 2D convex hull (monotone chain) of [x, z] points, counter-clockwise.
pub fn hull2(pts: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut p = pts.to_vec();
    p.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap().then(a[1].partial_cmp(&b[1]).unwrap()));
    let cr = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut h: Vec<[f64; 2]> = Vec::new();
    for pass in 0..2 {
        let s = h.len();
        let it: Box<dyn Iterator<Item = &[f64; 2]>> = if pass == 0 { Box::new(p.iter()) } else { Box::new(p.iter().rev()) };
        for q in it {
            while h.len() - s >= 2 && cr(h[h.len() - 2], h[h.len() - 1], *q) <= 1e-9 { h.pop(); }
            h.push(*q);
        }
        h.pop();
    }
    h
}

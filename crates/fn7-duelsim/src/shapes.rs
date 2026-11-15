//! Primitive solids: volume, centre of mass, inertia, collision vertices, signed distance, ray test, fragments.
//! Everything is in the part's own frame (origin at the part's centre, Y up). A port of `duel_lab/lib/shapes.js`.

use crate::math::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind { Planes, Cyl, Sphere }

#[derive(Debug, Clone)]
pub struct Prim {
    pub shape: String,
    pub size: Vec<f64>,
    pub kind: Kind,
    pub vol: f64,
    pub com: V3,
    /// Inertia tensor about the centre of mass, per unit mass.
    pub i: M3,
    pub verts: Vec<V3>,
    pub planes: Vec<(V3, f64)>,
    pub half: V3,
    /// Bounding radius.
    pub r: f64,
    pub rad: f64,
    pub hh: f64,
    /// Smallest cross-section (m^2): what a blow has to break.
    pub section: f64,
}

const T: f64 = 1.618033988749895;
fn platonic(shape: &str) -> Vec<V3> {
    match shape {
        "tetra" => vec![[1.0, 1.0, 1.0], [-1.0, -1.0, 1.0], [-1.0, 1.0, -1.0], [1.0, -1.0, -1.0]],
        "octa" => vec![[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]],
        "ico" => vec![[-1.0, T, 0.0], [1.0, T, 0.0], [-1.0, -T, 0.0], [1.0, -T, 0.0], [0.0, -1.0, T], [0.0, 1.0, T], [0.0, -1.0, -T], [0.0, 1.0, -T], [T, 0.0, -1.0], [T, 0.0, 1.0], [-T, 0.0, -1.0], [-T, 0.0, 1.0]],
        _ => { let i = 1.0 / T; vec![[-1.0, -1.0, -1.0], [-1.0, -1.0, 1.0], [-1.0, 1.0, -1.0], [-1.0, 1.0, 1.0], [1.0, -1.0, -1.0], [1.0, -1.0, 1.0], [1.0, 1.0, -1.0], [1.0, 1.0, 1.0],
            [0.0, -i, -T], [0.0, -i, T], [0.0, i, -T], [0.0, i, T], [-i, -T, 0.0], [-i, T, 0.0], [i, -T, 0.0], [i, T, 0.0], [-T, 0.0, -i], [T, 0.0, -i], [-T, 0.0, i], [T, 0.0, i]] }
    }
}
fn ring(r: f64, y: f64, n: usize) -> Vec<V3> {
    (0..n).map(|i| { let (s, c) = sincos((i as f64 / n as f64) * 2.0 * PI); [r * c, y, r * s] }).collect()
}
/// Brute-force convex hull of a small vertex set: outward face planes and triangles.
fn hull(verts: &[V3]) -> (Vec<(V3, f64)>, Vec<[V3; 3]>) {
    let (mut planes, mut tris, mut seen): (Vec<(V3, f64)>, Vec<[V3; 3]>, Vec<[i64; 4]>) = (vec![], vec![], vec![]);
    let n = verts.len();
    let rnd = |x: f64| (x * 1e4 + 0.5).floor() as i64;
    for i in 0..n { for j in i + 1..n { for k in j + 1..n {
        let mut nn = cross(sub(verts[j], verts[i]), sub(verts[k], verts[i]));
        if len(nn) < 1e-9 { continue; }
        nn = norm(nn);
        let mut d = dot(nn, verts[i]);
        let (mut pos, mut neg) = (0, 0);
        for v in verts { let s = dot(nn, *v) - d; if s > 1e-7 { pos += 1; } else if s < -1e-7 { neg += 1; } }
        if pos > 0 && neg > 0 { continue; }
        if pos > 0 { nn = mul(nn, -1.0); d = -d; }
        let key = [rnd(nn[0]), rnd(nn[1]), rnd(nn[2]), rnd(d)];
        if seen.contains(&key) { continue; }
        seen.push(key);
        planes.push((nn, d));
        let mut on: Vec<V3> = verts.iter().copied().filter(|v| (dot(nn, *v) - d).abs() < 1e-7).collect();
        let mut c = Z3;
        for v in &on { c = add(c, *v); }
        c = mul(c, 1.0 / on.len() as f64);
        let e0 = norm(sub(on[0], c)); let e1 = cross(nn, e0);
        let ang = |a: V3| atan2(dot(sub(a, c), e1), dot(sub(a, c), e0));
        on.sort_by(|a, b| ang(*a).partial_cmp(&ang(*b)).unwrap());
        for t in 1..on.len() - 1 { tris.push([on[0], on[t], on[t + 1]]); }
    } } }
    (planes, tris)
}
/// Mass properties of a closed triangle mesh of unit density: volume, centre of mass, inertia per unit mass about it.
fn poly_mass(tris: &[[V3; 3]]) -> (f64, V3, M3) {
    let (mut vol, mut c, mut cc) = (0.0, Z3, [0.0; 9]);
    for [a, b, d] in tris {
        let v = dot(*a, cross(*b, *d)) / 6.0; let s = add(add(*a, *b), *d);
        vol += v; c = add(c, mul(s, v / 4.0));
        for i in 0..3 { for j in 0..3 { cc[i * 3 + j] += (v / 20.0) * (s[i] * s[j] + a[i] * a[j] + b[i] * b[j] + d[i] * d[j]); } }
    }
    c = mul(c, 1.0 / vol);
    let tr = cc[0] + cc[4] + cc[8];
    let i = [tr - cc[0], -cc[1], -cc[2], -cc[3], tr - cc[4], -cc[5], -cc[6], -cc[7], tr - cc[8]];
    let sh = m3point(vol, c);
    let mut o = [0.0; 9];
    for k in 0..9 { o[k] = (i[k] - sh[k]) / vol; }
    (vol, c, o)
}
fn diag(x: f64, y: f64, z: f64) -> M3 { [x, 0.0, 0.0, 0.0, y, 0.0, 0.0, 0.0, z] }

pub fn primitive(shape: &str, size: &[f64]) -> Prim {
    let z0 = size.first().copied().unwrap_or(0.5);
    let mut p = match shape {
        "box" => {
            let (w, h, d) = (z0, size.get(1).copied().unwrap_or(z0), size.get(2).copied().unwrap_or(z0));
            let hx = [w / 2.0, h / 2.0, d / 2.0];
            let mut verts = vec![];
            for x in [-1.0, 0.0, 1.0] { for y in [-1.0, 0.0, 1.0] { for k in [-1.0, 0.0, 1.0] {
                if (x != 0.0) as u8 + (y != 0.0) as u8 + (k != 0.0) as u8 >= 2 { verts.push([x * hx[0], y * hx[1], k * hx[2]]); } // corners + edge midpoints
            } } }
            let mut s = [w, h, d]; s.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let planes = [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]].iter()
                .map(|n: &V3| (*n, n[0].abs() * hx[0] + n[1].abs() * hx[1] + n[2].abs() * hx[2])).collect();
            Prim { shape: String::new(), size: vec![], kind: Kind::Planes, vol: w * h * d, com: Z3, i: diag((h * h + d * d) / 12.0, (w * w + d * d) / 12.0, (w * w + h * h) / 12.0),
                verts, planes, half: hx, r: len(hx), rad: 0.0, hh: 0.0, section: s[0] * s[1] }
        }
        "cylinder" => {
            let (r, h) = (z0, size.get(1).copied().unwrap_or(z0 * 2.0));
            let mut verts = ring(r, h / 2.0, 12); verts.extend(ring(r, -h / 2.0, 12)); verts.push([0.0, h / 2.0, 0.0]); verts.push([0.0, -h / 2.0, 0.0]);
            Prim { shape: String::new(), size: vec![], kind: Kind::Cyl, vol: PI * r * r * h, com: Z3, i: diag((3.0 * r * r + h * h) / 12.0, r * r / 2.0, (3.0 * r * r + h * h) / 12.0),
                verts, planes: vec![], half: [r, h / 2.0, r], r: hypot2(r, h / 2.0), rad: r, hh: h / 2.0, section: (PI * r * r).min(2.0 * r * h) }
        }
        "sphere" => {
            let r = z0; let (k, q) = (std::f64::consts::FRAC_1_SQRT_2 * r, r / S3);
            let mut verts = vec![[r, 0.0, 0.0], [-r, 0.0, 0.0], [0.0, r, 0.0], [0.0, -r, 0.0], [0.0, 0.0, r], [0.0, 0.0, -r]];
            for a in [-1.0, 1.0] { for b in [-1.0, 1.0] {
                verts.push([a * k, b * k, 0.0]); verts.push([a * k, 0.0, b * k]); verts.push([0.0, a * k, b * k]);
                for c in [-1.0, 1.0] { verts.push([a * q, b * q, c * q]); }
            } }
            Prim { shape: String::new(), size: vec![], kind: Kind::Sphere, vol: (4.0 / 3.0) * PI * r * r * r, com: Z3, i: diag(0.4 * r * r, 0.4 * r * r, 0.4 * r * r),
                verts, planes: vec![], half: [r, r, r], r, rad: r, hh: 0.0, section: PI * r * r }
        }
        _ => {
            let verts: Vec<V3> = if shape == "cone" {
                let (r, h) = (z0, size.get(1).copied().unwrap_or(z0 * 2.0));
                let mut v = vec![[0.0, h / 2.0, 0.0]]; v.extend(ring(r, -h / 2.0, 10)); v
            } else { platonic(shape).into_iter().map(|v| mul(norm(v), z0)).collect() };
            let (planes, tris) = hull(&verts);
            let (vol, com, i) = poly_mass(&tris);
            let mut half = [0.0f64; 3]; let mut r = 0.0f64;
            for v in &verts { for k in 0..3 { half[k] = half[k].max(v[k].abs()); } r = r.max(len(*v)); }
            Prim { shape: String::new(), size: vec![], kind: Kind::Planes, vol, com, i, verts, planes, half, r, rad: 0.0, hh: 0.0, section: 0.5 * half[0] * half[2] * PI }
        }
    };
    p.shape = shape.to_string(); p.size = size.to_vec();
    p
}

/// Signed distance (approximate outside, exact enough inside) and the outward normal at a local point.
pub fn sd(pr: &Prim, p: V3) -> (f64, V3) {
    match pr.kind {
        Kind::Sphere => { let mut l = len(p); if l == 0.0 { l = 1e-9; } (l - pr.rad, [p[0] / l, p[1] / l, p[2] / l]) }
        Kind::Cyl => {
            let mut rr = hypot2(p[0], p[2]); if rr == 0.0 { rr = 1e-9; }
            let (dr, dy) = (rr - pr.rad, p[1].abs() - pr.hh);
            if dr > dy { (dr, [p[0] / rr, 0.0, p[2] / rr]) } else { (dy, [0.0, if p[1] < 0.0 { -1.0 } else { 1.0 }, 0.0]) }
        }
        Kind::Planes => {
            let (mut best, mut bn) = (f64::NEG_INFINITY, UP);
            for (n, d) in &pr.planes { let s = dot(*n, p) - d; if s > best { best = s; bn = *n; } }
            (best, bn)
        }
    }
}
/// Ray o + t dir against the solid (local frame): (t, normal) of the entry point, or None (also when it starts inside).
pub fn ray(pr: &Prim, o: V3, dir: V3, tmax: f64) -> Option<(f64, V3)> {
    if pr.kind == Kind::Sphere {
        let (b, c, dd) = (dot(o, dir), dot(o, o) - pr.rad * pr.rad, dot(dir, dir));
        let disc = b * b - dd * c;
        if disc < 0.0 { return None; }
        let t = (-b - disc.sqrt()) / dd;
        return if t >= 0.0 && t <= tmax { Some((t, norm(add(o, mul(dir, t))))) } else { None };
    }
    let (mut t0, mut t1, mut n0): (f64, f64, Option<V3>) = (0.0, tmax, None);
    let clip = |n: V3, d: f64, t0: &mut f64, t1: &mut f64, n0: &mut Option<V3>| -> bool {
        let (den, num) = (dot(n, dir), d - dot(n, o));
        if den.abs() < 1e-12 { return num >= 0.0; }
        let t = num / den;
        if den < 0.0 { if t > *t0 { *t0 = t; *n0 = Some(n); } } else if t < *t1 { *t1 = t; }
        *t0 <= *t1
    };
    if pr.kind == Kind::Cyl {
        if !clip(UP, pr.hh, &mut t0, &mut t1, &mut n0) || !clip([0.0, -1.0, 0.0], pr.hh, &mut t0, &mut t1, &mut n0) { return None; }
        let (a, b, c) = (dir[0] * dir[0] + dir[2] * dir[2], o[0] * dir[0] + o[2] * dir[2], o[0] * o[0] + o[2] * o[2] - pr.rad * pr.rad);
        if a < 1e-12 { if c > 0.0 { return None; } } else {
            let disc = b * b - a * c;
            if disc < 0.0 { return None; }
            let s = disc.sqrt(); let (ta, tb) = ((-b - s) / a, (-b + s) / a);
            if ta > t0 { t0 = ta; let h = add(o, mul(dir, ta)); n0 = Some(norm([h[0], 0.0, h[2]])); }
            if tb < t1 { t1 = tb; }
            if t0 > t1 { return None; }
        }
    } else {
        for (n, d) in &pr.planes { if !clip(*n, *d, &mut t0, &mut t1, &mut n0) { return None; } }
    }
    n0.map(|n| (t0, n))
}
/// Pre-cut a part into about n box fragments of the same total volume (seeded jitter): (size, offset) each.
pub fn fragments(pr: &Prim, n: usize, rnd: &mut Rng) -> Vec<(V3, V3)> {
    let k = cbrt(pr.vol / (8.0 * pr.half[0] * pr.half[1] * pr.half[2]));
    let h = [pr.half[0] * k, pr.half[1] * k, pr.half[2] * k];
    let mut ax = [0usize, 1, 2];
    ax.sort_by(|a, b| h[*b].partial_cmp(&h[*a]).unwrap());
    let ga = (((n as f64) * h[ax[0]] / h[ax[1]]).sqrt() + 0.5).floor().max(1.0) as usize;
    let gb = ((n as f64 / ga as f64) + 0.5).floor().max(1.0) as usize;
    let mut cuts = |g: usize, hh: f64| -> Vec<f64> {
        let mut c = vec![-hh];
        for i in 1..g { c.push(-hh + (2.0 * hh * (i as f64 + (rnd.next() - 0.5) * 0.5)) / g as f64); }
        c.push(hh); c
    };
    let ca = cuts(ga, h[ax[0]]); let cb = cuts(gb, h[ax[1]]);
    let mut out = vec![];
    for i in 0..ga { for j in 0..gb {
        let (mut size, mut offset) = ([0.0; 3], [0.0; 3]);
        size[ax[0]] = ca[i + 1] - ca[i]; offset[ax[0]] = (ca[i + 1] + ca[i]) / 2.0;
        size[ax[1]] = cb[j + 1] - cb[j]; offset[ax[1]] = (cb[j + 1] + cb[j]) / 2.0;
        size[ax[2]] = 2.0 * h[ax[2]];
        out.push((size, offset));
    } }
    out
}

// Primitive solids: volume, centre of mass, inertia, collision vertices, signed distance, ray test, fragments.
// Everything is in the part's own frame (origin at the part's centre, Y up).
import { add, sub, mul, dot, cross, len, norm, m3point } from './m.js';

const T = (1 + Math.sqrt(5)) / 2;
const unit = (list, r) => list.map((v) => mul(norm(v), r));
const PLATONIC = { // the same vertex sets three.js uses, so physics and picture agree
  tetra: [[1, 1, 1], [-1, -1, 1], [-1, 1, -1], [1, -1, -1]],
  octa: [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]],
  ico: [[-1, T, 0], [1, T, 0], [-1, -T, 0], [1, -T, 0], [0, -1, T], [0, 1, T], [0, -1, -T], [0, 1, -T], [T, 0, -1], [T, 0, 1], [-T, 0, -1], [-T, 0, 1]],
  dodeca: [[-1, -1, -1], [-1, -1, 1], [-1, 1, -1], [-1, 1, 1], [1, -1, -1], [1, -1, 1], [1, 1, -1], [1, 1, 1],
    [0, -1 / T, -T], [0, -1 / T, T], [0, 1 / T, -T], [0, 1 / T, T], [-1 / T, -T, 0], [-1 / T, T, 0], [1 / T, -T, 0], [1 / T, T, 0], [-T, 0, -1 / T], [T, 0, -1 / T], [-T, 0, 1 / T], [T, 0, 1 / T]],
};
const ring = (r, y, n, ph = 0) => Array.from({ length: n }, (_, i) => [r * Math.cos(ph + (i / n) * 2 * Math.PI), y, r * Math.sin(ph + (i / n) * 2 * Math.PI)]);

// brute-force convex hull of a small vertex set -> outward face planes { n, d } and triangles
function hull(verts) {
  const planes = [], tris = [], seen = new Set(), N = verts.length;
  for (let i = 0; i < N; i++) for (let j = i + 1; j < N; j++) for (let k = j + 1; k < N; k++) {
    let n = cross(sub(verts[j], verts[i]), sub(verts[k], verts[i])); if (len(n) < 1e-9) continue; n = norm(n);
    let d = dot(n, verts[i]), pos = 0, neg = 0;
    for (const v of verts) { const s = dot(n, v) - d; if (s > 1e-7) pos++; else if (s < -1e-7) neg++; }
    if (pos && neg) continue; if (pos) { n = mul(n, -1); d = -d; }
    const key = n.map((x) => Math.round(x * 1e4)).join(',') + '|' + Math.round(d * 1e4); if (seen.has(key)) continue; seen.add(key);
    planes.push({ n, d });
    const on = verts.filter((v) => Math.abs(dot(n, v) - d) < 1e-7), c = mul(on.reduce(add, [0, 0, 0]), 1 / on.length);
    const e0 = norm(sub(on[0], c)), e1 = cross(n, e0); on.sort((a, b) => Math.atan2(dot(sub(a, c), e1), dot(sub(a, c), e0)) - Math.atan2(dot(sub(b, c), e1), dot(sub(b, c), e0)));
    for (let t = 1; t + 1 < on.length; t++) tris.push([on[0], on[t], on[t + 1]]);
  }
  return { planes, tris };
}
// mass properties of a closed triangle mesh of unit density: volume, centre of mass, inertia tensor about it
function polyMass(tris) {
  let V = 0, c = [0, 0, 0]; const C = new Array(9).fill(0);
  for (const [a, b, d] of tris) {
    const v = dot(a, cross(b, d)) / 6, s = add(add(a, b), d); V += v; c = add(c, mul(s, v / 4));
    for (let i = 0; i < 3; i++) for (let j = 0; j < 3; j++) C[i * 3 + j] += (v / 20) * (s[i] * s[j] + a[i] * a[j] + b[i] * b[j] + d[i] * d[j]);
  }
  c = mul(c, 1 / V); const tr = C[0] + C[4] + C[8];
  const I = [tr - C[0], -C[1], -C[2], -C[3], tr - C[4], -C[5], -C[6], -C[7], tr - C[8]], sh = m3point(V, c);
  return { vol: V, com: c, I: I.map((x, i) => (x - sh[i]) / V) }; // inertia per unit mass
}
const diag = (x, y, z) => [x, 0, 0, 0, y, 0, 0, 0, z];
const boxPlanes = (h) => [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]].map((n) => ({ n, d: Math.abs(n[0]) * h[0] + Math.abs(n[1]) * h[1] + Math.abs(n[2]) * h[2] }));

const cache = new Map();
export function primitive(shape, size) {
  const z = Array.isArray(size) ? size : [size], key = shape + ':' + z.join(','); if (cache.has(key)) return cache.get(key);
  let P;
  if (shape === 'box') {
    const [w, h, d] = [z[0], z[1] ?? z[0], z[2] ?? z[0]], hx = [w / 2, h / 2, d / 2], verts = [];
    for (const x of [-1, 0, 1]) for (const y of [-1, 0, 1]) for (const k of [-1, 0, 1]) if ((x !== 0) + (y !== 0) + (k !== 0) >= 2) verts.push([x * hx[0], y * hx[1], k * hx[2]]); // corners + edge midpoints
    const s = [w, h, d].sort((a, b) => a - b);
    P = { kind: 'planes', vol: w * h * d, com: [0, 0, 0], I: diag((h * h + d * d) / 12, (w * w + d * d) / 12, (w * w + h * h) / 12), verts, planes: boxPlanes(hx), half: hx, r: len(hx), section: s[0] * s[1] };
  } else if (shape === 'cylinder') {
    const [r, h] = [z[0], z[1] ?? z[0] * 2];
    P = { kind: 'cyl', vol: Math.PI * r * r * h, com: [0, 0, 0], I: diag((3 * r * r + h * h) / 12, r * r / 2, (3 * r * r + h * h) / 12), verts: [...ring(r, h / 2, 12), ...ring(r, -h / 2, 12), [0, h / 2, 0], [0, -h / 2, 0]],
      half: [r, h / 2, r], r: Math.hypot(r, h / 2), rad: r, hh: h / 2, section: Math.min(Math.PI * r * r, 2 * r * h) };
  } else if (shape === 'sphere') {
    const r = z[0], k = Math.SQRT1_2 * r, q = r / Math.sqrt(3), verts = [[r, 0, 0], [-r, 0, 0], [0, r, 0], [0, -r, 0], [0, 0, r], [0, 0, -r]];
    for (const a of [-1, 1]) for (const b of [-1, 1]) { verts.push([a * k, b * k, 0], [a * k, 0, b * k], [0, a * k, b * k]); for (const c of [-1, 1]) verts.push([a * q, b * q, c * q]); }
    P = { kind: 'sphere', vol: (4 / 3) * Math.PI * r ** 3, com: [0, 0, 0], I: diag(0.4 * r * r, 0.4 * r * r, 0.4 * r * r), verts, half: [r, r, r], r, rad: r, section: Math.PI * r * r };
  } else {
    let verts;
    if (shape === 'cone') { const [r, h] = [z[0], z[1] ?? z[0] * 2]; verts = [[0, h / 2, 0], ...ring(r, -h / 2, 10)]; }
    else verts = unit(PLATONIC[shape] || PLATONIC.dodeca, z[0]);
    const H = hull(verts), M = polyMass(H.tris), half = [0, 1, 2].map((i) => Math.max(...verts.map((v) => Math.abs(v[i]))));
    P = { kind: 'planes', ...M, verts, planes: H.planes, tris: H.tris, half, r: Math.max(...verts.map(len)), section: 0.5 * half[0] * half[2] * Math.PI };
  }
  P.shape = shape; P.size = z; cache.set(key, P); return P;
}

// signed distance (approximate outside, exact-enough inside) and outward normal at a local point: [d, nx, ny, nz]
export function sd(P, p) {
  if (P.kind === 'sphere') { const l = len(p) || 1e-9; return [l - P.rad, p[0] / l, p[1] / l, p[2] / l]; }
  if (P.kind === 'cyl') { const rr = Math.hypot(p[0], p[2]) || 1e-9, dr = rr - P.rad, dy = Math.abs(p[1]) - P.hh; return dr > dy ? [dr, p[0] / rr, 0, p[2] / rr] : [dy, 0, Math.sign(p[1]) || 1, 0]; }
  let best = -Infinity, bn = null; for (const pl of P.planes) { const s = dot(pl.n, p) - pl.d; if (s > best) { best = s; bn = pl.n; } }
  return [best, bn[0], bn[1], bn[2]];
}
// ray o + t·dir against the solid (local frame): { t, n } of the entry point, or null
export function ray(P, o, dir, tmax = 1e9) {
  if (P.kind === 'sphere') { const b = dot(o, dir), c = dot(o, o) - P.rad * P.rad, dd = dot(dir, dir), D = b * b - dd * c; if (D < 0) return null;
    const t = (-b - Math.sqrt(D)) / dd; return t >= 0 && t <= tmax ? { t, n: norm(add(o, mul(dir, t))) } : null; }
  let t0 = 0, t1 = tmax, n0 = null;
  const clip = (n, d) => { const den = dot(n, dir), num = d - dot(n, o); if (Math.abs(den) < 1e-12) return num >= 0; const t = num / den;
    if (den < 0) { if (t > t0) { t0 = t; n0 = n; } } else if (t < t1) t1 = t; return t0 <= t1; };
  if (P.kind === 'cyl') {
    if (!clip([0, 1, 0], P.hh) || !clip([0, -1, 0], P.hh)) return null;
    const a = dir[0] * dir[0] + dir[2] * dir[2], b = o[0] * dir[0] + o[2] * dir[2], c = o[0] * o[0] + o[2] * o[2] - P.rad * P.rad;
    if (a < 1e-12) { if (c > 0) return null; } else { const D = b * b - a * c; if (D < 0) return null; const s = Math.sqrt(D), ta = (-b - s) / a, tb = (-b + s) / a;
      if (ta > t0) { t0 = ta; const h = add(o, mul(dir, ta)); n0 = norm([h[0], 0, h[2]]); } if (tb < t1) t1 = tb; if (t0 > t1) return null; }
  } else for (const pl of P.planes) if (!clip(pl.n, pl.d)) return null;
  return n0 ? { t: t0, n: n0 } : null; // n0 null: the ray starts inside
}
// pre-cut a part into n box fragments of the same total volume (seeded jitter): [{ size, offset }]
export function fragments(P, n, rnd) {
  const k = Math.cbrt(P.vol / (8 * P.half[0] * P.half[1] * P.half[2])), h = P.half.map((x) => x * k), ax = [0, 1, 2].sort((a, b) => h[b] - h[a]);
  const ga = Math.max(1, Math.round(Math.sqrt(n * h[ax[0]] / h[ax[1]]))), gb = Math.max(1, Math.round(n / ga)), out = [];
  const cuts = (g, H) => { const c = [-H]; for (let i = 1; i < g; i++) c.push(-H + (2 * H * (i + (rnd() - 0.5) * 0.5)) / g); c.push(H); return c; };
  const ca = cuts(ga, h[ax[0]]), cb = cuts(gb, h[ax[1]]);
  for (let i = 0; i < ga; i++) for (let j = 0; j < gb; j++) { const size = [0, 0, 0], offset = [0, 0, 0];
    size[ax[0]] = ca[i + 1] - ca[i]; offset[ax[0]] = (ca[i + 1] + ca[i]) / 2; size[ax[1]] = cb[j + 1] - cb[j]; offset[ax[1]] = (cb[j + 1] + cb[j]) / 2; size[ax[2]] = 2 * h[ax[2]];
    out.push({ size, offset }); }
  return out;
}

// Tiny deterministic vector / quaternion / 3x3 math on plain arrays. No three.js here, so the
// physics, the scene derivation and the bake run unchanged in the browser and in node.
export const G = 9.81, DEG = Math.PI / 180, S3 = Math.sqrt(3);
export const clamp = (x, a, b) => Math.min(b, Math.max(a, x));
export const clamp01 = (x) => Math.min(1, Math.max(0, x));
export const smooth = (x) => { x = clamp01(x); return x * x * (3 - 2 * x); };
export const lerp = (a, b, t) => a + (b - a) * t;
export const add = (a, b) => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
export const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
export const mul = (a, s) => [a[0] * s, a[1] * s, a[2] * s];
export const adds = (a, b, s) => [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s];
export const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
export const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
export const len = (a) => Math.sqrt(dot(a, a));
export const dist = (a, b) => len(sub(a, b));
export const norm = (a) => { const l = len(a); return l > 1e-12 ? mul(a, 1 / l) : [0, 0, 0]; };
export const mix = (a, b, t) => [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)];
export const flat = (a) => [a[0], 0, a[2]];

// hex grid: pointy-top axial [q, r] (fractions allowed) <-> world metres on the stage floor (y = 0)
export const HEX = 1.0;
export const hexToWorld = ([q, r]) => [HEX * S3 * (q + r / 2), 0, HEX * 1.5 * r];
export const worldToHex = (p) => { const r = p[2] / (HEX * 1.5); return [p[0] / (HEX * S3) - r / 2, r]; };

// quaternions [x, y, z, w]
export const QI = [0, 0, 0, 1];
export const qmul = (a, b) => [
  a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
  a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
  a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
  a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2]];
export const qconj = (q) => [-q[0], -q[1], -q[2], q[3]];
export const qnorm = (q) => { const l = Math.hypot(q[0], q[1], q[2], q[3]) || 1; return [q[0] / l, q[1] / l, q[2] / l, q[3] / l]; };
export const qaxis = (ax, ang) => { const s = Math.sin(ang / 2); return [ax[0] * s, ax[1] * s, ax[2] * s, Math.cos(ang / 2)]; };
export const qrot = (q, v) => { // v + 2 q.xyz x (q.xyz x v + w v)
  const u = [q[0], q[1], q[2]], t = cross(u, add(cross(u, v), mul(v, q[3]))); return adds(v, t, 2); };
// Euler degrees, three.js 'XYZ' order (matches Object3D.rotation.set(x, y, z))
export const qeuler = ([x, y, z]) => qmul(qmul(qaxis([1, 0, 0], x * DEG), qaxis([0, 1, 0], y * DEG)), qaxis([0, 0, 1], z * DEG));
export const qstep = (q, w, dt) => qnorm(add4(q, mul4(qmul([w[0], w[1], w[2], 0], q), 0.5 * dt)));
const add4 = (a, b) => [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]], mul4 = (a, s) => [a[0] * s, a[1] * s, a[2] * s, a[3] * s];
export const qnlerp = (a, b, t) => { const s = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3] < 0 ? -1 : 1;
  return qnorm([lerp(a[0], s * b[0], t), lerp(a[1], s * b[1], t), lerp(a[2], s * b[2], t), lerp(a[3], s * b[3], t)]); };
// rigid transforms { p, q }
export const xf = (p, q) => ({ p, q });
export const xfPoint = (X, v) => add(X.p, qrot(X.q, v));
export const xfInv = (X) => { const qi = qconj(X.q); return { p: mul(qrot(qi, X.p), -1), q: qi }; };
export const xfMul = (A, B) => ({ p: xfPoint(A, B.p), q: qmul(A.q, B.q) });
export const xfUnpoint = (X, v) => qrot(qconj(X.q), sub(v, X.p));

// 3x3 matrices, row-major arrays of 9
export const m3q = (q) => { const [x, y, z, w] = q; return [
  1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w),
  2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w),
  2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]; };
export const m3v = (m, v) => [m[0] * v[0] + m[1] * v[1] + m[2] * v[2], m[3] * v[0] + m[4] * v[1] + m[5] * v[2], m[6] * v[0] + m[7] * v[1] + m[8] * v[2]];
export const m3mul = (a, b) => { const o = new Array(9); for (let i = 0; i < 3; i++) for (let j = 0; j < 3; j++) o[i * 3 + j] = a[i * 3] * b[j] + a[i * 3 + 1] * b[3 + j] + a[i * 3 + 2] * b[6 + j]; return o; };
export const m3t = (m) => [m[0], m[3], m[6], m[1], m[4], m[7], m[2], m[5], m[8]];
export const m3add = (a, b) => a.map((x, i) => x + b[i]);
export const m3inv = (m) => { const [a, b, c, d, e, f, g, h, i] = m, A = e * i - f * h, B = -(d * i - f * g), C = d * h - e * g, det = a * A + b * B + c * C || 1e-12;
  return [A / det, -(b * i - c * h) / det, (b * f - c * e) / det, B / det, (a * i - c * g) / det, -(a * f - c * d) / det, C / det, -(a * h - b * g) / det, (a * e - b * d) / det]; };
// inertia of a point mass m at offset d (parallel-axis term)
export const m3point = (m, d) => { const dd = dot(d, d); return [m * (dd - d[0] * d[0]), -m * d[0] * d[1], -m * d[0] * d[2], -m * d[1] * d[0], m * (dd - d[1] * d[1]), -m * d[1] * d[2], -m * d[2] * d[0], -m * d[2] * d[1], m * (dd - d[2] * d[2])]; };

// seeded RNG (mulberry32): the only source of "randomness" anywhere in the simulation
export const rng = (seed) => () => { seed = (seed + 0x6D2B79F5) | 0; let t = Math.imul(seed ^ (seed >>> 15), 1 | seed); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
// FNV-1a over the bytes of typed arrays / strings
export const fnv = (h, bytes) => { for (let i = 0; i < bytes.length; i++) { h ^= bytes[i]; h = Math.imul(h, 16777619); } return h >>> 0; };
// 2D convex hull (monotone chain) of [x, z] points, counter-clockwise
export function hull2(pts) {
  const p = [...pts].sort((a, b) => a[0] - b[0] || a[1] - b[1]), cr = (o, a, b) => (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]), h = [];
  for (const pass of [p, [...p].reverse()]) { const s = h.length; for (const q of pass) { while (h.length - s >= 2 && cr(h[h.length - 2], h[h.length - 1], q) <= 1e-9) h.pop(); h.push(q); } h.pop(); }
  return h;
}

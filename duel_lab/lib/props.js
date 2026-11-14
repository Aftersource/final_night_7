// Derived physical properties, computed once at load from a scene spec's parts:
// mass, centre of mass, inertia, bounds, support polygon, tipping angle, surfaces and parkour affordances.
import { add, sub, mul, adds, dot, cross, len, norm, clamp, hexToWorld, worldToHex, qeuler, qaxis, qmul, qrot, qconj, DEG, m3q, m3mul, m3t, m3add, m3point, hull2, G } from './m.js';
import { primitive, ray } from './shapes.js';
import { mat } from './materials.js';

export const STAGE = { hx: 9.9, hz: 7.9 }; // half extents of the playable floor inside the box rails
const RAILS = { id: '_rails', kind: 'rails', at: [0, 0], fixed: true, hidden: true, parts: [
  { shape: 'box', size: [22, 3, 1], offset: [0, 1.5, STAGE.hz + 0.5], material: 'wood' }, { shape: 'box', size: [22, 3, 1], offset: [0, 1.5, -STAGE.hz - 0.5], material: 'wood' },
  { shape: 'box', size: [1, 3, 18], offset: [STAGE.hx + 0.5, 1.5, 0], material: 'wood' }, { shape: 'box', size: [1, 3, 18], offset: [-STAGE.hx - 0.5, 1.5, 0], material: 'wood' }] };

// mass, centre of mass (in the frame the parts are given in) and inertia tensor about it, for a set of parts { prim, mat, lp, lq }
export function massOf(parts) {
  let m = 0, c = [0, 0, 0];
  for (const p of parts) { p.mass = p.mat.density * p.prim.vol * (p.fill ?? 1); p.c = add(p.lp, qrot(p.lq, p.prim.com)); m += p.mass; c = adds(c, p.c, p.mass); }
  c = mul(c, 1 / (m || 1)); let I = new Array(9).fill(0);
  for (const p of parts) { const R = m3q(p.lq); I = m3add(I, m3add(m3mul(m3mul(R, p.prim.I.map((x) => x * p.mass)), m3t(R)), m3point(p.mass, sub(p.c, c)))); }
  return { mass: m, com: c, I };
}
const partVerts = (o, p) => p.prim.verts.map((v) => add(o.p, qrot(o.q, add(p.lp, qrot(p.lq, v)))));

const inTop = (t, x, z) => { const d = [x - t.c[0], 0, z - t.c[2]]; if (t.round) return Math.hypot(d[0], d[2]) <= t.round;
  return Math.abs(dot(d, t.u)) <= dot(t.u, t.u) && Math.abs(dot(d, t.v)) <= dot(t.v, t.v); };

export function deriveScene(spec) {
  const objects = [], byId = {}, tops = [], faces = [], rounds = [], unders = [], bars = [];
  for (const [oi, s] of [...(spec.objects || []), RAILS].entries()) {
    const o = { id: s.id || 'obj' + oi, kind: s.kind || 'thing', spec: s, fixed: !!s.fixed, hidden: !!s.hidden, note: s.note || '', index: oi };
    o.q = qaxis([0, 1, 0], (s.yaw || 0) * DEG); o.p = hexToWorld(s.at || [0, 0]);
    o.parts = (s.parts || []).map((p, pi) => ({ pi, prim: primitive(p.shape, p.size), mat: mat(p.material), material: p.material || 'wood', fill: p.fill ?? 1, glow: p.glow, lp: p.offset || [0, 0, 0], lq: qeuler(p.rot || [0, 0, 0]), color: p.color }));
    if (!o.parts.length) continue;
    // settle: loose things rest exactly on the floor, or on the top of the object named by `on`
    let lo = Infinity; for (const p of o.parts) for (const v of partVerts(o, p)) lo = Math.min(lo, v[1]);
    const under = s.on && byId[s.on] ? Math.max(0, ...tops.filter((t) => t.obj === s.on && inTop(t, o.p[0], o.p[2])).map((t) => t.y), 0) || byId[s.on].aabb[1][1] : 0;
    if (!o.fixed || lo < 0 || s.on) o.p = [o.p[0], under - lo, o.p[2]];
    // world poses of the parts, bounds
    const mn = [1e9, 1e9, 1e9], mx = [-1e9, -1e9, -1e9], low = [];
    for (const p of o.parts) { p.wp = add(o.p, qrot(o.q, p.lp)); p.wq = qmul(o.q, p.lq); p.wv = partVerts(o, p);
      p.lo = Math.min(...p.wv.map((v) => v[1])); p.hi = Math.max(...p.wv.map((v) => v[1]));
      for (const v of p.wv) for (let i = 0; i < 3; i++) { mn[i] = Math.min(mn[i], v[i]); mx[i] = Math.max(mx[i], v[i]); } }
    o.aabb = [mn, mx];
    const M = massOf(o.parts); o.mass = M.mass; o.comLocal = M.com; o.I = M.I; o.com = add(o.p, qrot(o.q, M.com));
    o.radius = Math.max(...o.parts.flatMap((p) => p.wv.map((v) => len(sub(v, o.com)))));
    // support polygon (hull of everything touching the base) and the tilt at which the centre of mass passes its edge
    for (const p of o.parts) for (const v of p.wv) if (v[1] < mn[1] + 0.05) low.push([v[0], v[2]]);
    o.support = hull2(low); o.baseY = mn[1];
    let tip = Infinity, tipDir = [1, 0, 0]; const n = o.support.length, h = Math.max(1e-6, o.com[1] - mn[1]);
    for (let i = 0; i < n && n >= 3; i++) { const a = o.support[i], b = o.support[(i + 1) % n], e = [b[0] - a[0], b[1] - a[1]], el = Math.hypot(e[0], e[1]) || 1e-9;
      const d = Math.abs((o.com[0] - a[0]) * e[1] - (o.com[2] - a[1]) * e[0]) / el; if (d < tip) { tip = d; tipDir = [e[1] / el, 0, -e[0] / el]; } }
    if (n < 3) tip = 0;
    o.tipAngle = Math.atan2(tip, h) / DEG; o.tipDir = tipDir; o.tipEnergy = o.mass * G * (Math.hypot(tip, h) - h); // joules to push it over
    objects.push(o); byId[o.id] = o; if (!o.hidden) surfacesOf(o);
  }
  // ---- surfaces: walkable tops, vertical faces, undersides, bars ----
  function surfacesOf(o) {
    for (const p of o.parts) {
      const P = p.prim, ax = (v) => qrot(p.wq, v), up = ax([0, 1, 0]);
      if (P.shape === 'box') {
        const h = P.half, A = [ax([1, 0, 0]), ax([0, 1, 0]), ax([0, 0, 1])];
        const order = [0, 1, 2].sort((a, b) => h[b] - h[a]), L = order[0];
        if (h[L] >= 0.4 && h[order[1]] <= 0.11 && Math.abs(A[L][1]) < 0.2 && p.wp[1] >= 1.9 && p.wp[1] <= 3.3) bars.push({ obj: o.id, part: p.pi, c: p.wp, dir: A[L], half: h[L], y: p.wp[1], r: h[order[1]] });
        for (let i = 0; i < 3; i++) for (const s of [1, -1]) {
          const n = mul(A[i], s), c = adds(p.wp, n, h[i]), j = (i + 1) % 3, k = (i + 2) % 3, u = mul(A[j], h[j]), v = mul(A[k], h[k]);
          if (n[1] > 0.9) tops.push({ obj: o.id, part: p.pi, c, u, v, y: c[1], area: 4 * h[j] * h[k] });
          else if (n[1] < -0.9) unders.push({ obj: o.id, part: p.pi, c, u, v, y: c[1] });
          else if (Math.abs(n[1]) < 0.15) { const [hu, vu] = Math.abs(u[1]) < Math.abs(v[1]) ? [u, v] : [v, u];
            faces.push({ obj: o.id, part: p.pi, c, n: norm([n[0], 0, n[2]]), t: norm([hu[0], 0, hu[2]]), w: 2 * len(hu), h: 2 * len(vu), y0: c[1] - len(vu), y1: c[1] + len(vu) }); }
        }
      } else if (P.shape === 'cylinder') {
        if (up[1] > 0.95) { tops.push({ obj: o.id, part: p.pi, c: adds(p.wp, up, P.hh), round: P.rad, y: p.wp[1] + P.hh, area: Math.PI * P.rad * P.rad });
          rounds.push({ obj: o.id, part: p.pi, c: p.wp, r: P.rad, y0: p.wp[1] - P.hh, y1: p.wp[1] + P.hh }); }
        else if (Math.abs(up[1]) < 0.2 && P.rad <= 0.11 && P.hh >= 0.4 && p.wp[1] >= 1.9 && p.wp[1] <= 3.3) bars.push({ obj: o.id, part: p.pi, c: p.wp, dir: up, half: P.hh, y: p.wp[1], r: P.rad });
      }
    }
  }
  // height of the static scene under a point (highest top not above maxY)
  const groundAt = (x, z, maxY = 1e9) => { let y = 0; for (const t of tops) if (t.y <= maxY && t.y > y && inTop(t, x, z)) y = t.y; return y; };
  const topAt = (x, z, maxY = 1e9) => { let b = null; for (const t of tops) if (t.y <= maxY && (!b || t.y > b.y) && inTop(t, x, z)) b = t; return b; };
  // merge stacked vertical faces / drums of one object into tall runs (a pillar of drums is one wall-run)
  const tall = [];
  for (const o of objects) { const rs = rounds.filter((r) => r.obj === o.id).sort((a, b) => a.y0 - b.y0); let cur = null;
    for (const r of rs) { if (cur && Math.abs(r.y0 - cur.y1) < 0.05 && Math.hypot(r.c[0] - cur.c[0], r.c[2] - cur.c[2]) < 0.05) { cur.y1 = r.y1; cur.r = Math.min(cur.r, r.r); cur.partsAll.push(r.part); } else { cur = { ...r, partsAll: [r.part] }; tall.push(cur); } } }
  // free headroom above a top, and ledges (top edges with a drop beside them)
  const edges = [];
  for (const t of tops) {
    t.walkable = t.area >= 0.1 && !objects.some((o) => o.parts.some((p) => !(o.id === t.obj && p.pi === t.part) && p.lo > t.y - 0.02 && p.lo < t.y + 1.5 && Math.hypot(p.wp[0] - t.c[0], p.wp[2] - t.c[2]) < Math.max(p.prim.half[0], p.prim.half[2]) * 0.9));
    if (!t.walkable) continue;
    if (t.round) { edges.push({ obj: t.obj, part: t.part, round: t.round, c: t.c, y: t.y }); continue; }
    for (const [e, f] of [[t.u, t.v], [mul(t.u, -1), t.v], [t.v, t.u], [mul(t.v, -1), t.u]]) { const out = norm([e[0], 0, e[2]]), mid = add(t.c, e), q = adds(mid, out, 0.35);
      if (groundAt(q[0], q[2], t.y + 0.3) < t.y - 0.4) edges.push({ obj: t.obj, part: t.part, p0: sub(mid, f), p1: add(mid, f), mid, out, y: t.y, drop: t.y - groundAt(q[0], q[2], t.y + 0.3) }); }
  }
  // ---- affordances: what a body can do with each surface ----
  const aff = [], A = (kind, s, pos, extra = {}) => aff.push({ kind, obj: s.obj, part: s.part, pos, hex: worldToHex(pos).map((x) => +x.toFixed(2)), ...extra });
  for (const t of tops) { if (!t.walkable) continue; const thin = t.round ? 2 * t.round : 2 * Math.min(len(t.u), len(t.v));
    if (t.y >= 0.5 && t.y <= 1.2 && thin <= 1.7) A('vault', t, t.c, { top: t, height: t.y });
    if (t.y >= 1.0) A('perch', t, t.c, { top: t, height: t.y }); }
  for (const e of edges) if (e.y > 1.2 && e.y <= 2.6) A('mantle', e, e.round ? e.c : [e.mid[0], e.y, e.mid[2]], { edge: e, height: e.y });
  for (const f of faces) { if (f.y1 < 1.0 || byId[f.obj].hidden) continue;
    if (f.w >= 2 && f.h >= 1.5) A('wallrun', f, f.c, { face: f, mode: 'along' }); else if (f.h >= 2 && f.w >= 0.4) A('wallrun', f, f.c, { face: f, mode: 'up' });
    if (f.h >= 0.9 && f.w >= 0.4) A('wallkick', f, f.c, { face: f }); }
  for (const r of tall) { if (r.y1 - r.y0 >= 2) A('wallrun', r, [r.c[0], (r.y0 + r.y1) / 2, r.c[2]], { round: r, mode: 'up' }); if (r.y1 >= 1.0 && r.y1 - r.y0 >= 0.9 && r.r >= 0.15) A('wallkick', r, [r.c[0], Math.min(r.y1, 1.2), r.c[2]], { round: r }); }
  for (const u of unders) if (u.y >= 0.5 && u.y <= 1.15 && 2 * Math.min(len(u.u), len(u.v)) >= 0.6 && groundAt(u.c[0], u.c[2], u.y + 0.02) < 0.15) A('slide', u, u.c, { under: u, clearance: u.y });
  for (const b of bars) A('swing', b, b.c, { bar: b, height: b.y });
  return { spec, objects, byId, tops, faces, rounds: tall, unders, bars, edges, affordances: aff, groundAt, topAt, inTop,
    afford: (kind, obj) => aff.filter((a) => a.kind === kind && a.obj === obj) };
}
// ray against the live scene: bodies = [{ p, q, parts: [{ prim, lp, lq, dead }] }]; returns { t, n, body, part } or null
export function castRay(bodies, o, dir, tmax = 1e9, skip = null) {
  let best = null;
  for (const b of bodies) { if (b.dead || b === skip) continue; const oc = sub(b.p, o), along = dot(oc, dir); if (along < -b.radius || along > tmax + b.radius || dot(oc, oc) - along * along > b.radius * b.radius) continue;
    const qi = qconj(b.q), lo = qrot(qi, sub(o, b.p)), ld = qrot(qi, dir);
    for (const p of b.parts) { if (p.dead) continue; const pi = qconj(p.lq), h = ray(p.prim, qrot(pi, sub(lo, p.lp)), qrot(pi, ld), best ? best.t : tmax);
      if (h) best = { t: h.t, n: qrot(b.q, qrot(p.lq, h.n)), body: b, part: p }; } }
  return best;
}

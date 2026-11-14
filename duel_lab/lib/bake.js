// The bake: run the rigid-body simulation once over the whole duel at a fixed 120 Hz, driven by the resolved
// script (strikes, landings, wall-kicks, the fighters' bodies), and store every body's transform at 60 Hz.
// Playback at any time T only interpolates this record, so scrubbing backwards un-breaks the pillar exactly.
import { add, sub, mul, adds, dot, len, norm, flat, qrot, qmul, qconj, qnlerp, lerp, clamp, fnv, G } from './m.js';
import { World, DT } from './physics.js';
import { castRay } from './props.js';
import { rootAt } from './paths.js';

export const SAMPLE = 2, RATE = 1 / (DT * SAMPLE); // record every 2nd step: 60 samples a second
const LIMB = { hand: { m: 0.12, v: 10, y: 1.3 }, kick: { m: 0.25, v: 11, y: 0.75 }, heavy: { m: 0.5, v: 12, y: 1.35 }, slam: { m: 1.0, v: 9, y: 1.1 } };

export function bake(W, R, FB, seed = 7) {
  const world = new World(W, seed), steps = Math.ceil((R.TOTAL / 1000) / DT), rec = [], hits = {};
  // the transform that takes a scene part from where it started to where it is now (live, during the bake)
  const moved = (ref, p) => { const o = W.byId[ref.obj]; if (!o) return null; const sp = o.parts[ref.part] || o.parts[0];
    for (const b of world.bodies) { if (b.dead || b.obj !== ref.obj || b.frag) continue; const pt = b.parts.find((x) => x.src === sp.pi && !x.dead); if (!pt) continue;
      const wq = qmul(b.q, pt.lq), wp = add(b.p, qrot(b.q, pt.lp)); return add(wp, qrot(wq, qrot(qconj(sp.wq), sub(p, sp.wp)))); }
    return null; };
  const track = (b) => { if (rec[b.id]) return; rec[b.id] = { born: Math.ceil(world.t * RATE - 1e-6), pos: [], quat: [] }; };
  const sample = () => { for (const b of world.bodies) { track(b); const r = rec[b.id]; if (b.dead || (b.fixed && r.pos.length)) continue; const o = add(b.p, qrot(b.q, b.ref)); r.pos.push(o[0], o[1], o[2]); r.quat.push(b.q[0], b.q[1], b.q[2], b.q[3]); } };
  let ii = 0, si = 0; const strikes = [...R.strikes].sort((a, b) => a.t - b.t), prev = {}, roots = { a: [], b: [] };
  for (let i = 0; i <= steps; i++) {
    const t = world.t, Tms = t * 1000; world.kin = []; world.loads = [];
    const st = {}; for (const side of ['a', 'b']) {
      const s = (st[side] = rootAt(R.tracks[side], Tms, moved)), F = FB[side], piv = add(s.p, [0, 0.95, 0]), v = prev[side] ? mul(sub(piv, prev[side]), 1 / DT) : [0, 0, 0]; prev[side] = piv;
      const up = Math.cos(s.pitch) * Math.cos(s.roll);
      if (!s.hung) world.kin.push({ id: side + '0', who: side, c: piv, r: 0.3, v, m: F.mass }); if (!s.low && !s.hung) world.kin.push({ id: side + '1', who: side, c: add(piv, [0, 0.45 * up, 0]), r: 0.26, v, m: F.mass });
      for (const r of s.rides) if (r.w >= 0.99 && !s.air) { const b = world.body(r.ref.obj); if (b && !b.fixed) world.loads.push({ body: b, F: [0, -F.mass * G, 0], at: s.p }); } // standing weight
      if (i % SAMPLE === 0) roots[side].push(s.p[0], s.p[1], s.p[2]);
    }
    // parkour reactions and landings resolved by the script: the fighter's momentum goes into what it touched
    while (ii < R.impulses.length && R.impulses[ii].t <= t + 1e-9) { const e = R.impulses[ii++], cs = world.bodies.filter((x) => !x.dead && !x.frag && x.obj === e.obj); if (!cs.length) continue; const b = cs.reduce((m, x) => (len(sub(x.p, e.at)) < len(sub(m.p, e.at)) ? x : m));
      const at = e.at; if (e.soft && !world.partAt(b, at).mat.brittle) { world.pose(b); world.events.push({ t, type: 'push', obj: e.obj, body: b.id, at, J: len(e.J), dir: norm(e.J), by: e.by, why: e.why, fixed: b.fixed, mass: b.m }); world.push(b, e.J, at); }
      else world.strike(b, e.J, at, { by: e.by, why: e.why }); }
    // strikes: aim from the fighter at the thing, find the real surface point, deliver limb mass x speed
    while (si < strikes.length && strikes[si].t <= t + 1e-9) { const c = strikes[si++]; if (!c.on || c.on === 'foe') continue;
      const F = FB[c.by], s = st[c.by], L = LIMB[c.kind] || LIMB.hand, cands = world.bodies.filter((b) => !b.dead && !b.frag && b.obj === c.on); if (!cands.length) { hits[c.id] = null; continue; }
      const o = add(s.p, [0, c.y ?? L.y, 0]), b = cands.reduce((m, x) => (len(flat(sub(x.p, o))) < len(flat(sub(m.p, o))) ? x : m));
      for (const x of cands) world.pose(x);
      let aim = norm(flat(sub(b.p, o))), h = castRay(cands, o, aim, 2.4); if (!h) { aim = norm(sub(b.p, o)); h = castRay(cands, o, aim, 2.4); }
      if (!h) { hits[c.id] = null; world.events.push({ t, type: 'whiff', by: c.by, obj: c.on, at: adds(o, aim, 1) }); continue; }
      const at = adds(o, aim, h.t), J = mul(norm(flat(aim)), L.m * F.mass * L.v * F.speed * (c.power ?? 1));
      hits[c.id] = { at, body: h.body.id, t }; world.strike(h.body, J, at, { by: c.by, limb: c.limb, strike: c.id });
    }
    if (i % SAMPLE === 0) sample();
    world.step();
  }
  // pack
  const n = Math.floor(steps / SAMPLE) + 1; let h = 2166136261;
  const bodies = world.bodies.map((b) => { const r = rec[b.id] || { born: n, pos: [], quat: [] }, pos = new Float32Array(r.pos), quat = new Float32Array(r.quat);
    h = fnv(fnv(h, new Uint8Array(pos.buffer)), new Uint8Array(quat.buffer));
    return { id: b.id, obj: b.obj, fixed: b.fixed, hidden: b.hidden, frag: b.frag, piece: !!b.piece, born: r.born, count: pos.length / 3, pos, quat, t0: b.born, t1: b.died, radius: b.radius, mass: b.m, com: mul(b.ref, -1),
      parts: b.parts.map((p) => ({ prim: p.prim, material: p.material, mat: p.mat, color: p.color, glow: p.glow, lp: add(p.lp, mul(b.ref, -1)), lq: p.lq, src: p.src, frag: !!p.frag, t0: p.born, t1: p.died, damage: p.damage })) }; });
  for (const e of world.events) h = fnv(h, new TextEncoder().encode(e.type + e.obj + e.t.toFixed(6) + (e.J || 0).toFixed(3)));
  for (const k of ['a', 'b']) { roots[k] = new Float32Array(roots[k]); h = fnv(h, new Uint8Array(roots[k].buffer)); }
  return { n, rate: RATE, bodies, events: world.events, hits, roots, hash: h.toString(16).padStart(8, '0'), steps };
}

// ---- playback: the record at time T (ms) ----
export function bodyAt(b, T, out = { p: [0, 0, 0], q: [0, 0, 0, 1] }) {
  const x = clamp((T / 1000) * RATE - b.born, 0, b.count - 1), i = Math.min(Math.floor(x), Math.max(0, b.count - 2)), f = b.count > 1 ? x - i : 0, P = b.pos, Q = b.quat, j = b.count > 1 ? i + 1 : i;
  out.p = [lerp(P[i * 3], P[j * 3], f), lerp(P[i * 3 + 1], P[j * 3 + 1], f), lerp(P[i * 3 + 2], P[j * 3 + 2], f)];
  out.q = qnlerp([Q[i * 4], Q[i * 4 + 1], Q[i * 4 + 2], Q[i * 4 + 3]], [Q[j * 4], Q[j * 4 + 1], Q[j * 4 + 2], Q[j * 4 + 3]], f); return out;
}
export const alive = (x, T) => T / 1000 >= x.t0 && T / 1000 < x.t1;
// how a scene part has moved since the start, read from the record at time T: { p, q } with now = p + q * start
export function movedXf(W, B, T, ref) {
  const o = W.byId[ref.obj]; if (!o) return null; const sp = o.parts[ref.part] || o.parts[0];
  for (const b of B.bodies) { if (b.obj !== ref.obj || b.frag || !alive(b, T)) continue; const pt = b.parts.find((x) => x.src === sp.pi && alive(x, T)); if (!pt) continue;
    const X = bodyAt(b, T), q = qmul(qmul(X.q, pt.lq), qconj(sp.wq)); return { p: sub(add(X.p, qrot(X.q, pt.lp)), qrot(q, sp.wp)), q }; }
  return null;
}
export const movedAt = (W, B, T) => (ref, p) => { const X = movedXf(W, B, T, ref); return X ? add(X.p, qrot(X.q, p)) : null; };
// the live scene as ray-castable bodies at time T
export function sceneAt(B, T) { const out = []; for (const b of B.bodies) { if (b.hidden || !alive(b, T)) continue; const X = bodyAt(b, T); out.push({ p: X.p, q: X.q, radius: b.radius + 0.6, src: b, parts: b.parts.map((p) => ({ prim: p.prim, lp: p.lp, lq: p.lq, dead: !alive(p, T) })) }); } return out; }

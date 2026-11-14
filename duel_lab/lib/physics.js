// A tiny deterministic rigid-body engine: compound bodies of convex primitives, gravity, ground / static / body
// contacts found at vertices ("feature contacts"), Coulomb friction, restitution, sequential impulses, sleeping,
// damage per part, and breaking: compounds come apart at their joints, brittle parts shatter into pre-cut shards.
// Fixed step, no randomness except the seeded rng, no clock: the same input always gives the same result.
import { add, sub, mul, adds, dot, cross, len, norm, qrot, qconj, qmul, qstep, m3q, m3mul, m3t, m3v, m3inv, rng, G } from './m.js';
import { sd, fragments, primitive } from './shapes.js';
import { massOf } from './props.js';
import { CRACK_AT } from './materials.js';

export const DT = 1 / 120;
const FLOOR = { friction: 0.6, restitution: 0.1 }, ITER = 10, SLOP = 0.004, BETA = 0.2;
const tangents = (n) => { const a = Math.abs(n[1]) < 0.9 ? [0, 1, 0] : [1, 0, 0], t1 = norm(cross(n, a)); return [t1, cross(n, t1)]; };

export class World {
  constructor(scene, seed = 7) {
    this.bodies = []; this.events = []; this.t = 0; this.kin = []; this.loads = []; this.touching = new Set(); this.rnd = rng(seed);
    for (const o of scene.objects) this.spawn({ obj: o.id, fixed: o.fixed, hidden: o.hidden, p: o.p, q: o.q, asleep: true,
      parts: o.parts.map((p) => ({ prim: p.prim, mat: p.mat, material: p.material, color: p.color, glow: p.glow, fill: p.fill, lp: p.lp, lq: p.lq, src: p.pi })) });
  }
  // make a body from parts given in the frame (p, q); its own frame is moved to the centre of mass
  spawn({ obj, fixed = false, hidden = false, p, q, parts, v = [0, 0, 0], w = [0, 0, 0], asleep = false, frag = false }) {
    const b = { id: this.bodies.length, obj, fixed, hidden, frag, p, q, v, w, parts, asleep, still: 0, moves: 0, born: this.t, died: Infinity, dead: false };
    for (const pt of parts) { pt.damage = 0; pt.cracked = false; pt.dead = false; pt.jbreak = pt.mat.strength * (pt.mat.brittle ? pt.prim.section * Math.sqrt(pt.fill ?? 1) : Math.max(pt.prim.section, 0.1)); pt.born = this.t; pt.died = Infinity; }
    this.remass(b); this.bodies.push(b); return b;
  }
  remass(b) {
    const live = b.parts.filter((p) => !p.dead), M = massOf(live);
    b.p = add(b.p, qrot(b.q, M.com)); for (const p of b.parts) p.lp = sub(p.lp, M.com); b.ref = sub(b.ref || [0, 0, 0], M.com); // ref: the frame the parts were authored in
    b.m = M.mass; b.invM = b.fixed ? 0 : 1 / M.mass; b.Ib = M.I; b.invIb = b.fixed ? new Array(9).fill(0) : m3inv(M.I);
    b.radius = Math.max(...live.map((p) => len(p.lp) + p.prim.r)); b.stamp = -1;
  }
  // world-space caches for a body's parts (centres, orientations, vertices), refreshed when it has moved
  pose(b) {
    if (b.stamp === b.moves) return; b.stamp = b.moves;
    const R = m3q(b.q); b.invI = b.fixed ? b.invIb : m3mul(m3mul(R, b.invIb), m3t(R));
    for (const p of b.parts) { if (p.dead) continue; p.wp = add(b.p, qrot(b.q, p.lp)); p.wq = qmul(b.q, p.lq); p.wqi = qconj(p.wq); const n = p.prim.verts.length; p.wv = new Array(n);
      for (let i = 0; i < n; i++) p.wv[i] = add(p.wp, qrot(p.wq, p.prim.verts[i])); }
  }
  wake(b, deep = true) { if (b.asleep) { b.asleep = false; b.still = 0; } if (deep) this.wakeNear(b); }
  // whatever rests against a disturbed body has to wake too, or it would hang in the air when its support leaves
  wakeNear(b) { for (const o of this.bodies) if (o.asleep && !o.dead && !o.fixed && len(sub(o.p, b.p)) < o.radius + b.radius + 0.05) { o.asleep = false; o.still = 0; } }
  velAt(b, r) { return add(b.v, cross(b.w, r)); }
  push(b, J, at) { if (b.fixed) return; this.wake(b); this.pose(b); b.v = adds(b.v, J, b.invM); b.w = add(b.w, m3v(b.invI, cross(sub(at, b.p), J))); }
  // the part of a body nearest a world point
  partAt(b, at) { let best = null, bd = Infinity; this.pose(b); for (const p of b.parts) { if (p.dead) continue; const d = sd(p.prim, qrot(p.wqi, sub(at, p.wp)))[0]; if (d < bd) { bd = d; best = p; } } return best; }
  body(obj) { return this.bodies.find((b) => b.obj === obj && !b.dead && !b.frag); }
  // a blow from outside the simulation (a fighter's strike, a landing): damages the part, may break it, then moves what is left
  strike(b, J, at, tag = {}) {
    const p = this.partAt(b, at), j = len(J); this.pose(b);
    this.events.push({ t: this.t, type: 'hit', obj: b.obj, body: b.id, at, J: j, dir: norm(J), word: p.mat.word, sound: p.mat.sound, fixed: b.fixed, mass: b.m, ...tag });
    const target = this.harm(b, p, j, at, J) || b; if (!target.dead) this.push(target, J, at);
  }
  // add damage to a part; returns the body now carrying it if the part broke off
  harm(b, p, j, at, J) {
    if (p.frag || j < 0.15 * p.jbreak) return null; p.damage += j / p.jbreak;
    if (p.damage >= CRACK_AT && !p.cracked) { p.cracked = true; this.events.push({ t: this.t, type: 'crack', obj: b.obj, body: b.id, part: b.parts.indexOf(p), at, damage: p.damage }); }
    return p.damage >= 1 ? this.breakPart(b, p, at, J) : null;
  }
  breakPart(b, p, at, J) {
    const alive = b.parts.filter((x) => !x.dead), solo = alive.length === 1, vAt = (c) => (b.fixed ? [0, 0, 0] : this.velAt(b, sub(c, b.p)));
    this.pose(b);
    if (!p.mat.brittle && solo) return null; // one tough block: it cracks, it does not come apart
    if (p.mat.brittle) { // shatter into pre-cut shards
      if (p.frag || p.mat.shards < 2) return null;
      const fr = fragments(p.prim, p.mat.shards, this.rnd), made = [];
      for (const f of fr) made.push(this.spawn({ obj: b.obj, frag: true, p: add(p.wp, qrot(p.wq, f.offset)), q: p.wq, v: vAt(p.wp), w: b.w,
        parts: [{ prim: primitive('box', f.size), mat: p.mat, material: p.material, color: p.color, glow: p.glow, fill: p.fill, lp: [0, 0, 0], lq: [0, 0, 0, 1], src: p.src, frag: true }] }));
      // share the breaking impulse among the shards, nearest the blow most, and scatter them a little
      let sum = 0; const wt = made.map((f) => { const k = 1 / (1 + (len(sub(f.p, at)) / 0.3) ** 2); sum += k * f.m; return k; });
      made.forEach((f, i) => { if (J && b.fixed) f.v = adds(f.v, J, wt[i] / sum); const s = Math.min(1.5, 0.4 + len(J || [0, 0, 0]) / (b.m || 1));
        f.v = add(f.v, [(this.rnd() - 0.5) * s, this.rnd() * s, (this.rnd() - 0.5) * s]); f.w = [(this.rnd() - 0.5) * 8, (this.rnd() - 0.5) * 8, (this.rnd() - 0.5) * 8]; });
      this.wakeNear(b);
      p.dead = true; p.died = this.t; this.events.push({ t: this.t, type: 'shatter', obj: b.obj, body: b.id, part: b.parts.indexOf(p), at, word: p.mat.word, sound: p.mat.sound, shards: made.length });
      if (solo) { b.dead = true; b.died = this.t; } else { this.remass(b); this.wake(b); }
      return null;
    }
    // a joint gives way: a fixed thing loses the struck segment and everything above it, a loose thing comes fully apart
    const off = alive.filter((x) => !b.fixed || x.wp[1] >= p.wp[1] - 0.02); let hit = null;
    for (const x of off) { const nb = this.spawn({ obj: b.obj, p: x.wp, q: x.wq, v: vAt(x.wp), w: b.fixed ? [0, 0, 0] : b.w,
      parts: [{ prim: x.prim, mat: x.mat, material: x.material, color: x.color, glow: x.glow, fill: x.fill, lp: [0, 0, 0], lq: [0, 0, 0, 1], src: x.src }] });
      nb.parts[0].damage = x === p ? 0.5 : x.damage; nb.parts[0].cracked = x.cracked; nb.piece = true; x.dead = true; x.died = this.t; if (x === p) hit = nb; }
    this.wakeNear(b);
    this.events.push({ t: this.t, type: 'break', obj: b.obj, body: b.id, part: b.parts.indexOf(p), at, word: p.mat.word, sound: p.mat.sound, pieces: off.length });
    if (off.length === alive.length) { b.dead = true; b.died = this.t; } else this.remass(b);
    return hit;
  }

  step() {
    const dt = DT, B = this.bodies, C = [];
    for (const b of B) { if (b.dead) continue; b.moves = b.moves || 0; if (!b.fixed && !b.asleep) b.v = [b.v[0], b.v[1] - G * dt, b.v[2]]; this.pose(b); }
    for (const l of this.loads) if (!l.body.dead && !l.body.fixed && !l.body.asleep) this.push(l.body, mul(l.F, dt), l.at);
    // ---- contacts ----
    const contact = (a, b, pa, pb, p, n, depth) => { // n points from b's solid toward a's vertex
      const ra = sub(p, a.p), rb = sub(p, b.p), vn = dot(sub(this.velAt(a, ra), this.velAt(b, rb)), n);
      if (a.asleep && !b.fixed && !b.asleep && Math.abs(vn) > 0.3) this.wake(a, false); if (b.asleep && !a.asleep && Math.abs(vn) > 0.3) this.wake(b, false);
      C.push({ a, b, pa, pb, p, n, depth, ra, rb, vn0: vn, mu: Math.sqrt(pa.mat.friction * pb.mat.friction), e: Math.min(pa.mat.restitution, pb.mat.restitution) }); };
    for (let i = 0; i < B.length; i++) { const a = B[i]; if (a.dead) continue;
      if (!a.fixed && !a.asleep) for (const pa of a.parts) { if (pa.dead || pa.wp[1] - pa.prim.r > 0) continue; // the floor
        if (pa.prim.kind === 'sphere') { const y = pa.wp[1] - pa.prim.rad; if (y < 0) { const p = [pa.wp[0], y, pa.wp[2]], ra = sub(p, a.p); C.push({ a, b: null, pa, p, n: [0, 1, 0], depth: -y, ra, vn0: this.velAt(a, ra)[1], mu: Math.sqrt(pa.mat.friction * FLOOR.friction), e: Math.min(pa.mat.restitution, FLOOR.restitution) }); } continue; }
        for (const v of pa.wv) if (v[1] < 0) { const ra = sub(v, a.p); C.push({ a, b: null, pa, p: v, n: [0, 1, 0], depth: -v[1], ra, vn0: this.velAt(a, ra)[1], mu: Math.sqrt(pa.mat.friction * FLOOR.friction), e: Math.min(pa.mat.restitution, FLOOR.restitution) }); } }
      for (let j = i + 1; j < B.length; j++) { const b = B[j]; if (b.dead || ((a.fixed || a.asleep) && (b.fixed || b.asleep))) continue;
        const rr = a.radius + b.radius, d = sub(a.p, b.p); if (dot(d, d) > rr * rr) continue;
        for (const pa of a.parts) { if (pa.dead) continue; for (const pb of b.parts) { if (pb.dead) continue;
          const r2 = pa.prim.r + pb.prim.r, e = sub(pa.wp, pb.wp); if (dot(e, e) > r2 * r2) continue;
          for (const v of pa.wv) { const s = sd(pb.prim, qrot(pb.wqi, sub(v, pb.wp))); if (s[0] < 0) contact(a, b, pa, pb, v, qrot(pb.wq, [s[1], s[2], s[3]]), -s[0]); }
          for (const v of pb.wv) { const s = sd(pa.prim, qrot(pa.wqi, sub(v, pa.wp))); if (s[0] < 0) contact(b, a, pb, pa, v, qrot(pa.wq, [s[1], s[2], s[3]]), -s[0]); } } } } }
    // the fighters: spheres on scripted paths that carry real mass into a collision but are never pushed back
    const now = new Set();
    for (const k of this.kin) for (const a of B) { if (a.dead || a.fixed) continue; const d = sub(a.p, k.c), rr = a.radius + k.r; if (dot(d, d) > rr * rr) continue;
      for (const pa of a.parts) { if (pa.dead) continue; const s = sd(pa.prim, qrot(pa.wqi, sub(k.c, pa.wp))); if (s[0] >= k.r) continue;
        const nw = qrot(pa.wq, [s[1], s[2], s[3]]), p = adds(k.c, nw, -s[0]), n = mul(nw, -1), ra = sub(p, a.p), vn = dot(sub(this.velAt(a, ra), k.v), n), key = k.id + ':' + a.id; now.add(key);
        const first = !this.touching.has(key) && !now.has(key + '!'); now.add(key + '!');
        if (first && vn < -0.3) this.events.push({ t: this.t, type: 'touch', fighter: k.who, obj: a.obj, body: a.id, at: p, speed: -vn });
        if (a.asleep) { if (vn < -0.3) this.wake(a, false); else continue; }
        C.push({ a, b: null, kin: k, pa, p, n, depth: Math.min(0.05, k.r - s[0]), ra, vn0: vn, mu: 0.4, e: 0.1, maxN: k.m * (G * dt * 1.5 + (first ? Math.max(0, -vn) * 1.1 : 0)) }); } }
    this.touching = now;
    // ---- solve: sequential impulses ----
    for (const c of C) { const { a, b, n, ra, rb } = c; [c.t1, c.t2] = tangents(n); c.jn = 0; c.j1 = 0; c.j2 = 0;
      const ia = a.asleep ? 0 : a.invM, ib = c.kin ? 1 / c.kin.m : b && !b.asleep ? b.invM : 0; c.ia = ia; c.ib = ib;
      const k = (d) => { let s = ia + ib; if (ia) s += dot(cross(m3v(a.invI, cross(ra, d)), ra), d); if (ib && !c.kin) s += dot(cross(m3v(b.invI, cross(rb, d)), rb), d); return s > 1e-12 ? 1 / s : 0; };
      c.kn = k(n); c.k1 = k(c.t1); c.k2 = k(c.t2);
      c.target = Math.max(Math.min(1.5, (BETA / dt) * Math.max(0, c.depth - SLOP)), c.vn0 < -1 ? -c.e * c.vn0 : 0); }
    const rel = (c) => { let v = this.velAt(c.a, c.ra); if (c.kin) v = sub(v, c.kin.v); else if (c.b) v = sub(v, this.velAt(c.b, c.rb)); return v; };
    const apply = (c, P) => { const { a, b } = c; if (c.ia) { a.v = adds(a.v, P, c.ia); a.w = add(a.w, m3v(a.invI, cross(c.ra, P))); }
      if (c.ib && !c.kin) { b.v = adds(b.v, P, -c.ib); b.w = sub(b.w, m3v(b.invI, cross(c.rb, P))); } };
    for (let it = 0; it < ITER; it++) for (const c of C) {
      let v = rel(c), d = (c.target - dot(v, c.n)) * c.kn, j0 = c.jn; c.jn = Math.max(0, j0 + d); if (c.maxN !== undefined) c.jn = Math.min(c.jn, c.maxN); apply(c, mul(c.n, c.jn - j0));
      const lim = c.mu * c.jn; v = rel(c);
      d = -dot(v, c.t1) * c.k1; j0 = c.j1; c.j1 = Math.max(-lim, Math.min(lim, j0 + d)); apply(c, mul(c.t1, c.j1 - j0));
      v = rel(c); d = -dot(v, c.t2) * c.k2; j0 = c.j2; c.j2 = Math.max(-lim, Math.min(lim, j0 + d)); apply(c, mul(c.t2, c.j2 - j0));
    }
    // ---- impacts: damage, cracks, breaks (collected first: breaking changes the body list) ----
    const imp = new Map();
    for (const c of C) { if (c.vn0 > -1 || c.jn <= 0) continue; const key = c.a.id + ':' + c.a.parts.indexOf(c.pa) + ':' + (c.kin ? 'k' + c.kin.id : c.b ? c.b.id + ':' + c.b.parts.indexOf(c.pb) : 'floor');
      const e = imp.get(key) || { c, j: 0 }; e.j += c.jn; imp.set(key, e); }
    for (const { c, j } of imp.values()) {
      if (j > 25) this.events.push({ t: this.t, type: 'hit', obj: c.a.obj, body: c.a.id, with: c.kin ? "fighter:" + c.kin.who : c.b ? c.b.obj : "floor", body2: c.b ? c.b.id : -1, at: c.p, J: j, dir: c.n, word: c.pa.mat.word, sound: c.pa.mat.sound, fixed: c.a.fixed, mass: c.a.m, sim: true });
      if (!c.pa.dead && !c.a.dead) this.harm(c.a, c.pa, j, c.p, mul(c.n, j));
      if (c.b && !c.kin && !c.pb.dead && !c.b.dead) this.harm(c.b, c.pb, j, c.p, mul(c.n, -j));
    }
    // ---- integrate, damp, sleep ----
    for (const b of B) { if (b.dead || b.fixed || b.asleep) continue;
      b.p = adds(b.p, b.v, dt); b.q = qstep(b.q, b.w, dt); b.v = mul(b.v, 0.9995); b.w = mul(b.w, 0.994); b.moves++;
      if (dot(b.v, b.v) < 0.006 && dot(b.w, b.w) < 0.02) { b.still += dt; if (b.still > 0.5) { b.asleep = true; b.v = [0, 0, 0]; b.w = [0, 0, 0]; } } else b.still = 0;
      if (b.p[1] < -5) { b.dead = true; b.died = this.t; } }
    this.t += dt;
  }
}

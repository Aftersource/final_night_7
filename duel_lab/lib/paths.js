// The script resolver: turns the duel's panels into, for each fighter, a flat list of segments on the global
// timeline, each with a body trajectory and timed hand / foot contact targets on real surfaces of the scene.
// Everything here is worked out ahead of time from the script, the derived scene and the fighters' masses.
import { add, sub, mul, adds, dot, len, norm, flat, mix, lerp, clamp, clamp01, smooth, hexToWorld, G } from './m.js';
import { REF_MASS } from './materials.js';

export const AIR = new Set(['leap', 'flip', 'knock', 'drop']);
const UP = [0, 1, 0], bump = (x) => { x = clamp01(x); return Math.sin(Math.PI * x) ** 2; };
const pulse = (x, a, b, f = 0.08) => smooth((x - a) / f) * (1 - smooth((x - b) / f));

// a ballistic arc under real gravity: apex h above the higher end
function arc(from, to, h) {
  const peak = Math.max(from[1], to[1]) + Math.max(h, 0.01), tu = Math.sqrt(2 * (peak - from[1]) / G), td = Math.sqrt(2 * (peak - to[1]) / G), T = tu + td, vh = mul(flat(sub(to, from)), 1 / T), vy = G * tu;
  return { T, h, peak, v0: [vh[0], vy, vh[2]], v1: [vh[0], vy - G * T, vh[2]], at: (t) => [from[0] + vh[0] * t, from[1] + vy * t - 0.5 * G * t * t, from[2] + vh[2] * t] };
}
// lower the apex until the flight fits the time the script gives it
function fitArc(from, to, h, avail) { let a = arc(from, to, h); if (a.T <= avail) return a; let lo = 0.01, hi = h;
  for (let i = 0; i < 24; i++) { const m = (lo + hi) / 2; if (arc(from, to, m).T > avail) hi = m; else lo = m; } a = arc(from, to, lo); a.squeezed = a.T > avail + 0.02; return a; }

// the nearest wall of an object, as seen from a point: outward normal, the point of the surface at floor level, its height span
function wallOf(W, obj, from) {
  let best = null;
  for (const a of W.affordances) { if (a.obj !== obj || (a.kind !== 'wallkick' && a.kind !== 'wallrun')) continue; let c;
    if (a.round) { const r = a.round, n = norm([from[0] - r.c[0], 0, from[2] - r.c[2]]);
      c = { n, t: [-n[2], 0, n[0]], base: [r.c[0] + n[0] * r.r, 0, r.c[2] + n[2] * r.r], y0: r.y0, y1: r.y1, d: Math.hypot(from[0] - r.c[0], from[2] - r.c[2]) - r.r, w: 2 * r.r, ref: { obj, part: r.part } }; }
    else { const f = a.face, rel = sub(from, f.c), d = dot(rel, f.n); if (d <= 0) continue; const s = clamp(dot(rel, f.t), -f.w / 2 + 0.15, f.w / 2 - 0.15);
      c = { n: f.n, t: f.t, base: [f.c[0] + f.t[0] * s, 0, f.c[2] + f.t[2] * s], y0: f.y0, y1: f.y1, d: Math.hypot(d, dot(rel, f.t) - s), w: f.w, face: f, ref: { obj, part: f.part } }; }
    if (!best || c.d < best.d - 1e-6 || (Math.abs(c.d - best.d) < 1e-6 && c.y1 > best.y1)) best = c; }
  return best;
}
const topOf = (W, obj, near) => { let best = null, bd = 1e9; for (const t of W.tops) { if (t.obj !== obj || !t.walkable) continue; const d = len(flat(sub(t.c, near))) - t.y * 0.01; if (d < bd) { bd = d; best = t; } } return best; };
const rideOf = (W, top) => (top ? { obj: top.obj, part: top.part } : null);

// resolve one segment. cur = { p, ride, pitch } is where the fighter is; it is advanced to the segment's end.
function resolveSeg(s, ms, t0, cur, F, foe, W, out, side) {
  const from = cur.p, sec = ms / 1000, ev = (dt, obj, at, J, tag) => out.impulses.push({ t: (t0 + dt * 1000) / 1000, obj, at, J, by: side, ...tag });
  const kind = s.path || (s.to ? 'dash' : 'stay'), seg = { ...s, kind, t0, ms, from, side }, ride0 = cur.ride, pitch0 = cur.pitch || 0;
  const toXZ = s.to ? hexToWorld(s.to) : [from[0], 0, from[2]], warn = (m) => out.warnings.push(`${side}@${(t0 / 1000).toFixed(1)}s ${kind}: ${m}`);
  let to = [toXZ[0], W.groundAt(toXZ[0], toXZ[2], from[1] + 0.45), toXZ[2]], ride1 = null, pitch1 = 0, fn;
  const R = (a, w) => (a ? [{ ref: a, w }] : []);

  if (AIR.has(kind)) {
    if (s.onto) { const top = topOf(W, s.onto, s.to ? toXZ : from); if (top) { if (!s.to || !W.inTop(top, toXZ[0], toXZ[2])) to = [top.c[0], top.y, top.c[2]]; else to[1] = top.y; ride1 = rideOf(W, top); } else warn('no top on ' + s.onto); }
    else { const top = W.topAt(toXZ[0], toXZ[2], from[1] + 3); if (top) { to[1] = top.y; ride1 = rideOf(W, top); } else to[1] = 0; }
    const scale = kind === 'knock' ? F.jump * Math.min(1.5, Math.pow(foe.mass / REF_MASS, 0.25)) : F.jump;
    const h = kind === 'drop' ? 0.12 : (s.h ?? 1.2) * scale, lead = kind === 'knock' || kind === 'drop' ? 0 : s.lead !== undefined ? s.lead / 1000 : clamp((sec - arc(from, to, h).T) * 0.35, 0, 0.22);
    const a = fitArc(from, to, h, sec - lead - 0.02), k = a.squeezed ? a.T / (sec - lead) : 1; if (a.squeezed) warn('flight squeezed to fit ' + ms + ' ms');
    const back = s.back ?? pitch0 < -0.5, dir = norm(flat(sub(to, from)));
    seg.arc = a; seg.lead = lead;
    if (ride0) ev(lead, ride0.obj, from, mul(a.v0, -F.mass), { soft: true, why: 'take-off' });
    if (ride1) ev(lead + a.T / k, ride1.obj, to, mul([a.v1[0] * 0.5, a.v1[1], a.v1[2] * 0.5], F.mass), { why: 'landing' });
    fn = (tau) => { const ta = (tau / 1000 - lead) * k, u = clamp01(ta / a.T), air = ta >= 0 && ta < a.T;
      const pitch = kind === 'flip' ? (air || ta < 0 ? lerp(pitch0, back ? -2 * Math.PI : 2 * Math.PI, ta < 0 ? 0 : u) : 0) : kind === 'knock' ? -1.0 * Math.sin(Math.PI * u) * (s.spin ?? 1) : pitch0 * (1 - smooth(u * 3));
      return { p: ta <= 0 ? from : air ? a.at(ta) : to, air, aw: air ? pulse(ta, 0, a.T, 0.07) : 0, pitch, roll: 0, face: back && air ? mul(dir, -1) : kind === 'knock' ? mul(dir, -1) : air || s.face === 'path' ? dir : null,
        rides: [...R(ride0, 1 - smooth(u / 0.25)), ...R(ride1, smooth((u - 0.6) / 0.4))], ik: [] }; };
  } else if (kind === 'dash') {
    // a ground move: the feet follow the walkable tops under the line (steps, a quay), a pace at a time
    const N = 24, ys = [from[1]]; for (let i = 1; i <= N; i++) { const p = mix(from, to, i / N); ys.push(W.groundAt(p[0], p[2], ys[i - 1] + 0.45)); }
    const sm = ys.map((_, i) => (ys[Math.max(0, i - 1)] + ys[i] + ys[Math.min(N, i + 1)]) / 3); sm[0] = ys[0]; sm[N] = ys[N]; to[1] = ys[N];
    const top = W.topAt(to[0], to[2], to[1] + 0.01); ride1 = to[1] > 0.01 ? rideOf(W, top) : null; const dir = norm(flat(sub(to, from)));
    fn = (tau) => { const u = smooth(tau / ms), x = u * N, i = Math.min(N - 1, Math.floor(x)), p = mix(from, to, u); p[1] = lerp(sm[i], sm[i + 1], x - i);
      return { p, air: false, pitch: pitch0 * (1 - smooth(tau / 200)), roll: 0, face: s.face === 'foe' ? null : dir, rides: [...R(ride0, 1 - u), ...R(ride1, u)], ik: [] }; };
  } else if (kind === 'vault') {
    const top = topOf(W, s.over || s.on, mix(from, to, 0.5)); if (!top) { warn('nothing to vault: ' + (s.over || s.on)); return resolveSeg({ ...s, path: 'leap', h: 1 }, ms, t0, cur, F, foe, W, out, side); }
    const d = flat(sub(to, from)), L = len(d) || 1, dir = mul(d, 1 / L), uc = clamp(dot(sub(top.c, from), dir) / L, 0.2, 0.8), side_ = s.hand === 'l' ? -1 : 1;
    const yPeak = Math.max(0.1, top.y - 0.5), plant = add([from[0] + d[0] * uc, top.y, from[2] + d[2] * uc], mul([-dir[2], 0, dir[0]], 0.22 * side_)), ref = rideOf(W, top);
    ev(sec * (uc - 0.08), top.obj, plant, add(mul(UP, -F.mass * 2.2), mul(dir, F.mass * 0.6)), { soft: true, why: 'vault hand' });
    fn = (tau) => { const u = tau / ms, b = bump((u - uc) / 0.7 + 0.5), p = mix(from, to, u); p[1] = lerp(from[1], to[1], u) + yPeak * b;
      return { p, air: b > 0.25, hung: true, pitch: 0.25 * b, roll: 1.2 * b * side_, face: dir, rides: [],
        ik: [{ limb: side_ > 0 ? 'hand_r' : 'hand_l', p: plant, ref, w: pulse(u, uc - 0.24, uc + 0.14, 0.1) }] }; };
  } else if (kind === 'slide') {
    const dir = norm(flat(sub(to, from)));
    fn = (tau) => { const u = tau / ms, k = pulse(u, 0.02, 0.86, 0.14), p = mix(from, to, u); p[1] = lerp(from[1], to[1], u) - 0.74 * k;
      return { p, air: false, low: k > 0.5, hung: k > 0.2, pitch: -1.38 * k, roll: 0, face: dir, rides: [], ik: [] }; };
  } else if (kind === 'wallrun' || kind === 'wallkick') {
    const wall = wallOf(W, s.on, from); if (!wall) { warn('no wall on ' + s.on); return resolveSeg({ ...s, path: s.to ? 'leap' : undefined }, ms, t0, cur, F, foe, W, out, side); }
    const n = wall.n, along = kind === 'wallrun' && s.to && wall.face && wall.w >= 2;
    if (kind === 'wallkick') {
      // up to the wall, one foot on it, off again on a ballistic arc
      const hk = clamp(wall.y0 + (s.kick_h ?? 1.0), wall.y0 + 0.3, wall.y1 - 0.12), c = add(adds(wall.base, n, 0.5), [0, Math.max(from[1], hk - 0.45), 0]), foot = [wall.base[0], hk, wall.base[2]];
      const tA = clamp(len(flat(sub(c, from))) / 7, 0.18, sec * 0.45), a = fitArc(c, to, (s.h ?? 1.0) * F.jump, sec - tA - 0.02), vin = mul(sub(c, from), 1 / tA);
      const top = W.topAt(to[0], to[2], c[1] + 3); if (top && to[1] > 0.01) ride1 = rideOf(W, top);
      ev(tA, wall.ref.obj, foot, mul(sub(a.v0, vin), -F.mass), { soft: true, why: 'wall-kick' }); seg.arc = a; seg.lead = tA;
      const dOut = norm(flat(sub(to, c)));
      fn = (tau) => { const t = tau / 1000, k = clamp01(t / tA), ta = t - tA, air = ta < a.T; let p;
        if (ta < 0) { p = mix(from, c, k); p[1] = lerp(from[1], c[1], 2 * k - k * k); } else p = air ? a.at(ta) : to;
        return { p, air: air && (ta > 0 || k > 0.3), hung: ta < 0.08, pitch: -0.5 * pulse(t, tA - 0.15, tA + 0.05, 0.12), roll: 0, face: ta < 0 ? mul(n, -1) : dOut, rides: R(ride1, smooth((ta / a.T - 0.6) / 0.4)),
          ik: [{ limb: 'foot_r', p: foot, ref: wall.ref, w: pulse(t, tA - 0.13, tA + 0.07, 0.07) }] }; };
    } else if (along) {
      // along a wide wall: three or four strides on the face, the body rolled toward it
      const f = wall.face, s0 = clamp(dot(sub(from, f.c), f.t), -f.w / 2 + 0.3, f.w / 2 - 0.3), s1 = clamp(dot(sub(to, f.c), f.t), -f.w / 2 + 0.3, f.w / 2 - 0.3), sg = Math.sign(s1 - s0) || 1;
      const P = (sv, off, y) => [f.c[0] + f.t[0] * sv + n[0] * off, y, f.c[2] + f.t[2] * sv + n[2] * off], e0 = P(s0, 0.8, 0), e1 = P(s1, 0.8, 0), steps = s.steps || 4, tdir = mul(f.t, sg), rollS = dot([-tdir[2], 0, tdir[0]], n) > 0 ? 1 : -1;
      const hi = Math.min(wall.y1 - 1.2, (s.h ?? 0.9) * Math.min(1, F.jump * 1.2)), u0 = 0.22, u1 = 0.8;
      for (let i = 0; i < steps; i++) { const k = (i + 0.5) / steps; ev(sec * lerp(u0, u1, k), wall.ref.obj, P(lerp(s0, s1, k), 0, hi + 0.3), add(mul(n, -F.mass * 1.2), mul(UP, -F.mass * G * sec * (u1 - u0) / steps)), { soft: true, why: 'wall-run step' }); }
      fn = (tau) => { const u = tau / ms, k = clamp01((u - u0) / (u1 - u0)), on = pulse(u, u0, u1, 0.1), ik = [];
        const p = u < u0 ? mix(from, e0, u / u0) : u > u1 ? mix(e1, to, (u - u1) / (1 - u1)) : mix(e0, e1, k); p[1] = lerp(from[1], to[1], u) + hi * Math.sin(Math.PI * clamp01((u - u0 * 0.5) / (1 - u0 * 0.5 - (1 - u1) * 0.5)));
        const st = k * steps, i = Math.min(steps - 1, Math.floor(st)); if (on > 0) ik.push({ limb: i % 2 ? 'foot_l' : 'foot_r', p: P(lerp(s0, s1, (i + 0.5) / steps), 0, p[1] + 0.32), ref: wall.ref, w: on * bump(st - i) });
        return { p, air: on > 0.5, hung: on > 0.2, pitch: 0, roll: 0.62 * on * rollS, face: u < u0 ? norm(flat(sub(e0, from))) : u > u1 ? norm(flat(sub(to, e1))) : tdir, rides: [], ik }; };
    } else {
      // straight up a tall face: a run-in, then strides up the wall leaning back, until the legs run out of speed
      const P0 = 1.05, steps = s.steps || 3, rise = Math.min(wall.y1 - wall.y0 - 1.2, (s.rise ?? 2.3) * F.jump), u0 = clamp(len(flat(sub(adds(wall.base, n, 0.35), from))) / 6.5 / sec, 0.1, 0.5);
      const S = (pf) => adds(wall.base, n, 0.3 + 0.62 * pf); to = add(S(1), [0, wall.y0 + rise, 0]); pitch1 = -P0; seg.wall = wall; ride1 = wall.ref;
      for (let i = 0; i < steps; i++) { const k = (i + 0.5) / steps, kk = 1 - (1 - k) ** 2; ev(sec * lerp(u0, 1, k), wall.ref.obj, [wall.base[0], wall.y0 + rise * kk + 0.55, wall.base[2]], add(mul(n, -F.mass * 1.0), mul(UP, -F.mass * G * sec * (1 - u0) / steps)), { soft: true, why: 'wall-run step' }); }
      fn = (tau) => { const u = tau / ms, k = clamp01((u - u0) / (1 - u0)), ke = 1 - (1 - k) ** 2, pf = smooth((u - u0 * 0.6) / (u0 * 0.6 + 0.08)), ik = [];
        const p = u < u0 ? mix(from, S(0), smooth(u / u0)) : S(pf).slice(); if (u < u0) { const q = S(pf), w = smooth(u / u0); p[0] = lerp(from[0], q[0], w); p[2] = lerp(from[2], q[2], w); }
        p[1] = lerp(from[1], wall.y0, clamp01(u / u0)) + rise * ke;
        const st = k * steps, i = Math.min(steps - 1, Math.floor(st)); if (u >= u0) ik.push({ limb: i % 2 ? 'foot_l' : 'foot_r', p: [wall.base[0] + wall.t[0] * (i % 2 ? -0.09 : 0.09), p[1] + 0.62, wall.base[2] + wall.t[2] * (i % 2 ? -0.09 : 0.09)], ref: wall.ref, w: bump(st - i) });
        return { p, air: k > 0.05, hung: pf > 0.3, pitch: -P0 * pf, roll: 0, face: mul(n, -1), rides: R(wall.ref, pf), ik }; };
    }
  } else if (kind === 'mantle') {
    // grab the ledge, hang, pull up, stand on top
    let best = null, bd = 1e9; for (const a of W.affordances) { if (a.obj !== s.on || !a.edge) continue; const e = a.edge; let E, o, tg;
      if (e.round) { o = norm([from[0] - e.c[0], 0, from[2] - e.c[2]]); E = [e.c[0] + o[0] * e.round, e.y, e.c[2] + o[2] * e.round]; tg = [-o[2], 0, o[0]]; }
      else { const ed = sub(e.p1, e.p0), L = len(ed); tg = mul(ed, 1 / L); const sv = clamp(dot(sub(from, e.p0), tg), Math.min(0.25, L / 2), Math.max(L - 0.25, L / 2)); E = adds(e.p0, tg, sv); E[1] = e.y; o = e.out; }
      const d = len(flat(sub(E, from))) - 2 * dot(norm(flat(sub(from, E))), o); if (d < bd) { bd = d; best = { E, o, tg, e }; } }
    if (!best) { warn('no ledge on ' + s.on); return resolveSeg({ ...s, path: 'leap', onto: s.on }, ms, t0, cur, F, foe, W, out, side); }
    const { E, o, tg, e } = best, H = e.y, hang = add(adds(E, o, 0.3), [0, Math.max(from[1], H - 1.72) - H, 0]), ref = { obj: e.obj, part: e.part }, inset = e.round ? Math.min(0.35, e.round * 0.9) : 0.38;
    to = adds(E, o, -inset); ride1 = ref; ev(sec * 0.26, e.obj, E, mul(UP, -F.mass * 2.5), { soft: true, why: 'ledge grab' });
    fn = (tau) => { const u = tau / ms, pull = smooth((u - 0.3) / 0.42), over = smooth((u - 0.5) / 0.32), j = clamp01(u / 0.26); let p;
      if (u < 0.26) { p = mix(from, hang, j); p[1] = lerp(from[1], hang[1], 2 * j - j * j); } else { p = mix(hang, to, over); p[1] = lerp(hang[1], H, pull); }
      const w = pulse(u, 0.16, 0.74, 0.1);
      return { p, air: u > 0.05 && u < 0.7, hung: u < 0.85, pitch: 0.45 * pulse(u, 0.34, 0.8, 0.14), roll: 0, face: mul(o, -1), rides: R(ref, smooth((u - 0.15) / 0.15)),
        ik: [{ limb: 'hand_l', p: adds(E, tg, 0.2), ref, w }, { limb: 'hand_r', p: adds(E, tg, -0.2), ref, w }] }; };
  } else { // stay / perch: hold the spot (and ride whatever it is on)
    to = from; ride1 = ride0; if (kind === 'perch' && !ride0) warn('perch with nothing under the feet');
    fn = (tau) => ({ p: from, air: false, hung: Math.abs(pitch0) > 0.3 && tau < 150, pitch: pitch0 * (1 - smooth(tau / 220)), roll: 0, face: null, rides: R(ride0, 1), ik: [] });
  }
  seg.to = to; seg.fn = fn; seg.ride1 = ride1; cur.p = to; cur.ride = ride1; cur.pitch = pitch1; return seg;
}

export function resolveDuel(D, W, FB) {
  const out = { tracks: {}, impulses: [], strikes: [], warnings: [], PAN: [], TOTAL: 0 };
  let t = D.intro_ms; out.PAN = D.panels.map((p) => { const o = { ...p, t0: t }; t += p.play_ms; return o; }); out.TOTAL = t + D.outro_ms;
  for (const side of ['a', 'b']) {
    const F = FB[side], foe = FB[side === 'a' ? 'b' : 'a'], s0 = hexToWorld(D[side].start), cur = { p: [s0[0], W.groundAt(s0[0], s0[2]), s0[2]], ride: null, pitch: 0 }, tr = []; let tt = 0;
    const push = (s, ms) => { tr.push(resolveSeg(s, ms, tt, cur, F, foe, W, out, side)); tt += ms; };
    push({ clip: 'idle_guard', loop: true }, D.intro_ms);
    for (const p of D.panels) { let used = 0; const segs = p[side].segs; segs.forEach((s, i) => { const ms = i === segs.length - 1 ? Math.max(s.ms, p.play_ms - used) : s.ms; push(s, ms); used += ms; }); }
    push({ clip: tr[tr.length - 1].clip, at: 99999 }, D.outro_ms); out.tracks[side] = tr;
  }
  out.PAN.forEach((p, pi) => (p.contacts || []).forEach((c, ci) => out.strikes.push({ ...c, t: (p.t0 + c.t_ms) / 1000, panel: pi, ci, id: pi + ':' + ci })));
  out.impulses.sort((a, b) => a.t - b.t);
  return out;
}
export function segAt(tr, T) { for (let k = 0; k < tr.length; k++) if (T < tr[k].t0 + tr[k].ms) return k; return tr.length - 1; }
// where a fighter's root is at time T (ms). partXf(ref) gives the live transform taking a scene part's
// starting pose to where it is now, so anything standing on / holding a moving object moves with it.
export function rootAt(tr, T, moved) {
  const i = segAt(tr, T), s = tr[i], tau = Math.max(0, T - s.t0), st = s.fn(tau); st.seg = s; st.i = i; st.tau = tau;
  if (moved) { let p = st.p; for (const r of st.rides) if (r.w > 0) { const q = moved(r.ref, st.p); if (q) p = adds(p, sub(q, st.p), r.w); } st.p = p;
    for (const k of st.ik) if (k.ref) k.p = moved(k.ref, k.p) || k.p; }
  return st;
}

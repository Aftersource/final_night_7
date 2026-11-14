// Debug overlay (key D): what the engine knows. Centres of mass, support polygons, surfaces, parkour
// affordances, contact points and impulses from the bake, IK targets, the fighters' balance and flight arcs.
import * as THREE from 'three';
import { movedXf, bodyAt, alive } from './bake.js';
import { qrot, add, worldToHex } from './m.js';

const COL = { vault: '#4cff7a', mantle: '#ffd84c', perch: '#ff9d3c', wallrun: '#4cc3ff', wallkick: '#b07cff', slide: '#ff5ca8', swing: '#ffffff' };
const line = (pts, color, loop) => { const g = new THREE.BufferGeometry().setFromPoints(pts.map((p) => new THREE.Vector3(...p))); return new (loop ? THREE.LineLoop : THREE.Line)(g, new THREE.LineBasicMaterial({ color, depthTest: false, transparent: true })); };
function label(text, color) { const c = document.createElement('canvas'); c.width = 256; c.height = 48; const g = c.getContext('2d'); g.font = '700 26px monospace'; g.fillStyle = '#000b'; g.fillRect(0, 0, 256, 48); g.fillStyle = color; g.textBaseline = 'middle'; g.fillText(text, 8, 25);
  const s = new THREE.Sprite(new THREE.SpriteMaterial({ map: new THREE.CanvasTexture(c), depthTest: false, transparent: true, sizeAttenuation: false })); s.scale.set(0.085, 0.016, 1); return s; }
const dot = (r, color) => new THREE.Mesh(new THREE.SphereGeometry(r, 10, 8), new THREE.MeshBasicMaterial({ color, depthTest: false, transparent: true }));

export function makeDebug(scene3, W, B, R, fighters, panel) {
  const root = new THREE.Group(); root.visible = false; root.renderOrder = 999; scene3.add(root);
  // per object: a group that follows the object's baked motion, holding its static derived geometry
  const objs = W.objects.filter((o) => !o.hidden).map((o) => { const g = new THREE.Group(); g.matrixAutoUpdate = false; root.add(g);
    if (o.support.length >= 3) g.add(line(o.support.map(([x, z]) => [x, o.baseY + 0.02, z]), o.fixed ? '#888' : '#ffe14a', true));
    for (const t of W.tops) if (t.obj === o.id && t.walkable) g.add(t.round ? line(Array.from({ length: 17 }, (_, i) => [t.c[0] + t.round * Math.cos(i * 0.3927), t.y + 0.015, t.c[2] + t.round * Math.sin(i * 0.3927)]), '#4cff7a') : line([[1, 1], [1, -1], [-1, -1], [-1, 1]].map(([a, b]) => [t.c[0] + t.u[0] * a + t.v[0] * b, t.y + 0.015, t.c[2] + t.u[2] * a + t.v[2] * b]), '#4cff7a', true));
    for (const f of W.faces) if (f.obj === o.id && f.y1 >= 1) g.add(line([[-1, f.y0], [1, f.y0], [1, f.y1], [-1, f.y1]].map(([a, y]) => [f.c[0] + f.t[0] * a * f.w / 2 + f.n[0] * 0.02, y, f.c[2] + f.t[2] * a * f.w / 2 + f.n[2] * 0.02]), '#3c7cff', true));
    for (const e of W.edges) if (e.obj === o.id && !e.round) g.add(line([[e.p0[0], e.y + 0.03, e.p0[2]], [e.p1[0], e.y + 0.03, e.p1[2]]], '#ffd84c'));
    const seen = {}; for (const a of W.affordances) { if (a.obj !== o.id) continue; const k = a.kind + (a.mode || ''); seen[k] = (seen[k] || 0) + 1; if (seen[k] > 1) continue;
      const s = label(a.kind + (a.height ? ' ' + a.height.toFixed(1) : a.mode ? ' ' + a.mode : ''), COL[a.kind]); s.position.set(a.pos[0], Math.max(a.pos[1], 0.3) + 0.25 + 0.2 * Object.keys(seen).length, a.pos[2]); g.add(s); }
    return { o, g }; });
  const coms = B.bodies.filter((b) => !b.hidden && !b.frag).map((b) => { const m = dot(0.07, b.fixed ? '#aaa' : '#ffe14a'); root.add(m); return { b, m }; });
  const marks = B.events.filter((e) => e.at && (e.type === 'hit' || e.type === 'push' || e.type === 'touch') && (e.J || 99) > 40).map((e) => { const g = new THREE.Group(), m = dot(0.06, e.type === 'push' ? '#4cc3ff' : '#ff3030'); g.add(m);
    if (e.dir) g.add(line([[0, 0, 0], e.dir.map((x) => x * Math.min(1.4, 0.25 + e.J / 900))], '#ff3030')); g.position.set(...e.at); root.add(g); return { e, g }; });
  const F = fighters.map((f) => ({ f, com: dot(0.08, '#ffffff'), drop: line([[0, 0, 0], [0, -1, 0]], '#ffffff'), sup: line([[0, 0, 0], [1, 0, 0]], '#4cff7a'), arc: null, iks: Array.from({ length: 6 }, () => dot(0.05, '#ff3cf0')) }));
  for (const x of F) { root.add(x.com, x.drop, x.sup, ...x.iks); }
  const arcs = []; for (const side of ['a', 'b']) for (const s of R.tracks[side]) if (s.arc) { const st = s.arc.squeezed ? 1 : 1, pts = []; for (let i = 0; i <= 16; i++) pts.push(s.arc.at((s.arc.T * i) / 16)); const l = line(pts, side === 'a' ? '#ff8a80' : '#80d8ff'); root.add(l); arcs.push({ l, s }); }
  const M = new THREE.Matrix4(), Q = new THREE.Quaternion(), P = new THREE.Vector3(), ONE = new THREE.Vector3(1, 1, 1);
  const kg = (m) => (m >= 1000 ? (m / 1000).toFixed(2) + ' t' : m.toFixed(0) + ' kg');
  function update(T, info) {
    if (!root.visible) return; const ts = T / 1000;
    for (const { o, g } of objs) { const X = movedXf(W, B, T, { obj: o.id, part: 0 }); g.visible = !!X; if (X) { g.matrix.compose(P.set(...X.p), Q.set(...X.q), ONE); g.matrixWorldNeedsUpdate = true; } }
    for (const { b, m } of coms) { m.visible = alive(b, T); if (m.visible) { const X = bodyAt(b, T); m.position.set(...add(X.p, qrot(X.q, b.com))); } }
    for (const { e, g } of marks) g.visible = Math.abs(ts - e.t) < 0.35;
    for (const { l, s } of arcs) l.visible = T >= s.t0 && T < s.t0 + s.ms;
    for (const x of F) { const f = x.f; x.com.position.copy(f.com); x.drop.position.copy(f.com); x.drop.scale.y = f.com.y; x.sup.position.copy(f.feet[0]).setY(f.feet[0].y + 0.02); x.sup.scale.set(1, 1, 1);
      const d = f.feet[1].clone().sub(f.feet[0]); x.sup.quaternion.setFromUnitVectors(new THREE.Vector3(1, 0, 0), d.clone().normalize()); x.sup.scale.x = d.length(); x.sup.visible = !f.st.air;
      x.iks.forEach((m, i) => { m.visible = i < f.ik.length; if (m.visible) m.position.copy(f.ik[i]); }); }
    // the readout
    const rows = [`bake ${B.hash} · ${info.check} · ${B.bodies.length} bodies · ${B.steps} steps @120 Hz · ${info.bakeMs} ms · ${info.fps} fps`];
    for (const f of fighters) { const s = f.st, h = worldToHex(s.p); rows.push(`${f.side.toUpperCase()} ${f.skin} ${kg(f.body.mass)} jump×${f.body.jump.toFixed(2)} · ${s.seg.kind}${s.air ? ' AIR' : ''} · hex ${h[0].toFixed(1)},${h[1].toFixed(1)} y ${s.p[1].toFixed(2)} · com y ${f.com.y.toFixed(2)}${f.lean ? ' lean' : ''} · ik ${f.ik.length}`); }
    const dmg = {}; for (const b of B.bodies) if (!b.frag) for (const p of b.parts) dmg[b.obj] = Math.max(dmg[b.obj] || 0, p.damage);
    for (const o of W.objects) if (!o.hidden) { const ev = B.events.filter((e) => e.obj === o.id && e.t <= ts && (e.type === 'crack' || e.type === 'break' || e.type === 'shatter')).map((e) => e.type).pop();
      rows.push(`${o.id.padEnd(11)} ${kg(o.mass).padStart(7)} ${o.parts[0].material.padEnd(6)} tip ${o.tipAngle.toFixed(0).padStart(2)}°${o.fixed ? ' fixed' : ''}${ev ? ' · ' + ev.toUpperCase() : ''}`); }
    const recent = B.events.filter((e) => e.t <= ts && ts - e.t < 1.2 && (e.J || 0) > 40).slice(-4).map((e) => `${e.t.toFixed(2)}s ${e.type} ${e.obj} ${e.J.toFixed(0)} N·s ${e.why || e.with || (e.by ? 'by ' + e.by : '')}`);
    panel.textContent = rows.concat(recent.length ? ['—'].concat(recent) : []).join('\n');
  }
  return { root, update, toggle: () => { root.visible = !root.visible; panel.style.display = root.visible ? 'block' : 'none'; return root.visible; }, dispose: () => scene3.remove(root) };
}

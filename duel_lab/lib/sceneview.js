// Draws the baked scene at time T: one mesh per part of every baked body (objects, broken-off segments, shards),
// cracks, the shudder of anchored things, impact dust, the brazier's light, and see-through for anything
// standing between the camera and a fighter.
import * as THREE from 'three';
import { bodyAt, alive } from './bake.js';
import { sd, ray } from './shapes.js';
import { rng, qrot, qconj, sub, add } from './m.js';

const GEO = { tetra: THREE.TetrahedronGeometry, octa: THREE.OctahedronGeometry, ico: THREE.IcosahedronGeometry, dodeca: THREE.DodecahedronGeometry };
const DUST = { stone: '#b9b0a0', wood: '#c9a877', glass: '#d8f4ff', metal: '#ffb347', clay: '#c9764a', soft: '#d8c8b8', bone: '#e8e0c8' };
function geometry(P) {
  const z = P.size;
  if (P.shape === 'box') return new THREE.BoxGeometry(z[0], z[1] ?? z[0], z[2] ?? z[0]);
  if (P.shape === 'cylinder') return new THREE.CylinderGeometry(z[0], z[0], z[1] ?? z[0] * 2, 18);
  if (P.shape === 'cone') return new THREE.ConeGeometry(z[0], z[1] ?? z[0] * 2, 10);
  if (P.shape === 'sphere') return new THREE.SphereGeometry(z[0], 18, 12);
  return new (GEO[P.shape] || GEO.dodeca)(z[0], 0);
}
export function makeSceneView(scene3, W, B) {
  const group = new THREE.Group(), mats = new Map(), X = { p: [0, 0, 0], q: [0, 0, 0, 1] }, views = [], lights = [];
  scene3.add(group);
  const material = (p) => { const key = p.material + (p.color || '') + (p.glow || ''); if (mats.has(key)) return mats.get(key); const m = p.mat;
    const M = new THREE.MeshStandardMaterial({ color: p.color || m.color, roughness: m.roughness, metalness: m.metalness, flatShading: p.prim.shape !== 'cylinder' && p.prim.shape !== 'sphere',
      transparent: true, opacity: m.opacity ?? 1, depthWrite: (m.opacity ?? 1) > 0.9, emissive: p.glow ? p.color || m.color : '#000', emissiveIntensity: p.glow || 0 });
    M.userData.base = m.opacity ?? 1; mats.set(key, M); return M; };
  for (const b of B.bodies) { if (b.hidden) continue; const g = new THREE.Group(); g.matrixAutoUpdate = true; group.add(g); const v = { b, g, meshes: [], fade: 1 };
    for (const p of b.parts) { const m = new THREE.Mesh(geometry(p.prim), material(p).clone()); m.material.userData.base = p.mat.opacity ?? 1; m.position.set(...p.lp); m.quaternion.set(...p.lq); m.castShadow = (p.mat.opacity ?? 1) > 0.9; m.receiveShadow = true; g.add(m); v.meshes.push({ m, p });
      if (p.glow && !p.frag) { const l = new THREE.PointLight(p.color || '#ff8a2a', 0, 16, 1.6); m.add(l); lights.push({ l, p, b }); } }
    views.push(v); }
  // cracks: jagged dark lines walked over the surface of the struck part from the point of the blow
  const crackMat = new THREE.LineBasicMaterial({ color: '#050403' }), cracks = [];
  B.events.forEach((e, ei) => { if (e.type !== 'crack') return; const b = B.bodies[e.body], p = b.parts[e.part]; if (!p || b.hidden) return; const Xb = bodyAt(b, e.t * 1000), rnd = rng(1000 + ei), pts = [];
    const local = qrot(qconj(p.lq), sub(qrot(qconj(Xb.q), sub(e.at, Xb.p)), p.lp)), snap = (v) => { for (let k = 0; k < 3; k++) { const s = sd(p.prim, v); v = [v[0] - s[1] * (s[0] - 0.006), v[1] - s[2] * (s[0] - 0.006), v[2] - s[3] * (s[0] - 0.006)]; } return v; };
    for (let arm = 0; arm < 5; arm++) { let c = snap(local), dir = [rnd() - 0.5, (rnd() - 0.5) * 2.4, rnd() - 0.5]; for (let i = 0; i < 6; i++) { const n = snap(add(c, [dir[0] * 0.16 + (rnd() - 0.5) * 0.1, dir[1] * 0.16 + (rnd() - 0.5) * 0.1, dir[2] * 0.16 + (rnd() - 0.5) * 0.1])); pts.push(...c, ...n); c = n; } }
    const geo = new THREE.BufferGeometry(); geo.setAttribute('position', new THREE.Float32BufferAttribute(pts, 3));
    for (const v of views) if (v.b.obj === b.obj) for (const mp of v.meshes) if (mp.p.src === p.src && !mp.p.frag) { const ln = new THREE.LineSegments(geo, crackMat); mp.m.add(ln); cracks.push({ ln, t: e.t }); } });
  // impact dust / splinters / sparks: every particle is a closed-form function of the time since its hit
  const hits = B.events.filter((e) => (e.type === 'hit' && e.J > 40) || e.type === 'shatter' || e.type === 'break'), NP = 14, MAXP = 20 * NP;
  const pgeo = new THREE.BufferGeometry(), ppos = new Float32Array(MAXP * 3), pcol = new Float32Array(MAXP * 3); pgeo.setAttribute('position', new THREE.BufferAttribute(ppos, 3)); pgeo.setAttribute('color', new THREE.BufferAttribute(pcol, 3));
  const points = new THREE.Points(pgeo, new THREE.PointsMaterial({ size: 0.09, vertexColors: true, transparent: true, opacity: 0.9, depthWrite: false, sizeAttenuation: true })); points.frustumCulled = false; group.add(points);
  const seeds = hits.map((e, i) => { const r = rng(77 + i * 13), a = []; for (let k = 0; k < NP; k++) a.push([(r() - 0.5) * 2, r() * 1.6 + 0.3, (r() - 0.5) * 2, 0.5 + r()]); return a; }), col = new THREE.Color();
  const shudder = B.events.filter((e) => (e.type === 'hit' || e.type === 'push') && (e.fixed || (e.body2 >= 0 && B.bodies[e.body2].fixed)) && e.J > 60);
  const seg = [new THREE.Vector3(), new THREE.Vector3()];

  function update(T, cam, watch) {
    const ts = T / 1000;
    for (const v of views) { const b = v.b; v.g.visible = alive(b, T); if (!v.g.visible) continue; bodyAt(b, T, X); v.g.position.set(X.p[0], X.p[1], X.p[2]); v.g.quaternion.set(X.q[0], X.q[1], X.q[2], X.q[3]);
      for (const mp of v.meshes) mp.m.visible = alive(mp.p, T);
      // see-through: is any live part of this body on the line from the camera to a fighter?
      let block = 0; if (cam && !b.frag) { const qi = qconj(X.q); for (const w of watch) { const o = qrot(qi, sub(cam, X.p)), d = qrot(qi, sub(w, cam)); for (const mp of v.meshes) { if (!mp.m.visible) continue; const pi = qconj(mp.p.lq); if (ray(mp.p.prim, qrot(pi, sub(o, mp.p.lp)), qrot(pi, d), 0.94)) { block = 1; break; } } if (block) break; } }
      v.fade += ((block ? 0.22 : 1) - v.fade) * 1; for (const mp of v.meshes) { const M = mp.m.material, op = M.userData.base * v.fade; if (M.opacity !== op) { M.opacity = op; M.depthWrite = op > 0.9; } } }
    // anchored things cannot move in the simulation; what a blow does to them is a few millimetres of ringing
    for (const e of shudder) { const dt = ts - e.t; if (dt < 0 || dt > 0.9) continue; const id = e.fixed ? e.body : e.body2, v = views.find((x) => x.b.id === id); if (!v || !v.g.visible) continue;
      const A = Math.min(0.035, (e.J / B.bodies[id].mass) * 0.03) * Math.exp(-dt / 0.16) * Math.sin(dt * 2 * Math.PI * 13); v.g.position.x += e.dir[0] * A; v.g.position.z += e.dir[2] * A; v.g.rotation.z += A * 0.25; }
    for (const c of cracks) c.ln.visible = ts >= c.t;
    let n = 0; hits.forEach((e, i) => { const dt = ts - e.t; if (dt < 0 || dt > 0.8 || n + NP > MAXP) return; col.set(DUST[e.sound] || '#ccc'); const k = Math.min(1.6, 0.5 + (e.J || 200) / 600), f = 1 - dt / 0.8;
      for (const s of seeds[i]) { const d = dt * s[3]; ppos[n * 3] = e.at[0] + s[0] * k * d * 1.6; ppos[n * 3 + 1] = Math.max(0.02, e.at[1] + s[1] * k * d * 1.4 - 4.9 * d * d); ppos[n * 3 + 2] = e.at[2] + s[2] * k * d * 1.6; pcol[n * 3] = col.r * f; pcol[n * 3 + 1] = col.g * f; pcol[n * 3 + 2] = col.b * f; n++; } });
    pgeo.setDrawRange(0, n); pgeo.attributes.position.needsUpdate = true; pgeo.attributes.color.needsUpdate = true;
    for (const { l, p, b } of lights) l.intensity = alive(b, T) && alive(p, T) ? 38 * (0.82 + 0.18 * Math.sin(T * 0.021) * Math.sin(T * 0.047 + 1)) : 0;
  }
  const dispose = () => { scene3.remove(group); group.traverse((o) => { if (o.geometry) o.geometry.dispose(); if (o.material && o.material.dispose) o.material.dispose(); }); };
  return { update, dispose, group, views };
}

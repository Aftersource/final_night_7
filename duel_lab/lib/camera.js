// The camera: every shot is fitted so that both fighters are inside the safe part of the frame (clear of the
// caption and the typing tray), with headroom when one is airborne. A pure function of the timeline.
import * as THREE from 'three';
import { smooth, clamp01, lerp } from './m.js';

const V = (x = 0, y = 0, z = 0) => new THREE.Vector3(x, y, z), UPV = V(0, 1, 0);
const SAFE = { x: 0.84, top: 0.78, bottom: -0.36 }, R = 1.12; // safe frame in NDC, and the radius of a fighter's bounding sphere
// direction from the subject to the camera for a named shot; d = A->B on the floor, n = its normal facing the audience
function style(name, d, n) {
  switch (name) {
    case 'side': return { dir: n.clone().multiplyScalar(1).add(V(0, 0.26, 0)), min: 6.5, bias: 0 };
    case 'low': return { dir: n.clone().add(V(0, 0.06, 0)), min: 6, bias: 0 };
    case 'over_a': return { dir: d.clone().multiplyScalar(-1).addScaledVector(n, 0.55).add(V(0, 0.42, 0)), min: 6, bias: 0 };
    case 'over_b': return { dir: d.clone().addScaledVector(n, 0.55).add(V(0, 0.42, 0)), min: 6, bias: 0 };
    case 'close_a': return { dir: n.clone().addScaledVector(d, 0.45).add(V(0, 0.22, 0)), min: 5, bias: -0.35 };
    case 'close_b': return { dir: n.clone().addScaledVector(d, -0.45).add(V(0, 0.22, 0)), min: 5, bias: 0.35 };
    default: return { dir: V(0, 0.5, 1), min: 12, bias: 0 }; // wide: from the front of the box
  }
}
// fit: the nearest distance along `dir` at which every sphere is inside the safe frame
function fit(pts, dir, cam, min) {
  dir.normalize(); const right = V().crossVectors(UPV, dir).normalize(), up = V().crossVectors(dir, right), tv = Math.tan((cam.fov * Math.PI) / 360), th = tv * cam.aspect;
  let x0 = 1e9, x1 = -1e9, y0 = 1e9, y1 = -1e9; for (const p of pts) { const x = p.dot(right), y = p.dot(up); x0 = Math.min(x0, x - R); x1 = Math.max(x1, x + R); y0 = Math.min(y0, y - R); y1 = Math.max(y1, y + R); }
  const cx = (x0 + x1) / 2, cy = (y0 + y1) / 2, sy = (SAFE.top - SAFE.bottom) / 2, oy = (SAFE.top + SAFE.bottom) / 2; let dist = min, zc = 0;
  for (const p of pts) zc += p.dot(dir) / pts.length;
  for (const p of pts) { const z = p.dot(dir) - zc; dist = Math.max(dist, (Math.abs(p.dot(right) - cx) + R) / (th * SAFE.x) + z, (Math.abs(p.dot(up) - cy) + R) / (tv * sy) + z); }
  const centre = V().addScaledVector(right, cx).addScaledVector(up, cy).addScaledVector(dir, zc), pos = centre.clone().addScaledVector(dir, dist);
  if (pos.y < 0.45) pos.y = 0.45;
  return { pos, look: centre.clone().addScaledVector(up, -oy * tv * dist), dist };
}
export function shot(name, A, B, cam, o = {}) {
  if (name === 'box') return { pos: V(0, 13, 33), look: V(0, 3.5, -2) };
  const a = V(A.x, A.y + 0.95, A.z), b = V(B.x, B.y + 0.95, B.z), d = V(B.x - A.x, 0, B.z - A.z); if (d.lengthSq() < 0.01) d.set(1, 0, 0); d.normalize();
  const n = V(-d.z, 0, d.x); if (n.z < 0) n.negate(); const s = style(name, d, n);
  if (o.yaw) s.dir.applyAxisAngle(UPV, o.yaw); if (s.dir.z < 0.12 && name !== 'over_a' && name !== 'over_b') { s.dir.z = 0.12; }
  const f = fit([a, b], s.dir, cam, s.min * (o.scale || 1));
  if (s.bias) f.look.addScaledVector(d, s.bias * 0.6);
  return f;
}
// the camera for a time inside panel p: its shot, eased in from the previous panel's, plus the panel's move
export function cameraAt(cam, p, prev, tau, A, B) {
  const k = clamp01(tau / p.play_ms), o = {};
  if (p.move === 'push') o.scale = lerp(1.32, 1.0, smooth(k * 1.15));            // a slow push-in over the panel
  if (p.move === 'whip') { const at = (p.whip_ms ?? p.play_ms * 0.42) / p.play_ms; o.yaw = lerp(-0.85, 0.85, smooth((k - at + 0.07) / 0.14)); } // a whip from one side of the fight to the other
  const cur = shot(p.shot, A, B, cam, o); if (!prev) return cur;
  const e = smooth(tau / 600); if (e >= 1) return cur; const pv = shot(prev.shot, A, B, cam, {});
  return { pos: pv.pos.lerp(cur.pos, e), look: pv.look.lerp(cur.look, e) };
}
// last line of defence: after any blend or shake, if a fighter is outside the safe frame, pull straight back until it is not
export function keepInFrame(cam, pos, look, pts) {
  const v = V(), dir = pos.clone().sub(look).normalize();
  for (let i = 0; i < 14; i++) { cam.position.copy(pos); cam.lookAt(look); cam.updateMatrixWorld(); let ok = true;
    for (const p of pts) { v.copy(p).project(cam); if (Math.abs(v.x) > 0.93 || v.y > 0.9 || v.y < -0.52 || v.z > 1) { ok = false; break; } }
    if (ok) return i; pos.addScaledVector(dir, 0.9 + i * 0.25); }
  return 14;
}

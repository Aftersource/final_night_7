// Analytic two-bone IK (shoulder-elbow-hand, hip-knee-foot), applied on top of the animation clip:
// bend the middle joint until the chain is the right length, swing the root joint until the end is on the target.
import * as THREE from 'three';
const A = new THREE.Vector3(), B = new THREE.Vector3(), C = new THREE.Vector3(), ax = new THREE.Vector3(), v1 = new THREE.Vector3(), v2 = new THREE.Vector3();
const qp = new THREE.Quaternion(), qr = new THREE.Quaternion(), q0 = new THREE.Quaternion(), q1 = new THREE.Quaternion();
const wpos = (b, o) => o.setFromMatrixPosition(b.matrixWorld);
// rotate a bone by a world-space rotation r, keeping its place in the hierarchy
function rotateWorld(bone, r) { bone.parent.getWorldQuaternion(qp); bone.quaternion.premultiply(qp).premultiply(r).premultiply(qp.invert()); }

export function twoBone(upper, lower, end, target, weight = 1, pole = null) {
  if (weight <= 0.002) return 0;
  q0.copy(upper.quaternion); q1.copy(lower.quaternion);
  wpos(upper, A); wpos(lower, B); wpos(end, C);
  const lab = A.distanceTo(B), lcb = B.distanceTo(C), reach = (lab + lcb) * 0.998, want = A.distanceTo(target), lat = Math.min(Math.max(want, Math.abs(lab - lcb) + 0.01), reach);
  // 1. the middle joint: open or close it so |A-C| equals |A-target|
  v1.subVectors(B, A); v2.subVectors(C, B); ax.crossVectors(v1, v2);
  if (ax.lengthSq() < 1e-8) { if (pole) ax.crossVectors(v1, pole); else ax.set(1, 0, 0); } ax.normalize();
  const cos0 = THREE.MathUtils.clamp(v1.clone().negate().normalize().dot(v2.clone().normalize()), -1, 1), cos1 = THREE.MathUtils.clamp((lab * lab + lcb * lcb - lat * lat) / (2 * lab * lcb), -1, 1);
  rotateWorld(lower, qr.setFromAxisAngle(ax, Math.acos(cos0) - Math.acos(cos1))); upper.updateMatrixWorld(true);
  // 2. the root joint: swing the whole chain so the end lands on the target
  wpos(end, C); v1.subVectors(C, A).normalize(); v2.subVectors(target, A).normalize();
  rotateWorld(upper, qr.setFromUnitVectors(v1, v2));
  if (weight < 0.998) { upper.quaternion.copy(q0.slerp(upper.quaternion, weight)); lower.quaternion.copy(q1.slerp(lower.quaternion, weight)); }
  upper.updateMatrixWorld(true);
  return want > reach ? want - reach : 0; // how far out of reach the target was
}

// The fighters: the rigged mannequin, its clips and skins, its mass (measured from the mesh and the skin's
// material), and the per-frame pose: clip -> root on the resolved path -> centre of mass -> IK on real contacts.
import * as THREE from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import * as SkeletonUtils from 'three/addons/utils/SkeletonUtils.js';
import { twoBone } from './ik.js';
import { rootAt } from './paths.js';
import { castRay } from './props.js';
import { movedAt } from './bake.js';
import { fighterBody, SEGMENTS, FIGURE_VOLUME } from './materials.js';
import { smooth, clamp01 } from './m.js';

export const SKINS = ['wood', 'obsidian', 'gold', 'dragonscale', 'butterfly'];
export const CLIPS = ['idle_guard', 'walk', 'run', 'strike_side', 'strike_slash', 'lunge', 'heavy_charged', 'heavy_hammer', 'jump_attack', 'combo_triple',
  'parry_high', 'parry_back', 'block', 'block_cross', 'shield_push', 'evade_stand', 'evade_roll', 'evade_spin', 'counter', 'kick', 'throw_shoulder',
  'grapple_throw_down', 'stagger_back', 'stagger_waist', 'airborne_hit', 'downed', 'rise', 'defeat', 'invoke', 'command', 'ground_slam', 'taunt',
  'victory', 'bow', 'kneel'];
const CHILD = { pelvis: 'spine_01', spine_01: 'spine_02', spine_02: 'spine_03', spine_03: 'neck_01', neck_01: 'head', head: 'head_end', clavicle_l: 'upperarm_l', upperarm_l: 'lowerarm_l', lowerarm_l: 'hand_l', hand_l: 'LeftHand_End',
  clavicle_r: 'upperarm_r', upperarm_r: 'lowerarm_r', lowerarm_r: 'hand_r', hand_r: 'RightHand_End', thigh_l: 'calf_l', calf_l: 'foot_l', foot_l: 'ball_l', ball_l: 'LeftToe_end', thigh_r: 'calf_r', calf_r: 'foot_r', foot_r: 'ball_r', ball_r: 'RightToe_end' };
const LIMBS = { hand_l: ['upperarm_l', 'lowerarm_l', 'hand_l'], hand_r: ['upperarm_r', 'lowerarm_r', 'hand_r'], foot_l: ['thigh_l', 'calf_l', 'foot_l'], foot_r: ['thigh_r', 'calf_r', 'foot_r'] };
const AIM = { head: 'head', chest: 'spine_03', belly: 'spine_01', legs: 'thigh_l' };
const loader = new GLTFLoader(), texLoader = new THREE.TextureLoader(), skinCache = {}, V = () => new THREE.Vector3();
const t1 = V(), t2 = V(), t3 = V(), DOWN = [0, -1, 0], GET = ['getX', 'getY', 'getZ', 'getW'];

export async function loadAssets() {
  const gltf = await loader.loadAsync('assets/mannequin.glb'), clips = {};
  await Promise.all(CLIPS.map(async (n) => { try { const c = (await loader.loadAsync(`assets/anims/${n}.glb`)).animations[0]; if (!c) return;
    for (const tr of c.tracks) if (tr.name === 'pelvis.position') for (let i = 0; i < tr.values.length; i += 3) { tr.values[i] = 0; tr.values[i + 2] = 0; } // in place: the script moves the body
    clips[n] = c; } catch (e) { /* clip not made yet */ } }));
  return { gltf, clips, measured: null };
}
function skinMat(name) {
  if (skinCache[name]) return skinCache[name];
  const t = (f, srgb) => { const x = texLoader.load(`assets/skins/${name}/${f}.jpg`); x.flipY = false; if (srgb) x.colorSpace = THREE.SRGBColorSpace; return x; };
  return (skinCache[name] = new THREE.MeshStandardMaterial({ map: t('base_color', true), normalMap: t('normal'), metalnessMap: t('metallic'), roughnessMap: t('roughness'), metalness: 1, roughness: 1, envMapIntensity: 2.6 }));
}
// volume of the figure and the share of it each bone carries, from the skinned mesh itself
function measure(model, bones) {
  let mesh = null; model.traverse((o) => { if (o.isSkinnedMesh && !mesh) mesh = o; }); if (!mesh) return null;
  const g = mesh.geometry, P = g.attributes.position, SI = g.attributes.skinIndex, SW = g.attributes.skinWeight, ix = g.index, sk = mesh.skeleton.bones, nb = sk.length, dom = new Int16Array(P.count), cen = new Float64Array(nb * 4);
  for (let i = 0; i < P.count; i++) { let b = 0, w = -1; for (let k = 0; k < 4; k++) { const x = SW[GET[k]](i); if (x > w) { w = x; b = SI[GET[k]](i); } } dom[i] = b; cen[b * 4] += P.getX(i); cen[b * 4 + 1] += P.getY(i); cen[b * 4 + 2] += P.getZ(i); cen[b * 4 + 3]++; }
  const vol = new Float64Array(nb), n = ix ? ix.count : P.count, a = V(), b = V(), c = V(), o = V();
  for (let i = 0; i < n; i += 3) { const ia = ix ? ix.getX(i) : i, ib = ix ? ix.getX(i + 1) : i + 1, ic = ix ? ix.getX(i + 2) : i + 2, d = dom[ia], k = cen[d * 4 + 3] || 1;
    o.set(cen[d * 4] / k, cen[d * 4 + 1] / k, cen[d * 4 + 2] / k); a.fromBufferAttribute(P, ia).sub(o); b.fromBufferAttribute(P, ib).sub(o); c.fromBufferAttribute(P, ic).sub(o); vol[d] += a.dot(b.cross(c)) / 6; }
  g.computeBoundingBox(); const ext = g.boundingBox.getSize(V()), tall = Math.max(ext.x, ext.y, ext.z), top = bones.head_end || bones.head, toe = bones.ball_l;
  const height = top.getWorldPosition(t1).y - toe.getWorldPosition(t2).y + 0.06, k3 = (height / tall) ** 3; let total = 0; for (let i = 0; i < nb; i++) total += Math.abs(vol[i]);
  const volume = total * k3, frac = {}; if (!(volume > 0.03 && volume < 0.14)) return null;
  sk.forEach((bn, i) => { let nm = bn.name; while (!CHILD[nm] && bones[nm] && bones[nm].parent && bones[nm].parent.isBone) nm = bones[nm].parent.name; frac[nm] = (frac[nm] || 0) + Math.abs(vol[i]) / total; });
  return { volume, frac, height };
}

export function makeFighter(scene, assets, spec, side) {
  const root = new THREE.Group(), spin = new THREE.Group(), model = SkeletonUtils.clone(assets.gltf.scene);
  spin.position.y = 0.95; model.position.y = -0.95; spin.add(model); root.add(spin); scene.add(root);
  model.traverse((o) => { if (o.isMesh) { o.castShadow = true; o.frustumCulled = false; } });
  const ring = new THREE.Mesh(new THREE.RingGeometry(0.55, 0.72, 6), new THREE.MeshBasicMaterial({ color: spec.aura, transparent: true, opacity: 0.8, side: THREE.DoubleSide, depthWrite: false }));
  ring.rotation.x = -Math.PI / 2; scene.add(ring);
  const glow = new THREE.PointLight(spec.aura, 9, 7, 1.5); glow.position.y = 1.2; root.add(glow);
  const mixer = new THREE.AnimationMixer(model), actions = {}, bones = {};
  for (const [n, c] of Object.entries(assets.clips)) { const a = mixer.clipAction(c); a.play(); a.setEffectiveWeight(0); actions[n] = a; }
  model.traverse((o) => { if (o.isBone) bones[o.name] = o; }); root.updateMatrixWorld(true);
  if (!assets.measured) assets.measured = measure(model, bones) || { volume: FIGURE_VOLUME, frac: SEGMENTS, fallback: true };
  const segs = Object.entries(assets.measured.frac).filter(([n]) => bones[n] && bones[CHILD[n]]).map(([n, frac]) => ({ a: bones[n], b: bones[CHILD[n]], frac }));
  const tot = segs.reduce((s, x) => s + x.frac, 0); segs.forEach((s) => (s.frac /= tot));
  const f = { side, root, spin, model, ring, glow, mixer, actions, clips: assets.clips, spec, bones, segs, pos: V(), com: V(), ik: [], feet: [V(), V()], volume: assets.measured.volume };
  f.setSkin = (n) => { const m = skinMat(n); model.traverse((o) => { if (o.isMesh) o.material = m; }); f.skin = n; f.body = fighterBody(n, f.volume); };
  f.setSkin(spec.skin);
  f.comOf = (out) => { out.set(0, 0, 0); for (const s of f.segs) { s.a.getWorldPosition(t1); s.b.getWorldPosition(t2); out.addScaledVector(t1.add(t2), 0.5 * s.frac); } return out; };
  f.clean = Object.values(LIMBS).flatMap((l) => l.slice(0, 2)).map((n) => [bones[n], bones[n].quaternion.clone()]); // the clip's own pose of every bone IK may touch
  f.comRest = 0.95; return f;
}
const clipTime = (f, s, tau) => { const dur = f.clips[s.clip].duration, ct = (s.at || 0) / 1000 + (tau / 1000) * (s.speed || 1); return s.loop ? ct % dur : Math.min(ct, dur - 0.01); };
const clipOf = (f, s) => (f.clips[s.clip] ? s.clip : 'idle_guard');
const lerpAngle = (a, b, t) => a + (((b - a + Math.PI * 3) % (Math.PI * 2)) - Math.PI) * t;

// where each fighter is facing at time T, before smoothing: along its path, at its aim, or at the foe
function rawYaw(side, T, ctx) {
  const mv = movedAt(ctx.W, ctx.B, T), me = rootAt(ctx.R.tracks[side], T, mv), s = me.seg; let d = me.face;
  if (!d && s.aim && ctx.W.byId[s.aim]) { const c = ctx.W.byId[s.aim].com; d = [c[0] - me.p[0], 0, c[2] - me.p[2]]; }
  if (!d) { const fo = rootAt(ctx.R.tracks[side === 'a' ? 'b' : 'a'], T, mv); d = [fo.p[0] - me.p[0], 0, fo.p[2] - me.p[2]]; }
  return Math.hypot(d[0], d[2]) > 0.05 ? Math.atan2(d[0], d[2]) : null;
}
// phase 1: clip, root on the path, centre of mass, feet and parkour contacts
export function poseFighter(f, T, ctx) {
  const tr = ctx.R.tracks[f.side], st = rootAt(tr, T, ctx.moved), s = st.seg, tau = st.tau; f.st = st; f.seg = s; f.ik.length = 0;
  f.pos.set(st.p[0], st.p[1], st.p[2]); f.root.position.copy(f.pos);
  let yaw = rawYaw(f.side, T, ctx); if (yaw === null) yaw = 0;
  if (tau < 220 && st.i > 0) { const y0 = rawYaw(f.side, s.t0 - 1, ctx); if (y0 !== null) yaw = lerpAngle(y0, yaw, smooth(tau / 220)); }
  f.root.rotation.y = yaw; f.spin.rotation.set(st.pitch, 0, st.roll);
  // the clip (cross-faded from the previous segment's)
  for (const a of Object.values(f.actions)) a.setEffectiveWeight(0);
  const name = clipOf(f, s), cur = f.actions[name], prev = st.i > 0 ? tr[st.i - 1] : null; let w = 1;
  if (prev && clipOf(f, prev) !== name) { w = smooth(tau / 220); const pa = f.actions[clipOf(f, prev)]; pa.time = clipTime(f, { ...prev, clip: clipOf(f, prev) }, prev.ms); pa.setEffectiveWeight(1 - w); }
  cur.time = clipTime(f, { ...s, clip: name }, tau); cur.setEffectiveWeight(w);
  // the mixer only rewrites a bone when its own result changes, so last frame's IK must be undone first or it would leak into this frame
  for (const [b, q] of f.clean) b.quaternion.copy(q);
  f.mixer.update(0); for (const [b, q] of f.clean) q.copy(b.quaternion); f.root.updateMatrixWorld(true);
  // centre of mass: in the air the body turns about it and it rides the ballistic arc exactly
  f.comOf(f.com);
  if (st.aw > 0) { t3.set(st.p[0], st.p[1] + f.comRest, st.p[2]).sub(f.com).clampLength(0, 2.5).multiplyScalar(st.aw); f.root.position.add(t3); f.root.updateMatrixWorld(true); f.com.add(t3); }
  const planted = !st.air && !st.hung && Math.abs(st.pitch) < 0.3 && Math.abs(st.roll) < 0.3;
  if (planted) {
    // feet: find the real surface under each (a stair, a crate top, a tilted lid) and the animation's own lift above the floor
    const B = f.bones, tg = [], py = f.root.position.y; let drop = 0, need = false;
    for (const [i, nm] of ['foot_l', 'foot_r'].entries()) { const p = B[nm].getWorldPosition(f.feet[i]), lift = p.y - py, w = 1 - smooth((lift - 0.16) / 0.22);
      const h = castRay(ctx.live(), [p.x, py + 0.55, p.z], DOWN, 1.5), g = h ? py + 0.55 - h.t : Math.max(0, py - 0.95), dy = g - py < -0.45 ? 0 : g - py;
      tg.push({ nm, p: p.clone().setY(p.y + dy * w), w, n: h ? h.n : [0, 1, 0], dy }); if (Math.abs(dy) > 0.012 && w > 0.05) need = true; drop = Math.min(drop, dy * w); }
    // balance: keep the centre of mass over the line between the feet, by shifting the pelvis
    const a = f.feet[0], b = f.feet[1], ex = b.x - a.x, ez = b.z - a.z, el = ex * ex + ez * ez || 1e-6, u = clamp01(((f.com.x - a.x) * ex + (f.com.z - a.z) * ez) / el), dx = f.com.x - (a.x + ex * u), dz = f.com.z - (a.z + ez * u), d = Math.hypot(dx, dz);
    f.lean = 0; if (d > 0.14 && tg[0].w > 0.6 && tg[1].w > 0.6) { const k = Math.min(0.2, (d - 0.14) * 0.8) / d; f.root.position.x -= dx * k; f.root.position.z -= dz * k; f.com.x -= dx * k; f.com.z -= dz * k; f.lean = d; need = true; }
    if (need) { f.root.position.y += drop * 0.85; f.root.updateMatrixWorld(true);
      for (const t of tg) { if (t.w < 0.05) continue; twoBone(B[LIMBS[t.nm][0]], B[LIMBS[t.nm][1]], B[t.nm], t.p, t.w); f.ik.push(t.p); } }
  }
  for (const k of st.ik) { if (k.w <= 0.01) continue; const L = LIMBS[k.limb], p = t1.set(k.p[0], k.p[1], k.p[2]).clone(); twoBone(f.bones[L[0]], f.bones[L[1]], f.bones[L[2]], p, k.w); f.ik.push(p); }
  f.ring.position.set(f.pos.x, Math.max(0.03, st.air || st.hung ? 0.03 : f.pos.y + 0.03), f.pos.z); f.ring.rotation.z = f.root.rotation.y; f.ring.scale.setScalar(1 - 0.35 * clamp01((f.pos.y - f.ring.position.y) / 2));
}
// phase 2 (both fighters posed): on a strike, the hitting hand or foot goes to the script's contact point at t_ms
export function strikeIK(f, foe, T, ctx) {
  for (const c of ctx.R.strikes) { if (c.by !== f.side || !c.limb || c.ranged) continue; const dt = T / 1000 - c.t; if (dt < -0.2 || dt > 0.2) continue;
    let w = smooth((dt + 0.2) / 0.14) * (1 - smooth((dt - 0.03) / 0.15)); const L = LIMBS[c.limb]; if (!L || w <= 0.01) continue; let p = null;
    if (c.on && c.on !== 'foe') { const h = ctx.B.hits[c.id]; if (h) p = t1.set(h.at[0], h.at[1], h.at[2]).clone(); }
    else if (!c.miss) p = foe.bones[AIM[c.at] || 'spine_03'].getWorldPosition(V());
    if (!p) continue; const sh = f.bones[L[0]].getWorldPosition(t2), far = sh.distanceTo(p); w *= 1 - smooth((far - 0.95) / 0.7);
    if (w > 0.01) { twoBone(f.bones[L[0]], f.bones[L[1]], f.bones[L[2]], p, w); f.ik.push(p); } }
}

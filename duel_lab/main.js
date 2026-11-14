// Duel Lab: a scrubbable three.js playback of one duel script (duel.json).
// Everything on screen is a pure function of the timeline time T (ms), so rewind and replay are exact.
import * as THREE from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import * as SkeletonUtils from 'three/addons/utils/SkeletonUtils.js';
import { RoomEnvironment } from 'three/addons/environments/RoomEnvironment.js';

const Q = new URLSearchParams(location.search);
const $ = (id) => document.getElementById(id);
const SKINS = ['wood', 'obsidian', 'gold', 'dragonscale', 'butterfly'];
const CLIPS = ['idle_guard', 'walk', 'run', 'strike_side', 'strike_slash', 'lunge', 'heavy_charged', 'heavy_hammer', 'jump_attack', 'combo_triple',
  'parry_high', 'parry_back', 'block', 'block_cross', 'shield_push', 'evade_stand', 'evade_roll', 'evade_spin', 'counter', 'kick', 'throw_shoulder',
  'grapple_throw_down', 'stagger_back', 'stagger_waist', 'airborne_hit', 'downed', 'rise', 'defeat', 'invoke', 'command', 'ground_slam', 'taunt',
  'victory', 'bow', 'kneel'];
const S = 1.0; // hex size in metres
const hex = ([q, r]) => new THREE.Vector3(S * Math.sqrt(3) * (q + r / 2), 0, S * 1.5 * r);
const clamp01 = (x) => Math.min(1, Math.max(0, x));
const smooth = (x) => { x = clamp01(x); return x * x * (3 - 2 * x); };

// ---------- renderer, scene, lights ----------
const renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
renderer.setSize(innerWidth, innerHeight);
renderer.shadowMap.enabled = true; renderer.shadowMap.type = THREE.PCFSoftShadowMap;
renderer.toneMapping = THREE.ACESFilmicToneMapping; renderer.toneMappingExposure = 1.0;
document.body.prepend(renderer.domElement);
const scene = new THREE.Scene(); scene.background = new THREE.Color('#07060a'); scene.fog = new THREE.Fog('#07060a', 30, 70);
scene.environment = new THREE.PMREMGenerator(renderer).fromScene(new RoomEnvironment(), 0.04).texture;
const camera = new THREE.PerspectiveCamera(38, innerWidth / innerHeight, 0.1, 200);
addEventListener('resize', () => { renderer.setSize(innerWidth, innerHeight); camera.aspect = innerWidth / innerHeight; camera.updateProjectionMatrix(); });

scene.add(new THREE.HemisphereLight('#5a6a9a', '#1a0f08', 0.22));
const key = new THREE.SpotLight('#ffd9a0', 260, 60, 0.6, 0.6, 1.6); key.position.set(-4, 18, 10); key.castShadow = true;
key.shadow.mapSize.set(2048, 2048); key.shadow.bias = -0.0004; scene.add(key, key.target);
const rim = new THREE.DirectionalLight('#5aa0ff', 1.3); rim.position.set(6, 8, -12); scene.add(rim);
const candles = [];

// ---------- the table and the box ----------
const wood = (c, r = 0.8) => new THREE.MeshStandardMaterial({ color: c, roughness: r, metalness: 0.05 });
const table = new THREE.Mesh(new THREE.PlaneGeometry(140, 140), wood('#1c120b', 0.9)); table.rotation.x = -Math.PI / 2; table.position.y = -0.62; table.receiveShadow = true; scene.add(table);
const BW = 21, BD = 17, BH = 0.6;
const base = new THREE.Mesh(new THREE.BoxGeometry(BW, BH, BD), wood('#3a2415')); base.position.y = -BH / 2 - 0.02; base.receiveShadow = true; base.castShadow = true; scene.add(base);
for (const [x, z, w, d] of [[0, BD / 2, BW, 0.35], [0, -BD / 2, BW, 0.35], [BW / 2, 0, 0.35, BD], [-BW / 2, 0, 0.35, BD]]) {
  const rail = new THREE.Mesh(new THREE.BoxGeometry(w + 0.35, 0.9, d), wood('#2a180d')); rail.position.set(x, -0.1, z); rail.castShadow = true; scene.add(rail);
}
for (const [x, z] of [[1, 1], [1, -1], [-1, 1], [-1, -1]]) {
  const c = new THREE.Mesh(new THREE.BoxGeometry(0.9, 1.0, 0.9), new THREE.MeshStandardMaterial({ color: '#b8893a', metalness: 1, roughness: 0.35 }));
  c.position.set(x * BW / 2, -0.1, z * BD / 2); scene.add(c);
}
// hex stage
const tileG = new THREE.CylinderGeometry(S * 0.97, S * 0.97, 0.3, 6);
const tiles = new THREE.Group(); scene.add(tiles);
let seed = 7; const rnd = () => (seed = (seed * 16807) % 2147483647) / 2147483647;
for (let q = -6; q <= 6; q++) for (let r = -5; r <= 5; r++) {
  const p = hex([q, r]); if (Math.abs(p.x) > BW / 2 - 1 || Math.abs(p.z) > BD / 2 - 1) continue;
  const m = new THREE.Mesh(tileG, wood(new THREE.Color().setHSL(0.07, 0.32, 0.085 + rnd() * 0.04), 0.85));
  m.position.set(p.x, -0.15 + rnd() * 0.05, p.z); m.receiveShadow = true; tiles.add(m);
}
// the lid: a painted sky, the CLASH arch and two plaques, drawn on a canvas
const lid = new THREE.Group(); lid.position.set(0, 0.3, -BD / 2 - 0.2); scene.add(lid);
const LH = 12;
const lidCv = document.createElement('canvas'); lidCv.width = 2048; lidCv.height = 1170;
const lidTex = new THREE.CanvasTexture(lidCv); lidTex.colorSpace = THREE.SRGBColorSpace;
const lidBack = new THREE.Mesh(new THREE.BoxGeometry(BW + 0.35, LH, 0.4), wood('#2a180d')); lidBack.position.set(0, LH / 2, -0.22); lidBack.castShadow = true;
const lidFace = new THREE.Mesh(new THREE.PlaneGeometry(BW - 0.4, LH - 0.4), new THREE.MeshStandardMaterial({ map: lidTex, roughness: 0.9, emissive: '#ffffff', emissiveMap: lidTex, emissiveIntensity: 0.22 }));
lidFace.position.set(0, LH / 2, 0);
lid.add(lidBack, lidFace);
function paintLid(d) {
  const g = lidCv.getContext('2d'), W = lidCv.width, H = lidCv.height;
  const night = d.time_of_day === 'night', dusk = d.time_of_day === 'dusk' || d.time_of_day === 'dawn';
  const sky = g.createLinearGradient(0, 0, 0, H);
  if (night) { sky.addColorStop(0, '#05060f'); sky.addColorStop(0.6, '#141a3a'); sky.addColorStop(1, '#2a2440'); }
  else if (dusk) { sky.addColorStop(0, '#231335'); sky.addColorStop(0.6, '#a04a2a'); sky.addColorStop(1, '#e0a050'); }
  else { sky.addColorStop(0, '#3b6ea8'); sky.addColorStop(1, '#c8d8e8'); }
  g.fillStyle = sky; g.fillRect(0, 0, W, H);
  let s = 11; const r = () => (s = (s * 16807) % 2147483647) / 2147483647;
  if (night) { g.fillStyle = '#fff'; for (let i = 0; i < 260; i++) { g.globalAlpha = r() * 0.8; g.fillRect(r() * W, r() * H * 0.7, 2, 2); } g.globalAlpha = 1; g.fillStyle = '#e8e4c8'; g.beginPath(); g.arc(W * 0.78, H * 0.36, 60, 0, 7); g.fill(); }
  g.strokeStyle = '#b8c8ff'; g.lineWidth = 4; g.shadowColor = '#8ab0ff'; g.shadowBlur = 30;
  for (const x0 of [W * 0.3, W * 0.62]) { g.beginPath(); let x = x0, y = H * 0.22; g.moveTo(x, y); while (y < H * 0.8) { x += (r() - 0.5) * 120; y += 40 + r() * 50; g.lineTo(x, y); } g.stroke(); }
  g.shadowBlur = 0;
  g.fillStyle = '#0a0a12'; g.beginPath(); g.moveTo(0, H); for (let x = 0; x <= W; x += 64) g.lineTo(x, H * 0.84 - Math.abs(Math.sin(x * 0.011)) * 110 - r() * 50); g.lineTo(W, H); g.fill();
  // the arch
  g.fillStyle = '#3a2415'; g.fillRect(0, 0, W, 190);
  const gold = g.createLinearGradient(0, 30, 0, 170); gold.addColorStop(0, '#f6d98a'); gold.addColorStop(0.5, '#c8963a'); gold.addColorStop(1, '#7a5418');
  g.strokeStyle = gold; g.lineWidth = 10; g.strokeRect(W / 2 - 430, 22, 860, 146); g.fillStyle = '#1e120a'; g.fillRect(W / 2 - 425, 27, 850, 136);
  g.fillStyle = gold; g.font = '900 128px Georgia'; g.textAlign = 'center'; g.textBaseline = 'middle'; g.fillText('C L A S H', W / 2, 100);
  // the plaques
  for (const [f, x] of [[d.a, 190], [d.b, W - 190]]) {
    g.fillStyle = '#2a180d'; g.fillRect(x - 150, 230, 300, 420); g.strokeStyle = gold; g.lineWidth = 8; g.strokeRect(x - 150, 230, 300, 420);
    const a = g.createRadialGradient(x, 400, 10, x, 400, 170); a.addColorStop(0, f.aura); a.addColorStop(1, '#0000'); g.fillStyle = a; g.fillRect(x - 142, 238, 284, 404);
    g.fillStyle = '#0c0a10'; g.beginPath(); g.arc(x, 380, 62, 0, 7); g.fill(); g.fillRect(x - 70, 440, 140, 150);
    g.fillStyle = '#e9dcbd'; g.font = '700 26px Georgia'; g.fillText(f.name, x, 618, 280);
  }
  lidTex.needsUpdate = true;
}
function candle(x, z) {
  const c = new THREE.Mesh(new THREE.CylinderGeometry(0.5, 0.55, 3, 12), new THREE.MeshStandardMaterial({ color: '#e8d8b0', emissive: '#ff9a3a', emissiveIntensity: 0.25 }));
  c.position.set(x, 0.9, z); scene.add(c);
  const l = new THREE.PointLight('#ff9a3a', 60, 40, 1.8); l.position.set(x, 3, z); scene.add(l); candles.push(l);
  const f = new THREE.Mesh(new THREE.SphereGeometry(0.22, 8, 8), new THREE.MeshBasicMaterial({ color: '#ffd27a' })); f.scale.y = 1.8; f.position.set(x, 2.75, z); scene.add(f);
}
candle(-BW / 2 - 3.5, 4); candle(BW / 2 + 3.5, 2);

// set dressing from the script: platonic solids and basic shapes
function buildSet(list) {
  const g = new THREE.Group(); scene.add(g);
  for (const s of list || []) {
    const p = hex(s.at), m = new THREE.MeshStandardMaterial({ color: s.tint || '#666', roughness: 0.85, flatShading: true }); let o;
    const z = s.size || 1;
    if (s.shape === 'pillar') { o = new THREE.Mesh(new THREE.CylinderGeometry(0.5, 0.6, z * 3, 6), m); o.position.y = z * 1.5; }
    else if (s.shape === 'box') { o = new THREE.Mesh(new THREE.BoxGeometry(z * 1.5, z * 1.2, z * 1.5), m); o.position.y = z * 0.6; o.rotation.y = 0.3; }
    else if (s.shape === 'cone') { o = new THREE.Group(); const t = new THREE.Mesh(new THREE.CylinderGeometry(0.12, 0.16, 0.6, 5), wood('#3a2415')); t.position.y = 0.3;
      const c = new THREE.Mesh(new THREE.ConeGeometry(z * 0.75, z * 2.2, 6), m); c.position.y = 0.6 + z * 1.1; o.add(t, c); }
    else { const G = { ico: THREE.IcosahedronGeometry, octa: THREE.OctahedronGeometry, tetra: THREE.TetrahedronGeometry }[s.shape] || THREE.DodecahedronGeometry;
      o = new THREE.Mesh(new G(z, 0), m); o.position.y = z * 0.8; o.rotation.set(0.4, 0.7, 0.2); }
    o.traverse((x) => { x.castShadow = true; x.receiveShadow = true; });
    const w = new THREE.Group(); w.position.copy(p); w.add(o); g.add(w);
  }
}

// ---------- fighters ----------
const loader = new GLTFLoader(), texLoader = new THREE.TextureLoader();
const skinCache = {};
function skinMat(name) {
  if (skinCache[name]) return skinCache[name];
  const t = (f, srgb) => { const x = texLoader.load(`assets/skins/${name}/${f}.jpg`); x.flipY = false; if (srgb) x.colorSpace = THREE.SRGBColorSpace; return x; };
  return (skinCache[name] = new THREE.MeshStandardMaterial({ map: t('base_color', true), normalMap: t('normal'), metalnessMap: t('metallic'), roughnessMap: t('roughness'), metalness: 1, roughness: 1, envMapIntensity: 1.6 }));
}
function makeFighter(gltf, clips, spec) {
  const root = new THREE.Group(), spin = new THREE.Group(), model = SkeletonUtils.clone(gltf.scene);
  spin.position.y = 0.95; model.position.y = -0.95; spin.add(model); root.add(spin); scene.add(root);
  model.traverse((o) => { if (o.isMesh) { o.castShadow = true; o.frustumCulled = false; } });
  const ring = new THREE.Mesh(new THREE.RingGeometry(0.55, 0.72, 6), new THREE.MeshBasicMaterial({ color: spec.aura, transparent: true, opacity: 0.85, side: THREE.DoubleSide }));
  ring.rotation.x = -Math.PI / 2; ring.position.y = 0.03; root.add(ring);
  const glow = new THREE.PointLight(spec.aura, 6, 6, 1.5); glow.position.y = 1.2; root.add(glow);
  const mixer = new THREE.AnimationMixer(model), actions = {};
  for (const [n, c] of Object.entries(clips)) { const a = mixer.clipAction(c); a.play(); a.setEffectiveWeight(0); actions[n] = a; }
  const f = { root, spin, model, ring, mixer, actions, clips, spec, track: [], pos: new THREE.Vector3() };
  f.setSkin = (n) => { const m = skinMat(n); model.traverse((o) => { if (o.isMesh) o.material = m; }); f.skin = n; };
  f.setSkin(spec.skin);
  return f;
}
// one fighter's whole duel as a flat list of segments on the global timeline
function buildTrack(f, side, d) {
  let t = 0, at = hex(f.spec.start); const tr = [];
  const push = (s, ms) => { const to = s.to ? hex(s.to) : at.clone(); tr.push({ ...s, clip: f.clips[s.clip] ? s.clip : 'idle_guard', t0: t, ms, from: at.clone(), to }); at = to; t += ms; };
  push({ clip: 'idle_guard', loop: true }, d.intro_ms);
  for (const p of d.panels) {
    let used = 0; const segs = p[side].segs;
    segs.forEach((s, i) => { const ms = i === segs.length - 1 ? Math.max(s.ms, p.play_ms - used) : s.ms; push(s, ms); used += ms; });
  }
  const last = tr[tr.length - 1]; push({ clip: last.clip, at: 99999 }, d.outro_ms);
  f.track = tr;
}
const clipTime = (f, s, tau) => { const dur = f.clips[s.clip].duration, ct = (s.at || 0) / 1000 + (tau / 1000) * (s.speed || 1); return s.loop ? ct % dur : Math.min(ct, dur - 0.01); };
function segAt(f, T) { const tr = f.track; let i = tr.length - 1; for (let k = 0; k < tr.length; k++) if (T < tr[k].t0 + tr[k].ms) { i = k; break; } return i; }
function placeFighter(f, T) {
  const i = segAt(f, T), s = f.track[i], tau = Math.max(0, T - s.t0), u = clamp01(tau / s.ms);
  const air = s.path === 'leap' || s.path === 'flip' || s.path === 'knock';
  f.pos.lerpVectors(s.from, s.to, air ? u : smooth(u)); f.pos.y = air ? (s.h || 1.5) * 4 * u * (1 - u) : 0;
  f.spin.rotation.x = s.path === 'flip' ? Math.PI * 2 * u : 0;
  f.seg = s; f.u = u;
  for (const a of Object.values(f.actions)) a.setEffectiveWeight(0);
  const cur = f.actions[s.clip], prev = i > 0 ? f.track[i - 1] : null;
  let w = 1;
  if (prev && prev.clip !== s.clip) { w = smooth(tau / 220); const pa = f.actions[prev.clip]; pa.time = clipTime(f, prev, prev.ms); pa.setEffectiveWeight(1 - w); }
  cur.time = clipTime(f, s, tau); cur.setEffectiveWeight(w);
  f.mixer.update(0);
}
function faceFighter(f, foe) {
  const s = f.seg; let dir;
  if (s.clip === 'run' && s.from.distanceToSquared(s.to) > 0.01) dir = s.to.clone().sub(s.from);
  else dir = foe.pos.clone().sub(f.pos);
  f.root.position.copy(f.pos); f.ring.position.y = 0.03 - f.pos.y; f.ring.scale.setScalar(1 - 0.35 * clamp01(f.pos.y / 2));
  f.root.rotation.y = Math.atan2(dir.x, dir.z);
}

// ---------- camera ----------
function shot(name, A, B) {
  const m = A.clone().add(B).multiplyScalar(0.5); m.y = 0;
  const d = B.clone().sub(A); d.y = 0; if (d.lengthSq() < 0.01) d.set(1, 0, 0); d.normalize();
  const n = new THREE.Vector3(-d.z, 0, d.x); if (n.z < 0) n.negate();
  const up = (y) => new THREE.Vector3(0, y, 0), a = A.clone().setY(0), b = B.clone().setY(0);
  const span = Math.max(4, A.distanceTo(B));
  switch (name) {
    case 'side': return [m.clone().addScaledVector(n, 3.6 + span * 0.8).add(up(2.0)), m.clone().add(up(1.1))];
    case 'low': return [m.clone().addScaledVector(n, 3.2 + span * 0.7).add(up(0.5)), m.clone().add(up(1.3))];
    case 'over_a': return [a.clone().addScaledVector(d, -4).addScaledVector(n, 1.8).add(up(2.5)), b.clone().add(up(1.1))];
    case 'over_b': return [b.clone().addScaledVector(d, 4).addScaledVector(n, 1.8).add(up(2.5)), a.clone().add(up(1.1))];
    case 'close_a': return [a.clone().addScaledVector(n, 4.2).addScaledVector(d, 0.8).add(up(1.6)), a.clone().add(up(1.15))];
    case 'close_b': return [b.clone().addScaledVector(n, 4.2).addScaledVector(d, -0.8).add(up(1.6)), b.clone().add(up(1.15))];
    case 'box': return [new THREE.Vector3(0, 13, 33), new THREE.Vector3(0, 3.5, -2)];
    default: return [new THREE.Vector3(m.x * 0.6, 5.2, m.z + 6 + span * 0.9), new THREE.Vector3(m.x * 0.8, 1.3, m.z)];
  }
}

// ---------- the timeline ----------
let D, FA, FB, T = +(Q.get('t') || 0), playing = !Q.has('paused'), speed = 1, TOTAL = 0, PAN = [];
function panelAt(t) { for (let i = 0; i < PAN.length; i++) if (t < PAN[i].t0 + PAN[i].play_ms) return i; return PAN.length - 1; }
const mask = (s, n, live) => { n = Math.floor(n); if (n <= 0) return ''; const shown = s.slice(0, n); return live ? '*'.repeat(n - 1) + shown.slice(-1) : '*'.repeat(shown.length); };

function frame(T) {
  placeFighter(FA, T); placeFighter(FB, T); faceFighter(FA, FB); faceFighter(FB, FA);
  // the box opens at the start and shuts at the end
  const open = Math.min(smooth(T / (D.intro_ms * 0.8)), smooth((TOTAL - T) / (D.outro_ms * 0.7)));
  lid.rotation.x = THREE.MathUtils.lerp(Math.PI / 2 - 0.02, -0.1, open);
  const inDuel = T >= D.intro_ms, pi = inDuel ? panelAt(T) : -1, p = pi >= 0 ? PAN[pi] : null, tau = p ? T - p.t0 : 0;
  // camera: this panel's shot, eased in from the previous one
  const cur = shot(p ? (Q.get('shot') || p.shot) : 'box', FA.pos, FB.pos), prev = shot(pi > 0 ? PAN[pi - 1].shot : 'box', FA.pos, FB.pos);
  const k = p ? smooth(tau / 550) : 1, cp = prev[0].lerp(cur[0], k), cl = prev[1].lerp(cur[1], k);
  let shake = 0, flash = 0, burst = null, flipA = 0;
  if (p) {
    for (const c of p.contacts || []) { const dt = tau - c.t_ms; if (dt >= 0 && dt < 600) { if (!c.miss) { shake = Math.max(shake, Math.exp(-dt / 130)); flash = Math.max(flash, Math.exp(-dt / 70) * 0.75); } if (dt < 520) burst = [c.text, dt, c.by]; } }
    if (p.flip) { flipA = tau < 900 ? Math.sin((tau / 900) * Math.PI) : 0; flash = Math.max(flash, Math.exp(-tau / 90) * 0.5); }
  }
  cp.x += Math.sin(T * 0.09) * shake * 0.28; cp.y += Math.cos(T * 0.13) * shake * 0.22;
  camera.position.copy(cp); camera.lookAt(cl);
  key.target.position.set((FA.pos.x + FB.pos.x) / 2, 0, (FA.pos.z + FB.pos.z) / 2);
  candles.forEach((l, i) => { l.intensity = 60 * (0.85 + 0.15 * Math.sin(T * 0.013 + i * 2) * Math.sin(T * 0.031 + i)); });
  // overlays
  $('flash').style.opacity = flash; $('flip').style.opacity = flipA;
  const b = $('burst');
  if (burst) { const s = 0.5 + 0.9 * (1 - Math.exp(-burst[1] / 60)); b.textContent = burst[0] + '!'; b.style.transform = `translate(-50%,-50%) scale(${s}) rotate(${burst[2] === 'a' ? -8 : 7}deg)`; b.style.opacity = 1 - smooth((burst[1] - 320) / 200); }
  else b.style.opacity = 0;
  $('caption').style.opacity = p ? 1 : 0; $('tray').style.opacity = p ? 1 : 0;
  let hpa = D.a.hp, hpb = D.b.hp;
  if (p) {
    $('pn').textContent = `PANEL ${pi + 1} OF ${PAN.length} · ${D.place.toUpperCase()}`;
    $('ct').textContent = p.caption.slice(0, Math.ceil((tau / 900) * p.caption.length));
    const before = pi > 0 ? PAN[pi - 1].hp : { a: D.a.hp, b: D.b.hp }, hit = (p.contacts || []).find((c) => !c.miss), ht = hit ? hit.t_ms : 0, h = smooth((tau - ht) / 300);
    hpa = before.a + (p.hp.a - before.a) * h; hpb = before.b + (p.hp.b - before.b) * h;
    const live = p.input_ms > 0 && tau < p.input_ms, frac = p.input_ms > 0 ? clamp01(tau / (p.input_ms * 0.92)) : 1;
    $('lA').textContent = p.a.line ? mask(p.a.line, frac * p.a.line.length, live) + (live ? '▌' : '  ⏎') : '';
    $('lB').textContent = p.b.line ? mask(p.b.line, frac * p.b.line.length, live) + (live ? '▌' : '  ⏎') : '';
    $('win').firstElementChild.style.width = clamp01(tau / p.play_ms) * 100 + '%';
    $('win').lastElementChild.style.left = (p.input_ms / p.play_ms) * 100 + '%';
    $('wl').textContent = p.input_ms === 0 ? 'THE END' : live ? 'YOUR WINDOW' : 'LOCKED · NEXT PANEL RENDERING';
    $('wr').textContent = live ? ((p.input_ms - tau) / 1000).toFixed(1) + ' s' : '';
  }
  $('hpA').querySelector('i').style.width = (hpa / D.a.hp) * 100 + '%'; $('hpB').querySelector('i').style.width = (hpb / D.b.hp) * 100 + '%';
  $('scrub').value = (T / TOTAL) * 1000; $('time').textContent = (T / 1000).toFixed(1) + ' / ' + (TOTAL / 1000).toFixed(1) + ' s';
  renderer.render(scene, camera);
}

// ---------- load and run ----------
async function main() {
  D = await (await fetch(Q.get('duel') || 'duel.json')).json();
  if (Q.get('a')) D.a.skin = Q.get('a'); if (Q.get('b')) D.b.skin = Q.get('b');
  const gltf = await loader.loadAsync('assets/mannequin.glb');
  const clips = {};
  await Promise.all(CLIPS.map(async (n) => { try {
    const c = (await loader.loadAsync(`assets/anims/${n}.glb`)).animations[0]; if (!c) return;
    for (const tr of c.tracks) if (tr.name === 'pelvis.position') for (let i = 0; i < tr.values.length; i += 3) { tr.values[i] = 0; tr.values[i + 2] = 0; } // in place: the script moves the body
    clips[n] = c; } catch (e) { /* clip not made yet */ } }));
  FA = makeFighter(gltf, clips, D.a); FB = makeFighter(gltf, clips, D.b);
  buildTrack(FA, 'a', D); buildTrack(FB, 'b', D); buildSet(D.set); paintLid(D);
  let t = D.intro_ms; PAN = D.panels.map((p) => { const o = { ...p, t0: t }; t += p.play_ms; return o; }); TOTAL = t + D.outro_ms;
  $('nA').textContent = D.a.name; $('nB').textContent = D.b.name; $('wA').textContent = D.a.name.toUpperCase(); $('wB').textContent = D.b.name.toUpperCase();
  document.documentElement.style.setProperty('--a', D.a.aura); document.documentElement.style.setProperty('--b', D.b.aura);
  for (const [id, f] of [['skA', FA], ['skB', FB]]) { const s = $(id); SKINS.forEach((n) => s.add(new Option(n, n, false, n === f.skin))); s.onchange = () => f.setSkin(s.value); }
  $('ticks').innerHTML = PAN.map((p) => `<i style="left:${(p.t0 / TOTAL) * 100}%"></i>`).join('');
  $('play').onclick = () => { playing = !playing; if (playing && T >= TOTAL) T = 0; };
  $('replay').onclick = () => { T = 0; playing = true; };
  $('speed').onchange = (e) => (speed = +e.target.value);
  $('scrub').oninput = (e) => { T = (e.target.value / 1000) * TOTAL; playing = false; };
  addEventListener('keydown', (e) => { if (e.code === 'Space') { e.preventDefault(); $('play').click(); } if (e.key === 'r') $('replay').click();
    if (e.key === 'ArrowLeft') { T = Math.max(0, T - 100); playing = false; } if (e.key === 'ArrowRight') { T = Math.min(TOTAL, T + 100); playing = false; } });
  $('load').remove(); window.READY = true; window.__setT = (t) => { T = t; playing = false; }; document.title = 'Duel Lab ready';
  let last = performance.now();
  renderer.setAnimationLoop((now) => { const dt = now - last; last = now; if (playing) { T += dt * speed; if (T >= TOTAL) { T = TOTAL; playing = false; } }
    $('play').textContent = playing ? 'Pause' : 'Play'; frame(Math.min(T, TOTAL - 1)); });
}
main().catch((e) => { $('load').textContent = 'FAILED: ' + e.message; console.error(e); });

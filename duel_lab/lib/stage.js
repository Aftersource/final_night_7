// The table, the wooden box, its painted lid, the dark hex floor and the candle-lit mood.
import * as THREE from 'three';
import { RoomEnvironment } from 'three/addons/environments/RoomEnvironment.js';
import { hexToWorld, HEX, rng } from './m.js';

export const BW = 21, BD = 17, BH = 0.6, LH = 12;
export function makeStage() {
  const renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
  renderer.setPixelRatio(Math.min(devicePixelRatio, 2)); renderer.setSize(innerWidth, innerHeight);
  renderer.shadowMap.enabled = true; renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  renderer.toneMapping = THREE.ACESFilmicToneMapping; renderer.toneMappingExposure = 0.85;
  document.body.prepend(renderer.domElement);
  const scene = new THREE.Scene(), NIGHT = '#06050a'; scene.background = new THREE.Color(NIGHT); scene.fog = new THREE.FogExp2('#0b0a12', 0.016);
  scene.environment = new THREE.PMREMGenerator(renderer).fromScene(new RoomEnvironment(), 0.04).texture; scene.environmentIntensity = 0.22;
  const camera = new THREE.PerspectiveCamera(38, innerWidth / innerHeight, 0.1, 200);
  addEventListener('resize', () => { renderer.setSize(innerWidth, innerHeight); camera.aspect = innerWidth / innerHeight; camera.updateProjectionMatrix(); });

  // light: one warm candle key that throws the shadows, a cold moon rim from behind, a breath of fill
  scene.add(new THREE.HemisphereLight('#2c3a66', '#0a0604', 0.3));
  const key = new THREE.SpotLight('#ff9d4d', 150, 70, 0.62, 0.75, 1.5); key.position.set(-7, 15, 13); key.castShadow = true;
  key.shadow.mapSize.set(2048, 2048); key.shadow.bias = -0.0004; key.shadow.camera.near = 4; key.shadow.camera.far = 50; scene.add(key, key.target);
  const rim = new THREE.DirectionalLight('#5b8cff', 0.9); rim.position.set(7, 9, -13); scene.add(rim);
  const candles = [];

  const wood = (c, r = 0.8) => new THREE.MeshStandardMaterial({ color: c, roughness: r, metalness: 0.05 });
  const table = new THREE.Mesh(new THREE.PlaneGeometry(140, 140), wood('#150d08', 0.92)); table.rotation.x = -Math.PI / 2; table.position.y = -0.62; table.receiveShadow = true; scene.add(table);
  const base = new THREE.Mesh(new THREE.BoxGeometry(BW, BH, BD), wood('#2a1a10')); base.position.y = -BH / 2 - 0.02; base.receiveShadow = true; base.castShadow = true; scene.add(base);
  for (const [x, z, w, d] of [[0, BD / 2, BW, 0.35], [0, -BD / 2, BW, 0.35], [BW / 2, 0, 0.35, BD], [-BW / 2, 0, 0.35, BD]]) {
    const rail = new THREE.Mesh(new THREE.BoxGeometry(w + 0.35, 0.9, d), wood('#20130a')); rail.position.set(x, -0.1, z); rail.castShadow = true; scene.add(rail); }
  for (const [x, z] of [[1, 1], [1, -1], [-1, 1], [-1, -1]]) {
    const c = new THREE.Mesh(new THREE.BoxGeometry(0.9, 1.0, 0.9), new THREE.MeshStandardMaterial({ color: '#a0762e', metalness: 1, roughness: 0.4 })); c.position.set(x * BW / 2, -0.1, z * BD / 2); scene.add(c); }
  // the hex floor: dark stained boards, each tile a slightly different stain, a little sheen so the candle reads on it
  const tileG = new THREE.CylinderGeometry(HEX * 0.975, HEX * 0.975, 0.3, 6), rnd = rng(7), tiles = [];
  for (let q = -7; q <= 7; q++) for (let r = -5; r <= 5; r++) { const p = hexToWorld([q, r]); if (Math.abs(p[0]) > BW / 2 - 1 || Math.abs(p[2]) > BD / 2 - 1) continue; tiles.push([p, rnd(), rnd()]); }
  const floor = new THREE.InstancedMesh(tileG, new THREE.MeshStandardMaterial({ roughness: 0.78, metalness: 0.05 }), tiles.length), M = new THREE.Matrix4(), C = new THREE.Color();
  tiles.forEach(([p, a, b], i) => { M.makeTranslation(p[0], -0.152 - a * 0.012, p[2]); floor.setMatrixAt(i, M); floor.setColorAt(i, C.setHSL(0.05 + b * 0.03, 0.28, 0.022 + a * 0.02)); });
  floor.receiveShadow = true; scene.add(floor);
  // the lid: a painted sky, the CLASH arch and two plaques, drawn on a canvas
  const lid = new THREE.Group(); lid.position.set(0, 0.3, -BD / 2 - 0.2); scene.add(lid);
  const cv = document.createElement('canvas'); cv.width = 2048; cv.height = 1170; const tex = new THREE.CanvasTexture(cv); tex.colorSpace = THREE.SRGBColorSpace;
  const back = new THREE.Mesh(new THREE.BoxGeometry(BW + 0.35, LH, 0.4), wood('#20130a')); back.position.set(0, LH / 2, -0.22); back.castShadow = true;
  const face = new THREE.Mesh(new THREE.PlaneGeometry(BW - 0.4, LH - 0.4), new THREE.MeshStandardMaterial({ map: tex, roughness: 0.9, emissive: '#ffffff', emissiveMap: tex, emissiveIntensity: 0.14 })); face.position.set(0, LH / 2, 0);
  lid.add(back, face);
  function paintLid(d) {
    const g = cv.getContext('2d'), W = cv.width, H = cv.height, night = d.time_of_day === 'night', dusk = d.time_of_day === 'dusk' || d.time_of_day === 'dawn';
    const sky = g.createLinearGradient(0, 0, 0, H);
    if (night) { sky.addColorStop(0, '#04050c'); sky.addColorStop(0.6, '#10152f'); sky.addColorStop(1, '#241d35'); }
    else if (dusk) { sky.addColorStop(0, '#231335'); sky.addColorStop(0.6, '#a04a2a'); sky.addColorStop(1, '#e0a050'); } else { sky.addColorStop(0, '#3b6ea8'); sky.addColorStop(1, '#c8d8e8'); }
    g.fillStyle = sky; g.fillRect(0, 0, W, H); const r = rng(11);
    if (night) { g.fillStyle = '#fff'; for (let i = 0; i < 260; i++) { g.globalAlpha = r() * 0.8; g.fillRect(r() * W, r() * H * 0.7, 2, 2); } g.globalAlpha = 1; g.fillStyle = '#d8d4b8'; g.beginPath(); g.arc(W * 0.78, H * 0.36, 60, 0, 7); g.fill(); }
    g.fillStyle = '#07070d'; g.beginPath(); g.moveTo(0, H); for (let x = 0; x <= W; x += 64) g.lineTo(x, H * 0.84 - Math.abs(Math.sin(x * 0.011)) * 110 - r() * 50); g.lineTo(W, H); g.fill();
    g.strokeStyle = '#07070d'; g.lineWidth = 5; for (let i = 0; i < 9; i++) { const x = W * (0.08 + r() * 0.84), h = 120 + r() * 220; g.beginPath(); g.moveTo(x, H * 0.84); g.lineTo(x, H * 0.84 - h); g.moveTo(x - 50, H * 0.84 - h * 0.7); g.lineTo(x + 50, H * 0.84 - h * 0.7); g.stroke(); } // masts
    g.fillStyle = '#2a1a10'; g.fillRect(0, 0, W, 190);
    const gold = g.createLinearGradient(0, 30, 0, 170); gold.addColorStop(0, '#e6c97a'); gold.addColorStop(0.5, '#b8862a'); gold.addColorStop(1, '#6a4410');
    g.strokeStyle = gold; g.lineWidth = 10; g.strokeRect(W / 2 - 430, 22, 860, 146); g.fillStyle = '#160d07'; g.fillRect(W / 2 - 425, 27, 850, 136);
    g.fillStyle = gold; g.font = '900 128px Georgia'; g.textAlign = 'center'; g.textBaseline = 'middle'; g.fillText('C L A S H', W / 2, 100);
    for (const [f, x] of [[d.a, 190], [d.b, W - 190]]) {
      g.fillStyle = '#20130a'; g.fillRect(x - 150, 230, 300, 420); g.strokeStyle = gold; g.lineWidth = 8; g.strokeRect(x - 150, 230, 300, 420);
      const a = g.createRadialGradient(x, 400, 10, x, 400, 170); a.addColorStop(0, f.aura); a.addColorStop(1, '#0000'); g.fillStyle = a; g.fillRect(x - 142, 238, 284, 404);
      g.fillStyle = '#0c0a10'; g.beginPath(); g.arc(x, 380, 62, 0, 7); g.fill(); g.fillRect(x - 70, 440, 140, 150);
      g.fillStyle = '#e9dcbd'; g.font = '700 26px Georgia'; g.fillText(f.name, x, 618, 280); }
    tex.needsUpdate = true;
  }
  function candle(x, z) {
    const c = new THREE.Mesh(new THREE.CylinderGeometry(0.5, 0.55, 3, 12), new THREE.MeshStandardMaterial({ color: '#d8c8a0', emissive: '#ff8a2a', emissiveIntensity: 0.22 })); c.position.set(x, 0.9, z); scene.add(c);
    const l = new THREE.PointLight('#ff8a2a', 55, 46, 1.7); l.position.set(x, 3, z); scene.add(l); candles.push(l);
    const f = new THREE.Mesh(new THREE.SphereGeometry(0.22, 8, 8), new THREE.MeshBasicMaterial({ color: '#ffd27a' })); f.scale.y = 1.8; f.position.set(x, 2.75, z); scene.add(f);
  }
  candle(-BW / 2 - 3.5, 4); candle(BW / 2 + 3.5, 2);
  // the mood at time T: candle flicker (a pure function of T) and the key following the fight
  const light = (T, mid) => { key.target.position.set(mid[0], 0, mid[2]); candles.forEach((l, i) => { l.intensity = 55 * (0.85 + 0.15 * Math.sin(T * 0.013 + i * 2) * Math.sin(T * 0.031 + i)); }); key.intensity = 150 * (0.94 + 0.06 * Math.sin(T * 0.017) * Math.sin(T * 0.041)); };
  return { renderer, scene, camera, lid, paintLid, light };
}

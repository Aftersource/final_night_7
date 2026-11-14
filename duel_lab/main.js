// Duel Lab: a scrubbable three.js playback of one duel script (duel.json).
// Everything on screen is a pure function of the timeline time T (ms): the fighters' paths are resolved ahead of
// time, the rigid bodies are baked ahead of time, and a frame only reads those records. Rewind and replay are exact.
import * as THREE from 'three';
import { makeStage } from './lib/stage.js';
import { deriveScene } from './lib/props.js';
import { validateScene } from './lib/scene_schema.js';
import { resolveDuel } from './lib/paths.js';
import { bake, movedAt, sceneAt } from './lib/bake.js';
import { makeSceneView } from './lib/sceneview.js';
import { loadAssets, makeFighter, poseFighter, strikeIK, SKINS } from './lib/fighter.js';
import { cameraAt, shot, keepInFrame } from './lib/camera.js';
import { makeDebug } from './lib/debug.js';
import { clamp01, smooth } from './lib/m.js';

const Q = new URLSearchParams(location.search), $ = (id) => document.getElementById(id);
const stage = makeStage(), { renderer, scene, camera } = stage;
let D, W, R, B, FA, FB, view, debug, T = +(Q.get('t') || 0), playing = !Q.has('paused'), speed = 1, TOTAL = 0, PAN = [];
const info = { check: 'checking…', bakeMs: 0, fps: 0 };
const panelAt = (t) => { for (let i = 0; i < PAN.length; i++) if (t < PAN[i].t0 + PAN[i].play_ms) return i; return PAN.length - 1; };
const mask = (s, n, live) => { n = Math.floor(n); if (n <= 0) return ''; const shown = s.slice(0, n); return live ? '*'.repeat(n - 1) + shown.slice(-1) : '*'.repeat(shown.length); };

// resolve the script against the scene and the fighters' masses, then bake the physics for the whole duel
function rebuild() {
  const FBody = { a: FA.body, b: FB.body }, t0 = performance.now(), on = debug && debug.root.visible;
  R = resolveDuel(D, W, FBody); B = bake(W, R, FBody); info.bakeMs = Math.round(performance.now() - t0); PAN = R.PAN; TOTAL = R.TOTAL;
  if (view) view.dispose(); if (debug) debug.dispose();
  view = makeSceneView(scene, W, B); debug = makeDebug(scene, W, B, R, [FA, FB], $('dbg')); if (on || Q.has('debug')) { debug.root.visible = false; debug.toggle(); }
  for (const w of R.warnings) console.log('script:', w);
  // self-check: the same input must give the same bake, bit for bit
  info.check = 'checking…'; setTimeout(() => { const again = bake(W, resolveDuel(D, W, FBody), FBody); info.check = again.hash === B.hash ? 'deterministic ✓' : 'MISMATCH ✗ ' + again.hash;
    console.log(`bake ${B.hash} vs ${again.hash}: ${info.check}`); window.__bake = { hash: B.hash, again: again.hash, ok: again.hash === B.hash, ms: info.bakeMs, bodies: B.bodies.length }; }, 50);
}

const head = new THREE.Vector3(), pts = Array.from({ length: 6 }, () => new THREE.Vector3());
function frame(T) {
  let live = null; const ctx = { R, W, B, moved: movedAt(W, B, T), live: () => live || (live = sceneAt(B, T)) };
  poseFighter(FA, T, ctx); poseFighter(FB, T, ctx); strikeIK(FA, FB, T, ctx); strikeIK(FB, FA, T, ctx);
  // the box opens at the start and shuts at the end
  const open = Math.min(smooth(T / (D.intro_ms * 0.8)), smooth((TOTAL - T) / (D.outro_ms * 0.7)));
  stage.lid.rotation.x = THREE.MathUtils.lerp(Math.PI / 2 - 0.02, -0.1, open);
  const inDuel = T >= D.intro_ms, pi = inDuel ? panelAt(T) : -1, p = pi >= 0 ? PAN[pi] : null, tau = p ? T - p.t0 : 0;
  // camera: this panel's shot fitted to both fighters, eased in from the previous one
  const forced = Q.get('shot'), cam = p ? cameraAt(camera, forced ? { ...p, shot: forced } : p, pi > 0 ? PAN[pi - 1] : { shot: 'box' }, tau, FA.pos, FB.pos) : shot('box');
  let shake = 0, flash = 0, burst = null, flipA = 0;
  if (p) {
    for (const c of p.contacts || []) { const dt = tau - c.t_ms; if (dt >= 0 && dt < 600) { if (!c.miss) { shake = Math.max(shake, Math.exp(-dt / 130)); flash = Math.max(flash, Math.exp(-dt / 70) * 0.35); } if (dt < 520) burst = [c.text, dt, c.by]; } }
    if (p.flip) { flipA = tau < 900 ? Math.sin((tau / 900) * Math.PI) : 0; flash = Math.max(flash, Math.exp(-tau / 90) * 0.25); }
  }
  cam.pos.x += Math.sin(T * 0.09) * shake * 0.22; cam.pos.y += Math.cos(T * 0.13) * shake * 0.18;
  if (p) { [FA, FB].forEach((f, i) => { pts[i * 3].copy(f.root.position); pts[i * 3 + 1].copy(f.root.position).setY(f.root.position.y + 1.0); f.bones.head.getWorldPosition(pts[i * 3 + 2]).setY(pts[i * 3 + 2].y + 0.25); }); keepInFrame(camera, cam.pos, cam.look, pts); }
  camera.position.copy(cam.pos); camera.lookAt(cam.look);
  view.update(T, [cam.pos.x, cam.pos.y, cam.pos.z], [FA, FB].map((f) => { f.bones.spine_02.getWorldPosition(head); return [head.x, head.y, head.z]; }));
  stage.light(T, [(FA.pos.x + FB.pos.x) / 2, 0, (FA.pos.z + FB.pos.z) / 2]); debug.update(T, info);
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
    const before = pi > 0 ? PAN[pi - 1].hp : { a: D.a.hp, b: D.b.hp }, hits = (p.contacts || []).filter((c) => !c.miss), hit = hits[hits.length - 1], ht = hit ? hit.t_ms : 0, h = smooth((tau - ht) / 300);
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

async function main() {
  D = await (await fetch(Q.get('duel') || 'duel.json')).json();
  if (Q.get('a')) D.a.skin = Q.get('a'); if (Q.get('b')) D.b.skin = Q.get('b');
  for (const m of validateScene(D.scene || {})) console.warn('scene:', m);
  W = deriveScene(D.scene || { objects: [] });
  const assets = await loadAssets();
  FA = makeFighter(scene, assets, D.a, 'a'); FB = makeFighter(scene, assets, D.b, 'b'); stage.paintLid(D); rebuild();
  $('nA').textContent = D.a.name; $('nB').textContent = D.b.name; $('wA').textContent = D.a.name.toUpperCase(); $('wB').textContent = D.b.name.toUpperCase();
  document.documentElement.style.setProperty('--a', D.a.aura); document.documentElement.style.setProperty('--b', D.b.aura);
  // a skin is a material: changing it changes the fighter's mass, so the paths are re-resolved and the physics re-baked
  for (const [id, f] of [['skA', FA], ['skB', FB]]) { const s = $(id); SKINS.forEach((n) => s.add(new Option(n, n, false, n === f.skin))); s.onchange = () => { f.setSkin(s.value); rebuild(); }; }
  $('ticks').innerHTML = PAN.map((p) => `<i style="left:${(p.t0 / TOTAL) * 100}%"></i>`).join('');
  $('play').onclick = () => { playing = !playing; if (playing && T >= TOTAL) T = 0; };
  $('replay').onclick = () => { T = 0; playing = true; };
  $('speed').onchange = (e) => (speed = +e.target.value);
  $('scrub').oninput = (e) => { T = (e.target.value / 1000) * TOTAL; playing = false; };
  addEventListener('keydown', (e) => { if (e.code === 'Space') { e.preventDefault(); $('play').click(); } if (e.key === 'r') $('replay').click(); if (e.key === 'd' || e.key === 'D') debug.toggle();
    if (e.key === 'ArrowLeft') { T = Math.max(0, T - 100); playing = false; } if (e.key === 'ArrowRight') { T = Math.min(TOTAL, T + 100); playing = false; } });
  // a digest of everything posed this frame (every bone of both fighters, every body, the camera): for the rewind check
  window.__digest = (t) => { frame(t); let h = 2166136261; const eat = (x) => { h ^= Math.round(x * 1e5); h = Math.imul(h, 16777619); };
    for (const f of [FA, FB]) for (const b of Object.values(f.bones)) b.matrixWorld.elements.forEach(eat); for (const v of view.views) { eat(v.g.visible ? 1 : 0); if (v.g.visible) { v.g.position.toArray().forEach(eat); v.g.quaternion.toArray().forEach(eat); } }
    camera.matrixWorld.elements.forEach(eat); return (h >>> 0).toString(16); };
  $('load').remove(); window.READY = true; window.__setT = (t) => { T = t; playing = false; }; document.title = 'Duel Lab ready';
  let last = performance.now(), acc = 0, n = 0;
  renderer.setAnimationLoop((now) => { const dt = now - last; last = now; acc += dt; if (++n >= 30) { info.fps = Math.round((1000 * n) / acc); window.__fps = info.fps; acc = 0; n = 0; }
    if (playing) { T += dt * speed; if (T >= TOTAL) { T = TOTAL; playing = false; } }
    $('play').textContent = playing ? 'Pause' : 'Play'; frame(Math.min(T, TOTAL - 1)); });
}
main().catch((e) => { $('load').textContent = 'FAILED: ' + e.message; console.error(e); });

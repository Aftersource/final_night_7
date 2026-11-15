// node lab_export.mjs <duel_lab dir> [duel.json] [a-skin] [b-skin] > export.json
// Runs the browser lab's own engine (duel_lab/lib, read only) on a duel and prints what happened in a neutral shape:
// the event log, and for every body where it started, where it ended, how far it went and how far it tilted.
// `duelsim_lab --lab export.json` compares this crate's bake against it (docs/DUEL_SIM.md, "Parity with the lab").
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
const [labDir = 'duel_lab', file, skinA, skinB] = process.argv.slice(2);
const lib = (f) => import(pathToFileURL(path.resolve(labDir, 'lib', f)).href);
const { deriveScene } = await lib('props.js'), { resolveDuel } = await lib('paths.js'), { bake } = await lib('bake.js'), { fighterBody } = await lib('materials.js'), { qrot } = await lib('m.js');
const D = JSON.parse(fs.readFileSync(file || path.resolve(labDir, 'duel.json')));
const W = deriveScene(D.scene), FB = { a: fighterBody(skinA || D.a.skin), b: fighterBody(skinB || D.b.skin) }, R = resolveDuel(D, W, FB);
const t0 = performance.now(), B = bake(W, R, FB), ms = performance.now() - t0;
const tilt = (q) => Math.acos(Math.max(-1, Math.min(1, qrot(q, [0, 1, 0])[1]))) * 180 / Math.PI;
const bodies = B.bodies.filter((b) => !b.hidden).map((b) => {
  const n = b.count, P = (i) => [b.pos[i * 3], b.pos[i * 3 + 1], b.pos[i * 3 + 2]], Q = (i) => [b.quat[i * 4], b.quat[i * 4 + 1], b.quat[i * 4 + 2], b.quat[i * 4 + 3]];
  if (!n) return { id: b.id, obj: b.obj, frag: b.frag, piece: b.piece, fixed: b.fixed, src: b.parts[0].src, samples: 0 };
  const up0 = qrot(Q(0), [0, 1, 0]); let maxTilt = 0, far = 0;
  for (let i = 0; i < n; i++) { const u = qrot(Q(i), [0, 1, 0]), d = Math.acos(Math.max(-1, Math.min(1, u[0] * up0[0] + u[1] * up0[1] + u[2] * up0[2]))) * 180 / Math.PI; maxTilt = Math.max(maxTilt, d);
    far = Math.max(far, Math.hypot(P(i)[0] - P(0)[0], P(i)[2] - P(0)[2])); }
  const p0 = P(0), p1 = P(n - 1);
  return { id: b.id, obj: b.obj, frag: b.frag, piece: b.piece, fixed: b.fixed, src: b.parts[0].src, samples: n, born: b.t0, died: Number.isFinite(b.t1) ? b.t1 : null, mass: b.mass,
    p0, p1, moved: Math.hypot(p1[0] - p0[0], p1[2] - p0[2]), far, max_tilt: maxTilt, final_tilt: tilt(Q(n - 1)),
    parts: b.parts.map((p) => ({ src: p.src, damage: p.damage, died: Number.isFinite(p.t1) ? p.t1 : null })) };
});
const events = B.events.map((e) => ({ t: e.t, type: e.type, obj: e.obj || '', J: e.J || 0, n: e.pieces || e.shards || 0, why: e.why || '', sim: !!e.sim, by: e.by || '', fighter: e.fighter || '', with: e.with || '' }));
process.stdout.write(JSON.stringify({ engine: 'duel_lab', intro_ms: D.intro_ms, skins: [FB.a.skin, FB.b.skin], masses: [FB.a.mass, FB.b.mass], steps: B.steps, hash: B.hash, bake_ms: ms, warnings: R.warnings, events, bodies }));

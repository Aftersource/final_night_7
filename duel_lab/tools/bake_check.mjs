// node bake_check.mjs [duel.json] [a-skin] [b-skin]: bake the duel twice and compare the hashes bit for bit,
// then print what happened (the event log an author tunes a script against).
import fs from 'node:fs';
import { deriveScene } from '../lib/props.js';
import { resolveDuel } from '../lib/paths.js';
import { bake } from '../lib/bake.js';
import { fighterBody } from '../lib/materials.js';
const D = JSON.parse(fs.readFileSync(new URL('../' + (process.argv[2] || 'duel.json'), import.meta.url)));
const run = () => { const W = deriveScene(D.scene), FB = { a: fighterBody(process.argv[3] || D.a.skin), b: fighterBody(process.argv[4] || D.b.skin) }, R = resolveDuel(D, W, FB); return { W, FB, R, B: bake(W, R, FB) }; };
const t0 = performance.now(), one = run(), t1 = performance.now(), two = run();
console.log(`fighters: a ${one.FB.a.skin} ${one.FB.a.mass.toFixed(0)} kg (jump x${one.FB.a.jump.toFixed(2)}), b ${one.FB.b.skin} ${one.FB.b.mass.toFixed(0)} kg (jump x${one.FB.b.jump.toFixed(2)})`);
console.log(`bake: ${one.B.steps} steps at 120 Hz, ${one.B.bodies.length} bodies, ${(t1 - t0).toFixed(0)} ms`);
console.log(`hash ${one.B.hash} / ${two.B.hash} -> ${one.B.hash === two.B.hash ? 'DETERMINISTIC' : 'MISMATCH'}`);
for (const w of one.R.warnings) console.log('warning:', w);
if (!process.argv.includes('-q')) for (const e of one.B.events) if (e.type !== 'hit' && e.type !== 'push' || e.J > 60) console.log(((e.t * 1000 - D.intro_ms) / 1000).toFixed(2).padStart(6), e.type.padEnd(7), (e.obj || '').padEnd(10), e.fighter ? 'fighter ' + e.fighter : '', e.with || e.why || '', e.by ? 'by ' + e.by : '', e.J ? e.J.toFixed(0) + ' N·s' : '', e.pieces || e.shards || '');
process.exit(one.B.hash === two.B.hash ? 0 : 1);

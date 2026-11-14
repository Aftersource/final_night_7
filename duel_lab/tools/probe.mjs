// node probe.mjs : derive the scene and print what the engine knows about every object
import fs from 'node:fs';
import { deriveScene } from '../lib/props.js';
const D = JSON.parse(fs.readFileSync(new URL('../' + (process.argv[2] || 'duel.json'), import.meta.url))), W = deriveScene(D.scene || D);
for (const o of W.objects) if (!o.hidden) console.log(o.id.padEnd(12), (o.mass.toFixed(0) + ' kg').padStart(9), 'com', o.com.map((x) => x.toFixed(2)).join(','), 'tips at', o.tipAngle.toFixed(0) + '°', o.fixed ? 'fixed' : '',
  '|', W.affordances.filter((a) => a.obj === o.id).map((a) => a.kind + (a.mode ? ':' + a.mode : '') + (a.height ? '@' + a.height.toFixed(2) : '')).join(' '));

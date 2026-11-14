// Ask Claude for a duel scene in the Duel Lab scene schema, validate it, and print what the engine derives from it.
//   node make_scene.mjs --place "Jakarta, old harbour market" --time night --backstory "..." --facts facts.txt --out ../scene.out.json
//   node make_scene.mjs --dry --place ... [--check ../duel.json]   prints the prompt, validates a file, calls nothing
// The key comes from the environment (ANTHROPIC_API_KEY or an `ant auth login` profile); this tool never reads key files.
import fs from 'node:fs';
import { SCENE_SCHEMA, validateScene, KINDS } from '../lib/scene_schema.js';
import { MATERIALS } from '../lib/materials.js';
import { deriveScene, STAGE } from '../lib/props.js';

const arg = (k, d) => { const i = process.argv.indexOf('--' + k); return i > 0 ? process.argv[i + 1] : d; }, has = (k) => process.argv.includes('--' + k);
const place = arg('place', 'an unnamed place'), time = arg('time', 'night'), backstory = arg('backstory', '');
const facts = arg('facts') ? fs.readFileSync(arg('facts'), 'utf8').split('\n').map((l) => l.trim()).filter(Boolean) : [];
const MODEL = arg('model', 'claude-sonnet-5-5');

const system = `You dress a miniature duel stage for a tabletop game. The stage is a wooden box floor of pointy-top hexes, 1 m each: hex [q, r] sits at x = 1.732 * (q + r / 2), z = 1.5 * r metres, y up. The playable floor is |x| <= ${STAGE.hx}, |z| <= ${STAGE.hz}. The audience looks from +z toward -z. Fighter A starts at hex [-3, 0], fighter B at [3, 0]; leave both hexes and a lane between them clear, and put tall things toward the back (negative r) or the sides.

You return one scene: 12 to 22 objects that this place, at this hour, with this history, would really have. Every object is built from primitive parts and will be simulated as a rigid body, so sizes are real metres and materials are real: mass is density x volume x fill.
- parts: shape box [w, h, d] · cylinder [radius, height] (axis up) · cone [radius, height] · sphere [radius] · tetra / octa / ico / dodeca [radius]. offset is the part's centre relative to the object's origin on the floor (y up, so a 0.7 m crate has offset [0, 0.35, 0]). rot is [x, y, z] degrees. Hex coordinates may be fractional.
- material: one of ${Object.keys(MATERIALS).join(', ')}. fill (0-1) is how much of the volume is solid: a crate or barrel is about 0.12, a cushion 0.25, solid stone 1.
- fixed: true for things built into the ground (walls, pillars, stairs, trees, statues); they do not move, but their parts are pre-cut segments that can break off, so build a pillar as stacked drums, a statue as plinth, body, head. Everything else is loose and will be kicked, landed on and thrown into.
- on: the id of an earlier object this one rests on (a jar on a desk, a crate on a crate).
- kind: one of ${KINDS.join(', ')}. glow (a number) makes a part emit light (coals, a lantern); color overrides the material's colour.
- note: one short line of who left it there or what it is; the fight's captions quote these.

The fighters do parkour, so give them something of each kind: a top 0.5-1.2 m high and under 1.7 m deep to vault (desk, low wall, cart), a ledge 1.2-2.6 m high to mantle and perch on (stacked crates, sacks, a platform), a vertical face at least 2 m tall or wide to wall-run (pillar, wall), a table-height underside with clear floor to slide under, a thin horizontal bar 2-3.2 m up to swing from, steps, loose light things to kick, and at least one brittle thing (glass, clay, ice) to shatter and one heavy fixed thing built in segments to crack.
Respect the settled facts: they are canon for this place. Do not invent people. Write the place's own backstory in two to four sentences.`;
const user = `Place: ${place}\nTime of day: ${time}\nBackstory so far: ${backstory || '(none given: write one)'}\nSettled facts about this place:\n${facts.length ? facts.map((f) => '- ' + f).join('\n') : '- (none yet)'}`;

function report(scene) {
  const bad = validateScene(scene, STAGE); if (bad.length) { console.error('INVALID SCENE:\n  ' + bad.join('\n  ')); return false; }
  const W = deriveScene(scene), kinds = {}; for (const a of W.affordances) kinds[a.kind] = (kinds[a.kind] || 0) + 1;
  console.error(`valid: ${scene.objects.length} objects`);
  for (const o of W.objects) if (!o.hidden) console.error(`  ${o.id.padEnd(14)} ${o.mass.toFixed(0).padStart(6)} kg  tips at ${o.tipAngle.toFixed(0).padStart(2)}°${o.fixed ? '  fixed' : ''}`);
  console.error('affordances: ' + Object.entries(kinds).map(([k, n]) => `${k} x${n}`).join(', '));
  for (const k of ['vault', 'mantle', 'wallrun', 'slide']) if (!kinds[k]) console.error(`  note: nothing to ${k}`);
  return true;
}

if (has('dry')) {
  console.log('--- system ---\n' + system + '\n--- user ---\n' + user + '\n--- model ---\n' + MODEL + ', output_config.format json_schema (' + JSON.stringify(SCENE_SCHEMA).length + ' bytes of schema)');
  if (arg('check')) { const d = JSON.parse(fs.readFileSync(arg('check'), 'utf8')); process.exit(report(d.scene || d) ? 0 : 1); }
  process.exit(0);
}
const { default: Anthropic } = await import('@anthropic-ai/sdk');
const client = new Anthropic(); // credentials from the environment only
try {
  const res = await client.messages.create({ model: MODEL, max_tokens: 16000, system, messages: [{ role: 'user', content: user }], output_config: { format: { type: 'json_schema', schema: SCENE_SCHEMA } } });
  if (res.stop_reason === 'refusal') { console.error('the model declined:', res.stop_details?.explanation || ''); process.exit(2); }
  if (res.stop_reason === 'max_tokens') { console.error('the scene was cut off at max_tokens'); process.exit(2); }
  const text = res.content.filter((b) => b.type === 'text').map((b) => b.text).join(''), scene = JSON.parse(text);
  const ok = report(scene), out = arg('out'); if (out) fs.writeFileSync(out, JSON.stringify(scene, null, 2) + '\n'); else console.log(JSON.stringify(scene, null, 2));
  process.exit(ok ? 0 : 1);
} catch (e) {
  if (e instanceof Anthropic.AuthenticationError) console.error('no valid credentials: set ANTHROPIC_API_KEY or run `ant auth login`');
  else if (e instanceof Anthropic.RateLimitError) console.error('rate limited, try again shortly');
  else if (e instanceof Anthropic.APIError) console.error(`API error ${e.status}: ${e.message}`);
  else console.error(e.message);
  process.exit(2);
}

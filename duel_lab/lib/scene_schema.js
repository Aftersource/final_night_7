// The scene spec: the shape an LLM authors (and the JSON schema it is held to), plus a validator.
import { MATERIALS } from './materials.js';

export const SHAPES = ['box', 'cylinder', 'cone', 'sphere', 'tetra', 'octa', 'ico', 'dodeca'];
export const KINDS = ['pillar', 'crate', 'desk', 'couch', 'wall', 'stairs', 'statue', 'tree', 'brazier', 'cart', 'barrel', 'jar', 'bottle', 'screen', 'sacks', 'bench', 'post', 'door', 'boulder', 'altar', 'chest', 'thing'];
const SIZE_LEN = { box: 3, cylinder: 2, cone: 2, sphere: 1, tetra: 1, octa: 1, ico: 1, dodeca: 1 };
const num = { type: 'number' }, str = { type: 'string' };
// JSON schema for structured output (every object closed; numeric ranges are checked by validateScene instead)
export const SCENE_SCHEMA = {
  type: 'object', additionalProperties: false, required: ['place', 'time_of_day', 'backstory', 'objects'],
  properties: {
    place: str, time_of_day: { type: 'string', enum: ['dawn', 'day', 'dusk', 'night'] }, backstory: str,
    objects: { type: 'array', items: {
      type: 'object', additionalProperties: false, required: ['id', 'kind', 'at', 'yaw', 'parts', 'fixed', 'note'],
      properties: {
        id: str, kind: { type: 'string', enum: KINDS }, at: { type: 'array', items: num }, yaw: num, fixed: { type: 'boolean' }, on: str, note: str,
        parts: { type: 'array', items: {
          type: 'object', additionalProperties: false, required: ['shape', 'size', 'offset', 'material'],
          properties: { shape: { type: 'string', enum: SHAPES }, size: { type: 'array', items: num }, offset: { type: 'array', items: num }, rot: { type: 'array', items: num },
            material: { type: 'string', enum: Object.keys(MATERIALS) }, fill: num, color: str, glow: num } } } } } },
  },
};
// returns a list of problems (empty = valid). stage: half extents of the floor in metres.
export function validateScene(s, stage = { hx: 9.9, hz: 7.9 }) {
  const bad = [], ids = new Set(), isNum = (x) => typeof x === 'number' && Number.isFinite(x), vec = (v, n) => Array.isArray(v) && v.length === n && v.every(isNum);
  if (!s || typeof s !== 'object') return ['scene is not an object'];
  for (const k of ['place', 'time_of_day', 'backstory']) if (typeof s[k] !== 'string' || !s[k]) bad.push(`${k}: missing`);
  if (!Array.isArray(s.objects) || !s.objects.length) return bad.concat('objects: need at least one');
  if (s.objects.length > 32) bad.push('objects: more than 32');
  s.objects.forEach((o, i) => { const w = `objects[${i}]${o && o.id ? ' ' + o.id : ''}`;
    if (!o || typeof o.id !== 'string' || !/^[a-z][a-z0-9_]*$/.test(o.id)) bad.push(`${w}: id must be lower_snake_case`); else if (ids.has(o.id)) bad.push(`${w}: duplicate id`);
    if (!vec(o.at, 2)) bad.push(`${w}: at must be hex [q, r]`); else { const x = Math.sqrt(3) * (o.at[0] + o.at[1] / 2), z = 1.5 * o.at[1]; if (Math.abs(x) > stage.hx || Math.abs(z) > stage.hz) bad.push(`${w}: at is off the stage (x ${x.toFixed(1)}, z ${z.toFixed(1)})`); }
    if (o.yaw !== undefined && !isNum(o.yaw)) bad.push(`${w}: yaw must be degrees`);
    if (o.on !== undefined && !ids.has(o.on)) bad.push(`${w}: on "${o.on}" must name an earlier object`);
    if (!Array.isArray(o.parts) || !o.parts.length || o.parts.length > 12) bad.push(`${w}: needs 1 to 12 parts`);
    else o.parts.forEach((p, j) => { const v = `${w} parts[${j}]`;
      if (!SHAPES.includes(p.shape)) return bad.push(`${v}: shape ${p.shape}`);
      if (!vec(p.size, SIZE_LEN[p.shape]) || p.size.some((x) => x <= 0.004 || x > 12)) bad.push(`${v}: ${p.shape} size needs ${SIZE_LEN[p.shape]} positive number(s) in metres`);
      if (p.offset !== undefined && !vec(p.offset, 3)) bad.push(`${v}: offset [x, y, z]`); if (p.rot !== undefined && !vec(p.rot, 3)) bad.push(`${v}: rot [x, y, z] degrees`);
      if (!MATERIALS[p.material]) bad.push(`${v}: unknown material ${p.material}`); if (p.fill !== undefined && !(p.fill > 0 && p.fill <= 1)) bad.push(`${v}: fill must be in (0, 1]`); });
    if (o && o.id) ids.add(o.id); });
  return bad;
}

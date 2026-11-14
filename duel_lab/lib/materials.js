// The materials table: what things are made of decides how they weigh, slide, bounce, break and sound.
// density kg/m^3 · friction (Coulomb mu) · restitution · strength: impulse (N·s) one square metre of the
// part's smallest cross-section survives; a hit past it breaks the part, 40 % of it cracks it ·
// brittle: breaks into `shards` fragments instead of coming apart at its joints · word/sound: impact class.
export const MATERIALS = {
  stone:    { density: 2500,  friction: 0.65, restitution: 0.10, strength: 5200,  brittle: false, shards: 3,  color: '#57534e', roughness: 0.95, metalness: 0.0,  word: 'KRAK',   sound: 'stone' },
  marble:   { density: 2700,  friction: 0.50, restitution: 0.12, strength: 4200,  brittle: false, shards: 4,  color: '#b9b2a4', roughness: 0.45, metalness: 0.0,  word: 'KRAK',   sound: 'stone' },
  wood:     { density: 650,   friction: 0.50, restitution: 0.25, strength: 6000,  brittle: false, shards: 5,  color: '#4a3220', roughness: 0.85, metalness: 0.0,  word: 'THOK',   sound: 'wood' },
  iron:     { density: 7800,  friction: 0.45, restitution: 0.25, strength: 30000, brittle: false, shards: 2,  color: '#3a3c40', roughness: 0.55, metalness: 0.9,  word: 'KLANG',  sound: 'metal' },
  gold:     { density: 19300, friction: 0.40, restitution: 0.15, strength: 9000,  brittle: false, shards: 2,  color: '#c9a03a', roughness: 0.30, metalness: 1.0,  word: 'DONG',   sound: 'metal' },
  cloth:    { density: 240,   friction: 0.80, restitution: 0.02, strength: 60000, brittle: false, shards: 1,  color: '#5a1f26', roughness: 1.0,  metalness: 0.0,  word: 'WHUMP',  sound: 'soft' },
  glass:    { density: 2500,  friction: 0.35, restitution: 0.30, strength: 450,   brittle: true,  shards: 12, color: '#9fd0d8', roughness: 0.08, metalness: 0.0,  word: 'KSSSH',  sound: 'glass', opacity: 0.45 },
  bone:     { density: 1800,  friction: 0.50, restitution: 0.20, strength: 1800,  brittle: true,  shards: 4,  color: '#d8cfb8', roughness: 0.7,  metalness: 0.0,  word: 'KRIK',   sound: 'bone' },
  ice:      { density: 917,   friction: 0.05, restitution: 0.10, strength: 900,   brittle: true,  shards: 8,  color: '#bfe6f2', roughness: 0.1,  metalness: 0.0,  word: 'KRSH',   sound: 'glass', opacity: 0.7 },
  obsidian: { density: 2400,  friction: 0.45, restitution: 0.15, strength: 1500,  brittle: true,  shards: 6,  color: '#17131c', roughness: 0.15, metalness: 0.3,  word: 'TSSK',   sound: 'glass' },
  clay:     { density: 1900,  friction: 0.60, restitution: 0.05, strength: 700,   brittle: true,  shards: 6,  color: '#8a5238', roughness: 0.9,  metalness: 0.0,  word: 'KRASH',  sound: 'clay' },
  leaf:     { density: 90,    friction: 0.70, restitution: 0.02, strength: 60000, brittle: false, shards: 1,  color: '#1f3a26', roughness: 1.0,  metalness: 0.0,  word: 'FSSH',   sound: 'soft' },
  rope:     { density: 500,   friction: 0.80, restitution: 0.05, strength: 20000, brittle: false, shards: 1,  color: '#8a7350', roughness: 1.0,  metalness: 0.0,  word: 'THWIP',  sound: 'soft' },
};
export const mat = (name) => MATERIALS[name] || MATERIALS.wood;
export const CRACK_AT = 0.4; // fraction of break strength at which a part cracks

// What a fighter's skin is made of. The figure is a cast: `fill` is how much of its volume is material.
export const SKIN_BODY = {
  wood:        { material: 'wood',     fill: 1.0 },   // solid limewood, the lightest
  butterfly:   { material: 'cloth',    fill: 1.0 },   // lacquered paper and silk: light, floats
  dragonscale: { material: 'bone',     fill: 0.8 },   // horn and scale
  obsidian:    { material: 'obsidian', fill: 0.4 } ,  // volcanic glass over a hollow core
  gold:        { material: 'gold',     fill: 0.14 },  // a hollow gold casting, still by far the heaviest
};
export const FIGURE_VOLUME = 0.066; // m^3, fallback when the mesh has not been measured (node tools)
export const REF_MASS = 43;         // kg: the wooden figure; jump heights in a script are written for it
// per-segment mass fractions (fallback; the browser measures them from the skinned mesh)
export const SEGMENTS = { pelvis: 0.14, spine_01: 0.1, spine_02: 0.12, spine_03: 0.13, neck_01: 0.01, head: 0.07,
  clavicle_l: 0.01, upperarm_l: 0.027, lowerarm_l: 0.016, hand_l: 0.007, clavicle_r: 0.01, upperarm_r: 0.027, lowerarm_r: 0.016, hand_r: 0.007,
  thigh_l: 0.1, calf_l: 0.045, foot_l: 0.012, ball_l: 0.003, thigh_r: 0.1, calf_r: 0.045, foot_r: 0.012, ball_r: 0.003 };
export function fighterBody(skin, volume = FIGURE_VOLUME) {
  const s = SKIN_BODY[skin] || SKIN_BODY.wood, m = mat(s.material), mass = m.density * s.fill * volume, k = REF_MASS / mass;
  // muscle does not scale with weight: a heavier figure launches slower. jump: apex scale, speed: strike / take-off speed scale
  return { skin, material: s.material, mass, jump: Math.min(1.25, Math.sqrt(k)), speed: Math.min(1.15, Math.pow(k, 0.25)), friction: m.friction, restitution: m.restitution };
}

# Duel sim: the server bakes the physics, every client plays it back

`crates/fn7-duelsim` is the browser lab's physics (`duel_lab/lib`: materials, mass properties, a rigid-body engine,
the bake, affordances, parkour paths, strike IK) in plain Rust, run **once, on the server**, per duel panel. Its
output is a compact `sim` object per panel: keyframe tracks for every object that moved, the break / shatter / hit
events with the ids of the pieces they make, and for each fighter a root track plus **timed limb targets** for the
client's IK. The Unreal box (and the lab, and any later client) only plays it back: no physics in Unreal C++, no
per-client drift, exact on rewind, resume and fork.

Status: the crate, its tests and the seam exist; **nothing in `fn7-plugin` calls it yet**. The lead wires it after
`backend/duel-weapons` merges (the 3 lines in [Wiring](#wiring)). Client shapes: docs/CLIENT_API.md, "Duel sim:
baked physics and limb targets".

## What is modelled

| Lab | Here | Notes |
|---|---|---|
| `materials.js` | `data/materials.json`, `materials.rs` | 13 materials (density, friction, restitution, strength, brittle, shards, impact word, sound) with the scene generator's names (a test reads `fn7-plugin/src/panel.rs` and fails on a name not in the table); 14 skins (5 live + 9 `future`), the mannequin figure, strike limbs, projectiles |
| `shapes.js` | `shapes.rs` | box, cylinder, cone, sphere, tetra, octa, ico, dodeca: volume, centre, inertia tensor, smallest section, contact vertices (a box carries its 12 edge midpoints too), signed distance, ray test, pre-cut shards |
| `props.js` | `props.rs` | compound bodies from `parts[]`: mass, centre of mass, inertia (parallel axis), bounds, support polygon, tipping angle and energy; stacking (`on`); tops, walls, rounds, and the affordances `vault mantle wallrun wallkick slide perch swing` |
| `physics.js` | `physics.rs` | fixed 120 Hz; gravity; floor, body-body (vertex / feature) and fighter-sphere contacts; Coulomb friction; restitution; sequential impulses (10 iterations, Baumgarte 0.2, slop 4 mm); sleeping; per-part damage, cracks at 40 %, joint breaks (a fixed thing loses the struck part and everything above it, a loose one comes apart), shatter into pre-cut shards for brittle parts; strikes, shoves, radial blasts |
| `paths.js` | `paths.rs` | dash, leap, flip, knock, vault, slide, wallrun, wallkick, mantle, drop, perch resolved on the real surfaces, with jump apex and speed scaled by mass; ballistic arcs; standing weight on what a fighter stands on; a knocked fighter landing on an object hits it with its weight |
| `bake.js` | `bake.rs`, `wire.rs` | incremental (panel n continues from n - 1), quantised keyframes only while moving, events, fighter root tracks with lean, limb targets, a chained hash |

Edge-edge contacts are not modelled (as in the lab): box edge midpoints stand in for them.

### Fighters

Mass = skin material density x skin fill x the mannequin's volume (0.066 m^3): **gold 178 kg, dragonscale 95,
obsidian 63, wood 43, butterfly 16** (`data/materials.json` `skins`; future skins and figures are rows there).
Segment masses are the figure's shares (22 UE5 mannequin bones), the standing centre of mass is at the hips (0.96 m).
Jump scale = sqrt(43 / mass) (gold 0.49), speed scale = (43 / mass)^(1/4) (butterflies: 1.25 / 1.15). A strike is
`limb share x mass x limb speed x speed scale x power` N s, with power = `0.5 + 2.6 x contacts[].impulse` (a full
blow is the lab's boosted 3.1). In the engine a fighter is two kinematic spheres (0.30 m at 0.95 m, 0.26 m at
1.40 m) carrying its real mass into a collision and never pushed back by scenery (the rules decide where fighters go).

### Projectiles

Both shapes are read: the lab's (`t_ms, kind, on | to: [q, r], y, mass, speed, blast, radius`) and the rules' planned
one (docs/DUEL_MOVES.md 4.6: `t_fire_ms, t_hit_ms, kind, to: "a" | "b" | <object id> | [q, r], speed, on_hit:
{ impulse, aoe_hexes }`). The first body on the line is hit with mass x speed x power; a rocket / fireball, or any
shot with `aoe_hexes`, bursts there as a radial blast over `aoe_hexes x 1.732 m` (falling off as (1 - d/r)^2).
`rock` reads as `stone`, `orb` as `fireball`. Each shot is in `sim.shots[]` for the client's tracer.

## Determinism

**Choice: `f64`, IEEE operations only, no libm, a fixed order.** Every value is an `f64` computed with `+ - * /` and
`sqrt` (correctly rounded by IEEE 754 on every target) in source order; Rust never contracts to fused multiply-add
or reassociates unless asked (no `mul_add`, no fast-math anywhere). `sin`, `cos`, `atan2`, `cbrt` are the crate's
own (`math.rs`: Cody-Waite reduction and fixed polynomials, built from the same exact operations; within 1e-14 of
libm, tested), so no platform library is ever called. One seeded RNG (mulberry32, the lab's: bit-identical draws,
tested); no clock, no hash maps, no threads; bodies, contacts and events in index order. Fixed point was rejected:
the lab's engine is floating point and parity with it is a requirement; fixed point would also need its own trig
and a re-tuned solver.

**Evidence.** The lab duel (gold vs obsidian, seed 7, 11 panels) bakes to wire hash `9f677152` and state hash
`bfc407b3a54f5c96` (FNV-1a 64 over every `f64` of the world, bit for bit) on aarch64-apple-darwin in debug and in
release and on **x86_64-apple-darwin (SSE2) under Rosetta**; also wood, butterfly and dragonscale variants matched
across the two architectures. The pin is the test `the_lab_duel_bakes_to_the_pinned_hash`. Windows and Linux on
x86_64 run the same machine instructions for these operations (SSE2 doubles), so the same hash is expected; run
`cargo test -p fn7-duelsim` there to confirm (a pin failure is a determinism bug, or an intended engine change that
re-pins). To repeat the cross-architecture check on a Mac:

```sh
rustup target add x86_64-apple-darwin
CARGO_TARGET_DIR=/tmp/tx86 cargo build --release -p fn7-duelsim --bin duelsim_lab --target x86_64-apple-darwin
arch -x86_64 /tmp/tx86/x86_64-apple-darwin/release/duelsim_lab duel_lab/duel.json | grep ^hash
./target/release/duelsim_lab duel_lab/duel.json | grep ^hash
```

**Incremental = from scratch.** `Sim` is the whole state after n panels; `Sim::panel` bakes the next. A test bakes
the lab duel panel by panel keeping the state, from scratch for every panel, from a stale state (ignored), from a
cloned checkpoint, through `with_sims`, `present_sim` and the cache, and requires the same bytes everywhere.

## Reconnects: recompute on demand, cache in memory

Nothing is stored in the match state: a panel's `sim` is a pure function of (scene, the two skins, the panels so
far, the duel's seed). The live seam keeps the newest `Sim` of up to 16 duels in a process-wide cache keyed by duel
id and checked against a signature of every input, so a streaming duel bakes each panel once; a miss (restart,
replay, fork, rewind) re-bakes from the panels. For a client that joins mid-duel, `present.duels[].sim` carries
`at`: a **checkpoint** of the stage as the newest panel starts (every displaced body's pose, every piece and shard
that exists, dead and cracked parts, the fighters' positions) and that panel's `sim`, so it never needs the earlier
panels' tracks.

## Cost (release, Apple M-series, the lab duel)

| | |
|---|---|
| a whole 11-panel duel (gold vs obsidian), fastest of 10 | 155 ms |
| per panel | 0.4 ms (intro, quiet) to 48 ms (the panel where the screen shatters and 47 bodies move) |
| a real rules duel from a recording (14 panels, 9 objects, no breakage) | 0.13 to 0.3 ms per panel |
| scene derivation (masses, affordances) | 0.5 to 0.6 ms |

The solver dominates (contacts x 10 iterations). The lab bakes the same duel in about 1 s in node.

## Sizes (JSON of `sim`, the lab duel)

| panel | what happens | bytes |
|---|---|---|
| intro, outro | standing; the outro's settle | 333, 568 |
| 1 | b mantles the sacks | 1,222 |
| 4 | a wallkick exchange | 2,085 |
| 2, 3, 5, 7, 9 | strikes, a crate kicked, vaults, wall-runs | 2,608 to 3,098 |
| 8 | wall-run, the pillar struck | 4,699 |
| 6 | the screen, a jar and a bottle shatter (47 bodies) | 11,148 |

A rules duel from a recording (no objects moved): 333 to 1,027 bytes per panel. Budgets tested in release:
every panel <= 12 KB, a panel without breakage <= 6 KB, a calm panel (no events, at most one body, no flights)
<= 1.5 KB. A parkour panel exceeds 1.5 KB mostly by its root tracks (16 bytes a key, a key wherever linear playback
would be off by 2 cm or 0.05 rad) and its limb targets.

## Parity with the lab

`tools/lab_export.mjs` runs the lab's own engine (read only) on a duel and writes its events and body motion;
`duelsim_lab --lab <export>` bakes the same duel here and prints the table (events matched by kind and object within
0.1 s and 5 % of impulse; bodies by how far they went, how far they tilted and where they rest).

```sh
node crates/fn7-duelsim/tools/lab_export.mjs ../fn7_main/duel_lab ../fn7_main/duel_lab/duel.json gold obsidian > lab_gold.json
cargo run --release -p fn7-duelsim --bin duelsim_lab -- ../fn7_main/duel_lab/duel.json --lab lab_gold.json [--lab-exact]
```

**`--lab-exact` (the lab's engine to the letter): every row within tolerance for both the gold and the wooden
fighter.** The default differs from the lab in one place: a sleeping body wakes when what it rests on moves (faster
than 0.5 m/s at the contact). The lab solves a sleeper as fixed and wakes only the neighbours of a struck body, so a
stack can hang in the air when its bottom is knocked out; this engine lets it fall (test
`a_stack_stands_and_falls_when_its_support_goes`). Because a rigid-body pile is chaotic, that one difference moves a
few secondary rows in the default mode (gold: 2 extra cracks on the desk and 1 extra touch; wood: crate_a and crate_c
of the stack end elsewhere, 1 touch fewer). Every event Chris named is the same in both modes:

| event | lab | here (default) | delta |
|---|---|---|---|
| a's kick on crate_e (gold) | 4.03 s, 309 N s | 4.03 s, 309 N s | 0 |
| crate_e skids into b | touch 5.71 s, 11.12 s | 5.71 s, 11.12 s | 0 |
| crate_e: farthest / at rest | 5.80 m, (1.29, -1.15) | 5.80 m, (1.46, -0.74) | 0.44 m |
| the glass screen shatters | crack 16.51 s, shatter 16.55 s, 12 shards | 16.51 s, 16.55 s, 12 | 0 |
| bottle / jar_b shatter | 15.59 s / 15.67 s | 15.52 s / 15.62 s | -0.07 / -0.06 s |
| b lands on the couch | 16.67 s, 395 N s | 16.67 s, 395 N s | 0 |
| the couch slides / tilts | 0.67 m, 10 deg | 0.67 m, 10 deg | 0.01 m |
| a normal heavy blow cracks the pillar (gold) | hit 750 N s, crack 10.55 s | 750 N s, 10.55 s | 0 |
| the boosted final blow, gold | 2,324 N s, break 22.00 s, 3 pieces | 2,324 N s, 22.00 s, 3 pieces | 0 |
| its pieces at rest | (0.40, -4.34), (-0.95, -4.68), (-0.98, -6.21) | (0.39, -4.33), (-0.95, -4.68), (-0.98, -6.21) | <= 0.01 m |
| the same blow, wooden fighter | 798 N s, crack only | 798 N s, crack only | 0 |

Times are from the end of the lab's intro (`intro_ms`).

## Limb targets and conventions

- **Stage frame**: metres (millimetres on the wire), x to the audience's right, y up, z toward the audience;
  right-handed. Hex `[q, r]` (pointy-top axial, fractions allowed) -> `x = 1.7320508 (q + r/2)`, `z = 1.5 r`, y = 0.
- **Unreal**: centimetres, Z up, left-handed. `UE = 100 x (x, z, y)`; this swaps two axes, which is exactly the
  handedness change, so nothing is mirrored. A rotation quaternion `[qx, qy, qz, qw]` becomes `(-qx, -qz, -qy, qw)` in
  UE's (X, Y, Z, W). A fighter's yaw (radians, about +y, 0 = facing +z, toward the audience) faces
  `(sin yaw, cos yaw)` in UE (X, Y): an actor whose forward is +X has UE yaw `90 - degrees(yaw)`.
- **Body tracks** are the pose of the object's authored origin (its `at` on the floor, with its `yaw` and `rot`
  already in the first key's rotation). A piece or shard track is the pose of that part's centre.
- **Root track**: the fighter's root on the floor (where the feet are), yaw / pitch / roll, the lean (the body tilts
  into its acceleration so the centre of mass stays over the support: tilt the body about the feet toward world +x
  by `lean_x` radians and toward +z by `lean_z`; at most 0.3), flags (air, hung, low).
- **Limb targets**: `[t0, t1, in, out, limb, kind, x, y, z, nx, ny, nz, on, part]`: from `t0` the client's two-bone IK
  blends the limb's end (`hand_l hand_r foot_l foot_r`) toward `(x, y, z)` over `in` ms, holds, and blends out over
  the last `out` ms up to `t1` (panel-local ms). `n` is the surface normal (x 1000): align the palm / sole to it.
  `kind`: `plant` (a foot on a surface that is not the floor plane: a step, a top, a tilted lid; only surfaces under
  45 degrees), `vault` / `mantle` (hands on a top), `wallrun` / `wallkick` (a sole on a wall), `strike` (the striking
  limb on the point it hits: an object's surface point, or the foe's body part anchor as the foe stands at that
  moment). `on`: the object id and its scene part index, or `"a"` / `"b"` and the body part, or `""` (the floor). A
  target on an object that moves (a crate) is where the surface is at the time of the contact.
- **Foe body-part anchors** (root-local metres, x left, y up, z forward): head (0, 1.58, 0.04), chest (0, 1.30,
  0.10), belly (0, 1.04, 0.10), legs (0.09, 0.55, 0.06). Sent in `duel.scene.sim.fighters.*.anchors`.
- **Head**: `look[]` = `[t_ms, what]`, what = `foe`, `path` (look where it is going) or an object id.
- **Arcs**: `[t0, t1, apex_mm, impulse_Ns]`: flights, for squash / dust at take-off and landing.

## Wiring

The seam is `seam.rs` (`live_with_sims`, `live_present`, `scene_sim`; also `panel_sim` for a caller that keeps the
state itself). The module doc there has the exact lines. In short, once `backend/duel-weapons` has merged:

```rust
// crates/fn7-plugin/Cargo.toml: fn7-duelsim.workspace = true
// panel.rs since(), given the lock's skins: instead of out.extend(b.panels.iter().skip(from).cloned());
let scene = b.scene.as_ref().unwrap_or(&Value::Null);
out.extend(fn7_duelsim::live_with_sims(&b.duel.id, scene, [skin_a, skin_b], b.duel.seed, &b.panels, from));
// present.rs duel_json: "sim": fn7_duelsim::live_present(&d.id, scene, [skin_a, skin_b], d.seed, &b.panels),
```

Two decisions for the lead: (1) the skins: `skin` is display-only today (`skin::of(w, d)`, never in the state), so a
skin change mid-duel would re-bake every panel with the new mass (consistent for everyone, but history changes);
locking them needs a field in the bout (a schema bump). (2) `since()` also feeds recorded outcomes
(`challenge.open.panels`): attach the sim only to the live stream and `present`, or gate it on a new schema, so old
recordings replay with 0 differences.

## Tests

`cargo test -p fn7-duelsim` (budgets: `cargo test --release -p fn7-duelsim`):

- `tests/model.rs`: primitive volumes, centres and inertia against closed forms; compound mass, centre, tipping
  angle of the lab's pillar, crate and desk by hand; parallel axis; fighter masses; the generator's materials in the
  table; the lab scene's affordances; our trig against libm; the RNG against the lab's draws.
- `tests/engine.rs`: rest is exact; a drop lands and sleeps; a low push skids the distance Coulomb predicts, a high
  one tumbles; a tall thing topples and a squat one does not; damage by strength (no mark, crack, break of the struck
  drum and everything above it; a pinned drum slides only v^2 / 2a); shatter into shards of the same volume; a stack
  falls when its support goes; a kick lands on the surface with the fighter's mass (gold kicks a crate twice as far);
  a running fighter shoves a crate; a landing on a couch weighs by skin; an arrow stops at a jar, a rocket blasts
  crates apart.
- `tests/bake.rs`: the pinned hash; two runs equal, skin and seed matter; incremental = from scratch by every entry
  point; the wire round trip and the reduced tracks within tolerance of every raw sample; limb targets on real
  surfaces (vault / mantle hands on the top, wall-run / wall-kick soles on the wall); gold breaks the pillar and wood
  only cracks it; heavier fighters fly lower arcs; a rules-shaped duel through the live seam; size and time budgets.

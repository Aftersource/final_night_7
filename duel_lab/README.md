# Duel Lab

A browser (three.js) playback of one duel, used to tune the challenge box before the Unreal client builds it.

Run: `cd duel_lab && python3 -m http.server 8177`, then open http://localhost:8177/ .
Keys: space = play/pause, R = replay, left/right = step 0.1 s, **D = debug overlay**. The slider scrubs.
URL: `?t=<ms>&paused=1&a=<skin>&b=<skin>&duel=<file>&shot=<name>&debug=1`.

**Time travel is the rule.** Everything on screen is a pure function of the timeline time T. The fighters' paths are
resolved ahead of time from the script; the rigid bodies are simulated once at load and recorded (the bake); a frame
only reads those records and solves IK on top. Scrub backwards and the pillar un-breaks exactly.
`tools/rewind_check.mjs` proves it (every bone, body and the camera, revisited out of order);
`tools/bake_check.mjs` bakes twice and compares hashes bit for bit (the page does the same after every bake).

## Files

| file | what it is |
|---|---|
| `duel.json` | the duel: fighters, `scene` (what an LLM authors), `panels` (the script) |
| `main.js` | timeline, HUD, the frame |
| `lib/m.js` | vectors, quaternions, 3x3, hex grid, seeded rng, hash. No three.js: the engine also runs in node |
| `lib/materials.js` | the materials table, what each skin is made of, fighter mass |
| `lib/shapes.js` | primitives: volume, centre of mass, inertia, collision vertices, signed distance, ray, shards |
| `lib/props.js` | derived properties of a scene: mass, COM, inertia, support polygon, tipping, surfaces, affordances |
| `lib/physics.js` | the rigid-body engine |
| `lib/paths.js` | the script resolver: segments to trajectories and timed contacts |
| `lib/bake.js` | runs the engine over the whole duel, records it, plays it back at any T |
| `lib/ik.js`, `lib/fighter.js` | two-bone IK; the mannequin, its mass from the mesh, pose per frame |
| `lib/sceneview.js`, `lib/stage.js`, `lib/camera.js`, `lib/debug.js` | drawing, mood, camera fit, overlay |
| `lib/scene_schema.js`, `tools/make_scene.mjs` | the scene JSON schema + validator; ask Claude for a scene |
| `tools/` | `shot.mjs` / `sheet.mjs` (headless screenshots into `shots/`), `bake_check.mjs`, `rewind_check.mjs`, `probe.mjs`, asset converters |

## The scene (`scene` in the duel file)

```json
{ "place": "...", "time_of_day": "dawn|day|dusk|night", "backstory": "two to four sentences",
  "objects": [ { "id": "pillar", "kind": "pillar", "at": [1, -2], "yaw": 0, "fixed": true, "on": "<id it rests on>",
                 "note": "one line", "parts": [ { "shape": "cylinder", "size": [0.3, 0.75], "offset": [0, 0.725, 0],
                 "rot": [0, 0, 0], "material": "stone", "fill": 1, "color": "#...", "glow": 0 } ] } ] }
```

- `at` is a hex `[q, r]` (fractions allowed): x = 1.732 (q + r/2), z = 1.5 r metres, y up, the audience at +z.
- `shape`/`size`: box `[w,h,d]`, cylinder `[r,h]`, cone `[r,h]`, sphere `[r]`, tetra/octa/ico/dodeca `[r]`. `offset` is the
  part's centre from the object's origin on the floor; `rot` is degrees. `fill` is the solid fraction (a crate is 0.12).
- `fixed`: built into the ground. It never moves, but its parts are pre-cut segments that can break off.
  Loose objects are settled onto the floor, or onto the top of the object named by `on`.
- Materials (`lib/materials.js`): density, friction, restitution, strength, brittle + shard count, colour / roughness /
  metalness, impact word and sound class. stone, marble, wood, iron, gold, cloth, glass, bone, ice, obsidian, clay, leaf, rope.

`node tools/make_scene.mjs --place "..." --time night --backstory "..." --facts facts.txt --out scene.json` asks
`claude-sonnet-5-5` (structured output on this schema; credentials from the environment only) and validates the result.
`--dry [--check ../duel.json]` prints the prompt and validates a file without calling anything.
`node tools/probe.mjs` prints what the engine derives from the scene.

## What is derived at load

Per object: volume, mass (density x volume x fill), centre of mass, inertia tensor, bounds, the support polygon and the
tilt at which the COM passes its edge. Per scene: walkable tops, vertical faces (stacked drums merge into one run),
undersides, ledges, bars, and from those the **affordances**: `vault` (a top 0.5-1.2 m, under 1.7 m deep), `mantle`
(a ledge 1.2-2.6 m), `perch` (a free top from 1 m), `wallrun` (`up`: a face 2 m tall; `along`: 2 m wide), `wallkick`,
`slide` (a table-height underside over clear floor), `swing` (a thin bar 1.9-3.3 m up; detected, no path uses it yet).

## The engine and the bake

Compound bodies of convex primitives; gravity; contacts at vertices against the floor, anchored and loose bodies;
Coulomb friction; restitution; 10 sequential-impulse iterations; sleeping. Fixed 120 Hz step, one seeded rng, no clock.
Damage is per part: a blow of J N·s adds J / (strength x smallest cross-section) once it is over 15 % of that; 40 % cracks
the part, 100 % breaks it. A brittle part shatters into its pre-cut shards; a part of a compound breaks at its joints
(an anchored thing loses the struck segment and everything above it, a loose thing comes apart); a single tough block only cracks.

Fighters have mass from the skin's material (`SKIN_BODY`) and the figure's volume, measured from the skinned mesh with
per-bone shares. They enter the simulation three ways: **strikes** (limb share of the mass x strike speed x `power`, at
the real surface point found by a ray from the fighter), **scripted contacts** (the momentum change of a take-off,
landing, wall-kick, wall-run step, ledge grab or vault hand goes into the object), and **their bodies** (two spheres on
the scripted path that hit with the fighter's real mass but are never pushed back). Jump apex scales with
sqrt(43 kg / mass), strike and take-off speed with the fourth root: gold jumps half as high and hits three times as hard.

The bake stores every body's transform at 60 Hz plus the event log (hit, push, touch, crack, break, shatter, whiff).

## The script (`panels[]`)

Each panel: `caption`, `shot` (`wide`, `side`, `low`, `over_a`, `over_b`, `close_a`, `close_b`), optional `move`
(`push` = slow push-in, `whip` = the camera whips across the fight, at `whip_ms`), `play_ms`, `input_ms` (when typing
locks), `flip` (initiative), `hp` after the panel, `a` / `b` with `line` and `segs[]`, and `contacts[]`.

A seg: `clip`, `ms`, `loop`, `speed`, `at` (clip start ms), `to` (hex), `face` (`foe` | `path`), `aim` (object id to face),
and `path`:

| path | fields | what it resolves to |
|---|---|---|
| `dash` | `to` | a ground move; the feet follow steps and tops under the line |
| `leap`, `flip` | `to`, `h`, `onto`, `lead`, `back` | a ballistic arc under g; `h` is the apex for a 43 kg figure. `onto: id` lands on that object's top. A flip off a wall-run is a back flip |
| `knock` | `to`, `h`, `onto`, `spin` | thrown by the foe: a ballistic arc, tumbling |
| `drop` | `to` | step off whatever you stand on |
| `vault` | `over: id`, `to`, `hand` | hand planted on the top, body rolled over it |
| `wallrun` | `on: id`, `steps`, `rise` or `to` | up a tall face leaning back (ends clinging; follow with `flip`/`leap`/`knock`), or along a wide one if `to` is given |
| `wallkick` | `on: id`, `to`, `h`, `kick_h` | to the wall, one foot on it, off on an arc |
| `mantle` | `on: id` | grab the nearest ledge, hang, pull up, stand on top |
| `perch` | | hold the spot and ride what is under the feet |
| `slide` | `under: id`, `to` | low and laid back along the floor |

A contact: `t_ms`, `by`, `text` (the burst word), then either `at` (`head`/`chest`/`belly`/`legs` on the foe) or
`on: <object id>`, with `kind` (`hand`, `kick`, `heavy`, `slam`), `limb` (`hand_l/r`, `foot_l/r`: that end effector is
IK'd to the contact point at `t_ms`), `power`, `y` (strike height). `miss: true` = no shake, `ranged: true` = no IK.

What stays scripted: which clip plays, where a fighter goes and when, who gets hit and for how much (`hp`), and the
timing of a reaction to a physics event (tune it against the event log from `node tools/bake_check.mjs`).

## Assets

- `assets/mannequin.glb`: the Meshy figure-drawing mannequin, rigged to the UE5 mannequin bone names.
- `assets/anims/*.glb`: 35 clips from Meshy's library, stripped to skeleton + animation, named by our pose names.
- `assets/skins/*`: wood, obsidian, gold, dragonscale, butterfly (PBR maps on the same UVs).

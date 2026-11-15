# The Earth simulator (state schema 13)

> "we need to get the god powers better - can you lmk how to do a low res voxel weather landmass moving earthquake
> volcano onscreen etc etc at whatever the proper level of zoom is, the timeline evolves fast so we'd want evolving
> simulations at whatever works for our engine, it should handle like a bunch of things at the same time as the earth
> [blows] from everyone overloading on entropy" / "again, simulator on backend, timetravel, keep as much as you need for
> keyframes or whatever so it happens mostly the same at time travel without too much data" (Chris, BUILDLOG)

The god powers act on a **server-side, deterministic, low-resolution voxel field** of the whole planet. Every mega,
mini, god-tier power and physical ramification starts a **phenomenon** (a storm, a volcano, a rupture, a drifting
plate ...); each phenomenon is a small cellular automaton on the field, all of them stepped together every beat with
sub-steps, interacting through the cells they share. What the field does reaches demigods, crowds, settlements, roads,
shrines, encounters and the DM's memory **through the existing rules**. The client draws instanced hex columns and
particles from compact deltas, and any past beat is rebuilt from keyframes.

Room setting `earth_sim_enabled` (default on for new matches; planet arenas only; absent in settings recorded before
schema 13, so every older recording replays unchanged). Code:

| Where | What |
|---|---|
| `crates/fn7-earth` (`grid.rs`, `sim.rs`, `codec.rs`) | the pure core: grid, base terrain, the field, the phenomena, one beat's step, keyframes. Knows nothing of a match |
| `crates/fn7-plugin/src/earth/rules.rs` | the hooks: casts / god powers / ramifications feed it; it steps once a beat; the consequences go back through the rules |
| `crates/fn7-plugin/src/earth/here.rs` | the earth underfoot (schema 17, §12): one short line of what the field holds at a place, for every prompt that acts there; a duel stage's ground |
| `crates/fn7-plugin/src/earth/view.rs` | what clients get: `earth` per beat, `game.earth`, `game.earth_at`, the board, the globe tiles, finer levels, the ASCII map |
| `crates/fn7-model/src/earth.rs` | what is stored (`dm.earth`: `EarthState`) |
| `crates/fn7-tools/src/bin/earth_view.rs` | `fn7-earth-view`, the ASCII viewer |
| `crates/fn7-server/tests/earth_balance.rs` | the balance harness (ignored; release) |
| tests | `fn7-plugin/src/earth/tests.rs` (the core), `earth/rules_tests.rs` (in a match), `fn7-tools` schema tests |

Hooks (each a line or two): `casts::process` (a landed cast → an input), `beat::resolve` (a god-tier signature → an
input; the step, after the society and before the DM; `public.earth`), `dm::settlements_step` (ruin lowers a target),
`dm::cell_state` (an observed hex carries its field values and the field's weather), `phases`/`present` (the summary).
From schema 17 every LLM prompt that acts in a place reads the field there (§12).

## 1. The field

**The grid.** Every regions-level hex (120 km, `fn7_model::hexgrid`, the same `face:level:q:r` keys as everything
else) of the six cube faces: **27,354 cells** (1,999 of them on a plate boundary), numbered once (`u16` index), with their six neighbours across the faces'
seams. Built once per process.

**The base terrain is a pure function of the geodata** (no per-match data, nothing stored). There is **no elevation
data** in the repositories, so the heightmap is derived: land or sea from the 1° land mask (`data/geo/land_mask_1deg.bin`,
seven samples per cell), height from hops to the coast, hops to the nearest PB2002 plate boundary (mountains on land,
ridges at sea) and two octaves of hashed noise. **16 levels**: 0..7 sea floor (7 = shelf), 8..15 land (15 = high
peaks); **sea level is 8**. Base material: rock (h ≥ 13), sand (15–32° latitude, inland, low noise), ice (polar), soil
(the rest), water (sea); base fuel (vegetation) by latitude band. An ASCII of the base world: `cargo test -p fn7-plugin
look_at_the_world -- --ignored --nocapture`.

**A cell's dynamic layers** (one byte each, all zero = the base, which is never stored):

| Layer | Meaning |
|---|---|
| `dh` (i8) | height change in levels (cooled lava, crater, raised or sunk land, rift, ridge) |
| `mat` | material override (0 = the base's) |
| `water` | standing water; 32 = one level |
| `lava` | molten lava; 32 = one level |
| `ash`, `fire`, `snow` | on the ground |
| `cloud`, `charge` | it rains above 112; it strikes at 200 |
| `heat` (i8) | above / below normal |
| `stress` (i8), `crack` | fault stress; cracks (≥ 128 = an open rift) |
| `ruin` | what settlements here have suffered |
| `dfuel` (i8) | vegetation change (burnt, eaten, grown) |
| `swarm`, `shake`, `plume` | swarm density; shaking this beat; ash and smoke in the air |

**The top material** a client draws (derived, never stored): water, rock, soil, sand, ice, forest, lava, fire, ash,
snow, flood, rift, swarm (in that index order: `tops` in every answer).

**Wind** is a pure function of (seed, beat, cell): westerlies between 30° and 60°, easterlies elsewhere; in a quarter of
the beats a seeded 30° × 30° patch (changing every 4 beats) blows another way; strength 1..2, +1 under heavy cloud.

**Finer levels** (villages 3 km, houses 30 m, people 3 m) are derived from the coarse cell under them, rulebook
chapter 4.3's extrapolate-on-zoom: relief ±7/16 of a level around the coarse height (never across the shoreline), and
each patchy layer (fire, water, lava, snow, ash, swarm, crack) is present in a fine hex with probability (coarse
value / 256) by the hex's own hash, so the same hexes burn every time you look and a growing fire only adds hexes; a
mini's own layer is certain within its small radius of where it was cast. A pure function of (field, seed, hex).

## 2. The phenomena

A phenomenon is `{id, kind, cell, origin, born, power, radius, dir, dur, owner, cause, parent, a, b, tier}` (16
integers). **Power** starts at magnitude × 16 × `earth_energy_scale` (a mini × `earth_mini_scale`, default 0.5) and
the magnitude is the cast **as it landed**, so a mega near a fault line arrives already strengthened (chapter 13). A
mega's reach is its radius in km / `earth_km_per_cell` (400), 1..4 cells; a mini is one cell. At most
`earth_max_phenomena` (48) live; a newcomer stronger than the weakest replaces it, else it is refused (`refused`).

Which phenomenon a power starts: `lightning` → lightning, `storm_clouds` → storm (blizzard where it is cold),
`volcano`, `earthquake`, `flood`, `wildfire`, `meteor`, `raise_land`, `sink_land`, `grow_forest` → forest, `swarm`.
Free-form powers and Entropy's ramifications (world mods born this beat) are read from their words (`volcan`, `lava` →
volcano; `tsunami` → tidal wave; `snow`, `frost` → blizzard; ...). God-tier: Nature → forest + swarm, Chance → storm +
lightning, Death → blizzard, Consequence → earthquake, Entropy → earthquake plus a volcano and a quake on two seeded
fault cells.

Each kind's rule (e = magnitude, P = power; per sub-step unless "per beat"):

| Kind | Starts | Each step | Ends |
|---|---|---|---|
| storm | 4 + e/2 beats | adds P/2 cloud (halving per ring); moves one cell downwind per beat, losing 1/8; +4 over warm sea (< 35°), +8 over fire | beats run out or P < 8 |
| blizzard | as storm (a storm cast where it is cold becomes one) | as storm, plus cold (−P/8 heat) and snow (P/8) on land | as storm |
| lightning | strikes the cell, plus e/3 more in its reach (mega) | a strike: +16 heat, +32 cloud, sets fuel ≥ 24 alight if dry | 1 beat |
| flood | 2 + e/3 beats | adds P/2 water (× 2 on the coast) on land; loses 1/8 a beat, +4 under heavy cloud | beats run out or P < 8 |
| wildfire | 3 + e/2 beats; lights the centre (96 + P/2) and half the reach | feeds burning cells P/8 per beat; the fire itself spreads by the cell rule (§3) | beats run out |
| volcano | 3 + e/2 beats; a mega lifts the cone 1 (2 at e ≥ 9) level of rock; hard shaking | P/4 lava, P/2 plume, P/8 heat at the vent; a mega spills P/8 into a seeded neighbour; lights a seeded cell within 2; −1/8 a beat | erupts, then **sleeps** 12 beats (a quake can wake it) |
| earthquake | ruptures **along the nearest plate boundary** within 4 hops: 2 + 2e cells (mega), 1 + e/2 (mini); far from any boundary only the epicentre, mildly. Cracks 48 + P/2 on a fault (P/2 off), stress −96, shaking 96 + 2P | an aftershock with odds 0.6 a beat on a random ruptured cell | 2 + e/3 beats |
| meteor | a mega digs a crater (−1 level, −2 at e ≥ 9; the ring around it too at radius ≥ 2), lights fuel in the reach, plume, ruin 255, shaking; a mini cracks and scorches one cell. **Offshore**: steam, and a tidal wave (e ≥ 4) | — | 2 beats |
| tidal wave | from an offshore quake (e ≥ 4, at least half the rupture at sea) or meteor | its front moves one ring a step across open water; every land neighbour of the front up to height 9 is flooded to P and ruined P/8; P × 13/16 a step | P < 12 or 12 rings |
| raise land | a mega lifts the cell a level a beat (its ring once), stresses the faults under it; a mini heals ruin and cracks | | 1 + e/3 beats (≤ 3) |
| sink land | a mega lowers the cell a level a beat (the coast around it too); a mini floods and cracks it | | as raise |
| forest | grows fuel (8 + e, halving per ring), clears ash and ruin; rock and sand under ≥ 48 fuel become soil | | 3 + e/2 beats |
| swarm | P/2 swarm on its cell (P/8 on the ring for a mega); moves a cell a beat toward the richest neighbour | | 4 + e/2 beats |
| plate drift | from a great on-fault quake (mega, e ≥ 9) or Entropy: a patch (radius 1–2) moves one cell along the boundary every `earth_plate_beats` (3): its columns shift, **a rift opens behind** (−2 levels, crack 255, lava, stress), **a ridge rises ahead** (+1 level) | | 2 + e/3 moves |

**Volcanic land is new land**: 32 units of cooled lava are one level of rock (each cooled unit is a 1/32 draw), so a
sea vent that keeps erupting builds an island (`earth.land_rose`).

## 3. The cells' rules and the interactions

Every dirty cell reacts once per sub-step (reads first, then writes, so no cell sees a half-updated neighbour; flows
into neighbours are applied after, in a fixed order). With m = (16 + 4 × Entropy level) × `earth_spread_scale` / 16
(Entropy level = entropy / 20, 0..5):

- **Fire** burns fuel (2 + f/32 a step), heats, ruins, and **lights a neighbour** with odds f × fuel × m × wind × dryness
  / 2²⁴ per step (a full fire in forest: 0.19 across the wind, 0.29–0.49 down it, 0.10 against it); burnt ground does
  not burn again until half its vegetation is back; it dies out without fuel (leaving ash).
- **Water** flows downhill to every lower neighbour's surface (up to w/3 × m a step), pools, evaporates (more where it
  is hot or cracked); the sea takes what reaches it; deep water ruins.
- **Lava** flows downhill when ≥ 16, cools 2 + lv/16 a step into rock that raises the column, sets fuel alight and
  ruins.
- **Cloud** rains above 112 (snow where cold), charges above 128, rides the wind; the cloud's own charge strikes at 200.
- **Plume** rides the wind, falls as ash, cools. **Snow** melts where warm or low. **Swarm** eats fuel, spreads toward
  more, dies of fire, cold, heavy rain and hunger. **Shaking** passes in a step, leaving ruin and cracks. Heat, stress,
  cracks below 128, ruin and vegetation relax back toward base.

| Interaction | Rule |
|---|---|
| flood quenches fire → steam | fire in water ≥ 8 (or at sea) goes out; f/2 cloud rises (`quenched`) |
| lava + water → rock + steam | lava in water, snow or sea cools 4× faster (rock sooner), lv/2 cloud, the water boils off (`steamed`) |
| quake near a volcano triggers it | a rupture within 2 cells of a volcano adds P/2, 2 beats, and wakes a sleeping one (`woke`) |
| storm feeds flood | rain adds water on land (snow where cold) |
| fire feeds storm | a burning cell puts up f/16 cloud, charge and plume; a storm over fire gains power |
| ash cools heat | ash ≥ 32 lowers heat; plume cools the air |
| lava sets fires | a lava cell with fuel ignites |
| offshore quake / meteor → tidal wave | (§2) |
| great quake → plate drift | a mega on a fault at e ≥ 9 |
| rain stops fire spreading, blizzard freezes water | cloud ≥ 160 blocks ignition; water at heat ≤ −48 becomes snow |

## 4. Entropy

The world's meter (0–100) is read each beat and recorded with the inputs. Its level L = entropy / 20:

- **Spread** × (1 + L/4) (×2.25 at heat death), through m above.
- **Sub-steps**: `earth_substeps` (1) + 1 at L ≥ 3, + 1 at L ≥ 4, + 1 at L = 5: the timeline runs faster when the world
  is coming apart.
- **The world's own phenomena**: every fault's tension builds 4L a beat; [0, 1, 1, 2, 4, 8] rolls a beat on random
  fault cells with odds ∝ stress × [0, 40, 90, 160, 256, 600] / 65,536 × `earth_spontaneous_scale`; each that fires
  is a quake (a volcano at L ≥ 2 with odds 0.4, a drifting plate at L ≥ 3 with odds 0.15) of magnitude 2 + L, and
  relieves tension. Freak weather anywhere with odds L/20 a beat. They are `cause: "entropy"`, owner none.
- **Heat death cascades** (L = 5): every live phenomenon flares × 1.5 and lasts a beat longer, every beat.

## 5. Consequences, through the rules

Nothing here bypasses the rules: damage is a `Damage` proposal through `merge::run` (so armour that resists the
element halves it), crowds get `MoodShift` and `Damage` proposals, settlements move by the DM's own stepping.

| What | Rule |
|---|---|
| demigods | each standing demigod in a hazardous cell takes one bounded hit a beat: fire × `earth_fire_damage` (0.6), lava × 1.2, flood × 0.5, quake × 0.5, a strike 0.4, blizzard 0.2, swarm 0.2, summed, capped at `earth_damage_cap` (1.0), × `earth_damage_scale`; named for its worst part ("burned by the wildfire"), so gear resistances apply |
| crowds | the `earth_crowd_effects` (6) worst-hit crowds a beat (each every other beat): fear +0.15, anger +0.05, hope −0.05, devotion +0.05 (× hazard × `earth_crowd_scale`); at hazard ≥ 0.5 deaths (≤ 5) |
| settlements | a settlement's level target drops one per 96 points of ruin in its cell (× `earth_ruin_scale`, at most 3), three if the sea took it; the DM's stepping then lowers it one level at a time |
| roads | a rift, lava ≥ 32, deep water or the sea over either end cuts a road (its wear falls under the threshold; it fades by the rules' own step) |
| shrines | hard shaking (≥ 160), lava, drowning or an impact topples a shrine: half its worshippers leave |
| encounters | a rupture, a crater or risen land uncovers a marked hex (one a beat, within the DM's cap) |
| new land | risen land is ground; remembered |
| memory | `earth.quake`, `earth.eruption`, `earth.drowned` ("the coast of Sumatra drowned at beat 31"), `earth.land_rose`, `earth.plate_moved`, `earth.impact`, `earth.wave`, `earth.omen` (the world's own), `earth.road_cut`, `earth.shrine_toppled`, `earth.settled` (the cap); at most `earth_events_per_beat` (6) a beat, public, so they are `dm_events` too |
| observed hexes | `place.observed` carries the hex's field values and the field's weather word |

## 6. Time travel and size

**The simulation is a pure function of (match seed, settings, the ordered inputs with their beats, the Entropy meter
at each beat).** Integer arithmetic throughout (u8/i8 layers, fixed-point settings in sixteenths, seeded `mix` hashes
for every draw: no floats in the step, no wall time). `dm.earth` (`EarthState`) keeps:

- `inputs[]`: every input `[beat, kind, cell, lon_e3, lat_e3, energy, radius, owner, cause, tier]` (35 bytes each);
- `entropy[]`: the meter (a byte) per stepped beat;
- the field now: the phenomena (16-integer rows) and the **sparse dirty cells** (packed: varint index deltas + layer
  masks + the non-zero bytes, base64);
- **keyframes** every `earth_keyframe_beats` (K = 16): the dirty cells as a delta against the previous keyframe, the
  phenomena, `next_id`, `tension`.

Any beat N is rebuilt by loading the newest keyframe ≤ N and re-simulating ≤ K beats of the recorded inputs
(`Sim::at_beat`, `game.earth_at`). Tests: state at N from a keyframe == from beat 1, bit for bit (every N of a heavy
match); a fork (the state at N plus different casts) continues identically until its own casts and diverges only by
them; the in-match `earth_at` equals the hash each beat's outcome published; recorded matches with the simulator on
(fn7-sim and a private server) replay with 0 differences.

**The cap**: past `earth_max_cells` (1,500) dirty cells the calmest (by a weight: what is happening counts double what
was left; ties by a seeded hash) settle back to base, down to 15/16 of the cap, remembered as one `earth.settled` entry.

**Measured** (release, Apple M-series, `measure_sizes_and_speed`, 60 beats each):

| Scenario | Step µs mean / worst | Dirty cells mean / worst | Phenomena max | `earth` per beat mean / worst | `dm.earth` per snapshot mean / end | Beat 55 from keyframe / from beat 1 |
|---|---|---|---|---|---|---|
| calm (4 minis + a mega, Entropy ≤ 12) | 11 / 86 | 26 / 95 | 2 | 587 B / 1.3 KB | 0.8 KB / 2.4 KB | 38 / 80 µs |
| busy (a mega + a mini a beat, Entropy → 60) | 243 / 1,256 | 850 / 1,481 | 15 | 8.2 KB / 15.7 KB | 22 KB / 49 KB | 0.9 / 5.2 ms |
| heavy (4 megas a beat, Entropy → 100, heat death) | 538 / 1,552 | 1,157 / 1,493 | 48 | 17.2 KB / 27.7 KB | 42 KB / 88 KB | 6.9 / 16.8 ms |

The heavy match's final state: inputs 300 = 10.5 KB, cells 18 KB, keyframes 56 KB. **Recording impact**: a live
recording holds ~2.3 MB per snapshot (350 MB over ~150 snapshots), so even a heavy match adds about 2 % (≈ 42 KB per
snapshot); a calm one ≈ 0.03 %. The delta alone per beat (only cells whose drawn values changed): calm 208 B, busy
5.3 KB, heavy 10.6 KB (packed; per-column base64 is 1.7×, JSON 3×).

## 7. The client contract

### Per beat: `round.outcome.public.earth`

```json
{ "beat": 31, "hash": "9f2c…", "dirty": 1131, "substeps": 2,
  "delta": { "n": 425, "enc": "pack", "data": "<base64>" },
  "phenomena": [ { "id": 12, "kind": "storm", "at": [101.2, -2.9], "cell": "4:regions:-35:32", "cell_index": 18211,
                   "origin": [100.5, -3.5], "radius_cells": 2, "radius_km": 520, "power": 96, "intensity": 6.0,
                   "age": 3, "born": 28, "beats_left": 4, "owner_seat": 1, "cause": "cast", "tier": "mega",
                   "velocity": { "dir": 0, "to": [103.1, -2.2], "to_cell": "4:regions:-34:32", "cells_per_beat": 1.0 } } ],
  "phenomena_total": 27,
  "ended": [ { "id": 7, "kind": "flood" } ], "strikes": ["4:regions:-36:35"],
  "did": { "spawned": [26], "spontaneous": 1, "ignitions": 9, "quenched": 2, "steamed": 1, "rock_made": 3, "melted": 0,
           "land_rose": [], "drowned": ["4:regions:-35:31"], "woke": [], "evicted": 0, "refused": 0 } }
```

- `delta` holds **only the cells whose drawn values changed** this beat (height, top exactly; a layer when its high
  four bits change, so a client copy is never more than 15/255 stale on a layer). **Packed block**, per cell in index
  order: varint (index − previous index), height byte (0..15), top byte, varint layer mask, then one byte per set
  layer in `layers` order (`water lava ash fire snow cloud charge heat crack ruin fuel swarm shake plume`; `heat` signed).
  Varints are LEB128. `view::unpack_cells` is the reference decoder.
- `phenomena`: the **20 biggest** (power × reach), each with a **stable `id`**; `phenomena_total` says how many live.
  Interpolate across a beat with `velocity.to` (storms, blizzards, swarms, plates), `front_cells`/`front_km` (a tidal
  wave's ring), `state` (a volcano erupting or dormant), `ruptured_cells` (a quake). `ended` lists the ids to fade.
- `hash` = `present.earth.hash`: compare with yours after a reconnect; fetch the cells only if they differ.

### The board: `game.earth {cell, level?, rings?, since?, coarse?}`

The Unreal board is a disc of **22 rings around a centre hex at the current level** (people 3 m, houses 30 m,
villages 3 km, regions 120 km): **1,519 hexes**. `game.earth {cell: "<face>:<level>:<q>:<r>"}` answers exactly that disc
at the key's level (or `level`), one **9-byte record per hex in the client's ring order** (ring 0; then ring k = 1..22
as `HexCell::ring` walks it: start at (q − k, r + k) and take k steps in each direction (+1, 0), (+1, −1), (0, −1),
(−1, 0), (−1, +1), (0, +1)), base64:

| byte | field |
|---|---|
| 0 | `h16`: height in 256ths of the 16 levels (128 = sea level; regions: level × 16 + 8; finer levels add seeded relief) |
| 1 | `top`: index into `tops` |
| 2..6 | `water`, `lava`, `ash`, `fire`, `snow` |
| 7 | `heat` (signed) |
| 8 | `crack` |

A hex past its face's edge takes the cell holding its centre on the next face, so every ring position has a record
(the client never has to skip). Height per hex is byte 0: the scenery builder reads it straight from the record.
Size: **13.7 KB raw, 18.2 KB base64** for the records; the whole answer adds the head and up to 20 phenomena (nearest
the centre first, only those whose reach touches the board): 18.8 KB with none, 21.8 KB with 9 (measured on the private
server). `max_phenomena: 0` leaves them out.

**Updating only what changed**: `since: <beat>` returns only the records that differ from that beat's: `changed`
(u16 LE ring positions) + their records in `data`. Typically a few hundred bytes to a few KB. The client asks with
the beat its board was drawn at after each `round.outcome` whose `delta` intersects its board. At the regions level
the delta's indices are board cells directly; below, `coarse: true` returns the regions cell index under each board hex
(u16 LE) once, and the client intersects that set with the delta's indices.

### The globe: `game.earth {tiles: 4, since?, layout?}`

The client's globe is not hexes: rings of latitude every 4°. `tiles` answers the whole planet in **rings of latitude
`deg` tall, rows from the north pole down, west to east in each; row y has max(1, round(360/deg × cos lat)) tiles**
(4°: 45 rows, **2,578 tiles**, ~445 km), `row_cols` giving each row's count. Each tile is the same 9-byte record: the
mean height of the regions cells whose centres fall in it, the most dramatic top (lava > fire > rift > flood > ash >
swarm > snow), else the most common, each layer's maximum, the largest heat. **38 KB** (23 KB raw); `since` sends only
the changed tiles (≈ 270 tiles, 11 KB, five beats into a heavy match). `layout: "grid"` gives a full 90 × 45 raster.

### Other forms

- `game.earth {}`: every regions cell that differs from base (packed, as the delta), with `field`; `bbox` a box of
  regions cells; `base: true` adds `globe` (h, top, fuel of all 27,354 cells, base64); `grid: true` adds the grid
  (face, q, r per index), once per session, to map indices to keys.
- `hexes: true` with a centre below regions: the verbose JSON hexes (every layer, weather word, forest).
- `game.earth_at {beat}`: the whole field (dirty cells and every phenomenon) at the end of a past beat, rebuilt from a
  keyframe; `resimulated_beats`, `from_keyframe`.
- `present.earth` and the PublicView's `earth`: `{beat, hash, dirty, tops{}, phenomena (20), phenomena_total,
  keyframes, first_beat, grid}`.

### How Unreal should draw it

- One **instanced static mesh of a hex column** per board (1,519 instances); per-instance custom data = the 9 bytes
  (h16 → Z scale, top → material index into an atlas, layers → shader blends: water sheen, lava emissive, ash
  darkening, fire flicker, snow cover, heat shimmer, cracks decal). On each beat, write the new record as the "to"
  state and lerp from the "from" state over the beat.
- **Particles** per phenomenon id (budget ~20 emitters / 50k particles: the server already sends the 20 that matter):
  storm/blizzard = cloud + rain/snow volume moving along `velocity.to`; lightning = bolts at `strikes`; volcano = plume +
  lava fountain while `erupting`; quake = camera shake scaled by `intensity` within `radius_km`, dust along the rupture;
  meteor = trail then flash then crater; tidal wave = an expanding ring at `front_km`; plate drift = slow slide, rift
  glow; swarm = a boid cloud. Fade an emitter when its id appears in `ended`.
- The globe: sample the 4° tiles' records onto the globe mesh (colour by top, displacement by h16, emissive by lava and
  fire).

### A worked frame (beat 21 of the private-server run, Sumatra, `fn7-earth-view`)

```
~~~~~///&&&&&&.....~~~       ~~~~~...."""~~    ~      / rift: a plate drifting along the Sunda trench
  ~~~~~//.........~~     ~~~~~"""""""""~~~~~          & fire: wildfires on the rifted coast
   ~~ ~~~.~......&~~~   ~~""""""""""""""~~~    ~      W the wildfire's centre, P the plate patches
         ~  WWW~~~..~~~  ~~~~~"""..""~~"""""""~~
           ~~~/////&&&~~~~~~~~~~~~~~~~~~~""""""~
 ~~~~~~...  PPP   &&&&&""~~~""~~~~~  ~~"""""~~~
```

## 8. Settings

All under `earth_*` (docs/schemas/settings.schema.json; `GET /sim/settings`): `earth_sim_enabled`, `earth_max_cells`
(1,500), `earth_max_phenomena` (48), `earth_substeps` (1), `earth_keyframe_beats` (16), `earth_plate_beats` (3),
`earth_km_per_cell` (400), `earth_spread_scale`, `earth_spontaneous_scale`, `earth_energy_scale` (1), `earth_mini_scale`
(0.5), `earth_damage_scale` (1), `earth_damage_cap` (1), `earth_fire_damage` (0.6), `earth_lava_damage` (1.2),
`earth_flood_damage` (0.5), `earth_quake_damage` (0.5), `earth_storm_damage` (0.4), `earth_cold_damage` (0.2),
`earth_swarm_damage` (0.2), `earth_crowd_scale` (1), `earth_crowd_effects` (6), `earth_ruin_scale` (1),
`earth_events_per_beat` (6).

## 9. Balance

`crates/fn7-server/tests/earth_balance.rs`: 128 seeds × 60 beats, 4 seats, rules only (no LLM), the simulator off and
on, through `simapi::stats::run_seed` (as `POST /sim/compare` runs them):

| | off | on |
|---|---|---|
| mean beats played | 58.4 | 58.3 |
| world ended early (last standing) | 7 (5 %) | 6 (5 %) |
| first elimination, mean beat | 30.2 | 29.0 |
| eliminations per match | 6.78 | 6.73 |
| eliminations the field caused | 0 | 35 (of 861) |
| champions by seat 0/1/2/3 | 27 / 42 / 26 / 33 | 34 / 41 / 28 / 25 |

The field takes a share of the killing without making matches shorter, crueller or less fair (seat spread is within
seed noise; the champion changes on 33 of 128 paired seeds). The built-in AI casts sparingly (≈ 3 sim inputs in a
60-beat AI match), so this measures the field's background effect; a table of humans all casting megas reaches heat
death in 3 beats at the default `entropy_scale` (the private-server run: 8 casts a beat, Entropy 44 → 88 → 100) — that
is Entropy's own rule, not the field's.

## 10. Tools

- `fn7-earth-view <recording.jsonl | state.json> [--from B --to B --step N] [--bbox w,s,e,n | --around lon,lat |
  --region pl:N] [--span deg] [--cols --rows] [--ansi]`: an ASCII (or ANSI colour) map of the top materials for a beat
  range, past beats rebuilt from the newest snapshot's keyframes.
- `FN7_BALANCE_SEEDS=128 cargo test --release -p fn7-server --test earth_balance -- --ignored --nocapture`.
- `cargo test --release -p fn7-plugin measure_sizes_and_speed -- --ignored --nocapture`.

## 11. Real and approximated

Real: plate boundaries (PB2002), land and sea (1° mask), fault-aware ruptures, downhill flow, wind bands, every number
above. Approximated: the heightmap (derived, no elevation data), materials and vegetation (latitude bands + noise),
the sea's depth (hops from the coast), fine levels (seeded, consistent, not simulated), the globe tiles (aggregates).

## 12. The earth underfoot: the field in every prompt (state schema 17)

> "for the weather effects on the board raising lowering all of that ... it's like populous right? real-time change,
> and then the effects of that change radically altering the prompt for that area if anyone is in it, and so on? i
> don't quite get how it's going to perfectly integrate" (Chris, BUILDLOG)

Yes: the field changes in real time (one step a beat, §2–4), and from schema 17 every LLM prompt that **acts in a
place** is told what the field holds there, so the model's narration and choices change with the ground. The line is
built deterministically from the stored field (`crates/fn7-plugin/src/earth/here.rs`): the cells as they stood at the
end of the last stepped beat, the live phenomena and the input log, integer thresholds only, no draw. Prompts are
asked before the beat's own step, so "now" is the beat being played; "N beats ago" counts from it.

**One place's line**: at most 3 clauses, worst first, each cut to a few words; nothing at all when the cell is base or
every layer is under its threshold (a calm prompt is byte for byte what it was). In order:

| Clause | When | Words |
|---|---|---|
| lava | lava ≥ 8 | "molten lava running" (≥ 64) / "cooling black lava, still glowing", + "steam hissing off it" (water, snow or sea) or "smoke"; "the eruption began N beats ago" (a volcano that reached the cell) or "the rift opened ..." |
| drowned | base land, now under sea level | "the sea has swallowed this land", "the wave came / it sank / the impact ..." |
| fire | fire ≥ 32 | "a wildfire roaring through" (≥ 128) / "fires burning", "started / lit by lightning / lit by the impact N beats ago" |
| flood | land, water ≥ 48 | "knee-deep" (48) / "waist-deep" (96) / "floodwater over a man's head" (160) / "floodwater frozen solid" (heat ≤ −48), "the flood came / the wave struck / the storm's rain began / the land sank N beats ago" |
| quake | crack ≥ 48 or shake ≥ 32 | "an open rift splits the ground" (≥ 128) / "the ground is cracked", "and still shaking" (shake ≥ 16 or a live quake within 4 cells), "after the quake N beats ago" |
| snow | land, snow ≥ 48 | "snow on the ground" / "deep snow" (96) / "snowed in, drifts deep" (160), "bitter cold" (heat ≤ −32), "the blizzard came ..." |
| swarm | swarm ≥ 64 | "a swarm thick in the air, eating everything green" |
| height | dh ≠ 0 (not a rift) | "the land here rose N levels" / "sank N levels" / "a crater N levels deep", "since beat B" (the first raise, volcano, plate, sink or impact that reached it) |
| ash | ash ≥ 48 or plume ≥ 96 | "grey ash lying thick" / "ash falling from a dark sky" |
| ruin | ruin ≥ 96 | "the buildings around here in ruins" |
| vegetation | dfuel ≤ −48 / ≥ 48 | "burnt black, nothing green left" / "a forest grown up out of nowhere" |
| sky | cloud ≥ 112, or heat ≥ 48 | "a thunderstorm overhead" (charge ≥ 96) / "a blizzard blowing" / "snow falling" / "heavy rain" / "rain"; "scorching heat" |
| volcano | a live volcano within 2 cells | "a volcano erupting nearby" |

"Since" and "ago" come from the live phenomenon or the logged input of the right kind whose reach (radius + 1; a
quake + 3, along its fault) covers the cell; the newest one for "ago", the first for "since". Ground the field has
carried far from any cast (a fire that spread, water that ran downhill) gets no time.

**Who gets which line** (at most 2 earth lines per prompt, ≤ 200 characters each; a line names who stands there when
the people in one prompt stand in different cells):

| Prompt | Where it reads | Line |
|---|---|---|
| a hand's orders (`hand`) | the demigod | "The earth here, now: waist-deep floodwater, the flood came last beat." |
| the act resolver (`resolve`) | each acting body's cell (in the world lines, after Entropy and the lasting mods) | "The earth here, now: ..." or "The earth where Ash's Left Hand stands, now: ..." |
| a subagent's step (`step`) | its body, else its demigod | "The earth here, now: ..." |
| a god's ruling (`god`) | the invoker, and the ruling's first target with a place (else the place it names) | "The earth where Ash stands, now: ...", "The earth where Brix stands, now: ..." |
| Entropy's ramifications (`ramify`) | where each cast lands, with what the cast **meets** there by §3's rules | "Where Brix's Left Hand's Volcano lands, the earth now: waist-deep floodwater, the flood came last beat; it meets: lava into this water will boil it to steam and make new rock." |
| a general (`general`) | its army | "The earth where your army stands, now: a wildfire roaring through." |
| the narrator (`narrate`) | the standing demigods | "The earth under Ash's Left Hand, now: ..." |
| a duel's deed (`duel`) | the duel's hex (the stage's ground at the lock) | "The ground they fought on: the ground is cracked and still shaking after the quake last beat." |

What a cast meets (`Ground::meets`): water (flood, storm, tidal wave) on lava → steam and rock; water on fire → the
fires drown in steam; a blizzard on floodwater → it freezes, on lava → steam; fire on floodwater → it drowns; under
heavy rain → it can't spread; on burnt ground → little to catch; on grown forest → it burns fast; a volcano or meteor
into water or snow → steam and new rock; a volcano on an open rift → magma pours up it; a quake within 2 cells of a
volcano → it can wake it; a quake on cracked ground → it splits wider; a swarm into fire → it burns, on burnt ground →
nothing to eat; raised land under floodwater → sheds it; sunk land on the coast → lets the sea in; forest on ash → fed.

**The duel's stage** (`duel.scene`, built at the lock): when the duel's hex is not calm the scene gains `ground`
(`{line, floor, hazards, slippery}`: floor one of `lava burning frozen flooded rift cracked snow ash ruins shaken`;
`slippery` for floodwater, ice, the sea or deep snow), the backstory ends "The ground: <line>.", and up to 4 hazard
props are added after every other object (so the rest of the stage is where it was): `lava` (a glowing pool; two when
molten), `fire`, `floodwater`, `ice`, `fissure`, `snowdrift` (vaultable), `rubble` (vaultable). They are ordinary scene
objects (lab primitives and materials: obsidian, clay, cloth, glass, ice, stone, with `color` and `glow`), so the
duel sim, the picker and the AI's typed lines ("I throw you into the lava") use them as they use a crate. The physics'
floor friction is unchanged (`slippery` is for the client's look and the minds).

Not given a line: the item engine's namer, the manifestation and summon minds, the picker (it sees the hazard props
in the scene's objects but its cache key is the line, not the ground) and the moderation pass; none of them acts in a
place. Encounters are resolved by the rules (and told by the resolver, which has the line).

**Replay**: everything above reads `World::earth_prompt_rules()` (schema ≥ `EARTH_PROMPT_SCHEMA` = 17); a schema-13..16
state builds the prompts and scenes it always did, byte for byte (tests in `earth/here_tests.rs`; release
`fn7-sim --replay` of schema-12 server recordings and a schema-16 earth-on sim recording: 0 differences).


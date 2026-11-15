# Duel moves: the list, the data, and the loop

How a typed duel line becomes a move and an animation in Final Night 7 (state schemas 12 and 14), what is recorded
about every line, and how Chris and Benito get players what they ask for. Client shapes are in `docs/CLIENT_API.md`
("AI duellists: typing scripts", "Words to moves and `clip_fallbacks`", "Duel weapons, hammer-space, projectiles and
the picker"); the rules are in `docs/DM_RULEBOOK.md` 21.8.

**Status at a glance**

| Piece | State |
|---|---|
| The list as data (`data/duel_moves.json`) | **loaded at startup** (`FN7_DUEL_MOVES`, else the file, else the compiled one), validated like the media style; the code tables are the compiled-in default (a test keeps the file equal to it) |
| The keyword mapper (words → class, clips, setups, leaps; typos) | built (`moves.rs`), in every schema-12 duel; schema 14 adds `shoot`/`chop` rows and weapon-generic words |
| One record per typed line (`duel_line`) + rollup CLI | built; schema 14: one record per line when its duel ends, with the picker's part and what was staged |
| The SQLite store and `/api/duel/lines` rollups | **built** (§2) |
| The LLM picker (§4.1) | **built**: a journaled side input, cache, settings, replay-exact |
| Clip trims on segs (`at`, `speed`) | **built** (schema 14); the trims themselves are still estimated by rule |
| Weapon classes, hammer-space draws, projectiles (§4.4-4.6) | **built** (schema 14) |
| The duel sim on live panels (docs/DUEL_SIM.md) | **wired** (schema 14: skins locked at the lock; live stream and `present` only) |
| Duel words (§5): the instant `stage`, only what is held or on the board (a whiff otherwise), up to 4 staged steps, fighter `look` and `usable` | **built** (schema 21) |

## 1. The list

`data/duel_moves.json` (print it: `fn7-duel-lines --list`):

```json
{ "version": "b9bdc8d3f2f5a2e2",
  "classes": [ { "class": "strike", "about": "a normal attack; from far it is a step in" }, ... 13 ],
  "clips":   [ { "clip": "flying_kick", "for": "strike", "about": "a flying kick, both feet off the ground",
                 "fallback": "kick", "len_s": 4.67, "at_ms": 0, "use_ms": 4670, "hit_ms": 2101, "trim": "estimated" }, ... 64 ],
  "paths":   [ { "path": "leap", "about": "a jump to a hex (h: apex)" }, ... ],
  "words":   [ { "words": ["jumpkick", "flying kick", "jump kick", "dropkick"], "class": "strike",
                 "clips": ["flying_kick"], "leap": true, "weak": false, "typos": true }, ... ] }
```

- `version`: a hash of everything else. It is in every data row (`list_version`) and on `duel.scene`, so a rollup knows
  which list a line was read with.
- `defaults[]`: each class's clips when the line names none. `weapons[]`: per weapon class, its clips per move class
  (`sets`) and what it shoots (`projectile`). `draw_clip`: the hammer-space clip.
- `clips[].for`: the classes the clip serves (`reaction`, `idle`, `outro` for the ones no line picks;
  `reserved: <weapon class>` for the 14 waiting for weapon classes). `fallback`: what a client without the clip plays
  (null for the 35 original ones).
- **Trims.** Several library clips are long. `len_s` is the measured length (34 clips so far); `at_ms` is where to
  start, `use_ms` how much to play, `hit_ms` the contact inside the used part. They are first guesses by rule (a clip
  of 5 s or more starts 35% in and uses 40%; a shorter one plays whole; an attack's contact is 45% into the used
  part) and marked `trim: "estimated"`: correct them by eye. Schema 14: every non-looping seg whose clip has a trim
  carries `at` (ms into the clip) and `speed` (`use_ms / ms`, kept within 0.5..2.5; when it would be faster, the
  played part is the one around the contact), and an attack's contact (`contacts[].t_ms`, the defender's reaction)
  is at the clip's `hit_ms`.
- `words[]`: the mapper's rows. `weak`: only the move when no other row matched (`knee` in "kick your knee").
  `typos`: may match within one or two edits in a command of at most four words.

**Editing without code.** The server reads the list from `FN7_DUEL_MOVES` (a path), else `data/duel_moves.json` in its
working directory, else the compiled one; a file that does not validate stops the server with the reason (every clip
one the rig has, `panel::CLIPS`, described once; every fallback one of the 35 original clips; every class one of the
13; every word row, default and weapon set naming known clips; every weapon class known; every trim inside its
clip). What can be edited in the file: descriptions (`about`: the picker reads them), fallbacks (sent to clients in
`duel.scene.clip_fallbacks`), word rows (`words`: the words, their class and clips, `leap`, `weak`, `typos`, `weapon`
for a generic attack word, `since`: the schema the row applies from), each class's `defaults`, the weapon classes'
clip `sets` and `projectile`, and the trims (`len_s`, `at_ms`, `use_ms`, `hit_ms`; set `trim` to `"measured"` once
checked by eye). The version is the content's hash: an edited file gets a new one, in every data row. A match records
the list in its ruleset when it is not the compiled one (`duel_moves.list`), and a replay installs it, so old matches
are read with the list they were played with. Restart the server to load an edit.

**Adding a clip** (in code; one commit):

1. `crates/fn7-plugin/src/panel.rs`: add its name to `CLIPS` and, if older clients must cope, `(name, fallback)` to
   `CLIP_FALLBACKS`.
2. `crates/fn7-plugin/src/moves.rs`: one line in `CLIP_INFO` (`name, classes, description`), its length in
   `CLIP_LEN`, and its name in the `clips` of the `TABLE` rows whose words should play it (or a new row: words,
   class, clips) and, if the class should use it without a word, in `defaults`.
3. `cargo run -p fn7-tools --bin fn7-duel-lines -- --list > data/duel_moves.json`, `cargo run -p fn7-tools --bin
   fn7-schemas`, `cargo test -p fn7-plugin moves`. The tests fail if a clip is undescribed, a fallback is not an
   original clip, or the file is stale.
4. Import the GLB in Unreal under the same name. A new clip changes what a schema-12 duel plays from then on; clips
   chosen are in the state, so recorded matches replay with the clips they had only while the tables' order is kept:
   **append, never reorder or remove** (a removal needs a schema gate).

## 2. The data: one record per typed line

Every line typed in a schema-12 duel (and every line an AI types) is recorded once, when a resolve is handed it, as a
trace event `duel_line` in the match's recording (`<THEATRE_RECORD_DIR>/<match>.jsonl`):

```json
{ "type": "event", "kind": "duel_line", "data": {
    "beat": 3, "duel_id": "duel:s9:0:1", "n": 4, "at_ms": 1794702116360, "by": "a", "demigod": "dg:0", "seat": 0,
    "ai": false, "player": "Chris", "account": null,
    "line": "jimpkick!", "normalized": "jimpkick", "list_version": "b9bdc8d3f2f5a2e2",
    "mapper": { "word": "jumpkick", "class": "strike", "clips": ["flying_kick"], "setup": [], "leap": true },
    "picker": null,
    "class": "strike", "result": "missed", "verdict": "fail", "damage": 0.0, "range": "close",
    "staged": { "clip": "flying_kick", "setup": [], "path": "leap" } } }
```

`mapper` is null when the line has no move word (it was read as in 0.8.1): those are the lines the list lacks.
`class` is what the rules made of it (range, bolts and armies gate the asked class). `staged` is what the panel
played.

**Schema 14.** A duel's records are written once it has ended (all its panels are out), so `staged` is what was
really played and `picker` is filled. A real row from the private-server run (`fn7-server/tests/duel_weapons.rs`):

```json
{ "beat": 12, "duel_id": "duel:s21:0:1", "n": 1, "seq": 22, "at_ms": 1794709068448, "by": "a", "demigod": "dg:0", "seat": 0,
  "ai": false, "player": "Ash", "account": null, "line": "I hit him hard", "normalized": "i hit him hard", "list_version": "d24da37ec8bd6819",
  "mapper": { "word": "hit", "class": "strike", "clips": ["jab", "punch_combo", "strike_side"], "setup": [], "leap": false },
  "picker": { "pick": { "move_class": "strike", "clips": ["roundhouse_kick"], "path": null, "object_id": null, "target_part": "head" },
              "pre": false, "cached": false, "latency_ms": 2, "tokens_in": 0, "tokens_out": 40, "cost_usd": 0.00002,
              "model": "claude-haiku-5-5", "error": null, "at_ms": 1794709068464,
              "used": true, "why": "came before the panel's input close" },
  "class": "strike", "result": "landed", "verdict": "success", "damage": 0.1, "range": "close",
  "staged": { "clip": "roundhouse_kick", "setup": [], "path": null, "object": null, "weapon_class": "one_handed", "draw": null, "panel": 1 } }
```

`picker.why`: known at arrival (cache); came before the panel's input close; came after it; no pick (timeout or
error: `error`); its clip does not serve the class the rules made of the line; its clip belongs to a weapon the
fighter does not wield. Tokens are the provider's (the OpenAI-compatible gateway reports output tokens only).

**The SQLite store.** The server also writes each record to `<THEATRE_DATA_DIR>/fn7/duel_lines.sqlite`
(`FN7_DUEL_LINES_DB` to move it, `FN7_DUEL_LINES=off` to stop it): table `duel_lines`, one row per (match, duel, blow
`n`), upserted, with the record's fields in columns (match, beat, duel, seat, account, player, raw and normalized
line, list version, mapper word / class / clip, picker class / clips / path / object / target / latency / tokens /
cost / used / pre / cached / error, resolved class, result, verdict, damage, range, staged clip / setup / path,
weapon class, draw) and the whole record as JSON (`row`). The pick cache is the table `pick_cache` (key, pick,
model). The recording stays the record of truth; the store is the queryable copy.

**`GET /api/duel/lines?by=line|clip|class|disagreement|unmatched[&ai=1][&limit=40]`**, key-gated (`?key=` or
`Authorization: Bearer`: `FN7_DUEL_LINES_KEY`, else `FN7_PROFILE_KEY`, else the admin token; none set: 403):
`{ "by", "lines", "ai", "rows": [...] }`, most frequent first. Rows by `line`: `{line, count, classes, clips,
no_move_word, picker_used}`; `clip`: `{clip, count, classes, weapon_classes, picker_used, avg_damage}`; `class`:
`{class, count, mapper_classes, picker_classes, successes, avg_damage}`; `disagreement` (the picker chose another
class than the mapper, or the mapper found no word): `{line, count, mapper_class, picker_class, mapper_clips,
picker_clips}`; `unmatched` (no move word): `{line, count, classes, picker_classes, clips}`. Human lines only
unless `ai=1`.

**Rollups** (reads recordings, writes nothing):

```
fn7-duel-lines runs/lan/recordings            # most typed lines, lines with no move word, move words read,
fn7-duel-lines runs/lan/recordings --json     # resolved classes, clip usage, asked-but-became, clips never played
fn7-duel-lines a.jsonl b.jsonl --top 40 --ai  # --ai counts the AI fighters' lines too
```

Recordings from before schema 12 have no `duel_line` events (their lines are in the `duel.blow` events).

## 3. The loop: getting players what they want

1. **Collect.** Play. Every line is recorded with the list version.
2. **Roll up** weekly (or after a playtest): `fn7-duel-lines runs/lan/recordings --top 40`.
   - *lines with no move word* → words the list lacks (add them to a row, or a new row);
   - *most typed lines* and *move words read* → what players reach for;
   - *asked for, but became* → classes players ask for that the range or their purse refuses (a rules question);
   - *clip usage* and *clips never played* → which animations earn their keep, which words need more clips.
3. **Rank** a request list: a new word for an existing clip (free, a table row), a new clip for an existing class
   (one Meshy generation on the same rig), or a new move class (a rules change: schema-gated).
4. **Add** it to the list ("Adding a clip" above), with its description and fallback, and regenerate the file.
5. **Import** into Unreal under the same name; older clients play the fallback.
6. **Live** with the next deploy; the list version in the rows changes, so before and after can be compared.

## 4. The picker, the list as data, weapons (state schema 14)

### 4.1 The picker (an LLM chooses from the list)

The designer asked for a list of options the agent can run through and select from, with no thinking mode needed for the
most part, and with the list right. all of it should be right there, as long as you collect data properly".

**The call** (`fn7_agents::duel_pick`): one request per typed human line, model `claude-haiku-5-5`, no thinking, effort
low (the Anthropic backend's defaults; temperature 0 is asked for, the backend sends none: current models take the
default), at most 300 output tokens. The system prompt says: read what the player means, pick only from the lists,
decide nothing else. The user message has the situation (range, initiative, stances, the foe's set defence, the
typist's weapon class and held items), the classes and the pickable clips with their descriptions (from the list:
everything but reactions, idles and outros), the paths, the scene's objects with what they afford, and the line
fenced as untrusted text. The reply's JSON schema has an **enum for every choice**, built from the list and the scene:
`move_class` (the 13), `clips` (1..3 of the pickable clips), `path` (dash, leap, vault, slide, wallkick or null),
`object_id` (the scene's ids or null), `target_part` (head, chest, belly, arms, legs). The reply is checked against
the same enums again.

**The replay problem, and the solution.** A blow is resolved inside pure, synchronous hooks: `on_instant`, the live
view, the resolve and a replay must compute the same blow and the same panel from the same inputs, and none of them
may wait for a model. So the pick is a **recorded side input ordered with the duel's actions**:

1. When a typed line arrives, the plugin looks the line up in the pick cache by (normalized line, range, the typist's
   weapon class, list version). A hit is journaled at once as a pick record stamped with the line's own `(received_at_ms,
   seq)`, ordered just before the line: the line's blow takes the pick's **class** (gated by range, bolts, armies and
   the weapon as any class) and its **staging**. (The weapon class is in the key because the clips a pick names
   depend on what is in hand.)
2. On a miss the line is resolved at once with the mapper's class, as before, and the server makes the call outside
   the hooks (a spawned task, `duel_picker_timeout_ms`, 700). Its answer, or its timeout or error (`pick: null`), is
   journaled as a pick record stamped with the server clock when it came back. A panel stages the line with the
   pick's clips, path, object and body part only if that stamp is at or before the panel's input close and the pick's
   first clip serves the class the rules resolved; otherwise the mapper's staging stands. A late answer still fills
   the cache (so the next time those words are typed they are picked at arrival).
3. Pick records are entries of the plugin's journal (`fields.duel_pick`; a client sending one is refused). So they are
   ordered with the duel's actions by `(received_at_ms, seq)` in the live view, survive a restart (the journal is on
   disk), and are handed to the resolve: the ones stamped at or before its batch's newest instant action (the mark it
   sets; later ones stay pending for the next resolve, exactly as the live view applied them). The resolve records
   them as the `duel_picks {beat, picks}` event; replay verification feeds that event back (`present::Feed`) and never
   calls a model. Each pick lives in the bout (`picks`), so snapshots, forks and resumes carry it.

Why not the LLM recorder: the call cannot be inside a hook (it would block the room), so the engine's LLM replay
(keyed by hook round and step) cannot serve it; and why not an internal room action: the engine has no way for the
server to submit one (and is read-only for this branch). The journal is the plugin's existing record of everything
between two resolves, with the same ordering rule as the actions themselves.

**Settings**: `duel_picker` (`on` | `off`, on), `duel_picker_timeout_ms` (700), `duel_picker_for_ai` (off: on, the
AI's typed lines are picked when the window they are typed in opens from an instant action; a pick back before the
line's Enter chooses its class). The room's `llm: false`, a server without an LLM, a replayed LLM or
`FN7_DUEL_PICKER=off` mean no picks. Tests: `fn7-plugin/src/picker_tests.rs` (fake gateway: the enums, a pick
staged before the close and replayed exactly from `duel_picks`, a timeout and the late answer cached, a cached pick
choosing the class, off / old / no LLM), `fn7-server/tests/duel_weapons.rs` (the real server).

### 4.2 Weapon classes (built)

`fn7_plugin::weapons`: `weapon_class` from kind and name by rules (launcher > bow > rifle > thrown > staff > shield >
hatchet-like one-handed > two-handed > one-handed words; a weapon whose words name nothing is one-handed; armour,
relics and artifacts only when their words name a weapon). Shown on items and on `look.loadout[]` lines. A fighter's
held wieldables are in its duel record (`f[].weapons`, best first), it wields the best at the lock (`wield`); a move
class plays its weapon class's clips (`weapons[].sets` in the list) for a generic attack word or a line with no move
word; segs carry the `prop`. Ranged weapons fight from far (no step in, a landed shot keeps the range, +0.5 damage at
far and mid, half in a grapple); a two-handed heavy +0.5.

### 4.3 Hammer-space (built)

"players can grab from hammer-space": a line with a draw phrase whose following words name a weapon draws it before
its blow. Something it holds: free, it wields it from then on (`draw.verdict: "drawn"`). Anything else: the item
engine's rules inside the duel: `manifest::odds` for the prop's tier (artifact when `duel_draw_sparks` are paid from
the purse, trinket otherwise; Death's favor, Entropy, the draws already made as repeats) turned into a d20 target
(`ceil(20 (1 - p)) + 6`, the duel's pressure), a seeded d20 (`<duel_id>:<n>:draw`; 20 always works, 1 never).
Success: a duel prop (no world item) that sets the weapon class for the rest of the duel. Failure: a fumble, the
initiative flips to the foe before the blow. At most `duel_draws` (2) per fighter per duel; sparks are paid at the
resolve.

### 4.4 Projectiles as mini data (built)

`duel.panel.projectiles[]` (`weapons::projectile`), one per staged ranged blow or invoke:

```json
{ "id": "p4.1", "kind": "rocket", "by": "a", "from": "a", "from_hex": [-1, 0], "to": "b", "to_hex": [1, 0],
  "t_fire_ms": 942, "t_hit_ms": 1086, "arc_h": 0.3, "speed": 24.0, "miss": false, "deflected_by": null,
  "on_hit": { "blow": 4, "impulse": 0.1, "burst": "KRA-THOOM", "aoe_hexes": [[1, 0], [2, 0], [0, 0], [1, 1], [1, -1], [2, -1], [0, 1]],
              "objects": [{ "id": "cairn_a", "impulse": 0.6 }] } }
```

All decided by the blow's verdict and the panel's seeded rng: a hit flies to the foe; a parry or a shield's guard
deflects it to a hex at the side; a miss lands on a scene object within 3 hexes of the foe or a hex beyond it. A rocket
bursts over the landing hex and its neighbours and knocks the objects in them. The contact lands at `t_hit_ms`
(`ranged: true`). The duel sim (`fn7-duelsim`) consumes `t_fire_ms`, `t_hit_ms`, `to`, `speed`, `on_hit.impulse`,
`on_hit.aoe_hexes`, `on_hit.objects`: stable names. Kinds: `arrow bullet rocket rock bolt orb`.

### 4.5 Not done

- Trims are still the rule's estimates; nobody has corrected them by eye.
- `duel_picker_for_ai` does not pick AI lines whose window was opened by a resolve (no instant action to carry the
  call); the picker does not detect draws (the rules do).
- Hammer-space props are duel-only: they never become world items. From schema 21 there are no new hammer-space draws
  (§5): a line uses only what its fighter holds and what is on the board.

## 5. Duel words: what a line names reaches the stage (state schema 21)

The designer asked why characters look like blank models in the duels. If the player says to pull out a sword, they should be able to
to see it pulling out a sword" / "i should be able to say hide behind that tree then pop up and slit his throat or duck
roll to the left ... then see that play out in real time" / "player will type roll left hit enter then quickly type jump
so then enter then character jumps on screen" / "i dont want u to be able to do anything only what items u have in your
inventory ... u can use all whats in the board and in ur inventory ... u could pick up dirt and throw in eyes etc".

All in `crates/fn7-plugin/src/duel_words.rs` (pure rules on the words), the bout (`duel.rs`: `uses`, `plan`,
`words_move`, `stage_of`), the panel (`panel.rs`: `stage_steps`) and the picker (`fn7_agents::duel_pick`: `steps`). The
gate is `World::duel_words_rules` (`fn7_model::dm::DUEL_WORDS_SCHEMA`, 21); a schema-20 duel plays exactly as before.

**5.1 Every Enter plays at once.** Every `duel.blow` carries `stage`: up to 4 steps `{clip, path?, object_id?, ground?,
target_part?, prop?}` from the keyword fallback (no model, no wait for the panel), next to the d20 verdict it already
had. Short commands map straight to clips: `roll left` / `roll right` → `evade_roll` with path `left` / `right` (`tumble
back` → `dodge_roll_back`, `back`), `jump` → `jump_attack` with `leap`, `run in circles` → `run` with `circle`, `charge` →
`run` with `in`, `duck` → `evade_stand`, `block` → `block`, `parry` → `parry_high`; an attack is its move's clip. A pure
movement line is still a blow: it resolves as an `evade` (a d20 for positioning). The panel stages the line later
(`move.steps`, or the picker's richer pick).

**5.2 Only what is held or on the board.** A fighter can use (`usable`, on the lock and `duel.scene.fighters`): the
items it holds (`Fighter::held`, from the state at the lock), the board's objects (the scene's trees, rocks, crates,
walls, pillars ...; not the flat hazard patches) and the hex's ground (`ground_of`: by biome, plus what the Earth
simulator left: snow, ash, mud ...). `duel_words::uses` reads what a line names:

- a weapon named as its own ("pull out a sword", "my obsidian blade", "draw my pistol"): the held item whose name shares
  a word with it, else one of the same weapon class. It is drawn (free, `draw.verdict: "drawn"`, `draw_from_nowhere`
  first) unless already in hand; the prop is THAT item: its real name, and the material and colour its name gives
  (`colour_of`: obsidian → black, golden → gold, silver, crimson → red, bone → ivory, ...: `prop.colour`,
  `prop.colour_hex`);
- a board object used ("hide behind that tree", "kick the crate", "throw a rock": the nearest of that kind; rock and
  boulder are one), or the ground thrown ("throw dirt in his eyes"): `move.uses`, and a throw flies (`move.throw`, a
  projectile `rock` or `dirt` with `thing`);
- named but not there ("I draw a pistol" with none held, "that cart" with no cart, sand in a forest): a **whiff**: the
  die is thrown, the verdict is `fail`, no damage, the caption says so ("you have no pistol", "no cart here"),
  `move.whiff {named, where}`, and the wit takes `unavailable` (−2). A line that uses a real thing takes `uses` (+1),
  which replaces the generic `item` tag. Nothing comes from hammer-space and nothing is summoned.

**5.3 Up to 4 staged steps.** `duel_words::plan` cuts a line into clauses ("then", "and then", "and", "before", commas)
and each into steps: to the cover (`run`, path `cover`, the object), hide (`kneel`), pop up (`rise` / `spin_rise`), a roll
or side step (`left` / `right` / `back`), a jump, a run in circles, a pick-up (`kneel` at the object or the ground) and a
throw (`throw_object`), and the blow (its clip; a slit or slash is `strike_slash`; the part from the words: throat →
`neck`). "hide behind that tree then pop up and slit his throat" → `run`/`cover`/tree, `kneel`, `rise`, `strike_slash`
at the neck. The blow is still one d20 and one matrix result: the steps are the staging. The panel plays them in order
in the blow's slot (`stage_steps`: `cover` runs to the object's far side from the foe, the last attacking step closes in
when the rules' range asks, its contact frame is the blow's), and a panel with steps plays 350 ms longer per step beyond
the first (up to 4800). The picker's reply gains `steps` (an enum list: clip, step path, scene object, part with `neck`)
in a schema-21 duel; its picks are cached apart (`|w` on the key) and journaled as before, so a replay feeds them back
and never calls a model.

**5.4 Looks.** Each fighter on the lock and on `duel.scene.fighters` has `look`: the demigod's look (loadout; images
on: `portrait_image`, `diorama_image`), `aura`, `held` (each item's name, kind, weapon class, material, colour, `icon`,
and when it has item art `art` and `image`), `figure_key` and `statue` (null from the plugin: the statue is at
`GET /api/assets/figures?name=`). Item art is passed through, never asked for by a duel.

Tests: `duel_words.rs` (the words), `duel_words_tests.rs` (the stage per command, held items and the whiff, a held
sword drawn, the board and the ground with the steps on the panel and the dirt flying, a replay, schema 20
untouched), `picker_tests.rs` (a pick with steps staged and replayed with no model call).

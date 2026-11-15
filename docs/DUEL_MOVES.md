# Duel moves: the list, the data, and the loop

How a typed duel line becomes a move and an animation in Final Night 7 (state schema 12), what is recorded about
every line, and how Chris and Benito get players what they ask for. Client shapes are in `docs/CLIENT_API.md`
("AI duellists: typing scripts", "Words to moves and `clip_fallbacks`"); the rules are in `docs/DM_RULEBOOK.md` 21.8.

**Status at a glance**

| Piece | State |
|---|---|
| The list as data (`data/duel_moves.json`, `fn7-duel-lines --list`) | built; generated from the code tables, checked by a test |
| The keyword mapper (words → class, clips, setups, leaps; typos) | built (`crates/fn7-plugin/src/moves.rs`), in every schema-12 duel |
| One record per typed line in the recording (`duel_line`) + rollup CLI | built (`fn7-duel-lines`) |
| 15 new clips in use, 14 more reserved (bows, guns, launcher, two-handed, hammer-space) with fallbacks | vocabulary and fallbacks built; the 14 are not emitted yet |
| Clip trims (`len_s`, `at_ms`, `use_ms`, `hit_ms`) | in the list, **estimated by rule**; segs do not carry them yet |
| The LLM picker, its settings, its cache | **designed below, not built** |
| The SQLite table and the `/api` rollup route | **not built** (the recording is the store today) |
| The list loaded from the file at startup (editable without code) | **not built** (the file is generated; the code tables are the source) |
| Weapon classes, hammer-space draws, projectiles | **designed below, not built** |

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

- `version`: a hash of everything else. It is in every data row (`list_version`), so a rollup knows which list a
  line was read with.
- `clips[].for`: the classes the clip serves (`reaction`, `idle`, `outro` for the ones no line picks;
  `reserved: <weapon class>` for the 14 waiting for weapon classes). `fallback`: what a client without the clip plays
  (null for the 35 original ones).
- **Trims.** Several library clips are long. `len_s` is the measured length (34 clips so far); `at_ms` is where to
  start, `use_ms` how much to play, `hit_ms` the contact inside the used part. They are first guesses by rule (a clip
  of 5 s or more starts 35% in and uses 40%; a shorter one plays whole; an attack's contact is 45% into the used
  part) and marked `trim: "estimated"`: correct them by eye. A client plays a seg of `ms` with
  `speed = use_ms / ms` from `at_ms`. The panels do not send `at` / `speed` on segs yet; the list is where they are.
- `words[]`: the mapper's rows. `weak`: only the move when no other row matched (`knee` in "kick your knee").
  `typos`: may match within one or two edits in a command of at most four words.

**Adding a clip** (today, in code; one commit):

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
played. `picker` is always null today.

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

## 4. Designed, not built

These were asked for while this branch was in flight. They are written down so the next branch can build them
without re-deriving the constraints.

### 4.1 The picker (an LLM chooses from the list)

One small call per typed human line: the model is handed the whole list (classes, clips with descriptions, paths),
the scene's object ids and affordances, the range, initiative and stances and the fighter's gear, with the line as
untrusted text, and returns a structured pick with enums built from the list (it cannot invent a clip):
`{move_class, clips: [1..3], path?, object_id?, target_part, intent_words}`. Settings `duel_picker` (off | on),
`duel_picker_timeout_ms`, `duel_picker_for_ai`.

The constraint that shapes it: **a blow is resolved inside pure, synchronous hooks, and panels are part of the
state.** `on_instant`, the live view, the resolve and a replay must all compute the same blow and the same panel
from the same inputs, and none of them may wait for a model. So:

- The blow is resolved at once from the mapper's class, as today. The picker may change the **presentation**
  (clips, path, object, target part) of the panel that stages it; if it may also change the class, the class must be
  fixed before the d20, which means the pick must already exist when the line arrives (only the cache can do that).
- The pick is a **side input**, like `duel_clock`: the server makes the call beside the plugin (as it does
  moderation), stamps the answer with the server clock, and hands it to the plugin as a journaled entry ordered with
  the duel's actions by `(at_ms, seq)`. A panel uses a pick only if its stamp is before the panel's window close;
  otherwise the mapper's. The resolve reads the picks from the journal, records them as a `duel_picks` event, and a
  replay feeds that event back (no model call on replay).
- Cache by `(normalized line, range, list version)`; a cached pick is known at arrival and can also set the class.
- The `picker` field of the `duel_line` record then carries the pick, whether it was used, its latency and tokens.

### 4.2 The SQLite table and the `/api` route

A table `duel_lines` (the record of section 2, one row per line) in `<data>/fn7/duel_lines.sqlite`, filled by the
server from the `duel_line` events as they are recorded, and `GET /api/duel/lines/rollup` (key-gated) serving the
CLI's JSON. The asset registry's database (another branch) is the better home once it has merged.

### 4.3 The list as an editable file

Today the code tables are the source and the file is generated. To let descriptions, fallbacks, trims and word
hints be edited without code: load `data/duel_moves.json` at startup, validate it (every clip known to the rig list,
every fallback an original clip, every class one of the 13), and put the list (not only its version) in the match's
recorded ruleset, because the mapper's reading decides classes: a replay must read old matches with the list they
were played with.

### 4.4 Weapon classes

`weapon_class` on every wieldable item (`unarmed | one_handed | two_handed | bow | rifle | launcher | thrown | staff |
shield`), derived by rules from its kind, name and material words, optional on the item, shown in payloads and in
the gear look's loadout; a fighter's class in a duel is its best held weapon's. The list gets clip sets per move
class and weapon class, falling back to the generic ones. The 14 reserved clips are for these sets.

### 4.5 Hammer-space

A fighter pulls an item it holds out of nowhere mid-duel, or with the right words one it does not hold ("i pull out
a rocket launcher"): an item-engine manifestation judged by the existing rules (cost, odds, a d20), resolved inside
the duel as a setup move. Success: the item exists (a trinket-tier duel prop unless paid for), the panel has a seg
`{clip: "draw_from_nowhere", prop: {item | kind, weapon_class, material}}`, and the fighter's weapon class changes
for the rest of the duel. Failure: a fumble, the initiative flips. Bounded by a setting (draws per duel).

### 4.6 Projectiles as mini data

Ranged moves add compact entries to the panel, all decided by the blow's verdict and seeded:

```json
"projectiles": [ { "id": "p1", "kind": "arrow|bullet|rocket|rock|bolt|orb", "by": "a", "from": "a", "to": "b",
                   "t_fire_ms": 900, "t_hit_ms": 1180, "arc_h": 0.4, "speed": 28,
                   "on_hit": { "blow": 4, "impulse": 0.5, "text": "THOOM", "aoe_hexes": 1 }, "miss": false,
                   "deflected_by": null } ]
```

A miss lands on a named scene object or a hex; a parry or a shield can deflect; a rocket has a small area that also
knocks scene objects. Ranged moves are legal at far and mid range and weak in a grapple, which needs new rows in
the interaction matrix (a rules change, schema-gated). The same struct could carry board-level casts between beats.

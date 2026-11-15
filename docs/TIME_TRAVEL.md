# Time travel: the whole data structure

Everything that lets a match be redrawn at the exact present (a client reconnects, a viewer joins, the server
restarts) or at any past moment (state at a round, a fork, a replay). One place, exact. State **schema 8** ("exact
resume") is the first with all of it, **schema 9** (rules 0.8.2) adds the duel panels, **schema 11** the looks (which
picture each demigod shows); older states keep their shapes and replay unchanged.

Code: `crates/fn7-plugin/src/present.rs` (the journal, the present, the side inputs), `duel.rs` (the bout), `panel.rs` (the scene and the panels; tests `panel_tests.rs`),
`crates/fn7-server/src/replay.rs` (replay verification). Tests: `fn7-plugin/src/resume_tests.rs`,
`fn7-server/tests/exact_resume.rs` (the real server: reconnect, viewer, `kill -9`, fork), `fn7-server/tests/recording.rs`
(`duels_summons_a_will_and_a_join_replay_with_zero_differences`).

## 1. The layers

| # | Layer | Where | Written | Holds |
|---|---|---|---|---|
| 1 | **State** (`World`) | the core's memory; `rooms/<room>.json` | by the plugin at Start, Resolve, Commit; on disk at each Commit | the whole game as of the last hook |
| 2 | **Journal** | the plugin's memory; `fn7/journal/<match>.jsonl` | at each accepted instant action | the instant actions since the last resolve |
| 3 | **Recording** | `recordings/<match>.jsonl` | by the core, as things happen | every hook, snapshot, event, LLM call, frame |
| 4 | **Event log** | `matches/<match>.events.jsonl` | by the core | the core's own events (phases, seats, submits) |
| 5 | **Sessions** | `sessions.json` | by the core | players and resume tokens |
| 6 | **Profiles** | `fn7/profiles.sqlite` | at match end | per-player history rows that point back into 3 |
| 7 | **Assets** | `fn7/assets/assets.sqlite` + `fn7/assets/{images,models,textures,animations}/…` | as each image is asked for, finished, refused or failed, and as each payload goes out | every generated picture and model with its file, exact prompt, timing and cost, and **where it was shown**: which match, which beat, on which demigod (§ 9a, [`ASSETS.md`](ASSETS.md)) |

All paths are under `THEATRE_DATA_DIR`. `FN7_JOURNAL_DIR` moves the journal; `FN7_JOURNAL=off` keeps it in memory only.

**The present = layer 1 + layer 2.** The state is everything up to the last resolve; the journal is everything
accepted since. Nothing else is needed to redraw the moment.

## 2. The state (`World`, layer 1)

One JSON value, opaque to the core (`fn7_model::World`; tables in docs/DATA_MODEL.md §2). The parts time travel reads:

| Field | Meaning |
|---|---|
| `meta.schema` | 11 for a match started by this server. Rules read it; 4..10 states behave as recorded |
| `meta.seed` | every die and every seeded pick derives from it and a query string, never from wall time |
| `meta.beat` | the beat (= the Theatre round). Set at the beat's Start |
| `demigods`, `ledger`, `intents`, ... | the board |
| `dm.memory[]` | the DM's place × time memory: every decision with its beat, hex and `query`. `duel.lock` and `duel.end` entries are the permanent record of a duel (`<duel_id>:lock`, `<duel_id>:end`), every d20 is `dice.rolled` |
| `dm.duels[]` | the duels under way: one **bout** each (below) |
| `dm.mark` | schema 8: `{at_ms, seq, resolved_ms, round_ms}` (below) |
| `dm.looks` | schema 11: each demigod's loadout and the image identity it shows; the item art granted (below) |
| `dm.earth` | schema 13: the Earth simulator: its input log, the Entropy meter per beat, the field now (phenomena + sparse dirty cells) and a keyframe every 16 beats (below) |

### The look (`dm.looks`, `fn7_plugin::look::LookBook`; schema 11)

Which picture a demigod shows is part of the state, so every snapshot, replay, fork and `present` shows the image that
was current at THAT beat. Written only at the end of a beat (`look::step`, rules only: no clock, no image service).

| Field | Meaning |
|---|---|
| `looks[]` | one record per demigod that ever held a loadout, in id order |
| `looks[].demigod` | the demigod |
| `looks[].held` | the hash of the loadout it holds now (`""`: nothing notable); kept for every demigod, the AI's too |
| `looks[].loadout` | the hash of the loadout its IMAGES show (`""`: the base identity images). A human's only |
| `looks[].since_beat` | the beat its images became these |
| `looks[].gear[]` | the pieces its images show: `{role, kind, material, tier, stem, blessing, name}` (the prompt is written from these and the demigod's name, never from items that may since have changed hands) |
| `looks[].portrait`, `.diorama` | sha256 of its portrait subject and its diorama subject (the image identity; `""`: base) |
| `looks[].lines[]` | every line of the pictured loadout (what a later loadout is compared with to say what was gained) |
| `looks[].made[]`, `.made_beat` | the gear pictures made for it this match (the cap) and the beat of the newest (the debounce) |
| `looks[].past[]` | its earlier looks, oldest first (the newest 8): `{loadout, since_beat, until_beat, gear[]}` |
| `art[]` | the item pictures granted this match (art keys, in the order granted) |

- **The image identity is the subject, not a media key.** A media key also depends on the generator and the style's
  reference files, which the plugin never sees. The same subject under the same generator and style is the same key,
  so a replayed, forked or scrubbed match resolves to the same cached files and generates nothing; under another
  model or style it is another (correct) key, as for every other image.
- **Every view derives the look from the state it is shown:** `public_state.demigods[].look`, the seat view, the
  admin's state at a round, a fork's first snapshot, `match.ended.summary.players[].demigods[].look` and `.looks[]`
  (the whole timeline). `look.changed` is a `dm.memory` entry (`look:<demigod>:<beat>`).
- **Pending is the server's, not the state's.** While a new image renders, the media layer shows the previous look's
  image and marks the new one `pending`. A snapshot shown later finds the image on disk and shows it at once.
- Gated on `meta.schema >= 11` and the room setting `looks_enabled`: older states have no `dm.looks` and no `look`.

### The bout (`dm.duels[]`, `fn7_plugin::duel::Bout`)

| Field | Meaning |
|---|---|
| `duel` | the lock: `id`, `beat`, both fighters as the duel saw them (`f[2]`: demigod, seat, name, health, items, lore, bolts), `at`, `cell`, `level`, `place`, `phase`, `seed`, `started_ms` (the challenge's receive stamp), `deadline_ms`, `ends_beat`, `leap` |
| `hp[2]` | health now |
| `flow` | range, initiative, both stances, the set defences |
| `n`, `typed`, `last_ms`, `last_seq`, `said`, `lines`, `invoked` | the counters the next blow's rules read |
| `applied`, `paid`, `opened`, `delivered_seq` | what earlier resolves already applied to the world |
| `blows[]` | **schema 8:** every blow event so far, in order, exactly as it was shown (`duel.blow`: die, modifiers, damage, hp, flavor, `anim`, `choreo`, its `counter`), with `at_ms` and `seq` |
| `full` | schema 8 marker: this bout keeps `blows` |
| `pauses[]` | schema 8: `[from_ms, to_ms]` spans its clock stood still; `duel.deadline_ms` already includes them |
| `scene` | schema 9: the duel's stage (`duel.scene`), built once at the lock from the seed and the hex |
| `panels[]` | schema 9: every panel so far (`duel.panel`), exactly as sent |
| `cycle` | schema 9: where the panel cycle stands: `closes_ms` (the newest panel's input close), `next_ms` (when the next panel starts), `fed` (blows already staged), `pos` (both fighters' hexes), `hp`, `range`, `initiative`, `done` |

A bout is a pure function of its lock and the ordered blows: `Bout::step(world, settings, side, text, at_ms, seq)`.
The d20 of blow `n` is seeded by `meta.seed` and `"<duel_id>:<n>"`. Timing reads only the stamps.

**Panels** are a pure function of the bout's blows and the stamps that closed each window
(`panel::advance(bout, now_ms, tick)`): a duel line, a `fields.duel_tick` (journaled and recorded like any instant
action) or a resolve's newest stamp. A panel's content depends only on the blows stamped before its close, never
on when it was computed. So the live view, the resolve, a restart's recovery, a replay and a fork all hold the same
`panels[]`; a paused room moves `cycle.closes_ms` / `next_ms` with the deadline.

### The mark (`dm.mark`)

| Field | Set | Meaning |
|---|---|---|
| `at_ms`, `seq` | each resolve | the newest **instant** action a resolve has been handed. An instant action stamped later, in `(at_ms, seq)` order, is not in this state yet |
| `resolved_ms` | each resolve | the newest receive stamp of any action in the last resolve's batch (its only clock) |
| `round_ms` | each Start, server only | the server's clock at this beat's Start (a recorded side input; absent in simulations). An AI's challenge takes it as its lock stamp |

### The Earth (`dm.earth`, `fn7_model::earth::EarthState`; schema 13)

The field is a **pure function of (seed, settings, the ordered inputs with their beats, the Entropy meter at each
beat)**, integer arithmetic only, so the state keeps just enough to restart it: `inputs[]` (35 bytes each), `entropy[]`
(a byte a beat), the phenomena and the dirty cells now (sparse, packed), and **keyframes** every
`earth_keyframe_beats` (16), delta-encoded against the previous one. Any beat is rebuilt from the newest keyframe at or
before it by re-simulating at most 16 beats (`game.earth_at {beat}`; 38 µs calm to 6.9 ms in a heat-death match,
release). Tested: a beat from its keyframe equals the same beat from beat 1 bit for bit; a fork continues identically
until its own casts; every beat's published `hash` is what `earth_at` rebuilds; matches recorded with the simulator on
replay with 0 differences. Size: 0.8 KB per snapshot calm, 22 KB busy, 42 KB (88 KB at the end) in a heat-death match,
about 2 % of a live recording at worst. See [`EARTH_SIM.md`](EARTH_SIM.md) §6.

## 3. The journal (layer 2)

One entry per accepted instant action (`on_instant`): cast, summoning, god power, challenge, duel line, duel tick.

```json
{ "boot": 1234, "beat": 4, "seat": 0, "text": "", "fields": { "duel": { "duel_id": "duel:s7:0:2", "text": "..." } },
  "seq": 9, "at_ms": 1794690930925, "seed": 4242, "event": { "event": "duel.blow", "...": "the public phase.event" } }
```

- `seq`, `at_ms`: the core's arrival stamps (room-wide order, epoch ms). `event`: what everyone was shown.
- `boot`: the server process that accepted it. `beat`: the state's beat on arrival.
- Kept per match (the recording run id; the seed when nothing records), entries older than 3 beats dropped at each
  Start. The file is appended as entries arrive and deleted when the match is summarized.
- An entry is **pending** for a state when it is stamped after the state's mark and arrived within one beat of it.

**The live view** is computed, never cached: the state's bouts, plus the built-in AI's challenges of the beat, plus
every pending entry applied in order with `duel::apply_act`, the same function the resolve runs. So what is shown live
and what the resolve decides are one computation on the same inputs.

## 4. The present (`public_state.present`, `game.present`)

Built by `Journal::present` on every `public_view`, so it is in every `match.snapshot` (a resume, a viewer's join)
and every `round.committed`. `game.present` (public, also `GET /api/rooms/{code}/query/present`) returns the same
object.

```json
"present": {
  "server_time_ms": 1794690927811,
  "beat": 2,
  "live": true,
  "mark": { "at_ms": 0, "seq": 0, "resolved_ms": 1794690925100, "round_ms": 1794690925300 },
  "paused_since_ms": null,
  "events": [ { "event": "summoning", "seq": 5, "received_at_ms": 1794690926350, "beat": 2, "...": "..." },
              { "event": "challenge.lock", "duel_id": "duel:s6:0:2", "seq": 6, "received_at_ms": 1794690926360, "beat": 2 },
              { "event": "duel.blow", "n": 1, "seq": 7, "received_at_ms": 1794690926371, "beat": 2 } ],
  "duels": [ {
    "duel_id": "duel:s6:0:2",
    "a": { "demigod": "dg:0", "name": "...", "health": 10.0, "max_health": 10.0, "portrait_prompt": "...", "...": "..." },
    "b": { "demigod": "dg:2", "...": "..." },
    "lock": { "event": "challenge.lock", "...": "the lock as it was shown" },
    "hp": { "a": 10.0, "b": 9.8 },
    "range": "close", "initiative": "a", "stances": { "a": "neutral", "b": "off_balance" }, "set": { "a": null, "b": null },
    "blows": [ { "event": "duel.blow", "n": 1, "text": "...", "roll": 14, "damage": 0.2, "hp": {}, "choreo": {}, "anim": {},
                 "counter": {}, "at_ms": 1794690926371, "seq": 7 } ],
    "n": 1, "typed": 1, "ko": null, "opened": false,
    "started_ms": 1794690926360, "deadline_ms": 1794691166360, "ends_beat": 97, "pauses": [],
    "clock": { "at_ms": 1794690927811, "elapsed_ms": 1451, "remaining_ms": 238549, "running": true },
    "over": false } ]
}
```

- `events`: the public event of every pending entry, in arrival order. Everything since the last resolve.
- `duels`: every open duel in full. `blows` is the whole timeline: the state's, then the pending ones. Schema 9:
  `scene` and `panels` (every panel so far). The panel playing now is the last with `starts_at_ms <= server_time_ms`;
  seek to `server_time_ms - starts_at_ms` in it.
- **Seeking:** a blow's offset in the duel is `at_ms - started_ms` minus the `pauses` before it; the offset to play from
  now is `clock.elapsed_ms`. `server_time_ms` is the server's clock when the view was built (compare with `sys.pong`).
- `paused_since_ms`: the room stands still since then (nobody connected, or restored and not started): the clocks in
  `duels[].clock` are frozen at that moment and `running` is false. The next Start moves the deadlines
  (`round.started.public.duel_clock`).
- `live: false`: the state asked about is not the match's live one (the admin's state at a round, a fork before its
  first Start). It is shown as it was: no pending entries, clocks frozen at its own last stamp.
- The rest of the snapshot is the core's: `round`, `phase`, `deadline`, `window` (the open slow window), `started`,
  `brief`, `outcome`, `you`.

## 5. The recording (layer 3)

JSON lines, one `theatre_trace::Record` each (`THEATRE_RECORD=full`).

| `type` | What | Time travel use |
|---|---|---|
| `run_start`, `ruleset`, `run_end` | settings, seats, rules version; winners and summary | identity |
| `snapshot` | the whole state after `on_match_start`, `on_round_start`, `resolve_round`, `commit_round`, `on_seat_takeover`, `fork` | **snapshots per beat**: three per beat, compared byte for byte on replay; state at a round; the fork's start |
| `hook` | each hook's input and output: `resolve_round` (`actions[]` with `seq`, `received_at_ms`, `instant`, `windowed`; `ctx.windows_closed`), `on_instant` (`action`, the public event), `validate_action`, `on_action_begin`, `query`, `on_seat_takeover`, `fallback_outcome` | the inputs replay re-feeds; **every instant action with its arrival stamp, twice** (as it arrived, as it was resolved) |
| `event` | core events (`instant.submit`, `window.*`, `phase.*`, `run.resumed`, joins) and plugin events (`duel_lock`, `duel_blow`, `summon_streak`, `cast_streak`, `beat`, `ledger_row`, ...) | timeline |
| `event` `duel_clock` | plugin, at each Start on the server: `{now_ms, pause: [from_ms, to_ms] or null}` | **side input**, fed back on replay |
| `event` `journal_recovered` | plugin, at the first resolve after a restart: `{beat, actions[]}` | **side input**, fed back on replay |
| `llm_call`, `http` | every LLM request and answer | replay answers LLM calls from these |
| `frame` | every protocol frame both ways, with recipients | attract mode re-sends them; exact client view |

A duel's timeline is rebuilt from the recording alone: the lock is the challenge's `on_instant` record
(`received_at_ms` = `duel.started_ms`), each typed blow is a duel line's record (`received_at_ms` = the blow's `at_ms`),
pauses are the `duel_clock` events. The test asserts the rebuilt list equals the state's `blows`.

## 6. Who reads what

| Operation | Reads | Result |
|---|---|---|
| **Client reconnect** (`auth.resume`) | state + journal (memory) | `match.snapshot` with `public_state.present`: the exact present |
| **Viewer joins mid-duel** (`room.join {viewer}`) | the same | the same snapshot, `you: null`; or `game.present` |
| **Server restart** | `rooms/<room>.json` (state at the last Commit on disk), `sessions.json`, journal file | the match paused at the next beat's Start; the present as before the kill; the first resolve is handed the undelivered instant actions; open duels' clocks stood still for the downtime |
| **Room pause** (every human gone; the core pauses at the next round) | the plugin's clock at Commit and at the next Start | open duels' clocks stood still for the pause |
| **Replay** (`fn7-sim --replay`) | recording: the first snapshot, each round's `resolve_round` input, takeovers, LLM answers, `duel_clock`, `journal_recovered` | every snapshot re-computed and compared; 0 differences |
| **State at a round** (`GET /admin/matches/{id}/state?after_round=N&view=public\|raw\|seat:N`) | the recording's `commit_round` snapshot | the state through the game's views; `present.duels` has the open duels as of that commit, `live: false` |
| **Fork** (`POST /admin/fork {match_id, after_round}`) | the same snapshot, seat statuses and scores from events | a new private room, paused; open duels are open with their whole timeline; at its first Start the time since the forked moment is a pause, so each duel has the time it had left |
| **Attract mode** | recording frames | recorded public messages re-sent |
| **What a demigod looked like** (`GET /api/assets/at?match=&beat=&demigod=`) | the asset registry's `uses` rows (layer 7) | per picture kind, the image that was on that demigod at that beat, with its file path, URL and sha256 (§ 9a) |
| **Profiles** (`game.history`, `/api/profiles/*`) | `matches` rows: `match_id`, `recording` (file name), `forked_from`, and the `TimeTravel` links `/admin/matches/{id}/timeline`, `/state?after_round={n}`, `POST /admin/fork` | a player's history points at the recording to scrub or fork |

## 7. Guarantees

1. **A reconnecting client or a joining viewer gets the exact present** in one message: beat, phase and its deadline,
   the open slow window, every demigod, every open duel with every blow, its choreography and server stamps, the
   deadline, the mid-beat events, and the server's clock.
2. **Live and authoritative agree.** The live view and the resolve run `duel::apply_act` on the same state and the
   same ordered actions.
3. **No accepted instant action is lost to a server kill.** It is on disk (appended, not fsynced: safe against a
   process kill, not against power loss) before the next one is taken, and handed to the first resolve after the
   restart, in arrival order, ahead of that beat's new actions.
4. **A duel's clock counts only time the room was running.** A pause moves `deadline_ms` and is listed in `pauses`.
5. **Replays are exact.** A match that was killed and resumed, paused, joined mid-match or forked replays with 0
   differences: the two side inputs are recorded and fed back.
6. **Older recordings replay unchanged.** Everything above is gated on `meta.schema >= 8`, the panels on `>= 9`,
   the looks on `>= 11`, the Earth on `>= 13` (and its setting, absent from older settings, is off there).
7. **Ids stay unique across restarts.** The core's `seq` restarts at 1 with the process; a challenge whose
   `duel:s<seq>:<a>:<b>` already names a duel of the match gets `seq + 1000000` (repeated until free). Duel input is
   ordered by `(received_at_ms, seq)`.

## 8. Measured gaps

Measured by `fn7-server/tests/exact_resume.rs` (debug build, this machine; three runs: `kill -9` in beat 4's Compose,
32 to 170 ms after the last accepted duel line, the server 2.8 s down; each time the match came back after committed
beat 3 with a paused snapshot):

| What | Lost at a kill | Why | Closed by |
|---|---|---|---|
| Instant actions since the last Commit (casts, summonings, challenges, duel lines) | **0** | the journal | done here |
| The duel (blows, hp, range, stances, deadline) | **0**; the deadline moves by the downtime | journal + `pauses` | done here |
| The beat in progress | **re-run from its Start**: up to one beat (`flow_ms` + the resolve) of world time is played again; if the kill lands before the Commit's snapshot is written, one more beat | the core persists at Commit, asynchronously | engine request 1 |
| Text typed into the killed beat's Compose, and a held slow-window submit | **lost** (`match.snapshot.you.action` is null after the restart) | the core keeps them in memory | engine request 1; a client resends its last text when it sees `paused` |
| The open slow window | **lost** (no `window` after the restart) | the core keeps it in memory | engine request 1 |
| The recording's tail | **the last buffered records** (under 8 KiB: the killed beat's small records), and one torn line where the restarted server appends | the core writes it through a `BufWriter` | engine request 2. The killed beat's instant actions are in `journal_recovered`, so replay is exact regardless |
| The duel's clock at a kill | starts again from the last sign of life (the newest of the state's stamps and the journal's), so up to one beat of running time before the kill is not counted | the plugin is not told when the room stops | engine request 4 |
| The duel's clock at a pause | exact to the Commit hook (ms) | measured between the plugin's Commit and Start hooks | engine request 4 would make it the core's own number |
| `present` for two live matches with one seed (a fork and its parent both running) | a view is matched to the run whose hooks last left exactly that beat and mark; between a parent's and a fork's identical states the newer wins | `public_view` runs outside the match's context | engine request 5 |
| A schema-7 match in flight at the upgrade | no `present`, the 0.8.1 in-memory cache as before | not migrated | none (it ends) |
| Power loss | the journal's last entries (no fsync) | cost per action | not planned |

## 9a. Pictures in time: the asset registry (layer 7)

The state and the recording say what happened; they name pictures only by media hash, and a hash is a request key,
not a file. The asset registry ([`ASSETS.md`](ASSETS.md)) closes that: for every image a payload carried it keeps
`uses(asset, match, beat … last_beat, role, subject ref, subject key, loadout hash)`, one row per run of beats, and the
asset row points at the file by the sha256 of its bytes.

```
GET /api/assets/at?match=<match id>&beat=<N>&demigod=<name | dg:N>
```

answers "what did this demigod look like at beat N of match M": per picture kind (`portrait`, `diorama`, the statue
image) the asset whose run of beats covers N (`matched: covers_beat`), else the newest one first seen at or before N
(`latest_before`), else the earliest (`earliest_seen`); each with `path` (under the assets root), `file_url`,
`media_url`, `sha256`, `loadout_hash`, `first_beat`, `last_beat`. Without `demigod`: every picture in effect at that
beat (all demigods' pictures and the moment cards up then). `GET /api/assets?match=<id>&beat=<N>` lists the same assets
with their rows, and `GET /api/assets/{id}` gives the exact prompt and cost of each.

- **Where it comes from.** Live: the server's outermost plugin wrapper reads each outgoing payload (public and per
  seat) after the media layer has annotated it and records every image object with its owner's `ref`, `name` and
  loadout hash at the hook's beat. It reads only; state, payloads and the recording are untouched, so replays are too
  (`fn7-server/tests/assets.rs` replays a played match with 0 differences).
- **A picture that changes mid-match** (gear-aware portraits: a new loadout is a new image) starts a new `uses` row
  at that beat with its `loadout_hash`; the row before it ends at the beat before. Coming back to an earlier look
  starts another row.
- **Older matches** (before the registry): `fn7-assets ingest --recordings` reads each recording's `media_request`,
  `media_cached` and `media_refused` events, which carry the beat, so the lookup works for them at the beats an image
  was asked for (`role: requested | cached`; `matched` is then usually `latest_before`). The profile library adds
  match-level rows (`beat -1`).
- **A fork** is a new match id; its payloads write their own rows from its first beat. What the parent showed before
  the fork point stays under the parent's id.
- **Not covered:** a match played with recording off has no match id and so no `uses`; a picture shown only by a
  client-side cache (never re-sent in a payload) is not seen by the server.

## 9. Engine requests

Written in full in docs/ENGINE_SYNC.md ("Open requests"). In short:

1. **Persist the round in progress**: the queued instant actions, the Compose and window submissions, the open window
   and `next_action_seq`, in the room snapshot (or a per-room append log), so a restart resumes inside the beat.
2. **Flush the recording per record** (or at least at `instant.submit` and each hook), and start a new line on reopen.
3. **A timer or hold-window callback** (`on_timer`), so a game can act between actions (see ENGINE_SYNC; phase 2).
4. **Tell the plugin about pauses and time**: `now_ms` and `paused_ms` in the Start hook's input (recorded with it).
5. **Run `public_view` / `seat_view` in the match's context**, or pass the match id to every hook.
6. **`server_time` in `match.snapshot`**, and several public events from one `on_instant`.
7. **Tell a resumed viewer it has no room** (today: `auth.ok` and silence).
8. **`round.brief` for viewers**, and `rules_version` in `/api/house`.

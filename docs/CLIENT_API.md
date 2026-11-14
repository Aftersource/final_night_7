# Final Night 7: client API

Final Night 7 runs as a game plugin (`final_night_7`) inside the Theatre server. This file covers what a client needs to
play it: the game-defined objects inside the Theatre's generic messages, the action format, the room settings, the
timing, and who sees what. The envelope, login/resume, the lobby, rate limits and the generic message list are in the
Theatre's [`PROTOCOL.md`](../../theatre/PROTOCOL.md) (also in the repo root of
[Aftersource/theatre](https://github.com/Aftersource/theatre)). Read that first; this file doesn't repeat it.

- **Schemas:** [`docs/schemas/`](schemas/) has a JSON Schema (draft 2020-12) for every game payload. They're generated
  from the code by `cargo run -p fn7-tools --bin fn7-schemas`, and a test fails if they go stale.
- **Examples:** [`docs/examples/`](examples/) has one real frame of every message type. They were recorded from one
  player (`Juniper`, seat 1) in a 4-player offline match (no LLM, 6 beats, `beat_ms` 1500), with arrays longer than 8
  cut to their first 6 items. A test validates each one against its schema, and that test also rejects any key the
  schema doesn't document.

## Connecting

```
ws://<host>:<port>/ws          LAN playtest: ws://10.0.0.34:8080/ws
GET http://<host>:<port>/healthz → ok        GET /stats → { version, game: "final_night_7", players, connected, rooms }
```

On connect the server sends `server.hello { protocol: 1, server_version, game: "final_night_7" }`. Check `game`. After
that you log in, then create, join or quick-join a room, set ready, and the host starts the match. All of that is
generic (PROTOCOL.md §auth, §room). A few limits matter more than usual here:

- **Frames:** at most 64 KiB in. **`action.submit.text` is capped at 500 characters by the core**, but each hand in
  `fields` may hold 2000. Put long speech in `fields` and keep `text` short or empty.
- **Rate:** at most 20 messages/s. Send `sys.ping` every ~20 s, because a socket that stays silent for 60 s is closed.
- **Room size:** 1–8 seats (`Room.limits`: `min_players` 1, `max_players` 8).

## The game in one paragraph

Each seat is a **demigod** with health 10 somewhere on **the whole planet** (the default arena; `arena: "city"` plays
one real city instead). The planet is divided into **regions** (UN subregions such as "Western Africa"), each with its
largest **cities**. The world's action runs in fast **flow beats** whether or not anyone types. You have two separate
inputs. **Text entry** is the God of Time interaction: you slow time (everyone is told who did), type orders to your two
**hands** (LLM orchestrators that act, raise **armies**, spawn **subagents**, possess people and invoke the seven gods:
Time, Nature, Consequence, Death, Chance, Memory and **Entropy**), and your Enter sends them and cuts off anyone who
typed slower. **Casts** are god powers (lightning, a volcano, a flood, a custom power...) thrown at any point of the
globe at any time, paid in **lightning bolts**: every bolt anyone spends gives every other demigod 2, and raises
**Entropy**, which makes the world wackier until heat death brings the Final Night at once. Every act runs up a bill with
Consequence. Time's clock shrinks a circle toward one city. The last demigod standing wins; on the Final Night exactly
one survives. Demigods no seat plays are played by the plugin's own AI (no Theatre seat).

## Message flow for one beat

One Theatre **round** is one **beat**, so `round` and `beat` are always the same number. There are no choices: `round.private.choices`
is always `[]`, so the Choose phase is always skipped. This is one real beat as player `Juniper` received it (ms from
match start):

```
  5  S→C phase.changed       { round: 1, phase: "start" }
 10  S→C round.started       { round, public: RoundStartedPublic, private: RoundStartedPrivate }
     S→C seat.eliminated     { seat, round, reason }        (only if someone fell at Start; then room.updated)
 10  S→C phase.changed       { round: 1, phase: "brief" }
 11  S→C round.brief         { round, public: BriefPublic, private: SeatView }
 11  S→C phase.changed       { round: 1, phase: "compose", deadline }   ← deadline = epoch ms
 11  C→S action.submit       { round: 1, text, fields: { left, right, zoom, slow, sent_at, window, cast } }
 12  S→C action.received     { round: 1, accepted: true }
     S→C phase.progress      { round, phase: "compose", done, total }   (as seats submit)
 13  S→C phase.changed       { round: 1, phase: "resolve" }             ← all LLM work happens here
 23  S→C phase.changed       { round: 1, phase: "reveal" }
 24  S→C round.outcome       { round, public: OutcomePublic }
 24  S→C round.private       { round, private: SeatDelta, choices: [] }
 24  S→C phase.changed       { round: 1, phase: "commit" }
 28  S→C round.committed     { round, scores: { "<seat>": number }, public_state: PublicView }
     S→C seat.eliminated     { seat, round, reason }        (anyone who fell this beat; then room.updated)
 30  S→C round.private_state { round, state: SeatView }
 33  S→C phase.changed       { round: 2, phase: "start" }   …
         …
     S→C match.ended         { match_id, winners: [seat], summary: MatchSummary }
```

| Message | Game object | Schema | Example |
|---|---|---|---|
| `match.started.rules` | validated settings | [settings](schemas/settings.schema.json) | [match.started](examples/match.started.json) |
| `round.started.public` / `.private` | Start of the beat | [public](schemas/round_started_public.schema.json), [private](schemas/round_started_private.schema.json) | [round.started](examples/round.started.json) |
| `round.brief.public` / `.private` | brief / your seat view | [public](schemas/brief_public.schema.json), [private](schemas/brief_private.schema.json) | [round.brief](examples/round.brief.json) |
| `action.submit.fields` | your input | [action_fields](schemas/action_fields.schema.json) | [action.submit](examples/action.submit.json) |
| `round.outcome.public` | what everyone sees | [outcome_public](schemas/outcome_public.schema.json) | [round.outcome](examples/round.outcome.json) |
| `round.private.private` | `SeatDelta`: what only you learn | [round_private](schemas/round_private.schema.json) | [round.private](examples/round.private.json) |
| `round.committed.public_state` | public view | [public_view](schemas/public_view.schema.json) | [round.committed](examples/round.committed.json) |
| `round.private_state.state` | seat view | [seat_view](schemas/seat_view.schema.json) | [round.private_state](examples/round.private_state.json) |
| `match.ended.summary` | the end screen | [match_summary](schemas/match_summary.schema.json) | [match.ended](examples/match.ended.json) |

Generic frames recorded in the same match: [server.hello](examples/server.hello.json), [auth.login](examples/auth.login.json),
[auth.ok](examples/auth.ok.json), [room.create](examples/room.create.json), [room.join](examples/room.join.json),
[room.snapshot](examples/room.snapshot.json), [room.ready](examples/room.ready.json), [room.updated](examples/room.updated.json),
[room.start](examples/room.start.json), [room.countdown](examples/room.countdown.json), [phase.changed](examples/phase.changed.json),
[action.received](examples/action.received.json), [seat.eliminated](examples/seat.eliminated.json).

On reconnect (`auth.resume`), `match.snapshot` carries the same objects: `public_state` is a PublicView, `started` /
`brief` / `outcome` hold the public parts of this round's messages, `you.private_state` is a SeatView, and
`you.started` / `you.brief` / `you.outcome` hold the private parts.

### Conventions

- **Refs** are strings: `"<prefix>:<n>"`, matching `^(dg|ch|cl|gr|gd|pl|it|ag|lg):\d+$`. The prefixes are `dg`
  demigod, `ch` character (a named person), `cl` cluster (a crowd, or an army), `gr` group, `gd` god, `pl` place (Earth,
  a region or a city; in a city arena, the city, a district or a site), `it` item, `ag` agent and `lg` ledger row. A
  ref is stable for the whole match.
- **Geotags.** Every news line, omen, cast, world event, army and world mod carries `at: [lon, lat]` and `region` (the
  region's name on the planet, the district's in a city). `at` is null only for bookkeeping rows with no place.
- **Every id is a ref.** Hands, subagents, bodies, news and debt rows, favor, omens, item origins: all `"ag:3"`,
  `"lg:12"`, `"gd:0"`, `"ch:16"`, `"it:4"`. No payload carries a bare numeric id.
- **Positions** are `[longitude, latitude]` in degrees (WGS84), **longitude first**.
- **Gods** are numbered by their index in the pantheon: `gd:0` Time, `gd:1` Nature, `gd:2` Consequence, `gd:3` Death,
  `gd:4` Chance, `gd:5` Memory, `gd:6` Entropy. The table arrives every beat in `round.brief.public.gods` (`[{ref, name, slug,
  domain}]`), so you don't need to hard-code it. Wherever a god appears, it is `{god: <name>, god_ref: "gd:N", ...}`.
- **Health** is a float from 0 to `max_health` (10 unless Time took years of your life). Damage uses Final Night's
  0–10 scale, and debts are measured in damage points.
- **Floats** are rounded by the server: 2 decimals (health, debts, favor, radii, scores, timings), 6 for positions
  (`pos`, `center`); `-0.0` never appears; settings echo at their own precision (`debt_scale: 0.04`).
- **Shapes are the same everywhere.** An omen is always `{god, god_ref, speech}` (the speech never repeats the god's
  name), `focus` is always an Entity object or null, a subagent is always a `SubagentLine` object, and `news` is always
  newest first with at most one line per ledger row.
- **Image prompts.** `portrait_prompt` (each demigod in the PublicView, stable all match, with `image_kind:
  "portrait"`), `image_prompt` (only the biggest world events, with `image_kind: "event"`) and `statue_prompt` (the
  end-of-match piles, with `image_kind: "statue"`) describe the subject only; the media server renders them (see
  [`MEDIA.md`](MEDIA.md)).
- **Unknown keys:** ignore them. New optional fields get added over time.

## Payloads

### `round.started` (the beat's Start, before anyone acts)

The plugin always sends this message. At Start, Time's clock shrinks the arena, and bodies outside it take 1–5 damage.
Debts that fall due are queued for this beat's merge, where a Time delay can still push them back. Subagents whose
budget has run out expire.

`public`
```json
{ "beat": 5, "beats_total": 6, "final_night": false,
  "arena_radius_km": 4.10,
  "caught_outside": ["Rook", "Juniper", "Quill", "Moth"],   // names hurt by Time's clock this Start
  "debts_due": 0,                                            // collections queued this beat (all demigods)
  "eliminated": ["Rook", "Quill"],                           // names that fell during Start
  "alive": 2 }
```

`private` (your demigod after Start)
```json
{ "beat": 5, "health": 1.0, "alive": true, "outside_arena": true, "debt_due": 0.0 }
```
`outside_arena: true` means you must move inward (for example, "go to <central district>") or keep taking damage every
beat. `debt_due` is the number of damage points Consequence will collect from you during this beat's Resolve.

### `round.brief`

`public`
```json
{ "title": "Beat 1 of 6", "beat": 1, "beats_total": 6, "final_night": false,
  "arena": { "city": "Jakarta", "center": [106.827492, -6.172472],
             "radius_km": 12.07, "start_radius_km": 14.06, "end_radius_km": 2.11 },
  "alive": 4,
  "rules": "Talk to your hands: fields {left, right} (or text lines `l: …` / `r: …`) …",
  "gods": [ { "ref": "gd:0", "name": "Time", "slug": "time", "domain": "slowing, hastening, rewinding …" }, … ] }
```
`rules` is a one-paragraph reminder of the action format, written for LLM players. The arena shrinks linearly from
`start_radius_km` at beat 0 to `end_radius_km` on the Final Night. `gods` is the god table in id order (`gd:0` …
`gd:6`). The planet edition adds:

```json
{ "arena": { "kind": "planet", "name": "Earth", "city": "Jakarta", "center": [106.827492, -6.172472],
             "radius_km": 19725.0, "start_radius_km": 20037.5, "end_radius_km": 1500.0 },
  "arena_kind": "planet",
  "powers": [ { "slug": "lightning", "name": "Lightning bolt", "icon": "lightning", "cost": 1, "radius_km": 30.0,
                "target": "unit", "magnitude": 6, "damage": 1.0, "tags": ["sky", "fire", "fast"] }, …,
              { "slug": "custom", "name": "Custom power", "icon": "custom", "cost": null, … } ],
  "entropy": { "value": 12.4, "max": 100.0, "level": 0, "key": "calm", "text": "…", "next_at": 20.0, "heat_death": null },
  "flow": { "flow_ms": 3000, "slow_ms": 30000, "window_beats": 10,
            "slow": { "by": "Rook", "by_ref": "dg:0", "window": 4, "closes_by_beat": 14, "beats_left": 9, "ms_left": 27000 } },
  "regions": [ { "ref": "pl:1", "name": "Australia and New Zealand", "center": [151.5, -35.3], "radius_km": 1200.0,
                 "cities": [ { "ref": "pl:2", "name": "Sydney", "center": [151.18, -33.92] }, … ] }, … ] }
```
- `arena.city` is where Time's circle closes (the planet's Final Night converges there); in a city arena it is the city.
- `powers` is the power palette (`data/powers.json`, the same in every match until the data changes), then the custom
  entry. `target` says what a power is aimed at: a `point`, a whole `region`, or a `unit` (a demigod, person, crowd or
  army); any power accepts any target form (below). `icon` is a data key (the power's slug).
- `entropy` is the meter (also in the outcome and the PublicView): thresholds at 20 (`weather`: freak weather), 40
  (`physics`: demigods flung, a third of casts off target), 60 (`riots`: crowds riot and worship), 80 (`godfall`: the
  gods take damage), 100 = heat death.
- `flow.slow` is the open slow window (null when none): `window` is its id (send it back with orders typed during it).
- `regions` lists every region with its cities (in a city arena: the districts and their sites).

`private` is your **SeatView** (below) as it stands at the start of the beat, with `beat` set to the coming beat.

### SeatView: `round.brief.private`, `round.private_state.state`, `match.snapshot.you.private_state`

This is everything your seat may see. You receive it twice per beat: at the brief (before you act) and after the
commit.

| Field | Type | Meaning |
|---|---|---|
| `seat` | int | Your seat |
| `demigod` | Ref | Your demigod, e.g. `"dg:1"` |
| `name` | string | Your demigod's name (your display name; de-duplicated with ` <seat>` appended if two match) |
| `alive` | bool | `false` = eliminated (you keep receiving this view as a spectator) |
| `beat` | int | Last beat started (in the brief: the coming beat) |
| `you` | YouLine | Health, position, open debt, favor, followers, echo charges (below) |
| `max_health` | number | Your health cap |
| `zoom` | `"world"` \| `"district"` \| `"city"` \| `"character"` | Your zoom level (starts at `district`; on the planet `district` is a region) |
| `focus` | Entity \| null | What you're focused on, as an **object** |
| `visible` | Entity[] (≤ 60) | What you can see at your zoom, in order: yourself, live rivals, your items, then for world zoom every district and the 10 biggest crowds, or for district/character zoom the focus, its district, everything in that district (people, crowds, sites, items held there) and the other districts. Bodies you possess come last. |
| `hands` | SeatHand[2] | `[left, right]`: `{ agent: Ref, name, live: bool, subagents: SubagentLine[] }` |
| `items` | Entity[] | Items your demigod holds |
| `debts` | DebtLine[] | Open debts: `{ row: Ref, owed_to: god name, owed_to_ref: Ref, amount: number, due_beat: int, status: "open"\|"delayed"\|"collected"\|"settled" }` |
| `news` | NewsLine[] (≤ 20) | Ledger rows you know about from the last ~3 beats, newest first, one line per row |
| `armies` | Army[] | Your live armies, with their `orders` |
| `orders` | `{hand, text, since}`[] | Your hands' standing orders: what they keep doing, by rules, while you are silent |
| `slow` | null \| `{yours, window, held}` | The open slow window as it concerns you: you opened it; orders of yours are held in it |

**Entity** (one thing you can see): `{ ref, name, kind, pos? }`. `kind` is one of `demigod`, `character`, `cluster`,
`group`, `god`, `place`, `item`, `agent` or `ledger`, matching the ref prefix. A cluster entity adds `size` (head
count) and `mood` (`calm`, `angry`, `afraid`, `hopeful` or `devoted`), and an army adds `army_of` (the demigod it marches
for). A character entity adds `alive`. No entity carries health; your own health is in `you`. `visible` follows your
zoom: `world` = you, rivals, every region, the 10 biggest crowds and every army; `district` (a region) = what is in it;
`city` = the city you're in (or focus) and what is there, its region and the region's other cities; `character` = the
focused person and their surroundings.

**Army:** `{ ref, name, demigod, demigod_ref, size, pos, region, dest, target }` (+ `orders` for your own). An army is
a crowd a hand raised; it marches up to 900 km a beat toward `target` (a place, or a demigod it hunts), clashes with
rival armies within 300 km and strikes rival demigods within 200 km.

**SubagentLine** (the same object in the SeatView and in `round.private`):
```json
{ "agent": "ag:14", "name": "Emeka Rasmussen (steered by Juniper)", "goal": "take over the captain",
  "line": "Steering the captain toward the docks.", "beats_left": null,
  "kind": "possession", "parent": "ag:3", "body": "ch:16" }
```
`kind` is `"subagent"`, `"possession"` (it steers the person in `body`) or `"paladin"` (a god's champion in `body`).
`parent` is the agent that spawned it (your hand). `body` is null for plain subagents. `line` is what it said this beat
in `round.private`, else its goal. `beats_left: null` means no budget; possessions last until you recall them.

**YouLine:**
```json
{ "health": 10.0, "pos": [106.890461, -6.235065], "debt_open": 0.05,
  "favor": [ { "god": "Time", "god_ref": "gd:0", "favor": 0.0 }, … ],   // every god, in id order
  "followers": 1, "echo_charges": 3, "bolts": 5 }
```
`bolts` is your lightning: casts and god invocations cost bolts; every bolt any rival spends gives you 2.

**NewsLine:** `{ row: Ref, text: string, refs: Ref[], how: string, at: [lon, lat] | null, region: string | null }`. `row` is the ledger row (`lg:<n>`), never repeated
within one payload. `refs` lists the actor first, then the targets, each once. `how` is one of `"you did it"`, `"it
happened to you"`, `"you saw it"`, `"you were told"`, `"your followers heard"` or `"rumor"`.

### `round.outcome.public` (everyone, including spectators)

```json
{ "beat": 2, "beats_total": 6,
  "previous_caption": "Beat 1: Rook goes after nothing in particular, and 8 more things went sideways.",
  "previous_callouts": ["Time: \"Granted, for the usual price. Read the fine print later.\""],
  "caption_beat": 1,                    // which beat the narration describes (null on the first beat)
  "caption": "Beat 1: Rook goes after …",   // = previous_caption (kept for older clients)
  "callouts": ["Time: \"Granted, …\""],     // = previous_callouts
  "eliminated": [],                     // names that fell during this Resolve
  "alive": 4, "arena_radius_km": 10.08,
  "omens": [ { "god": "Time", "god_ref": "gd:0", "speech": "\"Granted, for the usual price. …\"" } ],
  "entropy": { "value": 8.9, "max": 100.0, "level": 0, "key": "calm", "text": "…", "next_at": 20.0, "heat_death": null, "delta": 2.1 },
  "slow": [ { "event": "opened", "by": "Juniper", "by_ref": "dg:1", "window": 5, "closes_by_beat": 15, "beats_left": 10,
              "ms_left": 30000, "text": "Juniper slowed time" },
            { "event": "closed", "by": "Juniper", …, "how": "enter", "after_ms": 4200, "text": "Time snaps back: …" } ],
  "casts": [ { "by": "Vex", "by_ref": "dg:2", "power": "flood", "icon": "flood", "power_name": "Flood", "cost": 2,
               "at": [90.41, 23.72], "region": "Southern Asia", "region_ref": "pl:61", "place": "Dhaka",
               "target": null, "target_name": null, "off_target": false, "row": "lg:83" } ],
  "events": [ { "row": "lg:83", "kind": "cast", "text": "Vex casts flood on Dhaka", "at": [90.41, 23.72],
                "region": "Southern Asia", "refs": ["dg:2"], "magnitude": 7,
                "image_prompt": "Vex casts flood on Dhaka, in Southern Asia", "image_kind": "event" },
              { "row": "lg:84", "kind": "happen", "text": "The flood tears through Dhaka", "at": [90.2, 23.9],
                "region": "Southern Asia", "refs": ["gd:6"], "magnitude": 5 } ],
  "timing": { "total_ms": 6.8, "llm_calls": 19, "llm_fallbacks": 19,
              "stages": [ { "stage": "intake", "ms": 0.85 }, { "stage": "route", "ms": 2.33 }, … ] } }
```
- `slow` lists this beat's slow-window announcements: `opened` (with who; never who else is typing), `open` (each
  further beat), `closed` (`how`: `enter` = the initiator's orders, `timeout`, `fell`; `after_ms` = how long it stayed
  open).
- `casts` lists every cast settled this beat in arrival order; a fizzled one has `fizzled` (the reason) and cost
  nothing. Entropy's answer to each region's casts is an omen (`god: "Entropy"`, public when the cast's magnitude is
  ≥ 6), and the casts' ramifications are rows in `events` (and in the news of those who know them).
- `events` are the public, geotagged world events of the beat, in order (≤ 40): casts and their ramifications, armies
  raised, marching and fighting, freak weather, world mods appearing, stacking and fading, wounded gods, eliminations.
  Only the biggest (casts, eliminations, magnitude ≥ 8) carry `image_prompt`.

- **The narration is one beat late.** The Narrator (Deadpool's voice) runs off the critical path: beat N's narrator
  runs during beat N+1, so the outcome of beat N carries **beat N−1's** caption as `previous_caption` /
  `previous_callouts`, with `caption_beat` = N−1 (`null` and `""` on the first beat). `caption_beat` says which
  beat it describes. The last beat's caption arrives in `match.ended.summary.last_caption`.
- `caption` / `callouts` repeat `previous_caption` / `previous_callouts` exactly. They're kept so older clients keep
  working; new clients should read the `previous_*` fields.
- With the LLM off or late, the caption comes from a template and the callouts repeat the omen speeches
  (`"<God>: <speech>"`). A narrator line that contradicts the engine (an eliminated demigod acting, the champion
  fallen) is replaced by the template. Still, treat the structured fields (`eliminated`, `alive`, `champions`) as
  truth.
- `omens` are the **public** god rulings of this beat: big ones (magnitude ≥ 6) or ones that send a paladin. `speech`
  never starts with the god's name (`god` carries it).
- `timing` holds the server's wall-clock diagnostics. A flow beat nobody spoke or cast in makes no LLM call
  (`llm_calls: 0`) and resolves in milliseconds. `stage` runs through `intake`, `route`, `act`, `merge`,
  `consequence`, `narrate` and `broadcast`.

### `round.private.private`: the SeatDelta (what only you learn this beat)

```json
{ "beat": 3, "seat": 1, "zoom": "world",
  "focus": { "ref": "ch:12", "name": "Ada Okafor", "kind": "character", "pos": [106.86, -6.23], "alive": true },
  "hands": [ { "agent": "ag:2", "name": "Juniper's Left Hand", "said": "On it (1 things).",
               "subagents": [ { "agent": "ag:17", "name": "Juniper's Send #1", "goal": "send someone", "line": "send someone",
                                "beats_left": 5, "kind": "subagent", "parent": "ag:2", "body": null } ] },
             { "agent": "ag:3", "name": "Juniper's Right Hand", "said": "On it (1 things).", "subagents": [] } ],
  "news": [ { "row": "lg:49", "text": "Juniper's Send #1 works at it: send someone", "refs": ["ag:17"], "how": "you did it" },
            { "row": "lg:48", "text": "Juniper's Send #1 singles out someone", "refs": ["ag:17", "ch:18"], "how": "you did it" } ],
  "you": { … YouLine … },
  "omens": [ { "god": "Memory", "god_ref": "gd:5", "speech": "\"Granted, for the usual price. …\"" } ] }
```

- `hands[i].said` is the hand's one-line reply to you this beat. It's empty if you said nothing to that hand.
  `hands[i].subagents` are SubagentLine objects (same as the SeatView), with `line` = what each said this beat.
- `news` holds only the ledger rows that are **new this beat** and that you know about, newest first, one line per row.
- `omens` are the public rulings plus your own invocations, even small ones, as `{god, god_ref, speech}`.
- `held: true`: orders you sent are held in someone else's slow window; they go through when it closes. `cut_off: true`:
  orders you sent came after the window's initiator pressed Enter (or were for a window that had already closed); they
  did not go through.
- `zoom` and `focus` reflect your view after this beat's changes. `focus` is an Entity object (or null), as in the
  SeatView.

### `round.committed`

`scores` covers the human seats only: `"<seat>": 1e6 × alive + followers + 100 × total favor`. They order the seats; the
numbers themselves mean little. `public_state` is the **PublicView**:

```json
{ "beat": 1, "beats_total": 6,
  "arena": { "city": "Jakarta", "center": [106.827492, -6.172472], "radius_km": 12.07, "start_radius_km": 14.06, "end_radius_km": 2.11 },
  "alive": 4,
  "demigods": [
    { "ref": "dg:0", "name": "Rook", "seat": 0, "alive": true, "pos": [106.89, -6.11], "place": "Northeast Works" },
    { "ref": "dg:2", "name": "Quill", "seat": 2, "alive": false, "eliminated_beat": 5, "cause": "caught outside the arena by Time's clock" } ],
  "omens": [ { "god": "Time", "god_ref": "gd:0", "speech": "…" } ],
  "caption": "…", "caption_beat": 1 }   // the latest caption written, and which beat it describes (null before any)
```
`demigods` lists every demigod in id order, including the built-in AI's (`seat: null`), each with `bolts` (public),
`portrait_prompt` and `image_kind: "portrait"`. A live demigod has `pos`, `place` and `region`; a dead one has
`eliminated_beat` and `cause` instead. Health never appears here. The planet edition adds:

| Field | Meaning |
|---|---|
| `entropy` | The Entropy meter (as in the brief) |
| `slow` | The open slow window (`{by, by_ref, window, closes_by_beat, beats_left, ms_left}`) or null |
| `influence` | `[{region, region_ref, demigod, demigod_ref, share}]`: each demigod's share (0..1) of each region's people (crowds weighted by allegiance, its Following, its armies, its presence). A region's shares sum to ≤ 1; shares under 0.005 are left out |
| `armies` | Every live army (Army objects, without `orders`) |
| `mods` | Every live world mod: `{id, name, description, at, radius_km, region, region_ref, tags, stacks, since_beat, expires_beat, power, by}`. A mod is a lasting change a cast left (a permanent storm over a city, half gravity in a region): every beat it harms or heals demigods and armies inside its radius and sways nearby crowds, × its stacks (the same mod again in the same region stacks, up to 5). `expires_beat: null` = lasting. At most 24 live; the oldest is pushed out |
| `gods` | `[{ref, name, health}]`: gods take damage at Entropy ≥ 80; an unmade god's (health 0) invocations are answered by Entropy |

### `match.ended.summary`

```json
{ "champions": [ { "demigod": "dg:3", "name": "Moth", "seat": 3, "health": 2.0 } ],
  "champion_seat": 3,                   // null when a built-in AI demigod won
  "standings": [ { "rank": 1, "demigod": "dg:3", "name": "Moth", "seat": 3, "alive": true, "eliminated_beat": null, "cause": null, "score": 1000001.0 }, … ],
  "piles": [ { "demigod": "dg:0", "name": "Rook", "seat": 0, "alive": false,
               "items":   [ { "item": "it:0", "name": "build", "origin": "summoned", "origin_ref": null } ],
               "summons": [ { "item": "it:0", "name": "build" } ],
               "stats": { "words": 16, "acts": 6, "debt_paid": 0.0, "rulings": 3, "memorized_people": 0, "subagents": 1 },
               "statue_prompt": "An amiibo-style collectible figure of the demigod Rook, frozen mid-fall (…) …" } ],
  "last_caption": { "beat": 5, "caption": "Beat 5: Time's clock catches Rook outside the arena, …" },
  "final_cut": { "title": "Final Night 7", "story": "Beat 1: …\nBeat 2: …", "fallback": true },
  "stats": { "beats": 5, "beats_total": 6, "ledger_rows": 100, "characters": 22, "memorized": 6, "agents": 22,
             "rulings": 13, "intents": 36, "memories": 173, "llm_calls": 87, "llm_fallbacks": 87, "eliminations": 3 } }
```

- `champions` has one entry: the survivor, or the last to fall if nobody is left standing. `champion_seat` is its seat,
  or `null` for a built-in AI demigod.
- `winners` (in the envelope) is defined by the Theatre core as a list of seats. It holds the champion's seat, so
  **`winners: []` means a built-in AI demigod won** (`champion_seat: null`).
- `standings` lists every demigod, best first, with its `seat` (`null` for the built-in AI).
- `piles` has one entry per demigod, for the end screen: everything it held (`items`, with `origin` `"starting"`,
  `"summoned"`, `"found"`, `"echoed"` (a copy of the item `origin_ref`) or `"granted"` (by the god `origin_ref`)),
  everything it created (`summons`), its stats, and an image prompt for its amiibo-style statue.
- `last_caption` is the final beat's caption (narrated one beat late, so at the end of the match, with the champion
  given to the narrator). It's absent if the last beat had nothing to narrate.
- Each pile has `statue_prompt` with `image_kind: "statue"`, its final `bolts` and `slows` (times it slowed time).
  `entropy` is the meter at the end; `heat_death` is the beat heat death struck (the Final Night came early), or null.
  `stats` adds `casts`, `slows`, `armies` and `mods` (world mods ever made).
- `final_cut` is the Narrator's story of the match (≤ 250 words). With `fallback: true`, the beat captions are joined
  with newlines instead.

## The action: `action.submit { round, text, fields }`

There are **two separate input paths**, and one action may carry either or both.

### 1. Text entry: orders to your hands (the God of Time interaction)

```json
{ "t": "action.submit", "id": "a7",
  "d": { "round": 12, "text": "",
         "fields": { "left": "raise an army from the dockworkers and march on Lagos",
                     "right": "slow time and delay my debt\nsend a scout to spy on Vex",
                     "zoom": { "level": "city", "focus": "pl:41" },
                     "sent_at": 1763049612345, "window": 11 } } }
```

| Field | Type | Meaning |
|---|---|---|
| `fields.slow` | bool | You started typing: slow time. Opens a slow window if none is open (one at a time). |
| `fields.left` / `fields.right` | string \| null, ≤ 2000 chars | Your orders to that hand: free speech, several commands, newline-separated. Sending them is your **Enter**. An empty hand does nothing and makes no LLM call. |
| `fields.sent_at` | number (epoch ms) | Your clock when you sent it. The cut-off rule orders submissions by it, and it measures how long your window stayed open. Without it your orders can't prove they came before an Enter. |
| `fields.window` | int | The window (its id, `round.brief.public.flow.slow.window`) you typed these orders in. Orders for a window that already closed are cut off. |
| `fields.zoom` | null \| `""` \| level \| `{ level, focus? }` | An optional view change. `level` is `world`, `district`, `city` or `character` (case-insensitive; `map`/`planet`/`globe` = world, `region` = district, `person` = character). |
| `fields.zoom.focus` | string \| null, ≤ 200 chars | A ref (`"ch:12"`, `"pl:41"`, any ref that exists) or the exact name (case-insensitive) of something visible at district zoom. |
| `text` | string, ≤ 500 chars (core cap) | **Fallback** when `left` and `right` are both absent or null: lines starting `l:`/`left:` go to the left hand, `r:`/`right:` to the right hand, any other line to the left hand, and a line `/slow` is `fields.slow`. |

**How a slow window works (v0).**
- **Opening.** `slow: true`, or orders sent while no window is open, opens a window. Its initiator is whoever sent first
  that beat (by `sent_at`; the plugin's AI demigods count as first). Everyone gets `round.outcome.public.slow` =
  `{event: "opened", by, text: "<name> slowed time", window, beats_left, ms_left}`; nobody is ever told who else is
  typing. Time takes **1 bolt** for it (every other demigod gains 2); with no bolt, Time's debt below doubles.
- **The world keeps moving.** A window lasts up to `ceil(slow_ms / flow_ms)` flow beats (`flow.window_beats`), and
  those beats keep resolving: standing orders, armies, casts, Entropy, the AI.
- **Others may type.** Orders another seat sends while the window is open are **held** (`round.private.held: true`),
  hidden, and go through when it closes.
- **Enter.** The initiator's orders close the window in the beat they arrive. Orders sent with the slow request itself
  close it at once. Held orders go through with them, and so do other seats' orders that arrived in that same beat
  **before** the initiator's (by `sent_at`). Orders sent after the Enter, or tagged with a window that already closed,
  are **cut off**: they don't go through (`round.private.cut_off: true`). If the initiator never presses Enter, the
  window closes by timeout at `closes_by_beat` (held orders still go through); if the initiator falls, it closes too.
- **Time's penalty**, written when it closes: a debt to Time of `0.5 × (1 + seconds open / 10) × (1 + 0.5 × slows in
  the last 20 beats)` damage points (doubled if the bolt couldn't be paid), due in `debt_due_beats`; and Entropy rises
  by `0.5 + seconds / 30`. "Seconds open" comes from your `sent_at`s (the slow request to the Enter), else beats ×
  `flow_ms`. Pressing Enter early is cheap, and it cuts off whoever is slower.
- Orders from a window open at least one beat get up to `max_intents_per_window` acts per hand (12); orders sent in
  the beat they open their window get `max_intents_per_hand` (6).

**What the hands do with orders.** A hand turns speech into acts, in the order spoken. Each act is one of:
  - an ordinary act: walk, talk, fight, recruit, build, at any zoom level (planet, region, city, person)
  - an army act: "raise an army from the dockworkers", "march my host on Cairo", "send my army after Vex" (rules: the
    army is a crowd split off, it marches up to 900 km a beat, fights rival armies and strikes rival demigods)
  - a god invocation (routed by keywords first; it costs `1 + magnitude/5` bolts, paid unless the god refuses; a god
    whose price you can't pay twists or refuses):
    - Time: "slow time", "rewind", "undo", "speed up"; "cool the world", "calm the chaos" (cools Entropy, for a price)
    - Nature: "storm", "lightning", "summon a beast", "earthquake"
    - Consequence: "pay my debt", "absolve"
    - Death: "raise the dead", "resurrect", "kill from afar"
    - Chance: "roll the dice", "lucky"
    - Memory: "make them forget", "implant a false memory", "what do they know"
    - Entropy: "unleash infinite power", "let it all burn", "chaos" (costs nothing, always says yes, gives you 2 bolts,
      raises Entropy)
  - a spawn: "send a scout to spy on Vex" (≤ `max_subagents_per_hand` live per hand)
  - a possession: "possess Ada the baker"; a zoom: "zoom in on …"; a recall: "recall the scout"
- Each hand keeps its last two ordinary acts as **standing orders** (`seat_view.orders`) and carries them out by rules
  every other beat while you're silent: no LLM call. Typed text never casts a power (a "lightning" in your words is
  Nature's business).

### 2. Casts: god powers (instant UX actions)

```json
{ "t": "action.submit", "id": "a8",
  "d": { "round": 12, "text": "",
         "fields": { "cast": { "power": "volcano", "target": [3.38, 6.52] },
                     "bolts": ["dg:2"], "sent_at": 1763049612345 } } }
```

| Field | Type | Meaning |
|---|---|---|
| `fields.cast` | `{ power, target, text?, magnitude? }` | One power. `power` is a slug from `round.brief.public.powers` or `"custom"`. |
| `fields.casts` | array of the same | Several (at most 5 per action across all four fields). |
| `fields.bolt` / `fields.bolts` | `{ target }` or the bare target / array | Aliases for casting `lightning`. |
| `target` | ref \| `[lon, lat]` \| `"lon,lat"` \| name | A ref (`"dg:2"`, `"cl:7"`, `"pl:41"`), any point on the globe, or a name (a demigod, region, city, crowd or person). A body or crowd target takes the power's direct damage; a place or a point is struck at its position. |
| `text` | string ≤ 200 | `custom` only (required): what the power is ("a rain of frogs over the stadium"). |
| `magnitude` | 0..10 | `custom` only (default 5): its size; it costs `1 + magnitude/3` bolts (at most 4). |

- A cast costs its `cost` in bolts. **Every other live demigod gains 2 bolts per bolt spent**, and Entropy rises 0.4 per
  bolt. Casts are settled at the start of Resolve in arrival order (the plugin's AI first, then `sent_at`, then
  demigod), before anything else in the beat; a cast you can't pay for **fizzles** (`casts[].fizzled`, nothing spent).
- Every cast gets **Entropy's check, which always says yes** (an omen in Entropy's voice). Then the ramifications: one
  LLM call per region with casts this beat writes N ramifications (`1 + cost + magnitude/3` per cast, at most 10):
  harm, healing, crowds' moods, people flung about, things happening at points near the strike, and **world mods**
  (lasting changes: `public_state.mods`). They arrive as geotagged `events` and news rows.
- At Entropy ≥ 40 (physics), a third of casts land 50–400 km off target (`off_target: true`).
- A cast never carries orders and never slows time; it can ride on the same `action.submit` as text entry.

### Both paths

- **Errors:** a malformed action (`left` not a string, an unknown zoom level, a hand over 2000 chars, an unknown power,
  a custom power without `text`, more than 5 casts) gets an `error` reply with `bad_request` and a readable `message`.
  Submitting outside Compose, for the wrong round, or while eliminated gets `wrong_phase`. A `text` over 500 chars gets
  `too_long`.
- **Default action:** if you don't submit before the deadline, you get silence. The world keeps moving: Time's clock,
  debts, your standing orders, armies and live subagents all still run.
- **Resubmitting** while Compose is open replaces your action (send the casts again with it).

## Room settings (`room.create { settings }` / `room.settings`, returned as `match.started.rules`)

Every key is optional. Integers may also be sent as whole floats (`3.0`). An out-of-range value is rejected with a
readable `bad_request`. **Unknown keys are dropped silently.**

| Key | Type | Default | Range | Meaning |
|---|---|---|---|---|
| `seed` | int | picked at match start | ≥ 0 (u64) | All randomness derives from it. Not echoed in `rules` when unset. |
| `arena` | string | `"planet"` | `"planet"` \| `"city"` | The whole Earth (regions → cities) or one real city (districts → sites) |
| `city` | string | seeded pick (≥ 1M people, on land) | ≤ 80 chars | Fuzzy-matched. On the planet: where Time's circle closes for the Final Night; in a city arena: the arena |
| `players` | int | 4 | 1–8 | Demigods in the match. Every seat gets one, and the built-in AI plays the rest. Effectively `max(seats, players)`. |
| `beats_total` (alias `rounds`) | int | 100 | 1–1000 | The Final Night falls on this beat. `rounds` applies only when `beats_total` is absent. |
| `flow_ms` | int | 3000 | 500–120000 | Flow beat length. `compose_seconds` defaults to `flow_ms / 1000`. |
| `slow_ms` | int | 30000 | 1000–600000 | Longest a slow window stays open: `ceil(slow_ms / flow_ms)` flow beats |
| `beat_ms` | int | = `flow_ms` | 500–120000 | Alias of `flow_ms` (used when `flow_ms` is absent; echoed as `flow_ms`) |
| `start_bolts` | int | 3 | 0–99 | Lightning bolts each demigod starts with |
| `entropy_scale` | number | 1.0 | 0–10 | Multiplier on every Entropy rise (a playtest dial: 4 = heat death four times as fast) |
| `clusters` | int | 200 | 8–600 | Population clusters created at setup |
| `max_subagents_per_hand` | int | 3 | 0–10 | Live subagents + possessions per hand |
| `mind_timeout_ms` | int | 6000 | 50–120000 | Per-call timeout for every LLM mind. A late call takes its rules fallback. |
| `llm` | bool | true | | `false` = never call the LLM (rules fallbacks only; captions become templates) |
| `debt_scale` | number | 0.04 | 0–10 | Consequence's bill multiplier, per demigod per beat |
| `debt_due_beats` | int | 6 | 1–100 | Beats until a bill falls due |
| `memory_per_owner` | int | 40 | 4–1000 | Live memories per owner before old ones fold into a summary |
| `llm_folds_per_beat` | int | 2 | 0–32 | Memory folds the LLM writes per beat (the rest are done by rules) |
| `max_intents_per_hand` | int | 6 | 1–12 | Acts one hand makes from orders sent in the beat they open their window |
| `max_intents_per_window` | int | 12 | 1–24 | Acts one hand makes from orders that closed (or were held in) a window open at least one beat |
| `max_memorize_per_beat` | int | 6 | 0–64 | People memorized from player mentions per beat |
| `max_witnesses` | int | 4 | 1–64 | Witnesses recorded per act |
| `compose_seconds` | number | `flow_ms / 1000` | (0, 600] | The core's Compose timer (fixed for the room). If you set it explicitly, it no longer follows `flow_ms`. |
| `resolve_timeout_s` | number | 60 | (0, 1800] | The core's resolve timeout |
| `choose_seconds` | number | (core) | (0, 600] | Unused: there is no Choose phase |

For a playtest, set `beats_total` and `flow_ms` (or `beat_ms`) and leave `compose_seconds` unset. The server's `rules` then contain
every key above except `rounds`, plus `seed`, `city` and `choose_seconds` only when you set them.

## Timing and phases

| Phase | Timed? | What happens | You receive |
|---|---|---|---|
| `start` | no | Time's clock shrinks the arena and hurts bodies outside it, debts fall due, subagents expire | `round.started`, maybe `seat.eliminated` + `room.updated` |
| `brief` | no | | `round.brief` |
| `compose` | **yes**: `deadline` = epoch ms, `compose_seconds` (= `flow_ms`) after it opens | You type, slow time, cast | `phase.progress { done, total }` |
| `resolve` | no (capped by `resolve_timeout_s`, default 60) | Slow windows and cut-offs → casts → hands → mentions → route → one batch of acts (resolvers, gods, subagents, paladins, possessions, ramifications) → merge with the world's rules (armies, Entropy, mods) → consequence; the previous beat's narrator runs alongside | `phase.progress` at least every 5 s |
| `reveal` | no | | `round.outcome`, `round.private` |
| `choose` | never entered | | |
| `commit` | no | Heat death's collections, eliminations and the Final Night survivor rule | `round.committed`, maybe `seat.eliminated` + `room.updated`, `round.private_state` |

- **Flow beats.** Every round is a flow beat: Compose lasts `flow_ms` (the core's `compose_seconds`) and then the world
  resolves, whether or not anyone submitted. A beat nobody spoke or cast in makes no LLM call and resolves in
  milliseconds; a beat with orders or casts takes the LLM's time (the hands, then one batch of acts; each call capped
  by `mind_timeout_ms`).
- **Compose closes early once every active seat has submitted.** So submit only when you have something (orders, a
  slow request, a cast); an idle client should not submit, or it shortens everyone's flow beat. A submission that
  arrives after Compose has closed gets `wrong_phase`: hold it for the next beat.
- **What the engine doesn't do yet** (engine proposal v2.2, requested from the Theatre): the core's Compose timer is
  fixed per room (it reads `compose_seconds` from the room settings), so a slow window can't lengthen or end a Compose
  phase; the initiator's submission can't close Compose on its own; the core delivers each beat's actions keyed by seat,
  without their arrival order (hence `sent_at`); there is no mid-phase push and no "started typing" message; and
  `phase.progress { done, total }` counts submissions, so it shows that *someone* submitted. Slow windows, held orders
  and the cut-off are therefore game rules the plugin applies at Resolve to the actions the core delivered, over flow
  beats. Casts ride on the normal `action.submit` until the engine's instant actions arrive.
- Use `deadline` against your clock and correct for skew with `sys.pong.server_time`.

## Hidden information

| Everyone sees | Only you see |
|---|---|
| The arena (city, center, current/start/end radius) | Your health, max health, open debts, favor with each god, followers, echo charges |
| Every live demigod's position and place name; who fell, when and how | Your hands' replies and your subagents/possessions |
| The (previous beat's) caption, the callouts, and the public omens (big rulings, or ones that send a paladin) | Your view at your zoom (`visible`, `focus`) |
| Every demigod's bolts; casts and their ramifications; armies; world mods; freak weather; Entropy; influence per region | Your armies' orders, your standing orders |
| Who opened a slow window, and when it closed (and how long it stayed open) | Whether your orders are held or were cut off. Nobody ever learns whether anyone else typed |
| How many demigods are alive; the score ranking | `news`: ledger rows you **know** (below) |
| Names of demigods hurt by Time's clock | Your own god rulings, including small ones |

You **know** a ledger row if any of the following did it, witnessed it, was told about it, or was its target: you,
your hands, one of your subagents, or a body you possess. You also know a row once news of it reaches your
**Following** (a group), spreading one hop per beat along group overlaps. If Memory erased the row from you, you no
longer know it. Rivals' health is never sent to you. You can only guess it from the news and the captions.

## Elimination and spectating

A demigod falls when its health reaches 0 (from combat, an accident chain from Consequence, or Time's clock) or on
the Final Night. When that happens:

- Everyone gets `seat.eliminated { seat, round, reason }`, where `reason` is the cause in prose, followed by
  `room.updated` with the seat's `status: "eliminated"`.
- The fall is also listed by name in `round.started.public.eliminated` (if it happened at Start) or in
  `round.outcome.public.eliminated` (during Resolve). From then on the demigod shows `alive: false` in the
  PublicView.

**Spectators** stay in the room. They still get every public message, plus their own `round.brief.private`,
`round.private` and `round.private_state` (with `alive: false`; the hands show `live: false`). They can chat. Sending
`action.submit` gets `wrong_phase`.

**The match ends** after the commit of the beat where at most one demigod stands, or on the Final Night
(`beats_total`). A Final Night always leaves exactly one survivor: the demigod with the lowest outstanding debt, then
the most health. If everyone falls in the same beat, the survivor rule picks one and revives it. The match doesn't end
just because every human is out: the built-in AI demigods fight on, and spectators watch. If every human
**disconnects**, the match pauses.

## A minimal bot (~40 lines)

```python
ws = connect("ws://10.0.0.34:8080/ws")
send = lambda t, d, id=None: ws.send(json({"t": t, "id": id, "d": d}))
send("auth.login", {"display_name": "Botty", "client": {"kind": "my-bot", "version": "1"}}, "1")
send("room.quickjoin", {}, "2")              # or room.create {visibility, settings} / room.join {code}
send("room.ready", {"ready": True}, "3")
me = None; view = None; last_ping = now()

while True:
    if now() - last_ping > 20: send("sys.ping", {}); last_ping = now()
    m = ws.recv(timeout=1)
    if not m: continue
    t, d = m["t"], m.get("d", {})
    if t == "auth.ok":                save(d["resume_token"])            # for auth.resume after a drop
    elif t == "room.snapshot":       me = seat_of(d["seats"], my_player_id)
    elif t == "round.brief":         view = d["private"]                # SeatView: you, visible, debts, news
    # Every submission with orders is text entry (it slows time and costs a bolt); a real bot speaks every few beats and
    # casts now and then (fields.cast). Without anything to say, don't submit: the flow beat runs on.
    elif t == "phase.changed" and d["phase"] == "compose" and view and view["alive"] and d["round"] % 3 == 0:
        rivals  = [e for e in view["visible"] if e["kind"] == "demigod" and e["ref"] != view["demigod"]]
        crowds  = [e for e in view["visible"] if e["kind"] == "cluster"]
        left, right = [], []
        if outside_arena(view["you"]["pos"], latest_public_arena):       # from round.brief.public.arena
            left.append("go to " + nearest_district_to_center(view["visible"]))
        elif rivals and view["you"]["health"] >= 6:
            left.append("attack " + rivals[0]["name"])
        elif crowds:
            left.append("recruit the " + crowds[0]["name"])
        if view["you"]["debt_open"] > 2:   right.append("slow time and delay my debt")
        elif len(view["hands"][1]["subagents"]) == 0 and rivals:
            right.append("send a scout to spy on " + rivals[0]["name"])
        send("action.submit", {"round": d["round"], "text": "",
             "fields": {"left": "\n".join(left), "right": "\n".join(right), "zoom": "district",
                        "sent_at": now_ms()}}, "a%d" % d["round"])
    elif t == "round.outcome":       show(d["public"]["caption_beat"], d["public"]["previous_caption"], d["public"]["omens"])
    elif t == "round.private":       show(d["private"]["hands"], d["private"]["news"])
    elif t == "round.private_state": view = d["state"]
    elif t == "seat.eliminated" and d["seat"] == me: print("we fell:", d["reason"])   # keep watching
    elif t == "match.ended":         print(d["summary"]["champion_seat"], d["summary"]["standings"]); break
    elif t == "error":               log(d["code"], d["message"])       # bad_request / wrong_phase / too_long
```

For an LLM bot, pass the brief's `public.rules`, your SeatView and the last `round.private` to the model and ask it for
`{left, right, zoom}`. The real implementation is `crates/fn7-tools/src/bin/agents.rs` (`fn7-agents`, with `--claude`
for LLM players). Run it against a server for a full 4-player match:
`fn7-agents --url ws://10.0.0.34:8080/ws --players 4 --rounds 6`.

## Things that trip clients up

1. **Two paths:** orders (text entry) and casts are separate. Typed words never cast; a cast never carries orders or
   slows time. Send `sent_at` with everything.
2. **Don't submit when idle:** Compose closes once every seat has submitted, which shortens the flow beat for everyone.
3. **`text` vs `fields`:** `text` is capped at 500 chars by the core, while `fields.left`/`right` allow 2000 each. If
   either hand field is present, `text` is ignored completely.
4. **The narration is one beat late:** `round.outcome` of beat N narrates beat N−1 (`caption_beat`). Don't label beat
   N's events with it. The last beat's caption is `match.ended.summary.last_caption`.
5. **`[lon, lat]`**, not `[lat, lon]`.
6. **`winners` can be empty** when a built-in AI demigod wins. Use `summary.champion_seat` / `summary.champions`.
7. **The beat ends early** when every active seat has submitted, so don't count on getting the full `flow_ms` to
   edit.
8. **`llm: true` in `rules`** doesn't mean an LLM is in use. If the server has no LLM configured, every mind falls
   back silently; check `round.outcome.public.timing.llm_fallbacks`.
9. **Prose is not truth.** Captions, callouts, speeches and news text are written by LLMs. The structured fields
   (`eliminated`, `alive`, `champions`, `standings`) are the result.
10. **`round.started` deaths come first:** it can eliminate you before the brief, and the match can end right after
    a Start (then no brief follows, just `match.ended`).

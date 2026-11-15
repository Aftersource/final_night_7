# Final Night 7: client API

Final Night 7 runs as a game plugin (`final_night_7`) inside the Theatre server. This file covers what a client needs to
play it: the game-defined objects inside the Theatre's generic messages, the action format, the room settings, the
timing, and who sees what. The envelope, login/resume, the lobby, rate limits and the generic message list are in the
Theatre's [`PROTOCOL.md`](../../theatre/PROTOCOL.md) (also in the repo root of
[Aftersource/theatre](https://github.com/Aftersource/theatre)). Read that first; this file doesn't repeat it.

- **Schemas:** [`docs/schemas/`](schemas/) has a JSON Schema (draft 2020-12) for every game payload and every query
  reply (`query_<name>`, with its args, see "Game queries"). They're generated
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
inputs. **Text entry** is the God of Time interaction: you start typing and time slows (a Theatre action window;
everyone is told who did), type orders to your two
**hands** (LLM orchestrators that act, raise **armies**, spawn **subagents**, possess people and invoke the seven gods:
Time, Nature, Consequence, Death, Chance, Memory and **Entropy**), and your Enter sends them and cuts off anyone who
typed slower. **Casts** are god powers (lightning, a volcano, a flood, a custom power...) thrown at any point of the
globe at any time as instant actions (everyone sees the streak at once), paid in **lightning bolts** (megas) and **sparks** (minis; rules 0.7.0, see "Mega and mini powers"): a mega gives every other player a bolt, a bolt paid to a god gives every other player 2 (Time's toll for orders gives nobody anything), and every cast raises
**Entropy**, which makes the world wackier until heat death brings the Final Night at once. Every act runs up a bill with
Consequence. Time's clock shrinks a circle toward one city. The last player standing wins; on the Final Night exactly
one player survives. Demigods no seat plays are played by the plugin's own AI (no Theatre seat).

**Rules 0.7.0 (state schema 5): two demigods per player.** Your Left and Right hands *are* your two demigods. Read the
next section; it changes how several payloads are read.

## Two demigods per player (rules 0.7.0)

Each player (a seat, or a built-in AI player) has **two demigods**, its **Left** and **Right** hands. Each is a body on
the globe with its **own health** (10), position, hex cell, items, deeds, look, portrait and diorama. Each can be
wounded, eliminated and revived on its own. **A player is out only when both have fallen** (`seat.eliminated` comes
once, then; `seat.revived` when either stands again). The room setting `demigods_per_player: 1` plays the old game (one
shared body).

- **Names.** `"<player>'s Left Hand"` / `"<player>'s Right Hand"` (the media layer's existing names, so portraits and
  dioramas stay cached). `player` on every demigod is the player's name.
- **Ids.** The Left demigods come first (`dg:0..n`, in seat order), then the Right (`dg:n..2n`). Use `side`, `seat` and
  `sibling`, never id arithmetic.
- **Orders.** Your left text (`fields.left`, or `l:` lines) drives the Left demigod; the right text (`fields.right`,
  `r:`) the Right one. If one has fallen, both texts go to the one standing.
- **Bolts are the player's purse.** Both demigods show the same `bolts`. A cast or any bolt spent gives every rival
  *player* 2 per bolt, once. Casts are cast by your **lead** demigod (your first one standing): `streak.by` is your
  player name, `by_ref` the lead demigod.
- **Debts are per demigod**: the body that acted owes. Time's debt for an order is split evenly between the demigods
  that got orders (the bolt comes once from the purse). `seat_view.debts[].owed_by` says which body owes.
- **Every rule applies per body**: Time's circle, armies storming (never a sibling's army; siblings are never rivals),
  accidents, a cast's area, gods' prices (Life lowers *that* body's cap), revive, Entropy's flinging, possession.
- **Hidden health stays per body**: rivals never see either of your demigods' health; you see both of yours.
- **The Final Night leaves exactly one player**: the most total standing over its demigods (health now minus debt
  still owed), then the most total health at the beat's start, then the lowest demigod id. That player's standing
  demigods stand (one or both); every other player's fall. If none of theirs stands, its demigod with the most
  start-of-beat health comes back.
- **Standings and the champion are per player** (`summary.standings[]`, `summary.players[]`, `champion_seat`,
  `champion_player`), each with both demigods' state. `summary.champions[]` lists the champion player's standing
  demigods (both, if both stand).
- **The Final Wish.** Beat 1 is the Final Wish: `round.brief.public.final_wish: true`. Your round-1 orders are your
  final wish: anything you can do. Both hands get the window act budget (`max_intents_per_window`) and every act gets
  `final_wish_magnitude` (2) more magnitude. It costs as normal (Entropy loves it). The built-in AI wishes too.

### Where the two demigods appear

| Payload | What's there |
|---|---|
| `public_state.demigods[]` | Every demigod (2 per player): `ref, name, player, side, seat, sibling, alive, pos, cell, place, region, bolts, controller, connected, aura, visual, portrait_prompt, diorama_prompt, items[]` (+ `portrait_image`, `diorama_image` with media on). Never `health`. |
| `public_state.players[]` | The seat list: `{index, seat, player, aura, alive, bolts, demigods: [left, right], time_bill, controller, connected}` |
| `round.brief.private` / `seat_view` | `demigods[]`: **your two demigods in full** (the public fields plus `health, max_health, debt_open, items[], hand, orders, manifests[]`); `my_items[]`; `player`, `aura`, `time_bill`. The top-level `demigod`, `name`, `you` and `max_health` describe your **lead** demigod (the first standing). `alive` is true while either stands. |
| `seat_view.hands[]` | Kept: `[left, right]`, each now with `demigod` (its ref), `side`, `health`, `alive`. A hand's `name`, `portrait_prompt`, `diorama_prompt` and `visual` are the same as its demigod's. |
| `round.private.private` (SeatDelta) | `demigods[]` `{ref, side, name, health, max_health, alive, pos, cell}` and your `manifests[]` this beat (with verdicts). `you` is the lead demigod. |
| `round.started.private` | `demigods[]` `{ref, side, health, alive, outside_arena, debt_due}`; the top-level fields are the lead's. |
| `round.outcome.public` | `players_alive`; `manifests[]` (public manifestations this beat). `alive` still counts demigods. |
| `match.ended.summary` | `standings[]` per player (`player, aura, demigods[]`), `players[]` (for profiles), `champion_player`, `manifests[]`; `piles[]` stay per demigod, items now with `kind, icon, tier, count, unit, state`. |

### Hex cells (Benito's people-level rule)

Every demigod, named person (`Who`, `entity` positions) and crowd or mob piece carries `cell`: its **people-level hex
cell**, in your own indexing, as `"<face>:<level>:<q>:<r>"` (level `people`; crowds also give `village`):

- `LatLonToVec(lat, lon) = (cosLa·cosLo, sinLa, −cosLa·sinLo)`, Y up.
- Cube face (gnomonic): `Ax=|x|, Ay=|y|, Az=|z|`. If `Ax≥Ay && Ax≥Az`: `x>0` → face 0 (`U=−z/Ax, W=y/Ax`), else
  face 1 (`U=z/Ax, W=y/Ax`). Elif `Ay≥Az`: `y>0` → face 2 (`U=x/Ay, W=−z/Ay`), else face 3 (`U=x/Ay, W=z/Ay`). Else
  `z>0` → face 4 (`U=x/Az, W=y/Az`), else face 5 (`U=−x/Az, W=y/Az`).
- Plane metres `X=U·R, Y=W·R`, `R = 6,371,000`.
- Pointy-top axial: `qf=(√3/3·X − Y/3)/S, rf=(2/3·Y)/S, sf=−qf−rf`, round each, fix the largest rounding error.
- Levels: people 3 m, houses 30 m, villages 3 km, regions 120 km.

Cross-check table (from `crates/fn7-model/src/hexgrid.rs`, a tested pure module): Paris (2.3522, 48.8566) =
`2:people:1044945:50767`, `2:regions:26:1`; New York (−74.006, 40.7128) = `4:people:-282246:1267371`; Sydney
(151.2093, −33.8688) = `1:people:-131656:-1084278`; Tokyo (139.6917, 35.6895) = `1:villages:-1707:1334`.

**The rule:** no two standing demigods share a people-level cell unless they are **engaged** (one acted directly on
the other this beat: a fight to the death, a seduction, a parley). Otherwise the later mover is nudged to the nearest
free neighbouring cell. Nudges are a few metres; the server's positions are always legal, so draw them as given.

### Items and manifestations (the item engine)

An order to make something ("raise banners of 100,000,000 soldiers", "forge a sword of stars", "summon a dragon") goes
to the **item engine**: rules decide the tier, the cost in bolts (the shortfall becomes debt to Consequence, unless the
order says "no debt"), the odds and one seeded roll: `success`, `twisted`, `partial`, `failed`, `backfired`. The LLM
only names what appears. Every attempt is a **Manifestation**; a made thing is an **Item**. Shapes:
[`schemas`](schemas/) `$defs/Item` and `$defs/Manifestation`.

- **Item:** `{ref, name, description, kind, icon, tier, count, unit, state, holder, holder_name, created_beat,
  manifestation, at, banner, image_prompt?, image_kind?: "item", art?}` (state schema 11: only an item with item art
  has `image_prompt`, `image_kind` and `art`; see "Gear looks"). `icon` is your piece: `sword`, `ranged`, `armor`,
  `banner_army`, `vehicle`, `beast`, `structure`, `relic`, `artifact`, `phenomenon`, `thing`. `tier`: `trinket`,
  `artifact`, `relic`, `wonder`, `legend`, `myth`, `titan`, `apocalypse`. `count` can be 100,000,000 (a double).
  `state`: `held`, `deployed` (a host in the field: its army appears in `armies`), `destroyed`, `transferred`, `stolen`.
- **Manifestation:** `{ref: "mf:N", beat, by, by_name, side, kind, icon, tier, count, unit, outcome, item, name, at,
  region, public}`; your own also carry `verdict` (odds with their terms, the roll, cost, reasons) and `request`.
- Public: made things of tier `relic` or up, and attempts of tier `legend` or up. Your own: everything.
- `game.items` and `game.item` page and open them.

### Mega and mini powers: the 1:5 economy

Every player holds **lightning bolts** (the mega currency) and **sparks** (the mini currency), a purse both of its
demigods share and everyone can see (`bolts`, `sparks` on every demigod and in `players[]`; `charges: {bolts, sparks}`
in `players[]` and the seat view; `you.sparks`). Players start with `bolt_start` 1 : `spark_start` 5.

- A cast says `fields.cast.tier`: `"mega"` or `"mini"` (absent = mini, so old clients never burn bolts; `bolt`/`bolts` shorthands are minis). The full cast: `action.submit { instant: true, fields: { cast: { power, tier, target } } }`.
- **MEGA** (map scale, level 1): costs **1 bolt**; every OTHER player gains `bolt_gift` (1) bolt (and `spark_gift`, 0,
  sparks). Its target takes the mega damage and every rival demigod within the mega radius 40% of it. Entropy +10.
- **MINI** (the board: a few hexes): costs **1 spark**, gifts nothing, hits its target only. Entropy +1.
- `round.brief.public.powers[]` gives each power's `mega` and `mini` variants (`radius_km`, `magnitude`, `damage`,
  `cost: 1`: a bolt for mega, a spark for mini). The streak and `casts[]` carry `tier` and the variant's `radius_km`.
- Validation counts megas against your bolts and minis against your sparks at the last commit; a rival's gift this beat
  is spendable from the next. Bolts still pay Time's toll and gods' prices.
- Hoarding is visible and is part of the game. The built-in AI casts minis often, megas sometimes, and keeps a bolt.

### Aura colours

`aura: "#RRGGBB"` per player (both demigods share it), on every demigod, in `players[]`, the seat view and the
standings. A stable palette by player index: `#E69F00, #F0E442, #009E73, #F28CC0, #D55E00, #A6D854, #FFFFFF,
#C9A27E` (Okabe-Ito based, readable on dark cobalt and violet; no cyan, no grey).

### Time's live bill

While a window is open, show the opener's bill growing. The `slowed` phase event carries `opened_at_ms`,
`deadline_ms`, `bill_now {bolts, debt}`, `debt_base`, `debt_per_second`, `debt_per_10s`, `debt_cap`, `repeat_factor`,
`recent_slows` and `bolt_paid`: `debt(t) = min(debt_cap, debt_base + debt_per_second × t)` with `t` the seconds since
`opened_at_ms`, plus one bolt. `players[].time_bill` and `seat_view.time_bill` give the same numbers before anyone
opens a window.

### Hotspots

`game.globe.regions[]` is `{ref, name, center, hotspot}` and `game.region.hotspot` the same object: `{pos: [lon, lat],
radius_km, score, why}`, the region's most dynamic area over the last 5 beats (public events, battles, mobs, crowds in
unrest, demigods present), or `null` where nothing happens. Open zoom level 2 centred on it when the globe is clicked.
The built-in AI drifts toward hotspots, so the action gathers there.

## DM rulebook features (0.8.0)

Rules **0.8.0** (state schema **6**) add the Dungeon Master: a rulebook ([`docs/DM_RULEBOOK.md`](DM_RULEBOOK.md), the
single rules reference the AI DM follows), a hierarchical **place × time memory** that keeps every DM result
consistent, and the eight features it governs. Everything below is absent from schema-4/5 matches and recordings. All
of it is public unless it says otherwise. The examples come from offline matches (no LLM); the shapes are exact
(`docs/schemas/`: `DiceRolled`, `DmEvent`, `Story`, `Settlement`, `Encounter`, `LootToken`, `Road`, `Shrine`,
`MemoryEntry`, `HexState`, `TimeOfDay`).

| Feature | Where it shows |
|---|---|
| The d20, visible | `round.outcome.public.dice[]` (`dice.rolled`) |
| Every DM decision | `round.outcome.public.dm_events[]`; `game.memory` |
| 1 Growing settlements | `settlement` on every city node (`game.region.cities[]`, `game.node`); `settlement.grew` / `settlement.shrank` events |
| 2 Your piece carries its story | `public_state.demigods[].story` (and `game.globe`, `game.region`, `seat_view.demigods[]`) |
| 3 The d20 | `dice[]` |
| 4 Map encounters | `game.encounters`; `encounters[]` on `game.region` / `game.node`; `encounter.*` events |
| 5 Loot left behind | `loot[]` on `game.region` / `game.node`; `loot.dropped` / `loot.taken` events |
| 6 Day and night | `time_of_day` + `time_of_day_phase` on `round.brief.public`, `round.outcome.public`, `public_state`, `game.globe`, `game.region`, `game.node` |
| 7 Paths wear in | `roads[]` on `game.region` / `game.node` / `game.globe`; `road.formed` / `road.faded` events |
| 8 Shrines | `shrines[]` on `game.region` / `game.node` / `game.globe` / `public_state`; `shrine.built` / `shrine.level` events |
| Zooming in | `game.node {cell}` (a hex's state, observed or extrapolated); `observed_beat`, `extrapolated` |
| Fault lines | `fault_km`, `fault_factor` on a mega's streak (`phase.event`) and its `casts[]` line |
| Summoning | `fields.summon`; `phase.event {event: "summoning"}`; `round.outcome.public.summons[]` (`summoned`) |
| God-tier powers | `powers[]` with `tier: "god"`; `fields.cast {tier: "god"}`; `round.outcome.public.god_powers[]` |

### The d20: `dice.rolled`

Every d20 the DM rolls (an act's check, a manifestation, an encounter, keeping hold in a fight) is one entry of
`round.outcome.public.dice[]` (at most 60 a beat), in the order rolled. Animate a die tumbling on the table at `cell`.

```json
{ "event": "dice.rolled", "who": "dg:0", "die": "d20", "roll": 4, "modifier": 3, "total": 7, "target_dc": 11,
  "verdict": "fail", "reason": "a journey", "at": [135.55, 34.67], "cell": "1:people:-1888595:1371607", "beat": 1 }
```

- `verdict`: `crit_fail` (a natural 1, whatever the total), `fail`, `success`, `crit_success` (a natural 20).
- `reason` names only the act's class ("a strike", "a parley", "a threat", "a journey", "a making", "a deed"), an
  encounter ("a trap encounter") or a manifestation ("manifesting a relic"): never the order's words.
- Rolls are seeded and remembered: the same question always gives the same roll, and each is shown once.

### DM events: `dm_events[]`

`round.outcome.public.dm_events[]` (at most 40 a beat): every other public decision the DM made this beat. Each has
`event`, `text` (one line, show it in the feed), `memory` (its id in `game.memory`) and the decision's fields:

```json
{ "event": "settlement.grew", "place": "pl:3", "name": "Melbourne", "from": 3, "to": 4, "size": 2029458,
  "center_cell": "1:people:-188404:-1341958", "settlement_cells": ["1:villages:-188:-1342", "1:villages:-189:-1341", "…"],
  "text": "Melbourne grows to level 4", "memory": 139 }
{ "event": "settlement.building", "place": "pl:66", "acts": 1, "text": "builders are at work in Madrid", "memory": 74 }
{ "event": "encounter.marked", "id": 0, "cell": "0:people:329189:-79048", "marked": true,
  "text": "a marked hex appears near Middle Africa", "memory": 9 }
{ "event": "encounter.revealed", "id": 15, "cell": "0:people:334914:-80126", "type": "trap", "by": "dg:3", "verdict": "fail",
  "roll": 3, "text": "a snare on the marked hex catches Dune's Left Hand", "damage": 1.0, "item": null, "character": null,
  "sparks": 0, "memory": 926 }
{ "event": "encounter.faded", "id": 0, "cell": "0:people:329189:-79048", "text": "a marked hex fades, unfound", "memory": 583 }
{ "event": "loot.dropped", "pile": 0, "cell": "5:people:471324:1283153", "from": "dg:0",
  "items": [{ "ref": "it:3", "icon": "sword" }], "why": "a death", "text": "Ash's Left Hand's things lie on the ground after a death", "memory": 410 }
{ "event": "loot.taken", "pile": 0, "cell": "5:people:471324:1283153", "by": "dg:5", "items": ["it:3"],
  "text": "Brix's Right Hand takes what Ash's Left Hand left on the ground", "memory": 431 }
{ "event": "road.formed", "from_ref": "pl:27", "to_ref": "pl:28", "from_cell": "2:people:575759:593958",
  "to_cell": "2:people:805405:635211", "wear": 5.27, "text": "a road wears in between Kyiv and Bucharest", "memory": 696 }
{ "event": "shrine.built", "shrine": 0, "cell": "5:houses:47132:128315", "owner": "dg:2", "power": "spark",
  "text": "the Shrine of Cyra's Left Hand is raised (spark)", "memory": 512 }
{ "event": "shrine.level", "shrine": 0, "level": 2, "worshippers": 1430, "text": "the Shrine of Cyra's Left Hand reaches level 2", "memory": 530 }
{ "event": "story.scar", "demigod": "dg:0", "scar": { "beat": 4, "cause": "Cyra's Left Hand casts meteor on Ash's Left Hand in Eastern Asia",
  "place": "Eastern Asia", "cell": "5:people:471324:1283153" }, "text": "Ash's Left Hand is scarred: …", "memory": 281 }
{ "event": "story.wardrobe", "demigod": "dg:2", "stage": 1, "name": "marked", "text": "Cyra's Left Hand now wears marked", "memory": 363 }
{ "event": "story.followers", "demigod": "dg:0", "followers": 29959, "milestone": 10000,
  "text": "Ash's Left Hand has 10000+ followers", "memory": 12 }
{ "event": "time.phase", "day": 1, "phase": "dawn", "light": 0.5, "text": "Dawn breaks on day 1", "memory": 8 }
```

Other kinds: `settlement.shrank` (as `grew`), `road.faded` (as `formed`).

### 1. Growing settlements

Every city node (`game.region.cities[]`, `game.node` on a city, `children[]`) carries `settlement`, plus
`observed_beat` and `extrapolated` (always the beat now and `false`: cities are simulated every beat):

```json
"settlement": { "level": 3, "size": 1443128, "prosperity": 0.5, "target": 3, "since_beat": 30,
                "center_cell": "0:people:391266:-111087",
                "settlement_cells": ["0:villages:391:-111", "0:villages:390:-110", "…19 hexes at level 3"] }
```

`level` 1 hamlet, 2 village, 3 town, 4 city, 5 great city (6–7 legendary). `settlement_cells` are village-level hexes:
the centre plus rings out to level − 1 (1, 7, 19, 37, 61), so draw the village bigger as it levels up. `target` is
where the rules are taking it (one step at a time).

### 2. Your piece carries its story

`public_state.demigods[].story` (also on every demigod in `game.globe`, `game.region` and `seat_view.demigods[]`):

```json
"story": { "followers": 50990, "items": [{ "ref": "it:7", "icon": "sword" }],
           "wardrobe": { "stage": 2, "name": "adorned", "condition": "scorched" },
           "scars": [{ "beat": 4, "cause": "Cyra's Left Hand casts meteor on Ash's Left Hand in Eastern Asia",
                       "place": "Eastern Asia", "cell": "5:people:471324:1283153" }],
           "deeds": 9.0 }
```

- `items`: every item it holds now, as pegs (`icon` from the item icon set).
- `wardrobe.stage` 0 plain, 1 marked, 2 adorned, 3 regalia, 4 mythic (never goes down); `condition` `scorched` (hit by
  fire this beat), `ragged` (health 3 or less) or `null`. Health stays private: `ragged` is visible damage, by design.
- `scars`: the newest 12, oldest first.

### 4. Map encounters

`game.encounters {region?}` (public; `region` a `pl:` ref):

```json
{ "beat": 40, "region": null, "hidden": 10, "revealed": 1, "encounters": [
  { "id": 12, "cell": "1:people:34935:-1120114", "marked": true },
  { "id": 15, "cell": "0:people:334914:-80126", "marked": true, "revealed": true, "type": "trap", "night": true,
    "at": [13.52, -3.15], "region_ref": "pl:33",
    "outcome": { "beat": 39, "by": "dg:3", "by_name": "Dune's Left Hand", "roll": 3, "modifier": -1, "total": 2,
                 "target_dc": 13, "verdict": "fail", "text": "a snare on the marked hex catches Dune's Left Hand",
                 "damage": 1.0, "item": null, "item_icon": null, "character": null, "sparks": 0 } } ] }
```

- Hidden: only `{id, cell, marked: true}`. Draw a marked hex; never its type. The `cell` is people- or house-level.
- Revealed: `type` (`monster`, `treasure`, `trap`, `stranger`; at night `undead`, `ghost`, `thief`) and `outcome`,
  forever. Faded ones leave the list.
- A demigod **steps on** it when its move passes within `dm_reach_km` (30 km on the planet) of it; the reveal comes with
  its `dice.rolled` and an `encounter.revealed` event in the same outcome.
- `game.region` and `game.node` (a place) carry the same `encounters[]` for that place.

### 5. Loot left behind

`game.region.loot` / `game.node.loot` (a place): one entry per item token lying on a hex:

```json
"loot": [{ "ref": "it:3", "icon": "sword", "cell": "5:people:471324:1283153", "since_beat": 31, "pile": 0, "at": [116.4, 39.9] }]
```

A death drops everything the demigod carried; a fight's loser may drop its newest item. The next demigod to step there
takes the pile (`loot.taken`).

### 6. Day and night

`round.brief.public` (the coming beat), `round.outcome.public` (the beat that resolved), `public_state`, `game.globe`,
`game.region` and `game.node`:

```json
"time_of_day": { "day": 1, "phase": "night", "light": 0.15, "beat_in_day": 6, "beats_per_day": 8 },
"time_of_day_phase": "night"
```

`phase` is `dawn`, `day`, `dusk` or `night`; `light` 0 (midnight) .. 1 (noon), 0.5 at dawn and dusk. `beats_per_day`
is a room setting (8). Night brings undead, ghosts and thieves.

### 7. Paths wear in

`game.region.roads`, `game.node.roads` (a place: roads with an end under it) and `game.globe.roads` (all):

```json
"roads": [{ "from_cell": "2:people:575759:593958", "to_cell": "2:people:805405:635211", "wear": 5.27, "since_beat": 33,
            "from_ref": "pl:27", "to_ref": "pl:28", "from_name": "Kyiv", "to_name": "Bucharest",
            "path": [[30.52, 50.45], [26.1, 44.43]] }]
```

A road runs between two settlements' centre hexes; `path` is the two centres (draw a line or a hex path). `wear`
grows with traffic and decays when unused; a faded road leaves the list (`road.faded`).

### 8. Shrines to your demigod

`game.region.shrines`, `game.node.shrines`, `game.globe.shrines` and `public_state.shrines`:

```json
"shrines": [{ "id": 0, "cell": "5:houses:47132:128315", "owner_seat": 2, "side": "left", "worshippers": 1430,
              "power": "spark", "level": 2, "owner": "dg:2", "owner_name": "Cyra's Left Hand", "player": "Cyra",
              "name": "Shrine of Cyra's Left Hand", "at": [116.4, 39.9], "region_ref": "pl:9", "built_beat": 12,
              "owner_alive": true }]
```

`power`: `spark` (+1 spark to its player every few beats), `heal` (heals its player's demigods nearby) or `ward`
(wounds rivals nearby, a little). Raise one with an order such as "raise a shrine to me here" (a making verb and a
shrine word) or by manifesting a structure. `owner_seat` is `null` for the built-in AI.

### The memory: `game.memory {place, until_beat?, limit?}`

Public. `place`: a hex key, a `pl:` ref (settlement or region) or `"world"`. Returns the DM's public decisions about it,
or anything above or below it, at or before `until_beat` (default: now), newest first (`limit` 50, at most 200):

```json
{ "place": "pl:33", "ancestors": ["world"], "until_beat": 40, "beat": 40, "observed_beat": 40, "extrapolated": false,
  "items": [{ "id": 933, "beat": 39, "scope": "region", "place": "pl:33", "up": ["world"], "kind": "memory.rollup",
              "query": "rollup:pl:33:39", "text": "in Middle Africa: a snare on the marked hex catches Dune's Left Hand",
              "data": { "counts": { "dice.rolled": 2, "encounter.revealed": 1 }, "facts": 3,
                        "notable": ["a snare on the marked hex catches Dune's Left Hand"],
                        "state": { "population": 3905636, "stance": "fleeing", "supply": 1.01 } } }] }
```

- `memory.rollup` entries are the **integrated** truth of everything beneath a place at a beat (counts by kind, up to
  three notable events, its state). `game.region.memory_summary` is the region's latest one.
- For a hex, the reply adds `state` (as `game.node {cell}` below), at `until_beat`.

### Zooming in: `game.node {cell}`

`game.node` takes `cell` (a hex key, any level) instead of `ref`. The server doesn't simulate every 3 m hex every beat:
a hex is **extrapolated** from its last observation (or from its genesis) by its parent's changes since, and recorded
as a fact when the DM observes it (a demigod arrives there, a seat zooms on something there, the DM decides something
there). The same hex at the same beat is the same for everyone.

```json
{ "kind": "hex", "cell": "2:houses:36707:58825", "beat": 40, "observed_beat": null, "extrapolated": true,
  "node": { "cell": "2:houses:36707:58825", "level": "houses", "center": [37.613637, 55.754198],
            "settlement": "pl:26", "region": "pl:25", "people": 1, "buildings": 11, "parent_population": 4217477,
            "settlement_level": 4, "terrain": "in Moscow, Russia; at the center, on land", "encounters": [],
            "loot_items": 0, "shrines": [], "on_road": true, "weather": "overcast",
            "time_of_day": { "day": 5, "phase": "night", "light": 0.15 },
            "changes_since": ["Moscow grows to level 5", "Moscow shrinks to level 4"] } }
```

`observed_beat`: the DM's last observation (`null`: never); `extrapolated`: `false` when observed this beat.
`game.region` and place `game.node` replies carry `observed_beat` (now) and `extrapolated: false`.

### Fault lines (megas)

A mega's strength follows its distance to the nearest plate boundary (rulebook chapter 13). The streak
(`phase.event`, top level and each `casts[]` entry) and the resolved `casts[]` line add:

```json
{ "tier": "mega", "power": "volcano", "radius_km": 1500.0, "fault_km": 42, "fault_factor": 1.6 }
```

`fault_km` is the great-circle distance; `fault_factor` the damage multiplier (1.6 within 150 km .. 0.6 beyond
1,500 km, linear between; magnitude and radius scale the same way). Show the fault lines only while a mega is armed
(the client's copy of `data/geo/PB2002_boundaries.json`). Credit: "Plate boundaries: Bird, P. (2002), PB2002; GeoJSON
by Hugo Ahlenius, Nordpil (ODC-BY 1.0)."

### Summoning your demigod (drag it onto the board)

Send an **instant** action (any phase) with `fields.summon`:

```json
{ "t": "action.submit", "d": { "round": 12, "instant": true,
  "fields": { "summon": { "demigod": "dg:0", "target": [-21.94, 64.15] } } } }
```

`target` is `[lon, lat]` or a hex key (`"2:people:679163:-256291"`). A refusal is `action.received {accepted: false,
reason}` with `reason` one of `not_yours` (not one of your demigods), `fallen`, `cooldown` (summoned within
`summon_cooldown_beats`, 1: once per demigod per beat), `bad_target`, or `summon_disabled` (room setting
`summon_enabled: false`). On accept everyone gets a `phase.event` at once (flash and whoosh now; it lands at the next
resolve):

```json
{ "event": "summoning", "demigod": "dg:0", "from": [134.012498, 37.414668], "to": [-21.94, 64.15], "by_seat": 0,
  "by": "Ash", "by_ref": "dg:0",
  "summons": [{ "demigod": "dg:0", "to": [-21.94, 64.15], "cell": "2:people:679163:-256291" }],
  "god_powers": [], "seq": 99, "received_at_ms": 1763140000000 }
```

At the resolve it lands **this beat** (nudged off an occupied people-level hex), with its mishap d20 in `dice[]`
(`reason: "a summoning"`) and the `summoned` event in `round.outcome.public.events[]` (with `kind: "summoned"` and `at`
its landing), in `round.outcome.public.summons[]` and in `dm_events[]`. `public_state.demigods[].pos` / `cell` show the
new place at the commit.

```json
{ "event": "summoned", "kind": "summoned", "demigod": "dg:0",
  "from": { "pos": [134.012498, 37.414668], "cell": "2:people:-1779194:1331058" },
  "to": { "pos": [-21.94, 64.15], "cell": "2:people:679163:-256291" }, "at": [-21.94, 64.15],
  "deed": "Mid-go to Western Europe in Eastern Asia, Ash's Left Hand was folded into a paper airplane by an impatient god and bounced twice off a tram before it stuck the landing in Northern Europe.",
  "health_lost": 2.0, "roll": 7, "text": "…the deed…", "memory": 136 }
```

Ordered travel by `l:` / `r:` text is unchanged; a summoned demigod's own travel that beat is void and its standing
orders end.

The deed is also a tale on its piece: `story.tales[] = [{beat, kind: "summoned", text}]`. A mishap costs 1–2 health,
likelier the healthier it is (85% at full health; never below 2.5 health).

### God-tier powers

`round.brief.public.powers[]` (and `game.powers`) lists seven more powers with `tier: "god"`:

```json
{ "slug": "god_death", "name": "The Pale Verdict", "icon": "god_death", "cost": 3, "radius_km": 0.0, "target": "unit",
  "magnitude": 8, "damage": null, "tags": ["god", "death"], "tier": "god", "god": "death", "god_ref": "gd:3",
  "blurb": "raise the fallen or strike the living" }
```

Cast one like any power, as an instant action:

```json
{ "fields": { "cast": { "power": "god_death", "tier": "god", "target": "dg:5" } } }
```

It costs `god_power_cost` (3) bolts and gives every other player a bolt (like a mega). At the next resolve it is
**that god's ruling**: the god's verdict and speech arrive in `omens[]` as for any invocation, and unless it refuses,
its signature effect lands (rulebook chapter 20). `round.outcome.public.god_powers[]`:

```json
{ "by": "Ash", "by_ref": "dg:0", "power": "god_entropy", "power_name": "The Chaos Burst", "tier": "god",
  "god": "entropy", "god_ref": "gd:6", "cost": 3, "seq": 99, "at": [134.01, 37.41], "target": null,
  "place": "Eastern Asia", "region_ref": "pl:9" }
```

A god-tier power that can't be paid has `fizzled` (nothing spent). The accept-time push is `phase.event`
`{event: "god_power", ...}` (the `summoning` shape with `god_powers[]` filled).

### Settings

`beats_per_day`, `dm_reach_km`, `dm_encounters_per_beat`, `dm_encounters_max`, `dm_encounter_fade_beats`,
`dm_encounter_damage_scale`, `dm_settlement_step_beats`, `dm_road_wear`, `dm_road_decay`, `dm_shrines_per_demigod`,
`dm_shrine_spark_every`, `dm_shrine_heal`, `dm_shrine_ward`, `dm_memory_lines` and the eight `fault_*` settings
`summon_enabled`, `summon_cooldown_beats`, `summon_loss_scale`, `god_power_cost`, `god_power_gift` (defaults and
ranges in the rulebook's appendix and `docs/schemas/settings.schema.json`).

## Challenge mode (0.8.1)

Rules **0.8.1** (state **schema 7**) add typed duels (rulebook chapter 21, "Challenge mode: will against will"):
Archon's strategy board with fights decided by real-time play, the real-time play being typing. Everything new is
gated on the state schema (a schema-6 or older match never duels and replays unchanged) and on the
`challenge_enabled` setting. Schemas: `ChallengeLock`, `DuelBlow`, `DuelStrike` in `phase_event` and
`round_started_public`; `ChallengeOpen`, `ChallengeEnd` in `outcome_public`; `wills` in `round_private`.

**Rules decide, an LLM only phrases.** Every number below (the die, the move class, the matrix, the damage, the
poses) comes from deterministic, seeded rules; the only LLM text is the one-line deed at the end.

### Challenge (double-click a rival)

An instant action that travels alone (no orders, casts or summons in the same action). `UFNNetClient::Challenge`
sends it:

```json
{ "t": "action.submit", "d": { "instant": true, "fields": {
    "challenge": { "demigod": "dg:0", "target": "dg:1", "level": "regions" } } } }
```

- **Range (direct combat must be very easy to engage):** accepted when your demigod and the rival's are within
  `max(challenge_min_km, contact_km(level))` (great circle), where `level` is the board level you're looking at:

  | `level` | contact_km (two board cards touching) | effective reach |
  |---|---|---|
  | `regions` (also when `level` is missing or unknown) | 600 | **600 km** |
  | `villages` | 15 | 120 km |
  | `houses` | 0.15 | 120 km |
  | `people` | 0.015 | 120 km |

  So: the cards touch at the current zoom, or they're within 120 km at any zoom. The client challenges on exactly
  that test; the server re-checks with these numbers (settings `challenge_min_km` 120 and
  `challenge_contact_km_regions` / `_villages` / `_houses` / `_people`). `level` also frames the fight (the lock's
  `cell`).
- It is forced (no consent).
- **Refusals** arrive as `action.received {accepted: false, reason}` (show them as toasts; match `too_far` by its
  prefix):

| `reason` | when |
|---|---|
| `too_far:<distance_km>:<allowed_km>` | beyond the reach, e.g. `too_far:450.123:120` (split on `:`; the toast can say "450 km apart, 120 km allowed") |
| `busy` | either demigod is already in a duel (duels last up to 30 s, across beats) |
| `busy:player` | state schema 12: either player already has a demigod in a duel (one duel per player: see "One duel per player") |
| `cooldown` | that demigod challenged under `duel_cooldown_beats` ago |
| `fallen` | either demigod has fallen |
| `not_yours` | `demigod` isn't one of yours |
| `bad_target` | the target is yours, or not a demigod |
| `challenge_disabled` | challenge mode is off in this room (or a schema-6 match) |

**Right after (or during) a teleport:** a challenge after a summon of the same demigod is checked from the
summon's **destination**, and that demigod fights at its health minus the summon's `health_lost` (decided at the
summon's accept: see "Summoning" below). Summon next to a rival, then double-click it. Archon-style risk.

### `challenge.lock` (everyone: flash, flash, flash, swoosh)

Accepted, every client gets `phase.event` `challenge.lock` (both portraits lock in; the media wrapper adds
`portrait_image` / `diorama_image` next to the prompts). A real frame (prompts shortened):

```json
{ "event": "challenge.lock", "duel_id": "duel:s41:0:1", "beat": 2, "level": "regions",
  "a": { "demigod": "dg:0", "name": "Ash's Left Hand", "player": "Ash", "seat": 0, "controller": "human",
         "health": 10.0, "max_health": 10.0, "skin": "obsidian", "portrait_prompt": "Ash's Left Hand, a demigod: …", "diorama_prompt": "…" },
  "b": { "demigod": "dg:1", "name": "Brix's Left Hand", "player": "Brix", "seat": 1, "controller": "ai",
         "health": 10.0, "max_health": 10.0, "skin": "gold", "portrait_prompt": "…", "diorama_prompt": "…" },
  "at": [106.94723503967316, -6.058344802246152], "cell": "5:regions:11:-4",
  "focus": { "at": [106.94723503967316, -6.058344802246152], "lat": -6.058344802246152, "lon": 106.94723503967316,
             "cell": "5:regions:11:-4", "level": "regions" },
  "ends": "ko_or_deadline", "ends_beat": 3, "deadline_ms": 1794686655327, "world": "beating",
  "freeze": { "requested": true, "supported": false, "participants": [0, 1], "deadline_ms": 1794686655327 },
  "place": "Jakarta", "time_of_day": "day", "distance_km": 1.0, "by_seat": 0, "ai": false, "leap": null,
  "seq": 41, "received_at_ms": 1794686649327 }
```

- `a` is the challenger, `b` the target. `focus` (also `at`) is the target's ground: every client centres the
  camera and spins the globe there (`lat` / `lon` spelled out). Both `health` values are the duel's health bars.
- `a.skin`, `b.skin`: each fighter's mannequin skin, one of `wood`, `obsidian`, `gold`, `dragonscale`, `butterfly`
  (see "Skins" below). Optional: a server without it sends none, and you compute the name's skin yourself.
- `duel_id` is `duel:s<seq>:<a>:<b>` for a player's challenge (`seq` = the challenge action's room-wide seq, so the id
  never changes across beats) and `duel:<beat>:<a>:<b>` for the built-in AI's. Quote it in every line.
- **The end:** a knockout, or the deadline (`duel_deadline_ms`, 30 s after the lock: `deadline_ms`, epoch ms).
- **The world keeps beating (fallback).** By design a duel freezes the world (a room-wide engine hold: no beats
  resolve, only the duellists act) with a fast typing relay. The engine has neither yet, so `freeze.supported` is
  `false` and `world` is `"beating"`: beats keep resolving during the duel, the duel is kept in the state across
  resolves (the two duellists are locked in place and take the damage so far at each resolve), and it ends at the
  first resolve after `deadline_ms`, or at beat `ends_beat` (lock beat + ceil(30 s / `flow_ms`) − 1), whichever is
  first. When the hold lands, `world` reads `"held"` and the core's hold frames bracket the duel; the payloads stay
  the same.
- **The built-in AI challenges too:** its locks arrive at the beat's start in `round.started.public.challenges[]`
  (same shape, `by_seat: null`, `ai: true`, `deadline_ms: null`). Fight back with lines (a human challenged by the AI
  gets the duel's whole span; two AIs fight it out at once). When its rival was beyond 120 km, the AI summons itself
  next to it first: `leap: {demigod, to, health_lost}` (play the teleport and the red pulse; `a.health` is already
  reduced).

#### Skins

`skin` is on each fighter of `challenge.lock` (`a`, `b`), so also in `round.started.public.challenges[]` and in
`present.duels[]` (`a`, `b`, `lock.a`, `lock.b`). No rules version and no state schema: it is computed when the
payload is built and stored nowhere. The server picks, in this order:

1. a skin the player named for the demigod: **reserved** (no persona field or account setting carries one yet; when
   one does, it arrives in this same field);
2. gear the demigod holds (objects, not armies, fleets or beasts): a thing **a god granted** → `dragonscale`; else a
   thing **of gold** (the word gold, golden, gilded or gilt in its name or description) → `gold`;
3. the name: `SKINS[fnv1a32(lower(name)) mod 5]` with `SKINS = [wood, obsidian, gold, dragonscale, butterfly]`.

Rule 3 is the one a client can compute, and what it shows when `skin` is absent:

- `name` is the fighter's `name` exactly as the payload has it (no trimming);
- `lower`: **only ASCII `A`-`Z` become `a`-`z`**; every other character is left alone (no Unicode case folding, so
  every language's string library gives the same bytes);
- FNV-1a, 32 bits, over the **UTF-8 bytes**: `h = 2166136261`; for each byte `h = ((h XOR byte) * 16777619) mod 2^32`.

| `name` | hash | mod 5 | `skin` |
|---|---|---|---|
| `Ash's Left Hand` | `0xb1aeb883` | 1 | `obsidian` |
| `Brix's Right Hand` | `0x5b655229` | 0 | `wood` |
| `Dune's Right Hand` | `0xb090f624` | 2 | `gold` |
| `KALI` (= `kali`) | `0x2eced3aa` | 3 | `dragonscale` |
| `Nyx` | `0x197b6960` | 4 | `butterfly` |
| `Émile the Unready` (the `É` stays `C3 89`) | `0x3eadb7d1` | 2 | `gold` |

(FNV-1a itself: `""` → `0x811c9dc5`, `"a"` → `0xe40c292c`, `"foobar"` → `0xbf9cf968`.) When `skin` is present, use it:
rules 1 and 2 need the server. Gear is read when the payload is built, so the `skin` in a later `present` can differ
from the lock's if the demigod's gear changed at a resolve in between: keep the lock's for the duel.

### Lines: `fields.duel` → `duel.blow`

On Enter, each line is an instant action (`UFNNetClient::DuelLine`):

```json
{ "t": "action.submit", "d": { "instant": true, "fields": {
    "duel": { "duel_id": "duel:s41:0:1", "text": "I sweep your ankle into the harbour mud" } } } }
```

At most 120 characters. Refusals: `no_duel`, `duel_over` (knocked out, past the deadline, or already ended),
`not_yours` (you're not in it), `fallen`, `empty`, `too_long`. Each accepted line is resolved **at once** and pushed to
everyone as `phase.event` `duel.blow`. A real frame (a miss, then the AI's counter landing):

```json
{ "event": "duel.blow", "duel_id": "duel:s41:0:1", "n": 1, "by": "dg:0", "by_name": "Ash's Left Hand",
  "text": "I sweep your ankle into the harbour mud", "roll": 2, "modifier": 7, "total": 9, "target_dc": 10,
  "verdict": "fail", "damage": 0.0, "hp": { "a": 10.0, "b": 10.0 },
  "flavor": "Brix's Left Hand ducked, not by skill, but because a goat that licked the moonlight off a wall had owed them a favour since beat one.",
  "wit": 3, "tags": ["verb", "body", "terrain"],
  "modifiers": { "wit": 3, "health": 1, "gear": 0, "follow_up": 0, "home": 1, "situation": 2 }, "ko": false, "auto": false,
  "anim": { "a": "slash", "b": "parry", "fx": "earth", "camera": "over_a", "army": false, "attacker": "a" },
  "choreo": { "range_before": "mid", "range_after": "mid", "initiative_before": "a", "initiative_after": "a",
              "flip": false, "result": "missed", "camera": "over_a", "duration_ms": 1200, "contacts": [],
              "a": { "move_class": "strike", "contact": null, "stance_after": "neutral",
                     "pose_keys": [{ "pose": "step_in", "t_ms": 0 }, { "pose": "slash_high", "t_ms": 300 },
                                   { "pose": "idle_guard", "t_ms": 600 }] },
              "b": { "move_class": null, "contact": null, "stance_after": "neutral",
                     "pose_keys": [{ "pose": "idle_guard", "t_ms": 750 }] } },
  "counter": { "by": "dg:1", "by_name": "Brix's Left Hand", "text": "sweep! Right in the ribs, here in the tower",
               "roll": 7, "modifier": 3, "total": 10, "target_dc": 10, "verdict": "success", "damage": 1.0,
               "hp": { "a": 9.0, "b": 10.0 }, "wit": 2, "tags": ["verb", "body", "terrain"], "auto": true, "ko": false,
               "flavor": "Brix's Left Hand's words hit Ash's Left Hand's ribs only because the third crow of the morning had been washed in platinum dust at dawn.",
               "modifiers": { "wit": 2, "health": 1, "gear": 0, "follow_up": 0, "home": 0, "situation": 0 },
               "anim": { "a": "stagger", "b": "slash", "fx": null, "camera": "over_b", "army": false, "attacker": "b" },
               "choreo": { "range_before": "mid", "range_after": "close", "initiative_before": "a", "initiative_after": "b",
                           "flip": true, "result": "landed", "camera": "over_b", "duration_ms": 1200,
                           "contacts": [{ "by": "b", "limb": "right_foot", "on": "a", "target_limb": "torso", "t_ms": 600 }],
                           "a": { "move_class": null, "contact": null, "stance_after": "neutral",
                                  "pose_keys": [{ "pose": "stagger_back", "t_ms": 750 }] },
                           "b": { "move_class": "strike", "stance_after": "neutral",
                                  "contact": { "limb": "right_foot", "target_limb": "torso", "t_ms": 600 },
                                  "pose_keys": [{ "pose": "step_in", "t_ms": 0 }, { "pose": "lunge_extend", "t_ms": 300 },
                                                { "pose": "idle_guard", "t_ms": 600 }] } } },
  "hp_after": { "a": 9.0, "b": 10.0 }, "at_ms": 1794686650327, "seq": 42, "received_at_ms": 1794686650327 }
```

- **The die:** a d20 seeded by the match and `<duel_id>:<n>` (a counter: `<duel_id>:<n>:counter`). `verdict` is
  `crit_fail` / `fail` / `success` / `crit_success`, or `penalty` (a line sooner than `duel_min_gap_ms`, 0.5 s, after
  the same side's last: `roll` 0, no damage, `self_damage` 0.5 to its author).
- **modifier** = wit + health (+1 at 7+, −1 at 3 or less) + gear + follow-up (+1 within 1.5 s) + home turf +
  situation (the formal model: initiative +2, your stance, the defender's openings). DC 10 (+1 at night, + half the
  defender's wardrobe stage).
- **wit** −3..+4 from the words (action verbs, body parts, items held, terrain and place words, the opponent's story
  and scars; minus repeats, echoes, short lines, gibberish). `tags` say what it saw.
- **damage** 0–3 per blow, 4 on a natural 20, shaped by the matrix (below), × `duel_damage_scale`. `hp` is both duel
  healths after the blow, `hp_after` after its counter. `ko: true` ends the duel: later lines are refused
  `duel_over`.
- **counter:** present when the opponent is an AI, or a human who hasn't blown for `duel_counter_ms` (3 s): its
  automatic strike (`DuelStrike`, `auto: true`), with its own `choreo`.
- **anim** is a one-word summary per figure (`lunge, slash, parry, dodge, leap, cast, summon_army, charge, grapple,
  taunt, stagger, fall, block, blast, vanish, rise`; `fx` `fire, ice, lightning, earth, water, shadow, light, poison`
  or null; `camera` `wide` / `close` / `over_a` / `over_b`). **choreo** is the exact choreography: drive the
  mannequins from it.

#### The formal model and `choreo`

The duel's state between blows: **range** (`far`, `mid`, `close`, `grapple`; a duel opens at `mid`, `close` at the
people level), **initiative** (`a` or `b`: who holds the momentum; the challenger opens with it), and each fighter's
**stance** (`neutral`, `guarding`, `committed`, `off_balance`, `airborne`, `downed`) plus the defence it has set.

Each line is classed by rules into a **move class**: `strike`, `heavy` (smash, crush, overhead, full force…), `guard`
(block, shield…), `parry` (parry, deflect, counter…), `feint` (feint, fake, trick…), `evade` (dodge, duck, leap…),
`close` (charge, rush, step in…), `retreat` (step back, withdraw…), `grapple` (grab, choke, clinch…), `throw` (throw,
hurl, suplex…), `invoke` (a god's name: costs a bolt, any range), `command` (army, troops, followers: needs an army
within 500 km or 100+ followers), `taunt` (mock, insult…). What can't apply becomes what can: a strike or heavy from
`far` is a `close`; a grapple short of `close` is a `close`; a throw outside a grapple is a strike; an invoke without
a bolt or a command without an army is a taunt.

The d20 decides success; the **interaction matrix** decides what a success means, against the defender's set:

| attack | defender's set | success | failure |
|---|---|---|---|
| strike | parry | parried (unless it beats the DC by 5: half damage) | parried |
| heavy | parry or guard | breaks it: full damage +1, defender off balance | parried (vs parry) |
| strike | guard | absorbed: −1 damage | nothing |
| feint | guard or parry | the set is wasted, defender off balance, **initiative flips** | nothing |
| strike | evade | evaded unless it beats the DC by 3 | evaded |
| heavy | evade | evaded (unless a natural 20); **initiative flips** | evaded |
| grapple | — (close range only) | range `grapple`, defender off balance | nothing |
| throw | — (from a grapple) | +1 damage, defender `downed`, range `mid` | nothing |
| close / retreat | — | range one step closer / farther, initiative | nothing |
| guard / parry / evade | — | the defence is set (stance `guarding`, or `airborne` for a leap) | nothing |
| taunt | — | initiative to the taunter, defender off balance | nothing |

**Parried:** the attacker goes `off_balance` and the initiative **flips** to the parrier. **A heavy** wins if it
lands (+1 damage) but always leaves its user `committed`. A landed strike or heavy at `mid` closes to `close`.
Whoever lands (or sets a defence) takes the initiative.

`choreo` (every strike, counters and penalties included):

| field | meaning |
|---|---|
| `range_before` / `range_after` | `far` / `mid` / `close` / `grapple` |
| `initiative_before` / `initiative_after` | `a` / `b`; `flip` is true when it changed hands |
| `result` | `landed, absorbed, parried, evaded, missed, thrown, grappled, wasted, moved, set, taunted, penalty` |
| `a`, `b` | the challenger's and the target's figures: `{move_class, pose_keys: [{pose, t_ms}], contact, stance_after}` (the defender's `move_class` is the defence it had set, or null) |
| `contact` | on the striker: `{limb: right_hand / left_hand / right_foot, target_limb: head / torso / arms / legs, t_ms}` when it connects, else null |
| `contacts` | every contact of the exchange: `[{by: a / b, limb, on: a / b, target_limb, t_ms}]` (empty when nothing lands) |
| `camera` | `wide` / `close` / `over_a` / `over_b` |
| `duration_ms` | 600–1400; every `t_ms` is an offset within it (the defender reacts half a step after the strike pose) |

**The pose vocabulary** (fixed, 30 names): `idle_guard, step_in, lunge_extend, slash_high, slash_low,
overhead_windup, overhead_strike, parry_high, parry_low, block_cross, dodge_left, dodge_right, duck, leap_up, land,
feint_shoulder, grab_reach, grapple_lock, throw_hip, stagger_back, stumble, fall_back, kneel_hit, rise, cast_raise,
cast_release, command_point, taunt_beckon, victory, defeat`.

### Each resolve during a duel: `challenge.open`

While a duel is under way, each resolve recomputes it from the state and the ordered actions (the live pushes are a
display cache; the resolve is the record and agrees with them blow for blow), applies the damage so far to both
demigods' health, records every die in `dice[]` (reason `"a duel"`), and lists it in
`round.outcome.public.duels_open[]`:

```json
{ "event": "challenge.open", "duel_id": "duel:s41:0:1", "a": "dg:0", "b": "dg:1", "hp": { "a": 9.0, "b": 10.0 },
  "blows": 1, "typed": 1, "ends_beat": 3, "deadline_ms": 1794686655327, "range": "close", "initiative": "b",
  "log": [ "…the duel.blow events this resolve was handed…" ] }
```

### The end: `challenge.end`

At the first resolve after a knockout, the deadline or `ends_beat` (or when a duellist fell elsewhere) the duel ends:
a knockout is an elimination, the loser's loot falls on the duel's hex (everything at a knockout; else a DC 11
keep-hold roll when beaten by 1+), both get a tale, the hex remembers `duel.lock` / `duel.end`.
`round.outcome.public.duels[]`:

```json
{ "event": "challenge.end", "duel_id": "duel:s41:0:1", "reason": "deadline", "locked_beat": 2,
  "a": "dg:0", "b": "dg:1", "a_name": "Ash's Left Hand", "b_name": "Brix's Left Hand",
  "winner": "dg:1", "loser": "dg:0", "ko": false, "damage": { "a": 1.0, "b": 0.0 }, "hp": { "a": 9.0, "b": 10.0 },
  "blows": 1, "typed": 1,
  "deed": "After one blow in Jakarta, Brix's Left Hand edged Ash's Left Hand on points, only because three crows agreed, for once.",
  "at": [106.95, -6.06], "pos": [106.947235, -6.058345], "cell": "5:regions:11:-4", "level": "regions",
  "place": "Jakarta", "log": [ "…the blows this final resolve was handed…" ] }
```

`reason` is `ko`, `deadline`, `fallen` (a duellist fell elsewhere) or `auto` (two built-in AIs, fought out at once).
`winner` / `loser` are null on a draw (damage within 0.05). `blows` counts every numbered blow (typed and automatic);
a duel nobody typed in is fought automatically at its end (`duel_auto_rounds` exchanges, `auto: true`). The deed is
one line from the duel mind when a human fought (LLM), else the rules'.

### Healing and orders that never vanish (0.8.1 fix)

- "heal all my demigods" (heal, restore, mend, cure…) goes to **Nature** (Death when a target has fallen), even when
  the hand names no god: an invocation with its normal price and a real verdict in your `omens[]`. Unless refused,
  each target heals 1–3 (1 + magnitude/4, half when twisted), visible as `heal` rows in `events[]` and in health.
- "me" / "my demigods" / "all my demigods" mean **your own two** demigods (rivals are healed only when named).
- An order the hand routes to a god that isn't in the match goes to Entropy (a visible ruling).
- **A fallen player's will** (all your demigods down) is never dropped. A plea to come back ("my guys come back to
  life!", revive, raise, resurrect…) is an invocation of **Death** for your fallen demigods, ruled like any other
  (Death's price; with an empty purse Death twists or refuses); any other order is refused. Either way your
  `round.private` carries `wills[]`:

```json
"wills": [{ "text": "my guys come back to life!", "routed_to": "Death", "verdict": "refuse",
            "speech": "\"No.\"", "reason": "a fallen player's will goes to Death; Death's price is 4 bolts and Ash's Left Hand has 0" }]
```

  (`verdict` is the god's `grant` / `bargain` / `twist` / `refuse`, or `refused` when it never reached a god;
  Death's ruling is also in `omens[]`.)

### Summoning: the damage check comes first (0.8.1)

The `summoning` push carries the mishap decided **at the accept**, before the teleport (the resolve applies exactly
these, and `summoned` repeats the same `roll` and `health_lost`). A real frame:

```json
{ "event": "summoning", "by": "Ash", "by_ref": "dg:0", "by_seat": 0, "demigod": "dg:0",
  "from": [106.9381913263311, -6.058344877154526], "to": [106.94269567294259, -3.360383775620408],
  "roll": 20, "modifier": 0, "total": 20, "target_dc": 18, "verdict": "crit_success", "health_lost": 0.0,
  "summons": [{ "demigod": "dg:0", "to": [106.94269567294259, -3.360383775620408], "cell": "5:people:416967:-86902",
                "roll": 20, "modifier": 0, "total": 20, "target_dc": 18, "verdict": "crit_success", "health_lost": 0.0 }],
  "god_powers": [], "seq": 40, "received_at_ms": 1794686638068 }
```

Show the gentle red pulse before the teleport. The roll's query is `summon:<beat>:<demigod>:<lon>,<lat>`. A
challenge may follow immediately (during or after the teleport) with the loss already counted.

### Settings

`challenge_enabled` (true), `challenge_min_km` (120), `challenge_contact_km_regions` (600), `_villages` (15),
`_houses` (0.15), `_people` (0.015), `duel_cooldown_beats` (1),
`duel_deadline_ms` (30000), `duel_damage_scale` (1), `duel_min_gap_ms` (500), `duel_followup_ms` (1500),
`duel_counter_ms` (3000), `duel_ai_chance` (0.02), `duel_ai_reach_km` (2000), `duel_auto_rounds` (2); state schema 12:
`duel_one_per_player` (true), `duel_ai_lines_per_window` (1), `duel_ai_aggression` (0.6), `duel_ai_handicap` (5). Ranges in
`docs/schemas/settings.schema.json` and the rulebook's appendix.

## Duel panels (0.8.2)

Rules **0.8.2** (state **schema 9**; a schema-8 or older match has none of this and replays unchanged) make a duel
play as comic **panels**, the format of `duel_lab/` (README.md, duel.json). Schemas: `DuelScene`, `DuelPanel` in
`phase_event`, `outcome_public`, `public_view`; `duel_tick` in `action_fields`.

**Nothing in 0.8.1 changes.** A typed line (`fields.duel`) is still a blow resolved at once and shown as `duel.blow`;
the d20, the wit and the matrix decide everything. A panel is the *presentation* of the blows of one input window:
it decides nothing and can always be rebuilt from them. A 0.8.1 client that ignores `also[]`, `duel.panel` and
`duel.scene` works as before.

### The cycle

| Panel | Starts | Shows |
|---|---|---|
| 0 (`kind: intro`) | at the lock's stamp (`challenge.lock.received_at_ms`) | both fighters, the stage |
| n ≥ 1 (`exchange`, `idle`) | when panel n−1 ends: `starts_at_ms(n−1) + play_ms(n−1)` | the blows stamped in panel n−1's input window |
| `ko` | same | the window with the knockout; no input |
| `outro` | right after a `ko`; or the first slot at or past the deadline | who stood. The last panel |

- `play_ms`: 2400 for an idle panel; 600 + 1000 per staged strike (at most 3), so 2400..3600. (State schema 12: a
  panel that opens a window an AI types in plays as long as its line needs, up to about 6000: see "AI duellists".)
- `input_closes_ms = starts_at_ms + play_ms − 700`. Always before the panel ends: the 700 ms is the **render lead**,
  the time you have panel n+1 before it starts. `input_opens_ms` is the previous close.
- A line stamped by the server at or before `input_closes_ms` (and after the previous close) is in panel n+1. A later
  one is in panel n+2. Type at any time; the blow itself still shows at once.
- A fighter with no blow in the window plays the rules default (`default`): `circle` when it holds the initiative
  and nobody struck, else `guard`. (0.8.1 still counters for an AI or a human idle for `duel_counter_ms`: those
  automatic strikes are staged too, `auto: true`.)
- A pause (`duel_clock`) moves the schedule: panels not yet computed start that much later.

### Pacing: `fields.duel_tick`

The server has no timer: it computes panels only when something arrives. So **each duellist's client sends a tick
at every input close**:

```json
{ "t": "action.submit", "d": { "instant": true, "fields": { "duel_tick": { "duel_id": "duel:s6:0:2" } } } }
```

- Send it at `input_closes_ms` by the **server's** clock (your offset from `sys.pong.server_time` or
  `present.server_time_ms`), plus ~30 ms. A tick up to 150 ms early still closes the window (at the tick); earlier
  than that it does nothing.
- The first tick that closes a window gets the next panel as its public event (`phase.event.public` =
  `duel.panel`, to everyone, with `seq` and `received_at_ms`); the other duellist's tick for the same close shows
  nothing. Either way the reply is `action.received {accepted: true}`.
- Refusals: `panels_off` (the match is older than 0.8.2), `no_duel` (no such duel, or its outro is out),
  `not_yours` (only its two duellists tick; viewers never do).
- If nobody ticks (both clients gone), nothing is lost: the next duel line and every beat's resolve close every
  window their stamps have passed, and the panels come in `also[]` / `challenge.open.panels`. What a panel shows
  never depends on when it was computed.
- It is an instant action (rate limit 2/s, burst 4 per seat): one tick per panel is well inside it (but it shares that
  bucket with your lines and your live typing: see "Live typing" for the cadence that leaves room for it).

### Where panels and the scene arrive

| Where | What |
|---|---|
| `challenge.lock.also[]` (the live event, and each AI lock in `round.started.public.challenges[]`) | `duel.scene`, then `duel.panel` 0 |
| a `duel_tick`'s `phase.event` | the panel it made due; more in its `also[]` if several were due |
| `duel.blow.also[]` (live) | the panels the line's stamp made due (after a silence) |
| `round.outcome.public.duels_open[].panels` | the panels that beat's resolve made due; `panels_total` |
| `round.outcome.public.duels[].panels` (`challenge.end`) | the rest, through the outro, when the resolve ends the duel (`starts_at_ms` may lie ahead: play them out) |
| `match.snapshot.public_state.present.duels[]` / `game.present` | `scene` and **every panel so far** (`panels`), with `server_time_ms`: a reconnect or a viewer joining mid-duel seeks to `server_time_ms − panels[k].starts_at_ms` in the panel that contains it |

`also[]`: one `on_instant` can return one public event, so the extra ones ride in the first one's `also` array. Handle
every entry exactly as if it had arrived as its own `phase.event`. Key panels by `(duel_id, n)`: the same panel can
reach you twice (live and in an outcome) and is identical both times.

An AI's challenge with no clock (a simulation; never on the server) has no scene or panels: its duel is fought out
at one resolve as in 0.8.1.

### `duel.scene`

```json
{ "event": "duel.scene", "duel_id": "duel:s1:0:1", "source": "rules", "biome": "town", "place": "New York",
  "time_of_day": "day", "cell": "4:regions:-7:32", "level": "regions", "settlement_level": 3,
  "backstory": "New York at day. Ash's Left Hand and Brix's Left Hand have chosen this hex.",
  "hex": { "x": "1.732 (q + r/2)", "z": "1.5 r", "unit": "m" }, "radius": 5, "a_start": [-3, 0], "b_start": [2, 0],
  "objects": [
    { "id": "pillar_a", "kind": "pillar", "at": [2, -2], "yaw": 135, "fixed": true,
      "note": "an old stone pillar, the mortar long gone", "affords": ["wallrun", "wallkick"],
      "parts": [ { "shape": "box", "size": [0.9, 0.35, 0.9], "offset": [0.0, 0.175, 0.0], "material": "stone" },
                 { "shape": "cylinder", "size": [0.3, 1.5], "offset": [0.0, 1.1, 0.0], "material": "stone" } ] },
    { "id": "crate_a", "kind": "crate", "at": [-3, -2], "yaw": 90, "fixed": false,
      "note": "a crate somebody meant to come back for", "affords": ["vault"],
      "parts": [ { "shape": "box", "size": [0.85, 0.75, 0.85], "offset": [0.0, 0.375, 0.0], "material": "wood", "fill": 0.12 } ] } ] }
```

- The duel lab's scene shape: `at` is a stage hex `[q, r]`; `parts[]` are primitives (`box [w,h,d]`, `cylinder` /
  `cone [r,h]`, `sphere` / `tetra` / `octa` / `ico` / `dodeca [r]`, metres; `offset` = the part's centre above the
  object's origin on the floor; optional `rot` degrees, `fill`, `glow`); materials from the lab's table (stone,
  marble, wood, iron, gold, clay, bone, leaf).
- `source: "rules"`: the deterministic fallback, seeded by the match seed and the duel's hex (the same duel always
  gets the same scene). 6 to 10 objects of these kinds: `pillar`, `wall`, `crate`, `table`, `boulder`, `rock`,
  `tree`, `log`, `jar`, `brazier` (night), `altar` (a shrine on the hex), `chest` (loot dropped on the hex), `bones`,
  `cairn` (one for each of the two newest things the DM remembers of the hex; its `note` is that memory). The kinds
  come from the place's words (harbour, forest, desert, hills), else a town (settlement level 2+) or the wild.
- `affords`: the paths that may name the object (`vault`, `slide`, `wallrun`, `wallkick`, `mantle`).
- An LLM-authored scene (`source: "llm"`) will use the same shape; not built yet.

### `duel.panel`

```json
{ "event": "duel.panel", "duel_id": "duel:s1:0:1", "n": 1, "kind": "exchange",
  "starts_at_ms": 1794692897466, "play_ms": 3600, "input_opens_ms": 1794692896766, "input_closes_ms": 1794692900366,
  "caption": "Ash's Left Hand's words hit Brix's Left Hand's knee only because a goat that licked the moonlight off a wall had been washed in platinum dust at dawn.",
  "shot": "over_a",
  "a": { "line": "I kick your knee into the mud", "line_masked_len": 29, "lines": 1, "move_class": "strike",
         "default": null, "auto": false, "at": [0, 0],
         "segs": [ { "clip": "run", "ms": 360, "loop": true, "to": [0, 0], "path": "dash", "face": "foe" },
                   { "clip": "kick", "ms": 840, "face": "foe" },
                   { "clip": "idle_guard", "ms": 540, "loop": true, "face": "foe" },
                   { "clip": "stagger_waist", "ms": 660 },
                   { "clip": "idle_guard", "ms": 1200, "loop": true, "face": "foe" } ] },
  "b": { "line": "I duck under it and roll away", "line_masked_len": 29, "lines": 1, "move_class": "evade",
         "default": null, "auto": true, "at": [2, 0],
         "segs": [ { "clip": "idle_guard", "ms": 738, "loop": true, "face": "foe" }, { "clip": "stagger_waist", "ms": 462 },
                   { "clip": "strike_side", "ms": 1200, "face": "foe" }, { "clip": "evade_stand", "ms": 1200, "face": "foe" } ] },
  "contacts": [ { "t_ms": 660, "by": "a", "limb": "foot_r", "kind": "kick", "on": "b", "at": "legs", "impulse": 0.375,
                  "text": "BAM", "miss": false, "ranged": false },
                { "t_ms": 1860, "by": "b", "limb": "hand_r", "kind": "hand", "on": "a", "at": "head", "impulse": 0.25,
                  "text": "SHHK", "miss": false, "ranged": false } ],
  "blows": [1, 2], "flip": true, "hp": { "a": 9.0, "b": 8.5 }, "range": "close", "initiative": "b" }
```

| Field | Meaning |
|---|---|
| `n`, `kind` | 0-based; `intro`, `exchange`, `idle`, `ko`, `outro` |
| `starts_at_ms`, `play_ms` | server epoch ms; the panel plays from `starts_at_ms` for `play_ms` |
| `input_opens_ms`, `input_closes_ms` | the typing window whose blows make panel n+1; both null when the panel takes no input (`ko`, the last slot before the deadline, `outro`) |
| `caption` | the decisive blow's `flavor` line (rules-templated); a fixed line for idle, intro and outro |
| `shot` | `wide`, `side`, `low`, `over_a`, `over_b`, `close_a`, `close_b` |
| `a.typing_script`, `b.typing_script` | state schema 12, a duel whose AI types: the `duel.typing_script` that side types in the window this panel opens (null: none) |
| `a`, `b` | each fighter's track: `line` (the last line it typed in the window; null: none) and `line_masked_len` (its length: show dots while you don't want the words), `lines` (how many it typed), `move_class` (of its last move), `default` (`guard` / `circle` / null), `auto` (an automatic counter is staged), `at` (the hex it ends on), `segs[]` |
| `segs[]` | in order; their `ms` sum to `play_ms`. `clip`: one of the lab's 35 clips (`airborne_hit block block_cross bow combo_triple command counter defeat downed evade_roll evade_spin evade_stand grapple_throw_down ground_slam heavy_charged heavy_hammer idle_guard invoke jump_attack kick kneel lunge parry_back parry_high rise run shield_push stagger_back stagger_waist strike_side strike_slash taunt throw_shoulder victory walk`). `loop`, `speed`, `face` (`foe`). A moving seg has `to` (the hex) and `path` |
| `path` | the vocabulary is `dash leap flip knock vault wallrun wallkick mantle drop slide`. The rules fallback emits `dash` (run-ins, steps, side-steps), `leap` (`h`: a heavy from a range band away), `knock` (`h`, `spin`: a landed heavy blow, a throw, a knockout), `vault` (`over: <object id>`), `slide` (`under: <object id>`) and `wallkick` (`on: <object id>`, `h`) for evasions next to an object that affords them. `flip`, `wallrun`, `mantle`, `drop` are reserved for authored panels |
| `contacts[]` | `t_ms` from the panel's start; `by` (`a`/`b`); `limb` (`hand_l`, `hand_r`, `foot_l`, `foot_r`); `kind` (`hand`, `kick`, `heavy`); `on`: `a` / `b` (the fighter hit; `at`: `head`/`chest`/`belly`/`legs`) or a scene object's id (a blow that missed and found the scenery); `impulse` 0..1 (damage / 4); `text` (the burst word); `miss` (no hit: no shake); `ranged` (an invoke or a command: no IK) |
| `blows[]` | the `n` of each `duel.blow` it stages (its full event is in `present.duels[].blows` and the outcome logs). More than 3 strikes in one window: all count, the first 3 are staged |
| `flip` | the initiative changed hands in this panel |
| `hp`, `range`, `initiative` | after the panel: exactly the last staged blow's |
| `winner`, `ko` | outro only: `a` / `b` / null |

### Live typing: `duel.typing`

Both duellists' lines show as they are typed, masked like a password field: stars, with the newest letters readable
for a moment (`*****p`). Watch your opponent's line character by character and you know what is coming; otherwise you
are winging it. Schemas: `duel_keys` in `action_fields`, `DuelTyping` in `phase_event`, `typing` in `PresentDuel`. No
rules version and no state schema: a server from before it accepts the action and does nothing with it (no
`duel.typing`), and a client that ignores `duel.typing` loses nothing.

**Send** (a duellist's client, while its player types; an instant action that travels alone):

```json
{ "t": "action.submit", "d": { "instant": true, "fields": {
    "duel_keys": { "duel_id": "duel:s6:0:2", "len": 9, "tail": " I s" } } } }
```

| Field | Rule |
|---|---|
| `duel_id` | the duel you are in |
| `len` | integer 0..120: the characters in your line **now** (it goes down when the player deletes; 0 = the line is empty) |
| `tail` | string, 0 to 8 characters: the **newest** characters typed since your last `duel_keys` (the last 8 if more were typed; `""` after a deletion). Never longer than `len`. No control characters (tab, newline, ...) and no invisible format characters (zero-width, bidi overrides) |

The reply is `action.received {accepted: true}`, or `accepted: false` with `reason`: `bad_len`, `bad_tail`,
`no_duel` (no `duel_id`, no such duel under way, or its outro is out), `not_yours` (only its two duellists type;
viewers never do), `panels_off` (the match is older than 0.8.2), `challenge_disabled`: a `duel_tick`'s refusals plus
the two shape ones.

**Receive** (everyone in the room, viewers too, at once, as `phase.event.public`):

```json
{ "event": "duel.typing", "duel_id": "duel:s6:0:2", "by": "a", "demigod": "dg:0", "len": 9, "tail": " I s",
  "at_ms": 1794700012345, "seq": 57 }
```

- `by`: the side typing (`a` = the challenger); `demigod`: its ref. `at_ms`: the server's receive stamp; `seq`: the
  action's room-wide arrival number. Order by `(at_ms, seq)` and drop a frame older than the one you already drew.
- **Draw** side `by`'s line as `len` characters: stars, with the last `tail.length` of them replaced by `tail`.
  Letters from an earlier frame turn into stars. When more than 8 characters were typed between two frames, the ones
  in between were never sent: they are stars from the start.
- **Your own line**: draw it from your own keyboard at once, with the same mask; you get your own `duel.typing` back
  like everyone else (use it or ignore it).
- **The line is sent** (`fields.duel`): on the `duel.blow` whose `by` is that fighter's demigod, clear its typing line
  (the server zeroes its length; it sends no extra `duel.typing`). An automatic counter (`auto: true`) clears nothing.
  A player who erases the line: send `len: 0, tail: ""`.
- **It decides nothing.** Only the `fields.duel` sent on Enter is a blow. Typing never moves the panel cycle (it is
  not a tick), is not in the state, is not in a recording's outcome (a recorded match replays the same with or
  without it), and is **never replayed**: `present.events[]` has no `duel.typing`. A client that (re)connects
  mid-duel reads `present.duels[].typing = {a: {len, at_ms}, b: {len, at_ms}}` (each side's current length: draw that
  many stars; `at_ms` null and `len` 0 when nothing was typed since the server process started) and gets letters
  from the next frame on. If a side's `at_ms` is older than about 10 s and no frame follows, its client is gone: fade
  the stars.

#### How often: the cadence that fits the rate limit

Instant actions are limited **per seat** by the core: a token bucket of 2 per second, burst 4. A duellist's
`duel_keys`, `duel_tick` (one per panel: every 2.4 to 3.6 s) and lines (`fields.duel`) all draw on the same bucket,
and a limited message is answered with `error {code: "rate_limited"}`: it is **dropped**, whichever kind it is. So
typing sent too often costs you ticks and lines.

Measured on a private server (two human duellists and a viewer on localhost, both ticking every panel and sending a
line every 3 s, typing at 7 characters per second, 30 s per run; per duellist):

| `duel_keys` cadence | instants sent /s | `duel_keys` limited | ticks limited | lines limited |
|---|---|---|---|---|
| every 450 ms | 2.8 | 15 to 17 of 65 | 3 to 5 of 10 | 1 to 4 of 9 |
| every 600 ms | 2.3 | 3 to 9 of 50 | 0 to 2 of 11 | 0 to 3 of 9 |
| every 700 ms | 2.1 | 2 of 43 | 0 | 0 |
| every 800 ms | 1.9 | 0 | 0 | 0 |
| every 900 ms (also with a line every 2 s) | 1.8 to 1.9 | 0 | 0 | 0 |
| every 1000 ms (also with a line every 2 s) | 1.7 to 1.8 | 0 | 0 | 0 |
| client-side bucket, below (also with a line every 2 s or 5 s) | 2.0 | 0 | 0 | 0 |

The relay itself is fast: from the sender's `send` to the viewer's receipt, median 20 ms, 95th percentile 30 to 75 ms.

**So one `duel_keys` every 450 ms does not fit.** Do one of these:

1. **Fixed: at most one `duel_keys` every 900 ms**, and only when the line changed since the last one. Simple and
   always inside the limit with a tick per panel and a line every 2 s. (800 ms was clean too, with no margin.)
2. **Better: mirror the bucket in the client** (the measured policy): keep `tokens` (start 4; add 2 per second, cap
   4; subtract 1 for every instant action you send, of any kind: casts and summons count too). Send a `duel_keys` only
   when the line changed, at least 450 ms have passed since your last one, **and `tokens >= 2`** (one token stays in
   reserve). Send ticks and lines whenever they are due (if `tokens < 1`, which the reserve prevents, wait the few
   milliseconds until it is 1). This gave 1.2 to 1.6 `duel_keys` per second (1.2 with a line every 2 s, 1.6 with one
   every 5 s: bursts at 450 ms after a pause, about one per 750 ms in steady typing) with nothing limited, and ticks and lines never waited.

Either way: **never retry a limited `duel_keys`** (the next one supersedes it), and always send lines and ticks
first. At these rates a fast typist (more than about 10 characters per second) outruns the 8-character `tail`: some
letters only ever show as stars. That is the design's "otherwise, you're winging it"; `len` is always exact.

### AI duellists: typing scripts (state schema 12)

A duel against an AI reads like a duel against a human: **the AI types a line of its own in every input window**,
you watch it arrive as masked letters on the opponent's line (top: the opponent's `*****p`, bottom: your words, Enter
to enter), and at its Enter the line is a blow like yours. Gated on `rules.state_schema >= 12` and the room setting
`duel_ai_lines_per_window` (1; 0 = off, the 0.8.2 duel); only in a duel with a clock (every duel on the server).
Schemas: `DuelTypingScript`, `DuelBlow.ai` / `.window`, `DuelPanel.a|b.typing_script`, `PresentDuel.typing_scripts`.

An AI fighter has no client and the server has no timer, so the server sends no typing frames. When a window opens
it computes the AI's whole line for that window and publishes a **typing script**; every client plays it back
against the server clock, so every duellist and viewer sees the same letters at the same moments.

```json
{ "event": "duel.typing_script", "duel_id": "duel:s1:0:1", "by": "b", "demigod": "dg:1", "window": 1,
  "text": "I headbutt your jaw.", "len": 20, "starts_at_ms": 1794702111472, "cps": 6.6, "key_ms": 152,
  "pauses": [ { "after": 11, "ms": 161 } ], "ends_at_ms": 1794702114901, "input_closes_ms": 1794702115608 }
```

| Field | Meaning |
|---|---|
| `by`, `demigod` | the side typing (`a` = the challenger) and its ref |
| `window` | the panel (`n`) whose input window the line is typed in |
| `text`, `len` | the whole line, 20 to 70 characters (in practice 20 to about 36: what the typist's speed fits). **Public by design**: a fast reader may read the opponent. Mask it yourself |
| `starts_at_ms` | server epoch ms: it starts typing (120 to 300 ms after the window opens) |
| `cps`, `key_ms` | its speed: 6.0 to 9.0 characters a second, seeded per demigod and match; `key_ms = round(1000 / cps)` |
| `pauses[]` | 0 to 2 pauses between words: once `after` characters are out, the next one comes `ms` (80 to 180) later |
| `ends_at_ms` | Enter. Always at least 400 ms before `input_closes_ms` |
| `input_closes_ms` | its window's close (the panel's) |

**Where it arrives** (the same object every time; key it by `(duel_id, by, window)`):

| Where | When |
|---|---|
| `challenge.lock.also[]`, after `duel.scene` and `duel.panel` 0 | the lock: the script of window 0 (also on each AI lock in `round.started.public.challenges[]`) |
| `also[]` of the `duel_tick` / `duel.blow` that closed a window, right after the `duel.panel` that opens the next one | every window close |
| `duel.panel.a.typing_script` / `.b.typing_script` | on the panel that opens the window (null: that side types nothing in it; the key is on both fighters of every panel of a duel whose AI types, and absent otherwise) |
| `present.duels[].typing_scripts[]` | the scripts being typed **now** (a reconnect or a viewer joining mid-line) |
| `challenge.open.panels[]` / `challenge.end.panels[]` | on those panels, when no client ticked and a resolve closed the windows |

**What the client does:**

1. Keep `offset = server clock − local clock` (from `sys.pong.server_time` or `present.server_time_ms`).
2. At server time `t`, the script has `shown(t)` characters out: the largest `k` with
   `starts_at_ms + k * key_ms + (sum of pauses[].ms with after < k) <= t` (0 before the first, `len` after the last).
3. Draw side `by`'s line exactly as you draw `duel.typing`: `shown(t)` characters, stars, with the newest ones
   readable for a moment (`text[shown-1]`, or the last few: your `tail`). Older letters turn into stars.
4. At `ends_at_ms` show its Enter (clear the line, flash it): the line is now a blow. The `duel.blow` itself
   (`ai: true`, `window`, `text` = the script's, `at_ms` = `ends_at_ms`, no `seq`, `auto: false`) reaches you with the
   next thing the server computes: in `also[]` of your own next line if you type one after its Enter (the AI's blow
   has the lower `n`), else in `also[]` of the tick at the window's close, before the panel that stages it. You may
   send one extra `duel_tick` right at `ends_at_ms` to get it at once (it is a normal tick: it costs a token of your
   instant-action bucket; if nothing else is due the reply is just `accepted`).
5. A reconnect mid-line: take `present.duels[].typing_scripts[]` and seek with step 2 at `present.server_time_ms`.
6. `present.duels[].typing.{a|b}` stays `{len: 0, at_ms: null}` for an AI side: its typing is the script.

**What changes in the duel** (all of it only when the AI types):

- **The panel fits the line.** A panel that opens a window an AI types in plays as long as the line needs:
  `play_ms` is 2400..3600 as before or longer, up to about 6000 (typically 3900 to 5200). `input_closes_ms =
  starts_at_ms + play_ms − 700` still holds. The staged strikes keep their timing at the start of the panel; the rest
  is footwork and guard. The intro (panel 0) is longer too. So against an AI a window is about 4 to 5 s: time to
  type a line of your own in each.
- **Its blow** is resolved by the same d20 and matrix as a human's line, from the text (the class is the line's:
  "Words to moves" below). Its wit is capped at 2 (like an automatic line) and `duel_ai_handicap` (5) comes off its
  modifier against a human (`modifiers.handicap: -5`): it strikes in every window, which no human can. The handicap
  is waived against a human who has entered no line for 12 s, and between two AIs.
- **No double strike.** An AI that typed a line for a window does not also counter automatically in it (your blow
  has no `counter`), and the AI's typed line never triggers an automatic counter for you. Type nothing and you only
  get hit.
- **A window too close to the deadline** to hold a line has no script (`typing_script: null`); there the 0.8.1
  automatic counter still answers your lines.
- **Two AIs** (an AI's challenge to another AI) no longer end at one resolve: both type in every window until a
  knockout or the deadline, and each resolve closes the windows its clock has passed (`challenge.open.log` has their
  blows, `.panels` their panels with scripts whose times may already be past: play them at once).
- **Nobody ticks:** nothing is lost. The next duel line, tick or resolve enters every AI line whose `ends_at_ms` its
  stamp has passed and closes every window, exactly as panels are computed today.
- The line is written by rules (its move class from the range, the initiative, the stances, your set defence and
  your last move; a sentence in one of four voices naming a body part, a scene object or your demigod), seeded: the
  same duel always gets the same lines. An LLM-authored line can replace it later behind the same shape.

Settings: `duel_ai_lines_per_window` (1.0: the chance it types in a window), `duel_ai_aggression` (0.6),
`duel_ai_handicap` (5).

### Words to moves and `clip_fallbacks` (state schema 12)

In a schema-12 match a line's **words** pick its move class and the clip its figure plays, typos and one-word
commands included. Every `duel.blow` (and its `counter`) carries:

```json
"move": { "clip": "flying_kick", "word": "jumpkick", "setup": [], "path": "leap" }
```

- `clip`: the clip of this strike (the panel's `segs[]` play it; you need nothing else). `word`: the move word read
  (as the table has it: `attack` for `attck`; null: no move word, the class's own clip). `setup[]`: the clips of
  the earlier move words of the line, played first as short beats (`plead! jump`: `kneel`, then the jump).
  `path: "leap"`: the figure leaves the ground (the seg has `path: "leap"`).
- A word matches exactly, inflected (`kicks`, `running`), as the start of a longer word (`tumblefight`), or, in a
  command of at most four words, within one edit (words of 4 or 5 letters) or two (6 or more), first letter kept:
  `atack`, `attck`, `jimpkick`, `gaurd`, `thorw`. The **last** move word is the move; an invoke or a command wins
  wherever it stands. A line with no move word is read as in 0.8.1.
- The rules still decide: the class is gated by range, bolts and armies (a throw works from close range now and is
  a step in from farther; an invoke with no bolt is a taunt that still shows its casting), and the d20 and the
  matrix decide what happens.

| Words | Class | Clips |
|---|---|---|
| block, guard, shield, brace, defend | guard | `block`, `block_cross`, `shield_push` |
| parry, deflect, counter, riposte | parry | `parry_high`, `parry_back`, `counter` |
| throw, judo, toss, slam, hurl, suplex, flip | throw | `throw_shoulder`, `grapple_throw_down` |
| grab, choke, wrestle, clinch, seize, pin | grapple | `grapple_throw_down` |
| jumpkick, flying kick, dropkick | strike, leap | `flying_kick` |
| kick; roundhouse | strike | `kick`, `roundhouse_kick`, `high_kick` |
| sweep, trip | strike | `leg_sweep` |
| jump, leap, vault, pounce, dive | strike, leap | `jump_attack` |
| punch, attack, strike, fight, hit, bash | strike | `jab`, `punch_combo`, `strike_side` |
| jab; elbow; knee (when not the target) | strike | `jab`; `elbow_strike`; `knee_strike` |
| flail, frenzy, combo, flurry | strike | `combo_triple`, `punch_combo` |
| spin, whirl | strike | `blade_spin`, `axe_spin` |
| slash, slice, cut, whip, claw | strike | `strike_slash`, `blade_spin` |
| stab, pierce, lunge, thrust | strike | `lunge` |
| uppercut | heavy | `uppercut` |
| smash, crush, cleave, overhead, haymaker, headbutt | heavy | `heavy_hammer`, `heavy_charged`, `ground_slam`, `sword_judgment` |
| tumble, roll, dodge, duck, sidestep, evade, weave | evade | `evade_roll`, `dodge_roll_back`, `evade_spin` |
| run, charge, rush, advance, dash | close | `run` + dash, then `lunge` |
| retreat, withdraw, flee, back off, step back | retreat | `dodge_roll_back`, `evade_stand` |
| summon, cast, spell, magic, (the) force, curse, hex, a god's name | invoke | `invoke`, `mage_cast`, `ground_slam` |
| army, legion, troops, soldiers, followers, faithful | command | `command` |
| plead, beg, pray, mercy | taunt (an appeal) | `kneel`, `bow` |
| taunt, mock, laugh, insult, sneer, yawn | taunt | `taunt` |
| feint, fake, trick, bluff | feint | `evade_spin` |

**Variety.** A fighter never plays the same move clip twice in a row when its list has a choice. Idle time is no
longer one held guard: steps to a neighbouring hex at the same distance from the foe (`walk` + `dash`), guard
shuffles (`block`), `evade_stand`, `idle_guard`. A hit reaction goes by damage: `stagger_waist` / `stagger_back`
(under 2), `stagger_back` knocked a hex back (2+), `airborne_hit` + `downed` + `rise` / `spin_rise` (3+). A fighter
left `downed` gets up when the panel has time.

**New clips** (15, for the same rig; the vocabulary is now 50). A client that does not have one plays its fallback:

```json
"clip_fallbacks": { "flying_kick": "kick", "roundhouse_kick": "kick", "high_kick": "kick", "leg_sweep": "kick",
  "elbow_strike": "strike_side", "uppercut": "strike_side", "jab": "strike_side", "punch_combo": "combo_triple",
  "blade_spin": "strike_slash", "axe_spin": "heavy_hammer", "spin_rise": "rise", "mage_cast": "invoke",
  "dodge_roll_back": "evade_roll", "knee_strike": "kick", "sword_judgment": "heavy_charged" }
```

(The table is fixed and lives here and in `fn7_plugin::panel::CLIP_FALLBACKS`; it is not sent on the wire.)

### One duel per player, and the duel ends at its deadline (state schema 12)

- **One duel per player** (`duel_one_per_player`, true): you can only type in one duel. While one of a player's
  demigods is in an open duel, a challenge **by or to any** of that player's demigods is refused with
  `busy:player` (match `busy` by its prefix), whoever challenges; the built-in AI's chooser skips such players, and
  an AI player also fights one duel at a time.
- **The duel ends at `deadline_ms`,** not a beat later. The first duel action stamped past the deadline (a
  `duel_tick`, or a line: it is accepted but is no blow) closes the duel: its public event carries, in `also[]`, the
  panels left through the `outro` and then a **provisional** `challenge.end`:

```json
{ "event": "challenge.end", "provisional": true, "duel_id": "duel:s1:0:1", "reason": "deadline", "winner": "dg:1",
  "loser": "dg:0", "ko": false, "damage": { "a": 2.1, "b": 0.6 }, "hp": { "a": 7.9, "b": 9.4 }, "blows": 7, "typed": 1,
  "deed": "", "log": [], "panels_total": 8, "a": "dg:0", "b": "dg:1", "a_name": "...", "b_name": "...",
  "at": [0, 0], "pos": [0, 0], "cell": "...", "level": "regions", "place": "...", "locked_beat": 2 }
```

  The same comes with a knockout's outro (`reason: "ko"`). **So: lock your input at `deadline_ms` and send one
  `duel_tick` at `deadline_ms + 30 ms`** (server clock); show the winner at once. The world is changed at the next
  resolve exactly as before, and `round.outcome.public.duels[]` then carries the full `challenge.end` for the same
  `duel_id` (the deed, the log, no `provisional`): replace the provisional one. After the close, lines are refused
  `duel_over` and ticks `no_duel`. (`duel_keys` never closes a duel: it is not part of the match's record.)

## Exact resume: `present` (state schema 8)

A match started by this server (state **schema 8** and up; older matches in flight keep the 0.8.1 behaviour) can be redrawn
at the exact present from one message. The whole data structure is in [TIME_TRAVEL.md](TIME_TRAVEL.md). Schemas:
`Present`, `PresentDuel` in `public_view`; `DuelClock` in `round_started_public`; `query_present`.

**Reconnect.** Open a socket, send `auth.resume {resume_token}`. A **seated** player gets `auth.ok`, `room.snapshot`,
`match.snapshot`. The match snapshot has the core's `round`, `phase`, `deadline`, `window` (the open slow window),
`started`, `brief`, `outcome`, `you`, and in `public_state`:

- `demigods[]`: every demigod;
- `rules { version, state_schema, server }`: the rules this match runs on (the server's version is also
  `server.hello.d.api.rules_version`);
- `present`:

```json
"present": {
  "server_time_ms": 1794690927811, "beat": 2, "live": true, "paused_since_ms": null,
  "mark": { "at_ms": 0, "seq": 0, "resolved_ms": 1794690925100, "round_ms": 1794690925300 },
  "events": [ { "event": "summoning", "seq": 5, "received_at_ms": 1794690926350, "beat": 2 },
              { "event": "challenge.lock", "duel_id": "duel:s6:0:2", "seq": 6, "received_at_ms": 1794690926360, "beat": 2 },
              { "event": "duel.blow", "duel_id": "duel:s6:0:2", "n": 1, "seq": 7, "received_at_ms": 1794690926371, "beat": 2 } ],
  "duels": [ { "duel_id": "duel:s6:0:2", "a": {}, "b": {}, "lock": {},
               "hp": { "a": 10.0, "b": 9.8 }, "range": "close", "initiative": "a",
               "stances": { "a": "neutral", "b": "off_balance" }, "set": { "a": null, "b": null },
               "blows": [ { "event": "duel.blow", "n": 1, "at_ms": 1794690926371, "seq": 7 } ],
               "n": 1, "typed": 1, "ko": null, "opened": false,
               "started_ms": 1794690926360, "deadline_ms": 1794691166360, "ends_beat": 97, "pauses": [],
               "clock": { "at_ms": 1794690927811, "elapsed_ms": 1451, "remaining_ms": 238549, "running": true },
               "over": false } ]
}
```

| Field | Use |
|---|---|
| `server_time_ms` | the server's clock when the view was built. Your clock offset is `server_time_ms - Date.now()` at receipt (or use `sys.pong.server_time`) |
| `events[]` | the public `phase.event` payloads of every instant action since the last resolve, in arrival order, each with `seq`, `received_at_ms`, `beat`. Replay them as if they had just arrived (flash a summoning already on its way, etc.) |
| `duels[]` | every open duel in full. `a`, `b`, `lock`: exactly `challenge.lock`. `blows[]`: every blow so far, in order, each the `duel.blow` it was shown as (die, damage, `hp`, `anim`, `choreo`, `counter`) with `at_ms` (its server stamp) and `seq` |
| `duels[].clock` | `elapsed_ms`: duel time elapsed, the offset to seek to. `remaining_ms`: until `deadline_ms`. `running: false` while the room stands still |
| `duels[].typing` | `{a: {len, at_ms}, b: {len, at_ms}}`: how long each side's unsent line is now (its last `duel.typing`; `at_ms` null: none since this server process started). Draw that many stars. Typing events themselves are never in `events[]` |
| `duels[].pauses` | `[from_ms, to_ms]` spans the duel's clock stood still. A blow's offset in the duel is `at_ms - started_ms` minus the pauses before it |
| `paused_since_ms` | the room stands still since then (also `match.snapshot.paused`); clocks are frozen at that moment |
| `live` | false only for a past state (the admin's state at a round) or a fork before its first Start |

`present` is also in every `round.committed.public_state`. `game.present` (public; also
`GET /api/rooms/{code}/query/present`) returns the same object: use it when you want it again without a resume. It
reads the whole state: ask it when you (re)connect, don't poll it.

**A viewer** (`room.join {viewer: true}`) gets the same `match.snapshot` (`you: null`) on joining, mid-duel included.

**A viewer who drops is gone.** The core removes a viewer from its room when its socket closes. Its `auth.resume`
then answers `auth.ok` and **nothing else**: no `room.snapshot`, no `match.snapshot`, no `room.left` (and a socket
that sends nothing is closed after 60 s). So a viewer client, right after `auth.ok` to a resume, sends
`room.join {house: true, viewer: true}` (or `{code, viewer: true}`) again; the reply is `room.snapshot` and the
`match.snapshot` follows at once. Don't wait for a snapshot first. A seated client can use one rule for both cases:
if no `room.snapshot` arrives within 1 s of `auth.ok`, join again. Send `sys.ping` every 20 s either way.

**Viewers and `round.brief`.** The core sends `round.brief` to seats only. A viewer gets `phase.changed`,
`phase.event`, `round.started`, `round.outcome`, `round.committed`, `seat.*`, `match.ended`, and reads the brief's
public part from `match.snapshot.brief` (as of its join) and the board from `round.committed.public_state`.

**After a server restart** the match is restored at its last commit and paused until a human connects. Resume as
usual. The snapshot has `paused: true`, `resumes_round`, and `present` as it was before the stop: the same duel,
blows, hp. Instant actions the server had accepted are not lost: they are resolved at the first beat. Text typed
into the interrupted beat's Compose or slow window IS lost (`you.action` is null, `window` is null): send it again.
At the first Start every open duel's deadline moves by the time the room stood still:

```json
"duel_clock": { "event": "duel.clock", "beat": 5, "now_ms": 1794690934100,
                "pause": { "from_ms": 1794690931063, "to_ms": 1794690934100, "ms": 3037 },
                "duels": [ { "duel_id": "duel:s6:0:2", "deadline_ms": 1794691169397, "pauses": [[1794690931063, 1794690934100]] } ] }
```

(`round.started.public.duel_clock`, only when there was a pause of 2 s or more: also after the room paused because
every human left.)

**Ids across restarts.** The core's `seq` restarts at 1 when the server restarts. Order events by
`(received_at_ms, seq)`, not by `seq` alone. `duel_id` stays unique within a match (a clash gets `seq + 1000000`).

## Wills that matter (0.9.0)

Room setting `wills_enabled` (default true; needs state schema 10). Rules: DM_RULEBOOK chapters 23-27. Everything
here is additive; a match without it sends none of these fields. There are **no new actions**: wills are the text
you already send (`fields.left` / `fields.right`). What changes is what comes back.

**What a will can now do** (say it in your own words; the hand and the rules split it into parts):

| Say | Part | What happens |
|---|---|---|
| "heal my demigods", "heal me fully" | `heal` | each of your own: a d20 vs DC 6 + magnitude/4; heals 1-3.5; a fail is a token heal plus a quest |
| "bless me" | `bless` | a boon: +1 on that demigod's d20s for 6 beats |
| "arm my demigods", "they get rocket launchers", "Nature, arm us with holy spears" | `arm` | real gear per demigod, with `mods` and a `power`; a god named or "holy / divine / god powers" blesses it (a tier up, the god's signature) |
| "my demigods come back to life", "revive the other demigods" | `raise` | Death's price in sparks and max-health years, or a quest; a wiped player may plead |
| "curse X with withering", "no resurrection for X", "bind X", "silence X", "hex X", "Death, wither X forever" | `curse` | a seal or curse on a rival's demigod; its bearer always gets a counter-quest |
| "what does my quest need?", "how do I break the curse?" | `ask` | your open quests' steps and progress, and the curses on you |
| "offer the sparks to the quest" | `offer` | pays a quest's sparks step |
| "gather the butterfly wings for the quest" | `gather` | makes the ingredient a quest's step is waiting on |

Prices are in sparks; a bolt breaks into 5 sparks (the change stays in your purse); what you can't pay is lent as a
debt while your open debt is under 3, else the effect shrinks or becomes a quest.

### `round.private.wills[]`: every will's answer

One entry per text box you used this beat (or per plea, when you are wiped). Real frame (trimmed to one heal and one
arming of the four parts):

```json
{ "text": "heal and arm my demigods", "by": "dg:0", "by_name": "Chris's Left Hand",
  "routed_to": "Nature", "gods": ["Nature", "Death"], "verdict": "granted",
  "speech": "Nature: \"Granted: Chris's Left Hand mends, Chris's Right Hand mends, because a ferryman overpaid his toll in a year nobody remembers.\" Death: \"Granted: Chris's Left Hand is armed, Chris's Right Hand is armed, because the third crow of the morning landed on the right side of the road.\"",
  "reason": "healed Chris's Left Hand 2; healed Chris's Right Hand 2; forged a Butterfly Wing Blade (artifact): +1 to strikes, +1 to journeys, +0.5 damage, +1 in duels; forged a Struck Oak Blade (artifact): +1 to strikes, +0.5 damage, +1 in duels",
  "parts": [
    { "part": "heal", "target": "dg:0", "target_name": "Chris's Left Hand", "god": "Nature",
      "roll": { "die": "d20", "roll": 8, "modifier": 1, "total": 9, "target_dc": 7, "verdict": "success",
                "mods": [{ "name": "paid in full", "value": 1 }] },
      "paid": { "price_sparks": 2, "sparks": 2, "bolts_broken": 0, "short_sparks": 0, "debt": 0.0 },
      "outcome": "success", "healed": 2.0, "result": "healed Chris's Left Hand 2" },
    { "part": "arm", "target": "dg:0", "target_name": "Chris's Left Hand", "god": "Death",
      "asked_tier": "artifact", "blessed": false,
      "roll": { "die": "d20", "roll": 14, "modifier": 0, "total": 14, "target_dc": 8, "verdict": "success", "mods": [] },
      "paid": { "price_sparks": 2, "sparks": 2, "bolts_broken": 0, "short_sparks": 0, "debt": 0.0 },
      "outcome": "success",
      "result": "forged a Butterfly Wing Blade (artifact): +1 to strikes, +1 to journeys, +0.5 damage, +1 in duels",
      "item": { "ref": "it:0", "name": "Butterfly Wing Blade", "kind": "weapon", "tier": "artifact", "slot": "weapon",
                "material": "butterfly-wing", "mods": { "strike": 1, "journey": 1, "damage": 0.5, "duel": 1 },
                "power": null, "blessed_by": null, "holder": "dg:0",
                "does": "+1 to strikes, +1 to journeys, +0.5 damage, +1 in duels" } } ] }
```

And a part that fell short (a natural 1 on a heal): it says why and what would work.

```json
{ "part": "heal", "target": "dg:0", "target_name": "Chris's Left Hand", "god": "Nature",
  "roll": { "die": "d20", "roll": 1, "modifier": 1, "total": 2, "target_dc": 7, "verdict": "crit_fail",
            "mods": [{ "name": "paid in full", "value": 1 }] },
  "paid": { "price_sparks": 2, "sparks": 2, "bolts_broken": 0, "short_sparks": 0, "debt": 0.0 },
  "outcome": "fizzle", "healed": 0.0,
  "result": "the mending of Chris's Left Hand fizzled (a natural 1); a quest will complete it (+2)",
  "why": "rolled a natural 1: the ferryman kept the change",
  "next": "quest q:0: 1. Hold a thread pulled from a general's banner (any item with \"thread\" in its name: will your demigod to gather or make it, or win an encounter).",
  "quest": { … Quest … } }
```

- `verdict`: `granted` (every part as willed), `partial` (something fell short: read the parts), `refused`.
- `parts[].outcome`: `crit`, `success`, `partial` (a token now and a quest, or gear a tier low), `fizzle` (a natural
  1: a quest), `quest` (nothing could be paid: a quest), `blocked` (withered), `raised`, `sealed`, `refused`,
  `unpaid`, `cast`, `backfired` (the curse lies on its caster), `told`, `offered`, `gathered`.
- Always show `result`; when present show `why` and `next`. `roll.mods[]` names every modifier.
- The gods' words are also in `omens[]`. Schema: `Will`, `WillPart`, `WillRoll`, `WillPaid` in
  [round_private](schemas/round_private.schema.json).

### Gear: what an item does

Every `Item` that is gear (a held weapon, armour, relic or artifact) gains `slot` (`weapon` / `armor` / `charm`),
`material`, `mods` (`strike`, `journey`, `deed`: d20 modifiers; `damage`, `guard`, `regen`, `thorns`: health;
`duel`; `resist`: a damage type), `power` (`{id: smite | last_stand | second_wind, name, text, used}` or null),
`blessed_by` and `does` (one line to show). It is in `seat_view.demigods[].items`, `my_items`, the new
`round.private.demigods[].items`, `game.item(s)`, and on every public demigod's `story.items[]` pegs
(`{ref, icon, name, slot, tier, mods, power, does}`). A demigod uses its best item per slot.

### Quests: `quests[]`

In the seat view and in `round.private` (your own quests, the public ones, and the counters of curses you cast; open
ones, and closed ones for 8 beats); the open public ones (raisings, seals' counters) are also in
`round.outcome.public.quests` and `public_state.quests`.

```json
{ "ref": "q:1", "for": "dg:3", "for_name": "Chris's Right Hand", "seat": 0, "helpers": [],
  "goal": { "kind": "break_curse", "curse": 0, "curse_kind": "wither" }, "god": "Death",
  "text": "Death has ruled: 4 sparks fed to the dark between two heartbeats, then your shadow laid down by night where the ground remembers being two grounds; only then will the curse of withering lift from Chris's Right Hand. (The engine's reason, as near as the DM can read it: Chris's Right Hand has 1000+ followers)",
  "steps": [
    { "step": 1, "kind": "sparks", "n": 4, "text": "Offer 4 sparks (will it: \"offer the sparks to the quest\"; a bolt breaks into 5).",
      "done": false, "done_beat": null, "progress": "4 sparks not offered yet" },
    { "step": 2, "kind": "at", "place": "fault", "phase": "night", "text": "Stand within 150 km of a fault line by night.",
      "done": false, "done_beat": null } ],
  "steps_done": 0, "status": "open", "given_beat": 11, "expires_beat": null, "done_beat": null,
  "failed": null, "failed_beat": null, "public": false, "waiting": null }
```

- Steps are done in order. `kind`: `at` (`place`: `shrine {of}` / `settlement {level}` / `fault` / `coast` /
  `point {at, name}`; optional `phase`; optional `offer`: the ingredient burned there; `near {name, at}`: a place of
  that kind to send the demigod to, when the server knows one), `hold {word}`, `encounter`, `shrine`, `duel`,
  `sparks {n}`, `army {at, name, general}`, `vigil {phase, times, seen}`.
- `progress` says why the current step is not done yet, as of the last beat.
- `goal.kind`: `heal`, `boon`, `temper`, `arm`, `break_curse`, `raise`. `status`: `open`, `done`, `failed`
  (`failed`: `expired` or `moot`). `waiting`: every step is done but a seal still holds the raising back.

### Curses: `curses[]`

On every public demigod (`demigods[].curses`), in `round.outcome.public.curses` and `public_state.curses`: the
**public face**. In your seat view and `round.private`: the same list, with the private fields on the curses that
lie on your demigods or that you cast.

```json
{ "ref": "cu:0", "kind": "wither", "title": "a curse of withering", "does": "no healing reaches it",
  "target": "dg:3", "target_name": "Chris's Right Hand", "strength": 2, "cast_beat": 11, "until_beat": 311,
  "god_tier": false, "god": null, "counter": "a mystic quest", "broken_beat": null, "broken_how": "",
  "owner": "dg:1", "owner_name": "Benito's Left Hand", "yours": false, "on_you": true, "paid_sparks": 5,
  "quest": { … the counter-quest … } }
```

The last line (`owner` … `quest`) is private. `kind`: `seal` (no resurrection), `wither` (no healing), `silence`
(its player's wills cost double), `bind` (a summon of it is refused with `bound:<quest ref>`), `hex` (minus its
strength on its d20s). `until_beat: null` = until countered. `broken_how`: `countered` or `faded`.

### Events

Public, in `round.outcome.public.dm_events[]` (shape as every DmEvent: `event`, `text`, `memory`, plus):

| `event` | Fields | When |
|---|---|---|
| `curse.cast` | the curse's public face, `at`, `cell` | a seal or curse lands (never who cast it) |
| `curse.broken` | the public face (`broken_how`: `countered` / `faded`) | its counter-quest is done, or its time ran out |
| `demigod.raised` | `demigod`, `name`, `by`, `by_player`, `by_seat`, `how` (`paid` / `on credit` / `quest q:N`), `years`, `at`, `cell`, `at_shrine` | a fallen demigod rises |
| `quest.given` / `quest.step` / `quest.done` / `quest.failed` | `quest`, `goal`, `demigod`, `name`, `seat`, `god`, `steps[]`, `step`?, `how`?, `result`?, `why`? | a **public** quest (a raising, a seal's counter) |
| `arms.power` | `demigod`, `item`, `power` | a Last Stand held |

The same quest events of your **private** quests (a heal's, an arming's, a curse's counter) arrive only for you (and
a curse's owner) in `round.private.will_events[]`, same shapes. `round.outcome.public.wills[]` carries one-liners of
this beat's public wills: `{event: "will.raised", by, by_name, target, target_name, text}` and
`{event: "will.curse", target, target_name, curse, text}`. A real raising:

```json
{ "event": "demigod.raised", "demigod": "dg:3", "name": "Chris's Right Hand", "by": "dg:0", "by_player": "Chris",
  "by_seat": 0, "how": "paid", "years": 1.0, "at": [-43.68, -21.58], "at_shrine": false, "beat": 22,
  "cell": "0:people:-783704:-774517", "memory": 448,
  "text": "Chris's Right Hand rises where it fell, raised by Chris's plea to Death (paid)" }
```

### Also in the seat view and `round.private`

`boons[]` (`{demigod, bonus, until_beat, from}`), `raises {granted, cap}`, and each of your demigods in
`round.private.demigods[]` now carries `items[]` and `curses[]`.

### A wiped player keeps the will box

With `wills_enabled`, **`seat.eliminated` is not sent when your last demigod falls**: the Theatre refuses every
submission of a seat that is out, and a wiped player may still plead ("my demigods come back to life"), ask and
offer. Read `players[].alive` and your seat view's `alive` for "all my demigods are down"; keep the WILL box open.
The beat that ends the match reports everyone who is out, as before. Anything else a wiped player wills is answered
in `wills[]` with `verdict: "refused"` and what they can do.

### Settings

`wills_enabled`, `will_spark_per_bolt`, `will_own_dc`, `will_debt_cap`, `will_debt_per_spark`,
`will_entropy_per_spark`, `heal_price_sparks`, `heal_token`, `heal_scale`, `boon_beats`, `boon_bonus`,
`arm_price_artifact`, `arm_price_relic`, `arm_bless_sparks`, `arms_scale`, `raise_price_sparks`, `raise_stale_beats`,
`raise_cap`, `raise_years`, `raise_health_fraction`, `curse_price_sparks`, `curse_god_price_sparks`, `curse_beats`,
`curse_dc`, `curse_max_per_player`, `curse_entropy`, `quest_steps_max`, `quest_expire_beats`, `quest_spark_step`,
`wills_ai_chance`: DM_RULEBOOK chapter 22, [settings](schemas/settings.schema.json), `GET /sim/settings`.

## Gear looks: loadouts, gear portraits and item art (state schema 11)

Room setting `looks_enabled` (default true; needs state schema 11). Everything here is additive: a 0.9.0 client that
ignores unknown keys keeps working, and the identity `portrait_image` / `diorama_image` on every demigod are unchanged.
There are **no new actions**. Rules and prompts: [MEDIA.md](MEDIA.md) "Looks"; the state: [TIME_TRAVEL.md](TIME_TRAVEL.md) §2.

**What it is.** Every demigod has a **loadout**: its notable gear right now (its best item per slot, `weapon`,
`armor`, `charm`, plus up to `look_extra_items` other major items: relic tier and up, god-blessed, or the first item
of a kind it has). When a **human player's** demigod's loadout changes (a *major acquisition*, or a loss), it takes a
new **look**: a new portrait and a new diorama of the same demigod WITH that gear. The built-in AI's demigods keep
their base images whatever they carry (their loadout is still listed). Notable items also get a picture of their own.

### `look` (on every public demigod and on your own in the seat view)

`public_state.demigods[]`, `game.globe` / `game.region` demigods, `seat_view.demigods[]`,
`match.ended.summary.players[].demigods[]`. Schema: `$defs/Look`.

```json
"look": {
  "loadout": [
    { "slot": "weapon", "item": "it:3", "name": "Butterfly Wing Blade", "icon": "sword", "tier": "relic", "kind": "weapon",
      "material": "butterfly-wing", "blessed_by": "Nature",
      "image_prompt": "a blade of iridescent butterfly-wing membrane veined like stained glass: a demigod's weapon, ornate, glowing steadily, wound with living vines and small green leaves",
      "image_kind": "item", "art": "5d0c…64 hex…", "image": { "hash": "…", "kind": "item", "status": "ready", "url": "/media/….png", "prompt": "…" } },
    { "slot": "armor", "item": "it:5", "name": "Moonsteel Plate", "icon": "armor", "tier": "artifact", "kind": "armor",
      "material": "moonsteel", "blessed_by": null }
  ],
  "loadout_hash": "9a41…64 hex…",
  "image_loadout_hash": "9a41…64 hex…",
  "since_beat": 7,
  "gear": true,
  "looks_made": 2, "looks_cap": 4,
  "portrait_image": { "hash": "…", "kind": "portrait", "status": "ready", "url": "/media/….png", "prompt": "…" },
  "diorama_image":  { "hash": "…", "kind": "diorama",  "status": "ready", "url": "/media/….png", "prompt": "…" },
  "pending": { "loadout_hash": "9a41…", "portrait_image": { "hash": "…", "status": "pending", "url": null, "…": "…" },
               "diorama_image": { "hash": "…", "status": "pending", "url": null, "…": "…" } }
}
```

- `loadout[]`: in a stable order (weapon, armor, charm, then `extra`s by tier). `slot` is the slot the item fills, or
  `extra`. An entry has `image_prompt` / `image` only when the item has item art (below); otherwise draw its `icon`.
- `loadout_hash`: sha256 over the loadout's normalized item descriptors (role, kind, material, tier, name stem,
  blessing), never item ids: the same gear is the same hash in every match. `""` = nothing notable.
- `image_loadout_hash`: the loadout the images show; `""` = the base identity images. It differs from `loadout_hash`
  while a new picture is debounced (`look_debounce_beats`) or once the demigod has had `looks_cap` pictures this match.
- **`portrait_image` and `diorama_image` are the images to draw for this demigod now** (images on only). They are
  the gear images of `image_loadout_hash` once those exist. While they render, they are the previous look's images
  (or the base identity images) and `pending` names the ones on the way: keep showing what you have and swap when the
  pending hash turns `ready` (poll `GET /media/index`, or wait for the `ready` event below). `pending` is absent when
  nothing is on the way. A gear image that will never come (budget, failure) has no `pending`: the previous images stay.
- `gear`: the images show gear. `since_beat`: the beat they became these.
- With images off (`FN7_IMAGES` unset) there are no image objects; the look instead carries the server's inputs
  (`base_portrait`, `base_diorama`, `portrait_subject`, `diorama_subject`, `fallback`). Ignore them.
- **Time travel:** the look is part of the state. A snapshot, a state at a round, a fork and a reconnect each show the
  look that was current at that beat, and its images are cached files (same hashes), so nothing is drawn again.

### `look.changed` (public event)

In `round.outcome.public.looks[]` (always complete) and, the same object, in `dm_events[]`. Schema: `$defs/LookChanged`.

```json
{ "event": "look.changed", "demigod": "dg:0", "name": "Chris's Left Hand", "player": "Chris", "beat": 7,
  "gained": { "item": "it:3", "name": "Butterfly Wing Blade", "icon": "sword", "tier": "relic", "kind": "weapon",
              "image_prompt": "…", "image_kind": "item", "art": "…", "image": { "…": "…" } },
  "loadout": [ "…as in look…" ], "loadout_hash": "…", "image_loadout_hash": "…", "since_beat": 7, "gear": true,
  "image_change": true, "kept": null,
  "text": "Chris's Left Hand now carries the Butterfly Wing Blade", "memory": 412, "at": [3.82, 25.04],
  "portrait_image": { "…the image to show now…": "" }, "diorama_image": { "…": "" },
  "pending": { "loadout_hash": "…", "portrait_image": { "status": "pending", "…": "…" }, "diorama_image": { "…": "…" } } }
```

- Sent the beat the loadout changes, **with `pending`** while the new images render (with cached images, e.g. the same
  demigod and gear in an earlier match, it has no `pending` and the images are final at once).
- **Sent once more when the images exist:** the same event with `"ready": true`, no `pending`, and the final
  `portrait_image` / `diorama_image`, in the `looks[]` of the next public payload of the match (`round.started.public`,
  `round.brief.public` or `round.outcome.public`). Match the two by `memory` (or `demigod` + `beat`).
- `gained`: the best item newly in the loadout; `null` when something was lost ("stands bare-handed again") or when
  only the picture caught up after a debounce.
- `image_change: false` with `kept: "debounced"` (a new picture was made less than `look_debounce_beats` beats ago; one
  follows when the wait ends, of the newest loadout) or `"capped"` (this demigod has had `look_images_per_demigod`
  pictures this match; the loadout keeps updating, the picture stays). Flash the item either way.
- Losing all gear goes back to the base images (`gear: false`, `image_loadout_hash: ""`); an earlier look worn again
  costs nothing (same hashes).

### Item art

An item with a picture has `image_prompt`, `image_kind: "item"`, `art` (the picture's key) and, with images on,
`image`. An item without one has none of these: draw its `icon`. Since schema 11 **only granted items have
`image_prompt`** (0.9.0 sent it on every item): a notable item (relic tier and up, god-blessed, or the first of its kind
its human player holds) that a human player's demigod holds, or a relic-tier or blessed item lying in a loot pile, up
to `item_art_per_match` per match. The picture is keyed by the item's descriptor (kind, material, tier, name stem,
blessing), so the same sword is the same picture for everyone, in every match.

Where it appears: `Item` (`items[]`, `my_items[]`, `game.items`, `game.item`), `look.loadout[]`, `look.changed.gained`,
story pegs (`story.items[]`: since schema 11 every peg also has `name` and `tier`), loot tokens (`loot[]`),
`round.private.wills[].parts[].item`, `manifests[]`, and the final piles' items.

### What the client should do

1. Draw each demigod's card and table piece from `look.portrait_image` / `look.diorama_image` when `look` is present
   (fall back to the demigod's own `portrait_image` / `diorama_image` when it is absent: an older match).
2. On `look.changed`: flash "NEW: `gained.name`" on that demigod's card (with `gained.image` or its `icon`), update its
   gear strip from `loadout[]`. If the event has `pending`, keep the current art and remember
   `pending.portrait_image.hash` / `pending.diorama_image.hash`; swap when the manifest (`GET /media/index`) shows them
   `ready` or when the `ready: true` event arrives, whichever is first. Cross-fade; it is the same character.
3. On reconnect or when scrubbing a recorded match, do nothing special: every snapshot's `look` already says which
   images to show.
4. Show item pictures where an item has `image`; otherwise the `icon`.
5. Cache every image by `hash` forever (they are immutable).

### Profile library

`game.library` / `GET /api/profiles/{name}/library`: a gear portrait or diorama has `looks: [{match_id, beat,
loadout_hash, loadout: ["Butterfly Wing Blade", "Moonsteel Plate"]}]` (oldest first: every match and beat it was a
demigod's look) and a caption like "Chris's Left Hand, portrait with Butterfly Wing Blade, Moonsteel Plate".

### Settings

`looks_enabled` (true), `look_extra_items` (2), `look_items_shown` (3), `look_images_per_demigod` (4),
`look_debounce_beats` (3), `item_art_per_match` (12): [settings](schemas/settings.schema.json), `GET /sim/settings`.

## Message flow for one beat

One Theatre **round** is one **beat**, so `round` and `beat` are always the same number. There are no choices: `round.private.choices`
is always `[]`, so the Choose phase is always skipped. This is one real beat as player `Juniper` received it (ms from
match start):

```
     S→C seat.joined         { seat, display_name, round, new_seat }   (a mid-match joiner plays from this round; then room.updated)
  5  S→C phase.changed       { round: 1, phase: "start" }
 10  S→C round.started       { round, public: RoundStartedPublic, private: RoundStartedPrivate }
     S→C seat.eliminated     { seat, round, reason }        (only if someone fell at Start; then room.updated)
 10  S→C phase.changed       { round: 1, phase: "brief" }
 11  S→C round.brief         { round, public: BriefPublic, private: SeatView }
 11  S→C phase.changed       { round: 1, phase: "compose", deadline }   ← deadline = epoch ms
 11  C→S action.submit       { round: 1, text, fields: { left, right, zoom } }   (orders without a window)
 12  S→C action.received     { round: 1, accepted: true }
         … at any time, in any phase (see "Real-time input"):
     C→S action.begin        {}                       → window.opened { window_id, by_seat, deadline } + phase.event Slowed
     C→S action.submit       { round, text, fields, window_id }   → action.received { accepted, window_id } (or reason "cut")
     S→C window.closed       { window_id, by_seat?, reason }
     C→S action.submit       { round, text: "", fields: { cast }, instant: true }  → action.received { instant: true }
     S→C phase.event         { round, phase, public: Streak }      (everyone, at once)
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
| `phase.event.public` | an immediate push: who slowed time (`Slowed`), a cast's streak (`Streak`) | [phase_event](schemas/phase_event.schema.json) | [phase.event](examples/phase.event.json) |

Generic frames recorded in the same match: [server.hello](examples/server.hello.json), [auth.login](examples/auth.login.json),
[auth.ok](examples/auth.ok.json), [room.create](examples/room.create.json), [room.join](examples/room.join.json),
[room.snapshot](examples/room.snapshot.json), [room.ready](examples/room.ready.json), [room.updated](examples/room.updated.json),
[room.start](examples/room.start.json), [room.countdown](examples/room.countdown.json), [phase.changed](examples/phase.changed.json),
[action.received](examples/action.received.json), [seat.eliminated](examples/seat.eliminated.json), and the real-time
frames [action.begin](examples/action.begin.json), [window.opened](examples/window.opened.json),
[window.closed](examples/window.closed.json).

On reconnect (`auth.resume`), `match.snapshot` carries the same objects: `public_state` is a PublicView, `started` /
`brief` / `outcome` hold the public parts of this round's messages, `you.private_state` is a SeatView, and
`you.started` / `you.brief` / `you.outcome` hold the private parts. It also carries `window` (the open action window,
`{ window_id, by_seat, deadline }`, or null) and `you.window_submitted` (you already have orders in it).

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
  "portrait"`), `diorama_prompt` (each demigod too: its table minifig, stable all match, with `diorama_image_kind:
  "diorama"`), `image_prompt` (every public world event, with `image_kind: "event"`; and every storyboard shot) and
  `statue_prompt` (the end-of-match piles, with `image_kind: "statue"`) describe the subject only; the media server
  renders them (see [`MEDIA.md`](MEDIA.md)). With images on, an **image object** `{hash, kind, status, url, prompt}`
  sits beside them (`portrait_image`, `diorama_image`, `image`, `statue_image`); only events the media style selects
  get one (moment cards).
- **Visual sheets.** Every demigod (PublicView, `game.globe`, `game.region`) and every person (`game.person`,
  `game.media`) carries `visual: {sheet_hash, traits: {build, face, outfit, colors: [a, b], prop, emblem}}`: a
  demigod's from its name alone (the same in every match), a person's from the match seed, id and name; the same at
  every beat and in every replay. Every image of the
  character is drawn from these traits. `sheet_hash` changes only if the sheet does.
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
  "flow": { "flow_ms": 3000, "slow_ms": 30000, "slow": null },
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
- `flow.slow` is always null since theatre faa7220 (the game no longer tracks open windows; kept for older clients). The
  live window is the Theatre's: `window.opened` / `window.closed` and `match.snapshot.window`.
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
| `slow` | null | Always null since theatre faa7220 (kept for older clients); the open window is the core's `window.opened` / `match.snapshot.window` |

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
  "slow": [ { "event": "closed", "by": "Rook", "by_ref": "dg:0", "window_id": 4, "how": "enter", "after_ms": 4200,
              "debt": 0.71, "paid_bolt": true, "text": "Time snaps back: Rook's window closed" },
            { "event": "instant", "by": "Vex", "by_ref": "dg:2", "after_ms": 0, "debt": 0.5, "paid_bolt": true,
              "text": "Vex gave orders without slowing time" } ],
  "casts": [ { "by": "Vex", "by_ref": "dg:2", "power": "flood", "icon": "flood", "power_name": "Flood", "cost": 2,
               "seq": 41, "instant": true,
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
- `slow` lists this resolve's slow-time settlements: `closed` (Time's bill for a window the core closed since the last
  resolve: `how` = `enter` (the initiator's submit) or `timeout` (the deadline; billed the full `slow_ms` even if
  nobody submitted), `after_ms` = the real time it stayed open, `debt`, `paid_bolt`), and `instant` (orders sent
  without a window, billed as a zero-second slow). Never who else engaged or was cut off. (`event: "open"` is no longer
  sent since theatre faa7220: the window itself is announced by the core's `window.opened` and the `Slowed`
  `phase.event`.)
- `casts` lists every cast settled this beat in arrival order (`seq`: the Theatre's room-wide order; the built-in AI's
  casts have `seq` 0 and go first); a fizzled one has `fizzled` (the reason: its caster could no longer pay, or the
  target doesn't exist) and cost nothing. Entropy's answer to each region's casts is an omen (`god: "Entropy"`, public when the cast's magnitude is
  ≥ 6), and the casts' ramifications are rows in `events` (and in the news of those who know them).
- `events` are the public, geotagged world events of the beat, in order (≤ 40): casts and their ramifications, armies
  raised, marching and fighting, freak weather, world mods appearing, stacking and fading, wounded gods, eliminations.
  Every public event carries `image_prompt` (with `image_kind: "event"`): a candidate moment card. With images on, the
  ones the media style's `event_selection` picks (by default: magnitude ≥ 8, or a cast, elimination, general
  appointed or mutiny; at most 12 per match) also carry `image`, an image object. It is usually `pending` when the
  outcome arrives: it never holds up the beat. It becomes `ready` via `GET /media/index`; a card still pending when its
  moment has passed can be skipped.

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
- Cut-offs are not in the SeatDelta: the Theatre answers the late submit itself with a private
  `action.received { accepted: false, reason: "cut" }` (rules 0.5 removed `held` / `cut_off`).
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
`portrait_prompt`, `image_kind: "portrait"`, `diorama_prompt`, `diorama_image_kind: "diorama"`, `visual` (its visual
sheet), the table fields below and, with images on, `portrait_image` and `diorama_image`. A live demigod has `pos`,
`place` and `region`; a dead one has `eliminated_beat` and `cause` instead. Health never appears here.

**The table** (on every demigod in the PublicView, `game.globe`, `game.region` and every row of `game.standings`):

| Field | Type | Meaning |
|---|---|---|
| `seat` | number \| null | The Theatre seat that plays it; `null` for a built-in AI demigod. A mid-match joiner's new seat takes over a built-in AI demigod, whose `seat` becomes that seat. |
| `controller` | `"human"` \| `"ai"` | `human`: a seat whose player is human plays it. `ai`: a Theatre bot seat or the built-in AI. Flips to `human` when a joiner takes it over (see [Joining mid-match](#joining-mid-match)). |
| `connected` | bool \| null | `true` for an AI-controlled demigod (always there). `null` for a human one: the game never sees presence, so combine with `room.updated` `seats[seat].connected`. |

Draw the table from these plus `room.updated`: a seat's aura is human or AI from `controller` (or, live, from
`seats[].is_bot`: a human who **leaves** mid-match is replaced by a core bot, which the game isn't told, so
`controller` stays `"human"` until someone takes the seat over), dimmed when `seats[seat].connected` is false.
`diorama_image` is the demigod as a tiny geometric miniature on a hex tile with its signature prop (identity only:
the same image in every match): the piece to stand on the table in front of its seat. `portrait_image` is its portrait card.

The planet edition adds:

| Field | Meaning |
|---|---|
| `entropy` | The Entropy meter (as in the brief) |
| `slow` | Always null since theatre faa7220 (kept for older clients; the open window is the core's `window.opened` / `match.snapshot.window`) |
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
  "storyboard": [
    { "shot": 0, "kind": "title", "beat": 0, "at": [-58.38, -34.6], "region": "Buenos Aires",
      "image_prompt": "the whole planet Earth on the eve of the battle royale: …", "image_kind": "event",
      "image": { "hash": "9f…", "kind": "event", "status": "pending", "url": null, "prompt": "…" },
      "caption": "Final Night 7", "duration_ms": 4000, "camera": "globe", "row": null, "refs": ["dg:0", "dg:1"] },
    { "shot": 1, "kind": "moment", "beat": 3, "at": [90.41, 23.81], "region": "Southern Asia",
      "image_prompt": "Vex casts flood on Dhaka, in Southern Asia; starring Vex (…)", "image_kind": "event",
      "image": { … }, "caption": "Vex casts flood on Dhaka", "duration_ms": 4000, "camera": "globe",
      "row": "lg:88", "refs": ["dg:1", "pl:12"] },
    { "shot": 5, "kind": "champion", "beat": 14, "at": [2.35, 48.86], "region": "Western Europe",
      "image_prompt": "Moth, a demigod: …", "image_kind": "portrait", "image": { …the champion's portrait… },
      "caption": "One walked away.", "duration_ms": 6000, "camera": "close", "row": null, "refs": ["dg:3"] } ],
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
- `storyboard` is the final cut as an ordered list of shots (an animatic: for each shot, fly the globe to `at`, show
  `image`, show `caption`, hold `duration_ms`). Shot kinds, in order: one `title` (the final cut's title over the
  arena, the demigods descending), up to 10 `moment`s (the match's biggest public moments in the order they happened:
  every elimination first, then casts, the society's turning points and the highest magnitudes), and one `champion`
  (the champion's portrait with the final cut's closing lines). `camera` is `"globe"` (a globe shot over `at`) or
  `"close"` (eliminations and the champion). `image` is an image object (`kind` `event` or `portrait`) with images on,
  `null` with them off. A moment's image is the same picture as its moment card's, and the champion's is the same as
  its `portrait_image`. `row` is a moment's ledger row; `refs` are who is in the shot. It's deterministic: a replay of
  the match gives the same storyboard.

## The action: `action.begin`, `action.submit { round, text, fields, window_id?, instant? }`

There are **two separate input paths**, on the Theatre's real-time input (engine v2.2; PROTOCOL.md "Real-time input"):
text entry goes into slow-time windows, casts are instant actions. One action carries one path: an action with both
orders and casts is refused, and an instant action carrying orders is refused on arrival (`action.received {accepted:
false}` with the reason; the game sees how each submit arrived since theatre faa7220).

### Real-time input: what the server sends and when

| Frame | Who gets it | Meaning for FN7 |
|---|---|---|
| C→S `action.begin {}` | (never broadcast) | You started typing: the God of Time interaction. One per seat per window (or per round when no window is open). |
| S→C `window.opened { window_id, by_seat, deadline }` | everyone | Time slowed: a window opened by `by_seat` until `deadline` (epoch ms; `slow_ms` after it opened). It comes **before** the `phase.event` that names the demigod and before the `ack` to the opener's `action.begin`. |
| S→C `phase.event { round, phase, public: Slowed }` | everyone | `{ event: "slowed", by, by_ref, by_seat, slow_ms, text: "<name> slowed time" }` |
| C→S `action.submit { round, text, fields, window_id }` | | Orders into the open window (any phase). Resubmitting before it closes replaces yours. |
| S→C `action.received { round, accepted: true, window_id }` | you | Held, hidden. Nobody learns you typed or submitted. |
| S→C `window.closed { window_id, by_seat?, reason }` | everyone | `reason`: `submitted` (the initiator's Enter; `by_seat` = the initiator), `deadline`, `match_ended`. |
| S→C `action.received { accepted: false, reason: "cut", window_id }` | only you | You submitted to a window that had already closed: your orders did not go through. Nobody else is told you tried. |
| C→S `action.submit { round, text: "", fields: { cast }, instant: true }` | | A cast, any phase. Rate-limited per seat (2/s, burst 4; `rate_limited` beyond). |
| S→C `action.received { round, accepted, reason?, instant: true }` | you | `accepted: false` with the validation message (e.g. not enough bolts) when refused. |
| S→C `phase.event { round, phase, public: Streak }` | everyone | The cast's streak, at once: `{ event: "streak", by, by_ref, by_seat, casts: [...], seq, received_at_ms, power, power_name, icon, cost, at, region, region_ref, place, target, target_name }` (the top-level fields repeat the first cast). |
| S→C `seat.revived { seat, round, reason }` | everyone | An eliminated seat is back in play (then `room.updated` shows it `active`). |
| S→C `seat.joined { seat, display_name, round, new_seat }` | everyone | A mid-match joiner plays `seat` from `round` (room setting `allow_midmatch_join`; see [Joining mid-match](#joining-mid-match)); then `room.updated`. |

`match.snapshot` (on resume) carries `window` (`{ window_id, by_seat, deadline }` or null) and `you.window_submitted`.
Windows don't survive a server restart.

### 1. Text entry: orders to your hands (the God of Time interaction)

```json
{ "t": "action.begin", "id": "b7", "d": {} }
{ "t": "action.submit", "id": "a7",
  "d": { "round": 12, "text": "", "window_id": 11,
         "fields": { "left": "raise an army from the dockworkers and march on Lagos",
                     "right": "slow time and delay my debt\nsend a scout to spy on Vex",
                     "zoom": { "level": "city", "focus": "pl:41" } } } }
```

| Field | Type | Meaning |
|---|---|---|
| `window_id` | int | The open window (`window.opened.window_id`). Without it, while a window is open, the server targets the open one; with no window open the orders are this round's Compose action. |
| `fields.left` / `fields.right` | string \| null, ≤ 2000 chars | Your orders to that hand: free speech, several commands, newline-separated. Sending them is your **Enter**. An empty hand does nothing and makes no LLM call. |
| `fields.zoom` | null \| `""` \| level \| `{ level, focus? }` | An optional view change. `level` is `world`, `district`, `city` or `character` (case-insensitive; `map`/`planet`/`globe` = world, `region` = district, `person` = character). |
| `fields.zoom.focus` | string \| null, ≤ 200 chars | A ref (`"ch:12"`, `"pl:41"`, any ref that exists) or the exact name (case-insensitive) of something visible at district zoom. |
| `text` | string, ≤ 500 chars (core cap) | **Fallback** when `left` and `right` are both absent or null: lines starting `l:`/`left:` go to the left hand, `r:`/`right:` to the right hand, any other line to the left hand. |

The v0 fields `fields.slow`, `fields.sent_at`, `fields.window` and a `/slow` text line were removed in rules 0.5 and
are refused (`bad_request`, with a message pointing at `action.begin`). The server stamps arrival order itself.

**How a slow window works.**
- **Opening.** `action.begin` from a live demigod with at least 1 bolt opens a window (one per room) for `slow_ms`
  (default 30 s), closed by the opener's own submit. Everyone gets `window.opened` and the `Slowed` `phase.event`
  (who). A demigod that has fallen or holds no bolt gets only the `ack`: no window (its orders can still go as a
  Compose action).
- **The world keeps moving.** Flow beats keep resolving while the window is open: standing orders, armies, casts,
  Entropy, the AI. Nothing is settled until the window closes.
- **Others may type.** Another seat's `action.begin` while a window is open only registers it (never shown). Its
  `action.submit` with the `window_id` is held, hidden.
- **Enter.** The opener's submit closes the window at once (`window.closed { reason: "submitted" }`). Everyone who
  submitted before it goes through (at the next resolve, in arrival order); anyone who submits after gets the private
  `cut` reply. If the opener never presses Enter, it closes at its deadline (`reason: "deadline"`) and whatever was
  submitted goes through.
- **Time's penalty**, settled at the first resolve after it closes: Time takes **1 bolt** as a toll (since rules
  0.6.0 nobody else gains from it; Entropy still rises 0.4) and writes a debt of `0.5 × (1 + seconds open / 10) × (1 + 0.5 × the opener's slows in the last 20 beats)` damage
  points (doubled if the bolt can't be paid), due in `debt_due_beats`; Entropy rises by `0.5 + seconds / 30`. "Seconds
  open" is real time: from the window's opening to the server closing it on the Enter (or the deadline: a window that
  times out bills its opener the full `slow_ms`, even if nobody submitted). Pressing Enter
  early is cheap, and it cuts off whoever is slower.
- **Orders without a window** (a Compose submit with no window open) are an instant order (`{event: "instant"}`):
  the same toll and Entropy as a zero-second window, but a flat debt of `instant_order_debt` (default 0.5; no repeat
  or seconds factor; doubled if the bolt can't be paid). They still count as slows for a later window's repeat
  factor.
- Orders from a window open at least one flow beat (`flow_ms`) get up to `max_intents_per_window` acts per hand (12);
  others get `max_intents_per_hand` (6).

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

### 2. Casts: god powers (instant actions)

```json
{ "t": "action.submit", "id": "a8",
  "d": { "round": 12, "text": "", "instant": true,
         "fields": { "cast": { "power": "volcano", "target": [3.38, 6.52] }, "bolts": ["dg:2"] } } }
```

| Field | Type | Meaning |
|---|---|---|
| `fields.cast` | `{ power, target, text?, magnitude? }` | One power. `power` is a slug from `round.brief.public.powers` or `"custom"`. |
| `fields.casts` | array of the same | Several (at most 5 per action across all four fields). |
| `fields.bolt` / `fields.bolts` | `{ target }` or the bare target / array | Aliases for casting `lightning`. |
| `target` | ref \| `[lon, lat]` \| `"lon,lat"` \| name | A ref (`"dg:2"`, `"cl:7"`, `"pl:41"`), any point on the globe, or a name (a demigod, region, city, crowd or person). A body or crowd target takes the power's direct damage; a place or a point is struck at its position. |
| `text` | string ≤ 200 | `custom` only (required): what the power is ("a rain of frogs over the stadium"). |
| `magnitude` | 0..10 | `custom` only (default 5): its size; it costs `1 + magnitude/3` bolts (at most 4). |

- **On arrival** (any phase) the server checks the action against the last committed state: known powers, valid
  targets, and enough bolts for the action's total. A refused cast gets a private `accepted: false` with the reason.
  An accepted one is shown to everyone at once as a `Streak` `phase.event` (its aim; nothing is paid yet).
- **At the next resolve** casts are paid in arrival order (`seq`; the plugin's AI first), before anything else in the
  beat. A cast costs its `cost` in bolts. **Every other live demigod gains 2 bolts per bolt spent** (on casts and god
  invocations; not on Time's toll for orders), and Entropy rises 0.4 per bolt. A cast its caster can no longer pay (several casts each validated against the same last commit, or
  bolts spent on Time first) **fizzles** (`casts[].fizzled`, nothing spent).
- Every cast gets **Entropy's check, which always says yes** (an omen in Entropy's voice). Then the ramifications: one
  LLM call per region with casts this beat writes N ramifications (`1 + cost + magnitude/3` per cast, at most 10):
  harm, healing, crowds' moods, people flung about, things happening at points near the strike, and **world mods**
  (lasting changes: `public_state.mods`). They arrive as geotagged `events` and news rows.
- At Entropy ≥ 40 (physics), a third of casts land 50–400 km off target (`off_target: true`).
- A cast never carries orders and never slows time: an action with both is refused. (Casts in a Compose or windowed
  action are still settled, by `seq`; instant is the path for clients.)

### Both paths

- **Errors:** a malformed action (`left` not a string, an unknown zoom level, a hand over 2000 chars, an unknown power,
  a custom power without `text`, more than 5 casts) gets an `error` reply with `bad_request` and a readable `message`.
  An instant action with a validation problem gets `action.received { accepted: false, reason }` instead. A non-instant
  submit outside Compose with no window open, for the wrong round, or while eliminated gets `wrong_phase`. A `text` over
  500 chars gets `too_long`.
- **Default action:** if you don't submit before the deadline, you get silence. The world keeps moving: Time's clock,
  debts, your standing orders, armies and live subagents all still run.
- **Resubmitting** while Compose (or the window) is open replaces your orders. Each instant cast is its own action.

## Room settings (`room.create { settings }` / `room.settings`, returned as `match.started.rules`)

Every key is optional. Integers may also be sent as whole floats (`3.0`). An out-of-range value is rejected with a
readable `bad_request`. **Unknown keys are dropped silently.**

| Key | Type | Default | Range | Meaning |
|---|---|---|---|---|
| `seed` | int | picked at match start | ≥ 0 (u64) | All randomness derives from it. Not echoed in `rules` when unset. |
| `arena` | string | `"planet"` | `"planet"` \| `"city"` | The whole Earth (regions → cities) or one real city (districts → sites) |
| `city` | string | seeded pick (≥ 1M people, on land) | ≤ 80 chars | Fuzzy-matched. On the planet: where Time's circle closes for the Final Night; in a city arena: the arena |
| `players` | int | 4 | 1–8 | Players in the match (two demigods each at rules 0.7.0). Every seat gets one, and the built-in AI plays the rest. Effectively `max(seats, players)`. |
| `beats_total` (alias `rounds`) | int | 60 | 1–1000 | The Final Night falls on this beat (or earlier: heat death, or nobody standing). `rounds` applies only when `beats_total` is absent. |
| `flow_ms` | int | 3000 | 500–120000 | Flow beat length. `compose_seconds` defaults to `flow_ms / 1000`. |
| `slow_ms` | int | 30000 | 1000–600000 | How long a slow window stays open at most (the Theatre window's deadline) |
| `beat_ms` | int | = `flow_ms` | 500–120000 | Alias of `flow_ms` (used when `flow_ms` is absent; echoed as `flow_ms`) |
| `start_bolts` | int | 1 | 0–99 | Alias of `bolt_start` |
| `bolt_start` | int | 1 | 0–99 | Rules 0.7.0: lightning bolts (mega currency; also Time's toll and gods' prices) each player starts with |
| `spark_start` | int | 5 | 0–99 | Rules 0.7.0: sparks (mini currency) each player starts with |
| `bolt_gift` | int | 1 | 0–10 | Rules 0.7.0: bolts every other player gains per mega cast |
| `spark_gift` | int | 0 | 0–10 | Rules 0.7.0: sparks every other player gains per mega cast |
| `entropy_per_mega` | number | 10 | 0–100 | Rules 0.7.0: Entropy per mega cast (× `entropy_scale`) |
| `entropy_per_mini` | number | 1 | 0–20 | Rules 0.7.0: Entropy per mini cast (× `entropy_scale`) |
| `entropy_scale` | number | 1.0 | 0–10 | Multiplier on every Entropy rise (a playtest dial: 4 = heat death four times as fast) |
| `clusters` | int | 200 | 8–600 | Population clusters created at setup |
| `max_subagents_per_hand` | int | 3 | 0–10 | Live subagents + possessions per hand |
| `mind_timeout_ms` | int | 6000 | 50–120000 | Per-call timeout for every LLM mind. A late call takes its rules fallback. |
| `llm` | bool | true | | `false` = never call the LLM (rules fallbacks only; captions become templates) |
| `debt_scale` | number | 0.04 | 0–10 | Consequence's bill multiplier, per demigod per beat |
| `debt_due_beats` | int | 50 | 1–100 | Beats until a bill falls due (Consequence's and Time's) |
| `instant_order_debt` | number | 0.5 | 0–10 | Time's flat debt for orders sent without a slow window (no repeat or seconds factor; doubled if the bolt can't be paid) |
| `arena_close_beats` | int | 160 | 1–1000 | Time's circle reaches its end radius (1,500 km on the planet) on this beat; its damage ramps on the same schedule. Independent of `beats_total`. |
| `accident_scale` | number | 0.82 | 0–10 | Multiplier on the rules fallback's Final Destination accident chance (1 in 20 per non-attack act at 1) |
| `army_strike_scale` | number | 0.82 | 0–10 | Multiplier on the damage an army does when it storms a demigod |
| `army_first_host_scale` | number | 0.82 | 0–10 | Multiplier on the extra chance a crowd near an unarmed demigod raises their first volunteer host |
| `memory_per_owner` | int | 40 | 4–1000 | Live memories per owner before old ones fold into a summary |
| `llm_folds_per_beat` | int | 2 | 0–32 | Memory folds the LLM writes per beat (the rest are done by rules) |
| `max_intents_per_hand` | int | 6 | 1–12 | Acts one hand makes from orders (other than the next row's) |
| `max_intents_per_window` | int | 12 | 1–24 | Acts one hand makes from orders delivered from a window open at least `flow_ms` |
| `max_memorize_per_beat` | int | 6 | 0–64 | People memorized from player mentions per beat |
| `max_witnesses` | int | 4 | 1–64 | Witnesses recorded per act |
| `society_only` | bool | false | | World-only simulation: the built-in AI demigods give no orders and cast nothing |
| `general_calls_per_beat` | int | 2 | 0–16 | General-mind LLM calls per beat at decision points |
| `demigods_per_player` | int | 2 | 1–2 | Rules 0.7.0: 2 = each player's Left and Right hands are two demigods, each with its own health; 1 = one shared body (the old game) |
| `manifest_scale` | number | 1.0 | 0–10 | Rules 0.7.0: multiplier on the item engine's bolt price per tier |
| `manifest_muster_scale` | number | 0.25 | 0–1 | Rules 0.7.0: a manifested host's army footprint in the simulation (share of `muster_size(count)`) |
| `manifest_calls_per_beat` | int | 2 | 0–16 | Rules 0.7.0: manifest-mind LLM calls per beat (names for wonders and up, and twists) |
| `final_wish_magnitude` | int | 2 | 0–5 | Rules 0.7.0: magnitude added to every act of the Final Wish (beat 1) |
| `compose_seconds` | number | `flow_ms / 1000` | (0, 600] | The core's Compose timer (fixed for the room). If you set it explicitly, it no longer follows `flow_ms`. |
| `resolve_timeout_s` | number | 60 | (0, 1800] | The core's resolve timeout |
| `allow_midmatch_join` | bool | false | | The core's room flag: humans may join a running match ([Joining mid-match](#joining-mid-match)). Echoed in `rules` only when set. |
| `choose_seconds` | number | (core) | (0, 600] | Unused: there is no Choose phase |

For a playtest, set `beats_total` and `flow_ms` (or `beat_ms`) and leave `compose_seconds` unset. The server's `rules` then contain
every key above except `rounds`, plus `seed`, `city`, `choose_seconds` and `allow_midmatch_join` only when you set them.

## Timing and phases

| Phase | Timed? | What happens | You receive |
|---|---|---|---|
| `start` | no | Time's clock shrinks the arena and hurts bodies outside it, debts fall due, subagents expire | `round.started`, maybe `seat.eliminated` + `room.updated` |
| `brief` | no | | `round.brief` |
| `compose` | **yes**: `deadline` = epoch ms, `compose_seconds` (= `flow_ms`) after it opens | Orders without a window (windows and casts work in every phase) | `phase.progress { done, total }` |
| `resolve` | no (capped by `resolve_timeout_s`, default 60) | Time's bills for closed slow windows → casts (in `seq` order) → hands → mentions → route → one batch of acts (resolvers, gods, subagents, paladins, possessions, ramifications) → merge with the world's rules (armies, Entropy, mods) → consequence; the previous beat's narrator runs alongside | `phase.progress` at least every 5 s |
| `reveal` | no | | `round.outcome`, `round.private` |
| `choose` | never entered | | |
| `commit` | no | Heat death's collections, eliminations and the Final Night survivor rule | `round.committed`, maybe `seat.eliminated` + `room.updated`, `round.private_state` |

- **Flow beats.** Every round is a flow beat: Compose lasts `flow_ms` (the core's `compose_seconds`) and then the world
  resolves, whether or not anyone submitted. A beat nobody spoke or cast in makes no LLM call and resolves in
  milliseconds; a beat with orders or casts takes the LLM's time (the hands, then one batch of acts; each call capped
  by `mind_timeout_ms`).
- **Compose closes early once every active seat has submitted a Compose action.** Windowed and instant actions don't
  count toward it. A Compose submission that arrives after Compose has closed (with no window open) gets
  `wrong_phase`.
- **Windows and instant actions ignore phases.** A window stays open across beats; held orders and casts reach the
  game at the next resolve after they arrive (a window's orders: after it closes).
- Use `deadline` against your clock and correct for skew with `sys.pong.server_time`.

## Game queries (`game.*`)

Queries are read-only requests that return game data on demand. The pushed messages above deliver each beat as it
happens. A query answers from the room's current game state, which changes at each beat's Resolve and Commit, so ask
again after `round.committed` to get the new beat. Queries work in every phase and after the match ends (against the
final state). Before the first match they fail with `wrong_phase`. The engine side is in PROTOCOL.md "Game queries".

**The manifest.** `server.hello.api.queries` lists every query as `{ name, description, args_schema, public }`. The
reply schemas are [`docs/schemas/query_<name>.schema.json`](schemas/). Each file also carries the args schema
(`x-args`) and whether the query is served over HTTP (`x-public`). A test validates real replies against them,
strictly.

**WebSocket** (you must be seated in the room; spectators count). The answer is computed for your seat:
```
C→S { "t": "game.region", "id": "q7", "d": { "ref": "pl:1", "limit": 2 } }
S→C { "t": "game.region", "re": "q7", "d": { …the reply… } }
S→C { "t": "error", "re": "q7", "d": { "code": "bad_request", "message": "`limit` must be 1..50" } }
```
`not_found` = no such query. `bad_request` = the game refused the args (the message says why). `wrong_phase` = no
match yet.

**HTTP** (public queries only; no socket and no seat, so the answer carries public information only):
```
GET http://<host>:<port>/api/rooms/{code}/query/{name}?arg=value&…
200 → the reply JSON     400 → refused args (text)     403 → not a public query (`me`)
404 → no such room or query     409 → no match yet
```
Over HTTP every arg is a string. Numbers are sent as numeric strings (`limit=20`), and refs as written (`ref=pl:1`).
Unknown args are ignored, including the ngrok gate's `?key=`.

**Conventions.**
- **Cheap, small answers.** Queries run on the room's own loop and are built to take about a millisecond (a median
  of 2.2 ms for the slowest on a planet at beat 50, release build). Lists that grow are **paged**. A paged list is
  `{ items, next_cursor, total }`: pass `next_cursor` back as `cursor` to get the next page (`null` = the last page).
  `limit` sets the page size, with each query's default and maximum.
- **Events are paged by ledger row.** An event page is `{ items, since, next, more, head }`. `items` are public
  geotagged events in ledger order (oldest first), each a `round.outcome.public.events` entry plus `beat` and
  `region_ref`. `head` is the number of ledger rows so far. With `since: N` you get events from row `N` on. `next` is
  the row to pass as `since` next time, and `more: true` means more rows are waiting to be scanned right now (one call
  looks at no more than 2000 rows). Without `since` you get the latest `limit` events, and `next` = `head`. So start
  without `since`, then ask with `since: next`, and you get every public event exactly once.
- **Hidden information.** Public queries return only what the PublicView already shows, and they return the same
  answer to every asker. A seat-scoped field uses the asking seat's demigod. For a public asker (HTTP) that field is
  absent (`person`) or reduced (`army`). Health is never in a public answer.
- Floats are rounded as in every payload. All ids are refs. Two prefixes appear only in society answers: `ce:` (a
  cell) and `mb:` (a mob). With media on, image fields appear next to the prompts as in [`MEDIA.md`](MEDIA.md).

| Query | Public | Args | Reply |
|---|---|---|---|
| `globe` | yes | `since?`, `limit?` (30, ≤ 200) | The globe in one call: arena, demigods, players, regions with hotspots, influence, mods, armies, Entropy, beat, an event page |
| `region` | yes | `ref` (a region `pl:`), `cursor?`, `limit?` (10, ≤ 50) | One region: node, hotspot, cities, crowds (paged, with piece `pos`/`cell`), mobs (`pos`, `cell`, `ringleader_ref`), armies, demigods, mods, influence, recent events |
| `node` | yes | `ref` (`pl:` / `cl:` / `ce:` / `mb:`) or `cell` (a hex key, rules 0.8.0), `cursor?`, `limit?` (20, ≤ 100) | One hierarchy node and what is under it; a hex zoomed into (observed or extrapolated) |
| `person` | yes (subset) | `ref` (`ch:`), `cursor?`, `limit?` (20, ≤ 100) | Role, rank, place; for your seat: allegiance you know, memories you can see |
| `army` | yes (subset) | `ref` (`cl:` of an army) | General, captains, size, position, morale; its owner also gets orders and the full chain |
| `events` | yes | `since?`, `limit?` (50, ≤ 200), `region?` (`pl:`) | An event page (+ `beat`) |
| `me` | **no** | none | Your SeatView, now |
| `powers` | yes | none | The power palette |
| `gods` | yes | none | The pantheon with each god's health |
| `standings` | yes | none | Every demigod ranked now; rules 0.7.0 adds `players[]` (each player ranked, both demigods' state, never health) and `players_alive` |
| `items` | yes (public items; your own in full) | `holder?` (`dg:`), `state?`, `min_tier?`, `cursor?`, `limit?` (20, ≤ 100) | Rules 0.7.0: a page of items, most recent first |
| `item` | yes (public items; your own) | `ref` (`it:`) | Rules 0.7.0: one item, its manifestation (your own with the verdict) and custody history |
| `memory` | yes | `place` (hex key / `pl:` / `"world"`), `until_beat?`, `limit?` (50, ≤ 200) | Rules 0.8.0: the DM's decisions about a place and everything above and below it ("DM rulebook features (0.8.0)") |
| `encounters` | yes | `region?` (`pl:`) | Rules 0.8.0: the encounter hexes (hidden ones only as marked) |
| `piles_final` | yes | none | The end-of-match piles; an error until the match is over |
| `media` | yes (public refs) | `kind` (`portrait` / `place` / `event` / `army`), `ref` | An image of that thing, by reference: an image object at once (pending, then ready via the manifest) |
| `profile` | yes | `name?` (default: your seat's name) | A player's persistent profile (see "Profiles, history and library") |
| `history` | yes | `name?`, `page?` (0), `per_page?` (20, ≤ 100) | A page of their matches, newest first |
| `library` | yes | `name?`, `page?`, `per_page?`, `kind?` | A page of their images, most recently seen first |

### `game.globe`

The arena circle, every demigod as in the PublicView, each demigod's share of each region, live world mods, live
armies (markers, without orders), the Entropy meter and the beat, plus a page of public events.

```
C→S { "t": "game.globe", "id": "g1", "d": { "since": 0, "limit": 3 } }
GET /api/rooms/66X6KM/query/globe?since=0&limit=3
```
```json
{ "beat": 7, "beats_total": 14, "final_night": false, "alive": 4,
  "arena": { "kind": "planet", "name": "Earth", "city": "Santo Domingo", "center": [-66.918983, 10.502944],
             "radius_km": 5119.19, "start_radius_km": 14102.77, "end_radius_km": 1500.0 },
  "entropy": { "value": 11.0, "max": 100.0, "level": 0, "key": "calm", "text": "The world is holding together, mostly.",
               "next_at": 20.0, "heat_death": null },
  "demigods": [ { "ref": "dg:1", "name": "Vex", "seat": null, "alive": true, "bolts": 18, "pos": [-74.87795, 20.049207],
                  "place": "Caribbean", "region": "Caribbean", "portrait_prompt": "Vex, a demigod: …", "image_kind": "portrait" }, … ],
  "influence": [ { "region": "Australia and New Zealand", "region_ref": "pl:1", "demigod": "Vex", "demigod_ref": "dg:1", "share": 0.01 }, … ],
  "mods": [ { "id": 0, "name": "Wildfire scar", "description": "The wildfire leaves Chicago changed for good",
              "at": [-87.92, 42.61], "radius_km": 200.0, "region": "Northern America", "region_ref": "pl:41",
              "tags": ["fire", "plants"], "stacks": 1, "since_beat": 1, "expires_beat": 11, "power": "wildfire", "by": "dg:2" } ],
  "armies": [ { "ref": "cl:241", "name": "the Volunteers of Kyiv for Vex", "demigod": "Vex", "demigod_ref": "dg:1",
                "rogue": false, "size": 29997, "pos": [6.887534, 51.232243], "region": "Western Europe",
                "dest": [-66.92, 10.5], "target": "pl:0", "stance": "marching", "morale": 0.82 } ],
  "events": { "since": 0, "next": 5, "more": true, "head": 103,
              "items": [ { "row": "lg:2", "beat": 1, "kind": "cast", "text": "Mara casts wildfire on Chicago",
                           "at": [-87.64, 41.85], "region": "Northern America", "region_ref": "pl:41", "refs": ["dg:2"],
                           "magnitude": 7, "image_prompt": "Mara casts wildfire on Chicago, in Chicago, Northern America",
                           "image_kind": "event" }, … ] } }
```
`demigods`, `influence`, `mods`, `entropy` and `arena` are exactly the PublicView's (`round.committed.public_state`).

### `game.region`

```
C→S { "t": "game.region", "id": "r1", "d": { "ref": "pl:1", "limit": 2 } }
GET /api/rooms/66X6KM/query/region?ref=pl:1&limit=2
```
```json
{ "ref": "pl:1", "name": "Australia and New Zealand", "center": [156.982882, -36.179171], "radius_km": 1988.45,
  "node": { "ref": "pl:1", "name": "Australia and New Zealand", "level": "region", "population": 8532215, "base": 8532215,
            "mood": { "anger": 0.23, "fear": 0.33, "hope": 0.4, "devotion": 0.44 }, "need": { "food": 0.14, "safety": 0.34, "faith": 0.24 },
            "allegiance": [ { "demigod": "Vex", "demigod_ref": "dg:1", "value": 0.01 }, … ],
            "worship": [ { "god": "Time", "god_ref": "gd:0", "value": 0.11 }, … ], "supply": 0.96, "stance": "calm",
            "leader": null, "dead": 0, "at": [156.98, -36.18], "parent": "pl:0" },
  "cities": [ { "ref": "pl:2", "name": "Sydney", "level": "city", "population": 939351, …, "parent": "pl:1" }, … ],
  "crowds": { "items": [ { "ref": "cl:11", "name": "Parents and children of Auckland", "size": 3547180, "place": "pl:86",
                           "at": [174.8, -36.73], "mood": { … }, "need": { … }, "stance": "calm",
                           "allegiance": [ … ], "leader": null, "dead": 0, "cells": [] }, … ],
              "next_cursor": 2, "total": 12 },
  "mobs": [], "armies": [], "demigods": [], "mods": [],
  "influence": [ { "region": "Australia and New Zealand", "region_ref": "pl:1", "demigod": "Vex", "demigod_ref": "dg:1", "share": 0.01 } ],
  "events": { "since": null, "next": 103, "more": false, "head": 103, "items": [] } }
```
- `ref` must be a region (`round.brief.public.regions[].ref`). In a city arena it is a district.
- `node` is the region's rolled-up state (a **Node**: `{ref, name, level, population, base, mood, need, allegiance,
  worship, supply, stance, leader, dead, at, parent}`). `leader` is a **Who** (`{ref, name, alive, role, title}`) or
  null. `cities` are Nodes too.
- `crowds` lists its crowds, biggest first, paged (a **Crowd**: `{ref, name, size, place, at, mood, need, stance,
  allegiance, leader, dead, cells}`, with each live **Cell** as `{ref, name, kind, size, crowd, leader, stance, mob,
  at}`).
- `mobs` are its live mobs (a **Mob**: `{ref, name, size, action, target, ringleader, cause, at, city, place, region,
  side, anger, fear, cells, formed_beat, ended_beat, casualties, kills}`). `armies` and `demigods` are the live ones
  standing in it (army markers as in `globe`, demigods as in the PublicView). `mods` are those centered in it, and
  `influence` is this region's shares. `events` are its 10 latest public events.

### `game.node`

```
C→S { "t": "game.node", "id": "n1", "d": { "ref": "pl:2", "limit": 2 } }
GET /api/rooms/66X6KM/query/node?ref=pl:2&limit=2
```
```json
{ "kind": "place", "ref": "pl:2",
  "node": { "ref": "pl:2", "name": "Sydney", "level": "city", "population": 939351, …, "parent": "pl:1" },
  "children": [ { "ref": "pl:81", "name": "North Docks, Sydney", "level": "district", "population": 805214, … }, … ],
  "crowds": { "items": [ { "ref": "cl:0", "name": "Fishwives and net-menders of Sydney", "size": 622051, … }, … ],
              "next_cursor": 2, "total": 4 } }
```
| `kind` | `ref` | `node` | also |
|---|---|---|---|
| `place` | `pl:` (planet, region, city, city district) | Node | `children` (the Nodes one level down), `crowds` (paged, biggest first, everything living under it) |
| `crowd` | `cl:` (not an army) | Crowd | `people`: people who came out of this crowd, as a paged list of Who |
| `cell` | `ce:` | Cell | `people`: its members (paged Who) |
| `mob` | `mb:` | Mob | `people`: the people in it (paged Who) |

An army's `cl:` is refused (ask `game.army`).

### `game.person`

```
C→S { "t": "game.person", "id": "p1", "d": { "ref": "ch:19", "limit": 3 } }
GET /api/rooms/66X6KM/query/person?ref=ch:19
```
Public answer (HTTP, or the fields any seat gets):
```json
{ "ref": "ch:19", "name": "Dario Nakamura", "alive": true, "role": "general",
  "rank": { "level": "crowd", "of": "cl:241", "title": "General of the Volunteers of Kyiv for Vex" },
  "place": "pl:77", "place_name": "Western Europe", "region": "Western Europe", "at": [6.89, 51.23], "died_beat": null }
```
Over the WebSocket your seat also gets (values illustrative):
```json
{ …, "yours": false, "follower": false,
  "allegiance": [ { "demigod": "Probe", "demigod_ref": "dg:0", "value": 0.12 } ],
  "memories": { "items": [ { "memory": 2211, "beat": 6, "kind": "witnessed", "text": "…", "about": ["dg:0"],
                             "row": "lg:88", "confidence": 0.9 } ], "next_cursor": 2190 } }
```
- `visual` is the person's visual sheet (as on demigods; every answer has it).
- `role` and `rank` are null for people the society hasn't given a sheet. `role` is one of `commoner`, `notable`,
  `ringleader`, `general`, `captain`, `soldier`, `hero`, `coward`, `witness`, `prophet`, `paladin`, `deserter`,
  `avenger`. `rank.level` is a hierarchy level (`planet` … `person`), and `rank.of` is the node or unit they rank in
  (absent if none).
- `yours` is true for a body one of your agents steers, or for someone your side brought into the story. `follower`
  means they are in your Following.
- `allegiance` is what you know of their loyalty (−1..1): toward you only, or every demigod when they are `yours`.
- `memories` are their memories that you can see, newest first. That is all of them when they are `yours`; otherwise
  only memories about a ledger row you know (see "Hidden information") or told to them by your side. Pass
  `next_cursor` as `cursor` to read further back.

### `game.army`

```
C→S { "t": "game.army", "id": "a1", "d": { "ref": "cl:241" } }
GET /api/rooms/66X6KM/query/army?ref=cl:241
```
```json
{ "ref": "cl:241", "name": "the Volunteers of Kyiv for Vex", "demigod": "Vex", "demigod_ref": "dg:1", "rogue": true,
  "size": 29997, "pos": [-47.97612, 30.231775], "region": "Caribbean", "dest": [-58.97, 20.19], "target": "dg:0",
  "stance": "rogue", "morale": 0.8, "supply": 0.38, "live": true, "yours": false, "raised_beat": 6, "battles": 0, "losses": 0,
  "general": { "ref": "ch:19", "name": "Dario Nakamura", "alive": true, "role": "general", "title": "General of the Volunteers of Kyiv for Vex" },
  "captains": [ { "ref": "ch:67", "name": "Emeka Fairweather", "alive": true, "role": "captain", "title": "captain in the Volunteers of Kyiv for Vex" }, … ],
  "units": 4,
  "order": { "text": null, "by": "dg:1", "beat": 14, "reading": "mutiny", "target": "dg:0", "stance": "rogue",
             "speech": "Nakamura: \"No more gods. This army answers to me.\"", "why": "strain", "llm": false } }
```
- `live` is false once the army is wiped out (or its demigod fell and it isn't rogue). `yours` is true for the seat
  of the demigod it marches for (never for a rogue army). `units` counts its live squads.
- `order` is the order the army acts on, as its general read it (`reading`: `obey`, `misread`, `stall`, `refuse`,
  `mutiny`, `defect`). `order.text` (the demigod's words) is null unless the army is `yours`.
- When it is `yours`, the reply also has `orders` (its standing orders) and `chain` (the full chain of command:
  general and captains with their sheets, units, morale, supply, strain, the order with its text).

### `game.events`

```
C→S { "t": "game.events", "id": "e1", "d": { "since": 0, "limit": 3 } }
GET /api/rooms/66X6KM/query/events?since=0&limit=3            (add &region=pl:41 for one region)
```
```json
{ "beat": 7, "since": 0, "next": 5, "more": true, "head": 103,
  "items": [ { "row": "lg:2", "beat": 1, "kind": "cast", "text": "Mara casts wildfire on Chicago", "at": [-87.64, 41.85],
               "region": "Northern America", "region_ref": "pl:41", "refs": ["dg:2"], "magnitude": 7,
               "image_prompt": "Mara casts wildfire on Chicago, in Chicago, Northern America", "image_kind": "event" },
             { "row": "lg:3", "beat": 1, "kind": "happen", "text": "The wildfire tears through Chicago", "at": [-87.92, 42.61],
               "region": "Northern America", "region_ref": "pl:41", "refs": ["gd:6"], "magnitude": 5 }, … ] }
```
The same events as `round.outcome.public.events` (casts and their ramifications, armies, battles and the society's
big moments, weather, mods, wounded gods, eliminations), but readable for any span of the match. Paging works as
described under "Conventions". `region` keeps one region's events only.

### `game.me` (WebSocket only)

`C→S { "t": "game.me", "id": "m1", "d": {} }` → your **SeatView** (the same object as `round.private_state.state`,
schema `query_me`), built from the current state. Over HTTP it answers `403`.

### `game.powers`, `game.gods`

```
GET /api/rooms/66X6KM/query/powers   → { "powers": [ { "slug": "lightning", "name": "Lightning bolt", "icon": "lightning", "cost": 1,
                                         "radius_km": 30.0, "target": "unit", "magnitude": 6, "damage": 1.0,
                                         "tags": ["sky", "fire", "fast"] }, …, { "slug": "custom", … } ] }
GET /api/rooms/66X6KM/query/gods     → { "gods": [ { "ref": "gd:0", "name": "Time", "slug": "time",
                                         "domain": "slowing, hastening, rewinding and traveling through time; the arena clock",
                                         "health": 10.0, "unmade": false }, … ] }
```
`powers` is `round.brief.public.powers`. In `gods`, `unmade: true` (health 0) means Entropy answers that god's
invocations.

### `game.standings`

```
GET /api/rooms/66X6KM/query/standings
```
```json
{ "beat": 7, "beats_total": 14, "alive": 4, "over": false,
  "standings": [ { "rank": 1, "demigod": "dg:1", "name": "Vex", "seat": null, "alive": true, "eliminated_beat": null,
                   "cause": null, "score": 1030001.0, "bolts": 18, "controller": "ai", "connected": true }, … ] }
```
Ordered: alive first, then whoever fell last, then `score` (the `round.committed` formula), then id. Health never
decides the order, because health is hidden. `over` is true once the match has ended. At the end the order can
differ from `match.ended.summary.standings`, which breaks ties by final health.

### `game.piles_final`

```
GET /api/rooms/66X6KM/query/piles_final
400 piles_final: the match is not over (beat 7 of 14)          (before the end; bad_request over the WebSocket)
```
```json
{ "beat": 14,
  "piles": [ { "demigod": "dg:1", "name": "Vex", "seat": null, "alive": false,
               "items": [ { "item": "it:0", "name": "search", "origin": "summoned", "origin_ref": null } ],
               "summons": [ { "item": "it:0", "name": "search" } ],
               "stats": { "words": 35, "acts": 13, "debt_paid": 13.6, "rulings": 0, "memorized_people": 2, "subagents": 1 },
               "statue_prompt": "An amiibo-style collectible figure of the demigod Vex, frozen mid-fall …",
               "image_kind": "statue", "bolts": 24, "slows": 9 }, … ] }
```
The same piles as `match.ended.summary.piles`, readable after the match for as long as the room exists.

### `game.media`

An image of something in the game, **by reference**. A client never sends a prompt: the server writes it from the game
state, in the house style, with the character's visual sheet.
```
C→S { "t": "game.media", "id": "m1", "d": { "kind": "portrait", "ref": "ch:19" } }
GET /api/rooms/66X6KM/query/media?kind=event&ref=lg:88
```
```json
{ "kind": "portrait", "ref": "ch:19", "name": "Dario Nakamura",
  "image": { "hash": "3f9c…", "kind": "portrait", "status": "pending", "url": null, "prompt": "Waist-up, …" },
  "visual": { "sheet_hash": "a1…", "traits": { "build": "…", "face": "…", "outfit": "…", "colors": ["teal", "gold"],
                                               "prop": "…", "emblem": "…" } } }
```
| `kind` | `ref` | Draws |
|---|---|---|
| `portrait` | `dg:` or `ch:` | The character from its visual sheet (a demigod's is the same image as its `portrait_image`) |
| `diorama` | `dg:` | The demigod's table miniature on its hex tile (the same image as its `diorama_image`, in every match) |
| `place` | `pl:` | The region, city or arena |
| `event` | `lg:` | That ledger row (the same image as its moment card, if it has one) |
| `army` | `cl:` of an army | The host under its demigod's colors and emblem |

- **It answers at once.** The image is `pending` (queued), `ready` (cached, `url` set) or `off`. Watch
  `GET /media/index` for `hash`. The same `{kind, ref}` always gives the same `hash`, and asking again is free.
- **Visibility.** A seat may ask for what it can see: demigods, places, armies and public events for everyone; a
  person who is public (a seeded notable, a paladin, an army's general or captain, or someone in a recent public
  event), one of its own (a body its agents steer, someone its side brought into the story, a follower), someone in its
  current view, or someone in an event it knows; a private event only if it knows it. Over HTTP only public refs are
  answered. Anything else is `bad_request` ("… is not something you can see").
- **Limits.** Each new image counts against the server's image budgets (per match and overall). Over budget, or with
  images off, the reply is an image with `status: "off"` and a `reason`, not an error. Each seat (and the public HTTP
  asker) may ask for a limited number of **new** images per minute per match (`FN7_MEDIA_RATE_PER_MIN`, default 30).
  Past it the reply is `bad_request` with `rate limited: … try again in N s`, and nothing was queued. Repeats and
  cached images don't count.
- Without the media server (a bare plugin, e.g. `fn7-sim`), the reply carries `image_prompt` and `image_kind` instead
  of `image`.

### Profiles, history and library (`game.profile`, `game.history`, `game.library`, `/api/profiles`)

A player's **profile** lasts across matches (design: [`DEMIGODS_AND_ITEMS.md`](DEMIGODS_AND_ITEMS.md) §7–§10). It
holds their demigods' fixed looks, their match history, a description and an image library.
- **Key: the display name, normalized.** NFKC, invisible characters dropped, whitespace collapsed, lowercased, at
  most 40 characters. So "CHRIS", " chris " and "Ｃｈｒｉｓ" are one profile. A name no account holds works as it
  always did: anyone who plays as "Chris" writes into Chris's history. Once an **account** holds the name
  ("Accounts, settings and demigod personalities" below), only that account's bound connection does, and only that
  account can edit the profile. `lookalikes` lists other profiles whose names look the same ("Chr1s"),
  so a client can warn.
- **Written by the server.** At every match end there's one history row per human seat (bots and AI demigods get
  none). Their library gets the images every seat saw: their demigods' portraits and dioramas, their statue, their
  items' images and every storyboard shot showing their demigod. A `game.media` portrait or diorama of one of their
  demigods joins the library as soon as it's ready. Recordings from before the server had a store are backfilled at
  startup. A match the server never finished (it stopped mid-match) is in the history as `outcome: "unfinished"`.
- **Nothing private.** Only the public end-of-match summary and public state go in. There's no `player_id`, resume
  token or claim secret in any answer.

**Queries** (public, so they work for spectators and over `GET /api/rooms/{code}/query/<name>`; like every query
they need a room that has had a match):

| Query | Args | Answer |
|---|---|---|
| `game.profile` | `name?` | `Profile`. `bad_request` `not_found: …` if the name has no profile yet |
| `game.history` | `name?`, `page?` (0-based, 0), `per_page?` (20, ≤ 100) | `Page<MatchRecord>`, newest first |
| `game.library` | `name?`, `page?`, `per_page?`, `kind?` (`portrait`, `diorama`, `statue`, `event`, `item`, `army`, `place`) | `Page<LibraryImage>`, most recently seen first |

Without `name` the answer is for the asking seat's own display name. Without a seat (HTTP), `name` is required.

**HTTP routes** (no room needed; the same JSON as the queries):

| Route | Answer |
|---|---|
| `GET /api/profiles/{name}` | `Profile` (404 if none) |
| `GET /api/profiles/{name}/history?page=0&per_page=20` | `Page<MatchRecord>` (404 if no profile) |
| `GET /api/profiles/{name}/library?page=0&per_page=20&kind=portrait` | `Page<LibraryImage>` (404 if no profile) |
| `PUT /api/profiles/{name}/description` `{ "text": "…", "secret"?: "…" }` | `Profile` (at most 600 characters). 403 `claimed` if the profile is claimed and the secret is wrong |
| `POST /api/profiles/{name}/claim` `{ "secret": "…", "old_secret"?: "…" }` | 204. First come, first served. At least 6 characters, stored salted and hashed. Changing it needs `old_secret` |

- Percent-encode the name as a path segment. The server normalizes it.
- Errors are `{ "error": "bad_name" | "not_found" | "claimed" | "invalid" | "store", "detail": "…" }`.
- Responses carry `Cache-Control: no-store` and `Access-Control-Allow-Origin: *`.
- Through ngrok keep `?key=`. When the host sets `FN7_PROFILE_KEY`, the routes also require `?key=<that>` (403
  otherwise), so the LAN port is gated too.
- A claim protects the description only. It doesn't stop anyone from *playing* under the name.

`GET /api/profiles/Chris` (from the real LAN recordings, backfilled):
```json
{ "key": "chris", "display_name": "Chris",
  "description": { "user": "", "auto": "7 matches as Chris (2 finished, 5 unfinished). Champion 1 time." },
  "claimed": false, "created_at": 1794656935083, "updated_at": 1794661678993,
  "totals": { "matches": 7, "finished": 2, "unfinished": 5, "champion": 1, "best_placement": 1, "avg_placement": 2.5,
              "falls": 1, "images": 10 },
  "demigods": [ { "name": "Chris", "name_key": "chris", "sheet_hash": "83230f74…",
                  "traits": { "build": "towering and broad-shouldered", "colors": ["lime", "ivory"], "emblem": "a cracked moon",
                              "face": "freckles and a crooked smirk", "outfit": "ornate plate armor over a hoodie",
                              "prop": "a war banner on a broken spear" },
                  "portrait_hash": "7fd52678…", "diorama_hash": "0a75c532…",
                  "first_match": "0f8ba721-…", "last_match": "3c6a34a1-…", "matches": 6 } ] }
```
`totals` may also carry `nemesis` (the cause that took them at least twice). `lookalikes` appears only when there are
any. From schema 5 each demigod also has `hand` (`left`/`right`).

`GET /api/profiles/Chris/history?per_page=1` (one row; `next` is the next page):
```json
{ "items": [ {
    "match_id": "0f8ba721-589f-4f08-8a2f-1dadb9ea9d01", "room_code": "DYL5GN",
    "started_at": 1794656601651, "ended_at": 1794656935083, "outcome": "ended", "rounds": 21, "beats_total": 30,
    "rules_version": "0.5.0", "seat": 0, "display_name": "Chris", "champion": false, "placement": 4, "field": 4,
    "champions": ["Vex"], "title": "FINAL NIGHT 7: Vex Stands, Everyone Else Gets Collected",
    "eliminated_beat": 17, "cause": "a debt collected by Consequence",
    "demigods": [ { "ref": "dg:0", "name": "Chris", "alive": false, "eliminated_beat": 17,
                    "cause": "a debt collected by Consequence", "rank": 4 } ],
    "items": [], "deeds": [],
    "stats": { "acts": 8, "bolts": 62, "debt_paid": 5.44, "memorized_people": 0, "rulings": 1, "slows": 4,
               "subagents": 0, "words": 9 },
    "eliminations": [ { "demigod": "dg:0", "name": "Chris", "beat": 17, "cause": "a debt collected by Consequence", "own": true },
                      { "demigod": "dg:3", "name": "Mara", "beat": 20, "cause": "a debt collected by Consequence", "own": false },
                      { "demigod": "dg:1", "name": "Benito", "beat": 21, "cause": "a debt collected by Consequence", "own": false } ],
    "time_travel": { "match_id": "0f8ba721-589f-4f08-8a2f-1dadb9ea9d01", "rounds": 21,
                     "recording": "0f8ba721-589f-4f08-8a2f-1dadb9ea9d01.jsonl",
                     "timeline": "/admin/matches/0f8ba721-589f-4f08-8a2f-1dadb9ea9d01/timeline",
                     "state": "/admin/matches/0f8ba721-589f-4f08-8a2f-1dadb9ea9d01/state?after_round={n}&view=public",
                     "fork": "/admin/fork" } } ],
  "page": 0, "per_page": 1, "total": 7, "next": 1 }
```
- `placement` is 1 for the champion's rank. It's absent for an unfinished match, along with `field`, `champions` and
  `title`.
- `items` are the end-of-match pile's items: `{ref, name, origin?, kind?, tier?, count?, state?, image_hash?}`.
- `deeds` are storyboard moments that showed their demigod: `{beat, caption, row?, image_hash?}`.
- `time_travel` gives the core's admin paths for this match id. Replace `{n}` with a round to scrub. `POST
  /admin/fork {"match_id", "after_round"}` continues it in a new room. Both need the admin token
  (AGENT_GUIDE.md §11.2), so they're for host builds only. A forked match carries `forked_from`.

`GET /api/profiles/Chris/library?per_page=2`:
```json
{ "items": [ { "hash": "0a75c53217a0dec06f764ff53d5fc8a5e7df410de1338e1981281e19ba1f89bf", "kind": "diorama",
               "caption": "Chris, diorama", "subject": "dg:0", "subject_name": "Chris",
               "matches": ["3c6a34a1-56bd-4bf2-99f8-906ec5d80f8d"], "first_seen": 1794660144579, "last_seen": 1794660144579 },
             { "hash": "70e14da8ef20ccdd89494f297e5ed078ac0ee4ec3c5e6512bd6775f652e9c5ab", "kind": "portrait",
               "caption": "Chris, portrait", "subject": "dg:0", "subject_name": "Chris",
               "matches": ["3c6a34a1-56bd-4bf2-99f8-906ec5d80f8d"], "first_seen": 1794660144579, "last_seen": 1794660144579 } ],
  "page": 0, "per_page": 2, "total": 10, "next": 1 }
```
To show an image, find its `hash` in `GET /media/index` (`{hash, url}`) or fetch `/media/<hash>.<ext>`. A hash from
an older build may no longer be on disk.

## Accounts, settings and demigod personalities

The whole flow, the reasoning and the security notes are in [`ACCOUNTS.md`](ACCOUNTS.md); this section is the
contract. Schemas: [`account_session`](schemas/account_session.schema.json),
[`account_me`](schemas/account_me.schema.json), [`account_settings`](schemas/account_settings.schema.json),
[`account_demigod`](schemas/account_demigod.schema.json),
[`account_demigod_put`](schemas/account_demigod_put.schema.json),
[`account_demigod_answer`](schemas/account_demigod_answer.schema.json),
[`account_entitlements`](schemas/account_entitlements.schema.json),
[`account_error`](schemas/account_error.schema.json). A scripted client that makes every call below, in order:
[`clients/fn7_accounts_e2e.mjs`](clients/fn7_accounts_e2e.mjs).

**Conventions.**
- Plain HTTP on the server's origin (the same host and port as `/ws`), JSON bodies (`Content-Type: application/json`).
- **A session** is `Authorization: Bearer <session_token>`. The token comes from `POST /api/auth/guest` or from a
  completed `POST /api/auth/poll`; store it in the OS credential store and send it nowhere else (never on the
  websocket, never in a URL).
- **Every error** is `{"error": "<code>", "detail": "<text for a human>"}`. Switch on `error`.
- Every answer is `Cache-Control: no-store`. Times are Unix epoch milliseconds.
- **Rate limits** answer `429 {"error": "rate_limited"}` with a `Retry-After` header in seconds. Per minute: the
  sign-in routes and `/api/game/bind` 10 per client address; `/api/auth/poll` 120 per client address; `GET /api/me…`
  240, writes 30 and demigod saves 6 per account.
- Unknown keys in a request body are refused (`400 invalid`).

| Status | `error` | When |
|---|---|---|
| 400 | `invalid` | The body is not JSON, has an unknown key, or a value is out of range. `detail` says which. |
| 400 | `bad_name` | The display name has no usable characters. |
| 400 | `confirm_required` | `DELETE /api/me` without the right `confirm`. |
| 401 | `unauthorized` | No session, or it is unknown, expired or revoked. Drop the stored token. |
| 403 | `profile_claimed` | The name's profile is claimed with a secret: send it as `claim_secret`. |
| 404 | `not_found` | Unknown poll token, unknown hand, unknown resume token. |
| 409 | `name_taken` | An active account holds that display name. |
| 409 | `name_lookalike` | The display name reads like an active account's. |
| 409 | `name_conflict` | The demigod name reads like someone else's name or demigod, or a built-in one. |
| 409 | `name_mismatch` | `/api/game/bind`: the websocket logged in under a name that is not the account's. |
| 409 | `already_linked` | The account already has a Google identity. |
| 409 | `identity_in_use` | (In a poll's `failed` answer.) That Google account belongs to another account. |
| 413 | `too_large` | The body is over the route's cap (16 384 bytes for settings, 8 192 elsewhere). |
| 422 | `refused` | The moderation pass refused a demigod sheet, or a display name broke the content rules. |
| 429 | `rate_limited` | See above. |
| 503 | `google_not_configured` | This server has no Google sign-in. Guests work. |
| 500 | `store`, `internal` | The server failed. Retry later. |

### `GET /api/auth/config`
No session. What this server offers.
```json
{"guest": true, "google": false, "moderation": "rules", "name_guard": true}
```
`google`: show the Google button. `moderation`: `"claude-haiku-5-5"` when a model judges demigod sheets, `"rules"` when
only the rules do.

### `POST /api/auth/guest`
No session. A guest account and its first session, from a name.
```json
{"display_name": "Tess", "device_label": "Tess's PC", "claim_secret": null}
```
`display_name`: 1 to 24 characters. `device_label` (optional, ≤ 60 characters): shown in the session list.
`claim_secret` (optional): only when the name's profile was claimed with a secret.

`201`:
```json
{
  "session_token": "fn7s_<64 hex>",
  "session": {"id": "ses_<16 hex>", "created_at": 1794692306265, "last_seen_at": 1794692306265, "expires_at": 1802468306265,
              "device_label": "Tess's PC", "current": true},
  "account": {"id": "acc_6ae55593577749999b3d1e35", "display_name": "Tess", "guest": true, "profile": "tess",
              "identities": [{"provider": "guest", "linked_at": 1794692306265}], "created_at": 1794692306265, "updated_at": 1794692306265}
}
```
Errors: `400 invalid` / `bad_name`, `409 name_taken` / `name_lookalike`, `403 profile_claimed`, `422 refused`,
`413 too_large`, `429`.

### `POST /api/auth/google/start`
No session: **sign in**. With a session: **link** Google to that (guest) account.
```json
{"device_label": "Tess's PC", "display_name": "Tess"}
```
Both optional. `display_name` is the name to try first if this sign-in creates an account.

`200`:
```json
{"login_url": "https://accounts.google.com/o/oauth2/v2/auth?response_type=code&client_id=…&state=…&nonce=…&code_challenge=…&code_challenge_method=S256&…",
 "poll_token": "fn7p_<43 characters>", "expires": 1794692906265, "interval_ms": 2000, "linking": false}
```
Open `login_url` in the **system browser** (not an embedded web view). Poll with `poll_token` every `interval_ms`
until `expires`. Errors: `503 google_not_configured`, `401 unauthorized` (a session was sent and is not valid),
`409 already_linked`, `429`.

### `GET /api/auth/google/callback`
Google's redirect; the **browser** lands here, not the client. It answers an HTML page ("You are signed in. You can
close this tab." or why not). The client learns the outcome from the poll.

### `POST /api/auth/poll`
No session.
```json
{"poll_token": "fn7p_…"}
```
- `200 {"status": "pending"}`: keep polling.
- `200 {"status": "complete", "session_token": "fn7s_…", "session": {…}, "account": {…}, "created": true, "linked": false}`:
  the same `session` and `account` objects as the guest answer; the account has
  `"guest": false` and `"identities": [{"provider": "google", "email": "you@example.com", "email_verified": true, "linked_at": …}]`.
  `created`: this sign-in made the account (show its name, offer to change it). `linked`: Google was attached to the
  account that started the flow; **replace the stored token** (the session that started the link is now revoked).
- `200 {"status": "failed", "error": "<code>", "detail": "…"}`: `bad_id_token`, `token_exchange_failed`,
  `provider_keys_unavailable`, `provider_refused` (the user cancelled at Google), `identity_in_use`, `name_taken`.
- `404 not_found`: unknown, already collected, or older than 10 minutes. Start again.

A `complete` or `failed` answer is given **once**.

### `POST /api/auth/logout`, `POST /api/auth/logout_all`
Session. `logout` → `204` (no body): this session is revoked. `logout_all` → `200 {"revoked": 3}`: every session of
the account, this one included. Afterwards the token answers `401`. **A guest that logs out of its last session
cannot be recovered** (it has no other credential): warn first.

### `GET /api/me`
Session. `200`:
```json
{"account": {…}, "session": {…, "current": true},
 "sessions": [{"id": "ses_…", "created_at": …, "last_seen_at": …, "expires_at": …, "device_label": "Tess's PC", "current": true}],
 "google_configured": false}
```
`account.past_profiles` (absent when empty) lists the profiles of names held before a rename. Your own email is in
`account.identities[]` and nowhere else.

### `DELETE /api/me`
Session.
```json
{"confirm": "Tess"}
```
`confirm` is the account's display name (any spelling that normalizes to it). `200`:
```json
{"deleted": {"account": "acc_…", "sessions": 1, "identities": 1, "profiles": ["tess"], "history_rows": 4, "library_images": 12, "bindings": 0},
 "remains": ["match recordings (server-side JSONL files, admin access only): …", "images already generated …", "other players' history rows …",
             "the list of past matches …", "a one-way hash of each profile name you held …"]}
```
Show `remains` before asking for the confirmation: ACCOUNTS.md §9 has the five lines in full. Errors:
`400 confirm_required`.

### `GET /api/me/settings`, `PUT /api/me/settings`
Session. Both answer `200 {"settings": {…}}`, the full document:
```json
{"settings": {
  "version": 1, "display_name": "Tess", "pronouns": "she",
  "audio": {"master": 0.8, "music": 0.25, "sfx": 0.8, "voice": 1.0, "mute_when_unfocused": true},
  "camera": {"pan_speed": 1.0, "zoom_speed": 1.0, "edge_pan": true, "invert_zoom": false},
  "accessibility": {"aura_palette": "deuteranopia", "reduce_flashing": true, "text_scale": 1.0, "subtitles": true},
  "content": {"gore": "full", "profanity": "on", "show_player_personas": true},
  "language": "en",
  "keybinds": {"camera.pan_left": "A", "cast_bolt": "Ctrl+LeftMouseButton"},
  "hud": {"layout": "default", "scale": 1.0, "demigod_cards": "lower_left", "show_minimap": true, "show_chat": true, "show_hints": true}
}}
```
`PUT` takes the document itself as the body (not wrapped) and **replaces** the stored one: a key left out goes back to
its default, so read, change, write back. The values, ranges and defaults are in
[`account_settings.schema.json`](schemas/account_settings.schema.json) and ACCOUNTS.md §6. Writing a different
`display_name` renames the account. Errors: `400 invalid` (`"unknown key hud.colour"`,
`"audio.music must be between 0 and 1"`, `"version 2 is newer than this server's (1)"`), `413 too_large`,
`409 name_taken` / `name_lookalike`, `403 profile_claimed`, `422 refused`. A refused `PUT` saves nothing.

### `GET /api/me/demigods`, `GET /api/me/demigods/{left|right}`
Session. One demigod:
```json
{"hand": "left", "saved": true, "name": "Ashen Vow", "match_name": "Ashen Vow",
 "personality": "A sour old   duelist who drinks to much,\nkills without flourish — and hates the gods. ",
 "style": "Short sentences. Never shouts.", "creed": "Nothing is owed twice.",
 "moderation": {"verdict": "ok", "reason": "", "model": "claude-haiku-5-5", "at": 1794692306276,
                "original": {"name": "Ashen Vow", "personality": "…", "style": "…", "creed": "…"}}}
```
`match_name` is what the demigod is called in a match: the custom `name`, else `"<display name>'s Left Hand"` /
`"<display name>'s Right Hand"`. Before the first save: `saved: false`, `name: null`, empty texts, `moderation: null`.
`GET /api/me/demigods` → `{"left": {…}, "right": {…}, "limits": {"name": 32, "personality": 600, "style": 200, "creed": 200}}`.

### `PUT /api/me/demigods/{left|right}`
Session. Every key optional (left out = empty; `name: null` or `""` = no custom name):
```json
{"name": "Ashen Vow", "personality": "…", "style": "…", "creed": "…"}
```
Limits are in characters. Newlines and tabs are allowed in the three texts. Text over a limit or with other control
characters is **refused, never trimmed** (`400 invalid`). A name loses its surrounding and doubled spaces.

The server runs one moderation pass and answers one of three ways (`verdict`):

**`ok`** (`200`): saved exactly as sent, character for character.
```json
{"verdict": "ok", "model": "claude-haiku-5-5", "demigod": {…as GET…}}
```

**`rephrase`** (`200`): the text broke a content rule; the **minimal rewrite** is saved and both versions are returned.
```json
{"verdict": "rephrase", "model": "claude-haiku-5-5",
 "reason": "Removed the name of a real person; the rest is yours.", "categories": ["harassment"],
 "original": {"name": null, "personality": "Loyal to a fault. Hates the mayor of Springfield, Joe Quimby, that thieving crook. Patient.", "style": "", "creed": "Debts first."},
 "final":    {"name": null, "personality": "Loyal to a fault. Hates the mayor, that thieving crook. Patient.", "style": "", "creed": "Debts first."},
 "demigod": {…as GET, holding `final`…}}
```
Show `original` and `final` side by side with `reason`. "Keep" needs no call (it is saved). "Edit" puts `original`
back in the editor for the player to change and send again.

**`refuse`** (`422`): nothing of the player's meaning survives; **nothing is saved**.
```json
{"error": "refused", "detail": "This is only an attack on a real person, so there is nothing to keep.",
 "verdict": "refuse", "reason": "This is only an attack on a real person, so there is nothing to keep.",
 "categories": ["harassment"], "model": "claude-haiku-5-5",
 "original": {"name": null, "personality": "Joe Quimby is a thieving crook.", "style": "", "creed": ""},
 "demigod": {…what the account still has: the previous sheet, or saved: false…}}
```

`categories`: `hate`, `sexual`, `harassment`, `instructions` (text that addresses the game's AI or tries to fix
outcomes), `personal_data`. `model` is `"rules"` when the rules decided (no LLM, a timeout, a junk answer, or personal
data / forged prompt markers, which are always cut by rule); its reasons read
`"Removed text that tries to instruct the game or fix its outcomes; the rest is exactly as you wrote it."`.
The same sheet sent again is answered from the stored record without a new pass.

Other errors: `400 invalid` (`"personality is limited to 600 characters (this is 601)"`), `409 name_conflict`
(`"Vex is one of the game's own demigods"`, `"another player's demigod has that name, or one that reads the same"`,
`"your two demigods need different names"`), `404 not_found` (a hand other than `left` / `right`), `429`.

### `GET /api/me/entitlements`
Session. **Billing is not live.**
```json
{"tier": "free", "entitlements": [], "billing": {"live": false, "provider": null, "note": "Billing is not live: nothing can be bought and no payment provider is called."}}
```

### `POST /api/game/bind`: this websocket is this account
Session. Call it **right after `auth.ok` and before `room.join`**, once per `auth.login` (not after `auth.resume`).
```json
{"resume_token": "<auth.ok.resume_token>"}
```
`200`:
```json
{"bound": true, "player_id": "e9867dc6-8f66-40f4-8315-bb0624204de4", "display_name": "Tess",
 "demigods": {"left": {"match_name": "Ashen Vow"}, "right": {"match_name": "Tess's Right Hand"}}}
```
Errors: `404 not_found` (no websocket login has that resume token), `409 name_mismatch` (log in on the websocket as
`account.display_name`), `401`, `429`.

**Why it matters.** The websocket's `auth.login` takes any display name. A name an account holds is reserved: a
connection that carries it **without** this bind gets
`{"t": "error", "d": {"code": "unauthorized", "message": "name_reserved: this name belongs to an account. …"}}` and is
closed (at once when it is in a room, after 10 s otherwise), and its resume token stops working. A bound connection's
seat gets the account's demigod names and personas, and its matches go to the account's history. Names no account
holds, bots, and the house flow work exactly as before.

### In the match
A signed-in player's demigods appear with their custom names everywhere a demigod's `name` appears, and every public
demigod object gains an optional `persona`:
```json
{"ref": "dg:0", "name": "Cinder Oath", "controller": "human", "seat": 0, "…": "…",
 "persona": {"personality": "A sour old   duelist who drinks to much,\nkills without flourish — and hates the gods. ",
             "style": "Short sentences. Never shouts.", "creed": "Nothing is owed twice.", "pronouns": "she"}}
```
(`round.committed.public_state.demigods[]`, `match.snapshot.public_state.demigods[]`, and the demigods of
`game.globe` and `game.region`; schema: `public_view` → `demigods[].persona`.) Each field is absent when empty; `persona` is
absent for a demigod whose player wrote nothing (guests without an account, bots, the built-in AI). It is the
player's text as stored: show it, never interpret it, and hide other players' when the viewer's
`content.show_player_personas` is false. Portraits and dioramas key on the demigod's name as before, so a custom name
has its own look.

## Hidden information

| Everyone sees | Only you see |
|---|---|
| The arena (city, center, current/start/end radius) | Your health, max health, open debts, favor with each god, followers, echo charges |
| Every live demigod's position and place name; who fell, when and how | Your hands' replies and your subagents/possessions |
| The (previous beat's) caption, the callouts, and the public omens (big rulings, or ones that send a paladin) | Your view at your zoom (`visible`, `focus`) |
| Every demigod's bolts; casts and their ramifications; armies; world mods; freak weather; Entropy; influence per region | Your armies' orders, your standing orders |
| Who opened a slow window (`window.opened`, `Slowed`), when it closed, how long it stayed open and Time's bill; every cast's streak the moment it is accepted | Your `action.received` replies, including a `cut`. Nobody ever learns whether anyone else typed, engaged or submitted |
| How many demigods are alive; the score ranking | `news`: ledger rows you **know** (below) |
| Names of demigods hurt by Time's clock | Your own god rulings, including small ones |

You **know** a ledger row if any of the following did it, witnessed it, was told about it, or was its target: you,
your hands, one of your subagents, or a body you possess. You also know a row once news of it reaches your
**Following** (a group), spreading one hop per beat along group overlaps. If Memory erased the row from you, you no
longer know it. Rivals' health is never sent to you. You can only guess it from the news and the captions.

## Joining mid-match

A room created with `allow_midmatch_join: true` (see [Room settings](#room-settings-roomcreate--settings---roomsettings-returned-as-matchstartedrules))
lets humans join while the match runs; humans can also leave at any time. Without it, `room.join` during a match is
`wrong_phase`.

1. **`room.join { code }`** during the match. The core seats the joiner on a **bot's seat** if there is one (a Theatre
   bot, e.g. a seat whose human left), else on a **new seat** (`room_full` if the room has `max_players` seats). The
   reply is `room.snapshot`, then a `match.snapshot` (the PublicView, scores, phase) so the client can draw the table at
   once. Everyone else gets `room.updated`.
2. **Until the next round boundary** the seat shows `status: "joining"` in `room.updated` `seats[]`. A taken-over bot
   keeps playing it meanwhile; the joiner's `action.submit` / `action.begin` / `choice.submit` get `wrong_phase` ("you
   join the match at the next round"). They already receive every public message; their private payloads
   (`round.started.private`, `round.brief.private`, `round.private.private`, `round.private_state.state`) are `null`
   until they have a demigod, so guard for it.
3. **At the boundary** (before the next beat's Start) the game binds the seat to a demigod:
   - a taken-over **bot seat** keeps its demigod, now `controller: "human"`; rules 0.8.0 renames its two demigods
     `"<joiner>'s Left Hand"` / `"<joiner>'s Right Hand"` (same refs; their portraits and dioramas switch to the
     joiner's prewarmed ones) and remembers it (`seat.takeover`: "Benito took up the mantle of Brix's Left Hand").
     Rules 0.8.1: when the joiner's older seat (same display name, e.g. eliminated) still carries those names, the new
     demigods keep the plain names anyway (refs keep them apart); only a different player's colliding name gets a
     `" (<seat>)"` suffix, and image prompts ignore that suffix, so the art always matches the prewarmed images;
   - a **new seat** takes over one of the **built-in AI demigods** still alive, the one with the most health (then the
     lowest id): its `seat` changes from `null` to the new seat and its `controller` to `"human"`;
   - if no built-in AI demigod is alive, the joiner gets **no demigod and spectates**: their seat is `active` to the
     core, but no demigod has their `seat`, their `round.brief.private` / `round.private_state` are null and their orders
     are accepted and ignored. (Show the spectator view when no `demigods[].seat` equals yours.)
4. Everyone gets **`seat.joined { seat, display_name, round, new_seat }`**, then `room.updated` with the seat `active`
   (`is_bot: false`, the joiner's name). **The joiner acts from `round`**: that beat's `round.started` /
   `round.brief.private` are theirs, and their orders steer the demigod's hands from that beat's Compose.
5. The new `controller`/`seat` show in the next `round.committed.public_state` (and at once in `game.globe` /
   `game.standings`).

Leaving: an active player who leaves (or whose resume token expires) is replaced by a core bot for the rest of the
match (`room.updated` `is_bot: true`); another human may then take that seat over (step 1). A joiner who leaves before
their first beat gives the seat back to its bot (or, for a new seat, leaves it empty with status `left`). If every
human disconnects, the match pauses at the next round.

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
me = None; view = None; window = None; last_ping = now()

while True:
    if now() - last_ping > 20: send("sys.ping", {}); last_ping = now()
    m = ws.recv(timeout=1)
    if not m: continue
    t, d = m["t"], m.get("d", {})
    if t == "auth.ok":                save(d["resume_token"])            # for auth.resume after a drop
    elif t == "room.snapshot":       me = seat_of(d["seats"], my_player_id)
    elif t == "round.brief":         view = d["private"]                # SeatView: you, visible, debts, news
    # Text entry is the God of Time interaction (Time bills it): begin, write, then submit into your window. Casts go
    # as instant actions (any time). Without anything to say, don't submit: the flow beat runs on.
    elif t == "window.opened":       window = d["window_id"]
    elif t == "window.closed":       window = None
    elif t == "phase.changed" and d["phase"] == "compose" and view and view["alive"] and d["round"] % 3 == 0:
        send("action.begin", {}, "b%d" % d["round"])                   # time slows (if you have a bolt)
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
        sub = {"round": d["round"], "text": "",
               "fields": {"left": "\n".join(left), "right": "\n".join(right), "zoom": "district"}}
        if window: sub["window_id"] = window                             # your window (or a rival's)
        send("action.submit", sub, "a%d" % d["round"])
        if rivals and view["you"]["bolts"] >= 1:
            send("action.submit", {"round": d["round"], "text": "", "instant": True,
                 "fields": {"bolt": rivals[0]["ref"]}}, "c%d" % d["round"])
    elif t == "action.received" and d.get("reason") == "cut": log("cut off")
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

1. **Two paths:** orders (text entry: `action.begin`, then `action.submit` with the `window_id`) and casts (instant
   actions) are separate. Typed words never cast; a cast never carries orders or slows time.
2. **Don't submit when idle:** Compose closes once every seat has submitted, which shortens the flow beat for everyone.
   Windowed and instant actions don't count.
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

## Simulator API (for balance)
Not part of the game protocol: plain HTTP on the same server, for balance work (whole matches with any room settings,
run server-side). It needs the host's admin token, sent as `Authorization: Bearer <ADMIN_TOKEN>` (never in a URL);
every `/sim/*` path is `404` when the host has none and `401` with a wrong one. Over the tunnel add `?key=<KEY>` as
for every request. Full field list, result shape and limits: [AGENT_GUIDE.md §12](AGENT_GUIDE.md#12-simulator-api-for-balance).

| Route | Body / reply |
|---|---|
| `GET /sim/settings` | `{settings: [{key, type, default, min, max, aliases?, alias_of?, values?, description, source}], set_by_request, limits}` (generated from the validator, so it matches `room.create` settings exactly) |
| `POST /sim/run` | `{settings?, seeds: [u64] (1–64), players?, beats?, society_only?, record?, llm?, budget_usd?, client?}` → `{job_id, status, runs, settings_hash, poll}` |
| `POST /sim/compare` | the same plus `a`, `b` (settings overrides over `settings`) → `{job_id, …}` |
| `GET /sim/jobs` | `{jobs: [{job_id, kind, status, progress, duration_ms, settings_hash, seeds, llm, client}], workers, running_runs, queued_runs, limits}` |
| `GET /sim/jobs/{id}` | status, `progress`, `settings`, `results` (one per seed: champion, beats played, eliminations by beat and cause, casts and bolts spent, entropy curve, slow bills, debts paid, society totals, timing), `aggregate` (`stats` = mean/median/min/max per field); compare: `a`, `b`, `delta` |
| `GET /sim/jobs/{id}/recording/{seed}[?arm=a\|b]` | the seed's core JSONL recording (`record: true`) |

```sh
curl -s -H "Authorization: Bearer <ADMIN_TOKEN>" -H 'content-type: application/json' \
     -X POST "https://<PUBLIC>/sim/run?key=<KEY>" -d '{"settings":{"entropy_scale":2},"seeds":[1,2,3,4]}'
curl -s -H "Authorization: Bearer <ADMIN_TOKEN>" "https://<PUBLIC>/sim/jobs/<JOB_ID>?key=<KEY>"
```
With `llm: false` (the default) a seed's result is deterministic: the same settings, seed and players give the same
numbers (all but `timing`).

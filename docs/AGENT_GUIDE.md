# Final Night 7: start here (for agents building a client or frontend)

You are building a client for **Final Night 7**, a real-time battle royale of demigods across the whole planet, played
through LLM "hand" orchestrators and god powers. The game server is already running. This guide and `CLIENT_API.md`
are the **data contract**: what the server sends and accepts, when, and what every field means. How anything is
presented is up to the client's developers; nothing here prescribes UI. The server decides everything; a client never
simulates game rules.

Follow this file top to bottom. Every command here was run against the live server before this guide was written.

---

## 0. Two ways in: internet (ngrok) or the same Wi-Fi

The host gave you a **public URL** and an **access key**. Every request over the internet must carry `?key=<KEY>`; without it
you get `403`. Substitute these everywhere below:

| | Over the internet (anywhere) | On the host's Wi-Fi |
|---|---|---|
| Game WebSocket | `wss://<PUBLIC>/ws?key=<KEY>` | `ws://<LAN_HOST>:8080/ws` |
| Health | `https://<PUBLIC>/healthz?key=<KEY>` | `http://<LAN_HOST>:8080/healthz` |
| Docs (this folder) | `https://<PUBLIC>/docs/<file>?key=<KEY>` | `http://<LAN_HOST>:8081/<file>` |

`<PUBLIC>` looks like `something.ngrok-free.dev`. Note `wss://` (TLS) over the internet, `ws://` on the LAN.

**Fetch all docs over the internet:**
```sh
P=https://<PUBLIC>; K=<KEY>
mkdir fn7-docs && cd fn7-docs
for f in AGENT_GUIDE.md CLIENT_API.md MEDIA.md theatre/PROTOCOL.md data/pantheon.json data/powers.json clients/fn7_client.mjs; do
  mkdir -p "$(dirname "$f")"; curl -fsS "$P/docs/$f?key=$K" -o "$f"; done
for d in schemas examples; do mkdir -p $d
  curl -fsS "$P/docs/$d/?key=$K" | grep -o 'href="[^"]*\.json"' | cut -d'"' -f2 |
  while read f; do curl -fsS "$P/docs/$d/$f?key=$K" -o "$d/$f"; done; done
node clients/fn7_client.mjs --url "wss://${P#https://}/ws?key=$K" --name AgentTest --create --auto --beats 3 --beat-ms 8000
```
Browsers: a free ngrok domain shows an interstitial page to browsers. Send the header `ngrok-skip-browser-warning: 1`
on HTTP requests from a browser. WebSocket clients, curl and Unreal aren't affected. Treat the key like a password: it
lets anyone spend the host's LLM budget.

## 1. Where things are

| What | Where |
|---|---|
| Game server (WebSocket) | `ws://<LAN_HOST>:8080/ws` (LAN playtest host) |
| Health / stats | `http://<LAN_HOST>:8080/healthz` → `ok`, `http://<LAN_HOST>:8080/stats` |
| These docs, served over HTTP | `http://<LAN_HOST>:8081/` (browse or `curl`; same files as `final_night_7/docs/`) |
| Engine protocol (envelope, login, lobby) | `theatre/PROTOCOL.md` (here: [`theatre/PROTOCOL.md`](theatre/PROTOCOL.md); also in the `Aftersource/theatre` repo) |
| Game payloads (every Final Night 7 object) | [`CLIENT_API.md`](CLIENT_API.md) |
| JSON Schemas (draft 2020-12), one per payload and one per query reply (`query_<name>`) | [`schemas/`](schemas/) |
| One real recorded frame of every message | [`examples/`](examples/) |
| The seven gods (ids, names, keywords) | [`data/pantheon.json`](data/pantheon.json) |
| The power palette (casts: slugs, costs, radii) | [`data/powers.json`](data/powers.json) (also sent every beat in `round.brief.public.powers`) |
| Images rendered from the payloads' prompts | [`MEDIA.md`](MEDIA.md) |
| A working reference client (Node ≥ 22, no deps) | [`clients/fn7_client.mjs`](clients/fn7_client.mjs) |
| Game design (background, optional) | `final_night_7/DESIGN.md` on the host |

The host IP is the host's current Wi-Fi address. If nothing answers, ask the host for the current one (§9).

**Fetch everything at once** (on your machine):
```sh
mkdir fn7-docs && cd fn7-docs
for f in AGENT_GUIDE.md CLIENT_API.md MEDIA.md theatre/PROTOCOL.md data/pantheon.json data/powers.json clients/fn7_client.mjs; do
  mkdir -p "$(dirname "$f")"; curl -fsS "http://<LAN_HOST>:8081/$f" -o "$f"; done
for d in schemas examples; do mkdir -p $d
  curl -fsS "http://<LAN_HOST>:8081/$d/" | grep -o 'href="[^"]*\.json"' | cut -d'"' -f2 |
  while read f; do curl -fsS "http://<LAN_HOST>:8081/$d/$f" -o "$d/$f"; done; done
```

## 2. Read in this order
1. This file.
2. `theatre/PROTOCOL.md`, sections **Envelope**, **sys**, **auth**, **room**, **match**. About 10 minutes. This is the
   generic multiplayer protocol (`{t, id, re, d}` envelopes, `auth.login`, rooms, phases).
3. `CLIENT_API.md` completely. It defines every game object inside those messages, the action format, settings,
   timing and hidden information.
4. Skim `examples/round.brief.json`, `round.outcome.json`, `round.private.json`, `round.committed.json` and
   `match.ended.json` to see real data.
5. Use `schemas/` to generate types or validate. Don't hand-write types from prose.

## 3. Prove the connection (5 minutes)
```sh
curl -s http://<LAN_HOST>:8080/healthz          # ok
curl -s http://<LAN_HOST>:8080/stats            # {"game":"final_night_7",...}
node --version                                  # must be >= 22 (built-in WebSocket)
node clients/fn7_client.mjs --url ws://<LAN_HOST>:8080/ws --name AgentTest --create --auto --beats 3 --beat-ms 8000 --log frames.jsonl
```
That last command creates a private room, starts a short match against the built-in AI demigods, plays it with
scripted orders every third beat and a lightning bolt now and then, prints each beat's caption (one beat late), slow
windows, casts, world events, Entropy, your hands' replies, omens and news, and writes every frame (both directions)
to `frames.jsonl`. Expected output, abridged (a real run against a private offline server):
```
=== beat 6  arena 1500 km  alive 2  YOU ARE OUTSIDE THE ARENA
you: health 3.2  bolts 21  debt 0.1  zoom district  entropy 6.8 (calm)
>> [beat 5] Beat 5: Time's clock catches Mara outside the arena, and 5 more things went sideways.
   ⧗ time slows (window 6, seat 2, until 14:02:31)
   ⧗ Vex slowed time
   ⚡ Vex casts Flood at Dhaka (Southern Asia) [seq 41]
   ⧗ time snaps back (window 6: submitted, seat 2)
   ⧗ Time snaps back: Vex's window closed (enter, 4200 ms, debt 0.71)
   ⚡ Vex casts Flood on Dhaka (Southern Asia)
   ◦ The flood tears through Dhaka [Southern Asia]
   ◦ Crowds in Dhaka run screaming from the flood [Southern Asia]
   entropy 8.9/100 (calm)
   Entropy: "MORE POWER. That's the whole note. More. Power. Ahahhaahhaaa!"
MATCH OVER. champion: AgentTest (seat 0)
```
A flow beat nobody spoke or cast in resolves in about a millisecond; a beat with orders or casts takes 2–5 s of LLM
time on the host. **Read `frames.jsonl`**: it is the ground truth of what your client must handle, in order.

**Two humans in one match:** one runs `--create` (prints a room code like `VWRTNV`), the other runs
`--join VWRTNV`; drop `--auto` to play by typing. The creator types `/start` once both are in. Text entry: `/slow`
(sends `action.begin`: slow time; typing a first order line sends it too), `l: …` (left hand), `r: …` (right hand), `z: city` / `z: character
ch:12` (zoom), an empty line is Enter (sends your orders, with the open window's `window_id`). Casts (instant actions, any time):
`b: dg:2` (a lightning bolt at a target), `c: volcano 3.38,6.52` (a power from the palette at a ref, a `lon,lat` or a
name). `--script FILE` types `<delay_ms> <line>` lines for you (scripted tests).

## 4. The loop
```
connect → server.hello (check game == "final_night_7")
→ auth.login {display_name} → auth.ok (store resume_token!)
→ room.create {visibility:"private", settings:{beats_total, flow_ms}} | room.join {code} | room.quickjoin
→ room.ready {ready:true} → (host) room.start → room.countdown → match.started {rules}
per beat (round == beat; every beat is a flow beat of flow_ms):
  phase start   → round.started   {public: arena/eliminations, private: your health / outside_arena / debt_due}
  phase brief   → round.brief     {public: arena, gods, powers, entropy, flow (the open slow window), regions; private: SeatView}
  phase compose → (deadline = epoch ms) → action.submit {round, text:"", fields:{left, right, zoom}} when you have
                    orders and no window is open
  any phase     → text entry: action.begin {} when typing starts → window.opened {window_id, by_seat, deadline}
                    (everyone) + phase.event {public: Slowed}; action.submit {round, text:"", fields, window_id} on
                    Enter → action.received {accepted, window_id} | {accepted:false, reason:"cut"} (only you);
                    window.closed {window_id, by_seat?, reason}
                → casts: action.submit {round, text:"", fields:{cast | casts | bolt | bolts}, instant:true}
                    → action.received {accepted, reason?, instant:true}; phase.event {public: Streak} (everyone)
                → seat.revived {seat, round, reason} (an eliminated seat is back; then room.updated)
  phase resolve → (phase.progress every ≤5 s; milliseconds when nobody spoke or cast)
  phase reveal  → round.outcome   {public: previous_caption, omens, entropy, slow (open/closed/instant), casts, events, timing}
                → round.private   {private: SeatDelta = hands' replies, subagents, your news, your omens}
  phase commit  → round.committed {scores, public_state: PublicView (+ bolts, influence, armies, mods, entropy, slow, gods)}
                → round.private_state {state: SeatView (+ armies, orders, slow)}
→ match.ended {winners, summary: champions, standings, piles (+ statue_prompt), entropy, heat_death, final cut}
always: sys.ping every ~20 s; on disconnect reconnect with auth.resume {resume_token} → match.snapshot
```
Rules that bite:
- **Submit only when you have something.** Compose closes as soon as every active seat has submitted a Compose action,
  so an idle submission shortens everyone's flow beat (windowed and instant actions don't count). A Compose submit
  after Compose closed, with no window open, gets `wrong_phase`.
- **Text entry is fast or expensive.** Every message with orders is a Time interaction: a bolt to Time and a debt that
  grows with every real second between `window.opened` and the server's receipt of your Enter (orders without a window
  are billed as zero seconds). The opener's Enter closes the window and cuts off everyone who hadn't submitted (the
  private `cut` reply). Send the `window_id` from `window.opened` with orders typed during a window. Arrival order is
  stamped by the server; there is no client timestamp. The v0 fields `slow` / `sent_at` / `window` are refused.
- **Casts and orders are separate.** Typed words never cast; a cast never carries orders or slows time. Casts are
  instant actions (2/s, burst 4 per seat), checked against your bolts at the last commit and paid at the next resolve
  in arrival order (an overspent one fizzles).
- `text` is capped at 500 chars by the engine; each hand field allows 2000. Use `fields`.
- `round` equals `beat`. There is never a Choose phase.
- Ignore unknown keys; new optional fields appear over time.

## 5. Where each piece of data is

| Data | Source |
|---|---|
| The arena (Earth or a city), the convergence city, the shrinking radius | `round.brief.public.arena` (`kind`, `name`, `city`, `center`, `radius_km`, `start_radius_km`, `end_radius_km`), `PublicView.arena` |
| Regions and their cities (static for the match) | `round.brief.public.regions` (`[{ref, name, center, radius_km, cities: [{ref, name, center}]}]`) |
| Demigods' positions, regions, bolts, portraits | `PublicView.demigods[]` (`pos` is `[lon, lat]`, `region`, `bolts`, `portrait_prompt`) |
| Each demigod's share of each region | `PublicView.influence` (`[{region, region_ref, demigod, demigod_ref, share}]`) |
| Armies (positions, sizes, where they're headed) | `PublicView.armies`; yours with orders in `SeatView.armies` |
| World mods (lasting changes on the globe) | `PublicView.mods` (`at`, `radius_km`, `stacks`, `expires_beat`) |
| The Entropy meter | `round.brief.public.entropy`, `round.outcome.public.entropy` (with `delta`), `PublicView.entropy`; `match.ended.summary.heat_death` |
| The slow window, live: who opened it, its id and deadline; when it closed and why | `window.opened`, `window.closed`, `phase.event` `Slowed`; on resume `match.snapshot.window` |
| The slow window as the game recorded it; Time's bill | `round.brief.public.flow.slow`, `PublicView.slow`, `SeatView.slow` (`yours`, `window_id`); `round.outcome.public.slow` (`open` / `closed` with `how`, `after_ms`, `debt` / `instant`) |
| Whether your orders were cut off | `action.received { accepted: false, reason: "cut" }` (only to you) |
| A cast's streak, the moment it is accepted | `phase.event` `Streak` (`by`, `power`, `icon`, `at`, `region`, `target`, `seq`, `casts[]`) |
| Revivals | `seat.revived { seat, round, reason }`, then `room.updated` (status `active`) |
| The power palette (slug, icon key, cost, radius, target kind) | `round.brief.public.powers` |
| Casts settled this beat (power, icon key, cost, point, region, target, `seq`, `instant`, fizzled) | `round.outcome.public.casts` |
| Geotagged world events (casts, ramifications, armies, weather, mods, eliminations; `image_prompt` on the biggest) | `round.outcome.public.events` |
| Your health, bolts, debts, favor, followers | `SeatView.you` (`bolts`), `SeatView.max_health`, `SeatView.debts[]` |
| Your hands' standing orders | `SeatView.orders` |
| Zoom (world / region / city / character) and what is in view | `SeatView.zoom`, `SeatView.focus`, `SeatView.visible[]` |
| Hands' replies, subagents | `round.private.private.hands[]`, `SeatView.hands[]` |
| News you know (geotagged) | `round.private.private.news[]` (new this beat), `SeatView.news[]`; each `{row, text, refs, how, at, region}` |
| Narration (one beat late) | `round.outcome.public.previous_caption` / `previous_callouts` / `caption_beat`; the last in `match.ended.summary.last_caption` |
| God speech (geotagged) | `round.outcome.public.omens[]`, `round.private.private.omens[]`: `{god, god_ref, speech, at, region}` |
| The gods' health | `PublicView.gods` |
| Eliminations | `seat.eliminated`, `round.outcome.public.eliminated`, `SeatView.alive == false` |
| End of match | `match.ended.summary` (`champion_seat`, `standings`, `piles[]` with `statue_prompt`, `entropy`, `heat_death`) |
| Rendered images for the prompts | the media server (`MEDIA.md`): `portrait_image`, `image`, `statue_image` next to the prompts |

Positions are `[longitude, latitude]` degrees everywhere.

### 5.1 Game queries (pull instead of waiting for a push)
Everything above arrives pushed, once per beat. Queries let you ask for it at any time (CLIENT_API.md "Game queries";
reply schemas `schemas/query_<name>.schema.json`; the manifest is `server.hello.api.queries`):
- WebSocket, from your seat: `{ "t": "game.<name>", "id": "q1", "d": { …args } }` → `{ "t": "game.<name>", "re": "q1",
  "d": … }` or `error` (`bad_request` with the reason, `not_found`, `wrong_phase` before the first match).
- HTTP, public queries only, no socket needed (public data only):
  `curl "http://<LAN_HOST>:8080/api/rooms/<CODE>/query/globe?limit=30"` (args are query-string parameters; through
  ngrok keep the `?key=` gate parameter, it is ignored). `403` = a seat-only query (`me`), `404` = no such room.

| Need | Query |
|---|---|
| The globe in one poll (arena, demigods, influence, mods, armies, Entropy, beat, recent events) | `game.globe {since?, limit?}` |
| One region / one node of the hierarchy (city, district, crowd, cell, mob) | `game.region {ref, cursor?, limit?}`, `game.node {ref, cursor?, limit?}` |
| A person, an army | `game.person {ref}`, `game.army {ref}` (your seat also gets what only you know: allegiance and memories, your army's orders) |
| Every public event since you last looked | `game.events {since, limit?, region?}`: pass back `next` as `since` |
| Your seat view now | `game.me {}` (WebSocket only) |
| The palette, the pantheon, the ranking, the end piles | `game.powers`, `game.gods`, `game.standings`, `game.piles_final` (an error until the match is over) |

Lists that grow come back paged: `{items, next_cursor, total}`; pass `next_cursor` as `cursor`. Answers reflect the
current state; ask again after `round.committed` for the new beat. Each query takes about a millisecond on the
server, but it runs on the room's loop, so poll at the beat rate, not in a tight loop.

### 5.2 Profiles, match history and the image library (persist across matches)
A player's profile is keyed by their **display name** (normalized: "CHRIS", " chris " and "Ｃｈｒｉｓ" are one
profile). There are no accounts, so anyone typing the same name shares it. The server writes one history row per
human seat at every match end and adds the player's demigod portraits, dioramas, statues and moment cards to their
library. Full shapes: CLIENT_API.md "Profiles, history and library".
- **HTTP, any time (no room needed):** `GET /api/profiles/<name>`, `/api/profiles/<name>/history?page=0&per_page=20`,
  `/api/profiles/<name>/library?kind=portrait`. Percent-encode the name. Through ngrok keep `?key=`. `404` = no
  profile yet (they haven't finished or started a match on this server).
- **In a room:** `game.profile`, `game.history`, `game.library` with `{name?, page?, per_page?, kind?}`. Without
  `name` they answer for your own seat. Same JSON as HTTP.
- **Images:** a library entry is a media `hash`. Look it up in `GET /media/index` (`{hash, url}`) or fetch
  `/media/<hash>.<ext>`.
- **Time travel:** each history row has `time_travel` with the match id and the admin scrub and fork paths (§11.2).
  They need the admin token, so they're for host builds only.
- **Editing:** `PUT /api/profiles/<name>/description {text, secret?}`. `POST /api/profiles/<name>/claim {secret}`
  locks the description to a secret (6+ characters). Nothing private is ever served: no `player_id`, no resume token,
  no secret.

## 6. Developing without the server
- **Fixtures:** `examples/*.json` are real frames. Feed them into your router in order (§4) to build UI offline.
- **Your own recordings:** run `fn7_client.mjs --log frames.jsonl` once, then replay that JSONL into your client with
  the original timing (`at` is ms epoch). That's a full match, including resolve delays.
- **Validation:** check every inbound payload against `schemas/` in debug builds and log mismatches. The schemas are
  generated from the server code, so a mismatch is a real bug on one side.

## 7. Testing against the server
- **Bots fill seats:** a room with one human still has 4 demigods (`players`, default 4); the server's built-in AI plays
  the rest. Set `players` up to 8.
- **Fast iteration:** `settings: {beats_total: 10, flow_ms: 3000}`. With no LLM cost and instant beats, add
  `llm: false` (rules-only; captions become templates). `entropy_scale: 4` reaches the Entropy levels (and heat death)
  in a short match; `start_bolts` gives everyone more power; `arena: "city"` plays one city.
- **Several clients:** run `fn7_client.mjs --auto --join CODE` in extra terminals to add scripted players to your room.

## 8. Etiquette (the host pays for the LLM)
- A beat with orders or casts costs the host about a tenth of a cent; quiet flow beats cost nothing. A 30-beat
  4-demigod match measured about $0.03. Use `llm: false` for
  layout work and short matches (`beats_total` 3–10) for flow testing.
- Don't open rooms in a loop or leave `--auto` clients running unattended.
- Every match is recorded on the host (every frame, every prompt). Tell the host the room code if you want a match
  looked at.

## 9. Troubleshooting
| Symptom | Fix |
|---|---|
| `curl` to `:8080` times out | Not on the same Wi-Fi, guest-network isolation, the host's Mac is asleep, or the host IP changed. Ask the host for `ipconfig getifaddr en1`. |
| `server.hello.game` isn't `final_night_7` | Wrong server or port. |
| Socket closes after ~60 s | You're not sending `sys.ping` every ~20 s. |
| `error {code: "wrong_phase"}` on submit | Compose already closed (someone submitted last) and no window is open, you're eliminated, or the `round` is stale. |
| `action.received {accepted: false, reason: "cut"}` | You submitted to a window that had already closed: the opener pressed Enter first. Your orders didn't go through. |
| `error {code: "rate_limited"}` on a cast | More than 2 instant actions a second (burst 4). |
| `error {code: "too_long"}` | `text` > 500 chars. Put speech in `fields.left/right` (≤ 2000). |
| `error {code: "bad_request"}` | Read `message`. Usually a bad `zoom` level or a hand field that's not a string. |
| Nothing happens after `room.ready` | Someone must send `room.start` (the host: the creator). |
| Beats feel slow | Resolve is LLM time on the host (~8–10 s now; being optimized). Show the progress heartbeat. |

## 10. Known issues in the current server build (code defensively)
**Which build am I talking to?** These docs, schemas and examples describe rules `0.5.0` (the planet edition on the
Theatre's real-time input, engine v2.2): text entry through `action.begin` and action windows, casts as instant actions,
`phase.event` pushes, `seat.revived`. Rules `0.4.0` servers used `fields.slow` / `sent_at` / `window` and settled
everything at Resolve; rules `0.3.1` sends one city, six gods and no planet fields. `match.started.rules` doesn't carry
the rules version; the shape of `round.outcome.public.slow` (`event: "opened"` only exists on 0.4) tells them apart.

1. Windows don't survive a server restart (a window open at a restart is gone; resubmit as a Compose action).
2. `phase.progress { done, total }` counts Compose submissions, so it reveals that someone submitted (not who).
3. `caption` / `callouts` in `round.outcome.public` duplicate `previous_caption` / `previous_callouts` (kept for older
   clients). Read the `previous_*` fields.
4. `winners` in `match.ended` is defined by the Theatre core (seats only), so it is `[]` when a built-in AI demigod
   wins. Use `summary.champion_seat` / `summary.champions`.
5. Prose (captions, speeches, news, ramifications) is LLM- or template-written. The structured fields (`eliminated`,
   `alive`, `champions`, `standings`, `entropy`, `bolts`) are the result.
6. Template text (LLM off, late, or a quiet beat) is repetitive. It's correct, just plain.
7. `SubagentLine.line` in the SeatView is the subagent's goal; what it said this beat is only in `round.private`.

## 11. Fast-forward, time travel and local runs
Data contract and how-to only. Placeholders: `<PUBLIC>` and `<KEY>` (§0; over the tunnel every request carries
`?key=<KEY>`), `<ADMIN_TOKEN>` (the host's admin secret, sent only as the `Authorization: Bearer` header; never put it
in a URL, a log or a repo). On the host's Wi-Fi drop `?key=` and use `http://<LAN_HOST>:8080`.

**Which server has what.** Fast matches work on any rules `0.5.0` server. The admin API (11.2) and the core JSONL
recordings need a server built on Theatre `faa7220` or later and started with `THEATRE_RECORD=full`,
`THEATRE_RECORD_DIR=…` and `THEATRE_ADMIN_TOKEN=…`; without the token every `/admin/*` path is `404`. Check with
`GET /admin/matches` (below): `404` = not enabled, `401` = wrong token. `server.hello.api` carries `version` and
`rules_version`. Replaying a recording whose beat fell back (resolve failed twice or timed out) needs a server on
Theatre `1741d8f` or later, which records the `fallback_outcome` hook.

### 11.1 Fast matches on the live server
Create a room with these settings (`room.create { visibility: "private", settings }`), then `room.start`:
```json
{ "llm": false, "players": 4, "beats_total": 100, "flow_ms": 500, "compose_seconds": 0.001, "clusters": 40 }
```
- `llm: false`: every mind uses its rules fallback (no LLM calls, nothing spent, templated captions).
- `flow_ms` (alias `beat_ms`): 500–120000 ms. `compose_seconds` defaults to `flow_ms / 1000`; the Theatre accepts it
  down to 1 ms. With 1 ms, beats run back to back and a human seat gets its default (empty) action unless it uses the
  real-time paths: `action.begin` + windowed submits and instant casts work in any phase.
- `players` 1–8: the built-in AI fills every seat no human holds, so one connected client is enough to start.
- `clusters` 8–600: fewer crowds make the society step cheaper (200 is the default).
- A match ends when one demigod stands, usually well before `beats_total`.
- The room's start countdown (`THEATRE_COUNTDOWN_SECS`, host side, default 5 s) comes before `match.started`.

Measured on a private server (rules `0.5.0`, Theatre core, `match.started` → `match.ended` from the client's frame log,
4 demigods, `clusters: 40`, `llm: false`):

| Server build | `compose_seconds` | Beats played | Seconds | Beats/s |
|---|---|---|---|---|
| release (a copy of the host's current binary, on another port) | 0.001 | 41 | 0.76 | **54** |
| release | 0.05 | 43 | 3.1 | 13.9 |
| debug (unoptimized, Theatre faa7220) | 0.001 | 41 | 8.2 | 5.0 |
| debug, `clusters: 200` | 0.05 | 36 | 10.3 | 3.5 |

Every frame still flows at that speed (`round.started` … `round.committed` per beat), so a client that renders each
beat must keep up or skip; `round.committed.public_state` alone is enough to fast-forward a view.

### 11.2 Fast-forward and scrub recorded matches (admin API)
Every request carries the tunnel key as a query parameter and the admin token as a header. The Theatre ignores the
extra `key` parameter (checked on every route below: `200` with `?key=` present; `401` without the header).
```sh
P=https://<PUBLIC>; K=<KEY>; AUTH="Authorization: Bearer <ADMIN_TOKEN>"
curl -s -H "$AUTH" "$P/admin/matches?key=$K"
#   {"matches":[{"match_id","plugin","started_at","ended","committed_rounds","bytes","meta":{"code","room_id","seats":[…]}}, …]}  newest first
curl -s -H "$AUTH" "$P/admin/matches/<MATCH_ID>/timeline?key=$K"            # add &frames=true for every protocol frame
#   {"match_id","items":[{"type","id","name","round","at","snapshot"}, …]}  each item points at the snapshot in force
curl -s -H "$AUTH" "$P/admin/matches/<MATCH_ID>/state?after_round=<N>&view=public&key=$K"
curl -s -H "$AUTH" "$P/admin/matches/<MATCH_ID>/state?after_round=<N>&view=seat:<SEAT>&key=$K"
#   {"match_id","round","label","snapshot","view","state"}  view=public is round.committed.public_state (the PublicView
#   schema); view=seat:N is that seat's SeatView (hidden information: admin only); view=raw is the whole World;
#   at=<record id> instead of after_round picks any timeline item
curl -s -H "$AUTH" "$P/admin/matches/<MATCH_ID>/recording?key=$K" -o <MATCH_ID>.jsonl     # the raw JSONL recording
curl -s -H "$AUTH" -H 'content-type: application/json' -X POST "$P/admin/fork?key=$K" \
     -d '{"match_id":"<MATCH_ID>","after_round":<N>}'
#   {"room_id","code","match_id","resumes_at_round":N+1,"players":[{"seat","display_name","player_id","resume_token"}]}
```
- **Scrub:** `state?after_round=N` for `N` = 0 … `committed_rounds` is the world after each beat (0 = the start).
- **Fork (time travel):** a new private room that continues the match from after beat `N`, under a new `match_id`
  (its recording names its parent in `run_start.meta.forked_from`). Each human seat gets a fresh `resume_token`: open
  `wss://<PUBLIC>/ws?key=<KEY>` and send `auth.resume { resume_token }` instead of `auth.login` within the resume TTL
  (10 min by default); you get `auth.ok`, `room.snapshot` (state `in_match`) and `match.snapshot`, and the match runs on
  from beat `N+1`. Seats nobody resumes are played by bots. The reference client does it:
  `node fn7_client.mjs --url "wss://<PUBLIC>/ws?key=<KEY>" --resume <RESUME_TOKEN> --auto`.
- A fork plays the rules again from the same state; LLM answers are new unless the host runs a replay server
  (`THEATRE_REPLAY_FROM=<recording.jsonl>`, answers come from the recording at no cost).
- **Replay check:** `fn7-sim --replay <MATCH_ID>.jsonl` re-executes a downloaded recording (or a fork's) and checks
  every recorded state byte for byte (11.3).

Verified on a private server: list, timeline (1,167 items for a 14-beat match), `state` at beat 5 with `view=public`,
`seat:0` and `raw`, fork after beat 5 and resume with the reference client (beats 6–13 played), and a replay of both the
original and the fork with 0 differences.

### 11.3 Running locally (macOS, Linux, WSL Ubuntu)
Needs Rust ≥ 1.85 (`rustup`), a C compiler (`build-essential` on Ubuntu: SQLite is compiled in), git, and Node ≥ 22
for the reference client. TLS is rustls (no OpenSSL). The first build needs the network: the Theatre comes from
GitHub (`Aftersource/theatre`, branch `main`) and crates from crates.io. Nothing else outside the repo is needed.
```sh
git clone https://github.com/Aftersource/final_night_7 && cd final_night_7
# If the Theatre repo is private to you, let cargo use your git credentials:
export CARGO_NET_GIT_FETCH_WITH_CLI=true
cargo build --release -p fn7-server -p fn7-tools
```
Whole matches in-process, no server (seconds; prints every beat and a summary):
```sh
./target/release/fn7-sim --seed 7 --beats 100                       # rules only (no LLM); recorded to runs/recordings/
./target/release/fn7-sim --seed 7 --beats 100 --society-only        # only the society acts (no orders, no casts)
./target/release/fn7-sim --seed 7 --beats 30 --scripted             # offline scripted minds (exercises every LLM step)
./target/release/fn7-sim --replay runs/recordings/<RUN>.jsonl       # replay-verify (exit code 1 on any difference)
./target/release/fn7-project --db runs/fn7.sqlite runs/recordings/  # JSONL -> SQLite tables + fn7_* views
```
Measured on an unoptimized debug build (release is several times faster): rules-only seed 7, 25 beats (the match ended)
in 4.7 s, 35 ms per beat; `--society-only` seed 7, 85 beats in 18.6 s, 15 ms per beat.

Your own server (any free port; pick your own secret token):
```sh
THEATRE_BIND=127.0.0.1:8090 THEATRE_DATA_DIR=runs/data THEATRE_RECORD=full THEATRE_RECORD_DIR=runs/recordings \
THEATRE_ADMIN_TOKEN="$(openssl rand -hex 24)" ./target/release/fn7-server
node docs/clients/fn7_client.mjs --url ws://127.0.0.1:8090/ws --name Me --create --auto --beats 10
```
No `ANTHROPIC_API_KEY` / `LLM_BASE_URL` means no LLM (rules only). `fn7-server` reads a `.env` in the working
directory.

**Engine side by side (optional).** Only to change the engine: clone `Aftersource/theatre` next to `final_night_7`
and add a gitignored `final_night_7/.cargo/config.toml` that patches the six Theatre crates to `../theatre/crates/…`:
```toml
[patch."https://github.com/Aftersource/theatre"]
theatre-plugin-api = { path = "../theatre/crates/plugin-api" }
theatre-llm = { path = "../theatre/crates/llm" }
theatre-server = { path = "../theatre/crates/server" }
theatre-protocol = { path = "../theatre/crates/protocol" }
theatre-toolkit = { path = "../theatre/crates/toolkit" }
theatre-trace = { path = "../theatre/crates/trace" }
```
That still builds online (crates.io). Offline builds need a vendored crate directory (`cargo vendor`), which is not in
either repo.

## 12. Simulator API (for balance)
Whole matches run on the host's server, with any room settings, without building this code. Same placeholders and
auth as §11: `<PUBLIC>`, `<KEY>` (`?key=` is ignored by the routes, required by the tunnel) and `<ADMIN_TOKEN>` as the
`Authorization: Bearer` header. Every `/sim/*` path is `404` when the host has no admin token set and `401` with a
missing or wrong token. Errors are `{"error": "..."}` with `400` (bad request, the message says what), `404`, `409`
or `429` (queue full: retry later).

```sh
P=https://<PUBLIC>; K=<KEY>; AUTH="Authorization: Bearer <ADMIN_TOKEN>"; J='content-type: application/json'
curl -s -H "$AUTH" "$P/sim/settings?key=$K"          # every setting: key, type, default, min/max, description
curl -s -H "$AUTH" -H "$J" -X POST "$P/sim/run?key=$K" \
     -d '{"settings":{"entropy_scale":2,"start_bolts":5},"seeds":[1,2,3,4],"players":4,"beats":100,"client":"bridge"}'
#   {"job_id":"<JOB_ID>","status":"queued","runs":4,"settings_hash":"…","poll":"/sim/jobs/<JOB_ID>"}
curl -s -H "$AUTH" "$P/sim/jobs/<JOB_ID>?key=$K"     # poll until status is "done" (or "failed")
curl -s -H "$AUTH" "$P/sim/jobs?key=$K"              # recent jobs, newest first, and the pool's load
curl -s -H "$AUTH" -H "$J" -X POST "$P/sim/compare?key=$K" \
     -d '{"settings":{"clusters":200},"a":{"entropy_scale":1},"b":{"entropy_scale":3},"seeds":[1,2,3,4,5,6,7,8]}'
curl -s -H "$AUTH" "$P/sim/jobs/<JOB_ID>/recording/<SEED>?key=$K" -o <SEED>.jsonl   # needs "record": true
```

**`POST /sim/run`** fields (anything else is a 400):

| Field | Default | Notes |
|---|---|---|
| `seeds` | required | 1–64 distinct non-negative integers; one match per seed |
| `settings` | `{}` | room settings (`GET /sim/settings`); an unknown key is a 400 (typos don't pass silently); `seed` and `llm` are not allowed here |
| `players` | `settings.players` or 4 | 1–8 demigods, all played by the built-in AI |
| `beats` | `settings.beats_total` or 100 | the Final Night's beat, 1–500 (most matches end earlier: one demigod left) |
| `society_only` | `false` | only the society acts (no orders, no casts) |
| `record` | `false` | write each seed's core JSONL recording (at most 8 runs per job; 15–40 MB each) |
| `llm` | `false` | `true` uses the host's Claude backend and needs `budget_usd` (> 0, ≤ 1.0); calls stop at the budget (the minds fall back to rules) |
| `client` | none | your name for the host's job log |

**`POST /sim/compare`**: the same fields plus `a` and `b` (settings overrides merged over `settings`; they must
differ). Both arms run the same seeds; seeds × 2 runs.

**Job (`GET /sim/jobs/<JOB_ID>`)**:
```json
{ "job_id": "…", "kind": "run", "status": "queued|running|done|failed", "duration_ms": 2997,
  "progress": { "done": 4, "total": 4, "failed": 0 }, "settings_hash": "2568e8a8fe4acb57",
  "settings": { …validated room settings… },
  "results": [ {
    "seed": 1, "arena": "Earth", "beats_played": 21, "beats_total": 100, "ended": "last_standing|final_night|stopped",
    "champion": { "name": "Brix", "seat": 1, "health": 7.5, "demigod": "dg:1" }, "champions": […], "alive_end": 1,
    "eliminations": [ { "beat": 10, "name": "Ash", "seat": 0, "cause_kind": "army", "cause": "overrun by the Volunteers of Vienna for Brix" } ],
    "eliminations_count": 3, "first_elimination_beat": 10,
    "eliminations_by_cause_kind": { "army": 1, "debt": 2 }, "eliminations_by_cause": { "<cause text>": 1 },
    "eliminations_by_beat": { "10": 1, "21": 2 },
    "casts": { "count": 6, "fizzled": 0, "bolts_spent": 9, "by_power": { "flood": 2 } }, "bolts_end_total": 14,
    "entropy": { "final": 32.6, "max": 32.6, "heat_death_beat": null, "curve": [[1, 1.2], [2, 2.9], …] },
    "slow": { "windows": 0, "mean_open_ms": null, "timeouts": 0, "instant_orders": 30, "debt_billed": 54.5, "paid_with_bolt": 30 },
    "debts": { "paid": 43, "paid_amount": 55.95, "open": 2, "delayed": 0, "owed_amount": 1.75 },
    "society": { "mobs_formed": 25, "mobs_dissolved": 10, "mobs_ever": 25, "people_emerged": 40,
                 "people_emerged_by_role": { "ringleader": 25, "captain": 6, … }, "people_emerged_dead": 0,
                 "battles": 0, "clashes": 0, "migrations": 11, "migrants": 919283, "deaths": 41927, "cults": 5,
                 "mutinies": 1, "defections": 0, "promotions": 0, "deserters": 0, "hosts": 2,
                 "arena_population_start": 141995079, "arena_population_end": 141893117 },
    "llm": { "plugin_calls": 0, "plugin_fallbacks": 0, "provider_calls": 0 },
    "timing": { "wall_ms": 854.7, "beat_ms": { "n": 21, "mean": 3.6, "p50": 3.3, "p95": 6.1, "max": 7.3 }, "society_ms": { … } },
    "recording": { "run_id": "…", "file": "….jsonl", "download": "/sim/jobs/<JOB_ID>/recording/1" } } ],
  "aggregate": { "seeds_ok": 4, "seeds_failed": 0, "champion_seats": { "seat 1": 2, "seat 2": 1, "seat 3": 1 },
                 "ended": { "last_standing": 4 }, "eliminations_by_cause_total": { … },
                 "stats": { "beats_played": { "n": 4, "mean": 29, "median": 29.5, "min": 21, "max": 36 }, "entropy.final": { … }, … } } }
```
- A seed that is still waiting reads `{"seed", "status": "queued|running"}`; one that failed reads `{"seed", "error"}`.
- `aggregate.stats` has every numeric field above as a dotted path (`casts.count`, `society.battles`,
  `eliminations_by_cause_kind.debt`, `timing.beat_ms.mean` …). A compare job has `a` and `b` (each `settings`,
  `settings_hash`, `results`, `aggregate`) and `delta`: `{ "<path>": { "a", "b", "mean": b−a, "median": b−a, "pct" } }`
  for every stat that changed.
- `cause_kind` is a coarse class of the cause prose: `debt`, `arena_clock`, `accident`, `army`, `final_night`,
  `god_power`, `bolt`, `entropy`, `mod`, `other`.
- `slow.windows` is always 0 here: in a simulation the AI demigods give their orders without opening a slow-time window
  (each is an `instant_orders` bill). Wall-clock settings (`flow_ms`, `compose_seconds`, `resolve_timeout_s`) have no
  effect: beats run back to back.
- **Deterministic:** with `llm: false` the same settings, seed and players give the same result every time (all but
  `timing` and `recording`), so A/B differences are the settings' doing. `llm: true` results are not reproducible.
- A recording is the core's JSONL (the same format as the live server's): replay it with `fn7-sim --replay`, and when
  the host keeps sim recordings in the server's recording directory (the default) `GET /admin/matches/<run_id>/…`
  (§11.2) reads it too.
- **Limits:** 64 seeds per job, 500 beats, 1–8 players, `budget_usd` ≤ 1.0, at most 8 recorded runs per job, 512 runs
  queued across all jobs (then `429`). The host runs 2 simulations at a time on their own threads, so live rooms keep
  their pace; the rest wait their turn. The last 200 jobs are kept in memory (lost on a server restart).

Measured on the host's machine (release build, 2 workers, default settings, `llm: false`): 4 seeds (21–36 beats each)
in 3.0 s; 4 `society_only` seeds (65–100 beats) in 9.8 s; a compare of 8 seeds × 2 arms in 7.0 s; 64 seeds in 31.4 s
(about 1 s per seed, 5 ms per beat). A live room paced at 1 s per beat kept its pace while 64 seeds ran (median beat
interval 1032 ms alone, 1043 ms under load).

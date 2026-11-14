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
| Game WebSocket | `wss://<PUBLIC>/ws?key=<KEY>` | `ws://10.0.0.34:8080/ws` |
| Health | `https://<PUBLIC>/healthz?key=<KEY>` | `http://10.0.0.34:8080/healthz` |
| Docs (this folder) | `https://<PUBLIC>/docs/<file>?key=<KEY>` | `http://10.0.0.34:8081/<file>` |

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
| Game server (WebSocket) | `ws://10.0.0.34:8080/ws` (LAN playtest host) |
| Health / stats | `http://10.0.0.34:8080/healthz` → `ok`, `http://10.0.0.34:8080/stats` |
| These docs, served over HTTP | `http://10.0.0.34:8081/` (browse or `curl`; same files as `final_night_7/docs/`) |
| Engine protocol (envelope, login, lobby) | `theatre/PROTOCOL.md` (here: [`theatre/PROTOCOL.md`](theatre/PROTOCOL.md); also in the `Aftersource/theatre` repo) |
| Game payloads (every Final Night 7 object) | [`CLIENT_API.md`](CLIENT_API.md) |
| JSON Schemas (draft 2020-12), one per payload | [`schemas/`](schemas/) |
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
  mkdir -p "$(dirname "$f")"; curl -fsS "http://10.0.0.34:8081/$f" -o "$f"; done
for d in schemas examples; do mkdir -p $d
  curl -fsS "http://10.0.0.34:8081/$d/" | grep -o 'href="[^"]*\.json"' | cut -d'"' -f2 |
  while read f; do curl -fsS "http://10.0.0.34:8081/$d/$f" -o "$d/$f"; done; done
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
curl -s http://10.0.0.34:8080/healthz          # ok
curl -s http://10.0.0.34:8080/stats            # {"game":"final_night_7",...}
node --version                                  # must be >= 22 (built-in WebSocket)
node clients/fn7_client.mjs --url ws://10.0.0.34:8080/ws --name AgentTest --create --auto --beats 3 --beat-ms 8000 --log frames.jsonl
```
That last command creates a private room, starts a short match against the built-in AI demigods, plays it with
scripted orders every third beat and a lightning bolt now and then, prints each beat's caption (one beat late), slow
windows, casts, world events, Entropy, your hands' replies, omens and news, and writes every frame (both directions)
to `frames.jsonl`. Expected output, abridged (a real run against a private offline server):
```
=== beat 6  arena 1500 km  alive 2  YOU ARE OUTSIDE THE ARENA
you: health 3.2  bolts 21  debt 0.1  zoom district  entropy 6.8 (calm)
>> [beat 5] Beat 5: Time's clock catches Mara outside the arena, and 5 more things went sideways.
   ⧗ Vex slowed time (20 beats left, window 6)
   ⧗ Time snaps back: Vex's window closed (enter, 0 ms)
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
(slow time; typing a first order line sends it too), `l: …` (left hand), `r: …` (right hand), `z: city` / `z: character
ch:12` (zoom), an empty line is Enter (sends your orders). Casts: `b: dg:2` (a lightning bolt at a target), `c: volcano
3.38,6.52` (a power from the palette at a ref, a `lon,lat` or a name).

## 4. The loop
```
connect → server.hello (check game == "final_night_7")
→ auth.login {display_name} → auth.ok (store resume_token!)
→ room.create {visibility:"private", settings:{beats_total, flow_ms}} | room.join {code} | room.quickjoin
→ room.ready {ready:true} → (host) room.start → room.countdown → match.started {rules}
per beat (round == beat; every beat is a flow beat of flow_ms):
  phase start   → round.started   {public: arena/eliminations, private: your health / outside_arena / debt_due}
  phase brief   → round.brief     {public: arena, gods, powers, entropy, flow (the open slow window), regions; private: SeatView}
  phase compose → (deadline = epoch ms) → action.submit {round, text:"", fields} only when you have something:
                    text entry: fields {slow} when typing starts; {left, right, zoom, window, sent_at} on Enter
                    casts:      fields {cast | casts | bolt | bolts, sent_at}, any beat
  phase resolve → (phase.progress every ≤5 s; milliseconds when nobody spoke or cast)
  phase reveal  → round.outcome   {public: previous_caption, omens, entropy, slow (opened/open/closed), casts, events, timing}
                → round.private   {private: SeatDelta = hands' replies, subagents, your news, your omens, held, cut_off}
  phase commit  → round.committed {scores, public_state: PublicView (+ bolts, influence, armies, mods, entropy, slow, gods)}
                → round.private_state {state: SeatView (+ armies, orders, slow)}
→ match.ended {winners, summary: champions, standings, piles (+ statue_prompt), entropy, heat_death, final cut}
always: sys.ping every ~20 s; on disconnect reconnect with auth.resume {resume_token} → match.snapshot
```
Rules that bite:
- **Submit only when you have something.** Compose closes as soon as every active seat has submitted, so an idle
  submission shortens everyone's flow beat. Submit once per beat; resubmitting while Compose is open replaces your
  action. Anything after Compose closed gets `wrong_phase`: hold it for the next beat.
- **Send `sent_at` (epoch ms) with every submission.** The cut-off rule orders same-beat submissions by it, and it
  measures how long a slow window stayed open (Time's penalty). Orders without it can't prove they came first.
- **Text entry is fast or expensive.** Every message with orders is a Time interaction (a bolt to Time, a debt that
  grows with every second the window stays open), and the initiator's Enter cuts off orders sent after it. Send the
  window id (`round.brief.public.flow.slow.window`) with orders typed during a window.
- **Casts and orders are separate.** Typed words never cast; a cast never carries orders or slows time.
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
| The slow window: who opened it, beats and ms left | `round.brief.public.flow.slow`, `PublicView.slow`; this beat's announcements in `round.outcome.public.slow` (`opened` / `open` / `closed` with `how` and `after_ms`) |
| Whether your orders are held or were cut off | `round.private.private.held` / `.cut_off`; `SeatView.slow` (`yours`, `window`, `held`) |
| The power palette (slug, icon key, cost, radius, target kind) | `round.brief.public.powers` |
| Casts this beat (power, icon key, cost, point, region, target, fizzled) | `round.outcome.public.casts` |
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
| `error {code: "wrong_phase"}` on submit | Compose already closed (someone submitted last), you're eliminated, or the `round` is stale. Hold the input for the next beat. |
| `error {code: "too_long"}` | `text` > 500 chars. Put speech in `fields.left/right` (≤ 2000). |
| `error {code: "bad_request"}` | Read `message`. Usually a bad `zoom` level or a hand field that's not a string. |
| Nothing happens after `room.ready` | Someone must send `room.start` (the host: the creator). |
| Beats feel slow | Resolve is LLM time on the host (~8–10 s now; being optimized). Show the progress heartbeat. |

## 10. Known issues in the current server build (code defensively)
**Which build am I talking to?** These docs, schemas and examples describe rules `0.4.0` (the planet edition): its
`round.brief.public` has `powers`, `entropy`, `flow` and `regions`. A server on rules `0.3.1` sends one city, six gods
and none of those fields.

1. **Flow and slow are v0.** The engine's Compose timer is fixed per room and closes only when every seat submitted or
   the deadline passed; it can't be extended or closed by one seat, and there is no mid-phase push or "started typing"
   message (engine proposal v2.2). So a slow window spans flow beats, held orders and the cut-off are applied at Resolve,
   and arrival order comes from your `sent_at`. `phase.progress { done, total }` counts submissions, so it reveals that
   someone submitted (not who).
2. **Casts ride on `action.submit`** until the engine's instant actions arrive; they land at the next Resolve.
3. `caption` / `callouts` in `round.outcome.public` duplicate `previous_caption` / `previous_callouts` (kept for older
   clients). Read the `previous_*` fields.
4. `winners` in `match.ended` is defined by the Theatre core (seats only), so it is `[]` when a built-in AI demigod
   wins. Use `summary.champion_seat` / `summary.champions`.
5. Prose (captions, speeches, news, ramifications) is LLM- or template-written. The structured fields (`eliminated`,
   `alive`, `champions`, `standings`, `entropy`, `bolts`) are the result.
6. Template text (LLM off, late, or a quiet beat) is repetitive. It's correct, just plain.
7. `SubagentLine.line` in the SeatView is the subagent's goal; what it said this beat is only in `round.private`.

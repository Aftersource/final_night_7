# Brief: the Unreal client sessions

*For Chris's Mac session (`mac/*` branches) and Benito's Windows builder / gamedesign session (`benito/*` branches).
Designers: Chris and Benito. Read `../FULL_EXPLAINER.md` first; this is your subset.*

## 1. The contract in one paragraph

The server is authoritative; **the client only displays state and sends inputs.** Every payload, field and refusal is
defined in the server repo's `docs/CLIENT_API.md` with JSON Schemas in `docs/schemas/` and a real frame of every
message in `docs/examples/`. Ignore unknown keys (new optional fields get added). Positions are `[lon, lat]`,
longitude first. Rivals' health is never sent, except the duel's health bars. A reconnect (`auth.resume`) gets one
`match.snapshot` with the exact present (`present`): draw from it and do nothing special. Rate: at most 20 messages a
second, `sys.ping` every ~20 s; casts 2 a second (burst 4).

## 2. Two input paths, never mixed

| Path | Send | You get |
|---|---|---|
| **The will** (text) | `action.begin {}` on the first keystroke; then `action.submit {round, text, fields: {left, right, zoom}, window_id}` on Enter | `window.opened` + `phase.event` `slowed` (everyone), `action.received` (you), `window.closed`, or a private `cut` |
| **Casts** | `action.submit {instant: true, fields: {cast: {power, tier: "mega"\|"mini", target}}}` | `phase.event` `streak` (everyone, at once), or `action.received {accepted: false, reason}` |
| **Summon** | instant `fields.summon {demigod, target}` | `phase.event` `summoning`; `summoned` at the resolve |
| **Challenge** | instant `fields.challenge {demigod, target, level}` (travels alone) | `phase.event` `challenge.lock`, or a refusal toast |
| **Duel line** | instant `fields.duel {duel_id, text}` (<= 120 chars); live keys as `duel_keys` | `duel.blow` (everyone); `duel.typing` (masked) |

An action with both orders and casts is refused. Typed text never casts.

## 3. The locked controls (HoG §6, Chris, 2026-11-15) — do not regress

1. The land slides under the hand for the whole drag; the board re-centres seamlessly every 1.5 hexes and once on
   release; a re-centre never moves the land under the hand.
2. While panning, the relief keeps its scale (`bReliefHold`) and the board stays on its cube face.
3. Nothing else touches the board under a moving hand: earth playback holds until release
   (`AFNStudio::IsBoardPanning`), no rebuild storms, no coast or fling on the board.
4. The **system mouse pointer**; no drawn cursor.
5. The land under the pointer stays under the pointer (no camera motion during a grab; the grab plane is the pressed
   hex's height; measured within 3 px).
- A globe click (moves under 20 px, 1080p-scaled, within 0.6 s) dives to that region.
- Double-click zooms in, right-click zooms out (never eaten by a demigod), wheel one level per gesture. All zooms
  animate. Every blocked gesture logs its rule (`FN7 zoom: blocked by ...`).
- **Proof before push:** any change to input, pan, zoom or the board rebuild path gets a 60-second live drag-and-zoom
  test in Chris's editor at her real fps, with the log lines in the report.

### Today's demigod rules (portrait = piece)

| Gesture | Result |
|---|---|
| Double-click your own demigod (card or piece) | Glide to centre it **at the same zoom**, gold ring pulse |
| Double-click a rival in reach | The **challenge card**: "CHALLENGE? / A vs B / win %", one distance, DUEL or TELEPORT + DUEL; Enter = DUEL |
| Double-click a far rival | Centres it |
| ITEMS button on the card, or the statue's shelf | Opens that demigod's items |
| Anything else on a demigod | Nothing. No other code may take a click on a demigod |
| Press + drag your own demigod | SUMMON drag |

Reach (the server re-checks): the board cards touch at the current zoom (`regions` 600 km, `villages` 15, `houses`
0.15, `people` 0.015), or within 120 km at any zoom. The 17:00 EST session log names the regression's cause
(`Features/FNInventoryShelf.cpp` reading pressed buttons every frame; `FNEventGlide`'s restore undoing a user centre);
check the Unreal repo for the fix's state.

## 4. Every server message you must draw

| Message / event | Draw |
|---|---|
| `server.hello` | check `game == "final_night_7"` |
| `match.started` | the table; intro voice line |
| `phase.changed` | beat phases (`compose` carries the deadline) |
| `round.started` | beat start; `challenges[]` (AI duel locks, with `leap` = teleport + red pulse); falls at Start |
| `round.brief` | `powers[]` (mega/mini variants, god-tier), `gods`, `final_wish`; your seat view: `hint` (one big line top centre, arrow at `target`, `why` under it), `goal` (a corner), `time_bill` |
| `window.opened` / `phase.event` `slowed` | banner "<name> slowed time"; the opener's live bill (`bill_now`, `debt_per_second`, `debt_cap`) |
| `window.closed` | close the window (`submitted`, `deadline`, `match_ended`) |
| `action.received` | `cut` → "your orders were cut"; refusals as toasts (`too_far:<km>:<allowed>` split on `:`) |
| `phase.event` `streak` | the cast's streak at once; show `text` (place words), never formatted coordinates; `own_hit` |
| `phase.event` `summoning` / `god_power` | flash and whoosh now; lands at the resolve |
| `phase.event` `challenge.lock` | both portraits slam in, both health bars, every camera centres on `focus`; skins |
| `duel.scene`, `duel.panel`, `fields.duel_tick` | the box, the mannequins, the comic panels |
| `duel.typing` | masked stars with the newest letters |
| `duel.blow` | the blow, `hp`, `flavor`, `whiff`, `stage` |
| `round.outcome` | `dice[]` (table d20: whose and what for), `dm_events[]` (feed lines), `casts[]`, `events[]` (moment cards via `image`), `omens[]`, `slow[]`, `entropy`, `earth` (`phenomena`, `delta`, `effects`), `scenes[]` (token play), `looks[]`, `duels_open[]`, `duels[]` (`challenge.end`), `god_powers[]`, `summons[]`, `quests`, `curses`, `wills[]`, `eliminated`, `previous_caption` |
| `round.private` | `wills[]` (always show `result`; `why` and `next` when present), `will_events[]`, `hands`, `news`, private `omens` |
| `round.committed` | the public view: positions, cells, `look` per demigod, shrines, roads |
| `round.private_state` | your seat view |
| `seat.eliminated` / `seat.revived` / `seat.joined` | room state. With wills on, `seat.eliminated` is not sent for a wiped player: read `players[].alive` and keep the WILL box open |
| `match.ended` | end screen: `champions`, `standings`, `piles`, `final_cut`, `storyboard`; victory / defeat voice |
| `match.snapshot` | the exact present on reconnect |

Queries you draw from: `game.globe`, `game.region`, `game.node`, `game.earth`, `game.earth_at`, `game.people`,
`game.buildings`, `game.fault` (only while a mega is armed), `game.duel_odds` (the challenge card), `game.memory_tree`
(TREE), `game.media`, `game.me`.

## 5. Benito's world rule (EiW)

What you do changes the world, not the log: at every zoom the damage shows at that scale and persists, and the picture
matches the server's numbers (`earth.effects[].count`: draw that many tokens at people scale, scaled down per level;
`scars` for what lasts). Camera: stays put for others' disasters, glides to your own and to typed actions (never while
grabbing, zooming or typing; always returns); ~4 s per disaster, ~8 s per typed action. Ownership: board-life
reactions in `FNStudioTable` are Benito's side; the camera glide and the token-scene player are the Mac's.

## 6. Sound and voice hooks that exist

Sound pass (`UI/FNSoundPass.h`): MiniWhoosh / MegaWhoosh (cast), Rumble (quake), Clash (duel lock), Thump / Swish /
Puff (blow lands / misses / whiffs), Victory / Fall; studio Chime on urgent hints and dice Click / Chime / Boom.
Voice (`UI/FNVoiceOver.cpp`): welcome, intro, clash, victory, defeat, duel won, duel lost. New voice lines come from
`VOICEOVER.md`; each needs an import under `/Game/Audio/VO/` and a `SayOnce` hook with a stable key.

## 7. Never

- Never regress the locked controls; no rebuild, re-normalisation or face switch during a drag.
- Never compute rules on the client (damage, odds, verdicts): draw the server's numbers. Never show a rival's health
  outside the duel bars.
- Never commit to `main` or to another owner's branch; never force-push; never touch another layer's files without a
  request to the backend (PLAYBOOK Play 9, Play 10). Proof screenshots go under `docs/proof/` on your branch.
- Never put keys, tokens, server addresses or personal data in the repo, logs or screenshots (Play 6).
- Never spend Meshy or image credits without Chris or Benito saying so.

## 8. Read

`../FULL_EXPLAINER.md`; server repo `docs/CLIENT_API.md` (all of it, it is the contract), `docs/PLAYBOOK.md`; Unreal
repo `docs/design/hand_of_god/HAND_OF_GOD.md`, `docs/design/EVENTS_IN_THE_WORLD.md`, `docs/FOR_BENITO.md`,
`production_assets/audio/vo/README.md`.

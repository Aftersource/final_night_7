# Brief: the 30-60 second video and the pitch

*For whoever cuts the gameplay video and writes the pitch. Designers: Chris and Benito. The jam asks for a 30 to 60
second gameplay video (FOR_BENITO §5). Read `../FULL_EXPLAINER.md` first; this is your subset. Pitch deck drafts
live in `../../pitch/` (another agent's folder).*

## 1. The pitch in three lines (GD one page)

1. Seven gods sit at one table over a floating globe of the real Earth; each player is a god with two demigods on the
   board.
2. The only controls that matter are **the words you type and the lightning you throw**: typing slows time for
   everyone, and your Enter cuts off anyone slower.
3. An **AI Dungeon Master** runs the world and never waits; the dice decide, the AI makes it make mystical sense; the
   last god standing wins the Final Night.

**The theme, "Director's Cut":** the player directs history in words; Time controls the clock and the cut-off; a duel
cuts from the strategy board to an arena; any match can be scrubbed, replayed or forked into a new cut.

**What is new:** typing is the real-time skill (speed and wit decide the cut-off and the duels; an LLM never overrules
a die); history is a lens; one deterministic world from the globe down to a 3 m hex with named people, replayable
byte for byte.

## 2. The video, in order (about 55 s)

Each shot names the moment, what reads on screen, and the line or sound that exists today. Show only what is built.

| # | Seconds | Moment | What reads on screen | Audio that exists |
|---|---|---|---|---|
| 1 | 0-6 | The god room: the stone god statues at the table, the globe floating over the board | the two styles: a realistic room, a toy world | **intro** voice line ("Seven nights. Seven wars. ... FINAL NIGHT SEVEN.") |
| 2 | 6-10 | Grab and spin the globe, click to dive to a region, the board rises | the hand-of-god controls: the land stays under the pointer | ambient bed |
| 3 | 10-17 | A player starts typing: "<name> slowed time" banner, the bill growing; Enter; a slower rival's "YOUR ORDERS WERE CUT!" | the will and the cut-off: the core mechanic | (no sound for typing yet) |
| 4 | 17-22 | The table d20 tumbles with whose roll and what for; the god's verdict in the feed | the dice decide | dice clicks, chime or boom |
| 5 | 22-30 | A mega thrown at a plate junction: the fault lines while armed, the streak, a flood or wildfire spreading thing by thing; zoom to people level: tokens run to shelters | megas shake the map; the world changes, not the log | MegaWhoosh + Impact, Rumble on a quake |
| 6 | 30-35 | A token scene: the roof lifts like a dollhouse, bubbles, the d20, the outcome | your words played out by the little tokens | (no sound yet) |
| 7 | 35-47 | Double-click a rival: the challenge card with the win %, DUEL; portraits slam in, health bars; masked typing; blows land; a whiff ("you have no pistol") | will against will, an Archon cut to the arena | Clash + **clash** voice line; Thump / Swish / Puff; **duel won** |
| 8 | 47-52 | Entropy climbing toward heat death, or Death raising a fallen demigod | consequences and prices | ambient bed swelling with Entropy |
| 9 | 52-58 | The end screen and the storyboard | the last god standing | Victory + **victory** voice line |

Alternatives if a shot is not ready: the tree of consequence (TREE) as a star map (shot 8), the god form's statue
built at runtime (shot 1).

## 3. What reads best on screen

- The **slow-time banner and the cut-off**: one sentence explains the whole game.
- The **challenge lock**: two portraits slam in and every camera swoops. The strongest single beat.
- **Masked typing** in a duel (stars with the newest letters showing): shows "typing is the skill".
- **Fault lines lighting up** while a mega is armed, then the hit at a junction.
- **People tokens fleeing to shelters** and burnt stumps that stay.
- The **table d20** with whose roll it is.

## 4. Honest limits (do not show or claim)

- A duel does not yet freeze the world (the engine has no room-wide hold; beats keep resolving).
- A lone dead player rewinding the match is not built (`room.rewind` requested from the engine).
- Video clips per storyboard shot are not built.
- Only the seven listed voice lines exist; every other moment is silent or uses procedural sound.
- Show only real play from a real server; do not stage results the rules did not produce.

## 5. Never

- No keys, server addresses, tunnel URLs, emails, account names or other personal data on screen or in the
  description (check the HUD, the chat, the lobby and any terminal in frame).
- No profanity in captions or lines. Credit **Chris and Benito** as the designers.
- Disclose prior work per the jam rules (the 2024 Final Night prototype and Final Night 2.5; PROJECT_GUIDE §1).
- No real person's voice or likeness.

## 6. Read

`../FULL_EXPLAINER.md` (sections 0, 4, 5, 9, 11, 16-18); `../../design/GAME_DESIGN.md`; Unreal repo
`docs/FOR_BENITO.md`, `production_assets/audio/vo/README.md`; the earlier draft cut noted in the BUILDLOG session log
(54 s, recorded on a test server).

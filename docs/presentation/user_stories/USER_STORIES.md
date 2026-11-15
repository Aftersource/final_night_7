# Final Night 7: User Stories

*Epic AI MegaJam 2026. Designers: Chris and Benito. Companion to `docs/presentation/design/GAME_DESIGN.md`.*

Each story is "As a…, I want…, so that…", with acceptance criteria that match what is built, a **status** and the
**evidence** (a document section, a source file or a commit). "RB n" is chapter n of `docs/DM_RULEBOOK.md`; "Unreal
<hash>" is a commit on the Unreal client's main branch (merged as of a89c986).

**Status key:** **built** (on the live server or merged in the client) · **partial** (some criteria met, or built on one
side only) · **not built** (designed or requested, not done).

| Persona | Stories | Built | Partial | Not built |
|---|---|---|---|---|
| A. New player | 7 | 6 | 1 | 0 |
| B. Competitive speed-typer | 6 | 5 | 1 | 0 |
| C. A fallen player | 5 | 4 | 0 | 1 |
| D. Spectator or mid-match joiner | 4 | 3 | 1 | 0 |
| E. A judge exploring the data | 5 | 3 | 2 | 0 |
| F. The designer / balancer | 5 | 5 | 0 | 0 |

---

## A. New player

### A1. Know what to do next
As a **new player**, I want the game to tell me my next move every beat, so that I never sit wondering "what do I do now".

- Every beat my seat gets one private hint (a line, why it matters, a gesture, a target) and a goal line ("Last standing wins. You: 2 alive, 3 rivals. Beat 41 of 60.").
- Hints are picked by rules in a fixed priority (duel, danger, the fallen, spectators, rival in my hex, nearby event, mega at a junction, no gear, quest step, a will).
- Hints use plain words ("right here", not "0 km away"); low-urgency hints rotate every 3 beats; urgent ones stay.

**Status: built.** Evidence: RB 29; BUILDLOG "Clear words (state schema 22)"; `docs/CLIENT_API.md` "Next move hint".

### A2. One-press wills
As a **new player**, I want ready-made wills I can send with one press, so that I can act before I know what to type.

- The WILL shows four carved buttons: ankh (Death resurrection line), bow (arm and hunt), meal (Nature heal), armies (raise an army).
- A press fills the box with a line using my real demigods, a city and a rival, counts down 5 s, then sends it as Enter would.
- Typing in the box stops the auto-send; another press replaces the line and restarts the count.

**Status: built.** Evidence: Unreal f53ced8 / a89c986 (`SFNHudLive.cpp`, `SFNPowerTile.cpp`); GAME_DESIGN §3.1.

### A3. Find my demigod
As a **new player**, I want to double-click my demigod and see it centred, so that I never lose track of my pieces.

- Double-clicking my card or piece glides the view to centre it at the same zoom, with a gold ring pulse.
- A single click on a card selects it; a plain click never moves the view.
- Glides stay off for 6 s after I move the view myself.

**Status: built.** Evidence: Unreal f68312d; HAND_OF_GOD §6.

### A4. Understand the purse and Entropy
As a **new player**, I want bolts, sparks, debt and Entropy explained on screen, so that I know what my powers cost.

- Hovering Entropy explains what it means; the purse explains bolt, spark and debt with a one-line legend.
- The table d20 says whose roll it is and what for.

**Status: built.** Evidence: Unreal 4188736 (legibility, Benito's audit 5 and 13), 60abcfe.

### A5. A welcome that sets the scene
As a **new player**, I want a narrator to welcome me and open the match, so that I know I am a god at a table.

- The lobby plays "Welcome, god. Your two hands wait on the table below…"; a match start plays the "Seven nights. Seven wars." intro.
- Voiceovers play one at a time and any key skips.

**Status: built.** Evidence: Unreal 959734c; `production_assets/audio/vo/README.md`.

### A6. Make my god
As a **new player**, I want to describe my god as a stone statue, so that my seat at the table is mine.

- I type how my stone godform looks (up to 600 characters), choose a stone and a classical pose, and see a preview image.
- On accept, the picture goes on my pedestal at once and a 3D statue is built (1 per account) and shown in my chosen stone.
- Until I make one, my seat shows a house marble statue with my Left Hand's likeness.

**Status: built.** Evidence: ACCOUNTS §14; BUILDLOG "the god form, chosen in stone"; Unreal d1469c2.

### A7. Choose how to play
As a **new player**, I want to choose between playing bots, hosting, joining by code or the house game, so that I can start a match my way.

- The house game FINAL7 is always on; joining is one click.
- Hosting exists in the client (with a cancel).
- *Not verified here:* that every option of the "classic lobby" Benito asked for (play vs bots / host / join by code / the house) is in the shipped build.

**Status: partial.** Evidence: FOR_BENITO §2; BUILDLOG 2026-11-15 (classic lobby assigned to the Mac); Unreal f4f75c0 ("cancel hosting").

---

## B. Competitive speed-typer

### B1. Win the cut-off
As a **speed-typer**, I want my fast Enter to cut off slower rivals, so that typing speed is real power.

- Starting to type opens one slow-time window per room (up to 30 s) if my demigod stands and I hold a bolt; everyone sees I opened it.
- My Enter closes the window; later submits are cut privately.
- Time charges a bolt toll, a debt that grows with the seconds used and how often I slow time, and Entropy.

**Status: built.** Evidence: RB 12.2; PROJECT_GUIDE §2.3.

### B2. Duel with words
As a **speed-typer**, I want every line I type in a duel to be a blow judged at once, so that fast, vivid writing wins fights.

- Each line (up to 120 characters) rolls a d20 against DC 10 (+1 at night, + half the opponent's wardrobe), resolved by rules with no LLM wait.
- Wit adds up to +4 (verbs, a body part, a held item, a place word, the opponent's scars); repeats, short lines and gibberish cost.
- Two blows within 0.5 s: the second is a penalty costing me 0.5.

**Status: built.** Evidence: RB 21.2; DUEL_MOVES §5.

### B3. Know the odds before I fight
As a **speed-typer**, I want to see my win chance before I challenge, so that I pick fights I can win.

- Double-clicking a rival in reach opens the challenge card: "CHALLENGE? / A vs B / the big win %", one distance, DUEL or TELEPORT + DUEL; Enter = DUEL.
- The odds come from `game.duel_odds` (600 seeded trials), with a separate win % if I type nothing.

**Status: built.** Evidence: RB 29 "Duel odds"; Unreal f712d28.

### B4. Real moves from real words
As a **speed-typer**, I want my words to choose real moves and use what is actually there, so that clever lines beat lucky ones.

- Words pick a move class (strike, heavy, guard, parry, feint, evade, grapple, throw, invoke, command, taunt…) resolved by a range/initiative/stance matrix.
- I can use only items I hold or things on the board; naming what I don't have is a whiff ("you have no pistol", wit −2).
- A line can stage up to 4 steps ("hide behind that tree then pop up…") on one d20.

**Status: built** (state schema 21). Evidence: RB 21.4; DUEL_MOVES §5; BUILDLOG "duels that look like your main character".

### B5. A fair AI opponent
As a **speed-typer**, I want the AI to type its own visible lines with a handicap, so that beating it measures my typing.

- The AI's lines play as masked typing at 6-9 characters a second; it takes −5 against a human.
- Measured: a human typing good lines wins about 9 in 10 at 84 wpm, 2 in 3 at 60, 1 in 3 at 42.

**Status: built.** Evidence: RB 21.8.

### B6. The world stops for the duel
As a **speed-typer**, I want the whole world to freeze and watch my duel, so that the fight is the one thing on screen.

- Every client swoops to the fight; both portraits and health bars show; the narrator calls "CLASH!".
- The world freezing needs a room-wide hold in The Theatre that is not offered yet; until then the world keeps beating and the two duellists are locked in place.

**Status: partial.** Evidence: RB 21.1 ("Until the Theatre offers it, the world keeps beating").

---

## C. A fallen player

### C1. Plead for my demigods
As a **fallen player**, I want to plead to Death even when both my demigods are down, so that a wipe is not the end.

- A wiped player can still submit: a plea, questions about quests, and offerings; anything else is refused with what I can do.
- The ankh button fills "Death, bring back my demigods" when none stand.
- Death names its price (sparks plus years off max health); if paid or lent, the demigod rises at 40% health where it fell or at its shrine.

**Status: built.** Evidence: RB 25; Unreal f53ced8.

### C2. A way back when I can't pay
As a **fallen player**, I want a quest when I can't pay Death, so that I always have something to do.

- Unpayable raisings become a public quest (difficulty 2-3); the fallen get vigils and offerings as steps.
- A demigod raised by a quest owes Death the sparks as debt.
- My hint names the current quest step.

**Status: built.** Evidence: RB 25, RB 27, RB 29 rule 2.

### C3. Break a seal
As a **fallen player**, I want to know why my demigod can't rise and how to fix it, so that a seal is a puzzle, not a wall.

- A sealed demigod's answer says "a seal of no resurrection lies on it" and shows the counter-quest's steps.
- A seal's counter is public; anyone pleading for that demigod may take it up. When done, `curse.broken` is public.

**Status: built.** Evidence: RB 25 worked example; RB 26.

### C4. Limits I can see
As a **fallen player**, I want rising again to cost more each time, so that death still matters.

- Death's spark price is 5 × (1 + times raised) + 1 per 8 beats dead; each raising takes more years.
- At most 3 raisings per player per match.

**Status: built.** Evidence: RB 25; RB 22 settings `raise_*`.

### C5. Rewind as the last human
As a **fallen player** who is the only human left, I want to rewind and fork the match from an earlier beat, so that I can try again without an invented rule.

- A dead player who is the only active human may rewind the match to an earlier beat.
- The engine call (`room.rewind`) was requested from The Theatre and is not finished. Admin fork from any beat exists (see E3).

**Status: not built.** Evidence: BUILDLOG 2026-11-15 (the "mercy" rule cancelled in favour of time travel; `room.rewind` requested).

---

## D. Spectator or mid-match joiner

### D1. Join at the exact present
As a **joiner**, I want to see the exact current moment when I arrive, so that I can follow the match instantly.

- A viewer joining (even mid-duel) gets one snapshot with the beat, the open slow window, every demigod and every open duel with every blow.
- A reconnecting player gets the same; a server restart loses no accepted action.

**Status: built.** Evidence: TIME_TRAVEL §6-7; `fn7-server/tests/exact_resume.rs`.

### D2. Take over a bot's seat
As a **joiner**, I want to take over a bot's seat mid-match, so that I can play now instead of waiting.

- A human taking a bot's seat takes up its two demigods under the joiner's names (same refs), remembered as `seat.takeover`.

**Status: built.** Evidence: RB 15.

### D3. Watch the masked typing
As a **spectator**, I want to watch a duel's lines arrive, so that I can read the fight as it happens.

- The line grows as stars with only its newest letters showing, relayed live (median 20 ms on a private server).
- Keystrokes are never stored or replayed; only the line sent on Enter counts.

**Status: built.** Evidence: RB 21.7; CLIENT_API (relay measurements).

### D4. Be pointed at the action
As a **spectator** with nothing standing, I want to be pointed at the best thing to watch, so that I stay engaged.

- My hint becomes `watch`: an open duel, else the biggest public event of the last two beats, else Time's circle.
- Chat folds to an icon with an unread badge. *Chat history on join was requested from The Theatre and is not confirmed.*

**Status: partial.** Evidence: RB 29 rule 3; Unreal 3684b1b; BUILDLOG 2026-11-15 (chat history requested).

---

## E. A judge exploring the data

### E1. See that the dice are real
As a **judge**, I want every roll to be seeded and recorded, so that I can trust the LLM never overruled the dice.

- Every d20 is seeded by the match seed and its query and recorded; asking again returns the same face (a different answer is logged as a conflict).
- Every roll is public (`dice.rolled`); a die's public reason names only the act's class.

**Status: built.** Evidence: RB 2.5, RB 18; `fn7-plugin/src/memtree_tests.rs` (the d20 path reproduces the stored face).

### E2. Explore the tree of consequence
As a **judge**, I want to browse the DM's memory from the planet down to one hex, so that I can see how history bends what happens.

- `game.memory_tree` returns every node's fact counts, densities, roll-ups with the entries they cite, and recall paths into the DM's current prompt.
- TREE in the client header opens it as a star-map overlay.
- Private facts are counted, never shown; no health anywhere. About 2.3 ms median at beat 50.

**Status: built.** Evidence: BUILDLOG "the tree of consequence"; Unreal 6f8fa88.

### E3. Replay and fork any match
As a **judge**, I want to replay a recorded match exactly and fork it from any beat, so that I can check the world is deterministic.

- `fn7-sim --replay` recomputes every snapshot: 0 differences, LLM answers included.
- `GET /admin/matches/{id}/state?after_round=N` and `POST /admin/fork` give any past beat and a new room from it.
- *These are admin routes; how a judge gets access was a pending decision (a house player key with caps was recommended).*

**Status: partial** (built on the server; judge access not confirmed). Evidence: TIME_TRAVEL §6; BUILDLOG session log 2026-11-15 ("Decisions waiting").

### E4. Watch the Earth react
As a **judge**, I want to see a cast change the planet and the people on it, so that I can judge the living world.

- A cast starts a phenomenon on the voxel Earth; floods, fires, quakes and lava interact; a quake follows a real fault.
- `earth.effects[]` names what happened to which thing where (forests burnt, people fled to shelters, shelters destroyed, deaths).
- `game.people` and `game.buildings` show people with a reason for what they are doing, and dressed houses.

**Status: built** (server). Evidence: RB 28; EARTH_SIM §2, §13; BUILDLOG "people and houses".

### E5. See every generated asset and its cost
As a **judge**, I want to see every generated image and model with its prompt and cost, so that I can judge the AI's use.

- Every picture and Meshy model is an asset row with its file, exact prompt, timing, cost, and where and when it was shown.
- `GET /api/assets/at?match=&beat=&demigod=` answers what a demigod looked like at any beat.
- *A public, judge-facing view of the registry is not confirmed.*

**Status: partial.** Evidence: TIME_TRAVEL §1 layer 7, §6; `docs/ASSETS.md`; MEDIA "The asset registry".

---

## F. The designer / balancer

### F1. Every number is a setting
As the **designer**, I want every rule's number exposed as a validated room setting, so that I can tune without code.

- `GET /sim/settings` lists every setting with its type, default, range and description.
- DM, fault, duel, will, raise, curse and quest numbers are all settings (for example `duel_deadline_ms` 30000, `raise_cap` 3, `curse_price_sparks` 5).

**Status: built.** Evidence: RB 22; PROJECT_GUIDE §3.13.

### F2. Balance with simulations
As the **balancer**, I want seeded simulated matches, so that I can measure a rule before players feel it.

- Fault junction balance: 128 seeds × 60 beats, junctions off vs on, with seat-fairness χ² reported.
- Duel AI balance: 400 simulated duels per row give win rates by typing speed.
- An Earth balance harness exists for the voxel simulator.

**Status: built.** Evidence: RB 13.1 "Balance"; RB 21.8; EARTH_SIM §9 (`earth_balance.rs`).

### F3. Old matches never break
As the **designer**, I want every rule change gated on the state schema, so that older recordings still replay exactly.

- Each rules release is gated on a state schema (6 → 22); schema-gated settings recorded before a feature read back off.
- Recordings of older schemas replay with 0 differences.

**Status: built.** Evidence: RB intro, RB 18 rule 8; TIME_TRAVEL §7 rule 6.

### F4. The LLM can't break the rules
As the **designer**, I want rules to decide and the LLM only to phrase, so that the game stays fair and replayable.

- An LLM narrates a roll; it never overrules one or rolls again. Every LLM call has a rules fallback.
- Token scenes, duel deeds and quests are drawn after the ruling and decide nothing.

**Status: built.** Evidence: RB 0, RB 1, RB 21.4, RB 29 "Token play".

### F5. Wills that do real things
As the **designer**, I want a typed will to split into parts with real effects, so that "heal and arm my demigods" changes the game.

- A will splits into heal, bless, arm, raise, curse, ask, offer, gather; each part × target gets its own die, price and plain result.
- When a part falls short, the answer says why and what would work (a quest's steps or the price).
- Arming makes gear with real modifiers by slot, tier, material and blessing.

**Status: built** (rules 0.9.0, `wills_enabled`). Evidence: RB 23-24.

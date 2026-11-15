# Final Night 7: pitch deck

Epic AI MegaJam 2026, Unreal category. Theme: "Director's Cut". Team: Chris and Benito.

Every claim below traces to `BUILDLOG.md`, `README.md`, `docs/DM_RULEBOOK.md`, `docs/MEDIA.md`, `docs/TIME_TRAVEL.md`,
`docs/EARTH_SIM.md`, `docs/ACCOUNTS.md`, `docs/PROJECT_GUIDE.md` (this repo) or the Unreal repo's `docs/PACKAGING.md`,
`docs/MESHY.md`, `production_assets/audio/vo/README.md`. Images are in `images/`. Items marked **TO CONFIRM** need
Chris or Benito before the deck is shown.

---

## 1. Final Night 7

**Image:** `images/01_title_table.jpg`

- A multiplayer god game on the real Earth
- Gods at one table. Demigods on the board. Last one standing wins.
- Unreal 5.8 client, Rust server, on The Theatre engine

> Speaker notes: We're Chris and Benito. Final Night 7 is a god game where you play by typing. Everything you see here
> is decided live by the server's rules, its dice and an AI Dungeon Master.

---

## 2. Type faster than the gods

**Image:** `images/02_your_will.jpg`

**Your words become history.**

> Speaker notes: The designer's day-one brief was that input should be as fast as typing, yelling or speaking. You write
> what your demigods do. The world takes it in, rolls for it and remembers it.

---

## 3. Your first 30 seconds

**Image:** `images/03_the_table.jpg`

- Click YOUR WILL and write what your Left or Right Hand does
- Time slows for everyone while you write; Enter sends it
- Drop a power on the globe (mega) or the board (mini)
- Double-click a rival nearby: duel

> Speaker notes: That's the whole loop. A match runs about 7 minutes (Benito's target). The lobby line states the same idea: write
> what you do, time slows while you write. Last one standing wins."

---

## 4. The table

**Image:** `images/13_final_beat.jpg`

- Each god's seat holds a stone statue
- Your two demigods: the Left Hand and the Right Hand
- A voxel globe of the whole planet above a hex board you zoom into
- Seven gods rule the world: Time, Nature, Consequence, Death, Chance, Memory, Entropy

> Speaker notes: Benito's two styles: the gods' room is realistic stone and candlelight; the board and the earth are the
> gods' toys. You zoom from the planet to regions, villages, houses and single people.

---

## 5. Wills and the slow-time cut-off

**Image:** `images/02_your_will.jpg`

- Start typing: time slows for every player
- Rivals see who slowed time, never whether they are typing too
- The first Enter closes the window: later wills are cut, privately
- Time sends the bill: a bolt, a debt, more Entropy

> Speaker notes: This is the "hidden meddling" from the first build-log entry. Press Enter too fast and nobody else gets
> in; wait too long and you pay Time more. A will can heal, bless, arm, curse or gather.

---

## 6. Mega, mini, and a living earth

**Image:** `images/04_powers_fault_lines.jpg`

- 11 powers, each as a mega (a bolt) or a mini (a spark): 1 to 5
- Megas hit map-wide, 600 to 2,500 km, harder on real fault lines
- A server-side voxel earth: storms, floods, volcanoes, rising land
- Every spend raises Entropy; at 100, heat death ends the night

> Speaker notes: The powers on the rack: lightning bolt, storm clouds, volcano, earthquake, flood, wildfire, meteor,
> raise land, sink land, grow forest, swarm. It's Populous-style: the land changes, crowds flee to shelters, and what
> the earth did feeds every prompt for that place. The fault lines are real plate boundaries (PB2002).

---

## 7. The AI Dungeon Master

**Image:** `images/05_living_earth_event.jpg`

- "History is a lens": a place's past bends what can happen there
- The rules and one d20 have already decided; the DM never overrules them
- The DM makes the answer make mystical, Rube Goldberg sense

> Speaker notes: From the rulebook: not "it failed" but "the bridge held only because the ferryman had washed the
> keystone in platinum dust at dawn". A natural 1 on a god-blessed strike still misses. The same question always gets
> the same roll.

---

## 8. Duels: will against will

**Image:** `images/07_challenge_card.jpg` (then `images/08_clash_box_concept.jpg`)

- Double-click a rival in reach: the challenge card shows your odds
- Every line you type is a blow, played the moment you press Enter
- Use only what you hold, or what's on the board: a rock, a tree, dirt
- Name a pistol you don't have: you whiff

> Speaker notes: Inspired by Archon (1983): the board is strategy, the fight is real time, and here real time means
> typing. "Roll left", Enter, "jump", Enter. A duel lasts up to 30 seconds and everyone watches.

---

## 9. Fighters on a toy stage

**Image:** `images/09_mannequin_skins.jpg`

- A wooden figure-drawing mannequin, rigged to the UE5 skeleton
- Five skins: wood, obsidian, gold, dragonscale, butterfly
- Fight clips picked from your words by a small AI picker

> Speaker notes: Made with Meshy (rigging and animation). A Claude Haiku call per typed duel line picks the move from a
> fixed list; the d20 and the rules decide the hit. Every pick is recorded, so a duel replays with no model call.

---

## 10. Your god form, in stone

**Image:** `images/10_marble_gods.jpg` (and `images/11_heroic_demigods.jpg`)

- Describe your god in your own words
- Pick a stone and a pose: The Thinker, David, Atlas...
- Accept the preview: Meshy turns it into a 3D statue at your seat

> Speaker notes: The house statues were made the same way: 11 marble gods and heroic statues of the players' demigods.
> A moderation pass checks the words first. One statue per account.

---

## 11. The tree of consequence

**Image:** `images/06_demigod_diorama.jpg` **TO CONFIRM:** swap for a screenshot of the TREE overlay if one exists

- Press TREE: the DM's memory as the exact tree it is
- World → region → settlement → village → house → person
- Every fact counted; recall paths show a d20 replayed from its seed
- Private facts are counted, never shown

> Speaker notes: Chris asked for this for judges who ask "what's the data structure like". It's read-only and live, at
> any beat.

---

## 12. Time travel

**Image:** `images/13_final_beat.jpg` **TO CONFIRM:** a second shot would help here

- Every match is recorded: every event, snapshot and AI call
- Replays match exactly: 0 differences on the recordings we checked
- Fork any moment into a new match; reconnect or join at the exact present
- The earth, the duels and the pictures replay too

> Speaker notes: The house game runs an attract replay of a recorded match, and a joiner forks it. Replays answer every
> AI call from the recording, so no model is needed.

---

## 13. The Director's Cut

**Image:** `images/12_portrait.jpg`

- Every match ends in a storyboard: the final-cut animatic
- 12 shots: a title, up to 10 big moments, the champion's close-up
- Replay the cut, then fork any beat and shoot it again

> Speaker notes: Your vision is your words, your edits are your wills and powers, and the match is your final cut. The
> storyboard is built from the recorded match; video clips per shot were left for later.

---

## 14. How it's built

**Image:** `images/01_title_table.jpg`

- The Theatre: our multiplayer engine where players act in words (Rust)
- Final Night 7: a Theatre plugin plus its server; rules first, LLM second
- No AI? Every mind falls back to rules. Keys stay on the server.
- Unreal 5.8 client in C++; images, 3D statues, voiceovers

> Speaker notes: Game rules never touch I/O and AI minds never change state directly; they propose, the rules decide.
> All randomness comes from the match seed, which is why replays are exact.

---

## 15. AI tools we used

**Image:** `images/11_heroic_demigods.jpg`

- Claude (`claude-haiku-5-5`): the game master, the gods, the narrator, the duel picker, moderation
- OpenAI `gpt-image-1-mini`: portraits, dioramas, moment cards, storyboard, god previews
- Meshy: 3D statues, the mannequin, skins and fight clips
- Fish Audio (Chris's TTS pipeline): the voiceovers
- Claude Code with Opus 5.5 agents: development

> Speaker notes: Every generated picture and model is logged with its exact prompt and cost in one asset registry.

---

## 16. The team

**Image:** `images/03_the_table.jpg`

- **Chris**: design and direction, the server and engine side
- **Benito**: the Unreal client, style, the duel and event design
- Built during the jam; prior work declared below

> Speaker notes: **TO CONFIRM:** each person's credits and the team name. Prior work, declared: the 2024 Final Night
> prototype (Node backend, Unity client) and Final Night 2.5 (the earlier Theatre plugin), plus pre-jam prompts and
> scripts. **TO CONFIRM:** how much of The Theatre engine existed before Nov 12.

---

## 17. Close

**Image:** `images/01_title_table.jpg`

**Seven nights. Seven wars. Gods at one table, and the whole world for a board.**

> Speaker notes: That's the line from our intro voiceover. Thank you.

---

## Image list

| File | Source |
|---|---|
| `01_title_table.jpg` | Unreal `production_assets/beta/controls_ca97900/30_controls - double-click your card.png` (chat panel and taskbar cropped off) |
| `02_your_will.jpg` | Unreal `docs/proof/mac_two_demigods_will.png` |
| `03_the_table.jpg` | Unreal `docs/proof/mac_gods_dioramas.png` |
| `04_powers_fault_lines.jpg` | Unreal `docs/proof/mac_faults_and_terrace_aoe.png` |
| `05_living_earth_event.jpg` | `final_night_7/data/style_refs/approved/maxzoom_event.png` (approved style reference) |
| `06_demigod_diorama.jpg` | `final_night_7/data/style_refs/approved/maxzoom_diorama.png` (approved style reference) |
| `07_challenge_card.jpg` | Unreal `production_assets/beta/controls_ca97900/33_controls - double-click rival on board, CHALLENGE card.png` (cropped) |
| `08_clash_box_concept.jpg` | Unreal `docs/design/challenge_box_concept_v1.png` (concept art, not in-game) |
| `09_mannequin_skins.jpg` | Unreal `docs/design/mannequin_skins.png` |
| `10_marble_gods.jpg` | Unreal `production_assets/meshy/figures/ai_gods_sheet.png` |
| `11_heroic_demigods.jpg` | Unreal `production_assets/meshy/figures/heroic_pair.png` |
| `12_portrait.jpg` | `final_night_7/data/style_refs/approved/portrait.png` (approved style reference) |
| `13_final_beat.jpg` | Unreal `docs/proof/portraits_live.png` |

## TO CONFIRM

1. Team name and per-person credits (slide 16).
2. How much of The Theatre engine predates the jam (Nov 12); PROJECT_GUIDE and PACKAGING declare only the 2024 Final
   Night prototype and Final Night 2.5.
3. Whether the voiceovers are in the shipped build (BUILDLOG lists mac/voiceovers as in flight at the last log).
4. Player rewind for a lone fallen player (BUILDLOG: branch backend/death-mercy, retargeted to the rewind) is not
   claimed as shipped; the deck claims only recording, exact replay, fork and resume.
5. A screenshot of the TREE overlay (slide 11) and of the storyboard / end screen (slide 13), if they exist.
6. Duels: rules schema 21 allows only held items, board objects and the ground; the earlier "hammer-space" draws
   (anything else as an item roll) are off for new matches. The deck says "only what you hold or what's on the board".
7. That the style-reference images (05, 06, 12) were made with gpt-image-1-mini (the docs show them as approved style
   references, not their generator).

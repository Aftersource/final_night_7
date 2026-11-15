# Final Night 7: the full explainer for production agents

*Designers: **Chris** and her brother **Benito**. Epic AI MegaJam 2026, theme "Director's Cut". Unreal 5.8 client
(Win64 and Mac) on a Rust server built on **The Theatre**, a server-authoritative engine for games where players act
in plain language and an LLM decides what happens.*

Read this first, whatever your job is. Then read your own brief in `briefs/`. This page assumes you know nothing about
the game. Every rule, number and message name here comes from the sources at the bottom; anything not built or not
confirmed is marked so. If you need a rule that is not here, do not invent it: ask the backend.

Short references: **RB n** = `docs/DM_RULEBOOK.md` chapter n; **API** = `docs/CLIENT_API.md`; **GD** =
`docs/presentation/design/GAME_DESIGN.md`; **HoG** = the Unreal repo's `docs/design/hand_of_god/HAND_OF_GOD.md`;
**EiW** = the Unreal repo's `docs/design/EVENTS_IN_THE_WORLD.md`.

---

## 0. The game in one minute

Seven gods sit at one table over a floating globe of the real Earth. Each player is a god with **two demigods** on the
board, the **Left Hand** and the **Right Hand**. You play with two things only:

1. **The words you type** (a *will*): typing slows time for everyone, and your Enter cuts off anyone slower.
2. **The lightning you throw** (a *power*): bolts shake the map, sparks hit one spot.

An **AI Dungeon Master** (the server: rules code plus LLM minds) runs the world on a fixed beat and **never waits for
anyone**. When two demigods meet, a double-click cuts to a **30-second typed duel**. The world remembers every
decision by place and time; the simulated Earth floods, burns and heals; and the **last god with a demigod standing**
wins the **Final Night**.

The theme, "Director's Cut": the player directs history in words, Time controls the clock and the cut-off, a duel cuts
from the strategy board to an arena, and any match can be scrubbed, replayed or forked into a new cut (GD one page).

**Three ideas govern everything (RB 0):**

1. **History is a lens.** A place's recorded history bends what is possible there.
2. **The rules and dice are an engine that has already decided.** The DM never overrules them: a natural 1 on a
   god-blessed strike still misses.
3. **The DM makes the answer make mystical sense**, Rube Goldberg style ("the bridge held only because the ferryman
   had washed the keystone in platinum dust at dawn").

**The DM's four laws** (RB 1): the dice decide, you describe; the DM remembers; places nest; describe the world as the
rules left it.

---

## 1. Premise and the cast

| Element | What it is |
|---|---|
| The table | A god's room: a floating globe over a board. The selected part of the planet is projected onto the table as a "circle of action" |
| The gods at the table | Each player is a god, seated as a stone statue on a pedestal with a name plate in their aura colour |
| Two demigods each | Left Hand and Right Hand: two bodies, each with its own health (10), position, items, story and scars. A player is out only when both fall. Bolts and sparks are one purse per player; debts are per body (RB 15) |
| The pantheon | Seven god minds rule on anything divine: Time, Nature, Consequence, Death, Chance, Memory, Entropy (RB 12.1) |
| The world | The real Earth: regions, cities, crowds, armies and named people, simulated every beat |
| The Dungeon Master | The server: its rules plus its LLM minds (the game master, the gods, the narrator) |
| The built-in AI | Demigods no seat plays are played by the server's own AI; it casts, wills, challenges and types in duels |

**Two deliberate styles (Benito's rule):** the god's room is realistic; the board and the Earth are the gods' toys
(BUILDLOG 2026-11-15).

Names on the wire: a demigod is `"<player>'s Left Hand"` / `"<player>'s Right Hand"`. Refs are strings like `dg:0`
(demigod), `ch:12` (a named person), `cl:7` (a crowd or army), `pl:41` (a place), `it:3` (item), `gd:N` (god). Gods
are numbered `gd:0` Time, `gd:1` Nature, `gd:2` Consequence, `gd:3` Death, `gd:4` Chance, `gd:5` Memory, `gd:6`
Entropy (API "Conventions"). Positions are `[longitude, latitude]`, longitude first.

---

## 2. The screen and every control

### 2.1 What is on screen

- **The globe** floating over **the table board** (hex tiles). Zoom levels on the board: `regions`, `villages`,
  `houses`, `people` (a 3 m hex at people level).
- **The god statues** at each seat, with name plates.
- **The demigod cards** (portrait = piece: a card and its table piece are the same thing), each with an **ITEMS**
  button (Unreal f68312d).
- **The WILL box** (the text entry) with **four carved hot buttons** (section 4.2).
- **The power ring / palette**: mega and mini powers and each god's god-tier power.
- **The next-move hint**: one big line at top centre with an arrow at its target, a smaller "why" under it, and the
  goal line in a corner (API "Next move hint").
- **The table d20**: every public roll tumbles on the table and says whose roll it is and what for (Unreal 60abcfe).
- **The world log / feed**: the DM's one-line decisions (`dm_events[].text`), cast lines (`text`, never
  coordinates), the narrator's caption.
- **Chat**: folds to a speech-bubble icon with an unread badge; a click opens it (Unreal 3684b1b).
- **TREE** in the header: the tree of consequence as a star-map overlay (Unreal 6f8fa88).
- **Volumes** per account: master, music, sfx, voice.

### 2.2 The locked controls (HoG §6, locked by Chris 2026-11-15)

The game uses the **system mouse pointer** (the drawn hand cursor was removed). These rules are locked; every change
to input, pan, zoom or the board rebuild path needs a 60-second live drag-and-zoom test in Chris's editor before push.

1. The land slides under the hand for the whole drag. The board re-centres seamlessly every 1.5 hexes and once on
   release. A re-centre must never move the land under the hand.
2. While panning, the relief keeps its scale and the board stays on its cube face.
3. Nothing else touches the board under a moving hand: no earth playback, no rebuild storms, no coast or fling on the
   board.
4. The system mouse pointer; no drawn cursor.
5. The land under the pointer stays under the pointer: no camera motion during a grab; the pressed hex stays within
   3 px of the pointer.

**A click on the globe goes where you click:** a press that moves under 20 px (1080p-scaled) within 0.6 s is a click:
it runs the globe pick (dive to that region).

| Input | Empty globe or board | Inside a demigod's extended zone (its click zone x1.35) | Over UI |
|---|---|---|---|
| Press + drag | Grab and pan | Your own demigod: SUMMON drag. A rival: nothing | The UI's own drag |
| Click | Nothing moves (the globe: dive to that region) | Select | The UI |
| Double-click | Zoom in one level at that point, animated | (see the demigod rules below) | The UI |
| Right-click | Zoom out one level, animated | Zoom out one level (never eaten by a demigod) | The UI |
| Wheel | One level per gesture at the cursor | Same | The UI scrolls |

All zooms animate; nothing pops.

### 2.3 Today's demigod double-click rules (BUILDLOG session log 2026-11-15; Unreal f68312d)

Chris's design after the control regression: **portrait = piece.**

| Gesture | Result |
|---|---|
| Double-click your own demigod (card or piece) | The view glides to centre it **at the same zoom**, with a gold ring pulse |
| Double-click a rival in reach | The **challenge card** with the odds (section 8) |
| Double-click a far rival | Centres it |
| ITEMS on a card (or the statue's shelf) | Opens that demigod's items. **Nothing else takes a click on a demigod** |
| Drag your own demigod onto the board or globe | Summons it there this beat (risk: up to 2 health) |

Status: this was the open P1 at the 17:00 EST session log (a regression since an integration merge); the fix was in
flight on the Mac (`mac/fix-dblclick`). Treat the table above as the rule; check the Unreal repo for the fix's state.

---

## 3. The beat: the world never waits

**One beat = one Theatre round.** Length `flow_ms`, default **3 s** (the shared lobby uses 12 s). The Final Night falls
on beat `beats_total`, default **60**. Every LLM call has a rules fallback, and a late call takes it: "The beat never
waits." (RB 1, 5)

**A beat, in order** (RB 17): Start (Time's clock, due debts queued) → Brief → Compose → Resolve (Time's bills → casts
→ hands → mentions → route → one batch of minds → merge and world rules → the society → occupancy → the DM's step →
consequence and memory) → Reveal → Commit.

On the wire (API "Message flow for one beat"): `phase.changed {start}` → `round.started` → `phase.changed {brief}` →
`round.brief` (public brief + your private seat view) → `phase.changed {compose}` → (actions at any time) →
`phase.changed {resolve}` → `phase.changed {reveal}` → `round.outcome` (public) → `round.private` (only you) →
`phase.changed {commit}` → `round.committed` (public view) → `round.private_state` (your seat view) → next beat.
The match ends with `match.ended`.

- **Days and nights.** A game day is 8 beats: dawn, day, day, day, dusk, night, night, night. Night brings undead,
  ghosts and thieves, and makes monsters and traps harder (RB 5). Event: `time.phase` ("Dawn breaks on day 1").
- **Time's circle.** The safe zone shrinks around a convergence city from the whole globe to 1,500 km by beat 160
  (`arena_close_beats`); bodies outside take 0.5-3 damage a beat and are dragged in (RB 14). Shown as
  `arena_radius_km`.
- **The Final Wish.** Beat 1: every player's orders are their final wish: more acts, +2 magnitude on each, +2 on its
  d20s (RB 17). Flag: `round.brief.public.final_wish: true`.
- **The narration is one beat late.** The Narrator writes beat N's caption during beat N+1; read
  `previous_caption` / `previous_callouts` and `caption_beat` (API `round.outcome.public`). The API describes the
  Narrator as "Deadpool's voice".
- **The next move.** Every beat each human seat gets one private `hint` and a `goal` line ("Last standing wins. You:
  2 alive, 3 rivals. Beat 41 of 60."). Rules pick it, never an LLM, in priority order (RB 29): in a duel (type now),
  about to fall, the fallen, spectators, a rival in your hex, a world event near you, a mega at a junction, no gear, a
  quest step, a will. `urgency` 0-3; `kind` one of `duel`, `danger`, `own_hit`, `resurrect`, `break_curse`, `watch`,
  `challenge`, `event`, `mega`, `gear`, `quest`, `will`. Schema 22 ("clear words"): plain words ("right here", not
  "0 km away"), low-urgency hints rotate every 3 beats, and a cast that hit your own demigod is warned about the beat
  after (`own_hit`).

---

## 4. The will: typed orders that slow time

The will is the text box. **Text entry is the God of Time's interaction.** Typed text never casts a power, and casts
never carry orders (API "The action").

| Rule | How it works |
|---|---|
| Opening a slow window | Starting to type sends `action.begin`. If your demigod stands and you hold a bolt, it opens **one slow-time window per room**, up to `slow_ms` (30 s). The world keeps resolving |
| Who sees what | Everyone sees who opened the window (`window.opened` + `phase.event` `slowed`, "<name> slowed time"). **Nobody learns who else is typing** |
| The cut-off | The opener's Enter closes the window (`window.closed {reason: "submitted"}`). Anyone who submits after gets a private `action.received {accepted: false, reason: "cut"}`. If the opener never presses Enter, it closes at the deadline (`reason: "deadline"`) |
| Time's bill | At the next resolve: Time takes **1 bolt** as toll (no gift to rivals) and writes a debt of `0.5 x (1 + seconds/10) x (1 + 0.5 x slows in the last 20 beats)` (doubled if the bolt can't be paid, cap 10); Entropy rises `0.5 + seconds/30`. While the window is open, the client shows the opener's bill growing (`slowed` carries `bill_now`, `debt_per_second`, `debt_cap`...) |
| Instant orders | Orders without a window pay the same toll and Entropy and a flat 0.5 debt (`round.outcome.public.slow[]` `event: "instant"`) |
| Your hands | The Left and Right Hands are LLM orchestrators: they break the will into acts, spawn subagents, possess people, raise armies and invoke gods. Each keeps its last two ordinary acts as standing orders |
| No intent vanishes | Every order gets a visible ruling or a refusal (RB 21.6) |

### 4.1 Wills that matter (rules 0.9.0, RB 23-27)

A will is split into its **parts**: `heal`, `bless`, `arm`, `raise`, `curse` (incl. seal), `ask`, `offer`, `gather`.
Each part x each target is ruled on its own, with its own d20, price and plain-words result, and when it falls short,
**why** and **what would work** (a mystic quest's steps, or the price). "Every one of my demigods is fully healed and
gets rocket launchers" is four rulings, not one line of narration. The answer arrives privately in
`round.private.wills[]` with `verdict`: `granted`, `partial` or `refused` (and per part an `outcome`).

**Paying** (RB 23.2): prices are in sparks. The purse pays sparks first; then a bolt breaks into 5 sparks. What the
purse can't pay, the ruling god lends (0.2 debt per spark) while the demigod's open debt stays under 3; past that the
will becomes a mystic quest. Entropy rises 0.25 per spark paid.

### 4.2 The four will hot buttons

Four carved icons on the will: an ankh, a notched bow, a delicious meal, and a scene of armies. A press puts a
ready-made line in the box, counts down **5 seconds** and sends it as Enter would. Typing in the box stops the
auto-send ("type to make it yours"); another press replaces the line and restarts the count (Unreal a89c986).

| Button | Line it fills (live names fill in) | Rule |
|---|---|---|
| **Ankh** | "Death, bring back my demigods" or "Death, bring back my Left Hand, I offer 2 bolts" | Resurrection (RB 25) |
| **Bow** | "arm yourself with an obsidian blade in Vienna and hunt Vex's Right Hand" | Arming (RB 24) plus a hostile act |
| **Meal** | "Nature, heal my Right Hand" | Healing your own (RB 23.3) |
| **Armies** | "raise an army of 10,000 fishermen at the harbour of Vienna" | Armies (RB 14) |

---

## 5. Powers: mega and mini

Pick a power and throw it anywhere on the globe. A cast is an **instant action** (`action.submit {instant: true,
fields: {cast}}`), shown to everyone at once as a **streak** (`phase.event` `streak`) and paid at the next resolve.

### 5.1 Bolts and sparks: the 1:5 economy

Each player starts with **1 lightning bolt** (mega) and **5 sparks** (mini). One bolt breaks into 5 sparks when a will
needs paying.

| | Mega (a bolt) | Mini (a spark) |
|---|---|---|
| Scale | map: radius x4 within 600-2,500 km | board: radius / 20 within 3-30 km |
| Strength | magnitude +2, damage x1.5; 40% of it to every rival demigod in the radius | magnitude -3, damage x0.6, its target only |
| Gift | every other player gains 1 bolt (`bolt_gift`) | none |
| Entropy | +10 | +1 |
| Fault lines | scaled by the nearest fault line and junction | unaffected |

The 11 powers: lightning, storm clouds, volcano, earthquake, flood, wildfire, meteor, raise land, sink land, grow
forest, swarm (EARTH_SIM §2), plus `custom` ("a rain of frogs over the stadium", magnitude 0-10). Each cast gets
**Entropy's check, which always says yes** (an omen in Entropy's voice), and an LLM writes **ramifications**:
`1 + cost + magnitude/3` geotagged rows per cast (at most 10) and lasting world mods. A cast its caster can no longer
pay **fizzles** (nothing spent). A cast that covers your own demigod carries `own_hit` (schema 22).

### 5.2 Fault lines and junctions (RB 13)

A mega's power depends on where it lands relative to real plate boundaries (PB2002). Chris: megas should be "crazy
powerful at huge intersections and only 'tiny volcanos' near the no-faultline areas."

| Tier | Where | Effect |
|---|---|---|
| `junction` | within 300 km of one of ~97 plate junctions | up to magnitude +3, radius x1.5, damage x1.8 more, on top of the fault curve |
| `fault` | within 200 km of a boundary | magnitude +5, radius x2.2, damage x2.5 |
| `near` / `mild` | the curve between | linear |
| `tiny` | 1,000 km or more from any boundary | magnitude -4, radius x0.4, damage x0.25 |

Worked example (RB 13.1): a mega volcano on the Afar triple junction lands at damage 11.8, radius 1,789 km; the same
cast in central Australia: damage 0.8, radius 240 km. The client shows the fault lines only while a mega is armed;
`game.fault` previews the tier under the cursor.

### 5.3 God-tier powers (RB 20)

Each god has one signature power on the ring. It costs **3 bolts**, gifts every other standing player 1 bolt, adds
mega-level Entropy, and is that god's ruling: the god may grant, bargain, twist or refuse. Accept push: `phase.event`
`god_power`; at the resolve: `round.outcome.public.god_powers[]` and the god's speech in `omens[]`. Table in section 7.

### 5.4 Entropy and heat death (RB 12.4)

Entropy runs 0-100. Thresholds: **20** freak weather, **40** unreliable physics (a third of casts land 50-400 km off
target, `off_target: true`), **60** riots and worship, **80** wounds to the gods, **100 heat death: that beat becomes
the Final Night at once.** On the simulated Earth, Entropy quickens every spread, stresses every fault until the world
starts its own quakes and eruptions, and at heat death makes everything flare (RB 28). Time can cool Entropy for its
price. Wire: `round.outcome.public.entropy {value, max, level, key, text, next_at, heat_death, delta}`;
`match.ended.summary` says the beat heat death struck, or null. Measured: four seats casting at the client's maximum
rate reach heat death at the 2nd-3rd resolve (BALANCE_REPORT_2).

---

## 6. The AI Dungeon Master and the d20 table

> "history and its consequences are like lenses ... try to get it making mystical sense to fit with the world, but a
> lot rube-goldberg mystical. like needing to wash a stone in platinum dust" (Chris, RB 0)

### 6.1 The d20 table (RB 2.1)

| Situation | DC | Modifier |
|---|---|---|
| A demigod's ordinary act | 8 + magnitude/2 | the demigod's; +2 on the Final Wish |
| Manifesting an item | from the tier's odds | the odds' other terms |
| Stepping on an encounter | by type (10-13) | the demigod's; -1 monsters and traps at night; +1 near its own shrine |
| A fight's loser keeps its gear | 11 | the demigod's |
| A summoning's mishap | `round(20 x p) + 1` | none |
| A duel blow | 10, +1 at night, + half the opponent's wardrobe stage | the line's wit and the duellist's |
| A will on your own (heal, bless, arm) | 6 + magnitude/4 (arming: 6 + 2 per tier) | named in the answer |
| A curse | 10 + half the target's wardrobe | the caster's |

- A natural 1 is always `crit_fail`; a natural 20 always `crit_success`; otherwise total >= DC succeeds.
- The demigod's modifier: +1 hale (health 7+), -1 badly hurt (3 or less), plus half its wardrobe stage.
- **One question, one roll, forever:** every roll is seeded by the match seed and its query, and recorded.
- **Every roll is public**: `round.outcome.public.dice[]` (`dice.rolled`, at most 60 a beat). `reason` names only the
  act's class ("a strike", "a journey", "a trap encounter", "manifesting a relic"), never the order's words.
- Every other public DM decision: `round.outcome.public.dm_events[]` (at most 40 a beat), each with a one-line `text`.

---

## 7. The seven gods

When a demigod reaches into a god's domain, that god's mind **grants, refuses, bargains or twists**, and names a price.
An invocation costs `1 + magnitude/5` bolts. Precedence settles clashes on the same target (higher wins; Chance's
seeded coin breaks ties). Personas are quoted from `data/pantheon.json`.

| God | Domain | Persona (pantheon.json) | Prec. | God-tier power |
|---|---|---|---|---|
| **Time** | slowing, hastening, rewinding, travelling through time; the arena clock; owns the will's slow window | "A punctual, bone-dry bureaucrat of eternity who has never once been late and resents everyone who tries to be. Speaks in schedules, appointments and overdue notices." | 50 | **Time Stands Still**: a unit target is rewound 2 beats; at a point, rivals within 1,500 km are slowed a beat |
| **Death** | raising the dead, killing at a distance, souls | "Tired, fair, darkly funny. Has seen every trick. Respects style." (RB 25: "Death keeps a price list") | 45 | **The Pale Verdict**: a fallen target is raised; a standing one is struck for 3 |
| **Consequence** | debts: every act's bill, collected later as Final Destination-style accident chains | "A cheerful, meticulous collector with an infinite ledger and a love of Rube Goldberg machines. Never angry. Always exact." | 40 | **The Reckoning**: the target's biggest open debt is collected now |
| **Entropy** | chaos, overload, infinite magic power; the meter and heat death. Charges nothing, always says yes, gives 2 bolts, raises the meter | "A giggling, sparking, delighted maniac of heat death: the only god who WANTS you to use power. Talks fast, laughs mid-sentence (ahahhaahhaaa), says yes yes YES, begs everyone to spend more bolts, and treats every catastrophe as a gift." | 35 | **The Chaos Burst**: Entropy +15 |
| **Nature** | weather, beasts, plants, terrain, physics; rules heals | "Patient, enormous, amused and occasionally furious. Treats demigods as weather treats picnics." | 30 | **The Wild Surge**: 1.2 damage to rivals within 400 km |
| **Memory** | forgetting, false memories, learning what a crowd knows | "Elegant, gossipy, keeps everyone's secrets and sells them." | 20 | **The Great Forgetting**: every crowd in the target's region forgets its allegiances |
| **Chance** | luck, tricks, coin flips, the odds | "A grinning trickster who loves an underdog and hates a sure thing." | 10 | **The Coin-Flip Storm**: every demigod within 600 km heals 1.5 or takes 1.5 on a seeded coin |

**Consequence's bill** (RB 14): each landed act adds debt, due in 50 beats; due debts are collected as accidents. You
can win through a rival's own unpaid debts. Each god also has a paladin it may send (e.g. Time's "Warden of the Hour",
Entropy's "Herald of the Heat Death"; pantheon.json).

---

## 8. Items

**Making things** (RB 16): an order to manifest something is judged by rules; an LLM only names it. Tiers by size:
trinket (<= 12) 0 bolts, artifact 0, relic 1, wonder 1, legend 2, myth 3, titan 4, apocalypse 6; base odds 0.97 down
to 0.14. A shortfall becomes debt to Consequence. A host of 100+ takes the field as an army; a made structure is also
a shrine.

**Arming** (RB 24): gear for each standing demigod, a d20 against DC 6 / 8 / 10 by tier (trinket free, artifact 2
sparks, relic 5). A god named in the will, or blessing words, costs 5 more sparks per demigod and makes the gear a tier
up with that god's signature.

| Slot | What it adds (artifact → wonder+) | Once a match (relic+) |
|---|---|---|
| Weapon | +1 to +2 on strikes, +0.5 to +1 damage, +1 to +2 in duels | **Smite**: first landed strike on a rival deals 2 more |
| Armour | turns aside 0.5 to 1.0 per hit; resists its material's type | **Last Stand**: the first felling blow leaves it at 1 health (`arms.power`) |
| Charm | +1 to +2 on other deeds, 0.1 to 0.2 mending a beat | **Second Wind**: heals 2 the first time a beat ends at 3 health or less |

Materials matter (obsidian and dragonscale resist fire; platinum makes its bearer harder to curse; butterfly-wing and
heron-feather help journeys). A kit is capped (+3 on a d20, +1.5 damage, +3 in duels).

**Finding and losing:** encounters drop items; a death drops everything on its hex; a fight's loser may drop its
newest item; the next demigod to pass takes the pile (`loot.dropped` / `loot.taken`). Items are never lost.

**Seeing them:** gear shows on the demigods (`look.changed`: "NEW: <item>" on the card, new gear portrait and diorama
when rendered). Each card's **ITEMS** button opens its items.

---

## 9. Duels: challenge mode, will against will

> "everyone on the map needs to stop what they're doing and watch: the Archon-level fight, will against will"
> (Chris, RB 21)

Inspired by Archon (1983): the board is strategy; when two pieces meet, the game cuts to an arena where the real-time
play is **typing**.

| Step | How it works | Wire |
|---|---|---|
| Challenge | Double-click a rival in reach: the challenge card "CHALLENGE? / A vs B / the win %", one distance, and DUEL or TELEPORT + DUEL. Enter = DUEL | `action.submit {instant, fields: {challenge: {demigod, target, level}}}`; odds from `game.duel_odds` |
| Reach | The board cards touch at your zoom (600 km at regions), or within 120 km at any zoom. **No consent**: a challenge is forced | refusals: `too_far:<km>:<allowed>`, `busy`, `busy:player`, `cooldown`, `fallen`, `not_yours`, `bad_target`, `challenge_disabled` |
| Teleport | A summon first costs a mishap roll (up to 2 health), decided before the jump, shown as a red pulse | `summoning` / `summoned` |
| The lock | Everyone sees both portraits slam in, both health bars (**the one place a rival's health is shown**), and every client swoops to the fight | `phase.event` `challenge.lock` (the AI's: `round.started.public.challenges[]`) |
| The stage | The duel's stage is built from the place: a wooden box opens on the table and two mannequins fight in comic panels | `duel.scene` (once), `duel.panel`, `fields.duel_tick` |
| Blows | Each typed line (up to 120 characters) is a blow: d20 vs DC 10 (+1 at night, + half the opponent's wardrobe), resolved at once by rules | send `fields.duel {duel_id, text}`; everyone gets `duel.blow` |
| Wit (-3..+4) | +1 each: an action verb, a second verb, a body part, a held item, a terrain or place word, the opponent's story or scars. Repeats, very short lines and gibberish cost | `duel.blow.wit`, `tags` |
| Damage | 0 on a fail; 1 on a success, +1 beating the DC by 5, +1 with wit 3+; 4 on a natural 20 | `duel.blow.damage`, `hp` |
| Spam | Two blows inside 0.5 s: the second is a penalty and costs its author 0.5 | |
| Moves | Words pick a move class: strike, heavy, guard, parry, feint, evade, close, retreat, grapple, throw, taunt, invoke a god, command an army | DUEL_MOVES |
| Only what is real | A line can use only items you hold or things on the board (a tree, a rock, the mud). Name a pistol you don't have: a **whiff**, "you have no pistol", wit -2 | `duel.blow.whiff` |
| Staged steps | "hide behind that tree then pop up and slit his throat" plays as up to 4 staged steps on one d20 | |
| Watching | Others see the line grow as stars with only the newest letters showing, like a password field | `duel.typing` |
| The end | A knockout, the 30 s deadline, or a duellist falling elsewhere. The loser's loot falls on the hex; both get the duel's deed as a tale | `challenge.end` in `round.outcome.public.duels[]` (`reason`: `ko`, `deadline`, `fallen`, `auto`; `winner`/`loser` null on a draw) |

**Odds:** `game.duel_odds` plays the blow rule out in 600 seeded trials (your win %, and your win % if you type
nothing). **The AI types too**, with a handicap of 5 against a human: a human typing good lines wins about 9 in 10 at
84 words a minute, 2 in 3 at 60, 1 in 3 at 42; one who types nothing loses every time (RB 21.8). **Skins:** wood,
obsidian, gold, dragonscale, butterfly. One duel per player at a time.

**Honest note:** by design a duel freezes the world. The engine has no room-wide hold yet, so `world` is `"beating"`:
beats keep resolving during the duel and the two duellists are locked in place (RB 21.1).

---

## 10. Death, resurrection, seals and quests

| Topic | Rule (RB 14, 25-27) |
|---|---|
| Health | 10 per body: 3 a bad punch, 5 a stab, 8 a car hit, 10 instant elimination |
| Who can plead | Any player, for any fallen demigod. **A wiped player may plead too** (plead, ask about quests, make offerings; nothing else). With wills on, `seat.eliminated` is not sent when your last demigod falls: keep the WILL box open |
| Death's price | 5 sparks x (1 + times raised before) + 1 spark per 8 beats dead; plus years off max health; all or nothing |
| Can't pay | A public quest; a demigod raised by a quest owes Death the sparks as debt |
| Returning | At 40% of its (lowered) max health, where it fell or at its shrine. At most 3 raisings per player per match. Event `demigod.raised` |
| Seals and curses | **seal** (no resurrection), **wither** (no healing), **silence** (wills cost double), **bind** (no summoning), **hex** (minus its strength on d20s). 5 sparks; god-tier 15 sparks, strength 3, until countered. A natural 1 turns the curse on its caster. Events `curse.cast`, `curse.broken` |
| Always a counter | Every curse comes with a mystic counter-quest. Everyone sees the mark, **never who cast it** |
| Mystic quests | 1-3 steps the engine verifies: stand at a place (at a phase), hold an odd ingredient, win an encounter, raise a shrine, survive a duel, offer sparks, bring an army, keep a vigil. Events `quest.given` / `quest.step` / `quest.done` / `quest.failed` (public quests; private ones in `round.private.will_events[]`) |

> "mystic quest wings of a butterfly to counter a curse" (Chris, RB 27)

Odd ingredients include the wings of a butterfly that has crossed a named strait, a stone washed in platinum dust,
salt from a dried sea, a bell without a tongue, a thread from a general's banner.

**Not built:** a lone dead player rewinding the match (time travel, not an invented "mercy" rule) needs an engine call
(`room.rewind`) that was requested from The Theatre and is not finished.

---

## 11. The living world

### 11.1 The Earth simulator (RB 28, EARTH_SIM)

Every cast, god-tier power and physical ramification starts a **phenomenon** on a deterministic voxel field of the
whole planet (one column per 120 km hex, 16 height levels; materials, water, lava, ash, fire, snow, cloud, cracks,
ruin). Phenomena: storm, blizzard, lightning, flood, wildfire, volcano, earthquake, meteor, tidal wave, raise and sink
land, forest, swarm, drifting plates. They interact (a flood quenches fire into steam; lava in water becomes rock; a
quake near a volcano wakes it). Consequences go through the rules (a demigod in a burning or flooded cell takes a
bounded hit; armour of the right material halves it).

- **Thing by thing** (schema 20): `round.outcome.public.earth.effects[]` with `thing.kind` (building, forest, crowd,
  road, shrine, demigod, person, army, settlement) and `what` (burned, flooded, destroyed, damaged, fled, died,
  sheltered, buried, swept, returned), with `count` (draw that many people tokens at people scale) and `to_cell`.
- **What lasts:** `scars` (burnt, mud, ruins, wrecks, sheltering, fallen).
- **The world's own events:** `dm_events` `earth.quake`, `earth.eruption`, `earth.drowned`, `earth.land_rose`,
  `earth.plate_moved`, `earth.impact`, `earth.wave`, `earth.omen`, `earth.road_cut`, `earth.shrine_toppled`,
  `earth.settled`.
- **The earth underfoot:** every prompt that acts at a place carries one factual line from the field ("waist-deep
  floodwater, the flood came 2 beats ago"); the DM never contradicts it.

**Benito's rule (EiW):** what you do changes the WORLD, not the log. At every zoom you see the damage at that scale,
and it persists; the picture must match the server's numbers. Flood: dark stain on the globe, hexes sink, water climbs
terraces, houses tilt and sink, tokens swept away, mud for several beats. Wildfire: a flame front tree by tree, black
stumps on grey ash, people and animals flee, slow regrowth. Hurricane on a village: people run to shelters, roofs fly
off, strong storms break shelters, the wreckage stays. Approved defaults: the camera stays put for other players'
disasters and glides to watch your own (never while grabbing, zooming or typing; it always returns); about 4 s per
disaster, about 8 s per typed action.

### 11.2 People, houses and token scenes

- `game.people` lists everyone near a board (named people plus 2-4 residents per house), each with what they are doing
  and why (fleeing a flood to a shelter, rioting, cheering an army; or a routine by role and time of day).
  `game.buildings` lists houses with a kind and a look. A fixed library of 48 sprites (16 roles x 3 looks) and 24
  buildings (8 kinds x 3 looks) dresses them.
- **Token scenes** (`scene.play`, schema 19). Benito wants typed in-game actions to play out, for example going into the house
  to my left and seduce the maiden, I wanna see that play out with the little game tokens". After the DM rules, the
  camera drops to people level, the demigod's token walks to the house, the roof lifts like a dollhouse, bubbles and
  the d20, the outcome (a heart burst, or a slap and she runs out), then the camera returns. **The scene decides
  nothing**: it is drawn from the recorded ruling and die. One scene per demigod per beat.

### 11.3 Armies, settlements, roads, shrines, encounters, loot

| Feature | How it works | Events |
|---|---|---|
| Armies | Raised by a will from a crowd; marches up to 900 km a beat, fights rival armies, strikes rival demigods | in `events[]`; `game.army` |
| Settlements | Level 1 hamlet to 5 great city (6-7 legendary); one step at a time toward a target set by people, prosperity, builders and shrines | `settlement.grew`, `settlement.shrank`, `settlement.building` |
| Roads | Traffic wears paths; a road appears at wear 4 and grows over when unused | `road.formed`, `road.faded` |
| Shrines | Raised by a will; up to 3 per demigod; grant sparks, healing or a ward; gather worshippers | `shrine.built`, `shrine.level` |
| Encounters | Hidden marked hexes: monster, treasure, trap, stranger; at night undead, ghost, thief. Step on one: a public d20, and it stays revealed | `encounter.marked`, `encounter.revealed`, `encounter.faded` |
| Loot | A death drops everything on its hex; the next demigod to pass takes the pile | `loot.dropped`, `loot.taken` |
| The piece's story | Followers, items, wardrobe stage (plain to mythic, from deeds), condition, scars | `story.scar`, `story.wardrobe`, `story.followers` |
| Day and night | 8 beats a day | `time.phase` |
| Occupancy | Benito's rule: no two standing demigods share a 3 m hex unless engaged | |

---

## 12. Memory and the tree of consequence (RB 3-4)

Every decision is a memory entry with a beat, a place and a kind. Places nest: a 3 m hex sits in a settlement, in a
region, in the world. Each beat rolls up the hierarchy; an answered question keeps its answer; a hex nobody looked at
is extrapolated, and once observed it is remembered (the same hex at the same beat is the same for everyone). The
minds read the newest 6 settled public facts about a region, labelled "never contradict them". Queries:
`game.memory`, `game.node`.

**The tree of consequence** (`game.memory_tree`): the whole hierarchy at once, with **recall paths** that show how one
fact (a d20, an encounter, an earth event) travels up into what the DM's prompt says now. Private facts are counted,
never shown. Client: **TREE** in the header.

---

## 13. Time travel (TIME_TRAVEL)

Everything is seeded and stored, so a match can be redrawn at any moment.

| Operation | What you get |
|---|---|
| Exact resume | A reconnecting client, a viewer joining mid-duel or a restarted server gets the exact present in one message (`match.snapshot` with `present`) |
| Replay | A recording re-computes every snapshot and compares them byte for byte: 0 differences, LLM answers included |
| State at a round | Any past beat through the public, raw or seat views (admin) |
| Fork | A new private room from any past beat (admin); open duels keep the time they had left |
| What a demigod looked like | The exact picture at any beat (`/api/assets/at`) |
| Earth time | A keyframe every 16 beats; `game.earth_at {beat}` |

No accepted instant action is lost to a server kill: it is journalled before the next one is taken.

---

## 14. Accounts, the god form and Meshy statues (ACCOUNTS §3, §7, §14)

- Players start as **guests** (Google sign-in exists in the API). Each player can write their two demigods'
  personalities, kept word for word unless they break the content rules.
- **The god form:** each player's god sits at their seat as a stone statue. Chris described them as stone or marble statues
  and they're sorta sitting down or whatever in poses --- the thinker. michaelangelo's david." Steps: describe it (up to
  600 characters), pick a stone (white marble default, black onyx, granite, sandstone, jade, bronze, gold leaf, or your
  own words), pick a pose (The Thinker, David, Moses seated, the Discus Thrower, Atlas, Winged Victory, the Boxer at
  Rest, or custom), preview (up to 6 new pictures a day), accept. On accept, a **Meshy** image-to-3D statue (1 per
  account); the client shows the accepted picture on the pedestal at once, then builds the statue from the model.
- Until a player makes one, their seat shows a house marble statue with their Left Hand's likeness.

---

## 15. Media (MEDIA, SHARED_MEDIA_SPEC)

The server writes every image prompt from the state (a client never sends one) and renders it through the media
layer; every image is cached by hash forever and recorded in the asset registry.

| Kind | What | Look |
|---|---|---|
| `portrait` | each demigod, waist-up | Benito's dark fantasy |
| `diorama` | each demigod as a table minifig on one hex tile | Max zoom (hex diorama) |
| `event` | a **moment card** for a big public event (by default magnitude >= 8, or a cast, elimination, general appointed or mutiny; at most 12 a match) | Max zoom |
| `statue` | the end-of-match piles | Max zoom |
| `army`, `place`, `item` | armies, places, notable items | Max zoom |
| gear looks | new portrait + diorama when a major item is acquired (`look.changed`; capped per demigod) | as their kind |
| `sprite`, `building` | the people-and-houses library (48 + 24) | house palette, detailed individuals |
| storyboard | `match.ended.summary.storyboard`: a title shot, up to 10 moments, a champion close-up | event / portrait images |

Every image avoids words, gore and real people. Video clips per storyboard shot are **not built** (they need Chris's
budget).

---

## 16. Audio

### 16.1 The sound pass (Unreal `UI/FNSoundPass.h`, `FNStudio`)

Small procedural sounds, no assets, on the account's volumes (master x sfx; the ambient bed rides master x music and
**swells with Entropy**):

| Sound | When (call site) |
|---|---|
| MiniWhoosh / MegaWhoosh (+ the studio's Impact under a mega) | a power cast plays on the board (`FNEarthPlay`) |
| Rumble | the ground heaving: a quake on the board (`FNEarthPlay`) |
| Clash | a duel locks: steel on steel (louder for your own) |
| Thump / Swish / Puff | a blow lands / misses / whiffs |
| Victory / Fall | your side wins / one of yours falls (and the end screen when you fell) |
| Chime (studio) | the hint's urgency rises to 2 or more |
| Click x3, then Chime or Boom (studio) | the table d20 tumbles; Chime on a good roll, Boom and a shake on a bad one |

### 16.2 Voiceovers (Unreal `production_assets/audio/vo/README.md`, `UI/FNVoiceOver.cpp`)

Rendered by Chris's TTS pipeline (Fish Audio, designed voices; no real person's voice). 44.1 kHz stereo, -14 LUFS,
peaks under -1 dBFS; each line as `_master.wav` (plain) and `_god.wav` (pitched-down double; the game plays the god
versions). **One at a time**, a line already speaking or waiting is not queued twice, **at most three wait**, and
**any key skips** (the key still reaches the game). Volume = master x voice. Each hook plays **once** per key (a run,
a match id, a duel id).

| Line | Plays when | Hook |
|---|---|---|
| welcome | the signed-in lobby first opens (once a run) | `SFNStartPanel` |
| intro | a match starts (once per match id) | `SFNHudLive` |
| clash | **your** duel locks (not on a restore) | `SFNHudLive` |
| victory / defeat | the end screen, you won / you fell | `SFNHudLive` |
| duel won / duel lost | your duel ends with a winner (a draw: no line) | `SFNHudLive` |

Every other moment has no voice line yet: see the master table and `briefs/VOICEOVER.md`.

---

## 17. Winning and the Final Night (RB 17)

**Win:** be the last player standing. Kill directly, through paladins, riots, armies, encounters and wards, or through
a rival's own unpaid debts.

**The Final Night** comes on beat 60, at heat death (Entropy 100), or on any beat that would end with nobody standing.
Every outstanding debt is collected, and exactly one player survives: the most total standing (health minus debt),
then start-of-beat health, then the lowest id. The end: `match.ended {match_id, winners, summary}` with `champions`,
`standings`, `piles` (each demigod's items and stats, with a statue prompt), `last_caption`, `final_cut` (title and
story) and `storyboard`.

Hidden information (API "Hidden information"): everyone sees positions, who fell, casts, Entropy, bolts, who opened a
slow window and its bill. **Only you** see your health, debts, your hands' replies, your armies' orders, your
`action.received` replies (including a cut). **Rivals' health is never sent**, except the duel health bars.

---

## 18. INTERACTION POINTS (the master table)

Every moment a player or the world triggers something. "Message" names come from `docs/CLIENT_API.md`. Sound and
voice columns name what exists in the Unreal client today; **none yet** means nothing is wired. C→S = client to
server; S→C = server to client. "Everyone" includes spectators unless noted.

### 18.1 Session, lobby and match flow

| # | Trigger | Message / event | Who | What the client shows | Sound | Voice |
|---|---|---|---|---|---|---|
| 1 | Connect | S→C `server.hello {game: "final_night_7"}` | you | check `game` | none yet | none yet |
| 2 | Signed-in lobby opens | (client; `GET /api/me`) | you | the lobby | none yet | **welcome** (once a run) |
| 3 | Match starts | `match.started` (`rules`) | everyone | the table, seats, statues | none yet | **intro** (once per match) |
| 4 | Beat starts | `phase.changed {start}`, `round.started` | everyone | beat counter; AI challenges (`challenges[]`); falls at Start | none yet | none yet |
| 5 | Beat 1 is the Final Wish | `round.brief.public.final_wish: true` | everyone | Final Wish banner | none yet | none yet |
| 6 | Brief | `round.brief` (public + your seat view: `hint`, `goal`, `time_bill`) | everyone / you | hint line, goal corner | Chime when urgency rises to >= 2 | none yet |
| 7 | Someone joins mid-match | `seat.joined`; `seat.takeover` in `dm_events` | everyone | toast; seat renamed | none yet | none yet |
| 8 | Reconnect or viewer joins | `match.snapshot` (with `present`) | you | the exact present, open duels included | none | none |
| 9 | Narrator caption (one beat late) | `round.outcome.public.previous_caption`, `previous_callouts` | everyone | caption line | none yet | none yet |
| 10 | Day phase changes | `time.phase` in `dm_events` | everyone | day/night light | unconfirmed (a chime in the older `SFNRootWidget` flow) | none yet |

### 18.2 The will and slow time

| # | Trigger | Message / event | Who | What the client shows | Sound | Voice |
|---|---|---|---|---|---|---|
| 11 | Player starts typing | C→S `action.begin` | (never broadcast) | the box opens; nobody else learns | none yet | none yet |
| 12 | Time slowed | S→C `window.opened {window_id, by_seat, deadline}` + `phase.event` `slowed` ("<name> slowed time") | everyone | banner; the opener's live bill growing | Boom (in `SFNRootWidget`) | none yet |
| 13 | Others submit into the window | `action.received {accepted: true, window_id}` | that player only | "held" | none yet | none yet |
| 14 | The opener presses Enter | `window.closed {reason: "submitted"}` | everyone | window closes | none yet | none yet |
| 15 | A slower player is cut off | `action.received {accepted: false, reason: "cut"}` | that player only | "YOUR ORDERS WERE CUT! Whoever slowed time pressed Enter first." | none yet | none yet |
| 16 | Window times out | `window.closed {reason: "deadline"}` | everyone | window closes, full bill | none yet | none yet |
| 17 | Time's bill settled | `round.outcome.public.slow[]` (`closed` / `instant`) | everyone | "Time snaps back: <name>'s window closed", debt | none yet | none yet |
| 18 | Hot button pressed | (client) then the same as a will | you | line fills, 5 s countdown | none yet | none yet |
| 19 | The will is answered | `round.private.wills[]` (`verdict`, `parts[]`, `result`, `why`, `next`) | that player only | each part's result, why, what would work | none yet | none yet |
| 20 | A god rules | `omens[]` in `round.outcome.public` (big ones) and `round.private` (your own) | everyone / you | the god's speech | none yet | none yet |
| 21 | Hands reply | `round.private.private.hands[].said`, `news` | you | the hand's one line | none yet | none yet |
| 22 | Public will one-liners | `round.outcome.public.wills[]` (`will.raised`, `will.curse`) | everyone | feed line | none yet | none yet |

### 18.3 Powers and Entropy

| # | Trigger | Message / event | Who | What the client shows | Sound | Voice |
|---|---|---|---|---|---|---|
| 23 | Mega armed | `game.fault` (hover preview) | you | fault lines and the tier under the cursor | none yet | none yet |
| 24 | Cast thrown | C→S instant `fields.cast` → S→C `phase.event` `streak` (with `text`, `own_hit`) | everyone | the streak, at once | MiniWhoosh / MegaWhoosh + Impact (when it plays on the board) | none yet |
| 25 | Cast refused | `action.received {accepted: false, reason}` | that player | toast (e.g. not enough bolts) | none yet | none yet |
| 26 | Cast settled | `round.outcome.public.casts[]` (`fizzled`, `off_target`), `events[]` | everyone | cast lines, ramifications | none yet | none yet |
| 27 | Entropy's check | `omens[]` with `god: "Entropy"` (public at magnitude >= 6) | everyone | Entropy's speech | none yet | none yet |
| 28 | Own demigod hit | `own_hit` on the cast; next beat `hint.kind: "own_hit"` | the caster | warning hint | Chime (urgency 2) | none yet |
| 29 | God-tier power cast | `phase.event` `god_power`; `round.outcome.public.god_powers[]`; the god's speech in `omens[]` | everyone | the power and the verdict | none yet | none yet |
| 30 | Entropy crosses 20 / 40 / 60 / 80 | `round.outcome.public.entropy` (`level`, `key`, `text`, `next_at`) | everyone | the meter | ambient bed swells with Entropy | none yet |
| 31 | Heat death | `entropy.heat_death` (that beat becomes the Final Night); `match.ended.summary` | everyone | the Final Night | none yet | none yet |
| 32 | Moment card ready | `events[].image` turns `ready` (`GET /media/index`) | everyone | the card | none yet | none yet |

### 18.4 The living world

| # | Trigger | Message / event | Who | What the client shows | Sound | Voice |
|---|---|---|---|---|---|---|
| 33 | Phenomena move | `round.outcome.public.earth` (`phenomena`, `delta`) | everyone | storms, floods, fire, lava on globe and board | Rumble for a quake | none yet |
| 34 | The earth did things | `earth.effects[]` (burned, flooded, fled, sheltered, died...) and `scars` | everyone | stumps, mud, wrecks, fleeing and fallen tokens | none yet | none yet |
| 35 | The world's own events | `dm_events` `earth.quake`, `earth.eruption`, `earth.drowned`, `earth.wave`, `earth.omen`, `earth.road_cut`, `earth.shrine_toppled`, ... | everyone | feed line, board change | none yet | none yet |
| 36 | Token scene | `round.outcome.public.scenes[]` (`scene.play`) | everyone | camera drops, dollhouse roof, bubbles, d20, outcome | none yet | none yet |
| 37 | A d20 rolls | `round.outcome.public.dice[]` (`dice.rolled`) | everyone | the table d20 tumbles, whose and what for | Click x3, then Chime (good) or Boom (bad) | none yet |
| 38 | Encounter marked / revealed / faded | `encounter.marked`, `encounter.revealed`, `encounter.faded` | everyone | marked hex; the reveal with its d20 | none yet | none yet |
| 39 | Loot dropped / taken | `loot.dropped`, `loot.taken` | everyone | the pile on the hex | none yet | none yet |
| 40 | Settlement changes | `settlement.grew`, `settlement.shrank`, `settlement.building` | everyone | town level | none yet | none yet |
| 41 | Road forms / fades | `road.formed`, `road.faded` | everyone | the road | none yet | none yet |
| 42 | Shrine built / levels | `shrine.built`, `shrine.level` | everyone | the shrine | none yet | none yet |
| 43 | A piece's story grows | `story.scar`, `story.wardrobe`, `story.followers` | everyone | the card | none yet | none yet |
| 44 | Gear gained | `look.changed` (with `pending`, then `ready: true`) | everyone | "NEW: <item>", gear strip, cross-fade of the art | none yet | none yet |
| 45 | Time's circle closes | `arena_radius_km`; names hurt by Time's clock | everyone | the circle; `danger` hint for your own | none yet | none yet |

### 18.5 Summons and duels

| # | Trigger | Message / event | Who | What the client shows | Sound | Voice |
|---|---|---|---|---|---|---|
| 46 | Summon (drag your demigod) | C→S instant `fields.summon` → `phase.event` `summoning` | everyone | flash and whoosh now | none confirmed | none yet |
| 47 | Summon lands | `summoned` in `events[]`, `summons[]`, `dm_events`; mishap in `dice[]` ("a summoning") | everyone | the landing, the deed, red pulse on health lost | none confirmed | none yet |
| 48 | Challenge sent | C→S instant `fields.challenge`; refusals as toasts | the challenger | challenge card with odds (`game.duel_odds`) | none yet | none yet |
| 49 | Duel locks | `phase.event` `challenge.lock` (AI: `round.started.public.challenges[]`, `leap`) | everyone | portraits slam in, both health bars, every camera swoops | Clash (louder for your own) | **clash** (your own duel only) |
| 50 | Duel stage | `duel.scene` (once), `duel.panel`, `fields.duel_tick` | everyone | the box opens, mannequins, comic panels | none yet | none yet |
| 51 | A duellist types | C→S `duel_keys` → `duel.typing` | everyone | masked stars, newest letters | none yet | none yet |
| 52 | A blow | C→S `fields.duel` → `duel.blow` (`verdict`, `damage`, `wit`, `flavor`, `ko`) | everyone | the blow, health bars | Thump (lands) / Swish (misses) | none yet |
| 53 | A whiff | `duel.blow` with `whiff` ("you have no pistol") | everyone | stagger | Puff | none yet |
| 54 | Each resolve during a duel | `round.outcome.public.duels_open[]` (`challenge.open`) | everyone | damage so far applied | none | none |
| 55 | Duel ends | `challenge.end` in `round.outcome.public.duels[]` | everyone | winner, deed, loot on the hex | none yet | **duel won / duel lost** (your own; no line on a draw) |

### 18.6 Falls, raisings, curses, quests, the end

| # | Trigger | Message / event | Who | What the client shows | Sound | Voice |
|---|---|---|---|---|---|---|
| 56 | A demigod falls | `round.outcome.public.eliminated` (or `round.started.public.eliminated`) | everyone | the fall | Fall (one of yours) | none yet |
| 57 | A player is out | `seat.eliminated` (not sent while wills are on: read `players[].alive`) | everyone | spectator view or the fallen WILL | none yet | none yet |
| 58 | A seat is back | `seat.revived` | everyone | seat active | none yet | none yet |
| 59 | Raised by Death | `demigod.raised` (`how`: paid / on credit / quest) | everyone | the demigod rises | none yet | none yet |
| 60 | Curse or seal lands / breaks | `curse.cast`, `curse.broken` | everyone (never who cast it) | the mark | none yet | none yet |
| 61 | Quest given / step / done / failed | `quest.given`, `quest.step`, `quest.done`, `quest.failed` (public); private in `round.private.will_events[]` | everyone / that player | quest panel, `break_curse` / `quest` hint | none yet | none yet |
| 62 | Last Stand holds | `arms.power` | everyone | the demigod stays at 1 | none yet | none yet |
| 63 | The Final Night (beat 60, heat death, or nobody standing) | the last beat; then `match.ended` | everyone | the end | none yet | none yet |
| 64 | Match ends | `match.ended {winners, summary}` (`champions`, `standings`, `piles`, `final_cut`, `storyboard`) | everyone | the end screen, storyboard | Victory (you won) / Fall | **victory / defeat** |
| 65 | Open the tree | `game.memory_tree` | you | TREE overlay | none | none |

---

## 19. Sources

Server repo (this worktree): `docs/presentation/design/GAME_DESIGN.md`, `docs/DM_RULEBOOK.md`, `docs/CLIENT_API.md`,
`docs/TIME_TRAVEL.md`, `docs/EARTH_SIM.md`, `docs/MEDIA.md`, `docs/ASSETS.md`, `docs/ACCOUNTS.md`,
`docs/DUEL_MOVES.md`, `docs/PLAYBOOK.md`, `docs/BALANCE_REPORT_2.md`, `data/pantheon.json`, `BUILDLOG.md`.
Unreal repo: `docs/design/hand_of_god/HAND_OF_GOD.md`, `docs/design/EVENTS_IN_THE_WORLD.md`, `docs/FOR_BENITO.md`,
`production_assets/audio/vo/README.md`, `Source/FinalNight/UI/FNVoiceOver.cpp`, `Source/FinalNight/UI/FNSoundPass.h`
and the `PlaySfx` / `FN7Sound::Play` call sites.

Designers: Chris and Benito.

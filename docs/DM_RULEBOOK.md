# The Dungeon Master's Rulebook

*Final Night 7, rules 0.9.0, state schema 11 (gear looks switch on at schema 11 with the room setting `looks_enabled`: docs/MEDIA.md "Looks"; wills that matter switch on at schema 10 with the room setting
`wills_enabled`; duel panels at 9, exact resume at 8, challenge mode and the 0.8.1 fixes at 7, the DM's rulebook
features at 6).*

This is the rulebook the Dungeon Master follows. The DM is the server: its rules (code) and its LLM minds (the game
master who resolves acts, the gods, the narrator). Every chapter states a rule as the DM applies it, with the numbers
the code uses. When the code and this book disagree, the code is the bug report: `MECHANICS` in
`crates/fn7-plugin/src/lib.rs` is the machine list recorded with every match, and the client contract is
`docs/CLIENT_API.md` ("DM rulebook features (0.8.0)" and "Challenge mode (0.8.1)").

The core of this book (chapter 1's four laws, in short form) and its governing principle (chapter 0) are embedded in the game master's and the gods' system
prompts (`fn7_agents::rulebook::DM_CORE`) for every schema-6 match. Older recordings keep their old prompts, so their
LLM replay still matches request for request.

**Contents**

0. The Lens and the Engine (the governing principle)
1. The DM's duties
2. Rolls and the d20
3. Places and memory
4. Memory and time: integrate, compare, extrapolate
5. Time: beats, days and nights
6. Settlements
7. Characters and their pieces
8. Encounters
9. Loot
10. Roads
11. Shrines
12. Gods and powers
13. Fault lines
14. Combat, harm and the killers
15. The table: hexes, occupancy, hidden information
16. The item engine
17. The beat, the Final Night and winning
18. Consistency rules
19. Summoning your demigod
20. God-tier powers
21. Challenge mode: will against will
22. Appendix: the DM's settings
23. Wills on your own: healing, blessing, and what a will is made of
24. Arms and their powers
25. Resurrection
26. Seals and curses
27. Mystic quests

---

## 0. The Lens and the Engine

> "history and its consequences are like lenses. all of the rules just imagine this crazy algo that DOES all of that,
> and then its answer, no matter how ludicrous, and try to get it making mystical sense to fit with the world, but a lot
> rube-goldberg mystical. like needing to wash a stone in platinum dust" (Chris, BUILDLOG "history as lenses; the
> DM's Rube Goldberg mysticism")

Everything else in this book serves three ideas:

1. **History is a lens.** Every place's recorded history and its consequences (chapters 3–4) are lenses the DM looks
   through before it speaks. They bend what is possible there: a valley that buried an army last night breeds ghosts;
   a city that just grew is full of scaffolds and new money; a road that wore in carries strangers.
2. **The rules and the dice are an engine that has already decided.** The DM imagines a vast, crazy machine that
   computed the answer (chapters 2, 5–20). It never overrules that answer, however ludicrous: a natural 1 on a
   god-blessed strike still misses; a hamlet that leaps two levels in a week did leap.
3. **The DM makes the answer make mystical sense.** Its job is the causality, and the causality is Rube Goldberg: a
   chain of omens, rituals, absurd material requirements and coincidences that rhyme with earlier events in the
   memory. Not "it failed" but "the bridge held only because the ferryman had washed the keystone in platinum dust at
   dawn, as the Memory god demanded three beats ago."

The LLM DM reads a concise form of this principle (`fn7_agents::rulebook::LENS`) in the game master's, the gods',
the narrator's, Entropy's ramifications' and the summon mind's prompts (schema 6 only, so older recordings replay).
The rules' own lines carry small templated flourishes in the same spirit (an encounter's outcome ends "because a crow
landed on the wrong side of the road at dawn, as the omen of beat 12 foretold").

**Worked examples** (the engine's answer, then the DM's telling):

- **A natural 1 on a mega cast.** Engine: Ash's volcano on Kyoto, 40 km from the Japan trench (fault factor 1.6), rolls
  a natural 1 on the act; it lands nothing on its target. DM: "The trench answered the call so eagerly that the magma
  overshot the mountain, arced over Kyoto and quenched itself in the sacred carp pond, which the monks had overfilled
  that morning to apologise for beat 9's flood."
- **A summon transit.** Engine: Vex's Left Hand, at health 9, is summoned from Lagos to Reykjavik; the mishap d20 fails;
  it loses 2. DM: "Mid-sermon in Lagos, Vex's Left Hand was yanked skyward by her own ego, re-entered over the Faroes
  trailing three confused gulls, and landed face-first in a Reykjavik fish market that, by Chance's old decree,
  never closes on a Tuesday."
- **An encounter.** Engine: a trap hex near Kinshasa, at night, DC 13 with −1; Dune rolls a 3: fail, 1 damage. DM: "The
  snare only held because the trapper had greased it with lantern oil bought from the same merchant whose stall
  Dune's riders overturned at beat 21; the oil remembered."
- **A settlement leaping two levels.** Engine: Madrid steps from 3 to 4 (prosperity 0.7, a shrine raised there) and,
  six beats later, to 5 (builders at work). DM: "Ever since the Shrine of Cyra's Left Hand was carved from the old
  bullring's single unbroken stone, every brick laid in Madrid has been laid twice, once by masons and once by their
  shadows."
- **A god's twisted verdict.** Engine: Death, asked by a god-tier power to strike Brix's Right Hand, twists the ruling:
  the strike lands, and Death's price takes a year of the caster's life. DM: "Death struck as asked, but used the
  caster's own reflection as the arrow, which is why every mirror in the region now shows Ash a little older than he
  is."
- **A road wearing in.** Engine: wear between Kyiv and Bucharest reaches 4.0. DM: "So many armies marched it that the
  grass gave up and became cobbles, as grass will when trodden by the same boot thirty-seven times."

---

## 1. The DM's duties

The DM runs a battle royale of demigods on the real Earth. Its duties, in order:

1. **Keep the world moving.** One Theatre round is one beat. Every beat resolves whether or not anyone typed. Every LLM
   call has a rules fallback, and a late call takes it. The beat never waits.
2. **Let the dice decide.** When an outcome is uncertain, the DM rolls a d20 before anyone describes it (chapter 2).
   The LLM narrates a roll; it never overrules one or rolls again.
3. **Remember everything.** Every decision about a place at a time goes into the DM's memory (chapter 3). Asked again,
   the DM gives the recorded answer.
4. **Be consistent.** A fact once recorded holds: an uncovered encounter stays uncovered, a grown village keeps its
   size, a fallen demigod stays fallen until a god or the survivor rule raises it (chapter 18).
5. **Show the table.** Every roll and every change of the board is public (except what hidden information keeps
   private, chapter 15), so the table sees the die tumble.

The four laws, as the LLM DM reads them: *the dice decide, you describe; the DM remembers; places nest; describe the
world as the rules left it.*

## 2. Rolls and the d20

### 2.1 When to roll

The DM rolls one d20 when:

| Situation | Query key | DC | Modifier |
|---|---|---|---|
| A demigod's ordinary act (resolver) | `act:<intent>` | 8 + magnitude/2 | the demigod's (2.3), +2 on the Final Wish |
| A manifestation (item engine) | `mf:<id>` | from the tier's base odds | the odds' other terms (2.4) |
| An encounter is stepped on | `encounter:<id>` | by type (chapter 8) | the demigod's, −1 for monsters and traps at night, +1 near its own shrine |
| A fight's loser keeps hold of its gear | `fight:<beat>:<a>:<b>` | 11 | the demigod's |
| A summoning's mishap check | `summon:<beat>:<demigod>` (0.8.1: `summon:<beat>:<demigod>:<lon>,<lat>`) | `round(20 × p) + 1` (chapter 19) | none |
| A duel blow (0.8.1) | `<duel_id>:<n>` (a counter `<duel_id>:<n>:counter`) | 10, +1 at night, + half the opponent's wardrobe stage | the line's wit and the duellist's (chapter 21) |
| A duel's loser keeps hold (0.8.1) | `<duel_id>:hold` | 11 | the demigod's |
| A will's part for a target (0.9.0): heal, bless, arm, curse, gather | `will:<intent>:<part>:<demigod>` | own: 6 + magnitude/4 (an arming 6 + 2 per tier); a curse 10 + half the target's wardrobe | named in the answer (chapters 23, 24, 26) |

The DM does not roll for things the rules settle exactly (damage from Time's circle, armies' strikes, debts, the
survivor rule) or for gods' verdicts, god-tier ones included (a god decides; there are no odds to roll against).

### 2.2 Reading the die

`total = roll + modifier`, against `target_dc`:

| Verdict | When |
|---|---|
| `crit_fail` | a natural 1, whatever the total |
| `fail` | total below the DC |
| `success` | total at or above the DC |
| `crit_success` | a natural 20, whatever the total |

### 2.3 The demigod's modifier

+1 when hale (health 7 or more), −1 when badly hurt (3 or less), plus half its wardrobe stage (0 to +2, chapter 7).

### 2.4 The act's result follows the verdict

For an ordinary act resolved by rules: a strike lands its damage (`magnitude/2`, 1–6) on a success, half on a fail,
1.5× on a crit success (at most 7), and nothing on a natural 1. Talk and threats move feelings by the same factors. A
journey always arrives. A **natural 1 on an act with no intent to harm, confirmed** by the roll's second draw
(`(roll − 1) + confirm < accident_scale`, the same 1 in 20 × `accident_scale` as before 0.8.0), is a Final
Destination accident on the actor (1–3 damage).

For a manifestation, the success odds become a DC: `target_dc = 21 − round(20 × base odds of the tier)`, and the
modifier carries every other term of the odds (bolts offered, debt, favor, Entropy, recent attempts), so
`roll + modifier ≥ target_dc` exactly when the face reaches `21 − round(20 × odds)`. A natural 20 always succeeds
untwisted; a natural 1 fails or backfires (never a partial). The roll's second draw places the outcome inside the
success share (success or twisted) or the failure share (partial, failed, backfired), keeping the odds' shares.

### 2.5 The same question, the same roll

Every roll is seeded by the match seed and its query key (`Rng::derive(seed, [0xD20, hash(query)])`) and recorded in
memory under that key. Asked again (another beat, another modifier), the DM returns the recorded roll and logs a
`memory_conflict` if the question changed. The die is shown once: the `dice.rolled` event in that beat's outcome.

## 3. Places and memory

### 3.1 Places nest

| Scope | Key | What it is |
|---|---|---|
| cell | a hex key `"<face>:<level>:<q>:<r>"` (people 3 m, houses 30 m; also villages 3 km, regions 120 km) | where a thing is on the board |
| settlement | `pl:N` of a city (planet) or site (city arena) | the village or city node the cell lies in (the nearest within 1.5 × its radius, at least 15 km on the planet, 0.5 km in a city) |
| region | `pl:N` of a region (planet) or district (city arena) | the board level-1 hex group |
| world | `"world"` | everything |

### 3.2 What the DM records

Every decision is a memory entry: `{id, beat, scope, place, up (the ancestors' keys), kind, query?, public, text,
data}`. Kinds: `dice.rolled`, `settlement.level`, `settlement.building`, `settlement.grew`, `settlement.shrank`,
`encounter.marked`, `encounter.revealed`, `encounter.faded`, `loot.dropped`, `loot.taken`, `road.formed`,
`road.faded`, `shrine.built`, `shrine.level`, `story.scar`, `story.wardrobe`, `story.followers`, `time.phase`,
`memory.rollup`, `place.observed`. The memory is append-only and lives in the match state (`World.dm.memory`), so it is
in every snapshot and recording and replays byte for byte.

### 3.3 Asking the memory

"What was decided about place P (or any ancestor or descendant) at or before beat B": an entry matches when its place
is P, or P is one of its ancestors (a fact below P), or its place is one of P's ancestors (a fact above P). Public
entries only for clients: `game.memory {place, until_beat?}`.

### 3.4 Feeding the LLM DM

The game master resolving acts in a region reads the newest `dm_memory_lines` (6) public facts about that region (and
what lies below and above it), oldest first, under "The DM's memory of this place (settled facts; never contradict
them)". A god reads the same about its invoker's region. Rolls and roll-ups are left out of the prompt (each act shows
its own die).

## 4. Memory and time: integrate, compare, extrapolate

### 4.1 Integrate when time progresses

Every beat, each new fact is recorded at its own place and beat. Then the DM **rolls the beat up the hierarchy**: for
every ancestor touched this beat (settlement, region, world) one `memory.rollup` entry with the counts by kind, up to
three notable events and the ancestor's state now (a settlement's level and size; a region's population, stance and
supply; the world's Entropy and who stands). A coarse zoom reads the integrated truth of everything beneath it:
`game.region` carries the latest roll-up as `memory_summary`.

### 4.2 Compare against priors

Before any decision the DM looks up what is recorded:

- A decision with a query key that is already answered keeps the recorded answer; a different new answer is logged as
  `memory_conflict` (both values, in the recording) and dropped.
- A resolver's proposals (the LLM's or the rules') are checked against their act's recorded d20: on a natural 1 a
  strike, a healing, a gift or a recruit aimed at someone else becomes narration; on a fail damage is capped at 3. Each
  change is a `memory_conflict`.
- Every LLM DM prompt carries the settled facts (3.4) and the four laws.

### 4.3 Extrapolate when zooming in

The server cannot simulate every 3 m hex every beat. Settlements, regions and the world are simulated each beat; a
hex is known only when someone looks:

- A hex observed at beat B has a `place.observed` entry: its state then.
- Zoomed into at beat N (`game.node {cell}`, `game.memory {place: <cell>}`), its state is **extrapolated** from that
  record: people scale with the parent's population now over then (with a small drift seeded by the cell and the two
  beats), buildings follow the settlement's level change, and the DM adds what it recorded on the hex since
  (encounters, loot, shrines, a road through it), the weather (seeded by region and beat), the time of day and the
  parent's notable changes since B. A hex never observed starts from its genesis, seeded by the match and the cell
  (people from the parent's density, buildings from the settlement's level).
- The answer is pure: the same hex at the same beat gives the same answer to everyone.
- **Recorded as the new fact when the DM observes it:** each beat the DM observes the hexes where demigods arrive
  (and once a day where they stand), each seat's zoom focus (`fields.zoom`), and every hex it decided something about.
  The next look starts from that record. Detail exists once observed and is then remembered.

Payloads say which it is: `observed_beat` (the last observation; `null` for never) and `extrapolated` (`false` when
observed this beat, or for a simulated place).

## 5. Time: beats, days and nights

- One beat is one Theatre round (`flow_ms`, default 3 s; the shared lobby uses 12 s). The Final Night falls on
  `beats_total` (60).
- **A game day is `beats_per_day` beats (8).** The first eighth of a day is dawn, then day until half, dusk until five
  eighths, then night. With 8 beats: dawn, day, day, day, dusk, night, night, night.
- **Light** follows the sun: 0.5 at dawn and dusk, 1 at noon, 0 at midnight (`0.5 + 0.5 × sin(2π × (beat_in_day − 1) /
  beats_per_day)`).
- Night brings out night-only encounters (undead, ghost, thief) and makes monsters and traps harder (−1).
- Each change of phase is a world-scope `time.phase` entry.

## 6. Settlements

Villages and cities (planet cities; a city arena's sites) have a **level**: 1 hamlet, 2 village, 3 town, 4 city,
5 great city, 6–7 legendary.

- **Founded** at the first beat from their people: under 200k → 1, under 1M → 2, under 3M → 3, under 8M → 4, else 5 (a
  city arena's sites share their district's people).
- **Each beat** the DM sets a target: the level its people give now, +1 if prosperous (prosperity ≥ 0.62), −1 if
  wretched (< 0.3), +1 if builders worked there in the last 8 beats (an order with a building verb by a demigod
  standing in it), +1 if a shrine stands in it or a demigod holds its allegiance (≥ 0.5). Prosperity is
  `0.4 × supply/1.5 + 0.3 × hope + 0.3 × (1 − danger)`.
- **It moves one step at a time** toward a target held two beats running, after holding its level
  `dm_settlement_step_beats` (6) beats. Every step is `settlement.grew` or `settlement.shrank`, public.
- **Its hexes:** the village-level hex at its centre plus rings out to level − 1 (1, 7, 19, 37, 61 hexes):
  `settlement_cells`; `center_cell` is the people-level hex at its centre.

## 7. Characters and their pieces

Each demigod's playing piece carries its story, like the pegs in a Life car:

- **Followers:** its Following's headcount. Milestones (10, 100, 1,000, 10,000, 100,000) are remembered.
- **Items:** every item it holds now, as `{ref, icon}` pegs.
- **Wardrobe:** a stage from its deeds, never going down: 0 plain, 1 marked (3 deeds), 2 adorned (8), 3 regalia (16),
  4 mythic (30). Deeds: up to 3 a beat for landed public acts of magnitude 5+, 1 per thing made, 1–3 per encounter won,
  3 per shrine raised, 1 per loot taken. Its **condition** shows harm: `scorched` (hit by fire, lightning, lava or a
  meteor this beat) or `ragged` (health 3 or less).
- **Scars:** a hit of 1.5 health or more in one beat leaves a scar `{beat, cause, place, cell}` (the heaviest hit names
  the cause; the newest 12 are kept). Public: scars are flavour.

Named people: strangers met on encounters become named characters who join the demigod's Following (chapter 8);
people the players address are memorized from their crowds (`MECHANICS`, "Mentions").

## 8. Encounters

The DM marks hexes where something waits: a **monster, treasure, trap or stranger**, and at night an **undead, ghost
or thief**.

### 8.1 Placing

- Up to `dm_encounters_per_beat` (1) a beat, while fewer than `dm_encounters_max` (10) are hidden.
- Near activity, at the places demigods travel to: a region's centre, at most one hidden encounter per region, picked
  by weight: the region where Time's circle converges 3, a region with a hotspot 2, any other 1. Never a centre a
  standing demigod is within the step radius of. The hex lies 0.2–0.8 × `dm_reach_km` from the centre, so whoever
  travels there steps on it.
- Type by weight: day monster 3, treasure 3, trap 2, stranger 2; night monster 2, treasure 1, trap 2, stranger 1,
  undead 3, ghost 2, thief 2.
- Treasure, traps and thieves sit on a people-level hex; monsters, strangers, undead and ghosts on a house-level hex.
- **Hidden:** the public sees only `{id, cell, marked: true}`. An unfound encounter fades after
  `dm_encounter_fade_beats` (20).

### 8.2 Stepping on

A standing demigod **steps on** an encounter when its move this beat passes within the step radius of it
(`dm_reach_km`, 30 km on the planet; a city arena uses a hundredth). The closest mover takes it (ties by id). The
encounter is revealed and resolved with a visible d20; the result is applied, recorded and **stays revealed forever**.

| Type | DC | crit_success | success | fail | crit_fail |
|---|---|---|---|---|---|
| monster | 12 | slain: a trophy item | driven off | 1.5 damage | 3 damage |
| treasure | 10 | a hoard: an artifact-tier item | an item | an empty chest | a mimic: 1 damage |
| trap | 13 | the trapper's cache: an item | stepped around | 1 damage | 2.5 damage |
| stranger | 11 | a champion joins (named character) | a follower joins (named character) | walks away | an assassin: 1.5 damage |
| undead (night) | 13 | grave-goods: a relic | cut down | 2 damage | 3.5 damage |
| ghost (night) | 12 | +2 sparks | +1 spark | 1 damage | 2 damage |
| thief (night) | 12 | the thief's haul: an item | seen off | −1 spark | −1 spark and 1 damage |

Damage × `dm_encounter_damage_scale`. A stranger becomes a named character (memorized from a nearby crowd) who joins
the demigod's Following. An encounter death's cause starts with `encounter:`.

## 9. Loot

- **A death drops everything** the fallen demigod carried (held items, not deployed hosts) on its hex.
- **A fight:** of two demigods engaged this beat (one acted on the other), the one hurt worse (at least 1 health) rolls
  to keep hold (DC 11); on a fail its newest item drops on its hex.
- **Taking:** from the next beat, the standing demigod whose move passes closest within the step radius takes the whole
  pile; the one who dropped it only after 3 beats. A rival's take is `stolen`, a sibling's `transferred` (the item's
  custody history). Items are never lost: holders only grow.
- The pile stays on its hex, remembered, until taken.

## 10. Roads

- **Traffic** wears paths between settlements: each demigod move whose start and end lie near different settlements
  (within 400 km on the planet, 4 km in a city) adds 1; each army march adds 1 + size/20,000 (at most 3); each crowd
  migration adds 0.5.
- **A road appears** when a path's wear reaches `dm_road_wear` (4), remembered (`road.formed`).
- **Unused paths decay** by `dm_road_decay` (0.05) a beat; a road below half the threshold grows over (`road.faded`).
- A road is `{from_cell, to_cell, wear, since_beat}` between the two settlements' centre hexes.

## 11. Shrines

- **Raising:** an order with a making verb (build, raise, erect, construct, found, consecrate, carve, make, manifest,
  create, dedicate) and a shrine word (shrine, temple, monument, altar, statue, idol, obelisk, sanctuary, chapel) raises
  a shrine on the demigod's house-level hex; so does any structure the item engine makes. At most
  `dm_shrines_per_demigod` (3) each, one a beat. Free; 3 deeds.
- **Power** from its words (heal/mercy/life → heal; ward/war/wrath/fear/guard → ward; spark/light/power/lightning →
  spark), else a seeded pick:
  - **spark:** +1 spark to its player every `dm_shrine_spark_every` (6) beats, one beat sooner per level above 1 (at
    least 2);
  - **heal:** its player's demigods within 3 × the step radius heal `dm_shrine_heal` (0.15) × level a beat;
  - **ward:** rival demigods within 3 × the step radius take `dm_shrine_ward` (0.1) × level a beat.
- **Worshippers** grow each beat from crowds within 300 km (3 km in a city): size × (0.001 + 0.004 × devotion) ×
  (1 + spiritual hunger), at most 250,000 a beat, plus pilgrims from its demigod's Following (headcount/1,000, 5–500).
  An abandoned shrine (its demigod fallen) loses a tenth a beat and grants nothing.
- **Level:** 1 under 1,000 worshippers, then +1 per tenfold (at most 5).
- A shrine near an encounter gives its demigod +1 on that roll; a shrine in a settlement lifts its target level.

## 12. Gods and powers

### 12.1 The pantheon

`data/pantheon.json`. Gods by index: Time (`gd:0`, precedence 50), Nature (30), Consequence (40), Death (45), Chance
(10), Memory (20), Entropy (35).

- **Invocation** costs `1 + magnitude/5` bolts, paid unless refused; a god whose price can't be paid twists or
  refuses. Verdicts: grant, refuse, bargain, twist. Prices include Life (Time's years lower `max_health`).
- **Entropy** charges nothing, always says yes, gives 2 bolts and raises the meter. Invocations of an unmade god go to
  Entropy. Time can cool Entropy by `4 + 1.2 × magnitude` for its price.
- **Merge precedence:** Time's Delay beats Consequence's Collect on the same debt (it grows 50% and moves); a Time
  Rewind overrules Nature and Chance on its target that beat (never debts or prices); gods pulling one target opposite
  ways: higher precedence wins, Chance's seeded coin breaks ties; several damage proposals on one target become one hit
  at the median.

### 12.2 Text entry: Time's slow windows

A seat's `action.begin` opens a slow-time window (one per room, up to `slow_ms`) if its demigod stands and holds a
bolt. The world keeps resolving. The opener's Enter closes it; later submits are cut privately. Time's bill at the next
resolve: a bolt as toll (no gift), debt `0.5 × (1 + seconds/10) × (1 + 0.5 × slows in the last 20 beats)` (doubled if
the bolt can't be paid, cap 10), Entropy `0.5 + seconds/30`. An order without a window is an instant order: the same
toll and Entropy, a flat debt of `instant_order_debt` (0.5).

### 12.3 Casts: mega and mini powers (the 1:5 economy)

Each player holds lightning bolts (mega, start 1) and sparks (mini, start 5). A cast is an instant action, shown at
once as a streak and paid at the next resolve in arrival order.

- **Mega:** map scale (radius × 4 within 600–2,500 km, magnitude + 2, damage × 1.5; 40% of it to every rival demigod
  in the radius), costs a bolt; every other player gains `bolt_gift` (1) bolt; Entropy + `entropy_per_mega` (10).
  Scaled by the nearest fault line (chapter 13).
- **Mini:** the board (radius / 20 within 3–30 km, magnitude − 3, damage × 0.6, its target only), costs a spark, gifts
  nothing; Entropy + `entropy_per_mini` (1).
- A player spends at most what it held at the beat's start. At Entropy 40+ a third of casts land 50–400 km off target.
- **Ramifications:** one call per region with casts, N = `1 + cost + magnitude/3` per cast (a mega counts as cost 3),
  at most 10: geotagged rows and lasting world mods (at most 24 live; the same name in a region stacks to 5).

### 12.4 Entropy

0–100, every rise × `entropy_scale`. 20 freak weather, 40 unreliable physics, 60 riots and worship, 80 wounds to the
gods; 100 is heat death: that beat becomes the Final Night at once.

## 13. Fault lines

A **mega** cast's effect scales with its great-circle distance to the nearest plate boundary:

| Distance | Magnitude | Radius | Damage |
|---|---|---|---|
| ≤ `fault_near_km` (150 km) | + `fault_near_magnitude` (+3) | × `fault_near_radius` (1.5) | × `fault_near_damage` (1.6) |
| ≥ `fault_far_km` (1,500 km) | + `fault_far_magnitude` (−2) | × `fault_far_radius` (1.0) | × `fault_far_damage` (0.6): "sort of a mild volcano" |
| between | linear in distance | linear | linear |

More magnitude means more ramifications. Minis are unaffected. The lines are compiled once into a 5° segment index at
startup (deterministic). The streak and the resolved cast carry `fault_km` and `fault_factor` (the damage
multiplier). The client shows the lines only while a mega is armed.

The live house curve (room settings, lobby): near 200 km: magnitude +5, radius × 2.2, damage × 2.5; far 1,000 km:
magnitude −4, radius × 0.4, damage × 0.25.

### 13.1 Junctions and tiers (state schema 16)

Chris wants megapowers made much stronger at large intersections, the nearer they are to the faultlines, and only
'tiny volcanos' near the no-faultline areas." With `fault_junctions_enabled` (default true) and a schema-16 state:

**Junctions.** When the index is built, the plate-boundary junctions are found once, deterministically:

- *Triple junctions:* polyline ends within 30 km of each other whose boundaries touch 3 or more different plates
  (PB2002 names both plates of every boundary). Candidates chained within 150 km are one junction (the Galápagos
  microplate's three are one Cocos-Nazca-Pacific-Galápagos junction of four plates), placed at its biggest member.
- *Knots:* dense, twisting stretches of boundary (at least 1,500 km of boundary within 300 km: ridge-transform
  staircases, the Kermadec arc) at least 400 km from every triple junction and 500 km from a stronger knot.
- *Strength* (0..1, two decimals): for a triple junction, ¾ the size of the meeting (the mean of its three biggest
  plates' weights, a plate's weight being √(its boundary length ÷ the Pacific's), + 0.1 per plate past three) and ¼ the
  boundary length within 300 km, scaled so the strongest is 1. A knot's strength is 0.6 × its density ÷ the densest
  sampled boundary (at most 0.6: below the great triple junctions).

About 97 junctions (89 triple, 8 knots). The named ones (a table by location: the nearest triple junction within
250 km takes the name):

| Junction | Where (lon, lat) | Plates | Strength |
|---|---|---|---|
| Solomon Islands junction | 156.3, −8.17 | AU, NB, PA, SS, WL | 1.00 |
| Macquarie triple junction | 161.23, −61.46 | AN, AU, PA | 0.94 |
| Rivera triple junction | −105.59, 17.88 | CO, NA, PA, RI | 0.91 |
| Bouvet triple junction | −0.44, −54.85 | AF, AN, SA | 0.90 |
| Azores triple junction | −29.77, 39.40 | AF, EU, NA | 0.87 |
| Juan Fernández triple junction | −110.83, −35.09 | AN, JZ, PA | 0.85 |
| Galápagos triple junction | −102.18, 1.13 | CO, GP, NZ, PA | 0.84 |
| Rodrigues triple junction | 69.96, −25.65 | AN, AU, SO | 0.84 |
| Chile triple junction | −76.01, −45.66 | AN, NZ, SA | 0.80 |
| Kamchatka–Aleutian junction | 164.07, 55.21 | NA, OK, PA | 0.78 |
| Mendocino triple junction | −124.74, 40.31 | JF, NA, PA | 0.76 |
| Queen Charlotte triple junction | −130.85, 51.61 | JF, NA, PA | 0.74 |
| Afar triple junction | 42.03, 11.58 | AF, AR, SO | 0.71 |
| Easter triple junction | −112.98, −26.83 | EA, NZ, PA | 0.67 |
| Boso triple junction | 141.88, 34.21 | OK, PA, PS | 0.67 |
| Maraş triple junction | 36.41, 36.02 | AF, AR, AT | 0.57 |
| Karlıova triple junction | 41.00, 39.08 | AR, AT, EU | 0.57 |

`game.faults_overview` lists them all.

**The junction multiplier.** A mega within `fault_junction_km` (300) of a junction feels its pull = strength × (1 −
km ÷ `fault_junction_km`) (the strongest pull wins), on top of the distance curve:

| | At a junction of strength 1, at its point |
|---|---|
| magnitude | + `fault_junction_magnitude` (+3) × pull |
| radius | × (1 + (`fault_junction_radius` (1.5) − 1) × pull) |
| damage | × (1 + (`fault_junction_damage` (1.8) − 1) × pull) |

With the house curve, a mega volcano (magnitude 10, damage 3, radius 600 km) aimed at the Afar triple junction lands
at magnitude 10, damage 11.8 (× 3.92), radius 1,789 km; the same in central Australia (tiny) at magnitude 6, damage
0.8, radius 240 km, and its scar stays within 150 km. The narration and the cast's act say where: "… casts volcano on Djibouti at the Afar triple junction"
(unnamed: "at a triple junction", "at a junction of four plates", "in a knot of fault lines").

**Tiers.** Every mega lands in one: `junction` (within `fault_junction_km` of a junction), `fault` (≤
`fault_near_km`), `near` (the first half of the curve), `mild` (the second half), `tiny` (≥ `fault_far_km`). The
tier sets the world's answer: the ramifications' weight of the cast (N = 1 + weight + magnitude/3, chapter 12.3) is 4
at a junction, 3 at a fault or near, 2 mild, 1 tiny (the old rule: 3 for every mega), and a tiny mega's lasting world
mods reach at most `fault_tiny_mod_km` (150 km): a tiny volcano stays a local mod, never a regional one.

The streak, the resolved `casts[]` line and the `fault` event add `fault_tier` and `junction` (`{name?, kind, at,
strength, plates, km}` or null); `fault_factor` is then the whole damage multiplier, the junction's included.
`game.fault {at}` previews it under the cursor (read-only, deterministic).

Older states (schema ≤ 15) and settings recorded without `fault_junctions_enabled` keep the plain curve, so their
recordings replay exactly.

**Balance** (`crates/fn7-server/tests/fault_balance.rs`, 128 seeds × 60 beats, 4 players, rules only, the house
curve, junctions off vs on). At the house `bolt_start` (1) the built-in AI casts a mega only when it holds 2+ bolts,
which almost never happens, so the two arms come out identical (58.4 beats played; 19% end early, none by heat
death; seat wins 27/42/26/33, χ² 5.06 < 7.81). With `bolt_start` 6 (297 megas: 12 junction, 45 fault, 75 near, 55
mild, 110 tiny) both arms hit heat death at beat 19.7 on average: the Entropy each mega adds ends those worlds, not
the junctions (first elimination 19.0 → 18.9, eliminations 6.02 → 6.03 per match, the champion changes on 3 of 128
seeds, seats χ² 0.44). The AI aims at rivals, not at junctions; a player who hunts junctions on purpose lands megas
of about × 3.9 damage and × 3 radius. If that turns out too much, make a junction mega cost one more bolt (or more
Entropy) instead of weakening the multipliers.

Credit (required, ODC-BY 1.0): "Plate boundaries: Bird, P. (2002), PB2002; GeoJSON by Hugo Ahlenius, Nordpil
(ODC-BY 1.0)." (`data/geo/PB2002_boundaries.json`, `data/geo/README.md`.)

## 14. Combat, harm and the killers

- **Health** 10 per body on the 0–10 scale: 0 flashbang, 3 a bad punch, 5 a stab, 8 a car hit, 10 instant elimination.
- **Hostile by default:** everyone is hostile unless their acts say otherwise. Harm without intent is a Final
  Destination accident (2.4). The luck is skewed: very good and very bad things happen more often than you'd expect.
- **Time's circle** shrinks around the convergence city from the whole globe to 1,500 km by `arena_close_beats` (160),
  ease-out; bodies outside take `0.5 + 1.5 × progress + 4 × overshoot` (0.5–3) and are dragged in.
- **Armies:** march up to 900 km toward their target; rival armies within 300 km clash (8–16% losses); an army within
  200 km of a rival demigod strikes it for `(0.5 + size/40,000) × (0.6 + 0.6 × competence)`, at most 2, ×
  `army_strike_scale`. The chain: demigod → general → captains → units; orders go to the general, who judges them.
- **Consequence's bill:** per demigod per beat, `debt_scale × Σ debt_for(magnitude, words)` over its landed acts, due
  in `debt_due_beats` (50). Due debts are collected as accidents.
- **Encounters** (chapter 8) and **ward shrines** (chapter 11) do small, rolled harm.
- **Revive:** Death's raise-the-dead, or the survivor rule.

## 15. The table: hexes, occupancy, hidden information

- **Hexes** are the client's exactly: cube-face gnomonic projection, R = 6,371 km, pointy-top axial, sizes people 3 m,
  houses 30 m, villages 3 km, regions 120 km (`fn7-model/src/hexgrid.rs`). Every piece carries its people-level `cell`.
- **Occupancy (Benito's rule):** no two standing demigods share a people-level hex unless engaged (one acted on the
  other this beat); the later mover is nudged to the nearest free neighbour.
- **A human taking over a bot's seat** mid-match takes up its two demigods under the joiner's names
  (`"<name>'s Left Hand"` / `"<name>'s Right Hand"`, same refs), remembered as `seat.takeover`.
- **Two demigods per player:** the Left and Right hands are two bodies, each with its own health, position, items,
  story and scars. A player is out when both fall. Bolts and sparks are a per-player purse; debts are per body.
- **Hidden information:** a seat sees what its demigods, hands, agents and bodies did, saw, were told or were hit by,
  plus what its Following heard. Public: positions, cells, bolts and sparks, casts, armies, mods, Entropy, big
  rulings, the slow window's opener, influence, and (0.8.0) every d20, settlement levels, encounters as marked hexes
  (types once revealed), loot, roads, shrines, story pieces and the time of day. A die's public `reason` names only the
  act's class ("a strike", "a parley", "a journey"), never the order's words. Rivals' health is never sent, except
  inside a duel (0.8.1): the lock and every blow show both duellists' health (the duel's health bars).

## 16. The item engine

An order to manifest something (a making verb and a thing) is judged by rules; an LLM only names it.

- **Tier** from kind, count and magnitude: trinket ≤ 12, artifact ≤ 1,000, relic ≤ 100,000, wonder ≤ 1M, legend ≤ 10M,
  myth ≤ 100M, titan ≤ 1B, apocalypse beyond. Objects by magnitude (0–1 trinket … 10 apocalypse), +1 tier for ≥ 1,000
  copies, +2 for ≥ 1M.
- **Cost:** 0, 0, 1, 1, 2, 3, 4, 6 bolts × `manifest_scale` from the purse; the shortfall becomes debt to Consequence
  `0.6 × missing × (1 + tier/4)` (refused instead when the order says no debt). Bolts are spent whatever happens.
- **Odds:** `clamp(σ(logit(base) + 0.35 × extra bolts − 0.15 × missing + 1.2 × favor/10 + 0.8 × Entropy/100 (wonder+)
  − 0.3 × recent), 0.02, 0.99)`, bases 0.97, 0.90, 0.80, 0.66, 0.52, 0.38, 0.25, 0.14. Twists `0.04 + 0.30 × e` of
  successes; partials `0.25 + 0.05 × tier` of failures; backfires `(0.05 + 0.25 × e) × tier/7` of failures (0.5 + 0.25
  × tier damage to the maker).
- **The roll is a d20** (2.4) under schema 6, recorded as `mf:<id>`.
- A made thing is an item held by its maker (kind, icon, tier, count, state, banner, custody). A host of 100+ takes the
  field as an army of `muster_size(count) × manifest_muster_scale`. Public from relic tier up. A made structure is also
  a shrine (chapter 11).

## 17. The beat, the Final Night and winning

**The beat:** Start (Time's clock, due debts queued, agents expire) → Brief → Compose (`flow_ms`) → Resolve (Time's
bills → casts → hands → mentions → route → one batch of minds → merge and world rules → the society → occupancy →
**the DM's step** → consequence and memory) → Reveal → Commit.

**The DM's step** (after the society and occupancy each beat): time of day; encounters stepped on; shrines raised,
grown and granting; loot dropped and taken; encounters faded and marked; story pieces; settlements; roads; hexes
observed; the beat rolled up the hierarchy.

**The Final Wish:** beat 1. Every player's orders are their final wish: more acts, +`final_wish_magnitude` (2) on each,
+2 on its d20s.

**The Final Night** (beat `beats_total`, heat death, or any beat that would end with nobody standing): every
outstanding debt is collected; exactly one player survives: the most total standing (health minus debt), then
start-of-beat health, then the lowest id.

**Winning:** be the last player standing. Kill directly, through paladins, riots, armies, encounters and wards, or
through a rival's own unpaid debts.

## 18. Consistency rules

1. **Never contradict the memory.** An answered question keeps its answer (4.2).
2. **Never re-roll.** One query, one die, forever.
3. **Revealed is forever.** An encounter, once revealed, stays revealed with its outcome; a hidden one is never
   described.
4. **Levels move one step at a time,** and only by the rules (chapter 6).
5. **Items are never lost:** holders only grow; a dropped item lies on its hex until taken.
6. **The eliminated are gone** until a god or the survivor rule raises them; they never act.
7. **Detail, once observed, is remembered** (4.3); the same hex at the same beat is the same for everyone.
8. **Everything is seeded** by the match seed and stored in the state; a replay reproduces every snapshot byte for
   byte. Every 0.8.0 rule is gated on state schema 6 and every 0.8.1 rule on state schema 7, so older recordings replay
   unchanged.

## 19. Summoning your demigod

A player drags one of its demigods onto the board or the globe: an instant action `fields.summon {demigod, target}`
(target `[lon, lat]` or a hex key), in any phase. It lands at the next resolve, **this beat**.

- **Allowed** when `summon_enabled`, the demigod is the player's own and standing, and it wasn't summoned in the last
  `summon_cooldown_beats` (1: once a beat). Checked on arrival and again at the resolve.
- **The move** is a rules move row, so the occupancy rule (15) nudges it off an occupied people-level hex. Its hands'
  travel this beat is void, and its standing orders end (the summoning interrupted them).
- **Recklessness:** a visible d20 (`summon:<beat>:<demigod>`, reason "a summoning", no modifier) against DC
  `round(20 × p) + 1` with `p = clamp(0.25 + 0.6 × health/max, 0, 0.95)`; a failure is a mishap costing
  `round(0.2 × max × health/max)` clamped to 1..2 health (× `summon_loss_scale`). Below 2.5 health a mishap costs
  nothing. At full health that is an 85% chance of losing 2 (20%).
- **The damage check comes first (0.8.1):** the mishap d20 and the health it costs are decided at the accept, from the
  committed state, and shown in the instant `summoning` push (a red pulse before the teleport); the resolve applies
  exactly those, recomputed from the beat's starting health. A challenge right after a summon of the same demigod
  fights from its destination at its health minus that loss (chapter 21).
- **The deed:** the summon mind (LLM, else its rules fallback) tells in one hilarious line what it was doing, how it was
  yanked across the world and how it landed (the Lens and the Engine, chapter 0). The deed is a **tale** in its story
  (`story.tales`, the newest 8) and is remembered at both places: `summon.departed` at the hex it left and `summoned` at
  the hex it reached (the public event the client flashes on).

## 20. God-tier powers

Each god has one signature power on the ring, cast like any power with `tier: "god"`:
`fields.cast {power: "god_<slug>", tier: "god", target}`.

- **Paid at the next resolve:** `god_power_cost` (3) bolts from the purse; every other player with a demigod standing
  gains `god_power_gift` (1) bolt, like a mega; Entropy + `entropy_per_mega`.
- **It is that god's ruling,** not a cast: an invocation (magnitude 8) routed to the god's mind, which grants, bargains,
  twists or refuses, sets its price from its table and takes its place in the merge's precedence. Its bolt price is
  already paid. Unless the god refuses, its signature lands as the god's own proposals:

| Power | God | Signature |
|---|---|---|
| `god_time` "Time Stands Still" | Time | a unit target is rewound 2 beats; aimed at a point, rival demigods within 1,500 km are slowed a beat |
| `god_nature` "The Wild Surge" | Nature | a surge of growth and beasts: 1.2 damage to rival demigods within 400 km |
| `god_consequence` "The Reckoning" | Consequence | the targeted rival's biggest open debt is collected now |
| `god_death` "The Pale Verdict" | Death | a fallen target is raised; a standing one is struck for 3 |
| `god_chance` "The Coin-Flip Storm" | Chance | every demigod within 600 km: a seeded coin, heal 1.5 or take 1.5 |
| `god_memory` "The Great Forgetting" | Memory | every crowd in the target's region forgets its allegiances |
| `god_entropy` "The Chaos Burst" | Entropy | Entropy +15 (Entropy always says yes) |

## 21. Challenge mode: will against will

> "everyone on the map needs to stop what they're doing and watch: the Archon-level fight, will against will"
> (Chris, rules 0.8.1)

**The inspiration.** In Archon (1983) the board is chess-like strategy, but when two pieces meet the game cuts to an
arena and the fight is decided by real-time play. Final Night 7 does the same, with **typing** as the real-time play:
two demigods lock in, both diorama portraits slam onto the screen (a Mega Man X double lock-in), and every line a
player types is a blow. The world holds its breath and watches.

### 21.1 The challenge

A player double-clicks a rival demigod near one of its own: an instant action
`fields.challenge {demigod: "dg:mine", target: "dg:N", level}`. **Direct combat must be very straightforward to
engage:** the reach is `max(challenge_min_km, contact_km(level))`, i.e. the two board cards touch at the zoom the
player is looking at, or the demigods are within 120 km at any zoom. Contact distances: regions 600 km (also for a
missing or unknown level), villages 15 km, houses 150 m, people 15 m, so below the regions zoom it is 120 km (Chris:
"Too far to challenge" should not appear when the target is only 3 m away at the people zoom. The level also frames the fight
(the lock's hex).
**Forced:** there is no consent. Valid when (checked on arrival and again at the resolve, in arrival order):

| Refusal | When |
|---|---|
| `not_yours` | the demigod isn't the sender's |
| `fallen` | either demigod has fallen |
| `bad_target` | the target is no rival (another seat's) |
| `too_far:<km>:<allowed>` | great-circle distance > max(120 km, the level's contact distance); the refusal carries both numbers |
| `busy` | either is already in a duel (a duel lasts up to 30 s, across beats) |
| `cooldown` | its last challenge was under `duel_cooldown_beats` (1) ago |

Accepted, everyone sees `challenge.lock`: both duellists (name, portrait and diorama, **both healths**: the duel's
health bars are the one place a rival's health is shown), the ground (the target's position), its hex at the level,
the camera `focus` (lat, lon, cell: every client swoops there and spins the globe), and the duel's id
`duel:s<seq>:<a>:<b>` (the challenge action's room-wide seq, stable across beats; the built-in AI's
`duel:<beat>:<a>:<b>`).

**Right after a teleport.** A challenge may come during or right after a summon of the same demigod: it is checked
from the summon's destination, and that demigod fights at its health minus the summon's mishap, which was decided at
the summon's accept (chapter 19). Teleporting into a rival with a 2-point penalty is Archon-level danger.

**The world holds its breath.** By design a duel freezes the world: no beats resolve, timers pause and only the two
duellists act, until a knockout or `duel_deadline_ms` (30 s), with a fast relay for the masked live typing. That
needs a room-wide engine hold (requested and announced in the lock's `freeze`). **Until the Theatre offers it, the
world keeps beating (the fallback):** the duel is kept in the state between resolves (`World.dm.duels`), its two
demigods are locked in place (their hands' travel is void), each resolve applies the damage so far and publishes the
duel as `challenge.open`, and the duel ends at the first resolve after its deadline stamp (the batch's newest action
stamp is the resolve's clock) or at beat `ends_beat` = lock beat + ceil(`duel_deadline_ms` / `flow_ms`) − 1,
whichever is first. The lock says `world: "beating"`; with holds it will say `"held"` and the duel ends at the hold's
close. Everything else below is the same either way (`ENGINE_HOLDS` in `duel.rs` flips it).

### 21.2 Blows

Each line a duellist types (`fields.duel {duel_id, text}`, at most 120 characters; clients relay the live typing
preview through the engine's `room.signal {kind: "typing"}`) is a blow, resolved at once by rules (an LLM per blow would
be far too slow):

- **The die:** a d20 seeded by the match and `<duel_id>:<n>`, against DC 10 (+1 at night, + half the opponent's
  wardrobe stage).
- **The modifier:** the situation (21.4: initiative, stances, openings), the line's **wit** (−3..+4: an action verb +1, a second verb +1, a body part +1, an item the
  duellist holds +1, a terrain or place word +1, the opponent's story or scars +1, at most +4; the same line again −2,
  nearly the same −1, under three words −1, gibberish −1), plus +1 hale (duel health 7+) or −1 hurt (3 or less), half
  the wardrobe stage and +1 with any item, +1 for a quick follow-up (within `duel_followup_ms`, 1.5 s, of its own last
  blow), +1 on home turf.
- **Damage:** 0 on a fail or a natural 1; 1 on a success, +1 beating the DC by 5, +1 with wit 3 or more (so 1–3); 4 on
  a natural 20; × `duel_damage_scale`.
- **Spam:** a blow sooner than `duel_min_gap_ms` (0.5 s, i.e. 2 a second) after its own last is a `penalty`: no die,
  and the tangled words cost their author 0.5.
- **Counters:** against an AI, or a human who hasn't blown for `duel_counter_ms` (3 s), each blow carries the
  opponent's automatic counter from the AI phrase bank (its wit capped at +2).
- **The flavor** is a rules template in the Lens's voice ("the line landed only because a heron had been washed in
  platinum dust at dawn"), and every blow carries **animation cues** (`anim`: each figure's move from a fixed
  vocabulary, chosen from the line's verbs, the verdict, its items and an army nearby; an element effect; a camera).

### 21.3 The end

A duel ends at a **knockout** (a duellist's duel health reaches 0), at its **deadline** (21.1), when a duellist
**falls** elsewhere, or at once when **both are built-in AIs**: `reason` `ko`, `deadline`, `fallen` or `auto`. Each
resolve **recomputes every duel deterministically** from the state's open duels and the ordered challenge and duel
actions (the live display is a cache that rolls lines over from beat to beat, never the record), so the live blows
and the resolve agree and a replay reproduces it. While it lasts, each resolve applies the damage so far, records the
dice and the invokes' bolts, and lists it in `round.outcome.public.duels_open[]` (`challenge.open`). At its end:

- every die is in `dice[]` (reason "a duel") and in the memory at the duel's hex;
- the damage lands; a knockout is an elimination by the usual rules (cause "a duel with …");
- the loser's loot falls on the duel's hex: everything at a knockout, else (beaten by 1+) a DC 11 roll to keep hold
  of its newest thing;
- a heavy duel leaves a scar (chapter 7); both duellists get the duel's **deed** as a tale; the hex remembers
  `duel.lock` and `duel.end` (the Lens: the next fight there is fought under that history);
- `round.outcome.public.duels[]` carries `challenge.end {reason, winner, loser, ko, damage, blows, typed, deed, log}`:
  the deed one line from the duel mind (LLM when a human fought, else seeded rules parts), Rube Goldberg style. A
  duel nobody typed in is fought automatically at its end.

### 21.4 Challenge mode: the flips

A duel is a series of interrelations and flips, so tiny posable figures (jointed wooden mannequins) can act it out
exactly (`crates/fn7-plugin/src/choreo.rs`).

**State:** the `range` (far, mid, close, grapple; a duel opens at mid, at close on the people level); the
`initiative` (who holds the momentum; the challenger first); each fighter's `stance` (neutral, guarding, committed,
off_balance, airborne, downed) and health; and the defence each has **set** (a guard, parry or evade waits for the
next attack).

**Move class** of each line, by rules from its words: `invoke` (a god's name: ignores range, costs a bolt paid at the
resolve), `command` (army words: needs an army near or 100+ followers), `throw`, `grapple`, `feint`, `parry`, `guard`,
`retreat`, `evade`, `heavy` (smash, crush, overhead, full force...), `close` (charge, rush, step in...), `taunt`,
else `strike`. What can't apply becomes what can: a strike or heavy from far is a close; a grapple short of close
range is a close; a throw outside a grapple is a strike; an invoke without a bolt or a command without an army is a
taunt.

**The d20 still decides success**, with two more modifiers: +2 holding the initiative, the attacker's stance
(off balance −2, committed −1, downed −3, airborne +1) and +2 when the defender is open (committed, off balance or
downed). **The matrix decides what a success means:**

| Attack | Defender's set | On a success | On a failure |
|---|---|---|---|
| strike | parry | parried, unless it beats the DC by 5 (then half damage) | parried |
| heavy | parry or guard | the defence breaks: full damage +1, defender off balance | (parry) parried |
| strike | guard | absorbed: −1 damage | nothing |
| feint | guard or parry | the set is wasted (defender off balance), initiative flips | nothing |
| strike | evade | missed unless it beats the DC by 3 | missed |
| heavy | evade | missed unless a natural 20; initiative flips | missed; initiative flips |
| grapple | — (close only) | range grapple, defender off balance | nothing |
| throw | — (grapple only) | +1 damage, defender downed, range mid | nothing |
| close / retreat | — | range one step closer / farther, initiative | nothing |
| guard / parry / evade | — | the defence is set (guarding; airborne for an evade), initiative | nothing |
| taunt | — | initiative, defender off balance | nothing |
| invoke / command | — | full damage (+1 for an invoke) | nothing |

**Parried:** the attacker goes off balance and the initiative **flips** to the parrier. A heavy always leaves its
user committed. Whoever lands takes the initiative; a landed strike at mid closes to close range. A knockout leaves
the loser downed.

**Choreography:** every blow and counter carries `choreo` (the range and initiative before and after, `flip`, the
result, each figure's move class, timed pose keys from a fixed vocabulary of 30 poses, the stance after, `contacts`
(which limb of whom hits which body part of the other, and when), the camera, 600–1400 ms), deterministic from the
rules and the strike's seed. The poses: idle_guard, step_in, lunge_extend, slash_high, slash_low, overhead_windup,
overhead_strike, parry_high, parry_low, block_cross, dodge_left, dodge_right, duck, leap_up, land, feint_shoulder,
grab_reach, grapple_lock, throw_hip, stagger_back, stumble, fall_back, kneel_hit, rise, cast_raise, cast_release,
command_point, taunt_beckon, victory, defeat. **Rules decide; an LLM may only phrase** (the deed).

### 21.5 The built-in AI

**The built-in AI** challenges its nearest rival with chance `duel_ai_chance` (0.02) a beat, decided at the beat's
start (`round.started.public.challenges`, so a human can fight back): directly when the rival is within 120 km,
else, within `duel_ai_reach_km` (2,000 km), it first **summons itself next to it** (the lock's `leap`: the summoning's
mishap d20 and health loss apply, and it fights at the reduced health). Two AIs fight it out at once; a
human the AI challenged gets the duel's whole span to answer. A duel nobody types in is fought automatically,
`duel_auto_rounds` (2) exchanges from the phrase bank.

### 21.6 No intent vanishes

Every order gets a visible ruling or a refusal (the match 79fd73bf and f18d76a3 fixes, schema 7):

*Rules 0.9.0 go further (chapters 23-27): heals, armings, raisings and curses are resolved by rules and dice, part by
part, and are never a bare refusal; what follows is how a 0.8.x match (no `wills_enabled`) still behaves.*

- **Heals** (heal, restore, mend, cure...) go to **Nature** even when the hand names no god (Death when a target has
  fallen): an invocation with its normal price and a real verdict; unless refused each target heals 1 + magnitude/4
  (1–3, half when twisted). "me", "my demigods", "all my demigods" are the player's own two; rivals are healed only
  when named. (Live: "all my demigods are healed fully" came back from the hand as a god route naming no god, and
  became a world act that did nothing for 20 beats.)
- A god route naming no god in the match goes to **Entropy** (a visible ruling), never silently to the world.
- **A fallen player's will** (every demigod of theirs down) is never dropped. A plea to come back ("my guys come back
  to life!": revive, raise, resurrect, come back...) is an invocation of **Death** by their first demigod for all
  their fallen demigods, magnitude 6, ruled like any other (Death's price; with an empty purse Death twists or
  refuses, and says so); any other order is refused, because the fallen can't act, only plead. Either way the
  player's private outcome carries `wills[] {text, routed_to, verdict, speech, reason}`.

**Worked example.** Engine: Ash's Left Hand types "I sweep your ankle into the harbour mud" (verb, body, terrain: wit
+3, hale +1: modifier +4) and rolls 13 against DC 10: success by 7, wit 3: 3 damage. DM: "The line landed on Vex's
ankle only because the third crow of the morning had been washed in platinum dust at dawn."

### 21.7 Watching the words arrive, and the skins

While a duellist types, the table sees the line grow as a row of stars with only its newest letters showing, the way
a password field shows the last key (`duel.typing`: the line's length and its newest 0 to 8 characters, relayed the
moment the server gets them). Read your opponent letter by letter as it flashes past and you know what is coming; look
away and you are winging it. The relay decides nothing: the line that counts is still the one sent on Enter, and the
keystrokes are never stored, never replayed to someone who reconnects, never part of a recording's outcome. Each
fighter also steps into the ring in one of five skins: wood, obsidian, gold, dragonscale, butterfly. A demigod's name
picks it (the same name is always the same skin, in every match and on every client), unless it carries a thing a god
granted (dragonscale) or a thing of gold (gold).

### 21.8 The AI types too, words pick moves, one duel at a time (state schema 12)

An AI duellist used to strike only as an automatic counter and nobody saw it type. From state schema 12, in any duel
with a clock, **the AI acts in every input window**: when the window opens the rules pick its move (from the range,
the initiative, the stances, the defence its opponent has set and the opponent's last move; `duel_ai_aggression`
0.6) and write it a line of its own, a sentence of 20 characters or more in its voice. The line is published at once
as a typing script (the text, a speed of 6 to 9 characters a second seeded per demigod, a pause or two, and the
moment it presses Enter, at least 400 ms before the window closes), which every client plays back as masked typing;
the panel that opens the window lasts as long as the line needs. At its Enter the line is a blow by the same d20 and
matrix as anyone's, with its wit capped at 2 and `duel_ai_handicap` (5) off its modifier against a human, because it
strikes in every window and no human can; the handicap is waived against a human silent for 12 s and between two
AIs. An AI that typed in a window does not also counter in it, and its line never draws an automatic counter for an
idle human: who types nothing only gets hit. Measured over 400 simulated duels a row at the defaults, a human typing
good lines wins about 9 in 10 at 84 words a minute, 2 in 3 at 60 and 1 in 3 at 42; one who types nothing loses
every time (`docs/CLIENT_API.md`, "AI duellists"; `duel_ai_lines_per_window` 0.5 lifts the slow typist to about
half). Two AIs type their duel out window by window to a knockout or the deadline. In the same schema a line's words
choose its move class and its clip, typos and one-word commands included ("atack!!", "jimpkick!", "plead! jump": the
last move word is the move, the earlier ones are setups; a throw works from close range), a player fights one duel
at a time (`busy:player`), and a duel closes at the first action stamped past its deadline, which shows the outro
and a provisional `challenge.end` at once; the resolve applies it as before.

## 22. Appendix: the DM's settings

Room settings (validated in `crates/fn7-plugin/src/settings.rs`, listed by `GET /sim/settings`). The 0.8.0 and 0.8.1 ones:

| Setting | Default | Range | Rule |
|---|---|---|---|
| `beats_per_day` | 8 | 2–200 | 5 |
| `dm_reach_km` | 30 | 0.1–2000 | the step radius (8.2, 9, 11) |
| `dm_encounters_per_beat` | 1 | 0–8 | 8.1 |
| `dm_encounters_max` | 10 | 0–64 | 8.1 |
| `dm_encounter_fade_beats` | 20 | 1–1000 | 8.1 |
| `dm_encounter_damage_scale` | 1.0 | 0–10 | 8.2 |
| `dm_settlement_step_beats` | 6 | 1–100 | 6 |
| `dm_road_wear` | 4.0 | 0.5–1000 | 10 |
| `dm_road_decay` | 0.05 | 0–10 | 10 |
| `dm_shrines_per_demigod` | 3 | 0–20 | 11 |
| `dm_shrine_spark_every` | 6 | 1–100 | 11 |
| `dm_shrine_heal` | 0.15 | 0–5 | 11 |
| `dm_shrine_ward` | 0.1 | 0–5 | 11 |
| `dm_memory_lines` | 6 | 0–30 | 3.4 |
| `fault_near_km`, `fault_far_km` | 150, 1500 | 0–5000, 1–20000 | 13 |
| `fault_near_magnitude`, `fault_far_magnitude` | 3, −2 | −10–10 | 13 |
| `fault_near_radius`, `fault_far_radius` | 1.5, 1.0 | 0.1–10 | 13 |
| `fault_near_damage`, `fault_far_damage` | 1.6, 0.6 | 0–10 | 13 |
| `fault_junctions_enabled` | true | bool | 13.1 (state schema 16) |
| `fault_junction_km` | 300 | 0–3000 | 13.1 |
| `fault_junction_magnitude` | 3 | −10–10 | 13.1 |
| `fault_junction_radius` | 1.5 | 0.1–10 | 13.1 |
| `fault_junction_damage` | 1.8 | 0–10 | 13.1 |
| `fault_tiny_mod_km` | 150 | 20–1500 | 13.1 |
| `summon_enabled` | true | bool | 19 |
| `summon_cooldown_beats` | 1 | 1–100 | 19 |
| `summon_loss_scale` | 1.0 | 0–10 | 19 |
| `god_power_cost` | 3 | 0–20 | 20 |
| `god_power_gift` | 1 | 0–10 | 20 |
| `challenge_enabled` | true (false in settings recorded before 0.8.1) | bool | 21 |
| `challenge_min_km` | 120 | 0–20000 | 21.1 |
| `challenge_contact_km_regions`, `_villages`, `_houses`, `_people` | 600, 15, 0.15, 0.015 | 0–20000 | 21.1 |
| `duel_cooldown_beats` | 1 | 1–100 | 21.1 |
| `duel_deadline_ms` | 30000 | 1000–600000 | 21.1 |
| `duel_damage_scale` | 1.0 | 0–10 | 21.2 |
| `duel_min_gap_ms` | 500 | 0–5000 | 21.2 |
| `duel_followup_ms` | 1500 | 0–10000 | 21.2 |
| `duel_counter_ms` | 3000 | 0–60000 | 21.2 |
| `duel_ai_chance` | 0.02 | 0–1 | 21.3 |
| `duel_ai_reach_km` | 2000 | 0–20000 | 21.3 |
| `duel_auto_rounds` | 2 | 0–10 | 21.3 |

The 0.9.0 ones (chapters 23-27):

| Setting | Default | Range | Rule |
|---|---|---|---|
| `wills_enabled` | true | bool | wills that matter (the feature gate; also needs state schema 7+): a will splits into its parts, healing and blessing your own never just fail, arming makes gear with real modifiers, anyone may plead for a resurrection, seals and curses exist and every one has a mystic counter-quest. Settings recorded before 0.9.0 lack it and read back `false` (old recordings replay unchanged). |
| `will_spark_per_bolt` | 5 | 1–20 | sparks one lightning bolt breaks into when a will's price is paid from bolts (the 1:5 economy; the change stays in the purse as sparks). |
| `will_own_dc` | 6 | 2–20 | the base DC of a will on your own demigods (heal, bless, arm): DC = this + magnitude/4 (an arming adds 2 per tier). A hostile act's is 8 + magnitude/2. |
| `will_debt_cap` | 3.0 | 0.0–20.0 | the open debt (health points, per demigod) beyond which the gods finance no more unpaid wills: the will becomes a mystic quest instead. |
| `will_debt_per_spark` | 0.2 | 0.0–5.0 | debt (to the ruling god) per spark of a will's price the purse could not pay. |
| `will_entropy_per_spark` | 0.25 | 0.0–10.0 | Entropy raised per spark paid for a will. |
| `heal_price_sparks` | 1 | 0–20 | a heal or blessing costs this + magnitude/4 sparks per target. |
| `heal_token` | 0.5 | 0.0–5.0 | the token heal of a failed heal roll (the rest waits on its mystic quest). |
| `heal_scale` | 1.0 | 0.0–10.0 | multiplier on every will heal (1 + magnitude/4 health per target before it). |
| `boon_beats` | 6 | 1–100 | beats a boon (a blessing, or a critical heal's gift) lasts. |
| `boon_bonus` | 1 | 0–5 | the d20 bonus a boon gives its demigod's acts. |
| `arm_price_artifact` | 2 | 0–50 | sparks an artifact-tier arm costs per demigod (a trinket is free). |
| `arm_price_relic` | 5 | 0–50 | sparks a relic-tier arm costs per demigod. |
| `arm_bless_sparks` | 5 | 0–50 | sparks a god's blessing on an arming costs (per will part): the gear is a tier up and carries the god's signature. |
| `arms_scale` | 1.0 | 0.0–5.0 | multiplier on every number an item's modifiers add (strike, damage, guard, regeneration, duel); 0 = gear is decoration. |
| `raise_price_sparks` | 5 | 0–100 | Death's base price in sparks to raise a fallen demigod, times 1 + the times it was raised before, plus 1 per raise_stale_beats it has lain dead. |
| `raise_stale_beats` | 8 | 1–1000 | every this many beats a demigod has been dead adds a spark to Death's price. |
| `raise_cap` | 3 | 0–50 | raisings one player may be granted per match (0 = none). |
| `raise_years` | 1.0 | 0.0–10.0 | max-health years a raised demigod loses, times 1 + the times it was raised before. |
| `raise_health_fraction` | 0.4 | 0.05–1.0 | the share of its max health a raised demigod returns with. |
| `curse_price_sparks` | 5 | 0–100 | sparks a curse or a seal costs (a bolt breaks into will_spark_per_bolt sparks). |
| `curse_god_price_sparks` | 15 | 0–200 | sparks the god-tier variant of a curse costs (strength 3, until countered). |
| `curse_beats` | 12 | 1–1000 | beats an ordinary curse lasts (a seal and a god-tier curse last until countered). |
| `curse_dc` | 10 | 2–30 | the DC of casting a curse (plus half the target's wardrobe stage). |
| `curse_max_per_player` | 2 | 0–20 | curses and seals one player may have standing at once. |
| `curse_entropy` | 3.0 | 0.0–100.0 | Entropy raised by a curse (a god-tier one raises entropy_per_mega). |
| `quest_steps_max` | 3 | 1–3 | the most steps a mystic quest has. |
| `quest_expire_beats` | 24 | 0–1000 | beats a heal's or an arming's quest stays open (0 = until done); a curse's counter-quest lives as long as its curse, a raising's until the Final Night. |
| `quest_spark_step` | 2 | 0–50 | sparks a quest's offering step asks for (times the quest's difficulty). |
| `wills_ai_chance` | 0.25 | 0.0–1.0 | how readily the built-in AI uses wills (heals when hurt, arms itself, pleads for its fallen sibling, curses a rival, pursues its quests); 0 = never. |
| `looks_enabled` | true | bool | gear looks (needs state schema 11): loadouts, a human's demigod's gear portraits and dioramas, item art (docs/MEDIA.md "Looks"). |
| `look_extra_items` | 2 | 0–6 | major items a loadout lists beyond the best item per slot. |
| `look_items_shown` | 3 | 1–5 | items a gear portrait or diorama shows at most. |
| `look_images_per_demigod` | 4 | 0–20 | new gear pictures one human player's demigod may get per match. |
| `look_debounce_beats` | 3 | 0–100 | beats between two new gear pictures of one demigod (the newest loadout wins). |
| `item_art_per_match` | 12 | 0–200 | item pictures granted per match. |

Every other setting is in `docs/PROJECT_GUIDE.md` §3.13.

---

## 23. Wills on your own: healing, blessing, and what a will is made of

> Healing and arming units with god powers currently gives them nothing, and the will should show why
> translating to actual mega-changes" ... "healing and arming do nothing" (Chris, BUILDLOG "wills that matter")

*Rules 0.9.0 (room setting `wills_enabled`, state schema 10; older matches keep the 0.8.x rules and replay unchanged).
Chapters 23-27 are one design: `crates/fn7-plugin/src/wills/`.*

**The live case this fixes** (match d3a3779e, beat 42). Chris, at 0 bolts, 4 sparks and 2.26 of debt, with demigods at
3.8 and 7.1 health, willed "every one of mydemigods is fully healed and gets rocket launchers". His hand answered
"Healing and arming both demigods now." Then: the heal went to Nature, who was told "they can't pay: refuse, or twist
it" and refused outright; the arming went to the ordinary resolver as "a deed", rolled a 2 against DC 11, and became one
line of narration ("only a bent bottle cap appears"); nothing was attempted for the Right Hand; and nothing told him
why. Under this chapter the same will is four rulings (heal x 2, arm x 2), each with its die, its price, its result in
plain words and, where it falls short, the reason and the way forward.

### 23.1 A will splits into its parts

A will is read for its **parts**: `heal`, `bless`, `arm`, `raise`, `curse` (or seal), `ask`, `offer`, `gather`. The
hand mind is asked for one intent per part; the rules then read the player's words again and add any part the hand
dropped, so the LLM path and the rules fallback agree. **Each part x each target is resolved on its own, with its own
visible ruling**, by rules and dice, before the ordinary route (the resolver and the god minds never see it). A will
is resolved once: it never becomes a standing order. A will's other acts ("go to Paris and heal me": the journey) go
their usual way.

"Me", "my demigods", "all", "both" mean your own two; a named demigod means that one; rivals count only when named.

### 23.2 Paying: sparks, bolts, credit

Prices are in **sparks**. The purse pays sparks first; then **a bolt breaks into `will_spark_per_bolt` (5) sparks**
and the change stays in the purse. What the purse still can't pay, **the ruling god lends**: a debt of
`will_debt_per_spark` (0.2) per spark, due in `debt_due_beats`, as long as the actor's open debt stays under
`will_debt_cap` (3). Past the cap the gods lend no more: the effect shrinks to what was paid, or becomes a mystic
quest (chapter 27). A curse of silence doubles every price (chapter 26). Entropy rises `will_entropy_per_spark` (0.25)
per spark paid. Text entry is still Time's: an order without a slow window costs Time its bolt first (12.2).

### 23.3 Healing your own (Nature rules it)

| | |
|---|---|
| Price | `heal_price_sparks` (1) + magnitude/4 sparks per target |
| DC | `will_own_dc` (6) + magnitude/4 for your own (a named rival: the hostile 8 + magnitude/2) |
| Modifiers (each named in the answer) | half the actor's wardrobe stage; its charm (chapter 24); a boon; a hex; the god's favor (+1 at +2 or more, -1 at -2 or less); paid in full +1, on credit -1, part paid -1; the Final Wish +2 |
| The heal | (1 + magnitude/4, 1 to 3.5) x `heal_scale` x (0.5 + 0.5 x the share paid) |

| The d20 | What happens |
|---|---|
| natural 20 | the heal x 1.5, **and a boon**: `boon_bonus` (+1) on that demigod's d20s for `boon_beats` (6) |
| success | the heal |
| fail | a **partial**: `heal_token` (0.5) now, **and a mystic quest** that completes the rest |
| natural 1 | a fizzle, **and the quest** for all of it |
| nothing paid and nothing lent | no roll: the quest alone. Never silence. |

A demigod already whole costs nothing. A withered one can't be healed: the answer says so and shows the curse's
counter-quest.

### 23.4 Blessing your own

"Bless me", "empower my hands": the same price, DC and modifiers, and the effect is a **boon** (`boon_bonus` on its
d20s for `boon_beats`; a natural 20: one more and half again as long; a fail: 2 beats and a quest for the full boon; a
natural 1: the quest). Blessing words spoken with an arming belong to the gear (chapter 24).

### 23.5 The answer

Every will returns in the player's `round.private.wills[]`: the words, the gods that ruled, the gods' words
(rules-written), a verdict (`granted`, `partial`, `refused`) and `parts[]`, one per part and target: the roll against
the DC with every modifier named, what was paid, what happened in plain words, and when it fell short, **why** and
**what would work** (the quest's steps, or the price). Every will is also a private `will.ruled` entry in the DM's
memory. Acts that are public (a raising, a curse landing) add a public one-liner.

**Worked example** (a real answer, from the end-to-end run). Engine: Chris's Left Hand (4.0 health) and Right Hand
(6.8) hear "heal and arm my demigods". Heal, Left: 2 sparks paid, d20 8 + 1 (paid in full) = 9 against DC 7: healed 2.
Arm, Left: an artifact, 2 sparks, d20 14 against DC 8: "forged a Butterfly Wing Blade (artifact): +1 to strikes, +1 to
journeys, +0.5 damage, +1 in duels". DM (Nature, then Death, the smith's patron): "Granted: Chris's Left Hand mends,
Chris's Right Hand mends, because a ferryman overpaid his toll in a year nobody remembers." "Granted: Chris's Left
Hand is armed, Chris's Right Hand is armed, because the third crow of the morning landed on the right side of the
road." Three beats later a heal rolls a natural 1: "the mending of Chris's Left Hand fizzled (a natural 1); a quest
will complete it (+2)", why: "the ferryman kept the change", next: "Hold a thread pulled from a general's banner".
Chris wills "gather the thread for the quest"; the quest is done that beat and the 2 health arrive.

## 24. Arms and their powers

Arming is the item engine (chapter 16) with teeth: gear is an item (`it:`, with its `mf:` row), and **an item does
what its kind, tier, material and blessing say**. Nothing is stored for that but the powers already spent.

### 24.1 Arming your own

"Arm my demigods", "they get rocket launchers", "give us shields": gear for each of your own standing demigods.

- **What:** the gear the words name (a weapon, armour or a charm: "rocket launchers" is a weapon, the Rocket
  Launcher), else the slot the demigod lacks (weapon, then armour, then a charm), in a seeded material.
- **Tier asked:** by magnitude: 0-3 trinket, 4-6 artifact, 7+ relic ("legendary", "fully", "god powers" read as 8).
- **State schema 11:** kit words ("equipment", "gear", "armaments", "weaponry", "arsenal") and needing or wanting a
  piece of gear ("my guys need swords") are an arming too. The recorded "my demigods need crazy powerful equipment! do
  that!" armed nobody under schema 10, which replays as recorded.
- **Price per demigod:** trinket free, artifact `arm_price_artifact` (2) sparks, relic `arm_price_relic` (5). What
  can't be paid is lent (23.2); past the cap the tier comes down to what was paid, and the answer says what the tier
  asked for costs.
- **The d20:** DC `will_own_dc` (6) + 2 per tier (6, 8, 10). Natural 20: a tier up. Success: the tier paid for.
  Fail: **a tier low, and a quest that tempers it** to the tier paid for. Natural 1: nothing appears, and a quest
  that forges it.
- **God-blessed gear:** a god named in the will ("Nature, arm my demigods"), or blessing words ("holy", "divine",
  "god powers"), costs `arm_bless_sparks` (5) more per demigod: the gear is **a tier up** and carries **that god's
  signature**. A **god-tier power** (chapter 20) aimed at your own standing demigod also arms it: wonder-tier gear
  with that god's signature, once per god per demigod (Death arms its own instead of striking it).

### 24.2 What gear does

| Slot | trinket | artifact | relic | wonder+ | Once a match (relic+) |
|---|---|---|---|---|---|
| **weapon**: strike d20 / damage per landed strike / duel | +1 / 0 / 0 | +1 / +0.5 / +1 | +2 / +0.5 / +1 | +2 / +1 / +2 | **Smite**: its first strike landed on a rival demigod deals 2 more |
| **armour**: turned aside per hit / resists / duel | 0.25 / - / 0 | 0.5 / its material's type / 0 | 0.75 / type / +1 | 1.0 / type / +1 | **Last Stand**: the first blow that would fell its bearer leaves it at 1 health |
| **charm** (relic, artifact): other deeds' d20 / mending a beat | 0 / 0 | +1 / 0.1 | +1 / 0.15 | +2 / 0.2 | **Second Wind**: the first beat ended at 3 health or less, it heals 2 |

- **Materials** (in the name, else seeded): obsidian and dragonscale (fire), butterfly-wing (storm; +1 on journeys),
  platinum (curses: +2 to the DC of cursing its bearer), struck-oak (lightning), moonsteel (cold), whalebone (water),
  salt-glass (poison), gold (blades), heron-feather (falls; +1 on journeys). Armour of artifact tier and up halves
  damage of its material's type.
- **Signatures:** Time +2 on journeys; Nature +0.2 mending a beat; Consequence thorns (0.5 back at a demigod who
  strikes its bearer); Death Last Stand, whatever the slot; Chance +1 on every d20; Memory +1 in duels; Entropy +0.5
  damage.
- **Bounded:** a demigod fights with its best item per slot, and its whole kit is capped at +3 on a d20, +1.5
  damage, 1.5 turned aside (never more than half of one hit), 0.4 mending, +3 in duels; all x `arms_scale`.

### 24.3 Where the numbers are used

Act d20s (chapter 2: strikes and threats use the weapon, journeys the fleet gear, other deeds the charm); every
damage, heal and raising on its way into a merge (the weapon adds, the armour turns aside, then a resisted type
halves; thorns bite back); the duel's gear term (21.2: "+1 with any item" plus the kit's duel bonus); the end of the
beat (mending, Second Wind); Last Stand after the merge and the society, before the DM's step (so the gear never
drops). Gear shows what it does in its payload (`slot`, `material`, `mods`, `power`, `blessed_by`, `does`) and on the
piece's story pegs.

**Worked example.** Engine: a relic Obsidian Spear (+2 strikes, +0.5 damage, Smite unspent) lands a strike of 2 on a
demigod behind an artifact Gold Shield (turns aside 0.5, resists blades): 2 + 0.5 + 2 (Smite) - 0.5 = 4, halved to 2.
The next strike, Smite spent: (2 + 0.5 - 0.5) / 2 = 1. DM: "The spear found the one seam in the shield that the
goldsmith had closed on a Tuesday; the shield has since learned."

## 25. Resurrection

> "i should.. be able to revive the other demigods. a player even if their demigods were wiped should be able to try
> to resurrect their demigods unless the other player cast a no resurrect" (Chris)

Any player may plead for any fallen demigod: their own, an ally's, anyone's ("revive the other demigods": every
fallen one, two a will, your own first). **A wiped player may plead too**: the fallen can only plead, ask about their
quests and make offerings; any other order is refused with what they can do. (So that the plea can arrive at all, a
wiped player is no longer reported out to the Theatre until the match ends: the core refuses every submission of a
seat that is out.)

It is **Death's ruling**, and Death keeps a price list:

- **Sparks:** `raise_price_sparks` (5) x (1 + the times that demigod was raised before) + 1 per `raise_stale_beats`
  (8) it has lain dead.
- **Years:** `raise_years` (1) x (1 + the times raised) off its max health, for good.
- **All of it or none:** Death takes no deposits. Paid (or lent, 23.2): it rises at once. It can't be paid: **a
  quest** (public; difficulty 2, +1 per earlier raising, +1 for another player's demigod, at most 3; the fallen keep
  vigils), and a demigod raised by a quest **owes Death the sparks as a debt** (5 x 0.2 = 1.0).
- **How it returns:** at `raise_health_fraction` (0.4) of its (lowered) max health, where it fell, or at its shrine
  if it has one. `demigod.raised`, public.
- **Limits:** the price doubles, triples...; `raise_cap` (3) raisings per player per match; a seal (chapter 26)
  refuses it until the seal's counter-quest is done.

**Worked example** (from the end-to-end run). Engine: both of Chris's demigods fall; a rival seals the Left Hand's
grave. Chris wills "my demigods come back to life!". Right Hand: 5 sparks paid, it "rises at 3.6/9 health (paid: 5
sparks, 1 years of its life)". Left Hand: "can't be raised: a seal of no resurrection lies on it", next: "quest q:2:
1. Wait for the next dusk." DM (Death): "Chris's Left Hand is sealed; Chris's Right Hand may get up. I keep a price
list and a list of errands; choose." The dusk comes, the seal lifts (`curse.broken`), and the next plea raises the
Left Hand for 6 sparks (one more for the beats it lay dead).

## 26. Seals and curses

A rival may lay a seal or a curse on a demigod. They are first-class objects in the state: an owner, a target, a
strength (1-3), a duration or "until countered", what was paid, a public face and a private one, and **always a
counter**.

| Kind | On | What it does | God-tier through |
|---|---|---|---|
| `seal` ("no resurrection", "stay dead", "seal the grave of...") | a demigod, fallen or living (it bites when it falls) | NO RESURRECTION: nothing raises it, by any road | Death |
| `wither` ("wither", "no healing", "blight") | a living demigod | no healing reaches it: wills, shrines, gear, gods | Death |
| `silence` ("silence", "mute", "gag") | a living demigod | its player's wills cost double | Memory |
| `bind` ("bind", "chain", "root") | a living demigod | it can't be summoned (`bound:<quest>`) | Time |
| `hex` ("hex", "curse", "jinx") | a living demigod | minus its strength on that demigod's d20s | Chance |

- **Whom:** the rival the words name; unnamed: the nearest standing rival (a seal: the newest grave). The weak words
  (seal, bind, silence...) curse only when they name a demigod; "silence the crowd" is an ordinary order.
- **Price:** `curse_price_sparks` (5). The **god-tier variant** (a god named, "forever", "eternal", "until
  countered") costs `curse_god_price_sparks` (15): strength 3 and until countered. Paid whole (or lent), or not cast.
- **The d20:** DC `curse_dc` (10) + half the target's wardrobe stage (+2 against platinum gear), with the caster's
  modifier (2.3), boons and hexes, +2 for a god's hand. Natural 20: strength +1 and half again as long. Success:
  strength 2 for `curse_beats` (12); a seal holds until countered. Fail: it takes, weakly: strength 1 for half the
  time. **Natural 1: it turns on its caster.**
- **Limits:** one of a kind on a target; `curse_max_per_player` (2) standing per player. Entropy +`curse_entropy`
  (3); a god-tier one +`entropy_per_mega`.
- **Faces:** everyone sees the mark on the target's piece (kind, strength, until when) and a public line ("A curse
  of withering settles on..."), never who cast it. The bearer's seat sees the owner and the counter's steps; the
  owner's seat sees its curse and the counter's progress.
- **Always a counter:** the moment a curse is made, the DM gives its bearer a mystic quest (chapter 27) with as many
  steps as the curse is strong. Done, the curse breaks (`curse.broken`, public). A seal's counter is public, and
  whoever pleads for the sealed demigod may take it up. A curse whose time runs out fades (`curse.broken`, faded) and
  its quest lapses.

**Worked example** (from the end-to-end run). Engine: Benito wills "curse Chris's Right Hand with withering": 5
sparks, d20 9 + 1 = 10 against DC 10: strength 2. Chris's "heal Chris's Right Hand" answers "can't be healed: a curse
of withering lies on it", next: "1. Offer 4 sparks. 2. Stand within 150 km of a fault line by night." DM (Death):
"4 sparks fed to the dark between two heartbeats, then your shadow laid down by night where the ground remembers
being two grounds; only then will the curse of withering lift from Chris's Right Hand." Chris offers the sparks,
summons a demigod to a fault line, night falls: the curse breaks.

## 27. Mystic quests

> "mystic quest wings of a butterfly to counter a curse" (Chris)

A mystic quest is the Lens and the Engine (chapter 0) as something a player can do: the engine's answer was "not
yet", and the quest is the absurd chain of causes that would make it "yes".

- **Made by rules.** 1-3 steps, seeded by the match and the quest's number, from the tables below and the place x
  time memory; only steps the seat can actually do (the fallen get vigils and offerings; never a place a demigod
  already stands at, or an ingredient it already holds). Then told in one sentence, Rube Goldberg style, ending with
  the engine's reason as near as the DM can read it (a settled fact about the place).
- **Steps the engine verifies**, in order, each beat after the DM's step, from what actually happened:

| Step | Done when |
|---|---|
| `at` | a standing demigod of yours stands at the place: a shrine of the one who cursed you (or one of your own), a settlement of level N or more, within 150 km of a fault line, on a coast, or a spot the DM remembers (within 150 km); with a `phase`, at dawn / day / dusk / night; with an `offer`, holding that ingredient there, and it is burned |
| `hold` | one of yours holds an item whose name carries the ingredient's word |
| `encounter` | one of yours wins an encounter (chapter 8) |
| `shrine` | one of yours raises a shrine (chapter 11) |
| `duel` | one of yours comes out of a duel alive (chapter 21; only for strength-3 quests) |
| `sparks` | you will "offer the sparks to the quest": `quest_spark_step` (2) x its difficulty |
| `army` | an army of yours (its general named in the telling) stands within 300 km of the place |
| `vigil` | the hour has come N times (N > 1 only for the fallen) |

- **The odd ingredients:** the wings of a butterfly that has crossed a named strait, a stone washed in platinum dust,
  a heron's feather, salt from a dried sea, a whale-tallow candle, the ash of a twice-struck oak, a jar of dawn fog,
  a bell without a tongue, river glass, a thread from a general's banner. Will your demigod to gather or make one
  (a d20 against DC 6; only a natural 1 loses it), or win an encounter while a quest is waiting on one: it drops.
- **Asking:** "what does my quest need?", "how do I break the curse?" answers with every open quest's steps and
  progress, and every curse on your demigods.
- **Stored and remembered:** quests live in the state; `quest.given`, `quest.step`, `quest.done` and `quest.failed`
  go into the DM's memory at the place of the demigod they are for: public for a raising and a seal's counter, else
  only for the seats concerned.
- **Expire or stay:** a heal's or an arming's quest lapses after `quest_expire_beats` (24); a curse's counter lives
  as long as its curse; a raising's until it is done (or the demigod stands again).
- **On completion** the effect applies at once: the heal completes, the boon settles, the gear is tempered or
  forged, the curse breaks, the demigod rises (unless a seal still holds it: then the quest waits and says so).
- **The built-in AI** (`wills_ai_chance`, 0.25) heals when hurt, arms itself, pleads for its fallen sibling, curses
  a near rival now and then, and pursues its quests' steps.

**Worked example** (the designer's own). Engine: a hex lies on Ash's Left Hand; its counter is `hold butterfly`, then
`at` the curser's shrine, `phase` dawn, `offer` butterfly. DM: "The wings of a butterfly that has crossed the Sunda
Strait, burned at dawn on the shrine of the one who cursed you; only then will the hex lift." Ash wills "catch a
butterfly and take its wings"; walks to the shrine; at noon the step says "you stand there, but it is day, not dawn";
at the next dawn the wings burn, and everyone sees `curse.broken`.

## 28. The Earth (state schema 13)

The god powers no longer only hit what they are aimed at: every cast, god-tier power and physical ramification
starts a **phenomenon** on a simulated voxel field of the whole planet (docs/EARTH_SIM.md has every number). The DM
reads the field; it never invents it.

- **The field.** One column per regions hex (120 km): height in 16 levels (sea level 8), a material, and what is on
  it (water, lava, ash, fire, snow, cloud, heat, cracks, ruin ...). The base terrain is the same in every match (the
  land mask, the plate boundaries of chapter 13 and a fixed hash); only what differs is stored.
- **Phenomena.** Storm, lightning, flood, wildfire, volcano, earthquake, meteor, tidal wave, raise and sink land,
  forest, swarm, blizzard and a drifting plate. A quake ruptures **along the nearest fault**; a volcano's cooled lava
  **raises the land** (an island from the sea); a plate patch drifts along its boundary with a rift behind and a ridge
  ahead. They meet in the cells they share: a flood quenches a fire into steam, lava in water turns to rock and steam,
  a quake near a volcano wakes it, a storm feeds a flood, a fire feeds a storm, ash cools the air.
- **Entropy** (chapter 12.4) quickens every spread, adds sub-steps (the timeline runs faster as the world comes apart),
  stresses every fault until the world starts its own quakes, eruptions and drifting plates, and at heat death makes
  everything live flare each beat.
- **Consequences go through the rules.** A demigod in a burning, flooded, shaking or lava-filled cell takes one bounded
  hit a beat, named for its worst part, so armour that resists the element halves it (chapter 14). The worst-hit crowds
  fear the gods (and some die). Ruin pushes a settlement's level target down (one level per 96 points, three if the sea
  took it) and the settlement steps down one level at a time (chapter 6). A rift, lava or deep water over a road's end
  cuts the road (chapter 10); hard shaking, lava, drowning or an impact topples a shrine (half its worshippers leave,
  chapter 11). A rupture, a crater or new land uncovers a marked hex (chapter 8).
- **Memory** (chapter 3). The DM remembers the field's events by place and beat: `earth.quake`, `earth.eruption`,
  `earth.drowned` ("the coast of Sumatra drowned at beat 31"), `earth.land_rose`, `earth.plate_moved`, `earth.impact`,
  `earth.wave`, `earth.omen` (the world's own), `earth.road_cut`, `earth.shrine_toppled`, and `earth.settled` when old
  quiet scars return to base under the cap. At most `earth_events_per_beat` (6) a beat, public.
- **Zooming in** (chapter 4.3). A hex below the regions level is derived from the coarse cell under it, seeded by the
  hex: the same hexes burn every time anyone looks. An observed hex records its field values and the field's weather.
- **Time.** The field is a pure function of the seed, the settings, the casts with their beats and the Entropy meter;
  a keyframe every 16 beats lets any past beat be rebuilt by re-simulating at most 16 beats. A fork continues the same.
- **The earth underfoot (state schema 17).** The DM is told the ground wherever it speaks of a place: every prompt that
  acts there (a hand's orders, the resolver, a subagent, a god's ruling, Entropy's ramifications, a general, the
  narrator, a duel's deed) carries one short factual line built from the field ("The earth here, now: waist-deep
  floodwater, the flood came 2 beats ago"; "cooling black lava, still glowing, smoke"; "the land here rose 3 levels
  since beat 12"), and a duel's stage stands on it (`ground`, hazard props). Calm ground adds nothing. The line is the
  engine's answer: the DM never contradicts it and makes the rest fit, Rube Goldberg style (chapter 0): the punch
  landed only because the floodwater had carried the rival's boots off at dawn; Entropy's flood on fresh lava is told
  as steam and new black rock because that is what the field will do with it (docs/EARTH_SIM.md §12).

## 29. Token play (state schema 19)

Benito described typing an in-game action, such as going into the house to the left and approaching its occupant, and wanting to see it play out
outwiththelittlegametokens". What a player's will does at a place is **played out by the game tokens** on the
people-level board (docs/CLIENT_API.md "Token play" has the payload).

- **After the ruling, never before.** The DM rules first (the d20 of chapter 3, the society's sweet-talk and bribes,
  the gods' verdicts, chapter 0's Rube Goldberg tale); the scene is drawn afterwards from the ledger rows the will
  produced and the recorded die. A scene can never make a failure a success or add a consequence: it decides nothing.
- **One scene per demigod per beat**: its hand's act with the biggest row (then the one that was rolled for). Room
  setting `token_play`: every demigod (`all`, the default: it is rules only and costs nothing), seated humans only
  (`humans`), or nobody (`off`).
- **The stage is the place × time memory.** The demigod stands on its 3 m hex; "the house to my left" is the nearest
  hex to its west (then the other sides, ring 1 then ring 2) that the memory says has buildings (chapter 2's
  extrapolated `cell_state`, or an observation). If only the 30 m hex around it has houses, the scene picks the
  neighbour on the asked side and **records it** (`place.building`): from then on that hex is a house for everyone,
  every beat. No houses near: the scene plays in the street.
- **The cast is the society's.** The person the ruling names (a memorized character) plays herself, from where she
  really is when that is within 4 hexes (else she stands 2 hexes off), or inside the house the scene enters. A social
  act with nobody named brings forward the nearest known person within 50 m, else the society materializes one from a
  crowd of the place, the same way every mention is resolved (chapter 4 of the society): she is remembered and can be
  addressed by name next beat. Crowds, armies, rival demigods and uncovered beasts get tokens too.
- **The outcome is the record's.** Success, partial (a fail that still landed half), failed, twisted (a Final
  Destination accident, a god's twist): a seduction that worked ends in an embrace; a natural 1 ends in a slap and
  she runs out; a failed search ends with "finds nothing". The die's face, modifier and DC ride along so the client
  can tumble the same d20 the record holds.
- **Public, so built from public things.** The order's words never appear; subagents', possessions' and secret god
  rulings' acts make no scene.
- **Captions** are the rules' templates. A room may let a small model (`token_play_llm`, off by default) rewrite the
  wording of human seats' captions afterwards; it sees only the steps and the public outcome line, may only reword,
  and its answer is recorded, so replay never asks a model.

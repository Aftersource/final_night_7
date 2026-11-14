# The Dungeon Master's Rulebook

*Final Night 7, rules 0.8.1, state schema 6.*

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
   byte. Every 0.8.0 rule is gated on state schema 6, so schema-4 and schema-5 recordings replay unchanged.

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
`fields.challenge {demigod: "dg:mine", target: "dg:N", level}` (the level the board shows: regions, villages, houses or
people). **Forced:** there is no consent. Valid when (checked on arrival and again at the resolve, in arrival order):

| Refusal | When |
|---|---|
| `not_yours` | the demigod isn't the sender's |
| `fallen` | either demigod has fallen |
| `bad_target` | the target is no rival (another seat's) |
| `too_far` | great-circle distance > the level's hex size × `duel_reach_factor` (1.5): regions 180 km, villages 4.5 km, houses 45 m, people 4.5 m |
| `busy` | either is already in a duel this beat |
| `cooldown` | its last challenge was under `duel_cooldown_beats` (1) ago |

Accepted, everyone sees `challenge.lock`: both duellists (name, portrait and diorama, **both healths**: the duel's
health bars are the one place a rival's health is shown), the ground (the target's position), its hex at the level,
the camera `focus`, and the duel's id `duel:<beat>:<a>:<b>`.

**The world holds its breath.** By design a duel freezes the world: no beats resolve, timers pause and only the two
duellists act, until a knockout or `duel_deadline_ms` (30 s). That needs an engine hold (requested and announced in
the lock's `freeze`); until the Theatre offers it, the world keeps beating and the duel ends at the next resolve (or its
deadline, whichever comes first). Everything below is the same either way.

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

A duel ends at a **knockout** (a duellist's duel health reaches 0) or at the resolve. The resolve **recomputes every
duel deterministically** from the ordered challenge and duel actions on the committed state (the live display is a
cache, never the record), so the live blows and the resolve agree and a replay reproduces it:

- every die is in `dice[]` (reason "a duel") and in the memory at the duel's hex;
- the damage lands; a knockout is an elimination by the usual rules (cause "a duel with …");
- the loser's loot falls on the duel's hex: everything at a knockout, else (beaten by 1+) a DC 11 roll to keep hold
  of its newest thing;
- a heavy duel leaves a scar (chapter 7); both duellists get the duel's **deed** as a tale; the hex remembers
  `duel.lock` and `duel.end` (the Lens: the next fight there is fought under that history);
- `round.outcome.public.duels[]` carries `duel.end {winner, loser, ko, damage, blows, deed, log}`: the deed one line
  from the duel mind (LLM when a human fought, else seeded rules parts), Rube Goldberg style.

### 21.4 Challenge mode: the flips

A duel is a series of interrelations and flips, so tiny posable figures (jointed wooden mannequins) can act it out
exactly (`crates/fn7-plugin/src/choreo.rs`).

**State:** the `range` (far, mid, close, grapple; a duel opens at mid, at close on the people level); the
`initiative` (who holds the momentum; the challenger first); each fighter's `stance` (neutral, guarding, committed,
off_balance, airborne, downed) and health; and the defence each has **set** (a guard, parry or evade waits for the
next attack).

**Move class** of each line, by rules from its words: `invoke` (a god's name: ignores range, costs a bolt paid at the
resolve), `command` (army words: needs an army near or 100+ followers), `throw`, `grapple`, `feint`, `parry`, `guard`,
`retreat`, `evade`, `heavy` (smash, crush, overhead, full force...), `close_in` (charge, rush, step in...), `taunt`,
else `strike`. What can't apply becomes what can: a strike or heavy from far is a close_in; a grapple short of close
range is a close_in; a throw outside a grapple is a strike; an invoke without a bolt or a command without an army is a
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
| close_in / retreat | — | range one step closer / farther, initiative | nothing |
| guard / parry / evade | — | the defence is set (guarding; airborne for an evade), initiative | nothing |
| taunt | — | initiative, defender off balance | nothing |
| invoke / command | — | full damage (+1 for an invoke) | nothing |

**Parried:** the attacker goes off balance and the initiative **flips** to the parrier. A heavy always leaves its
user committed. Whoever lands takes the initiative; a landed strike at mid closes to close range. A knockout leaves
the loser downed.

**Choreography:** every blow and counter carries `choreo` (the range and initiative before and after, `flip`, each
figure's move class, timed pose keys from a fixed vocabulary of 30 poses, the contact limb and target limb, the stance
after, the camera, 600–1400 ms), deterministic from the rules and the strike's seed.

### 21.5 The built-in AI

**The built-in AI** challenges its nearest rival with chance `duel_ai_chance` (0.02) a beat, decided at the beat's
start (`round.started.public.challenges`, so a human can fight back): directly when the rival is within regions reach,
else, within `duel_ai_reach_km` (2,000 km), it first **summons itself next to it** (the lock's `leap`: the summoning's
mishap d20 and health loss apply, and it fights at the reduced health). A duel nobody types in is fought
automatically, `duel_auto_rounds` (2) exchanges from the phrase bank.

**Worked example.** Engine: Ash's Left Hand types "I sweep your ankle into the harbour mud" (verb, body, terrain: wit
+3, hale +1: modifier +4) and rolls 13 against DC 10: success by 7, wit 3: 3 damage. DM: "The line landed on Vex's
ankle only because the third crow of the morning had been washed in platinum dust at dawn."

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
| `summon_enabled` | true | bool | 19 |
| `summon_cooldown_beats` | 1 | 1–100 | 19 |
| `summon_loss_scale` | 1.0 | 0–10 | 19 |
| `god_power_cost` | 3 | 0–20 | 20 |
| `god_power_gift` | 1 | 0–10 | 20 |
| `challenge_enabled` | true (false in settings recorded before 0.8.1) | bool | 21 |
| `duel_reach_factor` | 1.5 | 0.5–10 | 21.1 |
| `duel_cooldown_beats` | 1 | 1–100 | 21.1 |
| `duel_deadline_ms` | 30000 | 1000–600000 | 21.1 |
| `duel_damage_scale` | 1.0 | 0–10 | 21.2 |
| `duel_min_gap_ms` | 500 | 0–5000 | 21.2 |
| `duel_followup_ms` | 1500 | 0–10000 | 21.2 |
| `duel_counter_ms` | 3000 | 0–60000 | 21.2 |
| `duel_ai_chance` | 0.02 | 0–1 | 21.3 |
| `duel_ai_reach_km` | 2000 | 0–20000 | 21.3 |
| `duel_auto_rounds` | 2 | 0–10 | 21.3 |

Every other setting is in `docs/PROJECT_GUIDE.md` §3.13.

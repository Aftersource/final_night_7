# Final Night 7: Game Design Document

*Epic AI MegaJam 2026, theme "Director's Cut". Designers: **Chris** and her brother **Benito**. Unreal client
(UE 5.8.4, Win64) on a Rust server built on **The Theatre**, a server-authoritative engine for games where players act
in plain language and an LLM decides what happens.*

Every rule and number below comes from the rulebook (`docs/DM_RULEBOOK.md`, rules 0.9.0, chapters 0-29), the build log
(`BUILDLOG.md`), the server docs in `docs/` and the Unreal repo's design docs. Section references like "RB 21" mean
rulebook chapter 21. Where something is designed but not finished, it says so.

---

## One page

**Pitch.** Seven gods sit at one table over a floating globe of the real Earth. Each player is a god with two demigods
on the board, and the only controls that matter are the words you type and the lightning you throw. An AI Dungeon
Master runs the world and never waits for anyone.

**The loop, in five bullets**

1. **The world beats.** Every 3 seconds a beat resolves, whether or not anyone typed (RB 1, RB 5). 60 beats to the
   Final Night.
2. **Type a will.** Your typed order slows time for everyone, and your Enter cuts off anyone slower. Time takes a bolt
   as a toll (RB 12.2).
3. **Throw power.** Mega powers (bolts) shake the map; mini powers (sparks) hit one spot. Every mega gifts each rival a
   bolt and pushes the world toward heat death (RB 12.3, 12.4).
4. **Fight will against will.** Double-click a rival to lock both demigods into a 30-second typed duel where every line
   is a blow judged by a d20 and your wit (RB 21).
5. **Live with the consequences.** The DM remembers every decision by place and time, the simulated Earth floods and
   burns, and the last god with a demigod standing wins the Final Night (RB 3-4, 17, 28).

**What makes it new**

- **Typing is the real-time skill.** Speed and wit decide the cut-off and the duels; an LLM never overrules a die.
- **History is a lens.** The rules and the dice decide first; the LLM's job is to make the result make mystical,
  Rube Goldberg sense, using what the world remembers (RB 0).
- **One deterministic world, all the way down.** From the globe to a 3 m hex with named people in it, every beat is
  seeded and recorded, so any match can be resumed exactly, replayed byte for byte, or forked from any past beat.
- **The Director's Cut.** The player directs history in words; Time controls the clock and the cut-off; a duel cuts
  from the strategy board to an arena; and any match's history can be scrubbed, replayed or forked into a new cut.

---

## 1. The premise

| Element | What it is | Source |
|---|---|---|
| The table | A god's room: a floating globe over a board; the selected part of the planet is projected onto the table as a "circle of action" | PROJECT_GUIDE §2.6 |
| The gods at the table | Each player is a god, seated as a stone statue on a pedestal with a name plate in their aura colour | ACCOUNTS §14; Unreal b0859bb, d1469c2 |
| Two demigods each | The **Left Hand** and the **Right Hand**: two bodies, each with its own health, position, items, story and scars. A player is out when both fall. Bolts and sparks are one purse per player; debts are per body | RB 15 |
| The pantheon | Seven god minds (Time, Nature, Consequence, Death, Chance, Memory, Entropy) that rule on anything divine | RB 12.1 |
| The world | The real Earth: regions, cities, crowds, armies and named people, all simulated every beat | PROJECT_GUIDE §2.4-2.5 |
| The Dungeon Master | The server: its rules (code) plus its LLM minds (the game master, the gods, the narrator) | RB intro |

Two deliberate styles (Benito): the god's room is realistic; the board and the Earth are the gods' toys (BUILDLOG
2026-11-15).

## 2. The core loop and the beat: the world never waits

**One beat = one Theatre round** (`flow_ms`, default 3 s; the shared lobby uses 12 s). The Final Night falls on beat
`beats_total` (60). Every LLM call has a rules fallback, and a late call takes it. "The beat never waits." (RB 1, 5)

**A beat, in order** (RB 17): Start (Time's clock, due debts queued) → Brief → Compose → Resolve (Time's bills → casts →
hands → mentions → route → one batch of minds → merge and world rules → the society → occupancy → the DM's step →
consequence and memory) → Reveal → Commit.

**Days and nights.** A game day is 8 beats: dawn, day, day, day, dusk, night, night, night. Night brings undead, ghosts
and thieves, and makes monsters and traps harder (RB 5).

**Time's circle.** The safe zone shrinks around a convergence city from the whole globe to 1,500 km by beat 160;
bodies outside take 0.5-3 damage a beat and are dragged in (RB 14).

**The Final Wish.** Beat 1: every player's orders are their final wish: more acts, +2 magnitude on each, +2 on its
d20s (RB 17).

**The next move.** Every beat the DM gives each human seat one private hint (a line, why it matters, a gesture, a
target) and a goal line ("Last standing wins. You: 2 alive, 3 rivals. Beat 41 of 60."). Rules pick it, never an LLM,
in priority order: in a duel (type now), about to fall, the fallen, spectators, a rival in your hex, a world event near
you, a mega at a junction, no gear, a quest step, a will (RB 29). From state schema 22 the hints use plain words
("right here", not "0 km away"), low-urgency hints rotate every 3 beats, and a cast that would hit your own demigod is
warned about (BUILDLOG "Clear words").

## 3. The will: typed orders that slow time

The will is the text box. **Text entry is the God of Time's interaction**: your words change the course of history,
separate from the lightning-bolt powers (PROJECT_GUIDE §2.3).

| Rule | How it works | Source |
|---|---|---|
| Opening a slow window | Starting to type (`action.begin`) opens one slow-time window per room, up to `slow_ms` (30 s), if your demigod stands and you hold a bolt. The world keeps resolving | RB 12.2 |
| Who sees what | Everyone sees who opened the window; nobody learns who else is typing | RB 15; PROJECT_GUIDE §2.3 |
| The cut-off | The opener's Enter closes the window; later submits are cut privately | RB 12.2 |
| Time's toll | At the next resolve: a bolt as toll (no gift to rivals), a debt of `0.5 × (1 + seconds/10) × (1 + 0.5 × slows in the last 20 beats)` (doubled if the bolt can't be paid, cap 10), and Entropy `+0.5 + seconds/30` | RB 12.2 |
| Instant orders | An order without a window pays the same toll and Entropy and a flat 0.5 debt | RB 12.2 |
| Your hands | Your Left and Right Hands are LLM orchestrators: they break the will into acts, spawn subagents, possess people, raise armies and invoke gods | PROJECT_GUIDE §2.1 |
| No intent vanishes | Every order gets a visible ruling or a refusal | RB 21.6 |

**Wills that matter** (rules 0.9.0, RB 23). A will is split into its **parts** (heal, bless, arm, raise, curse/seal,
ask, offer, gather). Each part × each target is ruled on its own, with its own d20, price and plain-words result, and
when it falls short, **why** and **what would work** (a mystic quest's steps, or the price). Example from the design:
"every one of my demigods is fully healed and gets rocket launchers" is four rulings (heal ×2, arm ×2), not one line of
narration.

**Paying for wills** (RB 23.2). Prices are in sparks. The purse pays sparks first; then a bolt breaks into 5 sparks
(the change stays in the purse). What the purse still can't pay, the ruling god lends (0.2 debt per spark) while the
demigod's open debt stays under 3; past that, the will becomes a mystic quest. Entropy rises 0.25 per spark paid.

### 3.1 The four will hot buttons

Four carved icons on the will: an ankh, a notched bow, a delicious meal, and a scene of armies. A press puts a
ready-made line in the box, counts down **5 seconds** on the will and sends it as Enter would. Typing in the box stops
the auto-send ("type to make it yours"); another press replaces the line and restarts the count (Unreal main a89c986,
`SFNHudLive.cpp`, `SFNPowerTile.cpp`).

| Button | What it does | The line it fills (live names and places fill in) | Rule it triggers |
|---|---|---|---|
| **Ankh** | Death's resurrection line | "Death, bring back my demigods" (all fallen) or "Death, bring back my Left Hand, I offer 2 bolts" | Resurrection (RB 25) |
| **Bow** | Arm and hunt | "arm yourself with an obsidian blade in Vienna and hunt Vex's Right Hand" | Arming (RB 24) plus a hostile act |
| **Meal** | Nature heals | "Nature, heal my Right Hand" | Healing your own (RB 23.3) |
| **Armies** | Raise an army | "raise an army of 10,000 fishermen at the harbour of Vienna" | Armies (RB 14) |

## 4. Mega and mini powers

Powers are a separate path from the will: pick a power and throw it anywhere on the globe. Typed text never casts, and
casts never carry orders (PROJECT_GUIDE §2.3). A cast is an instant action, shown at once as a streak and paid at the
next resolve (RB 12.3).

### 4.1 Bolts and sparks: the 1:5 economy

Each player starts with **1 lightning bolt** (mega) and **5 sparks** (mini). One bolt breaks into 5 sparks when a will
needs paying (RB 12.3, 23.2).

| | Mega (a bolt) | Mini (a spark) |
|---|---|---|
| Scale | map: radius × 4 within 600-2,500 km | board: radius / 20 within 3-30 km |
| Strength | magnitude + 2, damage × 1.5; 40% of it to every rival demigod in the radius | magnitude − 3, damage × 0.6, its target only |
| Gift | every other player gains 1 bolt | none |
| Entropy | +10 | +1 |
| Fault lines | scaled by the nearest fault line and junction | unaffected |

The 11 powers include lightning, storm clouds, volcano, earthquake, flood, wildfire, meteor, raise land, sink land,
grow forest and swarm (EARTH_SIM §2). Each cast also gets **Entropy's check, which always says yes**, and an LLM writes
**ramifications**: `1 + cost + magnitude/3` geotagged rows per cast (at most 10) and lasting world mods (RB 12.3).

### 4.2 Fault lines and junctions

A mega's power depends on where it lands relative to the real plate boundaries (PB2002 data, RB 13). Chris: megas
should be "crazy powerful at huge intersections and only 'tiny volcanos' near the no-faultline areas."

| Tier | Where | Effect (live house curve) |
|---|---|---|
| `junction` | within 300 km of one of ~97 plate junctions (89 triple junctions, 8 knots) | up to magnitude +3, radius × 1.5, damage × 1.8 more, on top of the fault curve |
| `fault` | within 200 km of a boundary | magnitude +5, radius × 2.2, damage × 2.5 |
| `near` / `mild` | the first / second half of the curve | linear between |
| `tiny` | 1,000 km or more from any boundary | magnitude −4, radius × 0.4, damage × 0.25; its lasting mods stay within 150 km |

Worked example (RB 13.1): a mega volcano aimed at the Afar triple junction lands at damage 11.8 and radius 1,789 km;
the same cast in central Australia lands at damage 0.8 and radius 240 km. The client shows the fault lines only while a
mega is armed, and `game.fault` previews the tier under the cursor.

### 4.3 God-tier powers

Each god has one signature power on the ring (RB 20). It costs 3 bolts, gifts every other standing player 1 bolt, adds
mega-level Entropy, and is that god's ruling: the god may grant, bargain, twist or refuse. (Table in section 6.)

### 4.4 Entropy and heat death

Entropy runs 0-100 (RB 12.4). Thresholds: **20** freak weather, **40** unreliable physics (a third of casts land
50-400 km off target), **60** riots and worship, **80** wounds to the gods, **100 heat death: that beat becomes the
Final Night at once.** On the simulated Earth, Entropy quickens every spread, stresses every fault until the world
starts its own quakes and eruptions, and at heat death makes everything flare (RB 28). Time can cool Entropy for its
price (RB 12.1).

## 5. The AI Dungeon Master and "history is a lens"

> "history and its consequences are like lenses. all of the rules just imagine this crazy algo that DOES all of that,
> and then its answer, no matter how ludicrous, and try to get it making mystical sense to fit with the world, but a
> lot rube-goldberg mystical. like needing to wash a stone in platinum dust" (Chris, RB 0)

Three ideas govern everything (RB 0):

1. **History is a lens.** A place's recorded history bends what is possible there: a valley that buried an army last
   night breeds ghosts.
2. **The rules and dice are an engine that has already decided.** The DM never overrules it: a natural 1 on a
   god-blessed strike still misses.
3. **The DM makes the answer make mystical sense**, Rube Goldberg style: "the bridge held only because the ferryman
   had washed the keystone in platinum dust at dawn."

**The DM's four laws** (embedded in the game master's and gods' prompts): *the dice decide, you describe; the DM
remembers; places nest; describe the world as the rules left it* (RB 1).

### 5.1 The d20 table (RB 2.1)

| Situation | DC | Modifier |
|---|---|---|
| A demigod's ordinary act | 8 + magnitude/2 | the demigod's; +2 on the Final Wish |
| Manifesting an item | from the tier's odds | the odds' other terms |
| Stepping on an encounter | by type (10-13) | the demigod's; −1 monsters and traps at night; +1 near its own shrine |
| A fight's loser keeps its gear | 11 | the demigod's |
| A summoning's mishap | `round(20 × p) + 1` | none |
| A duel blow | 10, +1 at night, + half the opponent's wardrobe stage | the line's wit and the duellist's |
| A will on your own (heal, bless, arm) | 6 + magnitude/4 (arming: 6 + 2 per tier) | named in the answer |
| A curse | 10 + half the target's wardrobe | the caster's |

**Reading the die:** a natural 1 is always `crit_fail`; a natural 20 is always `crit_success`; otherwise total ≥ DC
succeeds. **The demigod's modifier:** +1 hale (health 7+), −1 badly hurt (3 or less), plus half its wardrobe stage
(RB 2.2-2.3). **One question, one roll, forever:** every roll is seeded by the match seed and its query and recorded;
asked again, the DM returns the same face (RB 2.5). Every roll is public: the table sees the die tumble (RB 1), and the
client's table d20 says whose roll it is and what for (Unreal 60abcfe).

## 6. The seven gods

When a demigod reaches into a god's domain, that god's mind grants, refuses, bargains or twists, and names a price.
An invocation costs `1 + magnitude/5` bolts. Precedence settles clashes on the same target (higher wins; Chance's
seeded coin breaks ties) (RB 12.1, `data/pantheon.json`).

| God | Domain | Precedence | God-tier power (RB 20) |
|---|---|---|---|
| **Time** | slowing, hastening, rewinding and travelling through time; the arena clock; owns the will's slow window | 50 | **Time Stands Still**: a unit target is rewound 2 beats; at a point, rivals within 1,500 km are slowed a beat |
| **Death** | raising the dead, killing at a distance, souls | 45 | **The Pale Verdict**: a fallen target is raised; a standing one is struck for 3 |
| **Consequence** | debts: every act's bill, collected later as Final Destination-style accident chains | 40 | **The Reckoning**: the target's biggest open debt is collected now |
| **Entropy** | chaos, overload, infinite magic power; the Entropy meter and heat death. Charges nothing, always says yes, gives 2 bolts and raises the meter | 35 | **The Chaos Burst**: Entropy +15 |
| **Nature** | weather, beasts, plants, terrain, physics; rules heals | 30 | **The Wild Surge**: 1.2 damage to rivals within 400 km |
| **Memory** | forgetting, false memories, learning what a crowd knows | 20 | **The Great Forgetting**: every crowd in the target's region forgets its allegiances |
| **Chance** | luck, tricks, coin flips, the odds | 10 | **The Coin-Flip Storm**: every demigod within 600 km heals 1.5 or takes 1.5 on a seeded coin |

**Consequence's bill** (RB 14): each landed act adds debt, due in 50 beats; due debts are collected as accidents. You
can win through a rival's own unpaid debts (RB 17).

## 7. Items and the item engine

**Making things** (RB 16). An order to manifest something is judged by rules; an LLM only names it.

| Tier | trinket | artifact | relic | wonder | legend | myth | titan | apocalypse |
|---|---|---|---|---|---|---|---|---|
| Size (count) | ≤ 12 | ≤ 1,000 | ≤ 100,000 | ≤ 1M | ≤ 10M | ≤ 100M | ≤ 1B | beyond |
| Cost (bolts) | 0 | 0 | 1 | 1 | 2 | 3 | 4 | 6 |
| Base odds | 0.97 | 0.90 | 0.80 | 0.66 | 0.52 | 0.38 | 0.25 | 0.14 |

A shortfall becomes debt to Consequence. Successes can twist; failures can be partial or backfire on the maker. A host
of 100+ takes the field as an army; a made structure is also a shrine.

**Arming** (RB 24). "Arm my demigods", "they get rocket launchers": gear for each of your standing demigods, a d20
against DC 6 / 8 / 10 by tier (trinket free, artifact 2 sparks, relic 5). A god named in the will, or blessing words,
costs 5 more sparks per demigod and makes the gear a tier up with that god's signature.

| Slot | What it adds (artifact → wonder+) | Once a match (relic+) |
|---|---|---|
| Weapon | +1 to +2 on strikes, +0.5 to +1 damage, +1 to +2 in duels | **Smite**: first landed strike on a rival deals 2 more |
| Armour | turns aside 0.5 to 1.0 per hit; resists its material's type | **Last Stand**: the first felling blow leaves it at 1 health |
| Charm | +1 to +2 on other deeds, 0.1 to 0.2 mending a beat | **Second Wind**: heals 2 the first time a beat ends at 3 health or less |

Materials matter (obsidian and dragonscale resist fire, platinum makes its bearer harder to curse, butterfly-wing and
heron-feather help journeys). A kit is capped (+3 on a d20, +1.5 damage, +3 in duels).

**Finding and losing things.** Encounters drop items (RB 8). A death drops everything on its hex; a fight's loser may
drop its newest item; the next demigod to pass takes the pile. Items are never lost: holders only grow (RB 9, 18).

**Seeing them.** Gear shows on the demigods: gear portraits and dioramas update when a major item is acquired
(MEDIA "Looks", state schema 11). In the client, each demigod's card has an **ITEMS** button that opens its items
(Unreal f68312d).

## 8. Duels: challenge mode, will against will

> "everyone on the map needs to stop what they're doing and watch: the Archon-level fight, will against will"
> (Chris, RB 21)

Inspired by Archon (1983): the board is strategy, but when two pieces meet the game cuts to an arena, and here the
real-time play is **typing** (RB 21).

| Step | How it works | Source |
|---|---|---|
| Challenge | Double-click a rival in reach: a challenge card shows "CHALLENGE? / A vs B / the win %", one distance, and DUEL or TELEPORT + DUEL. Enter = DUEL | RB 21.1; Unreal f712d28 |
| Reach | The board cards touch at your zoom, or within 120 km at any zoom. No consent: a challenge is forced | RB 21.1 |
| Teleport | A summon first costs a mishap roll (up to 2 health), decided before the jump and shown as a red pulse | RB 19 |
| The lock | Everyone sees both portraits slam in, both health bars (the one place a rival's health is shown), and every client swoops to the fight. The narrator calls "CLASH! Will against will." | RB 21.1; vo README |
| Blows | Each typed line (up to 120 characters) is a blow: a d20 vs DC 10 (+1 at night, + half the opponent's wardrobe), resolved at once by rules | RB 21.2 |
| Wit (−3..+4) | +1 each: an action verb, a second verb, a body part, a held item, a terrain or place word, the opponent's story or scars. Repeats, very short lines and gibberish cost | RB 21.2 |
| Damage | 0 on a fail; 1 on a success, +1 beating the DC by 5, +1 with wit 3+; 4 on a natural 20 | RB 21.2 |
| Spam | Two blows inside 0.5 s: the second is a penalty and costs its author 0.5 | RB 21.2 |
| Moves | Words pick a move class (strike, heavy, guard, parry, feint, evade, close, retreat, grapple, throw, taunt, invoke a god, command an army), resolved through a range/initiative/stance matrix | RB 21.4 |
| Only what is real | A line can use only items you hold or things on the board (a tree, a rock, the mud). Name a pistol you don't have: a whiff, "you have no pistol", wit −2 | DUEL_MOVES §5 (schema 21) |
| Staged steps | "hide behind that tree then pop up and slit his throat" plays as up to 4 staged steps on one d20 | DUEL_MOVES §5 |
| Watching | Others see the line grow as stars with only the newest letters showing, like a password field | RB 21.7 |
| The end | A knockout, the 30 s deadline, or a duellist falling elsewhere. The loser's loot falls on the hex; both get the duel's deed as a tale; the hex remembers the fight | RB 21.3 |

**Odds.** `game.duel_odds` plays the blow rule out in 600 seeded trials and returns your win % and your win % if you
type nothing (RB 29). The challenge card shows it.

**The AI types too** (RB 21.8). The built-in AI writes and types its own lines, visible as masked typing, with a
handicap of 5 against a human. Measured over 400 simulated duels: a human typing good lines wins about 9 in 10 at 84
words a minute, 2 in 3 at 60, and 1 in 3 at 42; one who types nothing loses every time.

**Skins.** Each fighter steps in as wood, obsidian, gold, dragonscale or butterfly, picked by its name unless it
carries a god's gift (dragonscale) or gold (RB 21.7).

**Honest note.** By design a duel freezes the world. Until The Theatre offers a room-wide hold, the world keeps beating
during a duel and the two duellists are locked in place (RB 21.1).

## 9. Death, resurrection, seals and quests

| Topic | Rule | Source |
|---|---|---|
| Health | 10 per body: 3 a bad punch, 5 a stab, 8 a car hit, 10 instant elimination | RB 14 |
| Who can plead | Any player, for any fallen demigod. **A wiped player may plead too**: the fallen can plead, ask about quests and make offerings; nothing else | RB 25 |
| Death's price | 5 sparks × (1 + times raised before) + 1 spark per 8 beats dead; plus years off max health; all or nothing | RB 25 |
| Can't pay | A public quest; a demigod raised by a quest owes Death the sparks as debt | RB 25 |
| Returning | At 40% of its (lowered) max health, where it fell or at its shrine. At most 3 raisings per player per match | RB 25 |
| Seals and curses | **Seal** (no resurrection), **wither** (no healing), **silence** (wills cost double), **bind** (no summoning), **hex** (minus its strength on d20s). 5 sparks; god-tier 15 sparks, strength 3, until countered. A natural 1 turns the curse on its caster | RB 26 |
| Always a counter | Every curse comes with a mystic counter-quest for its bearer. Everyone sees the mark, never who cast it | RB 26 |
| Mystic quests | 1-3 steps the engine verifies: stand at a place (at a phase), hold an odd ingredient, win an encounter, raise a shrine, survive a duel, offer sparks, bring an army, keep a vigil | RB 27 |

> "mystic quest wings of a butterfly to counter a curse" (Chris, RB 27)

The odd ingredients include the wings of a butterfly that has crossed a named strait, a stone washed in platinum dust,
salt from a dried sea, a bell without a tongue and a thread from a general's banner. Example (RB 26): a curse of
withering lifts after "4 sparks fed to the dark between two heartbeats, then your shadow laid down by night where the
ground remembers being two grounds" (offer 4 sparks; stand near a fault line at night).

**Not built:** a lone dead player rewinding the match (time travel instead of an invented "mercy" rule) needs an engine
call (`room.rewind`) that was requested from The Theatre and is not finished (BUILDLOG 2026-11-15).

## 10. The living world

### 10.1 The Earth simulator (RB 28, EARTH_SIM)

Every cast, god-tier power and physical ramification starts a **phenomenon** on a deterministic voxel field of the
whole planet: one column per 120 km hex, 16 height levels, materials, water, lava, ash, fire, snow, cloud, cracks and
ruin.

- **Phenomena:** storm, blizzard, lightning, flood, wildfire, volcano, earthquake, meteor, tidal wave, raise and sink
  land, forest, swarm and drifting plates.
- **They interact:** a flood quenches fire into steam; lava in water becomes rock; a quake near a volcano wakes it; a
  quake ruptures along the nearest real fault; a sea vent that keeps erupting builds an island.
- **Consequences go through the rules:** a demigod in a burning or flooded cell takes a bounded hit (armour of the
  right material halves it); crowds fear the gods; settlements lose levels; roads are cut; shrines topple.
- **Thing by thing** (schema 20): forests burn cell by cell and stay burnt until they regrow; people run to the nearest
  shelter; strong hazards break shelters and the wreck stays.
- **The earth underfoot** (schema 17): every prompt that acts at a place carries one factual line from the field
  ("waist-deep floodwater, the flood came 2 beats ago"), and the DM never contradicts it.

Benito's rule for the client (EVENTS_IN_THE_WORLD): what you do changes the world, not the log; you see the damage at
every zoom and it persists.

### 10.2 Settlements, roads, shrines, encounters

| Feature | How it works | Source |
|---|---|---|
| Settlements | Level 1 hamlet to 5 great city (6-7 legendary). Move one step at a time toward a target set by people, prosperity, builders and shrines | RB 6 |
| Roads | Traffic wears paths between settlements; a road appears at wear 4 and grows over when unused | RB 10 |
| Shrines | Raised by a will ("build a shrine"); up to 3 per demigod; grant sparks, healing or a ward; gather worshippers; level up per tenfold | RB 11 |
| Encounters | Hidden marked hexes: monster, treasure, trap, stranger; at night undead, ghost, thief. Step on one: a public d20, and it stays revealed forever | RB 8 |
| The piece's story | Followers, items, wardrobe stage (plain to mythic, from deeds), condition, scars | RB 7 |
| Occupancy | Benito's rule: no two standing demigods share a 3 m hex unless engaged | RB 15 |

### 10.3 People and houses

Benito asked for people who are "moving around doing their thing living their lives" and houses that are not "boring
and blocky" (BUILDLOG "people and houses"). Built: `game.people` lists everyone near a board (named people plus 2-4
residents per house), each with what they are doing and why: an event reaction (fleeing a flood to a shelter, rioting,
cheering an army) or a routine by role and time of day (asleep at night, fishing, trading, praying, patrolling).
`game.buildings` lists the houses with a kind (dock, temple, market, manor, tower, dwelling) and a look. A library of
sprite and building pictures (48 sprites and 24 buildings prewarmed on the live server) dresses them.

### 10.4 Token scenes

> Benito wants typed in-game actions to play out, for example entering the house on the left and an encounter with the maiden, and wants to see
> that play out with the little game tokens" (spacing added; RB 29 "Token play")

After the DM rules, the will is played out by tokens on the people-level board: the demigod walks to the house on its
left, the roof lifts like a dollhouse, the person the ruling names plays herself, the d20 tumbles, and the outcome
plays (an embrace on a success; a slap and she runs out on a natural 1). **The scene decides nothing**: it is drawn
from the recorded ruling and die. One scene per demigod per beat; rules only, so it costs nothing (RB 29;
EVENTS_IN_THE_WORLD).

## 11. Memory and the tree of consequence

**The DM remembers everything** (RB 3-4). Every decision is a memory entry with a beat, a place and a kind. Places
nest: a 3 m hex sits in a settlement, in a region, in the world.

- **Integrate:** each beat rolls up the hierarchy, so a coarse zoom reads the truth of everything beneath it.
- **Compare:** an answered question keeps its answer; a contradicting new answer is logged as a conflict and dropped.
- **Extrapolate:** a hex nobody looked at is derived from its last observation and its parent's changes; once observed,
  its detail is remembered. The same hex at the same beat is the same for everyone.
- **Feed the minds:** the game master and gods read the newest 6 settled public facts about a region, labelled "never
  contradict them".

**The tree of consequence** (`game.memory_tree`, BUILDLOG 2026-11-15). The whole memory hierarchy at once: every node's
fact counts and density from the planet down to a 3 m hex, its roll-ups with the exact entries they cite, and **recall
paths** that show how one fact (a d20, an encounter, an earth event, a person's memory) travels up into what the DM's
prompt says now. Private facts are counted, never shown. In the client, **TREE** in the header opens it as a star-map
overlay (Unreal 6f8fa88).

## 12. Time travel: exact resume, fork, replay

Everything is seeded by the match seed and stored in the state (RB 18), so a match can be redrawn at any moment
(TIME_TRAVEL).

| Operation | What you get |
|---|---|
| **Exact resume** | A client that reconnects, a viewer who joins mid-duel, or a server that restarts gets the exact present in one message: the beat, the open slow window, every demigod, every open duel with every blow |
| **Replay** | A recording re-computes every snapshot and compares them byte for byte: 0 differences, LLM answers included |
| **State at a round** | Any past beat through the game's public, raw or seat views |
| **Fork** | A new private room from any past beat; open duels keep the time they had left |
| **What a demigod looked like** | The exact picture a demigod showed at any beat |
| **Earth time** | The Earth field keeps a keyframe every 16 beats, so any past beat is rebuilt by re-simulating at most 16 |

No accepted instant action is lost to a server kill: it is journalled to disk before the next one is taken.

## 13. Accounts and the god form

Players start as guests; each player can write their two demigods' personalities, kept word for word unless they break
the content rules (one moderation pass) (ACCOUNTS §3, §7).

**The god form** (ACCOUNTS §14). Each player's god sits at their seat as a stone statue, and the player chooses how it
looks. Chris described them as stone or marble statues, sitting in poses such as the thinker.
michaelangelo's david."

1. An entry box: "How does your stone godform look?" (up to 600 characters, the player's own words).
2. A stone: white marble (default), black onyx, granite, sandstone, jade, bronze, gold leaf, or the player's own words.
3. A pose: The Thinker, David, Moses seated, the Discus Thrower, Atlas, Winged Victory, the Boxer at Rest, or custom.
4. Preview (one reference image, up to 6 new pictures a day), then accept.
5. On accept, a **Meshy** image-to-3D statue (1 per account). The client puts the accepted picture on the pedestal at
   once, then builds the statue at runtime from the model in the chosen stone (Unreal d1469c2).

Until a player makes one, their seat shows a house marble statue with their Left Hand's likeness. The god form went
live with Meshy statues on, 1 per account (BUILDLOG 2026-11-15).

## 14. Controls (the hand of god)

Locked by Chris on 2026-11-15 (HAND_OF_GOD §6): the land slides under the hand for the whole drag, and the point under
the pointer stays under the pointer. A click on the globe dives to that region. Double-click zooms in, right-click zooms
out, never inside a demigod's click zone.

| Gesture | Result | Source |
|---|---|---|
| Double-click your own demigod (card or piece) | The view glides to centre it at the same zoom, with a gold ring pulse | Unreal f68312d |
| Double-click a rival in reach | The challenge card with the odds | Unreal f68312d, f712d28 |
| Double-click a far rival | Centres it | Unreal f68312d |
| ITEMS on a card | Opens that demigod's items | Unreal f68312d |
| Drag your demigod onto the board or globe | Summons it there this beat (risk: up to 2 health) | RB 19 |
| Chat | Folds to a speech-bubble icon with an unread badge; a click opens it | Unreal 3684b1b |

Towns, people, house cards and armies ride with the board every frame during a pan, zoom or glide (Unreal 2c9b36b).

## 15. Audio and voiceovers

**Voiceovers** (Unreal `production_assets/audio/vo`, imported in 959734c). Designed voices (no real person's voice),
one at a time, any key skips.

| Plays when | Line |
|---|---|
| The signed-in lobby opens | "Welcome, god. Your two hands wait on the table below. Speak, and the world will listen." |
| A match starts | "Seven nights. Seven wars. Gods at one table, and the whole world for a board. This is... FINAL NIGHT SEVEN." |
| Your duel locks | "CLASH! Will against will." |
| Your duel ends, won / lost | "Your words cut deeper. The duel is yours." / "Outspoken. Outfought. Rise, and speak faster next time." |
| You win / you fall | "The night is yours. Every other god has fallen silent. You are the last one standing." / "Your hands have fallen. The table goes dark. But the world remembers what you did." |

**Sound** (Unreal f49d26c): procedural sounds for casts (mini soft, mega deep), quakes, the duel clash, landed blows,
misses and whiffs, victory and falls, and an urgent-hint chime; the ambient bed swells with Entropy; master, music and
sfx volumes per account. The crackle in the ambient bed was removed (48c5c1b).

## 16. Winning and the Final Night

**Win:** be the last player standing. Kill directly, through paladins, riots, armies, encounters and wards, or through
a rival's own unpaid debts (RB 17).

**The Final Night** comes on beat 60, at heat death (Entropy 100), or on any beat that would end with nobody standing.
Every outstanding debt is collected, and exactly one player survives: the most total standing (health minus debt), then
start-of-beat health, then the lowest id (RB 17).

## 17. Credits and sources

- **Designers:** Chris and Benito. Every rule traces to their words in `BUILDLOG.md`.
- **Rules:** `docs/DM_RULEBOOK.md`. **Client contract:** `docs/CLIENT_API.md`. **Data and time:**
  `docs/TIME_TRAVEL.md`, `docs/EARTH_SIM.md`. **Accounts:** `docs/ACCOUNTS.md`. **Images:** `docs/MEDIA.md`. **Duel
  moves:** `docs/DUEL_MOVES.md`. **Overview:** `docs/PROJECT_GUIDE.md`.
- **Plate boundaries:** Bird, P. (2002), PB2002; GeoJSON by Hugo Ahlenius, Nordpil (ODC-BY 1.0).
- **Prior work** disclosed per the jam rules: the 2024 Final Night prototype and Final Night 2.5 (PROJECT_GUIDE §1).

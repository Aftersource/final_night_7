# FINAL NIGHT 7

> The sequel nobody asked for, numbered 7 because 7. A battle royale of demigods, played through orchestrator agents, in a
> world that remembers everyone you've ever spoken to.

A new Theatre game plugin (`final_night_7`), built during the jam. It reuses concepts and code from Final Night 2.5.0, listed
in §8.

**The game in one paragraph.** Four players, each a demigod. You don't control a character directly; you command
**orchestrators**, agents that plan, spawn **subagents** and act in the world for you. You can zoom from the whole map down to
one person in one street and micromanage them. Reality is run by a **pantheon**. When you reach into a god's domain (slow time,
raise the dead, call a storm), that god's orchestrator intervenes: it grants, refuses, or names its price, and sends its
**paladins** after you. Society is a giant **table of consequence**: every act, who saw it, who heard about it, and who owes
what. Most people exist only as **clusters** ("the night-shift dockworkers, ~4,000, furious") until someone addresses one of
them. From then on that person is **memorized**: a named individual with their own memory, permanently. Last demigod standing
wins.

---

## 1. Orchestrators all the way down

```
Player (speech/text, as fast as you can)
 ├─ LEFT HAND  orchestrator ─┬─ subagent: "turn the dockworkers against Vex"   (lives 5 beats, reports back)
 │                           └─ subagent: possess / micromanage "Ada the baker" (lives until released)
 └─ RIGHT HAND orchestrator ─┬─ subagent: "scout the cathedral"
                             └─ invocation → GOD OF TIME orchestrator ─┬─ ruling: grant / refuse / price
                                                                       └─ paladin subagent: "Warden Sel, enforce the price"
```

- **Hands are orchestrators.** Each player has two, **Left** and **Right**, with persistent memory of everything they've done
  for you. You talk to one by holding that side's trigger or typing `l:` / `r:`. A hand breaks your commands into tasks and
  either does them itself or **spawns subagents**.
- **Subagents** have a goal, a budget in beats, the entities they're allowed to touch, and a report they owe their hand. They
  run in parallel. When they finish or run out, their result (and what they learned) goes into memory.
- **Zoom.** The same command means different things at different zoom levels:

  | Level | What you see | What a command does |
  |---|---|---|
  | **World** | The map, demigods, the big clusters, gods' attention | Moves forces, starts campaigns, invokes gods |
  | **District** | Clusters and memorized characters in an area, the relationships between them | Targets groups, recruits, spreads rumors |
  | **Character** | One person: their memories, beliefs, loyalties | You micromanage them through a possession subagent: speak as them, steer them |

  Zooming in on a cluster member memorizes them (§3).

## 2. The pantheon (gods are orchestrators too)

Gods aren't a council that votes every beat. They're **domain orchestrators**, woken when someone acts in their domain. Every
intent is routed by domain:
- An ordinary act (walk, talk, punch, build) goes to the **world resolver**, which is Final Night's game master job.
- A divine act goes to the **owning god**.

| God | Domain | Typical price | Paladins do |
|---|---|---|---|
| **Time** | Slow, speed up, rewind, travel; the shrinking arena clock | Years of your life (lost max health); an appointment it will keep | Hunt down anyone who hides from the clock or skips it |
| **Nature** | Weather, beasts, growth, terrain | Something you summoned becomes wild and turns on you | Tend what you broke, and punish its breaker |
| **Consequence** | Debts: every act's bill, collected Final Destination style | Can't be bargained with, only delayed (by Time, at interest) | Collect |
| **Death** | Raising the dead, killing at a distance, the underworld | A soul for a soul: someone memorized, chosen by Death | Escort the dead, and fetch the overdue |
| **Chance** | Luck, tricks, coin flips the rules engine rolls | A random clause, revealed later | Meddle and steal outcomes |
| **Memory** | Forgetting, false memories, learning what a cluster knows | Something *you* forget: an item, an ally, a hand's memory | Erase witnesses, plant rumors |

- **Favor.** Each god keeps a running favor score for each demigod. Favor shifts the god's rulings, and worship by clusters in
  the world raises it.
- **Gods have agendas and rivalries,** and they interfere with each other:
  - Time vs Consequence: Time delays debts and Consequence charges interest.
  - Death vs Nature.
  - Memory vs everyone, because a forgotten act still leaves its ledger row.
- **Paladins** are memorized characters who serve a god. They're god-spawned subagents with bodies in the world. A demigod can
  fight one, bribe one, or convert one.
- **The pantheon is data, not code.** Each god is a row with a domain, a prompt persona, a price table, rivalries and a paladin
  template. Adding a god (War, Love, the Audience) is a config change.

## 3. The world: clusters, memorized characters, and the table of consequence

The core idea is **lazy individuality**. The world holds a few hundred clusters, and individuals exist only after someone looks
at them.

**Clusters.** Each has a label, an approximate size, a place, traits, mood, needs, and the groups it belongs to. ("Harbor
night-shift dockworkers · ~4,000 · Lagos port · tired, proud, owed wages · members of: Laborers, Harbor, Faithful of Nature".)

**Groups are Venn sets.** A cluster or a character can belong to many groups, and the overlaps matter:
- News travels along overlaps: an act witnessed by *Harbor* reaches *Laborers* next beat, then *Faithful of Nature*.
- Loyalty conflicts happen where groups intersect.
- "Who's in Harbor ∩ Faithful of Nature ∖ owes Vex?" is a real query.

**Memorization (addressing someone):**
1. A hand or subagent refers to someone ("the baker who saw the fire").
2. The resolver checks whether that person already exists, by fuzzy name and description match (toolkit consensus normalize
   and Jaro-Winkler).
3. If they don't, it **materializes** them from the best-matching cluster:
   - a name and traits drawn from the cluster's stats, with a seed fixed per cluster and per person, so replay is exact
   - memberships inherited from the cluster
   - starting memories: the ledger entries and rumors that cluster could know
4. From then on the character is permanent. They're in every relevant prompt, they can be zoomed into, and the cluster's count
   drops by one.

**The table of consequence (the ledger).** Every resolved act appends a row:

`beat · actor · act · targets (characters / clusters / groups) · magnitude · valence · witnessed_by · known_by · debt (to which god, how much, due when) · status`

- `known_by` grows each beat as news spreads through group overlaps, with decay, and Memory can erase it.
- Cluster mood and loyalty are **numbers** updated from the ledger by rules, with no LLM involved. The LLM only writes when
  something is addressed or crosses a threshold (a riot, a conversion, a rumor reaching a demigod).
- Consequence collects from this table, and the post-match "venn of the night" is drawn from it.

**Holistic memory.** Five stores, all derived from the ledger plus what each owner saw:

| Store | Owner | Contains |
|---|---|---|
| Episodic | Each memorized character, hand, god | What I saw, did, was told (with salience and the people it was about) |
| Collective | Each cluster and group | Rumors and beliefs, aggregated, with how sure they are |
| Relations | Pairs | Trust, fear, debt, loyalty |
| Favor | God × demigod | The running score |
| World | The resolver | Places, items, piles, arena |

Retrieval for a prompt is SQL-style selection: rows about the entities in play, then by salience, then by recency, with old
memories folded into summaries. There are no embeddings; the vendored crates don't include a vector index, and this doesn't
need one.

## 4. The beat loop

You can still type, yell or speak as fast as you like; more words give your hands more to do. Input is batched into **beats**
(about 6 s by default):

1. **Intake.** Each hand turns its buffered speech into intents and subagent spawns. That's 8 small calls in parallel.
2. **Route.** Each intent is labeled ordinary or divine, plus its domain (code with a keyword pass, and the LLM for ambiguous
   cases).
3. **Act.** In parallel: live subagents take their step; woken gods rule on invocations; the world resolver resolves ordinary
   acts per vicinity, Final Night's primer pattern.
4. **Merge.** Code combines everything: damage median when several sources hit one target, toolkit consensus for entities and
   items, god-precedence rules (a Time delay beats a Consequence collection, and the debt grows by 50%).
5. **Consequence.** Ledger rows are appended; news spreads one hop along group overlaps; cluster numbers update; due debts are
   collected.
6. **Broadcast.** Each player gets their view, at the zoom level they're on.

**The Narrator is off the critical path.** Beat N's Narrator (Deadpool voice) runs during beat N+1, concurrently with that
beat's hands and act batch, on beat N's applied rows (each with the effect that landed and on whom), its public rulings,
who fell and who stands. Its caption is stored on beat N's record and delivered in beat N+1's outcome as
`previous_caption` with `caption_beat` = N; the last beat's caption is written in the match summary (`last_caption`),
where the Narrator is also told the champion. So captions always arrive one beat late and never hold up a beat. The
engine's result is final: a caption that has a demigod who was already out acting, or a survivor (the champion
included) fallen, is replaced by the template caption (a callout like that is dropped), and the final cut falls back to
the joined captions if it has the champion fall.

**Cost control.** LLM calls go only to things in play: hands, live subagents, woken gods, addressed characters and active
vicinities. Everything else is numbers. Any late call is replaced by its rules fallback for that beat, Final Night style.

**Match end.** Time's clock shrinks the arena. On the final beat (the Final Night) every outstanding debt comes due at once and
exactly one demigod is left standing. Every demigod gets a pile and a statue, as in Final Night.

## 5. Battle royale rules
- 4 demigods, health 10, Final Night's 0–10 damage scale.
- You win by being the last demigod alive. You can kill directly, through paladins turned against a rival, through riots, or
  through a rival's own unpaid debts.
- Clusters and characters can be recruited, worshipped by, turned and lost. A demigod with no followers loses favor fast.
- Hidden information is per seat: you see what your hands and subagents saw, what your followers know, and public omens.

## 6. Running it on the Theatre

**v0 needs no engine changes:**
- one seat per player, both hands in one submission (`fields: { left, right, zoom }`)
- `compose_seconds` = beat length; no choices, so Choose is skipped
- the client buffers speech while a beat resolves
- per-seat views carry the zoomed-in data

**Asks for theatre_worker, which makes the case for P1–P3:**
- **P2 read-only queries** are exactly what zoom needs: `game.cluster`, `game.character`, `game.ledger`, `game.venn`. The
  client can browse the world without sending everything every beat.
- A streamed `round.outcome` for the Narrator.
- Accepting actions during resolve, so input never stops.

**State size:** a few hundred clusters, up to ~1,000 memorized characters, and a ledger capped by folding old rows into
summaries. That keeps snapshots in the low megabytes, which should be tested early. Recording, replay, fork and timeline work
unchanged.

## 7. Jam scope (deadline 15 Nov)

**Backend must-haves:**
- the plugin with hands and subagents (limits: ≤3 live per hand)
- 4 gods: Time, Nature, Consequence, Death
- one city's clusters (seeded from real terrain plus a generated society table)
- memorization, the ledger, one-hop rumor spread, debts
- the beat loop with fallbacks
- a 4-agent playtest through the real server

**Frontend must-haves (your brother):**
- push-to-talk per hand
- the zoom levels: map → district → character
- the Narrator feed and god callouts
- health, favor and debt meters

**Stretch:**
- Chance and Memory gods; paladin conversion
- the post-match Venn and ledger visual
- Time rewind shown as a scrub
- statues

## 8. Reused from Final Night 2.5.0

| Final Night | Used for |
|---|---|
| Primer per vicinity + structured takes | The world resolver for ordinary acts |
| `theatre_toolkit` consensus (normalize, Jaro-Winkler, median) | Merging, and recognizing an already-memorized character |
| Embedded geography, terrain, places | The city and its districts |
| Teleporter / duplicator → blink / echo | Base powers of every hand |
| Episodes, witnesses, damage scale, health 10, final-night survivor | Combat and the ending |
| Avengers, stalemate surge, motif fatigue | Character behavior, Nature escalation, the Narrator |
| Rules-engine fallbacks | Every LLM step |
| Recorder, replay, fork, timeline | Unchanged; Time's rewind uses the snapshot ring |
| Piles + statue prompts | The end screen |

## 9. The planet edition (rules 0.4.0)

The designer's last BUILDLOG entries, built: **the arena is the whole planet** (regions = UN subregions of the
toolkit's Natural Earth countries, each with its largest cities; zoom planet → region → city → person; demigods start
in different regions; Time's circle closes from everywhere to 1,500 km around one city). **Two separate inputs.** Text
entry is the God of Time interaction: a seat slows time (everyone is told who), the world keeps running in fast flow
beats, others' orders are held, the initiator's Enter sends theirs and cuts off anyone slower, and Time bills by the
second. **Casts** are UX god powers from a data-driven palette (`data/powers.json`: lightning, storm clouds, volcano,
earthquake, flood, wildfire, meteor, raise/sink land, grow forest, swarm, or custom), paid in **lightning bolts** (every
bolt spent gives every rival 2); every cast gets **Entropy**'s check (the seventh god, who always says yes) and N
ramifications that leave geotagged events and persistent **world mods**. Every bolt raises the **Entropy** meter:
weather insanity, unreliable physics, riots and worship, gods taking damage, then heat death, the Final Night at once.
Hands raise **armies** from crowds that march and fight across the globe. Every row, news line, omen, cast and event is
geotagged, and the public view carries each demigod's influence per region. Flow/slow is v0 on today's engine (see
README, "Engine boundary").

## 10. Open questions
1. **Arena:** one real city (proposed), or the whole Earth with zoom?
2. **Pantheon:** are these six gods right, and which four are in the jam build?
3. **Hands:** two per player, with up to 3 subagents each, or more?
4. **Possession:** when you micromanage a character, do you speak *as* them (they say your words), or *to* them (they decide)?

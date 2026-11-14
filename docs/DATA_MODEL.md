# Final Night 7: data model and data capture

The data is designed before the game logic. The Rust types are in `crates/fn7-model`, which is the source of truth; this
document explains them and pins down everything that gets recorded.

## 1. Principles
- **One state, dense tables.** The whole game is one `World` value, which is the plugin state the Theatre snapshots at each
  commit. Each table is a `Vec` indexed by a typed id (`CharId(12)` is `characters[12]`). Rows are never removed, only marked
  (dead, dissolved, forgotten, folded), so ids stay stable for the whole match and in every recording.
- **One reference format.** `Ref` is written as `"ch:12"` and points at any row. The same string is the key in game state,
  trace events, SQL projections and client payloads. Prefixes: `dg` demigod, `ch` character, `cl` cluster, `gr` group,
  `gd` god, `pl` place, `it` item, `ag` agent, `lg` ledger row.
- **One way to change the world.** Every change is an `Effect`. Hands, subagents, gods, the resolver and the rules all
  produce `Proposal`s (an effect plus who, why and who saw it). The merge turns each proposal into a `MergeResolution` (applied,
  merged, overruled with the rule that decided it, or invalid with the reason). Each applied effect becomes a ledger row.
  That makes replay, auditing and the post-match visuals one pipeline.
- **Deterministic.** Randomness is SplitMix64 (`fn7_model::seed`), implemented locally and pinned by a test, so the n-th
  person materialized from a cluster is always the same person. Maps are sorted vectors, not hash maps. Floats round-trip
  exactly (`serde_json` `float_roundtrip`).
- **Prompts never contain engine ids.** `World::name_of(ref)` supplies names; ids appear only in the hidden mapping tables
  sent alongside a prompt.

## 2. State tables (`World`)

| Table | Row type | Key fields | Grows with |
|---|---|---|---|
| `meta` | `Meta` | schema (`STATE_SCHEMA` = 3), rules and pantheon versions, seed, beat, beats_total, beat_ms (= flow_ms), arena, arena_radius_km, arena_end_radius_km (the Final Night radius, set at setup), arena_kind (planet/city), entropy (0..100), heat_death (the beat it struck) | — |
| `places` | `Place` | kind (arena/district/site; on the planet: Earth/region/city), parent, center, radius, real terrain prose, tags, archetype (a district's or city's template key) | fixed at setup |
| `groups` | `Group` | kind (faction/trade/faith/neighborhood/family/following/emergent), permeability, patron god, follows | emergent groups |
| `clusters` | `Cluster` | label, size, place, traits, needs, `Mood`, baseline `Mood` (its template's; drift relaxes toward it), `GroupSet`, worship, allegiance, materialized count, seed, `army` (an `Army`: demigod, orders, pos, dest, target, raised_beat, from) | armies are new clusters; size shrinks |
| `characters` | `Character` | name, aliases, `Origin` (materialized from cluster #i / paladin / created / seeded), place, pos, groups, health, max_health, alive, allegiance, serves, possessed_by, memorized_beat/by, tempo | every memorization |
| `demigods` | `Demigod` | seat, pos, health, max_health, alive, `[left, right]` hands, following group, items, echo charges, words spoken, tempo, bolts, slows (beats it slowed time), orders (`StandingOrder`s: hand, verb, targets, place, magnitude, span, since) | 4, fixed |
| `items` | `Item` | origin (starting/summoned/found/echoed/granted), creator, holders (only grows), uses | summons, echoes |
| `relations` | `Relation` | directed (a→b) trust, fear, debt, loyalty; sorted by (a, b) | interactions |
| `agents` | `Agent` | `AgentKind` (hand/subagent/god/paladin/possession), goal, scope, budget, status, report, children | spawns |
| `gods` | `GodState` | name (from the pantheon), favor per demigod, attention, paladins, its own agent, invocations, health (0..10; Entropy wounds gods) | 7, fixed |
| `rulings` | `Ruling` | god, intent, invoker, `Verdict` (grant/refuse/bargain/twist), price, speech, dispatched (an existing paladin), dispatch_mission, fallback flag, bolts paid, at (where it was invoked) | invocations, and Entropy's yes per region with casts |
| `intents` | `Intent` | agent, demigod, verb, targets (known refs or unresolved `Mention`s), magnitude, the words it came from, `Route` | every beat |
| `ledger` | `LedgerRow` | source, actor, verb, act prose, targets, place, magnitude 0–10, valence ±5, witnessed_by, known_by (`GroupSet`), known_at (the beat each group learned it), told_to, erased_from, `Debt`, cause (the chain link), intent, at (`[lon, lat]`) | every applied effect |
| `memory` | `MemoryItem` | owner, kind (witnessed/did/told/rumor/inherited/planted/summary/report), text, about, ledger, from, salience, confidence, forgotten, folded_into, folded_beat | every beat |
| `rewind` | `RewindRing` | the last N `RewindFrame`s of rewindable fields (pos, health, alive) | fixed cap |
| `beats` | `BeatRecord` | per-stage timing, LLM calls and fallbacks, counts, ledger range, memorized, rulings, spawned, eliminated, caption | one per beat |
| `seats` | `SeatState` | seat, demigod, is_bot, zoom, focus: each seat's view (set by the plugin; synthetic seats ≥ 200 are the plugin's own AI demigods) | fixed |
| `pending` | `Proposal` | Consequence's collections of the debts due this beat, queued at the beat's Start and merged in its Resolve (so a Time delay can beat them); empty between beats | per beat |
| `mods` | `WorldMod` | id, region, place, at, radius_km, name, description, tags, per_beat (`ModEffects`: harm, heal, anger, fear, hope, devotion), since_beat, expires_beat, stacks, by, power; live only (≤ `MAX_MODS` = 24; `mod_seq` is the next id) | casts' ramifications |
| `slow` | `SlowWindow` | the open slow-time window: initiator, opened_beat (its id), deadline_beat, opened_at (client ms), paid (the bolt), held (`HeldOrders`: demigod, left, right, beat, sent_at) | at most one |

**Field notes (schema 2).**
- `max_health` (default 10) is per body. `Price { kind: Life }` lowers it (health clamps to it; a cap of 0 kills), and
  heals, rewinds and revives cap at it.
- `tempo: Vec<Tempo { factor, until_beat }>` on demigods and characters holds Time's active slow (`factor` < 1) and
  haste (> 1) effects; `tempo_factor(&tempo, beat)` is the product of the active factors. Expired entries are dropped
  when a new one is added.
- `LedgerRow.known_at` lists `(group, beat)` in the order learned: the witnesses' groups at the row's beat, then one
  entry per spread hop. Rumor memories take their hop count from it (beats from the event to the group's entry), so a
  late write doesn't weaken a rumor.
- `Ruling.dispatch_mission` is set whenever a ruling sends a paladin. With `dispatched: None` it means a new paladin
  is to be raised (the plugin spawns it through `Effect::Dispatch`).
- `World::name_of(Ref::God)` returns `GodState.name`; a ledger row is "an earlier event".
- `SetupConfig` (match setup parameters, with `Default`) and `SpreadHop` (one hop of news: row, group, via, beat,
  chance) are plain model types, so the server and tools don't need `fn7-society` for them.
- `Meta.settings` holds the plugin's validated room settings (README, Settings); `World.seats` / `World.pending` are
  plugin bookkeeping (above). `Effect::Dispatch` carries `paladin: Option<CharId>`: an existing paladin of that god to
  retask (its mind relives with the new mission and the target as scope); `None`, or one that can't serve, raises a
  new one. All `#[serde(default)]`.
- **Live state vs the recording.** The state keeps only what is deterministic given the seed and the LLM replies, so a
  replay reproduces every snapshot byte for byte: a `BeatRecord` in state has its counts, caption and
  `timing.llm_calls` / `llm_fallbacks`, but `started_at_ms` 0 and empty `timing.ms` / zero `llm_ms_max`; the
  wall-clock values are in the `beat` event (`data.record`). To bound state size (under 4 MB at beat 100, checked by a
  test): folded memory originals keep their row but drop `text` and `about` (the `memory_fold` event carries them in
  full), a fold summary keeps its 12 most-mentioned `about` refs, rows past `SPREAD_HORIZON` drop `known_at` (the
  `spread` / `ledger_row` events have it; `known_by` stays), and `MemoryItem` / `LedgerRow` omit empty or default
  fields when serialized (`text`, `about`, `ledger`, `from`, `forgotten`, `folded_*`; `place`, `known_at`, `told_to`,
  `erased_from`, `debt`, `cause`, `intent`); readers default them.
- Every new field is `#[serde(default)]`, so schema-1 states still load: `max_health` 10, empty `tempo` and
  `known_at`, `baseline` 0.25 each (the old resting mood), an `arena_end_radius_km` of 0 (readers fall back to
  start radius × `ARENA_END_FRACTION`), an empty god `name` (`name_of` falls back to the god's mind agent).

**Schema 3 (the planet edition).** Every new field is `#[serde(default)]` (a schema-2 state loads as a city arena with
entropy 0, no bolts, gods at health 10, no mods or window). `LedgerRow.at` is set when the row is appended: a cast's or
happening's point, else the first target's position, else the actor's, else the place's center. On the planet the
places are Earth (the arena, centered on the convergence city; start radius 20,037.5 km, end 1,500 km), every UN
subregion of the toolkit's embedded Natural Earth countries that has a major city (a District: "Western Africa"), and its
three largest cities (Sites, 60 km, each with a templated archetype); crowds and notables live in the cities;
`geo::region_at` maps any point to a region through the country polygons (else the nearest region). `Zoom` adds `city`.
The power palette (`data/powers.json`, `PowerDef`: slug, name, icon, cost in bolts, radius_km, target point/region/unit,
magnitude, damage, tags, blurb) is compiled in like the pantheon.

**Effects.** `Effect` (in `intent.rs`) is the only way the world changes. Schema 3 adds `Cast`, `Bolts`, `Entropy`,
`RaiseArmy`, `March`, `Wound`, `Mod`, `Unmod` and `Happen` (below the schema-2 table). Schema 2 added:

| effect | what it does | invalid when |
|---|---|---|
| `Revive { who }` | Death's raise: a dead demigod or character comes back at `REVIVE_FRACTION` (0.5) of its `max_health`; a demigod's hands go live again | `who` is alive, has no body, or has a `max_health` of 0 |
| `Tempo { who, factor, beats }` | Time's slow/haste: pushes `Tempo { factor, until_beat: beat + beats }` onto the body | factor outside 0.25..4 or not finite, beats outside 1..10, `who` dead or bodiless |

| effect | what it does | invalid when |
|---|---|---|
| `Cast { by, power, at, target, cost, damage }` | A god power: `by` pays `cost` bolts, every other live demigod gains 2 per bolt, Entropy +0.4 per bolt; `damage` to a body target (a crowd or army loses a share) | `by` dead or short of bolts, a bad point |
| `Bolts { who, delta, to }` | delta < 0: bolts paid (to god `to`, or Time for a slow window), with the same gifts and Entropy; delta > 0: Entropy's gift (Entropy +0.2 per bolt) | not enough bolts |
| `Entropy { delta }` | Moves the meter (clamped 0..100); reaching 100 is heat death: `meta.heat_death` = this beat, `beats_total` = this beat | not finite |
| `RaiseArmy { demigod, from, size, name, orders }` | A new army cluster (in the demigod's Following) split from a crowd | `from` is an army or too small |
| `March { army, to, dest, target, orders }` | Moves an army (at most `ARMY_KM_PER_BEAT` = 900 km) and re-aims it | not an army, empty, too far |
| `Wound { god, amount }` | A god loses health (Entropy ≥ 80) | — |
| `Mod { m }` / `Unmod { id }` | A world mod appears (same name in the same region stacks, ≤ 5; past 24 the oldest is pushed out) / ends | bad point / unknown id |
| `Happen { text, at }` | Narration at a point on the globe (ramifications, freak weather) | bad point |

**Society Venn.** Membership is stored on the member as a sorted `GroupSet`, not on the group. Overlap tests are linear
merges, and "members of A ∩ B ∖ C" is a scan of clusters plus characters, which at these sizes (≤ 256 groups, ~300 clusters,
≤ ~1,000 characters) runs in microseconds.

**Size budget** (checked by the simulator and a test on the planet): under 4 MB of state at beat 100. `ledger` and `memory` fold old entries into
summaries past a threshold. The folded originals stay in the recording, so nothing is lost; only the live state is trimmed.

## 3. Data capture: everything is recorded

Recording goes through `theatre-trace` / `theatre-recorder` (shared with Final Night, and moving into the Theatre core with
P1). Each match is a run carrying its rules version, pantheon version and `STATE_SCHEMA`.

**Snapshots** (full `World`) at: `match_start`, each beat's `round_start` and `resolved`, `committed`, and `final`.

**LLM steps.** Each call is annotated with step and actor, so prompts, every sample, raw HTTP and headers are recorded:

| step | actor | one call per |
|---|---|---|
| `hand` | `ag:*` (the hand) | hand with input, per beat |
| `route` | `in:*` | intent no keyword could route (not made: the hand mind routes its own intents, keyword first) |
| `subagent` | `ag:*` | live subagent, per beat |
| `possession` | `ag:*` | live possession, per beat |
| `god` | `gd:*` | invocation |
| `paladin` | `ag:*` | dispatched paladin, per beat |
| `resolver` | `pl:*` (vicinity) | active vicinity, per beat |
| `materialize` | `cl:*` | memorization that needs a voice (the name and traits come from the seed; the LLM writes the flavor) |
| `fold` | owner ref | memory or ledger fold |
| `ramify` | `pl:*` (region) | region with casts this beat: Entropy's yes and N ramifications ("N ramifications in <region> with <context>", N = 1 + cost + magnitude/3 per cast, ≤ 10) |
| `narrator` | — | beat (beat N's call runs during beat N+1; the last beat's in the summary) |
| `final_cut` | — | match (the summary's story, told the engine's result and not to contradict it) |

Resolvers, gods, steps, materialize and ramify calls of a beat run in one concurrent batch; the previous beat's narrator and the
LLM memory folds run alongside the beat's hands and act batch. Every free-text field a mind returns has its prompt
labels (`P1`, `C3`, `K2`...) mapped back to names before it reaches the ledger, memory or a payload.
A call that fails, times out (`mind_timeout_ms`) or returns junk is replaced by its rules fallback and recorded as an
`llm_fallback` event (`{step, why}`). **Quiet minds use their rules by design** (not counted as LLM calls or fallbacks):
a resolver whose acts are all standing orders or from demigods who didn't speak, subagents and possessions of demigods
who didn't speak, paladins whose god didn't rule this beat, the narrator of a beat that was quiet (nobody spoke, cast or
fell), and memory folds in a quiet beat. Hands only run when their player spoke. So a flow beat nobody spoke in makes no
LLM call.

**Events** (`events.kind`, with `actor` a Ref string, `data` the full typed object):

| kind | data |
|---|---|
| `utterance` | `Utterance` (raw text, mode, words, spoken_ms); actor the demigod |
| `intent` | `{beat, intent: Intent, routed_by: keyword/llm/null, fallback}` (the `Intent` includes its `Route`); `standing: true` for a standing order carried out by rules, `cast: true` for a cast (routed to Entropy) |
| `mention_resolved` | `{beat, intent, mention, to: Ref or null, score, how: existing/materialized/named/none}` |
| `memorized` | `{beat, character: Character (groups as at memorization), cluster, index, seed}` (paladins too, with no cluster) |
| `agent_spawned` / `agent_step` / `agent_ended` | `{beat, agent: Agent, by}` / `{beat, agent, step, said, done, proposals, fallback}` / `{beat, agent: Agent, status, report}` |
| `ruling` | `Ruling` |
| `proposal` | `{beat, index, proposal: Proposal}` |
| `resolution` | `{beat, resolution: MergeResolution, rule}`; rule ∈ applied, invalid, damage_median, time_delay_beats_collect, rewind_undoes, precedence, chance_breaks_tie |
| `ledger_row` | `{beat, row: LedgerRow as appended, effect: Effect, proposal}` (`effect.fx` is the effect tag) |
| `spread` | one `SpreadHop` per event: `{row, group, via, beat, p}` (mirrored in `LedgerRow.known_at`) |
| `debt_due` / `debt_delayed` / `debt_collected` | `{beat, row, debt}` (+ `beats`, `by` = the delaying/collecting row) |
| `rewind` | `{beat, target, beats, frame: RewindEntry restored, row}` |
| `memory_write` / `memory_forget` / `memory_fold` | `{beat, items: [MemoryItem], row?}` / `{beat, owner, about, ids, row}` / `{beat, summary: MemoryItem, from: [ids], originals: [MemoryItem]}` |
| `hand_said` | `{agent, beat, said}`: the hand's one spoken line back to its player (`HandOut.said`) |
| `zoom` | `{beat, seat, zoom, focus}` |
| `beat` | `{beat, record: BeatRecord with wall-clock stage timings, total_ms}`. The record's `caption` is empty: the narrator runs one beat late (below) |
| `caption` | `{beat, caption, callouts, fallback}`: beat `beat`'s narration, emitted during the next beat (`events.round` = `beat + 1`), or in the summary for the last beat. `fallback` = the template was used. The caption is also set on that beat's `BeatRecord.caption` in state |
| `narration_rejected` | `{beat, lines}`: narrator lines that contradicted the engine (an already-out demigod acting, a survivor fallen); the caption fell back to the template, callouts were dropped |
| `final_cut_rejected` | `{champions, story}`: a final cut that had the champion fall (the summary uses the joined captions instead) |
| `elimination` | `{demigod, beat, cause, chain: [LedgerId] (the harmful rows that beat, then their causes), name}` |
| `final_night_survivor` | `{beat, demigod, revived, final_night}` (the survivor rule fired) |
| `llm_fallback` | `{step, why}` (from `fn7-agents`) |
| `slow_started` | `{beat, initiator, window, sent_at, deadline_beat, paid_bolt, with_orders}`: a seat opened a slow window (text entry) |
| `slow_closed` | `{beat, initiator, window, how: enter/timeout/fell, beats_open, ms_open, measured: client/beats, opened_at, enter_at, released, cut_off, debt, entropy, recent_slows, paid_bolt}`: time-to-Enter is `ms_open` |
| `slow_refused` | `{beat, demigod, window, reason}`: a second slow request while a window is open |
| `orders_held` / `orders_released` | `{beat, demigod, window, sent_at}` / `{beat, window, demigods}`: orders sent during someone's window, and their release at its close |
| `orders_cut_off` | `{beat, demigod, window, reason, sent_at, enter_at}`: orders that came after the initiator's Enter (or for a closed window) |
| `standing_orders` | `{beat, demigod, hand, orders}`: a hand's standing orders after its player spoke |
| `cast` / `cast_fizzled` | `{beat, by, power, cost, at, region, target, damage, off_target, row, bolts_left, custom, entropy}` / `{beat, by, power, cost?, have?, reason}` |
| `ramifications` | `{beat, region, n, casts, proposals, ruling, fallback}` (the proposals follow as `proposal`/`ledger_row` events) |
| `entropy` / `entropy_level` / `heat_death` | `{beat, before, after, delta, level}` / `{beat, from, to, key}` / `{beat, entropy, beats_total}` |
| `mod_created` / `mod_expired` | `{beat, mod: WorldMod, stacked, row}` / `{beat, id, row, reason?}` |

The Theatre round number is the beat everywhere (`events.round` = `data.beat`), except `caption`, which is emitted one
beat after the beat it describes.

**Captions arrive one beat late.** Beat N's narrator call (step `narrator`) runs during beat N+1, alongside its hands and
act batch; its reply lands in the `caption` event and on beat N's `BeatRecord.caption` at beat N+1's narrate stage
(always there, so timing never changes the state). Clients get it in beat N+1's `round.outcome.public` as
`previous_caption` / `previous_callouts` with `caption_beat: N`; the last beat's caption is in
`match.ended.summary.last_caption`. A projection or tool that wants a beat's caption reads the `caption` event (or the
final snapshot's `beats[].caption`), never the `beat` event.

**Client payloads** are built from the state by `fn7-plugin` (`payload.rs`, `phases.rs`, `beat.rs`) and are not part of
it: ids are `Ref` strings, omens are `{god, god_ref, speech}`, subagents are flat `SubagentLine` objects, news is newest
first with one line per ledger row, and floats are rounded (2 decimals; positions 6) at the payload layer only. The
state and the snapshots keep full f32 values, so replays stay byte-identical. Shapes: `docs/CLIENT_API.md` and
`docs/schemas/`.

**Protocol frames**: every WebSocket frame in both directions, verbatim, with millisecond timestamps (`fn7-agents` and the
core's message log).

## 4. SQL projection (`fn7-project`)

The recorder's tables stay generic. `fn7-project` reads a run's events and writes typed tables next to them, so analysis
and the UI's post-match views are plain SQL:

```sql
CREATE TABLE fn7_entities   (run_id, ref, kind, name, origin, cluster_ref, created_beat, ended_beat, PRIMARY KEY (run_id, ref));
CREATE TABLE fn7_membership (run_id, member_ref, group_ref, from_beat, to_beat);
CREATE TABLE fn7_ledger     (run_id, row_ref, beat, source, actor_ref, verb, act, place_ref, magnitude, valence,
                             cause_ref, intent_ref, debt_god_ref, debt_amount, debt_due, debt_status, PRIMARY KEY (run_id, row_ref));
CREATE TABLE fn7_ledger_ref (run_id, row_ref, role, ref);          -- role: target | witness | told | erased
CREATE TABLE fn7_known      (run_id, row_ref, group_ref, beat);     -- spread over the Venn, one row per known_at entry
CREATE TABLE fn7_memory     (run_id, mem_ref, owner_ref, beat, kind, text, salience, confidence, ledger_ref, forgotten, folded_into, folded_beat);
CREATE TABLE fn7_agents     (run_id, agent_ref, kind, parent_ref, demigod_ref, god_ref, body_ref, goal, spawned_beat, ended_beat, status, steps, report);
CREATE TABLE fn7_rulings    (run_id, ruling_ref, beat, god_ref, invoker_ref, intent_ref, verdict, price_kind, price_amount, speech, dispatched_ref, dispatch_mission, fallback);
CREATE TABLE fn7_intents    (run_id, intent_ref, beat, agent_ref, demigod_ref, verb, magnitude, route, routed_by, span);
CREATE TABLE fn7_beats      (run_id, beat, started_at, intake_ms, route_ms, act_ms, merge_ms, consequence_ms, narrate_ms,
                             total_ms, llm_calls, llm_fallbacks, words, intents, applied, caption);
CREATE TABLE fn7_geo        (run_id, row_ref, beat, lon, lat, place_ref, PRIMARY KEY (run_id, row_ref));   -- every row's geotag
CREATE TABLE fn7_casts      (run_id, beat, by_ref, power, cost, lon, lat, region_ref, target_ref, row_ref, off_target, fizzled);
CREATE TABLE fn7_slows      (run_id, window, initiator_ref, opened_beat, closed_beat, how, beats_open, ms_open, measured,
                             released, cut_off, debt, entropy, paid_bolt);
CREATE TABLE fn7_cutoffs    (run_id, beat, demigod_ref, window, reason, sent_at, enter_at);
CREATE TABLE fn7_entropy    (run_id, beat, before, after, level);
CREATE TABLE fn7_mods       (run_id, mod_id, beat, region_ref, name, lon, lat, radius_km, stacked, expired_beat);
CREATE INDEX fn7_ledger_actor  ON fn7_ledger (run_id, actor_ref, beat);
CREATE INDEX fn7_ledger_cause  ON fn7_ledger (run_id, cause_ref);
CREATE INDEX fn7_ledger_ref_ix ON fn7_ledger_ref (run_id, ref, role);
CREATE INDEX fn7_known_group   ON fn7_known (run_id, group_ref, beat);
CREATE INDEX fn7_memory_owner  ON fn7_memory (run_id, owner_ref, beat);
CREATE INDEX fn7_member_group  ON fn7_membership (run_id, group_ref);
CREATE INDEX fn7_agents_parent ON fn7_agents (run_id, parent_ref);
```

Queries these support directly:
- **The orchestration tree:** `fn7_agents` by `parent_ref`.
- **A Final Destination chain:** a recursive CTE on `fn7_ledger.cause_ref`.
- **How a rumor reached a demigod:** `fn7_known` ordered by beat.
- **Who knew what, when:** `fn7_known` joined to `fn7_membership`.
- **Every god's rulings and prices:** `fn7_rulings`.
- **Words per minute against debt:** `fn7_intents` joined to `fn7_ledger`.
- **Time-to-Enter and who got cut off:** `fn7_slows` (ms_open) and `fn7_cutoffs`.
- **The Entropy curve, the casts and the globe's scars:** `fn7_entropy`, `fn7_casts`, `fn7_mods`, `fn7_geo`.
- **Where each beat's time went:** `fn7_beats` (timings from the `beat` event; `caption` from the `caption` event, since
  the narrator runs one beat late).

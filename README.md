# Final Night 7

A real-time battle royale of demigods, played through orchestrator agents in a world that remembers everyone you've
spoken to. A game plugin for [The Theatre](https://github.com/Aftersource/theatre).

- `DESIGN.md`: the game
- `docs/DATA_MODEL.md`: every table, every recorded event, the SQL projection
- `docs/CLIENT_API.md` (+ `docs/schemas/`, `docs/examples/`, `docs/AGENT_GUIDE.md`): every client payload. Payloads are
  built in `crates/fn7-plugin/src/payload.rs` (shared shapes, float rounding) and `phases.rs` / `beat.rs`; after changing
  one, run `cargo run -p fn7-tools --bin fn7-schemas` and re-extract the examples (`fn7-schemas --examples-from`)

| Crate | Owns | Depends on |
|---|---|---|
| `fn7-model` | All data types. No logic, no I/O | serde |
| `fn7-society` | Clusters, memorization, Venn queries, rumor spread, mood, ledger writes (deterministic rules) | model, toolkit |
| `fn7-memory` | Memory stores, recall, folding | model |
| `fn7-pantheon` | God definitions (`data/pantheon.json`), the power palette (`data/powers.json`), keyword routing, favor, prices, precedence | model |
| `fn7-agents` | LLM minds: hands, subagents, possessions, gods, paladins, resolver, narrator, ramifications; fallbacks | model, memory, pantheon, plugin-api, toolkit, trace |
| `fn7-plugin` | The `GamePlugin`: beat loop, merge, views | all of the above |
| `fn7-server` | Theatre server binary with the plugin and recorder; `fn7-sim` (in-process matches, replay verification) | plugin, recorder |
| `fn7-tools` | sim, agents, replay, fork, timeline, SQL projection | plugin, recorder |

The dependency direction only ever points down this table. Game rules never touch I/O, and LLM minds never mutate state:
they return proposals, and only the merge in `fn7-plugin` applies effects.

```
cargo build --workspace && cargo test --workspace
```
Local development uses `.cargo/config.toml` (gitignored) to patch the Theatre to `../theatre` and build offline from its
vendored crates; recording comes from `../demo_final_night/{trace,recorder}` until it moves into the Theatre core.

## Running

**Server.** `fn7-server` is the Theatre server with Final Night 7 compiled in (same `THEATRE_*` / `LLM_*` configuration
as `theatre`; `THEATRE_GAME` is ignored). Clients connect over WebSocket at `/ws`.

```
cargo run --release -p fn7-server --bin fn7-server            # THEATRE_BIND=0.0.0.0:8080 by default
```
- **LLM:** Claude (Anthropic Messages API over raw HTTP, `src/anthropic.rs`) when `ANTHROPIC_API_KEY` is set and
  `LLM_BASE_URL` is not: model `LLM_DEFAULT_MODEL` (default `claude-haiku-5-5`), `ANTHROPIC_EFFORT` (default `low`),
  `ANTHROPIC_MAX_INFLIGHT` (default 8). Otherwise the core's OpenAI-compatible gateway (`LLM_BASE_URL`, `LLM_API_KEY`,
  `LLM_DEFAULT_MODEL`), and with neither, no LLM: every mind uses its rules fallback.
- **Recording:** every match goes to `THEATRE_RECORD` (default `runs/fn7.sqlite`; `off` disables) through
  `theatre_recorder::RecordingPlugin`: hooks, snapshots, LLM calls, raw HTTP, events (see `docs/DATA_MODEL.md` §3).

**Simulator.** `fn7-sim` plays a whole match in-process through the `GamePlugin` trait exactly as the core drives it
(Start → Brief → Compose → Resolve → Commit), with bot seats, recorded, printing each beat's caption and stage timings.

```
cargo run --release -p fn7-server --bin fn7-sim -- --seed 7 --beats 100          # rules fallbacks only, no LLM
cargo run --release -p fn7-server --bin fn7-sim -- --scripted --beats 30         # offline scripted replies for every mind
cargo run --release -p fn7-server --bin fn7-sim -- --claude --beats 10 --budget 0.5   # Claude (spends money)
cargo run --release -p fn7-server --bin fn7-sim -- --replay runs/fn7.sqlite --run <run-id>
```
Flags: `--seed --city --beats --players (seats, all bots) --clusters --mind-timeout-ms --stop-after --record <db>
--no-record --claude --model --scripted --budget <usd> --verbose --settings <json>` (`--settings '{"entropy_scale": 4,
"arena": "city"}'` merges extra room settings). `--replay <db> [--run <id>]` re-executes a recorded
run from its `match_start` snapshot with every LLM request answered from the recording
(`theatre_recorder::ReplayProvider`) and the recorded actions, and checks every snapshot (`round_start`, `resolved`,
`committed`, `final`) byte for byte; it exits non-zero on any difference.

## Settings

Room settings (`validate_settings` fills defaults; one Theatre round = one beat; there is no Choose phase):

| setting | default | range | meaning |
|---|---|---|---|
| `seed` | picked at match start | u64 | All randomness derives from it (recorded in `meta.seed` / `meta.settings`) |
| `arena` | `planet` | planet \| city | The whole Earth (regions = UN subregions, each with its 3 largest cities) or one real city (districts → sites) |
| `city` | seeded pick | name | Fuzzy match; cities ≥ 1M on land when unset. On the planet: where Time's circle closes for the Final Night |
| `players` | 4 | 1–8 | Demigods in the match; seats come first, the plugin's own AI plays the rest (synthetic seats ≥ 200) |
| `beats_total` (alias `rounds`) | 100 | 1–1000 | The Final Night falls on this beat |
| `flow_ms` (alias `beat_ms`) | 3000 | 500–120000 | Flow beat length; `compose_seconds` defaults to `flow_ms / 1000` |
| `slow_ms` | 30000 | 1000–600000 | Longest a slow-time window stays open (`ceil(slow_ms / flow_ms)` flow beats) |
| `start_bolts` | 3 | 0–99 | Lightning bolts per demigod at the start |
| `entropy_scale` | 1.0 | 0–10 | Multiplier on every Entropy rise (playtest dial) |
| `clusters` | 200 | 8–600 | Population clusters created at setup |
| `max_subagents_per_hand` | 3 | 0–10 | Live subagents + possessions per hand |
| `mind_timeout_ms` | 6000 | 50–120000 | Per-call timeout for every mind; a late call takes its rules fallback |
| `llm` | true | bool | false = never call the LLM (every mind falls back, no requests) |
| `debt_scale` | 0.04 | 0–10 | Consequence's bill: `debt_scale × Σ debt_for(magnitude, words this beat)` per demigod per beat |
| `debt_due_beats` | 6 | 1–100 | Beats until a bill falls due |
| `memory_per_owner` | 40 | 4–1000 | Live memories per owner before old ones fold into a summary |
| `llm_folds_per_beat` | 2 | 0–32 | Folds written by the fold mind per beat (the rest by rules) |
| `max_intents_per_hand` | 6 | 1–12 | Acts one hand turns its speech into per beat (orders sent in the beat they open their window) |
| `max_intents_per_window` | 12 | 1–24 | Acts per hand from orders that closed (or were held in) a window open at least one beat |
| `max_memorize_per_beat` | 6 | 0–64 | People memorized from player mentions per beat |
| `max_witnesses` | 4 | 1–64 | Witnesses recorded per act (targets and actor first) |
| `compose_seconds`, `choose_seconds`, `resolve_timeout_s` | `flow_ms/1000`, core, 60 | core | The core's timers, copied through (`util::copy_timer_settings`) |

**Actions** (two separate paths; `docs/CLIENT_API.md` has the full contract). *Text entry*, the God of Time
interaction: `fields: {slow, left, right, zoom, window, sent_at}`: `slow: true` opens a slow-time window (the world keeps
running in flow beats; others' orders are held), the initiator's orders (Enter) close it, orders sent after the Enter
are cut off, and Time charges a bolt plus a debt that grows with the seconds the window stayed open. Without
`left`/`right` fields, `text` is read as lines prefixed `l:` / `r:` (a `/slow` line = `slow`). Empty hands make no LLM
call; silent hands carry out their standing orders by rules. *Casts*, UX god powers: `fields: {cast | casts | bolt |
bolts}` (`{power, target: ref | [lon, lat] | name, text?, magnitude?}`), paid in bolts (every other demigod gains 2 per
bolt), Entropy's yes, and a ramifications call per region.

**Engine boundary.** The plugin uses only the Theatre's public `GamePlugin` API (`theatre_plugin_api`): settings
validation (including `compose_seconds` via `util::copy_timer_settings`), the round hooks, `validate_action`,
`default_action`, `bot_action`, `resolve_round` with the actions the core delivers, `commit_round`, the views and
`summarize`. Flow/slow v0 is game logic over those actions; the real mechanic (a Compose the initiator closes, an
extendable deadline, mid-phase pushes, a typing signal, instant actions) waits for engine proposal v2.2.

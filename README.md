# Final Night 7

A real-time battle royale of demigods, played through orchestrator agents in a world that remembers everyone you've
spoken to. A game plugin for [The Theatre](https://github.com/Aftersource/theatre).

- `DESIGN.md`: the game
- `docs/DM_RULEBOOK.md`: the Dungeon Master's rulebook, the single rules reference the AI DM follows (rules 0.8.0)
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
| `fn7-server` | Theatre server binary with the plugin (the core records every match); `fn7-sim` (in-process matches recorded in the core's JSONL format, replay verification) | plugin, theatre-server |
| `fn7-tools` | agents (client recording), JSONL → SQLite ingest + SQL projection, client schemas | model, pantheon, theatre-trace |

The dependency direction only ever points down this table. Game rules never touch I/O, and LLM minds never mutate state:
they return proposals, and only the merge in `fn7-plugin` applies effects.

```
cargo build --workspace && cargo test --workspace
```
Without any local config this builds against the Theatre's GitHub `main` (`Aftersource/theatre`, git dependencies;
needs network for the first build: GitHub + crates.io). Local engine development uses `.cargo/config.toml`
(gitignored) to `[patch]` the Theatre crates (`theatre-plugin-api`, `-llm`, `-server`, `-protocol`, `-toolkit`,
`-trace`) to `../theatre` and build offline from its vendored crates (`../theatre/vendor` is local only, not in the
Theatre repo). FN7 depends on nothing outside these two repos. A fresh Linux/WSL build is in `docs/AGENT_GUIDE.md`
("Fast-forward, time travel and local runs").

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
- **Accounts** (docs/ACCOUNTS.md): guest accounts work with no setup (`POST /api/auth/guest`); Google sign-in needs
  `FN7_GOOGLE_CLIENT_ID`, `FN7_GOOGLE_CLIENT_SECRET` and `FN7_PUBLIC_URL` (redirect URI:
  `<FN7_PUBLIC_URL>/api/auth/google/callback`); `FN7_NAME_GUARD=off` stops the name guard; `FN7_MODERATION=rules`
  judges demigod sheets by rules only. The scripted client: `node docs/clients/fn7_accounts_e2e.mjs --url http://HOST:PORT`.
- **Recording** is the Theatre core's (P1): run with `THEATRE_RECORD=full` (levels `off | events | full`; default
  `events`) and `THEATRE_RECORD_DIR=runs/recordings` (default `<THEATRE_DATA_DIR>/recordings`). Every match is
  `<dir>/<match_id>.jsonl`: run start/end, rulesets (plugin `version`, `rules_version`), every core and plugin event,
  hooks with inputs/outputs, LLM calls, state snapshots, and at `full` protocol frames and raw LLM HTTP (never
  credentials). JSONL is the store of record; `fn7-project` loads it into SQLite for analysis (see
  `docs/DATA_MODEL.md` §3–4). `THEATRE_REPLAY_FROM=<recording.jsonl>` answers every LLM call from a recording.
- **Images:** `FN7_IMAGES` (off by default) and the `/media` routes, see `docs/MEDIA.md`.
- **Assets:** every generated image and 3D model in one registry (`<THEATRE_DATA_DIR>/fn7/assets/`: content-addressed
  files + `assets.sqlite` with the exact prompts, timing, costs, lineage and uses), the `/api/assets` routes and the
  `fn7-assets` CLI, see `docs/ASSETS.md`.

Runbook (LAN / tunnel):
```
THEATRE_RECORD=full THEATRE_RECORD_DIR=runs/recordings THEATRE_ADMIN_TOKEN=<secret, from your shell or .env> \
  cargo run --release -p fn7-server --bin fn7-server
```

**Recording, replay and the admin API.** With `THEATRE_ADMIN_TOKEN` set (never print or commit it), the core serves
`/admin/*` over HTTP with `Authorization: Bearer <token>` (404 without the variable, 401 without the header). FN7 writes
no fork or timeline files of its own: these are the engine's.
```
curl -H "Authorization: Bearer $THEATRE_ADMIN_TOKEN" http://HOST:8080/admin/matches                     # recorded matches
curl -H "Authorization: Bearer $THEATRE_ADMIN_TOKEN" http://HOST:8080/admin/matches/<id>/recording      # raw JSONL
curl -H "Authorization: Bearer $THEATRE_ADMIN_TOKEN" http://HOST:8080/admin/matches/<id>/timeline       # compact items (+ ?frames=true)
curl -H "Authorization: Bearer $THEATRE_ADMIN_TOKEN" "http://HOST:8080/admin/matches/<id>/state?after_round=5&view=seat:0"
                                                                    # view = raw | public | seat:N (or at=<record id>)
curl -H "Authorization: Bearer $THEATRE_ADMIN_TOKEN" -H 'content-type: application/json' \
     -X POST http://HOST:8080/admin/fork -d '{"match_id":"<id>","after_round":5}'
     # -> {room_id, code, match_id, resumes_at_round, players: [{seat, display_name, player_id, resume_token}]}
node docs/clients/fn7_client.mjs --url ws://HOST:8080/ws --resume <resume_token> --auto   # take the seat back
```
A fork is a new private room that resumes after round N (bots take seats nobody resumes within the resume TTL); its
recording starts from a `fork` snapshot and points at its parent (`run_start.meta.forked_from`). Replay-verify any
recording (server or `fn7-sim`, a fork included):
```
cargo run --release -p fn7-server --bin fn7-sim -- --replay runs/recordings/<match_id>.jsonl
cargo run --release -p fn7-tools --bin fn7-project -- --db runs/fn7.sqlite runs/recordings/   # JSONL -> SQLite + fn7_* tables

**Simulator API (`/sim/*`).** The server also runs balance simulations for remote agents that don't build this code:
`POST /sim/run {settings, seeds, players?, beats?, society_only?, record?, llm?, budget_usd?}` and `POST /sim/compare`
(the same seeds under settings A and B) queue in-process matches on their own worker threads (`FN7_SIM_WORKERS`,
default 2; live rooms are never blocked), `GET /sim/jobs/<id>` returns per-seed results and mean/median aggregates,
`GET /sim/settings` lists every setting with default, range and description (generated from `settings.rs`). Same auth
as the admin API (`THEATRE_ADMIN_TOKEN`: 404 unset, 401 wrong). Rules-only results are deterministic per seed. Every job
is logged (target `fn7_sim`) and written to `<FN7_SIM_LOG_DIR, default runs/sim>/fn7-sim-jobs.jsonl`; `record: true`
recordings go to `FN7_SIM_RECORD_DIR` (default: the core's recording dir, so `/admin/matches` reads them too). Code in
`crates/fn7-server/src/simapi.rs`; usage in docs/AGENT_GUIDE.md §12.

**Simulator.** `fn7-sim` plays a whole match in-process through the `GamePlugin` trait exactly as the core drives it
(Start → Brief → Compose → Resolve → Commit), with bot seats, recorded in the core's JSONL format (the engine's
`JsonlSink` and `RunRec`, same records and snapshot labels as a server recording), printing each beat's caption and
stage timings.

```
cargo run --release -p fn7-server --bin fn7-sim -- --seed 7 --beats 100          # rules fallbacks only, no LLM
cargo run --release -p fn7-server --bin fn7-sim -- --scripted --beats 30         # offline scripted replies for every mind
cargo run --release -p fn7-server --bin fn7-sim -- --claude --beats 10 --budget 0.5   # Claude (spends money)
cargo run --release -p fn7-server --bin fn7-sim -- --replay runs/recordings/<match_id>.jsonl
```
Flags: `--seed --city --beats --players (seats, all bots) --clusters --mind-timeout-ms --stop-after --record <dir>
(default runs/recordings) --level events|full (default full) --no-record --claude --model --scripted --budget <usd>
--verbose --settings <json> --society-only` (`--settings '{"entropy_scale": 4, "arena": "city"}'` merges extra room
settings). `--replay <recording.jsonl>` re-executes a recorded match from its `on_match_start` (or `fork`) snapshot with
every LLM request answered by the engine's `ReplayLlm` and each round fed the actions and `RoundContext` recorded in
its `resolve_round` hook, and checks every snapshot (`on_round_start`, `resolve_round`, `commit_round`) and the summary
byte for byte; it exits non-zero on any difference.

## Settings

Room settings (`validate_settings` fills defaults; one Theatre round = one beat; there is no Choose phase):

| setting | default | range | meaning |
|---|---|---|---|
| `seed` | picked at match start | u64 | All randomness derives from it (recorded in `meta.seed` / `meta.settings`) |
| `arena` | `planet` | planet \| city | The whole Earth (regions = UN subregions, each with its 3 largest cities) or one real city (districts → sites) |
| `city` | seeded pick | name | Fuzzy match; cities ≥ 1M on land when unset. On the planet: where Time's circle closes for the Final Night |
| `players` | 4 | 1–8 | Demigods in the match; seats come first, the plugin's own AI plays the rest (synthetic seats ≥ 200) |
| `beats_total` (alias `rounds`) | 60 | 1–1000 | The Final Night falls on this beat (or earlier: heat death, nobody standing) |
| `flow_ms` (alias `beat_ms`) | 3000 | 500–120000 | Flow beat length; `compose_seconds` defaults to `flow_ms / 1000` |
| `slow_ms` | 30000 | 1000–600000 | Longest a slow-time window stays open (`ceil(slow_ms / flow_ms)` flow beats) |
| `start_bolts` | 3 | 0–99 | Lightning bolts per demigod at the start |
| `entropy_scale` | 1.0 | 0–10 | Multiplier on every Entropy rise (playtest dial) |
| `clusters` | 200 | 8–600 | Population clusters created at setup |
| `max_subagents_per_hand` | 3 | 0–10 | Live subagents + possessions per hand |
| `mind_timeout_ms` | 6000 | 50–120000 | Per-call timeout for every mind; a late call takes its rules fallback |
| `llm` | true | bool | false = never call the LLM (every mind falls back, no requests) |
| `debt_scale` | 0.04 | 0–10 | Consequence's bill: `debt_scale × Σ debt_for(magnitude, words this beat)` per demigod per beat |
| `debt_due_beats` | 50 | 1–100 | Beats until a bill falls due (Consequence's and Time's) |
| `instant_order_debt` | 0.5 | 0–10 | Time's flat debt for orders sent without a slow window (bots, the built-in AI, Compose submits): no repeat or seconds factor, doubled when the bolt can't be paid. Real windows keep `0.5 × (1 + secs/10) × (1 + 0.5 × recent)` |
| `arena_close_beats` | 160 | 1–1000 | Time's circle reaches its end radius (1,500 km) on this beat and its damage ramps on the same schedule; independent of `beats_total` (still closing when the Final Night falls if later) |
| `accident_scale` | 0.82 | 0–10 | Multiplier on the rules fallback's accident chance (1 in 20 per non-attack act at 1) |
| `army_strike_scale` | 0.82 | 0–10 | Multiplier on an army's strike damage on a demigod |
| `army_first_host_scale` | 0.82 | 0–10 | Multiplier on the extra chance (0.2) that a crowd near an unarmed demigod raises their first host |
| `memory_per_owner` | 40 | 4–1000 | Live memories per owner before old ones fold into a summary |
| `llm_folds_per_beat` | 2 | 0–32 | Folds written by the fold mind per beat (the rest by rules) |
| `max_intents_per_hand` | 6 | 1–12 | Acts one hand turns its speech into per beat |
| `max_intents_per_window` | 12 | 1–24 | Acts per hand from orders delivered from a slow window open at least `flow_ms` |
| `max_memorize_per_beat` | 6 | 0–64 | People memorized from player mentions per beat |
| `max_witnesses` | 4 | 1–64 | Witnesses recorded per act (targets and actor first) |
| `society_only` | false | bool | World-only simulation: the built-in AI demigods give no orders and cast nothing |
| `general_calls_per_beat` | 2 | 0–16 | General-mind LLM calls per beat at decision points (orders a hand gives always ask) |
| `compose_seconds`, `choose_seconds`, `resolve_timeout_s` | `flow_ms/1000`, core, 60 | core | The core's timers, copied through (`util::copy_timer_settings`) |
| `allow_midmatch_join` | false | bool | The core's room flag, copied through (`util::copy_timer_settings`): humans may join a running match; `on_seat_takeover` binds a taken-over bot seat to its demigod or a new seat to the healthiest live built-in AI demigod (none left: spectator). `docs/CLIENT_API.md` "Joining mid-match" |

**Actions** (two separate paths; `docs/CLIENT_API.md` has the full contract). *Text entry*, the God of Time
interaction, on the Theatre's action windows (engine v2.2): `action.begin` opens a slow-time window (`on_action_begin`;
the world keeps running in flow beats; the core holds others' orders, hidden), `action.submit { window_id, fields:
{left, right, zoom} }` sends orders into it, the initiator's submit (Enter) closes it and the core cuts off later
submits privately, and Time charges a bolt plus a debt that grows with the real seconds the window stayed open (settled
at resolve). Without `left`/`right` fields, `text` is read as lines prefixed `l:` / `r:`. Empty hands make no LLM
call; silent hands carry out their standing orders by rules. *Casts*, UX god powers, are instant actions (`instant: true`,
any phase; `on_instant` pushes the public streak at once): `fields: {cast | casts | bolt | bolts}` (`{power, target: ref |
[lon, lat] | name, text?, magnitude?}`), paid in bolts at the next resolve in `seq` order (every other demigod gains 2 per
bolt; Time's toll for orders gifts nothing), Entropy's yes, and a ramifications call per region.

**Engine boundary.** The plugin uses only the Theatre's public `GamePlugin` API (`theatre_plugin_api`): settings
validation (including `compose_seconds` via `util::copy_timer_settings`), the round hooks, `validate_action`,
`default_action`, `bot_action`, `resolve_round` with the actions the core delivers, `commit_round`, the views and
`summarize`, `on_seat_takeover` (mid-match join, `fn7-plugin/src/table.rs`), the real-time hooks (`on_action_begin` opens Time's slow windows with the core's exact `next_window_id`,
`on_instant` pushes cast streaks, `validate_action` sees the `SubmitKind` and refuses orders in instant actions;
`Committed.revived` / `RoundStart.revived` return a revived seat to play), and `queries()` / `query()` (the `game.*` queries:
globe, region, node, person, army, events, me, powers, gods, standings, piles_final; docs/CLIENT_API.md "Game
queries"). Time's bill settles at resolve from the core's `RoundContext.windows_closed` (opener, open/close times, reason)
and the actions' metadata (`windowed`, `window_id`, `instant`, `received_at_ms`, `seq`). `PluginInfo` carries
`version` (the crate version) and `rules_version` (`RULES_VERSION`), recorded in every match's ruleset.

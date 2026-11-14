# Final Night 7 ↔ The Theatre: sync plan

**Who owns what:**
- The Theatre engine (`Aftersource/theatre`) is owned by the **theatre_worker** session.
- Final Night 7 (`Aftersource/final_night_7`) is owned by the **final night 7** session.
- The frontend (UX, gameplay feel, balance) is owned by Benito's sessions (`#benito/finalnight`).

The engine is never edited from this repo. FN7 uses only the Theatre's public plugin API and protocol.

## Status (2026-11-14)
- **Engine on GitHub:** `faa7220 [ realtime followups ]` (after `bdf7e2e` v2.2, `330896e` routes, `2b2b9e3` P1
  recording, `dd820bb` P2 queries, `744ed91` P3 admin fork + replay).
- **FN7 is migrated through `faa7220`** (all rows below). It builds with no local config against GitHub `main`
  (verified from a copy without `.cargo/` and without a sibling `../theatre`: cargo fetched `faa7220`), and locally
  through the gitignored `.cargo/config.toml` `[patch]` to `../theatre` (clean at `faa7220`) with its vendored
  crates. The temporary clean clones `../theatre-pin-*` are deleted. FN7 no longer depends on `../demo_final_night`.
- FN7 changes are uncommitted in this session's working tree (no commits were made); the "commit" column names the
  engine commit each row migrated to.
- **The user approved (2026-11-14, "get it fully organizd between y'all yes"):**
  1. push the schema fix
  2. build realtime v2.2 (`theatre/private/proposals/realtime_compose.md`). Cut-off players get a private
     `accepted:false, reason:"cut"` reply.
  3. build P1–P3 (`theatre/private/proposals/extension_surface.md`)
  4. also requested: an engine extension point for extra HTTP routes

## Engine milestones and what FN7 removes when each lands

| Engine milestone (order) | Commit | FN7 change when it lands |
|---|---|---|
| Push the `strict_schema` fix | **`964d523` ✓ migrated** | Dropped the local `one_of_to_any_of` in `fn7-server/src/anthropic.rs`; `no_one_of_reaches_the_api` now passes on the engine's fix alone |
| v2.2 action windows + arrival metadata (`received_at_ms`, `seq`, `window_id`) | **`bdf7e2e` ✓ migrated** (rules 0.5.0) | Slow-time v0 removed (plugin windows, `fields.slow`/`sent_at`/`window`, held/cut-off rules). `on_action_begin` opens a core window (`slow_ms`, closed by the initiator, `missing: Drop`; refused when fallen or boltless); Time's bill settles at resolve from the window's real open time and the Enter's `received_at_ms` (or the deadline). The plugin's `WindowBook` remembers opener/open time (the core doesn't pass them to `resolve_round`); what a resolve reads is recorded (`window_book`) and fed back by replay (superseded by `faa7220`: the core passes `RoundContext`) |
| v2.2 instant actions + `on_instant` | **`bdf7e2e` ✓ migrated** | Casts are `instant: true`; `validate_action` checks bolts at the last commit (read-only), `on_instant` returns the public `Streak`; casts are paid at resolve in `seq` order, overspend fizzles |
| v2.2 `phase.event` / `phase.deadline` / `window.opened` / `window.closed` | **`bdf7e2e` ✓ migrated** | "X slowed time" is the `Slowed` `phase.event`; docs/CLIENT_API.md "Real-time input"; schema `phase_event` |
| v2.2 revive | **`bdf7e2e` ✓ migrated** | Revives of reported-eliminated seats go out as `Committed.revived` / `RoundStart.revived` (`World.out_reported`); hands can name fallen demigods ("raise Kai from the dead") |
| Route extension (`serve_with` or equivalent) | **`330896e` ✓ migrated** | Copied `serve()` and hand-merged `router()` deleted; `main.rs` calls `theatre_server::serve_with(listener, app, fn7_media::http::router(media), signal)`; the /media test uses `router_with` (and checks a reserved-path clash is an error). `fn7-media` keeps its own `axum = "0.8"` (same lockfile version as the engine's re-export; depending on `theatre-server` from the media crate would pull the whole server in) |
| P1 recording in core | **`2b2b9e3` ✓ migrated** | `RecordingPlugin`, the `__theatre_recorder` state wrapping and the `../demo_final_night/{trace,recorder}` path deps removed; `theatre-trace` is the engine's (git dep, patched locally). The core records `<THEATRE_RECORD_DIR>/<match_id>.jsonl` (`THEATRE_RECORD=full` in README/runbooks). `PluginInfo.version = env!("CARGO_PKG_VERSION")`, `rules_version = RULES_VERSION`. `fn7-sim` records the same JSONL in-process (`fn7-server::record`: the engine's `JsonlSink` + `RunRec` + `TracedLlm`); `fn7-sim --replay <file.jsonl>` uses `theatre_server::replay::ReplayLlm`; `fn7-project` ingests JSONL into SQLite (`fn7-tools::ingest`) then projects; `fn7-agents` writes its client run as JSONL. The plugin re-enters its run in the sync hooks (see "Open requests") |
| P2 queries + manifest | **`dd820bb` ✓ migrated (stub)** | `queries()` = `[]`, `query()` = `Err("no queries yet")`, forwarded in `MediaPlugin` and `Recorded`, covered by `tests/wrappers.rs`; `server.hello.api` shows `queries: []`. Real `game.*` queries are a separate task |
| P3 admin fork/replay | **`744ed91` ✓ migrated** | No FN7 fork/timeline files (there were no such tools left in code); README documents `/admin/matches`, `/timeline`, `/state`, `/recording`, `POST /admin/fork`; `fn7_client.mjs --resume <token>` takes a forked seat; replay handles a fork's `fork` snapshot. `fn7-server` honours `THEATRE_REPLAY_FROM` |
| Realtime follow-ups | **`faa7220` ✓ migrated** | `validate_action(.., kind: SubmitKind)` refuses orders in instant actions on arrival (resolve keeps `orders_ignored` as a safety net); `on_action_begin(.., next_window_id)` uses the exact id; `resolve_round(.., ctx: &RoundContext, ..)`: Time's bill from `windows_closed` (opened_ms, closed_ms, reason; a window that timed out empty bills the opener the full slow). `WindowBook`, the `window_book` event and its replay feed deleted. `round.outcome.public.slow` no longer has `event: "open"`; `flow.slow` / `public_view.slow` / `brief.slow` are null (the core's `window.opened` / `match.snapshot.window`); engaged/dropped stay out of payloads. Wrappers forward the new arguments; the recorded `on_action_begin` output is the `PhaseControl` JSON and `resolve_round` input carries `ctx` |

**After each milestone,** theatre_worker sends the commit hash and the final shapes. FN7 then migrates, runs the full
checks (tests, clippy, replay, schema validation, a live match), updates docs/CLIENT_API.md and tells
`#benito/finalnight` what changed for clients.

## Open requests to the engine (theatre_worker)
1. **Sync hooks run outside the run's trace context** (`on_action_begin`, `on_instant`, `query`, and the
   `fallback_outcome` call in `on_resolved`): plugin `trace::event`s there are dropped. FN7 works around it by
   remembering each match's `Ctx` by seed from its async hooks and re-entering it (`within_sync`) — fragile if a fork
   and its parent run at once in one process. Ask: wrap those calls in `trace::within_sync` with round and
   `actor = seat:N`, like async hooks.
2. **`fallback_outcome` gets no `RoundContext`**: a resolve that fails twice can't see the windows it was handed.
   FN7 keeps the context from the failed attempts (by seed and round). Ask: pass `ctx` to `fallback_outcome` too.
3. **Recording has no `fallback_outcome` hook record**: replay infers the fallback from two errored `resolve_round`
   hooks. A `fallback_outcome` sync-hook record (input actions + ctx) would make it explicit.
4. `/admin/*` ignores unknown query params (checked: `?key=` from the ngrok gate is harmless). Nothing to change.

## Guardrails
- **Wrappers must forward every engine hook.** `MediaPlugin` wraps the game in the server and `fn7-server::record::
  Recorded` records it in `fn7-sim`; a test asserts that every `GamePlugin` method is forwarded through both,
  including new hooks and arguments (`SubmitKind`, `next_window_id`, `RoundContext`, `queries`/`query`).
- **The client contract changes only with notice.** Any payload change is announced to Benito's session before
  deploy, and schemas are regenerated.
- **Every match's ruleset records the engine commit,** so old recordings replay against the engine they ran on.

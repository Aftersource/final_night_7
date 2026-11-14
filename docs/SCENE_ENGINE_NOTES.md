# Globe-table scene engine: design notes (pre-build)

The spec is the designer's wording in BUILDLOG.md, for the globe table. The frontend (globe, table, circle of action, fog,
rendering) is Benito's. This file covers the server side: scene memory, the hierarchical cell index and queries.

## Engine facts that constrain it (from theatre_worker, engine at `faa7220`)
1. **`query()` runs synchronously on the room actor.** A slow query (big cell scans, history scrubbing) stalls that
   room's timers, submits and broadcasts. Queries must be cheap lookups: precompute indexes into state at commit, or
   keep a scene store keyed for O(1)/O(log n) access.
2. **Frames.** Inbound frames are capped at 64 KiB, so query args stay small. Outbound replies have no size cap but are
   a single WebSocket frame each, and a client 512 frames behind is dropped (it can resume). Multi-MB replies work but
   are costly for mobile. **Paginate by args** (cell range, time range, cursor).
3. **HTTP** `GET /api/rooms/{code}/query/{name}` has no body limit and serves **public** queries to spectators and tools
   without a socket. It suits a three.js page.
4. **No push/subscription form of queries.** Live updates go through outcomes / `phase.event`. Streaming,
   subscriptions, chunked responses, or off-actor (async, read-only snapshot) queries for heavy scans would be an
   engine proposal; theatre_worker will size one when asked.

## Shape this implies (to confirm before building)
- **Cell ids:** one shared scheme with the frontend (Benito's cube-sphere hex keys, prefix = parent). Level 0 is a cube
  face; levels go down to about 1 cm.
- **Scene store:** `scene(cell, level, beat)`, generated once, deterministically, from the cell id plus the parent
  scene/hierarchy (children inherit, so connected scenes agree). Stored sparsely: only cells ever rendered or touched
  by events. Versioned by beat for scrubbing (copy-on-change, not per beat).
- **Queries:**
  - `game.scene {cell, level, at_beat?}`: one scene, paginated contents.
  - `game.cells {parent, level, cursor}`: overlay summaries for the globe.
  - Both public where safe, so they're reachable over HTTP.
  - Index lookups only, never scans.
- **Population:** rolled-up totals per cell (trillions never enumerated). People materialize when a scene is rendered
  at person level or emerge from the society simulation, and are then remembered.
- **Recording:** every scene served (cell, level, beat, viewer) is recorded, so replays show what players saw.
- **Open decision (designer):** real Earth, a super-Earth (twice the radius, 14 trillion people), or a hybrid seeded
  from the real map.

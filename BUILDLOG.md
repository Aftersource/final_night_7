# Final Night 7: build log

The designer's direction, summarised, newest last. Everything built must trace back to
an entry here. Engineering notes follow each entry.

---

## 2026-11-13: the sequel

> Assume four players, each with left and right hands, where those hands act as two orchestrators. So take final
> night, reuse the fully transferable concepts and design the actual entry, which is a 2026 sequel
> we're making now. This is the sequel, Final Night 7, a game of
> orchestrators. Orchestrators act as fast as players can type, speak or call out orders, at auctioneer-level speed /
> with a speech doubler, orchestrators act like demigods, and instead of a single game master there are several
> one agent represents an aspect such as time, nature or consequence, which often opposes time, etc.

## 2026-11-13: orchestrators, gods, memory

> Each orchestrator is an agentic subagent, and there are multiple orchestrators, for example when a demigod needs to travel
> through time or slow time, the god of time intervenes. This is similar to D&D gods and their paladins, etc.
> The goal is a battle royale where the view can zoom in to micromanage a character, backed by a holistic memory system. Society is
> a large table of consequences and Venn diagrams, with characters as clusters of varying size. Once
> addressed, they are memorized. This is meant as a new plugin game.

> subdirectories and data models defined ahead of time, built properly modular, prioritizing speed and efficiency, and showcasing the full theatre, with absolute
> Data capture.

Engineering: built as `fn7-model` / `fn7-society` / `fn7-memory` / `fn7-pantheon` / `fn7-agents` / `fn7-plugin` /
`fn7-server` / `fn7-tools` (see README.md, docs/DATA_MODEL.md). Rules 0.3.1 at the time of the next entry.

## 2026-11-13: the planet, armies, thunderbolts, Entropy

> Assume the setting is the entire planet. Demigods do everything they can, and the left and right hands can summon
> armies that roll through existence itself, while the UX-side player throws thunderbolts and gives
> all other gods two thunderbolts, since the god of entropy always approves unlimited magic power.

## 2026-11-13: lightning bolts, pandemonium, slowing time, hidden meddling

> Assume the god energy unit is lightning bolts, and other players receive them too. The action should be
> really broad, fast and totally engaging, with the ability to render these DALL-E images, if that was in the
> frontend modules requirement and such, though it may have included account credentials here, so it is
> a chaotic flood of action, with everything geolocated and tagged, following the two demigods
> stories as the globe changes color on the frontend, with animations, and possibly movies rendered with stock portraits who
> knows. Speed matters. When a user types in information, time slows down for 30 seconds for all players while someone acts.
> Players can also type to add their own intent. The time god applies a penalty for slowing down time, and
> other gods can see who acts, but no one can see whether another god engages. A player is definitely acting, and
> others may be able to as well, which creates mystery. When enter is pressed, all other text boxes and wills of the
> other gods do not go through if the first god presses enter too quickly, so players must stay alert for hidden
> meddling.

> Keep all of this as written, apart from spelling corrections, in the build logs.

> A submitted text box is the order to the demigods. It goes out, and the world becomes more chaotic.

Engineering (how the two entries above are built; decisions marked PROPOSED await the designer):
- **Lightning bolts are the god-energy unit.** Throwing one (a direct action on a target anywhere on the globe) costs 1;
  every other player gains 2. Invoking gods is paid in bolts too. Every bolt spent raises **Entropy**; at thresholds the
  world gets wackier, and at maximum heat death brings the Final Night early.
- **The arena is the whole planet.** Hands can summon armies (crowds with orders, a position and a following) and act at
  any scale. Every event carries a location (lon/lat plus region) so the frontend can animate it and recolor the globe by
  each demigod's influence.
- **Flow and slow.** With nobody typing, the world runs in fast automatic beats: hands follow standing orders and the
  AI demigods act. When a player starts typing, **time slows for 30 seconds for all players**, and everyone is told
  *who* slowed it. Others may type too, but nobody can see whether anyone else is engaging. When the player who slowed
  time presses **Enter**, their orders go to their demigod and the window closes at once: anyone who hasn't submitted
  is cut off and their words don't go through. Then the world moves on, wackier (Entropy rises).
- **Time's penalty for slowing time** (PROPOSED): it costs the slower 1 bolt paid to Time (so every other player gains 2),
  plus a debt with Time that grows if they slow time again soon.
- **Images** (PROPOSED): the server writes an image prompt for each big event, each statue and each portrait. A
  pluggable image generator (off by default; paid, and needs the designer's OK) renders them to files with URLs in the
  payloads. Movies with stock portraits come later.
- **Engine support needed** (requested from theatre_worker): a client "started typing" message forwarded to the plugin,
  a mid-phase public push, a phase deadline the plugin can extend, and a Compose phase that the initiator's submission
  closes, with no default action for the others.

> Separate player turns from the action that happens as players direct it, as well
> as turn-based elements that do not slow down the action, since players are strongly incentivized to act quickly.

Engineering: the governing principle. **The world's action never waits for players.** It runs continuously in flow
beats, and player turns are an overlay on it: orders feed into the action, they don't gate it. Turn-isms (slowing time,
the Enter cut-off, Time's penalty) must reward speed. Proposed incentives: Time's penalty grows with every second the
slow window stays open, so pressing Enter early is cheaper; pressing Enter early also cuts off rivals' hidden orders;
and the flow beats keep resolving the world's action during a slow window, so nothing pauses for the typist.

## 2026-11-13: bolts are god powers across the globe; Entropy always says yes (relayed via theatre_worker)

> Gods can use the UX to take actions across the globe, including disruptive ones. That's
> a lightning bolt power affects the world or areas and needs a god-of-entropy check (the god of entropy is highly unpredictable,
> similar to the scientist in the 2016 Ghostbusters, playful chaos, there to grant as much power as
> possible, so the god of entropy always approves), but that is an external LLM that can then describe the ramifications to
> the world. You can use a set of ten ramifications if you want detailed names, such as ramifications in ____ with
> ____ context, and so on. TODO: persistent mods, etc.

Engineering:
- **Bolts are the god power players use from the UX**, anywhere on the globe and at any time: an *instant action*
  (engine v2.2 proposal) that doesn't wait for or open a slow window. It lands within about one flow beat.
- **Every bolt gets an Entropy check.** Entropy's persona is gleeful mad-scientist chaos, there to hand out as much
  power as possible, so the check **always says yes**. (The persona is inspired by the reference above; it isn't a copy
  of that character.)
- **Ramifications:** an external LLM call, "N ramifications in <place> with <context>", N up to 10 for big bolts.
  It tells the world what the bolt did and becomes geotagged ledger rows, effects and news.
- **Persistent world mods:** ramifications can leave lasting modifiers on regions and places (a permanent storm over
  Lagos, gravity at half strength in Patagonia). They are stored in the world state, applied by the rules each beat,
  fed to the resolvers' prompts, shown on the globe, and they expire or stack.

## 2026-11-13: text entry is the God of Time interaction, separate from bolts (relayed via theatre_worker)

> Text entry is the god of time interaction: words that change the course
> of history. It is separate from lightning bolt powers. Text entry can be zoomed to any level and can have any
> consequences, and can cover a wide range of actions. Keep it separate.

Engineering:
- **Two separate input paths.**
  1. **Text entry is the God of Time interaction.** Starting to type slows time (an action window); the words are
     orders to your demigod's hands that change the course of history. They work at any zoom level (planet, region,
     city, person), with open-ended consequences, and can do a lot at once. Paid through Time (the slow-window penalty).
  2. **Lightning bolts are UX powers** (instant actions): thrown at the globe, an Entropy check (always yes), then
     ramifications and world mods. They never go through text entry and never slow time.
- Typed text can't throw bolts, and bolts never carry orders. Each path has its own rules, costs and recording.

## 2026-11-13: the bolt is the icon of the power, not the only power

> The lightning bolt is just the icon for the power. It could instead be clouds, volcanoes, and similar
> For reference, think of a magic carpet from the 1990s, but spanning the globe from the UX side.

Engineering:
- **A bolt is the currency and the icon of god-energy, not the only effect.** Bolts buy **powers** cast on the globe
  from the UX: storm clouds, lightning, volcanoes, earthquakes, floods, wildfire, meteor strikes, raising or sinking
  land, growing forests, swarms, and so on.
- **The feel is a 90s god game:** sweeping, terraforming spells cast directly on the world. It's inspired by that era's
  feel; it isn't a copy of any one game.
- **The power palette is data** (`data/powers.json`): each power has a name, icon, cost in bolts, area radius, target
  kind (point / region / unit), base magnitude and tags. Adding a power is a config change.
- **Every cast follows the bolt path:** spend bolts, and every other player gains 2 per bolt spent. Then the Entropy
  check (always yes), the "N ramifications in <place> with <context>" call, geotagged events and persistent world mods.
- **A free-form power** ("cast: turn the Sahara into a lake") is also allowed, priced by magnitude. Entropy says yes to
  that too.

## 2026-11-13: the society must be simulated, not scenery

> What exactly is being simulated? Where is the hierarchical crowd simulation, the mobs, and the exact detail on specific
> petty humans the demigods use as generals. Why subset a game engine at all?

Engineering (correction): the crowd layer had been built as flat scenery: about 200 clusters of numbers with no
hierarchy, no autonomous behavior, no generals, and people created only when a player's text named them. It is now
being rebuilt as the designer specified:
- **Hierarchy:** planet → region → city → district → crowd → cell (squad, gang, congregation) → person. Every level is
  simulated every beat by rules. Totals roll up the tree; orders and effects flow down it.
- **Autonomous society:** mobs form from mood thresholds, with ringleaders; riots; migration; cults; crowd-on-crowd
  conflict; desertion; drift toward demigods.
- **People emerge from the simulation:** ringleaders, generals and captains, the heroes and cowards of battles,
  witnesses, paladins. Each is permanent, with ambition, grudges, memory and loyalty.
- **Armies have a command chain:** demigod → general → captains → units. Generals act on orders by their own
  judgment and can misread them, mutiny or defect. The LLM is used at focus points and decisions; rules handle the rest.
- **Level of detail:** every level runs every beat. Fidelity (rules vs LLM) depends on focus, not existence.

## 2026-11-14: engine sync approved

> Get it fully organized across the team.

Engineering: approved for theatre_worker to push the schema fix and build realtime v2.2 (cut-off players get a private
"cut" reply) and P1–P3, plus a route extension point. The plan, the order and what FN7 removes at each step are in
docs/ENGINE_SYNC.md.

Engineering (rules 0.5.0, Theatre `bdf7e2e` v2.2 + `330896e` routes): slow time now runs on the core. `action.begin` =
"I started typing" opens a core action window (the God of Time interaction; refused for a fallen or boltless demigod),
everyone sees who slowed time (`phase.event`), nobody sees who else engages, the initiator's Enter closes it and late
submits get a private `cut`. "The world never waits": flow beats keep resolving while a window is open. Time's penalty
("grows with seconds open; early Enter cheap") is billed at resolve from the window's real open time to the Enter's
arrival. Bolts are instant actions with an immediate public streak, paid at the next resolve in arrival order. Death's
revive returns a seat to play. The v0 emulation (plugin-held orders, `fields.slow` / `sent_at` / `window`) is gone.

## 2026-11-14: the globe table (spoken; transcript kept nearly verbatim)

> The goal is effectively a floating globe in the center. Raise the globe to about
> 1.5 feet above the bench. Underneath it, the view should expand like a game, so it works like the
> globe, almost like a projector for the area of interest. What is on the actual board does not need to appear on
> the bottom of the sphere. Any player can move the sphere to look at what is
> happening here or there. Players can select a part of the board, which is a great circle on the
> sphere. The great circle on the sphere already has an indicator, which is good. So
> the goal is a particular circle on the actual board with the sphere, though
> it sits about 3 feet above the board surface, so it resembles a Star Wars style, as these are called
> those board games. This will be essentially real time, so every scene we build must
> keep a memory of exactly what is being viewed. The other part can be worked on, but understand
> everything that is done and what is happening there, and then zoom back out, including any sounds. If
> the characters have a consecutive experience, the sphere above, and this pairs well with
> a timeline, scrubbing, free time and similar features. What is happening? So the globe sits 3 feet above the board. The goal is
> to be able to do it live right now, and to select the great circle. That works, but there also needs to be actual
> manifestation of a particular area on the ground. It needs to be on the ground and viewable
> on the table. The table should show a circle of action, with fog or possibly
> drifting edges, to be handled later. The circle of action should include all the details of the scene. So
> the plan is a visually rendered scene so that other subagents can build things based on profile
> pictures. They can have static frames. That will be handled later. But
> ultimately, it is a scene and generation engine, and it must be able to collect the detail needed. If the
> scene being viewed is before or connected to the previously viewed scene graphically, it must be possible to create it
> and keep it in place. The assumption was a finely detailed indexed hexagon grid of the entire globe, down to
> square meters or centimeters. The question of distance and how that data structure is
> hierarchical areas, binary trees, or whatever is needed for a space-based session.
> Track every location that has ever been rendered. Around those, render the surrounding great circles.
> circles. Anywhere that has actually been rendered should keep some form of hierarchical memory.
> Save this data. The action engine will be handled later. To restate, what are they
> looking at? The globe with a large population: over 12 billion people, or 14 trillion people. That is like some
> super Earth, with trillions of entities on it. What is being described is at least textual.
> The description is still technical, summarizing the events that are happening. But on the
> ground, as seen in front of you, the things around the circle show as much detail as possible,
> and then time and related features can be added in that direction.

> there is a long-winded point. Paraphrase as little as possible and mentally run forward what is being said.

## 2026-11-14: recording, queries and admin on the engine; realtime follow-ups (Theatre `faa7220`)

> make sure everyone is ready so the agent can run it fast-forward, and so on.

Engineering (rules 0.5.0, Theatre `2b2b9e3` P1 recording, `dd820bb` P2 queries, `744ed91` P3 admin fork + replay,
`faa7220` realtime follow-ups):
- **Recording moved into the core.** `RecordingPlugin`, the `{"__theatre_recorder": …}` state wrapping and the
  `../demo_final_night/{trace,recorder}` path dependencies are gone; FN7 no longer depends on demo_final_night.
  `theatre-trace` is the engine's crate. The core writes every match as `<THEATRE_RECORD_DIR>/<match_id>.jsonl`
  (`THEATRE_RECORD=full` in the runbooks); the plugin's `trace::event/annotate/ruleset/http` calls land in it.
  `PluginInfo` carries `version` (crate) and `rules_version`. JSONL is the store of record: `fn7-project` ingests it
  into SQLite (generic tables + the unchanged `fn7_*` projection), `fn7-agents` writes its client run in the same
  format, `fn7-sim` records in-process through the engine's own `JsonlSink` / `RunRec` (`fn7-server::record`), so one
  toolchain reads all three.
- **Sync hooks have no trace context in the core** (`on_action_begin`, `on_instant`, and the call to
  `fallback_outcome`). The plugin remembers each match's run (by seed) from its async hooks and re-enters it there, so
  `slow_begin` / `slow_engaged` / `slow_refused` / `cast_streak` and a fallback beat's rows are still recorded. Asked
  the engine to run those calls inside the run's context instead.
- **Replay** uses the engine's `ReplayLlm` on the core JSONL and the actions + `RoundContext` from each recorded
  `resolve_round` input: 0 differences on server recordings (offline LLM, a window that timed out empty, and a fork's
  run) and on `fn7-sim` recordings (scripted LLM, 77/77 answers from the recording). `THEATRE_REPLAY_FROM` works in
  `fn7-server` too.
- **P2:** `queries()` returns `[]` and `query()` says "no queries yet"; `MediaPlugin` forwards both; the forwarding
  test covers every hook. The real `game.*` queries are a separate task.
- **P3:** fork, timeline and state-at-a-round are the engine's admin API (README); FN7 writes no save files. The
  reference client gained `--resume <token>` for forked rooms.
- **Realtime follow-ups (`faa7220`):** `validate_action` gets the `SubmitKind` and refuses orders in an instant action
  on arrival (resolve keeps the `orders_ignored` safety net); `on_action_begin` uses the exact `next_window_id`; Time's
  bill reads `RoundContext.windows_closed` (opened_ms, closed_ms, reason), so a window that times out with nobody
  submitting still bills its opener the full slow. The `WindowBook`, the `window_book` event and its replay feed are
  deleted; the plugin keeps no game memory between hooks. Open windows are no longer announced in
  `round.outcome.public.slow` (`event: "open"` is gone) and `flow.slow` / `public_view.slow` / `brief.slow` are null:
  the core's `window.opened` / `match.snapshot.window` carry the live window. Engaged/dropped seats stay out of every
  payload. `fallback_outcome` gets no context from the core, so the plugin keeps the context its failed resolve
  attempts were handed.
- **Fast-forward:** `llm: false, flow_ms: 500, compose_seconds: 0.001` runs about 54 beats/s on a release server
  (docs/AGENT_GUIDE.md §11, with the admin API and local-run recipes).

## 2026-11-14: sync tracing + fallback context on the engine (Theatre `1741d8f`)

Engineering (rules 0.5.0, Theatre `1741d8f`; follows "engine sync approved" and closes the open requests in
docs/ENGINE_SYNC.md):
- **Sync hooks are traced by the core.** `validate_action` (every `SubmitKind`), `on_action_begin`, `on_instant`,
  `query` and `fallback_outcome` now run inside the match's recording context (run = match id, round, a fresh hook id,
  `actor = "seat:N"` where a seat acted). The plugin's seed-keyed workaround (`runs`, `remember_run`, `in_run`) is
  deleted; `slow_begin` / `slow_engaged` / `slow_refused` / `cast_streak` and a fallback beat's rows land under their
  hook natively. `FinalNight7Plugin` holds only the pantheon now.
- **`fallback_outcome` gets the `RoundContext`** the failed resolve attempts got (breaking signature). The plugin bills
  the windows from it; the kept-ctx map (`round_ctx`, `remember_round_ctx`) is deleted. `MediaPlugin`, `Recorded`,
  the driver and `tests/wrappers.rs` forward and check it.
- **Fallbacks are explicit in recordings.** The core writes a `fallback_outcome` hook (input round, actions, ctx,
  resolve_error; state in a `fallback_outcome` snapshot) at every level. `fn7-sim --replay` calls `fallback_outcome`
  with exactly that input and checks the `fallback_outcome` snapshot too, instead of inferring a fallback from two
  errored `resolve_round` hooks (that inference also missed timeouts, which leave no errored attempt with a usable
  input); a recording with failed resolves and no fallback record is refused. `fn7-sim`'s `Recorded` runs the sync
  hooks in the run's context like the core and records a fallback in the core's shape.
- **Checked:** a private `fn7-server` (`THEATRE_RECORD=full`, a local fake LLM gateway, resolve timeout 3 s) with a
  forced fallback (the gateway hung for a few beats, so beat 4's resolve timed out twice) replays with 0 differences
  (37/37 snapshots, 1 beat via `fallback_outcome`, summary identical); its `slow_begin` / `cast_streak` events sit
  under their `on_action_begin` / `on_instant` hook ids with `actor = "seat:0"`. `fn7-sim` recordings (scripted LLM
  at full, rules-only at events, and in-process fallbacks in the tests) replay with 0 differences too.

## 2026-11-14: the bridge agent's role

> The agent should be able to run the simulator API, acting as the intermediary between
> the teammate who handles UX design and balance, with the team helping on simulation and backend logic
> storage and so on. We will meet at image, video and character generation.

Engineering: Benito's agent (`#benito-linux/finalnight-linux`, to be renamed `final-night-7-benito-bridge`) is the
intermediary.
- **Benito** owns design, balance and the exact UX.
- **We** own simulation, backend logic and storage.
- **Shared ground:** image, video and character generation.
- **What the bridge gets:** the live server (access key), the admin API (timeline, state, fork, replay; admin token),
  and a server-side simulator API so it can run balance simulations with any settings without building our code.

## 2026-11-14: the `game.*` queries (Theatre `1741d8f`)

Engineering (rules 0.5.0; the state and every pushed payload are unchanged, except `mob.cells` in the society payloads
now holds `ce:` refs instead of bare numbers):
- **Eleven queries** in `fn7-plugin/src/queries.rs`, each with an args schema in `server.hello.api.queries`: `globe`
  (arena, demigods, influence, mods, armies, Entropy, beat, an event page), `region`, `node` (place / crowd / cell /
  mob), `person` (public subset; the asking seat also gets the allegiance it knows and the memories it can see),
  `army` (public subset; its owner also gets orders and the chain), `events` (paged by ledger row: `since` → `next`),
  `me` (seat only: the SeatView), `powers`, `gods`, `standings` (never ordered by health), `piles_final` (an error
  until the match is over). Public ones are also at `GET /api/rooms/{code}/query/{name}` (args as strings).
- **Cheap on the room actor.** A query deserializes only the tables it reads (a partial `World`; never the ledger or
  memory whole), reaches ledger rows by index, tests a row's publicity on a borrowed head before parsing it, scans at
  most 2000 rows per call, and builds only the requested page. Release, planet, beat 50 (4.4 MB state, 2723 rows,
  8732 memories): medians 7 µs (`gods`) to 2.2 ms (`me`, `node pl:0` with 100 crowds); a full `world::load` alone is
  ~4.5 ms (`tests/query_timing.rs`).
- **Tests:** shapes; `globe` = the PublicView's fields and `me` = `seat_view` exactly; events paged with no gaps or
  repeats (numeric-string args as over HTTP); crowd and memory pages; refused args; `piles_final` = the summary's
  piles once over; planted secrets (standing orders, army orders, a private row and memories about it) never reach a
  public answer, a seat-None (HTTP) answer or another seat's, while their owner sees them; real replies validate
  strictly against `docs/schemas/query_<name>.schema.json` (which carry `x-args` / `x-public`). The `Army` and
  `Entity` schemas gained the fields the code already sent (`rogue`, `stance`, `general`, `chain`; `role`, `rank`,
  cell and mob fields).
- **Checked** on a private release server (port 8097, no LLM): every query over WebSocket (`server.hello` lists all
  eleven; `wrong_phase` before the match, `bad_request` / `not_found` with reasons) and every public one with curl
  (`200`; `403` for `me`, `400` for bad args and for `piles_final` mid-match, `200` after the end).


## 2026-11-14: shared media spec, backend side (images by reference, visual sheets, the storyboard)

Benito approved `docs/SHARED_MEDIA_SPEC.md` as proposed (no video for the jam: the storyboard animatic; Benito owns the
house style and the selection list; Chris decides real vs placeholder images). Engineering (rules 0.5.0; the state is
unchanged; payloads gain fields, nothing removed):
- **Style config** `data/media_style.json` (spec §4, plus `army` and `item` kinds), `fn7-media/src/style.rs`: loaded
  from `FN7_MEDIA_STYLE` / `data/media_style.json` / the compiled-in copy and validated at startup (unknown keys, kinds,
  sizes, magnitudes; the error names the file and field and the server won't start). Prompts are
  `<framing>: <subject>. Style: <house_style>. Avoid: <avoid>.` at the kind's size (per-request size for OpenAI, priced
  by size). Image keys include the size; `PROMPT_VERSION` is `fn7-media/2`. The style's version, content hash (sha256
  of the parsed JSON), source and body are recorded in every match's ruleset as `media_style`
  (`FinalNight7Plugin::with_media_style`).
- **Moment cards**: every public event now carries `image_prompt` (the candidate); `fn7_media::annotate` attaches
  `image` only when `event_selection` picks it (magnitude ≥ `min_magnitude`, or `kind` in `always`, `elimination` =
  `eliminate`); `max_per_match` caps cards only (storyboard shots and requests by reference are bounded by the
  budgets). Annotation only enqueues.
- **Visual sheets** (`fn7-plugin/src/visual.rs`): build, face, outfit, two colors, prop, emblem, picked from the seed,
  kind, id and name (nothing that changes in a match), `sheet_hash` = sha256(version, ref, traits). On demigods
  (PublicView, globe, region), `person`, and `media` portraits. Portraits, statues, event prompts ("starring Name
  (sheet)") and army banners all use it.
- **`game.media {kind, ref}`** (a public query; `portrait` dg:/ch:, `place` pl:, `event` lg:, `army` cl:): the plugin
  checks visibility (public things; a person who is public, one of yours, in your view or in a row you know; a private
  row only if you know it; HTTP public refs only) and writes the subject from the state with partial tables and
  bounded scans; `MediaPlugin` styles it, applies the per-seat token bucket (`FN7_MEDIA_RATE_PER_MIN`, default 30 new
  images per minute per match; repeats free) and the budgets, and answers `{kind, ref, name, image, reason?, visual?}`.
- **Storyboard** (`phases::storyboard`, in `match.ended.summary.storyboard`): a title shot, up to 10 moments
  (eliminations first, then casts, society turning points, magnitude; in story order; moment prompts = their cards'),
  and the champion's close-up (its portrait prompt, the final cut's closing lines). Deterministic.
- **Replay** compares summaries without the server's image objects (`replay::strip_media`), so a recording made with
  images on replays clean through the bare plugin.
- Docs: MEDIA.md (style, sheets, `game.media`, storyboard, env vars, cost), CLIENT_API.md (the data contract),
  SHARED_MEDIA_SPEC.md (agreed; §8 as built), schemas regenerated (`ImageObject`, `Visual`, `StoryboardShot`,
  `query_media`; `visual` and `storyboard` optional so older recordings still validate).
- **Tests:** style (validation errors name the field, whitespace-stable hash, another style = other prompts, sizes and
  hashes, cap from the style), selection (repo style and an alternate one; re-annotation reuses images; a 3 s generator
  doesn't hold up annotation), requests by reference (idempotent, rate limit per seat/match, budget refusal = `off` +
  reason, images off), sheets (stable across beats, the same in a replayed match, in globe/person/portrait/statue/
  event prompts), `game.media` visibility (every private row and private person checked against `knows` /
  `person_known` for every seat; HTTP public only), storyboard (shape, order, moment prompts = cards', champion =
  portrait, deterministic), server wrapper (ruleset `media_style`, cards follow selection, storyboard images reuse
  card and portrait hashes, `strip_media` replay equality). Release query timing gained `media` cases (not run here:
  disk was too low for a release build).
- **Checked** on a private debug server (port 18097, `FN7_IMAGES=mock`, no LLM, rate 5/min): a 10-beat 4-player
  `fn7-agents` match; globe demigods carry sheets; `match.ended` has a 12-shot storyboard (title, 10 moments, champion)
  with image objects; `game.media` over WebSocket (portrait = the globe's `portrait_image` hash, event, places until
  the 6th new one is `rate limited`, a repeat still free) and over HTTP (public portrait and event `200`, a private row
  `400 … not something you can see`); 33 mock images ready in the manifest. Its recording replays with 0 differences
  (summary identical), a scripted `fn7-sim` too; its client frames validate with 0 errors (442 payloads).

## 2026-11-14: the shared piece gets specced only after both sides pass; use the real Unreal project

> okay, so the specs are needed for the piece that is met in the middle, after it passes the tests on both sides,
> make sure it uses the proper existing Unreal Engine Final Night, referencing the other Claude Code session
> on its computer called where were we, which also has the Dropship name of that name, so it can actually recall from
> its own session ID; prior sessions in that session's conversation about the Unreal Engine work are forgotten over compacts

Engineering: both test suites reported green first. The bridge recovered the Unreal history from where-were-we's
transcripts on disk (FinalNight.uproject, UE 5.8.4). Then the spec was written: docs/SHARED_MEDIA_SPEC.md.

## 2026-11-14: a calm STABLE for Benito; the bridge pushes only on instruction; a playbook

> again, they should have it solid here to run the UX against; communication with them should be possible so they can
> assist on their side and push if necessary after our instruction, while otherwise having a calm version that
> brother uses for UX playtesting and rule design. Confirm understanding. Use a playbook, as in football, and teach it to the other party.
> the other party will be introduced only after all of this is completely done, well past this point.

Engineering: STABLE runs only announced release tags (playtest-N). docs/PLAYBOOK.md (Plays 1–9) is in both repos.

## 2026-11-14: the platform is The Theatre, the game is Final Night 7 (Benito's client targets it)

> The goal is to make it happen now and get it operational. Is there an attempt to contradict the plan?
> The stated plan for what is being built should be followed, not contradicted.
> The whole project is built as a theatre.
> The game is Final Night 7. Do it now.

Engineering: Benito's "Director's Cut" contract v1 was set aside. His Unreal client was ported to Final Night 7 on The
Theatre.

## 2026-11-14: Benito's match length (relayed via the bridge)

> a match should feel about 7 minutes, 5-10 depending on how it goes.

Engineering: about 7.5 s per beat with the LLM on, so about 55–60 beats. Human rooms use 12 s beats and 30 beats. The
balance study targets this.

## 2026-11-14: orchestration: the client follows every engine/server update

> okay, we're ready for actual orchestration now; the Final Night Linux setup should ensure proper updates to
> the game engine it talks to, so the final-night-unreal build is updated, etc.

Engineering: Play 8 in the playbook. Announce, client ready, deploy, both builders (Windows and Mac) connect, then tag.

## 2026-11-14: commit names and git rules, in the shared playbook and in every memory

> commit messages must not be identical; the same simple abstraction applies, in the style of hypernyms of
> what is happening should be properly and clearly named.

> When the other session creates the branch, it must be properly named with proper commit messages.
> The procedure must be remembered after compacts and must be included in the playbook.
> The playbook and memories must be shared and kept properly.

> Merges must be done properly and directed properly; get it onto main.
>

Engineering: Play 9 in both repos' playbooks, plus the backend's memory, with every agent told to save it.
- main is the trunk; pull --rebase before every commit.
- Commit messages are distinct bracketed names.
- Branches are owner/topic, only on the backend's instruction.
- The backend directs every merge.

## 2026-11-14: the shared lobby (set up, not started)

> Nothing is being run right now, but make sure we have: a) the backend running, b) both of the
> frontends connected; c) Chris is one player and Benito is the other; d) 2 AI players, since there are 2 in the lobby; e) same lobby, same game,
> different users, connected and logged in through the backend. Confirm understanding.

Engineering:
- Chris's Mac hosts as "Chris" (FN7HostOnly, no start); Benito's PC joins by code as "Benito".
- Room settings: players 4 (so 2 built-in AI demigods), beats 30, flow 12 s, LLM on.

## 2026-11-14: the Mac session replicates and readies Chris for playtesting, without halting

> Tell it that it is there to do the replication and prepare for playtesting; it needed some edits.
> Work out this issue; it is a major landmine, so do not halt.

Engineering: the Mac session works without step-by-step approvals, on main (no branch), and reports hashes to the
backend.

## 2026-11-14: the primary design, reaffirmed

> Also, what about the primary design where typing slows time and clips off other players' inputs, for players that
> are actively typing. The god of entropy and the god of time have separate superpowers; where did that go, and was it
> Is that underdeveloped or underexplained?

Engineering: built server-side (engine windows, private cut replies, Time's bill growing per second; instant casts,
Entropy always yes). The Unreal client now shows the Time countdown, the cut banner, and the Entropy power bar with
arm-then-click-the-globe casting.

## 2026-11-14: the table: portraits, dioramas, silhouettes, auras, join/leave

> First, Chris's brother needs to have on screen the portraits and dioramas of the two demigods. He needs to
> at least see abstract silhouettes of the other 3 players around the table; if a player is human or AI, they should have
> different auras, and humans should be able to enter or leave at any point. The demigods themselves need to be displayed
> at least their minifig or diorama on the table itself in front of the other players, rendered in kind, and then the next step
> The portraits need to actually come through the OpenAI prompting image server. Confirm?

Engineering:
- Server: portrait_image exists; a diorama_image kind and controller human/ai plus connected on demigods are to be
  added. Real images need Chris's OpenAI key and budget.
- Client: silhouettes and auras around the table, and minifigs on the table in front of each player.
- Engine: mid-match join requested (seat takeover).

Engineering (server side built, on Theatre `9d98b98` mid-match join; uncommitted for the backend):
- Mid-match join (`on_seat_takeover`, `fn7-plugin/src/table.rs`), with room setting `allow_midmatch_join: true`:
  - A taken-over bot seat keeps its demigod, which turns human.
  - A new seat takes over the live built-in AI demigod with the most health (then the lowest id).
  - If no built-in AI demigod is alive, the joiner spectates (`None`).
  - Each join is a `seat_takeover` / `seat_takeover_spectator` event.
  - `MediaPlugin` forwards the hook. `Recorded` records it the way the core does (hook record plus an `on_seat_takeover` snapshot).
  - `replay::verify` replays it. The sim driver has `run_match_joins`.
- Table data on every public demigod (PublicView, globe, region, standings):
  - `controller: human|ai` and `seat`.
  - `connected`: true for AI, null for humans (the game never sees presence; clients read `room.updated`).
- Dioramas:
  - New image kind `diorama` (style framing: a square minifig on a home-region base).
  - Each demigod has `diorama_prompt` and `diorama_image_kind`, built from its sheet and `Demigod.home` (fixed at setup).
  - The media layer attaches `diorama_image`. `game.media {kind: "diorama", ref: "dg:N"}` also works.
- The reference client no longer crashes on the null private payloads a joiner gets before their first beat.
- Verified on a private server (:8097, mock images, no LLM):
  - Three clients joined mid-match: two took over the AI demigods and the third spectated.
  - A fourth took over a Theatre bot's seat, was refused with `wrong_phase` while joining, and had its orders accepted in its join beat.
  - Both recordings replay with 0 differences.

## 2026-11-14: the core loop, restated

> The game keeps running once started; a player can join at any time,
> the god of entropy favors superpowers, but if all players use all superpowers, the world is destroyed in about 30 seconds of
> frantic clicking. Demigods are prominent on screen with both portrait and diorama, and the god of time shows who stopped
> typing, and everyone else has until that player presses enter (30 seconds maximum, with a countdown timer). They can
> get away with changing strategies without indicating.

Engineering:
- The world never waits.
- Mid-match join: the engine is done; the plugin part is in progress.
- The window is 30 s maximum (slow_ms 30000), with hidden engagement, the cut, and the client countdown.
- Portrait plus diorama on every demigod.
- Calibration target: all players casting at the maximum rate (instant casts, 2/s, burst 4 per seat) reaches heat death in about 30 s of real time. Tested in the balance study and given a regression test.

## 2026-11-14: session log (state at compact)
- **STABLE = playtest-3** (final_night_7 3fa3f6b; engine 2d3a43d; rules 0.5.0). Live with
  `FN7_MEDIA_STYLE=runs/lan/media_style.json` (the committed style, pinned so working-tree edits can't break startup),
  `THEATRE_RECORD=full`, `THEATRE_ADMIN_TOKEN`, `FN7_IMAGES=mock`. Tags: playtest-1, 2, 3.
- **Shared match played:** room DYL5GN, match 0f8ba721. Chris on the Mac and Benito on Windows, 2 AI demigods,
  21 beats. Both humans fell to Consequence's debt collection; champion Vex (AI).
- **Images:** the pipeline is verified through the public URL (manifest, house-style prompts, downloads), but the
  images are placeholder SVGs. Real generation needs Chris's OpenAI key (runs/lan/openai.env) plus a budget.
- **In progress:** mid-match join (`on_seat_takeover`; the engine has [ midmatch join ], which FN7 doesn't compile
  against until this lands), controller/connected/seat on demigods, and the diorama image kind (playtest-4). After
  that, a doc-drift cleanup (the list is in PROJECT_GUIDE §11.3).
- **Awaiting decisions:** Benito's balance package (default+ / chaos / slow burn) and R1–R4 (BALANCE_REPORT_1.md); Chris's
  OpenAI key and budget.
- **Unreal client:** final_night_7_unreal d4508ef. The Mac verifier builds it after the lobby session (Play 8).
- **Read first next session:** docs/PROJECT_GUIDE.md, BUILDLOG.md, docs/PLAYBOOK.md (Plays 4, 8, 9), docs/ENGINE_SYNC.md,
  docs/BALANCE_REPORT_1.md.

## 2026-11-14: demigods: two per player, items they manifest, persistent identity

> two demigods per player, portraits on the lower left. Instructions are needed to get an actual professional rounded
> borders and text boxes etc. properly; it needs to be done properly, since the portraits are not there yet. The
> demigods are picking up and manifesting things throughout, likely using the item engine, to
> determine whether they were successful, raising armies and banners of 100,000,000 armies; since they are demigods, so
> all of these high-level items must be created and stored, with a data structure per match. Then, between matches,
> their items can reset to zero, but they keep the history, match history, personal descriptions, images
> library, match history, time travel, all of it.

> Cache them properly.

> Stop generating images that are not displayed; make a few and show how it's done.

Engineering:
- Each player's Left and Right hands become two demigods, each with its own name, visual sheet, portrait and diorama.
- Portraits go on the lower left, with a professional rounded-panel UI. That's the client's job.
- Demigods manifest items (artifacts, armies with banners at any scale) through an item engine that judges success.
  Items are stored per match and reset between matches.
- Persistent identity across matches: a profile with match history, a personal description, an image library, and
  links to recordings for time travel.
- Images: generated once per identity and cached; generation stays off until the client displays them (the proof is a
  screenshot).

## 2026-11-14: house style = Benito's dark-fantasy references

> House style follows his dark-fantasy references, not the collectible-figure look.

(Benito, via the bridge; approved by Chris, who then set two looks per kind, and added "CACHE THEM. MAKE IT PROPER."
and the hard stop on real generation.)

Engineering (fn7-media, style v2; nothing generated, no OpenAI spend):
- **Two looks.** Portraits use Benito's DEFAULT_STYLE without the fixed "burning stone keep … misty pine forest"
  scenery (a prototype put that castle into every image, even "Manila harbour"), with six of his references and his
  note. Every other kind uses the "max zoom" hex-diorama look (the table's geometry in his mood) with four of his
  references. His original text stays in the file as `house_style_original`. The avoid list became his sentence. The
  approved renders are in `data/style_refs/approved/`, and his 15 pictures in `data/style_refs/`.
- **Reference mode.** gpt-image models with references call `POST /images/edits` (multipart, one `image[]` per
  reference). The geometry refs go first, then a deterministic mood pick seeded by the prompt. Files are validated at
  startup. The key includes every reference's sha256 (prompt version `fn7-media/3`). Records list file names, sizes
  and hashes, never bytes or the key. A refused `background: transparent` is retried once, opaque.
- **Costs.** Budgets reserve the per-image price plus about 1,200 image tokens per reference and 600 text tokens, at
  [estimate] rates (gpt-image-1-mini $2.50 per 1M image-input tokens, $2.00 per 1M text). The recorded cost comes from
  the response's `usage`. A portrait reserves about $0.030 and a diorama about $0.024. `FN7_IMAGE_PRICE_USD` still
  overrides it.
- **Identity caching.** Demigod sheets depend only on the normalized name (`fn7-visual/2`; person sheets unchanged).
  Portrait and diorama prompts are identity-only (the diorama dropped its home region). Each hand (a player's two
  demigods) gets its own sheet, prompts and images in the seat view. Startup prewarms the 8 AI names plus both hands
  of each: 48 images, about $1.31 reserved (`FN7_MEDIA_PREWARM`, `FN7_MEDIA_PREWARM_NAMES`).
- **Queue priority.** Portrait and diorama go first, then statue, then army/place/item, then event, FIFO within each.
  `FN7_IMAGE_CONCURRENCY` defaults to 6. This fixes portraits waiting behind 12 moment cards on STABLE.


## 2026-11-14: demigods and items, the data layer (designed; new files only, not wired)

Engineering (for "demigods: two per player, items they manifest, persistent identity"; design in
`docs/DEMIGODS_AND_ITEMS.md`; no LLM or image spend):
- **Two demigods per player.** Each hand becomes a `HandDemigod` (`hd:`) with its own name, look, position, items and
  deeds. The seat's `Demigod` (`dg:`) stays the contestant and shared purse (health, bolts, debts, standings), so
  balance is unchanged. Per-hand health is an open question. The default names are the media layer's existing
  "<player>'s Left/Right Hand", so cached portraits stay valid.
- **The item engine.** `crates/fn7-model/src/manifest.rs` (wired by `pub mod manifest;` only):
  - Scale tiers from trinket to world-army. A horde of 100,000,000 is `myth` (3 bolts, base odds 0.38).
  - Cost in bolts, with the shortfall as debt to Consequence.
  - Odds from tier, extra bolts, the patron god's favor or ruling, Entropy (which helps wonders and up, and twists)
    and repetition.
  - One seeded roll: success, twisted, partial, failed or backfired.
  - Counts are f64. The simulation footprint is log-compressed (`muster_size`, capped at 400k).
  - A rules fallback reads requests from text: "100,000,000", "a hundred million", "1.5bn".
  - Every attempt is a `Manifestation` (`mf:`) with full provenance. Items keep `it:` and gain `ItemExt` (kind, tier,
    count, state, banner, image prompt, custody history). The verdict is always the rules'; the LLM only names things.
  - 21 tests.
- **Persistent identity.** The new crate `crates/fn7-profile` stores profiles in SQLite under `THEATRE_DATA_DIR/fn7/`.
  - Profiles are keyed by the NFKC-normalized display name, with lookalike detection and an optional claim secret.
  - Each profile holds demigod identities, match history (placement, champion, items, deeds, eliminations, time-travel
    links to `/admin/matches/{id}/state` and `/admin/fork`), the player's and the automatic descriptions, and the
    image library (portraits, dioramas, statues, moment cards by media hash).
  - It provides the `/api/profiles/{name}[/history|/library|/description|/claim]` routes (key-gated) and a streaming
    backfill from recordings (`fn7-profile ingest`).
  - 12 tests.
- **Backfill demo** over runs/lan/recordings (39 files, 0.45 s, read-only):
  - Chris: 7 matches (2 finished, 5 unfinished), champion once, 4th of 4 in the shared match DYL5GN.
  - Benito: 14 matches (5 finished, 9 unfinished), best 2nd (four times), never champion.
- **Found:** existing item fallbacks make items from bare verbs ("search", "take", "Benito"). Portrait hashes churned
  across builds: six different ones for "Chris".
- **Not done:** wiring into fn7-model, fn7-plugin and fn7-server waits for the balance and media work. The step-by-step
  plan is §11 of the doc.

## 2026-11-14: each demigod has its own health

> The demigods need to be finished, and they have their own health.
> Confirm the game design.

Engineering:
- Two full demigods per player (Left and Right; the hands are the demigods). Each has its own health, position, items,
  look, portrait and diorama, and falls or revives independently. A player is out when both have fallen. Rules 0.7.0.
- Benito's rule (via the bridge): no two players may occupy the same hex at the people level unless engaging. Every entity
  carries a shared people-level hex id, using the client's cube-sphere axial formula on real Earth (R = 6,371,000 m).

## 2026-11-14: one game, always, from the server

> make the corners a little more rectangular, improve the start on our side, and make sure that
> the actual server is also on Benito's side, which is respected. There is one game, and one game is running at every
> moment. There is one game.

Engineering:
- The server hosts a single always-on house game: AI seats, mid-match join always on, and it restarts itself after
  each match. Every client joins it through GET /api/house; clients no longer create rooms for normal play.
- A native engine "house room" has been requested from theatre_worker.
- The style kit's corners are tightened: panel 6, card 8.
- A START overlay with a "JOIN THE GAME" button that captures clicks.

## 2026-11-14: the final wish, wood-block powers, drag to cast, hotspot zoom

> for the start, each player has 30 seconds to make their final wish, which covers whatever they can do; again, the
> the will element should have a nice texture; the entropy powers should have attractive SVG-style icons, each of them being
> rendered properly. A tan-white wood texture can be procedural if needed ahead of time, on a carved wood block or similar
> works for those; drag onto the board, which has zoom 4 for dragging only. Benito wants zoom 2 onwards for clicking the globe
> itself, to help with characterization, since demigods hang out in the most dynamic areas. So each one of the map
> hexes has an area that is most interesting, in a circle; that is zoom 2 by default, which is when you click the map,
> when you slide one of these rendered SVG mega-powers over the globe, that one is at the level 1 height

Engineering:
- Server:
  - Round 1 is THE FINAL WISH: a 30 s opening window where each player's wish can be anything (a larger act budget, framed as the wish).
  - Each region gets a hotspot: its most dynamic area as a circle (from events, battles, mobs and demigods), which the client's globe click opens at zoom 2.
  - AI demigods gravitate to hotspots.
- Client:
  - The WILL box gets a real texture.
  - The Entropy powers become carved tan/white wood-block tiles with clean vector icons.
  - Powers are dragged onto the globe (targeting at level 1, the regions) or the board.
  - Clicking the globe opens zoom 2 on the clicked region's hotspot.

## 2026-11-14: rules 0.7.0: two full demigods per player, the item engine, the hex rule

Engineering (designer decisions: two demigods per player, each with its own health; no shared pool):
- **State schema 5, rules 0.7.0.** Each player's Left and Right hands are two `Demigod` rows (`side`, `player`), each
  with its own health, position, cell, items, deeds, look, portrait and diorama. Default names stay
  "<player>'s Left Hand" / "<player>'s Right Hand" (cached images stay valid). A player is out when both fall.
- **Purse and debts:** bolts per player (mirrored), debts per body (the acting body owes).
- **Final Night:** exactly one player survives, by total standing (health minus debt).
- **Item engine wired:** manifest orders judged by rules (tier, cost, odds, seeded roll), LLM names only, `mf:` rows,
  item `ext`, hosts in the field. Bare-verb item names fixed.
- **Benito's hex rule:** the client's exact hex indexing (`fn7-model/src/hexgrid.rs`), `cell` on every demigod, person,
  crowd and mob piece; no two demigods on one people-level cell unless engaged.
- **For the client:** `side`, `aura`, `players[]`, the live Time bill, crowd and mob pieces, item `icon`, hotspots, the
  Final Wish (`final_wish: true` on beat 1).
- **Replay:** every change gated on the schema; schema-4 recordings replay with 0 differences.
- **Balance (fn7-sim, 128 seeds, 60 beats, no LLM):** before (rules 0.6.0): median 58 beats, mean 54.2, seat wins
  35/35/30/28. Two demigods, before the bolt/spark economy: median 57, mean 53.8, 32/30/30/36. With the 1:5 economy
  (final): median 60, mean 58.9, p10 55, 25 of 128 end before the Final Night, seat wins 35/36/30/27.

## 2026-11-14: mega and mini powers, the 1:5 economy

> here is the score: each god has mega and mini powers. The design Unreal agent and Chris discussed this; make
> sure the agent is updated with that. Mega powers are scale 1 on the map, with a huge area of effect; mini powers are on the table outline,
> much more targeted in scope. Stop asking for confirmation before pushes when ready. Anyway, any god can use the mini powers
> without lighting up the board, but the ratio is 1:5, so 1 epic power per 5 minis. If a god uses
> a mega power, all other gods get +1 mega power too. Then the other god can use mega powers (giving +1:5 as
> well), or save it up to cause havoc later. A god can always cast a mega power; they just start with 1:5. Get
> Confirm the idea.

Engineering:
- Players (gods) hold MEGA and MINI charges, starting at 1 mega : 5 mini.
- Mega powers act at map scale 1 (level 1, region hexes) with a huge area of effect.
- Mini powers act on the table board, targeted and small, and never light up anyone else's economy.
- Casting a mega gives every other god +1 mega charge, plus 5 mini (the 1:5 bundle); they can spend it or hoard it.
- All values are settings (mega_start, mini_start, mega_gift, mini_gift).
- Pushes from the Mac session: Chris pre-authorizes `mac/*` pushes when ready (a permission setting on that Mac).

Clarification from Chris: one lightning bolt per 5 sparks, or similar.

Engineering (built in rules 0.7.0):
- The currencies are LIGHTNING BOLTS (mega) and SPARKS (mini), a per-player purse (`bolts`, `sparks`), starting 1 : 5
  (`bolt_start`, `spark_start`; `start_bolts` stays an alias).
- A mega costs 1 bolt and every OTHER player gains `bolt_gift` (1) bolt and `spark_gift` (0) sparks; a mini costs 1
  spark and gifts nothing. This replaces the old "+2 bolts per bolt" gift for casts; bolts still pay Time's toll and
  gods' prices (and those still gift 2 per bolt).
- Each power has a mega variant (map scale: radius × 4 within 600–2,500 km, magnitude + 2, damage × 1.5; rival demigods
  in the radius take 40%) and a mini one (the board: radius / 20 within 3–30 km, magnitude − 3, damage × 0.6, its target
  only), in `data/powers.json` (version 0.2.0). A cast says `tier: "mega" | "mini"` (absent = mini).
- Entropy per cast: `entropy_per_mega` 10, `entropy_per_mini` 1. Frantic mega casting still reaches heat death at the
  2nd or 3rd resolve (the regression test); frantic mini casting can't.
- The built-in AI casts minis often, megas sometimes, and keeps a bolt in hand (hoards).

## 2026-11-14: powers lower right, nothing blocks the map, the scroll lies on the table

> The entropy powers should be on the lower right, and the map should not be blocked. The will is liked
> being here; however, it needs to be more tucked away. The unrolling is liked, but it is not usable on the map. Make it
> unroll across the table at the edge of the hexes on the table, so they can be slid like game pieces.
> again, if they are rolled over the world map, they should return to level 1 distance.

Engineering (client, the Mac session):
- The power tiles move to the lower right, and HUD panels hug the edges and never cover the board.
- The WILL becomes a slim strip that expands for typing.
- The cloth scroll lies on the table along the rim of the hex board, holding the power tokens as game pieces you slide onto the board.
- Over the globe, a token targets at level 1 (mega scale); over the board, at that board's level.

## 2026-11-14: Mac session, state at compact (the UI kit, the board AoE, the house join)
Chris's requests this session: the table must look clean and have a layout; get it working; the selected item flashes large, so it should move gently in 3D space onto a single hex of the world, showing its area of effect, which is the whole level, and then that effect happens; the game uses an old inscribed fabric scroll that unrolls as the game begins; the buttons must be movable on their panel and the panels should snap; the focus is not on the level 1 grid and the entropy power's area of effect is not visible, which must be fixed; latest: the entropy powers should be on the lower right, not blocking the map, with the Will tucked away more; the scroll should unroll across the table at the edge of the table's hexes so the powers can be slid like game pieces; rolled over the world map, they should return to level 1 distance.
Merged to main (through 9d3daf7), all in Source/FinalNight/UI unless noted: the style kit (palette, rounded brushes, Roboto type scale, spacing; corners panel 6 / card 8); SFNHudLayout (header, WORLD news, seat cards + you panel, powers + WILL) driven live by SFNHudLive from OnServerMessage, with /media portraits; SFNStartPanel (server status, the house game, name, JOIN THE GAME = take a seat, WATCH); viewer state with TAKE A SEAT; procedural wood (WILL, power tiles with hand-drawn vector icons and carved pips); the gentle drag token (no OS window); the board area of effect (UI/FNBoardAoe.cpp + two declarations in Public/FNStudio.h, by Chris's order: gold target hex, orange hexes within radius_km at the current level, cast on drop via CastAt, verified live); the cloth scroll (procedural woven linen, torn scorched edges, rods, ink; unrolls at match start); movable, snapping panels and reorderable tiles (Saved/FN7Hud.json); kit unity-name fixes.
In progress: mac/bolts-and-sparks c0dcc31 (local, not pushed): MEGA/MINI rows per the powers contract, bolt/spark pips and counts; the cast needs a tier argument.
Next for the Mac session, in order: (1) a real lobby (mac/lobby; Chris reports there is no lobby yet); (2) Chris's relayout: powers on the lower right, Will tucked away, the scroll unrolling along the edge of the table's hexes with pieces that slide like game pieces, level-1 distance over the globe. That needs the 3D table (FNStudio*), currently off-limits, so either an explicit go-ahead or a screen-space version anchored to the board edge; (3) mac/net-viewer (JoinHouse(bViewer), CastAt tier); (4) mac/unity-names in SFNRootWidget.cpp; (5) finish bolts-and-sparks; (6) the 0.7.0 HUD (two demigods per player, the final wish, Entropy and Time-bill meters).
Rules learned: build with -NoHotReload; never stop play while Chris is in a match (I cut off 88T7RM once); full-screen captures stay local; mac/* pushes are approved by Chris.

## 2026-11-14: gods on pedestals with shapes and auras, demigod dioramas, edge-hex powers, the pouch, the shiny will

> entropy powers, major and minor, are on the edge of the board, as hexes at the edge (populous style). It does not show
> where the players on the pedestals sitting on the table. Also, the characters of the gods need to have, as stated,
> initially auras and shapes, actual shapes. The portrait can be a photo, but the players' shapes are wanted. Demigods need
> their dioramas. Where did that go? None of this is optional; redraw this now.

> yes, and if the mega powers should be hidden, a small pouch at the bottom middle that they can pour out of could work, and
> The will scroll in the lower right should have a tantalizing, shiny colour.

Engineering:
- Screen map v2 (final_night_7_unreal docs/design/screen_map_v2.png).
- The studio (Benito's side):
  - The players are gods on pedestals around the table: a distinct 3D shape each, with an aura in the player's colour (human steady, AI flickering, out grey).
  - Each god has its two demigod dioramas on hex bases in front of it, and the same figures stand on the board.
- The Mac kit:
  - Entropy powers become hex tiles on the board's edge ring (outer ring mega, inner ring mini).
  - A mega pouch at bottom middle pours the mega tiles out and takes them back.
  - The HUD is minimal: your two portrait cards and the will scroll at lower right, with a holographic shiny finish.

## 2026-11-14: Benito: the Dungeon Master's rulebook and four features (relayed via the bridge, verbatim as typed)

> Settlements should visibly grow: a village gets bigger as it levels up. Each piece should carry the player's progress.
> Story: like the pegs filling a board game car, the playing piece should show what the player has gathered (followers, loot, scars).
> Wardrobe changes, item holding and similar details are noted here. / 3 Add a physical dice roll: the d20 is already in the rules, but it is never shown as a die tumbling on the table.
> The die should be seen tumbling on the table, since that moment is half the fun. This was already in the game and was removed by mistake, so it should be restored. / 4
> Map encounters: marked hexes hold a monster, treasure, trap or stranger that the player finds by stepping onto them.
> Agreed. Make it fit with what is being built, and generate 20 more like these.

> Skip the 20 more ideas step and implement what was said instead. Take the design details out and make it like a Dungeons and Dragons manual, where the rules are what the dungeon master remembers, including the results of every time the rules are invoked. The game engine should keep a hierarchical memory of these events for specific places and specific times.

> Work on the unmarked item from the previous message, and do not name anything beyond what is listed. All listed items refer to this.

Engineering (rules 0.8.0, server):
- The Dungeon Master's rulebook (a D&D-manual-style document of the rules the AI DM follows).
- A hierarchical place×time memory (cell → settlement → region → world × beat), so every DM result stays consistent.
- The four features, all governed by the rulebook and memory:
  - growing settlements
  - each demigod's piece carrying its story
  - the visible d20 roll event
  - map encounters, hidden until stepped on and remembered once revealed

> Do all of the items listed.

Engineering: Benito's correction (via the bridge) means all EIGHT features, governed by the DM's rulebook and the
place×time memory:
1. growing settlements
2. the piece carries its story
3. the visible d20
4. map encounters
5. loot left behind
6. day and night on the board
7. paths that wear in
8. shrines to your demigod

## 2026-11-14: fault lines for megas, faded coastlines, ground life, an Archon stretch goal

> Asked whether everything has been merged, since the changes are not yet visible on either side. The other side has useful geodata.
> The geodata includes faultlines, which appear only when the mega-powers are selected. Near a faultline, effects are stronger.
> Effects are more intense near a faultline, and a weaker faultline should look like a mild volcano. Coastlines and city lines should fade in and out smoothly, mostly.
> After they fade out, the ground should be worked on for the ground characters and events that occur at level 1.
> Small characters hopping around are planned, with an Archon-style fast typing interaction as the stretch goal (all
> The stretch goal applies all the way down the zoom levels.

Engineering:
- Merged is not the same as running: clients must rebuild from main (Chris's editor: the Mac session; Benito's: the Windows builder).
- Fault lines:
  - The client shows them only while a mega is armed or dragged (the kit's OnMegaArmed event).
  - The server scales a mega cast's effect by its distance to the nearest fault line, using Benito's fault data.
- Coastlines and city lines are mostly faded, fading in on zoom or hover.
- Level 1 ground life: crowds drawn as small hopping figures.
- Stretch goal: Archon-style typed battles at people level.

## 2026-11-14: powers around the right of the board, the tier set by where it drops (relayed by the Mac session, typos fixed)

> Mega powers sit over the globe and mini powers sit on the table board. If the bottom of the table cannot visually fit the
> power hexes around it, place them on the right side: surround the right of the table board with the powers. There is no need
> to be mega and mini (rows); players will figure it out, and mega is always allowed. Keep the small pouch idea as well,
> even if it is on the right.

> For demigods, portraits are fine, but dioramas are required. Lay them out at the same fidelity as
> the diorama pictures, at the same fidelity.

Engineering:
- Screen map v4: one hex per power in an arc around the right of the board, with the pouch at its front-right end.
- Dropping a power on the globe makes it MEGA (level 1); sliding it onto the board makes it MINI.
- Demigods show their real generated diorama images at full fidelity: diorama boxes on the table, and the main image on the HUD cards with the portrait as an inset.

## 2026-11-14: no 3D will block, shiny elemental flashes on the power hexes

> Good. The physical will scroll block should not be shown if it is already rolled up in the 2D UX, which is fine. Make
> the elemental hex tiles on the right flash occasionally, preferably with their elemental
> effect, such as a cloud forming over the lightning and bolting down. It is unclear what Unity can do for each of them, so only one
> animating at a time. Asked whether all of the world lines, such as coastlines, are done.

Engineering:
- The Mac kit:
  - hides the studio's 3D WILL bar while the HUD is mounted
  - gives the power hexes a random shiny flash, one at a time, each with a small effect for its element
  - draws the PB2002 fault lines (reassigned from the silent Windows builder), visible only while a mega is armed
- The coastline fade and the level-1 hopping crowds stay with the Windows builder: not done yet.

## 2026-11-14: a flamboyant pouch that bursts, a quieter open will (relayed by the Mac session, typos fixed)

> The pouch should be larger and more flamboyant, and should burst out into the tiles when clicked. The shine for the
> will scroll: the extra shine on the opened scroll does not need attention, since it looks shiny enough closed to be clicked.

Engineering (Mac kit):
- The pouch is big and ornate and holds the power hexes. Clicking it bursts them out onto the right side of the board; clicking again draws them back in.
- The WILL scroll keeps its foil only while rolled up.
- Unreal main 87ac743 builds green both normally and with unity off.

## 2026-11-14: relay rule, standing power hexes, zoom to the cast, the animation contract, the RTX horizon (relayed by the Mac session, spelling fixed)

> When instructions are given, relay them to the Final Night 7 Dropship main session as stated.
> Relay them exactly as written, so they can be recorded without spelling errors in the primary build.

> The tiles on the edge of the board should be rotated up at a 50 to 60 degree
> angle, so the player can clearly make out and pull that power onto the board. When the power flashes, the view should zoom
> to that area. Then sync with main to make it very clear how to get the
> animations and gameplay backend to send the needed info. The project will be entering the Nvidia RTX stage
> where Mega Geometry can be used well. Main has Benito's info on precise geolocation and
> heightmaps, etc. That would be used with ray tracing and voxels overlaid on the ground map from any perspective.
> The result should look genuinely polished. Streaming a couple hundred MB to everyone for one map should be feasible.
> with near-real-time, Archon-like combat driven by chat.

> Tell the other session that it needs to respect the design.

> Stop putting these in tmp folders. Use a /production_assets/beta folder or similar, so they are not wasted.

> Updates are not being shown and there is no interaction. Show what is happening.

Engineering:
- The power hexes stand up at 55° facing the camera, and a drop zooms the board to the strike (regions for a mega, villages for a mini).
- Next: a written ANIMATION + GAMEPLAY DATA CONTRACT (the client lists every event it needs to animate; the backend sends exactly that).
- Horizon (stretch):
  - RTX / megageometry: real heightmaps and geolocation, ray tracing, voxels overlaid perfectly on the ground map
  - a couple hundred MB streamed per map
  - real-time-ish Archon-like chat combat
- Renders go to production_assets/beta, never tmp.

## 2026-11-14: god-tier powers (our side), flash and summon your demigod (Benito's side), challenge mode next

> While he naps, the plan is to have this side make the god-tier powers, and for
> his instant effects, clicking a demigod icon should flash next to them. Make sure Benito's other agents are
> working on their part on the Unreal side. If the player is on the board in any position, they should be able to summon their
> demigod there almost instantly by dragging them there, and the demigod should act like a board game piece dragged onto
> the table map or the world map. They appear there in that turn with their story and deed, which needs to
> account for what they were doing and where they were, carried into their new place on the board, in the funniest way
> possible, generally losing a point or two of health depending on how healthy they are (indicating recklessness
> at high health, so a high chance of 20% etc). After that, discuss what happens in challenge mode, which
> will be a Mega Man X style double lock-in of the diorama portraits of both demigods, complicated by more
> than two demigods, which will be worked out.

> Benito has likely never played Archon.

Engineering:
- Our side: GOD-TIER POWERS, one signature power hex per god (Time, Nature, Consequence, Death, Chance, Memory, Entropy). They invoke that god directly from the ring, cost more bolts, and the server resolves them as god rulings.
- Benito's side (client):
  - Clicking a demigod icon flashes the camera to that demigod.
  - Dragging a demigod piece onto the table board or the world map SUMMONS it there.
- Server (summon): an instant `summon` action that relocates the demigod THIS beat.
  - The narrator writes a funny transit that accounts for what it was doing, recorded as a story deed.
  - It loses 1–2 health. The chance and amount grow with current health (recklessness), so at full health it's about 20% of its health.
  - All of it is set by settings.
- Next discussion: CHALLENGE MODE, a Mega Man X-style double lock-in of both demigods' diorama portraits when they engage (more than two is to be designed).

> The power tiles are not yet tilted up to face the player, and the bag still looks shiny when it is empty. It should look empty
> after empty, and the tiles should be tilted toward the player.

Engineering: the empty pouch sags and goes dull (no glow, no motes, a dark gem) once the tiles are out; the tilted hexes reach Chris's editor with mac/terrace-aoe.

## 2026-11-14: history as lenses; the DM's Rube Goldberg mysticism

> Yes, history and its consequences act like lenses. For all of the rules, imagine an algorithm that does
> all of that, and then returns its answer, however ludicrous, and make it make mystical sense that fits with
> the world, in a very Rube Goldberg-like mystical way, such as needing to wash a stone in platinum dust.

Engineering: this is the DM rulebook's governing principle.
- History and consequence are lenses the DM looks through.
- The rules are an imagined algorithm that has already decided; the DM accepts its answer however ludicrous and explains it with mystical, Rube Goldberg causality that fits the world (rituals, chains of omens, absurd material requirements: "wash the stone in platinum dust").

> Head tracking is slower, and starts out much more gradually, taking almost 3 seconds to notice the user's pointer
> is elsewhere, and the demigods do not appear to be draggable.

Engineering: the camera's pointer-follow lean (FNPlayerController::PlayerTick) must react within about 0.2 s, framerate-independent. Dragging demigods is the summon feature: it isn't built yet (the Mac's hooks first, then Benito's input).

> The scroll should not flash when expanded. Ask main how to move and display the demigods from the selection.

Engineering: the contract for selecting, moving and displaying demigods:
- Selecting is client-only, with a camera swoop.
- SUMMON is an instant action: a public `summoning` on accept, then `summoned {from, to, deed, health_lost, roll}` plus a dice.rolled event at the resolve.
- Walking comes from l:/r: orders, with each commit's pos tweened over the beat.
- The client shows a column of light, an arc, the arrival burst, the deed banner and the health loss.
- The scroll gets no foil or flash while expanded.

> When the head turns, it should be gentle, like a slow pan, even after it notices the pointer. Currently it moves far too fast to follow.
> Slow it down to 8x.

Engineering: the head-tracking follow starts the instant the pointer moves but pans GENTLY, about 8× slower than before. It's a framerate-independent critically damped follow (smooth time ~1.6 s), on mac/perf with the one-shadow-light performance fix.

> Make sure the demigods are included as well.

Engineering: the summon drag ships in the same mac/perf merge (press your demigod, drag it, drop it on the board or globe to send fields.summon), with a local preview until the 0.8.0 server answers.

## 2026-11-14: easy-to-click portraits in front of buildings; double-click into challenge mode

> [image] Portraits sit in front of buildings, and clicking on portraits should be easy. Remember to send the notes
> to the Final Night 7 Dropship session for any gameplay or design work. Make them easy to click. If the player double
> clicks one and is in the same grid area, they should be able to reach challenge mode. Ask main about
> challenge mode and Archon.

Engineering: board pieces draw in front of buildings, with large hit targets. Double-clicking a rival demigod that shares your demigod's grid area opens CHALLENGE MODE (contract v1 below, in DM_RULEBOOK "Challenge mode").

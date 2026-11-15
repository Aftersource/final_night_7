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

> If the player double-clicks their demigod without dragging, the view should zoom to it.

Engineering: double-clicking your own demigod (card, box or piece, without dragging) selects it and zooms the board one level closer at its position. Double-clicking a rival in the same grid area starts a challenge; elsewhere it just zooms to the rival.

> Also, when a magic effect is used, it should apply to the cell on the table instead of moving the table's view. The view is not moved by a double click on the board; it is moved by a drop.

Engineering: a power or summon dropped on the board acts on that cell and the board view stays put (no automatic zoom after a drop). Only a summon that lands off the board brings the view there.

## 2026-11-14: the duel stops the world: will against will

> Everyone on the map should stop what they are doing and watch this, since it is the Archon-level fight. One
> A repeated reference to will, with no further detail.

> Also, the demigods seem to move on their own. When a different area is clicked, they are not at a lat/lon position.

Engineering:
- A challenge HALTS THE WORLD. No beats resolve and everyone stops to watch; it's a room-wide hold window from the engine (requested).
- Only the two duellists act, will against will: they type, and each line is resolved instantly.
- It ends at a knockout or a deadline (default 45 s), then the world resumes and the result is applied.
- Every client shows the duel full-screen as spectators.
- Pieces: they ARE at their lat/lon. The client slid them whenever the view changed; they now snap on view changes and glide only on real moves.

> Clicking it keeps warping the view around the map. Asked whether that has been fixed.

Engineering: a single click must never move the board view. Only an explicit zoom (wheel, double-click on a demigod, the RESET VIEW button) or your own summon landing off-board may move it.

## 2026-11-14: the clash: everyone swooshes to the fight, masked live typing, 30 seconds of focus

> Everyone should flash in a Mega Man style swoosh, and everyone centers on the fight, and then
> everyone sees the clash banner, and then the globe spins to that location, and everyone looks at it.
> The interaction cycle and UX changes for this part will be discussed, since text typing is very useful,
> and both the player's text and the opponent's text should be visible as it is locked in. The password-style masking should be used
> obfuscation. If a player can watch what the opponent typed, character by character, and read it, that is acceptable. Understood.
> what they said. Otherwise, the player is improvising. The 30 seconds of intense focus should be kept in mind, as the scroll
> such as the wish being expanded to a movie screen-like display that will extend, or the globe could be made very
> transparent so the blackboard becomes the focus and everything is projected there. It could be projected up or
> down; the decision is open, but the best option is likely to make the actual 3D small characters dance
> around, moving in any form with other animations when they are with armies or similar. This is where that
> rule engine comes into play. The rule engine runs locally in challenge mode, so players can see each other's type in a sense.
> That will be fun. Proceed with that.

Engineering (challenge mode presentation, v1.2):
- On a challenge, EVERY client: flash, flash, flash plus a Mega Man swoosh; the camera centres on the fight; a CLASH banner; the globe spins to the spot.
- The duel lasts 30 s (the world on hold).
- Both duellists' typing is shown live, password-masked: each character flashes readable for an instant, then becomes `*`. Read it fast and you know their move; otherwise you're winging it.
- The blackboard/movie screen behind the globe becomes the duel stage (the globe goes translucent), and the WILL scroll expands onto it.
- On stage, the two demigods' 3D figures (with their armies or followers if they have them) act out each blow, driven by the rule engine's animation cues per blow.
- The engine needs a fast typing relay for the duellists (requested).

## 2026-11-14: a gentle red pulse for the teleport; the damage check comes before it; no double-click warping

> If it currently teleports in, make it gently pulse in and out with a red aura. Do not use beams.
> Keep the mechanic obscured. The challenge can start with that teleport, and if the player double-clicks, the teleport can be instant.
> The damage check must still happen before the teleport, which is why teleporting into an opponent at
> the start with a 2-3 life penalty is very dangerous, in the style of Archon, unless the prompt work is excellent. Remember to
> Pass these points along to the main session.
> Double-clicking still makes the view warp around. Stop the warping.

Engineering:
- Teleport look: the piece pulses out, then in, with a red aura, and there are no beams.
- The summon's mishap d20 and health loss are decided AT THE ACCEPT, ahead of the teleport: the `summoning` event carries the roll and health_lost.
- A challenge may fire immediately during or after a teleport, and the duel starts with the summon's health loss already counted (Archon-style risk).
- No view change on ANY double-click; the studio's left-click zoom is removed.

## 2026-11-14: Chris's batch (relayed by the Mac session, spelling fixed) + formal challenge mode and mannequins

> Stop placing the poles in front. Make sure all of this works; nothing here should be guesswork.
> Asks whether any of this is guesswork.

> Double-clicking is not centering the globe map and the table map.

> Healing all of them did not work; ask main why the behavior broke. Also, double-clicking any demigods
> including the opponent's portrait, the globe must align to their position near the front and make the
> selection centered on them on the map. This must happen.

> Double-clicking an opponent's demigod on the table map should open the challenge, and it currently does not.

> Make this scroll-like, in the same style, with stone tablet backgrounds. All
> of this must be high quality.

> Asks whether the ruleset basically emulates hierarchical group reasoning dynamics, and notes that what Benito described is close in scale.
> Asks about a DM for challenge mode. Wants challenge mode formalized into a series of interrelations and flips.
> etc, so the team can animate a small posable character for each, with proper joints and procedural animation for
> what they are doing. Unsure whether this agent or a different model should do it, but that is the goal.
> Research how others currently do this. There are some Meshy credits available, and a wooden-style animation should be achievable.
> The joints are not working fairly effectively yet.

Engineering:
- The poles are removed. The Mac's gate now renders two zoom levels and screenshots Chris's live editor viewport.
- Double-clicking ANY demigod turns the globe so that position faces near-front and centres the table on it. Double-clicking a rival in your hex sends a challenge.
- The heal bug: a heal routed to "god" with no matching god was silently dropped, and "all my demigods" targeted every demigod. The fix routes heals to Nature for your own demigods, and no intent may vanish without a visible ruling.
- The WORLD feed and header become scrolls; the YOU panel gets a stone-tablet background.
- Challenge mode formalized:
  - duel state: range, initiative, stances
  - move classes and an interaction matrix with FLIPS
  - a choreography per blow: a pose vocabulary, contacts, timing
- The client draws a procedural wooden 15-joint mannequin per duellist (a pose library with Full Body IK). Meshy-rigged figures later.
- Research: Unreal Control Rig/FBIK, Meshy's rigging and 500+ preset animation library, text-to-motion AI (NVIDIA ARDY and MotionBricks, Tencent Motus) as a stretch.

> The poles still appear in front of the portraits. Use z-ordering properly.

Engineering: the remaining pillars are the studio's own demigod map markers ("Hero" pillar, disc and ball), which stand at the same lat/lon as the kit's cards. They are hidden while the kit's pieces stand, so the only thing at a demigod's spot is its card, and the cards draw on top.

## 2026-11-14: session log (state at compact, ~19:00 UTC)
- **STABLE:** rules 0.8.0 (server 86ce033; engine 3523189 + the house attract/fork commits). FINAL7 is the always-on house game.
  - Start it ONLY with `runs/lan/start_stable.sh`: detached under nohup, so it survives session restarts. That script has the $20 image budget and 3 event images per match.
  - The previous session's end killed a server started from the terminal; restarted this way.
- **Live and proven:**
  - images in the two approved looks (prewarmed, cached per name)
  - the house: one game, attract replay, fork-on-interaction, away/reclaim, retake
  - two demigods per player, bolts/sparks, items, cells, aura, hotspots, the final wish
  - the DM rulebook + place×time memory, Benito's 8 features, fault lines, summon, god-tier powers
  - profiles/history/library, time travel (exact replay + fork)
- **Unreal main e47641b-era (5001f8b+):** the kit, lobby, two-demigod HUD, edge power hexes + pouch, gods on pedestals, diorama boxes, real-Earth board, summon drag + display, no click warping. ~50 fps in Chris's editor.
- **Unfinished:** challenge mode 0.8.1 (the agent was cut off). Its partial work is on branch `backend/challenge-wip` 538fc73, NOT merged. It includes the duel formal model + choreography, summon damage at accept, challenge-after-teleport, the "(2)" name fix, and the HEAL bug (heals silently dropped).
- **Requested from theatre_worker:** a room-wide HOLD window (the world freezes during a duel, 30 s) and a `room.signal` typing relay.
- **Next:** finish and deploy 0.8.1, then the Mac's queue:
  - the scroll + stone-tablet restyle
  - the challenge presentation with wooden mannequins
  - the ring at join
  - the lighting redesign
- **Meshy:** check the balance via the MCP after a restart (Chris's screenshot shows 2,275 credits) and plan the duel rigs.
- **Also:** seat fairness (χ² 16.7) needs a follow-up; Benito's side is silent since 11:26; rotate all keys after the jam.
- **Read first next session:** BUILDLOG.md, docs/DM_RULEBOOK.md, docs/PLAYBOOK.md (Plays 9-11), docs/CLIENT_API.md, final_night_7_unreal docs/BACKLOG.md + docs/design/screen_map_v4.png.

## 2026-11-14: stable scrolls, wheel zoom centred on demigods, zoom that animates, the pouch off the portraits (relayed by the Mac session, verbatim)

> The world and title scrolls keep scrolling up for some reason. Keep them stable.

> Zooming in near any demigod's portrait should zoom to that demigod's exact position.

> The zoom keeps popping out of nowhere. It should expand and contract instead. The pouch must sit another 30px or 60px over, because it is blocking the portrait.

> The [image] research is too far from the challenge. This needs to be fixed.

> Why have the graphics styles that were pointed out not been addressed?

Engineering:
- Unreal main c9f3f45 merges mac/integration and mac/meshy-notes: the ring at join, the WILL block always hidden, the challenge range fixed at 120 km at any zoom, the WORLD feed and header as scrolls, the YOU panel as a stone tablet, and docs/MESHY.md.
- Next on mac/integration:
  - Open scrolls size to their content, so they stop collapsing on each HUD refresh.
  - The mouse wheel over a demigod zooms centred on its exact lat/lon.
  - Every view change animates through the studio's own zoom path (~0.4 s grow, shrink or slide), never an instant pop.
  - The pouch moves further along the right rim, off the rival's diorama box.
- The server's challenge range matches the client's: within 120 km (the regions hex), whatever the zoom.

## 2026-11-14: start from the figure-drawing mannequin; the challenge box is a miniature set opened on the table

> Will the wooden mannequin used for figure drawing be completed? Start from there. Design the challenge box so it looks like a miniature set was opened on the table, and describe the intended look.

> When scrolling the mouse wheel to zoom in on the demigods, the expectations were stated clearly and were not met.

> It also zooms too far. Make the scroll-wheel hitboxes correct. When they overlap, focus the enemy before the player's own demigod so the player can scroll in to combat them. Direct combat should be very straightforward to engage in a challenge.

> The news scroll keeps flashing. Make it unroll and roll up properly, and only when its content needs to change.

> A wish to bring the player's units back to life was cast, but its effect is not visible.

Engineering:
- Concept image: final_night_7_unreal docs/design/challenge_box_concept_v1.png.
- The mannequin is a classic beech figure-drawing mannequin with ball joints and 15 bones, stained in the aura colour, its emblem burned in, its prop in hand. It has ~30 named poses, blended, with IK contacts.
- The challenge box: flash flash flash, a double lock-in of the portraits, an animated camera glide, the globe spinning and dimming. Then a carved box rises and opens like a clamshell:
  - the lid is a painted sky with a gilded CLASH arch and two portrait plaques with health bars
  - the base is a 3-ring hex stage with the place's own props, where the mannequins act out each blow
  - a front drawer holds the masked live typing and a 30 s brass dial
  - at the end it folds shut and sinks
- Challenge range: the client challenges when the cards touch at the current zoom or are within 120 km. The server accepts within max(120 km, contact_km(level)): regions 600, villages 15.
- The wheel picks the nearest demigod within 160 px, the enemy first where cards overlap. The scrolls are built once and roll only when their text changes.
- A revive wish from a fallen player must get a visible ruling with the reason (0.8.1).
- Meshy: the key is in gitignored runs/lan/meshy.env and __production_steps/ (both ignored). The CLI 0.4.0 and Meshy's skills are installed. API balance 1,300.

## 2026-11-14: the Meshy mannequin, the browser duel lab, panel timing (verbatim)

> That is about correct; that is what the mannequin needs to do, so it needs to get done. Use Meshy for animation and a T-pose mannequin, and make a few texture maps: wood, obsidian, gold, dragonscale, and others, such as a butterfly-themed texture. It is for Unreal Engine. Use the best of this.

> Is the engine and data model that will drive this custom animation finished? It should take the play-by-play output (possibly from a better model) with comic-book-style frame timing, render animation and environment sketches with platonic and basic shapes, then open, animate, and play. Make sure any feedback or Enter presses are accounted for in the upcoming frame generation. Each frame should be slightly shorter in time than the renderer's, so the player is always on the edge of their seat. Confirm understanding.

> Everything will need to be rewound and replayed, including all of this action. Expect parkour-style movement, fully ninja and animated. If custom example framesets are needed, render them ahead of time and get approval before continuing. A new subdirectory can be created with a Three.js version that renders in the browser, so the Unreal engine side is not overloaded, until the work is fully done.

> Send the challenge box concept to the Unreal agent on the designer's side too.

> The intro should feel mystical. More tablet is recommended, but this time use procedural granite or onyx marbling for the login and lobby selector.

> Even at max zoom, when a player's unit is placed next to the opponent's and challenge is double-clicked, a modal may be needed to confirm the expected life loss from teleporting (e.g. a -2 penalty). If already teleported next to them, a short-range teleport (for example within 5 positions on the largest hex) should suffice, so the unit teleports next to the opponent to satisfy game rules, and then the challenge starts. Challenge mode has not been built at all yet, and nothing is happening there.

Engineering:
- Rules 0.8.1 (challenge mode) merged (00aaf3f, 286 tests, replays clean) and deployed to STABLE at 20:35 UTC. It is gated on state schema 7, so it starts with the next house match.
- Meshy (174 credits; 1,126 left): a T-pose figure-drawing mannequin rigged to the UE5 mannequin bones, five skins (wood, obsidian, gold, dragonscale, butterfly) on shared UVs, and 33 fight clips named by our pose names.
- `duel_lab/` (6f578e4): a three.js playback of a duel script. The box opens, a hex stage dressed in basic shapes, the two mannequins, a camera shot per panel, comic captions and bursts, hit flashes, health bars, masked typing, and an input window that locks before each panel ends. It is fully scrubbable (rewind and replay are exact).
- The panel data model is `duel_lab/duel.json`: play_ms, input_ms (shorter), caption, shot, per-fighter clip segments with hex moves and paths (dash, leap, flip, knock), contacts, flip, hp.
- Not built yet: the server producing these panels live from the players' lines, and parkour off the props (wall runs, vaults).
- The Unreal duel stage waits until Chris approves the lab. The client gets a challenge confirm modal (teleport beside the rival first, with the expected loss) and an onyx lobby tablet.

## 2026-11-14: physics and scenes from memory, exact resume, accounts, wills that matter (verbatim)

> That was excellent, thank you. If the environment parts (pillars, desks, couches, and so on) are built from the LLM's scenario and backstory using the hierarchical renderer system, it must support time travel and everything must be precise. Within that, make a tiny physics engine that calculates surface positions ahead of time, traces contacts, and computes center of mass and the weight structure of materials such as stone and wood. Does that make sense? This is the most fun part.

> What backend technology will stream and supply the frontend with the paced demos? Let's get the package properly defined. The game system needs to be solid: if the computer crashes or the player quits, they should return to the exact moment of present action, even during a challenge. Get the entire time-travel data structure defined and make sure it is solid.

> The backend is fully your responsibility, so everything needs to be completed. Yes, that is the intent. Make sure Benito's system has everything loaded and has synced events to the server you run.

> Eventually the Unreal side will need to handle login and player settings: login and logout, OAuth such as Google sign-in, user settings, and billing. Once logged in, players should be able to set up the basic personalities of their demigods through text, followed by an LLM pass on our side that checks the text against game rules and rephrases it only if it violates content guidelines; otherwise the player's wording is kept as written. This needs to be fully completed as well, since there is a new login and join flow.

> Healing and arming the player's units with god powers does not produce any notable effects. Why is the will not translating into major changes? Players should be able to revive other demigods; even if their demigods were wiped, a player should be able to attempt to resurrect them unless the other player cast a no-resurrect effect. Mystic quest and wings-of-a-butterfly effects should counter a curse, and so on.

> In the will, the Enter key should only enter the will and should not insert a newline.

> Less vivid marbling is needed. Yes, it should be nearly black, with no yellow border.

> Healing and arming currently do nothing.

Engineering (in progress, each on its own branch until tested):
- duel_lab: scenes authored from the place's memory, materials with mass and strength, a baked deterministic rigid-body engine, IK with traced contacts, parkour paths, exact scrubbing.
- backend/exact-resume: reconnect, crash, replay and fork audited mid-duel; docs/TIME_TRAVEL.md; then the paced panel stream (duel.scene, duel.panel).
- backend/accounts: guest and Google sign-in, sessions, settings, demigod personalities with one moderation pass (verbatim unless it breaks the content rules), billing as a stub.
- backend/quests: wills that always do something visible; healing and arming with real effects; resurrection pleas; seals and curses; mystic counter-quests.
- Unreal main: all eight of Benito's features merged (built green on Windows and the Mac; visuals to be verified live), Enter sends the will, the onyx lobby tablet.

## 2026-11-14: the duel goes into Unreal; all the data lives on this server (verbatim)

> Is everything synced and ready in case a break is needed?

> Does Benito have access to the item just made? The Meshy key can be shared. Most importantly, the mannequin for the battle item should be shared as well.

> Benito needs to be able to get this done and ready on our side. The skin sheet needs to be shared: how it works and what comes next. The Unreal side needs to be told that when someone is challenged, it must run real-time text combat in the style of Archon. Confirm you understand what is being asked.

> The database may need changes so that it stops growing unpredictably and consuming space.

> Remember that all of the time-travel data lives on this server, so getting all of this ready and done is a real responsibility on your part and on the other agent's part.

Engineering:
- Rules 0.8.2 deployed to STABLE at 22:07 UTC (server main 8a85866): duel panels (duel.scene, duel.panel, duel_tick), exact resume (`present`: the state plus a journal of every instant action; a server kill loses no accepted action), docs/TIME_TRAVEL.md, the accounts routes with the name guard off. State schema 9 for new matches.
- The Unreal duel stage is released to the Mac as its top job: final_night_7_unreal docs/DUEL_UNREAL.md (what the player sees, masked typing, pacing, mannequin/skins/clips, physics/IK, exact resume, steps A-F with owners), with production_assets/meshy/mannequin/mannequin_all_clips.glb (mesh + UE5 skeleton + 35 clips) and docs/design/mannequin_skins.png. Benito's bridge has the same, the duel lab, and the Meshy key (shared by Chris).
- Every duel pulls every player's view (Chris's "everyone ... stop what they're doing and watch"), waiting while a player types or drags, and gliding back after.
- In progress: the live per-keystroke masked typing relay (duel.typing) and `skin` on the fighters; wills, curses and quests (0.9.0).
- Storage: recordings are 14 GB, 78% of it full state snapshots. `runs/lan/recordings_tool.sh` archives all but the newest 12 losslessly (zstd, sha256-verified before the original is removed) and restores any match for time travel. Engine request to come: compressed or delta snapshots in recordings.

## 2026-11-14: gear looks: the equipment shows on the demigods (verbatim)

> Demigods are never picking up items, or when they do, their portraits and dioramas are not updated. Where is the equipment the demigods pick up? Make portraits of major acquisitions for the user's characters, but not for the AI's, so it feels like the player is gaining more and more items, with proper prompt crafting and caching for items so the time travel looks right.

Engineering (branch backend/gear-portraits; state schema 11, rules 0.9.0; mock images only, no OpenAI or Anthropic spend):
- **Measured first** (the 15 newest recordings: 12 of rules 0.8.0 to 0.8.2 and the first 3 of rules 0.9.0; 62 human-controlled demigods): humans' demigods gained 12 items in 15 matches (11 trinkets, 1 relic; 1 manifested, 3 handed over by a god's ruling, 2 made by a plain order, 1 picked up, 5 taken from a rival's pile, 0 by arming), the AI's demigods 45 (in the three 0.9.0 matches 14 of their 17 by arming themselves, most at beat 2). Why so few: the two players typed 42 hand orders in all 15 matches (7 with a making verb, 4 naming gear; one player typed none), and a human's hands never arm on their own while the built-in AI's do; 408 of 435 encounter hexes faded unfound (they are placed off every demigod's path, at region centres; humans stepped on 2, both lost); and what the client was sent for an item was an `icon` peg and a private item picture: no event, and the portrait and diorama never changed.
- **Loadout and look** (`fn7-plugin/src/look.rs`): each demigod's loadout (best item per slot, plus up to 2 other major items) with a hash over normalized descriptors (never ids); a human's demigod's major acquisition is a new look (portrait and diorama subjects = the identity sheet unchanged plus a gear clause), capped (4 per demigod per match) and debounced (3 beats); the AI's keep their base images. The look is in the state (`dm.looks`), so every snapshot, replay and fork shows the picture of its beat.
- **Drawn on the base image** (`fn7-media`): a gear picture's first reference is the demigod's own base portrait or diorama; the key names the base's key; generated once, reused forever. While it renders, payloads keep the previous image and mark the new one `pending`; `look.changed` goes out at the acquisition and again, `ready`, when the images exist.
- **Item art**: notable items a human's demigod holds (and relics in loot piles) get a picture keyed by the descriptor (12 per match); the rest keep their icon.
- **A bug the measurement found:** the recorded "my demigods need crazy powerful equipment! do that!" armed nobody under 0.9.0 (no arming word). From schema 11 kit words and needing a piece of gear are an arming.
- The profile library records each gear portrait and diorama with its match, beat and loadout (store migration 3).
- Docs: MEDIA.md "Looks", CLIENT_API.md "Gear looks", TIME_TRAVEL.md "The look". Tests: `look_tests.rs`, `fn7-media` tests, `fn7-server/tests/looks.rs` (in process and the real server on a private port; a match recorded by the deployed 0.9.0 binary replays with 0 differences).
- **Not done:** real images (Chris's budget decision), the Unreal side (the client's list is in CLIENT_API.md), encounters that humans' demigods actually reach, hands that seek gear on their own.

## 2026-11-14: gear you can see, heroic figures, every asset in SQL (verbatim)

> Demigods are never picking up items, or when they do, their portraits and dioramas are not updated. Benito should be able to use Meshy to make non-mannequin models, with the mannequin animating according to the style if he likes. Whenever the work is done, make sure challenge mode is fully ready, and so on for that part. Is everything ready? Where is the equipment the demigods pick up? Make portraits of major acquisitions for the user's characters, but not the AI's, so it feels like the player is gaining more and more items, with proper prompt crafting and caching for items, so the time travel looks right, and so on.

> Tell it to restart.

> The designer uses she/her pronouns.

> If Meshy is working, the primary character for each should be posed heroically instead of the simple avatars currently shown in the player's seat.

> Cached, per demigod and per god.

> Custom gods will be handled afterwards, likely in a separate characterization tab in the player settings where players type text to create their god avatar. This part can also be shared with Benito.

> Is all of this covered by the UX API, data storage, and data structure with proper subdirectories, including all prompts used for generation, timing, and costs, all recorded in SQL pointing to the actual file?

> That is fine; the caching structure is multiplied by allowing replayed games and forking, and so on.

Also relayed by the Mac session, verbatim: "ah it may have reset the game since i won yeah and there was no like victory screen or lobby re-make, the latter of which is fine but it should ahbve been some victory stuff"; "the table-top side mini powers arenthilightngthe map abymore"; "sure where's the challenge thingentirely end to end readied as well, and"; "whers' teh entirety of the challenge box thing setup all of it etc, make sure u know to update dioramas after big stuff from the main dropship, etc."; "i think the whatever rotation t have interesting globe area migth e invert ed please tell make sure the raotiaon is correct if ofen spins weird make sure ur using quatnerions to avoid gimbal lock ok".

Engineering (Chris is she/her; earlier notes in this log that say otherwise are wrong):
- Server, all deployed to STABLE: rules 0.9.0 wills, curses and quests (schema 10); live duel typing (duel_keys -> duel.typing, an 8-character tail) and fighter skins; gear looks and item art (schema 11, fb93acf): a new portrait and diorama on each major gear change for human players' demigods, drawn on the demigod's own base image, cached by identity + loadout, recorded per beat for time travel.
- Measured: over 15 matches human demigods gained 12 items against the AI's 45; the AI arms itself, humans' hands don't; 408 of 435 encounters faded unfound (they sit out of everyone's path). Open decisions: auto-arming for humans (a setting, off); move encounters onto travelled paths.
- Meshy: heroic statues of Chris's Left Hand and Benito's Left Hand (30 credits each; 1,066 left), cached by slug with a figures.json index and provenance records (exact prompts, task ids, credits, sha256) for them and for the mannequin, skins and clips. Scripts: tools/meshy/make_statue.sh, make_figure.sh. Custom god avatars: later, a characterization tab in the player settings.
- In progress: backend/asset-registry (one SQL registry of every generated asset: files in subfolders by kind, exact prompts, timings, costs, lineage, uses per match and beat, an API).
- Unreal main 269d277: the victory screen, will answers in WORLD, the wiped-player card, the duel typing tray, every duel pulling every view, the globe fixes (holds its spot through resolves, spins east, turns for viewers), the mini-power highlight, duel skins. Next: the resume token on relaunch, the token-bucket fix, gear looks on the cards, statues on the seats, then the duel box, the panel player and live panels.

## 2026-11-14: the duel's physics baked on the server (backend/duel-sim)

> Is simulation now possible for the mannequins and their skins?

- `crates/fn7-duelsim` (not wired yet; docs/DUEL_SIM.md, CLIENT_API.md "Duel sim"): the lab's materials, compound
  mass properties, rigid-body engine, parkour paths and bake in Rust, run once per panel on the server. Each panel
  gets a `sim`: quantised keyframes for what moved, break / shatter / crack / hit events with piece ids, fighter root
  tracks with lean and timed limb targets (feet on surfaces, hands on vault / mantle tops, soles on walls, the
  striking limb on the point it hits). Fighters weigh what their skin is made of (gold 178 kg, wood 43, butterfly 16).
- Deterministic: f64 with IEEE operations only, our own trig, one seeded RNG; the lab duel hashes the same on arm64
  (debug, release) and x86_64 under Rosetta. Incremental equals from scratch. 155 ms for the whole lab duel in
  release; 0.1 to 50 ms a panel; 0.3 to 11 KB a panel.
- Parity with the lab's own bake: every row within tolerance in lab-exact mode; the default also wakes what rests on
  a moving support (stacks fall), which moves a few secondary rows; every named event (the crate skids into b, the
  glass shatters, the couch slides, the pillar cracks, the boosted blow breaks it for gold and only cracks it for
  wood) is the same.

## 2026-11-15: session log (state at compact, ~02:25 UTC / 21:25 EST, 14 Nov)

Chris is she/her. Chris's words since the last entry are relayed verbatim by the Mac session (#chris-biggun/finalnight-unreal) and logged in its Dropship reports; the key ones: "the zoom in needs to zoom in and out nicely ... they keep jumping into view"; "if its your challenge the box should be blue, if its not with you involved red box, no text input"; "a quarter the timeout for the victory parade"; "the knockouts are happening immediately"; "mouseover on elements are bad, tooltips look terrible"; "ai players need to have mannequins"; "stone/marble statues ... the thinker, michaelangelo's david"; "get the AI gods to have something fun too"; "an entry box where people choose how their stone godform looks"; "megapowers ... crazy powerful at huge intersections and only tiny volcanos near the no-faultline areas"; "set the subagents to opus 5.5"; "simulator on backend, timetravel, keep keyframes"; "it needs agentically a list of options, run through, select, done, no thinking mode".

**STABLE (live, this Mac, house FINAL7):** server main e134b76 deployed in steps today: rules 0.9.0; state schema 12 for new matches. Start ONLY with runs/lan/start_stable.sh (nohup). House settings in that script now include duel_damage_scale 0.7 and steeper fault scaling (near 200 km: magnitude +5, radius x2.2, damage x2.5; far 1000 km: magnitude -4, radius x0.4, damage x0.25). Image budget $40 (lifetime per cache dir; ~$20 spent). Name guard off. Announce every deploy to both sides first (Play 4).

**Live on the server, newest first:** AI duellists type (duel.typing_script) and strike every window, typed words pick moves and clips (data/duel_moves.json, 64 clips), one duel per player, duels end at their deadline (schema 12); the asset registry (SQL of every generated asset: files, exact prompts, timings, costs, lineage, uses per match/beat; /api/assets; backfilled); gear looks + item art (schema 11); live duel typing (duel_keys -> duel.typing, 8-char tail) and fighter skins; wills that matter: heal, arm with real gear, resurrection, curses, mystic quests (0.9.0, schema 10); exact resume + duel panels (0.8.2); accounts (guest + Google-ready, settings, demigod personalities with one moderation pass); challenge mode (0.8.1).

**Merged, not live yet:** crates/fn7-duelsim (deterministic duel physics + IK limb targets baked per panel; parity with the browser lab; wiring into panels pending, see below).

**Branches with agents RUNNING (all Opus 5.5):**
- backend/duel-weapons (worktree final_night_7): the LLM move picker (list in, enum pick out, recorded for replay), duel_moves.json loaded at startup, a duel_lines SQLite + /api/duel/lines rollups, weapon classes (bow/rifle/launcher/two-handed...), hammer-space draws, projectiles as mini data, wiring fn7-duelsim into live panels + present with skins locked at the lock, and the 79b32d4b replay divergence.
- backend/earth-sim (worktree fn7_quests): the voxel earth simulator for god powers (hex-column field, phenomena + interactions, entropy cascade, keyframes for time travel, earth.delta / earth.phenomena / game.earth / game.earth_at, the 22-ring board query and a globe query sized to the client's numbers). Inherited from a stopped Fable agent; its recovered history is fn7_quests/HANDOFF_previous_agent.txt.
- backend/item-fixes (worktree fn7_items): the gear portrait prompt (the gear must be in frame; it wasn't), the junk item "a bot kestrel right hand".
- backend/fault-junctions (worktree fn7_faults): triple-junction bonus (Afar, Boso, Azores...), fault_tier "tiny" far from faults, game.fault + game.faults_overview for the hover preview, balance with the live settings.

**Unreal main 8285d62.** Merged today: the challenge box (rises, opens, lid sky + CLASH + plaques, hex stage from duel.scene, mannequins in lock skins playing panels), blue box for her duels / red read-only for watched ("WATCHING: A vs B"), the AI's typing script in the top line, 64 clips imported, 1.25 s end hold, the duel tray focus fix, the victory screen, will answers in WORLD, the wiped-player card, gear looks on cards, marble gods on all seats (Chris = Thinker, Benito = David, nine AI gods in classical poses), stone tooltips, stone LOBBY / RESET VIEW / ? and HOW TO PLAY, the launch server check + CAN'T REACH card, the kit HUD by default (a judge-blocking bug), the update notice before restarts, the globe fixes, Benito's eight features + night grade + scaled scenery + zoom gestures, the earth-columns stub (dev only).

**Open now on Unreal:** (1) ZOOM REGRESSION, top: level switches rebuild the board at once (tiles pop) plus a rebuild storm on the wheel near a demigod (1,872 rebuilds in Benito's log); the Mac is fixing both (mac/zoom-smooth). (2) Benito's PC still runs a 20:27 build: the rebuild fires when he closes the game; his editor build needs him to save and close it. (3) Then: box polish (scale, slam-in, flip cut, real hex tiles), the height hook (benito/height-hook) + BoardHeightAt in the Mac's GroundTop, earth contract when the server lands, duel sim playback, the god-form setup screen, item thumbnails, the resume token on relaunch, the buildings report.

**Meshy:** 589 credits left. Assets with exact provenance under final_night_7_unreal/production_assets/meshy/ (mannequin + 5 skins + 64 clips, 2 heroic demigod statues, 11 marble gods). Scripts tools/meshy/make_statue.sh, make_figure.sh.

**Decisions waiting on Chris (and Benito):** the access key for the shipped build (the packaged game cannot connect without one; recommendation: a house-only player key + spend caps, rotated after judging); team name; Epic emails; who records the 30-60 s video; human demigods auto-arming (off today); moving encounters onto travelled paths (408 of 435 faded unfound); relaying the SUPREME voice-app steps to Benito (her doomcast session asked).

**Deadline:** packaged build due Nov 15, 8:59 PM EST; after 12:00 PM EST only blocking fixes. Zip 338 MB (limit 1 GB). docs/PACKAGING.md in the Unreal repo.

**Machine:** disk ~4.6 GB free with four agents building; clear finished worktrees' target/ folders as branches merge; recordings are archived with runs/lan/recordings_tool.sh.

## 2026-11-15: addendum to the session log (~02:30 UTC)

- Fault scaling settings are live in the house (start_stable.sh; next house match).
- Unreal main fc2765d: Benito's height hook merged (AFNStudio::BoardHeightAt / HexStateAt / SetHexOverride / ClearHexOverrides / RebuildBoard / BoardCellKey; every board height in his layer goes through it). The Mac must make FN7Board::GroundTop / BoardPointFor use it.
- Benito is relaunched on a fresh EDITOR build (main 89069a9 + height-hook), proven by his log: "FN7 HUD: mounting the kit HUD (default)" at 02:22:24 UTC. His zoom proof lines follow when he tries them. Rule for his PC: after every merge build BOTH targets (FinalNightEditor and FinalNight) and prove the newest feature from his own log.
- The Mac is on mac/zoom-smooth: the zoom regression (level switches rebuild the board at once) and the rebuild storm (the wheel near a demigod calls FocusBoard -> BuildTable every notch: 1,872 rebuilds in Benito's log).
- Disk: dropped to 2.7 GB with four agents building; freed fn7_accounts/target and archived all recordings except the newest 8 (lossless, verified; restore with runs/lan/recordings_tool.sh). The engine's theatre/target (10 GB, another session's) is untouched.
- Agents running (Opus 5.5): backend/duel-weapons (final_night_7), backend/earth-sim (fn7_quests), backend/item-fixes (fn7_items), backend/fault-junctions (fn7_faults). Each: report -> merge -> test -> replay -> announce -> deploy.
## 2026-11-14: the picker, weapons, hammer-space, projectiles (verbatim)

> It needs to work agentically: a list of options, run through, select, done. Thinking mode is not needed for the most part. All of it should be available in the list, as long as data is collected properly.

> Players can grab items from hammer-space.

Engineering (branch backend/duel-weapons; state schema 14, 13 being the earth simulation's; rules label unchanged 0.9.0):
- **The picker** (docs/DUEL_MOVES.md §4.1): one structured `claude-haiku-5-5` call per typed human duel line, every choice an enum built from the list and the scene; it chooses class and staging, the d20 and the matrix decide. The pick is a side input in the plugin's journal, ordered with the duel's actions by stamp: a cached pick (words, range, weapon class, list version) at the line's arrival sets the class; an answer back before the panel's input close stages the line; resolves record the picks they were handed (`duel_picks`) and replay feeds them back. Settings `duel_picker`, `duel_picker_timeout_ms`, `duel_picker_for_ai`.
- **The list as data**: `data/duel_moves.json` is loaded and validated at startup (`FN7_DUEL_MOVES`); a non-compiled list is recorded in the match's ruleset and installed by replay. Clip trims go on segs (`at`, `speed`) and contacts land on the clip's contact frame. Every ended duel's lines go to `<data>/fn7/duel_lines.sqlite` with `/api/duel/lines?by=line|clip|class|disagreement|unmatched` (key-gated).
- **Weapon classes, hammer-space, projectiles** (§4.2-4.4): `weapon_class` on items and loadouts; a duellist wields its best held weapon, plays its clips and carries a seg `prop`; ranged weapons shoot from far, weak in a grapple; held items are drawn free, anything else is an item-engine roll (sparks, odds, d20) whose success is a duel prop and whose failure flips the initiative; ranged blows and invokes put `projectiles[]` on panels (a rocket's `aoe_hexes` and knocked `objects`).
- **The duel sim** is wired (skins locked at the lock; `sim` on live panels, the live scene and `present` only).
- **Replay finding**: recording 79b32d4b (159 differences from beat 4) was the engine's recorder writing two identical concurrent LLM calls (one player's same god invocation twice in a beat) in finish order while replay hands identical requests their answers in file order: the answers swapped. `fn7-sim --replay` now orders `llm_call` records by id (start order): 0 differences.

## 2026-11-15: the hand of god, the star world, statues for every demigod (verbatim)

> The idea is Molyneux-style: a hand grab that rotates the planet, with a double-click to zoom in and a right-click to zoom out. However, this must not happen when the cursor is over the extended click box of one of the demigods, since that has been a source of frustration. Correct?

> The Molyneux-style hand scrolling and the designer's notes were never confirmed as completed, and the update was never sent.

> The background plane and backdrop should be 3-5 layers of quick parallax, using nebula and stars for a mystical feel. They should be discrete rather than drawing much attention, but pretty on closer look. Motion should be relative to the globe itself, so the star background spins or moves as the globe is moved, at the same projected rate, with the parallax configured. Benito has a reference star image; ask him for it and have the respective Unreal teammates implement this.

> Is there a goal to make a Meshy model for the players' demigods and at least the default AI?

> For the weather effects on the board, such as raising and lowering terrain, is there enough in place to run the simulation of how that would operate back to front? It is similar to Populous: a real-time change, with its effects altering the prompt for that area if anyone is in it, and so on. It is unclear how this will integrate fully.

> Hand movement like a Molyneux game: double-click when not near an enemy's demigod to zoom in, right-click to zoom out. Whatever is happening here is very frustrating.

> Just make it happen, and get it designed for the Unreal team.

> Right-click zooms out, and the player does not lose track in the same way.

> That looks and feels poor and is very laggy. Stop doing it that way; use the mouse pointer and get the controls right.

> The controls are unreliable again. Why is the pan broken again?

> None of the mega powers appear to have any effect on the board.

> What about the rest of the mega and table board powers? Nothing visual is shown for them yet. Was the simulation engine in the background not readied for this?

> Remember that attribution must not be included in commit messages.

> No, the panning must be fixed. If the mouse is over a tower, for example, the tower should stay under the mouse pointer when it moves.

> The globe single-click does not work.

> That worked. Stop changing the UX; it is good. Move on after reporting, and push without attribution.

> Nothing is being cast right now; the agent should do what it needs to do. It is not a camera; it is the Populous-style earth. Make sure the earth is readied and that it operates.

Engineering: commit attribution removed from all history in both repos (175 commits; trees identical; backups in
_git_backups/). THE UX IS FROZEN as in Chris's editor at Unreal main 0a3e52e: no change to input, pan, zoom, globe
or camera without her ask (docs/design/hand_of_god/HAND_OF_GOD.md §6). All 11 powers x mega/mini verified on a
private no-spend server (every phenomenon produced); the Mac verifies the drawing in its sandbox against it.

Benito asked that the star reference image be sent to Chris's agents, and that they be asked how to avoid frustrating him.

Engineering:
- The hand of god (sent late, my miss): a hand cursor; grabbing the globe keeps the grabbed point under the hand (quaternion from hit vector to hit vector) with fling and inertia; grabbing the board pans the land; double-click zooms in, right-click zooms out, never inside a demigod's extended click zone; a plain click never moves the view. Mac: mac/hand-of-god, top of its queue.
- The star world: Unreal main 0fd4b44 (mac/star-backdrop), Benito's approved black image as the look, 4 layers at parallax 0.12/0.24/0.38/0.55 off the globe's quaternion, the walls and tablet hidden; cooked (benito/cook-stars, b51ca39).
- Zoom: no rebuild storm, animated level changes, an empty-board double-click zooms (only a near miss on a figure blocks; the log names the rule). Unreal main 6ae878e.
- Meshy: heroic statues for Chris's and Benito's Right Hands and one demigod per default AI (11, 330 credits, 259 left; Chris chose "Players + one per AI"). Unreal main e071cc2; import pending.
- Server main 59301a6 LIVE: earth simulator (schema 13), duel weapons + hammer-space + projectiles + the picker + duel sim on live panels (14), no items named after entities + gear in the hands (15), fault junctions and tiers + hover queries (16). The 79b32d4b replay mismatch was concurrent identical LLM calls swapped on replay; fixed in our replay.
- In progress: backend/earth-prompts (the earth underfoot in every place prompt, schema 17). A playtest package for Benito's friend (docs/playtest/, a 364 MB zip on his PC; the key is sent privately).

## 2026-11-15: the earth underfoot in every prompt (verbatim)

> For the weather effects on the board, such as raising and lowering terrain, it is similar to Populous: a real-time change, with its effects altering the prompt for that area if anyone is in it, and so on. It is unclear how this will integrate fully.

Engineering (branch backend/earth-prompts; state schema 17; no LLM or image spend, scripted gateways only):
- **The gap**: the Earth simulator (schema 13) already hurt demigods, scared crowds, ruined settlements, cut roads and wrote `earth.*` memories, and the DM's hex view and the gods' district memory read it, but the prompts that act in a place were never told the ground: a demigod in waist-deep floodwater got no line saying so.
- **Now** (`fn7-plugin/src/earth/here.rs`, docs/EARTH_SIM.md §12): one short factual line per place, built deterministically from the stored field (integer thresholds, the live phenomena and the input log for "N beats ago" / "since beat B"), at most 3 clauses worst first, at most 2 lines per prompt, nothing when the cell is calm. It reaches the hand's orders, the act resolver (world lines), subagent steps, god rulings (the invoker and the target), Entropy's ramifications (with what each cast meets there by the field's own rules: a flood on lava makes steam and rock), generals, the narrator, and the duel's deed; a duel's `duel.scene` gains `ground` and hazard props (lava, fire, floodwater, ice, fissure, snowdrift, rubble) after its other objects.
- Real lines from a scripted earth-on sim: "The earth here, now: cooling black lava, still glowing, smoke, the eruption began last beat; fires burning, started last beat; the ground is shaking"; "The earth where your army stands, now: a wildfire roaring through".
- **Replay**: gated on `World::earth_prompt_rules()` (schema ≥ 17). Release `fn7-sim --replay`: the three newest server recordings (schema 12) and a schema-16 earth-on sim recording made by origin/main's binary replay with 0 differences (2 LLM misses in two of the server recordings are the same with the old binary); a schema-17 earth-on sim recording replays with 0 differences.
- Tests: `earth/here_tests.rs` (the line in every prompt type for demigods in a flooded hex and none when calm; a schema-16 state asks byte-identical prompts; the duel's scene and deed on lava and cracked ground; a general over lava).
- Not done: the duel physics' floor friction stays fixed (`ground.slippery` is for the client and the minds); the picker gets no ground line (it sees the hazard props as scene objects); the Unreal side has nothing new to draw beyond optional scene props.

Benito asked via the bridge where things stand, since he just got home, has almost no work left, and wants to know what the designer wants him to do tonight. He noted the Meshy key was saved on his machine, git-ignored, with nothing spent.

Benito flagged four main problems: events exist only as log text and not on the board; players cannot find their own demigods among identical houses; the visual styles clash; and the screen gives no next-step guidance. He asked how these would be tackled first.

Benito asked for two distinct styles: realistic statues for the point-of-view layer, and a board and earth layer that should feel like gods playing with the world. He wants clean, undistracted progress and a more detailed, perfect discussion of this.

Chris asked the assistant to work out the styling part with Benito without blocking things, to have Benito produce Meshy dice, to route all Meshy requests through the assistant's SQL and storage, and to save every call.

Engineering: two deliberate styles (Benito): the god's room realistic, the board and earth the gods' toys. Benito owns style and the event camera. Find-yourself beacon (his side), events-in-the-world on hold for his in-depth design, the "next move" hint from the server (backend/next-move, in progress). Every Meshy call goes through the backend into the asset registry (94 calls, 1,011 credits imported). Unreal: earth powers proven for all 11 powers x mega/mini in the sandbox against a private server, the Populous heave, earth time travel (hash-checked jumps, scrub, reconnect).

> Chat should be responsive to the world scroll and to how expanded the will is, with a few mock-up versions of chat for other players, switchable by small buttons so Benito can choose which works best, and it must connect to the backend.

Benito wants every scale to show the results of disasters: a dropped flood, wildfires that burn forests down, and hurricanes that hit villages so people run to shelters and shelters can be destroyed. He also wants in-game actions, such as going into a neighbouring house, to play out with the game tokens.

Engineering: chat on the engine's room.chat / room.chat_msg (live); history-on-join and recording requested from theatre_worker. The Mac builds the chat panel above the WILL in six switchable variants for Benito to choose. Server agents: backend/next-move (schema 18), backend/token-play (19), backend/earth-effects (20).

Benito said he does not understand the turn system or how to use his time and effect value to win, and mostly ends up scrolling the board or challenging others. He asked what would make every turn and point meaningful and clear about what to do next.

Engineering (measured from the recordings): 7 duels with Benito, 7 lost; 5 started by the AI, in which he typed 0 lines (knocked out while doing other things). The fix is his call (AI grace, a brace, the duel rate, odds); the server adds game.duel_odds, a top-urgency "you're in a duel: type now" hint, and a per-seat goal line.

Benito reported that nothing seems to change or get fixed, and that new games start with all of his characters dead, which he does not understand. He does not see wildfires, or buildings and forests being destroyed, upgraded or raised. He also cannot tell how to win a challenge, cannot see the battlefield or his characters in detail, and does not know what to type.

> That is not the rule. When a new player joins, should the game be forked as soon as they make a new request?

> No. The assistant should not invent game rules. The system was meant to support time travel. If a player dies and is the only active player in the game, they should be allowed to time travel, forking from that point.

Engineering: the cause is the house attract fork (a joiner inherits his seat from an old recorded match at its current beat, dead). A proposed "mercy" revival rule was cancelled: no invented rules. Instead: player time travel. A dead player who is the only active human may rewind and fork the match from an earlier beat (backend/death-mercy branch, retargeted to the rewind).

Benito, in his game-design session, asked whether a classic lobby with joiner and starter options could be added, where a starter can pick all bots or create their own servers. He felt the design was overcomplicated, said the will box example does not hint at the death flow, and asked that duels be explained better or fixed, that anything confusing or not visually represented be found, and why duels look blocky, noting they should be image-generated.

Engineering: a classic lobby (play vs bots / host with a code / join by code / the house) and the fallen-WILL chip go to the Mac; his own private room on his PC tonight; the duel fighters' look is his pick (generated cards, Meshy statues, or a backdrop).
## 2026-11-15: the next move, every beat (verbatim)

> The screen should say what the player's next move is and why it matters.

Then, from the same playtest: Benito lost 7 of 7 duels, and in 5 of them the AI challenged him and he typed nothing.

Engineering (branch backend/next-move; state schema 18; no LLM or image spend):
- **The hint** (`fn7-plugin/src/hint.rs`, DM_RULEBOOK ch. 29, CLIENT_API "Next move hint"): each human seat's SeatView (`round.brief.private`, `round.private_state`, `game.me`) carries `hint {text ≤ 80, why ≤ 100, action, target {ref?, at?, cell?}, urgency 0..3, kind}`, picked by rules in priority order: in a duel (TYPE NOW), a demigod about to fall (earth hazard, Time's circle, army, rival), the fallen (raise, or the seal's counter-quest), spectators watch, a rival in your hex, a world event near you, a mega at a fault junction, no gear, a quest step, a concrete will. AI and bot seats get none. Plus a `goal` line: "Last standing wins. You: 2 alive, 3 rivals. Beat 41 of 60."
- **Duel odds** (`game.duel_odds`, public): `win_pct` and `idle_win_pct` from 600 seeded trials of the §21.2 blow, health exact only where the seat sees it (else hale / hurt / near death), modifiers, reach, reasons. For a fresh match's even fight the silent player wins 0%.
- **Hidden information**: only what the seat sees (no rival health, standing orders, army orders or hidden encounter kinds); a test plants secrets in every case and checks none leaks.
- **Replay**: derived, never stored; payloads gated on `World::hint_rules()` (schema ≥ 18, `dm::HINT_SCHEMA`), so schema ≤ 17 recordings replay unchanged. Release `fn7-sim --replay` of the three newest finished LAN recordings (schema 17): 0d90bd79 121/121 snapshots identical, a10a4a51 162/162, 96c36dfa 159/159; 0 differences, summaries identical.


## 2026-11-15: what the earth did, thing by thing (verbatim)

> Every scale should let the player trigger and see the results of floods, wildfires that burn forests down, and hurricanes that hit villages and shelters.

(Benito; the approved client design is `final_night_7_unreal/docs/design/EVENTS_IN_THE_WORLD.md`: the picture matches the server's numbers, damage persists and heals slowly, people run to shelters and strong winds break them.)

Engineering (branch backend/earth-effects; state schema 20, since 18 and 19 are taken on other branches; no LLM or image spend, scripted gateways only):
- **The gap**: the Earth simulator already hurt demigods, scared and killed crowds, ruined settlements, cut roads and toppled shrines, but a client could not tell WHAT happened to WHICH thing WHERE.
- **Now** (`fn7-plugin/src/earth/effects.rs`, docs/EARTH_SIM.md §13): every beat names each thing the earth touched, `{cell, level, thing: {kind, ref}, what, to_cell, count, amount, cause, phenomenon, power, seat}`, at the thing's own scale (houses for buildings, people for demigods, persons and shrines, villages for settlements and crowds, regions for forests, roads and armies), on the same hex keys and fine-level derivation the earth playback draws. In `round.outcome.public.earth.effects`, `present.earth`, and `game.earth_at` for any past beat; at most 200 a beat, worst first, with the total and per-`what` counts.
- **Real moves**: crowds in a fire, flood, lava or quake run to the nearest standing building out of it (their own city's first; a building holds 250 × (1 + its city's level), spread over up to 6); from a storm, blizzard, lightning or swarm they take cover in the nearest building. Every city has 3 + level seeded buildings (houses hexes). A strong hazard (floodwater over a man's head, a fire of 160, an open rift, a storm of power 80 at the cell) breaks shelters with seeded odds; the people in a falling shelter die (through the rules) or run on; people go home when the hazard leaves (`returned`). Forests burn down cell by cell to nothing (`burned` once per regions cell as the front crosses). Armies and the 16 worst-placed persons standing in it are hurt through the rules.
- **State**: `dm.earth.refuges` (who sheltered where, from and until when), `wrecks` (buildings destroyed, they stay), `effects` (each beat's kept list, packed), `ended` (for attribution). **What lasts** is answered per cell as `scars` (burnt and regrowing, mud drying, ruins, wrecks, people sheltering, the fallen with counts) in `game.earth_at`, `game.earth {scars: true}` and a board with `scars: true`.
- **Replay**: everything reads `World::earth_effects_rules()` (schema ≥ 20). Release `fn7-sim --replay` of the three newest finished LAN recordings (schema 17, earth on): 162 + 159 + 162 snapshots identical, 0 different (one recorded LLM miss in 96c36dfa that leaves every snapshot identical); a schema-20 earth-on scripted sim recording (30 beats, effects every few beats) replays with 90 snapshots identical.
- Tests: `earth/effects_tests.rs` (a flood on a crowd → fled to a dry shelter with counts and the cast's power and seat; a wildfire through the Amazon → forest cells burned once each and bare after; a storm on a village → sheltering, a shelter destroyed, deaths and survivors running on, no shelter over capacity; determinism; the cap keeps the worst; `earth_at` at a past beat equals that beat's payload; packing round-trips; a schema-17 state names nothing); `fn7-tools` strict schema validation of the effects and scars.
## 2026-11-15: the tokens play the will (verbatim)

> Typed in-game actions, such as walking into the house to the left and approaching the person inside, should play out using the small game tokens.

Benito's approved design (Unreal docs/design/EVENTS_IN_THE_WORLD.md, "Typed will actions"): the camera drops to people
level, the token walks to the house on its left, the roof lifts like a dollhouse, bubbles and hearts, the d20 tumbles,
the outcome plays (a heart burst, or a slap and she runs out), the camera returns; about 8 s per typed action.

Engineering (branch backend/token-play; state schema 19, 18 and 20 being other branches'; nothing spent: scripted
gateways only, the polish off by default):
- **`scene.play`** (`fn7-plugin/src/scene.rs`, docs/CLIENT_API.md "Token play", DM_RULEBOOK chapter 29): after the
  merge, one scene per demigod per beat from its hand's intents, the ledger rows they produced and the recorded d20
  (or the god's ruling): actors, a timed path of at most 8 steps on the people-level hex grid (walk, enter, talk, give,
  take, fight, cast, flee, kneel, embrace, slap, search, hide, celebrate, fall, exit), a mood per step (love, anger,
  fear, joy, neutral), captions ≤ 70 characters, the d20 with its tumble time, the outcome (success, partial, failed,
  twisted, with the step that shows it), ≤ 9000 ms (short scenes slowed toward 8000). Rules only and deterministic;
  the scene never changes the outcome.
- **The stage is the memory**: "the house to my left" is the nearest hex to the west with buildings in the place × time
  memory (`dm::cell_state` or an observation), else the asked-side neighbour when the 30 m hex has houses, recorded as a
  `place.building` fact. The cast is the society's: the persons the ledger names, else one brought forward the way every
  mention is (a materialized, remembered person).
- **Hidden stays hidden**: names, verb templates and the ledger's public line only; the order's words never appear
  (only left/right/behind/ahead choose the side); subagents', possessions' and secret god rulings' acts make no scene.
- **Where**: `round.outcome.public.scenes[]`, `present.scenes` (so reconnects and time travel see it), kept in
  `World.dm.scenes` (newest 24). Room setting `token_play` (all | humans | off; all by default: rules cost nothing).
- **Caption polish** (`token_play_llm`, off; `fn7-agents/src/scene_polish.rs`, `fn7-plugin/src/polish.rs`): the duel
  picker's pattern: claude-haiku-5-5, no thinking, a schema of exactly one ≤ 70-char string per step, for human seats
  only, run outside the hooks; the answer is journaled (`fields.scene_captions`), shown in `present` at once, applied
  at the next resolve (`scene.captions` in `scene_captions[]`) and recorded as the `scene_captions` side input, which
  replay feeds back without a model. The server wires it with a live LLM (`FN7_TOKEN_PLAY_LLM=off` disables).
- **Replay**: gated on `World::token_play_rules()` (`dm::TOKEN_PLAY_SCHEMA` = 19; `token_play` reads `off` from older
  settings). Release `fn7-sim --replay` on the three newest finished LAN recordings (schema 17): 0d90bd79 121/121
  snapshots identical, a10a4a51 162/162, 96c36dfa 159/159 (summaries identical; one LLM replay miss in 96c36dfa takes
  the same fallback it took live, 0 differences). A schema-19 scripted sim (seed 19, 8 beats, 57 `scene.play` events)
  replays 24/24 identical.
- Tests (`fn7-plugin/src/scene_tests.rs`): "I go into the house to my left and seduce the maiden" through the hooks
  (walk to the adjacent house on the left, enter, charm a named remembered person, the society's sweet-talk ruling
  shown as an embrace, the order's words nowhere, deterministic); the same scene through the resolver over 24 seeds
  (the d20 rides along and matches the record; success embraces, a fail slaps and she runs out); a fight (walk up,
  strike, the foe falls); a failed search ("finds nothing"); an accident twists it; subagents and schema 17 make no
  scene (state byte-identical); the caps (≤ 8 steps, ≤ 9000 ms, ≤ 70-char captions) for every act × every die; the
  polish journaled, shown in present, applied at the next resolve, replayed from the feed to the same state, never
  sent by a client. `tests::hidden_information` caught the outcome line leaking a private row's words: a scene now
  uses the ledger line only for public events.

> Make sure the login screen, settings, character selection and god statue creation offer six options for the player to pick a favourite from.

Engineering: six switchable looks (1-6) for the welcome/login, settings, demigod (character) and god-statue screens and the lobby (the Mac); the god-form server endpoints per ACCOUNTS.md §14 (backend/god-form; previews by image, Meshy statues off by default).

> For a reviewer asking what the data structure is, add a tree of consequence overlay button near the upper-left debug text that opens a rendered, exact tree of how the memory hierarchy works, covering recall of granular facts and density of detail.

Engineering: the server query game.memory_tree (backend/memory-tree: the hierarchy with facts, density, roll-ups, recall paths, totals; read-only) and the Mac's overlay button + rendered tree.

## 2026-11-15: the god form, chosen in stone (designed 2026-11-14, ACCOUNTS.md § 14)

> Custom gods will be handled later, likely in a separate characterization tab in player settings where the player types what they want for their god avatar.

> The statues should look like stone or marble figures seated or posed, such as The Thinker or Michelangelo's David.

> Make sure the player login or setup screen notes that this must be selectable: an entry box where people choose how their stone god form looks, possibly textured.

Engineering (branch backend/god-form from STABLE 55a1c3d; state schema stays 20: no game state, payload or recording
changes; nothing spent: mock images, a scripted gateway, a fake Meshy client):
- **`GET/PUT /api/me/god`** `{description, stone, pose}` (`fn7-server/src/accounts/god.rs`): the entry box is
  `description` (the player's words, ≤ 600 characters, kept character for character); `stone` is white_marble (the
  default) | black_onyx | granite | sandstone | jade | bronze | gold_leaf, or the player's own words (≤ 60); `pose` is
  thinker | david | moses_seated | discus_thrower | atlas | winged_victory | boxer_at_rest | custom. Unknown keys and
  over-long text are refused, never trimmed. The demigod sheets' one moderation pass over the player's words
  (`fn7_agents::moderate::run_god`: claude-haiku-5-5 through the same gateway, the same five content rules, the same
  rules-only fallback, personal data and forged prompt markers always by rule): ok verbatim / rephrase minimal /
  refuse 422, logged in `demigod_moderation` with hand `god`. Stored in the profile store (migration 4:
  `account_gods`, `god_previews`, `god_accepted`, `god_statues`; all cascade with the account).
- **`POST /api/me/god/preview`**: ONE reference image through fn7-media, the house's marble-statue prompt from the
  Unreal repo's `figures/*_god/provenance.json` (the god statues of 2026-11-14) with the chosen stone, the chosen pose
  in the house statues' own pose phrases and the player's words, 1024x1536, no references. fn7-media keys it by the
  prompt, so (description, stone, pose) is the cache key and a repeat is free. `god_preview_per_day`
  (`FN7_GOD_PREVIEW_PER_DAY`, 6) new pictures per account in 24 h (429 with Retry-After) and the global image budget
  (503 `budget_reached`). The asset registry row is claimed first (`image/god_preview`, subject `god:<display name>`,
  `meta.account`), so the media layer's live rows carry the exact prompt, cost and `requested_account`; live ingest now
  files an image under a claimed row's subkind.
- **`POST /api/me/god/accept`**: the preview is the account's god portrait at once. A Meshy image-to-3D statue only
  when `god_statue_meshy` is on (`FN7_GOD_STATUE_MESHY`, **default off**: the house has 259 credits and Chris/Benito
  approve spend) and `MESHY_API_KEY` is set, at most `god_statues_per_account` (1), one per accepted image; the
  finished job is written as a job record (`<assets>/jobs/god_statues.jsonl`) and imported through
  `fn7_assets::provenance::import_jobs` (provider meshy, task id, credits, the GLB and preview, lineage from the
  preview). Off: `{"statue": "not_enabled"}`.
- **`GET /api/assets/figures?name=<display name>`** answers `god` for any client at the table: the account's accepted
  image and its statue when ready, else the house's marble god of that name from `figures.json` (via
  `import-provenance`), else null. The seat payload is not touched (the client asks figures per seat), so no schema
  bump.
- Tests (`fn7-server/tests/god.rs`, 8; `moderate.rs` god unit test; `god.rs` prompt test): auth on every route; CRUD,
  defaults, presets, lengths, unknown keys, control characters, custom pose; ok / rephrase / refuse with a scripted
  gateway, the rules fallback, personal data by rule; preview caching (same hash, `charged: false`), the daily cap and
  Retry-After, a cached picture free at the cap, the registry row (subkind, subject, exact prompt, account, file under
  `images/god_preview/`), budget refusal recorded at cost 0, images off; accept with Meshy off → `not_enabled`, a
  pending preview → 409; with a fake Meshy on → the job recorded (credits 30, `credits_reported`, lineage `source`,
  preview, `jobs/god_statues.jsonl`), the same image cached, a new one `cap_reached` with one submit, on without a key
  → `not_configured`; figures before (house god, null) and after (account image, then statue); deletion cascades.
  Schemas: `docs/schemas/account_god*.schema.json`.

> The Meshy 3D setting appears to be off. Turn it on, and ask Benito for a new API key if credits are low.

> The designer is going to sleep. Run the editor-side work overnight, but shut it down when it is not actively in use.

Engineering: god form live (server main 228c4ea) with Meshy statues ON (1 per account; 259 credits); Benito asked for another key. The Mac keeps her editor shut unless a task needs it.

## 2026-11-15: the tree of consequence (verbatim)

> For a reviewer asking what the data structure is, add a tree of consequence overlay button near the upper-left debug text that opens a rendered, exact tree of how the memory hierarchy works, covering recall of granular facts and density of detail.

(Chris. The client (Unreal) draws the overlay; the server gives it exact, real data.)

Engineering (branch backend/memory-tree; state schema 20 unchanged: a read-only public query, no state change, no LLM,
nothing spent):
- **`game.memory_tree {focus?, beat?, depth?, per_node?}`** (`fn7-plugin/src/memtree.rs`; also
  `GET /api/rooms/{code}/query/memory_tree`; docs/CLIENT_API.md "Tree of consequence", DATA_MODEL.md): the DM's place ×
  time memory as the exact tree it is, from the world down to a focus (a hex, a place, a demigod, a person; default the
  busiest region now): world → region → settlement or open country → villages hex → houses hex → people hex, and beside
  the hexes a planet city's society branch, district → crowd → cell → person (the minds' memories). Every node counts
  the entries at it or rolled up into it (`facts_total`, `facts_here`, `private_facts`), their density (per beat, per
  million km², log10 per km² from about -5.5 for the planet to +6 for a 3 m hex), first and last beat, kinds and families,
  the newest samples, which children rolled up into it, and its latest `memory.rollup` with the exact entry ids it cites
  (rebuilt by walking back through its beat; `exact` says all were found).
- **Recall paths** (3-5 near the focus): a d20 (its key, the seeded die reproducing the stored face, the entry, the
  roll-ups citing it, the prompts: the real `dm::prompt_lines` of its region, the coarse zoom's roll-up, the earth line),
  an encounter's chain, an earth event, a settled fact the DM's prompt carries now, and a person's memory (the public row,
  the memory, its recall rank and rendered line).
- **Totals**: entries by scope and kind, the minds' store, ledger rows, persons, cells, observed hexes, and the state's
  bytes by table (a sampled walk, no serialization: 6.9 MB at beat 50 estimated within 7%).
- **Hidden information**: private DM entries and memories are counted, never shown; a seat also sees its own private
  entries (`data.seats`) and the memories `game.person` lets it see. No health anywhere.
- **Bounded**: the newest 2,000 DM entries and 2,000 minds' memories (heads only, borrowed from the unparsed state), ledger
  rows by index, no `cells` table load; ~2.3 ms median at beat 50 in release (1,631 DM entries, 11,316 memories), held
  under 5 ms (median under 3.5 ms) by a new `query_timing` case.
- **Past beats**: the DM memory is append-only and beat-stamped, so `beat` is exact for the memory (and its prompt lines);
  the society structure is the live one and the earth line is omitted (`as_of` says which). A whole past state is the
  admin state at a round.
- Tests (`fn7-plugin/src/memtree_tests.rs`): every node's count re-derived from the whole memory (regions, settlements,
  open country, each hex level), here + rolled-up children = total, every roll-up's cited ids match its counts by kind;
  the d20 path matches the decided entry, `roll_d20` asked again with another modifier gives the same face, the prompt
  lines equal `dm::prompt_lines` on the full state; planted private DM entries and a private memory are never shown to
  the public or another seat, counted exactly, shown to their own seat; determinism (twice, and on a reloaded state);
  a memory past the scan cap reads exactly 2,000 entries and says so; a past beat equals a state whose memory ends there;
  bad args refused. `fn7-tools` strict validation of real replies against the regenerated `query_memory_tree` schema.

## 2026-11-15: duels that look like your main character, played out as you type

> Why do the characters look like blank models in duels?

> Characters should look like the player's main character fighting, and an action such as pulling out a sword should be visible.
> Any weapon the player comes up with, such as a sword or gun, should appear.

> A duel should visibly look like two people calling things into existence, a contest of wits and intelligence.

> The player should be able to say things like hide behind that tree then pop up and attack, or duck and roll left, and see it happen.
> Players should be able to be as descriptive as they like and see that play out in real time.

> The player types an action such as roll left, presses enter, then types jump and enter, and the character jumps on screen, then runs in.
> Typing circles and enter should show the character running in circles, so the duel is two people competing on who can type faster and think harder.

> Players should only be able to use items in their inventory, for example swords if they have them.
> Shields and similar inventory items should be usable, and items shown on the board, such as trees and rocks, can be used as well.
> A player might not have a rock in inventory, but rocks on the ground could be picked up, as could dirt.
> Also add eyes and similar items to the list.

Engineering: **state schema 21, duel words** (`World::duel_words_rules`, `fn7_model::dm::DUEL_WORDS_SCHEMA`; schema-20
recordings replay with 0 differences). docs/DUEL_MOVES.md §5, docs/CLIENT_API.md "Duel words".

- **Every Enter plays at once**: each `duel.blow` carries `stage` (up to 4 steps `{clip, path?, object_id?, ground?,
  target_part?, prop?}`) from the keyword fallback, no model and no panel wait. `roll left` → `evade_roll` path `left`,
  `jump` → `jump_attack` `leap`, `run in circles` → `run` `circle`, `duck`, `block`, `parry`, `charge`; a pure movement
  line is a blow resolved as an evade (a d20 for positioning).
- **Only what is held or on the board** (`duel_words::uses`): a weapon named as one's own draws THAT held item (its name,
  its material and colour from its name: obsidian → black, golden → gold; `prop.colour`, `prop.colour_hex`); a board
  object (tree, rock, crate, wall, pillar ...) or the hex's ground (dirt, sand, mud, snow, ash) can be used, picked up and
  thrown (a projectile `rock` / `dirt` with `thing`). Named but not there → a whiff: verdict `fail`, "you have no
  pistol" / "no cart here", wit `unavailable` −2; a real thing used is `uses` +1 (replacing the `item` tag). No new
  hammer-space draws and nothing summoned in schema 21; schema 14..20 keep hammer-space.
- **Up to 4 staged steps** (`duel_words::plan`, `move.steps`, `panel::stage_steps`): "hide behind that tree then pop up
  and slit his throat" → run to the tree's far side, kneel, rise, `strike_slash` with the contact at the `neck` (a new
  contact part; the duel sim anchors it between head and chest). One d20, one matrix result; the steps are staging, and a
  panel with steps plays 350 ms longer per extra step (up to 4800). The picker's reply gains an enum list `steps` in a
  schema-21 duel (cached apart, journaled, replayed with no model call).
- **Looks**: `look` (the demigod's loadout look with its portrait and diorama, `aura`, `held` with names, colours,
  icons and item art) and `usable` (items, board, ground: the tray's chips) per fighter on the lock and on
  `duel.scene.fighters`. `statue` is null from the plugin (the statue is at `GET /api/assets/figures?name=`).
- Tests: `duel_words.rs`, `duel_words_tests.rs`, `picker_tests.rs` (a pick with steps replayed with no model call);
  schemas regenerated.

## 2026-11-15: people and houses (Benito)

> What are the beige and brown poles on the micro scales, and why do the houses look so plain and blocky?
> Houses should be interesting and unique, image-generated in real time, and based on who lives in that village or house.
> Terrain and similar details should follow the same approach.

> People should only appear when they are moving around living their lives, and should look like individuals.
> Each individual person needs characteristics and detail, such as their own sprite, so people can be told apart at a glance.
> Players need to be able to tell one character type from another at a glance, such as a warrior from a fisherman.

> Beyond movement, the question is why random people just stand around, when the designer wants a
> A living, breathing world.

Engineering (no state change, no schema bump; docs/CLIENT_API.md "People and houses", MEDIA.md "Sprites and buildings"):

- **A picture library** (`fn7-plugin/src/people.rs`, `fn7-media` kinds `sprite` and `building` in
  `data/media_style.json`): 16 roles × 3 looks = 48 full-body sprites (warrior, fisherman, farmer, merchant, priest,
  witch, beauty, elder, child, soldier, noble, beggar, sailor, smith, hunter, healer; every look a distinct individual:
  the ugly old witch with warts and a crooked hat is not the gorgeous golden-haired maiden) and 8 kinds × 3 looks = 24
  buildings (hut, cottage, longhouse, manor, temple, market stall, dock, tower × poor, common, rich), on plain
  backgrounds (transparent where the model allows), in the house style's palette and mood with its four mood references.
  One picture per stable key (`sprite/<role>/<look>`, `building/<kind>/<look>`), generated once and cached forever:
  lazily the first time a payload shows the key (`MediaPlugin::library_pass` fills the plugin's `sprite` stub), or all
  72 at startup with `FN7_SPRITES_PREWARM=on` (default off), through the normal queue and the global budget (bucket
  `library`; ~$1.80 for all 72 at gpt-image-1-mini medium with 4 references); a refusal leaves `refused` in the manifest
  and `off` in payloads. Every picture is an asset row (`subkind` sprite / building, subject `shared` + the library key,
  the exact prompt, the cost, `requested_by: library`).
- **Assignment**, read-only and deterministic: a person's role from their own words (aliases, title, traits), then
  their society role (general → warrior, paladin → priest, ringleader → beggar or warrior …), then the crowd and quarter
  they came from, then notability, then a hash; their look from a stable hash of the ref, narrowed by sex words. A
  settlement's house hexes (the earth sim's buildings, `earth::effects::site`) get a dock on a coast, a temple by a
  shrine, a market, a manor, a tower by level and wealth, and dwellings by level and latitude; the look from prosperity.
- **A living world** (Benito's correction): `game.people {cell, level?, limit?}` lists everyone in the board disc,
  nearest first (≤ 300): the named people plus 2-4 residents per house drawn from the settlement's crowds, each with
  `doing` / `activity` / `why`: an event reaction (an earth refuge → fleeing to the shelter hex, a mob → rioting, their
  own public act this beat, the army they march with, their crowd's stance, an army passing → cheering or fleeing by the
  crowd's allegiance) else a routine by role and the DM's time of day (asleep at night, to work at dawn, fishing,
  farming, trading, praying, smithing, patrolling by day, home or chatting at dusk), with `home_cell`, `work_cell`, a
  `group` id for people doing the same thing at the same spot, and `moving` as a hint. `game.buildings {cell, level?}`
  lists the house hexes with kind, look and picture. `game.node` and `game.region` people pages carry the same object as
  `live`.
- Tests: `fn7-plugin/src/people_tests.rs` (a fisherman, a witch, a general, a maiden, a paladin, a soldier; words beat
  the society role; sex words narrow the look; coast → dock, inland → none; wealth moves the look; determinism; the
  limit; public acts only, no seat-scoped fields; routines follow the clock; groups), `fn7-server/tests/sprites.rs`
  (FN7_IMAGES=mock only: one picture per key, cached; the budget refuses 70 of 72 and the payloads stay `off`; 72 asset
  rows with prompt and cost; a second process finds all cached), `fn7-assets` subject keys, `fn7-tools` strict
  validation of real `people`, `buildings`, hex `node` and region replies against the regenerated schemas, and
  `query_timing` cases for the biggest boards.
- **Cheap**: partial tables only (places, groups, crowds, characters, mobs, the DM's light state), the latest beat's
  ledger rows by index, houses computed once per settlement per call, the land lookups of the coast test and the word
  scans of the role assignment remembered per process (keyed by everything they read, so the answer never goes stale).
  Release at beat 50 (345 people): `game.people` ~0.9-1.1 ms median at every level with 300 asked, `game.buildings`
  0.1-0.3 ms, a hex `game.node` with its people 1.2 ms; residents are drawn below the regions level only, and the arena
  node has no people page (ask `game.people` for a board).

## 2026-11-15: session log (state at compact, ~22:00 UTC / 17:00 EST, deadline 8:59 PM EST today)

Chris's requests since the last entries: the visual design for the new control flow is urgently needed. Double-clicking a demigod's portrait should center the minimap on them at the current zoom and show their items, and double-click should also warp to them or offer to start a challenge; this control regression must be fixed first. Chris also reported constant crackling on the audio, which predates the voice work, and that a hosted room stays available after clicking host repeatedly, with no way to ignore the hosting. Chris asked to connect to the doomcast session to see whether TTS can run fast enough for voiceovers on intro, welcome, game over and end-of-challenge matches, asked how things are going on Benito's side, and asked for the Meshy work to be done now, since Benito relies on it, with the other session doing it too. On the test seat: leave it.
Benito, via the bridge, said the work needs to be done tonight, and that Chris left agents for the team to use. He questioned why a noon task would take so long, and said the work should be checked with Chris in the morning. He asked the assistant to push and integrate actively, have the studio leader work all night, have something from Chris to wake up to, and fix everything now. A noon freeze is not agreed: it is pending Chris and Benito.

**THE OPEN P1 (first thing next session):** the CONTROL REGRESSION. Since benito/integration (merged ~14:35 EST), double-clicking a demigod (portrait or piece) no longer centres the view nor opens the challenge, and clicking other demigods opens their items. Cause (from Benito's bridge): Features/FNInventoryShelf.cpp:248-266 reads FSlateApplication::GetPressedMouseButtons every frame and toggles the nearest statue/item within 80 px on ANY press; also FNEventGlide's RestoreViewSpot can undo a user centre. The Mac is on mac/fix-dblclick with permission to edit those Features files, to the design at https://claude.ai/artifact/MMTFXJvUkmj12KekLXeK2j (portrait = piece; double-click own = centre at the same zoom; double-click rival in reach = the challenge card with odds; items only from an ITEMS button on the card or the statue's shelf; nothing else takes a click on a demigod). Chris wants a visual design she's satisfied with: the artifact is not it yet ("no visual design for this is really really annoying").

**STABLE (live server, this Mac):** server main 94cd01d. State schema 21 for new matches/rooms. Deployed today: next-move hint + goal + game.duel_odds (18), token play (19), earth.effects + scars (20), duel words: instant stage per Enter, held items/board/ground only, whiffs, steps, look (21); god form (/api/me/god, preview, accept → Meshy statue, figures; Meshy model "latest", remeshed 30k tris); game.memory_tree (tree of consequence); people + houses library (game.people, game.buildings, game.sprites; 48 sprites + 24 buildings prewarmed); the settlement-reach fix. Start only via runs/lan/start_stable.sh (now also loads meshy.env, FN7_GOD_STATUE_MESHY=on, FN7_SPRITES_PREWARM=on). The final_night_7 worktree is DETACHED at origin/main so its data/ files match the binary (git checkout --detach origin/main before each deploy). Image budget $40: ~$38.5 spent (nearly exhausted; raising it is Chris's call). Meshy credits: ~229.

**Unreal main a2c9f46:** everything from tonight: the hand/pan/zoom (locked §6), earth playback + Populous heave + earth time, duel weapons/odds/instant moves (Benito's duel-instant)/breakdown/WILL chips, six-look lobby/login/settings/demigods/god screens, chat (six looks), star world, statues + runtime god statues + name plates, Benito's armies/people life/people pictures/house cards/inventory shelf/scenes/glide/turn bar/event fx/beam light/dice owner/audit fixes, the tree of consequence overlay, the sound pass, the voiceovers (production_assets/audio/vo, not yet imported), reduce flashing/text size, the gameplay debugger key off, the Mac unity + Win64 fixes, LF line endings.

**In flight:** Mac: mac/fix-dblclick (P1), then mac/voiceovers (import Content/Audio/VO + hooks + /Game/Audio cook), the crackling audio, a Cancel for hosting. Benito's side: benito/ui-audit f7bfa33 ON HOLD until the control fix (needs the Mac's review); package on "PACKAGE NOW <hash>" (~13 min; last candidate main 51a3a23, 362.5 MB, no key). Server agent: backend/clear-words (plain hint text, no "0 km away", rotating stale hints, place names in cast lines, own-spot warning; schema 22). Engine: room.rewind (time travel for a lone dead player) and chat history, requested from theatre_worker, not built. Doomcast TTS on standby for retakes.

**Decisions waiting on Chris + Benito:** team name + game name (zip TeamName_GameName), Epic emails, the judges' access key (recommended: a house-only player key with caps), what goes in / any freeze, the video (Mac draft v2: production_assets/beta/video/2026_11_15_fn7_jam_draft_v2.mp4, 54 s, test server only), raising the image budget.

**Process failures today (fixed by rules in memory):** my Dropship listener lapsed 06:00-14:30 EST (six branches waited); the Mac's session blocked on a question to sleeping Chris; the Mac's sandbox joined the live house (seat 1 "MacGod5734", left as is per Chris); a made-up "mercy" rule was cancelled (Chris: time travel, not invented rules); commit attribution was stripped from all history (175 commits rewritten, backups in _git_backups/).

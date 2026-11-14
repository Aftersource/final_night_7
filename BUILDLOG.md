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

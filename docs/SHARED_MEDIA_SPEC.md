# Shared ground: image, video and character generation (spec v1, PROPOSAL)

The backend and the bridge meet here (see PLAYBOOK.md §1). This spec is written against what exists today:
- **Server:** `crates/fn7-media` (see docs/MEDIA.md).
- **Client:** Benito's `FinalNight.uproject` (UE 5.8.4, Slate UI in `SFNRootWidget`, HTTP + WebSockets modules).

Status: **proposal**. It's final when Chris (budget, direction) and Benito (art direction, UX) both agree.

## 1. Who owns what
| Area | Owner | Notes |
|---|---|---|
| Generating, storing and caching images (and later video) | **Backend** | Central storage on the server. Every image keyed by content hash, recorded with its match. The image API key never leaves the server. |
| Deciding which game moments get images, and writing prompts from game state | **Backend**, tuned by **Benito** | Selection thresholds and prompt wording live in a style config (§4) that Benito edits. |
| Art direction: house style, framing, palette, what's off-limits | **Benito** | Through the style config; the bridge proposes changes on a `bridge/media-style` branch when the backend asks (playbook Play 5). |
| Displaying images, transitions, the end-screen "final cut" | **Bridge / Benito** | Unreal: download, turn into a texture, cache by hash, render in Slate. |
| Spend: provider, model, per-match and global budgets | **Chris** | Off (placeholders) until Chris sets a budget. |

## 2. What exists today (playtest-1)
- **Image objects in payloads.** Next to every `image_prompt` / `portrait_prompt` / `statue_prompt`, the server adds
  `{hash, kind: portrait|item|event|statue, status: pending|ready|off, url|null, prompt}`. The places this happens:
  - big events: casts, eliminations, magnitude ≥ 8
  - demigod portraits in the public view, stable for the whole match
  - end-of-match statues in `match.ended.summary.piles[]`
- **Manifest.** `GET /media/index?since=<cursor>&key=<KEY>` → `{enabled, mode, cursor, items:[{hash, kind, status, url, ...}]}`.
  Poll it and pass back `cursor`.
- **Files.** `GET /media/<hash>.<ext>?key=<KEY>` serves the image (PNG with a real provider; SVG placeholder cards in
  mock mode now), immutable and cacheable forever by hash.
- **Mode on STABLE:** `mock` (free placeholders). Real generation (OpenAI Images, default `gpt-image-1-mini`, about
  $0.09–0.22 per 4-player match) is built but off until Chris sets a budget.

## 3. What we add (to agree)
**3.1 Character visual identity.** Every demigod and named person gets a stable **visual sheet** generated once from
the server's seeded data: species/build, face, outfit, signature colors, one prop, and their emblem. Every image of
that character reuses the sheet, so the same person looks the same in a portrait, an event and the statue.
- Exposed on `person` / `globe.demigods[]` as `visual: {sheet_hash, traits:{...}}`.
- Portrait = sheet + pose. Event images that name a character include its sheet traits.

**3.2 Request by reference.** A client can ask for an image of something in the game, never a free-text prompt.
- Request: WS `t:"game.media"` with `{kind:"portrait"|"place"|"event"|"army", ref:"ch:12"|"pl:4"|"lg:88"|"cl:7"}`.
- Reply: an image object (pending, then ready via the manifest).
- The server writes the prompt from state and checks visibility (you can't request an image of a private event you
  don't know). Each request counts against the budget, with a per-seat rate limit.
- Use: Benito's UX shows a portrait the moment a player zooms into a person.

**3.3 The "final cut" storyboard (video, phase 1).** `match.ended.summary.storyboard`: an ordered list of shots, each
`{beat, at:[lon,lat], region, image (event or portrait object), caption, duration_ms, camera: "globe"|"close"}`, built
from the match's biggest moments plus the narrator's final cut. The client plays it as an animatic: globe fly-to,
image, caption.
- **Phase 2 (later, needs Chris's budget):** server-side video clips per shot (`video` objects, same lifecycle as
  images).

**3.4 Live moment cards.** During play, `round.outcome.public.events[]` items with an image object are "moment cards".
The client may show the card when `status` becomes `ready` (via the manifest) and skip it if it's still pending
when the moment has passed. Images never block the beat.

## 4. The style config (Benito's lever)
`data/media_style.json` on the server, versioned, recorded in each match's ruleset:
```json
{ "version": "1",
  "house_style": "collectible-figure game illustration, Fortnite meets Final Destination, chunky stylized 3D, bold saturated colors",
  "kinds": { "portrait": {"framing": "waist-up, three-quarter view, dramatic rim light", "size": "1024x1024"},
             "event":    {"framing": "wide cinematic, seen from high above", "size": "1536x1024"},
             "statue":   {"framing": "amiibo-style figure on a round base atop a pile of its items", "size": "1024x1024"},
             "place":    {"framing": "establishing shot", "size": "1536x1024"} },
  "avoid": ["text", "logos", "real people's likenesses", "gore close-ups"],
  "event_selection": { "min_magnitude": 8, "always": ["cast", "elimination", "general_appointed", "mutiny"], "max_per_match": 12 } }
```
Benito tunes the style and selection here; the backend only enforces budgets and ids.

## 5. Unreal integration notes (for the bridge)
- Download with `FHttpModule` (`GET /media/<hash>.png?key=…`), decode with `FImageUtils::ImportBufferAsTexture2D`,
  wrap as an `FSlateBrush` for `SImage`.
- Cache in memory by hash, and on disk under `Saved/FN7Media/<hash>.png`: the hash is content-addressed, so the
  cache never goes stale.
- Placeholder SVGs (mock mode) can be skipped. Show your own placeholder whenever `status` is `pending` or `off`.

## 6. Acceptance (both sides)
**Backend:**
- character sheets, `game.media` and the storyboard are implemented
- tests for determinism (same match → same hashes), visibility and budgets
- a mock-mode match on DEV produces a full storyboard

**Bridge:**
- the Unreal client shows portraits, moment cards and the storyboard animatic from a recorded match on STABLE in mock
  mode
- the same works on a real-image match once Chris enables a budget

**Release:** the media additions ship as one announced release (`playtest-N`).

## 7. Decisions needed
1. **Chris:** a real image provider and budget (or stay on placeholders for the jam).
2. **Benito:** the house style and the selection list in §4.
3. **Both:** whether video phase 2 is in scope for the jam (recommendation: no; ship the animatic storyboard).

# Shared ground: image, video and character generation (spec v1, AGREED; backend implemented)

The backend and the bridge meet here (see PLAYBOOK.md §1). This spec is written against what exists today:
- **Server:** `crates/fn7-media` (see docs/MEDIA.md).
- **Client:** Benito's `FinalNight.uproject` (UE 5.8.4, Slate UI in `SFNRootWidget`, HTTP + WebSockets modules).

Status: **agreed, backend implemented** (2026-11-14). Benito approved it as proposed: no video for the jam (the
storyboard animatic instead), Benito owns the house style and the selection list, Chris decides real vs placeholder
images. The backend side of §3 and §4 is built and tested (§8 lists what changed in the details); the data contract is
in CLIENT_API.md (`visual`, `game.media`, `storyboard`) and MEDIA.md. Images stay on mock placeholders until Chris sets
a budget.

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
  `{hash, kind: portrait|item|event|statue|place|army|diorama, status: pending|ready|off, url|null, prompt}`. The places this happens:
  - big events: casts, eliminations, magnitude ≥ 8
  - demigod portraits in the public view, stable for the whole match
  - demigod dioramas in the public view (`diorama_prompt` → `diorama_image`): the miniature on a hex tile, for the
    table in front of each seat, the same in every match (§9)
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
             "place":    {"framing": "establishing shot", "size": "1536x1024"},
             "diorama":  {"framing": "the demigod as a collectible minifig standing on a small square themed diorama base that reflects its home region and shows off its signature prop, …", "size": "1024x1024"}, … },
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

## 7. Decisions
1. **Chris:** a real image provider and budget (or stay on placeholders for the jam). *Open: placeholders (mock) until
   Chris sets `FN7_IMAGES=openai` and a budget.*
2. **Benito:** the house style and the selection list in §4. *Decided: as proposed (`data/media_style.json`).*
3. **Both:** whether video phase 2 is in scope for the jam. *Decided: no; the animatic storyboard ships.*

## 8. As built (backend), and what changed from the proposal
- **Style config** `data/media_style.json`: exactly §4, plus two kinds the proposal didn't list, `army` and `item`
  (backend defaults; Benito may retune them like the rest). Loaded from `FN7_MEDIA_STYLE` / `data/media_style.json` /
  the compiled-in copy, validated at startup (a bad file stops the server, naming the field; keys starting with `_` are
  comments). Prompts are `<framing>: <subject>. Style: <house_style>. Avoid: <avoid>.` at the kind's size. Each match's
  ruleset records `media_style: {version, hash, source, body}`.
- **Selection:** `always` matches `events[].kind` (the ledger verb); `elimination` is accepted for `eliminate`.
  Every public event now carries `image_prompt` (a candidate); only selected ones get `image`. `max_per_match` caps
  moment cards only: storyboard shots and `game.media` requests are bounded by the budgets, not by this cap.
  `FN7_IMAGE_MAX_EVENTS`, if set, still overrides it.
- **Visual sheets:** `visual: {sheet_hash, traits: {build, face, outfit, colors: [a, b], prop, emblem}}` on demigods
  (PublicView, `game.globe`, `game.region`), on `game.person` and on `game.media` portraits. No separate "species"
  trait (everyone is human-shaped in the house style; `build` covers it). The sheet comes from the seed, kind, id and
  name only, so it never changes mid-match. Army banners use their demigod's colors and emblem.
- **`game.media`** is a regular game query (declared in `server.hello.api.queries`, public over HTTP for public refs):
  `{kind, ref}` → `{kind, ref, name, image, reason?, visual?}`. Rate limit: `FN7_MEDIA_RATE_PER_MIN` (default 30) new
  images per seat per match (the HTTP asker counts as one more "seat"); repeats are free. Over the rate = a
  `bad_request` error; over budget or images off = an `off` image with `reason`.
- **Storyboard** shots carry, beyond §3.3's fields, `shot` (index), `kind` (`title` | `moment` | `champion`),
  `image_kind`, `image_prompt`, `row` (a moment's ledger row) and `refs`. `image` is `null` when images are off.
- **Moment cards** never block a beat: annotation only enqueues (tested with a 3 s generator: the payload is built
  in well under 0.5 s).
- Image keys now include the size, and the prompt version is `fn7-media/2`: placeholders made before this release
  are not reused (they're free to remake).

## 9. House style v2: Benito's references (2026-11-14)
Benito stated via the bridge that the house style is his dark-fantasy references, not the collectible-figure look. Chris approved
it, with the final split into two looks chosen per kind (details, costs and the config in MEDIA.md, "Two looks" and
"Reference mode"):
- **Portraits:** Benito's dark-fantasy style (his DEFAULT_STYLE without the fixed keep-and-forest scenery, which put
  the same castle into every image), sent with six of his reference pictures and his note ("Match the reference images
  exactly in art style, …").
- **Everything else** (diorama, event, statue, army, place, item): the "max zoom" look. Final Night 7's hex-tile
  tabletop geometry (sparse low-poly miniatures, white pillar figures, voxel props) in the mood of his references,
  sent with four of his pictures. The Unreal table screenshots go first once they're captured: that is a data change in
  `references.geometry`.
- **The style config** gains `house_style_original` (his verbatim text, for him to choose), per-kind `style` and
  `references` overrides, `references: {geometry, mood, mood_per_request, note}` (files in `data/style_refs/`, checked
  at startup), `avoid` as one sentence ("No words or letters in the picture, no gore, no real people."), and
  `subject_rewrites`. Style `version` is `"2"`.
- **Generation:** with references on a gpt-image model, each image is a `POST /images/edits` call carrying the
  reference images. The reference set is part of the image key, so no collectible-style picture is reused. The prompt
  version is `fn7-media/3`.
- **Identity caching:** a demigod's sheet now depends only on its name (`fn7-visual/2`), and its portrait and diorama
  prompts carry only its identity (the diorama no longer names its home region). So each demigod, and each of its two
  hands (a player's two demigods, now with images of their own), is one image ever. The server prewarms the built-in AI
  roster at startup.
- **Queue:** portraits and dioramas start first, then statues, then the rest, then moment cards. Concurrency default
  is 6.
- **Spend:** an edits call costs about $0.023–0.030 with `gpt-image-1-mini` medium (the per-image price plus the
  input images, about 1,130 tokens each). Images stay off until the client displays them; nothing was generated for
  this change.


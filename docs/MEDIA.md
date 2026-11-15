# Images (`fn7-media`)

The server turns image prompts in payloads into pictures: **portraits** (demigods, memorized characters), demigod
**dioramas** (each demigod as a tiny geometric miniature on a hex tile: the table piece), **items**,
**events** (moment cards: the public events the style selects; god rulings), end-of-match **statues**, and, on request
by reference (`game.media`), **places** and **armies**. Every character has a stable **visual sheet**, so the same
person looks the same in every picture, and every match ends with a **storyboard** (the final-cut animatic). Generation runs in the background and never
holds up a beat. A payload carries a small image object at once; the picture arrives later, and clients learn it is
ready by polling a manifest.

**Images are off by default.** Real generation costs money and needs the designer's OK (see "Enabling" below).

## What a client sees

### In payloads

Wherever a payload object has an image prompt, the server puts an image object beside it:

| Prompt field (from the plugin) | Image object added | Kind |
|---|---|---|
| `image_prompt` | `image` | `image_kind` if present, else inferred |
| `statue_prompt` (in `match.ended.summary.piles[]`) | `statue_image` | `statue` |
| `portrait_prompt` | `portrait_image` | `portrait` |
| `diorama_prompt` (every demigod in the PublicView, `game.globe`, `game.region`) | `diorama_image` | `diorama` |
| a `look` or `look.changed` (state schema 11; the look inputs `base_portrait` …, see "Looks") | `portrait_image`, `diorama_image`, `pending` (the inputs are removed) | `portrait`, `diorama` |

Moment cards: an event object (in `events[]`, with a numeric `magnitude` and a `kind`) gets its `image` only if the style
config's `event_selection` picks it (below). Storyboard shots (`match.ended.summary.storyboard[]`) get theirs always.

```json
"statue_image": {
  "hash": "3f9c…64 hex…",
  "kind": "statue",                 // "portrait" | "item" | "event" | "statue" | "place" | "army" | "diorama"
  "status": "pending",              // "pending" | "ready" | "off"
  "url": null,                      // "/media/<hash>.<ext>" once ready
  "prompt": "A slightly larger faceted low-poly figure of the demigod on a raised hexagonal plinth …: the demigod Rook (…), … Style: Final Night 7's tabletop world: …. Match the reference images' palette, …. No words or letters in the picture, no gore, no real people."
}
```

- `status: "ready"`: `url` is set. Fetch it relative to the server's HTTP origin (the same host and port as `/ws`:
  `ws://host:8080/ws` → `http://host:8080/media/<hash>.png`; with ngrok, `https://<tunnel>/media/<hash>.png`).
- `status: "pending"`: queued or generating. Watch the manifest for `hash`.
- `status: "off"`: no picture will come (images off, over budget, over the per-match event cap, or the generator
  failed). Show your own placeholder. Don't retry.
- `hash` is the image's key: sha256 of the prompt version (`fn7-media/3`), the generator (provider, model, quality),
  the size, the prompt text and the sha256 of every reference image sent with it. A picture drawn on a base image (a gear portrait or diorama) also keys on the base image's key. The same prompt under the same generator is always the same hash and is generated once, ever (it is
  cached on disk across matches and restarts).
- `prompt` is the full text sent to the image model: `<Framing for its kind>: <subject>. Style: <the kind's house
  style>. <reference note> <avoid>`, all from the style config. It never contains engine ids.

The plugin sends `image_prompt` on every public event (and storyboard shot), so the interim rules below are off in
practice; they remain for older plugins. Until the plugin sends `image_prompt` fields of its own, the server adds images from what payloads already carry:
`statue_prompt` (statues), public `omens[]` (event images of the god rulings) and `demigods[]` lists (portraits from
the name). The interim omen and demigod images stop for good the first time an `image_prompt` appears. Unknown keys
were always to be ignored, so older clients are unaffected.

### `GET /media/index?since=<cursor>`: the manifest

```json
{ "enabled": true, "mode": "openai", "cursor": 1763049612345,
  "items": [
    { "hash": "3f9c…", "kind": "statue", "status": "ready", "url": "/media/3f9c….png", "ext": "png",
      "prompt": "…", "updated_at": 1763049612345, "cost_usd": 0.011 },
    { "hash": "a01b…", "kind": "event", "status": "refused", "url": null, "ext": null,
      "prompt": "…", "updated_at": 1763049612340, "reason": "event image cap reached (12 per match)", "cost_usd": 0.0 } ] }
```

- Lists every image the server has been asked for since it started whose entry changed after `since`, oldest change
  first. Start with `since=0`, then pass back the `cursor` you got. `updated_at` strictly increases, so nothing is
  missed or repeated.
- Manifest statuses are finer than payload statuses: `pending`, `ready`, `failed` (generator error, see `reason`),
  `refused` (budget, cap, unknown price, full queue; see `reason`). `failed` and `refused` show as `"off"` in payloads.
- Polling every 2–5 s while anything you're showing is `pending` is plenty. With images off it answers
  `{"enabled": false, "mode": "off", "items": []}`, so it is safe to poll unconditionally.
- It is per server process: a restart starts an empty manifest (the images stay on disk; payloads for them come back
  `ready` at once).

### `GET /media/<hash>.<ext>`

The image bytes. `ext` is `png` (OpenAI), `svg` (the mock), or `webp` / `jpeg`. Responses are immutable
(`Cache-Control: public, max-age=31536000, immutable`); cache them forever by hash. Both routes send
`Access-Control-Allow-Origin: *`. Unknown or malformed names answer 404.

The routes are served on the game server's own port (the Theatre's router is extended, alongside `/ws`, `/healthz`,
`/stats`), so they work through the same ngrok tunnel.

### The asset registry

Every image this layer is asked for is also written to the asset registry ([`ASSETS.md`](ASSETS.md)): SQL rows with
the exact prompt, the request parameters, timing, cost and token usage, and the file by the sha256 of its bytes
(hard-linked from this cache, so both paths serve the same bytes). The routes above are unchanged. A media `hash`
maps to its asset directly: `GET /api/assets/<hash>` (the row, its generations, lineage, uses) and
`GET /api/assets/<hash>/file`; the asset id is `ast_` + the first 24 hex of `sha256("media:" + hash)`. The seam is
`fn7_media::observe` (`Media::set_observer`): an observer is told at the same five moments the `media_*` events below
are written.

## The style config: `data/media_style.json`

Benito's lever (docs/SHARED_MEDIA_SPEC.md §4). The server loads `FN7_MEDIA_STYLE` (a path) if set, else
`data/media_style.json` in its working directory if it exists, else the copy compiled into the binary (whose reference
paths resolve against the repo's `data/` at build time).

### Two looks (style version 2, 2026-11-14)

Chris's decision: two looks, chosen per kind.

| Look | Kinds | House style | References (in order) | Note |
|---|---|---|---|---|
| **Benito's dark fantasy** | `portrait` | Benito's DEFAULT_STYLE without its fixed scenery: "a dark-fantasy world at night under a vast starry sky …, dreamy and eerie" | his six: IMG_3751, 3754, 3758, 3762, 3764, 3765 (the approved render's) | "Match the reference images exactly in art style, rendering, colour palette, lighting and level of detail; this is a new shot in that same world, not a copy of them." |
| **Max zoom (hex diorama)** | `diorama`, `event`, `statue`, `army`, `place`, `item` | "Final Night 7's tabletop world: an extremely sparse, geometric low-poly miniature diorama on a dark candlelit wooden table: … hexagonal tiles …, voxel blocks, … white pillars and cones standing in for people … Combined with the mood of the reference images: … PlayStation 2 era dark-fantasy, Dark Souls atmosphere …" | geometry refs first (the Unreal table screenshots, when added), then 4 mood refs: IMG_3751, 3754, 3762, 3765 | "Match the reference images' palette, lighting and atmosphere; keep the geometry extremely simple and the scale miniature, like a tabletop game piece; …" |

Approved example renders (not sent to the model) are kept for the record in `data/style_refs/approved/`:
`portrait.png` (dark fantasy), `maxzoom_diorama.png` and `maxzoom_event.png` (max zoom). All 15 of Benito's reference
pictures are in `data/style_refs/` (IMG_3751 … IMG_3765, 2.8 MB).

Why the portrait style has no scenery: a prototype showed that naming a fixed scene ("a burning stone keep … at the edge
of a misty pine forest") puts the same castle into every image, even "Manila harbour". Benito's full original text is
kept verbatim in the file as `house_style_original` for him to choose; it is never sent.

```json
{ "version": "2",
  "house_style": "Final Night 7's tabletop world: … dreamy and eerie",
  "house_style_original": "a dark-fantasy world at night: a burning stone keep of crumbling walls, … dreamy and eerie",
  "kinds": {
    "portrait": { "framing": "waist-up, three-quarter view, dramatic rim light", "size": "1024x1024",
                  "style": "a dark-fantasy world at night under a vast starry sky …",
                  "references": { "geometry": [], "mood": ["style_refs/IMG_3751.jpg", …6], "mood_per_request": 6,
                                  "note": "Match the reference images exactly in art style, …" } },
    "event":    { "framing": "seen from high above the hex-tile table: {subject}, rendered with blocky geometry, tiny white pillar figures for crowds, voxel props, candlelight and starry violet haze", "size": "1536x1024" },
    "statue":   { "framing": "a slightly larger faceted low-poly figure of the demigod on a raised hexagonal plinth atop a heap of blocky voxel item pieces, lit by candles and torches on the dark table", "size": "1024x1024" },
    "diorama":  { "framing": "the demigod as a tiny, extremely simple geometric miniature figure (…) standing on a single raised hexagonal diorama tile with its signature prop, three-quarter view from slightly above, centered on the dark candlelit table", "size": "1024x1024" },
    "place": {…}, "army": {…}, "item": {…} },
  "avoid": "No words or letters in the picture, no gore, no real people.",
  "subject_rewrites": [ { "from": "a collectible minifig", "to": "a tiny, extremely simple geometric miniature figure" }, … ],
  "references": { "geometry": [], "mood": ["style_refs/IMG_3751.jpg", "style_refs/IMG_3754.jpg", "style_refs/IMG_3762.jpg", "style_refs/IMG_3765.jpg"],
                  "mood_per_request": 4, "note": "Match the reference images' palette, lighting and atmosphere; …" },
  "event_selection": { "min_magnitude": 8, "always": ["cast", "elimination", "general_appointed", "mutiny"], "max_per_match": 12 } }
```

- **Prompts.** `<Framing>: <subject>. Style: <house style>. <note> <avoid>`, at the kind's `size`. A framing containing
  `{subject}` puts the subject there instead of after a colon (the event, place and army framings do). The house style
  is `kinds.<k>.style` if set, else `house_style`; the references and note are `kinds.<k>.references` if set, else
  `references`; no references = no note. `avoid` is a sentence sent as written (the shipped file) or a list (`Avoid: a, b.`).
  A kind the file leaves out falls back to the built-in framing.
- **Subject rewrites.** `subject_rewrites` (in order, case-insensitive) are applied to every subject before framing.
  They retire the plugin's older collectible-figure wording (the statue prompt's "amiibo-style collectible figure")
  without a plugin change. The payload's `prompt` shows the rewritten text.
- **Selection.** An event gets a moment card when its `magnitude` ≥ `min_magnitude` or its `kind` is in `always`
  (`kind` as in `events[].kind`: the ledger verb; `elimination` is accepted for `eliminate`). At most `max_per_match`
  moment-card images per match (`FN7_IMAGE_MAX_EVENTS` overrides it). Storyboard shots and requests by reference don't
  count against this cap (budgets still apply).
- **Validation.** At startup: unknown keys (typos) are errors except `_comment`-style keys starting with `_`; kinds
  must be known; sizes `WIDTHxHEIGHT` (64..4096 each); `min_magnitude` 0..10; every reference file must exist and be
  readable (`media style: data/media_style.json: references.mood[1] "style_refs/IMG_0000.jpg": cannot read reference
  image …`); a note must not be empty; at most 16 images per request. A bad file stops the server with the file and
  field named. The startup log line `media images=… style=… style_version=… style_hash=…` says which style is live.
- **Recorded.** Every match's ruleset carries `media_style: {version, hash, source, body}` (`hash` = sha256 of the parsed
  JSON, so reformatting doesn't change it; the per-kind styles and references are in it). A new framing, house style,
  note, avoid text, size or reference file changes every affected image's key (no stale picture is reused); retuning
  only `event_selection` keeps every cached image.

### Reference mode

With references configured and a gpt-image model, each image is one `POST /images/edits` call (multipart:
`model, prompt, size, quality, n=1, output_format=png`, `background=transparent` for diorama/statue/item, and one
`image[]` part per reference). Otherwise (no references, or `dall-e-*`) it is `POST /images/generations` as before; the
references' note stays in the prompt text but no images are sent.

- **Which references.** Every `geometry` image first, in order, then `mood_per_request` of the `mood` images, picked
  deterministically from the prompt (each mood image is ranked by sha256 of the prompt's kind, size and framed subject
  plus the image's sha256; the lowest win, kept in file order). The same prompt always gets the same references;
  different prompts spread over a larger pool. With the shipped lists the pool equals the pick (6 of 6 for portraits,
  4 of 4 otherwise); add more of Benito's pictures to a `mood` list to vary it.
- **Adding the Unreal geometry screenshots** (`data/style_refs/unreal/{table_wide,hex_close,globe_close,figures}.png`)
  is a data change: list them in `references.geometry` (paths relative to the style file). They then go first in every
  max-zoom request. Not added yet (the files don't exist yet).
- **Transparency.** Edits may refuse `background: transparent` on some models. Then the call is repeated once without
  it (recorded as two HTTP exchanges, the first with its 400) and the picture comes back opaque. The max-zoom framings
  put the piece on the dark table anyway.
- **Recording.** The HTTP record of an edits call holds the text fields and, for each `image[]`, `{file, bytes,
  sha256}`, never the image bytes and never the key (only the `authorization` header carries it, recorded as
  `Bearer <redacted>`). The response is redacted as before (base64 replaced by its length). `media_request` lists the
  reference file names (`refs`).

## Looks: gear portraits, gear dioramas and item art (state schema 11)

The designer asked where the equipment the demigods pick up is, and for portraits of each piece, made every so often
major acquisition for the user's characters but not for the ai's? ... so it feels like ur getting more and more stuff
with proper promptcrafting, caching for items etc etc so the time travel looks perfect".

Code: `crates/fn7-plugin/src/look.rs` (the rules, the state, the prompt templates; tests `look_tests.rs`),
`crates/fn7-media/src/annotate.rs` (the look pass), `crates/fn7-server/src/media.rs` (the library and the ready
events; test `fn7-server/tests/looks.rs`). Room setting `looks_enabled`; payload shapes in CLIENT_API.md "Gear looks".

### The loadout and its hash

A demigod's **loadout** is its notable gear right now, in a stable order:

1. its best item per slot, `weapon`, `armor`, `charm` (the 0.9.0 arms rule: the higher tier wins the slot, the newer
   of equals), then
2. up to `look_extra_items` (2) other major items, role `extra`: relic tier and up or god-blessed first (highest tier
   first), then the first item of each kind not listed yet. Extras are ordered by tier, then by descriptor.

Items lying in a loot pile and destroyed things never count; a deployed host (an army in the field) does. State
schema 15: neither does a thing that is not a real object, one whose name is empty or is a demigod's, a person's, a
player's or a god's (`look::real_object`, `World::names_entity`; such a thing gets no item art either). The live case
(match ae54b0af): the resolver wrote a find as "Bot Kestrel's Right Hand: finds a rusted fish-gutting knife among the
trout", the finder's name became the item's name, and Chris's Left Hand later wore "tucked into the belt, a bot
kestrel right hand". From schema 15 such a find is named after its object instead ("Rusted Fish-Gutting Knife";
`fn7_society::effects::thing_name`), see DEMIGODS_AND_ITEMS.md.

Each item is reduced to a **descriptor**: `kind | material | tier | name stem | blessing` (the arms rule's material for
gear, `""` otherwise; the stem is the name lowercased, possessives and punctuation dropped, leading articles dropped,
at most 8 words; the blessing is the god's slug). The **loadout hash** is sha256 of `fn7-loadout/1` and one line
`role|descriptor` per item, in order; `""` for an empty loadout. No item id, demigod id, seed or match enters it, so
the same gear is the same hash in any match (`look_tests::the_same_gear_is_the_same_hash_in_any_match_whatever_the_item_ids`).

### A major acquisition

At the end of every beat (after the DM's step and the gear's upkeep) `look::step` notes each demigod's loadout hash.
A **major acquisition** is a change of the loadout (a trinket that takes no slot and is not a first of its kind
changes nothing). For a **human-controlled**, standing demigod whose loadout is not the one its images show:

- it takes a new **look**: the portrait and diorama subjects with a gear clause for the loadout's first
  `look_items_shown` (3) items; a public `look.changed` is remembered (the DM's memory) and sent;
- **cap:** at most `look_images_per_demigod` (4) new gear pictures per demigod per match; beyond it the loadout keeps
  updating and the picture stays (`kept: "capped"`);
- **debounce:** at most one new picture per `look_debounce_beats` (3) beats per demigod; a change inside the wait is
  announced (`kept: "debounced"`) and when the wait ends ONE picture is made, of the newest loadout;
- a look worn before in this match (the same subjects) and the base look (all gear lost) are free: they count against
  neither limit;
- a fallen demigod keeps the look it fell with.

The built-in AI's demigods and Theatre bot seats never get a gear picture (their loadout is still listed in `look`). A
human who takes a seat over mid-match counts from then: at the end of the first beat after the takeover its demigods'
loadouts are pictured (under their new names).

### The prompts

The subject is the base identity subject **unchanged** (name, visual sheet, pose: so the face and build stay the same),
then a gear clause, then a closing instruction. Each piece is written place first, then a concrete noun, its material
in the house style's vocabulary, its glow by tier, and its god's signature when blessed. Never a number, a stat or an
id. Templates (`look.rs`: `worn`, `placed`, `phrase`, `material_look`, `tier_glow`, `signature`):

| Role | Portrait | Diorama |
|---|---|---|
| weapon | "gripped in the right hand, …" | "raised in its right hand, …" |
| armor | "strapped over the chest and shoulders, …" (a shield: "carried on the left forearm"; gauntlets: "worn on the forearms"; a helm: "worn on the head"; a cloak or shroud: "draped over the shoulders") | "as blocky plates over its torso and shoulders, …" |
| charm | "hanging on a cord at the throat, …" (a ring: "worn on the left hand"; a crown: "worn on the brow"; a horn, compass, lantern: "hanging from the belt") | "as a small bright bead at its chest, …" |
| extra | a weapon "slung across the back", armour "buckled at the hip", a relic "hanging from the belt", an army "planted behind the left shoulder, the tall war banner of …", a structure "on the horizon behind, the small silhouette of …", a creature "crouched at their side", a phenomenon "churning in the sky behind" | "at the tile's edge, the banner of …", "at the back of the tile, …: a small voxel building", "beside it, …" |

| Material | Words | Tier | Words | Blessing | Signature |
|---|---|---|---|---|---|
| obsidian | black volcanic obsidian glass with razor facets | trinket | plain and battered | Time | ringed by a thin halo of slow-turning clock hands of pale gold light |
| dragonscale | overlapping ember-red dragon scales | artifact | finely made, with a faint glow along its edges | Nature | wound with living vines and small green leaves |
| butterfly-wing | iridescent butterfly-wing membrane veined like stained glass | relic | ornate, glowing steadily | Consequence | studded with barbed iron thorns that glow dull red |
| platinum | pale mirror-bright platinum | wonder | radiant, wrapped in a halo of light | Death | inlaid with pale bone, a cold grey mist trailing from it |
| struck-oak | lightning-charred oak with glowing orange cracks in the grain | legend and up | blazing with light that spills over everything near it | Chance | set with dice-pip studs that flicker gold |
| moonsteel | cold blue-white moonsteel | | | Memory | etched with mirror-bright lines that glow soft blue |
| whalebone | carved ivory whalebone lashed with tarred cord | | | Entropy | split by hairline cracks leaking violet static |
| salt-glass | cloudy sea-green salt-glass | | | | |
| gold | beaten gold | | | | |
| heron-feather | long grey-white heron feathers bound in silver wire | | | | |

A full gear portrait prompt (the subject is from `look_tests::example_subjects`; the framing, style, reference note and
avoid text are the portrait kind's as today; the last sentence is the style's `base_note`):

> Waist-up, three-quarter view, dramatic rim light: Chris's Left Hand, a demigod: wiry and long-limbed, silver braids
> and a serene stare, wearing ornate plate armor over a hoodie in violet and jet black, carrying a golden abacus, with
> the emblem of a thorned rose; a larger-than-life figure with two glowing orchestrator hands floating at their sides
> and lightning crackling between their fingers; newly geared: gripped in the right hand, a blade of iridescent
> butterfly-wing membrane veined like stained glass, ornate, glowing steadily, wound with living vines and small green
> leaves; strapped over the chest and shoulders, plate armour of cold blue-white moonsteel, finely made, with a faint
> glow along its edges; hanging on a cord at the throat, a talisman of beaten gold, ornate, glowing steadily, inlaid
> with pale bone, a cold grey mist trailing from it; the same face, build, clothes and colours as before, with only
> this gear added. Style: a dark-fantasy world at night under a vast starry sky … dreamy and eerie. Match the reference
> images exactly in art style, rendering, colour palette, lighting and level of detail; this is a new shot in that same
> world, not a copy of them. No words or letters in the picture, no gore, no real people. The first reference image is
> this same character: keep its face, build, clothes, colours and pose family exactly as they are there and add only
> the gear described; the other reference images set the art style.

The diorama subject is the base diorama subject, then "the miniature now carries its gear as chunky faceted low-poly
pieces: raised in its right hand, a blade of …; as blocky plates over its torso and shoulders, plate armour of …; as a
small bright bead at its chest, a talisman of …; the same figure and tile as before, with only this gear added", in the
diorama kind's framing and the max-zoom style.

### The base image as the first reference

A gear picture is **drawn on the demigod's own base image**: the request's first reference image is the demigod's base
portrait (or base diorama), followed by the kind's style references as always, so the character stays recognisably the
same person (the existing reference mode, `POST /images/edits`). `Media::prompt_on(kind, subject, &base_prompt)` builds
it: the prompt's `base` is the base image's **key** and the style's `base_note` (a key of `media_style.json`; default
in `style.rs`) is appended to the text. The base is always asked for first, so it is ahead in the queue; the gear job
waits for the base file (up to 5 minutes) and fails, refunded, if the base was refused or failed (a later match tries
again).

### The cache key

`sha256(fn7-media/3 \n generator label \n size \n prompt text [\n ref:<sha256 of each style reference>] [\n base:<the
base image's key>])`. So a gear picture's key is (kind: in the text's framing; identity: the base subject in the text
and the base key; loadout: the gear clause in the text; style version: the style text and the reference files;
references: their hashes and the base key). The base's key, not its bytes, is used, so the key is known the moment the
look changes (the payload can name the pending hash at once). **Generated once, reused forever, across matches**: the
same demigod name with the same shown gear in a later match is a cache hit (`fn7-media` test
`a_gear_look_is_drawn_on_its_base_and_swaps_in_when_ready`).

### The pending swap

The plugin sends, in every `look` and `look.changed`, the media layer's inputs: `base_portrait`, `base_diorama`,
`portrait_subject`, `diorama_subject` (null for the base look) and `fallback` (its earlier gear looks, newest first, at
most 3). The look pass (`Media::annotate_seen`, before the generic annotation) replaces them with:

- `portrait_image`, `diorama_image`: **the image to show now**: the gear image if it is ready, else the newest earlier
  look's that is ready, else the base image;
- `pending: {loadout_hash, portrait_image?, diorama_image?}` while a gear image is queued or rendering (absent
  otherwise, also when it was refused or failed: then the previous images simply stay).

The server wrapper adds the event's second sending: a `look.changed` that went out with `pending` is sent again, with
`ready: true`, in the `looks[]` of the next public payload of that match once both images exist (kept in memory per
match, at most 64; dropped if an image fails). Clients may equally poll the manifest for the pending hashes.

### Item art

A notable item gets a picture of its own (`image_kind: "item"`: one object on one hex tile in the max-zoom style).
The subject is written from the descriptor alone, e.g. "a blade of iridescent butterfly-wing membrane veined like
stained glass: a demigod's weapon, ornate, glowing steadily, wound with living vines and small green leaves", so the
same descriptor is the same prompt, the same key and the same picture everywhere and forever. `art` in payloads is
sha256 of `fn7-item-art/1` and the descriptor.

Granted at the end of each beat, in item order, up to `item_art_per_match` (12) per match (the list is in the state):
an item a human player's demigod holds that is relic tier or up, god-blessed, or the first of its kind that player
holds; and a relic-tier or blessed item lying in a loot pile (on the board for everyone). An item whose descriptor was
granted shows the picture whoever holds it later. Every other item has no `image_prompt` since schema 11 and keeps its
`icon` (0.9.0 asked for a picture of every item, under a prompt that held the place and beat it was made).

### Budgets and costs

The existing budgets apply to every one of these images (`FN7_IMAGE_BUDGET_USD`, `FN7_IMAGE_MATCH_BUDGET_USD`, the
queue, the priorities: gear portraits and dioramas go in the first lane with the base ones, item pictures in the third).
The new limits are room settings, enforced by rules in the plugin (so they are in the state and replay exactly):

| Setting | Default | Limits |
|---|---|---|
| `look_images_per_demigod` | 4 | new gear pictures (a portrait and a diorama each) per human demigod per match |
| `look_debounce_beats` | 3 | beats between two new gear pictures of one demigod |
| `look_items_shown` | 3 | items one picture shows |
| `look_extra_items` | 2 | major items in the loadout beyond the three slots |
| `item_art_per_match` | 12 | item pictures per match |

Reserved per call with the default model (`gpt-image-1-mini` medium, 1024x1024, the [estimate] input rates above; one
more reference than the base images: the base itself): a gear portrait $0.011 + 7 x 1,200 x $2.50/1M + 600 x $2.00/1M
= **$0.0332**, a gear diorama (5 references) **$0.0272**, so **$0.0604 per look**; an item picture (4 references)
$0.0242. Worst case per match at the default caps: **$0.24 per human demigod, $0.48 per human player** (2 demigods x
4 looks), plus at most $0.29 of item art for the whole match (12 pictures). On the measured basis (1,128 tokens per
reference, about 450 text tokens) a look is about $0.058. Cached looks (the same name with the same gear again) and
the base look cost nothing; the mock generator costs nothing.

### The library

Each gear portrait and diorama of a human player's demigod goes into that player's library once it is ready
(registered when its `look` or `look.changed` goes out; again from the summary's look timeline,
`players[].demigods[].looks[]`, at match end), with the match, the beat it became the look and the loadout
(`library_matches.beat`, `.detail`; the library's `looks[]`).

## Visual sheets (character identity)

Every demigod and named person has a **visual sheet**: `visual: {sheet_hash, traits: {build, face, outfit, colors:
[a, b], prop, emblem}}`, picked deterministically from the match seed and the character's id and name (nothing that
changes in a match), so it is the same at every beat and in every replay. It appears on `demigods[]` in the PublicView,
`game.globe` and `game.region`, on `game.person`, and on `game.media` portraits.

Every image of a character is drawn from its sheet: a portrait is the sheet plus a pose; event images (moment cards,
storyboard moments) name who is in them with their sheets ("…; starring Vex (wiry and long-limbed, …, in teal and gold,
carrying …, with the emblem of a broken crown)"); statues carry the sheet; an army's banners use its demigod's colors
and emblem. A demigod's `portrait_image`, its `game.media` portrait and its storyboard close-up are one image.

**Dioramas.** Every demigod also carries `diorama_prompt` (with `diorama_image_kind: "diorama"`; its `image_kind`
stays `"portrait"`, the kind of `portrait_prompt`): its name and sheet as a tiny geometric miniature on a single hex
diorama tile with its signature **prop** built into the scene and its two colors as trim. It no longer names the home
region (identity only, below), so it is one image per demigod across every match (`diorama_image`, square), the same as
`game.media {kind: "diorama", ref: "dg:N"}`. The style config's `kinds.diorama` sets its framing and size.

**Identity: one image per demigod, ever** (sheet version `fn7-visual/2`). A demigod's sheet depends only on its name
(trimmed, lowercased), never the seed, its id or its `dg:N` ref, and its portrait and diorama prompts hold only the name
and the sheet (no home region, beat or health). So "Vex" is the same picture in every match, and the disk cache serves
it at once. Person sheets are unchanged (seed, id and name; their hashes keep `fn7-visual/1`).

**Hands.** A player's Left and Right hands are its two demigods: each hand in the seat view (`hands[]` in
`round.brief` per seat and `game.me` / the seat view) carries its own `visual`, `portrait_prompt` and `diorama_prompt`
(identity only, from its name, e.g. "Vex's Left Hand"), and the server adds `portrait_image` and `diorama_image` beside
them. The demigod's own portrait is unchanged. Hands appear only in private seat payloads, not in the public view.

**Prewarm.** With images on, the server queues, at startup, the portrait and diorama of every built-in AI demigod
(`fn7_plugin::phases::AI_NAMES`: Vex, Mara, Oko, Lune, Sable, Quill, Rook, Juno) and of both its hands, plus any names
in `FN7_MEDIA_PREWARM_NAMES`: 6 images per name, 48 for the roster. Already-cached images cost nothing. It logs `media
prewarm: N queued, M already cached`. It spends from a `prewarm` budget bucket, so `FN7_IMAGE_MATCH_BUDGET_USD` and
`FN7_IMAGE_BUDGET_USD` must both cover it (about $1.31 reserved for the roster with `gpt-image-1-mini` medium, see
the costs; about $0.16 per extra name).

## Images by reference: `game.media`

`game.media {kind: "portrait"|"diorama"|"place"|"event"|"army", ref}` (`diorama`: a `dg:` ref only) (WebSocket for a seat; `GET
/api/rooms/{code}/query/media?kind=…&ref=…` for public refs) answers at once with `{kind, ref, name, image, reason?,
visual?}`. The server writes the prompt from the state (a client never sends one), checks visibility (a seat sees public
things, its own people, people in its view and events it knows; HTTP sees public refs only), applies the per-seat rate
limit and the budgets, and only enqueues. The same ref always gives the same `hash`; repeats and cached images are free.
Over budget or with images off: an `off` image with a `reason`. Over the rate: a `bad_request` error (`rate limited: …
try again in N s`). Contract details in CLIENT_API.md (`game.media`).

## The storyboard (final-cut animatic)

`match.ended.summary.storyboard` is an ordered list of shots `{shot, kind, beat, at: [lon, lat], region, image_prompt,
image_kind, image, caption, duration_ms, camera: "globe"|"close", row, refs}`: a `title` shot (the final cut's title
over the arena), up to 10 `moment`s (the biggest public moments, eliminations first, in story order) and a `champion`
close-up with the final cut's closing lines. Built by the plugin from the state and the narrator's final cut
(deterministic); the server fills `image` (an event or portrait image object). A moment's image is its moment card's;
the champion's is its portrait. Video clips per shot (phase 2) are not built: they need Chris's budget.

## Enabling (the host)

| Variable | Default | Meaning |
|---|---|---|
| `FN7_IMAGES` | off | `off`, `mock` (free SVG placeholders, for client work) or `openai` (paid) |
| `OPENAI_API_KEY` | | Required for `openai`; without it images stay off |
| `FN7_IMAGE_BUDGET_USD` | `0` | The most ever spent on images in `FN7_MEDIA_DIR`. **0 = nothing is ever generated by `openai`.** What earlier runs spent counts (summed from the sidecars), so this is a lifetime cap for the cache directory |
| `FN7_IMAGE_MATCH_BUDGET_USD` | = the budget | The most one match may spend |
| `FN7_MEDIA_STYLE` | `data/media_style.json` | The style config (see above); invalid = the server won't start |
| `FN7_IMAGE_MAX_EVENTS` | the style's `max_per_match` (12) | Moment-card images per match |
| `FN7_MEDIA_RATE_PER_MIN` | `30` | New images one seat (or the public HTTP asker) may request by reference per minute per match; 0 = unlimited |
| `FN7_IMAGE_MODEL` | `gpt-image-1-mini` | Any OpenAI Images model (`gpt-image-1`, `dall-e-3`, `dall-e-2`, …) |
| `FN7_IMAGE_SIZE` | `1024x1024` | Only for a request without a size; the style config gives every kind its size |
| `FN7_IMAGE_QUALITY` | `medium` | `low/medium/high` (gpt-image), `standard/hd` (dall-e-3) |
| `FN7_IMAGE_PRICE_USD` | from the table | Price per image, **the whole call** (in reference mode it replaces the output price plus the input-image estimate); required for a model the table doesn't know (otherwise every request is refused) |
| `FN7_IMAGE_BASE_URL` | `https://api.openai.com/v1` | |
| `FN7_IMAGE_CONCURRENCY` | `6` | Generations in flight. Waiting jobs start by priority: portrait and diorama, then statue, then army/place/item, then event (first come, first served within a priority), so portraits never wait behind moment cards |
| `FN7_MEDIA_PREWARM` | on | `0`/`off`/`false` turns the startup prewarm off |
| `FN7_MEDIA_PREWARM_NAMES` | | Extra names to prewarm, comma-separated (e.g. `Chris,Benito`) |
| `FN7_IMAGE_QUEUE` | `256` | Queued generations; beyond it requests are refused |
| `FN7_IMAGE_TIMEOUT_S` | `120` | Per request |
| `FN7_MEDIA_DIR` | `runs/media` | The cache: `<hash>.<ext>` + `<hash>.json` |
| `FN7_MEDIA_URL_PREFIX` | `/media` | Prefix of `url` in payloads (set an absolute origin if clients can't resolve relative URLs) |

```
# free placeholders, to build the client against
FN7_IMAGES=mock cargo run --release -p fn7-server --bin fn7-server

# real images, at most $2 in all and $0.50 per match (needs the designer's OK)
FN7_IMAGES=openai OPENAI_API_KEY=sk-… FN7_IMAGE_BUDGET_USD=2 FN7_IMAGE_MATCH_BUDGET_USD=0.5 \
  cargo run --release -p fn7-server --bin fn7-server
```
The server logs its image mode at startup (`media images=…`).

**Budget enforcement.** A request is priced before it is queued (price table below, or `FN7_IMAGE_PRICE_USD`); if
the match's spend plus the price would pass the match budget, or the directory's lifetime spend plus the price would
pass the global budget, it is refused and never sent. The price is reserved while the request runs and refunded if it
fails. Cache hits are free. A match is identified by its recorder run id; with `THEATRE_RECORD=off` every match
shares one bucket, so the match budget then caps the whole process.

**Price table** (USD per image, OpenAI's published prices; check <https://openai.com/api/pricing> before relying on
them, they change):

| Model | 1024×1024 | 1024×1536 / 1536×1024 (or 1792) |
|---|---|---|
| `gpt-image-1-mini` low / medium / high | 0.005 / 0.011 / 0.036 | 0.006 / 0.015 / 0.052 |
| `gpt-image-1` low / medium / high | 0.011 / 0.042 / 0.167 | 0.016 / 0.063 / 0.25 |
| `dall-e-3` standard / hd | 0.04 / 0.08 | 0.08 / 0.12 |
| `dall-e-2` (256 / 512 / 1024) | 0.016 / 0.018 / 0.02 | |

**Estimated cost per match** (4 demigods: 4 portraits, 4 statues, and N event images, N ≤ `FN7_IMAGE_MAX_EVENTS`):

| Model, quality | 0 events | 12 events (the cap) |
|---|---|---|
| `gpt-image-1-mini` medium (default) | $0.09 | $0.22 |
| `gpt-image-1-mini` low | $0.04 | $0.10 |
| `gpt-image-1` medium | $0.34 | $0.84 |
| `dall-e-3` standard | $0.32 | $0.80 |

That is `(portraits + statues + N) × price`; portraits of returning names are cached across matches. More players or
memorized-character portraits add one image each.

**Reference mode (style version 2).** An edits call also pays for its input images. The budget reserves, per call,
the per-image price above plus `refs × 1,200` input image tokens and 600 input text tokens, at these rates
**[estimate]** (OpenAI's per-1M-token prices as we know them, not confirmed against the live pricing page):
`gpt-image-1-mini` $2.50 image input, $2.00 text input; `gpt-image-1` $10 and $5. Measured (prototype, `gpt-image-1-mini`,
1024x1024 medium): 1,128 image tokens per reference (4 refs = 4,512, 6 refs = 6,768), 260–340 text tokens, 1,056 output
tokens. After a call, the cost recorded (and charged against the budgets) is computed from the response's `usage`:
the per-image price plus the measured input tokens at these rates. `FN7_IMAGE_PRICE_USD` overrides all of it.

| `gpt-image-1-mini` medium | Refs | Reserved | Measured-basis |
|---|---|---|---|
| portrait (1024x1024) | 6 | $0.0302 | ≈ $0.0285 |
| diorama / statue / item (1024x1024) | 4 | $0.0242 | ≈ $0.0229 |
| event / place / army (1536x1024) | 4 | $0.0282 | ≈ $0.0269 |

A 4-player match where every identity is new (12 portraits and 12 dioramas: 4 demigods and their 8 hands; plus 4
statues) reserves about $0.75 before moment cards, and each moment card adds about $0.028 ($0.34 more at the 12-card cap). Prewarmed and returning
identities cost nothing (one image per identity, ever), so in practice a match pays mostly for its moment cards,
statues and human players' new names.

**Since the style config** (media release, before reference mode): event, place and army images are wide (`1536x1024`), so they cost the wide
price (`gpt-image-1-mini` medium: $0.015 instead of $0.011). The storyboard adds its title shot and any moment that had
no card (usually 1 to 4 images; eliminations and casts already have cards), and every `game.media` request for a new
image adds one. For the default model and the repo style, a 4-player match is about **$0.10** with no moment cards and
**$0.30** at the 12-card cap, plus whatever clients request by reference (bounded by the per-match budget).

## Recording

Every step is recorded into the match's run (the image work runs under the hook's tracing context):

| Event | When | Data |
|---|---|---|
| `media_request` | Queued | `hash, kind, provider, model, price_usd, prompt, refs` (reference file names) |
| `media_cached` | Already on disk | `hash, kind, provider, path` |
| `media_refused` | Over budget / cap / unknown price / queue full | `hash, kind, provider, reason, prompt` |
| `media_rate_limited` | A `game.media` request past the per-seat rate | `who, rate_per_min, retry_after_s` |
| `media_ready` | Stored | `hash, kind, provider, model, status, latency_ms, cost_usd, bytes, path` |
| `media_failed` | Generator error | `hash, kind, provider, status, latency_ms, error` |

Each OpenAI exchange is also an `http` record: the exact request body (no key; the `authorization` header is
redacted; for an edits call the text fields plus `{file, bytes, sha256}` per reference image instead of the multipart
bytes), status, response headers, latency, token usage when given, and the response body with the base64 image
replaced by its length. Image bytes never go into SQLite; the `path` points at the cache file.

## For the plugin

Put `image_prompt: "<subject>"` (and optionally `image_kind: "portrait" | "item" | "event" | "statue" | "diorama"`) on any
payload object that deserves a picture. Write only the subject (who, what, where; plain names, no refs); the server
adds the framing and house style and scrubs any `xx:N` ids. Without `image_kind` the kind is inferred: `statue` in
`piles`/`statues`, `portrait` under a key containing "portrait" or on an object with a `dg:`/`ch:` `ref`, `demigod` or
`character`, `item` on an `it:` ref, otherwise `event`. The plugin itself never calls the image service, so payloads
built by the plugin (and replays) stay deterministic; the server's `MediaPlugin` wrapper adds the image objects on the
way out and never touches the state.

Rust API (`crates/fn7-media`): `MediaStyle` (`style.rs`; `house_style_for`, `references_for`, `suffix_for`,
`References::pick`, `base_note`), `ImagePrompt::{styled, from_subject, restyle, on_base, portrait, item,
event, statue}`, `Media::{prompt, prompt_on, peek, request, request_with, request_by_ref, annotate, annotate_seen, manifest, style, wait_idle}`,
`ImageGenerator` (`MockGenerator`, `OpenAiImages`), `fn7_media::http::router`, `observe::{MediaObserver, MediaEvent, HttpCall}` and `Media::set_observer`. Plugin side (`crates/fn7-plugin`):
`visual.rs` (sheets), `payload::{event_subject, portrait_prompt, diorama_prompt, person_portrait_prompt, place_prompt, army_prompt}`,
`phases::storyboard`, the `media` query (`queries.rs`). Server side: `fn7-server/src/media.rs` (`MediaPlugin`: payload
annotation and `game.media` replies) and `FinalNight7Plugin::with_media_style` (the ruleset record).

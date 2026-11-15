# Assets: the registry (`fn7-assets`)

One place that knows every generated asset of Final Night 7: the pictures the server makes (portraits, dioramas,
moment cards, items, statue images), the reference images sent with them, and the 3D work done with Meshy (statues,
figures, the mannequin, its skins and clips). For each one: **the file**, **who or what it shows**, **every provider
call that made it with the exact prompt, parameters, timing and cost**, **what it was made from**, and **where it was
shown** (which match, which beat, on which demigod).

- **Files**: content-addressed, under `<THEATRE_DATA_DIR>/fn7/assets/` in subdirectories by kind.
- **SQL**: one SQLite database, `<THEATRE_DATA_DIR>/fn7/assets/assets.sqlite`, whose rows point at those files.
- **API**: `/api/assets…` (below). `/media/*` is unchanged.
- **CLI**: `fn7-assets` (backfill, import, inspect).

Code: `crates/fn7-assets` (`schema.rs`, `lib.rs`, `files.rs`, `live.rs`, `ingest.rs`, `provenance.rs`, `query.rs`,
`http.rs`, `bin/fn7-assets.rs`; tests in `tests.rs`), `crates/fn7-server/src/assets.rs` (the server side),
`crates/fn7-media/src/observe.rs` (the seam in the media layer). Tests: `fn7-assets` (`cargo test -p fn7-assets`),
`fn7-server/tests/assets.rs` (the real server, a played match).

`FN7_ASSETS_DIR` moves the root. `FN7_ASSETS=off` turns the registry off (no rows, no routes). `FN7_ASSETS_KEY` (else
`FN7_PROFILE_KEY`) gates the routes. If the registry cannot be opened at startup (a full disk, a database from a newer
build) the server logs the error and runs without it: the game and `/media` never depend on it.

## 1. Layout

```
<THEATRE_DATA_DIR>/fn7/assets/
  assets.sqlite  (+ assets.sqlite-wal, assets.sqlite-shm while open)
  images/portrait/<sha256>.png        a demigod's or person's portrait
  images/diorama/<sha256>.png         the table piece
  images/event/<sha256>.png           moment cards, storyboard shots
  images/item/<sha256>.png
  images/statue/<sha256>.png          the end-of-match statue PICTURE (the media layer's `statue` kind)
  images/place/…  images/army/…
  images/reference/<sha256>.jpg|png   style references sent with requests; heroic-pose and A-pose images made for Meshy
  images/preview/<sha256>.png         a provider's thumbnail of a model or skin
  models/statue/<sha256>.glb          a static posed statue of a demigod or god
  models/figure/<sha256>.glb          a rigged figure of a demigod
  models/mannequin/<sha256>.fbx       the shared duel mannequin
  textures/skin/<sha256>.png          one map of one mannequin skin (base_color, metallic, normal, roughness)
  animations/clip/<sha256>.glb|fbx    a clip, or the bundle of all clips
  incoming/                           (optional) where an operator drops a big file for `POST /api/assets/register`
```

- The file name is the **sha256 of the file's bytes** plus its extension. The directory is `<kind>s/<subkind>/`.
- A file is written once (to a temp name, then renamed) and never changed. Storing the same bytes again does nothing.
- Media-layer images are **hard-linked** from the media cache (`FN7_MEDIA_DIR/<media hash>.<ext>`), so the old path
  keeps working and the bytes are on disk once. Across volumes the link falls back to a copy. Everything else is
  copied in (on APFS a copy is a clone: no disk is used until one side changes).
- Several rows may point at one file (the 33 library clips of the mannequin live inside one bundle GLB).

## 2. Schema (version 2)

`PRAGMA user_version` holds the version; `fn7_assets::schema::migrate` brings an older database up, one transaction per
step, and refuses a database from a newer build. Times are unix milliseconds (UTC). JSON columns hold JSON text.

### `assets`: one row per asset version

| Column | Type | Meaning |
|---|---|---|
| `id` | TEXT PK | `ast_` + 24 hex. Derived from the asset's identity, so re-ingesting gives the same id: for a media-layer image `sha256("media:" + media hash)`; for an imported file `sha256("file:<sha256>:<kind>/<subkind>:<subject kind>:<subject key>:<variant>")`; for a recorded result with no file, from its task |
| `sha256` | TEXT | sha256 of the file's bytes. NULL until there is a file |
| `kind` | TEXT | `image` \| `model` \| `texture` \| `animation` |
| `subkind` | TEXT | `portrait` \| `diorama` \| `event` \| `item` \| `statue` \| `place` \| `army` \| `reference` \| `preview` (images); `statue` \| `figure` \| `mannequin` (models); `skin` (textures); `clip` (animations) |
| `subject_kind` | TEXT | `demigod` \| `god` \| `item` \| `place` \| `match` \| `shared` \| `person` |
| `subject_key` | TEXT | Who or what it shows. A demigod, god or person: the **normalized name** (the profile rule: NFKC, invisibles dropped, whitespace collapsed, lowercased, 40 characters; a trailing ` (2)` counter dropped first, as the visual sheet does). An item: its normalized descriptor. `match`: the match id (moment cards, armies). `shared`: a fixed key (`mannequin`, `style_ref:img_3751.jpg`, `unknown`) |
| `variant` | TEXT | Which one of several for the subject: `heroic` / `apose` / `style` (reference images), `wood/base_color` (skin and map), a clip name, `rigged` / `mesh`, `fbx`. Empty for the default |
| `display_name` | TEXT | The subject as written (`Chris's Left Hand`), or the start of a moment card's subject |
| `path` | TEXT | The file, relative to the assets root (`images/portrait/<sha256>.png`). NULL until there is a file |
| `bytes`, `mime` | INTEGER, TEXT | |
| `width`, `height` | INTEGER | Pixels (PNG, JPEG, SVG headers) |
| `tris` | INTEGER | Triangles (GLB: summed over the mesh primitives) |
| `created_at` | INTEGER | When the asset was made (the provider finished; else the file's time; else when first recorded) |
| `updated_at` | INTEGER | Last change of this row |
| `status` | TEXT | `pending` (asked for) \| `ready` (has its file) \| `failed` \| `refused` (budget, cap, queue: nothing was sent) \| `external` (made and recorded, but its file is not kept in the registry: a Meshy intermediate that was never downloaded, a reference image that was not at hand, an image whose cache file is gone) |
| `status_reason` | TEXT | Why refused / the failure / why external |
| `style_version`, `prompt_version` | TEXT | The media style config's `version` and `fn7-media/3`, when known |
| `media_hash` | TEXT UNIQUE | The fn7-media key (`GET /media/<hash>.<ext>`), for media-layer images |
| `source` | TEXT | `live` \| `backfill` \| `recording` \| `provenance` \| `job` \| `operator` (who wrote the row first) |
| `origin_path` | TEXT | Where the file was imported from, as given |
| `meta` | JSON | Anything else: `size`, a skin's `map`, a clip's `action_id`, notes |

Indexes: `media_hash` (unique), `(subject_kind, subject_key, kind, subkind, variant, created_at)`, `sha256`,
`(created_at, id)` (paging), `(kind, subkind, status)`.

### `generations`: one row per provider call

| Column | Type | Meaning |
|---|---|---|
| `id` | TEXT PK | `gen_` + 24 hex, derived from what identifies the call: `req:<media hash>:<match>` for the media layer (one request cycle of one image in one match; `refused:<media hash>:<match>` for a refusal), `http:<media hash>:<started_at>:<attempt>` for an earlier exchange of the same cycle, `task:<provider>:<task id>` for a provider task |
| `asset_id` | TEXT FK | The primary output |
| `provider` | TEXT | `openai` \| `meshy` \| `anthropic` \| `rules` \| `mock` (the free placeholder generator) |
| `model` | TEXT | `gpt-image-1-mini`, … |
| `operation` | TEXT | `images/edits` \| `images/generations` \| `image-to-3d` \| `text-to-3d preview` \| `text-to-3d refine` \| `retexture` \| `rigging` \| `animate` \| `mock` |
| `endpoint` | TEXT | The URL or route called, when known |
| `prompt` | TEXT | **The exact prompt text sent**, byte for byte. Never rewritten once set |
| `negative_prompt` | TEXT | The avoid text. For images it is the tail of `prompt` (`No words or letters …`), repeated here; for a provider with a separate field, that field |
| `params` | JSON | Every other parameter sent. Images: the request as recorded minus the prompt (`model, size, quality, n, output_format, background, image[]: [{file, bytes, sha256}]`), plus `price_reserved_usd`. Meshy: the task's parameters |
| `reference_asset_ids` | JSON array | The assets sent as references or inputs, in order |
| `provider_task_id` | TEXT | Meshy's task id; OpenAI's `x-request-id` |
| `requested_at` | INTEGER | Asked for (queued) |
| `started_at` | INTEGER | The provider call began |
| `finished_at` | INTEGER | The provider answered and the file was stored |
| `duration_ms` | REAL | The provider call |
| `cost_usd`, `cost_credits` | REAL | See "Costs" |
| `cost_basis` | TEXT | How the cost was arrived at: `usage` \| `price_table` \| `fixed_price` \| `sidecar` \| `credits_reported` \| `credits_recorded` \| `estimated` \| `free` \| `unknown` |
| `usage` | JSON | The provider's usage object (tokens), when kept |
| `status` | TEXT | `pending` \| `ok` \| `failed` \| `refused` |
| `error` | TEXT | The failure or the refusal's reason |
| `requested_by` | TEXT | `match:<id>` \| `prewarm` \| `operator` \| `unknown` |
| `requested_match`, `requested_beat` | TEXT, INTEGER | The match (the recording's run id) and beat whose hook asked |
| `requested_seat`, `requested_account` | INTEGER, TEXT | The seat that asked (`game.media`, a seat's view) and its account, when the request came from one |
| `source` | TEXT | `live` \| `sidecar` \| `recording` \| `provenance` \| `job` \| `operator` |
| `recorded_at` | INTEGER | When this row was first written |

Indexes: `asset_id`, `(requested_match, requested_beat)`, `provider_task_id`, `(provider, finished_at)`.

### `generation_outputs`: every file a call produced

`(generation_id, asset_id, output_role)`. A statue task gives a model (`primary`) and a thumbnail (`preview`); a skin
gives four maps (`base_color`, `metallic`, `normal`, `roughness`) and a preview; the rigging task gave the rig and the
walk and run clips (`member`). An asset's generations are the rows it is an output of.

### `lineage`: what was made from what

`(parent_id, child_id, role, note, created_at)`, primary key the first three. Roles:

| Role | Reads as |
|---|---|
| `reference` | the parent was sent along as a reference for the child (a style reference; the portrait behind a heroic image) |
| `source` | the child was made from the parent (heroic image → statue) |
| `retexture_of` | the child is a retexture of the parent (mesh → skin map) |
| `rig_of` | the child is the parent, rigged |
| `clip_of` | the child is a clip for the parent rig |
| `variant_of` | the child is another form of the parent (FBX of a GLB; a clip inside the bundle) |
| `preview_of` | the child is a picture of the parent |

So **portrait → heroic image → statue → preview** is one chain: `GET /api/assets/{id}/chain` from any link.

### `uses`: where an asset was shown or asked for

| Column | Meaning |
|---|---|
| `asset_id`, `match_id` | |
| `beat`, `last_beat` | The run of beats it was seen there: first and last. `-1` = a match-level record with no beat (the profile library) |
| `role` | The payload key it sat under (`portrait_image`, `diorama_image`, `statue_image`, `image`), or `requested` / `cached` / `refused` (the media layer was asked at that beat), or `library` |
| `subject_ref` | The engine ref in that match (`dg:0`, `hd:1`, `it:4`), when the payload gives one |
| `subject_kind`, `subject_key` | As in `assets` (the name's key), so a lookup by name needs no ref |
| `loadout_hash` | The gear the picture shows, when the payload says (`loadout_hash` on the image object or its owner, or `gear_hash`); a different loadout starts a new row |
| `seat` | The seat whose private payload showed it, when not public |
| `first_seen_at`, `last_seen_at`, `source` (`live` \| `recording` \| `library`) | |

One row per **run** of beats, not per beat: a portrait shown from beat 1 to 60 is one row (`beat 1, last_beat 60`);
when the demigod's picture changes at beat 20 a new row starts there, and if the old picture returns at beat 31 it
gets a new row from 31. Unique on `(asset_id, match_id, beat, role, subject_ref)`. Indexes: `(match_id, beat)`,
`(match_id, subject_key, beat)`, `asset_id`.

### `current_assets` (view)

The newest `ready` row per `(kind, subkind, subject_kind, subject_key, variant)`. **Nothing is overwritten**: a new
statue for the same demigod is a new row; the old one stays, its file stays, and `?current=1` or the view picks the
newest.

### `ingest_runs`

`(id, kind, source, started_at, finished_at, report)`: what each backfill or import run did (JSON counts).

## 3. Lifecycle

1. **Asked for** → `assets.status = pending`, a `generations` row `pending` with the prompt exactly as it will be
   sent, the reserved price, who asked (match, beat, seat, account).
2. **Refused** (budget, moment-card cap, unknown price, full queue) → asset `refused`, generation `refused` with the
   reason, cost 0. Nothing was sent to a provider.
3. **Failed** → asset `failed`, generation `failed` with the error and the time it took. Every HTTP exchange of the
   attempt is on record.
4. **Ready** → the file is linked into the layout; the asset gets `sha256, path, bytes, mime, width, height,
   created_at`, status `ready`; the generation gets `started_at, finished_at, duration_ms, cost_usd, cost_basis, usage,
   provider_task_id, endpoint, params`. An earlier exchange of the same generation (a refused transparent background, a
   429) is a `failed` row of its own.
5. **Served from the cache later** → no new generation; a `uses` row (`cached`) for the match and beat that asked.
6. **Shown** → `uses` rows as payloads go out.

Rules that hold everywhere: `ready` is final (a ready asset's file, hash and status never change); a generation that is
`ok` stays `ok`; the prompt is written once; a refused or failed image asked for again by another match goes back to
`pending` and gets a **new** generation row for that match (both attempts stay on record).

## 4. Ingestion

All three paths write the same rows through one function (`fn7_assets::live::apply`), keyed by derived ids, so they
can run in any order and any number of times.

### 4.1 Live

**What was touched in the media layer.** `fn7-media` gained one additive seam and nothing else changed shape:

- `crates/fn7-media/src/observe.rs` (new): `MediaObserver` (one method, `on_media(&MediaEvent)`), `MediaEvent`,
  `HttpCall`, and `collect` / `http`.
- `crates/fn7-media/src/lib.rs`: `Media::set_observer`, `Media::label`, an `observer` field, and one call at each of
  the four places that already write a `media_*` trace event (cached; requested or refused; ready; failed). The job
  runs its generator inside `observe::collect`.
- `crates/fn7-media/src/generator.rs`: the three `theatre_trace::http(rec)` calls became `crate::observe::http(rec)`,
  which records the exchange as before and also hands it to the generation being collected.

The trace events, the manifest, the cache, the sidecars, the keys and the routes are as they were. With no observer
attached (tests, tools) the media layer behaves exactly as before.

**In the server** (`fn7-server/src/assets.rs`, `main.rs`): the registry is opened before the prewarm;
`LiveHook` (the observer) copies each event and hands it to a writer thread, so a hook or the image queue never waits
on SQLite or the disk; `AssetsPlugin`, the outermost plugin wrapper, reads each outgoing payload after the media
wrapper has annotated it, finds every image object, and reports it with its owner's `ref`, `name` and loadout hash, the
beat and the match (`uses`). It also tells the observer which seat and account a hook is running for. It changes no
payload and no state, so recordings and replays are untouched.

A match needs a recording run id (`THEATRE_RECORD` on) to have a match id; without one, images are registered but have
no `uses`.

### 4.2 Backfill: `fn7-assets ingest`

```
fn7-assets ingest --data-dir <THEATRE_DATA_DIR> --media-dir <FN7_MEDIA_DIR> [--recordings <dir>] [--style-refs <dir>]
                  [--profiles <db>] [--no-media] [--no-profiles]
```

1. **Media cache**: every `<hash>.json` sidecar with its image → an asset (file hard-linked) and one generation from
   the sidecar: exact prompt, provider, model, created, latency, cost, the match that asked first.
2. **Recordings** (optional, `--recordings`): every `media_request` / `media_cached` / `media_refused` /
   `media_ready` / `media_failed` event and every image `http` record of each `*.jsonl` (and archived `*.jsonl.zst`,
   through `zstd`). Adds what sidecars do not hold: the **beat** that asked, the **reference images** sent (by file
   and sha256), the **request parameters**, the provider's **request id**, the **token usage**, the request time; and
   **refusals and failures**, which leave no file. Recordings are streamed and only read.
3. **Style references** (optional, `--style-refs <dir>`, the repo's `data/style_refs`): a recording names the
   reference images by file name and sha256 only; every file under the directory whose sha256 a waiting reference
   asset expects is copied in (`external` → `ready`). The live server does this by itself for new images.
4. **Profile library**: `library` × `library_matches` of the profile store → `uses` rows (`role = library`, match
   level). The store is opened read-only (copy it first if the server is running and you want no reader on it).

Images made outside any recorded match (the prewarm) have no recording, so they keep the sidecar's facts only.

### 4.3 External: Meshy

```
fn7-assets import-provenance <final_night_7_unreal>/production_assets/meshy
fn7-assets import-jobs <jobs.jsonl> [--root <dir>]
```

`import-provenance` reads `figures/figures.json`, `figures/<slug>/provenance.json` and `mannequin/provenance.json` as
they are written today:

- **A figure** (a demigod or a god): the portrait (the media layer's asset, found by its media hash, not duplicated);
  a likeness taken as it is (the asset that already has those bytes, e.g. the player's Left Hand portrait); the
  heroic / marble / A-pose reference images (OpenAI, exact prompt; the cost from the response's `usage` when the file
  kept it, else the file's own **estimate**, else unknown); the statue (Meshy, task id, credits, parameters, the GLB
  and its preview), made from the image the step names, else the index's `source_image`, else the image before it.
- **The mannequin**: the mesh (no file: `external`), the wood texture, the rig, the skins (every map a
  `textures/skin` asset), and the clips: each library clip a row at its share of the step's credits, walk and run as
  outputs of the rig task, all pointing into the bundle GLB unless a clip has a file of its own (`anims/<name>.fbx`);
  clips a later step adds live in the same bundle.
- Lineage throughout. Credits written as text (`"30"`) are read as numbers.
- **Checks**: every recorded sha256 is compared with the file; the report lists `hash_mismatches` (a file remade
  without its record being updated) and `missing_files` (registered `external`). Neither stops the import: the row
  always carries the hash of the bytes actually stored.
- Where the Meshy CLI's project folders still exist next to them (`<date>_<name>_<id>/metadata.json`,
  `task_<id>.json`; git-ignored in the Unreal repo), each task's **measured** start, finish and consumed credits are
  read too, and the clips get their task ids.

The directory is only read; files are copied in.

**Job records** (`import-jobs`): one JSON object per line, appended by a tool after each provider job. Lines starting
with `#` and blank lines are skipped. Paths are relative to the file's directory (or `--root`).

```json
{"v":1,"at":"2026-11-15T10:00:00Z","provider":"meshy","operation":"image-to-3d","task_id":"01a1…","status":"SUCCEEDED",
 "credits":30,"requested_at":1794698221145,"started_at":1794698221157,"finished_at":1794698319490,
 "endpoint":"/openapi/v1/image-to-3d","model":null,"prompt":null,"negative_prompt":null,
 "params":{"model_type":"standard","should_texture":true,"enable_pbr":true,"texture_resolution":"2k","target_formats":"glb,fbx"},
 "subject":{"kind":"demigod","name":"Chris's Left Hand","slug":"chris_left_hand"},
 "asset":{"kind":"model","subkind":"statue","variant":""},
 "inputs":[{"path":"figures/chris_left_hand/heroic.png","role":"source","variant":"heroic"}],
 "outputs":[{"path":"figures/chris_left_hand/chris_left_hand_statue.glb","role":"primary"},
            {"path":"figures/chris_left_hand/preview.png","role":"preview"},
            {"path":"figures/chris_left_hand/chris_left_hand_statue.fbx","role":"fbx","optional":true}],
 "lineage":[{"role":"rig_of","parent_task_id":"01a0…"}],
 "requested_by":"operator","tool":"tools/meshy/make_statue.sh"}
```

| Field | Meaning |
|---|---|
| `provider`, `operation` | Required |
| `task_id` | The provider's id: the record's identity (importing a line twice is a no-op) |
| `status` | `SUCCEEDED` (or absent) = ok; anything else = a failed job, kept on record with `error` |
| `credits`, `cost_usd`, `cost_basis` | What it cost. `credits` alone gets basis `credits_reported` |
| `requested_at`, `started_at`, `finished_at` | Unix ms or RFC 3339; `at` stands in for a missing one |
| `prompt`, `negative_prompt`, `model`, `endpoint`, `params`, `usage` | As sent |
| `subject` | `kind` (`demigod` \| `god` \| `item` \| `place` \| `shared`), `name`, `slug` |
| `asset` | `kind`, `subkind`, `variant` of the primary output |
| `inputs[]` | `{path, role}` (`source` or `reference`): matched to a known asset by content, else registered as a reference image |
| `outputs[]` | `{path, role, sha256?, optional?}`: `primary` is the asset (none = the result was not downloaded: the asset is `external`); `preview` an `images/preview`; others variants. `optional: true` = skip silently when the file is absent |
| `lineage[]` | `{role, parent_task_id}`: what the output is to an earlier task's output (`rig_of`, `retexture_of`, `clip_of`) |

**The tool side.** `docs/patches/meshy_job_records.patch` is a proposed patch for the Unreal repo's
`tools/meshy/make_statue.sh` and `make_figure.sh` (apply there with `patch -p1`; not applied by this repo): after each
job they append one record to `production_assets/meshy/jobs.jsonl` from the Meshy CLI's task file (status, consumed
credits, created / started / finished). Then `fn7-assets import-jobs production_assets/meshy/jobs.jsonl` puts every
job in SQL; importing the whole file again after each run is fine. A job with no primary output (the figure's
untextured model, never downloaded) is registered as `external`.

### 4.4 By hand: `POST /api/assets/register` (admin)

See the API.

## 5. `/media` hashes and asset ids

`/media/*` is unchanged: `GET /media/index`, `GET /media/<hash>.<ext>`, the image objects in payloads.

- A media **hash** is the image's *key*: sha256 of the prompt version, generator, size, prompt text and reference
  hashes. It names a request, not the bytes.
- The asset **id** is `ast_` + the first 24 hex of `sha256("media:" + <hash>)`: a pure function, so a client can compute
  it; or use the hash itself wherever an id goes: `GET /api/assets/<hash>`, `/api/assets/<hash>/file`,
  `GET /api/assets?media_hash=<hash>`.
- The asset's `sha256` is the hash of the **bytes**; `path` is by that. Every asset row of a media image carries
  `media_hash`, and its JSON carries `media_url` (`/media/<hash>.<ext>`) next to `file_url`.
- The profile library (`/api/profiles/{name}/library`) lists media hashes; each resolves the same way.

## 6. API

Like the profile routes: with a key configured (`FN7_ASSETS_KEY`, else `FN7_PROFILE_KEY`) every request needs
`?key=<KEY>` (403 without); every answer sends `Access-Control-Allow-Origin: *`; JSON answers are `no-store`. Errors:
`{"error": "not_found" | "invalid" | "unauthorized" | "store", "detail": "…"}`.

| Route | Answer |
|---|---|
| `GET /api/assets?subject_kind=&subject=&kind=&match=&beat=&status=&current=&media_hash=&sha256=&limit=&after=` | A page, newest first: `{items, next, limit, total}`. `subject` is normalized like a name. `kind` is a kind (`model`), a subkind (`portrait`) or `kind/subkind`. `match` (with `beat`) = shown or asked for in that match (at that beat). `current=1` = only the newest ready version of each. `limit` ≤ 500 (default 50); pass `next` back as `after` |
| `GET /api/assets/{id}` | `{asset, current, generations[], lineage: {parents[], children[]}, uses[], uses_total}`. `{id}` is an asset id or a media hash |
| `GET /api/assets/{id}/file` | The bytes. `Cache-Control: public, max-age=31536000, immutable`, `ETag: "<sha256>"`, `X-Asset-Sha256`, `Accept-Ranges: bytes`; `Range: bytes=a-b` → 206 (`416` when unsatisfiable); `If-None-Match` → 304. Streamed |
| `GET /api/assets/{id}/chain` | `{id, chain[]}`: every ancestor and descendant, oldest first, with `depth` (0 = the asset) and `parents` (`role:parent_id`) |
| `GET /api/assets/figures?name=` | What to put on a seat (below). Without `name`: every figure |
| `GET /api/assets/costs?by=day\|kind\|provider\|match\|subject\|basis\|model&usd_per_credit=` | Roll-ups (below) |
| `GET /api/assets/at?match=&beat=&demigod=` | Time travel (below) |
| `GET /api/assets/mine` | The assets of the signed-in account (`Authorization: Bearer <session token>`): its display name, its profiles, its demigods' names, and anything a seat bound to it asked for. Same paging and filters as the list |
| `GET /api/assets/stats` | Counts by kind and status, cost totals, and the known gaps (assets without a prompt, a cost, a use) |
| `POST /api/assets/register` | Admin (`Authorization: Bearer <THEATRE_ADMIN_TOKEN>`; 404 when the server has none; no key). Registers an operator-made asset |

An asset in any answer:

```json
{ "id": "ast_5d0c7e0f2a1b9c3d4e5f6a7b", "kind": "image", "subkind": "portrait",
  "subject_kind": "demigod", "subject_key": "chris's left hand", "display_name": "Chris's Left Hand", "variant": "",
  "status": "ready", "sha256": "9c8fd0a9…", "path": "images/portrait/9c8fd0a9….png", "bytes": 1435011,
  "mime": "image/png", "width": 1024, "height": 1024, "tris": null,
  "created_at": 1794696498123, "media_hash": "8ec91c54…", "style_version": "2", "prompt_version": "fn7-media/3",
  "source": "live", "file_url": "/api/assets/ast_5d0c7e0f2a1b9c3d4e5f6a7b/file", "media_url": "/media/8ec91c54….png" }
```

A generation (`GET /api/assets/{id}` → `generations[]`):

```json
{ "id": "gen_…", "provider": "openai", "model": "gpt-image-1-mini", "operation": "images/edits",
  "endpoint": "https://api.openai.com/v1/images/edits",
  "prompt": "Waist-up, three-quarter view, dramatic rim light: Chris's Left Hand, a demigod: wiry and long-limbed, … No words or letters in the picture, no gore, no real people.",
  "negative_prompt": "No words or letters in the picture, no gore, no real people.",
  "params": { "model": "gpt-image-1-mini", "size": "1024x1024", "quality": "medium", "n": "1", "output_format": "png",
              "image[]": [ { "file": "IMG_3751.jpg", "bytes": 166511, "sha256": "1632b3c9…" }, "…" ],
              "price_reserved_usd": 0.0302 },
  "reference_asset_ids": ["ast_…", "ast_…"], "provider_task_id": "req_d900e0f2…",
  "requested_at": 1794696489001, "started_at": 1794696489020, "finished_at": 1794696498123, "duration_ms": 9101.4,
  "cost_usd": 0.0285, "cost_credits": null, "cost_basis": "usage",
  "usage": { "input_tokens": 7045, "input_tokens_details": { "image_tokens": 6768, "text_tokens": 277 }, "output_tokens": 1056 },
  "status": "ok", "error": null, "requested_by": "match:1ccfab63-…", "requested_match": "1ccfab63-…",
  "requested_beat": 2, "requested_seat": null, "requested_account": null, "source": "live", "output_role": "primary" }
```

### `GET /api/assets/figures?name=Chris's Left Hand`

```json
{ "name": "Chris's Left Hand", "key": "chris's left hand", "found": true,
  "statue": { "id": "ast_…", "kind": "model", "subkind": "statue", "variant": "", "sha256": "c358e92d…",
              "bytes": 43081148, "mime": "model/gltf-binary", "tris": 1225996, "path": "models/statue/c358e92d….glb",
              "file_url": "/api/assets/ast_…/file", "created_at": 1794698319490 },
  "figure": null,
  "models": [ "…every current statue/figure file of this name…" ], "previews": [ "…" ],
  "fallback": null,
  "mannequin": { "models": [ { "variant": "rigged", "…": "…" } ], "clips": [ "…" ], "skins": [ "…" ] } }
```

`statue` is the current static statue (the GLB), `figure` the current rigged figure; either is `null` when there is
none. `found: false` comes with `"fallback": "mannequin"` and the mannequin's files, so a client always has something
to seat. Names are normalized, so any spelling of the name finds it.

### `GET /api/assets/costs?by=provider`

```json
{ "by": "provider", "usd_per_credit": null,
  "items": [ { "key": "meshy", "calls": 42, "ok": 42, "failed": 0, "refused": 0, "pending": 0, "cost_usd": 0.0,
               "cost_credits": 234, "usd_recorded": 0.0, "usd_estimated": 0.0, "ok_without_cost": 0,
               "duration_ms": 1203345.0, "avg_duration_ms": 28651.1, "first_at": 1794687610309, "last_at": 1794698380000 },
             { "key": "openai", "…": "…" } ],
  "total": { "calls": 1300, "…": "…" } }
```

`by`: `day` (UTC, the day the call finished), `kind` (`image/portrait`), `provider`, `model`, `match`
(`<match id>`, `prewarm`, `operator`, `unknown`), `subject` (`demigod:vex`), `basis`. `usd_per_credit=0.02` adds
`credits_usd_estimate` to each row.

### `GET /api/assets/at?match=<id>&beat=7&demigod=Vex`: time travel

"What did this demigod look like at beat N of match M": per picture kind (portrait, diorama, statue image) the asset
that was on that demigod at that beat, with its file.

```json
{ "match": "1ccfab63-…", "beat": 7, "demigod": "Vex", "key": "vex", "uses_in_match": 9,
  "items": [ { "role": "portrait_image", "subject_ref": "dg:0", "subject_key": "vex", "first_beat": 6, "last_beat": 9,
               "loadout_hash": "…", "matched": "covers_beat",
               "id": "ast_…", "subkind": "portrait", "sha256": "…", "path": "images/portrait/….png",
               "file_url": "/api/assets/ast_…/file", "media_url": "/media/….png", "media_hash": "…" },
             { "role": "diorama_image", "…": "…" } ],
  "models": [ "…the current statue / figure of that name…" ] }
```

- `demigod` is a name (normalized) or an engine ref of that match (`dg:0`, `hd:1`).
- `matched`: `covers_beat` (its run of beats includes the beat), `latest_before` (the newest one first seen at or
  before it: asked for a beat after the last one recorded), `earliest_seen` (asked for a beat before anything was
  seen), `latest` (no `beat` given).
- Without `demigod`: everything in effect at that beat of the match (every demigod's pictures and the moment cards up
  at that beat).
- `models` are per identity, not per match: the statue and figure as they are now.

### `POST /api/assets/register`

```json
{ "kind": "model", "subkind": "statue", "subject_kind": "god", "subject": "Consequence", "variant": "",
  "ext": "glb", "data_base64": "Z2xURg…",
  "meta": { "note": "sculpted by hand" },
  "generation": { "provider": "meshy", "operation": "image-to-3d", "task_id": "…", "credits": 30, "prompt": "…" },
  "lineage": [ { "parent": "ast_…", "role": "source" } ] }
```

→ `201` with the asset's detail. The file is `data_base64` + `ext` (up to 96 MB), or `"path": "<file>"` under the
assets root's `incoming/` directory. The same bytes for the same subject register once; different bytes are a new
version, and the newest is current. For many or large files prefer a job record and `fn7-assets import-jobs`.

## 7. Costs: what is measured and what is estimated

| `cost_basis` | Where the number comes from | How good |
|---|---|---|
| `usage` | An OpenAI `images/edits` call: the per-image **output price from the repo's price table** plus the **measured input tokens** (the response's `usage.input_tokens_details`: image and text tokens) at the repo's per-token rates | Tokens are measured; the prices they are multiplied by are the repo's table (`docs/MEDIA.md`), which is marked there as not confirmed against the live pricing page. Not a bill |
| `price_table` | A flat per-image table price (`images/generations`, or an edits call whose response had no usage) | An estimate from the table |
| `fixed_price` | `FN7_IMAGE_PRICE_USD` was set: that number per call | Whatever the host set |
| `sidecar` | Only the media cache's sidecar is left: the number the media layer wrote at the time, by one of the rules above. The usage that produced it was not kept (unless the recording is ingested, which upgrades the row to `usage`) | As good as the rule that wrote it |
| `credits_reported` | Meshy's `consumed_credits` from the task file, or a job record's `credits` | Measured (credits, not dollars) |
| `credits_recorded` | Credits written into a provenance file by hand at the time | Recorded, not re-checked |
| `estimated` | A provenance file's own estimate (`about $0.05 by the repo's price estimate`): the heroic and A-pose images, whose responses were not kept | An estimate; `usd_estimated` in the roll-ups keeps it apart |
| `free` | The mock generator; a refusal; a failed call | 0 |
| `unknown` | Nothing on record | NULL cost |

Credits are never turned into dollars in the database (`cost_credits` and `cost_usd` are separate columns);
`usd_per_credit` on the costs route applies a rate you choose at read time. Timing: `duration_ms` is the provider
call as timed by the server (live, recordings) or the sidecar's latency (the whole generator call, retries included);
for Meshy it is the task's own `finished_at - started_at`.

## 8. Retention and backup

- **Retention**: nothing is deleted. A refused or failed request keeps its rows. A superseded version keeps its row
  and its file. The media cache directory can be dropped once ingested only if nothing else reads it (the media layer
  still does: it looks its images up by key in `FN7_MEDIA_DIR`), so in practice keep both: they share the bytes.
- **A complete copy** is the assets root: `assets.sqlite` (copy it with `sqlite3 assets.sqlite ".backup <dest>"` while
  the server runs, or copy `assets.sqlite`, `-wal` and `-shm` together while it is stopped) **plus the whole file
  tree** (`images/`, `models/`, `textures/`, `animations/`). `rsync -a --hard-links` keeps the hard links to the media
  cache if both are copied together; copied alone, the tree is self-contained.
- `fn7-assets verify` checks every row's file: present, the recorded size, the recorded sha256.
- `GET /api/assets/stats` (and `fn7-assets stats`) sums `bytes` per row; rows that share one file (the clips inside
  the bundle) count it each time. `distinct_files` is the number of different files.
- The registry can be rebuilt from its sources (the media cache and its sidecars, the recordings, the profile store,
  the Unreal repo's provenance files and job records). What only the registry holds: operator registrations, and the
  `seat` / `account` / payload-level `uses` of live matches.

## 9. Running the backfill on the live server (at deploy)

Safe to run while the server is up (SQLite serializes the two writers) and safe to re-run. From
`final_night_7/` (the server's working directory; `THEATRE_DATA_DIR=runs/lan`, `FN7_MEDIA_DIR` default `runs/media`):

```
A="--assets-dir runs/lan/fn7/assets"

# 1. the media cache, the recordings, the style references, then the profile library  (about 20 s: hashes ~1.4 GB
#    once and hard-links it, streams ~8 GB of recordings)
fn7-assets $A ingest --data-dir runs/lan --media-dir runs/media \
    --recordings runs/lan/recordings --style-refs data/style_refs

# 2. the archived recordings (needs zstd on PATH), and the profile library again for what they added
fn7-assets $A ingest --data-dir runs/lan --media-dir runs/media --no-media --recordings runs/lan/recordings_archive

# 3. the Meshy work (copies ~270 MB in; on APFS as clones)
fn7-assets $A import-provenance ../final_night_7_unreal/production_assets/meshy

# 4. check: counts, costs, gaps; every file against its row
fn7-assets $A stats
fn7-assets $A verify
```

Run from a copy of the live directories on the evening of 2026-11-14 (22 raw and 66 archived recordings, a media cache
of 1,221 images), steps 1 and 2 took about 30 s and gave 4,185 assets: 1,227 with files (1,221 images and the 6 style
references), 2,912 refusals (moment-card cap, budget), 44 failures and 2 requests with no outcome; $19.96 of recorded
image cost over 864 paid calls (604 priced from their recorded token usage, 106 flat table prices from before reference
mode, 154 with only the sidecar's number); 5,091 `uses` rows over 84 matches. Run again, no row changed (the full
table dumps hash the same). Order matters only for completeness, not correctness: the profile library step links what
the registry already knows, so it runs last (each `ingest` runs it after its other steps).

The new server writes live rows from its first image on; the backfill fills in everything before that.

## 10. Known limits

- A request that never got an outcome (the server stopped mid-generation) stays `pending`; the backfill counts these
  (`no_outcome`) and does not guess.
- Prewarm images have no match and no beat (`requested_by = prewarm`), and no recording to enrich them from.
- `uses` from payloads need a match id (recording on).
- Subjects of moment cards are the match, not the demigods in them (the prompt names several).
- `import-provenance` understands the provenance files as they are written today; new tool runs should append job
  records instead.
- No Anthropic or rules-made assets are registered yet (the columns allow them).

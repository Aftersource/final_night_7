# Demigods, manifested items and persistent identity

Status: **rules 0.7.0 (state schema 5) wires the game side; the server side (`fn7-server`, `fn7-profile` steps 19–21,
24) is still open.** §0 says what was built and how it differs from this design; the rest of the document is the
original design, kept for its reasoning.

## 0. As built (rules 0.7.0)

The designer decided (2026-11-14): **two full demigods per player, each with its own health**; a shared health pool was
rejected. So phase 2 of §2 is what shipped, and its structure changed:

- **Two `Demigod` rows per seat, not a `HandDemigod` (`hd:`) sidecar.** Every rule that touches "the demigod" (Time's
  circle, armies striking, accidents, casts' area, Life prices, revive, Entropy flinging, possession, Consequence's
  bill) already iterates `World.demigods`, so two rows make all of them per body for free. `Demigod` gains `side`
  (`left`/`right`) and `player`; both rows share `seat`, `following` and `home`. Left rows keep the setup ids
  (`dg:0..n`), Right rows are appended (`dg:n..2n`), so every per-id table (gods' favor, crowds' allegiance) stays
  valid. Each row's `hands` is its own hand agent twice, and that agent's `AgentKind::Hand.demigod` is the row, so the
  left text drives the Left body. `fn7_society::split_hands` does it at setup. `HandDemigod` stays in `manifest.rs`
  unused, and no `hd:`/`mf:` `Ref` variants were added (`mf:N` appears only as a string in payloads).
- **Bolts: a per-player purse,** mirrored on both rows (`World::set_bolts`; every write goes through it). The gift is
  per rival *player*. **Debts: per body** (the acting body owes; Time's order debt is split between the bodies that got
  orders).
- **Survivor rule:** exactly one *player*: the most total standing (health minus outstanding debt), then start-of-beat
  health, then the lowest id. Lowest-debt-first (the old key) made players who had already lost a body win the Final
  Night (fewer bodies, less debt); "standing" fixed a measured seat skew.
- **Elimination reporting:** `seat.eliminated` only when both bodies have fallen; `seat.revived` when either stands.
- **Item engine:** wired in `fn7-plugin/src/items.rs` (§4 rules unchanged; prices × `manifest_scale`). Requests are read
  by rules (`is_manifest_act`, `request_from_text`); the hand prompt was **not** changed (a prompt change would break
  old recordings' LLM replay), so step 6 is open. The `manifest` mind (step 7) names and describes. Hosts muster
  `muster_size × manifest_muster_scale` (0.25). The bare-verb item fallbacks (§15) are fixed for schema 5
  (`items_by_engine`); creatures default to one beast, and rules names use the thing's own noun.
- **Answers to §14:** (1) separate health per body, the player out when both fall; (2) default names kept, proper names
  a later setting; (3) manifest bolts follow the universal bolt rule (rivals +2 per bolt); (4) the tier table and
  patrons as designed; (5) public from relic tier up (plus legend+ attempts).
- **Not done here:** fn7-server/fn7-profile wiring (steps 19–21, 24: another agent's crates), `ItemState` transfers and
  steals (no `Effect::ItemState` yet), `game.profile/history/library`.

Source of intent: BUILDLOG 2026-11-14, "demigods: two per player, items they manifest, persistent identity":

> two demigods per player, portraits on the lower left ... the demigods are picking up and manifesting things
> obviously all throughout, using our item engine or whatever likely to determine whether they were successful,
> raising armies banners of 100,000,000 armies who knows they're demigods, so all of these high level items must be
> created and stored, data structure per-match.. then sure, BETWEEN matches their items can reset to zero, but they
> have the history match history and etc etc etc personal descriptions image library match history time travel all
> of it.

> Cache them properly.

> Stop generating images that are not displayed; make a few and show how it's done.

Contents: 1 the shape · 2 two demigods per player · 3 per-match tables (schema 5) · 4 the item engine · 5 LLM
touchpoints · 6 payloads and queries · 7 the profile store · 8 match end: history and library · 9 HTTP · 10 privacy ·
11 integration plan · 12 the Unreal client · 13 backfill demo · 14 open questions · 15 findings along the way

---

## 1. The shape

```
                 one match (World, state schema 5)                       across matches (fn7-profile, SQLite)
 seat 0 ── dg:0 (the contestant: health, bolts, debts, standing)        profile "chris"
            ├── hd:0  Left demigod  "Ashkar"  ── ag:4 (hand mind)         ├── demigods: Ashkar, Velline (fixed looks)
            │     └── manifests mf:3 ──► it:7 "Banner of the Horde…"      ├── history: match rows (seat, placement, items,
            └── hd:1  Right demigod "Velline" ── ag:5                     │            deeds, eliminations, time travel)
                  └── manifests mf:4 (failed: no item)                    ├── description: player-written + auto
                                                                          └── library: portraits, dioramas, statues,
 items it:* reset every match (a new World)                                            moment cards (by media hash)
```

- **Per match**, everything lives in the `World`: the two hand demigods (`hd:`), every manifestation attempt (`mf:`)
  and every item (`it:`, extended). All of it is in the recording and replays byte for byte.
- **Across matches**, nothing in the `World` carries over. A separate store, keyed by the player's normalized display
  name, keeps who they are and what happened.

---

## 2. Two demigods per player

**Decision: the Left and Right hands become the player's two demigods (`HandDemigod`, ref `hd:`). The seat's existing
`Demigod` row (`dg:`) stays, as the contestant and the shared purse.**

Why not two `Demigod` rows per seat? Every rule, view, query, standing and test keys on one `Demigod` per seat
(`demigod_of_seat`, `SeatState.demigod`, standings, survivor rule, `selves`, debts' `owed_by`). Splitting it would
rewrite the game and redo the balance work (BALANCE_REPORT_1/2). A sidecar table, one row per hand, gives each demigod
its full identity with no change to the battle-royale math.

| | Phase 1 (proposed for schema 5) | Phase 2 (Chris decides; see §14) |
|---|---|---|
| Name, look, portrait, diorama | per hand demigod | same |
| Position on the globe, table presence | per hand demigod (starts at `dg`'s position, moves with its own acts) | same |
| Items and deeds | per hand demigod | same |
| Health, elimination | shared: `dg:` health; both fall together | each `hd` has `health`; the seat is out when both fall |
| Bolts, debts, Time's bill, standings | shared purse on `dg:` | same |

`HandDemigod` (`fn7-model/src/manifest.rs`): `id, demigod, seat, hand, agent (the hand's ag:), name, identity (the
normalized name), pos, place, home, alive, health (phase 2: Some), items, deeds {acts, manifests, manifested, top_tier,
soldiers, notable}, profile`.

**Names and looks.** A demigod's visual sheet is already a function of its normalized name alone (`visual.rs`,
`fn7-visual/2`), so the same name gives the same look, portrait and diorama in every match.

The media agent's work in the tree (BUILDLOG "house style = Benito's dark-fantasy references") already gives **each
hand its own sheet, portrait and diorama** in the seat view (`SeatView.hands[]`). They are keyed on the hand agent's
name, `payload::hand_names` = `"<player>'s Left Hand"` / `"<player>'s Right Hand"`, and prewarmed at startup. Schema 5
builds on that rather than replacing it:
- **Default names are exactly those**: `"Chris's Left Hand"` and `"Chris's Right Hand"`. So every portrait and diorama
  already cached or prewarmed stays valid ("CACHE THEM"), and the profile's identities line up with what the media
  layer makes.
- **A chosen name replaces the default.** The profile stores the player's pair (`identities` rows with `hand =
  left|right`), set from the client (§9, §14). A new name means a new look, once, and is then cached like any other.
- Names must be unique within a match (as now: a clash appends the seat). AI seats get `"<AI name>'s Left Hand"` and
  so on, which is what the prewarm already makes.
- What changes in schema 5 is that the hand stops being only a mind with a portrait. It becomes a body on the globe,
  with items, deeds and table presence, and goes public (in `PublicView.demigods[].hand_demigods`), so rivals see both
  minifigs.

**Deeds.** Ledger rows whose actor is the hand's agent or its subagents (`selves` already walks them), plus manifest
counts. `deeds.notable` keeps the 12 highest-magnitude rows.

---

## 3. Per-match tables (state schema 5)

`STATE_SCHEMA` goes 4 → 5. Every new field is `#[serde(default)]`, so schema-4 states and recordings still load.

| Table / field | Type | Ref | Notes |
|---|---|---|---|
| `World.hand_demigods` | `Vec<HandDemigod>` | **`hd:`** (new) | Two per seat, created at setup with the hands; index = id |
| `World.manifests` | `Vec<Manifestation>` | **`mf:`** (new) | Every attempt, made or not. Append-only |
| `World.items[i].ext` | `ItemExt`, `#[serde(flatten)]` | `it:` (unchanged) | kind, tier, count (f64), unit, state, manifestation, banner, image_prompt, at, place, muster, history |
| `SeatState.display_name`, `SeatState.profile` | `String`, `Option<String>` | | The seat's display name at setup and its profile key (for the summary's `players[]`) |
| `Meta.schema` | 5 | | |

**Prefixes checked.** In use: `dg ch cl gr gd pl it ag lg ce mb` (refs) and `mm in ru` (ids). `hd` and `mf` are free.
A test in `manifest.rs` asserts neither parses as a `Ref` today. `it:` stays the item ref, and manifested things *are*
items, so the piles, `present_in`, `visible`, the `item` image kind and `game.media` keep working.

### 3.1 JSON shapes

An item (schema 5; the first block is the existing `Item`):

```json
{ "id": 7, "name": "Banner of the Horde of 100,000,000", "description": "A copper-and-teal stag banner over a sea of spears",
  "origin": "summoned", "created_beat": 5, "created_by": "ag:4", "holders": ["dg:0"], "uses": 0,
  "kind": "army", "tier": "myth", "count": 100000000.0, "unit": "soldiers", "state": "deployed", "manifestation": 3,
  "banner": { "device": "a stag's head", "colors": ["copper", "teal"], "motto": "Ten thousand for every star" },
  "image_prompt": "A banner: a stag's head on copper and teal, over a horde of a hundred million…",
  "at": [2.35, 48.85], "place": 41, "muster": 212,
  "history": [ { "beat": 5, "state": "held", "holder": "dg:0", "row": 140, "count": 100000000.0 },
               { "beat": 6, "state": "deployed", "holder": "dg:0", "row": 151 } ] }
```

A manifestation (`mf:3`):

```json
{ "id": 3, "beat": 5, "by": "hd:0", "demigod": 0, "hand": "left", "agent": 4, "intent": 11,
  "request": { "kind": "army", "name": "", "count": 100000000.0, "magnitude": 7,
               "span": "raise banners of 100,000,000 soldiers", "offer_bolts": 0, "accept_debt": true },
  "place": 41, "at": [2.35, 48.85],
  "verdict": { "outcome": "success", "tier": "myth", "roll": 0.21,
               "odds": { "success": 0.448, "twist": 0.145, "partial": 0.5, "backfire": 0.098,
                         "terms": { "base": -0.49, "extra_bolts": 0.0, "debt": 0.0, "favor": 0.0, "stance": 0.0, "entropy": 0.28, "repeat": 0.0 } },
               "cost": { "required": 3, "bolts": 3, "extra": 0, "missing": 0, "debt": 0.0 },
               "god": 4, "favor": 0.0, "entropy": 35.0, "reasons": ["myth tier: 3 bolts", "Entropy loves this"], "flavor": "llm" },
  "item": 7, "rows": [140, 141], "public": true }
```

Counts are `f64`. Integers are exact up to 2^53 (9e15); `MAX_COUNT` is 1e15 and anything "countless" is that. JS and
Unreal JSON read doubles natively. Payloads round counts to integers.

---

## 4. The item engine

The rules decide. An LLM only writes words around the decision (§5), so the verdict is deterministic from the seed and
recordings replay exactly.

**Pipeline**, in the resolve, after the hands and before the merge:

```
hand intent (route "manifest") ─► ManifestRequest (hand mind's `manifest` field, or rules: request_from_text)
   ─► tier = classify(kind, count, magnitude)
   ─► cost(tier, purse bolts, offer, accept_debt)        ── refused if short and debt refused
   ─► odds(tier, cost, favor of the god, stance, Entropy, recent attempts)
   ─► roll = Rng::derive(seed, [MANIFEST_SALT, beat, mf id]).f64()  ─► outcome
   ─► proposals: Effect::Manifest (mf row + item if made), Bolts (payment), Debt (shortfall), Damage (backfire),
      Entropy (+0.4/bolt + entropy_rise(tier)), RaiseArmy-from-muster for a deployed host
   ─► flavor (LLM for tier ≥ Wonder or twists, batched; rules name otherwise)
```

### 4.1 Scale tiers

Hosts (army, fleet, creature) are tiered by `max(count tier, magnitude tier)`. Objects are tiered by magnitude, plus
one tier for ≥ 1,000 copies and two for ≥ 1,000,000.

| Tier | Host up to | Host word | Object example | Bolts | Base odds |
|---|---|---|---|---|---|
| `trinket` | 12 | squad | a dagger, a lamp | 0 | 0.97 |
| `artifact` | 1,000 | warband | a ring that hears lies | 0 | 0.90 |
| `relic` | 100,000 | army | Excalibur-class | 1 | 0.80 |
| `wonder` | 1,000,000 | host | a fortress overnight | 1 | 0.66 |
| `legend` | 10,000,000 | grand host | a wall across a nation | 2 | 0.52 |
| `myth` | **100,000,000** | horde | the designer's banners; a sky-bridge between continents | 3 | 0.38 |
| `titan` | 1,000,000,000 | world-host | a second moon | 4 | 0.25 |
| `apocalypse` | beyond | world-army | an army larger than humanity | 6 | 0.14 |

Magnitude → tier: 0–1 trinket, 2–3 artifact, 4 relic, 5 wonder, 6 legend, 7 myth, 8–9 titan, 10 apocalypse.

### 4.2 Cost

- **Bolts:** the tier's price, then any extra bolts the player offered, as far as the purse goes.
- **Debt for the shortfall** (the default; `accept_debt: false` refuses instead): `0.6 × missing × (1 + tier/4)`, owed to
  Consequence, due in `debt_due_beats`. For example, a myth two bolts short is 2.7 debt; an apocalypse six short is 9.9.
- Bolts are spent whatever the outcome. Only a refusal costs nothing.
- **Proposed** (§14): manifest bolts follow the universal bolt rule, so every rival gains 2 per bolt and Entropy rises
  0.4 per bolt. Entropy also gets `0.15 × (tier − relic)`.

### 4.3 Odds

```
x = logit(base[tier])
  + 0.35 × min(extra bolts, 4)          offering more
  − 0.15 × missing bolts                financed by debt (Consequence watching)
  + 1.2 × favor/10                      the god's favor toward the payer (−10..10)
  + stance                              invoked god's ruling: grant +1.0, bargain +0.3, twist 0, refuse → refused
  + 0.8 × entropy/100   (tier ≥ wonder) "Entropy loves superpowers"
  − 0.3 × min(recent, 4)                same hand demigod's attempts in the last 5 beats
success = clamp(σ(x), 0.02, 0.99)
twist    = min(0.04 + 0.30 × e, 0.5)    share of successes that are twisted (1.0 under a twist ruling)
partial  = 0.25 + 0.05 × tier           share of failures made smaller (a tenth of a host; one tier down for objects)
backfire = (0.05 + 0.25 × e) × tier/7   share of failures that hurt the payer (0.5 + 0.25 × tier damage)
```

The god whose favor counts is the invoked god, else the kind's patron: weapon or armor → Death, army or fleet →
Chance, structure or creature → Nature, relic → Memory, artifact, phenomenon or other → Entropy. This is data and can
change.

The draw `u ∈ [0,1)` splits as `[0, s(1−twist))` success, then `[.., s)` twisted. The failure span splits into its
first `partial` share (partial), its last `backfire` share (backfired), and failed in between.

**Worked examples** (P of each outcome; computed by the same functions the tests pin):

| Attempt | success | twisted | partial | backfired | failed |
|---|---|---|---|---|---|
| A sword (trinket), calm world | 0.931 | 0.039 | 0.008 | 0.000 | 0.023 |
| 100M horde (myth), paid, Entropy 35 | 0.383 | 0.065 | 0.276 | 0.054 | 0.222 |
| …two bolts short, on debt | 0.321 | 0.054 | 0.312 | 0.061 | 0.251 |
| …+4 bolts offered, Chance favor +5 | 0.733 | 0.124 | 0.072 | 0.014 | 0.057 |
| …Chance grants it, Entropy 80 | 0.547 | 0.213 | 0.120 | 0.043 | 0.077 |
| World-army (apocalypse), paid, Entropy 60 | 0.162 | 0.046 | 0.475 | 0.158 | 0.158 |
| A wonder fortress, 2 recent attempts, Entropy 10 | 0.498 | 0.038 | 0.186 | 0.015 | 0.264 |

### 4.4 Hosts in the simulation

The **narrative count is unbounded** (a horde of 1e8 is stored and shown as 100,000,000). The **simulation footprint
is log-compressed**: a deployed host spawns one army cluster of `muster_size(count)` people: linear to 50,000, then
`50,000 × (1 + ln(count/50,000))`, capped at 400,000. A million fights like about 200k, ten million like about 315k,
and a hundred million hits the cap. The existing army rules (march 900 km, clashes losing 8–16%, strikes capped at 2
damage) then hold, and balance doesn't explode. Losses scale back to the narrative count proportionally
(`ItemEvent.count`). The item's `muster` links the cluster. The general and captains come from the existing command
chain. The banner is the hand demigod's emblem and colors.

### 4.5 States

`held → deployed` (armies march, structures stand), `→ destroyed` (kept as a wreck; items are never deleted, as the
Final Night rule requires), `→ transferred` (given) or `→ stolen` (taken). Every change appends an `ItemEvent` with its
ledger row: the chain of custody. A new `Effect::ItemState { item, state, holder }` does it. A steal is an ordinary act
the resolver can grant.

---

## 5. LLM touchpoints and fallbacks

| Where | What the LLM does | Fallback (rules) | Budget |
|---|---|---|---|
| Hand mind (existing call) | Adds an optional `manifest: {kind, name, count, offer_bolts}` to an intent with route `manifest` | `request_from_text(span, magnitude)`: `classify_kind` keywords, `parse_count` ("100,000,000", "a hundred million", "1.5bn", "10k", "countless") | 0 extra calls |
| `manifest` mind (new, in the beat's one batch) | Names and describes a made item, writes a banner motto, and writes the twist for a twisted outcome. Only for tier ≥ wonder or a twist | `rules_name(kind, tier, count)` ("Banner of the Horde of 100,000,000"); description from the request span; no motto | ≤ 2 per beat (`manifest_calls_per_beat`, new setting) |
| Invoked god's mind (existing) | Its ruling becomes the `stance` (grant, bargain, twist, refuse) | The god's existing rules fallback | existing |
| Narrator (existing) | Big manifestations become caption material (they're ledger rows) | template captions | existing |
| Profile auto-description (later) | A two-line bio from the history | `fn7_profile::auto_summary` (built) | at match end, ≤ 1 per player |

The verdict, odds, cost and roll never come from an LLM.

---

## 6. Payloads and queries

### 6.1 Per beat

- `round.outcome.public.manifests[]`: public attempts (tier ≥ relic, or deployed): `{ref: "mf:3", by: "hd:0", name,
  hand, kind, tier, count, outcome, item: "it:7", at, region, image_prompt, image_kind: "item"}`. The media layer
  attaches `image` (the `item` kind already exists; tier ≥ relic and made items only, so we don't make images nobody
  displays).
- `round.outcome.per_seat[s].manifests[]`: the seat's own attempts, all tiers, with the full verdict (odds, cost,
  reasons).
- `round.outcome.public.events[]`: a `manifest` event kind for public attempts (moment-card candidates via the existing
  `event_selection`).
- `PublicView.demigods[]` gains `hand_demigods: [{ref: "hd:0", hand, name, pos, region, alive, visual, portrait_prompt,
  diorama_prompt, image_kind, items: <count of public items>}]`. The media layer then attaches `portrait_image` and
  `diorama_image` per hand demigod.
- `SeatView` gains `my_items[]`: the seat's items, full detail.

### 6.2 At match end

`match.ended.summary` gains `players[]`. `fn7-profile` reads this block first and falls back to the legacy
standings/piles/public-state derivation:

```json
"players": [ { "seat": 0, "display_name": "Chris", "profile": "chris", "demigod": "dg:0",
  "demigods": [ { "ref": "hd:0", "demigod": "dg:0", "hand": "left", "name": "Ashkar", "alive": false, "rank": 4,
                  "visual": {"sheet_hash": "…", "traits": {}}, "portrait_image": {"hash": "…"}, "diorama_image": {"hash": "…"} },
                { "ref": "hd:1", "demigod": "dg:0", "hand": "right", "name": "Velline", "…": "…" } ] } ]
```

Piles gain per-item `kind, tier, count, state, image` (`NotableItem` already reads them).

### 6.3 New `game.*` queries

| Query | Public | Args | Answer | Answered by |
|---|---|---|---|---|
| `game.items` | yes | `holder?` (`dg:`/`hd:`), `state?`, `min_tier?`, `cursor`, `limit` (20/100) | A page of items: public items for everyone; the asking seat's own items in full | plugin (`queries.rs`), from state |
| `game.item` | yes | `ref` (`it:N`) | One item with its manifestation (verdict, provenance, ledger rows) and its custody history. A private item is visible only to its holder's seat | plugin |
| `game.profile` | yes | `name?` (default: the asking seat's display name) | `Profile` | server wrapper (`MediaPlugin` or a `ProfilePlugin`), from `fn7-profile` |
| `game.history` | yes | `name?`, `page`, `per_page` | `Page<MatchRecord>` | server wrapper |
| `game.library` | yes | `name?`, `page`, `per_page`, `kind?` | `Page<LibraryImage>` | server wrapper |

The three profile queries aren't game state. The plugin stays pure, and the server's wrapper intercepts those names the
way it already intercepts `game.media`. Their answers are the same JSON as the HTTP routes (§9).

---

## 7. The profile store

**Key: the normalized display name.** The Theatre has no accounts. The resume token and `player_id` change every
session (see `run_start.meta.seats[].player_id`: a new UUID each match), so the display name is the only stable handle.
`normalize_name`:
1. NFKC
2. drop invisibles (controls, zero-width characters, bidi overrides, BOM, soft hyphen, tag characters)
3. trim and collapse whitespace
4. lowercase
5. keep at most 40 characters (as the plugin does)

So "Ｃｈｒｉｓ", " chris " and "CHRIS" are one profile.

**Collision and impersonation risk** (inherent until accounts exist):
- Anyone who types "Chris" plays as Chris and writes into Chris's history. Two different people named Chris share a
  profile.
- **Lookalikes:** `skeleton()` folds Cyrillic and Greek lookalikes, digits read as letters (0/1/3/4/5/7/8), i/l, rn/m
  and separators. A profile whose skeleton matches another's shows `lookalikes: ["Chr1s"]`, so the client can warn.
  Nothing is merged automatically.
- **Claims:** `POST /api/profiles/{name}/claim {secret}` (first come, first served; at least 6 characters; salted
  sha256). A claimed profile's description can only be changed with the secret. A claim doesn't stop anyone *playing*
  under the name. Making match records "verified" needs the client to present the secret when joining, which is an
  engine/lobby change (§14).
- The same name twice in one room: the plugin already renames the second ("Chris 1"). Each seat is recorded under the
  name the room had (`run_start.meta.seats[].display_name`), so a renamed duplicate lands in "chris 1".

**Storage:** SQLite (rusqlite 0.42, bundled, as `fn7-project` uses) at `$FN7_PROFILE_DB`, else
`$THEATRE_DATA_DIR/fn7/profiles.sqlite`. WAL journal, foreign keys on, migrations by `PRAGMA user_version`
(`schema.rs`; append-only list).

| Table | Key | Holds |
|---|---|---|
| `profiles` | `key` | display_name (latest spelling), skeleton, description_user, description_auto, claim salt/hash, created/updated |
| `identities` | `(profile_key, name_key)` | demigod name, hand, sheet_hash, traits, portrait/diorama hash, first/last match |
| `matches` | `match_id` | room code, times, outcome `ended`/`unfinished`, rounds, beats_total, rules_version, forked_from, champions, title, field, recording file, source |
| `match_players` | `(match_id, seat)` | profile_key, display_name, champion, placement, alive, eliminated_beat, cause, score, demigods/items/deeds/stats/eliminations (JSON) |
| `library` | `(profile_key, hash)` | kind, caption, subject ref and name, first/last seen |
| `library_matches` | `(profile_key, hash, match_id)` | which matches an image appeared in (a portrait spans many) |

**API** (`ProfileStore`): `open`, `open_in_memory`, `upsert_profile`, `profile`, `set_description(name, text,
secret?)`, `claim`, `record_match(&MatchInput)`, `record_match_with(.., include_bots)`, `add_image(name, NewImage)`,
`history(name, page, per_page?)`, `library(name, page, per_page?, kind?)`, `list`, `ingest_recording(path)`. Pure
helpers: `normalize_name`, `skeleton`, `auto_summary`, `ingest::read_recording`, `ingest::slices`.

**Idempotent per match id.** Recording a match again replaces its rows. An unfinished input never overwrites an ended
match, and an ended one upgrades an unfinished one. Library and identity merges are min/max. So the live path, a
startup catch-up and a manual backfill can all run over the same match safely.

**Totals** come from the history: matches, finished, unfinished, champion, best and average placement, falls, nemesis
(the cause that took them at least twice), and images.

---

## 8. Match end: history and library

**Live path** (wired, `fn7-server/src/profile.rs`, called from `MediaPlugin::summarize`): after the media layer has
annotated the summary (so image hashes are present):
1. The match id is `theatre_trace::current().map(|c| c.run)`: hooks run in the match's trace context, and the run id
   is the core's match id (what `/admin/matches/{id}` and `/admin/fork` take). With recording off there's no run, so
   the row gets a `local-<uuid>` id and no time travel.
2. The core's recorder is flushed and the match's own recording (`<THEATRE_RECORD_DIR>/<id>.jsonl`, no `run_end` yet)
   is streamed through `ingest::read_recording`. That gives the room code, start time, rules version, `run_start`
   seats and every `elimination`.
3. Overlaid on that: the hook's summary, `ended_at = now`, and `public_state = public_view(state)` (annotated, so
   portraits, dioramas and sheets are captured). Without a recording: `MatchInput::from_match_ended` with seats from
   `World.seats` (name = schema-5 `display_name`, else the seat's demigod's name).
4. `store.record_match(&input)` on a `spawn_blocking`, awaited, so the row exists before `match.ended` goes out. A
   store error is logged and never fails the match.

**Backfill at startup** (wired): when the store has no matches, or `FN7_PROFILE_BACKFILL=1` (`0` = never), the server
runs `ProfileStore::backfill_dir` in the background over `FN7_PROFILE_BACKFILL_DIR`, else the recordings directory.
It ingests every `*.jsonl` whose match id isn't stored as `ended` (one `SELECT` per file name; ended ones aren't
opened). Recordings are only read. 40 real LAN recordings (642 MB) took 416 ms in a debug build. A periodic catch-up
(every 10 minutes, for matches cut off by a crash) is not wired yet. `FN7_PROFILE_BACKFILL=1` at the next start covers
it.

**What lands in the library** (only images that exist as hashes in what clients were sent, so nothing new is ever
generated for the library):
- `portrait` and `diorama` of each of the player's demigods (from the public state's `portrait_image` and
  `diorama_image`)
- their `statue` (`piles[].statue_image`)
- `item` images of their items (schema 5 piles)
- every storyboard shot that shows one of their demigods (title card, moments, champion close-up), as the image's kind

Captions come from the shot caption or "<name>, portrait" / "<name>'s statue, <title> (<room>)". Images are fetched by
hash through the existing `GET /media/index` (`{hash, url}`) and `GET /media/<hash>.<ext>`.

**Media registering images directly** (wired): when `game.media` asks for a portrait or diorama of a `dg:`/`hd:` ref
owned by a human seat, the wrapper adds it to that player's library (`NewImage{hash, kind, caption "<name>,
portrait", subject, match_id}`) once the media layer marks it ready. A cached image goes in at once, a pending one via
a watcher that polls `Media::entry` (up to 15 minutes), and a refused or failed one never. `fn7-media` has no
ready-callback, and none was added. AI demigods and bots belong to nobody. The store dedupes per (player, hash,
match), so it doesn't matter whether that or the match-end pass gets there first.

**"CACHE THEM."** A demigod's portrait and diorama are identity-only prompts (already true), so one cached image per
name for as long as the style holds. The library shows how often that failed historically: six different portrait
hashes for "Chris" across builds as prompts and style versions changed (§13). Pin `FN7_MEDIA_STYLE` and bump prompt
versions rarely.

---

## 9. HTTP

Built in `fn7_profile::http::router(store, HttpConfig)` and merged into the server's router beside `/media` and
`/sim` (`serve_with`). Client-facing reference: CLIENT_API.md "Profiles, history and library".

| Route | Answer |
|---|---|
| `GET /api/profiles/{name}` | `Profile` (404 if none) |
| `GET /api/profiles/{name}/history?page=0&per_page=20` | `Page<MatchRecord>`, newest first |
| `GET /api/profiles/{name}/library?page=0&per_page=20&kind=portrait` | `Page<LibraryImage>`, most recently seen first |
| `PUT /api/profiles/{name}/description` `{text, secret?}` | `Profile`; 403 `claimed` if the secret is wrong |
| `POST /api/profiles/{name}/claim` `{secret, old_secret?}` | 204 |

- **Gating.** Through ngrok every request already carries `?key=<KEY>` (the host's gate). The router also checks
  `?key=` itself when `FN7_PROFILE_KEY` is set (set it to the tunnel key), so the LAN port 8080 is gated too. It returns
  `403` otherwise.
- Errors are `{error: bad_name|not_found|claimed|invalid|store, detail}`.
- Responses are `Cache-Control: no-store` with `Access-Control-Allow-Origin: *`, matching `/media`.
- Names are URL path segments, percent-encoded by the client and normalized server-side.
- Page sizes are clamped to 1..100.
- Time travel: each `MatchRecord.time_travel` holds `timeline`, `state` (with `{n}`) and `fork` paths. Those are the
  core's admin routes and need the admin token. The profile routes never proxy them.

---

## 10. Privacy: what is never stored or served

- No `player_id`, resume token, IP or session data. The ingest reads `display_name`, `seat` and `is_bot` from
  `run_start.meta.seats` and drops the rest. A test greps the JSON for `player_id`, `claim_hash`, `token` and the
  secret.
- Only what every seat saw at match end goes in: the public `match.ended` summary (standings, piles, storyboard, final
  cut) and the last public state. No `SeatView`, private memories, hidden orders, rivals' health mid-match, cut
  messages or window engagement.
- Claim secrets are stored only as salted sha256, and the API never returns them.
- Built-in AI demigods and Theatre bots get no profile (`record_match_with(.., true)` opts bots in).

---

## 11. Integration plan (after the balance and media agents finish)

Run each step's tests before the next. All changes are additive, and schema-4 recordings must keep replaying with 0
differences (`fn7-sim --replay`).

**fn7-model**
1. `refs.rs`: add `Ref::HandDemigod(HandDemigodId)` (`"hd"`) and `Ref::Manifest(ManifestationId)` (`"mf"`) to the enum,
   `Display`, `FromStr`, the `from_id!` list, the JSON Schema pattern (`^(dg|…|mb|hd|mf):\d+$`) and the round-trip
   test. `is_actor`: `hd` yes, `mf` no. Move `HandDemigodId`/`ManifestationId` into `ids.rs` via its `id!` macro, and
   delete `new_id!` from `manifest.rs`.
2. `lib.rs`: `STATE_SCHEMA = 5`; `pub use manifest::*` (resolve name clashes first: `Outcome`, `Cost`, `Flavor`,
   `Banner`, `Deeds` are new names, so check against `gods.rs`/`beat.rs` exports); add `World.hand_demigods:
   Vec<HandDemigod>` and `World.manifests: Vec<Manifestation>` (`#[serde(default, skip_serializing_if =
   "Vec::is_empty")]`); `World::name_of` arms for `hd` (the name) and `mf` ("a manifestation"); `groups_of` for `hd`
   (its demigod's following); `World::hand_demigod_of(seat, hand)`.
3. `society.rs`: `Item` gains `#[serde(flatten)] pub ext: ItemExt`, with a test that schema-4 items load (mirrors
   `item_ext_flattens_with_defaults`).
4. `SeatState` gains `#[serde(default)] display_name: String` and `profile: Option<String>`.
5. `intent.rs`: `Route::Manifest { request: ManifestRequest }`; `Effect::Manifest { manifestation: Manifestation,
   item: Option<NewItem> }` and `Effect::ItemState { item, state, holder: Option<Ref> }`.

**fn7-agents**
6. `hand.rs`: add `RouteKind::Manifest`, plus an optional `manifest: {kind, name, count, offer_bolts}` on `IntentOut`;
   the prompt gets one line ("manifest: conjure an object, structure, army or creature; give kind and count"). Fallback
   intents get route `manifest` when `classify_kind` isn't `other` and the verb is in {manifest, summon, raise, forge,
   conjure, build, create}.
7. New `manifest.rs` mind (`MindCall::Manifest`): input = request, tier, outcome and the hand demigod's sheet brief;
   output = `{name, description, motto?, twist?}`; fallback = `rules_name` plus the span. Recorded `llm_call` step
   `manifest`.
8. `resolver.rs`/`step.rs`/`god.rs`: their `CreateItem` paths become trinket- or artifact-tier items with `ext` filled
   by `classify_kind`, so old paths still make items, now with kinds. Stop making items from bare verbs (§15).

**fn7-society**
9. `effects.rs`: apply `Effect::Manifest` (push the `mf` row; create the item via `new_item` and set `ext`; link it to
   the hand demigod's `items`; ledger row verb `manifest`, magnitude from tier, `at` = place) and `Effect::ItemState`
   (append to `history`, move `holders`). For a deployed host: split no crowd, but **spawn** an army cluster of
   `muster_size(count)` with the existing army fields (general via the existing appointment rule) and set `ext.muster`.
10. `world_rules.rs`/`sim/armies.rs`: when a muster cluster loses people, scale the item's `count` by the same ratio
    and log an `ItemEvent`. When it dissolves, mark the item `destroyed`.

**fn7-plugin**
11. `phases.rs` setup: create two `HandDemigod` rows per demigod. Names come from the seat's profile pair if the
    server passes one (a room setting `hand_names: {seat: [left, right]}` the server fills from the store), else
    `payload::hand_names(player)` (the names the media layer already prewarms). `home` is the demigod's, and `pos` is
    its position with the two ~150 km apart. Keep the hand agents' `name` equal to the hand demigod's name, so the
    existing `SeatView.hands[]` images stay the same images. Fill
    `SeatState.display_name`/`profile` from `SeatInfo`.
12. New `items.rs`: `fn manifest(w, seat, intent) -> Vec<Proposal>` using `classify`, `cost`, `odds` and `roll` with
    `Rng::derive(seed, [MANIFEST_SALT, beat, next mf id])`, favor from `gods[god].favor[demigod]`, `recent` counted from
    `w.manifests`; plus `items_json` and `item_json` for payloads and queries. Call it from `beat.rs` where routes are
    dispatched (next to `Route::God`), before the minds batch, so the `manifest` flavor call joins the batch.
13. `payload.rs`: `hand_demigods` on public demigods (with `portrait_prompt_for(name)`/`diorama_prompt_for(name)`,
    already identity-only); `manifests[]` public and per seat; `my_items` on the SeatView.
14. `phases.rs` summary: `players[]` (§6.2); piles gain item `kind/tier/count/state/image_prompt`; statue prompts
    mention the biggest items ("atop a horde of 100,000,000 under a stag banner").
15. `queries.rs`: `game.items`, `game.item` (§6.3). Add `T::Manifests`/`T::HandDemigods` to the partial-state loader.
16. `table.rs` (`on_seat_takeover`): a taken-over seat keeps its two hand demigods, which turn human. Their `profile`
    becomes the joiner's, and their names stay for this match.
17. `settings.rs`: `manifest_calls_per_beat` (2, 0–16), `manifest_scale` (1.0, 0–10: multiplies tier prices; balance by
    settings, Play 3). `lib.rs` `MECHANICS`: a "Manifest" entry with the formulas in §4.
18. Tests: replay determinism with manifests; a 100M horde end to end (rules only, no LLM); schema-4 recordings still
    load and replay.

**fn7-server** (19 and 20 are done, see §8. The startup backfill makes step 24 automatic. 21 isn't done)
19. `Cargo.toml`: depend on `fn7-profile`. `main.rs`: `let profiles = Arc::new(ProfileStore::open(&default_path(&cfg.data_dir,
    env))?)`; merge `fn7_profile::http::router(profiles.clone(), HttpConfig::from_env(env))` into `extra`; spawn the
    catch-up task (§8) over `app.recorder` dir.
20. `media.rs` (`MediaPlugin`): hold `Option<Arc<ProfileStore>>`. Answer `game.profile`/`game.history`/`game.library`
    in `query` (default `name` = the seat's display name from state). After `summarize` (post-`mark`), call
    `record_match` (§8). Optionally `add_image` on `request_by_ref` for demigod portraits. Make sure the `tests/wrappers.rs`
    guardrail still passes (every hook forwarded).
21. `fn7-sim`: `--profiles <db>` to record simulated matches too (off by default).

**fn7-tools and docs**
22. `fn7-schemas`: schemas for `game.items`, `game.item` and the profile replies; regenerate `docs/schemas`.
23. `CLIENT_API.md` (manifests, hand_demigods, players[], the five queries, the profile routes), `DATA_MODEL.md`
    (schema 5), `PROJECT_GUIDE.md` §3/§5/§6, `MEDIA.md` (library registration). `fn7-project` projection: `fn7_items`,
    `fn7_manifests`.
24. Backfill the live store once: `fn7-profile --db $THEATRE_DATA_DIR/fn7/profiles.sqlite ingest runs/lan/recordings`.

---

## 12. What the Unreal client needs

1. **Lower-left portraits for the two demigods** (the designer asked for professional rounded borders and text boxes):
   - Two stacked cards, Left above Right, each in a rounded panel (~12 px radius, 2 px border in the demigod's first
     signature color, soft drop shadow, a dark translucent fill).
   - Each card holds a square portrait from `hand_demigods[].portrait_image` (placeholder silhouette while `pending`),
     a name plate in a rounded text box, a hand tag ("LEFT"/"RIGHT"), the shared health and bolts bar (phase 1), and an
     item count badge.
   - Clicking a card zooms the globe to that demigod and filters the item list to it.
   - Keyed by `visual.sheet_hash`, so art caches per identity.
2. **Minifigs on the table**: two per player (`diorama_image` per hand demigod), in front of each seat.
3. **Item list** (a drawer, from `my_items` plus `game.items`):
   - Rows show icon by kind, name, tier chip, count (grouped digits; "100,000,000 soldiers"), state, and the holder
     demigod.
   - Tapping a row opens `game.item`: provenance (who, when, where, verdict with odds and cost, ledger rows), custody
     history, and the item image if any.
   - Public manifestations get a toast and a banner sweep on the globe at `at`.
4. **History screen** (main menu and post-match):
   - Profile header: name, both demigods' portraits, description with an edit box, totals.
   - Match list from `/api/profiles/{name}/history`: date, room, placement/field, champion badge, demigods, notable
     items, deeds.
   - A library grid from `/library` with kind filter tabs.
   - A "Time travel" button per match that opens the admin scrub/fork. It needs the admin token, so it's for host builds
     only (§14).
5. **Lookalike warning**: if `lookalikes` isn't empty, show "another player uses a similar name".

---

## 13. Backfill demo (2026-11-14)

Command: `fn7-profile --db <scratch>/profiles.sqlite ingest runs/lan/recordings`. It's read-only and streaming; 39
files (≈ 0.6 GB) took **0.45 s**, because only the `run_start`, `ruleset`, last `round.committed`, `elimination` and
`run_end` lines are parsed. Result: **22 ended, 17 unfinished** (no `run_end`: server stops, or matches still running
during the scan). 15 profiles; the rest are test names (`globe`, `ResumeTest`, `ImageCheck`...).

| Profile | Matches | Finished | Unfinished | Champion | Best / avg placement | Falls | Library |
|---|---|---|---|---|---|---|---|
| **Chris** | 7 | 2 | 5 | 1 (`3f4693ae`, 3-beat solo vs 3 AI) | 1st / 2.5 | 1 | 10 (6 portrait, 1 diorama, 2 statue, 1 event) |
| **Benito** | 14 | 5 | 9 | 0 | 2nd / 2.4 | 5 | 18 (8 event, 5 portrait, 5 statue) |

- **The shared match `0f8ba721` (room DYL5GN, 21 beats, champion Vex)**:
  - Chris: seat 0, 4th of 4, fell at beat 17 to "a debt collected by Consequence".
  - Benito: seat 1, 2nd of 4, fell at beat 21 to the same cause.
  - Both rows carry the time-travel paths to `0f8ba721…`.
- **Benito's finished matches**: 2nd (`aa614e88`, Vex), 3rd (`c309e021`, Oko), 2nd (`da704b0d`, Vex; 5 deeds from the
  storyboard), 2nd (`0f8ba721`, Vex) and 3rd (`a0508d3c`, Oko).
- **Benito's causes of death**: Time's clock, "Scorched sky over Kyiv", a slowed-world bill, the Final Night and
  Consequence's debt. No cause repeats, so he has no nemesis yet.
- **Auto-descriptions**:
  - Chris: 7 matches (2 finished, 5 unfinished), champion 1 time.
  - Benito: 14 matches (5 finished, 9 unfinished). Best finish: 2nd. Recently carried: Lever, Benito, Lever
    Two, umbrella."

---

## 14. Open questions for Chris

1. **Phase 2 health:** do the two demigods share one life (phase 1, no balance change), or each have health, with the
   seat out when both fall?
2. **Names:** the defaults are the current "<player>'s Left Hand" / "Right Hand" (already cached and prewarmed). Do
   players pick real names for them (in the lobby, stored in the profile), and can they rename later (a new name means
   a new look and one new pair of images)?
3. **Bolt rule for manifesting:** should manifest bolts give every rival +2 like casts and gods (proposed), or be a
   toll like Time's (no gift)?
4. **Odds and prices:** are the tier table (myth = 3 bolts, base 0.38) and the formula right, and are the
   god-of-each-kind patrons right?
5. **Visibility:** is an attempt public from relic tier (100k soldiers, or a legendary object) up, while smaller ones
   stay private to the seat?
6. **Identity security:** is name-keyed plus an optional claim secret enough for now? Verified records would need the
   client to send the secret at join (a lobby or engine field). Do we want that?
7. **Time travel from the client:** the scrub and fork routes need the admin token. Host-only, or a safe public
   read-only scrub (`view=public`) proxied by the profile routes?
8. **Test names:** should profiles like "globe", "ResumeTest" and "ImageCheck" be purged, hidden, or kept?
9. **AI demigods:** should Vex, Mara and the others get public "legend" profiles (their own histories), or stay
   profile-less?

---

## 15. Findings along the way

- **Items made from bare verbs.** Existing recordings have items named "search", "take", "Benito" and "Lever Two". The
  `ResKind::Item` / `ActionKind::Item` fallbacks in `fn7-agents` (`resolver.rs`, `step.rs`) turn an act's text into an
  item name. The integration (step 8) should create items only on a real manifest intent.
- **Portrait churn.** "Chris" has six different portrait hashes across recordings, and Benito has five: prompt and style
  versions changed between builds, so the identity cache missed each time. "CACHE THEM" depends on keeping prompt
  versions stable (§8).
- **Unfinished matches are common** (17 of 39). The catch-up ingest (§8) keeps them in history, marked `unfinished`,
  with time-travel links, and upgrades them if they later end.

# Brief: art, images and Meshy

*For any session making pictures, statues, figures, skins or style references. Designers: Chris and Benito; Benito
owns style and the event camera (BUILDLOG 2026-11-15). Read `../FULL_EXPLAINER.md` first; this is your subset.*

## 1. The two styles (Benito's rule)

Benito, 2026-11-15: there need to be **two styles**: the realistic statues and everything in the god's point of view
real and realistic, and the game board and the Earth feel like **the gods' toys**, playing with the world.

| Space | Style | What lives there |
|---|---|---|
| **The god room** (realistic) | real materials, real light | the gods' stone statues on pedestals (the god form), name plates, the table itself, the floating globe |
| **The board and the Earth** (the gods' toys) | miniature, tabletop, game pieces | hex tiles, demigod minifigs, people tokens, houses, armies, disasters, the duel box with its mannequins |

Do not merge the two looks.

## 2. The house style for generated images (`data/media_style.json`, MEDIA "Two looks")

Chris's decision (style version 2): two looks, chosen per image kind.

| Look | Kinds | Style |
|---|---|---|
| **Benito's dark fantasy** | `portrait` | "a dark-fantasy world at night under a vast starry sky ..., dreamy and eerie"; matched to Benito's six reference pictures. No fixed scenery (a named scene put the same castle into every image) |
| **Max zoom (hex diorama)** | `diorama`, `event`, `statue`, `army`, `place`, `item` | "Final Night 7's tabletop world: an extremely sparse, geometric low-poly miniature diorama on a dark candlelit wooden table: ... hexagonal tiles ..., voxel blocks, ... white pillars and cones standing in for people", with the mood of PS2-era dark fantasy |
| Library | `sprite`, `building` | the house palette and mood (cobalt and violet with orange candlelight), detailed individuals on a plain or transparent background |

Every prompt avoids: "No words or letters in the picture, no gore, no real people." The server writes every prompt
from the state (a client never sends one). Approved example renders are in `data/style_refs/approved/`; Benito's
reference pictures in `data/style_refs/`.

## 3. Image kinds and where they show

| Kind | What | Notes |
|---|---|---|
| `portrait` | each demigod, waist-up, three-quarter, rim light | stable all match |
| `diorama` | the demigod as a tiny geometric figure on one raised hex tile | the table piece |
| `event` | moment cards for big public events | by default magnitude >= 8, or a cast, elimination, general appointed or mutiny; at most 12 per match; never holds up the beat |
| `statue` | the end-of-match piles: the demigod on a plinth atop its items | `match.ended.summary.piles` |
| `army`, `place`, `item` | armies, places, notable items | item art only for granted items (relic+, god-blessed, or a human's first of its kind), up to `item_art_per_match` (12) |
| gear looks | a new portrait + diorama when a major item is acquired (`look.changed`) | `look_images_per_demigod` 4, `look_debounce_beats` 3 |
| `sprite` (48) / `building` (24) | 16 roles x 3 looks; 8 kinds x 3 looks | one picture per key, cached forever |
| storyboard | title shot, up to 10 moments, champion close-up | video per shot not built (needs Chris's budget) |
| god preview | the god form's preview picture | up to 6 new pictures a day per account |

Every character is drawn from its **visual sheet** (`visual.traits`: build, face, outfit, colors, prop, emblem), the
same in every match and replay. Images are cached by hash and never change.

## 4. Meshy: only through the backend and the asset registry

- **Every Meshy call goes through the backend into the asset registry** (BUILDLOG 2026-11-15; `docs/ASSETS.md` §4.3,
  §4.4). Nothing is spent without Chris or Benito saying so (FOR_BENITO).
- **God statues:** on accepting the god form, one Meshy image-to-3D statue per account (live; model "latest", remeshed
  to 30k triangles). The client puts the accepted picture on the pedestal at once, then builds the statue at runtime
  in the chosen stone.
- **Figures and the duel mannequin:** heroic statues for Chris's and Benito's Right Hands and one demigod per default
  AI (BUILDLOG); the duel mannequin, its five skins (wood, obsidian, gold, dragonscale, butterfly) and its fight clips
  came from Meshy (FOR_BENITO). A new skin costs about 10 credits.
- Provenance: `production_assets/meshy/` in the Unreal repo (`figures.json`, per-figure `provenance.json`, the
  mannequin's), imported with `fn7-assets import-provenance` / `import-jobs`. Every job records its credits.

## 5. Budgets (as of the 17:00 EST session log, 2026-11-15)

- Image budget: $40 set, about $38.5 spent: nearly exhausted. Raising it is Chris's call.
- Meshy credits: about 229 left.
- Per-call reservations (MEDIA "Budgets and costs"): a gear portrait $0.0332, a gear diorama $0.0272 ($0.0604 per
  look), an item picture $0.0242; worst case $0.48 per human player per match plus at most $0.29 of item art.
- Over budget, an image comes back `status: "off"` and the client keeps its placeholder.

## 6. Never

- Never call Meshy or an image provider directly, outside the backend and the registry; never spend without approval.
- Never put words, gore or real people in a picture; never use a real person's likeness.
- Never mix the two styles (a realistic statue on the toy board, a toy piece in the god room).
- Never commit keys (the Meshy key lives in local env files only); no personal data in file names or metadata.

## 7. Read

`../FULL_EXPLAINER.md`; server repo `docs/MEDIA.md`, `docs/ASSETS.md`, `docs/SHARED_MEDIA_SPEC.md`,
`docs/ACCOUNTS.md` §14, `data/media_style.json`; Unreal repo `docs/FOR_BENITO.md`, `docs/design/` concepts.

//! The registry's SQLite schema and its migrations (`PRAGMA user_version`). Every column is documented in
//! `docs/ASSETS.md`; this file is the source of truth.

use rusqlite::Connection;

/// The newest schema version.
pub const VERSION: i64 = 2;

/// Asset statuses. `external` = made and recorded, but its file is not kept in the registry (a Meshy intermediate
/// that was never downloaded).
pub const STATUSES: [&str; 5] = ["pending", "ready", "failed", "refused", "external"];
pub const SUBJECT_KINDS: [&str; 7] = ["demigod", "god", "item", "place", "match", "shared", "person"];
pub const KINDS: [&str; 4] = ["image", "model", "texture", "animation"];
/// What the child is to the parent (`retexture_of`, `rig_of`, `clip_of`, `variant_of`, `preview_of`), or what the
/// parent was for the child (`reference`: sent along as a style or identity reference; `source`: the input it was
/// made from).
pub const LINEAGE_ROLES: [&str; 7] =
    ["reference", "source", "retexture_of", "rig_of", "clip_of", "variant_of", "preview_of"];

const V1: &str = r#"
CREATE TABLE assets (
    id            TEXT PRIMARY KEY,                -- 'ast_' + 24 hex, derived from the asset's identity (stable across re-ingestion)
    sha256        TEXT,                            -- sha256 of the file's bytes (hex); NULL until there is a file
    kind          TEXT NOT NULL CHECK (kind IN ('image','model','texture','animation')),
    subkind       TEXT NOT NULL,                   -- portrait | diorama | event | item | statue | place | army | reference | preview | figure | mannequin | skin | clip
    subject_kind  TEXT NOT NULL CHECK (subject_kind IN ('demigod','god','item','place','match','shared','person')),
    subject_key   TEXT NOT NULL,                   -- normalized name / item descriptor / match id / shared key
    variant       TEXT NOT NULL DEFAULT '',        -- which one of several for the subject: skin name + map, clip name, 'heroic', 'apose', a file format
    display_name  TEXT NOT NULL,
    path          TEXT,                            -- file path relative to the assets root (images/portrait/<sha256>.png); NULL until there is a file
    bytes         INTEGER,
    mime          TEXT,
    width         INTEGER,                         -- pixels, where known
    height        INTEGER,
    tris          INTEGER,                         -- triangles, where known (GLB)
    created_at    INTEGER NOT NULL,                -- unix ms: when the asset was made (the provider finished), else when first recorded
    updated_at    INTEGER NOT NULL,                -- unix ms: last change of this row (status, enrichment)
    status        TEXT NOT NULL CHECK (status IN ('pending','ready','failed','refused','external')),
    status_reason TEXT,                            -- why refused / the failure
    style_version TEXT,                            -- the media style config version ('2'), when known
    prompt_version TEXT,                           -- the prompt version ('fn7-media/3'), when known
    media_hash    TEXT,                            -- the fn7-media key (GET /media/<hash>.<ext>), for media-layer images
    source        TEXT NOT NULL,                   -- live | backfill | recording | provenance | job | operator
    origin_path   TEXT,                            -- where the file was imported from, as given
    meta          TEXT                             -- JSON: anything else worth keeping (size, sheet, action ids, notes)
);
CREATE UNIQUE INDEX assets_media_hash ON assets(media_hash) WHERE media_hash IS NOT NULL;
CREATE INDEX assets_subject ON assets(subject_kind, subject_key, kind, subkind, variant, created_at);
CREATE INDEX assets_sha ON assets(sha256);
CREATE INDEX assets_page ON assets(created_at, id);
CREATE INDEX assets_kind ON assets(kind, subkind, status);

CREATE TABLE generations (
    id                 TEXT PRIMARY KEY,           -- 'gen_' + 24 hex, derived from what identifies the call (stable across re-ingestion)
    asset_id           TEXT NOT NULL REFERENCES assets(id),   -- the primary output
    provider           TEXT NOT NULL,              -- openai | meshy | anthropic | rules | mock
    model              TEXT,
    operation          TEXT,                       -- images/edits | images/generations | image-to-3d | text-to-3d preview | retexture | rigging | animate | mock
    endpoint           TEXT,                       -- the URL or route called, when known
    prompt             TEXT,                       -- the EXACT prompt text sent
    negative_prompt    TEXT,                       -- the avoid / negative text, when it is a separate field (for images it is part of `prompt`; kept here too)
    params             TEXT,                       -- JSON: every other parameter sent (size, quality, background, formats, pose, ...)
    reference_asset_ids TEXT,                      -- JSON array of asset ids sent as references / inputs, in order
    provider_task_id   TEXT,                       -- the provider's task or request id
    requested_at       INTEGER,                    -- unix ms: asked for (queued)
    started_at         INTEGER,                    -- unix ms: the provider call began
    finished_at        INTEGER,                    -- unix ms: the provider answered / the file was stored
    duration_ms        REAL,                       -- the provider call (started → finished)
    cost_usd           REAL,
    cost_credits       REAL,
    cost_basis         TEXT,                       -- usage | price_table | fixed_price | sidecar | credits_reported | estimated | free | unknown
    usage              TEXT,                       -- JSON: the provider's usage object (tokens), when kept
    status             TEXT NOT NULL CHECK (status IN ('pending','ok','failed','refused')),
    error              TEXT,
    requested_by       TEXT,                       -- 'match:<id>' | 'prewarm' | 'operator' | 'unknown'
    requested_match    TEXT,
    requested_beat     INTEGER,
    requested_seat     INTEGER,
    requested_account  TEXT,
    source             TEXT NOT NULL,              -- live | sidecar | recording | provenance | job | operator
    recorded_at        INTEGER NOT NULL            -- unix ms: when this row was first written
);
CREATE INDEX generations_asset ON generations(asset_id);
CREATE INDEX generations_match ON generations(requested_match, requested_beat);
CREATE INDEX generations_task ON generations(provider_task_id);
CREATE INDEX generations_when ON generations(provider, finished_at);

-- Every file a provider call produced (a statue task gives a model and a preview; a skin gives four maps).
CREATE TABLE generation_outputs (
    generation_id TEXT NOT NULL REFERENCES generations(id),
    asset_id      TEXT NOT NULL REFERENCES assets(id),
    output_role   TEXT NOT NULL,                   -- primary | preview | base_color | metallic | normal | roughness | fbx | glb | member
    PRIMARY KEY (generation_id, asset_id)
);
CREATE INDEX generation_outputs_asset ON generation_outputs(asset_id);

CREATE TABLE lineage (
    parent_id  TEXT NOT NULL REFERENCES assets(id),
    child_id   TEXT NOT NULL REFERENCES assets(id),
    role       TEXT NOT NULL CHECK (role IN ('reference','source','retexture_of','rig_of','clip_of','variant_of','preview_of')),
    note       TEXT,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (parent_id, child_id, role)
);
CREATE INDEX lineage_child ON lineage(child_id);

-- Where an asset was shown or asked for. One row per (asset, match, role, subject ref) per run of beats: `beat` is
-- the first beat it was seen there, `last_beat` the last so far.
CREATE TABLE uses (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_id     TEXT NOT NULL REFERENCES assets(id),
    match_id     TEXT NOT NULL,
    beat         INTEGER NOT NULL DEFAULT -1,      -- first beat seen (-1 = unknown: a match-level record)
    last_beat    INTEGER NOT NULL DEFAULT -1,
    role         TEXT NOT NULL,                    -- the payload key (portrait_image | diorama_image | statue_image | image), 'requested', 'cached', 'library'
    subject_ref  TEXT NOT NULL DEFAULT '',         -- the engine ref in that match (dg:0, hd:1, it:4), when known
    subject_kind TEXT,
    subject_key  TEXT,                             -- normalized name (the same key as assets.subject_key)
    loadout_hash TEXT,                             -- the gear the picture shows, when the payload says
    seat         INTEGER,
    first_seen_at INTEGER NOT NULL,
    last_seen_at  INTEGER NOT NULL,
    source       TEXT NOT NULL,                    -- live | recording | library
    UNIQUE (asset_id, match_id, beat, role, subject_ref)
);
CREATE INDEX uses_match ON uses(match_id, beat);
CREATE INDEX uses_subject ON uses(match_id, subject_key, beat);
CREATE INDEX uses_asset ON uses(asset_id);

-- The newest ready asset per (kind, subkind, subject, variant). Older versions stay as rows.
CREATE VIEW current_assets AS
SELECT a.* FROM assets a
WHERE a.status = 'ready'
  AND NOT EXISTS (
    SELECT 1 FROM assets b
    WHERE b.status = 'ready' AND b.kind = a.kind AND b.subkind = a.subkind
      AND b.subject_kind = a.subject_kind AND b.subject_key = a.subject_key AND b.variant = a.variant
      AND (b.created_at > a.created_at OR (b.created_at = a.created_at AND b.id > a.id))
  );
"#;

/// v2: what an import run did (one row per ingestion, for the record and for "is the backfill done").
const V2: &str = r#"
CREATE TABLE ingest_runs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    kind        TEXT NOT NULL,                     -- media-cache | profile-library | recordings | provenance | jobs
    source      TEXT NOT NULL,                     -- the directory or file read
    started_at  INTEGER NOT NULL,
    finished_at INTEGER NOT NULL,
    report      TEXT NOT NULL                      -- JSON: counts
);
"#;

pub(crate) const MIGRATIONS: [&str; VERSION as usize] = [V1, V2];

/// Bring the database up to [`VERSION`]. Each step runs in a transaction; a database newer than this build is refused.
pub fn migrate(conn: &mut Connection) -> rusqlite::Result<i64> {
    let have: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if have > VERSION {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "assets.sqlite is schema {have}, this build knows {VERSION}"
        )));
    }
    for v in have..VERSION {
        let tx = conn.transaction()?;
        tx.execute_batch(MIGRATIONS[v as usize])?;
        tx.pragma_update(None, "user_version", v + 1)?;
        tx.commit()?;
    }
    Ok(have)
}

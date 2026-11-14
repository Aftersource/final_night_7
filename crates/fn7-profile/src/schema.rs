//! The store's SQLite schema and its migrations, tracked with `PRAGMA user_version` (each migration runs once, in a
//! transaction, in order). Add a migration by appending to [`MIGRATIONS`]; never edit a shipped one.

use rusqlite::Connection;

/// Each entry migrates from version `i` to `i + 1`.
pub const MIGRATIONS: &[&str] = &[
    // 0 → 1: profiles, demigod identities, matches, per-match players, the image library.
    r#"
CREATE TABLE profiles (
    key              TEXT PRIMARY KEY,             -- normalized display name
    display_name     TEXT NOT NULL,                -- the latest spelling seen
    skeleton         TEXT NOT NULL,                -- lookalike skeleton (normalize::skeleton)
    description_user TEXT NOT NULL DEFAULT '',     -- written by the player
    description_auto TEXT NOT NULL DEFAULT '',     -- written by the rules after each match
    claim_salt       TEXT,                         -- set when the profile is claimed
    claim_hash       TEXT,                         -- sha256(salt || secret), hex
    created_at       INTEGER NOT NULL,             -- epoch ms
    updated_at       INTEGER NOT NULL
);
CREATE INDEX profiles_skeleton ON profiles(skeleton);

CREATE TABLE identities (
    profile_key   TEXT NOT NULL REFERENCES profiles(key) ON DELETE CASCADE,
    name_key      TEXT NOT NULL,                   -- normalized demigod name: its visual sheet's key
    name          TEXT NOT NULL,
    hand          TEXT,                            -- 'left' | 'right' | NULL (one demigod per seat, before schema 5)
    sheet_hash    TEXT,
    traits        TEXT,                            -- JSON {build, face, outfit, colors, prop, emblem}
    portrait_hash TEXT,
    diorama_hash  TEXT,
    first_match   TEXT,
    last_match    TEXT,
    first_seen    INTEGER,
    last_seen     INTEGER,
    PRIMARY KEY (profile_key, name_key)
);

CREATE TABLE matches (
    match_id      TEXT PRIMARY KEY,
    room_code     TEXT,
    started_at    INTEGER,
    ended_at      INTEGER,
    outcome       TEXT NOT NULL,                   -- 'ended' | 'unfinished'
    rounds        INTEGER,
    beats_total   INTEGER,
    rules_version TEXT,
    forked_from   TEXT,
    champions     TEXT NOT NULL DEFAULT '[]',      -- JSON array of names
    champion_seat INTEGER,
    title         TEXT,
    field         INTEGER,                         -- demigods in the standings
    recording     TEXT,                            -- the JSONL file name (time travel)
    source        TEXT NOT NULL,                   -- 'live' | 'backfill'
    ingested_at   INTEGER NOT NULL
);

CREATE TABLE match_players (
    match_id        TEXT NOT NULL REFERENCES matches(match_id) ON DELETE CASCADE,
    seat            INTEGER NOT NULL,
    profile_key     TEXT NOT NULL REFERENCES profiles(key) ON DELETE CASCADE,
    display_name    TEXT NOT NULL,
    champion        INTEGER NOT NULL DEFAULT 0,
    placement       INTEGER,
    alive           INTEGER,
    eliminated_beat INTEGER,
    cause           TEXT,
    score           REAL,
    demigods        TEXT NOT NULL DEFAULT '[]',    -- JSON [MatchDemigod]
    items           TEXT NOT NULL DEFAULT '[]',    -- JSON [NotableItem]
    deeds           TEXT NOT NULL DEFAULT '[]',    -- JSON [Deed]
    stats           TEXT NOT NULL DEFAULT '{}',    -- JSON (the pile's stats, bolts, slows)
    eliminations    TEXT NOT NULL DEFAULT '[]',    -- JSON [Elimination]
    PRIMARY KEY (match_id, seat)
);
CREATE INDEX match_players_profile ON match_players(profile_key);

CREATE TABLE library (
    profile_key  TEXT NOT NULL REFERENCES profiles(key) ON DELETE CASCADE,
    hash         TEXT NOT NULL,                    -- the media hash (GET /media/index resolves it)
    kind         TEXT NOT NULL,                    -- portrait | diorama | statue | event | item | army | place
    caption      TEXT NOT NULL,
    subject      TEXT,                             -- the ref it shows, in its match (dg:0, hd:1, it:4)
    subject_name TEXT,
    first_seen   INTEGER NOT NULL,
    last_seen    INTEGER NOT NULL,
    PRIMARY KEY (profile_key, hash)
);
CREATE INDEX library_recent ON library(profile_key, last_seen DESC);

CREATE TABLE library_matches (
    profile_key TEXT NOT NULL,
    hash        TEXT NOT NULL,
    match_id    TEXT NOT NULL,
    at          INTEGER NOT NULL,
    PRIMARY KEY (profile_key, hash, match_id),
    FOREIGN KEY (profile_key, hash) REFERENCES library(profile_key, hash) ON DELETE CASCADE
);
"#,
];

/// The schema version a fully migrated store has.
pub fn latest() -> u32 {
    MIGRATIONS.len() as u32
}

/// Bring `conn` to the latest version; returns the version it was at.
pub fn migrate(conn: &mut Connection) -> rusqlite::Result<u32> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    // journal_mode answers with a row (an in-memory store stays "memory").
    conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    let from: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(from as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as u32)?;
        tx.commit()?;
    }
    Ok(from)
}

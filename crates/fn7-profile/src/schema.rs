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
    // 1 → 2: accounts (docs/ACCOUNTS.md). Nothing above is touched: every existing profile stays as it is, unlinked,
    // until an account takes its name (`account_profiles`).
    r#"
CREATE TABLE accounts (
    id           TEXT PRIMARY KEY,                 -- public id: 'acc_' + 24 hex
    display_name TEXT NOT NULL,
    name_key     TEXT NOT NULL UNIQUE,             -- normalized display name: the name this account holds
    skeleton     TEXT NOT NULL,                    -- lookalike skeleton of name_key
    profile_key  TEXT,                             -- the linked profile (= name_key when it was linked)
    name_since   INTEGER NOT NULL,                 -- when it took name_key (epoch ms)
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);
CREATE INDEX accounts_skeleton ON accounts(skeleton);

-- Every profile an account holds: its current name's, and the names it held before a rename.
CREATE TABLE account_profiles (
    profile_key TEXT PRIMARY KEY,
    account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    linked_at   INTEGER NOT NULL,
    how         TEXT NOT NULL                      -- 'new' | 'unclaimed' | 'secret'
);
CREATE INDEX account_profiles_account ON account_profiles(account_id);

CREATE TABLE account_identities (
    provider       TEXT NOT NULL,                  -- 'guest' | 'google'
    subject        TEXT NOT NULL,                  -- guest: the account id; google: the ID token's `sub`
    account_id     TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    email          TEXT,                           -- stored; only ever returned to the account itself
    email_verified INTEGER NOT NULL DEFAULT 0,
    created_at     INTEGER NOT NULL,
    PRIMARY KEY (provider, subject)
);
CREATE INDEX account_identities_account ON account_identities(account_id);

CREATE TABLE sessions (
    id           TEXT PRIMARY KEY,                 -- 'ses_' + 16 hex (not a credential)
    account_id   TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    token_hash   TEXT NOT NULL UNIQUE,             -- sha256 of the opaque token; the token itself is never stored
    created_at   INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL,
    device_label TEXT NOT NULL DEFAULT '',
    revoked_at   INTEGER
);
CREATE INDEX sessions_account ON sessions(account_id);

CREATE TABLE account_settings (
    account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    version    INTEGER NOT NULL,
    doc        TEXT NOT NULL,                      -- the validated settings document (JSON)
    updated_at INTEGER NOT NULL
);

CREATE TABLE account_demigods (
    account_id   TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    hand         TEXT NOT NULL CHECK (hand IN ('left', 'right')),
    name         TEXT,                             -- custom name, or NULL
    name_key     TEXT,                             -- normalized
    name_skeleton TEXT,
    personality  TEXT NOT NULL DEFAULT '',
    style        TEXT NOT NULL DEFAULT '',
    creed        TEXT NOT NULL DEFAULT '',
    original     TEXT NOT NULL DEFAULT '{}',       -- JSON: what the player submitted
    verdict      TEXT NOT NULL DEFAULT 'ok',       -- 'ok' | 'rephrase'
    reason       TEXT NOT NULL DEFAULT '',
    model        TEXT NOT NULL DEFAULT '',         -- the model that judged it, or 'rules'
    moderated_at INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    PRIMARY KEY (account_id, hand)
);
CREATE INDEX account_demigods_skeleton ON account_demigods(name_skeleton);

-- Every moderation pass, refusals included (the last 50 per account are kept).
CREATE TABLE demigod_moderation (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    hand       TEXT NOT NULL,
    original   TEXT NOT NULL,                      -- JSON
    final      TEXT,                               -- JSON, NULL when refused
    verdict    TEXT NOT NULL,                      -- 'ok' | 'rephrase' | 'refuse'
    reason     TEXT NOT NULL DEFAULT '',
    categories TEXT NOT NULL DEFAULT '[]',
    model      TEXT NOT NULL DEFAULT '',
    at         INTEGER NOT NULL
);
CREATE INDEX demigod_moderation_account ON demigod_moderation(account_id, id);

-- Billing is NOT live (docs/ACCOUNTS.md § Billing): no row is ever written by a payment provider yet.
CREATE TABLE entitlements (
    account_id   TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    sku          TEXT NOT NULL,
    status       TEXT NOT NULL,                    -- 'active' | 'past_due' | 'canceled' | 'expired'
    source       TEXT NOT NULL,                    -- 'grant' | 'stripe'
    granted_at   INTEGER NOT NULL,
    expires_at   INTEGER,
    external_ref TEXT,
    PRIMARY KEY (account_id, sku)
);
CREATE TABLE billing_events (
    event_id    TEXT PRIMARY KEY,                  -- the provider's event id: a webhook is applied once
    provider    TEXT NOT NULL,
    kind        TEXT NOT NULL,
    account_id  TEXT,
    received_at INTEGER NOT NULL
);

-- An engine player (the Theatre's player_id of one websocket login) proven to belong to an account.
CREATE TABLE game_bindings (
    player_id  TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    session_id TEXT NOT NULL,
    bound_at   INTEGER NOT NULL
);
CREATE INDEX game_bindings_session ON game_bindings(session_id);

-- A deleted account's profile names (hashed): a backfill never rebuilds their history from older recordings.
CREATE TABLE profile_tombstones (
    key_hash   TEXT PRIMARY KEY,
    deleted_at INTEGER NOT NULL
);
"#,
    // 2 → 3: gear looks. An image's appearance in a match may say when and with what: the beat it became the
    // demigod's look and the loadout it shows (gear portraits and dioramas).
    r#"
ALTER TABLE library_matches ADD COLUMN beat INTEGER;
ALTER TABLE library_matches ADD COLUMN detail TEXT;   -- JSON {loadout_hash, loadout: [item names]}
"#,
    // 3 → 4: the god form (docs/ACCOUNTS.md § 14). Each account's stone godform as the player wrote it (moderated like
    // the demigod sheets; the passes go to demigod_moderation with hand = 'god'), every reference image it asked for,
    // the image it accepted (the god's portrait), and the statues queued from it.
    r#"
CREATE TABLE account_gods (
    account_id   TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    description  TEXT NOT NULL DEFAULT '',         -- the player's words (or the minimal rewrite)
    stone        TEXT NOT NULL DEFAULT 'white_marble', -- a preset id, or the player's own words
    pose         TEXT NOT NULL DEFAULT 'thinker',  -- a preset id, or 'custom'
    original     TEXT NOT NULL DEFAULT '{}',       -- JSON: what the player submitted
    verdict      TEXT NOT NULL DEFAULT 'ok',       -- 'ok' | 'rephrase'
    reason       TEXT NOT NULL DEFAULT '',
    model        TEXT NOT NULL DEFAULT '',
    moderated_at INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

-- Every preview asked for: a new picture (charged = 1, counts toward the day's cap) or a cached one (charged = 0).
CREATE TABLE god_previews (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    media_hash  TEXT NOT NULL,                     -- the fn7-media key: GET /media/<hash>.<ext>
    description TEXT NOT NULL,
    stone       TEXT NOT NULL,
    pose        TEXT NOT NULL,
    prompt      TEXT NOT NULL,                     -- exactly as sent to the image model
    charged     INTEGER NOT NULL,
    at          INTEGER NOT NULL
);
CREATE INDEX god_previews_account ON god_previews(account_id, at);

-- Accepted previews, newest last: the newest is the account's god portrait; the older ones stay (the library).
CREATE TABLE god_accepted (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    media_hash  TEXT NOT NULL,
    description TEXT NOT NULL,
    stone       TEXT NOT NULL,
    pose        TEXT NOT NULL,
    accepted_at INTEGER NOT NULL
);
CREATE INDEX god_accepted_account ON god_accepted(account_id, id);

-- Statues queued from an accepted preview (Meshy image-to-3D; only when the server's god_statue_meshy is on).
CREATE TABLE god_statues (
    task_id     TEXT PRIMARY KEY,                  -- the provider's task id
    account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    media_hash  TEXT NOT NULL,                     -- the accepted preview it was made from
    provider    TEXT NOT NULL,                     -- 'meshy'
    status      TEXT NOT NULL,                     -- 'queued' | 'ready' | 'failed'
    credits     REAL,
    asset_id    TEXT,                              -- the registry's model asset, once ready
    error       TEXT,
    requested_at INTEGER NOT NULL,
    finished_at INTEGER
);
CREATE INDEX god_statues_account ON god_statues(account_id);
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

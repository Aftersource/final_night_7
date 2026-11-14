//! # fn7-profile
//!
//! Persistent player identity across Final Night 7 matches (design: `docs/DEMIGODS_AND_ITEMS.md` §6–8). Items reset
//! to zero between matches; a player's **profile** does not:
//!
//! - their demigods' fixed looks ([`DemigodIdentity`]: a demigod's visual sheet is a function of its name, so the
//!   same name is the same look in every match);
//! - match history ([`MatchRecord`]: match id, date, seat, demigods, champion or not, placement, notable items and
//!   deeds, eliminations);
//! - a personal description (written by the player, plus an automatic summary the rules rewrite after each match);
//! - an image library ([`LibraryImage`]: every portrait, diorama, statue and moment card made for them, by media
//!   hash);
//! - time travel ([`TimeTravel`]: each match's recording, scrubbed with `GET /admin/matches/{id}/state?after_round=N`
//!   and forked with `POST /admin/fork`).
//!
//! The Theatre has no accounts, so a profile is keyed by the **normalized display name** ([`normalize_name`]). See the
//! module docs of [`normalize`] for the collision and impersonation caveats.
//!
//! Storage is one SQLite file (rusqlite, as `fn7-project` uses), `<THEATRE_DATA_DIR>/fn7/profiles.sqlite` by default
//! ([`default_path`]). Every write is idempotent per match id, so the live path and a backfill
//! ([`ingest::read_recording`]) can both run over the same match.

pub mod ingest;
pub mod normalize;
pub mod schema;

#[cfg(feature = "http")]
pub mod http;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub use ingest::{MatchInput, SeatIn, Source};
pub use normalize::{normalize_name, skeleton};

// ---------------------------------------------------------------------------------------------------------------------
// Types (every one is also its JSON shape over HTTP)
// ---------------------------------------------------------------------------------------------------------------------

/// A player's persistent profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// The normalized display name: the profile's key.
    pub key: String,
    /// The latest spelling seen.
    pub display_name: String,
    pub description: Description,
    /// Claimed with a secret: its description can only be changed with that secret.
    pub claimed: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub totals: Totals,
    /// The player's demigods, most recently seen first.
    pub demigods: Vec<DemigodIdentity>,
    /// Display names of other profiles that look like this one (a possible impersonation; nothing is merged).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lookalikes: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Description {
    /// Written by the player (at most [`MAX_DESCRIPTION`] characters).
    pub user: String,
    /// Written by the rules after each match ([`auto_summary`]).
    pub auto: String,
}

/// Longest player-written description, in characters.
pub const MAX_DESCRIPTION: usize = 600;

/// Career totals, computed from the history.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Totals {
    pub matches: u32,
    pub finished: u32,
    pub unfinished: u32,
    pub champion: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_placement: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avg_placement: Option<f64>,
    /// Matches in which they fell.
    pub falls: u32,
    /// What took them most often (at least twice).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nemesis: Option<String>,
    pub images: u32,
}

/// One of a player's demigods, as fixed across matches: its name is its look.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DemigodIdentity {
    pub name: String,
    /// The normalized name (the visual sheet's key).
    pub name_key: String,
    /// `left` | `right` once a player has two demigods (schema 5); absent for the one-demigod matches before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hand: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet_hash: Option<String>,
    /// `{build, face, outfit, colors: [a, b], prop, emblem}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traits: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portrait_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diorama_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_match: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_match: Option<String>,
    pub matches: u32,
}

/// One match in a player's history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatchRecord {
    pub match_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<i64>,
    /// `ended` | `unfinished`.
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rounds: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beats_total: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules_version: Option<String>,
    pub seat: u8,
    /// The name they played under that match.
    pub display_name: String,
    pub champion: bool,
    /// 1 = champion's rank; absent for an unfinished match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<u32>,
    /// Demigods in the standings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<u32>,
    pub champions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eliminated_beat: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
    pub demigods: Vec<MatchDemigod>,
    pub items: Vec<NotableItem>,
    pub deeds: Vec<Deed>,
    pub stats: Value,
    pub eliminations: Vec<Elimination>,
    pub time_travel: TimeTravel,
}

/// One of the player's demigods in one match.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatchDemigod {
    /// Its ref in that match (`dg:N`, or `hd:N` from schema 5).
    pub r#ref: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hand: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alive: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eliminated_beat: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet_hash: Option<String>,
}

/// An item the player held or manifested (from the end-of-match pile). Schema-5 fields are absent in older matches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotableItem {
    pub r#ref: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_hash: Option<String>,
}

/// A moment that showed the player's demigod (a storyboard shot).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Deed {
    pub beat: u32,
    pub caption: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_hash: Option<String>,
}

/// A demigod's fall in the match (`own` = one of this player's).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Elimination {
    pub demigod: String,
    pub name: String,
    pub beat: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
    pub own: bool,
}

/// How to revisit a match. The admin routes need the admin token (`Authorization: Bearer`); these are just paths.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimeTravel {
    pub match_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rounds: Option<u32>,
    /// The recording's file name (`<THEATRE_RECORD_DIR>/<file>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recording: Option<String>,
    pub timeline: String,
    /// `{n}` = the round to scrub to.
    pub state: String,
    /// `POST` this path with `{"match_id": ..., "after_round": n}` to fork the match into a new private room.
    pub fork: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from: Option<String>,
}

impl TimeTravel {
    pub fn new(match_id: &str, rounds: Option<u32>, recording: Option<String>, forked_from: Option<String>) -> Self {
        Self {
            match_id: match_id.to_string(),
            rounds,
            recording,
            timeline: format!("/admin/matches/{match_id}/timeline"),
            state: format!("/admin/matches/{match_id}/state?after_round={{n}}&view=public"),
            fork: "/admin/fork".into(),
            forked_from,
        }
    }
}

/// One image in a player's library. Fetch it by hash: `GET /media/index` lists `{hash, url}` for ready images.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryImage {
    pub hash: String,
    /// portrait | diorama | statue | event | item | army | place.
    pub kind: String,
    pub caption: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_name: Option<String>,
    /// The matches it appeared in (a portrait is the same image in every match).
    pub matches: Vec<String>,
    pub first_seen: i64,
    pub last_seen: i64,
}

/// An image to add to a library by hand (`add_image`), e.g. one the media layer just made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewImage {
    pub hash: String,
    pub kind: String,
    pub caption: String,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub subject_name: Option<String>,
    #[serde(default)]
    pub match_id: Option<String>,
    /// Epoch ms; `None` = now.
    #[serde(default)]
    pub at: Option<i64>,
}

/// A page of results. `page` is 0-based; `next` is the next page, if any.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub page: u32,
    pub per_page: u32,
    pub total: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<u32>,
}

/// Default and largest page sizes.
pub const PER_PAGE: u32 = 20;
pub const MAX_PER_PAGE: u32 = 100;

/// What `record_match` did.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RecordReport {
    pub match_id: String,
    /// Profile keys written.
    pub players: Vec<String>,
    pub images: usize,
    /// The match was already stored as ended and this input was unfinished: nothing changed.
    pub skipped: bool,
}

#[derive(Debug)]
pub enum ProfileError {
    /// The name normalizes to nothing.
    BadName,
    NotFound,
    /// The profile is claimed and the secret didn't match.
    Locked,
    Invalid(String),
    Sql(rusqlite::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileError::BadName => write!(f, "bad name"),
            ProfileError::NotFound => write!(f, "no such profile"),
            ProfileError::Locked => write!(f, "profile is claimed"),
            ProfileError::Invalid(s) => write!(f, "invalid: {s}"),
            ProfileError::Sql(e) => write!(f, "sqlite: {e}"),
            ProfileError::Json(e) => write!(f, "json: {e}"),
        }
    }
}

impl std::error::Error for ProfileError {}

impl From<rusqlite::Error> for ProfileError {
    fn from(e: rusqlite::Error) -> Self {
        ProfileError::Sql(e)
    }
}

impl From<serde_json::Error> for ProfileError {
    fn from(e: serde_json::Error) -> Self {
        ProfileError::Json(e)
    }
}

pub type Result<T> = std::result::Result<T, ProfileError>;

// ---------------------------------------------------------------------------------------------------------------------
// The store
// ---------------------------------------------------------------------------------------------------------------------

/// `FN7_PROFILE_DB` if set, else `<data_dir>/fn7/profiles.sqlite` (`data_dir` = the core's `THEATRE_DATA_DIR`).
pub fn default_path(data_dir: &Path, env: impl Fn(&str) -> Option<String>) -> PathBuf {
    env("FN7_PROFILE_DB")
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir.join("fn7").join("profiles.sqlite"))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn page_args(page: u32, per_page: Option<u32>) -> (u32, u32) {
    let per = per_page.unwrap_or(PER_PAGE).clamp(1, MAX_PER_PAGE);
    (page, per)
}

fn clean_text(s: &str, max: usize) -> String {
    s.chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}

fn secret_hash(salt: &str, secret: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"fn7-profile/1\0");
    h.update(salt.as_bytes());
    h.update([0]);
    h.update(secret.as_bytes());
    hex::encode(h.finalize())
}

/// The persistent store. Cheap to share behind an `Arc`; every call takes the one connection's lock (writes are
/// a few rows per match, reads are small pages).
pub struct ProfileStore {
    conn: Mutex<Connection>,
}

impl ProfileStore {
    /// Open (creating the file and its directory) and migrate.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| ProfileError::Invalid(format!("{}: {e}", dir.display())))?;
        }
        Self::with(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::with(Connection::open_in_memory()?)
    }

    fn with(mut conn: Connection) -> Result<Self> {
        schema::migrate(&mut conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The schema version (after migration, [`schema::latest`]).
    pub fn version(&self) -> Result<u32> {
        Ok(self.lock().pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    /// Create the profile for `display_name` if it is new, or refresh its spelling. Returns the profile.
    pub fn upsert_profile(&self, display_name: &str) -> Result<Profile> {
        let key = normalize_name(display_name).ok_or(ProfileError::BadName)?;
        {
            let conn = self.lock();
            upsert_profile_tx(&conn, &key, &normalize::display_name(display_name), now_ms())?;
        }
        self.profile(display_name)?.ok_or(ProfileError::NotFound)
    }

    /// A profile by any spelling of its name.
    pub fn profile(&self, name: &str) -> Result<Option<Profile>> {
        let key = normalize_name(name).ok_or(ProfileError::BadName)?;
        let conn = self.lock();
        load_profile(&conn, &key)
    }

    /// Set the player-written description. A claimed profile needs its secret.
    pub fn set_description(&self, name: &str, text: &str, secret: Option<&str>) -> Result<Profile> {
        let key = normalize_name(name).ok_or(ProfileError::BadName)?;
        {
            let conn = self.lock();
            check_claim(&conn, &key, secret)?;
            let n = conn.execute(
                "UPDATE profiles SET description_user = ?2, updated_at = ?3 WHERE key = ?1",
                params![key, clean_text(text, MAX_DESCRIPTION), now_ms()],
            )?;
            if n == 0 {
                return Err(ProfileError::NotFound);
            }
        }
        self.profile(name)?.ok_or(ProfileError::NotFound)
    }

    /// Claim a profile with a secret (first come, first served), or change the secret given the old one. Protects the
    /// description; it doesn't stop anyone playing under the name (see the design doc).
    pub fn claim(&self, name: &str, secret: &str, old_secret: Option<&str>) -> Result<()> {
        let key = normalize_name(name).ok_or(ProfileError::BadName)?;
        if secret.chars().count() < 6 {
            return Err(ProfileError::Invalid("secret must be at least 6 characters".into()));
        }
        let conn = self.lock();
        check_claim(&conn, &key, old_secret)?;
        let salt = {
            let mut h = Sha256::new();
            h.update(key.as_bytes());
            h.update(now_ms().to_le_bytes());
            h.update(format!("{:p}", &conn).as_bytes());
            hex::encode(&h.finalize()[..16])
        };
        let n = conn.execute(
            "UPDATE profiles SET claim_salt = ?2, claim_hash = ?3, updated_at = ?4 WHERE key = ?1",
            params![key, salt, secret_hash(&salt, secret), now_ms()],
        )?;
        if n == 0 {
            return Err(ProfileError::NotFound);
        }
        Ok(())
    }

    /// Record one match: the match row, every human seat's history row, their demigod identities and library images,
    /// then each player's automatic description. Idempotent per match id; an unfinished input never overwrites an
    /// ended match.
    pub fn record_match(&self, m: &MatchInput) -> Result<RecordReport> {
        self.record_match_with(m, false)
    }

    /// [`record_match`](Self::record_match), optionally with the Theatre's bot seats too.
    pub fn record_match_with(&self, m: &MatchInput, include_bots: bool) -> Result<RecordReport> {
        if m.match_id.is_empty() {
            return Err(ProfileError::Invalid("no match id".into()));
        }
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let existing: Option<String> = tx
            .query_row("SELECT outcome FROM matches WHERE match_id = ?1", [&m.match_id], |r| r.get(0))
            .optional()?;
        if existing.as_deref() == Some("ended") && !m.ended() {
            return Ok(RecordReport {
                match_id: m.match_id.clone(),
                skipped: true,
                ..Default::default()
            });
        }
        let now = now_ms();
        let at = m.ended_at.or(m.started_at).unwrap_or(now);
        let (champions, champion_seat, field) = ingest::champions_of(m.summary.as_ref());
        let title = m.summary.as_ref().and_then(|s| s["final_cut"]["title"].as_str()).map(str::to_string);
        tx.execute("DELETE FROM matches WHERE match_id = ?1", [&m.match_id])?;
        tx.execute("DELETE FROM library_matches WHERE match_id = ?1", [&m.match_id])?;
        tx.execute(
            "INSERT INTO matches (match_id, room_code, started_at, ended_at, outcome, rounds, beats_total, rules_version,
                forked_from, champions, champion_seat, title, field, recording, source, ingested_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                m.match_id,
                m.room_code,
                m.started_at,
                m.ended_at,
                if m.ended() { "ended" } else { "unfinished" },
                m.rounds,
                m.beats_total,
                m.rules_version,
                m.forked_from,
                serde_json::to_string(&champions)?,
                champion_seat,
                title,
                field,
                m.recording,
                m.source.as_str(),
                now
            ],
        )?;
        let mut report = RecordReport {
            match_id: m.match_id.clone(),
            ..Default::default()
        };
        for p in ingest::slices(m, include_bots) {
            let Some(key) = normalize_name(&p.display_name) else {
                continue;
            };
            upsert_profile_tx(&tx, &key, &normalize::display_name(&p.display_name), at)?;
            tx.execute(
                "INSERT OR REPLACE INTO match_players (match_id, seat, profile_key, display_name, champion, placement,
                    alive, eliminated_beat, cause, score, demigods, items, deeds, stats, eliminations)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    m.match_id,
                    p.seat,
                    key,
                    p.display_name,
                    p.champion,
                    p.placement,
                    p.alive,
                    p.eliminated_beat,
                    p.cause,
                    p.score,
                    serde_json::to_string(&p.demigods)?,
                    serde_json::to_string(&p.items)?,
                    serde_json::to_string(&p.deeds)?,
                    serde_json::to_string(&p.stats)?,
                    serde_json::to_string(&p.eliminations)?
                ],
            )?;
            for id in &p.identities {
                let Some(name_key) = normalize_name(&id.name) else {
                    continue;
                };
                tx.execute(
                    "INSERT INTO identities (profile_key, name_key, name, hand, sheet_hash, traits, portrait_hash,
                        diorama_hash, first_match, last_match, first_seen, last_seen)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?10, ?10)
                     ON CONFLICT (profile_key, name_key) DO UPDATE SET
                        name = CASE WHEN excluded.last_seen >= last_seen THEN excluded.name ELSE name END,
                        hand = COALESCE(excluded.hand, hand),
                        sheet_hash = COALESCE(excluded.sheet_hash, sheet_hash),
                        traits = COALESCE(excluded.traits, traits),
                        portrait_hash = COALESCE(excluded.portrait_hash, portrait_hash),
                        diorama_hash = COALESCE(excluded.diorama_hash, diorama_hash),
                        first_match = CASE WHEN excluded.first_seen < first_seen THEN excluded.first_match ELSE first_match END,
                        first_seen = MIN(first_seen, excluded.first_seen),
                        last_match = CASE WHEN excluded.last_seen >= last_seen THEN excluded.last_match ELSE last_match END,
                        last_seen = MAX(last_seen, excluded.last_seen)",
                    params![
                        key,
                        name_key,
                        id.name,
                        id.hand,
                        id.sheet_hash,
                        id.traits.as_ref().map(|t| t.to_string()),
                        id.portrait_hash,
                        id.diorama_hash,
                        m.match_id,
                        at
                    ],
                )?;
            }
            for img in &p.images {
                add_image_tx(
                    &tx,
                    &key,
                    &NewImage {
                        hash: img.hash.clone(),
                        kind: img.kind.clone(),
                        caption: img.caption.clone(),
                        subject: img.subject.clone(),
                        subject_name: img.subject_name.clone(),
                        match_id: Some(m.match_id.clone()),
                        at: Some(at),
                    },
                )?;
                report.images += 1;
            }
            if !report.players.contains(&key) {
                report.players.push(key);
            }
        }
        for key in &report.players {
            let text = auto_summary(&totals_tx(&tx, key)?, &identities_tx(&tx, key)?, &recent_items_tx(&tx, key)?);
            tx.execute(
                "UPDATE profiles SET description_auto = ?2, updated_at = ?3 WHERE key = ?1",
                params![key, text, now],
            )?;
        }
        tx.commit()?;
        Ok(report)
    }

    /// Add one image to a player's library (creating the profile if needed). Idempotent per (player, hash, match).
    pub fn add_image(&self, name: &str, img: &NewImage) -> Result<()> {
        let key = normalize_name(name).ok_or(ProfileError::BadName)?;
        if img.hash.is_empty() || img.hash.len() > 128 || !img.hash.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(ProfileError::Invalid("bad image hash".into()));
        }
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        upsert_profile_tx(&tx, &key, &normalize::display_name(name), img.at.unwrap_or_else(now_ms))?;
        add_image_tx(&tx, &key, img)?;
        tx.commit()?;
        Ok(())
    }

    /// A player's matches, newest first.
    pub fn history(&self, name: &str, page: u32, per_page: Option<u32>) -> Result<Page<MatchRecord>> {
        let key = normalize_name(name).ok_or(ProfileError::BadName)?;
        let (page, per) = page_args(page, per_page);
        let conn = self.lock();
        let total: u32 = conn.query_row("SELECT COUNT(*) FROM match_players WHERE profile_key = ?1", [&key], |r| r.get(0))?;
        let mut st = conn.prepare(
            "SELECT m.match_id, m.room_code, m.started_at, m.ended_at, m.outcome, m.rounds, m.beats_total, m.rules_version,
                    p.seat, p.display_name, p.champion, p.placement, m.field, m.champions, m.title, p.eliminated_beat,
                    p.cause, p.demigods, p.items, p.deeds, p.stats, p.eliminations, m.recording, m.forked_from
             FROM match_players p JOIN matches m ON m.match_id = p.match_id
             WHERE p.profile_key = ?1
             ORDER BY COALESCE(m.ended_at, m.started_at, m.ingested_at) DESC, m.match_id, p.seat
             LIMIT ?2 OFFSET ?3",
        )?;
        let json = |s: String| serde_json::from_str::<Value>(&s).unwrap_or(Value::Null);
        let items = st
            .query_map(params![key, per, page as i64 * per as i64], |r| {
                let match_id: String = r.get(0)?;
                let rounds: Option<u32> = r.get(5)?;
                Ok(MatchRecord {
                    room_code: r.get(1)?,
                    started_at: r.get(2)?,
                    ended_at: r.get(3)?,
                    outcome: r.get(4)?,
                    rounds,
                    beats_total: r.get(6)?,
                    rules_version: r.get(7)?,
                    seat: r.get(8)?,
                    display_name: r.get(9)?,
                    champion: r.get(10)?,
                    placement: r.get(11)?,
                    field: r.get(12)?,
                    champions: serde_json::from_value(json(r.get(13)?)).unwrap_or_default(),
                    title: r.get(14)?,
                    eliminated_beat: r.get(15)?,
                    cause: r.get(16)?,
                    demigods: serde_json::from_value(json(r.get(17)?)).unwrap_or_default(),
                    items: serde_json::from_value(json(r.get(18)?)).unwrap_or_default(),
                    deeds: serde_json::from_value(json(r.get(19)?)).unwrap_or_default(),
                    stats: json(r.get(20)?),
                    eliminations: serde_json::from_value(json(r.get(21)?)).unwrap_or_default(),
                    time_travel: TimeTravel::new(&match_id, rounds, r.get(22)?, r.get(23)?),
                    match_id,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(paged(items, page, per, total))
    }

    /// A player's image library, most recently seen first; optionally one kind.
    pub fn library(&self, name: &str, page: u32, per_page: Option<u32>, kind: Option<&str>) -> Result<Page<LibraryImage>> {
        let key = normalize_name(name).ok_or(ProfileError::BadName)?;
        let (page, per) = page_args(page, per_page);
        let conn = self.lock();
        let total: u32 = conn.query_row(
            "SELECT COUNT(*) FROM library WHERE profile_key = ?1 AND (?2 IS NULL OR kind = ?2)",
            params![key, kind],
            |r| r.get(0),
        )?;
        let mut st = conn.prepare(
            "SELECT l.hash, l.kind, l.caption, l.subject, l.subject_name, l.first_seen, l.last_seen,
                    (SELECT json_group_array(match_id) FROM
                        (SELECT match_id FROM library_matches x WHERE x.profile_key = l.profile_key AND x.hash = l.hash
                         ORDER BY x.at))
             FROM library l WHERE l.profile_key = ?1 AND (?2 IS NULL OR l.kind = ?2)
             ORDER BY l.last_seen DESC, l.hash LIMIT ?3 OFFSET ?4",
        )?;
        let items = st
            .query_map(params![key, kind, per, page as i64 * per as i64], |r| {
                let ms: String = r.get(7)?;
                Ok(LibraryImage {
                    hash: r.get(0)?,
                    kind: r.get(1)?,
                    caption: r.get(2)?,
                    subject: r.get(3)?,
                    subject_name: r.get(4)?,
                    first_seen: r.get(5)?,
                    last_seen: r.get(6)?,
                    matches: serde_json::from_str(&ms).unwrap_or_default(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(paged(items, page, per, total))
    }

    /// Every profile key with its match count, most matches first (for the CLI and admin views).
    pub fn list(&self) -> Result<Vec<(String, String, u32)>> {
        let conn = self.lock();
        let mut st = conn.prepare(
            "SELECT p.key, p.display_name, (SELECT COUNT(*) FROM match_players m WHERE m.profile_key = p.key) AS n
             FROM profiles p ORDER BY n DESC, p.key",
        )?;
        let v = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    /// Ingest one recording file (backfill). `Ok(None)` = not a Final Night 7 recording.
    pub fn ingest_recording(&self, path: &Path) -> anyhow::Result<Option<RecordReport>> {
        let (input, _) = ingest::read_recording(path)?;
        match input {
            Some(m) => Ok(Some(self.record_match(&m)?)),
            None => Ok(None),
        }
    }
}

fn paged<T>(items: Vec<T>, page: u32, per: u32, total: u32) -> Page<T> {
    let next = ((page as u64 + 1) * per as u64) < total as u64;
    Page {
        items,
        page,
        per_page: per,
        total,
        next: next.then_some(page + 1),
    }
}

fn upsert_profile_tx(conn: &Connection, key: &str, display: &str, at: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO profiles (key, display_name, skeleton, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)
         ON CONFLICT (key) DO UPDATE SET
            display_name = CASE WHEN excluded.updated_at >= updated_at THEN excluded.display_name ELSE display_name END,
            created_at = MIN(created_at, excluded.created_at),
            updated_at = MAX(updated_at, excluded.updated_at)",
        params![key, if display.is_empty() { key } else { display }, skeleton(key), at],
    )?;
    Ok(())
}

fn add_image_tx(conn: &Connection, key: &str, img: &NewImage) -> Result<()> {
    let at = img.at.unwrap_or_else(now_ms);
    conn.execute(
        "INSERT INTO library (profile_key, hash, kind, caption, subject, subject_name, first_seen, last_seen)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
         ON CONFLICT (profile_key, hash) DO UPDATE SET
            caption = CASE WHEN excluded.first_seen < first_seen THEN excluded.caption ELSE caption END,
            first_seen = MIN(first_seen, excluded.first_seen),
            last_seen = MAX(last_seen, excluded.last_seen)",
        params![key, img.hash, img.kind, clean_text(&img.caption, 300), img.subject, img.subject_name, at],
    )?;
    if let Some(mid) = &img.match_id {
        conn.execute(
            "INSERT OR IGNORE INTO library_matches (profile_key, hash, match_id, at) VALUES (?1, ?2, ?3, ?4)",
            params![key, img.hash, mid, at],
        )?;
    }
    Ok(())
}

fn check_claim(conn: &Connection, key: &str, secret: Option<&str>) -> Result<()> {
    let row: Option<(Option<String>, Option<String>)> = conn
        .query_row("SELECT claim_salt, claim_hash FROM profiles WHERE key = ?1", [key], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    match row {
        None => Err(ProfileError::NotFound),
        Some((Some(salt), Some(hash))) => match secret {
            Some(s) if secret_hash(&salt, s) == hash => Ok(()),
            _ => Err(ProfileError::Locked),
        },
        Some(_) => Ok(()),
    }
}

fn load_profile(conn: &Connection, key: &str) -> Result<Option<Profile>> {
    let row = conn
        .query_row(
            "SELECT display_name, description_user, description_auto, claim_hash IS NOT NULL, created_at, updated_at, skeleton
             FROM profiles WHERE key = ?1",
            [key],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, bool>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, String>(6)?,
                ))
            },
        )
        .optional()?;
    let Some((display_name, user, auto, claimed, created_at, updated_at, skel)) = row else {
        return Ok(None);
    };
    let mut st = conn.prepare("SELECT display_name FROM profiles WHERE skeleton = ?1 AND key != ?2 ORDER BY key")?;
    let lookalikes = st
        .query_map(params![skel, key], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(Some(Profile {
        key: key.to_string(),
        display_name,
        description: Description { user, auto },
        claimed,
        created_at,
        updated_at,
        totals: totals_tx(conn, key)?,
        demigods: identities_tx(conn, key)?,
        lookalikes,
    }))
}

fn totals_tx(conn: &Connection, key: &str) -> Result<Totals> {
    let mut t = conn.query_row(
        "SELECT COUNT(*),
                COALESCE(SUM(m.outcome = 'ended'), 0),
                COALESCE(SUM(m.outcome != 'ended'), 0),
                COALESCE(SUM(p.champion), 0),
                MIN(p.placement),
                AVG(p.placement),
                COALESCE(SUM(m.outcome = 'ended' AND p.eliminated_beat IS NOT NULL AND NOT p.champion), 0)
         FROM match_players p JOIN matches m ON m.match_id = p.match_id WHERE p.profile_key = ?1",
        [key],
        |r| {
            Ok(Totals {
                matches: r.get(0)?,
                finished: r.get(1)?,
                unfinished: r.get(2)?,
                champion: r.get(3)?,
                best_placement: r.get(4)?,
                avg_placement: r.get::<_, Option<f64>>(5)?.map(|a| (a * 100.0).round() / 100.0),
                falls: r.get(6)?,
                nemesis: None,
                images: 0,
            })
        },
    )?;
    t.nemesis = conn
        .query_row(
            "SELECT cause FROM match_players WHERE profile_key = ?1 AND cause IS NOT NULL AND NOT champion
             GROUP BY cause HAVING COUNT(*) >= 2 ORDER BY COUNT(*) DESC, cause LIMIT 1",
            [key],
            |r| r.get(0),
        )
        .optional()?;
    t.images = conn.query_row("SELECT COUNT(*) FROM library WHERE profile_key = ?1", [key], |r| r.get(0))?;
    Ok(t)
}

fn identities_tx(conn: &Connection, key: &str) -> Result<Vec<DemigodIdentity>> {
    let mut st = conn.prepare(
        "SELECT i.name, i.name_key, i.hand, i.sheet_hash, i.traits, i.portrait_hash, i.diorama_hash, i.first_match,
                i.last_match
         FROM identities i WHERE i.profile_key = ?1 ORDER BY i.last_seen DESC, i.name_key",
    )?;
    // Matches per identity: every history row's demigods, by normalized name.
    let mut seen: Vec<String> = vec![];
    let mut q = conn.prepare("SELECT demigods FROM match_players WHERE profile_key = ?1")?;
    for row in q.query_map([key], |r| r.get::<_, String>(0))? {
        let dgs: Vec<MatchDemigod> = serde_json::from_str(&row?).unwrap_or_default();
        let mut keys: Vec<String> = dgs.iter().filter_map(|d| normalize_name(&d.name)).collect();
        keys.sort();
        keys.dedup();
        seen.extend(keys);
    }
    let count = |k: &str| seen.iter().filter(|x| *x == k).count() as u32;
    let mut v = st
        .query_map([key], |r| {
            Ok(DemigodIdentity {
                name: r.get(0)?,
                name_key: r.get(1)?,
                hand: r.get(2)?,
                sheet_hash: r.get(3)?,
                traits: r.get::<_, Option<String>>(4)?.and_then(|t| serde_json::from_str(&t).ok()),
                portrait_hash: r.get(5)?,
                diorama_hash: r.get(6)?,
                first_match: r.get(7)?,
                last_match: r.get(8)?,
                matches: 0,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for d in &mut v {
        d.matches = count(&d.name_key);
    }
    Ok(v)
}

fn recent_items_tx(conn: &Connection, key: &str) -> Result<Vec<String>> {
    let mut st = conn.prepare(
        "SELECT DISTINCT json_extract(i.value, '$.name') FROM match_players p JOIN matches m ON m.match_id = p.match_id,
                json_each(p.items) i
         WHERE p.profile_key = ?1 ORDER BY COALESCE(m.ended_at, m.started_at) DESC LIMIT 5",
    )?;
    let v = st
        .query_map([key], |r| r.get::<_, Option<String>>(0))?
        .filter_map(|x| x.transpose())
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(v)
}

fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// The automatic description: plain, rules-written, no LLM (an LLM rewrite is a later touchpoint with this as its
/// fallback).
pub fn auto_summary(t: &Totals, demigods: &[DemigodIdentity], items: &[String]) -> String {
    if t.matches == 0 {
        return "No matches yet.".into();
    }
    let mut parts = vec![];
    let names: Vec<&str> = demigods.iter().take(2).map(|d| d.name.as_str()).collect();
    let as_whom = if names.is_empty() { String::new() } else { format!(" as {}", names.join(" and ")) };
    parts.push(format!(
        "{} match{}{as_whom} ({} finished{}).",
        t.matches,
        if t.matches == 1 { "" } else { "es" },
        t.finished,
        if t.unfinished > 0 { format!(", {} unfinished", t.unfinished) } else { String::new() }
    ));
    if t.champion > 0 {
        parts.push(format!("Champion {} time{}.", t.champion, if t.champion == 1 { "" } else { "s" }));
    } else if let Some(b) = t.best_placement {
        parts.push(format!("Best finish: {}.", ordinal(b)));
    }
    if let Some(n) = &t.nemesis {
        parts.push(format!("Most often undone by {n}."));
    }
    if !items.is_empty() {
        parts.push(format!("Recently carried: {}.", items.join(", ")));
    }
    parts.join(" ")
}

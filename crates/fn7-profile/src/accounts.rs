//! Accounts on top of the name-keyed profiles (docs/ACCOUNTS.md): an account, its identities (guest, Google), its
//! sessions, its settings document, its two demigods' personalities, its entitlements, and the engine players bound
//! to it. Everything lives in the profile store's SQLite file (migration 2); existing profiles are untouched until an
//! account takes their name.
//!
//! **Names.** An account holds exactly one display name at a time, by its normalized key ([`crate::normalize_name`]),
//! and the profile under that key is its linked profile. The rules ([`ProfileStore::create_guest`],
//! [`ProfileStore::rename_account`], [`ProfileStore::google_sign_in`] all go through one check):
//! - a name held by an **active** account is taken (`name_taken`). An account is active while it has a Google
//!   identity or at least one live session; a guest account with no live session has lapsed and gives its name up;
//! - a name whose lookalike skeleton ([`crate::skeleton`]) equals an active account's is refused (`name_lookalike`);
//! - an existing profile under the name that nobody holds is **claimed by the first account that takes the name**.
//!   If that profile was claimed with a secret (`POST /api/profiles/{name}/claim`), the account must present the
//!   secret (`profile_claimed` otherwise): the secret is the proof. An unclaimed profile needs no proof beyond being
//!   first, exactly as anyone could play under that name before accounts existed;
//! - a name an account held before a rename stays linked to it (its history is not up for grabs).
//!
//! **Sessions.** The token is 244 bits from the OS random source, shown to the client once and stored only as
//! `sha256("fn7-session/1\0" || token)`. A session slides: each use moves `expires_at` to now + [`SESSION_TTL_MS`]
//! (written at most once per [`TOUCH_EVERY_MS`]). Revoked or expired sessions never authenticate.
//!
//! **Emails** are stored on the identity row and returned only inside the account's own [`Account`]; no other type
//! in this crate carries one and nothing here logs one.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::normalize::{display_name as clean_display, normalize_name, skeleton};
use crate::{check_claim, now_ms, upsert_profile_tx, ProfileError, ProfileStore};

/// A session lives this long after its last use (90 days).
pub const SESSION_TTL_MS: i64 = 90 * 24 * 3600 * 1000;
/// `last_seen_at` / `expires_at` are rewritten at most this often (5 minutes).
pub const TOUCH_EVERY_MS: i64 = 5 * 60 * 1000;
/// The engine refuses display names longer than this at `auth.login`.
pub const MAX_DISPLAY_NAME: usize = 24;
/// Longest device label kept.
pub const MAX_DEVICE_LABEL: usize = 60;
/// Sessions kept per account (the oldest are revoked past this).
pub const MAX_SESSIONS: usize = 20;
/// Moderation passes kept per account.
pub const MODERATION_LOG_KEEP: i64 = 50;

#[derive(Debug)]
pub enum AccountError {
    /// The name normalizes to nothing.
    BadName,
    /// An active account holds the name.
    NameTaken,
    /// The name looks like an active account's name (which is given).
    NameLookalike(String),
    /// The profile under the name is claimed with a secret, and the right secret was not given.
    ProfileClaimed,
    /// The Google identity already belongs to another account.
    IdentityInUse,
    NotFound,
    Invalid(String),
    Store(ProfileError),
}

impl AccountError {
    /// The stable error code clients switch on.
    pub fn code(&self) -> &'static str {
        match self {
            AccountError::BadName => "bad_name",
            AccountError::NameTaken => "name_taken",
            AccountError::NameLookalike(_) => "name_lookalike",
            AccountError::ProfileClaimed => "profile_claimed",
            AccountError::IdentityInUse => "identity_in_use",
            AccountError::NotFound => "not_found",
            AccountError::Invalid(_) => "invalid",
            AccountError::Store(_) => "store",
        }
    }
}

impl std::fmt::Display for AccountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccountError::BadName => write!(f, "that name has no usable characters"),
            AccountError::NameTaken => write!(f, "that name belongs to another account"),
            AccountError::NameLookalike(n) => write!(f, "that name looks like another account's name ({n})"),
            AccountError::ProfileClaimed => {
                write!(f, "that name's profile is claimed with a secret: send it as claim_secret")
            }
            AccountError::IdentityInUse => write!(f, "that Google account is already linked to another account"),
            AccountError::NotFound => write!(f, "no such account"),
            AccountError::Invalid(s) => write!(f, "{s}"),
            AccountError::Store(e) => write!(f, "store: {e}"),
        }
    }
}

impl std::error::Error for AccountError {}

impl From<rusqlite::Error> for AccountError {
    fn from(e: rusqlite::Error) -> Self {
        AccountError::Store(ProfileError::Sql(e))
    }
}

impl From<ProfileError> for AccountError {
    fn from(e: ProfileError) -> Self {
        AccountError::Store(e)
    }
}

impl From<serde_json::Error> for AccountError {
    fn from(e: serde_json::Error) -> Self {
        AccountError::Store(ProfileError::Json(e))
    }
}

pub type AResult<T> = std::result::Result<T, AccountError>;

/// An account, as its owner sees it (`GET /api/me`). Never served to anyone else.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Account {
    /// The public id (`acc_…`).
    pub id: String,
    pub display_name: String,
    /// `true` while the account has no identity but `guest`.
    pub guest: bool,
    /// The linked profile's key (the normalized display name): `GET /api/profiles/{profile}`.
    pub profile: String,
    /// Profiles of names held before a rename (still this account's).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub past_profiles: Vec<String>,
    pub identities: Vec<Identity>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Identity {
    /// `guest` | `google`.
    pub provider: String,
    /// The owner's own email, when the provider gave one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email_verified: Option<bool>,
    pub linked_at: i64,
}

/// A session, without its token.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionInfo {
    /// `ses_…`: names the session in lists; not a credential.
    pub id: String,
    pub created_at: i64,
    pub last_seen_at: i64,
    pub expires_at: i64,
    pub device_label: String,
    /// The session this request came with.
    #[serde(default)]
    pub current: bool,
}

/// A session just created: the only time its token exists outside the client.
#[derive(Debug, Clone, PartialEq)]
pub struct NewSession {
    pub token: String,
    pub info: SessionInfo,
}

/// One of an account's two demigods, as stored.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DemigodDoc {
    /// The custom name, or `null` for `"<display name>'s Left Hand"` / `"… Right Hand"`.
    pub name: Option<String>,
    pub personality: String,
    pub style: String,
    pub creed: String,
}

impl DemigodDoc {
    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.personality.is_empty() && self.style.is_empty() && self.creed.is_empty()
    }
}

/// A stored demigod with its moderation record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DemigodRow {
    /// `left` | `right`.
    pub hand: String,
    /// What the match uses.
    pub r#final: DemigodDoc,
    /// What the player submitted (equal to `final` when the verdict is `ok`).
    pub original: DemigodDoc,
    /// `ok` | `rephrase`.
    pub verdict: String,
    pub reason: String,
    /// The model that judged it, or `rules`.
    pub model: String,
    pub moderated_at: i64,
    pub updated_at: i64,
}

/// One entitlement row (billing is not live: see docs/ACCOUNTS.md § Billing).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entitlement {
    pub sku: String,
    pub status: String,
    pub source: String,
    pub granted_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
}

/// Who holds a name right now.
#[derive(Debug, Clone, PartialEq)]
pub struct NameHolder {
    pub account_id: String,
    /// When the account took the name (epoch ms).
    pub since: i64,
}

/// What `delete_account` removed.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Deletion {
    pub account: String,
    pub sessions: usize,
    pub identities: usize,
    pub profiles: Vec<String>,
    pub history_rows: usize,
    pub library_images: usize,
    pub bindings: usize,
}

fn random_hex(uuids: usize) -> String {
    (0..uuids).map(|_| uuid::Uuid::new_v4().simple().to_string()).collect()
}

/// The stored form of a session token.
pub fn token_hash(token: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"fn7-session/1\0");
    h.update(token.as_bytes());
    hex::encode(h.finalize())
}

fn key_hash(key: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"fn7-tombstone/1\0");
    h.update(key.as_bytes());
    hex::encode(h.finalize())
}

fn clean_label(s: &str) -> String {
    let flat: String = s.chars().filter(|c| !c.is_control()).collect();
    flat.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(MAX_DEVICE_LABEL).collect()
}

/// An email as stored: trimmed, lowercased, at most 254 characters, with exactly the shape `x@y`.
fn clean_email(s: &str) -> Option<String> {
    let e = s.trim().to_lowercase();
    let ok = e.len() <= 254 && e.split('@').count() == 2 && !e.starts_with('@') && !e.ends_with('@');
    ok.then_some(e)
}

fn is_active(conn: &Connection, account: &str, now: i64) -> rusqlite::Result<bool> {
    let google: i64 = conn.query_row(
        "SELECT COUNT(*) FROM account_identities WHERE account_id = ?1 AND provider <> 'guest'",
        [account],
        |r| r.get(0),
    )?;
    if google > 0 {
        return Ok(true);
    }
    let live: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sessions WHERE account_id = ?1 AND revoked_at IS NULL AND expires_at > ?2",
        params![account, now],
        |r| r.get(0),
    )?;
    Ok(live > 0)
}

/// Remove an account and everything that cascades from it. Its profiles stay (unlinked) unless `with_profiles`.
fn drop_account(conn: &Connection, account: &str, with_profiles: bool, now: i64) -> AResult<Deletion> {
    let mut d = Deletion {
        account: account.to_string(),
        ..Default::default()
    };
    let count = |sql: &str| -> rusqlite::Result<usize> { conn.query_row(sql, [account], |r| r.get::<_, i64>(0)).map(|n| n as usize) };
    d.sessions = count("SELECT COUNT(*) FROM sessions WHERE account_id = ?1")?;
    d.identities = count("SELECT COUNT(*) FROM account_identities WHERE account_id = ?1")?;
    d.bindings = count("SELECT COUNT(*) FROM game_bindings WHERE account_id = ?1")?;
    let keys: Vec<String> = {
        let mut st = conn.prepare("SELECT profile_key FROM account_profiles WHERE account_id = ?1 ORDER BY profile_key")?;
        let rows = st.query_map([account], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    if with_profiles {
        for k in &keys {
            d.history_rows += conn.query_row("SELECT COUNT(*) FROM match_players WHERE profile_key = ?1", [k], |r| r.get::<_, i64>(0))? as usize;
            d.library_images += conn.query_row("SELECT COUNT(*) FROM library WHERE profile_key = ?1", [k], |r| r.get::<_, i64>(0))? as usize;
            conn.execute("DELETE FROM library_matches WHERE profile_key = ?1", [k])?;
            conn.execute("DELETE FROM profiles WHERE key = ?1", [k])?;
            conn.execute(
                "INSERT INTO profile_tombstones (key_hash, deleted_at) VALUES (?1, ?2)
                 ON CONFLICT (key_hash) DO UPDATE SET deleted_at = excluded.deleted_at",
                params![key_hash(k), now],
            )?;
        }
        d.profiles = keys;
    }
    conn.execute("DELETE FROM accounts WHERE id = ?1", [account])?;
    Ok(d)
}

/// Give `account` the name `raw` (see the module doc for the rules). The account row must already exist unless
/// `insert` is set, in which case it is created with this name. Returns the cleaned display name and its key.
fn take_name(
    conn: &Connection,
    account: &str,
    raw: &str,
    claim_secret: Option<&str>,
    insert: bool,
    now: i64,
) -> AResult<(String, String)> {
    let key = normalize_name(raw).ok_or(AccountError::BadName)?;
    let display = clean_display(raw);
    if display.chars().count() > MAX_DISPLAY_NAME {
        return Err(AccountError::Invalid(format!("a display name is limited to {MAX_DISPLAY_NAME} characters")));
    }
    let skel = skeleton(&key);
    // The name's holder (current name or a name it held before), if any.
    let holder: Option<String> = conn
        .query_row("SELECT id FROM accounts WHERE name_key = ?1", [&key], |r| r.get(0))
        .optional()?
        .or(conn
            .query_row("SELECT account_id FROM account_profiles WHERE profile_key = ?1", [&key], |r| r.get(0))
            .optional()?);
    if let Some(h) = holder.filter(|h| h != account) {
        if is_active(conn, &h, now)? {
            return Err(AccountError::NameTaken);
        }
        // A lapsed guest: the account goes, its profiles stay for the name's next holder.
        drop_account(conn, &h, false, now)?;
    }
    // Lookalikes of active accounts' names.
    let similar: Vec<(String, String)> = {
        let mut st = conn.prepare("SELECT id, display_name FROM accounts WHERE skeleton = ?1 AND name_key <> ?2 AND id <> ?3")?;
        let rows = st.query_map(params![skel, key, account], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for (id, name) in similar {
        if is_active(conn, &id, now)? {
            return Err(AccountError::NameLookalike(name));
        }
    }
    // The profile under the name: new, unclaimed (first come), or claimed with a secret (the secret is the proof).
    let already_mine: bool = conn
        .query_row(
            "SELECT 1 FROM account_profiles WHERE profile_key = ?1 AND account_id = ?2",
            params![key, account],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    let how = if already_mine {
        None
    } else {
        match check_claim(conn, &key, claim_secret) {
            Ok(()) => {
                let claimed: bool = conn.query_row("SELECT claim_hash IS NOT NULL FROM profiles WHERE key = ?1", [&key], |r| r.get(0))?;
                Some(if claimed { "secret" } else { "unclaimed" })
            }
            Err(ProfileError::NotFound) => Some("new"),
            Err(ProfileError::Locked) => return Err(AccountError::ProfileClaimed),
            Err(e) => return Err(e.into()),
        }
    };
    upsert_profile_tx(conn, &key, &display, now)?;
    if insert {
        conn.execute(
            "INSERT INTO accounts (id, display_name, name_key, skeleton, profile_key, name_since, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?3, ?5, ?5, ?5)",
            params![account, display, key, skel, now],
        )?;
    } else {
        let changed: bool = conn
            .query_row("SELECT name_key <> ?2 FROM accounts WHERE id = ?1", params![account, key], |r| r.get(0))
            .optional()?
            .ok_or(AccountError::NotFound)?;
        conn.execute(
            "UPDATE accounts SET display_name = ?2, name_key = ?3, skeleton = ?4, profile_key = ?3,
                name_since = CASE WHEN ?6 THEN ?5 ELSE name_since END, updated_at = ?5 WHERE id = ?1",
            params![account, display, key, skel, now, changed],
        )?;
    }
    if let Some(how) = how {
        conn.execute(
            "INSERT INTO account_profiles (profile_key, account_id, linked_at, how) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (profile_key) DO UPDATE SET account_id = excluded.account_id, linked_at = excluded.linked_at,
                how = excluded.how",
            params![key, account, now, how],
        )?;
    }
    Ok((display, key))
}

fn new_session_tx(conn: &Connection, account: &str, device_label: &str, now: i64) -> AResult<NewSession> {
    let token = format!("fn7s_{}", random_hex(2));
    let id = format!("ses_{}", &random_hex(1)[..16]);
    let label = clean_label(device_label);
    conn.execute(
        "INSERT INTO sessions (id, account_id, token_hash, created_at, last_seen_at, expires_at, device_label)
         VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6)",
        params![id, account, token_hash(&token), now, now + SESSION_TTL_MS, label],
    )?;
    // Cap the live sessions of one account: the least recently used go first.
    conn.execute(
        "UPDATE sessions SET revoked_at = ?2 WHERE account_id = ?1 AND revoked_at IS NULL AND id NOT IN (
            SELECT id FROM sessions WHERE account_id = ?1 AND revoked_at IS NULL ORDER BY last_seen_at DESC, created_at DESC
            LIMIT ?3)",
        params![account, now, MAX_SESSIONS as i64],
    )?;
    Ok(NewSession {
        token,
        info: SessionInfo {
            id,
            created_at: now,
            last_seen_at: now,
            expires_at: now + SESSION_TTL_MS,
            device_label: label,
            current: true,
        },
    })
}

fn load_account(conn: &Connection, id: &str) -> AResult<Option<Account>> {
    let row = conn
        .query_row(
            "SELECT display_name, name_key, created_at, updated_at FROM accounts WHERE id = ?1",
            [id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?)),
        )
        .optional()?;
    let Some((display_name, key, created_at, updated_at)) = row else {
        return Ok(None);
    };
    let identities: Vec<Identity> = {
        let mut st = conn.prepare(
            "SELECT provider, email, email_verified, created_at FROM account_identities WHERE account_id = ?1
             ORDER BY created_at, provider",
        )?;
        let rows = st.query_map([id], |r| {
            let email: Option<String> = r.get(1)?;
            Ok(Identity {
                provider: r.get(0)?,
                email_verified: email.as_ref().map(|_| r.get::<_, i64>(2).unwrap_or(0) != 0),
                email,
                linked_at: r.get(3)?,
            })
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let past_profiles: Vec<String> = {
        let mut st = conn.prepare(
            "SELECT profile_key FROM account_profiles WHERE account_id = ?1 AND profile_key <> ?2 ORDER BY linked_at",
        )?;
        let rows = st.query_map(params![id, key], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    Ok(Some(Account {
        id: id.to_string(),
        display_name,
        guest: identities.iter().all(|i| i.provider == "guest"),
        profile: key,
        past_profiles,
        identities,
        created_at,
        updated_at,
    }))
}

fn doc_json(d: &DemigodDoc) -> String {
    serde_json::to_string(d).unwrap_or_else(|_| "{}".into())
}

impl ProfileStore {
    // ---- accounts and sessions ------------------------------------------------------------------------------------

    /// A guest account under `name` with its first session. No setup: the name is the whole request.
    pub fn create_guest(&self, name: &str, device_label: &str, claim_secret: Option<&str>) -> AResult<(Account, NewSession)> {
        let now = now_ms();
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let id = format!("acc_{}", &random_hex(1)[..24]);
        take_name(&tx, &id, name, claim_secret, true, now)?;
        tx.execute(
            "INSERT INTO account_identities (provider, subject, account_id, created_at) VALUES ('guest', ?1, ?1, ?2)",
            params![id, now],
        )?;
        let session = new_session_tx(&tx, &id, device_label, now)?;
        let account = load_account(&tx, &id)?.ok_or(AccountError::NotFound)?;
        tx.commit()?;
        Ok((account, session))
    }

    /// The account and session a token belongs to, or `None` (unknown, revoked or expired). Slides the session.
    pub fn session_account(&self, token: &str) -> AResult<Option<(Account, SessionInfo)>> {
        if token.is_empty() || token.len() > 200 {
            return Ok(None);
        }
        let now = now_ms();
        let conn = self.lock();
        let row = conn
            .query_row(
                "SELECT id, account_id, created_at, last_seen_at, expires_at, device_label, revoked_at
                 FROM sessions WHERE token_hash = ?1",
                [token_hash(token)],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, Option<i64>>(6)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, account_id, created_at, mut last_seen_at, mut expires_at, device_label, revoked_at)) = row else {
            return Ok(None);
        };
        if revoked_at.is_some() || expires_at <= now {
            return Ok(None);
        }
        if now - last_seen_at >= TOUCH_EVERY_MS {
            last_seen_at = now;
            expires_at = now + SESSION_TTL_MS;
            conn.execute("UPDATE sessions SET last_seen_at = ?2, expires_at = ?3 WHERE id = ?1", params![id, now, expires_at])?;
        }
        let Some(account) = load_account(&conn, &account_id)? else {
            return Ok(None);
        };
        Ok(Some((
            account,
            SessionInfo {
                id,
                created_at,
                last_seen_at,
                expires_at,
                device_label,
                current: true,
            },
        )))
    }

    /// Another session for an account that already proved itself (used when a sign-in completes).
    pub fn new_session(&self, account: &str, device_label: &str) -> AResult<NewSession> {
        let conn = self.lock();
        new_session_tx(&conn, account, device_label, now_ms())
    }

    /// Revoke one session and the engine players it bound. Returns whether it was live.
    pub fn revoke_session(&self, session_id: &str) -> AResult<bool> {
        let conn = self.lock();
        let n = conn.execute(
            "UPDATE sessions SET revoked_at = ?2 WHERE id = ?1 AND revoked_at IS NULL",
            params![session_id, now_ms()],
        )?;
        conn.execute("DELETE FROM game_bindings WHERE session_id = ?1", [session_id])?;
        Ok(n > 0)
    }

    /// Revoke every session of an account (and its bindings). Returns how many were live.
    pub fn revoke_all_sessions(&self, account: &str) -> AResult<usize> {
        let conn = self.lock();
        let n = conn.execute(
            "UPDATE sessions SET revoked_at = ?2 WHERE account_id = ?1 AND revoked_at IS NULL",
            params![account, now_ms()],
        )?;
        conn.execute("DELETE FROM game_bindings WHERE account_id = ?1", [account])?;
        Ok(n)
    }

    /// The account's live sessions, most recently used first (`current` marks `current_id`).
    pub fn sessions(&self, account: &str, current_id: Option<&str>) -> AResult<Vec<SessionInfo>> {
        let conn = self.lock();
        let mut st = conn.prepare(
            "SELECT id, created_at, last_seen_at, expires_at, device_label FROM sessions
             WHERE account_id = ?1 AND revoked_at IS NULL AND expires_at > ?2 ORDER BY last_seen_at DESC, created_at DESC",
        )?;
        let rows = st.query_map(params![account, now_ms()], |r| {
            let id: String = r.get(0)?;
            Ok(SessionInfo {
                current: current_id == Some(id.as_str()),
                id,
                created_at: r.get(1)?,
                last_seen_at: r.get(2)?,
                expires_at: r.get(3)?,
                device_label: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn account(&self, id: &str) -> AResult<Option<Account>> {
        let conn = self.lock();
        load_account(&conn, id)
    }

    /// Change the account's display name (the name rules apply; the old name's profile stays linked to it).
    pub fn rename_account(&self, id: &str, name: &str, claim_secret: Option<&str>) -> AResult<Account> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        take_name(&tx, id, name, claim_secret, false, now_ms())?;
        let a = load_account(&tx, id)?.ok_or(AccountError::NotFound)?;
        tx.commit()?;
        Ok(a)
    }

    /// Complete a verified Google sign-in for `subject` (the ID token's `sub`).
    ///
    /// - `link_to = Some(account)`: attach Google to that account (a guest keeps its name, history and settings).
    ///   `identity_in_use` if the subject already belongs to a different account.
    /// - `link_to = None`: sign in to the subject's account, creating it on first sight under the first of
    ///   `preferred_names` that is free (then `"<name> 2"`, `"<name> 3"`, …).
    ///
    /// Returns the account, a fresh session and whether the account was created.
    pub fn google_sign_in(
        &self,
        subject: &str,
        email: Option<&str>,
        email_verified: bool,
        preferred_names: &[String],
        device_label: &str,
        link_to: Option<&str>,
    ) -> AResult<(Account, NewSession, bool)> {
        if subject.is_empty() || subject.len() > 255 {
            return Err(AccountError::Invalid("bad subject".into()));
        }
        let now = now_ms();
        let email = email.and_then(clean_email);
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT account_id FROM account_identities WHERE provider = 'google' AND subject = ?1",
                [subject],
                |r| r.get(0),
            )
            .optional()?;
        let (id, created) = match (existing, link_to) {
            (Some(owner), Some(want)) if owner != want => return Err(AccountError::IdentityInUse),
            (Some(owner), _) => {
                tx.execute(
                    "UPDATE account_identities SET email = ?2, email_verified = ?3 WHERE provider = 'google' AND subject = ?1",
                    params![subject, email, email_verified],
                )?;
                (owner, false)
            }
            (None, Some(want)) => {
                if load_account(&tx, want)?.is_none() {
                    return Err(AccountError::NotFound);
                }
                let has: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM account_identities WHERE account_id = ?1 AND provider = 'google'",
                    [want],
                    |r| r.get(0),
                )?;
                if has > 0 {
                    return Err(AccountError::Invalid("this account already has a Google identity".into()));
                }
                (want.to_string(), false)
            }
            (None, None) => {
                let id = format!("acc_{}", &random_hex(1)[..24]);
                let mut taken = false;
                let mut bases: Vec<String> = preferred_names.iter().map(|n| clean_display(n)).filter(|n| !n.is_empty()).collect();
                bases.push("Player".into());
                'names: for base in &bases {
                    for n in 1..=50u32 {
                        let suffix = if n == 1 { String::new() } else { format!(" {n}") };
                        let keep = MAX_DISPLAY_NAME.saturating_sub(suffix.chars().count());
                        let name = format!("{}{suffix}", base.chars().take(keep).collect::<String>().trim_end());
                        match take_name(&tx, &id, &name, None, true, now) {
                            Ok(_) => {
                                taken = true;
                                break 'names;
                            }
                            Err(AccountError::NameTaken | AccountError::NameLookalike(_) | AccountError::ProfileClaimed) => continue,
                            Err(AccountError::BadName | AccountError::Invalid(_)) => continue 'names,
                            Err(e) => return Err(e),
                        }
                    }
                }
                if !taken {
                    return Err(AccountError::NameTaken);
                }
                (id, true)
            }
        };
        tx.execute(
            "INSERT OR IGNORE INTO account_identities (provider, subject, account_id, email, email_verified, created_at)
             VALUES ('google', ?1, ?2, ?3, ?4, ?5)",
            params![subject, id, email, email_verified, now],
        )?;
        // A guest that linked Google is no longer a guest.
        tx.execute("DELETE FROM account_identities WHERE provider = 'guest' AND account_id = ?1", [&id])?;
        tx.execute("UPDATE accounts SET updated_at = ?2 WHERE id = ?1", params![id, now])?;
        let session = new_session_tx(&tx, &id, device_label, now)?;
        let account = load_account(&tx, &id)?.ok_or(AccountError::NotFound)?;
        tx.commit()?;
        Ok((account, session, created))
    }

    /// Delete an account and its personal data: identities (emails), sessions, settings, demigod personalities and
    /// their moderation log, entitlements, bindings, and its linked profiles with their history and library rows.
    /// The profile names are tombstoned (hashed) so a backfill never rebuilds them from older recordings.
    pub fn delete_account(&self, id: &str) -> AResult<Deletion> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        if load_account(&tx, id)?.is_none() {
            return Err(AccountError::NotFound);
        }
        let d = drop_account(&tx, id, true, now_ms())?;
        tx.commit()?;
        Ok(d)
    }

    // ---- names --------------------------------------------------------------------------------------------------

    /// The active account that holds `name` (current name or one it held before), if any.
    pub fn name_holder(&self, name: &str) -> AResult<Option<NameHolder>> {
        let Some(key) = normalize_name(name) else {
            return Ok(None);
        };
        let conn = self.lock();
        let row: Option<(String, i64)> = conn
            .query_row("SELECT id, name_since FROM accounts WHERE name_key = ?1", [&key], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?
            .or(conn
                .query_row(
                    "SELECT account_id, linked_at FROM account_profiles WHERE profile_key = ?1",
                    [&key],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?);
        match row {
            Some((account_id, since)) if is_active(&conn, &account_id, now_ms())? => Ok(Some(NameHolder { account_id, since })),
            _ => Ok(None),
        }
    }

    /// The account a profile is linked to, and since when (whether or not the account is active).
    pub fn profile_account(&self, name: &str) -> AResult<Option<NameHolder>> {
        let Some(key) = normalize_name(name) else {
            return Ok(None);
        };
        let conn = self.lock();
        Ok(conn
            .query_row(
                "SELECT account_id, linked_at FROM account_profiles WHERE profile_key = ?1",
                [&key],
                |r| Ok(NameHolder { account_id: r.get(0)?, since: r.get(1)? }),
            )
            .optional()?)
    }

    // ---- settings ---------------------------------------------------------------------------------------------------

    /// The stored settings document and its version, if the account ever saved one.
    pub fn settings_doc(&self, account: &str) -> AResult<Option<(u32, Value)>> {
        let conn = self.lock();
        let row: Option<(u32, String)> = conn
            .query_row("SELECT version, doc FROM account_settings WHERE account_id = ?1", [account], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        Ok(match row {
            Some((v, doc)) => Some((v, serde_json::from_str(&doc)?)),
            None => None,
        })
    }

    pub fn put_settings(&self, account: &str, version: u32, doc: &Value) -> AResult<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO account_settings (account_id, version, doc, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (account_id) DO UPDATE SET version = excluded.version, doc = excluded.doc,
                updated_at = excluded.updated_at",
            params![account, version, serde_json::to_string(doc)?, now_ms()],
        )?;
        Ok(())
    }

    // ---- demigods ---------------------------------------------------------------------------------------------------

    /// One of the account's demigods (`hand` = `left` | `right`), if it was ever saved.
    pub fn demigod(&self, account: &str, hand: &str) -> AResult<Option<DemigodRow>> {
        let conn = self.lock();
        let row = conn
            .query_row(
                "SELECT name, personality, style, creed, original, verdict, reason, model, moderated_at, updated_at
                 FROM account_demigods WHERE account_id = ?1 AND hand = ?2",
                params![account, hand],
                |r| {
                    Ok((
                        DemigodDoc {
                            name: r.get(0)?,
                            personality: r.get(1)?,
                            style: r.get(2)?,
                            creed: r.get(3)?,
                        },
                        r.get::<_, String>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, String>(6)?,
                        r.get::<_, String>(7)?,
                        r.get::<_, i64>(8)?,
                        r.get::<_, i64>(9)?,
                    ))
                },
            )
            .optional()?;
        Ok(row.map(|(fin, original, verdict, reason, model, moderated_at, updated_at)| DemigodRow {
            hand: hand.to_string(),
            original: serde_json::from_str(&original).unwrap_or_else(|_| fin.clone()),
            r#final: fin,
            verdict,
            reason,
            model,
            moderated_at,
            updated_at,
        }))
    }

    /// Why `name` cannot be this account's demigod name, if it cannot: it reads like another account's display name
    /// or another account's demigod, or like this account's other demigod. (The built-in names are the caller's.)
    pub fn demigod_name_conflict(&self, account: &str, hand: &str, name: &str) -> AResult<Option<String>> {
        let Some(key) = normalize_name(name) else {
            return Ok(Some("the name has no usable characters".into()));
        };
        let skel = skeleton(&key);
        let conn = self.lock();
        let other_hand: Option<String> = conn
            .query_row(
                "SELECT name FROM account_demigods WHERE account_id = ?1 AND hand <> ?2 AND name_skeleton = ?3",
                params![account, hand, skel],
                |r| r.get(0),
            )
            .optional()?;
        if other_hand.is_some() {
            return Ok(Some("your two demigods need different names".into()));
        }
        // Only active accounts hold names: a lapsed guest's demigod names and display name are free again.
        let now = now_ms();
        let owners = |sql: &str| -> rusqlite::Result<Vec<String>> {
            let mut st = conn.prepare(sql)?;
            let rows = st.query_map(params![account, skel], |r| r.get(0))?;
            rows.collect()
        };
        for other in owners("SELECT DISTINCT account_id FROM account_demigods WHERE account_id <> ?1 AND name_skeleton = ?2")? {
            if is_active(&conn, &other, now)? {
                return Ok(Some("another player's demigod has that name, or one that reads the same".into()));
            }
        }
        for other in owners("SELECT id FROM accounts WHERE id <> ?1 AND skeleton = ?2")? {
            if is_active(&conn, &other, now)? {
                return Ok(Some("that is another player's name, or reads like it".into()));
            }
        }
        Ok(None)
    }

    /// Store a judged demigod (`verdict` = `ok` | `rephrase`) and log the pass.
    #[allow(clippy::too_many_arguments)]
    pub fn put_demigod(
        &self,
        account: &str,
        hand: &str,
        original: &DemigodDoc,
        fin: &DemigodDoc,
        verdict: &str,
        reason: &str,
        categories: &[String],
        model: &str,
    ) -> AResult<DemigodRow> {
        if !matches!(hand, "left" | "right") {
            return Err(AccountError::Invalid("hand must be left or right".into()));
        }
        let now = now_ms();
        {
            let mut conn = self.lock();
            let tx = conn.transaction()?;
            let key = fin.name.as_deref().and_then(normalize_name);
            tx.execute(
                "INSERT INTO account_demigods (account_id, hand, name, name_key, name_skeleton, personality, style, creed,
                    original, verdict, reason, model, moderated_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)
                 ON CONFLICT (account_id, hand) DO UPDATE SET name = excluded.name, name_key = excluded.name_key,
                    name_skeleton = excluded.name_skeleton, personality = excluded.personality, style = excluded.style,
                    creed = excluded.creed, original = excluded.original, verdict = excluded.verdict,
                    reason = excluded.reason, model = excluded.model, moderated_at = excluded.moderated_at,
                    updated_at = excluded.updated_at",
                params![
                    account,
                    hand,
                    fin.name,
                    key,
                    key.as_deref().map(skeleton),
                    fin.personality,
                    fin.style,
                    fin.creed,
                    doc_json(original),
                    verdict,
                    reason,
                    model,
                    now
                ],
            )?;
            log_moderation_tx(&tx, account, hand, original, Some(fin), verdict, reason, categories, model, now)?;
            tx.commit()?;
        }
        self.demigod(account, hand)?.ok_or(AccountError::NotFound)
    }

    /// Log a refused pass (nothing is stored for the match; the previous demigod, if any, stays).
    pub fn log_refusal(
        &self,
        account: &str,
        hand: &str,
        original: &DemigodDoc,
        reason: &str,
        categories: &[String],
        model: &str,
    ) -> AResult<()> {
        let conn = self.lock();
        log_moderation_tx(&conn, account, hand, original, None, "refuse", reason, categories, model, now_ms())
    }

    /// The account's moderation log, newest first: `(hand, verdict, reason, model, at, original, final)`.
    pub fn moderation_log(&self, account: &str) -> AResult<Vec<Value>> {
        let conn = self.lock();
        let mut st = conn.prepare(
            "SELECT hand, verdict, reason, categories, model, at, original, final FROM demigod_moderation
             WHERE account_id = ?1 ORDER BY id DESC",
        )?;
        let rows = st.query_map([account], |r| {
            let parse = |s: Option<String>| s.and_then(|s| serde_json::from_str::<Value>(&s).ok()).unwrap_or(Value::Null);
            Ok(serde_json::json!({
                "hand": r.get::<_, String>(0)?, "verdict": r.get::<_, String>(1)?, "reason": r.get::<_, String>(2)?,
                "categories": parse(r.get(3)?), "model": r.get::<_, String>(4)?, "at": r.get::<_, i64>(5)?,
                "original": parse(r.get(6)?), "final": parse(r.get(7)?),
            }))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ---- entitlements -----------------------------------------------------------------------------------------------

    /// The account's entitlement rows (none = the free tier). Billing is not live: only `grant_entitlement` writes.
    pub fn entitlements(&self, account: &str) -> AResult<Vec<Entitlement>> {
        let conn = self.lock();
        let mut st = conn.prepare(
            "SELECT sku, status, source, granted_at, expires_at FROM entitlements WHERE account_id = ?1 ORDER BY sku",
        )?;
        let rows = st.query_map([account], |r| {
            Ok(Entitlement {
                sku: r.get(0)?,
                status: r.get(1)?,
                source: r.get(2)?,
                granted_at: r.get(3)?,
                expires_at: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Grant (or update) an entitlement by hand. No route calls this: it is the seam a payment webhook would use.
    pub fn grant_entitlement(&self, account: &str, sku: &str, status: &str, source: &str, expires_at: Option<i64>) -> AResult<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO entitlements (account_id, sku, status, source, granted_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (account_id, sku) DO UPDATE SET status = excluded.status, source = excluded.source,
                expires_at = excluded.expires_at",
            params![account, sku, status, source, now_ms(), expires_at],
        )?;
        Ok(())
    }

    // ---- engine players ---------------------------------------------------------------------------------------------

    /// Bind an engine player (the Theatre's `player_id`) to an account, through the session that proved it.
    pub fn bind_player(&self, player_id: &str, account: &str, session_id: &str) -> AResult<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO game_bindings (player_id, account_id, session_id, bound_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (player_id) DO UPDATE SET account_id = excluded.account_id, session_id = excluded.session_id,
                bound_at = excluded.bound_at",
            params![player_id, account, session_id, now_ms()],
        )?;
        Ok(())
    }

    /// The account an engine player is bound to, if its session is still live.
    pub fn binding(&self, player_id: &str) -> AResult<Option<String>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                "SELECT b.account_id FROM game_bindings b JOIN sessions s ON s.id = b.session_id
                 WHERE b.player_id = ?1 AND s.revoked_at IS NULL AND s.expires_at > ?2",
                params![player_id, now_ms()],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Drop the bindings of engine players that no longer exist. Returns how many went.
    pub fn prune_bindings(&self, live_players: &[String]) -> AResult<usize> {
        let conn = self.lock();
        let all: Vec<String> = {
            let mut st = conn.prepare("SELECT player_id FROM game_bindings")?;
            let rows = st.query_map([], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let mut n = 0;
        for p in all.iter().filter(|p| !live_players.contains(p)) {
            n += conn.execute("DELETE FROM game_bindings WHERE player_id = ?1", [p])?;
        }
        Ok(n)
    }

    /// Whether `name`'s profile was deleted with its account at or after `at` (a backfill skips such history).
    pub(crate) fn tombstoned_tx(conn: &Connection, key: &str, at: i64) -> rusqlite::Result<bool> {
        let t: Option<i64> = conn
            .query_row("SELECT deleted_at FROM profile_tombstones WHERE key_hash = ?1", [key_hash(key)], |r| r.get(0))
            .optional()?;
        Ok(t.is_some_and(|deleted| at <= deleted))
    }

    /// The account a profile is linked to, inside a transaction: `(account id, linked_at)`.
    pub(crate) fn linked_tx(conn: &Connection, key: &str) -> rusqlite::Result<Option<(String, i64)>> {
        conn.query_row(
            "SELECT account_id, linked_at FROM account_profiles WHERE profile_key = ?1",
            [key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
    }
}

#[allow(clippy::too_many_arguments)]
fn log_moderation_tx(
    conn: &Connection,
    account: &str,
    hand: &str,
    original: &DemigodDoc,
    fin: Option<&DemigodDoc>,
    verdict: &str,
    reason: &str,
    categories: &[String],
    model: &str,
    now: i64,
) -> AResult<()> {
    conn.execute(
        "INSERT INTO demigod_moderation (account_id, hand, original, final, verdict, reason, categories, model, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            account,
            hand,
            doc_json(original),
            fin.map(doc_json),
            verdict,
            reason,
            serde_json::to_string(categories)?,
            model,
            now
        ],
    )?;
    conn.execute(
        "DELETE FROM demigod_moderation WHERE account_id = ?1 AND id NOT IN (
            SELECT id FROM demigod_moderation WHERE account_id = ?1 ORDER BY id DESC LIMIT ?2)",
        params![account, MODERATION_LOG_KEEP],
    )?;
    Ok(())
}

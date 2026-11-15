//! The god form (docs/ACCOUNTS.md § 14): each account's stone godform as the player wrote it, every reference image it
//! asked for, the image it accepted (its god's portrait) and the statues queued from it. Migration 4; every row goes
//! with its account (`ON DELETE CASCADE`). The moderation passes share the demigods' log (`demigod_moderation`,
//! `hand = 'god'`).

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::accounts::{AResult, AccountError, MODERATION_LOG_KEEP};
use crate::{now_ms, ProfileStore};

/// A god form: the player's description, the stone (a preset id or their own words) and the pose (a preset id or
/// `custom`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GodDoc {
    pub description: String,
    pub stone: String,
    pub pose: String,
}

impl Default for GodDoc {
    fn default() -> Self {
        Self { description: String::new(), stone: "white_marble".into(), pose: "thinker".into() }
    }
}

/// The stored god form with its moderation record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GodRow {
    /// What is drawn.
    pub r#final: GodDoc,
    /// What the player submitted (equal to `final` when the verdict is `ok`).
    pub original: GodDoc,
    /// `ok` | `rephrase`.
    pub verdict: String,
    pub reason: String,
    pub model: String,
    pub moderated_at: i64,
    pub updated_at: i64,
}

/// One preview the account asked for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GodPreview {
    pub media_hash: String,
    pub doc: GodDoc,
    pub prompt: String,
    /// A new picture (counts toward the day's cap), not a cached one.
    pub charged: bool,
    pub at: i64,
}

/// An accepted preview: the god's portrait.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GodAccepted {
    pub media_hash: String,
    pub doc: GodDoc,
    pub accepted_at: i64,
}

/// A statue queued from an accepted preview.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GodStatue {
    pub task_id: String,
    pub media_hash: String,
    pub provider: String,
    /// `queued` | `ready` | `failed`.
    pub status: String,
    pub credits: Option<f64>,
    pub asset_id: Option<String>,
    pub error: Option<String>,
    pub requested_at: i64,
    pub finished_at: Option<i64>,
}

fn doc_json(d: &GodDoc) -> String {
    serde_json::to_string(d).unwrap_or_else(|_| "{}".into())
}

impl ProfileStore {
    /// The account's god form, if it ever saved one.
    pub fn god(&self, account: &str) -> AResult<Option<GodRow>> {
        let conn = self.lock();
        let row = conn
            .query_row(
                "SELECT description, stone, pose, original, verdict, reason, model, moderated_at, updated_at
                 FROM account_gods WHERE account_id = ?1",
                [account],
                |r| {
                    Ok((
                        GodDoc { description: r.get(0)?, stone: r.get(1)?, pose: r.get(2)? },
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, String>(6)?,
                        r.get::<_, i64>(7)?,
                        r.get::<_, i64>(8)?,
                    ))
                },
            )
            .optional()?;
        Ok(row.map(|(fin, original, verdict, reason, model, moderated_at, updated_at)| GodRow {
            original: serde_json::from_str(&original).unwrap_or_else(|_| fin.clone()),
            r#final: fin,
            verdict,
            reason,
            model,
            moderated_at,
            updated_at,
        }))
    }

    fn log_god_tx(
        conn: &rusqlite::Connection,
        account: &str,
        original: &GodDoc,
        fin: Option<&GodDoc>,
        verdict: &str,
        reason: &str,
        categories: &[String],
        model: &str,
        now: i64,
    ) -> AResult<()> {
        conn.execute(
            "INSERT INTO demigod_moderation (account_id, hand, original, final, verdict, reason, categories, model, at)
             VALUES (?1, 'god', ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![account, doc_json(original), fin.map(doc_json), verdict, reason, serde_json::to_string(categories)?, model, now],
        )?;
        conn.execute(
            "DELETE FROM demigod_moderation WHERE account_id = ?1 AND id NOT IN (
                SELECT id FROM demigod_moderation WHERE account_id = ?1 ORDER BY id DESC LIMIT ?2)",
            params![account, MODERATION_LOG_KEEP],
        )?;
        Ok(())
    }

    /// Store a judged god form (`verdict` = `ok` | `rephrase`) and log the pass.
    #[allow(clippy::too_many_arguments)]
    pub fn put_god(
        &self,
        account: &str,
        original: &GodDoc,
        fin: &GodDoc,
        verdict: &str,
        reason: &str,
        categories: &[String],
        model: &str,
    ) -> AResult<GodRow> {
        if !matches!(verdict, "ok" | "rephrase") {
            return Err(AccountError::Invalid("a stored god form is ok or rephrase".into()));
        }
        let now = now_ms();
        {
            let mut conn = self.lock();
            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO account_gods (account_id, description, stone, pose, original, verdict, reason, model,
                    moderated_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
                 ON CONFLICT (account_id) DO UPDATE SET description = excluded.description, stone = excluded.stone,
                    pose = excluded.pose, original = excluded.original, verdict = excluded.verdict,
                    reason = excluded.reason, model = excluded.model, moderated_at = excluded.moderated_at,
                    updated_at = excluded.updated_at",
                params![account, fin.description, fin.stone, fin.pose, doc_json(original), verdict, reason, model, now],
            )?;
            Self::log_god_tx(&tx, account, original, Some(fin), verdict, reason, categories, model, now)?;
            tx.commit()?;
        }
        self.god(account)?.ok_or(AccountError::NotFound)
    }

    /// Log a refused god form (nothing is stored; the previous one, if any, stays).
    pub fn log_god_refusal(&self, account: &str, original: &GodDoc, reason: &str, categories: &[String], model: &str) -> AResult<()> {
        let conn = self.lock();
        Self::log_god_tx(&conn, account, original, None, "refuse", reason, categories, model, now_ms())
    }

    /// Record a preview the account asked for.
    pub fn note_god_preview(&self, account: &str, media_hash: &str, doc: &GodDoc, prompt: &str, charged: bool) -> AResult<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO god_previews (account_id, media_hash, description, stone, pose, prompt, charged, at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![account, media_hash, doc.description, doc.stone, doc.pose, prompt, charged, now_ms()],
        )?;
        Ok(())
    }

    /// New pictures (charged previews) the account asked for since `since_ms`, and the oldest of them.
    pub fn god_previews_since(&self, account: &str, since_ms: i64) -> AResult<(usize, Option<i64>)> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT COUNT(*), MIN(at) FROM god_previews WHERE account_id = ?1 AND charged = 1 AND at > ?2",
            params![account, since_ms],
            |r| Ok((r.get::<_, i64>(0)? as usize, r.get(1)?)),
        )?)
    }

    /// The account's newest preview (or the newest of `media_hash`).
    pub fn god_preview(&self, account: &str, media_hash: Option<&str>) -> AResult<Option<GodPreview>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                "SELECT media_hash, description, stone, pose, prompt, charged, at FROM god_previews
                 WHERE account_id = ?1 AND (?2 IS NULL OR media_hash = ?2) ORDER BY id DESC LIMIT 1",
                params![account, media_hash],
                |r| {
                    Ok(GodPreview {
                        media_hash: r.get(0)?,
                        doc: GodDoc { description: r.get(1)?, stone: r.get(2)?, pose: r.get(3)? },
                        prompt: r.get(4)?,
                        charged: r.get(5)?,
                        at: r.get(6)?,
                    })
                },
            )
            .optional()?)
    }

    /// Make a preview the account's god portrait (accepting the current one again changes nothing).
    pub fn accept_god(&self, account: &str, preview: &GodPreview) -> AResult<GodAccepted> {
        if let Some(cur) = self.accepted_god(account)? {
            if cur.media_hash == preview.media_hash {
                return Ok(cur);
            }
        }
        {
            let conn = self.lock();
            conn.execute(
                "INSERT INTO god_accepted (account_id, media_hash, description, stone, pose, accepted_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![account, preview.media_hash, preview.doc.description, preview.doc.stone, preview.doc.pose, now_ms()],
            )?;
        }
        self.accepted_god(account)?.ok_or(AccountError::NotFound)
    }

    /// The account's god portrait: its newest accepted preview.
    pub fn accepted_god(&self, account: &str) -> AResult<Option<GodAccepted>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                "SELECT media_hash, description, stone, pose, accepted_at FROM god_accepted WHERE account_id = ?1
                 ORDER BY id DESC LIMIT 1",
                [account],
                |r| {
                    Ok(GodAccepted {
                        media_hash: r.get(0)?,
                        doc: GodDoc { description: r.get(1)?, stone: r.get(2)?, pose: r.get(3)? },
                        accepted_at: r.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    /// The account's statues, oldest first.
    pub fn god_statues(&self, account: &str) -> AResult<Vec<GodStatue>> {
        let conn = self.lock();
        let mut st = conn.prepare(
            "SELECT task_id, media_hash, provider, status, credits, asset_id, error, requested_at, finished_at
             FROM god_statues WHERE account_id = ?1 ORDER BY requested_at, task_id",
        )?;
        let rows = st.query_map([account], |r| {
            Ok(GodStatue {
                task_id: r.get(0)?,
                media_hash: r.get(1)?,
                provider: r.get(2)?,
                status: r.get(3)?,
                credits: r.get(4)?,
                asset_id: r.get(5)?,
                error: r.get(6)?,
                requested_at: r.get(7)?,
                finished_at: r.get(8)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// A statue was queued with the provider.
    pub fn add_god_statue(&self, account: &str, task_id: &str, media_hash: &str, provider: &str, credits: Option<f64>) -> AResult<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT OR IGNORE INTO god_statues (task_id, account_id, media_hash, provider, status, credits, requested_at)
             VALUES (?1, ?2, ?3, ?4, 'queued', ?5, ?6)",
            params![task_id, account, media_hash, provider, credits, now_ms()],
        )?;
        Ok(())
    }

    /// A queued statue finished (`ready`, with its model asset) or failed.
    pub fn finish_god_statue(&self, task_id: &str, status: &str, credits: Option<f64>, asset_id: Option<&str>, error: Option<&str>) -> AResult<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE god_statues SET status = ?2, credits = COALESCE(?3, credits), asset_id = COALESCE(?4, asset_id),
                error = ?5, finished_at = ?6 WHERE task_id = ?1",
            params![task_id, status, credits, asset_id, error, now_ms()],
        )?;
        Ok(())
    }
}

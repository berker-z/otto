use crate::types::{
    Account, AccountSettings, BodyRecord, FolderState, MessageRecord, Provider, now_ts,
};
use anyhow::{Context, Result};
use chrono::NaiveDate;
use dirs::home_dir;

use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};
use std::env;
use std::path::{Path, PathBuf};
use tracing::warn;

const DB_FILE_NAME: &str = "otto.db";

#[derive(Clone, Debug, Default)]
pub struct FolderStateUpdate {
    pub uidvalidity: Option<u32>,
    pub highest_uid: Option<u32>,
    pub highestmodseq: Option<u64>,
    pub exists_count: Option<u32>,
    pub last_sync_ts: Option<i64>,
    pub last_uid_scan_ts: Option<i64>,
}

pub type MessageLocationUpdate = (
    String,
    String,
    u32,
    Vec<String>,
    Vec<String>,
    Option<String>,
    Option<i64>,
    Option<u32>,
);

#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
    path: PathBuf,
}

impl Database {
    pub async fn new_default() -> Result<Self> {
        Self::new_named(DB_FILE_NAME).await
    }

    pub async fn new_named(file_name: &str) -> Result<Self> {
        let base = default_data_dir()?;
        let db_path = base.join(file_name);
        let url = format!("sqlite://{}?mode=rwc", db_path.display());

        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating data directory {}", parent.display()))?;
        }

        let pool = SqlitePool::connect(&url)
            .await
            .with_context(|| format!("connecting to sqlite at {}", db_path.display()))?;

        let db = Database {
            pool,
            path: db_path,
        };
        db.migrate().await?;
        Ok(db)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    async fn migrate(&self) -> Result<()> {
        sqlx::query("PRAGMA foreign_keys = ON;")
            .execute(&self.pool)
            .await
            .context("enabling foreign keys")?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS accounts (
                id TEXT PRIMARY KEY,
                email TEXT NOT NULL,
                provider TEXT NOT NULL,
                cutoff_since TEXT NOT NULL,
                poll_interval_minutes INTEGER NOT NULL,
                prefetch_recent INTEGER NOT NULL,
                safe_mode INTEGER NOT NULL,
                folders TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS folders (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id TEXT NOT NULL,
                name TEXT NOT NULL,
                uidvalidity INTEGER,
                highest_uid INTEGER,
                highestmodseq INTEGER,
                exists_count INTEGER,
                last_sync_ts INTEGER,
                last_uid_scan_ts INTEGER,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                UNIQUE(account_id, name),
                FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_folders_account ON folders(account_id);

            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL,
                folder TEXT NOT NULL,
                uid INTEGER,
                thread_id TEXT,
                internal_date INTEGER,
                subject TEXT,
                from_addr TEXT,
                to_addrs TEXT,
                cc_addrs TEXT,
                bcc_addrs TEXT,
                flags TEXT,
                labels TEXT,
                has_attachments INTEGER NOT NULL DEFAULT 0,
                size_bytes INTEGER,
                raw_hash TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_messages_account_folder ON messages(account_id, folder);
            CREATE INDEX IF NOT EXISTS idx_messages_internal_date ON messages(account_id, internal_date DESC);
            CREATE INDEX IF NOT EXISTS idx_messages_account_raw_hash ON messages(account_id, raw_hash);

            CREATE TABLE IF NOT EXISTS bodies (
                message_id TEXT PRIMARY KEY,
                raw_rfc822 BLOB,
                sanitized_text TEXT,
                mime_summary TEXT,
                attachments_json TEXT,
                sanitized_at INTEGER,
                FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE CASCADE
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .context("running migrations")?;

        // Migration: Add highestmodseq column to folders table if it doesn't exist
        // This is for existing databases that were created before this column was added
        let _ = sqlx::query(
            r#"
            ALTER TABLE folders ADD COLUMN highestmodseq INTEGER;
            "#,
        )
        .execute(&self.pool)
        .await;
        // Ignore errors (column might already exist)

        // Migration: Add last_uid_scan_ts column to folders table if it doesn't exist
        let _ = sqlx::query(
            r#"
            ALTER TABLE folders ADD COLUMN last_uid_scan_ts INTEGER;
            "#,
        )
        .execute(&self.pool)
        .await;
        // Ignore errors (column might already exist)

        // Migration: Add exists_count column to folders table if it doesn't exist
        let _ = sqlx::query(
            r#"
            ALTER TABLE folders ADD COLUMN exists_count INTEGER;
            "#,
        )
        .execute(&self.pool)
        .await;
        // Ignore errors (column might already exist)

        Ok(())
    }

    pub async fn save_account(&self, account: &Account) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO accounts (id, email, provider, cutoff_since, poll_interval_minutes, prefetch_recent, safe_mode, folders, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(id) DO UPDATE SET
                email = excluded.email,
                provider = excluded.provider,
                cutoff_since = excluded.cutoff_since,
                poll_interval_minutes = excluded.poll_interval_minutes,
                prefetch_recent = excluded.prefetch_recent,
                safe_mode = excluded.safe_mode,
                folders = excluded.folders,
                updated_at = excluded.updated_at;
            "#,
        )
        .bind(&account.id)
        .bind(&account.email)
        .bind(provider_to_str(&account.provider))
        .bind(account.settings.cutoff_since.to_string())
        .bind(account.settings.poll_interval_minutes as i64)
        .bind(account.settings.prefetch_recent as i64)
        .bind(if account.settings.safe_mode { 1 } else { 0 })
        .bind(serde_json::to_string(&account.settings.folders).unwrap_or_else(|_| "[]".into()))
        .bind(account.created_at)
        .bind(account.updated_at)
        .execute(&self.pool)
        .await
        .context("upserting account")?;
        Ok(())
    }

    pub async fn list_accounts(&self) -> Result<Vec<Account>> {
        let rows = sqlx::query(
            r#"
            SELECT id, email, provider, cutoff_since, poll_interval_minutes, prefetch_recent, safe_mode, folders, created_at, updated_at
            FROM accounts;
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .context("loading accounts")?;

        let mut out = Vec::new();
        for row in rows {
            let cutoff_raw: String = row.get(3);
            let cutoff = NaiveDate::parse_from_str(&cutoff_raw, "%Y-%m-%d")
                .unwrap_or_else(|_| NaiveDate::from_ymd_opt(2025, 12, 1).unwrap());
            let folders_json: String = row.get(7);
            let folders: Vec<String> =
                serde_json::from_str(&folders_json).unwrap_or_else(|_| vec!["INBOX".into()]);
            out.push(Account {
                id: row.get(0),
                email: row.get(1),
                provider: provider_from_str(&row.get::<String, _>(2)),
                settings: AccountSettings {
                    cutoff_since: cutoff,
                    poll_interval_minutes: row.get::<i64, _>(4) as u32,
                    prefetch_recent: row.get::<i64, _>(5) as u32,
                    safe_mode: row.get::<i64, _>(6) == 1,
                    folders,
                },
                created_at: row.get(8),
                updated_at: row.get(9),
            });
        }
        Ok(out)
    }

    pub async fn upsert_folder_state(
        &self,
        account_id: &str,
        name: &str,
        update: &FolderStateUpdate,
    ) -> Result<FolderState> {
        let now = now_ts();
        sqlx::query(
            r#"
            INSERT INTO folders (account_id, name, uidvalidity, highest_uid, highestmodseq, exists_count, last_sync_ts, last_uid_scan_ts, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(account_id, name) DO UPDATE SET
                uidvalidity = excluded.uidvalidity,
                highest_uid = excluded.highest_uid,
                highestmodseq = excluded.highestmodseq,
                exists_count = excluded.exists_count,
                last_sync_ts = excluded.last_sync_ts,
                last_uid_scan_ts = excluded.last_uid_scan_ts,
                updated_at = excluded.updated_at;
            "#,
        )
        .bind(account_id)
        .bind(name)
        .bind(update.uidvalidity.map(|v| v as i64))
        .bind(update.highest_uid.map(|v| v as i64))
        .bind(update.highestmodseq.map(|v| v as i64))
        .bind(update.exists_count.map(|v| v as i64))
        .bind(update.last_sync_ts)
        .bind(update.last_uid_scan_ts)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("upserting folder")?;

        let row = sqlx::query(
            r#"
            SELECT id, uidvalidity, highest_uid, highestmodseq, exists_count, last_sync_ts, last_uid_scan_ts, updated_at
            FROM folders
            WHERE account_id = ?1 AND name = ?2
            "#,
        )
        .bind(account_id)
        .bind(name)
        .fetch_one(&self.pool)
        .await
        .context("reloading folder")?;

        Ok(FolderState {
            id: row.get::<i64, _>(0),
            account_id: account_id.to_string(),
            name: name.to_string(),
            uidvalidity: row.get::<Option<i64>, _>(1).map(|v| v as u32),
            highest_uid: row.get::<Option<i64>, _>(2).map(|v| v as u32),
            highestmodseq: row.get::<Option<i64>, _>(3).map(|v| v as u64),
            exists_count: row.get::<Option<i64>, _>(4).map(|v| v as u32),
            last_sync_ts: row.get::<Option<i64>, _>(5),
            last_uid_scan_ts: row.get::<Option<i64>, _>(6),
        })
    }

    pub async fn list_folders(&self, account_id: &str) -> Result<Vec<FolderState>> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, uidvalidity, highest_uid, highestmodseq, exists_count, last_sync_ts, last_uid_scan_ts
            FROM folders
            WHERE account_id = ?1
            ORDER BY name ASC;
            "#,
        )
        .bind(account_id)
        .fetch_all(&self.pool)
        .await
        .context("loading folders")?;

        let mut out = Vec::new();
        for row in rows {
            out.push(FolderState {
                id: row.get(0),
                account_id: account_id.to_string(),
                name: row.get(1),
                uidvalidity: row.get::<Option<i64>, _>(2).map(|v| v as u32),
                highest_uid: row.get::<Option<i64>, _>(3).map(|v| v as u32),
                highestmodseq: row.get::<Option<i64>, _>(4).map(|v| v as u64),
                exists_count: row.get::<Option<i64>, _>(5).map(|v| v as u32),
                last_sync_ts: row.get(6),
                last_uid_scan_ts: row.get(7),
            });
        }
        Ok(out)
    }

    pub async fn load_message_ids_by_uids(
        &self,
        account_id: &str,
        folder: &str,
        uids: &[u32],
    ) -> Result<std::collections::HashMap<u32, String>> {
        use std::collections::HashMap;

        if uids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut qb: QueryBuilder<Sqlite> =
            QueryBuilder::new("SELECT uid, id FROM messages WHERE account_id = ");
        qb.push_bind(account_id);
        qb.push(" AND folder = ");
        qb.push_bind(folder);
        qb.push(" AND uid IN (");
        {
            let mut separated = qb.separated(", ");
            for uid in uids {
                separated.push_bind(*uid as i64);
            }
        }
        qb.push(")");

        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .context("loading message ids by uid list")?;

        let mut out = HashMap::new();
        for row in rows {
            let uid = row.get::<Option<i64>, _>(0).map(|v| v as u32).unwrap_or(0);
            let id: String = row.get(1);
            if uid > 0 {
                out.insert(uid, id);
            }
        }

        Ok(out)
    }

    pub async fn load_uid_to_message_id_map_by_folder(
        &self,
        account_id: &str,
        folder: &str,
    ) -> Result<std::collections::HashMap<u32, String>> {
        use std::collections::HashMap;

        let rows = sqlx::query(
            r#"
            SELECT uid, id
            FROM messages
            WHERE account_id = ?1 AND folder = ?2 AND uid IS NOT NULL;
            "#,
        )
        .bind(account_id)
        .bind(folder)
        .fetch_all(&self.pool)
        .await
        .context("loading message uid map by folder")?;

        let mut out = HashMap::new();
        for row in rows {
            let uid = row.get::<Option<i64>, _>(0).map(|v| v as u32).unwrap_or(0);
            let id: String = row.get(1);
            if uid > 0 {
                out.insert(uid, id);
            }
        }
        Ok(out)
    }

    pub async fn batch_update_message_flags_by_uid(
        &self,
        account_id: &str,
        folder: &str,
        updates: &[(u32, Vec<String>, Vec<String>)],
    ) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        let now = now_ts();
        let mut tx = self.pool.begin().await.context("beginning transaction")?;

        for (uid, flags, labels) in updates {
            sqlx::query(
                r#"
                UPDATE messages
                SET flags = ?1, labels = ?2, updated_at = ?3
                WHERE account_id = ?4 AND folder = ?5 AND uid = ?6;
                "#,
            )
            .bind(serde_json::to_string(flags).unwrap_or_else(|_| "[]".into()))
            .bind(serde_json::to_string(labels).unwrap_or_else(|_| "[]".into()))
            .bind(now)
            .bind(account_id)
            .bind(folder)
            .bind(*uid as i64)
            .execute(&mut *tx)
            .await
            .context("updating message flags/labels")?;
        }

        tx.commit().await.context("committing flag update tx")?;
        Ok(())
    }

    pub async fn load_existing_message_ids(
        &self,
        account_id: &str,
        ids: &[String],
    ) -> Result<std::collections::HashSet<String>> {
        use std::collections::HashSet;

        if ids.is_empty() {
            return Ok(HashSet::new());
        }

        let mut qb: QueryBuilder<Sqlite> =
            QueryBuilder::new("SELECT id FROM messages WHERE account_id = ");
        qb.push_bind(account_id);
        qb.push(" AND id IN (");
        {
            let mut separated = qb.separated(", ");
            for id in ids {
                separated.push_bind(id);
            }
        }
        qb.push(")");

        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .context("loading existing message ids")?;

        let mut out = HashSet::new();
        for row in rows {
            let id: String = row.get(0);
            out.insert(id);
        }
        Ok(out)
    }

    pub async fn batch_update_message_location_by_id(
        &self,
        account_id: &str,
        updates: &[MessageLocationUpdate],
    ) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        let now = now_ts();
        let mut tx = self.pool.begin().await.context("beginning transaction")?;

        for (message_id, folder, uid, flags, labels, thread_id, internal_date, size_bytes) in
            updates
        {
            sqlx::query(
                r#"
                UPDATE messages
                SET folder = ?1,
                    uid = ?2,
                    flags = ?3,
                    labels = ?4,
                    thread_id = ?5,
                    internal_date = ?6,
                    size_bytes = ?7,
                    updated_at = ?8
                WHERE account_id = ?9 AND id = ?10;
                "#,
            )
            .bind(folder)
            .bind(*uid as i64)
            .bind(serde_json::to_string(flags).unwrap_or_else(|_| "[]".into()))
            .bind(serde_json::to_string(labels).unwrap_or_else(|_| "[]".into()))
            .bind(thread_id)
            .bind(internal_date)
            .bind(size_bytes.map(|v| v as i64))
            .bind(now)
            .bind(account_id)
            .bind(message_id)
            .execute(&mut *tx)
            .await
            .context("updating message location")?;
        }

        tx.commit().await.context("committing location update tx")?;
        Ok(())
    }

    /// Deletes "fallback-id" duplicates (e.g. `account:folder:uid`) when a stable Gmail id row
    /// already exists for the same raw message bytes (`raw_hash`).
    ///
    /// This is a local-only cleanup (no IMAP calls) that helps migrate older DBs that were created
    /// before `X-GM-MSGID` was extracted.
    pub async fn dedupe_fallback_messages_by_raw_hash(
        &self,
        account_id: &str,
        limit: usize,
    ) -> Result<usize> {
        // Find duplicates where:
        // - one row has a numeric id (Gmail X-GM-MSGID stored as string)
        // - another row has a fallback id containing ':' (legacy format)
        // - both share the same raw_hash
        let rows = sqlx::query(
            r#"
            SELECT legacy.id
            FROM messages AS stable
            JOIN messages AS legacy
              ON legacy.account_id = stable.account_id
             AND legacy.raw_hash = stable.raw_hash
            WHERE stable.account_id = ?1
              AND stable.raw_hash IS NOT NULL
              AND stable.id GLOB '[0-9]*'
              AND legacy.id LIKE '%:%'
              AND legacy.id != stable.id
            LIMIT ?2;
            "#,
        )
        .bind(account_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .context("finding fallback duplicates")?;

        let mut deleted = 0usize;
        for row in rows {
            let legacy_id: String = row.get(0);
            self.delete_message(&legacy_id).await?;
            deleted += 1;
        }

        Ok(deleted)
    }

    pub async fn upsert_message(
        &self,
        message: &MessageRecord,
        body: Option<&BodyRecord>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO messages (
                id, account_id, folder, uid, thread_id, internal_date,
                subject, from_addr, to_addrs, cc_addrs, bcc_addrs,
                flags, labels, has_attachments, size_bytes, raw_hash,
                created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
            ON CONFLICT(id) DO UPDATE SET
                account_id = excluded.account_id,
                folder = excluded.folder,
                uid = excluded.uid,
                thread_id = excluded.thread_id,
                internal_date = excluded.internal_date,
                subject = excluded.subject,
                from_addr = excluded.from_addr,
                to_addrs = excluded.to_addrs,
                cc_addrs = excluded.cc_addrs,
                bcc_addrs = excluded.bcc_addrs,
                flags = excluded.flags,
                labels = excluded.labels,
                has_attachments = excluded.has_attachments,
                size_bytes = excluded.size_bytes,
                raw_hash = excluded.raw_hash,
                updated_at = excluded.updated_at;
            "#,
        )
        .bind(&message.id)
        .bind(&message.account_id)
        .bind(&message.folder)
        .bind(message.uid.map(|v| v as i64))
        .bind(&message.thread_id)
        .bind(message.internal_date)
        .bind(&message.subject)
        .bind(&message.from)
        .bind(&message.to)
        .bind(&message.cc)
        .bind(&message.bcc)
        .bind(serde_json::to_string(&message.flags).unwrap_or_else(|_| "[]".into()))
        .bind(serde_json::to_string(&message.labels).unwrap_or_else(|_| "[]".into()))
        .bind(if message.has_attachments { 1 } else { 0 })
        .bind(message.size_bytes.map(|v| v as i64))
        .bind(&message.raw_hash)
        .bind(message.created_at)
        .bind(message.updated_at)
        .execute(&self.pool)
        .await
        .context("upserting message")?;

        if let Some(body) = body {
            sqlx::query(
                r#"
                INSERT INTO bodies (message_id, raw_rfc822, sanitized_text, mime_summary, attachments_json, sanitized_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ON CONFLICT(message_id) DO UPDATE SET
                    raw_rfc822 = excluded.raw_rfc822,
                    sanitized_text = excluded.sanitized_text,
                    mime_summary = excluded.mime_summary,
                    attachments_json = excluded.attachments_json,
                    sanitized_at = excluded.sanitized_at;
                "#,
            )
            .bind(&body.message_id)
            .bind(&body.raw_rfc822)
            .bind(&body.sanitized_text)
            .bind(&body.mime_summary)
            .bind(&body.attachments_json)
            .bind(body.sanitized_at)
            .execute(&self.pool)
            .await
            .context("upserting body")?;
        }

        Ok(())
    }

    pub async fn load_messages(
        &self,
        account_id: &str,
        limit: usize,
    ) -> Result<Vec<(MessageRecord, Option<BodyRecord>)>> {
        let rows = sqlx::query(
            r#"
            SELECT id, folder, uid, thread_id, internal_date, subject, from_addr, to_addrs, cc_addrs, bcc_addrs,
                   flags, labels, has_attachments, size_bytes, raw_hash, created_at, updated_at
            FROM messages
            WHERE account_id = ?1
            ORDER BY internal_date DESC NULLS LAST
            LIMIT ?2;
            "#,
        )
        .bind(account_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .context("loading messages")?;

        let mut out = Vec::new();
        for row in rows {
            let flags: Vec<String> =
                serde_json::from_str(&row.get::<String, _>(10)).unwrap_or_default();
            let labels: Vec<String> =
                serde_json::from_str(&row.get::<String, _>(11)).unwrap_or_default();
            let msg_id: String = row.get(0);
            let body = sqlx::query(
                r#"
                SELECT raw_rfc822, sanitized_text, mime_summary, attachments_json, sanitized_at
                FROM bodies
                WHERE message_id = ?1
                "#,
            )
            .bind(&msg_id)
            .fetch_optional(&self.pool)
            .await
            .context("loading body")?
            .map(|brow| BodyRecord {
                message_id: msg_id.clone(),
                raw_rfc822: brow.get::<Option<Vec<u8>>, _>(0),
                sanitized_text: brow.get::<Option<String>, _>(1),
                mime_summary: brow.get::<Option<String>, _>(2),
                attachments_json: brow.get::<Option<String>, _>(3),
                sanitized_at: brow.get::<Option<i64>, _>(4),
            });

            out.push((
                MessageRecord {
                    id: msg_id,
                    account_id: account_id.to_string(),
                    folder: row.get(1),
                    uid: row.get::<Option<i64>, _>(2).map(|v| v as u32),
                    thread_id: row.get(3),
                    internal_date: row.get(4),
                    subject: row.get(5),
                    from: row.get(6),
                    to: row.get(7),
                    cc: row.get(8),
                    bcc: row.get(9),
                    flags,
                    labels,
                    has_attachments: row.get::<i64, _>(12) == 1,
                    size_bytes: row.get::<Option<i64>, _>(13).map(|v| v as u32),
                    raw_hash: row.get(14),
                    created_at: row.get(15),
                    updated_at: row.get(16),
                },
                body,
            ));
        }

        Ok(out)
    }

    pub async fn load_messages_by_folder(
        &self,
        account_id: &str,
        folder: &str,
        limit: usize,
    ) -> Result<Vec<MessageRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, folder, uid, thread_id, internal_date, subject, from_addr, to_addrs, cc_addrs, bcc_addrs,
                   flags, labels, has_attachments, size_bytes, raw_hash, created_at, updated_at
            FROM messages
            WHERE account_id = ?1 AND folder = ?2
            ORDER BY internal_date DESC NULLS LAST
            LIMIT ?3;
            "#,
        )
        .bind(account_id)
        .bind(folder)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .context("loading messages by folder")?;

        let mut out = Vec::new();
        for row in rows {
            let flags: Vec<String> =
                serde_json::from_str(&row.get::<String, _>(10)).unwrap_or_default();
            let labels: Vec<String> =
                serde_json::from_str(&row.get::<String, _>(11)).unwrap_or_default();

            out.push(MessageRecord {
                id: row.get(0),
                account_id: account_id.to_string(),
                folder: row.get(1),
                uid: row.get::<Option<i64>, _>(2).map(|v| v as u32),
                thread_id: row.get(3),
                internal_date: row.get(4),
                subject: row.get(5),
                from: row.get(6),
                to: row.get(7),
                cc: row.get(8),
                bcc: row.get(9),
                flags,
                labels,
                has_attachments: row.get::<i64, _>(12) == 1,
                size_bytes: row.get::<Option<i64>, _>(13).map(|v| v as u32),
                raw_hash: row.get(14),
                created_at: row.get(15),
                updated_at: row.get(16),
            });
        }

        Ok(out)
    }

    pub async fn upsert_body(&self, body: &BodyRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO bodies (message_id, raw_rfc822, sanitized_text, mime_summary, attachments_json, sanitized_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(message_id) DO UPDATE SET
                raw_rfc822 = excluded.raw_rfc822,
                sanitized_text = excluded.sanitized_text,
                mime_summary = excluded.mime_summary,
                attachments_json = excluded.attachments_json,
                sanitized_at = excluded.sanitized_at;
            "#,
        )
        .bind(&body.message_id)
        .bind(&body.raw_rfc822)
        .bind(&body.sanitized_text)
        .bind(&body.mime_summary)
        .bind(&body.attachments_json)
        .bind(body.sanitized_at)
        .execute(&self.pool)
        .await
        .context("upserting body")?;
        Ok(())
    }

    /// Batch upsert messages and bodies in a single transaction for maximum performance
    pub async fn batch_upsert_messages_with_bodies(
        &self,
        messages: &[MessageRecord],
        bodies: &[BodyRecord],
    ) -> Result<()> {
        if messages.is_empty() {
            return Ok(());
        }

        if messages.len() != bodies.len() {
            anyhow::bail!("messages and bodies length mismatch");
        }

        // Use a transaction for atomic batch write
        let mut tx = self.pool.begin().await.context("beginning transaction")?;

        for (message, body) in messages.iter().zip(bodies.iter()) {
            // Insert/update message
            sqlx::query(
                r#"
                INSERT INTO messages (
                    id, account_id, folder, uid, thread_id, internal_date,
                    subject, from_addr, to_addrs, cc_addrs, bcc_addrs,
                    flags, labels, has_attachments, size_bytes, raw_hash,
                    created_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
                ON CONFLICT(id) DO UPDATE SET
                    account_id = excluded.account_id,
                    folder = excluded.folder,
                    uid = excluded.uid,
                    thread_id = excluded.thread_id,
                    internal_date = excluded.internal_date,
                    subject = excluded.subject,
                    from_addr = excluded.from_addr,
                    to_addrs = excluded.to_addrs,
                    cc_addrs = excluded.cc_addrs,
                    bcc_addrs = excluded.bcc_addrs,
                    flags = excluded.flags,
                    labels = excluded.labels,
                    has_attachments = excluded.has_attachments,
                    size_bytes = excluded.size_bytes,
                    raw_hash = excluded.raw_hash,
                    updated_at = excluded.updated_at;
                "#,
            )
            .bind(&message.id)
            .bind(&message.account_id)
            .bind(&message.folder)
            .bind(message.uid.map(|v| v as i64))
            .bind(&message.thread_id)
            .bind(message.internal_date)
            .bind(&message.subject)
            .bind(&message.from)
            .bind(&message.to)
            .bind(&message.cc)
            .bind(&message.bcc)
            .bind(serde_json::to_string(&message.flags).unwrap_or_else(|_| "[]".into()))
            .bind(serde_json::to_string(&message.labels).unwrap_or_else(|_| "[]".into()))
            .bind(if message.has_attachments { 1 } else { 0 })
            .bind(message.size_bytes.map(|v| v as i64))
            .bind(&message.raw_hash)
            .bind(message.created_at)
            .bind(message.updated_at)
            .execute(&mut *tx)
            .await
            .context("batch upserting message")?;

            // Insert/update body
            sqlx::query(
                r#"
                INSERT INTO bodies (message_id, raw_rfc822, sanitized_text, mime_summary, attachments_json, sanitized_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ON CONFLICT(message_id) DO UPDATE SET
                    raw_rfc822 = excluded.raw_rfc822,
                    sanitized_text = excluded.sanitized_text,
                    mime_summary = excluded.mime_summary,
                    attachments_json = excluded.attachments_json,
                    sanitized_at = excluded.sanitized_at;
                "#,
            )
            .bind(&body.message_id)
            .bind(&body.raw_rfc822)
            .bind(&body.sanitized_text)
            .bind(&body.mime_summary)
            .bind(&body.attachments_json)
            .bind(body.sanitized_at)
            .execute(&mut *tx)
            .await
            .context("batch upserting body")?;
        }

        // Commit the entire batch atomically
        tx.commit().await.context("committing batch transaction")?;

        Ok(())
    }

    pub async fn delete_message(&self, message_id: &str) -> Result<()> {
        // Delete body first (foreign key constraint)
        sqlx::query("DELETE FROM bodies WHERE message_id = ?1")
            .bind(message_id)
            .execute(&self.pool)
            .await
            .context("deleting body")?;

        // Delete message
        sqlx::query("DELETE FROM messages WHERE id = ?1")
            .bind(message_id)
            .execute(&self.pool)
            .await
            .context("deleting message")?;

        Ok(())
    }

    pub async fn delete_messages_by_folder(&self, account_id: &str, folder: &str) -> Result<u64> {
        let mut tx = self.pool.begin().await.context("beginning delete tx")?;

        sqlx::query(
            r#"
            DELETE FROM bodies
            WHERE message_id IN (
                SELECT id FROM messages WHERE account_id = ?1 AND folder = ?2
            );
            "#,
        )
        .bind(account_id)
        .bind(folder)
        .execute(&mut *tx)
        .await
        .context("deleting bodies by folder")?;

        let res = sqlx::query("DELETE FROM messages WHERE account_id = ?1 AND folder = ?2;")
            .bind(account_id)
            .bind(folder)
            .execute(&mut *tx)
            .await
            .context("deleting messages by folder")?;

        tx.commit().await.context("committing delete tx")?;
        Ok(res.rows_affected())
    }

    pub async fn delete_messages_by_folder_and_uids(
        &self,
        account_id: &str,
        folder: &str,
        uids: &[u32],
    ) -> Result<u64> {
        if uids.is_empty() {
            return Ok(0);
        }

        let mut tx = self.pool.begin().await.context("beginning delete tx")?;

        // Delete bodies first to be robust even if foreign key cascading is misconfigured.
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "DELETE FROM bodies WHERE message_id IN (SELECT id FROM messages WHERE account_id = ",
        );
        qb.push_bind(account_id);
        qb.push(" AND folder = ");
        qb.push_bind(folder);
        qb.push(" AND uid IN (");
        {
            let mut separated = qb.separated(", ");
            for uid in uids {
                separated.push_bind(*uid as i64);
            }
        }
        qb.push("))");

        qb.build()
            .execute(&mut *tx)
            .await
            .context("deleting bodies by uid list")?;

        let mut qb: QueryBuilder<Sqlite> =
            QueryBuilder::new("DELETE FROM messages WHERE account_id = ");
        qb.push_bind(account_id);
        qb.push(" AND folder = ");
        qb.push_bind(folder);
        qb.push(" AND uid IN (");
        {
            let mut separated = qb.separated(", ");
            for uid in uids {
                separated.push_bind(*uid as i64);
            }
        }
        qb.push(")");

        let res = qb
            .build()
            .execute(&mut *tx)
            .await
            .context("deleting messages by uid list")?;

        tx.commit().await.context("committing delete tx")?;
        Ok(res.rows_affected())
    }
}

pub(crate) fn default_data_dir() -> Result<PathBuf> {
    if let Ok(custom) = env::var("OTTO_DATA_DIR") {
        let path = PathBuf::from(custom);
        std::fs::create_dir_all(&path)
            .with_context(|| format!("creating OTTO_DATA_DIR at {}", path.display()))?;
        return Ok(path);
    }

    if let Some(home) = home_dir() {
        let path = home.join("otto");
        if std::fs::create_dir_all(&path).is_ok() {
            return Ok(path);
        } else {
            warn!(
                "Unable to create {}/otto; falling back to workspace-local storage",
                home.display()
            );
        }
    }

    let cwd = env::current_dir().context("determining current directory")?;
    let path = cwd.join("otto-data");
    std::fs::create_dir_all(&path)
        .with_context(|| format!("creating fallback data directory {}", path.display()))?;
    Ok(path)
}

fn provider_to_str(provider: &Provider) -> String {
    match provider {
        Provider::GmailImap => "gmail-imap".to_string(),
    }
}

fn provider_from_str(raw: &str) -> Provider {
    match raw {
        "gmail-imap" => Provider::GmailImap,
        _ => Provider::GmailImap,
    }
}

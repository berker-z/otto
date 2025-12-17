use anyhow::{Context, Result};
use chrono::Utc;
use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone)]
pub struct PendingOp {
    pub id: i64,
    pub account_id: String,
    pub kind: String,
    pub target: String,
    pub payload: Option<String>,
    pub created_at: i64,
}

pub async fn ensure_ops_table(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS pending_ops (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            target TEXT NOT NULL,
            payload TEXT,
            created_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_ops_account ON pending_ops(account_id);
        "#,
    )
    .execute(pool)
    .await
    .context("creating pending_ops table")?;
    Ok(())
}

pub async fn enqueue_op(
    pool: &SqlitePool,
    account_id: &str,
    kind: &str,
    target: &str,
    payload: Option<String>,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO pending_ops (account_id, kind, target, payload, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5);
        "#,
    )
    .bind(account_id)
    .bind(kind)
    .bind(target)
    .bind(payload)
    .bind(Utc::now().timestamp())
    .execute(pool)
    .await
    .context("enqueue pending op")?;
    Ok(())
}

pub async fn list_ops(pool: &SqlitePool, account_id: &str) -> Result<Vec<PendingOp>> {
    let rows = sqlx::query(
        r#"
        SELECT id, account_id, kind, target, payload, created_at
        FROM pending_ops
        WHERE account_id = ?1
        ORDER BY created_at ASC;
        "#,
    )
    .bind(account_id)
    .fetch_all(pool)
    .await
    .context("list pending ops")?;

    let mut ops = Vec::new();
    for row in rows {
        ops.push(PendingOp {
            id: row.get(0),
            account_id: row.get(1),
            kind: row.get(2),
            target: row.get(3),
            payload: row.get(4),
            created_at: row.get(5),
        });
    }
    Ok(ops)
}

pub async fn count_ops(pool: &SqlitePool, account_id: &str) -> Result<i64> {
    let row = sqlx::query("SELECT COUNT(*) FROM pending_ops WHERE account_id = ?1")
        .bind(account_id)
        .fetch_one(pool)
        .await
        .context("count pending ops")?;
    Ok(row.get::<i64, _>(0))
}

pub async fn clear_op(pool: &SqlitePool, id: i64) -> Result<()> {
    sqlx::query("DELETE FROM pending_ops WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await
        .context("clear pending op")?;
    Ok(())
}

use anyhow::Result;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Row, SqlitePool};

use crate::types::{AgentRecord, EventRecord};

pub async fn connect(url: &str) -> Result<SqlitePool> {
    if let Some(path) = url.strip_prefix("sqlite://") {
        let file = path.split('?').next().unwrap_or(path);
        if let Some(parent) = std::path::Path::new(file).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
    }
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(url)
        .await?;
    migrate(&pool).await?;
    Ok(pool)
}

async fn migrate(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            job_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            tx_hash TEXT,
            detail TEXT,
            created_at TEXT NOT NULL
        );
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_events_job ON events(job_id, id)")
        .execute(pool)
        .await?;
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS agents (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            account_id TEXT NOT NULL,
            description TEXT NOT NULL,
            runtime TEXT NOT NULL
        );
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS patches (
            hash TEXT PRIMARY KEY,
            content TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        "#,
    )
    .execute(pool)
    .await?;
    // Evaluator artifacts + independent re-run storage (added in v2).
    let _ = sqlx::query("ALTER TABLE patches ADD COLUMN logs TEXT")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE patches ADD COLUMN logs_hash TEXT")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE patches ADD COLUMN rerun_logs TEXT")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE patches ADD COLUMN rerun_logs_hash TEXT")
        .execute(pool)
        .await;
    Ok(())
}

pub async fn seed_agents(pool: &SqlitePool, a_account: &str, b_account: &str) -> Result<()> {
    let agents = [
        (
            "forge",
            "Forge",
            a_account,
            "Live agent node. Bids and submits with a scoped function-call key; bond locked from its pool.",
            "opencode",
        ),
        (
            "sable",
            "Sable",
            b_account,
            "Fast but unverified agent. Its patches face the same acceptance test and bond.",
            "reference",
        ),
    ];
    for (id, name, account, description, runtime) in agents {
        sqlx::query(
            "INSERT OR IGNORE INTO agents (id, name, account_id, description, runtime) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(name)
        .bind(account)
        .bind(description)
        .bind(runtime)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn register_agent(
    pool: &SqlitePool,
    id: &str,
    name: &str,
    account_id: &str,
    description: &str,
    runtime: &str,
) -> Result<bool> {
    let result = sqlx::query(
        "INSERT OR IGNORE INTO agents (id, name, account_id, description, runtime) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(name)
    .bind(account_id)
    .bind(description)
    .bind(runtime)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn list_agents(pool: &SqlitePool) -> Result<Vec<AgentRecord>> {
    let rows =
        sqlx::query("SELECT id, name, account_id, description, runtime FROM agents ORDER BY id")
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .map(|row| AgentRecord {
            id: row.get("id"),
            name: row.get("name"),
            account_id: row.get("account_id"),
            description: row.get("description"),
            runtime: row.get("runtime"),
        })
        .collect())
}

pub async fn get_agent(pool: &SqlitePool, id: &str) -> Result<Option<AgentRecord>> {
    let row =
        sqlx::query("SELECT id, name, account_id, description, runtime FROM agents WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|row| AgentRecord {
        id: row.get("id"),
        name: row.get("name"),
        account_id: row.get("account_id"),
        description: row.get("description"),
        runtime: row.get("runtime"),
    }))
}

pub async fn record_event(
    pool: &SqlitePool,
    job_id: &str,
    kind: &str,
    tx_hash: Option<&str>,
    detail: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO events (job_id, kind, tx_hash, detail, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(job_id)
    .bind(kind)
    .bind(tx_hash)
    .bind(detail)
    .bind(chrono::Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn events_for_job(pool: &SqlitePool, job_id: &str) -> Result<Vec<EventRecord>> {
    let rows = sqlx::query(
        "SELECT id, job_id, kind, tx_hash, detail, created_at FROM events WHERE job_id = ? ORDER BY id ASC",
    )
    .bind(job_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| EventRecord {
            id: row.get("id"),
            job_id: row.get("job_id"),
            kind: row.get("kind"),
            tx_hash: row.get("tx_hash"),
            detail: row.get("detail"),
            created_at: row.get("created_at"),
        })
        .collect())
}

pub async fn save_patch(pool: &SqlitePool, hash: &str, content: &str) -> Result<()> {
    sqlx::query("INSERT OR REPLACE INTO patches (hash, content, created_at) VALUES (?, ?, ?)")
        .bind(hash)
        .bind(content)
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn save_patch_logs(
    pool: &SqlitePool,
    hash: &str,
    logs: &str,
    logs_hash: &str,
) -> Result<()> {
    sqlx::query("UPDATE patches SET logs = ?, logs_hash = ? WHERE hash = ?")
        .bind(logs)
        .bind(logs_hash)
        .bind(hash)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn save_rerun(pool: &SqlitePool, hash: &str, logs: &str, logs_hash: &str) -> Result<()> {
    sqlx::query("UPDATE patches SET rerun_logs = ?, rerun_logs_hash = ? WHERE hash = ?")
        .bind(logs)
        .bind(logs_hash)
        .bind(hash)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_patch_record(
    pool: &SqlitePool,
    hash: &str,
) -> Result<Option<(String, Option<String>, Option<String>, Option<String>)>> {
    let row =
        sqlx::query("SELECT content, logs, logs_hash, rerun_logs_hash FROM patches WHERE hash = ?")
            .bind(hash)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|row| {
        (
            row.get("content"),
            row.get("logs"),
            row.get("logs_hash"),
            row.get("rerun_logs_hash"),
        )
    }))
}

pub async fn get_patch(pool: &SqlitePool, hash: &str) -> Result<Option<String>> {
    let row = sqlx::query("SELECT content FROM patches WHERE hash = ?")
        .bind(hash)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|row| row.get("content")))
}

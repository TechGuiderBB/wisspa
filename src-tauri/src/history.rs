use anyhow::{Context, Result};
use once_cell::sync::OnceCell;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, Runtime};

static DB: OnceCell<Mutex<Connection>> = OnceCell::new();

/// Storage cap for the history table: the most recent 1000 rows are kept and
/// older rows are pruned on every insert. Fixed on purpose — no settings UI;
/// the display limit (100 rows) lives in the frontend's `getHistory(100)` call.
const MAX_HISTORY_ROWS: i64 = 1000;

const SCHEMA: &str = r#"
    CREATE TABLE IF NOT EXISTS history (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      timestamp INTEGER NOT NULL,
      mode TEXT NOT NULL,
      active_app TEXT,
      raw_transcript TEXT NOT NULL,
      output TEXT,
      action_id TEXT,
      duration_ms INTEGER,
      status TEXT NOT NULL,
      input_tokens INTEGER,
      output_tokens INTEGER
    );
    CREATE INDEX IF NOT EXISTS idx_history_timestamp ON history(timestamp DESC);
"#;

/// Bring an existing history.db up to the current schema. Additive only:
/// SQLite has no `ADD COLUMN IF NOT EXISTS`, so each column is guarded by a
/// `pragma_table_info` lookup, and no existing column or row is ever touched.
/// Rows written before metering existed keep NULL token columns.
fn migrate(conn: &Connection) -> Result<()> {
    for (column, ddl) in [
        (
            "input_tokens",
            "ALTER TABLE history ADD COLUMN input_tokens INTEGER",
        ),
        (
            "output_tokens",
            "ALTER TABLE history ADD COLUMN output_tokens INTEGER",
        ),
    ] {
        let exists = conn
            .prepare("SELECT 1 FROM pragma_table_info('history') WHERE name = ?1")?
            .exists(params![column])?;
        if !exists {
            conn.execute_batch(ddl)?;
            log::info!("history.db migrated: added column {column}");
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: i64,
    pub timestamp: i64,
    pub mode: String,
    pub active_app: Option<String>,
    pub raw_transcript: String,
    pub output: Option<String>,
    pub action_id: Option<String>,
    pub duration_ms: Option<i64>,
    pub status: String,
    /// Anthropic token accounting for the LLM call(s) behind this entry
    /// (summed when a mode made two calls — prompt rewrite + critique).
    /// None when no usage was captured: cancelled/failed runs, STT-only rows,
    /// or rows written before metering existed.
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct NewEntry {
    pub mode: String,
    pub active_app: Option<String>,
    pub raw_transcript: String,
    pub output: Option<String>,
    pub action_id: Option<String>,
    pub duration_ms: Option<i64>,
    pub status: String,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
}

fn db_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .context("app data dir unavailable")?;
    std::fs::create_dir_all(&dir).context("create app data dir")?;
    Ok(dir.join("history.db"))
}

pub fn initialize<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let path = db_path(app)?;
    let conn = Connection::open(&path).with_context(|| format!("open {}", path.display()))?;
    conn.execute_batch(SCHEMA)?;
    // Existing installs' DBs predate the token columns: add them (no-op on a
    // fresh DB, where SCHEMA already created them).
    migrate(&conn)?;
    DB.set(Mutex::new(conn))
        .map_err(|_| anyhow::anyhow!("history DB already initialised"))?;
    log::info!("history.db ready at {}", path.display());
    Ok(())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn insert(entry: NewEntry) -> Result<()> {
    let Some(db) = DB.get() else {
        return Err(anyhow::anyhow!("history DB not initialised"));
    };
    let guard = db.lock().map_err(|_| anyhow::anyhow!("history mutex"))?;
    insert_into(&guard, entry)
}

/// Insert against an explicit connection, split out from [`insert`] so the
/// write path (and its schema assumptions) is unit-testable on an in-memory
/// DB without touching the process-global one.
fn insert_into(conn: &Connection, entry: NewEntry) -> Result<()> {
    conn.execute(
        "INSERT INTO history (timestamp, mode, active_app, raw_transcript, output, action_id, duration_ms, status, input_tokens, output_tokens)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            now_ms(),
            entry.mode,
            entry.active_app,
            entry.raw_transcript,
            entry.output,
            entry.action_id,
            entry.duration_ms,
            entry.status,
            entry.input_tokens,
            entry.output_tokens,
        ],
    )?;
    prune(conn, MAX_HISTORY_ROWS)?;
    Ok(())
}

/// Delete all but the newest `max` rows. "Oldest" is decided by `id` (the
/// AUTOINCREMENT primary key), i.e. insertion order — ties in `timestamp`
/// (same-millisecond inserts) can't prune the wrong row.
fn prune(conn: &Connection, max: i64) -> Result<()> {
    let deleted = conn.execute(
        "DELETE FROM history
         WHERE id NOT IN (SELECT id FROM history ORDER BY id DESC LIMIT ?1)",
        params![max],
    )?;
    if deleted > 0 {
        log::debug!("history pruned {deleted} row(s) beyond the {max}-row cap");
    }
    Ok(())
}

pub fn recent(limit: i64) -> Result<Vec<Entry>> {
    let Some(db) = DB.get() else {
        return Ok(Vec::new());
    };
    let guard = db.lock().map_err(|_| anyhow::anyhow!("history mutex"))?;
    recent_from(&guard, limit)
}

/// Read against an explicit connection — see [`insert_into`].
fn recent_from(conn: &Connection, limit: i64) -> Result<Vec<Entry>> {
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, mode, active_app, raw_transcript, output, action_id, duration_ms, status, input_tokens, output_tokens
         FROM history
         ORDER BY timestamp DESC
         LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit], |row| {
            Ok(Entry {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                mode: row.get(2)?,
                active_app: row.get(3)?,
                raw_transcript: row.get(4)?,
                output: row.get(5)?,
                action_id: row.get(6)?,
                duration_ms: row.get(7)?,
                status: row.get(8)?,
                input_tokens: row.get(9)?,
                output_tokens: row.get(10)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn clear_all() -> Result<()> {
    let Some(db) = DB.get() else {
        return Err(anyhow::anyhow!("history DB not initialised"));
    };
    let guard = db.lock().map_err(|_| anyhow::anyhow!("history mutex"))?;
    guard.execute("DELETE FROM history", [])?;
    Ok(())
}

pub fn export_csv() -> Result<String> {
    let entries = recent(10_000)?;
    let mut out = String::from(
        "id,timestamp_ms,mode,active_app,raw_transcript,output,action_id,duration_ms,status\n",
    );
    for e in entries {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            e.id,
            e.timestamp,
            csv_escape(&e.mode),
            csv_escape(&e.active_app.unwrap_or_default()),
            csv_escape(&e.raw_transcript),
            csv_escape(&e.output.unwrap_or_default()),
            csv_escape(&e.action_id.unwrap_or_default()),
            e.duration_ms.map(|d| d.to_string()).unwrap_or_default(),
            csv_escape(&e.status),
        ));
    }
    Ok(out)
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        let escaped = s.replace('"', "\"\"");
        format!("\"{escaped}\"")
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(SCHEMA).expect("create schema");
        conn
    }

    fn insert_n(conn: &Connection, n: i64) {
        for i in 1..=n {
            conn.execute(
                "INSERT INTO history (timestamp, mode, raw_transcript, status)
                 VALUES (?1, 'dictation', ?2, 'success')",
                params![i, format!("entry {i}")],
            )
            .expect("insert");
        }
    }

    fn row_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
            .expect("count")
    }

    #[test]
    fn prune_keeps_only_the_newest_rows() {
        let conn = test_conn();
        let over = MAX_HISTORY_ROWS + 25;
        insert_n(&conn, over);
        prune(&conn, MAX_HISTORY_ROWS).expect("prune");
        assert_eq!(row_count(&conn), MAX_HISTORY_ROWS);
        // The survivors must be the most recently inserted rows (highest ids).
        let min_id: i64 = conn
            .query_row("SELECT MIN(id) FROM history", [], |r| r.get(0))
            .expect("min id");
        assert_eq!(min_id, 26, "the oldest 25 rows should have been pruned");
        let max_id: i64 = conn
            .query_row("SELECT MAX(id) FROM history", [], |r| r.get(0))
            .expect("max id");
        assert_eq!(max_id, over, "the newest row must survive");
    }

    #[test]
    fn prune_under_cap_is_a_no_op() {
        let conn = test_conn();
        insert_n(&conn, 10);
        prune(&conn, MAX_HISTORY_ROWS).expect("prune");
        assert_eq!(row_count(&conn), 10);
    }

    /// The pre-metering schema: what an existing install's history.db looks
    /// like before this version's migration runs.
    const SCHEMA_V1: &str = r#"
        CREATE TABLE IF NOT EXISTS history (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          timestamp INTEGER NOT NULL,
          mode TEXT NOT NULL,
          active_app TEXT,
          raw_transcript TEXT NOT NULL,
          output TEXT,
          action_id TEXT,
          duration_ms INTEGER,
          status TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_history_timestamp ON history(timestamp DESC);
    "#;

    fn column_names(conn: &Connection) -> Vec<String> {
        conn.prepare("SELECT name FROM pragma_table_info('history')")
            .expect("table_info")
            .query_map([], |r| r.get(0))
            .expect("query")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("collect")
    }

    #[test]
    fn migration_adds_token_columns_to_old_schema() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(SCHEMA_V1).expect("create v1 schema");
        // A row written by the old build must survive the migration untouched.
        conn.execute(
            "INSERT INTO history (timestamp, mode, raw_transcript, status)
             VALUES (1, 'dictation', 'legacy entry', 'success')",
            [],
        )
        .expect("insert legacy row");

        migrate(&conn).expect("migrate");
        let cols = column_names(&conn);
        assert!(cols.iter().any(|c| c == "input_tokens"), "cols: {cols:?}");
        assert!(cols.iter().any(|c| c == "output_tokens"), "cols: {cols:?}");

        // Idempotent: a second run (e.g. next app launch) is a no-op.
        migrate(&conn).expect("re-migrate must not fail");

        // The legacy row reads back with NULL tokens (no usage was captured).
        let rows = recent_from(&conn, 10).expect("recent");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].raw_transcript, "legacy entry");
        assert_eq!(rows[0].input_tokens, None);
        assert_eq!(rows[0].output_tokens, None);
    }

    #[test]
    fn migrated_old_schema_accepts_metered_inserts() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(SCHEMA_V1).expect("create v1 schema");
        migrate(&conn).expect("migrate");
        insert_into(
            &conn,
            NewEntry {
                mode: "prompt".to_string(),
                raw_transcript: "raw".to_string(),
                output: Some("rewritten".to_string()),
                status: "success".to_string(),
                input_tokens: Some(2100),
                output_tokens: Some(230),
                ..Default::default()
            },
        )
        .expect("metered insert into migrated db");
        let rows = recent_from(&conn, 10).expect("recent");
        assert_eq!(rows[0].input_tokens, Some(2100));
        assert_eq!(rows[0].output_tokens, Some(230));
    }

    #[test]
    fn insert_round_trips_token_usage() {
        let conn = test_conn();
        insert_into(
            &conn,
            NewEntry {
                mode: "dictation".to_string(),
                raw_transcript: "raw words".to_string(),
                output: Some("cleaned words".to_string()),
                status: "success".to_string(),
                input_tokens: Some(1500),
                output_tokens: Some(120),
                ..Default::default()
            },
        )
        .expect("insert");
        let rows = recent_from(&conn, 10).expect("recent");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].input_tokens, Some(1500));
        assert_eq!(rows[0].output_tokens, Some(120));
    }

    #[test]
    fn insert_without_usage_keeps_tokens_null() {
        // STT-only / cancelled / pre-LLM-failure rows carry no usage — the
        // columns must stay NULL, not collapse to a misleading 0.
        let conn = test_conn();
        insert_into(
            &conn,
            NewEntry {
                mode: "dictation".to_string(),
                status: "cancelled".to_string(),
                ..Default::default()
            },
        )
        .expect("insert");
        let rows = recent_from(&conn, 10).expect("recent");
        assert_eq!(rows[0].input_tokens, None);
        assert_eq!(rows[0].output_tokens, None);
    }
}

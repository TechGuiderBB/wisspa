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
      status TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_history_timestamp ON history(timestamp DESC);
"#;

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
    guard.execute(
        "INSERT INTO history (timestamp, mode, active_app, raw_transcript, output, action_id, duration_ms, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            now_ms(),
            entry.mode,
            entry.active_app,
            entry.raw_transcript,
            entry.output,
            entry.action_id,
            entry.duration_ms,
            entry.status,
        ],
    )?;
    prune(&guard, MAX_HISTORY_ROWS)?;
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
    let mut stmt = guard.prepare(
        "SELECT id, timestamp, mode, active_app, raw_transcript, output, action_id, duration_ms, status
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
}

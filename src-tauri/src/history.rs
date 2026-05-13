use anyhow::{Context, Result};
use once_cell::sync::OnceCell;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, Runtime};

static DB: OnceCell<Mutex<Connection>> = OnceCell::new();

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
    conn.execute_batch(
        r#"
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
        "#,
    )?;
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
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        let escaped = s.replace('"', "\"\"");
        format!("\"{escaped}\"")
    } else {
        s.to_string()
    }
}

//! File logger with size-based rotation (issue #33).
//!
//! Replaces `env_logger`, which wrote to stderr — redirected to `/dev/null`
//! when macOS Launch Services starts the app, so a shipped build produced no
//! diagnostic artifact at all. This writes to
//! `~/Library/Logs/Wisspa/wisspa.log`, rotating at 5 MB and keeping the last 3
//! files (`wisspa.log`, `wisspa.1.log`, `wisspa.2.log`, `wisspa.3.log`).
//!
//! It is a plain synchronous `log::Log` backend, so all existing `log::info!` /
//! `warn!` / `error!` calls keep working unchanged and there is no background
//! appender thread whose flush-guard could be dropped early (a known footgun
//! with async appenders). It also mirrors to stderr so `pnpm tauri dev` still
//! shows logs in the terminal.

use log::{LevelFilter, Log, Metadata, Record};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

const MAX_BYTES: u64 = 5 * 1024 * 1024;
/// Number of rotated files kept besides the live `wisspa.log`.
const KEEP: usize = 3;

pub fn log_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join("Library").join("Logs").join("Wisspa")
}

pub fn log_path() -> PathBuf {
    log_dir().join("wisspa.log")
}

struct Sink {
    file: File,
    written: u64,
}

struct FileLogger {
    level: LevelFilter,
    dir: PathBuf,
    sink: Mutex<Option<Sink>>,
}

impl FileLogger {
    fn open(&self) -> std::io::Result<Sink> {
        fs::create_dir_all(&self.dir)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("wisspa.log"))?;
        let written = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Sink { file, written })
    }

    /// wisspa.log → wisspa.1.log → wisspa.2.log → wisspa.3.log (oldest dropped).
    fn rotate(&self) {
        let numbered = |n: usize| self.dir.join(format!("wisspa.{n}.log"));
        let _ = fs::remove_file(numbered(KEEP));
        for n in (1..KEEP).rev() {
            let _ = fs::rename(numbered(n), numbered(n + 1));
        }
        let _ = fs::rename(self.dir.join("wisspa.log"), numbered(1));
    }
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        // Epoch millis avoids pulling in a date formatting crate. Good enough
        // for ordering log lines and correlating with a bug report's timestamp.
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let line = format!(
            "{ts} {:<5} {}: {}\n",
            record.level(),
            record.target(),
            record.args()
        );
        // Mirror to stderr (visible under `tauri dev`; /dev/null in a packaged
        // launch, which is exactly why the file sink below exists).
        eprint!("{line}");

        let Ok(mut guard) = self.sink.lock() else {
            return;
        };
        if guard.is_none() {
            match self.open() {
                Ok(s) => *guard = Some(s),
                Err(_) => return,
            }
        }
        if let Some(sink) = guard.as_mut() {
            if sink.file.write_all(line.as_bytes()).is_ok() {
                sink.written += line.len() as u64;
            }
            if sink.written >= MAX_BYTES {
                self.rotate();
                // Reopen a fresh wisspa.log; on failure drop the sink so the
                // next call retries from scratch.
                *guard = self.open().ok();
            }
        }
    }

    fn flush(&self) {
        if let Ok(mut guard) = self.sink.lock() {
            if let Some(sink) = guard.as_mut() {
                let _ = sink.file.flush();
            }
        }
    }
}

/// Install the file logger. Level comes from `RUST_LOG` (a bare level like
/// `debug`) or defaults to `info`. Safe to call once; a second call is ignored.
pub fn init() {
    let level = std::env::var("RUST_LOG")
        .ok()
        .and_then(|s| s.parse::<LevelFilter>().ok())
        .unwrap_or(LevelFilter::Info);
    let logger = FileLogger {
        level,
        dir: log_dir(),
        sink: Mutex::new(None),
    };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(level);
    }
}

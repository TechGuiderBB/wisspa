use super::Action;
use anyhow::{Context, Result};
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode, DebouncedEventKind};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime};

pub const REGISTRY_RELOADED_EVENT: &str = "wisspa://actions-reloaded";

static REGISTRY: Lazy<RwLock<HashMap<String, Action>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

/// Snapshot all loaded actions (cheap clone of the map values).
pub fn snapshot() -> Vec<Action> {
    REGISTRY
        .read()
        .map(|r| r.values().cloned().collect())
        .unwrap_or_default()
}

/// Resolve the user actions directory under macOS app support.
pub fn user_actions_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .context("app data dir unavailable")?
        .join("actions");
    std::fs::create_dir_all(&dir).context("create actions dir")?;
    Ok(dir)
}

/// On first launch, seed the user's actions directory with the bundled defaults
/// from `default-actions/` at the repo root. We only copy files that don't yet
/// exist so the user's edits persist across upgrades.
pub fn seed_defaults_if_empty<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let user_dir = user_actions_dir(app)?;
    let entries = std::fs::read_dir(&user_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "yaml" || x == "yml"))
        .count();
    if entries > 0 {
        return Ok(());
    }

    // Find the source default-actions directory. In dev, it's a sibling of
    // src-tauri/. In a packaged build, ship them inside the bundle resources.
    let candidates = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|p| p.join("default-actions")),
        app.path()
            .resource_dir()
            .ok()
            .map(|p| p.join("default-actions")),
    ];
    let src = candidates
        .into_iter()
        .flatten()
        .find(|p| p.is_dir())
        .ok_or_else(|| anyhow::anyhow!("default-actions source dir not found"))?;

    for entry in std::fs::read_dir(&src)? {
        let entry = entry?;
        let p = entry.path();
        if p.extension().is_some_and(|x| x == "yaml" || x == "yml") {
            let dest = user_dir.join(p.file_name().unwrap());
            std::fs::copy(&p, &dest).with_context(|| format!("copy {p:?}"))?;
        }
    }
    log::info!("seeded default actions into {}", user_dir.display());
    Ok(())
}

/// Load every yaml file in `dir`, populate the global registry. Invalid files
/// log a warning but don't abort the load.
pub fn load_all(dir: &Path) -> Result<usize> {
    let mut map = HashMap::new();
    let mut invalid: Vec<(String, String)> = Vec::new();

    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if !path.extension().is_some_and(|x| x == "yaml" || x == "yml") {
                continue;
            }
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("?").to_string();
            match parse_file(&path) {
                Ok(action) => {
                    if !action.enabled {
                        continue;
                    }
                    map.insert(action.id.clone(), action);
                }
                Err(e) => {
                    log::warn!("invalid action file {name}: {e:#}");
                    invalid.push((name, format!("{e:#}")));
                }
            }
        }
    }

    let count = map.len();
    if let Ok(mut w) = REGISTRY.write() {
        *w = map;
    }
    log::info!("loaded {count} actions ({} invalid)", invalid.len());
    Ok(count)
}

fn parse_file(path: &Path) -> Result<Action> {
    let raw = std::fs::read_to_string(path)?;
    let action: Action = serde_yaml::from_str(&raw).context("parse yaml")?;
    crate::actions::executor::validate(&action)?;
    Ok(action)
}

/// Start watching the user actions directory. On any change, reload + emit.
pub fn start_watcher<R: Runtime>(app: AppHandle<R>) -> Result<()> {
    let dir = user_actions_dir(&app)?;
    let dir_clone = dir.clone();
    let app_handle = app.clone();

    std::thread::Builder::new()
        .name("wisspa-actions-watcher".to_string())
        .spawn(move || {
            let (tx, rx) = std::sync::mpsc::channel();
            let mut debouncer = match new_debouncer(Duration::from_millis(400), tx) {
                Ok(d) => d,
                Err(e) => {
                    log::error!("could not create file watcher: {e:#}");
                    return;
                }
            };
            if let Err(e) = debouncer
                .watcher()
                .watch(&dir_clone, RecursiveMode::NonRecursive)
            {
                log::error!("watcher.watch({}): {e:#}", dir_clone.display());
                return;
            }

            for event in rx {
                match event {
                    Ok(events) => {
                        let touched = events
                            .iter()
                            .any(|e| matches!(e.kind, DebouncedEventKind::Any));
                        if touched {
                            if let Err(e) = load_all(&dir_clone) {
                                log::warn!("reload after watcher event failed: {e:#}");
                            } else {
                                let _ = app_handle.emit(REGISTRY_RELOADED_EVENT, ());
                            }
                        }
                    }
                    Err(e) => log::warn!("watcher error: {e:?}"),
                }
            }
        })?;

    Ok(())
}

/// First-launch initialisation: seed defaults, load, start watcher.
pub fn initialize<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    seed_defaults_if_empty(app)?;
    let dir = user_actions_dir(app)?;
    load_all(&dir)?;
    start_watcher(app.clone())?;
    Ok(())
}

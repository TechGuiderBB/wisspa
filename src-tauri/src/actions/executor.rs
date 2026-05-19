use super::{Action, ActionType};
use anyhow::{anyhow, Context, Result};
use tauri::{AppHandle, Runtime};
use tauri_plugin_clipboard_manager::ClipboardExt;

/// Tokens forbidden inside shell commands per PRD §5.2.3.
const SHELL_DENY: &[&str] = &[
    "sudo ",
    "rm -rf",
    "dd ",
    "mkfs",
    " :(){",
    "curl | sh",
    "curl|sh",
    "wget | sh",
    "wget|sh",
    "| sudo",
    "shutdown",
    "halt ",
    "reboot",
    "killall",
];

/// Validate an action shape on registry load. Currently only enforces the
/// shell allowlist for `shell`/`applescript` payloads; other types are
/// implicitly safe (open_app/open_url/keystroke).
pub fn validate(action: &Action) -> Result<()> {
    match action.action_type {
        ActionType::Shell => check_shell(&action.command),
        ActionType::Applescript => check_applescript(&action.command),
        _ => Ok(()),
    }
}

fn check_shell(cmd: &str) -> Result<()> {
    let lower = cmd.to_lowercase();
    for bad in SHELL_DENY {
        if lower.contains(bad) {
            return Err(anyhow!("shell command rejected (contains '{bad}')"));
        }
    }
    Ok(())
}

fn check_applescript(cmd: &str) -> Result<()> {
    // AppleScript can shell out via `do shell script`; deny that too.
    let lower = cmd.to_lowercase();
    if lower.contains("do shell script") {
        return Err(anyhow!(
            "applescript command rejected (contains 'do shell script')"
        ));
    }
    Ok(())
}

/// POSIX shell single-quote wrap. Any embedded `'` is closed, escaped via
/// `'"'"'`, and re-opened. Safe to splice into a `/bin/sh -c` command;
/// metacharacters inside the wrapped value cannot escape the literal context.
fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\"'\"'");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// AppleScript string-literal wrap. Escapes `\` to `\\` and `"` to `\"`,
/// then wraps in `"…"`. Safe to splice into a string literal in a script
/// passed to `osascript -e`.
fn applescript_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// How to transform each substituted placeholder value before insertion.
#[derive(Clone, Copy)]
enum QuoteMode {
    /// Insert the value as-is. Used by action types that don't pass through
    /// a shell or AppleScript interpreter (open_url, open_app, keystroke).
    Verbatim,
    /// POSIX single-quote wrap each substituted value.
    Shell,
    /// AppleScript string-literal wrap each substituted value.
    Applescript,
}

fn apply_quote(mode: QuoteMode, value: &str) -> String {
    match mode {
        QuoteMode::Verbatim => value.to_string(),
        QuoteMode::Shell => shell_quote(value),
        QuoteMode::Applescript => applescript_quote(value),
    }
}

async fn resolve_with<R: Runtime>(
    app: &AppHandle<R>,
    raw: &str,
    query: &str,
    mode: QuoteMode,
) -> String {
    // Single-pass walk over `raw`. Any text that appears in a substituted
    // value never re-enters the scan range, so a voice query containing the
    // literal text `{clipboard}` cannot trigger a nested clipboard read.
    // Clipboard and active-app are fetched lazily on first reference.
    let mut clipboard: Option<String> = None;
    let mut active_app: Option<String> = None;

    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;

    loop {
        let Some(open) = rest.find('{') else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 1..];
        let Some(close) = after_open.find('}') else {
            // No matching close brace; copy the remainder verbatim and stop.
            out.push_str(&rest[open..]);
            return out;
        };
        let name = &after_open[..close];
        let replacement: Option<String> = match name {
            "query" => Some(apply_quote(mode, query)),
            "clipboard" | "selected_text" => {
                // For v1 we approximate selected text by reading the
                // clipboard. A proper "Cmd+C then read" path lands in
                // Phase 5 for prompt mode.
                if clipboard.is_none() {
                    clipboard = Some(app.clipboard().read_text().unwrap_or_default());
                }
                Some(apply_quote(mode, clipboard.as_deref().unwrap()))
            }
            "active_app" => {
                if active_app.is_none() {
                    active_app = Some(
                        crate::app_detector::frontmost_app_name()
                            .await
                            .unwrap_or_else(|_| "Finder".to_string()),
                    );
                }
                Some(apply_quote(mode, active_app.as_deref().unwrap()))
            }
            _ => None,
        };
        match replacement {
            Some(r) => out.push_str(&r),
            None => {
                // Unknown placeholder name; pass through `{name}` verbatim.
                out.push('{');
                out.push_str(name);
                out.push('}');
            }
        }
        rest = &after_open[close + 1..];
    }
}

/// Verbatim placeholder substitution for non-shell, non-applescript action
/// types (open_url, open_app, keystroke). Values are inserted as-is — these
/// dispatch paths do not invoke a shell interpreter, so there is no
/// metacharacter context to escape against.
pub async fn resolve_placeholders<R: Runtime>(
    app: &AppHandle<R>,
    raw: &str,
    query: &str,
) -> String {
    resolve_with(app, raw, query, QuoteMode::Verbatim).await
}

/// Shell-safe placeholder substitution. Every substituted value is wrapped in
/// POSIX single quotes (with embedded `'` escaped via `'"'"'`), so the result
/// is safe to pass to `/bin/sh -c`. The static command template is
/// pre-validated by `check_shell()` at registry load time; substituted values
/// cannot break out of their single-quoted context.
pub async fn resolve_placeholders_shell<R: Runtime>(
    app: &AppHandle<R>,
    raw: &str,
    query: &str,
) -> String {
    resolve_with(app, raw, query, QuoteMode::Shell).await
}

/// AppleScript-safe placeholder substitution. Every substituted value is
/// wrapped in `"…"` with `\` and `"` escaped, so the result is safe to pass
/// to `osascript -e`. The static template is pre-validated by
/// `check_applescript()` at registry load time; substituted values cannot
/// break out of their string-literal context.
pub async fn resolve_placeholders_applescript<R: Runtime>(
    app: &AppHandle<R>,
    raw: &str,
    query: &str,
) -> String {
    resolve_with(app, raw, query, QuoteMode::Applescript).await
}

/// Outcome of executing an action.
pub struct ExecOutcome {
    pub success: bool,
    pub message: String,
}

pub async fn execute<R: Runtime>(
    app: &AppHandle<R>,
    action: &Action,
    query: &str,
) -> Result<ExecOutcome> {
    let resolved = match action.action_type {
        ActionType::Shell => resolve_placeholders_shell(app, &action.command, query).await,
        ActionType::Applescript => resolve_placeholders_applescript(app, &action.command, query).await,
        _ => resolve_placeholders(app, &action.command, query).await,
    };

    if let Some(outcome) = check_permissions(app, action).await {
        return Ok(outcome);
    }

    if action.destructive {
        let id = crate::actions::pending::store(
            action.clone(),
            resolved.clone(),
            query.to_string(),
        );
        crate::tray::set_pending_confirmation(app, Some(&action.name));
        crate::actions::pending::schedule_timeout(app.clone(), id);
        return Ok(ExecOutcome {
            success: false,
            message: format!(
                "Confirm \"{}\" via the Wisspa tray menu within {}s.",
                action.name,
                crate::actions::pending::CONFIRMATION_TIMEOUT.as_secs()
            ),
        });
    }

    let env = settings_env(app);
    Ok(dispatch(action, &resolved, query, &env).await)
}

/// Dispatch a pre-confirmed action. Skips the destructive gate (the user
/// just confirmed via the tray) but re-checks permissions in case the user
/// revoked something during the confirmation window.
pub async fn run_confirmed<R: Runtime>(
    app: &AppHandle<R>,
    action: &Action,
    resolved: &str,
    query: &str,
) -> ExecOutcome {
    if let Some(outcome) = check_permissions(app, action).await {
        return outcome;
    }
    let env = settings_env(app);
    dispatch(action, resolved, query, &env).await
}

/// Returns Some(failure outcome) if any declared permission is missing —
/// also deep-links the user to the matching System Settings pane.
async fn check_permissions<R: Runtime>(
    _app: &AppHandle<R>,
    action: &Action,
) -> Option<ExecOutcome> {
    let missing = crate::permissions::check_required(&action.requires_permissions).await;
    if missing.is_empty() {
        return None;
    }
    // Deep-link the first missing item that has a real settings pane. An
    // empty pane means the YAML declared an unknown permission key; there's
    // nowhere to send the user, but the gate still fails closed below.
    if let Some(target) = missing.iter().find(|m| !m.pane.is_empty()) {
        let _ = crate::permissions::open_settings_for(target.pane);
    }
    let list = missing
        .iter()
        .map(|m| m.label.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let has_unknown = missing.iter().any(|m| m.pane.is_empty());
    let (noun, pronoun) = if missing.len() == 1 {
        ("permission", "it")
    } else {
        ("permissions", "them")
    };
    let suffix = if has_unknown {
        format!(" Fix the action YAML's requires_permissions list.")
    } else {
        format!(" Opening System Settings — grant {pronoun} and try again.")
    };
    Some(ExecOutcome {
        success: false,
        message: format!("\"{}\" needs {list} {noun}.{suffix}", action.name),
    })
}

async fn dispatch(
    action: &Action,
    resolved: &str,
    query: &str,
    env: &[(String, String)],
) -> ExecOutcome {
    log::info!(
        "executing action '{}' ({:?}) → {}",
        action.id,
        action.action_type,
        resolved
    );
    let result = match action.action_type {
        ActionType::Shell => run_shell(resolved, action.working_dir.as_deref(), env).await,
        ActionType::Applescript => run_applescript(resolved).await,
        ActionType::OpenUrl => open_url(resolved).await,
        ActionType::OpenApp => open_app(resolved).await,
        ActionType::Keystroke => send_keystroke(resolved).await,
    };
    match result {
        Ok(()) => ExecOutcome {
            success: true,
            message: render(&action.success_feedback, resolved, query),
        },
        Err(e) => ExecOutcome {
            success: false,
            message: format!(
                "{}: {e:#}",
                render(&action.failure_feedback, resolved, query)
            ),
        },
    }
}

fn render(template: &str, resolved: &str, query: &str) -> String {
    template
        .replace("{query}", query)
        .replace("{resolved}", resolved)
}

async fn run_shell(
    command: &str,
    working_dir: Option<&str>,
    env: &[(String, String)],
) -> Result<()> {
    // No runtime allowlist check here: the static template was already
    // validated by `check_shell()` at registry load time (and re-validated by
    // the file watcher on YAML changes), and `resolve_placeholders_shell`
    // single-quote-wraps every substituted value so user-spoken content can
    // never inject shell metacharacters into the resolved command.
    let mut cmd = tokio::process::Command::new("/bin/sh");
    cmd.args(["-c", command]);
    if let Some(dir) = working_dir {
        cmd.current_dir(shellexpand_home(dir));
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    let output = cmd.output().await.context("spawn shell")?;
    if !output.status.success() {
        return Err(anyhow!(
            "shell exit {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

/// Surface a few settings as env vars so YAML actions can reference them
/// (e.g. `$WISSPA_NOTES_PATH`). Tilde-expanded; safe even if settings.json
/// is missing.
fn settings_env<R: Runtime>(app: &AppHandle<R>) -> Vec<(String, String)> {
    let mut env = Vec::new();
    if let Ok(settings) = crate::settings_store::load(app) {
        let path = shellexpand_home(&settings.general.notes_path);
        env.push(("WISSPA_NOTES_PATH".to_string(), path));
    }
    env
}

async fn run_applescript(script: &str) -> Result<()> {
    // No runtime allowlist check here: the static template was already
    // validated by `check_applescript()` at registry load time (and re-
    // validated by the file watcher on YAML changes), and
    // `resolve_placeholders_applescript` string-literal-wraps every
    // substituted value so user-spoken content can never break out of its
    // quoted context to inject `do shell script` or similar.
    let output = tokio::process::Command::new("osascript")
        .args(["-e", script])
        .output()
        .await
        .context("spawn osascript")?;
    if !output.status.success() {
        return Err(anyhow!(
            "osascript exit {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

async fn open_url(url: &str) -> Result<()> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(anyhow!("refusing to open non-http URL '{url}'"));
    }
    let output = tokio::process::Command::new("open")
        .arg(url)
        .output()
        .await
        .context("spawn open")?;
    if !output.status.success() {
        return Err(anyhow!("open url exit {}", output.status));
    }
    Ok(())
}

async fn open_app(name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(anyhow!("no app name to open"));
    }
    let output = tokio::process::Command::new("open")
        .args(["-a", name])
        .output()
        .await
        .context("spawn open -a")?;
    if !output.status.success() {
        return Err(anyhow!(
            "open -a exit {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

async fn send_keystroke(combo: &str) -> Result<()> {
    let script = combo_to_applescript(combo)?;
    run_applescript(&script).await
}

fn combo_to_applescript(combo: &str) -> Result<String> {
    // Parse strings like "cmd+shift+5", "ctrl+cmd+q", "cmd+v".
    let lower = combo.to_lowercase();
    let parts: Vec<&str> = lower.split('+').map(|s| s.trim()).collect();
    let (mods, key_part) = parts.split_at(parts.len().saturating_sub(1));
    let key_str = key_part.first().copied().unwrap_or("");

    let mut using: Vec<&str> = Vec::new();
    for m in mods {
        match *m {
            "cmd" | "command" => using.push("command down"),
            "shift" => using.push("shift down"),
            "ctrl" | "control" => using.push("control down"),
            "alt" | "opt" | "option" => using.push("option down"),
            "fn" => {
                // fn isn't reachable via System Events keystroke in v1; bail loudly.
                return Err(anyhow!(
                    "fn modifier is not supported by AppleScript keystroke"
                ));
            }
            other => return Err(anyhow!("unknown modifier '{other}'")),
        }
    }

    // Special keys → key code numbers (subset of common ones).
    let key_code: Option<u16> = match key_str {
        "return" | "enter" => Some(36),
        "tab" => Some(48),
        "space" => Some(49),
        "esc" | "escape" => Some(53),
        "left" => Some(123),
        "right" => Some(124),
        "down" => Some(125),
        "up" => Some(126),
        "delete" | "backspace" => Some(51),
        "f1" => Some(122),
        "f2" => Some(120),
        "f3" => Some(99),
        "f4" => Some(118),
        "f5" => Some(96),
        "f6" => Some(97),
        "f7" => Some(98),
        "f8" => Some(100),
        "f9" => Some(101),
        "f10" => Some(109),
        "f11" => Some(103),
        "f12" => Some(111),
        _ => None,
    };

    let using_clause = if using.is_empty() {
        String::new()
    } else {
        format!(" using {{{}}}", using.join(", "))
    };

    if let Some(code) = key_code {
        return Ok(format!(
            r#"tell application "System Events" to key code {code}{using_clause}"#
        ));
    }

    // Default: keystroke a single character.
    if key_str.len() != 1 {
        return Err(anyhow!(
            "keystroke '{key_str}' is not a single character or known special key"
        ));
    }
    let esc = key_str.replace('\\', "\\\\").replace('"', "\\\"");
    Ok(format!(
        r#"tell application "System Events" to keystroke "{esc}"{using_clause}"#
    ))
}

fn shellexpand_home(path: &str) -> String {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs_home() {
            return home.join(stripped).to_string_lossy().into_owned();
        }
    }
    path.to_string()
}

fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

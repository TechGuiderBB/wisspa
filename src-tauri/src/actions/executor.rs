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

/// Substitute {query}, {clipboard}, {selected_text}, {active_app} placeholders.
pub async fn resolve_placeholders<R: Runtime>(
    app: &AppHandle<R>,
    raw: &str,
    query: &str,
) -> String {
    let mut out = raw.to_string();
    out = out.replace("{query}", query);

    if out.contains("{clipboard}") {
        let clip = app.clipboard().read_text().unwrap_or_default();
        out = out.replace("{clipboard}", &clip);
    }
    if out.contains("{selected_text}") {
        // For v1 we approximate selected text by reading the clipboard. A
        // proper "Cmd+C then read" path lands in Phase 5 for prompt mode.
        let clip = app.clipboard().read_text().unwrap_or_default();
        out = out.replace("{selected_text}", &clip);
    }
    if out.contains("{active_app}") {
        let name = crate::app_detector::frontmost_app_name()
            .await
            .unwrap_or_else(|_| "Finder".to_string());
        out = out.replace("{active_app}", &name);
    }
    out
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
    let resolved = resolve_placeholders(app, &action.command, query).await;

    log::info!(
        "executing action '{}' ({:?}) → {}",
        action.id,
        action.action_type,
        resolved
    );

    match action.action_type {
        ActionType::Shell => run_shell(&resolved, action.working_dir.as_deref()).await,
        ActionType::Applescript => run_applescript(&resolved).await,
        ActionType::OpenUrl => open_url(&resolved).await,
        ActionType::OpenApp => open_app(&resolved).await,
        ActionType::Keystroke => send_keystroke(&resolved).await,
    }
    .map(|()| ExecOutcome {
        success: true,
        message: render(&action.success_feedback, &resolved, query),
    })
    .or_else(|e| {
        Ok(ExecOutcome {
            success: false,
            message: format!(
                "{}: {e:#}",
                render(&action.failure_feedback, &resolved, query)
            ),
        })
    })
}

fn render(template: &str, resolved: &str, query: &str) -> String {
    template
        .replace("{query}", query)
        .replace("{resolved}", resolved)
}

async fn run_shell(command: &str, working_dir: Option<&str>) -> Result<()> {
    check_shell(command)?;
    let mut cmd = tokio::process::Command::new("/bin/sh");
    cmd.args(["-c", command]);
    if let Some(dir) = working_dir {
        cmd.current_dir(shellexpand_home(dir));
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

async fn run_applescript(script: &str) -> Result<()> {
    check_applescript(script)?;
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

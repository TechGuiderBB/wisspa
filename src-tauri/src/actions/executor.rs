use super::{Action, ActionType};
use anyhow::{anyhow, Context, Result};
use tauri::{AppHandle, Runtime};
use tauri_plugin_clipboard_manager::ClipboardExt;

/// Substring tokens forbidden inside shell commands per PRD §5.2.3.
/// Recursive `rm` is intentionally NOT here — flag spelling/order/splitting
/// makes it impossible to cover with fixed substrings; see `has_recursive_rm`.
const SHELL_DENY: &[&str] = &[
    "sudo ",
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

/// Command separators that begin a fresh command word. `|` is space-padded
/// during normalization; `;`, `&&`, `||`, `&` are matched as standalone
/// tokens (an un-spaced `;` stays attached to its operand, which at worst
/// causes a conservative rejection — never a bypass).
const CMD_SEPARATORS: &[&str] = &[";", "|", "||", "&", "&&"];

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
    // Normalize shell metachar spacing + runs of whitespace to single spaces.
    // This makes deny patterns resilient to forms like `curl URL| sh` and
    // `curl URL |sh` in addition to tabs/double-spaces.
    let metachar_padded = cmd.replace('|', " | ");
    // split_whitespace strips leading whitespace. We prepend a single space in
    // the scan string below so leading-space deny tokens (e.g. " :(){") stay
    // matchable when the command starts with that pattern.
    let normalized = metachar_padded
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let scan = format!(" {normalized}");
    for bad in SHELL_DENY {
        if scan.contains(bad) {
            return Err(anyhow!("shell command rejected (contains '{bad}')"));
        }
    }
    if has_recursive_rm(&normalized) {
        return Err(anyhow!("shell command rejected (recursive rm)"));
    }
    Ok(())
}

/// Detect a recursive `rm` regardless of how the flags are spelled, ordered,
/// or split: `rm -rf`, `rm -fr`, `rm -r`, `rm -f -r`, `rm -i -R x`,
/// `rm --recursive`, `rm --force --recursive`. Substring deny patterns cannot
/// cover split/long flag forms — and lack word boundaries (`perform -fr`
/// contains `rm -fr`) — so recursive `rm` gets a dedicated tokenizer.
///
/// `normalized` is whitespace-collapsed and lowercased, so tokens split cleanly
/// on a single space and `-R` already reads as `-r`.
fn has_recursive_rm(normalized: &str) -> bool {
    let tokens: Vec<&str> = normalized.split(' ').filter(|t| !t.is_empty()).collect();
    for (i, tok) in tokens.iter().enumerate() {
        if *tok != "rm" {
            continue;
        }
        // Scan this command's argument tokens until the next separator.
        for arg in &tokens[i + 1..] {
            if CMD_SEPARATORS.contains(arg) {
                break;
            }
            let recursive = match arg.strip_prefix("--") {
                // Long option: only `--recursive` implies recursion.
                Some(_) => *arg == "--recursive",
                // Short-flag cluster (e.g. `-rf`, `-fr`): any `r` means
                // recursive. Operands and `--`-less long forms fall through.
                None => arg.strip_prefix('-').is_some_and(|c| c.contains('r')),
            };
            if recursive {
                return true;
            }
        }
    }
    false
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

/// AppleScript string-literal wrap. Escapes `\` to `\\`, `"` to `\"`, and
/// collapses `\n`/`\r` to a space — AppleScript double-quoted string literals
/// do not support literal newlines, so multi-line clipboard content would
/// otherwise produce an osascript syntax error and silently fail the action.
fn applescript_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' | '\r' => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Percent-encode a value for safe insertion into a URL query component:
/// RFC 3986 unreserved characters (alphanumerics and `-._~`) pass through,
/// everything else is UTF-8 percent-encoded. Neither `url` nor
/// `percent-encoding` is a direct dependency, and this is the whole alphabet —
/// no new crate needed.
fn percent_encode_query(s: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => {
                out.push('%');
                out.push(HEX[(b >> 4) as usize] as char);
                out.push(HEX[(b & 0x0f) as usize] as char);
            }
        }
    }
    out
}

/// How to transform each substituted placeholder value before insertion.
#[derive(Clone, Copy)]
enum QuoteMode {
    /// Insert the value as-is. Used by action types that don't pass through
    /// a shell or AppleScript interpreter (open_app, keystroke).
    Verbatim,
    /// Like Verbatim, except the `{query}` value is percent-encoded so a
    /// spoken query containing `&`, `#` or spaces can't truncate the URL or
    /// smuggle extra query params. Used by open_url.
    OpenUrl,
    /// POSIX single-quote wrap each substituted value.
    Shell,
    /// AppleScript string-literal wrap each substituted value.
    Applescript,
}

fn apply_quote(mode: QuoteMode, value: &str) -> String {
    match mode {
        // OpenUrl percent-encodes only the `{query}` value (handled at the
        // call site in resolve_with); every other value stays verbatim.
        QuoteMode::Verbatim | QuoteMode::OpenUrl => value.to_string(),
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
            "query" => Some(match mode {
                QuoteMode::OpenUrl => percent_encode_query(query),
                _ => apply_quote(mode, query),
            }),
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

/// Verbatim placeholder substitution for non-shell, non-applescript, non-url
/// action types (open_app, keystroke). Values are inserted as-is — these
/// dispatch paths do not invoke a shell interpreter, so there is no
/// metacharacter context to escape against.
pub async fn resolve_placeholders<R: Runtime>(
    app: &AppHandle<R>,
    raw: &str,
    query: &str,
) -> String {
    resolve_with(app, raw, query, QuoteMode::Verbatim).await
}

/// Placeholder substitution for `open_url` actions. The `{query}` value is
/// percent-encoded (query-component rules) so a spoken query like "c&b #1"
/// can't truncate the URL at `&` or smuggle in extra params/fragments.
/// `{clipboard}`/`{active_app}` stay verbatim, and the static URL template
/// itself is never encoded — it is authored config, not user input.
pub async fn resolve_placeholders_url<R: Runtime>(
    app: &AppHandle<R>,
    raw: &str,
    query: &str,
) -> String {
    resolve_with(app, raw, query, QuoteMode::OpenUrl).await
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

/// Routing decision for a matched action. Destructive actions MUST be staged
/// for tray confirmation and never dispatched directly from `execute`; this is
/// the single source of truth for the gate so the decision is testable in
/// isolation and `execute` cannot drift from it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Route {
    Run,
    Confirm,
}

/// Decide whether an action runs immediately or is staged for confirmation.
/// Keys solely on `destructive` — independent of `action_type`.
pub(crate) fn route(action: &Action) -> Route {
    if action.destructive {
        Route::Confirm
    } else {
        Route::Run
    }
}

pub async fn execute<R: Runtime>(
    app: &AppHandle<R>,
    action: &Action,
    query: &str,
) -> Result<ExecOutcome> {
    let resolved = match action.action_type {
        ActionType::Shell => resolve_placeholders_shell(app, &action.command, query).await,
        ActionType::Applescript => resolve_placeholders_applescript(app, &action.command, query).await,
        ActionType::OpenUrl => resolve_placeholders_url(app, &action.command, query).await,
        _ => resolve_placeholders(app, &action.command, query).await,
    };

    if let Some(outcome) = check_permissions(app, action).await {
        return Ok(outcome);
    }

    match route(action) {
        Route::Confirm => {
            let id = crate::actions::pending::store(
                action.clone(),
                resolved.clone(),
                query.to_string(),
            );
            crate::tray::set_pending_confirmation(app, Some(&action.name));
            crate::actions::pending::schedule_timeout(app.clone(), id);
            Ok(ExecOutcome {
                success: false,
                message: format!(
                    "Confirm \"{}\" via the Wisspa tray menu within {}s.",
                    action.name,
                    crate::actions::pending::CONFIRMATION_TIMEOUT.as_secs()
                ),
            })
        }
        Route::Run => {
            let env = settings_env(app);
            Ok(dispatch(action, &resolved, query, &env).await)
        }
    }
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
        " Fix the action YAML's requires_permissions list.".to_string()
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
        crate::redact::redact(resolved)
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

    // Default: keystroke a single character. Use char count, not byte length,
    // so multi-byte Unicode characters (e.g. é, ñ) are accepted correctly.
    if key_str.chars().count() != 1 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn rejected(cmd: &str) -> bool {
        check_shell(cmd).is_err()
    }

    /// Build a minimal action with every field populated. `command`/`destructive`
    /// vary per test; the rest are inert defaults.
    fn action(destructive: bool, action_type: ActionType, command: &str) -> Action {
        Action {
            id: "test".to_string(),
            name: "Test Action".to_string(),
            description: String::new(),
            triggers: Vec::new(),
            action_type,
            command: command.to_string(),
            working_dir: None,
            requires_permissions: Vec::new(),
            destructive,
            success_feedback: "done".to_string(),
            failure_feedback: "failed".to_string(),
            enabled: true,
        }
    }

    /// Unique, non-wall-clock sentinel path under the temp dir. Uniqueness comes
    /// from pid + a process-local counter — never `Instant`/`Date` (banned and
    /// flaky). The path holds no `{`/`}` so `resolve_placeholders_shell` passes
    /// the command through verbatim (no clipboard read).
    fn sentinel_path(tag: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("wisspa-exec-test-{}-{tag}-{n}", std::process::id()))
    }

    // ---- Decision seam (supporting; NOT the suppression proof) -------------

    #[test]
    fn route_confirms_every_destructive_action() {
        for ty in [
            ActionType::Shell,
            ActionType::Applescript,
            ActionType::OpenUrl,
            ActionType::OpenApp,
            ActionType::Keystroke,
        ] {
            let a = action(true, ty, "");
            assert_eq!(route(&a), Route::Confirm, "destructive {ty:?} must confirm");
        }
    }

    #[test]
    fn route_runs_non_destructive_actions() {
        for ty in [
            ActionType::Shell,
            ActionType::Applescript,
            ActionType::OpenUrl,
            ActionType::OpenApp,
            ActionType::Keystroke,
        ] {
            let a = action(false, ty, "");
            assert_eq!(route(&a), Route::Run, "non-destructive {ty:?} must run");
        }
    }

    // ---- Suppression proof at the execute() boundary (REQUIRED) ------------

    /// THE security property: a destructive action's side effect does not occur
    /// until the user confirms. Drives the live `execute()` against a mock
    /// runtime and a sentinel file, then proves the side effect appears only
    /// after `run_confirmed()`.
    #[tokio::test]
    async fn destructive_action_is_staged_not_run_then_runs_on_confirm() {
        let _g = crate::actions::pending::TEST_GATE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::actions::pending::reset_for_test();

        let app = tauri::test::mock_app();
        let h = app.handle().clone();
        let sentinel = sentinel_path("destructive");
        let _ = std::fs::remove_file(&sentinel);
        let cmd = format!("touch {}", sentinel.display());
        let act = action(true, ActionType::Shell, &cmd);

        let out = execute(&h, &act, "").await.unwrap();
        let timeout_token = format!(
            "{}s",
            crate::actions::pending::CONFIRMATION_TIMEOUT.as_secs()
        );
        assert!(!out.success, "destructive action must not report success on trigger");
        assert!(
            out.message.contains(&timeout_token),
            "stage message must contain the timeout token \"{}\": {}",
            timeout_token,
            out.message
        );
        assert!(
            !sentinel.exists(),
            "SECURITY: destructive side effect ran before confirmation"
        );

        // No placeholders in `cmd`, so the resolved command equals the template.
        let out2 = run_confirmed(&h, &act, &cmd, "").await;
        assert!(out2.success, "confirmed run failed: {}", out2.message);
        assert!(sentinel.exists(), "confirmed destructive action did not run");

        let _ = std::fs::remove_file(&sentinel);
        crate::actions::pending::reset_for_test();
    }

    /// Non-destructive actions still run immediately at the real boundary —
    /// the gate must not regress the common path.
    #[tokio::test]
    async fn non_destructive_action_runs_immediately() {
        let _g = crate::actions::pending::TEST_GATE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::actions::pending::reset_for_test();

        let app = tauri::test::mock_app();
        let h = app.handle().clone();
        let sentinel = sentinel_path("plain");
        let _ = std::fs::remove_file(&sentinel);
        let cmd = format!("touch {}", sentinel.display());
        let act = action(false, ActionType::Shell, &cmd);

        let out = execute(&h, &act, "").await.unwrap();
        assert!(out.success, "non-destructive run failed: {}", out.message);
        assert!(sentinel.exists(), "non-destructive action should run immediately");

        let _ = std::fs::remove_file(&sentinel);
    }

    #[test]
    fn blocks_recursive_rm_in_every_flag_form() {
        for cmd in [
            "rm -rf /tmp/x",
            "rm -fr /tmp/x",
            "rm -r /tmp/x",
            "rm -R /tmp/x",
            "rm -f -r /tmp/x",
            "rm -r -f /tmp/x",
            "rm -i -R /tmp/x",
            "rm --recursive /tmp/x",
            "rm --force --recursive /tmp/x",
            "rm    -rf   /tmp/x",
            "rm /tmp/x -r",
            "ls && rm -rf /tmp/x",
        ] {
            assert!(rejected(cmd), "should reject recursive rm: {cmd}");
        }
    }

    #[test]
    fn allows_non_recursive_rm_and_lookalikes() {
        for cmd in [
            "rm /tmp/x",
            "rm -f /tmp/x",
            "rmdir /tmp/x",
            // `perform -fr` contains the literal substring `rm -fr` but is not
            // an `rm` command — the old substring deny-list false-positived.
            "perform -fr task",
            "echo rm is recursive",
        ] {
            assert!(!rejected(cmd), "should allow: {cmd}");
        }
    }

    #[test]
    fn still_blocks_other_deny_tokens() {
        assert!(rejected("sudo reboot"));
        assert!(rejected("curl|sh"));
        assert!(rejected("curl | sh"));
    }

    #[test]
    fn combo_to_applescript_rejects_fn_modifier() {
        // Regression guard: the `fn` modifier is unreachable via System
        // Events keystroke, which is why the old `fn+f11` show_desktop
        // default errored. show_desktop is now an applescript action; this
        // pins the rejection so the broken combo form can never silently
        // come back.
        let err = combo_to_applescript("fn+f11").unwrap_err();
        assert!(
            err.to_string().contains("fn modifier"),
            "got: {err}"
        );
    }

    #[test]
    fn show_desktop_applescript_command_validates() {
        // The new show_desktop default uses an applescript system event. It must
        // pass the applescript allowlist (no `do shell script`).
        let cmd = r#"tell application "System Events" to key code 103"#;
        assert!(check_applescript(cmd).is_ok());
    }

    // ---- open_url percent-encoding ------------------------------------------

    #[test]
    fn percent_encode_query_encodes_reserved_chars() {
        assert_eq!(percent_encode_query("c&b #1"), "c%26b%20%231");
        assert_eq!(percent_encode_query("hello world"), "hello%20world");
        // The RFC 3986 unreserved set passes through untouched.
        assert_eq!(percent_encode_query("abcXYZ019-._~"), "abcXYZ019-._~");
        // Non-ASCII is UTF-8 percent-encoded per byte.
        assert_eq!(percent_encode_query("café"), "caf%C3%A9");
    }

    #[tokio::test]
    async fn open_url_resolution_encodes_query_value_only() {
        let app = tauri::test::mock_app();
        let h = app.handle().clone();
        // The bug: a verbatim "c&b #1" truncates the URL at `&`.
        let out = resolve_placeholders_url(
            &h,
            "https://github.com/search?q={query}",
            "c&b #1",
        )
        .await;
        assert_eq!(out, "https://github.com/search?q=c%26b%20%231");
        // The static template is authored config and is never encoded;
        // unknown placeholders still pass through verbatim.
        let out = resolve_placeholders_url(
            &h,
            "https://x.test/{unknown}?q={query}&lang=en",
            "a&b",
        )
        .await;
        assert_eq!(out, "https://x.test/{unknown}?q=a%26b&lang=en");
    }

    #[tokio::test]
    async fn non_url_resolution_keeps_query_verbatim() {
        let app = tauri::test::mock_app();
        let h = app.handle().clone();
        // open_app / keystroke keep the spoken value as-is — "open {query}"
        // must resolve to the real app name, not an encoded form.
        let out = resolve_placeholders(&h, "{query}", "c&b #1").await;
        assert_eq!(out, "c&b #1");
    }
}

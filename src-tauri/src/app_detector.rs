use anyhow::{Context, Result};

/// Returns the name of the frontmost application process, e.g. "TextEdit", "Cursor", "Slack".
/// Implemented via AppleScript; requires Automation permission (granted on first prompt).
pub async fn frontmost_app_name() -> Result<String> {
    let output = tokio::process::Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events" to get name of first application process whose frontmost is true"#,
        ])
        .output()
        .await
        .context("spawn osascript for frontmost app")?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "osascript frontmost failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() {
        return Err(anyhow::anyhow!("empty frontmost app name"));
    }
    Ok(name)
}

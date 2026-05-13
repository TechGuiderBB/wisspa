use serde::Serialize;
use std::sync::atomic::{AtomicU8, Ordering};

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Unknown,
    Granted,
    Denied,
}

impl From<u8> for Status {
    fn from(v: u8) -> Self {
        match v {
            1 => Status::Granted,
            2 => Status::Denied,
            _ => Status::Unknown,
        }
    }
}

impl Status {
    fn as_u8(self) -> u8 {
        match self {
            Status::Unknown => 0,
            Status::Granted => 1,
            Status::Denied => 2,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PermissionsSnapshot {
    pub microphone: Status,
    pub accessibility: Status,
    pub screen_recording: Status,
    pub automation: Status,
}

/// Microphone status is tricky to query from Rust without an AVFoundation
/// dependency, so the frontend reports its `getUserMedia` result back to us.
static MIC_STATUS: AtomicU8 = AtomicU8::new(0);

pub fn set_microphone_status(granted: bool) {
    MIC_STATUS.store(
        if granted { Status::Granted } else { Status::Denied }.as_u8(),
        Ordering::SeqCst,
    );
}

pub fn microphone() -> Status {
    Status::from(MIC_STATUS.load(Ordering::SeqCst))
}

pub fn accessibility() -> Status {
    if crate::injector::accessibility_trusted() {
        Status::Granted
    } else {
        Status::Denied
    }
}

pub fn screen_recording() -> Status {
    #[cfg(target_os = "macos")]
    {
        if unsafe { CGPreflightScreenCaptureAccess() } {
            Status::Granted
        } else {
            Status::Denied
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Status::Granted
    }
}

/// Probes Automation by running a tiny harmless osascript that needs the
/// "System Events" target. Result -1743 (errAEEventNotPermitted) → Denied.
pub async fn automation() -> Status {
    let output = tokio::process::Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events" to return name of first application process whose frontmost is true"#,
        ])
        .output()
        .await;
    match output {
        Ok(o) if o.status.success() => Status::Granted,
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr);
            if err.contains("-1743") || err.to_lowercase().contains("not allowed") {
                Status::Denied
            } else {
                Status::Unknown
            }
        }
        Err(_) => Status::Unknown,
    }
}

pub async fn snapshot() -> PermissionsSnapshot {
    PermissionsSnapshot {
        microphone: microphone(),
        accessibility: accessibility(),
        screen_recording: screen_recording(),
        automation: automation().await,
    }
}

/// Trigger the macOS Screen Recording permission prompt (only fires once if
/// the app hasn't been granted yet; otherwise this is a no-op).
pub fn request_screen_recording_access() {
    #[cfg(target_os = "macos")]
    unsafe {
        let _ = CGRequestScreenCaptureAccess();
    }
}

/// Deep-link to a specific Privacy & Security pane in System Settings.
pub fn open_settings_for(pane: &str) -> std::io::Result<()> {
    let anchor = match pane {
        "microphone" => "Privacy_Microphone",
        "accessibility" => "Privacy_Accessibility",
        "screen_recording" => "Privacy_ScreenCapture",
        "automation" => "Privacy_Automation",
        _ => "Privacy",
    };
    let url = format!("x-apple.systempreferences:com.apple.preference.security?{anchor}");
    std::process::Command::new("open").arg(url).spawn().map(|_| ())
}

//! Tauri commands for "Meeting with screen" (`video/`). macOS only;
//! elsewhere the same commands exist so the bindings match, and report that
//! screen recording is unavailable.

use tauri::AppHandle;

#[cfg(target_os = "macos")]
pub use crate::video::{VideoChanged, VideoMeta};

#[cfg(not(target_os = "macos"))]
mod stubs {
    #[derive(Clone, Debug, serde::Serialize, serde::Deserialize, specta::Type)]
    pub struct VideoMeta {}
    #[derive(
        Clone, Debug, serde::Serialize, serde::Deserialize, specta::Type, tauri_specta::Event,
    )]
    pub struct VideoChanged {
        pub folder: String,
    }
}
#[cfg(not(target_os = "macos"))]
pub use stubs::*;

#[cfg(not(target_os = "macos"))]
const UNAVAILABLE: &str = "Screen recording needs macOS.";

/// Starts a meeting with its screen. Returns the meeting id. The error
/// "screen_not_allowed" means macOS was just asked for Screen Recording;
/// if the screen fails otherwise, the meeting records without it.
/// Stops with the meeting (the recording pill or `stop_meeting_recording`).
#[tauri::command]
#[specta::specta]
pub async fn start_meeting_with_screen(app: AppHandle) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        log::info!("Meeting with screen via the Sokki window");
        return tokio::task::spawn_blocking(move || {
            crate::video::start(&app, None).or_else(|e| {
                // Without Screen Recording the bar says how to allow it.
                // Any other failure still records the meeting, without the
                // screen, as the call window does: a meeting is never lost.
                if e == "screen_not_allowed" {
                    return Err(e);
                }
                log::warn!("Meeting with screen: {e}; recording the meeting without it");
                crate::meeting::start_meeting(&app, None)
            })
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Err(UNAVAILABLE.into())
    }
}

/// Every meeting's screen video, newest first.
#[tauri::command]
#[specta::specta]
pub fn video_list() -> Result<Vec<VideoMeta>, String> {
    #[cfg(target_os = "macos")]
    return Ok(crate::video::list());
    #[cfg(not(target_os = "macos"))]
    Err(UNAVAILABLE.into())
}

/// Shows a file or folder in Finder.
#[tauri::command]
#[specta::specta]
pub fn video_reveal(path: String) -> Result<(), String> {
    std::process::Command::new("open")
        .arg("-R")
        .arg(&path)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Whether this Mac lets Sokki record the screen. Never asks.
#[tauri::command]
#[specta::specta]
pub fn screen_permission() -> bool {
    #[cfg(target_os = "macos")]
    return crate::video::screen_allowed();
    #[cfg(not(target_os = "macos"))]
    false
}

/// Asks macOS for Screen Recording (its own prompt, or System Settings when
/// declined before). True when it is already allowed; once allowed, macOS
/// asks to quit and reopen Sokki.
#[tauri::command]
#[specta::specta]
pub async fn request_screen_permission() -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    return tokio::task::spawn_blocking(crate::video::request_screen)
        .await
        .map_err(|e| e.to_string());
    #[cfg(not(target_os = "macos"))]
    Err(UNAVAILABLE.into())
}

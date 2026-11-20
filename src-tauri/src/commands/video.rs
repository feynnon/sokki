//! Tauri commands for screen and camera recording (`video/`). macOS only;
//! elsewhere the same commands exist so the bindings match, and report that
//! recording is unavailable.

use crate::settings::VideoSettings;
use tauri::AppHandle;

#[cfg(target_os = "macos")]
pub use crate::video::{VideoChanged, VideoMeta, VideoSources, VideoStartOptions};

#[cfg(not(target_os = "macos"))]
mod stubs {
    #[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, specta::Type)]
    pub struct VideoSources {}
    #[derive(Clone, Debug, serde::Serialize, serde::Deserialize, specta::Type)]
    pub struct VideoMeta {}
    #[derive(Clone, Debug, serde::Serialize, serde::Deserialize, specta::Type)]
    pub struct VideoStartOptions {}
    #[derive(Clone, Debug, serde::Serialize, serde::Deserialize, specta::Type, tauri_specta::Event)]
    pub struct VideoChanged {
        pub folder: String,
    }
}
#[cfg(not(target_os = "macos"))]
pub use stubs::*;

#[cfg(not(target_os = "macos"))]
const UNAVAILABLE: &str = "Video recording needs macOS.";

/// Displays, cameras and whether each may be recorded. Never asks.
#[tauri::command]
#[specta::specta]
pub fn video_sources() -> Result<VideoSources, String> {
    #[cfg(target_os = "macos")]
    return Ok(crate::video::sources());
    #[cfg(not(target_os = "macos"))]
    Err(UNAVAILABLE.into())
}

/// Asks macOS for "screen" or "camera". True when allowed.
#[tauri::command]
#[specta::specta]
pub async fn video_request_access(kind: String) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    return tokio::task::spawn_blocking(move || crate::video::request_access(&kind))
        .await
        .map_err(|e| e.to_string());
    #[cfg(not(target_os = "macos"))]
    {
        let _ = kind;
        Err(UNAVAILABLE.into())
    }
}

/// Starts recording the screen and/or camera with the meeting audio. Stops
/// with the meeting (the recording pill or `stop_meeting_recording`).
#[tauri::command]
#[specta::specta]
pub async fn video_start(app: AppHandle, options: VideoStartOptions) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        // The bar closes once recording runs; on an error it stays and says why.
        let handle = app.clone();
        let id = tokio::task::spawn_blocking(move || crate::video::start(&handle, options))
            .await
            .map_err(|e| e.to_string())??;
        crate::video_bar::hide(&app);
        return Ok(id);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, options);
        Err(UNAVAILABLE.into())
    }
}

/// All videos, newest first.
#[tauri::command]
#[specta::specta]
pub fn video_list() -> Result<Vec<VideoMeta>, String> {
    #[cfg(target_os = "macos")]
    return Ok(crate::video::list());
    #[cfg(not(target_os = "macos"))]
    Err(UNAVAILABLE.into())
}

/// Changes a video's look and renders it again.
#[tauri::command]
#[specta::specta]
pub fn video_set_look(app: AppHandle, folder: String, look: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return crate::video::set_look(&app, &folder, &look);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, folder, look);
        Err(UNAVAILABLE.into())
    }
}

/// Moves a video's folder to the Trash.
#[tauri::command]
#[specta::specta]
pub fn video_delete(app: AppHandle, folder: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return crate::video::delete(&app, &folder);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, folder);
        Err(UNAVAILABLE.into())
    }
}

/// Shows a video in Finder: the rendered file when there is one, else the
/// folder.
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

/// Opens the recording bar (pick what to record, then Record).
#[tauri::command]
#[specta::specta]
pub fn video_bar_show(app: AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    crate::video_bar::show(&app);
    #[cfg(not(target_os = "macos"))]
    let _ = app;
    Ok(())
}

/// Closes the recording bar without recording.
#[tauri::command]
#[specta::specta]
pub fn video_bar_hide(app: AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::video_bar::hide(&app);
        crate::video::bubble(false, None);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
    Ok(())
}

/// Shows or hides the live camera bubble (only when the camera is allowed).
#[tauri::command]
#[specta::specta]
pub fn video_bubble(show: bool, camera: Option<String>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    crate::video::bubble(show, camera.as_deref());
    #[cfg(not(target_os = "macos"))]
    let _ = (show, camera);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn set_video_settings(app: AppHandle, settings: VideoSettings) -> Result<(), String> {
    let mut all = crate::settings::get_settings(&app);
    all.videos = settings;
    crate::settings::write_settings(&app, all);
    Ok(())
}

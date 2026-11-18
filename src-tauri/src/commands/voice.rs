//! Commands for the "Your voice" page (`crate::voice_data`).

use crate::voice_data::{self, CommunityTotal, VoiceDataStats};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

/// How much of the user's own voice is kept, per language and source.
#[tauri::command]
#[specta::specta]
pub fn voice_data_stats(app: AppHandle) -> Result<VoiceDataStats, String> {
    voice_data::stats(&app).map_err(|e| format!("{e:#}"))
}

/// "off", "sell" or "donate" (see `VoiceDataStats::choice`).
#[tauri::command]
#[specta::specta]
pub fn voice_data_set_choice(app: AppHandle, choice: String) -> Result<(), String> {
    if !["off", "sell", "donate"].contains(&choice.as_str()) {
        return Err(format!("Unknown choice: {choice}"));
    }
    voice_data::set_choice(&app, &choice).map_err(|e| format!("{e:#}"))
}

/// Deletes every kept clip.
#[tauri::command]
#[specta::specta]
pub fn voice_data_delete(app: AppHandle) -> Result<(), String> {
    voice_data::delete_all(&app).map_err(|e| format!("{e:#}"))
}

/// The total over all opted-in installs; `None` without a counter or offline.
#[tauri::command]
#[specta::specta]
pub async fn voice_data_community() -> Result<Option<CommunityTotal>, String> {
    tauri::async_runtime::spawn_blocking(voice_data::community_total)
        .await
        .map_err(|e| e.to_string())
}

/// Opens the folder with the kept clips in Finder.
#[tauri::command]
#[specta::specta]
pub fn voice_data_open_folder(app: AppHandle) -> Result<(), String> {
    let stats = voice_data::stats(&app).map_err(|e| format!("{e:#}"))?;
    std::fs::create_dir_all(&stats.folder).map_err(|e| e.to_string())?;
    app.opener()
        .open_path(stats.folder, None::<String>)
        .map_err(|e| e.to_string())
}

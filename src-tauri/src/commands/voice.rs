//! Commands for the "Your data" page (`crate::voice_data`).

use crate::voice_data::{self, CommunityTotal, VoiceDataStats};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

/// How much of the user's own data is kept, and the two choices.
#[tauri::command]
#[specta::specta]
pub fn voice_data_stats(app: AppHandle) -> Result<VoiceDataStats, String> {
    voice_data::stats(&app).map_err(|e| format!("{e:#}"))
}

/// Sets "Help improve Sokki", "Share all my recordings" and "Ask me when
/// there's a buyer".
#[tauri::command]
#[specta::specta]
pub fn voice_data_set_choices(
    app: AppHandle,
    improve: bool,
    support: bool,
    buyers: bool,
) -> Result<(), String> {
    voice_data::set_choices(&app, improve, support, buyers).map_err(|e| format!("{e:#}"))
}

/// Deletes every kept recording on this Mac.
#[tauri::command]
#[specta::specta]
pub fn voice_data_delete(app: AppHandle) -> Result<(), String> {
    voice_data::delete_all(&app).map_err(|e| format!("{e:#}"))
}

/// Deletes everything the counter holds of this install.
#[tauri::command]
#[specta::specta]
pub async fn voice_data_forget(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || voice_data::forget(&app))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{e:#}"))
}

/// Answers the current buyer message: "interested" or "no".
#[tauri::command]
#[specta::specta]
pub async fn voice_data_answer_buyer(app: AppHandle, answer: String) -> Result<(), String> {
    if !["interested", "no"].contains(&answer.as_str()) {
        return Err(format!("Unknown answer: {answer}"));
    }
    tauri::async_runtime::spawn_blocking(move || voice_data::answer_buyer(&app, &answer))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{e:#}"))
}

/// The total over all counted installs; `None` without a counter or offline.
#[tauri::command]
#[specta::specta]
pub async fn voice_data_community() -> Result<Option<CommunityTotal>, String> {
    tauri::async_runtime::spawn_blocking(voice_data::community_total)
        .await
        .map_err(|e| e.to_string())
}

/// Opens the folder with the kept recordings in Finder.
#[tauri::command]
#[specta::specta]
pub fn voice_data_open_folder(app: AppHandle) -> Result<(), String> {
    let stats = voice_data::stats(&app).map_err(|e| format!("{e:#}"))?;
    std::fs::create_dir_all(&stats.folder).map_err(|e| e.to_string())?;
    app.opener()
        .open_path(stats.folder, None::<String>)
        .map_err(|e| e.to_string())
}

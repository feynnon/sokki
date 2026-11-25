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

/// A setup screen was shown: counted on this Mac for "Help improve Sokki"
/// (`crate::usage`). Names other than setup's screens are ignored.
#[tauri::command]
#[specta::specta]
pub fn usage_setup_step(app: AppHandle, step: String) {
    crate::usage::setup_step(&app, &step);
}

/// Deletes every kept recording on this Mac.
#[tauri::command]
#[specta::specta]
pub fn voice_data_delete(app: AppHandle) -> Result<(), String> {
    voice_data::delete_all(&app).map_err(|e| format!("{e:#}"))
}

/// Deletes everything the counter holds of this install. `true`: some of it
/// is still being deleted (the counter finishes over the next days).
#[tauri::command]
#[specta::specta]
pub async fn voice_data_forget(app: AppHandle) -> Result<bool, String> {
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

/// "Your data": hours, words dictated and meetings, all time.
#[tauri::command]
#[specta::specta]
pub async fn usage_totals(app: AppHandle) -> Result<crate::milestones::Totals, String> {
    tauri::async_runtime::spawn_blocking(move || crate::milestones::totals(&app))
        .await
        .map_err(|e| e.to_string())
}

/// The important message from Sokki to show in the window now, if any.
#[tauri::command]
#[specta::specta]
pub fn pending_message(app: AppHandle) -> Result<Option<voice_data::MessageView>, String> {
    voice_data::pending_message(&app).map_err(|e| format!("{e:#}"))
}

/// Answers the message window: "later" (again in a day) or "done".
#[tauri::command]
#[specta::specta]
pub fn answer_message(app: AppHandle, id: i64, answer: String) -> Result<(), String> {
    voice_data::answer_message(&app, id, &answer).map_err(|e| format!("{e:#}"))
}

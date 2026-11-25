//! The milestone card (hours transcribed reached 100, 200, 500 or 1,000)
//! and, before it, the star card.

use base64::Engine;
use tauri::AppHandle;

/// The milestone card to show now, if any.
#[tauri::command]
#[specta::specta]
pub fn get_milestone(app: AppHandle) -> Result<Option<crate::milestones::Milestone>, String> {
    Ok(crate::milestones::due(&app))
}

/// Closes the card for `hours` (and every milestone below it) for good.
#[tauri::command]
#[specta::specta]
pub fn close_milestone(app: AppHandle, hours: u32) -> Result<(), String> {
    crate::milestones::close(&app, hours);
    Ok(())
}

/// Whether the star card ("Enjoying Sokki?") shows now.
#[tauri::command]
#[specta::specta]
pub fn get_star_ask(app: AppHandle) -> Result<bool, String> {
    Ok(crate::milestones::star_due(&app))
}

/// Closes the star card for good: starred, "Not now" or its X.
#[tauri::command]
#[specta::specta]
pub fn close_star_ask(app: AppHandle) -> Result<(), String> {
    crate::milestones::close_star(&app);
    Ok(())
}

/// Saves the card's picture (a PNG made in the app) and shows it in Finder.
#[tauri::command]
#[specta::specta]
pub fn save_picture(path: String, png_base64: String) -> Result<(), String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(png_base64.as_bytes())
        .map_err(|e| e.to_string())?;
    std::fs::write(&path, bytes)
        .map_err(|e| format!("The picture could not be saved ({path}): {e}"))?;
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open")
        .args(["-R", &path])
        .spawn();
    Ok(())
}

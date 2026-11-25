//! Developer mode, "Keep what I write": the app's side of `sokki-journal`.
//! Starts the reader, keeps it in step with the setting, and gives
//! Settings its numbers and buttons. The files stay in
//! `<app data>/typing_log/`, on this Mac; nothing here sends them anywhere.
//!
//! Test mode (`SOKKI_TEST_MIC`) with `SOKKI_TEST_JOURNAL_PID=<pid>` reads
//! that process's focused field with the journal on, whatever the setting,
//! so the reader can be tested on a helper app without touching the screen.
//! Test mode has no keyboard hook: `SOKKI_TEST_JOURNAL_KEYS=<file>` holds
//! the helper's own count of keys it "pressed", so typed and pasted text
//! can still be told apart.

use std::sync::OnceLock;

use serde::Serialize;
use specta::Type;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use sokki_journal::{Journal, Options};

static JOURNAL: OnceLock<Journal> = OnceLock::new();

fn test_pid() -> Option<i32> {
    if !crate::test_mic::active() {
        return None;
    }
    std::env::var("SOKKI_TEST_JOURNAL_PID").ok()?.parse().ok()
}

/// Key presses so far: the keyboard hook's count, or in test mode the
/// helper app's (a number in the file `SOKKI_TEST_JOURNAL_KEYS` names).
fn key_downs() -> u64 {
    static TEST_KEYS: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();
    let file = TEST_KEYS.get_or_init(|| {
        test_pid()?;
        std::env::var_os("SOKKI_TEST_JOURNAL_KEYS").map(std::path::PathBuf::from)
    });
    match file {
        Some(path) => std::fs::read_to_string(path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0),
        None => sokki_keys::key_downs(),
    }
}

/// Starts the reader (off until the setting says on). Call once at startup.
pub fn start(app: &AppHandle) {
    let dir = match crate::portable::app_data_dir(app) {
        Ok(d) => d.join("typing_log"),
        Err(e) => {
            log::warn!("Keep what I write: no app data folder ({e})");
            return;
        }
    };
    let test_pid = test_pid();
    if let Some(pid) = test_pid {
        log::info!("Test mode: the journal reads process {pid}");
    }
    let journal = Journal::start(Options {
        dir,
        own_pid: std::process::id() as i32,
        test_pid,
        trusted: crate::accessibility::trusted,
        key_downs,
    });
    let _ = JOURNAL.set(journal);
    sync(app);
}

/// Applies the settings: on or off, and the excluded apps.
pub fn sync(app: &AppHandle) {
    let Some(journal) = JOURNAL.get() else {
        return;
    };
    let settings = crate::settings::get_settings(app);
    journal.set_excluded(settings.journal_excluded_apps.clone());
    journal.set_enabled(settings.keep_what_i_write || test_pid().is_some());
}

/// A dictation was pasted (or, in test mode, would have been).
pub fn note_dictation(raw: &str, pasted: &str) {
    if let Some(journal) = JOURNAL.get() {
        journal.note_dictation(raw.to_string(), pasted.to_string());
    }
}

/// What Settings shows under "Keep what I write".
#[derive(Debug, Clone, Serialize, Type)]
pub struct JournalStats {
    pub entries: u32,
    pub bytes: u32,
    /// The first day with an entry, "YYYY-MM-DD".
    pub since: Option<String>,
}

fn journal() -> Result<&'static Journal, String> {
    JOURNAL
        .get()
        .ok_or_else(|| "The journal is not running".to_string())
}

#[tauri::command]
#[specta::specta]
pub fn journal_stats() -> Result<JournalStats, String> {
    let s = journal()?.stats();
    Ok(JournalStats {
        entries: s.entries.min(u32::MAX as u64) as u32,
        bytes: s.bytes.min(u32::MAX as u64) as u32,
        since: s.since,
    })
}

#[tauri::command]
#[specta::specta]
pub fn journal_open_folder(app: AppHandle) -> Result<(), String> {
    let dir = journal()?.dir().to_path_buf();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| format!("Could not open the folder: {e}"))
}

#[tauri::command]
#[specta::specta]
pub fn journal_delete_all() -> Result<(), String> {
    journal()?.delete_all().map_err(|e| e.to_string())?;
    log::info!("Keep what I write: all entries deleted");
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_keep_what_i_write_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = crate::settings::get_settings(&app);
    settings.keep_what_i_write = enabled;
    crate::settings::write_settings(&app, settings);
    sync(&app);
    Ok(())
}

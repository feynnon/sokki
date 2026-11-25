//! Applies edits made to the settings file from outside the app (by hand or
//! by Claude). The settings file is the app's single config file, documented
//! in CONFIG.md. Every second the file is checked; when its `settings` differ
//! from what the app holds, the app reloads them, re-registers changed
//! shortcuts, re-applies the theme and meeting detection, and tells the
//! settings window to refresh.
//!
//! The app's own saves are recognised (same content) and ignored. A file that
//! does not parse (e.g. half-written by an editor) is skipped until it does.

use log::{info, warn};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_store::StoreExt;

use crate::settings::{self, AppSettings, SETTINGS_STORE_PATH};

/// Absolute path of the settings file.
pub fn settings_file(app: &AppHandle) -> PathBuf {
    match crate::portable::data_dir() {
        Some(dir) => dir.join(SETTINGS_STORE_PATH),
        None => app
            .path()
            .app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(SETTINGS_STORE_PATH),
    }
}

pub fn start(app: &AppHandle) {
    let handle = app.clone();
    std::thread::Builder::new()
        .name("config-watch".into())
        .spawn(move || watch(handle))
        .ok();
}

fn watch(app: AppHandle) {
    let path = settings_file(&app);
    info!("Watching {} for external edits", path.display());
    let mut last_mtime: Option<SystemTime> = mtime(&path);
    let mut warned_invalid = false;
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let now = mtime(&path);
        if now.is_none() || now == last_mtime {
            continue;
        }
        last_mtime = now;
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let on_disk = match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(v) => v.get("settings").cloned(),
            Err(e) => {
                if !warned_invalid {
                    warn!(
                        "Settings file is not valid JSON right now ({e}); waiting for a valid save"
                    );
                    warned_invalid = true;
                }
                continue;
            }
        };
        warned_invalid = false;
        let Some(on_disk) = on_disk else { continue };
        let Ok(store) = app.store(crate::portable::store_path(SETTINGS_STORE_PATH)) else {
            continue;
        };
        if store.get("settings").as_ref() == Some(&on_disk) {
            continue; // the app's own save
        }
        let before = settings::get_settings(&app);
        if let Err(e) = store.reload() {
            warn!("Reloading the edited settings failed: {e}");
            continue;
        }
        // Re-reads with validation and migrations; broken fields fall back.
        let after = settings::get_settings(&app);
        info!("Applied settings edited outside the app");
        apply_side_effects(&app, &before, &after);
        let _ = app.emit(
            "settings-changed",
            serde_json::json!({ "setting": "external" }),
        );
    }
}

fn apply_side_effects(app: &AppHandle, before: &AppSettings, after: &AppSettings) {
    // Shortcuts whose key combination changed.
    for (id, new) in &after.bindings {
        let old = before.bindings.get(id);
        if old.map(|o| &o.current_binding) != Some(&new.current_binding) {
            if let Some(old) = old {
                let _ = crate::shortcut::unregister_shortcut(app, old.clone());
            }
            if let Err(e) = crate::shortcut::register_shortcut(app, new.clone()) {
                warn!("Shortcut '{id}' from the edited settings could not be registered: {e}");
            }
        }
    }
    if before.theme != after.theme {
        crate::shortcut::apply_window_theme(app, after.theme.clone());
    }
    #[cfg(target_os = "macos")]
    if before.meetings != after.meetings {
        crate::meeting::detect::sync_with_settings(app);
    }
    if before.keep_what_i_write != after.keep_what_i_write
        || before.journal_excluded_apps != after.journal_excluded_apps
    {
        crate::journal::sync(app);
    }
    crate::tray::update_tray_menu(app);
}

fn mtime(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

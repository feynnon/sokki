//! Fixes typos as you type, in the apps where macOS doesn't (`crates/sokki-autocorrect`).
//!
//! Off until the user switches it on (`autocorrect_enabled`). No new keyboard hook: the
//! existing one only counts key presses (`sokki_keys::key_downs`), and the autocorrect thread
//! reads the focused field on its own, so typing never waits for it. The thread does nothing
//! while Accessibility isn't granted, and the hook itself stays in step with that permission
//! (`accessibility.rs`). The model's tables load only when the switch is first turned on.
//! Sokki's log never holds what was typed: only the app, what happened and how long it took.
//! The words of each fix go only into the list of fixes on this Mac,
//! `autocorrect_fixes.jsonl` in the app data folder (`sokki_autocorrect::fixlog`).

use tauri::AppHandle;
#[cfg(target_os = "macos")]
use tauri::Manager;

/// The model's files, bundled as resources (paths relative to src-tauri, as in tauri.conf.json).
#[cfg(target_os = "macos")]
const TABLES: &str = "../mobile/keyboard/src/main/assets/sokki";
#[cfg(target_os = "macos")]
const LEXICON: &str = "../crates/sokki-typos/data";

#[cfg(target_os = "macos")]
fn front_app() -> Option<(i32, String)> {
    let app = objc2_app_kit::NSWorkspace::sharedWorkspace().frontmostApplication()?;
    let id = app.bundleIdentifier()?.to_string();
    Some((app.processIdentifier(), id))
}

/// Starts the autocorrect thread (idle while the switch is off).
pub fn start(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        use sokki_autocorrect::{runner, sokki_typos, Outcome};
        if crate::test_mic::active() {
            log::info!("Test mode: autocorrect stays off (no keyboard hook)");
            return;
        }
        let resolve = {
            let app = app.clone();
            move |rel: &str| {
                app.path()
                    .resolve(rel, tauri::path::BaseDirectory::Resource)
                    .map_err(|e| format!("{rel}: {e}"))
            }
        };
        let (tables, lexicon) = match (resolve(TABLES), resolve(LEXICON)) {
            (Ok(t), Ok(l)) => (t, l),
            (Err(e), _) | (_, Err(e)) => {
                log::error!("autocorrect: no model files: {e}");
                return;
            }
        };
        let data_dir = app.path().app_data_dir().ok();
        let kept_file = data_dir
            .as_ref()
            .map(|d| d.join("autocorrect_kept_words.txt"));
        let fixes = data_dir
            .map(|d| sokki_autocorrect::fixlog::FixLog::new(d.join("autocorrect_fixes.jsonl")));
        let settings_app = app.clone();
        runner::start(runner::Options {
            load: Box::new(move || sokki_typos::Typos::load(&tables, &lexicon)),
            kept_file,
            keys: sokki_keys::key_downs,
            trusted: crate::accessibility::trusted,
            enabled: Box::new(move || {
                crate::settings::get_settings(&settings_app).autocorrect_enabled
            }),
            front: front_app,
            report: Box::new(move |app, outcome, took| {
                if let Some(fixes) = &fixes {
                    if let Err(e) = fixes.record(app, outcome, took) {
                        log::warn!("autocorrect: can't add to the list of fixes: {e}");
                    }
                }
                let what = match outcome {
                    Outcome::Fixed { exact: true, .. } => "fixed a word".to_string(),
                    Outcome::Fixed { exact: false, .. } => {
                        "fixed a word, but the field doesn't hold what was meant".to_string()
                    }
                    Outcome::Undone { .. } => "fix undone with Backspace; word kept".to_string(),
                    Outcome::Skipped(why) => format!("left a word: {why}"),
                    Outcome::Nothing => return,
                };
                log::info!("autocorrect: {app}: {what} ({} ms)", took.as_millis());
            }),
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

#[tauri::command]
#[specta::specta]
pub fn change_autocorrect_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = crate::settings::get_settings(&app);
    settings.autocorrect_enabled = enabled;
    crate::settings::write_settings(&app, settings);
    Ok(())
}

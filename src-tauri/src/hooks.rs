//! The app's side of hooks (`sokki_engine::hooks`, HOOKS.md): starts the
//! runner, tells it when a meeting needs the Mac, shows a failed hook as a
//! notice, and turns meetings and dictations into events.
//!
//! The synchronous hook that changes a dictation before it is pasted
//! (`dictation.transform`) is `transcription_hook.rs`, unchanged.

use crate::notices::{self, Notice, NoticeCode, NoticeSource};
use sokki_engine::hooks::{self, Gate, HookEvent, Host, Meta, Paths, Report};
use std::sync::{Arc, Mutex, PoisonError};
use tauri::AppHandle;

/// The hook the Hooks notice is about, so only its next success clears it.
static NOTICE_FOR: Mutex<Option<String>> = Mutex::new(None);

/// Where the queue and the logs live: `<app data>/hooks/`, next to the
/// `transcription` hook.
pub fn state_dir(app: &AppHandle) -> Option<std::path::PathBuf> {
    crate::portable::app_data_dir(app)
        .ok()
        .map(|d| d.join("hooks"))
}

/// Starts the runner. In test mode only with hooks of the test's own
/// (`SOKKI_HOOKS_DIR`), never the person's.
pub fn start(app: &AppHandle) {
    if crate::test_mic::active() && std::env::var_os("SOKKI_HOOKS_DIR").is_none() {
        log::info!("Test mode: hooks off (no SOKKI_HOOKS_DIR)");
        return;
    }
    let Some(state_dir) = state_dir(app) else {
        log::warn!("Hooks off: no app data folder");
        return;
    };
    let paths = Paths {
        config_dir: hooks::default_config_dir(),
        state_dir,
    };
    let (g, r, e) = (app.clone(), app.clone(), app.clone());
    let host = Host {
        paths,
        gate: Arc::new(move || gate(&g)),
        report: Arc::new(move |report| show(&r, report)),
        env: Arc::new(move || env(&e)),
    };
    if let Err(e) = hooks::start(host) {
        log::warn!("Hooks off: {e:#}");
    }
}

/// A meeting outranks every hook.
fn gate(app: &AppHandle) -> Gate {
    #[cfg(target_os = "macos")]
    {
        use tauri::Manager;
        let recording = app
            .try_state::<Arc<crate::meeting::MeetingManager>>()
            .is_some_and(|m| m.is_recording());
        if recording || crate::meeting::priority::recording() {
            return Gate::Recording;
        }
        if crate::meeting::process::busy() {
            return Gate::Processing;
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
    Gate::Free
}

/// What every hook finds in its environment, besides the event's own.
fn env(app: &AppHandle) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let settings = crate::settings::get_settings(app);
    out.push((
        "SOKKI_NOTES_DIR".to_string(),
        settings
            .meetings
            .notes_dir_path()
            .to_string_lossy()
            .into_owned(),
    ));
    if let Ok(dir) = crate::portable::app_data_dir(app) {
        out.push((
            "SOKKI_APP_DATA".to_string(),
            dir.to_string_lossy().into_owned(),
        ));
    }
    out
}

fn set_notice_for(hook: Option<String>) {
    *NOTICE_FOR.lock().unwrap_or_else(PoisonError::into_inner) = hook;
}

/// A failed or unapproved hook becomes the Hooks notice; its next success
/// clears it.
fn show(app: &AppHandle, report: Report) {
    match report {
        Report::Failed { hook, what, log } => {
            set_notice_for(Some(hook.clone()));
            notices::raise(
                app,
                Notice::new(NoticeCode::HookFailed, NoticeSource::Hooks)
                    .with_detail(format!("{hook} {what}. Log: {}", log.display())),
            );
        }
        Report::ConfigInvalid { error } => {
            set_notice_for(None);
            notices::raise(
                app,
                Notice::new(NoticeCode::HooksConfigInvalid, NoticeSource::Hooks).with_detail(error),
            );
        }
        Report::Succeeded { hook } => {
            let mut about = NOTICE_FOR.lock().unwrap_or_else(PoisonError::into_inner);
            if about.as_deref() == Some(hook.as_str()) {
                *about = None;
                drop(about);
                notices::clear(app, NoticeSource::Hooks);
            }
        }
    }
}

/// At quit: what waits in memory goes into the queue for the next start.
pub fn flush() {
    hooks::flush();
}

/// `dictation.done` for a dictation saved to the history.
pub fn dictation_done(history_id: i64, text: &str, language: Option<&str>) {
    if text.trim().is_empty() {
        return;
    }
    hooks::emit(HookEvent::dictation(
        history_id,
        text,
        Meta {
            language: language.map(str::to_string),
            ..Default::default()
        },
    ));
}

#[cfg(target_os = "macos")]
pub use meetings::*;

#[cfg(target_os = "macos")]
mod meetings {
    use super::*;
    use crate::meeting::store::{MeetingRecord, MeetingStore, SummaryMissing};
    use sokki_engine::hooks::event;
    use std::path::Path;
    use tauri::Manager;

    pub fn meta(r: &MeetingRecord) -> Meta {
        Meta {
            title: r.title.clone(),
            started_at: Some(r.started_at.clone()),
            duration_s: (r.duration_s > 0.0).then_some(r.duration_s),
            app: r.source_app.clone(),
            participants: r.speakers.clone(),
            language: None,
        }
    }

    /// Emits `kind` for the meeting as it is now. The sample meeting
    /// (invented people, the onboarding demo) fires nothing.
    pub fn meeting_event_for(kind: &str, r: &MeetingRecord) {
        if r.sample {
            return;
        }
        hooks::emit(HookEvent::meeting(
            kind,
            &r.id,
            r.note_path.as_deref().map(Path::new),
            meta(r),
        ));
    }

    pub fn meeting_event(app: &AppHandle, kind: &str, id: &str) {
        let Some(store) = app.try_state::<Arc<MeetingStore>>() else {
            return;
        };
        if let Some(r) = store.get(id) {
            meeting_event_for(kind, &r);
        }
    }

    /// `own_notes.saved`, once the notes rest for
    /// [`hooks::OWN_NOTES_QUIET`] (they are saved as they are typed).
    pub fn own_notes_saved(r: &MeetingRecord, notes_path: &Path) {
        if r.sample {
            return;
        }
        // Test mode can shorten the rest (SOKKI_TEST_OWN_NOTES_QUIET_S).
        let quiet = std::env::var("SOKKI_TEST_OWN_NOTES_QUIET_S")
            .ok()
            .filter(|_| crate::test_mic::active())
            .and_then(|s| s.parse().ok())
            .map(std::time::Duration::from_secs)
            .unwrap_or(hooks::OWN_NOTES_QUIET);
        hooks::emit_after_quiet(
            &format!("own_notes:{}", r.id),
            quiet,
            HookEvent::own_notes(&r.id, notes_path, meta(r)),
        );
    }

    /// `meeting.note_ready`, unless the summary still waits for the
    /// language model: then it fires once the summary is in.
    pub fn note_ready(app: &AppHandle, id: &str) {
        let Some(store) = app.try_state::<Arc<MeetingStore>>() else {
            return;
        };
        let Some(r) = store.get(id) else { return };
        if r.note_path.is_none() || r.summary_missing == Some(SummaryMissing::Waiting) {
            return;
        }
        meeting_event_for(event::MEETING_NOTE_READY, &r);
    }
}

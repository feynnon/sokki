//! Cleans up each dictation before it is pasted, with the built-in cleanup
//! model (`sokki_engine::cleanup`, Gemma 4 E4B in llama-server), while it is
//! spoken.
//!
//! - [`begin`] when a dictation starts: the model's server starts (or is
//!   still running from the last dictation) and a session waits for text.
//! - [`live_text`] with the transcript's final part as it grows (the live
//!   text's committed part): pieces of about 60 words are cleaned at once.
//! - [`take`] when the dictation stops, then [`Pending::finish`] with the
//!   final transcript: the rest is cleaned, and the cleaned text is pasted.
//!   When the model fails or takes too long, the transcript is pasted as it
//!   is.
//!
//! The server stops after [`IDLE`] without a dictation, which frees its
//! memory (4.5–5 GB). It never runs next to a meeting: it does not start
//! while a meeting records or is processed or a local summary model is open,
//! and it stops when one of those begins ([`stop_model`]); that dictation
//! is then pasted as transcribed. On 24.11.2026 a model run during a call
//! cost that call its audio.

use crate::commands::meeting::LlmDownload;
use crate::meeting::pull::Flight;
use crate::settings::get_settings;
use log::{info, warn};
use once_cell::sync::Lazy;
use serde::Serialize;
use sokki_engine::cleanup::{Engine, Session};
use sokki_engine::llm::{self, PullProgress, CLEANUP_MODEL};
use specta::Type;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::AppHandle;

/// The model's server stops after this long without a dictation, so its
/// 4.5–5 GB are not held next to a summary model. Loading it again took
/// 4.4 s (12.4 s from a cold disk cache) on an M1 Pro, started with the
/// dictation, so speaking hides most of it.
pub const IDLE: Duration = Duration::from_secs(2 * 60);

/// Longest wait after a dictation stops. Expected: about 1 s for 30–60
/// words, 1.6–4.6 s for 140–484 words (M1 Pro, 24.11.2026); a model still
/// loading adds a few seconds.
const FINISH_TIMEOUT: Duration = Duration::from_secs(20);

static ENGINE: Lazy<Arc<Engine>> = Lazy::new(|| Engine::new(CLEANUP_MODEL, IDLE));

struct Current {
    session: Session,
    /// The last live text handed in, so a repeat is not sent again.
    last: String,
}

static CURRENT: Mutex<Option<Current>> = Mutex::new(None);

fn current() -> std::sync::MutexGuard<'static, Option<Current>> {
    CURRENT.lock().unwrap_or_else(|e| e.into_inner())
}

/// One download of the model at a time (`meeting::pull`).
static DOWNLOAD: Flight = Flight::new();

/// Why the cleanup must not run now, if it must not.
fn blocked(
    meeting_recording: bool,
    meeting_processing: bool,
    local_model_job: bool,
) -> Option<&'static str> {
    if meeting_recording {
        Some("a meeting is recording")
    } else if meeting_processing {
        Some("a meeting is being processed")
    } else if local_model_job {
        Some("a summary model is running")
    } else {
        None
    }
}

/// A dictation starts: begins its cleanup, if it is on.
pub fn begin(app: &AppHandle) {
    let settings = get_settings(app);
    if !settings.dictation_cleanup_enabled {
        return;
    }
    if let Some(why) = blocked(
        crate::actions::meeting_recording(app),
        crate::meeting::process::busy(),
        llm::local_job_running(),
    ) {
        info!("Dictation cleanup skipped: {why}; the transcript is pasted as it is");
        return;
    }
    if !ENGINE.available() {
        info!("Dictation cleanup is on, but its model is not downloaded");
        return;
    }
    let session = ENGINE.begin(&settings.custom_words);
    // A session left from an earlier dictation is dropped, which cancels it.
    *current() = Some(Current {
        session,
        last: String::new(),
    });
}

/// The transcript's final part so far (the whole of it, each time).
pub fn live_text(committed: &str) {
    let mut cur = current();
    if let Some(c) = cur.as_mut() {
        if c.last != committed && !committed.trim().is_empty() {
            c.last = committed.to_string();
            c.session.text(committed);
        }
    }
}

/// The dictation was cancelled: its cleanup stops.
pub fn cancel() {
    current().take();
}

/// The cleanup of a dictation that stopped, waiting for its final transcript.
pub struct Pending(Session);

/// A dictation stopped: its cleanup, if one runs. Dropping it cancels it.
pub fn take() -> Option<Pending> {
    current().take().map(|c| Pending(c.session))
}

impl Pending {
    /// The cleaned dictation for its final `transcript`, or `None` when the
    /// model failed or took too long (the transcript is pasted as it is).
    pub async fn finish(self, transcript: String) -> Option<String> {
        if transcript.trim().is_empty() {
            return None;
        }
        let t0 = Instant::now();
        let words = transcript.split_whitespace().count();
        let result = tauri::async_runtime::spawn_blocking(move || {
            self.0.finish(&transcript, FINISH_TIMEOUT)
        })
        .await;
        match result {
            Ok(Ok(text)) if !text.trim().is_empty() => {
                info!(
                    "Dictation cleaned up ({words} words) {:.2} s after the transcript",
                    t0.elapsed().as_secs_f64()
                );
                Some(text)
            }
            Ok(Ok(_)) => {
                warn!("Dictation cleanup returned nothing; pasting the transcript");
                None
            }
            Ok(Err(e)) => {
                warn!("Dictation cleanup failed ({e:#}); pasting the transcript");
                None
            }
            Err(e) => {
                warn!("Dictation cleanup task failed ({e}); pasting the transcript");
                None
            }
        }
    }
}

/// Stops the model's server now: a meeting starts or is processed, or a
/// summary model opens. A dictation being cleaned is pasted as transcribed.
pub fn stop_model() {
    if current().take().is_some() {
        info!("Dictation cleanup stopped for a meeting or summary");
    }
    ENGINE.stop();
}

/// What Settings shows about the cleanup model.
#[derive(Debug, Clone, Serialize, Type)]
pub struct CleanupModel {
    /// "ready" (downloaded), "missing" (can be downloaded) or "unavailable"
    /// (this build has no built-in model server).
    pub state: String,
    /// Bytes to download.
    pub size: u64,
    /// The download running now, if any.
    pub download: Option<LlmDownload>,
}

#[tauri::command]
#[specta::specta]
pub fn dictation_cleanup_model() -> CleanupModel {
    CleanupModel {
        state: if !llm::builtin_server_available() {
            "unavailable"
        } else if llm::model_present(&CLEANUP_MODEL) {
            "ready"
        } else {
            "missing"
        }
        .into(),
        size: CLEANUP_MODEL.download_size(),
        download: DOWNLOAD.progress().map(|p| LlmDownload {
            status: p.status,
            completed: p.completed,
            total: p.total,
        }),
    }
}

/// Downloads the cleanup model. Progress arrives as
/// "dictation-cleanup-pull" events ({status, completed, total}).
#[tauri::command]
#[specta::specta]
pub async fn download_dictation_cleanup_model(app: AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    tauri::async_runtime::spawn_blocking(move || {
        DOWNLOAD.run(|note| {
            llm::download_model(&CLEANUP_MODEL, |p: PullProgress| {
                note(p.clone());
                let _ = app.emit("dictation-cleanup-pull", p);
            })
            .map_err(|e| format!("{e:#}"))
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
#[specta::specta]
pub fn change_dictation_cleanup_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.dictation_cleanup_enabled = enabled;
    crate::settings::write_settings(&app, settings);
    if !enabled {
        cancel();
        stop_model();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::blocked;

    #[test]
    fn never_while_a_meeting_records() {
        assert!(blocked(true, false, false).is_some());
        assert!(blocked(true, true, true).is_some());
    }

    #[test]
    fn never_while_a_meeting_or_summary_is_processed() {
        assert!(blocked(false, true, false).is_some());
        assert!(blocked(false, false, true).is_some());
    }

    #[test]
    fn otherwise_it_runs() {
        assert_eq!(blocked(false, false, false), None);
    }
}

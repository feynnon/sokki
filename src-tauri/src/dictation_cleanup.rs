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
use crate::settings::get_settings;
use log::{info, warn};
use once_cell::sync::Lazy;
use serde::Serialize;
use sokki_engine::cleanup::{Engine, Session};
use sokki_engine::llm::{self, CLEANUP_MODEL};
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
    /// The last text handed to the cleanup, so a repeat is not sent again.
    last: String,
    /// The live text's final part, as last emitted.
    committed: String,
    /// Its tentative part, for redrawing the panel after a step, and the
    /// one before it, to tell which sentences in it have settled.
    tentative: String,
    previous_tentative: String,
    app: AppHandle,
}

impl Current {
    /// What the live panel shows: the cleaned paragraphs, then the part of
    /// the transcript not cleaned yet, split into its final and its
    /// still-changing part. Before the first step, or when the live text no
    /// longer starts with what was cleaned, all of it raw.
    fn panel(&self, committed: &str) -> (String, String, String) {
        let raw = (String::new(), committed.to_string(), self.tentative.clone());
        let view = self.session.view();
        if view.cleaned.is_empty() {
            return raw;
        }
        if let Some(rest) = view.rest(committed) {
            return (view.cleaned, rest.to_string(), self.tentative.clone());
        }
        // Settled sentences of the changing part were cleaned too.
        let all = format!("{committed} {}", self.tentative);
        match view.rest(all.trim()) {
            Some(rest) => (view.cleaned, String::new(), rest.to_string()),
            None => raw,
        }
    }
}

static CURRENT: Mutex<Option<Current>> = Mutex::new(None);

fn current() -> std::sync::MutexGuard<'static, Option<Current>> {
    CURRENT.lock().unwrap_or_else(|e| e.into_inner())
}

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
    let session = ENGINE.begin(&settings.custom_words, redraw);
    // A session left from an earlier dictation is dropped, which cancels it.
    *current() = Some(Current {
        session,
        last: String::new(),
        committed: String::new(),
        tentative: String::new(),
        previous_tentative: String::new(),
        app: app.clone(),
    });
}

/// The live text: `committed` is the transcript's final part so far (the
/// whole of it, each time), `tentative` what may still change. The cleanup
/// gets the final part plus the sentences of the changing part that have
/// settled ([`cleanup::settled`]). Returns what the panel shows: the cleaned
/// paragraphs, and the rest as final and changing text.
pub fn live_text(committed: &str, tentative: &str) -> (String, String, String) {
    let mut cur = current();
    let Some(c) = cur.as_mut() else {
        return (String::new(), committed.to_string(), tentative.to_string());
    };
    c.previous_tentative = std::mem::replace(&mut c.tentative, tentative.to_string());
    c.committed = committed.to_string();
    let settled = sokki_engine::cleanup::settled(&c.previous_tentative, tentative);
    let text = format!("{committed} {settled}").trim().to_string();
    if c.last != text && !text.is_empty() {
        c.last = text.clone();
        c.session.text(&text);
    }
    c.panel(committed)
}

/// A live step finished: the panel shows its cleaned paragraphs at once.
fn redraw() {
    let shown = current().as_ref().map(|c| {
        let (cleaned, committed, tentative) = c.panel(&c.committed);
        (
            c.app.clone(),
            crate::managers::transcription::StreamTextEvent {
                cleaned,
                committed,
                tentative,
            },
        )
    });
    if let Some((app, event)) = shown {
        if crate::test_mic::active() {
            info!(
                "Test mode, live cleaned: {} ‖ {} | {}",
                event.cleaned, event.committed, event.tentative
            );
        }
        use tauri_specta::Event;
        let _ = event.emit(&app);
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
        download: crate::meeting::pull::LLM.progress().map(|p| LlmDownload {
            status: p.status,
            completed: p.completed,
            total: p.total,
        }),
    }
}

/// Downloads the cleanup model: the built-in model, which summaries use
/// too. Progress arrives as "dictation-cleanup-pull" events ({status,
/// completed, total}).
#[tauri::command]
#[specta::specta]
pub async fn download_dictation_cleanup_model(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::meeting::process::download_builtin(&app))
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

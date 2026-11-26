//! Cleans up each dictation before it is pasted, with the built-in cleanup
//! model (`sokki_engine::cleanup`, Gemma 4 E4B in llama-server), while it is
//! spoken.
//!
//! - [`begin`] when a dictation starts: the model's server starts (or is
//!   still running from the last dictation) and a session waits for text.
//! - [`live_text`] with the transcript's final part as it grows (the live
//!   text's committed part and its settled sentences): pieces of about 30
//!   words are cleaned at once.
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
use crate::managers::transcription::StreamTextEvent;
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

/// Test mode only: `SOKKI_TEST_PIECE_WORDS` sets the live piece size, for
/// measuring it on recorded dictations.
fn test_piece_words() -> Option<usize> {
    std::env::var("SOKKI_TEST_PIECE_WORDS")
        .ok()
        .filter(|_| crate::test_mic::active())
        .and_then(|s| s.parse().ok())
}

/// Test mode: how many words the panel shows cleaned and raw (counts only),
/// to measure how much raw text Robin sees while speaking.
fn log_panel(cleaned: &str, committed: &str, tentative: &str) {
    if crate::test_mic::active() {
        let n = |t: &str| t.split_whitespace().count();
        info!(
            "Test mode, panel: {} cleaned, {} raw",
            n(cleaned),
            n(committed) + n(tentative)
        );
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
    let live = sokki_engine::cleanup::Live::new(&settings.custom_words);
    let live = match test_piece_words() {
        Some(n) => live.with_piece_words(n),
        None => live,
    };
    let session = ENGINE.begin_with(live, redraw);
    // So the focused field is readable at the stop, for the instant paste.
    crate::instant_paste::wake();
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
pub fn live_text(committed: &str, tentative: &str) -> StreamTextEvent {
    let mut cur = current();
    let Some(c) = cur.as_mut() else {
        return StreamTextEvent {
            cleaned: String::new(),
            committed: committed.to_string(),
            tentative: tentative.to_string(),
            cleanup: false,
        };
    };
    c.previous_tentative = std::mem::replace(&mut c.tentative, tentative.to_string());
    c.committed = committed.to_string();
    let settled = sokki_engine::cleanup::settled(&c.previous_tentative, tentative);
    let text = format!("{committed} {settled}").trim().to_string();
    if c.last != text && !text.is_empty() {
        c.last = text.clone();
        c.session.text(&text);
    }
    let (cleaned, committed, tentative) = c.panel(committed);
    log_panel(&cleaned, &committed, &tentative);
    StreamTextEvent {
        cleaned,
        committed,
        tentative,
        cleanup: true,
    }
}

/// A live step finished: the panel shows its cleaned paragraphs at once.
fn redraw() {
    let shown = current().as_ref().map(|c| {
        let (cleaned, committed, tentative) = c.panel(&c.committed);
        (
            c.app.clone(),
            StreamTextEvent {
                cleaned,
                committed,
                tentative,
                cleanup: true,
            },
        )
    });
    if let Some((app, event)) = shown {
        if crate::test_mic::active() {
            info!(
                "Test mode, live cleaned: {} ‖ {} | {}",
                event.cleaned, event.committed, event.tentative
            );
            log_panel(&event.cleaned, &event.committed, &event.tentative);
        }
        use tauri_specta::Event;
        let _ = event.emit(&app);
    }
}

/// The cleanup of one phone dictation through this Mac
/// (`meeting/phone_link.rs`): its own session, beside the Mac's own
/// dictation and its overlay. Same model, settings and live steps.
pub struct PhoneCleanup {
    session: Session,
    last: String,
    committed: String,
    tentative: String,
    previous_tentative: String,
    /// Set when a live step finished: the phone gets the cleaned text.
    stepped: Arc<std::sync::atomic::AtomicBool>,
}

/// The phone keyboard opened: start the cleanup model, when the cleanup is
/// on and may run, so a dictation finds it loaded (4.4 s, 12.4 s from a
/// cold disk on an M1 Pro). It stops again after [`IDLE`] unused.
pub fn warm_for_phone(app: &AppHandle) {
    let settings = get_settings(app);
    if settings.dictation_cleanup_enabled
        && ENGINE.available()
        && blocked(
            crate::actions::meeting_recording(app),
            crate::meeting::process::busy(),
            llm::local_job_running(),
        )
        .is_none()
    {
        ENGINE.warm();
    }
}

/// A phone dictation starts: its cleanup, when the cleanup is on and may
/// run now (no meeting recording or being processed, no summary model).
pub fn begin_for_phone(app: &AppHandle) -> Option<PhoneCleanup> {
    let settings = get_settings(app);
    if !settings.dictation_cleanup_enabled {
        return None;
    }
    if let Some(why) = blocked(
        crate::actions::meeting_recording(app),
        crate::meeting::process::busy(),
        llm::local_job_running(),
    ) {
        info!("Phone dictation cleanup skipped: {why}");
        return None;
    }
    if !ENGINE.available() {
        return None;
    }
    let stepped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = stepped.clone();
    let live = sokki_engine::cleanup::Live::new(&settings.custom_words);
    let session = ENGINE.begin_with(live, move || {
        flag.store(true, std::sync::atomic::Ordering::SeqCst)
    });
    Some(PhoneCleanup {
        session,
        last: String::new(),
        committed: String::new(),
        tentative: String::new(),
        previous_tentative: String::new(),
        stepped,
    })
}

impl PhoneCleanup {
    /// The live text: `committed` is final, `tentative` may still change.
    /// Hands the cleanup the final part plus the settled sentences, as for a
    /// dictation here.
    pub fn live(&mut self, committed: &str, tentative: &str) {
        self.previous_tentative = std::mem::replace(&mut self.tentative, tentative.to_string());
        self.committed = committed.to_string();
        let settled = sokki_engine::cleanup::settled(&self.previous_tentative, tentative);
        let text = format!("{committed} {settled}").trim().to_string();
        if self.last != text && !text.is_empty() {
            self.last = text.clone();
            self.session.text(&text);
        }
    }

    /// A live step finished since the last call.
    pub fn stepped(&self) -> bool {
        self.stepped
            .swap(false, std::sync::atomic::Ordering::SeqCst)
    }

    /// What the phone shows: the cleaned paragraphs, then what isn't
    /// cleaned yet as transcribed.
    pub fn shown(&self) -> String {
        let all = format!("{} {}", self.committed, self.tentative)
            .trim()
            .to_string();
        let view = self.session.view();
        if view.cleaned.is_empty() {
            return all;
        }
        let rest = view
            .rest(&self.committed)
            .map(|r| format!("{r} {}", self.tentative))
            .or_else(|| view.rest(&all).map(str::to_string));
        match rest {
            Some(rest) if !rest.trim().is_empty() => format!("{} {}", view.cleaned, rest.trim()),
            Some(_) => view.cleaned,
            None => all,
        }
    }

    /// The whole dictation right away, for its final `transcript`: the
    /// paragraphs cleaned so far, a blank line, the rest as transcribed (as
    /// the cleanup's own draft, without waiting for a step that runs).
    pub fn draft(&self, transcript: &str) -> String {
        let view = self.session.view();
        match view.rest(transcript) {
            Some(rest) if !view.cleaned.is_empty() && !rest.trim().is_empty() => {
                format!("{}\n\n{}", view.cleaned, rest.trim())
            }
            Some(_) if !view.cleaned.is_empty() => view.cleaned,
            _ => transcript.trim().to_string(),
        }
    }

    /// The cleaned dictation for its final `transcript`, or `None` when the
    /// model failed or took too long. Blocks. `on_draft` gets the whole text
    /// before the last steps (the paragraphs cleaned so far, the rest as
    /// transcribed), as the instant paste does.
    pub fn finish(
        self,
        transcript: &str,
        on_draft: impl FnOnce(String) + Send + 'static,
    ) -> Option<String> {
        if transcript.trim().is_empty() {
            return None;
        }
        let t0 = Instant::now();
        let on_draft: Box<dyn FnOnce(sokki_engine::cleanup::Draft) + Send> =
            Box::new(move |d| on_draft(d.text()));
        match self
            .session
            .finish_with_draft(transcript, FINISH_TIMEOUT, Some(on_draft))
        {
            Ok(text) if !text.trim().is_empty() => {
                info!(
                    "Phone dictation cleaned up ({} words) {:.2} s after the transcript",
                    transcript.split_whitespace().count(),
                    t0.elapsed().as_secs_f64()
                );
                Some(text)
            }
            Ok(_) => None,
            Err(e) => {
                warn!("Phone dictation cleanup failed ({e:#}); sending the transcript");
                None
            }
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
    /// `on_draft` gets the draft before the last steps (`instant_paste`).
    pub async fn finish(
        self,
        transcript: String,
        on_draft: Option<Box<dyn FnOnce(sokki_engine::cleanup::Draft) + Send>>,
    ) -> Option<String> {
        if transcript.trim().is_empty() {
            return None;
        }
        let t0 = Instant::now();
        let words = transcript.split_whitespace().count();
        let result = tauri::async_runtime::spawn_blocking(move || {
            self.0
                .finish_with_draft(&transcript, FINISH_TIMEOUT, on_draft)
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
    let done = tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || crate::meeting::process::download_builtin(&app)
    })
    .await
    .map_err(|e| e.to_string())?;
    crate::overlay::set_tall_live(active(&app));
    done
}

/// Whether dictations are cleaned up now: the switch is on and the model is
/// there. Without the model (8 GB Macs, or not downloaded yet) the cleanup
/// does nothing and the live panel keeps its three lines.
pub fn active(app: &AppHandle) -> bool {
    get_settings(app).dictation_cleanup_enabled && ENGINE.available()
}

#[tauri::command]
#[specta::specta]
pub fn change_dictation_cleanup_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.dictation_cleanup_enabled = enabled;
    crate::settings::write_settings(&app, settings);
    crate::overlay::set_tall_live(active(&app));
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

//! The line Sokki posts into the meeting chat when it starts recording a
//! call, so everyone in the call knows, and can say no.
//!
//! - Once per call, from the person's own account, through the meeting
//!   app's chat field (`meeting-chat` crate, macOS Accessibility). Zoom and
//!   Teams apps, Google Meet in Chrome, Arc, Edge and Brave.
//! - Then Sokki watches that chat. When someone writes "stop" (or "stopp")
//!   as a message of its own, Sokki ends the recording, deletes it, and
//!   answers in the chat. GDPR Art. 21 wants an objection to work in
//!   practice (BayLDA report 2025, 13.1); Sokki can't take one voice out
//!   of the mixed call audio, so it deletes the whole recording.
//! - If the line can't be posted, a small banner says so.
//!
//! Switch: `meetings.chat_notice_enabled`.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use meeting_chat::{PostError, Target};
use tauri::{AppHandle, Manager};

use super::MeetingManager;

/// How long Sokki keeps trying while the meeting isn't on screen yet
/// (still joining, Meet in a background tab, someone typing a draft).
const TRY_FOR: Duration = Duration::from_secs(120);
const RETRY_EVERY: Duration = Duration::from_secs(5);
const WATCH_EVERY: Duration = Duration::from_secs(3);

/// The texts, in the language of Sokki's interface (German or English).
pub struct Texts {
    pub notice: &'static str,
    pub stopped_reply: &'static str,
    /// Banners for the person; `{app}` is "Zoom", "Teams" or "Google Meet".
    pub stopped_banner: &'static str,
    pub failed_banner: &'static str,
    pub needs_accessibility: &'static str,
}

pub fn texts(app_language: &str) -> Texts {
    if app_language.to_lowercase().starts_with("de") {
        Texts {
            // Robin approved this wording on 2026-11-24.
            notice: "Ich schreibe lokal mit Sokki mit, nichts geht in eine Cloud. Nicht okay? Schreib \u{201A}stopp\u{2018}. sokki.org",
            stopped_reply: "Gestoppt und gelöscht.",
            stopped_banner: "Jemand hat \u{201E}stopp\u{201C} in den {app}-Chat geschrieben. Die Aufnahme ist gestoppt und gelöscht.",
            failed_banner: "Der Hinweis im {app}-Chat ging nicht. Sag den anderen, dass du aufnimmst.",
            needs_accessibility: " Sokki braucht dafür die Bedienungshilfen.",
        }
    } else {
        Texts {
            // The English counterpart; waits for Robin's word-for-word OK.
            notice: "I'm taking notes locally with Sokki, nothing goes to a cloud. Not okay? Type 'stop'. sokki.org",
            stopped_reply: "Stopped and deleted.",
            stopped_banner: "Someone wrote \u{201C}stop\u{201D} in the {app} chat. The recording is stopped and deleted.",
            failed_banner: "Couldn't post the notice in the {app} chat. Let the others know you're recording.",
            needs_accessibility: " Sokki needs Accessibility for it.",
        }
    }
}

/// Which call already got its line: once per call, also when the recording
/// is stopped and started again during it.
#[derive(Default)]
pub struct ChatNoticeState {
    noticed: Mutex<Option<String>>,
}

/// The call (a `meeting-detect` app id) has ended.
pub fn call_ended(app: &AppHandle, call_app: &str) {
    let state = app.state::<Arc<ChatNoticeState>>();
    let mut noticed = state.noticed.lock().unwrap_or_else(|e| e.into_inner());
    if noticed.as_deref() == Some(call_app) {
        *noticed = None;
    }
}

/// Sokki started recording `meeting_id` while the call in `call_app` runs.
pub fn recording_started(app: &AppHandle, meeting_id: &str, call_app: &str) {
    let settings = crate::settings::get_settings(app);
    if !settings.meetings.chat_notice_enabled {
        return;
    }
    let Some(target) = Target::for_app(call_app) else {
        log::info!("Chat notice: {call_app} has no chat Sokki can write in");
        return;
    };
    {
        let state = app.state::<Arc<ChatNoticeState>>();
        let mut noticed = state.noticed.lock().unwrap_or_else(|e| e.into_inner());
        if noticed.as_deref() == Some(call_app) {
            log::info!("Chat notice: already posted in this {} call", target.name());
            return;
        }
        *noticed = Some(call_app.to_string());
    }
    let texts = texts(&settings.app_language);
    let (app, meeting_id) = (app.clone(), meeting_id.to_string());
    let spawned = std::thread::Builder::new()
        .name("chat-notice".into())
        .spawn(move || run(&app, &meeting_id, target, texts));
    if let Err(e) = spawned {
        log::warn!("Chat notice: could not start: {e}");
    }
}

fn still_recording(app: &AppHandle, meeting_id: &str) -> bool {
    app.state::<Arc<MeetingManager>>().active_id().as_deref() == Some(meeting_id)
}

fn run(app: &AppHandle, meeting_id: &str, target: Target, texts: Texts) {
    // Let the call window settle (the start prompt was just answered).
    std::thread::sleep(Duration::from_secs(2));
    let start = Instant::now();
    let mut chat = loop {
        if !still_recording(app, meeting_id) {
            return;
        }
        match meeting_chat::post(target, texts.notice) {
            Ok(chat) => {
                log::info!("Chat notice posted in {} for {meeting_id}", target.name());
                break chat;
            }
            Err(e) if e.retryable() && start.elapsed() < TRY_FOR => {
                log::info!("Chat notice: {e}; trying again");
                std::thread::sleep(RETRY_EVERY);
            }
            Err(e) => {
                log::warn!("Chat notice not posted in {}: {e}", target.name());
                failed(target, &e, &texts);
                return;
            }
        }
    };
    while still_recording(app, meeting_id) {
        std::thread::sleep(WATCH_EVERY);
        match chat.stop_requested() {
            Some(true) => {
                log::info!(
                    "Chat notice: someone wrote stop in the {} chat",
                    target.name()
                );
                if !still_recording(app, meeting_id) {
                    return;
                }
                match super::discard_meeting(app) {
                    Ok(id) => {
                        log::info!("Meeting {id} stopped and deleted after a stop in the chat")
                    }
                    Err(e) => log::warn!("Stopping after a stop in the chat failed: {e}"),
                }
                if let Err(e) = chat.reply(texts.stopped_reply) {
                    log::warn!("Chat notice: reply not posted: {e}");
                }
                banner(&texts.stopped_banner.replace("{app}", target.name()));
                return;
            }
            Some(false) => {}
            None => {
                log::info!(
                    "Chat notice: the {} chat can't be read any more; not watching it",
                    target.name()
                );
                return;
            }
        }
    }
}

/// A banner for the person; in test mode only logged.
fn banner(body: &str) {
    if crate::test_mic::active() {
        log::info!("Chat notice: test mode, banner not shown: {body}");
    } else {
        super::detect::notify(crate::brand::app_name(), body);
    }
}

fn failed(target: Target, error: &PostError, texts: &Texts) {
    let mut body = texts.failed_banner.replace("{app}", target.name());
    if *error == PostError::NotTrusted {
        body.push_str(texts.needs_accessibility);
    }
    banner(&body);
}

#[cfg(test)]
mod tests {
    use super::texts;

    #[test]
    fn language() {
        assert!(texts("de-DE").notice.starts_with("Ich schreibe"));
        assert!(texts("en").notice.starts_with("I'm taking"));
        assert!(texts("fr").notice.starts_with("I'm taking"));
    }

    #[test]
    fn notice_is_not_a_stop() {
        for lang in ["de", "en"] {
            let t = texts(lang);
            assert!(!meeting_chat::is_stop(t.notice));
            assert!(!meeting_chat::is_stop(t.stopped_reply));
        }
    }
}

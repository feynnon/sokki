//! Meeting detection in the app: when a call app starts using the microphone,
//! ask whether to record; when it stops, offer to stop (and stop on its own
//! after a while, so a recording never runs on after the call). Never starts
//! recording without the user saying yes.
//!
//! Detection itself is the `meeting-detect` crate (approach from Lark, MIT).

use meeting_detect::{MeetingEvent, Watcher, WatcherConfig};
use std::process::Command;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

use super::MeetingManager;

/// Keeps the watcher alive and remembers which meeting detection started.
#[derive(Default)]
pub struct DetectState {
    watcher: Mutex<Option<Watcher>>,
    /// (app id, meeting id) of a recording started from a detection prompt.
    started: Mutex<Option<(String, String)>>,
}

/// Starts or stops the watcher to match the setting. Safe to call repeatedly.
pub fn sync_with_settings(app: &AppHandle) {
    let enabled = crate::settings::get_settings(app)
        .meetings
        .detection_enabled;
    let state = app.state::<Arc<DetectState>>();
    let mut watcher = state.watcher.lock().unwrap_or_else(|e| e.into_inner());
    match (enabled, watcher.is_some()) {
        (true, false) => {
            let handle = app.clone();
            match Watcher::spawn(WatcherConfig::default(), std::process::id(), move |event| {
                on_event(&handle, event)
            }) {
                Ok(w) => *watcher = Some(w),
                Err(e) => log::warn!("Meeting detection could not start: {e}"),
            }
        }
        (false, true) => {
            if let Some(w) = watcher.take() {
                w.stop();
            }
        }
        _ => {}
    }
}

fn on_event(app: &AppHandle, event: MeetingEvent) {
    match event {
        MeetingEvent::Started { app: meeting } => {
            if app.state::<Arc<MeetingManager>>().is_recording() {
                return;
            }
            let handle = app.clone();
            let (id, name) = (meeting.id.to_string(), meeting.name.to_string());
            std::thread::spawn(move || {
                let question = format!("{name}-Meeting erkannt. Aufnehmen?\n\nSag allen Beteiligten, dass du aufnimmst.");
                // Sokki also offers the heads-up line for the call chat.
                let mut buttons = vec!["Nicht jetzt", "Aufnehmen"];
                if crate::brand::is_sokki() {
                    buttons.push(COPY_AND_RECORD);
                }
                let answer = choose(&question, &buttons, "Aufnehmen", 45);
                if answer.as_deref() == Some(COPY_AND_RECORD) {
                    if let Err(e) = super::heads_up::copy(&handle) {
                        log::warn!("Copying the heads-up failed: {e}");
                    }
                }
                if matches!(answer.as_deref(), Some("Aufnehmen" | COPY_AND_RECORD)) {
                    match super::start_meeting(&handle, Some(name)) {
                        Ok(meeting_id) => {
                            let state = handle.state::<Arc<DetectState>>();
                            *state.started.lock().unwrap_or_else(|e| e.into_inner()) =
                                Some((id, meeting_id));
                        }
                        Err(e) => log::warn!("Starting the detected meeting failed: {e}"),
                    }
                }
            });
        }
        MeetingEvent::Ended { app: meeting, .. } => {
            let state = app.state::<Arc<DetectState>>();
            let ours = state
                .started
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let manager = app.state::<Arc<MeetingManager>>();
            let Some((app_id, meeting_id)) = ours else {
                return;
            };
            if app_id != meeting.id || manager.active_id().as_deref() != Some(meeting_id.as_str()) {
                return;
            }
            let handle = app.clone();
            let name = meeting.name.to_string();
            std::thread::spawn(move || {
                // No answer within 20 s counts as "stop": the call is over.
                let keep = ask(
                    &format!("Das {name}-Meeting ist vorbei. Aufnahme beenden?"),
                    "Beenden",
                    "Weiter aufnehmen",
                    20,
                ) == Some(false);
                if !keep {
                    let still_ours = handle.state::<Arc<MeetingManager>>().active_id().as_deref()
                        == Some(meeting_id.as_str());
                    if still_ours {
                        if let Err(e) = super::stop_meeting(&handle) {
                            log::warn!("Stopping the detected meeting failed: {e}");
                        }
                    }
                }
                let state = handle.state::<Arc<DetectState>>();
                *state.started.lock().unwrap_or_else(|e| e.into_inner()) = None;
            });
        }
    }
}

/// The third button on the "meeting detected" dialog in Sokki.
const COPY_AND_RECORD: &str = "Aufnehmen + Hinweis kopieren";

/// Shows a dialog with two buttons. `Some(true)` for `yes`, `Some(false)` for
/// `no`, `None` when it timed out or failed.
fn ask(question: &str, yes: &str, no: &str, timeout_s: u32) -> Option<bool> {
    choose(question, &[no, yes], yes, timeout_s).map(|b| b == yes)
}

/// Shows a dialog with up to three buttons (left to right). The label of the
/// button pressed, or `None` when it timed out or failed.
fn choose(question: &str, buttons: &[&str], default: &str, timeout_s: u32) -> Option<String> {
    let list = buttons
        .iter()
        .map(|b| applescript_string(b))
        .collect::<Vec<_>>()
        .join(", ");
    let script = format!(
        "display dialog {} buttons {{{}}} default button {} with title {} giving up after {}",
        applescript_string(question),
        list,
        applescript_string(default),
        applescript_string(crate::brand::app_name()),
        timeout_s
    );
    let out = Command::new("osascript")
        .args(["-e", &script])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    if text.contains("gave up:true") || !out.status.success() {
        return None;
    }
    // Output is "button returned:X, gave up:false".
    let pressed = text.split("button returned:").nth(1)?;
    let pressed = pressed.split(", gave up:").next()?.trim();
    buttons
        .iter()
        .find(|b| **b == pressed)
        .map(|b| b.to_string())
}

/// A macOS notification banner, as Sokki (Script Editor via osascript only
/// when the app can't use the notification centre).
pub fn notify(title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    if super::notify_macos::notify(title, body, notify_osascript) {
        return;
    }
    notify_osascript(title, body);
}

fn notify_osascript(title: &str, body: &str) {
    let script = format!(
        "display notification {} with title {}",
        applescript_string(body),
        applescript_string(title)
    );
    let _ = Command::new("osascript").args(["-e", &script]).spawn();
}

fn applescript_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::applescript_string;

    #[test]
    fn quotes_are_escaped() {
        assert_eq!(applescript_string(r#"a "b" \c"#), r#""a \"b\" \\c""#);
    }
}

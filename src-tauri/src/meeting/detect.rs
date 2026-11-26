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
    /// App id of the call running now, however the recording was started.
    current: Mutex<Option<String>>,
}

/// A recording started (from the prompt, a shortcut or the window): if a
/// call is running, Sokki says so in its chat (`chat_notice`).
pub fn recording_started(app: &AppHandle, meeting_id: &str) {
    let Some(state) = app.try_state::<Arc<DetectState>>() else {
        return;
    };
    let call = state
        .current
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .or_else(crate::test_mic::call);
    if let Some(call) = call {
        #[cfg(target_os = "macos")]
        super::call_speakers::recording_started(app, meeting_id, &call);
        super::chat_notice::recording_started(app, meeting_id, &call);
        super::slack::recording_started(app, meeting_id, &call);
    }
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
            *app.state::<Arc<DetectState>>()
                .current
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(meeting.id.to_string());
            if let Some(recording) = app.state::<Arc<MeetingManager>>().active_id() {
                // Recording already runs (started before the call): the
                // call still gets its line.
                #[cfg(target_os = "macos")]
                super::call_speakers::recording_started(app, &recording, meeting.id);
                super::chat_notice::recording_started(app, &recording, meeting.id);
                return;
            }
            let handle = app.clone();
            let (id, name) = (meeting.id.to_string(), meeting.label().to_string());
            std::thread::spawn(move || {
                // The screen can be recorded too (macOS only).
                let answer = super::prompt::ask(
                    &handle,
                    super::prompt::Kind::Started {
                        screen: cfg!(target_os = "macos"),
                    },
                    &name,
                    std::time::Duration::from_secs(45),
                );
                let started = match answer.as_deref() {
                    Some("record") => {
                        log::info!("Meeting start via call detection ({name})");
                        Some(super::start_meeting(&handle, Some(name)))
                    }
                    #[cfg(target_os = "macos")]
                    Some("record_screen") => {
                        log::info!("Meeting start with screen via call detection ({name})");
                        // Without Screen Recording yet (macOS was just asked),
                        // the meeting is still recorded, without the screen.
                        Some(
                            crate::video::start(&handle, Some(name.clone())).or_else(|e| {
                                log::warn!(
                                    "Meeting with screen: {e}; recording the meeting without it"
                                );
                                super::start_meeting(&handle, Some(name))
                            }),
                        )
                    }
                    _ => None,
                };
                if started.is_some() {
                    crate::usage::count(&handle, crate::usage::Count::MeetingStartedFromCall);
                }
                if let Some(started) = started {
                    match started {
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
            {
                let mut current = state.current.lock().unwrap_or_else(|e| e.into_inner());
                if current.as_deref() == Some(meeting.id) {
                    *current = None;
                }
            }
            super::chat_notice::call_ended(app, meeting.id);
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
                let keep = super::prompt::ask(
                    &handle,
                    super::prompt::Kind::Ended,
                    &name,
                    std::time::Duration::from_secs(20),
                )
                .as_deref()
                    == Some("keep");
                if !keep {
                    let still_ours = handle.state::<Arc<MeetingManager>>().active_id().as_deref()
                        == Some(meeting_id.as_str());
                    if still_ours {
                        log::info!("Meeting stop via call detection: the {name} call ended");
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

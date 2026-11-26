//! Who spoke when, as the call app shows it (board card speaker-names):
//! Google Meet's captions and Zoom's "active speaker", read through
//! Accessibility (`meeting_chat::SpeakerWatch`) while a call is recorded,
//! saved as `call_speakers.json` next to the audio. After processing,
//! `voices::after_processing` lays these spans over Sokki's own speakers
//! (`sokki_engine::voices::names_from_call`).
//!
//! Names from the call app are no biometric data: Sokki keeps no voice for
//! them (only the user's consent does that).
//!
//! Test mode: `SOKKI_TEST_CALL_SPEAKERS=<file>` copies a prepared
//! `call_speakers.json` into the test meeting instead of reading a call.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use meeting_chat::Target;
use sokki_engine::voices::CallSpan;
use tauri::{AppHandle, Manager};

use super::store::MeetingStore;
use super::MeetingManager;

pub const FILE: &str = "call_speakers.json";
const POLL: Duration = Duration::from_millis(500);
/// A name keeps speaking across pauses shorter than this.
const GAP_S: f64 = 1.5;
const SAVE_EVERY: Duration = Duration::from_secs(10);

/// Sokki started recording `meeting_id` while the call in `call_app` runs.
pub fn recording_started(app: &AppHandle, meeting_id: &str, call_app: &str) {
    let store = app.state::<Arc<MeetingStore>>().inner().clone();
    let dir = store.audio_dir(meeting_id);
    if let Some(file) =
        std::env::var_os("SOKKI_TEST_CALL_SPEAKERS").filter(|_| crate::test_mic::active())
    {
        let _ = std::fs::create_dir_all(&dir);
        match std::fs::copy(&file, dir.join(FILE)) {
            Ok(_) => log::info!(
                "Test mode: call speakers from {}",
                Path::new(&file).display()
            ),
            Err(e) => log::warn!("Test mode: call speakers not copied: {e}"),
        }
        return;
    }
    if !crate::settings::get_settings(app)
        .meetings
        .call_names_enabled
    {
        return;
    }
    let Some(target) = Target::for_app(call_app) else {
        return;
    };
    if matches!(target, Target::Teams) {
        return;
    }
    let (app, id) = (app.clone(), meeting_id.to_string());
    let spawned = std::thread::Builder::new()
        .name("call-speakers".into())
        .spawn(move || {
            // Seconds from the recording's start: the call may begin after it.
            let offset = store
                .get(&id)
                .and_then(|r| chrono::DateTime::parse_from_rfc3339(&r.started_at).ok())
                .map(|t| {
                    (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_milliseconds() as f64
                        / 1000.0
                })
                .unwrap_or(0.0)
                .max(0.0);
            run(&app, &id, &dir, target, offset)
        });
    if let Err(e) = spawned {
        log::warn!("Call speakers: could not start: {e}");
    }
}

fn still_recording(app: &AppHandle, meeting_id: &str) -> bool {
    app.state::<Arc<MeetingManager>>().active_id().as_deref() == Some(meeting_id)
}

fn run(app: &AppHandle, meeting_id: &str, dir: &Path, target: Target, offset: f64) {
    let started = Instant::now();
    // Let the call window settle, as the chat notice does.
    std::thread::sleep(Duration::from_secs(3));
    let mut watch = None;
    let mut spans: Vec<CallSpan> = Vec::new();
    let mut saved = Instant::now();
    let mut logged_names = 0usize;
    while still_recording(app, meeting_id) {
        std::thread::sleep(POLL);
        if watch.is_none() {
            match meeting_chat::SpeakerWatch::new(target) {
                Ok(w) => watch = Some(w),
                Err(e) => {
                    log::info!("Call speakers: not reading {}: {e}", target.name());
                    if !e.retryable() {
                        return;
                    }
                    std::thread::sleep(Duration::from_secs(5));
                    continue;
                }
            }
        }
        let Some(w) = watch.as_mut() else { continue };
        let now = offset + started.elapsed().as_secs_f64();
        for name in w.poll() {
            add(&mut spans, &name, now);
        }
        let names = distinct(&spans);
        if names > logged_names {
            // Counts only: names stay out of the log.
            log::info!("Call speakers: {names} name(s) seen in {}", target.name());
            logged_names = names;
        }
        if saved.elapsed() >= SAVE_EVERY {
            save(dir, &spans);
            saved = Instant::now();
        }
    }
    save(dir, &spans);
    log::info!(
        "Call speakers: {} span(s) of {} name(s) from {}",
        spans.len(),
        distinct(&spans),
        target.name()
    );
}

/// `name` speaks at `now`: extends its last span or opens a new one.
fn add(spans: &mut Vec<CallSpan>, name: &str, now: f64) {
    if let Some(s) = spans.iter_mut().rev().find(|s| s.name == name) {
        if now - s.end_s <= GAP_S {
            s.end_s = now;
            return;
        }
    }
    spans.push(CallSpan {
        start_s: (now - POLL.as_secs_f64()).max(0.0),
        end_s: now,
        name: name.to_string(),
    });
}

fn distinct(spans: &[CallSpan]) -> usize {
    let mut names: Vec<&str> = spans.iter().map(|s| s.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    names.len()
}

fn save(dir: &Path, spans: &[CallSpan]) {
    if spans.is_empty() {
        return;
    }
    if let Ok(text) = serde_json::to_string(spans) {
        let _ = std::fs::write(dir.join(FILE), text);
    }
}

pub fn load(dir: &Path) -> Vec<CallSpan> {
    std::fs::read_to_string(dir.join(FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_merge_short_pauses() {
        let mut s = Vec::new();
        add(&mut s, "Lena", 1.0);
        add(&mut s, "Lena", 1.5);
        add(&mut s, "Lena", 2.5);
        add(&mut s, "Jonas", 3.0);
        add(&mut s, "Lena", 6.0);
        assert_eq!(s.len(), 3);
        assert_eq!((s[0].start_s, s[0].end_s), (0.5, 2.5));
        assert_eq!(distinct(&s), 2);
    }
}

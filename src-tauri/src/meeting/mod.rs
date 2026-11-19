//! Meeting notes (macOS): record, then turn the recording into a Markdown note.
//!
//! Recording (from Mandy, MIT): the microphone and, unless in microphone-only
//! mode, the system audio are written to separate FLAC files in the meeting's
//! own folder. The microphone is the user's side (plus a phone on speaker or
//! people in the room); system audio is the remote side of a call on this Mac.
//!
//! Processing (`process`): speakers are separated, every speaker turn is
//! transcribed with Handy's model, echoes of the system track are removed
//! from the microphone track, a language model writes the summary, and the
//! note is saved to the notes folder. Progress is tracked in `store`.
//!
//! Dictation has its own recorder and state; this module shares none of them.

pub mod detect;
pub mod diarize_worker;
pub mod flac;
pub mod indicator;
pub mod memguard;
pub mod mic;
#[cfg(target_os = "macos")]
mod notify_macos;
pub mod process;
pub mod rate;
pub mod store;
pub mod system_tap;

use anyhow::{bail, Context, Result};
use mic::{MicCapture, MicRecording};
use std::path::Path;
use std::sync::{Arc, Mutex};
use store::{MeetingRecord, MeetingStatus, MeetingStore};
use system_tap::{SystemAudioCapture, SystemRecording};
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

pub use crate::commands::meeting::MeetingChanged;

use crate::notices::{self, Notice, NoticeCode, NoticeSource};

/// Live input levels (RMS, f32 bits) of the running meeting, for the
/// recording indicator. Written by the capture threads, read by its ticker.
pub static MIC_LEVEL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub static SYSTEM_LEVEL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Raises a level meter to the RMS of `samples` (never lowers it; the
/// indicator's ticker reads and resets it).
pub fn note_level(meter: &std::sync::atomic::AtomicU32, samples: &[f32]) {
    use std::sync::atomic::Ordering;
    if samples.is_empty() {
        return;
    }
    let rms = (samples.iter().map(|v| v * v).sum::<f32>() / samples.len() as f32).sqrt();
    let _ = meter.try_update(Ordering::Relaxed, Ordering::Relaxed, |old| {
        (rms > f32::from_bits(old)).then_some(rms.to_bits())
    });
}

/// Wall-clock time (µs since the epoch) of the first sample of each track of
/// the running meeting; 0 until it arrives. The two captures start a little
/// apart (opening the microphone takes a moment), so processing lines the
/// tracks up with these.
pub static MIC_FIRST_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static SYSTEM_FIRST_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Records the start time of a track from its first delivered chunk of
/// `frames` frames at `rate` Hz (the chunk ends now).
pub fn note_first_sample(slot: &std::sync::atomic::AtomicU64, frames: usize, rate: u32) {
    use std::sync::atomic::Ordering;
    if slot.load(Ordering::Relaxed) != 0 || rate == 0 {
        return;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0);
    let chunk_us = frames as u64 * 1_000_000 / rate as u64;
    let _ = slot.compare_exchange(
        0,
        now.saturating_sub(chunk_us),
        Ordering::Relaxed,
        Ordering::Relaxed,
    );
}

/// Start times of the tracks, saved next to the audio for processing.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, Default)]
pub struct TrackTiming {
    pub mic_first_us: u64,
    pub system_first_us: u64,
}

pub const TIMING_FILE: &str = "timing.json";

pub const MIC_FILE: &str = "mic.flac";
pub const SYSTEM_FILE: &str = "system.flac";

/// Files produced by one meeting, finalized.
#[derive(Debug)]
pub struct MeetingRecording {
    pub mic: MicRecording,
    pub system: Option<SystemRecording>,
}

struct ActiveMeeting {
    meeting_id: String,
    started: std::time::Instant,
    // Stopped in `stop`. Dropping them ends the capture as well.
    system: Option<SystemAudioCapture>,
    mic: MicCapture,
}

#[derive(Default)]
pub struct MeetingManager {
    active: Mutex<Option<ActiveMeeting>>,
}

impl MeetingManager {
    /// Starts recording into `dir`: always the microphone (`mic.flac`), and
    /// system audio (`system.flac`) whenever it can be captured. Whether the
    /// meeting was online or on one microphone is decided afterwards, from
    /// whether the system track has speech. Returns warnings (e.g. system
    /// audio unavailable) that do not stop the recording.
    pub fn start(&self, meeting_id: &str, dir: &Path) -> Result<Vec<String>> {
        let mut active = self.lock();
        if active.is_some() {
            bail!("A meeting is already being recorded.");
        }
        std::fs::create_dir_all(dir).with_context(|| format!("Creating {}", dir.display()))?;

        MIC_FIRST_US.store(0, std::sync::atomic::Ordering::Relaxed);
        SYSTEM_FIRST_US.store(0, std::sync::atomic::Ordering::Relaxed);
        // System audio first: it is the step that can fail on permission.
        // Without it the meeting still records the microphone.
        let mut warnings = Vec::new();
        let system = match SystemAudioCapture::start(dir.join(SYSTEM_FILE)) {
            Ok(s) => Some(s),
            Err(e) => {
                log::warn!("System audio capture unavailable: {e:#}");
                warnings.push(format!(
                    "Only the microphone was recorded; system audio was unavailable ({e:#}). Allow \"System Audio Recording\" for {} in System Settings > Privacy & Security to capture calls on this Mac.",
                    crate::brand::app_name()
                ));
                None
            }
        };
        let mic = match MicCapture::start(&dir.join(MIC_FILE)) {
            Ok(mic) => mic,
            Err(e) => {
                if let Some(system) = system {
                    let _ = system.stop();
                }
                return Err(e).context("Starting the microphone");
            }
        };

        *active = Some(ActiveMeeting {
            meeting_id: meeting_id.to_string(),
            started: std::time::Instant::now(),
            system,
            mic,
        });
        Ok(warnings)
    }

    /// Stops both halves and finalizes their files. Both are stopped even
    /// when one fails; the first error is returned.
    pub fn stop(&self) -> Result<(String, f64, MeetingRecording)> {
        let Some(meeting) = self.lock().take() else {
            bail!("No meeting is being recorded.");
        };
        let duration = meeting.started.elapsed().as_secs_f64();
        let mic = meeting.mic.stop();
        let system = meeting.system.map(|s| s.stop());
        let system = match system {
            Some(r) => Some(r.context("Finishing the system audio recording")?),
            None => None,
        };
        Ok((
            meeting.meeting_id,
            duration,
            MeetingRecording {
                mic: mic.context("Finishing the microphone recording")?,
                system,
            },
        ))
    }

    pub fn is_recording(&self) -> bool {
        self.lock().is_some()
    }

    pub fn active_id(&self) -> Option<String> {
        self.lock().as_ref().map(|m| m.meeting_id.clone())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<ActiveMeeting>> {
        // A poisoned lock still holds valid state; keep recording rather than panic.
        self.active.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub fn emit_changed(app: &AppHandle, id: &str) {
    let _ = MeetingChanged { id: id.to_string() }.emit(app);
    crate::tray::refresh_meeting_state(app);
}

/// Starts a meeting. Failures raise a notice and are returned to the caller.
/// Used by the command, the shortcut, the tray and meeting detection.
pub fn start_meeting(
    app: &AppHandle,
    source_app: Option<String>,
) -> std::result::Result<String, String> {
    let meetings = app.state::<Arc<MeetingManager>>();
    let store = app.state::<Arc<MeetingStore>>();
    let now = chrono::Local::now();
    let meeting_id = format!("meeting-{}", now.format("%Y%m%d-%H%M%S-%3f"));
    let dir = store.audio_dir(&meeting_id);
    let warnings = match meetings.start(&meeting_id, &dir) {
        Ok(w) => w,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(raise_failure(app, NoticeCode::MeetingStartFailed, &e));
        }
    };
    let record = MeetingRecord {
        id: meeting_id.clone(),
        started_at: now.to_rfc3339(),
        duration_s: 0.0,
        status: MeetingStatus::Recording,
        tracks: Vec::new(),
        source_app,
        title: None,
        note_path: None,
        speakers: Vec::new(),
        warnings,
        audio_deleted: false,
        online: None,
    };
    if let Err(e) = store.insert(record) {
        log::warn!("Saving the meeting record failed: {e:#}");
    }
    notices::clear(app, NoticeSource::Meeting);
    indicator::show(app);
    indicator::start_ticker(app);
    emit_changed(app, &meeting_id);
    Ok(meeting_id)
}

/// Stops the meeting and starts processing it in the background.
pub fn stop_meeting(app: &AppHandle) -> std::result::Result<String, String> {
    let meetings = app.state::<Arc<MeetingManager>>();
    let store = app.state::<Arc<MeetingStore>>();
    indicator::hide(app);
    let (meeting_id, duration, recording) = match meetings.stop() {
        Ok(r) => r,
        Err(e) => return Err(raise_failure(app, NoticeCode::MeetingStopFailed, &e)),
    };

    let timing = TrackTiming {
        mic_first_us: MIC_FIRST_US.load(std::sync::atomic::Ordering::Relaxed),
        system_first_us: SYSTEM_FIRST_US.load(std::sync::atomic::Ordering::Relaxed),
    };
    if let Ok(text) = serde_json::to_string(&timing) {
        let _ = std::fs::write(store.audio_dir(&meeting_id).join(TIMING_FILE), text);
    }
    let mut tracks = Vec::new();
    if recording.mic.frames > 0 {
        tracks.push(MIC_FILE.to_string());
    }
    if let Some(system) = &recording.system {
        if system.path.is_some() && system.frames > 0 {
            tracks.push(SYSTEM_FILE.to_string());
        } else {
            log::warn!("Meeting {meeting_id}: system audio track skipped (no frames captured)");
        }
    }
    let mut warnings = Vec::new();
    if let Some(err) = &recording.mic.error {
        warnings.push(format!("Microphone problem during the recording: {err}"));
    }
    log::info!(
        "Meeting {meeting_id}: mic {} ({} frames at {} Hz, {} dropped)",
        recording.mic.path.display(),
        recording.mic.frames,
        recording.mic.sample_rate,
        recording.mic.dropped_frames
    );
    if recording.mic.dropped_frames > recording.mic.sample_rate as u64 {
        warnings.push(format!(
            "About {} s of microphone audio were lost (the disk was too slow).",
            recording.mic.dropped_frames / recording.mic.sample_rate.max(1) as u64
        ));
    }
    if let Some(system) = &recording.system {
        let rate = system.sample_rate.unwrap_or(48_000).max(1) as u64;
        if system.dropped_samples > rate {
            warnings.push(format!(
                "About {} s of system audio were lost (the disk was too slow).",
                system.dropped_samples / rate
            ));
        }
    }
    let _ = store.update(&meeting_id, |r| {
        r.duration_s = duration;
        r.tracks = tracks.clone();
        r.warnings.extend(warnings);
        r.status = if tracks.is_empty() {
            MeetingStatus::Failed {
                error: "Nothing was recorded.".into(),
            }
        } else {
            MeetingStatus::Processing {
                stage: "waiting".into(),
                progress: None,
            }
        };
    });
    emit_changed(app, &meeting_id);
    notices::clear(app, NoticeSource::Meeting);
    if !tracks.is_empty() {
        process::spawn(app.clone(), meeting_id.clone());
    }
    Ok(meeting_id)
}

/// At quit: closes a running recording so its FLAC files are complete and
/// marks it for processing at the next launch (`resume_interrupted`).
pub fn finalize_on_exit(app: &AppHandle) {
    let meetings = app.state::<Arc<MeetingManager>>();
    if !meetings.is_recording() {
        return;
    }
    let store = app.state::<Arc<MeetingStore>>();
    match meetings.stop() {
        Ok((id, duration, recording)) => {
            let mut tracks = Vec::new();
            if recording.mic.frames > 0 {
                tracks.push(MIC_FILE.to_string());
            }
            if recording
                .system
                .as_ref()
                .is_some_and(|s| s.path.is_some() && s.frames > 0)
            {
                tracks.push(SYSTEM_FILE.to_string());
            }
            let timing = TrackTiming {
                mic_first_us: MIC_FIRST_US.load(std::sync::atomic::Ordering::Relaxed),
                system_first_us: SYSTEM_FIRST_US.load(std::sync::atomic::Ordering::Relaxed),
            };
            if let Ok(text) = serde_json::to_string(&timing) {
                let _ = std::fs::write(store.audio_dir(&id).join(TIMING_FILE), text);
            }
            let _ = store.update(&id, |r| {
                r.duration_s = duration;
                r.tracks = tracks;
                r.status = MeetingStatus::Failed {
                    error: RESUME_MARKER.to_string(),
                };
            });
            log::info!("Meeting {id} finalized at quit; it will be processed at the next launch");
        }
        Err(e) => log::warn!("Finalizing the meeting at quit failed: {e:#}"),
    }
}

/// Status text of a meeting closed at quit, picked up by `resume_interrupted`.
pub const RESUME_MARKER: &str =
    "Recording ended because the app quit; processing at the next launch.";

/// At launch: processes meetings that were closed at quit.
pub fn resume_interrupted(app: &AppHandle) {
    let store = app.state::<Arc<MeetingStore>>();
    for r in store.list() {
        // Closed at quit, or quit while processing (store marks it interrupted).
        let resumable = matches!(&r.status, MeetingStatus::Failed { error }
            if error == RESUME_MARKER || error.starts_with("Interrupted:"));
        if resumable && !r.tracks.is_empty() && !r.audio_deleted {
            let closed_at_quit =
                matches!(&r.status, MeetingStatus::Failed { error } if error == RESUME_MARKER);
            let _ = store.update(&r.id, |rec| resume_record(rec, closed_at_quit));
            process::spawn(app.clone(), r.id.clone());
        }
    }
}

/// Marks a record for processing again. Only a recording that was really cut
/// off by quitting gets the warning about it.
fn resume_record(rec: &mut MeetingRecord, closed_at_quit: bool) {
    rec.status = MeetingStatus::Processing {
        stage: "waiting".into(),
        progress: None,
    };
    rec.warnings.retain(|w| !w.starts_with("The app quit"));
    if closed_at_quit {
        rec.warnings
            .push("The app quit during this meeting; the recording ends there.".into());
    }
}

#[cfg(test)]
mod resume_tests {
    use super::*;

    fn rec() -> MeetingRecord {
        MeetingRecord {
            id: "m".into(),
            started_at: "t".into(),
            duration_s: 1.0,
            status: MeetingStatus::Failed {
                error: "Interrupted: x".into(),
            },
            tracks: vec![MIC_FILE.into()],
            source_app: None,
            title: None,
            note_path: None,
            speakers: vec![],
            warnings: vec!["The app quit during this meeting; the recording ends there.".into()],
            audio_deleted: false,
            online: None,
        }
    }

    #[test]
    fn quit_warning_only_when_the_recording_was_cut_off() {
        let mut r = rec();
        resume_record(&mut r, false);
        assert!(r.warnings.is_empty());
        let mut r = rec();
        resume_record(&mut r, true);
        assert_eq!(r.warnings.len(), 1);
    }
}

/// Shortcut entry point: starts a meeting if none is running, else stops it.
pub fn toggle_meeting(app: &AppHandle) {
    let active = app.state::<Arc<MeetingManager>>().is_recording();
    let result = if active {
        stop_meeting(app)
    } else {
        start_meeting(app, None)
    };
    if let Err(e) = result {
        log::warn!("Meeting toggle failed: {e}");
    }
}

fn raise_failure(app: &AppHandle, code: NoticeCode, error: &anyhow::Error) -> String {
    let detail = format!("{error:#}");
    notices::raise(
        app,
        Notice::new(code, NoticeSource::Meeting).with_detail(detail.clone()),
    );
    detail
}

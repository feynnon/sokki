//! Meeting notes (macOS): record, then turn the recording into a Markdown note.
//!
//! Recording (from Mandy, MIT): the microphone and, unless in microphone-only
//! mode, the system audio are written to separate FLAC files in the meeting's
//! own folder. The microphone is the user's side (plus a phone on speaker or
//! people in the room); system audio is the remote side of a call on this Mac.
//!
//! Processing (`process`): speakers are separated, every speaker turn is
//! transcribed with Sokki's model, echoes of the system track are removed
//! from the microphone track, a language model writes the summary, and the
//! note is saved to the notes folder. Progress is tracked in `store`.
//!
//! Dictation has its own recorder and state; this module shares none of them.

#[cfg(target_os = "macos")]
pub mod auto;
pub mod calendar;
#[cfg(target_os = "macos")]
pub mod call_speakers;
pub mod chat_notice;
pub mod detect;
pub mod diarize_worker;
pub mod flac;
#[cfg(target_os = "macos")]
pub mod gmail_account;
pub mod indicator;
pub mod keep_awake;
pub mod live;
#[cfg(target_os = "macos")]
pub mod mail;
pub mod memguard;
pub mod mic;
#[cfg(target_os = "macos")]
pub(crate) mod notify_macos;
pub mod phone_link;
pub mod priority;
pub mod process;
pub mod prompt;
pub mod preload;
pub mod pull;
pub mod rate;
pub mod sample;
pub mod silence;
pub mod slack;
pub mod store;
pub mod system_tap;
pub mod title;
pub mod voice_types;
#[cfg(target_os = "macos")]
pub mod voices;

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

/// Microseconds on a monotonic clock (since its first use in this process),
/// for the activity marks below.
pub fn mono_us() -> u64 {
    static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    EPOCH
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_micros() as u64
}

/// When the call track last had a sample that was not exactly zero, and
/// when its last callback came ([`mono_us`]; 0 = not in this meeting). A
/// dead tap keeps calling back with exact zeros (2026-11-24); a quiet call
/// still has noise in it. Written by the tap's callback, read by its
/// watchdog and the indicator's ticker.
pub static SYSTEM_SOUND_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static SYSTEM_CALLBACK_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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

/// Start times of the tracks and the track file names, shared with every host.
pub use sokki_engine::tracks::{TrackTiming, MIC_FILE, SYSTEM_FILE, TIMING_FILE};

/// Files produced by one meeting, finalized.
pub struct MeetingRecording {
    pub mic: MicRecording,
    pub system: Option<SystemRecording>,
    /// What was transcribed while recording.
    pub live: Option<live::Live>,
}

struct ActiveMeeting {
    meeting_id: String,
    started: std::time::Instant,
    // Stopped in `stop`. Dropping them ends the capture as well.
    system: Option<SystemAudioCapture>,
    mic: MicCapture,
    // Keeps the Mac from idle-sleeping until the meeting is stopped; dropped
    // with it, after both files are finalized.
    _awake: keep_awake::KeepAwake,
    /// Transcribes the meeting while it records; handed to processing.
    live: Option<live::Live>,
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
    /// audio unavailable) that do not stop the recording. `app`, when given,
    /// transcribes the meeting while it records ([`live`]).
    pub fn start(
        &self,
        meeting_id: &str,
        dir: &Path,
        app: Option<&AppHandle>,
    ) -> Result<Vec<String>> {
        let begun = std::time::Instant::now();
        let mut active = self.lock();
        if active.is_some() {
            bail!("A meeting is already being recorded.");
        }
        std::fs::create_dir_all(dir).with_context(|| format!("Creating {}", dir.display()))?;
        // Held from before the captures open; released if starting fails.
        let awake = keep_awake::KeepAwake::acquire();

        MIC_FIRST_US.store(0, std::sync::atomic::Ordering::Relaxed);
        SYSTEM_FIRST_US.store(0, std::sync::atomic::Ordering::Relaxed);
        SYSTEM_SOUND_US.store(0, std::sync::atomic::Ordering::Relaxed);
        SYSTEM_CALLBACK_US.store(0, std::sync::atomic::Ordering::Relaxed);
        silence::reset();
        // System audio first: it is the step that can fail on permission.
        // Without it the meeting still records the microphone.
        let mut warnings = Vec::new();
        // Asked before the captures start, so the first samples go in too.
        let with_system = system_tap::is_supported() || sample::active();
        let live = app.and_then(|app| live::Live::start(app, with_system, dir));
        // Each writer feeds the live transcript and the silence hint.
        let tee = |track| {
            let watch = match track {
                meeting_core::TrackKind::Mic => {
                    silence::tee(&silence::MIC_SPEECH_US, Some(&silence::MIC_SOUND_US))
                }
                meeting_core::TrackKind::System => silence::tee(&silence::SYSTEM_SPEECH_US, None),
            };
            Some(silence::both(live.as_ref().map(|l| l.tee(track)), watch))
        };
        let system_begun = std::time::Instant::now();
        let system = match SystemAudioCapture::start(
            dir.join(SYSTEM_FILE),
            tee(meeting_core::TrackKind::System),
        ) {
            Ok(s) => Some(s),
            Err(e) => {
                log::warn!("System audio capture unavailable: {e:#}");
                warnings.push(system_audio_warning());
                None
            }
        };
        let system_ms = system_begun.elapsed().as_millis();
        let mic_begun = std::time::Instant::now();
        let mic = match MicCapture::start(&dir.join(MIC_FILE), tee(meeting_core::TrackKind::Mic)) {
            Ok(mic) => mic,
            Err(e) => {
                if let Some(system) = system {
                    let _ = system.stop();
                }
                return Err(e).context("Starting the microphone");
            }
        };

        let mic_ms = mic_begun.elapsed().as_millis();
        *active = Some(ActiveMeeting {
            meeting_id: meeting_id.to_string(),
            started: std::time::Instant::now(),
            system,
            mic,
            _awake: awake,
            live,
        });
        priority::set_recording(true);
        log::info!(
            "Meeting {meeting_id} recording {} ms after the start (system audio {system_ms} ms, microphone {mic_ms} ms)",
            begun.elapsed().as_millis()
        );
        Ok(warnings)
    }

    /// Stops both halves and finalizes their files. Both are stopped even
    /// when one fails; the first error is returned.
    pub fn stop(&self) -> Result<(String, f64, MeetingRecording)> {
        let Some(meeting) = self.lock().take() else {
            bail!("No meeting is being recorded.");
        };
        priority::set_recording(false);
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
                live: meeting.live,
            },
        ))
    }

    pub fn is_recording(&self) -> bool {
        self.lock().is_some()
    }

    pub fn active_id(&self) -> Option<String> {
        self.lock().as_ref().map(|m| m.meeting_id.clone())
    }

    /// The microphone the running meeting records from right now.
    pub fn mic_name(&self) -> Option<String> {
        self.lock().as_ref().and_then(|m| m.mic.device_name())
    }

    /// Moves the running meeting to another microphone (by name), for the
    /// rest of the recording. The file goes on; the switch takes about a
    /// second (the supervisor's tick).
    pub fn switch_mic(&self, name: &str) -> Result<()> {
        match self.lock().as_ref() {
            Some(m) => {
                m.mic.prefer(name);
                Ok(())
            }
            None => bail!("No meeting is being recorded."),
        }
    }

    /// What the running meeting's live transcript has heard so far.
    pub fn heard(&self) -> Option<sokki_engine::live::Heard> {
        self.lock()
            .as_ref()
            .and_then(|m| m.live.as_ref())
            .map(|l| l.heard())
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
    // A real meeting outranks the sample: the sample goes, at once.
    if let Some(id) = sample::active_id() {
        log::info!("Sample meeting {id} replaced by a real one");
        let _ = discard_meeting(app);
    }
    start_meeting_as(app, source_app, false)
}

/// Starts a meeting; `sample` marks the sample meeting ([`sample`]).
pub(crate) fn start_meeting_as(
    app: &AppHandle,
    source_app: Option<String>,
    sample: bool,
) -> std::result::Result<String, String> {
    let begun = std::time::Instant::now();
    // The dictation cleanup model never runs next to a recording.
    crate::dictation_cleanup::stop_model();
    let meetings = app.state::<Arc<MeetingManager>>();
    let store = app.state::<Arc<MeetingStore>>();
    let now = chrono::Local::now();
    let meeting_id = format!("meeting-{}", now.format("%Y%m%d-%H%M%S-%3f"));
    let dir = store.audio_dir(&meeting_id);
    let live = (!crate::test_mic::no_live()).then_some(app);
    let warnings = match meetings.start(&meeting_id, &dir, live) {
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
        summary_missing: None,
        audio_deleted: false,
        online: None,
        title_by_hand: false,
        title_provisional: false,
        sample,
        mailed: None,
    };
    if let Err(e) = store.insert(record) {
        log::warn!("Saving the meeting record failed: {e:#}");
    }
    if let Some(heard) = meetings.heard() {
        title::spawn_provisional(app.clone(), meeting_id.clone(), heard);
    }
    preload::spawn(app.clone(), meeting_id.clone());
    notices::clear(app, NoticeSource::Meeting);
    indicator::show(app);
    indicator::start_ticker(app);
    emit_changed(app, &meeting_id);
    if sample {
        crate::usage::count(app, crate::usage::Count::SampleMeetingStarted);
    } else {
        crate::usage::count(app, crate::usage::Count::MeetingStarted);
        crate::usage::time(app, crate::usage::Timing::MeetingStart, begun.elapsed());
    }
    log::info!(
        "Meeting {meeting_id} shown as recording {} ms after the start{}",
        begun.elapsed().as_millis(),
        if process::busy() {
            ", while an earlier meeting is processed"
        } else {
            ""
        }
    );
    // Never a chat notice for the sample, even with a real call running.
    if !sample {
        detect::recording_started(app, &meeting_id);
    }
    Ok(meeting_id)
}

/// Stops the recording and deletes it: audio, record and screen video, no
/// note. For when someone in the call objects (wrote "stop" in its chat).
pub fn discard_meeting(app: &AppHandle) -> std::result::Result<String, String> {
    sample::end();
    let meetings = app.state::<Arc<MeetingManager>>();
    let store = app.state::<Arc<MeetingStore>>();
    let stopped = meetings.stop();
    indicator::hide(app);
    let (meeting_id, duration, _recording) = match stopped {
        Ok(r) => r,
        Err(e) => return Err(raise_failure(app, NoticeCode::MeetingStopFailed, &e)),
    };
    // No summary for it: a model loaded for one is stopped.
    if !process::busy() {
        sokki_engine::llm::drop_preloaded();
    }
    #[cfg(target_os = "macos")]
    {
        crate::video::on_meeting_stopped(app, &meeting_id, duration);
        crate::video::delete_for_meeting(&meeting_id);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = duration;
    if let Err(e) = store.remove(&meeting_id) {
        log::warn!("Meeting {meeting_id}: deleting the discarded recording failed: {e:#}");
    }
    emit_changed(app, &meeting_id);
    notices::clear(app, NoticeSource::Meeting);
    Ok(meeting_id)
}

/// Stops the meeting and starts processing it in the background.
pub fn stop_meeting(app: &AppHandle) -> std::result::Result<String, String> {
    sample::end();
    let begun = std::time::Instant::now();
    let meetings = app.state::<Arc<MeetingManager>>();
    let store = app.state::<Arc<MeetingStore>>();
    // The pill shows a spinner until the files are saved, then goes.
    let stopped = meetings.stop();
    indicator::hide(app);
    let (meeting_id, duration, recording) = match stopped {
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
    // A screen video recorded with this meeting: finish it, start its render.
    #[cfg(target_os = "macos")]
    crate::video::on_meeting_stopped(app, &meeting_id, duration);
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
        log::warn!("Meeting {meeting_id}: microphone problem during the recording: {err}");
        warnings.push(MIC_PROBLEM.into());
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
        process::spawn(app.clone(), meeting_id.clone(), recording.live);
    }
    log::info!(
        "Meeting {meeting_id} stopped in {} ms",
        begun.elapsed().as_millis()
    );
    Ok(meeting_id)
}

/// At quit: closes a running recording so its FLAC files are complete and
/// marks it for processing at the next launch (`resume_interrupted`).
pub fn finalize_on_exit(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    crate::video::finalize_on_exit(app);
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

/// The meeting card's line when the microphone failed during a recording.
pub const MIC_PROBLEM: &str =
    "The microphone stopped working for a while, so parts may be missing.";

/// The meeting card's line when system audio could not be recorded.
pub fn system_audio_warning() -> String {
    format!(
        "Only the microphone was recorded. To record calls too, allow System Audio Recording for {} in System Settings → Privacy & Security.",
        crate::brand::app_name()
    )
}

/// Status text of a meeting closed at quit, picked up by `resume_interrupted`.
pub const RESUME_MARKER: &str =
    "Recording ended because the app quit; processing at the next launch.";

/// At launch: processes meetings that were closed at quit.
pub fn resume_interrupted(app: &AppHandle) {
    let store = app.state::<Arc<MeetingStore>>();
    for r in store.list() {
        // Quit while a summary was being added: the note is complete, the
        // summary is tried again by `process::retry_summaries`.
        if r.summary_missing.is_some()
            && r.note_path.is_some()
            && matches!(&r.status, MeetingStatus::Failed { error } if error.starts_with("Interrupted:"))
        {
            let _ = store.update(&r.id, |rec| rec.status = MeetingStatus::Done);
            continue;
        }
        // Closed at quit, or quit while processing (store marks it interrupted).
        let resumable = matches!(&r.status, MeetingStatus::Failed { error }
            if error == RESUME_MARKER || error.starts_with("Interrupted:"));
        if resumable && !r.tracks.is_empty() && !r.audio_deleted {
            let closed_at_quit =
                matches!(&r.status, MeetingStatus::Failed { error } if error == RESUME_MARKER);
            let _ = store.update(&r.id, |rec| resume_record(rec, closed_at_quit));
            process::spawn(app.clone(), r.id.clone(), None);
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
            summary_missing: None,
            audio_deleted: false,
            online: None,
            title_by_hand: false,
            title_provisional: false,
            sample: false,
            mailed: None,
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

/// Shortcut, menu bar and command-line entry point: starts a meeting if none
/// is running, else stops it. `via` names the entry point for the log, so a
/// recording that starts or stops unexpectedly can be traced.
pub fn toggle_meeting(app: &AppHandle, via: &str) {
    let active = app.state::<Arc<MeetingManager>>().is_recording();
    log::info!(
        "Meeting {} via {via}",
        if active { "stop" } else { "start" }
    );
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

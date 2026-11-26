//! Turns a recorded meeting into a note, on a background thread:
//! decode tracks → diarize → transcribe turns → remove echoes → name speakers
//! → summarize (optional) → write the Markdown note → apply audio retention.
//!
//! The steps themselves are `sokki_engine::run`, shared with the Linux
//! service; this file supplies Sokki's transcription engine, the GPU
//! diarizer worker and voice profiles, and reports progress to the meeting
//! store and the menu bar.

use anyhow::{Context, Result};
use meeting_core::{Track, TrackKind, Transcriber, SAMPLE_RATE};
use sokki_engine::tracks::WholeTrack;
use sokki_engine::Stage;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager};

use super::store::{MeetingRecord, MeetingStatus, MeetingStore, SummaryMissing};
use super::{emit_changed, MIC_FILE, SYSTEM_FILE};
use crate::audio_toolkit::decode_audio_file;
use crate::managers::transcription::{ChunkControl, TranscriptionManager};
use crate::settings::MeetingSettings;

/// One meeting processes at a time; others wait their turn.
static PROCESSING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A meeting (or a summary) is being processed right now.
pub fn busy() -> bool {
    matches!(
        PROCESSING.try_lock(),
        Err(std::sync::TryLockError::WouldBlock)
    )
}

/// Starts processing on a background thread. `live`: what was transcribed
/// during the recording, if it just ended. The thread yields to any meeting
/// that records meanwhile ([`super::priority`]).
pub fn spawn(app: AppHandle, id: String, live: Option<super::live::Live>) {
    std::thread::Builder::new()
        .name(format!("meeting-{id}"))
        .spawn(move || {
            super::priority::follow_recording();
            let _turn = PROCESSING.lock().unwrap_or_else(|e| e.into_inner());
            // Nor next to processing (dictation_cleanup::begin checks busy()).
            crate::dictation_cleanup::stop_model();
            let store = app.state::<Arc<MeetingStore>>().inner().clone();
            super::memguard::set_current(Some(&id));
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                process(&app, &store, &id, live)
            }));
            super::memguard::set_current(None);
            // A model loaded during the meeting that no summary took.
            if sokki_engine::llm::drop_preloaded() {
                log::info!("Meeting {id}: stopped the summary model loaded during the meeting");
            }
            let error = match result {
                Ok(Ok(())) => None,
                Ok(Err(e)) => Some((format!("{e:#}"), failure_message(&e))),
                Err(p) => Some((
                    crate::managers::transcription::panic_payload_message(p.as_ref()),
                    PROCESSING_FAILED.to_string(),
                )),
            };
            // While a meeting records, the menu bar shows its time only.
            let recording = super::priority::recording();
            if error.is_none() && !recording {
                super::indicator::flash_menu_bar_status(&app, "🟢");
            }
            if let Some((detail, error)) = error {
                if !recording {
                    super::indicator::flash_menu_bar_status(&app, "⚠️");
                }
                log::error!("Meeting {id} failed: {detail}");
                let _ = store.update(&id, |r| r.status = MeetingStatus::Failed { error });
            }
            emit_changed(&app, &id);
        })
        .expect("spawn meeting thread");
}

/// The meeting card's line for a meeting that could not be processed; the
/// card offers to try again. The error itself is in the log.
const PROCESSING_FAILED: &str = "This meeting couldn't be processed.";

/// What the card says about a processing error: the error itself when it is
/// already a sentence for the user, else [`PROCESSING_FAILED`].
fn failure_message(e: &anyhow::Error) -> String {
    const FOR_THE_USER: [&str; 1] = ["The recording has no audio."];
    match FOR_THE_USER.iter().find(|m| e.to_string() == **m) {
        Some(m) => m.to_string(),
        None => PROCESSING_FAILED.to_string(),
    }
}

fn set_stage(app: &AppHandle, store: &MeetingStore, id: &str, stage: &str, progress: Option<f32>) {
    super::priority::yield_now();
    let pct = progress
        .map(|p| format!(" {}%", (p * 100.0).round() as u32))
        .unwrap_or_default();
    // While a meeting records, the menu bar shows its time (the ticker).
    if !super::priority::recording() {
        super::indicator::set_menu_bar_status(app, Some(&format!("🔵{pct}")));
    }
    let _ = store.update(id, |r| {
        r.status = MeetingStatus::Processing {
            stage: stage.to_string(),
            progress,
        }
    });
    emit_changed(app, id);
}

/// Clears processing's status next to the menu bar icon, unless a meeting
/// records: then the ticker's time is there.
fn clear_menu_bar_status(app: &AppHandle) {
    if !super::priority::recording() {
        super::indicator::set_menu_bar_status(app, None);
    }
}

/// The meeting settings in the engine's terms.
pub fn engine_settings(all: &crate::settings::AppSettings) -> sokki_engine::Settings {
    let m = &all.meetings;
    let mut glossary = m.glossary.clone();
    for w in all.custom_words.iter().chain(&all.vocabulary_learned) {
        if !glossary.contains(w) {
            glossary.push(w.clone());
        }
    }
    sokki_engine::Settings {
        user_name: m.user_name.clone(),
        summary: m.summary_enabled,
        glossary,
        footer: m.footer_enabled,
        supporter_code: m.supporter_code.clone(),
        notes_dir: m.notes_dir_path(),
        llm: llm_settings(m, &all.post_process_api_keys),
        language: None,
    }
}

/// The language-model part of the meeting settings.
pub fn llm_settings(
    m: &MeetingSettings,
    keys: &HashMap<String, String>,
) -> sokki_engine::llm::LlmSettings {
    sokki_engine::llm::LlmSettings {
        provider: m.llm_provider.clone(),
        cloud_model: m.cloud_model.clone(),
        base_url: m.llm_base_url.clone(),
        model: m.llm_model.clone(),
        keys: keys.clone(),
    }
}

fn process(
    app: &AppHandle,
    store: &MeetingStore,
    id: &str,
    live: Option<super::live::Live>,
) -> Result<()> {
    let record = store.get(id).context("meeting record missing")?;
    let all_settings = crate::settings::get_settings(app);
    let settings = all_settings.meetings.clone();
    let engine = engine_settings(&all_settings);
    let dir = store.audio_dir(id);

    // Decode the tracks to 16 kHz mono ("mic.flac" / "system.flac" from a
    // recording here, "mic.phone" from the phone app) while the live
    // session transcribes its last pieces and finds its last speakers.
    let decode = || -> Result<Vec<Track>> {
        let mut tracks = Vec::new();
        for file in &record.tracks {
            let kind = if file.starts_with("mic.") {
                TrackKind::Mic
            } else if file.starts_with("system.") {
                TrackKind::System
            } else {
                continue;
            };
            let path = dir.join(file);
            let samples =
                decode_audio_file(&path).with_context(|| format!("decoding {}", path.display()))?;
            tracks.push(Track { kind, samples });
        }
        Ok(tracks)
    };
    let (decoded, (live, transcriber)) = std::thread::scope(|scope| {
        let finishing = scope.spawn(move || match live {
            Some(l) => l.finish(),
            None => (None, Arc::new(EngineTranscriber::new(app))),
        });
        let decoded = decode();
        let finished = finishing
            .join()
            .unwrap_or_else(|_| (None, Arc::new(EngineTranscriber::new(app))));
        (decoded, finished)
    });
    let tracks = decoded?;
    let timing: Option<super::TrackTiming> = std::fs::read_to_string(dir.join(super::TIMING_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());

    let diarizer_notes = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut warnings = record.warnings.clone();
    let diarizer: Box<dyn meeting_core::Diarizer> =
        match build_diarizer(app, &settings, &dir, diarizer_notes.clone()) {
            Ok(d) => d,
            Err(e) => {
                log::warn!("Speakers not separated: {e:#}");
                meeting_core::pipeline::warn_once(
                    &mut warnings,
                    meeting_core::pipeline::SPEAKERS_NOT_SEPARATED,
                );
                Box::new(WholeTrack)
            }
        };
    let profiles = voice_profiles(app);
    let namer = |_track: TrackKind, _cluster: &str, embedding: Option<&[f32]>| -> Option<String> {
        let e = embedding?;
        profiles.as_ref()?.identify(e).map(|(name, _)| name)
    };
    let progress = |stage: Stage, progress: Option<f32>| {
        set_stage(app, store, id, stage.as_str(), progress);
    };
    // A meeting records: the summary (the language model on the GPU) waits
    // for the background retry, which runs once nothing records.
    let hold_summary = || {
        let hold = super::priority::recording();
        if hold {
            log::info!("Meeting {id}: summary later, a meeting is recording");
        }
        hold
    };
    let out = match sokki_engine::run(
        sokki_engine::Input {
            tracks,
            timing,
            started_at: record.started_at.clone(),
            source_app: record.source_app.clone(),
            previous_note: record.note_path.as_deref().map(PathBuf::from),
            warnings,
            live,
        },
        sokki_engine::Deps {
            transcriber: &*transcriber,
            diarizer,
            diarizer_notes: Some(diarizer_notes),
            namer: Some(&namer),
            progress: &progress,
            hold_summary: Some(&hold_summary),
        },
        &engine,
    ) {
        Ok(out) => out,
        Err(e) if e.downcast_ref::<sokki_engine::NoSpeech>().is_some() => {
            return no_speech(app, store, &record);
        }
        Err(e) => return Err(e),
    };
    save_speakers(&dir, &out.speaker_info);

    // "Your data": only the user's own microphone segments
    // (named after the user, which happens on calls only). Never in test mode.
    let mic = out.tracks.iter().find(|t| t.kind == TrackKind::Mic);
    if let Some(mic) = mic.filter(|_| !crate::test_mic::active() && !record.sample) {
        let own: Vec<crate::voice_data::OwnSegment> = out
            .transcript
            .segments
            .iter()
            .filter(|s| s.track == TrackKind::Mic && s.speaker == settings.user_name)
            .map(|s| crate::voice_data::OwnSegment {
                start_s: s.start_s,
                end_s: s.end_s,
                text: &s.text,
            })
            .collect();
        crate::voice_data::add_meeting(app, &mic.samples, &own);
    }

    // Audio retention.
    let delete_audio = settings.keep_audio_days == 0;
    if delete_audio {
        for f in [MIC_FILE, SYSTEM_FILE] {
            let _ = std::fs::remove_file(dir.join(f));
        }
    }

    store
        .update(id, |r| {
            r.status = MeetingStatus::Done;
            super::title::set_processed(r, &out.title, out.summary_missing.is_some());
            r.note_path = Some(out.note_path.to_string_lossy().into_owned());
            r.speakers = out.speakers.clone();
            r.warnings = out.warnings.clone();
            r.summary_missing = out.summary_missing.map(Into::into);
            r.duration_s = out.duration_s;
            r.online = Some(out.online);
            if delete_audio {
                r.audio_deleted = true;
            }
        })
        .inspect(super::title::write_typed_into_note)?;
    if !record.sample {
        crate::milestones::add_meeting(app, id, out.duration_s);
        usage_done(app, id, &settings, out.summary_missing.is_none());
    }
    // Names from the guest list before the note goes out by mail.
    #[cfg(target_os = "macos")]
    if !record.sample {
        let recognised: Vec<String> = out
            .speaker_info
            .iter()
            .filter(|s| s.named_by_hook)
            .map(|s| s.name.clone())
            .collect();
        super::voices::after_processing(app, id, &recognised);
    }
    emit_changed(app, id);
    notify_done(app, &out.note_path);
    #[cfg(target_os = "macos")]
    super::mail::after_note(app, id);
    super::slack::note_ready(app, id);
    Ok(())
}

/// Sent to the window when a meeting had no speech and was removed; the
/// record bar says so for a moment ("No speech, nothing saved").
pub const NO_SPEECH_EVENT: &str = "meeting-no-speech";

/// Nobody spoke on any track (Robin, 2026-11-25: "prevent them from coming
/// to existence"): no note, the audio goes to the Trash and the meeting
/// leaves the list. A meeting that had a note already (reprocessed) keeps
/// it, and a screen video stays where it is.
fn no_speech(app: &AppHandle, store: &MeetingStore, record: &MeetingRecord) -> Result<()> {
    let id = &record.id;
    if record.note_path.is_some() {
        log::info!("Meeting {id}: no speech on reprocessing; the earlier note stays");
        store.update(id, |r| r.status = MeetingStatus::Done)?;
        emit_changed(app, id);
        return Ok(());
    }
    if let Err(e) = store.trash(id) {
        // The record stays, so the audio is not lost track of.
        log::warn!("Meeting {id}: no speech, but the audio wasn't moved to the Trash: {e:#}");
        store.update(id, |r| {
            r.status = MeetingStatus::Failed {
                error: NO_SPEECH.into(),
            }
        })?;
        emit_changed(app, id);
        return Ok(());
    }
    log::info!(
        "Meeting {id}: no speech in {:.0} s ({}); audio moved to the Trash, nothing saved",
        record.duration_s,
        record.source_app.as_deref().unwrap_or("no app")
    );
    let _ = tauri::Emitter::emit(app, NO_SPEECH_EVENT, id);
    emit_changed(app, id);
    Ok(())
}

/// The card's line for a meeting without speech whose audio could not go to
/// the Trash.
const NO_SPEECH: &str = "No speech found; nothing saved.";

/// Counts a finished meeting, its summary and how long after the call's
/// end the note was ready.
fn usage_done(app: &AppHandle, id: &str, settings: &MeetingSettings, summarized: bool) {
    use crate::usage::{count, time, Count, Timing};
    count(app, Count::MeetingDone);
    if settings.summary_enabled {
        count_summary(app, settings, summarized);
    }
    let store = app.state::<Arc<MeetingStore>>();
    let ended = store.get(id).and_then(|r| {
        let start = chrono::DateTime::parse_from_rfc3339(&r.started_at).ok()?;
        Some(start.timestamp_millis() + (r.duration_s * 1000.0) as i64)
    });
    if let Some(ended) = ended {
        let ms = (chrono::Utc::now().timestamp_millis() - ended).max(0) as u64;
        time(
            app,
            Timing::MeetingReady,
            std::time::Duration::from_millis(ms),
        );
    }
}

fn count_summary(app: &AppHandle, settings: &MeetingSettings, ok: bool) {
    use crate::usage::Count;
    let cloud = CLOUD_PROVIDERS.contains(&settings.llm_provider.as_str());
    crate::usage::count(
        app,
        match (ok, cloud) {
            (false, _) => Count::SummaryFailed,
            (true, true) => Count::SummaryCloud,
            (true, false) => Count::SummaryOnMac,
        },
    );
}

// ------------------------------------------------- summary added later

/// Adds the summary to a done meeting whose note has none, on a background
/// thread, after any meeting being processed. `by_hand`: the person pressed
/// "Try again", so Ollama is started if it isn't running; the background
/// retry only comes when the model is ready already.
pub fn spawn_summarize(app: AppHandle, id: String, by_hand: bool) {
    std::thread::Builder::new()
        .name(format!("summary-{id}"))
        .spawn(move || {
            super::priority::follow_recording();
            let _turn = PROCESSING.lock().unwrap_or_else(|e| e.into_inner());
            let store = app.state::<Arc<MeetingStore>>().inner().clone();
            summarize_safely(&app, &store, &id, by_hand);
        })
        .expect("spawn summary thread");
}

/// [`summarize`], and whatever happens in it (a panic included), the
/// meeting ends up done: with its summary, or with the summary missing.
/// Never left "processing", since the note is complete either way.
fn summarize_safely(app: &AppHandle, store: &MeetingStore, id: &str, by_hand: bool) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        summarize(app, store, id, by_hand)
    }));
    if let Err(p) = result {
        log::error!(
            "Meeting {id}: adding the summary panicked: {}",
            crate::managers::transcription::panic_payload_message(p.as_ref())
        );
        clear_menu_bar_status(app);
    }
    let _ = store.update(id, end_summary_attempt);
    emit_changed(app, id);
}

/// A summary attempt is over: a meeting still marked as summarizing is done,
/// with the summary missing.
fn end_summary_attempt(r: &mut super::store::MeetingRecord) {
    if matches!(&r.status, MeetingStatus::Processing { stage, .. } if stage == "summarizing")
        && r.note_path.is_some()
    {
        r.status = MeetingStatus::Done;
        if r.summary_missing.is_none() {
            set_summary_missing(r, SummaryMissing::Failed);
        }
    }
}

fn summarize(app: &AppHandle, store: &MeetingStore, id: &str, by_hand: bool) {
    let Some(record) = store.get(id) else { return };
    let Some(note_path) = record.note_path.clone().map(PathBuf::from) else {
        return;
    };
    if record.status != MeetingStatus::Done || record.summary_missing.is_none() {
        return;
    }
    let all_settings = crate::settings::get_settings(app);
    let engine = engine_settings(&all_settings);
    if by_hand {
        let _ = sokki_engine::llm::prepare(&engine.llm, &engine.user_name).join();
    }
    if !sokki_engine::llm::is_ready(&engine.llm, &engine.user_name) {
        // Still waiting; the card keeps saying so.
        let _ = store.update(id, |r| set_summary_missing(r, SummaryMissing::Waiting));
        return;
    }
    set_stage(app, store, id, "summarizing", None);
    let result = sokki_engine::summarize_note(&note_path, &engine);
    count_summary(app, &all_settings.meetings, result.is_ok());
    clear_menu_bar_status(app);
    let updated = store.update(id, |r| {
        r.status = MeetingStatus::Done;
        match &result {
            Ok(done) => {
                r.note_path = Some(done.note_path.to_string_lossy().into_owned());
                super::title::set_summarized(r, done.title.as_deref());
                r.summary_missing = None;
                r.warnings.retain(|w| {
                    w != sokki_engine::SUMMARY_WAITING && w != sokki_engine::SUMMARY_FAILED
                });
            }
            Err(_) => set_summary_missing(r, SummaryMissing::Failed),
        }
    });
    if let Ok(r) = updated {
        super::title::write_typed_into_note(&r);
    }
    match result {
        Ok(done) => {
            log::info!(
                "Meeting {id}: summary added to {}",
                done.note_path.display()
            );
            #[cfg(target_os = "macos")]
            super::mail::after_note(app, id);
            super::slack::note_ready(app, id);
        }
        Err(e) => log::warn!("Meeting {id}: adding the summary failed: {e:#}"),
    }
}

/// Sets why the summary is missing, with the card's one line for it.
fn set_summary_missing(r: &mut super::store::MeetingRecord, m: SummaryMissing) {
    r.summary_missing = Some(m);
    r.warnings
        .retain(|w| w != sokki_engine::SUMMARY_WAITING && w != sokki_engine::SUMMARY_FAILED);
    r.warnings.push(
        match m {
            SummaryMissing::Waiting => sokki_engine::SUMMARY_WAITING,
            SummaryMissing::Failed => sokki_engine::SUMMARY_FAILED,
        }
        .to_string(),
    );
}

/// How often the background retry looks for summaries to add.
const SUMMARY_RETRY_EVERY: Duration = Duration::from_secs(60);

/// When a failed summary is tried again by itself, counted from when it
/// failed (or from the app's start): three tries, then only by hand.
const FAILED_RETRY_AFTER: [Duration; 3] = [
    Duration::from_secs(10 * 60),
    Duration::from_secs(30 * 60),
    Duration::from_secs(2 * 3600),
];

/// Automatic tries of failed summaries: per meeting, how many so far and
/// since when it waits for the next.
#[derive(Default)]
struct FailedTries(HashMap<String, (usize, std::time::Instant)>);

impl FailedTries {
    /// Whether the background retry should try `id`'s summary now.
    fn due(&mut self, id: &str, missing: SummaryMissing, now: std::time::Instant) -> bool {
        match missing {
            SummaryMissing::Waiting => true,
            SummaryMissing::Failed => {
                let (tries, since) = *self.0.entry(id.to_string()).or_insert((0, now));
                FAILED_RETRY_AFTER
                    .get(tries)
                    .is_some_and(|wait| now.duration_since(since) >= *wait)
            }
        }
    }

    /// A background try of `id` is over; `failed`: the summary is still
    /// missing because the model failed (not because it wasn't ready).
    fn tried(&mut self, id: &str, failed: bool, now: std::time::Instant) {
        if failed {
            let e = self.0.entry(id.to_string()).or_insert((0, now));
            *e = (e.0 + 1, now);
        } else {
            self.0.remove(id);
        }
    }
}

/// Background retry: once a minute, adds summaries that wait for the
/// language model (as soon as it is ready) or that failed (after
/// [`FAILED_RETRY_AFTER`]). Only while the Mac has memory to spare, nothing
/// is recorded and no meeting is processed, so a summary never competes
/// with a meeting. It never starts Ollama itself (that happens when a
/// meeting needs it, or by hand).
pub fn retry_summaries(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("summary-retry".into())
        .spawn(move || {
            let mut tries = FailedTries::default();
            loop {
                std::thread::sleep(SUMMARY_RETRY_EVERY);
                retry_summaries_once(&app, &mut tries);
            }
        });
}

fn retry_summaries_once(app: &AppHandle, tries: &mut FailedTries) {
    let store = app.state::<Arc<MeetingStore>>().inner().clone();
    let records = store.list();
    if records.iter().any(|r| r.status == MeetingStatus::Recording) {
        return;
    }
    let now = std::time::Instant::now();
    let due: Vec<String> = records
        .into_iter()
        .filter(|r| r.status == MeetingStatus::Done && r.note_path.is_some())
        .filter_map(|r| {
            let missing = r.summary_missing?;
            tries.due(&r.id, missing, now).then_some(r.id)
        })
        .collect();
    if due.is_empty() {
        return;
    }
    let engine = engine_settings(&crate::settings::get_settings(app));
    if !engine.summary || !super::memguard::memory_to_spare() {
        return;
    }
    if sokki_engine::llm::check(&engine.llm, &engine.user_name, false).state != "ready" {
        return;
    }
    for id in due {
        // A meeting being processed (or a summary by hand) goes first.
        let _turn = match PROCESSING.try_lock() {
            Ok(turn) => turn,
            Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return,
        };
        if !super::memguard::memory_to_spare() {
            return;
        }
        log::info!("Meeting {id}: adding the missing summary in the background");
        summarize_safely(app, &store, &id, false);
        let failed = store
            .get(&id)
            .is_some_and(|r| r.summary_missing == Some(SummaryMissing::Failed));
        tries.tried(&id, failed, std::time::Instant::now());
    }
}

/// Deletes audio older than the retention period (called at startup).
pub fn apply_retention(app: &AppHandle) {
    let settings = crate::settings::get_settings(app).meetings;
    let store = app.state::<Arc<MeetingStore>>();
    if settings.keep_audio_days == 0 {
        return;
    }
    let cutoff = chrono::Local::now() - chrono::Duration::days(settings.keep_audio_days as i64);
    for r in store.list() {
        let old = chrono::DateTime::parse_from_rfc3339(&r.started_at)
            .map(|t| t < cutoff)
            .unwrap_or(false);
        if old && !r.audio_deleted && r.status == MeetingStatus::Done {
            let dir = store.audio_dir(&r.id);
            for f in [MIC_FILE, SYSTEM_FILE] {
                let _ = std::fs::remove_file(dir.join(f));
            }
            let _ = store.update(&r.id, |r| r.audio_deleted = true);
        }
    }
}

fn notify_done(_app: &AppHandle, note_path: &std::path::Path) {
    if crate::test_mic::active() {
        return; // nothing pops up on the developer's screen
    }
    let title = note_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    super::detect::notify("Meeting-Notiz fertig", &title);
}

// ------------------------------------------------------------- transcriber

/// Sokki's engine, chunked, yielding to dictation between chunks.
pub(super) struct EngineTranscriber {
    app: AppHandle,
    tm: Arc<TranscriptionManager>,
    /// Clean-up for the window last transcribed with word times; the
    /// pipeline polishes a window's lines right after transcribing it.
    polisher: std::sync::Mutex<Option<crate::managers::transcription::Polisher>>,
}

impl EngineTranscriber {
    pub(super) fn new(app: &AppHandle) -> Self {
        Self {
            app: app.clone(),
            tm: app.state::<Arc<TranscriptionManager>>().inner().clone(),
            polisher: Default::default(),
        }
    }
}

struct YieldToDictation<'a> {
    app: &'a AppHandle,
}

impl ChunkControl for YieldToDictation<'_> {
    fn proceed(&self) -> bool {
        super::priority::yield_now();
        while crate::tray::dictation_in_progress(self.app) {
            std::thread::sleep(Duration::from_millis(250));
        }
        true
    }
    fn progress(&self, _done: usize, _total: usize) {}
}

impl Transcriber for EngineTranscriber {
    fn transcribe(&self, samples: &[f32]) -> Result<String> {
        if samples.len() < SAMPLE_RATE as usize / 10 {
            return Ok(String::new());
        }
        let control = YieldToDictation { app: &self.app };
        let out = self.tm.transcribe_in_chunks(samples.to_vec(), &control)?;
        Ok(out.map(|t| t.text.trim().to_string()).unwrap_or_default())
    }

    fn transcribe_words(&self, samples: &[f32]) -> Result<Option<Vec<meeting_core::TimedWord>>> {
        if samples.len() < SAMPLE_RATE as usize / 10 {
            return Ok(Some(Vec::new()));
        }
        let control = YieldToDictation { app: &self.app };
        let Some((t, polisher)) = self.tm.transcribe_words(samples.to_vec(), &control)? else {
            return Ok(Some(Vec::new()));
        };
        if t.words.is_empty() && !t.text.trim().is_empty() {
            return Ok(None); // this model gives no word times
        }
        *self.polisher.lock().unwrap_or_else(|e| e.into_inner()) = Some(polisher);
        Ok(Some(
            t.words
                .into_iter()
                .map(|w| meeting_core::TimedWord {
                    start_s: w.t0_ms as f64 / 1000.0,
                    end_s: w.t1_ms as f64 / 1000.0,
                    text: w.text,
                })
                .collect(),
        ))
    }

    fn polish(&self, text: &str) -> String {
        match &*self.polisher.lock().unwrap_or_else(|e| e.into_inner()) {
            Some(p) => p.apply(text.to_string()).trim().to_string(),
            None => text.to_string(),
        }
    }
}

// --------------------------------------------- diarizer, profiles, summary

/// Tells the engine where the built-in language model's server (bundled
/// with the app) and its model file (downloaded next to the speaker models)
/// are.
pub fn set_builtin_llm_paths(app: &AppHandle) {
    let server = app
        .path()
        .resource_dir()
        .ok()
        .map(|d| d.join("resources").join("llm").join("llama-server"));
    // Any summary or title with a local model first frees the cleanup
    // model's memory.
    sokki_engine::llm::set_before_local_job(crate::dictation_cleanup::stop_model);
    sokki_engine::llm::set_builtin_paths(sokki_engine::llm::BuiltinPaths {
        server,
        models_dir: Some(models_dir(app)),
    });
}

/// Downloads the built-in model (Gemma 4 E4B: summaries and dictation
/// cleanup use the same files), one download at a time (`pull::LLM`).
/// Progress goes to both screens that offer it ("meeting-llm-pull" and
/// "dictation-cleanup-pull"). Afterwards the model it replaced is deleted.
pub fn download_builtin(app: &AppHandle) -> std::result::Result<(), String> {
    use tauri::Emitter;
    let result = super::pull::LLM.run(|note| {
        sokki_engine::llm::download_model(&sokki_engine::llm::BUILTIN_MODEL, |p| {
            note(p.clone());
            let _ = app.emit("meeting-llm-pull", p.clone());
            let _ = app.emit("dictation-cleanup-pull", p);
        })
        .map_err(|e| format!("{e:#}"))
    });
    sokki_engine::llm::remove_replaced_models();
    result
}

/// Gemma 4 E4B replaced Qwen 3.5 9B as the built-in model (25.11.2026).
/// Whoever had the 9B gets Gemma downloaded in the background, and the 9B's
/// 5.7 GB are freed once Gemma is complete. Until then a summary waits and
/// is retried (`retry_summaries`).
pub fn move_to_gemma(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("move-to-gemma".into())
        .spawn(move || {
            if sokki_engine::llm::replaced_model_waiting() {
                log::info!("The built-in model is now Gemma 4 E4B; downloading it");
                match download_builtin(&app) {
                    Ok(()) => log::info!("Gemma 4 E4B downloaded; the 9B is deleted"),
                    Err(e) => log::warn!("Downloading Gemma 4 E4B failed: {e}"),
                }
            } else {
                sokki_engine::llm::remove_replaced_models();
            }
        });
}

pub(crate) fn models_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("meeting-models")
}

pub(super) fn build_diarizer(
    app: &AppHandle,
    settings: &MeetingSettings,
    dir: &std::path::Path,
    notes: Arc<std::sync::Mutex<Vec<String>>>,
) -> Result<Box<dyn meeting_core::Diarizer>> {
    if !settings.diarization_enabled {
        return Ok(Box::new(WholeTrack));
    }
    // Sortformer finds who spoke when; sherpa-onnx adds a voice embedding per
    // speaker for recognising known voices. Models download on first use.
    Ok(Box::new(ChildDiarizer {
        models: models_dir(app),
        scratch: dir.to_path_buf(),
        cpu: std::sync::Mutex::new(None),
        notes,
    }))
}

/// Speaker detection on the GPU in a child process (`diarize_worker`), with
/// the in-process CPU diarizer as fallback.
struct ChildDiarizer {
    models: PathBuf,
    scratch: PathBuf,
    cpu: std::sync::Mutex<Option<diarize::SortformerDiarizer>>,
    /// Problems to report in the note (fallbacks).
    notes: Arc<std::sync::Mutex<Vec<String>>>,
}

impl meeting_core::Diarizer for ChildDiarizer {
    fn diarize(&self, samples: &[f32]) -> Result<Vec<meeting_core::SpeakerTurn>> {
        // A GPU failure is not the user's problem: the CPU does the same
        // work, only slower. So it goes to the log, not into the note.
        match super::diarize_worker::diarize_in_child(samples, &self.models, &self.scratch) {
            Ok(out) => {
                self.note_embedder_error(out.embedder_error.as_deref());
                return Ok(out.turns);
            }
            Err(e) => log::warn!("GPU speaker detection failed, using the CPU: {e:#}"),
        }
        let mut cpu = self.cpu.lock().unwrap_or_else(|p| p.into_inner());
        if cpu.is_none() {
            // transcribe.cpp in this process must stay CPU-only: its backends
            // are registered once per process, and dictation's Silero VAD
            // registers them CPU-only ("init_backends_with: status 8").
            crate::audio_toolkit::cpu_engine::init_cpu_backends()?;
            *cpu = Some(diarize::SortformerDiarizer::new(
                diarize::DiarizeConfig::new(&self.models),
            )?);
        }
        let cpu = cpu.as_ref().expect("set above");
        self.note_embedder_error(cpu.embedder_error());
        meeting_core::Diarizer::diarize(cpu, samples)
    }
}

impl ChildDiarizer {
    /// Speakers were told apart, but without the voice-embedding model known
    /// voices can't be recognised. One line in the card for the meeting.
    fn note_embedder_error(&self, error: Option<&str>) {
        let Some(error) = error else { return };
        log::warn!("Known voices not recognised: {error}");
        let mut notes = self.notes.lock().unwrap_or_else(|p| p.into_inner());
        if !notes.iter().any(|n| n == VOICES_NOT_RECOGNISED) {
            notes.push(VOICES_NOT_RECOGNISED.to_string());
        }
    }
}

/// Shown on the meeting card when the voice-embedding model failed.
const VOICES_NOT_RECOGNISED: &str =
    "Known voices weren't recognized this time, so speakers have generic names.";

fn profiles_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("voice-profiles.json")
}

/// Known voices; `None` when the file can't be read.
pub fn voice_profiles(app: &AppHandle) -> Option<diarize::VoiceProfiles> {
    match diarize::VoiceProfiles::open(profiles_path(app)) {
        Ok(p) => Some(p),
        Err(e) => {
            log::warn!("Voice profiles unavailable: {e:#}");
            None
        }
    }
}

pub use sokki_engine::llm::{default_cloud_model, PullProgress, CLOUD_PROVIDERS};
pub use sokki_engine::speakers::{load as load_speakers, SavedSpeaker, SPEAKERS_FILE};

fn save_speakers(dir: &std::path::Path, speakers: &[meeting_core::pipeline::SpeakerInfo]) {
    sokki_engine::speakers::save(dir, speakers)
}

/// What the settings page shows about the language model.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct LlmStatus {
    /// "off", "ready", "no_key", "no_ollama", "not_running",
    /// "model_missing" or "error".
    pub state: String,
    /// Model name, or the error text for "error".
    pub detail: String,
}

/// The language model's state. `probe` sends a tiny real request to a
/// cloud provider (checks key, model name and network); without it only
/// the presence of a key is checked.
pub fn llm_status(
    settings: &MeetingSettings,
    keys: &HashMap<String, String>,
    probe: bool,
) -> LlmStatus {
    if !settings.summary_enabled {
        return LlmStatus {
            state: "off".into(),
            detail: String::new(),
        };
    }
    let s = sokki_engine::llm::check(&llm_settings(settings, keys), &settings.user_name, probe);
    LlmStatus {
        state: s.state,
        detail: s.detail,
    }
}

/// Starts Ollama (the app, or `ollama serve` for a Homebrew install) and
/// waits up to 15 s for it to answer.
pub fn start_ollama(settings: &MeetingSettings) -> Result<()> {
    sokki_engine::llm::start_ollama(&llm_settings(settings, &HashMap::new()))
}

/// Downloads the local model (the built-in one, or through Ollama),
/// reporting progress.
pub fn pull_model(settings: &MeetingSettings, progress: impl FnMut(PullProgress)) -> Result<()> {
    sokki_engine::llm::pull_model(&llm_settings(settings, &HashMap::new()), progress)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(status: MeetingStatus) -> super::super::store::MeetingRecord {
        super::super::store::MeetingRecord {
            id: "m".into(),
            started_at: "2026-11-20T14:20:00+01:00".into(),
            duration_s: 2450.0,
            status,
            tracks: vec!["mic.flac".into()],
            source_app: None,
            title: None,
            note_path: Some("/notes/2026-11-20 1420 Meeting.md".into()),
            speakers: Vec::new(),
            warnings: Vec::new(),
            summary_missing: None,
            audio_deleted: false,
            online: None,
            title_by_hand: false,
            title_provisional: false,
            sample: false,
            mailed: None,
        }
    }

    /// 20.11.2026: the summary of a 41-minute meeting failed; "Try again"
    /// showed the meeting as summarizing for minutes. However an attempt
    /// ends, the meeting is done afterwards.
    #[test]
    fn an_ended_summary_attempt_leaves_the_meeting_done() {
        let mut r = record(MeetingStatus::Processing {
            stage: "summarizing".into(),
            progress: None,
        });
        end_summary_attempt(&mut r);
        assert_eq!(r.status, MeetingStatus::Done);
        assert_eq!(r.summary_missing, Some(SummaryMissing::Failed));
        assert_eq!(r.warnings, [sokki_engine::SUMMARY_FAILED]);

        // A meeting that is processed from its audio again is left alone.
        let mut r = record(MeetingStatus::Processing {
            stage: "transcribing".into(),
            progress: Some(0.5),
        });
        end_summary_attempt(&mut r);
        assert!(matches!(r.status, MeetingStatus::Processing { .. }));

        // A summary that was added stays added.
        let mut r = record(MeetingStatus::Done);
        end_summary_attempt(&mut r);
        assert_eq!(r.summary_missing, None);
    }

    #[test]
    fn a_failed_summary_is_tried_again_three_times_then_only_by_hand() {
        let mut t = FailedTries::default();
        let t0 = std::time::Instant::now();
        let min = |m: u64| t0 + Duration::from_secs(m * 60);
        // Waiting for the model: whenever it's ready.
        assert!(t.due("w", SummaryMissing::Waiting, t0));
        // Failed: after 10 min, then 30 min after that try, then 2 h.
        assert!(!t.due("f", SummaryMissing::Failed, t0));
        assert!(!t.due("f", SummaryMissing::Failed, min(9)));
        assert!(t.due("f", SummaryMissing::Failed, min(10)));
        t.tried("f", true, min(10));
        assert!(!t.due("f", SummaryMissing::Failed, min(39)));
        assert!(t.due("f", SummaryMissing::Failed, min(40)));
        t.tried("f", true, min(40));
        assert!(t.due("f", SummaryMissing::Failed, min(160)));
        t.tried("f", true, min(160));
        assert!(!t.due("f", SummaryMissing::Failed, min(100_000)));
        // Model not ready at a try: no try used up, counting starts again.
        let mut t = FailedTries::default();
        assert!(!t.due("g", SummaryMissing::Failed, t0));
        t.tried("g", false, min(10));
        assert!(!t.due("g", SummaryMissing::Failed, min(15)));
        assert!(t.due("g", SummaryMissing::Failed, min(25)));
    }

    #[test]
    fn engine_settings_follow_the_meeting_settings() {
        let mut s = MeetingSettings::default();
        s.llm_provider = "anthropic".into();
        s.user_name = "Robin".into();
        let keys: HashMap<String, String> = [("anthropic".to_string(), "k".to_string())].into();
        let l = llm_settings(&s, &keys);
        assert_eq!(l.provider, "anthropic");
        assert_eq!(l.keys.get("anthropic").map(String::as_str), Some("k"));
        let cfg = sokki_engine::llm::config(&l, &s.user_name);
        assert_eq!(cfg.model, "claude-opus-5-5");
    }
}

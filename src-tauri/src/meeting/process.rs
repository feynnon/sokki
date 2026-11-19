//! Turns a recorded meeting into a note, on a background thread:
//! decode tracks → diarize → transcribe turns → remove echoes → name speakers
//! → summarize (optional) → write the Markdown note → apply audio retention.
//!
//! The steps themselves are `sokki_engine::run`, shared with the Linux
//! service; this file supplies Handy's transcription engine, the GPU
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

use super::store::{MeetingStatus, MeetingStore, SummaryMissing};
use super::{emit_changed, MIC_FILE, SYSTEM_FILE};
use crate::audio_toolkit::decode_audio_file;
use crate::managers::transcription::{ChunkControl, TranscriptionManager};
use crate::settings::MeetingSettings;

/// One meeting processes at a time; others wait their turn.
static PROCESSING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Starts processing on a background thread.
pub fn spawn(app: AppHandle, id: String) {
    std::thread::Builder::new()
        .name(format!("meeting-{id}"))
        .spawn(move || {
            let _turn = PROCESSING.lock().unwrap_or_else(|e| e.into_inner());
            let store = app.state::<Arc<MeetingStore>>().inner().clone();
            super::memguard::set_current(Some(&id));
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                process(&app, &store, &id)
            }));
            super::memguard::set_current(None);
            let error = match result {
                Ok(Ok(())) => None,
                Ok(Err(e)) => Some((format!("{e:#}"), failure_message(&e))),
                Err(p) => Some((
                    crate::managers::transcription::panic_payload_message(p.as_ref()),
                    PROCESSING_FAILED.to_string(),
                )),
            };
            if error.is_none() {
                super::indicator::flash_menu_bar_status(&app, "🟢");
            }
            if let Some((detail, error)) = error {
                super::indicator::flash_menu_bar_status(&app, "⚠️");
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
    let pct = progress
        .map(|p| format!(" {}%", (p * 100.0).round() as u32))
        .unwrap_or_default();
    super::indicator::set_menu_bar_status(app, Some(&format!("🔵{pct}")));
    let _ = store.update(id, |r| {
        r.status = MeetingStatus::Processing {
            stage: stage.to_string(),
            progress,
        }
    });
    emit_changed(app, id);
}

/// The meeting settings in the engine's terms.
pub fn engine_settings(
    all: &crate::settings::AppSettings,
) -> sokki_engine::Settings {
    let m = &all.meetings;
    let mut glossary = m.glossary.clone();
    for w in &all.custom_words {
        if !glossary.contains(w) {
            glossary.push(w.clone());
        }
    }
    sokki_engine::Settings {
        user_name: m.user_name.clone(),
        correction: m.correction_enabled,
        summary: m.summary_enabled,
        glossary,
        footer: m.footer_enabled,
        notes_dir: m.notes_dir_path(),
        llm: llm_settings(m, &all.post_process_api_keys),
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

fn process(app: &AppHandle, store: &MeetingStore, id: &str) -> Result<()> {
    let record = store.get(id).context("meeting record missing")?;
    let all_settings = crate::settings::get_settings(app);
    let settings = all_settings.meetings.clone();
    let engine = engine_settings(&all_settings);
    let dir = store.audio_dir(id);

    // Decode the tracks to 16 kHz mono: "mic.flac" / "system.flac" from a
    // recording here, "mic.phone" from the phone app.
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
    let tm = app.state::<Arc<TranscriptionManager>>().inner().clone();
    let transcriber = EngineTranscriber {
        app: app.clone(),
        tm,
        polisher: Default::default(),
    };
    let profiles = voice_profiles(app);
    let namer = |_track: TrackKind, _cluster: &str, embedding: Option<&[f32]>| -> Option<String> {
        let e = embedding?;
        profiles.as_ref()?.identify(e).map(|(name, _)| name)
    };
    let progress = |stage: Stage, progress: Option<f32>| {
        set_stage(app, store, id, stage.as_str(), progress);
    };
    let out = sokki_engine::run(
        sokki_engine::Input {
            tracks,
            timing,
            started_at: record.started_at.clone(),
            source_app: record.source_app.clone(),
            previous_note: record.note_path.as_deref().map(PathBuf::from),
            warnings,
        },
        sokki_engine::Deps {
            transcriber: &transcriber,
            diarizer,
            diarizer_notes: Some(diarizer_notes),
            namer: Some(&namer),
            progress: &progress,
        },
        &engine,
    )?;
    save_speakers(&dir, &out.speaker_info);

    // "Your data": only the user's own microphone segments
    // (named after the user, which happens on calls only).
    if let Some(mic) = out.tracks.iter().find(|t| t.kind == TrackKind::Mic) {
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

    store.update(id, |r| {
        r.status = MeetingStatus::Done;
        r.title = Some(out.title.clone());
        r.note_path = Some(out.note_path.to_string_lossy().into_owned());
        r.speakers = out.speakers.clone();
        r.warnings = out.warnings.clone();
        r.summary_missing = out.summary_missing.map(Into::into);
        r.duration_s = out.duration_s;
        r.online = Some(out.online);
        if delete_audio {
            r.audio_deleted = true;
        }
    })?;
    crate::milestones::add_meeting(app, id, out.duration_s);
    emit_changed(app, id);
    notify_done(app, &out.note_path);
    Ok(())
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
            let _turn = PROCESSING.lock().unwrap_or_else(|e| e.into_inner());
            let store = app.state::<Arc<MeetingStore>>().inner().clone();
            summarize(&app, &store, &id, by_hand);
            emit_changed(&app, &id);
        })
        .expect("spawn summary thread");
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
    super::indicator::set_menu_bar_status(app, None);
    let _ = store.update(id, |r| {
        r.status = MeetingStatus::Done;
        match &result {
            Ok(done) => {
                r.note_path = Some(done.note_path.to_string_lossy().into_owned());
                if let Some(t) = &done.title {
                    r.title = Some(t.clone());
                }
                r.summary_missing = None;
                r.warnings.retain(|w| {
                    w != sokki_engine::SUMMARY_WAITING && w != sokki_engine::SUMMARY_FAILED
                });
            }
            Err(_) => set_summary_missing(r, SummaryMissing::Failed),
        }
    });
    match result {
        Ok(done) => log::info!(
            "Meeting {id}: summary added to {}",
            done.note_path.display()
        ),
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

/// Background retry: once a minute, while some note waits for its summary
/// and the language model is ready, adds the summaries. It never starts
/// Ollama itself (that happens when a meeting needs it, or by hand).
pub fn retry_summaries(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("summary-retry".into())
        .spawn(move || loop {
            std::thread::sleep(SUMMARY_RETRY_EVERY);
            let store = app.state::<Arc<MeetingStore>>().inner().clone();
            let waiting: Vec<String> = store
                .list()
                .into_iter()
                .filter(|r| {
                    r.status == MeetingStatus::Done
                        && r.summary_missing == Some(SummaryMissing::Waiting)
                })
                .map(|r| r.id)
                .collect();
            if waiting.is_empty() {
                continue;
            }
            let engine = engine_settings(&crate::settings::get_settings(&app));
            if !engine.summary
                || sokki_engine::llm::check(&engine.llm, &engine.user_name, false).state != "ready"
            {
                continue;
            }
            for id in waiting {
                let _turn = PROCESSING.lock().unwrap_or_else(|e| e.into_inner());
                summarize(&app, &store, &id, false);
                emit_changed(&app, &id);
            }
        });
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
    let title = note_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    super::detect::notify("Meeting-Notiz fertig", &title);
}

// ------------------------------------------------------------- transcriber

/// Handy's engine, chunked, yielding to dictation between chunks.
struct EngineTranscriber {
    app: AppHandle,
    tm: Arc<TranscriptionManager>,
    /// Clean-up for the window last transcribed with word times; the
    /// pipeline polishes a window's lines right after transcribing it.
    polisher: std::sync::Mutex<Option<crate::managers::transcription::Polisher>>,
}

struct YieldToDictation<'a> {
    app: &'a AppHandle,
}

impl ChunkControl for YieldToDictation<'_> {
    fn proceed(&self) -> bool {
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

fn models_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("meeting-models")
}

fn build_diarizer(
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
    if !settings.summary_enabled && !settings.correction_enabled {
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

/// Downloads the local model through Ollama, reporting progress.
pub fn pull_model(settings: &MeetingSettings, progress: impl FnMut(PullProgress)) -> Result<()> {
    sokki_engine::llm::pull_model(&llm_settings(settings, &HashMap::new()), progress)
}

#[cfg(test)]
mod tests {
    use super::*;

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

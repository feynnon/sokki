//! Turns a recorded meeting into a note, on a background thread:
//! decode tracks → diarize → transcribe turns → remove echoes → name speakers
//! → summarize (optional) → write the Markdown note → apply audio retention.
//!
//! The platform-independent steps live in the `meeting-core` crate; this file
//! supplies Handy's transcription engine, the diarizer, voice profiles and the
//! summary model, and reports progress to the meeting store.

use anyhow::{bail, Context, Result};
use meeting_core::pipeline::{self, Hooks, PipelineConfig};
use meeting_core::summary::{LlmConfig, LlmSummarizer};
use meeting_core::{MeetingMeta, MeetingNote, Track, TrackKind, Transcriber, SAMPLE_RATE};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager};

use super::store::{MeetingStatus, MeetingStore};
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
                Ok(Err(e)) => Some(format!("{e:#}")),
                Err(p) => Some(crate::managers::transcription::panic_payload_message(
                    p.as_ref(),
                )),
            };
            if error.is_none() {
                super::indicator::flash_menu_bar_status(&app, "🟢");
            }
            if let Some(error) = error {
                super::indicator::flash_menu_bar_status(&app, "⚠️");
                log::error!("Meeting {id} failed: {error}");
                let _ = store.update(&id, |r| r.status = MeetingStatus::Failed { error });
            }
            emit_changed(&app, &id);
        })
        .expect("spawn meeting thread");
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

fn process(app: &AppHandle, store: &MeetingStore, id: &str) -> Result<()> {
    let record = store.get(id).context("meeting record missing")?;
    let all_settings = crate::settings::get_settings(app);
    let keys: HashMap<String, String> = (*all_settings.post_process_api_keys).clone();
    let settings = all_settings.meetings;
    let dir = store.audio_dir(id);

    // 1. Decode the tracks to 16 kHz mono.
    let mut tracks = Vec::new();
    for (file, kind) in [(MIC_FILE, TrackKind::Mic), (SYSTEM_FILE, TrackKind::System)] {
        if !record.tracks.iter().any(|t| t == file) {
            continue;
        }
        let path = dir.join(file);
        let samples =
            decode_audio_file(&path).with_context(|| format!("decoding {}", path.display()))?;
        if !samples.is_empty() {
            tracks.push(Track { kind, samples });
        }
    }
    if tracks.is_empty() {
        bail!("The recording has no audio.");
    }
    let timing: Option<super::TrackTiming> = std::fs::read_to_string(dir.join(super::TIMING_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    if let Some(t) = timing {
        if let Some(w) = align_tracks(&mut tracks, t.mic_first_us, t.system_first_us) {
            log::warn!("Meeting {id}: {w}");
        }
    }
    let duration_s = tracks.iter().map(|t| t.duration_s()).fold(0.0, f64::max);

    // Load the language model while speakers are detected (saves ~6 s later).
    if settings.correction_enabled || settings.summary_enabled {
        warm_up_llm(&settings, &keys);
    }

    // 2–4. Diarize, transcribe, dedup, name.
    set_stage(app, store, id, "diarizing", None);
    let mut warnings = record.warnings.clone();
    let diarizer_notes = Arc::new(std::sync::Mutex::new(Vec::new()));
    let diarizer: Box<dyn meeting_core::Diarizer> =
        match build_diarizer(app, &settings, &dir, diarizer_notes.clone()) {
            Ok(d) => d,
            Err(e) => {
                warnings.push(format!("Speakers not separated: {e:#}"));
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
    let namer = |track: TrackKind, _cluster: &str, embedding: Option<&[f32]>| -> Option<String> {
        let _ = track;
        let e = embedding?;
        profiles.as_ref()?.identify(e).map(|(name, _)| name)
    };
    let last_report = std::sync::Mutex::new(std::time::Instant::now() - Duration::from_secs(5));
    let progress = |done: usize, total: usize| {
        let mut last = last_report.lock().unwrap_or_else(|e| e.into_inner());
        if last.elapsed() >= Duration::from_millis(700) || done == total {
            *last = std::time::Instant::now();
            set_stage(
                app,
                store,
                id,
                "transcribing",
                Some(done as f32 / total.max(1) as f32),
            );
        }
    };
    let cfg = PipelineConfig {
        user_name: settings.user_name.clone(),
        ..Default::default()
    };
    let result = pipeline::run_with(
        &tracks,
        diarizer.as_ref(),
        &transcriber,
        &cfg,
        Hooks {
            namer: Some(&namer),
            progress: Some(&progress),
        },
    )?;
    warnings.extend(result.warnings.iter().cloned());
    // Free the diarizer as soon as it is done.
    drop(diarizer);
    warnings.extend(
        diarizer_notes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .drain(..),
    );
    save_speakers(&dir, &result.speakers);

    // 4b. Correct misheard words with the language model.
    let mut corrections = Vec::new();
    let mut transcript = result.transcript.clone();
    if settings.correction_enabled && !transcript.segments.is_empty() {
        set_stage(app, store, id, "correcting", None);
        let all = crate::settings::get_settings(app);
        let mut glossary = settings.glossary.clone();
        for w in all.custom_words {
            if !glossary.contains(&w) {
                glossary.push(w);
            }
        }
        let cfg = meeting_core::correct::CorrectConfig {
            glossary,
            ..Default::default()
        };
        // The model's default temperature (none is sent).
        // Answers are a few short lines; the cap stops a runaway answer
        // (seen: 3072 tokens, 133 s for one request).
        let llm = LlmSummarizer::new(LlmConfig {
            max_output_tokens: 400,
            ..llm_config(&settings, &keys)
        });
        let (fixed, applied, warns) =
            meeting_core::correct::correct_transcript(&llm, &transcript, &cfg);
        if !warns.is_empty() {
            warnings.push(format!(
                "Transcript correction partly skipped: {}",
                warns[0]
            ));
        }
        log::info!("Meeting {id}: {} transcript corrections", applied.len());
        transcript = fixed;
        corrections = applied;
    }

    // 5. Summary.
    let summary = if settings.summary_enabled && !transcript.segments.is_empty() {
        set_stage(app, store, id, "summarizing", None);
        match summarize(&settings, &keys, &transcript) {
            Ok(s) => Some(s),
            Err(e) => {
                warnings.push(format!("No summary: {e:#}"));
                None
            }
        }
    } else {
        None
    };

    // 6. Note.
    set_stage(app, store, id, "writing", None);
    let note = MeetingNote {
        meta: MeetingMeta {
            started_at: record.started_at.clone(),
            duration_s,
            source_app: record.source_app.clone(),
        },
        transcript: transcript.clone(),
        summary,
    };
    let notes_dir = settings.notes_dir_path();
    // Reprocessing replaces the earlier note instead of adding a second one.
    if let Some(old) = record.note_path.as_deref().map(PathBuf::from) {
        if old.starts_with(&notes_dir) && old.exists() {
            let _ = std::fs::remove_file(&old);
        }
    }
    let note_path = meeting_core::note::write_note(&notes_dir, &note)?;
    if !corrections.is_empty() {
        use std::io::Write;
        let german =
            meeting_core::note::note_language(&note) == meeting_core::lang::Language::German;
        let section = meeting_core::note::corrections_section(&corrections, german);
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&note_path) {
            let _ = f.write_all(section.as_bytes());
        }
    }
    let title = meeting_core::note::note_title(&note);

    // Opt-in "Your voice" dataset: only the user's own microphone segments
    // (named after the user, which happens on calls only).
    if let Some(mic) = tracks.iter().find(|t| t.kind == TrackKind::Mic) {
        let own: Vec<crate::voice_data::OwnSegment> = transcript
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

    // 7. Audio retention.
    let delete_audio = settings.keep_audio_days == 0;
    if delete_audio {
        for f in [MIC_FILE, SYSTEM_FILE] {
            let _ = std::fs::remove_file(dir.join(f));
        }
    }

    let speakers = result.transcript.speakers();
    let online = result
        .transcript
        .segments
        .iter()
        .any(|s| s.track == TrackKind::System);
    store.update(id, |r| {
        r.status = MeetingStatus::Done;
        r.title = Some(title);
        r.note_path = Some(note_path.to_string_lossy().into_owned());
        r.speakers = speakers;
        r.warnings = warnings;
        r.duration_s = duration_s;
        r.online = Some(online);
        if delete_audio {
            r.audio_deleted = true;
        }
    })?;
    emit_changed(app, id);
    notify_done(app, &note_path);
    Ok(())
}

/// Lines the tracks up in time: the track that started later gets leading
/// silence for the difference, so the same moment has the same timestamp on
/// both. Returns a warning when the offset looks implausible and is ignored.
fn align_tracks(tracks: &mut [Track], mic_first_us: u64, system_first_us: u64) -> Option<String> {
    if mic_first_us == 0 || system_first_us == 0 || tracks.len() < 2 {
        return None;
    }
    let offset_us = mic_first_us as i64 - system_first_us as i64;
    if offset_us.abs() > 5_000_000 {
        return Some(format!(
            "track start offset {offset_us} µs looks wrong; not aligned"
        ));
    }
    let pad = (offset_us.unsigned_abs() * SAMPLE_RATE as u64 / 1_000_000) as usize;
    // Positive offset: the microphone started later.
    let later = if offset_us > 0 {
        TrackKind::Mic
    } else {
        TrackKind::System
    };
    if let Some(t) = tracks.iter_mut().find(|t| t.kind == later) {
        let mut padded = vec![0.0f32; pad];
        padded.extend_from_slice(&t.samples);
        t.samples = padded;
    }
    log::info!(
        "Aligned tracks: {} started {} ms later",
        if offset_us > 0 { "mic" } else { "system" },
        offset_us.abs() / 1000
    );
    None
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

/// No diarization: the whole track is one speaker.
struct WholeTrack;

impl meeting_core::Diarizer for WholeTrack {
    fn diarize(&self, samples: &[f32]) -> Result<Vec<meeting_core::SpeakerTurn>> {
        if samples.is_empty() {
            return Ok(Vec::new());
        }
        Ok(vec![meeting_core::SpeakerTurn {
            start_s: 0.0,
            end_s: samples.len() as f64 / SAMPLE_RATE as f64,
            speaker: "S1".into(),
            embedding: None,
        }])
    }
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
        match super::diarize_worker::diarize_in_child(samples, &self.models, &self.scratch) {
            Ok(turns) => return Ok(turns),
            Err(e) => {
                log::warn!("GPU speaker detection failed, using the CPU: {e:#}");
                self.notes
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push(format!("Speaker detection on the GPU failed ({e:#}); the CPU was used instead (slower)."));
            }
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
        meeting_core::Diarizer::diarize(cpu.as_ref().expect("set above"), samples)
    }
}

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

/// One speaker of a processed meeting, kept so a name given later can be
/// remembered as a voice.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct SavedSpeaker {
    pub name: String,
    pub embedding: Option<Vec<f32>>,
}

pub const SPEAKERS_FILE: &str = "speakers.json";

fn save_speakers(dir: &std::path::Path, speakers: &[pipeline::SpeakerInfo]) {
    let saved: Vec<SavedSpeaker> = speakers
        .iter()
        .map(|s| SavedSpeaker {
            name: s.name.clone(),
            embedding: s.embedding.clone(),
        })
        .collect();
    if let Ok(text) = serde_json::to_string(&saved) {
        let _ = std::fs::write(dir.join(SPEAKERS_FILE), text);
    }
}

pub fn load_speakers(dir: &std::path::Path) -> Vec<SavedSpeaker> {
    std::fs::read_to_string(dir.join(SPEAKERS_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Provider ids that can summarise in the cloud, as in Handy's provider list.
pub const CLOUD_PROVIDERS: [&str; 3] = ["anthropic", "openai", "openrouter"];

/// Model used when `cloud_model` is empty.
pub fn default_cloud_model(provider: &str) -> &'static str {
    match provider {
        "anthropic" => "claude-opus-5-5",
        "openai" => "gpt-6.2-sol",
        "openrouter" => "anthropic/claude-opus-5.5",
        _ => "",
    }
}

fn llm_config(settings: &MeetingSettings, keys: &HashMap<String, String>) -> LlmConfig {
    let provider = settings.llm_provider.trim();
    if CLOUD_PROVIDERS.contains(&provider) {
        let model = match settings.cloud_model.trim() {
            "" => default_cloud_model(provider).to_string(),
            m => m.to_string(),
        };
        let key = keys
            .get(provider)
            .filter(|k| !k.trim().is_empty())
            .map(|k| k.trim().to_string());
        let mut cfg = match provider {
            "anthropic" => LlmConfig::anthropic(model, key),
            "openai" => LlmConfig::openai_compatible("https://api.openai.com/v1", model, key),
            _ => LlmConfig::openai_compatible("https://openrouter.ai/api/v1", model, key),
        };
        if provider != "anthropic" {
            // Cloud models have large contexts: one request per meeting.
            cfg.context_tokens = 128_000;
            cfg.max_output_tokens = 16_000;
        }
        cfg.timeout = Duration::from_secs(600);
        cfg.user_name = Some(settings.user_name.clone());
        return cfg;
    }
    let base = settings
        .llm_base_url
        .trim()
        .trim_end_matches('/')
        .to_string();
    let mut cfg = if is_ollama(&base) {
        LlmConfig {
            base_url: base,
            ..LlmConfig::ollama(settings.llm_model.trim())
        }
    } else {
        LlmConfig::openai_compatible(base, settings.llm_model.trim(), None)
    };
    cfg.user_name = Some(settings.user_name.clone());
    cfg
}

/// Ollama's native API, unless the URL looks like an OpenAI-style `/v1` base.
fn is_ollama(base: &str) -> bool {
    !base.ends_with("/v1")
}

fn summarize(
    settings: &MeetingSettings,
    keys: &HashMap<String, String>,
    transcript: &meeting_core::Transcript,
) -> Result<meeting_core::MeetingSummary> {
    use meeting_core::Summarizer;
    LlmSummarizer::new(llm_config(settings, keys)).summarize(transcript)
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

fn status(state: &str, detail: impl Into<String>) -> LlmStatus {
    LlmStatus {
        state: state.into(),
        detail: detail.into(),
    }
}

const OLLAMA_APP: &str = "/Applications/Ollama.app";
const OLLAMA_BINARIES: [&str; 2] = ["/opt/homebrew/bin/ollama", "/usr/local/bin/ollama"];

fn ollama_installed() -> bool {
    std::path::Path::new(OLLAMA_APP).exists()
        || OLLAMA_BINARIES
            .iter()
            .any(|b| std::path::Path::new(b).exists())
}

fn quick_agent(secs: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(secs)))
        .build()
        .into()
}

/// Model names Ollama has, or `None` when no server answers.
fn ollama_models(base: &str) -> Option<Vec<String>> {
    let body: serde_json::Value = quick_agent(3)
        .get(&format!("{base}/api/tags"))
        .call()
        .ok()?
        .body_mut()
        .read_json()
        .ok()?;
    Some(
        body["models"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|m| m["name"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    )
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
        return status("off", "");
    }
    let cfg = llm_config(settings, keys);
    let provider = settings.llm_provider.trim();
    if CLOUD_PROVIDERS.contains(&provider) {
        if cfg.api_key.is_none() {
            return status("no_key", &cfg.model);
        }
        if !probe {
            return status("ready", &cfg.model);
        }
        let llm = LlmSummarizer::new(LlmConfig {
            max_output_tokens: 300,
            timeout: Duration::from_secs(60),
            ..cfg.clone()
        });
        return match llm.chat_plain("Answer with the single word OK.", "Ping") {
            Ok(_) => status("ready", &cfg.model),
            Err(e) => status("error", format!("{e:#}")),
        };
    }
    let base = cfg.base_url.trim_end_matches('/').to_string();
    if cfg.backend != meeting_core::summary::Backend::Ollama {
        // A custom OpenAI-compatible local server (set in the config file).
        return match quick_agent(3).get(&format!("{base}/models")).call() {
            Ok(_) => status("ready", &cfg.model),
            Err(e) => status("error", format!("{base}: {e}")),
        };
    }
    match ollama_models(&base) {
        None if !ollama_installed() => status("no_ollama", &cfg.model),
        None => status("not_running", &cfg.model),
        Some(names) => {
            let m = cfg.model.as_str();
            let found = names
                .iter()
                .any(|n| n == m || (!m.contains(':') && n.split(':').next() == Some(m)));
            if found {
                status("ready", m)
            } else {
                status("model_missing", m)
            }
        }
    }
}

/// Starts Ollama (the app, or `ollama serve` for a Homebrew install) and
/// waits up to 15 s for it to answer.
pub fn start_ollama(settings: &MeetingSettings) -> Result<()> {
    let base = settings
        .llm_base_url
        .trim()
        .trim_end_matches('/')
        .to_string();
    if ollama_models(&base).is_some() {
        return Ok(());
    }
    if std::path::Path::new(OLLAMA_APP).exists() {
        std::process::Command::new("open")
            .args(["-g", "-a", "Ollama"])
            .status()
            .context("starting the Ollama app")?;
    } else if let Some(bin) = OLLAMA_BINARIES
        .iter()
        .find(|b| std::path::Path::new(b).exists())
    {
        std::process::Command::new(bin)
            .arg("serve")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("starting ollama serve")?;
    } else {
        bail!("Ollama is not installed");
    }
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(500));
        if ollama_models(&base).is_some() {
            return Ok(());
        }
    }
    bail!("Ollama did not start within 15 seconds")
}

/// Progress of a model download, sent as the "meeting-llm-pull" event.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PullProgress {
    pub status: String,
    pub completed: u64,
    pub total: u64,
}

/// Downloads the local model through Ollama, reporting progress.
pub fn pull_model(
    settings: &MeetingSettings,
    mut progress: impl FnMut(PullProgress),
) -> Result<()> {
    let base = settings.llm_base_url.trim().trim_end_matches('/');
    let model = settings.llm_model.trim();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(4 * 3600)))
        .build()
        .into();
    let mut resp = agent
        .post(&format!("{base}/api/pull"))
        .send_json(serde_json::json!({ "model": model, "stream": true }))
        .with_context(|| format!("No model server at {base}"))?;
    let reader = std::io::BufReader::new(resp.body_mut().as_reader());
    use std::io::BufRead;
    for line in reader.lines() {
        let line = line.context("reading download progress")?;
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if let Some(e) = v["error"].as_str() {
            bail!("{e}");
        }
        let st = v["status"].as_str().unwrap_or_default().to_string();
        progress(PullProgress {
            status: st.clone(),
            completed: v["completed"].as_u64().unwrap_or(0),
            total: v["total"].as_u64().unwrap_or(0),
        });
        if st == "success" {
            return Ok(());
        }
    }
    bail!("The download ended early")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_track_gets_leading_silence() {
        let mut tracks = vec![
            Track {
                kind: TrackKind::Mic,
                samples: vec![1.0; 10],
            },
            Track {
                kind: TrackKind::System,
                samples: vec![1.0; 10],
            },
        ];
        // Mic started 250 ms after system audio.
        assert!(align_tracks(&mut tracks, 1_000_250_000, 1_000_000_000).is_none());
        assert_eq!(tracks[0].samples.len(), 10 + 4_000);
        assert_eq!(tracks[0].samples[0], 0.0);
        assert_eq!(tracks[1].samples.len(), 10);
        // Implausible offsets are ignored.
        assert!(align_tracks(&mut tracks, 10_000_000_000, 1_000_000_000).is_some());
    }

    #[test]
    fn ollama_vs_openai_detection() {
        assert!(is_ollama("http://localhost:11434"));
        assert!(!is_ollama("http://localhost:1234/v1"));
        let mut s = MeetingSettings::default();
        s.llm_base_url = "http://localhost:11434/".into();
        s.user_name = "Robin".into();
        let cfg = llm_config(&s, &HashMap::new());
        assert_eq!(cfg.base_url, "http://localhost:11434");
        assert_eq!(cfg.user_name.as_deref(), Some("Robin"));
    }

    #[test]
    fn cloud_provider_uses_its_key_and_default_model() {
        use meeting_core::summary::Backend;
        let mut s = MeetingSettings::default();
        let keys: HashMap<String, String> =
            [("anthropic".to_string(), " key-a ".to_string())].into();
        assert_eq!(llm_config(&s, &keys).backend, Backend::Ollama);
        s.llm_provider = "anthropic".into();
        let cfg = llm_config(&s, &keys);
        assert_eq!(cfg.backend, Backend::Anthropic);
        assert_eq!(cfg.model, "claude-opus-5-5");
        assert_eq!(cfg.api_key.as_deref(), Some("key-a"));
        s.llm_provider = "openrouter".into();
        s.cloud_model = "google/gemini-3.8-flash".into();
        let cfg = llm_config(&s, &keys);
        assert_eq!(cfg.base_url, "https://openrouter.ai/api/v1");
        assert_eq!(cfg.model, "google/gemini-3.8-flash");
        assert!(cfg.api_key.is_none());
    }
}

/// Asks Ollama to load the model in the background, with the same context
/// size the requests use (a different `num_ctx` would load it again).
fn warm_up_llm(settings: &MeetingSettings, keys: &HashMap<String, String>) {
    let cfg = llm_config(settings, keys);
    if cfg.backend != meeting_core::summary::Backend::Ollama {
        return;
    }
    let url = format!("{}/api/generate", cfg.base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": cfg.model,
        "keep_alive": cfg.keep_alive.clone().unwrap_or_else(|| "1m".into()),
        "options": { "num_ctx": cfg.context_tokens },
    });
    std::thread::spawn(move || {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(120)))
            .build()
            .into();
        match agent.post(&url).send_json(&body) {
            Ok(_) => log::info!("Language model loaded in advance"),
            Err(e) => log::warn!("Loading the language model in advance failed: {e}"),
        }
    });
}

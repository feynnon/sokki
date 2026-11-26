//! One recorded meeting in, one note out:
//! align tracks → echo cancellation → speakers → transcribe turns → dedup
//! and names → summary → Markdown note.
//!
//! The host brings the platform parts (its speech recogniser, its speaker
//! detector, known voices) and gets progress callbacks; everything else is
//! here, the same on every platform.

use anyhow::{bail, Result};
use meeting_core::pipeline::{self, Hooks, PipelineConfig, SpeakerInfo, SpeakerNamer};
use meeting_core::summary::{LlmConfig, LlmSummarizer};
use meeting_core::{
    Diarizer, MeetingMeta, MeetingNote, Track, TrackKind, Transcriber, Transcript,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::llm::{self, LlmSettings};
use crate::tracks::{align_tracks, TrackTiming};

/// The person's meeting settings that processing needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// How the person appears in notes ("Ich", "Robin").
    pub user_name: String,
    /// Summary, decisions and tasks by the language model.
    pub summary: bool,
    /// Names and terms the summary spells right, even where speech
    /// recognition misheard them.
    pub glossary: Vec<String>,
    /// End the note with the Sokki line. Only a supporter
    /// (`supporter_code`) can turn it off; see [`Settings::sokki_line`].
    pub footer: bool,
    /// Proof of at least €20 paid ([`crate::supporter`]); empty: none.
    pub supporter_code: String,
    /// Where notes are written.
    pub notes_dir: PathBuf,
    pub llm: LlmSettings,
    /// The person's own language, for a note with no words to go by.
    /// None: German.
    pub language: Option<meeting_core::lang::Language>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            user_name: "Ich".into(),
            summary: true,
            glossary: Vec::new(),
            footer: true,
            supporter_code: String::new(),
            notes_dir: PathBuf::from("Notes"),
            llm: LlmSettings::default(),
            language: None,
        }
    }
}

impl Settings {
    /// Whether notes end with the Sokki line: always, unless a supporter
    /// switched it off.
    pub fn sokki_line(&self) -> bool {
        self.footer || crate::supporter::verify(&self.supporter_code).is_none()
    }

    /// The Sokki line for a summary in `language` that goes anywhere (a
    /// mail, a chat post), or `None` when a supporter switched it off. The
    /// one place for mails and posts to ask; the note uses the same rule.
    pub fn sokki_line_text(&self, language: meeting_core::lang::Language) -> Option<&'static str> {
        let opts = self.note_options();
        opts.footer
            .then(|| meeting_core::note::sokki_line(language, opts.cloud_summary))
    }

    /// The note options these settings give.
    pub fn note_options(&self) -> meeting_core::note::NoteOptions {
        meeting_core::note::NoteOptions {
            footer: self.sokki_line(),
            cloud_summary: llm::CLOUD_PROVIDERS.contains(&self.llm.provider.as_str()),
            language: self.language,
        }
    }
}

/// What processing is doing, for progress displays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Diarizing,
    Transcribing,
    Summarizing,
    Writing,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Diarizing => "diarizing",
            Stage::Transcribing => "transcribing",
            Stage::Summarizing => "summarizing",
            Stage::Writing => "writing",
        }
    }
}

/// Why a note has no summary although summaries are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SummaryMissing {
    /// The language model was not ready (not running, not downloaded, no
    /// key). The host adds the summary once it is, with [`summarize_note`].
    Waiting,
    /// The language model answered, but the summary failed. Retried only
    /// when the person asks.
    Failed,
}

/// The meeting card's line for [`SummaryMissing::Waiting`].
pub const SUMMARY_WAITING: &str = "The summary follows once the language model is ready.";
/// The meeting card's line for [`SummaryMissing::Failed`].
pub const SUMMARY_FAILED: &str = "The summary couldn't be written.";

/// One recorded meeting.
pub struct Input {
    /// Decoded 16 kHz mono tracks.
    pub tracks: Vec<Track>,
    /// When each track started, to line them up.
    pub timing: Option<TrackTiming>,
    /// RFC 3339 local start time.
    pub started_at: String,
    /// The call app, if detection saw one ("Zoom", "Google Meet").
    pub source_app: Option<String>,
    /// The note from an earlier run, replaced instead of duplicated.
    pub previous_note: Option<PathBuf>,
    /// Problems from recording, carried into the note's record.
    pub warnings: Vec<String>,
    /// What was transcribed during the recording ([`crate::live`]).
    pub live: Option<crate::live::Live>,
}

/// The platform parts.
pub struct Deps<'a> {
    pub transcriber: &'a dyn Transcriber,
    /// Dropped as soon as speakers are done, to free its memory.
    pub diarizer: Box<dyn Diarizer + 'a>,
    /// Problems the diarizer reports (e.g. "GPU failed, used the CPU"),
    /// read after it is done.
    pub diarizer_notes: Option<Arc<Mutex<Vec<String>>>>,
    /// Names known voices.
    pub namer: Option<&'a dyn SpeakerNamer>,
    /// Called with each stage and, when known, its progress (0–1).
    pub progress: &'a (dyn Fn(Stage, Option<f32>) + Sync),
    /// Asked before the summary: `true` saves the note without it, marked
    /// as waiting, for the host's retry (the Mac app: a meeting records).
    pub hold_summary: Option<&'a (dyn Fn() -> bool + Sync)>,
}

/// The result: the note and what a meeting list shows about it.
#[derive(Debug, Clone)]
pub struct Output {
    pub note_path: PathBuf,
    pub title: String,
    pub duration_s: f64,
    /// Speaker names in the transcript.
    pub speakers: Vec<String>,
    /// Speakers with their voice embeddings (for "remember this voice").
    pub speaker_info: Vec<SpeakerInfo>,
    /// One short sentence per problem, for the meeting card. The error
    /// text behind each is in the log.
    pub warnings: Vec<String>,
    /// Set when summaries are on but this note has none.
    pub summary_missing: Option<SummaryMissing>,
    /// Someone spoke on the call track (an online meeting).
    pub online: bool,
    pub transcript: Transcript,
    /// The tracks as processed (aligned, echo removed): the transcript's
    /// times refer to these.
    pub tracks: Vec<Track>,
}

/// [`run`]'s error when no track has a single word: speaker detection found
/// no turn, or every turn came back empty. A failed transcription is not
/// this ([`pipeline::PARTS_NOT_TRANSCRIBED`]): then the audio had speech.
/// No note is written and an earlier note stays as it was. Hosts find it
/// with `err.downcast_ref::<NoSpeech>()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoSpeech;

impl std::fmt::Display for NoSpeech {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("No speech found on any track; nothing saved.")
    }
}

impl std::error::Error for NoSpeech {}

/// The pipeline heard nothing: no turn, or no word in any turn, and no
/// transcription failed.
fn no_speech(r: &pipeline::PipelineResult) -> bool {
    let words = r
        .transcript
        .segments
        .iter()
        .any(|s| !s.text.trim().is_empty());
    !words
        && !r
            .warnings
            .iter()
            .any(|w| w == pipeline::PARTS_NOT_TRANSCRIBED)
}

/// Turns one meeting into a note. Blocking; minutes for an hour of audio.
/// Fails with [`NoSpeech`] when nobody said anything.
pub fn run(input: Input, deps: Deps, settings: &Settings) -> Result<Output> {
    let Input {
        mut tracks,
        timing,
        started_at,
        source_app,
        previous_note,
        mut warnings,
        live,
    } = input;
    tracks.retain(|t| !t.samples.is_empty());
    if tracks.is_empty() {
        bail!("The recording has no audio.");
    }
    let mut clock = StepClock::new(&tracks);
    let mut echo_from = Some(0);
    if let Some(t) = timing {
        if let Some(w) = align_tracks(&mut tracks, t.mic_first_us, t.system_first_us) {
            log::warn!("{w}");
            // Not lined up, the system track is no reference for the echo.
            echo_from = None;
        } else if tracks.len() > 1 {
            echo_from = crate::tracks::track_pads(t.mic_first_us, t.system_first_us).map(|p| p.1);
        }
    }
    // Remove the remote voices the mic picked up from the speakers, so they
    // are not found as an extra speaker on the mic. From where the system
    // track's audio starts: before it nothing played.
    let echo_from = echo_from.filter(|f| meeting_core::aec::echo_applies(&tracks, *f));
    // The call cleaned the mic the same way already, when it lined it up
    // alike: take that instead of filtering it again.
    let mut live = live;
    let taken = live
        .as_mut()
        .is_some_and(|l| crate::live::take_mic(l, timing, echo_from, &mut tracks));
    if let (Some(f), false) = (echo_from, taken) {
        meeting_core::aec::cancel_echo_in_tracks(&mut tracks, f);
        clock.step("echo cancellation");
    } else {
        clock.skip();
    }
    let texts = live.as_ref().map(|l| l.texts.clone());
    let (pieces, live_turns) = match live.map(|l| crate::live::usable(l, timing, echo_from)) {
        Some((p, t)) => (Some(p), Some(t)),
        None => (None, None),
    };
    // Turns transcribed during the call come from there, by their audio.
    let cached = texts.as_deref().map(|c| meeting_core::live::Cached::new(deps.transcriber, c));
    let transcriber: &dyn Transcriber = match &cached {
        Some(c) => c,
        None => deps.transcriber,
    };
    let duration_s = tracks.iter().map(|t| t.duration_s()).fold(0.0, f64::max);

    // Load the language model while speakers are detected (saves ~6 s
    // later), starting Ollama first if it is not running.
    let wants_llm = settings.summary;
    let preparing = wants_llm.then(|| llm::prepare(&settings.llm, &settings.user_name));

    // Speakers, transcription, dedup, names.
    (deps.progress)(Stage::Diarizing, None);
    let last_report = Mutex::new(Instant::now() - Duration::from_secs(5));
    let progress = |done: usize, total: usize| {
        let mut last = last_report.lock().unwrap_or_else(|e| e.into_inner());
        if last.elapsed() >= Duration::from_millis(700) || done == total {
            *last = Instant::now();
            (deps.progress)(Stage::Transcribing, Some(done as f32 / total.max(1) as f32));
        }
    };
    let cfg = PipelineConfig {
        user_name: settings.user_name.clone(),
        ..Default::default()
    };
    let result = pipeline::run_with(
        &tracks,
        deps.diarizer.as_ref(),
        transcriber,
        &cfg,
        Hooks {
            namer: deps.namer,
            progress: Some(&progress),
            pieces: pieces.as_deref(),
            turns: live_turns.as_deref(),
        },
    )?;
    for d in &result.details {
        log::warn!("{d}");
    }
    if result.voices_merged > 0 {
        log::info!(
            "Speakers: joined {} speaker(s) with the same voice on one track",
            result.voices_merged
        );
    }
    if no_speech(&result) {
        // Nothing was said on any track: no note (Robin, 2026-11-25: empty
        // meetings only fill the list). The host removes the meeting.
        log::info!(
            "No speech found on any track ({} turns): no note written",
            result.jobs
        );
        return Err(NoSpeech.into());
    }
    clock.add("speakers", result.speakers_s);
    clock.add("transcription", result.transcribe_s);
    clock.step("echo removal and names");
    if pieces.is_some() {
        log::info!(
            "{} of {} turns taken from the live transcript, {} transcribed during the call; speakers of {} tracks found during the call; mic {}",
            result.jobs_from_pieces,
            result.jobs,
            cached.as_ref().map_or(0, |c| c.hits()),
            live_turns.as_ref().map_or(0, Vec::len),
            if taken { "taken from the call" } else { "filtered again" }
        );
    }
    warnings.extend(result.warnings.iter().cloned());
    drop(deps.diarizer);
    if let Some(notes) = &deps.diarizer_notes {
        warnings.extend(notes.lock().unwrap_or_else(|p| p.into_inner()).drain(..));
    }

    // Without a ready language model the transcript is saved as it is, and
    // the summary follows later (see `summarize_note`).
    if let Some(p) = preparing {
        let _ = p.join();
    }
    let held = wants_llm && deps.hold_summary.is_some_and(|hold| hold());
    let llm_ready = wants_llm && !held && llm::is_ready(&settings.llm, &settings.user_name);
    clock.step("waiting for the language model");

    let transcript = result.transcript.clone();

    // Summary.
    let mut summary_missing = None;
    let summary = if settings.summary && !transcript.segments.is_empty() {
        if llm_ready {
            (deps.progress)(Stage::Summarizing, None);
            // The built-in model loads here (part of the "summary" step)
            // and is freed when `model` is dropped, right after it.
            let result = llm::open(&settings.llm, &settings.user_name).and_then(|model| {
                summarize_logged(
                    &LlmSummarizer::new(summary_config(settings, &model)),
                    &transcript,
                )
            });
            clock.step("summary");
            match result {
                Ok(s) => Some(s),
                Err(e) => {
                    log::warn!("No summary: {e:#}");
                    summary_missing = Some(SummaryMissing::Failed);
                    pipeline::warn_once(&mut warnings, SUMMARY_FAILED);
                    None
                }
            }
        } else {
            summary_missing = Some(SummaryMissing::Waiting);
            pipeline::warn_once(&mut warnings, SUMMARY_WAITING);
            None
        }
    } else {
        None
    };

    // The note.
    (deps.progress)(Stage::Writing, None);
    let note = MeetingNote {
        meta: MeetingMeta {
            started_at,
            duration_s,
            source_app,
        },
        transcript: transcript.clone(),
        summary,
    };
    // Reprocessing replaces the earlier note instead of adding a second one.
    if let Some(old) = previous_note {
        if old.starts_with(&settings.notes_dir) && old.exists() {
            let _ = std::fs::remove_file(&old);
        }
    }
    let note_path = meeting_core::note::write_note(
        &settings.notes_dir,
        &note,
        &settings.note_options(),
    )?;
    let title = meeting_core::note::note_title(&note);
    clock.step("writing the note");
    clock.finish();
    let online = result
        .transcript
        .segments
        .iter()
        .any(|s| s.track == TrackKind::System);
    Ok(Output {
        note_path,
        title,
        duration_s,
        speakers: result.transcript.speakers(),
        speaker_info: result.speakers,
        warnings,
        summary_missing,
        online,
        transcript,
        tracks,
    })
}

/// A note that got its summary later.
#[derive(Debug, Clone, PartialEq)]
pub struct Summarized {
    /// Where the note is now: renamed after the summary's title unless the
    /// person had renamed it.
    pub note_path: PathBuf,
    /// The summary's title, if it has one.
    pub title: Option<String>,
}

/// Adds the summary to a note written without one (the language model was
/// not ready then), from the note's own transcript, so it works after the
/// audio is deleted and keeps the person's edits. Fails when the note has
/// no transcript or has a summary already.
pub fn summarize_note(note_path: &Path, settings: &Settings) -> Result<Summarized> {
    use anyhow::Context;
    let md = std::fs::read_to_string(note_path)
        .with_context(|| format!("reading {}", note_path.display()))?;
    let transcript = meeting_core::note::transcript_from_note(&md);
    if transcript.segments.is_empty() {
        bail!("the note has no transcript to summarize");
    }
    let model = llm::open(&settings.llm, &settings.user_name)?;
    let summary =
        summarize_logged(&LlmSummarizer::new(summary_config(settings, &model)), &transcript)?;
    drop(model);
    let new_md =
        meeting_core::note::add_summary(&md, &summary, &settings.note_options()).context("the note has a summary already")?;
    let title = Some(summary.title.trim().to_string()).filter(|t| !t.is_empty());
    let new_stem = note_path
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| meeting_core::note::summarized_file_stem(s, &summary.title));
    let dir = note_path.parent().unwrap_or(Path::new("."));
    if let Some(stem) = new_stem {
        for n in 1..1000 {
            let name = if n == 1 {
                format!("{stem}.md")
            } else {
                format!("{stem} {n}.md")
            };
            let path = dir.join(name);
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut f) => {
                    use std::io::Write;
                    f.write_all(new_md.as_bytes())
                        .with_context(|| format!("writing {}", path.display()))?;
                    let _ = std::fs::remove_file(note_path);
                    return Ok(Summarized {
                        note_path: path,
                        title,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e).with_context(|| format!("creating {}", path.display())),
            }
        }
    }
    // Keep the name: write next to it, then replace it in one step.
    let tmp = note_path.with_extension("md.tmp");
    std::fs::write(&tmp, new_md.as_bytes())
        .with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, note_path)
        .with_context(|| format!("replacing {}", note_path.display()))?;
    Ok(Summarized {
        note_path: note_path.to_path_buf(),
        title,
    })
}

/// A title for a meeting that still records, from what its live transcript
/// has heard so far ([`crate::live::Heard`]). `background`: the built-in
/// model runs in the background band, below the recording.
pub fn provisional_title(heard: &str, settings: &Settings, background: bool) -> Result<String> {
    if heard.split_whitespace().next().is_none() {
        bail!("nothing heard yet");
    }
    let model = llm::open_with(&settings.llm, &settings.user_name, background)?;
    LlmSummarizer::new(summary_config(settings, &model)).provisional_title(heard)
}

/// The language model config for summaries: the person's dictionary goes
/// into the prompt, so names come out spelled right.
fn summary_config(settings: &Settings, model: &llm::Llm) -> LlmConfig {
    LlmConfig {
        glossary: settings.glossary.clone(),
        ..model.config()
    }
}

/// Summarizes and logs how: size, single request or parts, time.
fn summarize_logged(
    llm: &LlmSummarizer,
    transcript: &Transcript,
) -> Result<meeting_core::MeetingSummary> {
    let started = Instant::now();
    let cfg = llm.config();
    match llm.summarize_with_stats(transcript) {
        Ok((summary, st)) => {
            let how = if st.map_reduce {
                format!("in {} parts", st.chunks)
            } else {
                "in one request".to_string()
            };
            log::info!(
                "Summary by {}: ~{} tokens {how}, {} requests, {:.1} s",
                cfg.model,
                st.estimated_tokens,
                st.requests,
                st.elapsed.as_secs_f64()
            );
            Ok(summary)
        }
        Err(e) => {
            let tokens = meeting_core::lang::estimate_tokens(&transcript.to_plain_text());
            log::warn!(
                "Summary by {} failed after {:.1} s (~{tokens} tokens, timeout {} s per request)",
                cfg.model,
                started.elapsed().as_secs_f64(),
                llm.request_timeout(tokens).as_secs()
            );
            Err(e)
        }
    }
}

/// Times the steps of one meeting and logs each as it ends, so a slow
/// meeting can be read from the log: "Meeting step speakers: 95.2 s".
struct StepClock {
    started: Instant,
    last: Instant,
    /// Seconds of steps measured elsewhere since `last` (see `add`).
    inside: f64,
    steps: Vec<(&'static str, f64)>,
}

impl StepClock {
    fn new(tracks: &[Track]) -> Self {
        let audio: Vec<String> = tracks
            .iter()
            .map(|t| {
                let kind = match t.kind {
                    TrackKind::Mic => "mic",
                    TrackKind::System => "system",
                };
                format!("{kind} {}", minutes(t.duration_s()))
            })
            .collect();
        log::info!("Meeting processing starts: {}", audio.join(", "));
        let now = Instant::now();
        StepClock {
            started: now,
            last: now,
            inside: 0.0,
            steps: Vec::new(),
        }
    }

    /// The step that ends now: the time since the last step, less the
    /// steps added with `add` meanwhile.
    fn step(&mut self, name: &'static str) {
        let s = (self.last.elapsed().as_secs_f64() - self.inside).max(0.0);
        self.skip();
        self.record(name, s);
    }

    /// Time up to now belongs to no step worth naming.
    fn skip(&mut self) {
        self.last = Instant::now();
        self.inside = 0.0;
    }

    /// A step measured elsewhere, within the time up to the next `step`.
    fn add(&mut self, name: &'static str, seconds: f64) {
        self.inside += seconds;
        self.record(name, seconds);
    }

    fn record(&mut self, name: &'static str, seconds: f64) {
        log::info!("Meeting step {name}: {seconds:.1} s");
        self.steps.push((name, seconds));
    }

    /// One line with every step.
    fn finish(&self) {
        let list: Vec<String> = self
            .steps
            .iter()
            .map(|(n, s)| format!("{n} {s:.1} s"))
            .collect();
        log::info!(
            "Meeting processed in {:.1} s: {}",
            self.started.elapsed().as_secs_f64(),
            list.join(", ")
        );
    }

    #[cfg(test)]
    fn names(&self) -> Vec<&'static str> {
        self.steps.iter().map(|(n, _)| *n).collect()
    }
}

/// "40:50" for 2450 s.
fn minutes(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    format!("{}:{:02} min", s / 60, s % 60)
}

/// Expands a leading `~/` in a configured path.
pub fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(rest),
        None => Path::new(path).to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracks::WholeTrack;
    use meeting_core::SAMPLE_RATE;

    /// Says the same words for every turn.
    struct Parrot;

    impl Transcriber for Parrot {
        fn transcribe(&self, _samples: &[f32]) -> Result<String> {
            Ok("Hallo zusammen, das ist ein Test.".into())
        }
    }

    fn tone(seconds: f64) -> Vec<f32> {
        (0..(seconds * SAMPLE_RATE as f64) as usize)
            .map(|i| (i as f32 * 0.05).sin() * 0.3)
            .collect()
    }

    #[test]
    fn writes_a_note_without_language_model() {
        let dir = std::env::temp_dir().join(format!("sokki-engine-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let settings = Settings {
            summary: false,
            notes_dir: dir.clone(),
            ..Default::default()
        };
        let stages = Mutex::new(Vec::new());
        let progress = |s: Stage, _p: Option<f32>| stages.lock().unwrap().push(s);
        let out = run(
            Input {
                tracks: vec![Track {
                    kind: TrackKind::Mic,
                    samples: tone(3.0),
                }],
                timing: None,
                started_at: "2026-11-19T10:00:00+01:00".into(),
                source_app: Some("Google Meet".into()),
                previous_note: None,
                warnings: vec![],
                live: None,
            },
            Deps {
                transcriber: &Parrot,
                diarizer: Box::new(WholeTrack),
                diarizer_notes: None,
                namer: None,
                progress: &progress,
                hold_summary: None,
            },
            &settings,
        )
        .unwrap();
        let text = std::fs::read_to_string(&out.note_path).unwrap();
        assert!(text.contains("das ist ein Test"), "{text}");
        assert!(!out.online);
        assert_eq!(
            stages.lock().unwrap().first().copied(),
            Some(Stage::Diarizing)
        );
        assert_eq!(stages.lock().unwrap().last().copied(), Some(Stage::Writing));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn mic_input(seconds: f64) -> Input {
        Input {
            tracks: vec![Track {
                kind: TrackKind::Mic,
                samples: tone(seconds),
            }],
            timing: None,
            started_at: "2026-11-19T10:00:00+01:00".into(),
            source_app: None,
            previous_note: None,
            warnings: vec![],
            live: None,
        }
    }

    /// An OpenAI-style server on this machine that answers every request
    /// with the same summary. Returns its base URL ("…/v1").
    fn fake_llm() -> String {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let (mut line, mut length) = (String::new(), 0usize);
                while reader.read_line(&mut line).unwrap_or(0) > 2 {
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                    line.clear();
                }
                let mut body = vec![0u8; length];
                let _ = reader.read_exact(&mut body);
                let summary = serde_json::json!({
                    "title": "Test plan",
                    "summary": "We talked about the test.",
                    "decisions": [],
                    "action_items": []
                });
                let reply = serde_json::json!({
                    "choices": [{"message": {"content": summary.to_string()}}],
                    "data": []
                })
                .to_string();
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
            }
        });
        base
    }

    fn llm_settings(base_url: &str, dir: &Path) -> Settings {
        Settings {
            summary: true,
            notes_dir: dir.to_path_buf(),
            llm: LlmSettings {
                provider: "local".into(),
                base_url: base_url.into(),
                model: "test".into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn without_a_language_model_the_note_is_saved_and_the_summary_waits() {
        let dir = std::env::temp_dir().join(format!("sokki-engine-wait-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // Nothing listens there (an OpenAI-style URL, so Ollama is not started).
        let settings = llm_settings("http://127.0.0.1:9/v1", &dir);
        let progress = |_s: Stage, _p: Option<f32>| {};
        let out = run(
            mic_input(3.0),
            Deps {
                transcriber: &Parrot,
                diarizer: Box::new(WholeTrack),
                diarizer_notes: None,
                namer: None,
                progress: &progress,
                hold_summary: None,
            },
            &settings,
        )
        .unwrap();
        let first = std::fs::read_to_string(&out.note_path).unwrap();
        assert!(first.contains("das ist ein Test"));
        // No summary yet, so no Sokki line (it says "summarized").
        assert!(!first.contains("sokki.org"), "{first}");
        assert_eq!(out.summary_missing, Some(SummaryMissing::Waiting));
        assert_eq!(out.warnings, vec![SUMMARY_WAITING]);

        // Later the model is there: the same note gets its summary and the
        // title's name.
        let settings = llm_settings(&fake_llm(), &dir);
        let done = summarize_note(&out.note_path, &settings).unwrap();
        assert_eq!(done.title.as_deref(), Some("Test plan"));
        assert_eq!(
            done.note_path.file_name().unwrap().to_str().unwrap(),
            "2026-11-19 1000 Test plan.md"
        );
        assert!(!out.note_path.exists());
        let md = std::fs::read_to_string(&done.note_path).unwrap();
        assert!(md.contains("# Test plan\n"), "{md}");
        assert!(md.contains("We talked about the test."), "{md}");
        assert!(md.contains("das ist ein Test"), "{md}");
        // With the summary comes the Sokki line (in the note's language,
        // German here), last in the note and under the summary.
        let line = "Von KI zusammengefasst. Lokal, kostenlos, Open Source. Diktier-App und Meeting-Notetaker: sokki.org";
        assert!(md.ends_with(&format!("\n\n{line}\n")), "{md}");
        assert!(meeting_core::note::split_note(&md).summary.ends_with(line));
        // Only once.
        assert!(summarize_note(&done.note_path, &settings).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Switching the line off takes a supporter code; without one (or with
    /// a made-up one) the note still ends with it.
    #[test]
    fn the_sokki_line_stays_without_a_supporter_code() {
        let dir = std::env::temp_dir().join(format!("sokki-engine-line-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut settings = llm_settings(&fake_llm(), &dir);
        settings.footer = false;
        settings.supporter_code = "sokki-eyJ2IjoxfQ.bm90IGEgc2lnbmF0dXJl".into();
        assert!(settings.sokki_line());
        let progress = |_s: Stage, _p: Option<f32>| {};
        let out = run(
            mic_input(3.0),
            Deps {
                transcriber: &Parrot,
                diarizer: Box::new(WholeTrack),
                diarizer_notes: None,
                namer: None,
                progress: &progress,
                hold_summary: None,
            },
            &settings,
        )
        .unwrap();
        let md = std::fs::read_to_string(&out.note_path).unwrap();
        assert!(md.contains("We talked about the test."), "{md}");
        assert!(md.trim_end().ends_with("Dictation app and meeting notetaker: sokki.org"), "{md}");

        // Mails and posts get the same line from the same place (English:
        // the summary is).
        assert!(md.contains(
            settings
                .sokki_line_text(meeting_core::lang::Language::English)
                .unwrap()
        ));

        // A cloud model's summary is neither local nor free.
        settings.llm.provider = "anthropic".into();
        assert!(settings.note_options().cloud_summary);
        assert!(settings
            .sokki_line_text(meeting_core::lang::Language::English)
            .unwrap()
            .starts_with("Summarized by AI. "));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A meeting records while this one is processed (the Mac app): the
    /// note is saved without the summary even though the model is ready,
    /// and the summary waits for the retry.
    #[test]
    fn a_held_summary_waits_although_the_model_is_ready() {
        let dir = std::env::temp_dir().join(format!("sokki-engine-hold-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let settings = llm_settings(&fake_llm(), &dir);
        let progress = |_s: Stage, _p: Option<f32>| {};
        let asked = std::sync::atomic::AtomicBool::new(false);
        let hold = || {
            asked.store(true, std::sync::atomic::Ordering::Relaxed);
            true
        };
        let out = run(
            mic_input(3.0),
            Deps {
                transcriber: &Parrot,
                diarizer: Box::new(WholeTrack),
                diarizer_notes: None,
                namer: None,
                progress: &progress,
                hold_summary: Some(&hold),
            },
            &settings,
        )
        .unwrap();
        assert!(asked.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(out.summary_missing, Some(SummaryMissing::Waiting));
        assert_eq!(out.warnings, vec![SUMMARY_WAITING]);
        // Not held: the same model writes the summary.
        let not_held = || false;
        let out = run(
            mic_input(3.0),
            Deps {
                transcriber: &Parrot,
                diarizer: Box::new(WholeTrack),
                diarizer_notes: None,
                namer: None,
                progress: &progress,
                hold_summary: Some(&not_held),
            },
            &settings,
        )
        .unwrap();
        assert_eq!(out.summary_missing, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn step_clock_does_not_count_added_steps_twice() {
        let mut c = StepClock::new(&mic_input(2.0).tracks);
        std::thread::sleep(Duration::from_millis(50));
        c.add("speakers", 30.0);
        c.add("transcription", 60.0);
        c.step("names");
        assert_eq!(c.names(), ["speakers", "transcription", "names"]);
        // 50 ms passed, 90 s were added: the rest is not negative.
        assert_eq!(c.steps[2].1, 0.0);
        std::thread::sleep(Duration::from_millis(30));
        c.step("summary");
        assert!(c.steps[3].1 >= 0.03 && c.steps[3].1 < 1.0, "{:?}", c.steps);
        assert_eq!(minutes(2449.969), "40:50 min");
    }

    #[test]
    fn empty_recording_is_an_error() {
        let progress = |_s: Stage, _p: Option<f32>| {};
        let err = run(
            Input {
                tracks: vec![],
                timing: None,
                started_at: String::new(),
                source_app: None,
                previous_note: None,
                warnings: vec![],
                live: None,
            },
            Deps {
                transcriber: &Parrot,
                diarizer: Box::new(WholeTrack),
                diarizer_notes: None,
                namer: None,
                progress: &progress,
                hold_summary: None,
            },
            &Settings::default(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("no audio"));
    }

    /// Hears nothing in any turn, as Whisper does on silence.
    struct Silence;

    impl Transcriber for Silence {
        fn transcribe(&self, _samples: &[f32]) -> Result<String> {
            Ok("  ".into())
        }
    }

    /// Fails every turn.
    struct Broken;

    impl Transcriber for Broken {
        fn transcribe(&self, _samples: &[f32]) -> Result<String> {
            bail!("model gone")
        }
    }

    fn run_two_tracks(transcriber: &dyn Transcriber, dir: &Path) -> Result<Output> {
        let settings = Settings {
            summary: false,
            notes_dir: dir.to_path_buf(),
            ..Default::default()
        };
        let progress = |_s: Stage, _p: Option<f32>| {};
        run(
            Input {
                tracks: vec![
                    Track {
                        kind: TrackKind::Mic,
                        samples: tone(3.0),
                    },
                    Track {
                        kind: TrackKind::System,
                        samples: vec![0.0; 3 * SAMPLE_RATE as usize],
                    },
                ],
                timing: None,
                started_at: "2026-11-19T10:00:00+01:00".into(),
                source_app: Some("Zoom".into()),
                previous_note: None,
                warnings: vec![],
                live: None,
            },
            Deps {
                transcriber,
                diarizer: Box::new(WholeTrack),
                diarizer_notes: None,
                namer: None,
                progress: &progress,
                hold_summary: None,
            },
            &settings,
        )
    }

    #[test]
    fn no_word_on_any_track_writes_no_note() {
        let dir =
            std::env::temp_dir().join(format!("sokki-engine-nospeech-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let err = run_two_tracks(&Silence, &dir).unwrap_err();
        assert_eq!(err.downcast_ref::<NoSpeech>(), Some(&NoSpeech));
        let notes = std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0);
        assert_eq!(notes, 0, "no note in {}", dir.display());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_transcription_is_not_silence() {
        let dir = std::env::temp_dir().join(format!("sokki-engine-broken-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // The audio had speech: the note is kept, with the warning.
        match run_two_tracks(&Broken, &dir) {
            Ok(out) => assert!(out
                .warnings
                .iter()
                .any(|w| w == pipeline::PARTS_NOT_TRANSCRIBED)),
            Err(e) => assert!(e.downcast_ref::<NoSpeech>().is_none(), "{e:#}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

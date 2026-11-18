//! Headless end-to-end harness for the meeting pipeline.
//!
//! Runs real speech recognition (Parakeet v3 through transcribe-cpp, the same
//! engine the app uses) on the synthetic fixtures in `testdata/audio/`, and
//! scores the result against each fixture's `truth.json`:
//! - word error rate of the transcript text,
//! - speaker attribution (did each truth turn land under one consistent
//!   label, and are different truth speakers kept apart),
//! - echo duplicates left over from the system track.
//!
//! Paths:
//! - fixtures: `$MEETING_TESTDATA` or `<repo>/testdata/audio`
//! - models:   `$MEETING_MODELS` or `~/.cache/handy-meetings/models`

use anyhow::{bail, Context, Result};
use meeting_core::{Segment, Track, TrackKind, Transcriber, Transcript, SAMPLE_RATE};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once};

pub const PARAKEET_FILE: &str = "parakeet-tdt-0.6b-v3-Q8_0.gguf";

pub fn testdata_dir() -> PathBuf {
    if let Ok(p) = std::env::var("MEETING_TESTDATA") {
        return PathBuf::from(p);
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/audio")
}

pub fn models_dir() -> PathBuf {
    if let Ok(p) = std::env::var("MEETING_MODELS") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    Path::new(&home).join(".cache/handy-meetings/models")
}

// ---------------------------------------------------------------- fixtures

#[derive(Debug, Clone, Deserialize)]
pub struct TruthTurn {
    pub track: String,
    pub speaker: String,
    pub lang: String,
    pub text: String,
    pub start_s: f64,
    pub end_s: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Truth {
    pub scenario: String,
    pub duration_s: f64,
    pub speakers: Vec<String>,
    pub turns: Vec<TruthTurn>,
}

pub struct Fixture {
    pub dir: PathBuf,
    pub truth: Truth,
    pub tracks: Vec<Track>,
}

impl Fixture {
    /// `None` when the fixture folder is missing (tests then skip).
    pub fn load(scenario: &str) -> Result<Option<Fixture>> {
        let dir = testdata_dir().join(scenario);
        if !dir.join("truth.json").exists() {
            return Ok(None);
        }
        let truth: Truth = serde_json::from_str(&std::fs::read_to_string(dir.join("truth.json"))?)?;
        let mut tracks = Vec::new();
        for (name, kind) in [("mic", TrackKind::Mic), ("system", TrackKind::System)] {
            let p = dir.join(format!("{name}.wav"));
            if p.exists() {
                tracks.push(Track {
                    kind,
                    samples: read_wav_16k_mono(&p)?,
                });
            }
        }
        Ok(Some(Fixture { dir, truth, tracks }))
    }

    pub fn track(&self, kind: TrackKind) -> Option<&Track> {
        self.tracks.iter().find(|t| t.kind == kind)
    }
}

pub fn read_wav_16k_mono(path: &Path) -> Result<Vec<f32>> {
    let mut r = hound::WavReader::open(path).with_context(|| format!("open {}", path.display()))?;
    let spec = r.spec();
    if spec.sample_rate != SAMPLE_RATE || spec.channels != 1 {
        bail!(
            "{}: expected 16 kHz mono, got {} Hz x{}",
            path.display(),
            spec.sample_rate,
            spec.channels
        );
    }
    Ok(match spec.sample_format {
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<_, _>>()?
        }
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>()?,
    })
}

// ------------------------------------------------------------- transcriber

static BACKENDS: Once = Once::new();

/// `Transcriber` backed by transcribe-cpp, loading the model once.
pub struct EngineTranscriber {
    session: Mutex<transcribe_cpp::Session>,
    /// Options for every run (language hint, Whisper prompt).
    run: transcribe_cpp::RunOptions,
    // Keeps the model alive for the session.
    _model: transcribe_cpp::Model,
}

impl EngineTranscriber {
    pub fn load(path: &Path) -> Result<Self> {
        BACKENDS.call_once(|| {
            let _ = transcribe_cpp::init_backends_default();
        });
        if !path.exists() {
            bail!("model not found: {}", path.display());
        }
        let model =
            transcribe_cpp::Model::load_with(path, &transcribe_cpp::ModelOptions::default())
                .map_err(|e| anyhow::anyhow!("load {}: {e}", path.display()))?;
        let session = model
            .session()
            .map_err(|e| anyhow::anyhow!("session: {e}"))?;
        Ok(Self {
            session: Mutex::new(session),
            run: Default::default(),
            _model: model,
        })
    }

    /// The model in `MEETING_ASR_MODEL` (a file name in the models dir or a
    /// path), else Parakeet v3. `MEETING_ASR_LANGUAGE` sets a language hint
    /// ("de"); `MEETING_ASR_PROMPT` gives Whisper an initial prompt (e.g. the
    /// user's dictionary). `None` if the model file is missing.
    pub fn from_env() -> Result<Option<Self>> {
        let name = std::env::var("MEETING_ASR_MODEL").unwrap_or_else(|_| PARAKEET_FILE.into());
        let p = if name.contains('/') {
            PathBuf::from(&name)
        } else {
            models_dir().join(&name)
        };
        if !p.exists() {
            return Ok(None);
        }
        let mut t = Self::load(&p)?;
        if let Ok(lang) = std::env::var("MEETING_ASR_LANGUAGE") {
            if !lang.is_empty() {
                t.run.language = Some(lang);
            }
        }
        if let Ok(prompt) = std::env::var("MEETING_ASR_PROMPT") {
            if !prompt.is_empty() {
                t.run.family = Some(transcribe_cpp::RunExtension::Whisper(
                    transcribe_cpp::WhisperRunOptions {
                        initial_prompt: Some(prompt),
                        ..Default::default()
                    },
                ));
            }
        }
        Ok(Some(t))
    }

    /// Runs with explicit options (e.g. a per-call prompt).
    pub fn run_with(
        &self,
        samples: &[f32],
        opts: &transcribe_cpp::RunOptions,
    ) -> Result<transcribe_cpp::Transcript> {
        let mut s = self.session.lock().unwrap_or_else(|e| e.into_inner());
        s.run(samples, opts)
            .map_err(|e| anyhow::anyhow!("transcribe: {e}"))
    }

    pub fn run_options(&self) -> &transcribe_cpp::RunOptions {
        &self.run
    }

    /// Parakeet v3 from the models dir; `None` if not downloaded.
    pub fn parakeet() -> Result<Option<Self>> {
        let p = models_dir().join(PARAKEET_FILE);
        if !p.exists() {
            return Ok(None);
        }
        Self::load(&p).map(Some)
    }
}

impl Transcriber for EngineTranscriber {
    fn transcribe(&self, samples: &[f32]) -> Result<String> {
        if samples.len() < SAMPLE_RATE as usize / 10 {
            return Ok(String::new());
        }
        let mut s = self.session.lock().unwrap_or_else(|e| e.into_inner());
        let t = s
            .run(samples, &self.run)
            .map_err(|e| anyhow::anyhow!("transcribe: {e}"))?;
        Ok(t.text.trim().to_string())
    }

    fn transcribe_words(&self, samples: &[f32]) -> Result<Option<Vec<meeting_core::TimedWord>>> {
        if samples.len() < SAMPLE_RATE as usize / 10 {
            return Ok(Some(Vec::new()));
        }
        let mut s = self.session.lock().unwrap_or_else(|e| e.into_inner());
        let opts = transcribe_cpp::RunOptions {
            timestamps: transcribe_cpp::TimestampKind::Word,
            ..self.run.clone()
        };
        let t = s
            .run(samples, &opts)
            .map_err(|e| anyhow::anyhow!("transcribe: {e}"))?;
        if t.words.is_empty() && !t.text.trim().is_empty() {
            return Ok(None); // no word times from this model
        }
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
}

/// `PipelineConfig::window_s` from `MEETING_WINDOW` (seconds; `0` = turn by
/// turn), for comparing both ways on the fixtures.
pub fn window_from_env() -> Option<Option<f64>> {
    let v = std::env::var("MEETING_WINDOW").ok()?;
    let s: f64 = v.parse().ok()?;
    Some(if s > 0.0 { Some(s) } else { None })
}

// ----------------------------------------------------------------- metrics

/// Lowercase words, punctuation stripped, digits kept.
pub fn normalize_words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric()))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Word error rate: edit distance over reference word count.
pub fn wer(reference: &str, hypothesis: &str) -> f64 {
    let r = normalize_words(reference);
    let h = normalize_words(hypothesis);
    if r.is_empty() {
        return if h.is_empty() { 0.0 } else { 1.0 };
    }
    let mut prev: Vec<usize> = (0..=h.len()).collect();
    for i in 1..=r.len() {
        let mut cur = vec![i; h.len() + 1];
        for j in 1..=h.len() {
            let sub = prev[j - 1] + usize::from(r[i - 1] != h[j - 1]);
            cur[j] = sub.min(prev[j] + 1).min(cur[j - 1] + 1);
        }
        prev = cur;
    }
    prev[h.len()] as f64 / r.len() as f64
}

/// For each truth turn: the transcript label covering most of its time.
pub fn label_per_turn(truth: &Truth, transcript: &Transcript) -> Vec<Option<String>> {
    truth
        .turns
        .iter()
        .map(|turn| {
            let mut by_label: HashMap<&str, f64> = HashMap::new();
            for s in &transcript.segments {
                let ov = (turn.end_s.min(s.end_s) - turn.start_s.max(s.start_s)).max(0.0);
                if ov > 0.0 {
                    *by_label.entry(s.speaker.as_str()).or_default() += ov;
                }
            }
            by_label
                .into_iter()
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                .map(|(l, _)| l.to_string())
        })
        .collect()
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Scores {
    pub scenario: String,
    pub wer: f64,
    /// Truth turns attributed to the label their speaker maps to (best
    /// one-to-one mapping found greedily by co-occurrence).
    pub speaker_accuracy: f64,
    pub truth_speakers: usize,
    pub found_speakers: usize,
    /// Transcript segments that duplicate another track's speech.
    pub echo_duplicates: usize,
}

pub fn score(truth: &Truth, transcript: &Transcript) -> Scores {
    let reference = truth
        .turns
        .iter()
        .map(|t| t.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let hypothesis = transcript
        .segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let labels = label_per_turn(truth, transcript);

    // Greedy one-to-one mapping truth speaker -> label by co-occurrence count.
    let mut counts: HashMap<(String, String), usize> = HashMap::new();
    for (turn, label) in truth.turns.iter().zip(&labels) {
        if let Some(l) = label {
            *counts.entry((turn.speaker.clone(), l.clone())).or_default() += 1;
        }
    }
    let mut pairs: Vec<_> = counts.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1));
    let mut mapping: HashMap<String, String> = HashMap::new();
    let mut used: Vec<String> = Vec::new();
    for ((sp, l), _) in pairs {
        if !mapping.contains_key(&sp) && !used.contains(&l) {
            used.push(l.clone());
            mapping.insert(sp, l);
        }
    }
    let correct = truth
        .turns
        .iter()
        .zip(&labels)
        .filter(|(t, l)| l.is_some() && mapping.get(&t.speaker) == l.as_ref())
        .count();

    // Echo duplicates: a mic segment overlapping a system truth turn while
    // repeating its words.
    let echo_duplicates = transcript
        .segments
        .iter()
        .filter(|s| s.track == TrackKind::Mic)
        .filter(|s| {
            truth.turns.iter().any(|t| {
                t.track == "system"
                    && (t.end_s.min(s.end_s) - t.start_s.max(s.start_s)) > 0.3
                    && wer(&t.text, &s.text) < 0.6
            })
        })
        .count();

    Scores {
        scenario: truth.scenario.clone(),
        wer: wer(&reference, &hypothesis),
        speaker_accuracy: if truth.turns.is_empty() {
            1.0
        } else {
            correct as f64 / truth.turns.len() as f64
        },
        truth_speakers: truth.speakers.len(),
        found_speakers: transcript.speakers().len(),
        echo_duplicates,
    }
}

/// Transcript built straight from the truth turns (perfect diarization), with
/// each turn transcribed by `t`. Isolates ASR quality from diarization.
pub fn transcribe_truth_turns(fx: &Fixture, t: &dyn Transcriber) -> Result<Transcript> {
    let mut segments = Vec::new();
    for turn in &fx.truth.turns {
        let kind = if turn.track == "system" {
            TrackKind::System
        } else {
            TrackKind::Mic
        };
        let track = fx.track(kind).context("track missing")?;
        let a = ((turn.start_s - 0.15).max(0.0) * SAMPLE_RATE as f64) as usize;
        let b = (((turn.end_s + 0.15) * SAMPLE_RATE as f64) as usize).min(track.samples.len());
        segments.push(Segment {
            start_s: turn.start_s,
            end_s: turn.end_s,
            track: kind,
            speaker: turn.speaker.clone(),
            text: t.transcribe(&track.samples[a..b])?,
        });
    }
    Ok(Transcript { segments })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wer_basics() {
        assert_eq!(wer("Hallo Welt", "hallo, welt!"), 0.0);
        assert!((wer("a b c d", "a x c") - 0.5).abs() < 1e-9);
        assert_eq!(wer("", ""), 0.0);
    }
}

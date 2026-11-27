//! meeting-core: the platform-independent part of meeting notes.
//!
//! The app records one or two tracks (microphone = "me", system audio =
//! "them"), then runs this pipeline after the meeting ends:
//!
//! ```text
//! tracks ──► Diarizer (per track) ──► speaker turns
//!        ──► Transcriber (per turn) ──► Segments
//!        ──► merge + cross-track dedup ──► Transcript
//!        ──► Summarizer (LLM, optional) ──► MeetingSummary
//!        ──► render Markdown note ──► file in the notes folder
//! ```
//!
//! This crate defines the shared types and traits. Implementations live in:
//! - `diarize` crate: `Diarizer` (FluidAudio on macOS)
//! - this crate, `pipeline`/`dedup`/`note`/`summary` modules
//! - the app (`src-tauri`): `Transcriber`, backed by Sokki's transcription engine
//!
//! Everything here is plain Rust with no Tauri or OS dependencies, so it can
//! be tested quickly and reused on other platforms later.

use serde::{Deserialize, Serialize};

pub mod aec;
pub mod cite;
pub mod dedup;
pub mod echo;
pub mod lang;
pub mod live;
pub mod note;
pub mod own_notes;
pub mod pipeline;
pub mod share;
pub mod stitch;
pub mod summary;

/// All audio handed to this crate is 16 kHz mono f32 in [-1, 1].
pub const SAMPLE_RATE: u32 = 16_000;

/// Which recording a piece of audio came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    /// The local microphone: the user, plus anyone in the room or on a phone
    /// speaker next to the laptop.
    Mic,
    /// System audio: the remote side of a call running on this computer.
    System,
}

/// One recorded track, already decoded to 16 kHz mono.
#[derive(Debug, Clone)]
pub struct Track {
    pub kind: TrackKind,
    pub samples: Vec<f32>,
}

impl Track {
    pub fn duration_s(&self) -> f64 {
        self.samples.len() as f64 / SAMPLE_RATE as f64
    }
}

/// A stretch of one track where one speaker talks, as found by diarization.
/// Times are seconds from the start of the track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeakerTurn {
    pub start_s: f64,
    pub end_s: f64,
    /// Cluster id local to this track and this run, e.g. "S1", "S2".
    pub speaker: String,
    /// Optional voice embedding for this speaker cluster (L2-normalised),
    /// used to recognise known voices across meetings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Vec<f32>>,
}

/// Splits one track into speaker turns.
pub trait Diarizer: Send + Sync {
    /// `samples` is 16 kHz mono. Returns turns sorted by start time; turns do
    /// not overlap. Silence is not covered by any turn. An empty or silent
    /// track returns an empty vector.
    fn diarize(&self, samples: &[f32]) -> anyhow::Result<Vec<SpeakerTurn>>;
}

/// Turns a short stretch of 16 kHz mono audio into text.
pub trait Transcriber: Send + Sync {
    fn transcribe(&self, samples: &[f32]) -> anyhow::Result<String>;

    /// The words in `samples` with their times, in seconds from the start of
    /// `samples`. `Ok(None)` when the model gives no word times; the pipeline
    /// then transcribes turn by turn with [`Transcriber::transcribe`].
    fn transcribe_words(&self, _samples: &[f32]) -> anyhow::Result<Option<Vec<TimedWord>>> {
        Ok(None)
    }

    /// Clean-up applied to the text of each line built from
    /// [`Transcriber::transcribe_words`] (e.g. filler words, custom words).
    /// [`Transcriber::transcribe`] is expected to apply it itself.
    fn polish(&self, text: &str) -> String {
        text.to_string()
    }
}

/// One recognised word. Times in seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct TimedWord {
    pub start_s: f64,
    pub end_s: f64,
    pub text: String,
}

/// One line of the final transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    /// Seconds from the start of the meeting.
    pub start_s: f64,
    pub end_s: f64,
    pub track: TrackKind,
    /// Display name: a known name ("Robin"), or a generic label
    /// ("Speaker 1", "Speaker 2").
    pub speaker: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Transcript {
    /// Sorted by `start_s`.
    pub segments: Vec<Segment>,
}

impl Transcript {
    /// Distinct speaker names in order of first appearance.
    pub fn speakers(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for s in &self.segments {
            if !out.contains(&s.speaker) {
                out.push(s.speaker.clone());
            }
        }
        out
    }

    /// Plain text, one line per segment: "[mm:ss] Speaker: text".
    pub fn to_plain_text(&self) -> String {
        self.segments
            .iter()
            .map(|s| format!("[{}] {}: {}", format_mmss(s.start_s), s.speaker, s.text))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub text: String,
}

/// What the LLM produces from a transcript.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MeetingSummary {
    /// Short title, a few words.
    pub title: String,
    /// A few sentences or bullet points, in the meeting's language.
    pub summary: String,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub action_items: Vec<ActionItem>,
}

/// Produces a summary from a transcript.
pub trait Summarizer: Send + Sync {
    fn summarize(&self, transcript: &Transcript) -> anyhow::Result<MeetingSummary>;
}

/// Facts about the meeting itself, for the note's frontmatter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeetingMeta {
    /// The meeting's stable id: its folder name ("meeting-20261117-140251-015").
    /// Written into the note's frontmatter as `id:`, so the note, its folder
    /// and the index (`crates/sokki-index`) refer to each other. Never
    /// changes when the note is renamed or retitled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Local start time, RFC 3339, e.g. "2026-11-17T14:03:00+01:00".
    pub started_at: String,
    pub duration_s: f64,
    /// The app that held the microphone when the meeting started, if known
    /// (e.g. "Zoom", "Google Chrome").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_app: Option<String>,
}

/// Everything that goes into one Markdown note.
#[derive(Debug, Clone, PartialEq)]
pub struct MeetingNote {
    pub meta: MeetingMeta,
    pub transcript: Transcript,
    /// None when no summary model is configured or summarising failed.
    pub summary: Option<MeetingSummary>,
}

/// "mm:ss", or "h:mm:ss" from one hour on.
pub fn format_mmss(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mmss_formats_minutes_and_hours() {
        assert_eq!(format_mmss(0.0), "00:00");
        assert_eq!(format_mmss(65.4), "01:05");
        assert_eq!(format_mmss(3725.0), "1:02:05");
    }

    #[test]
    fn speakers_in_order_of_first_appearance() {
        let seg = |sp: &str, t: f64| Segment {
            start_s: t,
            end_s: t + 1.0,
            track: TrackKind::Mic,
            speaker: sp.into(),
            text: "x".into(),
        };
        let t = Transcript {
            segments: vec![seg("Robin", 0.0), seg("Speaker 2", 1.0), seg("Robin", 2.0)],
        };
        assert_eq!(t.speakers(), vec!["Robin", "Speaker 2"]);
    }
}

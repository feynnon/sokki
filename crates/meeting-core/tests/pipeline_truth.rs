//! End-to-end pipeline runs driven by the ground truth of the synthetic audio
//! scenarios (`testdata/audio/*/truth.json`, copied to tests/fixtures/truth/).
//!
//! A fake diarizer returns the truth turns for each track and a fake
//! transcriber returns the truth text for the audio it is given, so these
//! tests check the pipeline logic (naming, merge order, echo removal) on
//! realistic turn layouts without any models.

mod common;

use std::sync::Mutex;

use anyhow::Result;
use meeting_core::note::render_markdown;
use meeting_core::pipeline::{run, PipelineConfig};
use meeting_core::{
    Diarizer, MeetingMeta, MeetingNote, SpeakerTurn, Track, TrackKind, Transcriber, SAMPLE_RATE,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Truth {
    duration_s: f64,
    turns: Vec<TruthTurn>,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Deserialize, Clone)]
struct TruthTurn {
    track: String,
    speaker: String,
    text: String,
    start_s: f64,
    end_s: f64,
}

fn load_truth(name: &str) -> Truth {
    let p = common::fixture_path(&format!("truth/{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

/// Sample values encode track and time: mic = 0.1 + t·1e-4, system = 0.5 + t·1e-4.
fn track(kind: TrackKind, dur: f64) -> Track {
    let base = if kind == TrackKind::Mic { 0.1 } else { 0.5 };
    let n = (dur * SAMPLE_RATE as f64) as usize;
    Track {
        kind,
        samples: (0..n)
            .map(|i| base + (i as f64 / SAMPLE_RATE as f64 * 1e-4) as f32)
            .collect(),
    }
}
fn decode(sample: f32) -> (TrackKind, f64) {
    if sample >= 0.45 {
        (TrackKind::System, ((sample - 0.5) / 1e-4) as f64)
    } else {
        (TrackKind::Mic, ((sample - 0.1) / 1e-4) as f64)
    }
}

/// Turns per track; on the mic, the system turns appear again as echo
/// (own cluster "E") when `echo` is set.
struct TruthDiarizer {
    turns: Vec<TruthTurn>,
    echo: bool,
}
impl Diarizer for TruthDiarizer {
    fn diarize(&self, samples: &[f32]) -> Result<Vec<SpeakerTurn>> {
        let (kind, _) = decode(samples[0]);
        let mut out: Vec<SpeakerTurn> = Vec::new();
        for t in &self.turns {
            let on_this = (t.track == "mic") == (kind == TrackKind::Mic);
            if on_this {
                out.push(SpeakerTurn {
                    start_s: t.start_s,
                    end_s: t.end_s,
                    speaker: t.speaker.clone(),
                    embedding: None,
                });
            } else if self.echo && kind == TrackKind::Mic {
                // The diarizer sees the laptop-speaker echo as one more voice.
                out.push(SpeakerTurn {
                    start_s: t.start_s + 0.05,
                    end_s: t.end_s + 0.05,
                    speaker: "E".into(),
                    embedding: None,
                });
            }
        }
        out.sort_by(|a, b| a.start_s.total_cmp(&b.start_s));
        Ok(out)
    }
}

/// Returns the truth text whose turn contains the middle of the audio. Echo
/// copies on the mic come back with typical ASR damage (lower case, no
/// punctuation, one word misheard).
struct TruthTranscriber {
    turns: Vec<TruthTurn>,
    calls: Mutex<usize>,
}
impl Transcriber for TruthTranscriber {
    fn transcribe(&self, samples: &[f32]) -> Result<String> {
        *self.calls.lock().unwrap() += 1;
        let (kind, a) = decode(samples[0]);
        let (_, b) = decode(*samples.last().unwrap());
        let mid = (a + b) / 2.0;
        let hit = self
            .turns
            .iter()
            .find(|t| t.start_s <= mid && mid <= t.end_s + 0.2);
        Ok(match hit {
            None => String::new(),
            Some(t) if (t.track == "mic") == (kind == TrackKind::Mic) => t.text.clone(),
            Some(t) => {
                // Echo on the mic of a system turn.
                let mut words: Vec<String> = t
                    .text
                    .split_whitespace()
                    .map(|w| {
                        w.trim_matches(|c: char| !c.is_alphanumeric())
                            .to_lowercase()
                    })
                    .collect();
                if words.len() > 4 {
                    words[3] = "äh".into();
                }
                words.join(" ")
            }
        })
    }
}

fn run_truth(name: &str, echo: bool) -> (meeting_core::pipeline::PipelineResult, Truth) {
    let truth = load_truth(name);
    let mut tracks = vec![track(TrackKind::Mic, truth.duration_s)];
    if truth.turns.iter().any(|t| t.track == "system") || echo {
        tracks.push(track(TrackKind::System, truth.duration_s));
    }
    let d = TruthDiarizer {
        turns: truth.turns.clone(),
        echo,
    };
    let t = TruthTranscriber {
        turns: truth.turns.clone(),
        calls: Mutex::new(0),
    };
    (
        run(&tracks, &d, &t, &PipelineConfig::default()).unwrap(),
        truth,
    )
}

#[test]
fn call_with_laptop_speaker_echo() {
    let (r, truth) = run_truth("call_tracks", true);
    assert!(truth.note.unwrap().contains("echo"));
    // Every echo copy removed; the remaining transcript equals the truth.
    assert_eq!(r.dedup.removed, 3, "{:#?}", r.transcript);
    let got: Vec<(&str, &str)> = r
        .transcript
        .segments
        .iter()
        .map(|s| (s.speaker.as_str(), s.text.as_str()))
        .collect();
    let want: Vec<(&str, &str)> = truth
        .turns
        .iter()
        .map(|t| {
            let sp = match t.speaker.as_str() {
                "Me" => "Ich",
                "R1" => "Speaker 2",
                "R2" => "Speaker 3",
                other => panic!("{other}"),
            };
            (sp, t.text.as_str())
        })
        .collect();
    assert_eq!(got, want);
    // The echo cluster is gone entirely: no mic speaker besides the user.
    assert_eq!(
        r.speakers
            .iter()
            .filter(|s| s.track == TrackKind::Mic)
            .count(),
        1
    );
}

#[test]
fn call_with_headphones_has_nothing_to_remove() {
    let (r, truth) = run_truth("call_tracks", false);
    assert_eq!(r.dedup.removed + r.dedup.trimmed, 0);
    assert_eq!(r.transcript.segments.len(), truth.turns.len());
    assert_eq!(
        r.transcript.speakers(),
        vec!["Ich", "Speaker 2", "Speaker 3"]
    );
}

#[test]
fn in_room_meeting_two_people_on_mic() {
    let (r, _) = run_truth("de_two", false);
    // Everything on one microphone: nobody is guessed to be the user, every
    // voice gets a neutral label in order of first appearance.
    assert_eq!(r.transcript.speakers(), vec!["Speaker 1", "Speaker 2"]);
    assert_eq!(r.transcript.segments.len(), 6);
    let md = render_markdown(&MeetingNote {
        meta: MeetingMeta {
            id: None,
            started_at: "2026-11-17T09:30:00+01:00".into(),
            duration_s: 38.6,
            source_app: None,
        },
        transcript: r.transcript.clone(),
        summary: None,
    });
    assert!(md.contains("**[00:01] Speaker 1:** Hallo zusammen"), "{md}");
    assert!(md.contains("**[00:07] Speaker 2:** Gerne."), "{md}");
}

#[test]
fn three_remote_speakers_only_on_system() {
    let (r, _) = run_truth("three_remote", false);
    assert_eq!(
        r.transcript.speakers(),
        vec!["Speaker 1", "Speaker 2", "Speaker 3"]
    );
    assert!(r.speakers.iter().all(|s| !s.is_user));
}

#[test]
fn mixed_language_in_room() {
    let (r, _) = run_truth("de_en_mixed", false);
    assert_eq!(r.transcript.segments.len(), 6);
    assert_eq!(r.transcript.speakers().len(), 2);
}

#[test]
fn silence_gives_empty_transcript() {
    let (r, _) = run_truth("silence", false);
    assert!(r.transcript.segments.is_empty());
    assert!(r.warnings.is_empty());
}

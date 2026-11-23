//! Full pipeline on the synthetic fixtures with real speech recognition:
//! tracks → diarizer → Parakeet → dedup → naming → (summary) → Markdown note.
//!
//! Two diarizers:
//! - `oracle`: speaker turns straight from truth.json. Scores the rest of the
//!   pipeline (ASR, chunking, dedup, naming, note) independent of diarization.
//! - the real one from the `diarize` crate (see `real_diarizer.rs`).
//!
//! Set `MEETING_LIVE_LLM=1` to also summarise with local Ollama.

use meeting_core::pipeline::{self, PipelineConfig};
use meeting_core::{Diarizer, MeetingMeta, MeetingNote, SpeakerTurn, TrackKind};
use meeting_e2e::*;

/// Diarization from the fixture's ground truth, for one track.
struct Oracle {
    turns: Vec<SpeakerTurn>,
}

impl Oracle {
    fn for_track(truth: &Truth, track: &str) -> Self {
        let turns = truth
            .turns
            .iter()
            .filter(|t| t.track == track)
            .map(|t| SpeakerTurn {
                start_s: t.start_s,
                end_s: t.end_s,
                speaker: format!("S-{}", t.speaker),
                embedding: None,
            })
            .collect();
        Self { turns }
    }
}

/// Routes each track to its own oracle (the pipeline calls the diarizer once
/// per track, mic first).
struct OracleByTrack {
    mic: Oracle,
    system: Oracle,
    calls: std::sync::Mutex<usize>,
    order: Vec<TrackKind>,
}

impl Diarizer for OracleByTrack {
    fn diarize(&self, _samples: &[f32]) -> anyhow::Result<Vec<SpeakerTurn>> {
        let mut n = self.calls.lock().unwrap();
        let kind = self.order[*n];
        *n += 1;
        Ok(match kind {
            TrackKind::Mic => self.mic.turns.clone(),
            TrackKind::System => self.system.turns.clone(),
        })
    }
}

fn transcriber() -> Option<EngineTranscriber> {
    match EngineTranscriber::from_env() {
        Ok(Some(t)) => Some(t),
        Ok(None) => {
            eprintln!("SKIP: {PARAKEET_FILE} not in {}", models_dir().display());
            None
        }
        Err(e) => panic!("loading Parakeet failed: {e:#}"),
    }
}

fn out_dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join("hm-e2e-notes");
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn oracle_pipeline_on_all_fixtures() {
    let Some(asr) = transcriber() else { return };
    let live = std::env::var("MEETING_LIVE_LLM").is_ok_and(|v| v == "1");
    let mut failures = Vec::new();
    for scenario in [
        "de_two",
        "de_en_mixed",
        "phone_speaker",
        "call_tracks",
        "three_remote",
        "silence",
    ] {
        let Some(fx) = Fixture::load(scenario).unwrap() else {
            eprintln!("SKIP {scenario}: fixture missing");
            continue;
        };
        let diarizer = OracleByTrack {
            mic: Oracle::for_track(&fx.truth, "mic"),
            system: Oracle::for_track(&fx.truth, "system"),
            calls: Default::default(),
            order: fx.tracks.iter().map(|t| t.kind).collect(),
        };
        let mut cfg = PipelineConfig {
            user_name: "Robin".into(),
            ..Default::default()
        };
        if let Some(w) = meeting_e2e::window_from_env() {
            cfg.window_s = w;
        }
        let started = std::time::Instant::now();
        let result = pipeline::run(&fx.tracks, &diarizer, &asr, &cfg).unwrap();
        let s = score(&fx.truth, &result.transcript);
        eprintln!(
            "{scenario:14} WER {:5.1}%  speakers {}/{}  attribution {:5.1}%  echoes left {}  removed {}  ({:.1}s)",
            s.wer * 100.0,
            s.found_speakers,
            s.truth_speakers,
            s.speaker_accuracy * 100.0,
            s.echo_duplicates,
            result.dedup.removed,
            started.elapsed().as_secs_f64()
        );

        // Expectations with perfect diarization.
        if scenario == "silence" {
            if !result.transcript.segments.is_empty() {
                failures.push(format!("{scenario}: silence produced text"));
            }
            continue;
        }
        let max_wer = if scenario == "phone_speaker" {
            0.30
        } else {
            0.15
        };
        if s.wer > max_wer {
            failures.push(format!(
                "{scenario}: WER {:.1}% > {:.0}%",
                s.wer * 100.0,
                max_wer * 100.0
            ));
        }
        if s.speaker_accuracy < 0.99 {
            failures.push(format!(
                "{scenario}: attribution {:.0}%",
                s.speaker_accuracy * 100.0
            ));
        }
        if s.echo_duplicates > 0 {
            failures.push(format!(
                "{scenario}: {} echo duplicates left",
                s.echo_duplicates
            ));
        }
        if s.found_speakers != s.truth_speakers {
            failures.push(format!(
                "{scenario}: {} speakers, expected {}",
                s.found_speakers, s.truth_speakers
            ));
        }

        // Note, with an optional live summary.
        let summary = if live {
            use meeting_core::Summarizer;
            let llm = meeting_core::summary::LlmSummarizer::new(meeting_core::summary::LlmConfig {
                user_name: Some("Robin".into()),
                ..meeting_core::summary::LlmConfig::ollama(
                    std::env::var("MEETING_LLM_MODEL").unwrap_or_else(|_| "qwen3.5:9b".into()),
                )
            });
            let t = std::time::Instant::now();
            let sm = llm.summarize(&result.transcript).unwrap();
            eprintln!(
                "   summary in {:.0}s: {}",
                t.elapsed().as_secs_f64(),
                sm.title
            );
            Some(sm)
        } else {
            None
        };
        let note = MeetingNote {
            meta: MeetingMeta {
                started_at: "2026-11-17T10:00:00+01:00".into(),
                duration_s: fx.truth.duration_s,
                source_app: None,
            },
            transcript: result.transcript.clone(),
            summary,
        };
        let path = meeting_core::note::write_note(&out_dir(), &note, &Default::default()).unwrap();
        let md = std::fs::read_to_string(&path).unwrap();
        if !md.starts_with("---\n") || !md.contains("type: meeting") {
            failures.push(format!("{scenario}: note frontmatter missing"));
        }
        if !md.contains("**[") {
            failures.push(format!("{scenario}: note has no transcript lines"));
        }
        eprintln!("   note: {}", path.display());
    }
    assert!(failures.is_empty(), "failures:\n{}", failures.join("\n"));
}

/// The real diarizer (Sortformer + sherpa-onnx embeddings), as in the app.
#[test]
fn real_pipeline_on_all_fixtures() {
    let Some(asr) = transcriber() else { return };
    let models = std::env::var("DIARIZE_MODELS_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::Path::new(&std::env::var("HOME").unwrap())
                .join("Library/Caches/sokki/diarize-models")
        });
    let live = std::env::var("MEETING_LIVE_LLM").is_ok_and(|v| v == "1");
    let mut failures = Vec::new();
    let mut table = Vec::new();
    for scenario in [
        "de_two",
        "de_en_mixed",
        "phone_speaker",
        "call_tracks",
        "silence",
    ] {
        let Some(fx) = Fixture::load(scenario).unwrap() else {
            continue;
        };
        // A fresh diarizer per meeting, as the app does (ggml-metal must not
        // outlive the process teardown).
        let diarizer =
            diarize::SortformerDiarizer::new(diarize::DiarizeConfig::new(&models)).unwrap();
        let mut cfg = PipelineConfig {
            user_name: "Robin".into(),
            ..Default::default()
        };
        if let Some(w) = meeting_e2e::window_from_env() {
            cfg.window_s = w;
        }
        let started = std::time::Instant::now();
        let result = pipeline::run(&fx.tracks, &diarizer, &asr, &cfg).unwrap();
        drop(diarizer);
        let secs = started.elapsed().as_secs_f64();
        let s = score(&fx.truth, &result.transcript);
        table.push(format!(
            "{scenario:14} WER {:5.1}%  speakers {}/{}  attribution {:5.1}%  echoes left {}  {:.1}s for {:.0}s audio",
            s.wer * 100.0, s.found_speakers, s.truth_speakers, s.speaker_accuracy * 100.0,
            s.echo_duplicates, secs, fx.truth.duration_s
        ));
        if scenario == "silence" {
            if !result.transcript.segments.is_empty() {
                failures.push(format!(
                    "{scenario}: silence produced {} segments",
                    result.transcript.segments.len()
                ));
            }
            continue;
        }
        let max_wer = if scenario == "phone_speaker" {
            0.30
        } else {
            0.18
        };
        if s.wer > max_wer {
            failures.push(format!("{scenario}: WER {:.1}%", s.wer * 100.0));
        }
        if s.speaker_accuracy < 0.8 {
            failures.push(format!(
                "{scenario}: attribution {:.0}%",
                s.speaker_accuracy * 100.0
            ));
        }
        if s.found_speakers != s.truth_speakers {
            failures.push(format!(
                "{scenario}: {} speakers, expected {}",
                s.found_speakers, s.truth_speakers
            ));
        }
        if s.echo_duplicates > 0 {
            failures.push(format!("{scenario}: {} echo duplicates", s.echo_duplicates));
        }
        // Online meeting: the mic voice is the person recording ("Robin").
        // Single microphone: nobody may be labelled "Robin".
        let recorder = match scenario {
            "call_tracks" => Some("Me"),
            _ => Some("<nobody>"),
        };
        if let Some(who) = recorder {
            let labels = label_per_turn(&fx.truth, &result.transcript);
            let wrong = fx
                .truth
                .turns
                .iter()
                .zip(&labels)
                .filter(|(t, l)| (t.speaker == who) != (l.as_deref() == Some("Robin")))
                .count();
            if wrong > 0 {
                failures.push(format!("{scenario}: user label wrong on {wrong} turns"));
            }
        }

        let summary = if live {
            use meeting_core::Summarizer;
            let llm = meeting_core::summary::LlmSummarizer::new(meeting_core::summary::LlmConfig {
                user_name: Some("Robin".into()),
                ..meeting_core::summary::LlmConfig::ollama("qwen3.5:9b")
            });
            match llm.summarize(&result.transcript) {
                Ok(sm) => Some(sm),
                Err(e) => {
                    failures.push(format!("{scenario}: summary failed: {e:#}"));
                    None
                }
            }
        } else {
            None
        };
        let note = MeetingNote {
            meta: MeetingMeta {
                started_at: "2026-11-17T10:00:00+01:00".into(),
                duration_s: fx.truth.duration_s,
                source_app: None,
            },
            transcript: result.transcript.clone(),
            summary,
        };
        let path =
            meeting_core::note::write_note(&out_dir().join("real"), &note, &Default::default())
                .unwrap();
        table.push(format!("   note: {}", path.display()));
    }
    eprintln!("\n{}", table.join("\n"));
    assert!(failures.is_empty(), "failures:\n{}", failures.join("\n"));
}
/// Transcribing during the recording (`meeting_core::live`) against turn by
/// turn after it, on every fixture with perfect speakers: the tracks are cut
/// into pieces as the app cuts them live, the pieces transcribed with word
/// times, and the pipeline takes each turn's words from them where it may.
/// The word error rate must not get worse.
#[test]
fn live_pieces_on_all_fixtures() {
    use meeting_core::live::{place_words, CutConfig, Piece, PieceCutter};
    use meeting_core::Transcriber;
    let Some(asr) = transcriber() else { return };
    let mut failures = Vec::new();
    for scenario in [
        "de_two",
        "de_en_mixed",
        "phone_speaker",
        "call_tracks",
        "three_remote",
        "silence",
    ] {
        let Some(fx) = Fixture::load(scenario).unwrap() else {
            eprintln!("SKIP {scenario}: fixture missing");
            continue;
        };
        let oracle = || OracleByTrack {
            mic: Oracle::for_track(&fx.truth, "mic"),
            system: Oracle::for_track(&fx.truth, "system"),
            calls: Default::default(),
            order: fx.tracks.iter().map(|t| t.kind).collect(),
        };
        let cfg = PipelineConfig {
            user_name: "Robin".into(),
            ..Default::default()
        };
        let today = pipeline::run(&fx.tracks, &oracle(), &asr, &cfg).unwrap();
        // Cut small enough that the short fixtures get several pieces.
        let cut = CutConfig {
            min_s: 4.0,
            max_s: 10.0,
        };
        let mut pieces = Vec::new();
        for t in &fx.tracks {
            let mut c = PieceCutter::new(cut);
            let mut ps = c.push(&t.samples);
            ps.extend(c.finish());
            for (at, audio) in ps {
                let start_s = at as f64 / 16_000.0;
                let words = asr.transcribe_words(&audio).unwrap().unwrap_or_default();
                pieces.push(Piece {
                    track: t.kind,
                    start_s,
                    end_s: start_s + audio.len() as f64 / 16_000.0,
                    words: place_words(words, start_s),
                });
            }
        }
        let hooks = pipeline::Hooks {
            pieces: Some(&pieces),
            ..Default::default()
        };
        let live = pipeline::run_with(&fx.tracks, &oracle(), &asr, &cfg, hooks).unwrap();
        let (a, b) = (
            score(&fx.truth, &today.transcript),
            score(&fx.truth, &live.transcript),
        );
        eprintln!(
            "{scenario:14} WER turn by turn {:5.1}%  live {:5.1}%  ({} of {} turns from {} pieces)",
            a.wer * 100.0,
            b.wer * 100.0,
            live.jobs_from_pieces,
            live.jobs,
            pieces.len()
        );
        if b.wer > a.wer + 0.02 {
            failures.push(format!(
                "{scenario}: WER {:.1}% live vs {:.1}% turn by turn",
                b.wer * 100.0,
                a.wer * 100.0
            ));
        }
        if b.speaker_accuracy + 1e-9 < a.speaker_accuracy {
            failures.push(format!("{scenario}: speaker attribution got worse"));
        }
    }
    assert!(failures.is_empty(), "failures:\n{}", failures.join("\n"));
}

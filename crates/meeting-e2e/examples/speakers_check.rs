//! Speaker detection on a real recording, as the app does it: align the
//! tracks, cancel the echo on the mic, diarize,
//! and, unless `--diar-only`, transcribe and name the speakers.
//!   cargo run --release --example speakers_check -- [--diar-only] mic.wav [system.wav mic_lead_ms]
//! `mic_lead_ms`: how much later the mic started than the system audio.
//! Env: NO_AEC=1 skips echo cancellation, MERGE=0.75 turns on merging of
//! similar voices (off in the app).
//! `--diar-only` prints per track each voice's speech time, its similarity
//! to the saved voice profiles and to the other voices on the track.
use meeting_core::pipeline::{self, Hooks, PipelineConfig};
use meeting_core::{Diarizer, Track, TrackKind};
use meeting_e2e::*;
use std::collections::HashMap;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let diar_only = args.first().is_some_and(|a| a == "--diar-only");
    if diar_only {
        args.remove(0);
    }
    transcribe_cpp::init_backends_default().map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut tracks = vec![Track {
        kind: TrackKind::Mic,
        samples: read_wav_16k_mono(Path::new(&args[0]))?,
    }];
    if args.len() >= 3 {
        let lead_ms: f64 = args[2].parse()?;
        let pad = (lead_ms * 16.0) as usize;
        let mut mic = vec![0.0f32; pad];
        mic.extend_from_slice(&tracks[0].samples);
        tracks[0].samples = mic;
        tracks.push(Track {
            kind: TrackKind::System,
            samples: read_wav_16k_mono(Path::new(&args[1]))?,
        });
    }
    if std::env::var_os("NO_AEC").is_none() {
        let t0 = std::time::Instant::now();
        let ran = meeting_core::aec::cancel_echo_in_tracks(&mut tracks);
        eprintln!(
            "echo cancellation: {ran} ({:.1}s)",
            t0.elapsed().as_secs_f64()
        );
    }
    let home = std::env::var("HOME")?;
    let models = Path::new(&home).join("Library/Caches/handy-meetings/diarize-models");
    let mut cfg = diarize::DiarizeConfig::new(&models);
    if let Ok(m) = std::env::var("MERGE") {
        cfg.merge_similarity = Some(m.parse()?);
    }
    let diarizer = diarize::SortformerDiarizer::new(cfg)?;
    let profiles = diarize::VoiceProfiles::open(
        Path::new(&home)
            .join("Library/Application Support/dev.handymeetings.app/voice-profiles.json"),
    )
    .ok();

    if diar_only {
        for t in &tracks {
            let turns = diarizer.diarize(&t.samples)?;
            let mut talk: HashMap<String, f64> = HashMap::new();
            let mut emb: HashMap<String, Vec<f32>> = HashMap::new();
            for x in &turns {
                *talk.entry(x.speaker.clone()).or_default() += x.end_s - x.start_s;
                if let Some(e) = &x.embedding {
                    emb.entry(x.speaker.clone()).or_insert_with(|| e.clone());
                }
            }
            let mut names: Vec<&String> = talk.keys().collect();
            names.sort();
            for n in &names {
                let mut line = format!("{:?} {n}: {:5.0}s", t.kind, talk[*n]);
                if let Some(e) = emb.get(*n) {
                    if let Some((who, sim)) = profiles
                        .as_ref()
                        .and_then(|p| p.scores(e).into_iter().next())
                    {
                        line += &format!("  profile {who} {sim:.2}");
                    }
                    for m in &names {
                        if m != n {
                            if let Some(f) = emb.get(*m) {
                                line += &format!("  vs {m} {:.2}", diarize::cosine(e, f));
                            }
                        }
                    }
                }
                println!("{line}");
            }
        }
        return Ok(());
    }

    let asr = EngineTranscriber::parakeet()?.expect("parakeet model");
    let namer = |_: TrackKind, _: &str, e: Option<&[f32]>| -> Option<String> {
        profiles.as_ref()?.identify(e?).map(|(n, _)| n)
    };
    let r = pipeline::run_with(
        &tracks,
        &diarizer,
        &asr,
        &PipelineConfig::default(),
        Hooks {
            namer: Some(&namer),
            progress: None,
        },
    )?;
    println!("{} speakers:", r.speakers.len());
    for s in &r.speakers {
        println!(
            "  {:?} {} ({}): {:.0}s",
            s.track, s.name, s.cluster, s.speech_s
        );
    }
    Ok(())
}

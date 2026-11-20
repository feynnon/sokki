//! Transcript of a real recording, as the app's pipeline makes it:
//! diarize → echo filter → transcribe turns → dedup.
//!   cargo run --release --example raw_transcript -- mic.wav [system.wav] out.json
use meeting_core::pipeline::{self, PipelineConfig};
use meeting_core::{Track, TrackKind};
use meeting_e2e::*;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (inputs, out) = args.split_at(args.len() - 1);
    let mut tracks = Vec::new();
    for (p, kind) in inputs.iter().zip([TrackKind::Mic, TrackKind::System]) {
        tracks.push(Track {
            kind,
            samples: read_wav_16k_mono(Path::new(p))?,
        });
    }
    let models =
        Path::new(&std::env::var("HOME")?).join("Library/Caches/handy-meetings/diarize-models");
    let diarizer = diarize::SortformerDiarizer::new(diarize::DiarizeConfig::new(&models))?;
    let asr = EngineTranscriber::from_env()?.expect("speech model (MEETING_ASR_MODEL)");
    let mut cfg = PipelineConfig {
        user_name: "Robin".into(),
        ..Default::default()
    };
    if let Some(w) = window_from_env() {
        cfg.window_s = w;
    }
    let r = pipeline::run(&tracks, &diarizer, &asr, &cfg)?;
    drop(diarizer);
    std::fs::write(&out[0], serde_json::to_string_pretty(&r.transcript)?)?;
    eprintln!(
        "{} lines, warnings: {:?}",
        r.transcript.segments.len(),
        r.warnings
    );
    Ok(())
}

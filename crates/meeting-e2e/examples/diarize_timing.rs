//! Where speaker detection spends its time on one track: Sortformer per
//! operating point, and the voice embeddings on top. Prints numbers only.
//!   cargo run --release --example diarize_timing -- track.wav [seconds]
//! `seconds` takes only the start of the track (to see how time grows).
use meeting_core::Diarizer;
use std::path::Path;
use std::time::Instant;
use transcribe_cpp::SortformerPreset;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    transcribe_cpp::init_backends_default().map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut samples = meeting_e2e::read_wav_16k_mono(Path::new(&args[0]))?;
    if let Some(s) = args.get(1) {
        samples.truncate((s.parse::<f64>()? * 16_000.0) as usize);
    }
    let minutes = samples.len() as f64 / 16_000.0 / 60.0;
    let models =
        Path::new(&std::env::var("HOME")?).join("Library/Caches/handy-meetings/diarize-models");
    let presets = [
        ("very_high", Some(SortformerPreset::VeryHighLatency)),
        ("high", Some(SortformerPreset::HighLatency)),
        ("low", Some(SortformerPreset::LowLatency)),
    ];
    for (name, preset) in presets {
        if name == "low" && std::env::var_os("SKIP_LOW").is_some() {
            continue;
        }
        let mut cfg = diarize::DiarizeConfig::new(&models);
        cfg.speaker_embeddings = false;
        let d = diarize::SortformerDiarizer::new(cfg)?.with_preset(preset);
        let t0 = Instant::now();
        let turns = d.diarize(&samples)?;
        let mut speakers: Vec<&str> = turns.iter().map(|t| t.speaker.as_str()).collect();
        speakers.sort();
        speakers.dedup();
        println!(
            "{minutes:.1} min  sortformer {name:9} {:6.1} s  {} turns, {} speakers",
            t0.elapsed().as_secs_f64(),
            turns.len(),
            speakers.len()
        );
    }
    let d = diarize::SortformerDiarizer::new(diarize::DiarizeConfig::new(&models))?;
    let t0 = Instant::now();
    d.diarize(&samples)?;
    println!(
        "{minutes:.1} min  very_high + embeddings {:6.1} s",
        t0.elapsed().as_secs_f64()
    );
    Ok(())
}

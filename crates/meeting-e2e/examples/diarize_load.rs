//! Load time vs run time of the speaker models on one stretch of audio, as
//! the diarization worker does it for each window (fresh process: backends,
//! models, run). Prints numbers only.
//!   cargo run --release --example diarize_load -- track.wav [from_s to_s]
use meeting_core::Diarizer;
use std::path::Path;
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let t0 = Instant::now();
    transcribe_cpp::init_backends_default().map_err(|e| anyhow::anyhow!("{e}"))?;
    let backends = t0.elapsed().as_secs_f64();
    let mut samples = meeting_e2e::read_wav_16k_mono(Path::new(&args[0]))?;
    if let (Some(a), Some(b)) = (args.get(1), args.get(2)) {
        let (a, b) = ((a.parse::<f64>()? * 16_000.0) as usize, (b.parse::<f64>()? * 16_000.0) as usize);
        samples = samples[a.min(samples.len())..b.min(samples.len())].to_vec();
    }
    let models =
        Path::new(&std::env::var("HOME")?).join("Library/Caches/handy-meetings/diarize-models");
    let t1 = Instant::now();
    let mut cfg = diarize::DiarizeConfig::new(&models);
    cfg.speaker_embeddings = std::env::var_os("NO_EMB").is_none();
    let d = diarize::SortformerDiarizer::new(cfg)?;
    let load = t1.elapsed().as_secs_f64();
    for k in 0..2 {
        let t2 = Instant::now();
        d.diarize(&samples)?;
        println!(
            "{:.0} s audio: backends {backends:.2} s, models {load:.2} s, run #{} {:.2} s",
            samples.len() as f64 / 16_000.0,
            k + 1,
            t2.elapsed().as_secs_f64()
        );
    }
    Ok(())
}

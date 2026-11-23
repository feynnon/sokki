//! Diarizes one WAV and writes the turns as JSON; `--cpu` registers only the
//! CPU backend (as the app process does).
//!   cargo run --release --example diarize_once -- [--cpu] in.wav out.json
use meeting_core::Diarizer;
use std::path::Path;
fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let cpu = args.first().is_some_and(|a| a == "--cpu");
    if cpu {
        args.remove(0);
        transcribe_cpp::init_backends_with(None::<&Path>, transcribe_cpp::BackendMask::CPU)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
    } else {
        transcribe_cpp::init_backends_default().map_err(|e| anyhow::anyhow!("{e}"))?;
    }
    let samples = meeting_e2e::read_wav_16k_mono(Path::new(&args[0]))?;
    let models =
        Path::new(&std::env::var("HOME")?).join("Library/Caches/sokki/diarize-models");
    let d = diarize::SortformerDiarizer::new(diarize::DiarizeConfig::new(&models))?;
    let t0 = std::time::Instant::now();
    let turns = d.diarize(&samples)?;
    eprintln!(
        "{} turns in {:.1}s ({})",
        turns.len(),
        t0.elapsed().as_secs_f64(),
        if cpu { "CPU" } else { "GPU" }
    );
    std::fs::write(&args[1], serde_json::to_string(&turns)?)?;
    Ok(())
}

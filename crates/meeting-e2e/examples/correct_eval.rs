//! Runs the app's transcript correction on the real-call eval data and writes
//! the result next to it, for `scripts/correction_eval.py --score-only`:
//!   cargo run --release --example correct_eval -- [model]
//! CORRECT_IN / CORRECT_OUT pick the files in each call folder (default
//! raw.json → out_rust.json); CORRECT_CALLS limits the calls ("call1402").
use meeting_core::correct::{correct_transcript, CorrectConfig};
use meeting_core::summary::{LlmConfig, LlmSummarizer};
use meeting_core::Transcript;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let model = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "qwen3.5:9b".into());
    let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/real");
    let cfg_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(real.join("config.json"))?)?;
    let mut dictionary: Vec<String> = Vec::new();
    for key in ["glossary", "names"] {
        for v in cfg_json[key].as_array().into_iter().flatten() {
            dictionary.push(v.as_str().unwrap_or_default().to_string());
        }
    }
    // Unset: the model's own default, as in the app.
    let temperature: Option<f32> = std::env::var("CORRECT_TEMP")
        .ok()
        .and_then(|t| t.parse().ok());
    let llm = LlmSummarizer::new(LlmConfig {
        temperature,
        max_output_tokens: 400,
        ..LlmConfig::ollama(&model)
    });
    eprintln!("temperature {temperature:?}");
    let mut cfg = CorrectConfig {
        glossary: dictionary,
        ..Default::default()
    };
    if let Some(w) = std::env::var("CORRECT_WINDOW")
        .ok()
        .and_then(|w| w.parse().ok())
    {
        cfg.window = w;
    }
    eprintln!("window {}", cfg.window);
    let input = std::env::var("CORRECT_IN").unwrap_or_else(|_| "raw.json".into());
    let output = std::env::var("CORRECT_OUT").unwrap_or_else(|_| "out_rust.json".into());
    let calls = std::env::var("CORRECT_CALLS").unwrap_or_else(|_| "call1402,call1426".into());
    for call in calls.split(',') {
        let dir = real.join(call);
        let raw: Transcript = serde_json::from_str(&std::fs::read_to_string(dir.join(&input))?)?;
        let t0 = std::time::Instant::now();
        let (out, applied, warnings) = correct_transcript(&llm, &raw, &cfg);
        eprintln!(
            "{call}: {} corrections in {:.0}s, warnings {warnings:?}",
            applied.len(),
            t0.elapsed().as_secs_f64()
        );
        for c in &applied {
            eprintln!("   [{}] {:?} -> {:?}", c.segment, c.from, c.to);
        }
        std::fs::write(dir.join(&output), serde_json::to_string_pretty(&out)?)?;
    }
    Ok(())
}

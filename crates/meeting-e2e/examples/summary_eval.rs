//! Summarises the real-call eval transcripts with the app's summariser:
//!   cargo run --release --example summary_eval -- [model] [file in each call folder]
use meeting_core::summary::{LlmConfig, LlmSummarizer};
use meeting_core::{Summarizer, Transcript};
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let model = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "qwen3.5:9b".into());
    let file = std::env::args().nth(2).unwrap_or_else(|| "raw.json".into());
    let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/real");
    let mut cfg = LlmConfig {
        user_name: Some("Robin".into()),
        ..LlmConfig::ollama(&model)
    };
    if let Some(t) = std::env::var("SUMMARY_CHUNK_TOKENS")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        cfg.map_chunk_tokens = t;
    }
    eprintln!("map_chunk_tokens {}", cfg.map_chunk_tokens);
    let llm = LlmSummarizer::new(cfg);
    let calls = std::env::var("SUMMARY_CALLS").unwrap_or_else(|_| "call1402,call1426".into());
    for call in calls.split(',') {
        let t: Transcript =
            serde_json::from_str(&std::fs::read_to_string(real.join(call).join(&file))?)?;
        let t0 = std::time::Instant::now();
        let s = llm.summarize(&t)?;
        println!(
            "=== {call} ({:.0}s, {} tokens)\nTITLE: {}\nSUMMARY:\n{}\nDECISIONS:\n- {}\nTASKS:",
            t0.elapsed().as_secs_f64(),
            meeting_core::lang::estimate_tokens(&t.to_plain_text()),
            s.title,
            s.summary,
            s.decisions.join("\n- ")
        );
        for a in &s.action_items {
            println!("- {}", serde_json::to_string(a)?);
        }
    }
    Ok(())
}

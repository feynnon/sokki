//! Times one summary of a note's transcript with a given provider, for
//! comparing the built-in model with Ollama. Prints numbers only; the
//! summary itself goes to OUT (keep it out of the repository).
//!
//! cargo run --release --example summary_bench -- NOTE.md builtin|local OUT.json
//! For "builtin": SOKKI_LLAMA_SERVER=… and SOKKI_MODELS_DIR=… (the folder
//! holding llm/Qwen3.5-9B-Q4_K_M.gguf).

use std::time::Instant;

use meeting_core::summary::LlmSummarizer;
use sokki_engine::llm::{self, LlmSettings};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (note, provider, out) = (&args[1], &args[2], &args[3]);
    llm::set_builtin_paths(llm::BuiltinPaths {
        server: None,
        models_dir: std::env::var_os("SOKKI_MODELS_DIR").map(Into::into),
    });
    let md = std::fs::read_to_string(note)?;
    let transcript = meeting_core::note::transcript_from_note(&md);
    let mut settings = LlmSettings {
        provider: provider.clone(),
        ..Default::default()
    };
    // SOKKI_BASE_URL: a server already running (e.g. llama-server by hand).
    if let Ok(url) = std::env::var("SOKKI_BASE_URL") {
        settings.base_url = url;
    }
    let t0 = Instant::now();
    let model = llm::open(&settings, "Ich")?;
    let opened = t0.elapsed();
    let (summary, stats) = LlmSummarizer::new(model.config()).summarize_with_stats(&transcript)?;
    let summarized = t0.elapsed();
    drop(model);
    std::fs::write(out, serde_json::to_string_pretty(&summary)?)?;
    println!(
        "provider={provider} segments={} est_tokens={} open_s={:.1} total_s={:.1} \
         requests={} chunks={} map_reduce={} parse_retries={} \
         summary_chars={} decisions={} action_items={} stopped_s={:.1}",
        transcript.segments.len(),
        stats.estimated_tokens,
        opened.as_secs_f64(),
        summarized.as_secs_f64(),
        stats.requests,
        stats.chunks,
        stats.map_reduce,
        stats.parse_retries,
        summary.summary.chars().count(),
        summary.decisions.len(),
        summary.action_items.len(),
        t0.elapsed().as_secs_f64(),
    );
    Ok(())
}

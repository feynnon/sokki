//! One summary of a note's transcript, exactly as Sokki's built-in provider
//! configures it (OpenAI-compatible, 16k context, no sampling sent, glossary
//! and user name from the app settings), against a llama-server at BASE_URL.
//! sumbench NOTE.md BASE_URL OUT.json SETTINGS.json
use meeting_core::summary::{LlmConfig, LlmSummarizer};
use std::time::Instant;
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let md = std::fs::read_to_string(&a[1])?;
    let t = meeting_core::note::transcript_from_note(&md);
    let s: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&a[4])?)?;
    let m = &s["settings"]["meetings"];
    let mut cfg = LlmConfig::openai_compatible(a[2].clone(), "bench", None);
    cfg.context_tokens = 16_384;
    cfg.keep_alive = None;
    cfg.user_name = m["user_name"].as_str().map(String::from);
    cfg.glossary = m["glossary"].as_array().map(|v| v.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    let llm = LlmSummarizer::new(cfg);
    let t0 = Instant::now();
    let (sum, st) = llm.summarize_with_stats(&t)?;
    let secs = t0.elapsed().as_secs_f64();
    std::fs::write(&a[3], serde_json::to_string_pretty(&sum)?)?;
    println!("{{\"segments\":{},\"est_tokens\":{},\"secs\":{:.1},\"requests\":{},\"chunks\":{},\"decisions\":{},\"tasks\":{},\"summary_chars\":{}}}",
        t.segments.len(), st.estimated_tokens, secs, st.requests, st.chunks, sum.decisions.len(), sum.action_items.len(), sum.summary.len());
    Ok(())
}

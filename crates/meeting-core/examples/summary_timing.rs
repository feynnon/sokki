//! Times the summary of one meeting note with the app's local settings,
//! once per single-pass limit, to compare one request with parts:
//!   cargo run --release --example summary_timing -- NOTE.md [model] [limits]
//! `limits`: comma-separated single-pass limits in tokens (default
//! "100000,6000": the whole meeting in one request, then in parts).
//! Prints only sizes and times, not the meeting's content.
use meeting_core::summary::{LlmConfig, LlmSummarizer};
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let note = args.next().expect("path to a meeting note");
    let model = args.next().unwrap_or_else(|| "qwen3.5:9b".into());
    let limits = args.next().unwrap_or_else(|| "100000,6000".into());
    let md = std::fs::read_to_string(&note)?;
    let t = meeting_core::note::transcript_from_note(&md);
    let tokens = meeting_core::lang::estimate_tokens(&t.to_plain_text());
    println!("{} segments, ~{tokens} tokens", t.segments.len());
    for limit in limits.split(',') {
        let cfg = LlmConfig {
            single_pass_tokens: limit.trim().parse()?,
            ..LlmConfig::ollama(&model)
        };
        let timeout = cfg.timeout.as_secs();
        let llm = LlmSummarizer::new(cfg);
        let t0 = Instant::now();
        match llm.summarize_with_stats(&t) {
            Ok((s, st)) => println!(
                "limit {limit}: {:.1} s, {} parts, {} requests, summary {} lines, {} tasks",
                t0.elapsed().as_secs_f64(),
                st.chunks,
                st.requests,
                s.summary.lines().count(),
                s.action_items.len()
            ),
            Err(e) => println!(
                "limit {limit}: failed after {:.1} s (timeout {timeout} s per request): {e:#}",
                t0.elapsed().as_secs_f64()
            ),
        }
    }
    Ok(())
}

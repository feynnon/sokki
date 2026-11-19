//! `sokki transcribe` and `sokki ask`: Sokki's speech model and language
//! model for scripts and local agents, the same two commands as the Mac's
//! `scripts/sokki`. Neither needs the service running.

use anyhow::{bail, Context, Result};
use std::io::{IsTerminal, Read};
use std::process::{Command, Stdio};
use std::time::Instant;

use crate::config::Config;

/// Any audio or video file to text: ffmpeg to 16 kHz mono, then Parakeet.
/// `json` prints `{"text", "audio_secs", "model", "load_ms", ...}` like the
/// Mac command.
pub fn transcribe(file: &str, json: bool) -> Result<()> {
    if !std::path::Path::new(file).is_file() {
        bail!("no such file: {file}");
    }
    let out = Command::new("ffmpeg")
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i", file])
        .args(["-vn", "-ac", "1", "-ar", "16000", "-f", "f32le", "-"])
        .stdin(Stdio::null())
        .output()
        .context("needs ffmpeg (sudo pacman -S ffmpeg)")?;
    if !out.status.success() {
        bail!(
            "could not read audio from {file}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let samples = crate::audio::read_f32(&out.stdout[..]);
    let audio_secs = samples.len() as f64 / 16_000.0;

    let model = &crate::models::PARAKEET;
    if !crate::models::present(model) {
        bail!("the speech model is not downloaded yet: sokki models install");
    }
    let cfg = Config::load(&crate::paths::config_file())?;
    let t = Instant::now();
    let mut transcriber = sokki_engine::asr::LocalTranscriber::load(&crate::models::path(model))?;
    let lang = cfg.dictation.language.trim();
    if !lang.is_empty() {
        transcriber.set_language(Some(lang.to_string()));
    }
    let load_ms = t.elapsed().as_millis() as u64;
    let t = Instant::now();
    let text = transcriber.transcribe_long(&samples)?;
    let ms = t.elapsed().as_millis() as u64;

    if json {
        let model_id = model.path.trim_end_matches(".gguf");
        println!(
            "{}",
            serde_json::json!({
                "model": model_id,
                "audio_secs": audio_secs,
                "load_ms": load_ms,
                "transcribe_ms": [ms],
                "best_ms": ms,
                "text": text,
            })
        );
    } else {
        println!("{text}");
    }
    Ok(())
}

/// Asks the language model Sokki uses for summaries. The prompt is the
/// arguments, with stdin appended after a blank line. Same call as the
/// summaries: Ollama's /api/chat (thinking off unless `think`, the model's
/// own temperature), or /chat/completions for a base URL ending in /v1.
pub fn ask(words: &[&str], think: bool) -> Result<()> {
    let mut prompt = words.join(" ");
    if !std::io::stdin().is_terminal() {
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input)?;
        if !input.trim().is_empty() {
            prompt = if prompt.is_empty() {
                input
            } else {
                format!("{prompt}\n\n{input}")
            };
        }
    }
    if prompt.trim().is_empty() {
        bail!("usage: sokki ask PROMPT   (or pipe text in)");
    }
    let cfg = Config::load(&crate::paths::config_file())?;
    let base = cfg.llm.base_url.trim().trim_end_matches('/').to_string();
    let model = std::env::var("SOKKI_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| cfg.llm.model.trim().to_string());
    let messages = serde_json::json!([{ "role": "user", "content": prompt }]);
    let (url, body) = if base.ends_with("/v1") {
        (
            format!("{base}/chat/completions"),
            serde_json::json!({ "model": model, "messages": messages, "stream": false }),
        )
    } else {
        // A local Ollama that is installed but stopped is started first.
        if sokki_engine::llm::check(&cfg.llm, &cfg.meetings.user_name, false).state
            == "not_running"
        {
            let _ = sokki_engine::llm::start_ollama(&cfg.llm);
        }
        (
            format!("{base}/api/chat"),
            serde_json::json!({ "model": model, "messages": messages, "stream": false, "think": think }),
        )
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(600)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut resp = agent
        .post(&url)
        .header("Content-Type", "application/json")
        .send(body.to_string())
        .with_context(|| format!("no language model at {base}. Is Ollama running?"))?;
    let status = resp.status();
    let text = resp.body_mut().read_to_string()?;
    let data: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("unreadable answer from {url}"))?;
    if !status.is_success() {
        let why = data["error"]
            .as_str()
            .map(str::to_string)
            .or_else(|| data["error"]["message"].as_str().map(str::to_string))
            .unwrap_or_else(|| status.to_string());
        bail!("{why}");
    }
    let answer = data["choices"][0]["message"]["content"]
        .as_str()
        .or_else(|| data["message"]["content"].as_str())
        .unwrap_or_default();
    println!("{}", answer.trim());
    Ok(())
}

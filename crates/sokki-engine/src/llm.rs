//! Language-model settings, status, starting Ollama and downloading a model.
//!
//! The model itself is called by `meeting_core::summary` (correction and
//! summary); this module turns the person's settings into that config and
//! answers "is it ready?".

use anyhow::{bail, Context, Result};
use meeting_core::summary::{Backend, LlmConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// Which language model to use, as the person set it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmSettings {
    /// "local", or a cloud provider from [`CLOUD_PROVIDERS`].
    pub provider: String,
    /// Cloud model; empty means [`default_cloud_model`].
    pub cloud_model: String,
    /// Local server: Ollama's address, or an OpenAI-style base ending in `/v1`.
    pub base_url: String,
    /// Local model name.
    pub model: String,
    /// API keys per cloud provider.
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub keys: HashMap<String, String>,
}

impl Default for LlmSettings {
    fn default() -> Self {
        LlmSettings {
            provider: "local".into(),
            cloud_model: String::new(),
            base_url: "http://localhost:11434".into(),
            model: "qwen3.5:9b".into(),
            keys: HashMap::new(),
        }
    }
}

/// Provider ids that can summarise in the cloud, as in Handy's provider list.
pub const CLOUD_PROVIDERS: [&str; 3] = ["anthropic", "openai", "openrouter"];

/// Model used when `cloud_model` is empty.
pub fn default_cloud_model(provider: &str) -> &'static str {
    match provider {
        "anthropic" => "claude-opus-5-5",
        "openai" => "gpt-6.2-sol",
        "openrouter" => "anthropic/claude-opus-5.5",
        _ => "",
    }
}

/// The request config for these settings. `user_name` lets the model say
/// "you" for the person whose notes these are.
pub fn config(s: &LlmSettings, user_name: &str) -> LlmConfig {
    let provider = s.provider.trim();
    if CLOUD_PROVIDERS.contains(&provider) {
        let model = match s.cloud_model.trim() {
            "" => default_cloud_model(provider).to_string(),
            m => m.to_string(),
        };
        let key = s
            .keys
            .get(provider)
            .filter(|k| !k.trim().is_empty())
            .map(|k| k.trim().to_string());
        let mut cfg = match provider {
            "anthropic" => LlmConfig::anthropic(model, key),
            "openai" => LlmConfig::openai_compatible("https://api.openai.com/v1", model, key),
            _ => LlmConfig::openai_compatible("https://openrouter.ai/api/v1", model, key),
        };
        if provider != "anthropic" {
            // Cloud models have large contexts: one request per meeting.
            cfg.context_tokens = 128_000;
            cfg.max_output_tokens = 16_000;
        }
        cfg.timeout = Duration::from_secs(600);
        cfg.user_name = Some(user_name.to_string());
        return cfg;
    }
    let base = s.base_url.trim().trim_end_matches('/').to_string();
    let mut cfg = if is_ollama(&base) {
        LlmConfig {
            base_url: base,
            ..LlmConfig::ollama(s.model.trim())
        }
    } else {
        LlmConfig::openai_compatible(base, s.model.trim(), None)
    };
    cfg.user_name = Some(user_name.to_string());
    cfg
}

/// Ollama's native API, unless the URL looks like an OpenAI-style `/v1` base.
pub fn is_ollama(base: &str) -> bool {
    !base.ends_with("/v1")
}

/// What a settings screen shows about the language model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    /// "ready", "no_key", "no_ollama", "not_running", "model_missing" or
    /// "error".
    pub state: String,
    /// Model name, or the error text for "error".
    pub detail: String,
}

fn status(state: &str, detail: impl Into<String>) -> Status {
    Status {
        state: state.into(),
        detail: detail.into(),
    }
}

/// Where an Ollama install can be found on this platform.
fn ollama_binaries() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "macos") {
        out.push("/Applications/Ollama.app".into());
        out.push("/opt/homebrew/bin/ollama".into());
        out.push("/usr/local/bin/ollama".into());
    }
    if let Some(path) = std::env::var_os("PATH") {
        out.extend(std::env::split_paths(&path).map(|d| d.join("ollama")));
    }
    out
}

fn ollama_installed() -> bool {
    ollama_binaries().iter().any(|b| b.exists())
}

fn quick_agent(secs: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(secs)))
        .build()
        .into()
}

/// Model names Ollama has, or `None` when no server answers.
pub fn ollama_models(base: &str) -> Option<Vec<String>> {
    let body: serde_json::Value = quick_agent(3)
        .get(&format!("{base}/api/tags"))
        .call()
        .ok()?
        .body_mut()
        .read_json()
        .ok()?;
    Some(
        body["models"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|m| m["name"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    )
}

/// The language model's state. `probe` sends a tiny real request to a cloud
/// provider (checks key, model name and network); without it only the
/// presence of a key is checked.
pub fn check(s: &LlmSettings, user_name: &str, probe: bool) -> Status {
    let cfg = config(s, user_name);
    let provider = s.provider.trim();
    if CLOUD_PROVIDERS.contains(&provider) {
        if cfg.api_key.is_none() {
            return status("no_key", &cfg.model);
        }
        if !probe {
            return status("ready", &cfg.model);
        }
        let llm = meeting_core::summary::LlmSummarizer::new(LlmConfig {
            max_output_tokens: 300,
            timeout: Duration::from_secs(60),
            ..cfg.clone()
        });
        return match llm.chat_plain("Answer with the single word OK.", "Ping") {
            Ok(_) => status("ready", &cfg.model),
            Err(e) => status("error", format!("{e:#}")),
        };
    }
    let base = cfg.base_url.trim_end_matches('/').to_string();
    if cfg.backend != Backend::Ollama {
        // A custom OpenAI-compatible local server (set in the config file).
        return match quick_agent(3).get(&format!("{base}/models")).call() {
            Ok(_) => status("ready", &cfg.model),
            Err(e) => status("error", format!("{base}: {e}")),
        };
    }
    match ollama_models(&base) {
        None if !ollama_installed() => status("no_ollama", &cfg.model),
        None => status("not_running", &cfg.model),
        Some(names) => {
            let m = cfg.model.as_str();
            let found = names
                .iter()
                .any(|n| n == m || (!m.contains(':') && n.split(':').next() == Some(m)));
            if found {
                status("ready", m)
            } else {
                status("model_missing", m)
            }
        }
    }
}

/// Starts Ollama (the macOS app, else `ollama serve`) and waits up to 15 s
/// for it to answer.
pub fn start_ollama(s: &LlmSettings) -> Result<()> {
    let base = s.base_url.trim().trim_end_matches('/').to_string();
    if ollama_models(&base).is_some() {
        return Ok(());
    }
    let found = ollama_binaries().into_iter().find(|b| b.exists());
    match found {
        Some(app) if app.extension().is_some_and(|e| e == "app") => {
            std::process::Command::new("open")
                .args(["-g", "-a", "Ollama"])
                .status()
                .context("starting the Ollama app")?;
        }
        Some(bin) => {
            std::process::Command::new(bin)
                .arg("serve")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .context("starting ollama serve")?;
        }
        None => bail!("Ollama is not installed"),
    }
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(500));
        if ollama_models(&base).is_some() {
            return Ok(());
        }
    }
    bail!("Ollama did not start within 15 seconds")
}

/// Progress of a model download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullProgress {
    pub status: String,
    pub completed: u64,
    pub total: u64,
}

/// Downloads the local model through Ollama, reporting progress.
pub fn pull_model(s: &LlmSettings, mut progress: impl FnMut(PullProgress)) -> Result<()> {
    let base = s.base_url.trim().trim_end_matches('/');
    let model = s.model.trim();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(4 * 3600)))
        .build()
        .into();
    let mut resp = agent
        .post(&format!("{base}/api/pull"))
        .send_json(serde_json::json!({ "model": model, "stream": true }))
        .with_context(|| format!("No model server at {base}"))?;
    let reader = std::io::BufReader::new(resp.body_mut().as_reader());
    use std::io::BufRead;
    for line in reader.lines() {
        let line = line.context("reading download progress")?;
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if let Some(e) = v["error"].as_str() {
            bail!("{e}");
        }
        let st = v["status"].as_str().unwrap_or_default().to_string();
        progress(PullProgress {
            status: st.clone(),
            completed: v["completed"].as_u64().unwrap_or(0),
            total: v["total"].as_u64().unwrap_or(0),
        });
        if st == "success" {
            return Ok(());
        }
    }
    bail!("The download ended early")
}

/// Where Ollama runs the model: loads it briefly, then asks Ollama how much
/// of it sits in GPU memory. `Some(true)` on the GPU (at least partly),
/// `Some(false)` all on the CPU, `None` when that can't be told (not Ollama,
/// not running, model missing).
pub fn ollama_on_gpu(s: &LlmSettings) -> Option<bool> {
    let base = s.base_url.trim().trim_end_matches('/');
    if !is_ollama(base) || CLOUD_PROVIDERS.contains(&s.provider.trim()) {
        return None;
    }
    let model = s.model.trim();
    // An empty prompt only loads the model.
    let loaded = quick_agent(180)
        .post(&format!("{base}/api/generate"))
        .send_json(serde_json::json!({ "model": model, "prompt": "", "keep_alive": "2m" }))
        .ok()?;
    drop(loaded);
    let ps: serde_json::Value = quick_agent(5)
        .get(&format!("{base}/api/ps"))
        .call()
        .ok()?
        .body_mut()
        .read_json()
        .ok()?;
    let entry = ps["models"].as_array()?.iter().find(|m| {
        let name = m["name"].as_str().unwrap_or_default();
        name == model || (!model.contains(':') && name.split(':').next() == Some(model))
    })?;
    Some(entry["size_vram"].as_u64().unwrap_or(0) > 0)
}

/// Gets the language model going for a meeting, in the background: starts
/// Ollama when it is installed but not running (the person chose it for
/// summaries, so a meeting that needs it may start it), then loads the
/// model. Join the handle before asking whether the model is ready.
pub fn prepare(s: &LlmSettings, user_name: &str) -> std::thread::JoinHandle<()> {
    let (s, user_name) = (s.clone(), user_name.to_string());
    std::thread::spawn(move || {
        if check(&s, &user_name, false).state == "not_running" {
            match start_ollama(&s) {
                Ok(()) => log::info!("Started Ollama for the meeting"),
                Err(e) => log::warn!("Could not start Ollama: {e:#}"),
            }
        }
        warm_up(&s, &user_name);
    })
}

/// Whether the language model can be used now (no request is sent to a
/// cloud provider). Logs why not.
pub fn is_ready(s: &LlmSettings, user_name: &str) -> bool {
    let st = check(s, user_name, false);
    if st.state != "ready" {
        log::warn!("Language model not ready: {} ({})", st.state, st.detail);
    }
    st.state == "ready"
}

/// Asks Ollama to load the model in the background, with the same context
/// size the requests use (a different `num_ctx` would load it again).
pub fn warm_up(s: &LlmSettings, user_name: &str) {
    let cfg = config(s, user_name);
    if cfg.backend != Backend::Ollama {
        return;
    }
    let url = format!("{}/api/generate", cfg.base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": cfg.model,
        "keep_alive": cfg.keep_alive.clone().unwrap_or_else(|| "1m".into()),
        "options": { "num_ctx": cfg.context_tokens },
    });
    std::thread::spawn(move || {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(120)))
            .build()
            .into();
        match agent.post(&url).send_json(&body) {
            Ok(_) => log::info!("Language model loaded in advance"),
            Err(e) => log::warn!("Loading the language model in advance failed: {e}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_vs_openai_detection() {
        assert!(is_ollama("http://localhost:11434"));
        assert!(!is_ollama("http://localhost:1234/v1"));
        let s = LlmSettings {
            base_url: "http://localhost:11434/".into(),
            ..Default::default()
        };
        let cfg = config(&s, "Robin");
        assert_eq!(cfg.base_url, "http://localhost:11434");
        assert_eq!(cfg.user_name.as_deref(), Some("Robin"));
    }

    #[test]
    fn cloud_provider_uses_its_key_and_default_model() {
        let mut s = LlmSettings {
            keys: [("anthropic".to_string(), " key-a ".to_string())].into(),
            ..Default::default()
        };
        assert_eq!(config(&s, "Ich").backend, Backend::Ollama);
        s.provider = "anthropic".into();
        let cfg = config(&s, "Ich");
        assert_eq!(cfg.backend, Backend::Anthropic);
        assert_eq!(cfg.model, "claude-opus-5-5");
        assert_eq!(cfg.api_key.as_deref(), Some("key-a"));
        s.provider = "openrouter".into();
        s.cloud_model = "google/gemini-3.8-flash".into();
        let cfg = config(&s, "Ich");
        assert_eq!(cfg.base_url, "https://openrouter.ai/api/v1");
        assert_eq!(cfg.model, "google/gemini-3.8-flash");
        assert!(cfg.api_key.is_none());
    }
}

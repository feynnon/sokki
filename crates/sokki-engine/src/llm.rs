//! Language-model settings, status, the built-in model, Ollama, and
//! downloading a model.
//!
//! The model itself is called by `meeting_core::summary`; this module
//! turns the person's settings into that config, answers "is it ready?",
//! and runs the built-in model ([`open`]).
//!
//! The built-in model ("builtin") is one GGUF file from [`BUILTIN_MODEL`],
//! run by llama.cpp's `llama-server` on the GPU (Metal) in its own process:
//! a crash or hang there can't take the app down. It runs only while a
//! summary is written and quits right after, which frees its memory.

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
    /// "builtin" (llama.cpp inside Sokki), "local" (Ollama or another
    /// server at `base_url`), or a cloud provider from [`CLOUD_PROVIDERS`].
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
            provider: DEFAULT_PROVIDER.into(),
            cloud_model: String::new(),
            base_url: "http://localhost:11434".into(),
            model: "qwen3.5:9b".into(),
            keys: HashMap::new(),
        }
    }
}

/// The built-in model where llama.cpp is shipped (Macs with Apple
/// silicon); Ollama elsewhere.
pub const DEFAULT_PROVIDER: &str = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    BUILTIN
} else {
    "local"
};

/// Provider id of the built-in model.
pub const BUILTIN: &str = "builtin";

/// A model file the built-in engine can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinModel {
    /// Model name in requests and logs.
    pub id: &'static str,
    /// File name under the models directory's `llm/` folder.
    pub file: &'static str,
    /// Pinned to one revision, so the checksum stays valid.
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
    /// The model's own sampling defaults, as its publisher gives them, for
    /// llama-server. Needed because the GGUF carries none and llama.cpp's
    /// fallback (temperature 0.8, top-k 40, min-p 0.05) is not the model's.
    /// Requests still send no sampling values.
    pub sampling: &'static [&'static str],
}

/// Qwen 3.5 9B, Q4_K_M: the same model as Ollama's `qwen3.5:9b` (there
/// NVFP4, also about 4.5 bits per weight). Apache-2.0.
pub const BUILTIN_MODEL: BuiltinModel = BuiltinModel {
    id: "qwen3.5-9b-q4_k_m",
    file: "Qwen3.5-9B-Q4_K_M.gguf",
    url: "https://huggingface.co/unsloth/Qwen3.5-9B-GGUF/resolve/3885219b6810b007914f3a7950a8d1b469d598a5/Qwen3.5-9B-Q4_K_M.gguf",
    sha256: "03b74727a860a56338e042c4420bb3f04b2fec5734175f4cb9fa853daf52b7e8",
    size: 5_680_522_464,
    // Qwen's values, the same Ollama ships with qwen3.5:9b (Robin,
    // 2026-11-20: "Nimm Qwens Werte").
    sampling: &[
        "--temp", "1.0", "--top-k", "20", "--top-p", "0.95", "--min-p", "0",
        "--presence-penalty", "1.5", "--repeat-penalty", "1.0",
    ],
};

/// Context the built-in model runs with: the same as Ollama's (`num_ctx`).
const BUILTIN_CONTEXT: usize = 16_384;

/// Where the built-in engine and its model live; set once by the host.
#[derive(Debug, Clone, Default)]
pub struct BuiltinPaths {
    /// The `llama-server` executable.
    pub server: Option<PathBuf>,
    /// Directory for model files (the model goes into its `llm/` folder).
    pub models_dir: Option<PathBuf>,
}

static BUILTIN_PATHS: std::sync::OnceLock<BuiltinPaths> = std::sync::OnceLock::new();

/// Tells the engine where the built-in model's server and files are. Call
/// once at startup; later calls are ignored. `SOKKI_LLAMA_SERVER` overrides
/// the server path (for tests and measurements).
pub fn set_builtin_paths(paths: BuiltinPaths) {
    let _ = BUILTIN_PATHS.set(paths);
}

fn builtin_server() -> Option<PathBuf> {
    std::env::var_os("SOKKI_LLAMA_SERVER")
        .map(PathBuf::from)
        .or_else(|| BUILTIN_PATHS.get().and_then(|p| p.server.clone()))
}

/// Where the built-in model file belongs, whether or not it is there yet.
pub fn builtin_model_path() -> Option<PathBuf> {
    BUILTIN_PATHS
        .get()
        .and_then(|p| p.models_dir.as_ref())
        .map(|d| d.join("llm").join(BUILTIN_MODEL.file))
}

fn builtin_model_present() -> bool {
    builtin_model_path()
        .and_then(|p| std::fs::metadata(p).ok())
        .is_some_and(|m| m.len() == BUILTIN_MODEL.size)
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
    if provider == BUILTIN {
        // The address and key are filled in by [`open`] once the server runs.
        let mut cfg = LlmConfig::openai_compatible("", BUILTIN_MODEL.id, None);
        cfg.context_tokens = BUILTIN_CONTEXT;
        cfg.keep_alive = None;
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
    /// "error". The built-in model is "ready", "model_missing" or "error".
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

/// Whether something accepts connections at the server's address. Tells
/// "not running" (refused) apart from "running but slow": on 20.11.2026
/// Ollama took 3–5 s for `/api/tags` while the Mac was swapping, the 3 s
/// check gave up, and Sokki "started" an Ollama that was running already.
pub fn listening(base: &str) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};
    let rest = base.split("://").nth(1).unwrap_or(base);
    let host = rest.split('/').next().unwrap_or(rest);
    let host = if host
        .rsplit_once(':')
        .is_some_and(|(_, p)| p.parse::<u16>().is_ok())
    {
        host.to_string()
    } else if base.starts_with("https://") {
        format!("{host}:443")
    } else {
        format!("{host}:80")
    };
    let Ok(addrs) = host.to_socket_addrs() else {
        return false;
    };
    addrs
        .into_iter()
        .any(|a| TcpStream::connect_timeout(&a, Duration::from_secs(1)).is_ok())
}

/// How long a check waits for a running Ollama that answers slowly.
const BUSY_PATIENCE: Duration = Duration::from_secs(30);

/// Model names from a running Ollama that may be slow to answer: tries
/// every few seconds for up to `patience`.
fn ollama_models_patiently(base: &str, patience: Duration) -> Option<Vec<String>> {
    let start = std::time::Instant::now();
    loop {
        if let Some(names) = ollama_models(base) {
            return Some(names);
        }
        if start.elapsed() >= patience || !listening(base) {
            return None;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// The language model's state. `probe` sends a tiny real request to a cloud
/// provider (checks key, model name and network); without it only the
/// presence of a key is checked.
pub fn check(s: &LlmSettings, user_name: &str, probe: bool) -> Status {
    check_with(s, user_name, probe, BUSY_PATIENCE)
}

fn check_with(s: &LlmSettings, user_name: &str, probe: bool, patience: Duration) -> Status {
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
    if provider == BUILTIN {
        return match builtin_server() {
            Some(server) if server.exists() => {
                if builtin_model_present() {
                    status("ready", BUILTIN_MODEL.id)
                } else {
                    status("model_missing", BUILTIN_MODEL.id)
                }
            }
            _ => status(
                "error",
                "This build of Sokki has no built-in language model.",
            ),
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
    let models = ollama_models(&base).or_else(|| {
        listening(&base)
            .then(|| ollama_models_patiently(&base, patience))
            .flatten()
    });
    match models {
        None if listening(&base) => status("error", format!("{base} is not answering")),
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
    if listening(&base) {
        // Running, only slow: a second `ollama serve` couldn't bind the port.
        return match ollama_models_patiently(&base, BUSY_PATIENCE) {
            Some(_) => Ok(()),
            None => bail!("Ollama is running but not answering"),
        };
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

/// Downloads the local model (the built-in one, or through Ollama),
/// reporting progress.
pub fn pull_model(s: &LlmSettings, mut progress: impl FnMut(PullProgress)) -> Result<()> {
    if s.provider.trim() == BUILTIN {
        let dest = builtin_model_path().context("No folder for the model")?;
        return download_builtin(&dest, &mut progress);
    }
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

/// Gets the language model going for a meeting, in the background: loads
/// Ollama's model when Ollama runs. It never starts Ollama (only the person
/// does, in Settings), and the built-in model loads only for the summary
/// ([`open`]). Join the handle before asking whether the model is ready.
pub fn prepare(s: &LlmSettings, user_name: &str) -> std::thread::JoinHandle<()> {
    let (s, user_name) = (s.clone(), user_name.to_string());
    std::thread::spawn(move || {
        if s.provider.trim() != BUILTIN {
            warm_up(&s, &user_name);
        }
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

/// The language model for one job (a summary): its request config, and for
/// the built-in model the running server. Dropping it stops the server,
/// which frees the model's memory.
pub struct Llm {
    cfg: LlmConfig,
    _server: Option<Server>,
}

impl Llm {
    /// The request config, pointing at the running server.
    pub fn config(&self) -> LlmConfig {
        self.cfg.clone()
    }
}

/// Gets the language model ready for one job. The built-in model starts its
/// server and loads (a few seconds); every other provider is used as set.
pub fn open(s: &LlmSettings, user_name: &str) -> Result<Llm> {
    let mut cfg = config(s, user_name);
    if s.provider.trim() != BUILTIN {
        return Ok(Llm { cfg, _server: None });
    }
    let bin = builtin_server().context("This build of Sokki has no built-in language model")?;
    let model = builtin_model_path()
        .filter(|_| builtin_model_present())
        .context("The language model is not downloaded")?;
    let server = Server::start(&bin, &model, cfg.context_tokens)?;
    cfg.base_url = format!("http://127.0.0.1:{}/v1", server.port);
    cfg.api_key = Some(server.key.clone());
    Ok(Llm {
        cfg,
        _server: Some(server),
    })
}

/// How long loading the built-in model may take.
const SERVER_START_TIMEOUT: Duration = Duration::from_secs(180);

/// Starts the server and stops it with the parent: when Sokki's end of
/// stdin closes (Sokki exits, crashes, or drops [`Server`]), the shell
/// stops llama-server, so no model is left holding memory.
const WATCHDOG: &str = r#"exec 3<&0
"$@" </dev/null 3<&- &
pid=$!
(read _ <&3; kill $pid 2>/dev/null) &
w=$!
wait $pid
s=$?
kill $w 2>/dev/null
exit $s"#;

/// A running llama-server with the built-in model.
struct Server {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    port: u16,
    key: String,
}

impl Server {
    fn start(bin: &std::path::Path, model: &std::path::Path, context: usize) -> Result<Server> {
        use std::process::{Command, Stdio};
        let port = std::net::TcpListener::bind("127.0.0.1:0")?
            .local_addr()?
            .port();
        let key = random_key();
        let t0 = std::time::Instant::now();
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c")
            .arg(WATCHDOG)
            .arg("sokki-llm")
            .arg(bin)
            .arg("--model")
            .arg(model)
            .args(["--host", "127.0.0.1", "--port", &port.to_string()])
            .args(["--api-key", &key])
            // One request at a time, with the context Ollama used.
            .args(["--ctx-size", &context.to_string(), "--parallel", "1"])
            // The summary asks for JSON directly; Qwen 3.5 would think first.
            .args(["--jinja", "--reasoning", "off"])
            // The model's own sampling defaults (see `BuiltinModel::sampling`).
            .args(BUILTIN_MODEL.sampling)
            // No prompt cache in RAM (default 8 GB), no chat page.
            .args(["--cache-ram", "0", "--no-ui"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let mut child = cmd
            .spawn()
            .with_context(|| format!("starting {}", bin.display()))?;
        let stdin = child.stdin.take();
        let tail = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
        if let Some(err) = child.stderr.take() {
            let tail = tail.clone();
            std::thread::spawn(move || {
                use std::io::BufRead;
                for line in std::io::BufReader::new(err).lines().map_while(|l| l.ok()) {
                    log::debug!("llama-server: {line}");
                    let mut t = tail.lock().unwrap_or_else(|e| e.into_inner());
                    if t.len() == 20 {
                        t.pop_front();
                    }
                    t.push_back(line);
                }
            });
        }
        let mut server = Server {
            child,
            stdin,
            port,
            key,
        };
        let health = format!("http://127.0.0.1:{port}/health");
        let agent = quick_agent(2);
        loop {
            if let Ok(Some(st)) = server.child.try_wait() {
                std::thread::sleep(Duration::from_millis(200));
                let t = tail.lock().unwrap_or_else(|e| e.into_inner());
                let log = t.iter().cloned().collect::<Vec<_>>().join("\n");
                bail!("The language model stopped while loading ({st}):\n{log}");
            }
            if let Ok(r) = agent.get(&health).call() {
                if r.status().as_u16() == 200 {
                    break;
                }
            }
            if t0.elapsed() > SERVER_START_TIMEOUT {
                bail!(
                    "The language model did not load within {} s",
                    SERVER_START_TIMEOUT.as_secs()
                );
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        log::info!(
            "Built-in language model loaded in {:.1} s",
            t0.elapsed().as_secs_f64()
        );
        Ok(server)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // Closing stdin makes the watchdog stop llama-server.
        drop(self.stdin.take());
        for _ in 0..50 {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                log::info!("Built-in language model stopped");
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        // Still there after 5 s: kill the whole group.
        let _ = std::process::Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{}", self.child.id())])
            .status();
        let _ = self.child.wait();
        log::warn!("Built-in language model killed after it did not stop");
    }
}

/// A key only Sokki knows, so no other program on this computer can use
/// the server while it runs.
fn random_key() -> String {
    use std::hash::{BuildHasher, Hasher};
    (0..2)
        .map(|i| {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u64(i);
            h.write_u128(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos(),
            );
            format!("{:016x}", h.finish())
        })
        .collect()
}

/// Downloads the built-in model to `dest`, resuming a partial download and
/// checking its SHA-256 before it is used.
fn download_builtin(dest: &std::path::Path, progress: &mut dyn FnMut(PullProgress)) -> Result<()> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    let m = BUILTIN_MODEL;
    if std::fs::metadata(dest).is_ok_and(|d| d.len() == m.size) {
        return Ok(());
    }
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let part = dest.with_extension("gguf.part");
    let have = std::fs::metadata(&part).map(|d| d.len()).unwrap_or(0);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(Duration::from_secs(60)))
        .build()
        .into();
    let mut req = agent.get(m.url);
    if have > 0 && have < m.size {
        req = req.header("Range", &format!("bytes={have}-"));
    }
    let mut resp = req
        .call()
        .with_context(|| format!("downloading {}", m.url))?;
    let resumed = resp.status().as_u16() == 206;
    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resumed)
        .truncate(!resumed)
        .open(&part)
        .with_context(|| format!("creating {}", part.display()))?;
    let mut done = if resumed { have } else { 0 };
    let mut reader = resp.body_mut().with_config().limit(m.size).reader();
    let mut buf = vec![0u8; 1 << 20];
    let mut last = std::time::Instant::now();
    loop {
        let n = reader.read(&mut buf).context("downloading the model")?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])
            .with_context(|| format!("writing {}", part.display()))?;
        done += n as u64;
        if last.elapsed() >= Duration::from_millis(300) {
            last = std::time::Instant::now();
            progress(PullProgress {
                status: "downloading".into(),
                completed: done,
                total: m.size,
            });
        }
    }
    out.flush()?;
    drop(out);
    if done != m.size {
        bail!("The download ended early");
    }
    progress(PullProgress {
        status: "verifying".into(),
        completed: done,
        total: m.size,
    });
    let mut f = std::fs::File::open(&part)?;
    let mut hasher = Sha256::new();
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let got: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if got != m.sha256 {
        let _ = std::fs::remove_file(&part);
        bail!("The downloaded model is damaged; please download it again");
    }
    std::fs::rename(&part, dest).with_context(|| format!("saving {}", dest.display()))?;
    progress(PullProgress {
        status: "success".into(),
        completed: m.size,
        total: m.size,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_vs_openai_detection() {
        assert!(is_ollama("http://localhost:11434"));
        assert!(!is_ollama("http://localhost:1234/v1"));
        let s = LlmSettings {
            provider: "local".into(),
            base_url: "http://localhost:11434/".into(),
            ..Default::default()
        };
        let cfg = config(&s, "Robin");
        assert_eq!(cfg.base_url, "http://localhost:11434");
        assert_eq!(cfg.user_name.as_deref(), Some("Robin"));
    }

    #[test]
    fn a_closed_port_is_not_listening_an_open_one_is() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        assert!(listening(&format!("http://localhost:{port}")));
        assert!(listening(&format!("http://127.0.0.1:{port}/v1")));
        drop(l);
        assert!(!listening(&format!("http://127.0.0.1:{port}")));
    }

    /// 20.11.2026: Ollama ran but took >3 s to list its models; the check
    /// said "not_running" and a second Ollama was started.
    #[test]
    fn a_running_but_slow_ollama_is_not_reported_as_not_running() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        // Connections wait in the listen queue: accepted, never answered.
        let s = LlmSettings {
            provider: "local".into(),
            base_url: format!("http://127.0.0.1:{port}"),
            ..Default::default()
        };
        let st = check_with(&s, "Ich", false, Duration::from_secs(1));
        assert_eq!(st.state, "error", "{st:?}");
        drop(l);
    }

    #[test]
    fn nothing_at_the_address_is_not_running_or_not_installed() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        let s = LlmSettings {
            provider: "local".into(),
            base_url: format!("http://127.0.0.1:{port}"),
            ..Default::default()
        };
        let st = check_with(&s, "Ich", false, Duration::from_secs(1));
        assert!(
            st.state == "not_running" || st.state == "no_ollama",
            "{st:?}"
        );
    }

    #[test]
    fn builtin_model_is_called_like_an_openai_server_without_temperature() {
        let s = LlmSettings {
            provider: BUILTIN.into(),
            ..Default::default()
        };
        let cfg = config(&s, "Robin");
        assert_eq!(cfg.backend, Backend::OpenAiCompatible);
        assert_eq!(cfg.model, BUILTIN_MODEL.id);
        assert_eq!(cfg.context_tokens, 16_384);
        assert_eq!(cfg.temperature, None);
        assert_eq!(cfg.user_name.as_deref(), Some("Robin"));
        assert!(BUILTIN_MODEL.url.contains(BUILTIN_MODEL.file));
        assert_eq!(BUILTIN_MODEL.sha256.len(), 64);
        // Qwen's own defaults, as in Ollama: temperature 1.0, top-k 20.
        let arg = |name: &str| {
            let i = BUILTIN_MODEL
                .sampling
                .iter()
                .position(|a| *a == name)
                .unwrap();
            BUILTIN_MODEL.sampling[i + 1]
        };
        assert_eq!(arg("--temp"), "1.0");
        assert_eq!(arg("--top-k"), "20");
        assert_eq!(arg("--presence-penalty"), "1.5");
    }

    #[cfg(unix)]
    #[test]
    fn watchdog_stops_the_server_when_sokki_lets_go() {
        use std::process::{Command, Stdio};
        let dir = std::env::temp_dir().join(format!("sokki-watchdog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pidfile = dir.join("pid");
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(WATCHDOG)
            .arg("sokki-llm")
            .arg("/bin/sh")
            .arg("-c")
            .arg(format!("echo $$ > '{}'; exec sleep 100", pidfile.display()))
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let mut pid = String::new();
        for _ in 0..50 {
            pid = std::fs::read_to_string(&pidfile).unwrap_or_default();
            if !pid.trim().is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let pid = pid.trim().to_string();
        assert!(!pid.is_empty(), "the server did not start");
        let alive = |pid: &str| {
            Command::new("/bin/kill")
                .args(["-0", pid])
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success()
        };
        assert!(alive(&pid));
        assert!(child.try_wait().unwrap().is_none());
        // Sokki exits or drops the server: its end of stdin closes.
        drop(child.stdin.take());
        let mut stopped = false;
        for _ in 0..60 {
            if !alive(&pid) && child.try_wait().unwrap().is_some() {
                stopped = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(stopped, "the server kept running");
    }

    #[test]
    fn random_keys_differ() {
        let (a, b) = (random_key(), random_key());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }

    #[test]
    fn cloud_provider_uses_its_key_and_default_model() {
        let mut s = LlmSettings {
            provider: "local".into(),
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

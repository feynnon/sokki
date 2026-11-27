//! The `sokki` command on the Mac: the app owns the models and the data;
//! this binary reads its files and talks to it over the local socket.
//! It replaces the old `scripts/sokki` (bash), with the same `transcribe`,
//! `ask` and `config`.

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const INSTALLED_PLIST: &str = "/Applications/Sokki.app/Contents/Info.plist";

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn plist(key: &str) -> Option<String> {
    let out = Command::new("defaults")
        .args(["read", INSTALLED_PLIST, key])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The installed app's bundle id, so this works before and after an update
/// that renamed it.
fn app_id() -> String {
    plist("CFBundleIdentifier").unwrap_or_else(|| sokki_api::MAC_APP_ID.to_string())
}

/// `~/Library/Application Support/org.sokki.app` (`SOKKI_DATA_DIR` for
/// tests).
pub fn data_dir() -> PathBuf {
    env_path("SOKKI_DATA_DIR").unwrap_or_else(|| home().join("Library/Application Support").join(app_id()))
}

pub fn settings_file() -> PathBuf {
    env_path("SOKKI_SETTINGS").unwrap_or_else(|| data_dir().join("settings_store.json"))
}

fn app_bin() -> PathBuf {
    env_path("SOKKI_APP_BIN").unwrap_or_else(|| {
        let exe = plist("CFBundleExecutable").unwrap_or_else(|| "sokki".into());
        PathBuf::from("/Applications/Sokki.app/Contents/MacOS").join(exe)
    })
}

fn settings() -> Value {
    std::fs::read_to_string(settings_file())
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .map(|v| v["settings"].clone())
        .unwrap_or(Value::Null)
}

fn meeting_setting(s: &Value, key: &str, default: &str) -> String {
    s["meetings"][key]
        .as_str()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or(default)
        .to_string()
}

/// The agent access setting, narrowed by `SOKKI_SCOPE` (a hook's scope).
pub fn scope() -> Result<sokki_index::Scope> {
    let set = sokki_index::Scope::parse(settings()["agent_access"].as_str().unwrap_or("off"))?;
    Ok(match std::env::var("SOKKI_SCOPE").ok().filter(|s| !s.is_empty()) {
        Some(s) => set.narrow(sokki_index::Scope::parse(&s)?),
        None => set,
    })
}

pub fn memory_env() -> Result<crate::memory::Env> {
    let data = data_dir();
    let s = settings();
    Ok(crate::memory::Env {
        paths: sokki_index::Paths {
            index: env_path("SOKKI_INDEX").unwrap_or_else(|| data.join("index.sqlite")),
            meetings_dir: data.join("meetings"),
            history_db: Some(data.join("history.db")),
            user_name: meeting_setting(&s, "user_name", "Ich"),
        },
        scope: scope()?,
        how_to_allow: "sokki config agent_access summaries   (or transcripts, dictations)",
        audit_log: data.join("agent-access.log"),
    })
}

fn meetings_index() -> PathBuf {
    env_path("SOKKI_MEETINGS_INDEX").unwrap_or_else(|| data_dir().join("meetings/index.json"))
}

/// A meeting records or is being processed. Then a transcription or a
/// question to the model waits: on 2026-11-24 eight transcriptions started
/// from another session ran during a call, and the call audio went dead for
/// the rest of the meeting.
pub fn meeting_busy() -> bool {
    let Ok(text) = std::fs::read_to_string(meetings_index()) else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    let list = v.as_array().cloned().or_else(|| v["meetings"].as_array().cloned()).unwrap_or_default();
    list.iter().any(|m| {
        matches!(
            m["status"]["kind"].as_str().or(m["status"].as_str()),
            Some("recording" | "processing")
        )
    })
}

pub fn wait_for_meeting() {
    if !meeting_busy() {
        return;
    }
    eprintln!("sokki: Sokki is recording or processing a meeting; waiting until it is done…");
    let every = std::env::var("SOKKI_WAIT_EVERY_S")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10u64);
    let began = Instant::now();
    while meeting_busy() {
        std::thread::sleep(Duration::from_secs(every));
    }
    eprintln!(
        "sokki: the meeting is done after {} s; going on.",
        began.elapsed().as_secs()
    );
}

/// Any audio or video file to text with Sokki's speech model: ffmpeg to a
/// 16 kHz mono WAV, then the installed app's headless `--transcribe-file`.
pub fn transcribe(file: &str, json: bool) -> Result<()> {
    if !Path::new(file).is_file() {
        bail!("no such file: {file}");
    }
    let bin = app_bin();
    if !bin.is_file() {
        bail!("Sokki is not installed at {}", bin.display());
    }
    let ffmpeg = which("ffmpeg").unwrap_or_else(|| PathBuf::from("/opt/homebrew/bin/ffmpeg"));
    if !ffmpeg.is_file() {
        bail!("needs ffmpeg: brew install ffmpeg");
    }
    let tmp = std::env::temp_dir().join(format!("sokki-transcribe-{}", std::process::id()));
    std::fs::create_dir_all(&tmp)?;
    let wav = tmp.join("audio.wav");
    let result = (|| -> Result<()> {
        let st = Command::new(&ffmpeg)
            .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i", file])
            .args(["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"])
            .arg(&wav)
            .status()
            .context("running ffmpeg")?;
        if !st.success() {
            bail!("could not read audio from {file}");
        }
        wait_for_meeting();
        let out = Command::new(&bin)
            .arg("--transcribe-file")
            .arg(&wav)
            .arg("--json")
            .stdin(Stdio::null())
            .output()
            .context("running Sokki")?;
        if !out.status.success() {
            let log = String::from_utf8_lossy(&out.stderr);
            let tail: Vec<&str> = log.lines().rev().take(3).collect();
            for l in tail.iter().rev() {
                eprintln!("{l}");
            }
            bail!("transcription failed");
        }
        let text = String::from_utf8_lossy(&out.stdout);
        if json {
            println!("{}", text.trim());
        } else {
            let v: Value = serde_json::from_str(text.trim()).context("unreadable answer from Sokki")?;
            println!("{}", v["text"].as_str().unwrap_or_default());
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

fn which(cmd: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(cmd))
            .find(|f| f.is_file())
    })
}

/// Asks the language model Sokki uses for summaries: Ollama's /api/chat
/// (thinking off unless `think`, the model's own temperature), or
/// /chat/completions for a base URL ending in /v1. The prompt is the
/// arguments, with stdin appended after a blank line.
pub fn ask(words: &[&str], think: bool) -> Result<()> {
    let mut prompt = words.join(" ");
    if !std::io::stdin().is_terminal() {
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input)?;
        if !input.trim().is_empty() {
            prompt = if prompt.is_empty() { input } else { format!("{prompt}\n\n{input}") };
        }
    }
    if prompt.trim().is_empty() {
        bail!("usage: sokki ask PROMPT   (or pipe text in)");
    }
    let s = settings();
    let base = meeting_setting(&s, "llm_base_url", "http://localhost:11434");
    let base = base.trim_end_matches('/').to_string();
    let model = std::env::var("SOKKI_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| meeting_setting(&s, "llm_model", "qwen3.5:9b"));
    wait_for_meeting();
    let messages = serde_json::json!([{ "role": "user", "content": prompt }]);
    let (url, body) = if base.ends_with("/v1") {
        (
            format!("{base}/chat/completions"),
            serde_json::json!({ "model": model, "messages": messages, "stream": false }),
        )
    } else {
        (
            format!("{base}/api/chat"),
            serde_json::json!({ "model": model, "messages": messages, "stream": false, "think": think }),
        )
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(600)))
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
    let data: Value = serde_json::from_str(&text).with_context(|| format!("unreadable answer from {url}"))?;
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

fn secret(key: &str) -> bool {
    let k = key.to_lowercase();
    ["key", "token", "secret", "password"].iter().any(|w| k.contains(w))
}

fn shown(key: &str, v: &Value) -> String {
    let empty = v.is_null()
        || v.as_str() == Some("")
        || v.as_object().is_some_and(|o| o.is_empty())
        || v.as_array().is_some_and(|a| a.is_empty());
    if secret(key) && !empty {
        return "\"(hidden)\"".into();
    }
    v.to_string()
}

fn flat(prefix: &str, v: &Value, out: &mut Vec<(String, Value)>) {
    match v.as_object() {
        Some(o) if !o.is_empty() && !secret(prefix) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            for k in keys {
                let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                flat(&key, &o[k], out);
            }
        }
        _ => out.push((prefix.to_string(), v.clone())),
    }
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Bool(_) => "true or false",
        Value::Number(_) => "a number",
        Value::String(_) => "text",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
        Value::Null => "empty",
    }
}

/// `sokki config [KEY [VALUE]]`: every setting, one, or a change. Only
/// keys that exist, with the type they have; keys, tokens and passwords
/// never print. The running app applies a change within a second
/// (`src-tauri/src/config_watch.rs`).
pub fn config(args: &[&str]) -> Result<()> {
    if args.len() > 2 {
        bail!("usage: sokki config [KEY [VALUE]]");
    }
    let path = settings_file();
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("no settings yet at {} (start Sokki once)", path.display()))?;
    let mut store: Value = serde_json::from_str(&text).context("the settings file is not JSON")?;
    if !store["settings"].is_object() {
        bail!("the settings file has no settings");
    }
    let mut all = Vec::new();
    flat("", &store["settings"], &mut all);
    let Some(key) = args.first() else {
        for (k, v) in &all {
            println!("{k} = {}", shown(k, v));
        }
        return Ok(());
    };
    // agent_access before the app wrote it (an older app, or one not
    // started since the update): its default is off, and it may be set.
    if *key == "agent_access" && store["settings"].get("agent_access").is_none() {
        store["settings"]["agent_access"] = Value::String("off".into());
        if args.len() == 1 {
            println!("\"off\"");
            return Ok(());
        }
    }
    let parts: Vec<&str> = key.split('.').collect();
    let mut node = &mut store["settings"];
    for p in &parts[..parts.len() - 1] {
        node = node.get_mut(*p).filter(|n| n.is_object()).with_context(|| missing(key, &all))?;
    }
    let last = parts[parts.len() - 1];
    let parent = node.as_object_mut().with_context(|| missing(key, &all))?;
    let old = parent.get(last).cloned().with_context(|| missing(key, &all))?;
    let Some(raw) = args.get(1) else {
        println!("{}", shown(key, &old));
        return Ok(());
    };
    let mut value: Value = serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()));
    if !old.is_null() && kind(&old) != kind(&value) {
        if kind(&old) == "text" && !value.is_object() && !value.is_array() {
            value = Value::String(raw.to_string());
        } else {
            bail!("{key} takes {}, not {raw:?}", kind(&old));
        }
    }
    if old.is_i64() || old.is_u64() {
        if let Some(f) = value.as_f64().filter(|_| value.is_f64()) {
            if f.fract() != 0.0 {
                bail!("{key} takes a whole number, not {raw:?}");
            }
            value = Value::from(f as i64);
        }
    }
    if key == &"agent_access" {
        sokki_index::Scope::parse(value.as_str().unwrap_or(""))?;
        // The person's consent, not an agent's: only from a terminal a
        // person types in (an agent's shell has none). Not a lock (the file
        // is the person's), a stop for the easy path.
        if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
            bail!(
                "agent_access is the person's choice: type `sokki config agent_access {raw}` in a terminal yourself"
            );
        }
    }
    parent.insert(last.to_string(), value.clone());
    let tmp = path.with_file_name(format!(".settings-{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_string_pretty(&store)?)?;
    std::fs::rename(&tmp, &path)?;
    println!("{key} = {}  (was {})", shown(key, &value), shown(key, &old));
    Ok(())
}

fn missing(key: &str, all: &[(String, Value)]) -> String {
    let last = key.rsplit('.').next().unwrap_or(key);
    // Keys that contain the typed name, or start like it (a typo near the end).
    let close = |k: &str| {
        let tail = k.rsplit('.').next().unwrap_or(k);
        let common = tail.chars().zip(last.chars()).take_while(|(a, b)| a == b).count();
        k.contains(last) || common >= 5.min(last.chars().count())
    };
    let near: Vec<&str> = all
        .iter()
        .map(|(k, _)| k.as_str())
        .filter(|k| close(k))
        .take(8)
        .collect();
    let hint = if near.is_empty() {
        String::new()
    } else {
        format!("; did you mean: {}", near.join(", "))
    };
    format!("no setting {key}{hint}. `sokki config` lists them all.")
}

/// Opens a meeting's note in the default Markdown app.
pub fn open_path(path: &str) -> Result<()> {
    Command::new("open")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("opening the note")?;
    Ok(())
}

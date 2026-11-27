//! `sokki`: dictation and meeting notes on Linux. One binary: the
//! background service, its command line, and the processing worker.

mod api;
mod audio;
mod client;
mod config;
mod daemon;
mod hooks;
mod keep_awake;
mod local;
mod models;
mod output;
mod paths;
mod store;
mod worker;

use anyhow::{bail, Context, Result};
use api::{MeetingState, Request};

const HELP: &str = "\
sokki: dictation and meeting notes, on your computer.

Usage:
  sokki meeting record|stop|toggle|dismiss   Record a meeting, or answer the question
  sokki dictation start|stop|toggle|cancel   Dictate into the focused window
  sokki status [--json]                      What Sokki is doing
  sokki notes [--json] [-n N]                Recent meetings
  sokki open [ID|latest]                     Open a meeting's note
  sokki speakers ID [--json]                 Speakers of a meeting
  sokki name ID SPEAKER NAME [--forget]      Name a speaker (remembers the voice)
  sokki transcribe FILE [--json]             Any audio or video file to text
  sokki ask [--think] [PROMPT]               Ask the local language model (stdin is appended)
  sokki models install                       Download the speech models
  sokki reload                               Re-read ~/.config/sokki/sokki.toml
  sokki events                               Stream state and hook events as JSON lines
  sokki hooks list|allow|run|test|log        Your own commands on events (HOOKS.md)
  sokki doctor                               Check the setup
  sokki daemon                               Run the service (systemd does this)
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The service and the worker log their steps; commands for people and
    // agents (transcribe, ask, status …) only warnings.
    let level = match args.first().map(String::as_str) {
        Some("daemon" | "process") => "info",
        _ => "warn",
    };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(level))
        .format_timestamp(None)
        .init();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    if let ["process", id] = words.as_slice() {
        std::process::exit(worker::main(id));
    }
    if let Err(e) = run(&words) {
        eprintln!("sokki: {e:#}");
        std::process::exit(1);
    }
}

fn flag(words: &[&str], f: &str) -> bool {
    words.contains(&f)
}

fn run(words: &[&str]) -> Result<()> {
    let json = flag(words, "--json");
    let plain: Vec<&str> = words.iter().copied().filter(|w| !w.starts_with("--")).collect();
    let simple = |req: Request| -> Result<()> {
        client::request(&req)?;
        Ok(())
    };
    match plain.as_slice() {
        ["daemon"] => daemon::main(),
        ["hooks", ..] => hooks::main(&words[words.iter().position(|w| *w == "hooks").unwrap_or(0) + 1..]),
        ["events"] => client::events(),
        ["meeting", "record" | "start"] => simple(Request::MeetingRecord),
        ["meeting", "stop"] => simple(Request::MeetingStop),
        ["meeting", "toggle"] => simple(Request::MeetingToggle),
        ["meeting", "dismiss"] => simple(Request::MeetingDismiss),
        ["dictation", "start"] => simple(Request::DictationStart),
        ["dictation", "stop"] => simple(Request::DictationStop),
        ["dictation", "toggle"] => simple(Request::DictationToggle),
        ["dictation", "cancel"] => simple(Request::DictationCancel),
        ["reload"] => simple(Request::Reload),
        ["status"] => status(json),
        ["notes"] | ["notes", "-n", _] => {
            let limit = match plain.as_slice() {
                [_, _, n] => n.parse().context("-n needs a number")?,
                _ => 10,
            };
            notes(limit, json)
        }
        ["open"] => open("latest"),
        ["open", id] => open(id),
        ["speakers", id] => speakers(id, json),
        ["name", id, speaker, name @ ..] if !name.is_empty() => simple(Request::NameSpeaker {
            meeting: id.to_string(),
            speaker: speaker.to_string(),
            name: name.join(" "),
            remember: !flag(words, "--forget"),
        }),
        ["models", "install"] => models_install(),
        ["transcribe", file] => local::transcribe(file, json),
        ["ask", rest @ ..] => local::ask(rest, flag(words, "--think")),
        ["doctor"] => doctor(),
        ["help"] | [] => {
            print!("{HELP}");
            Ok(())
        }
        ["version"] => {
            println!("sokki {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => {
            if flag(words, "--help") || flag(words, "-h") {
                print!("{HELP}");
                return Ok(());
            }
            bail!("unknown command. Try: sokki help")
        }
    }
}

fn status(json: bool) -> Result<()> {
    let reply = client::request(&Request::Status)?;
    if json {
        println!("{}", serde_json::to_string(&reply)?);
        return Ok(());
    }
    let state = reply.state.unwrap_or_default();
    match &state.meeting {
        MeetingState::Idle => println!("Meeting: idle"),
        MeetingState::Asking { app, .. } => println!("Meeting: asking whether to record {app}"),
        MeetingState::Recording { app, since_ms, .. } => {
            let secs = (chrono::Utc::now().timestamp_millis() as u64).saturating_sub(*since_ms) / 1000;
            println!(
                "Meeting: recording {}for {}:{:02}",
                app.as_ref().map(|a| format!("{a} ")).unwrap_or_default(),
                secs / 60,
                secs % 60
            );
        }
    }
    if let Some(p) = &state.processing {
        let pct = p
            .progress
            .map(|p| format!(" {}%", (p * 100.0).round()))
            .unwrap_or_default();
        println!("Writing note: {}{pct}", p.stage);
    }
    if state.queued > 0 {
        println!("Waiting: {}", state.queued);
    }
    println!("Dictation: {:?}", state.dictation);
    match (&state.models.ready, &state.models.downloading) {
        (_, Some(d)) => println!(
            "Models: downloading {} ({} of {} MB)",
            d.file,
            d.done >> 20,
            d.total >> 20
        ),
        (true, None) => println!("Models: ready"),
        (false, None) => println!("Models: missing (sokki models install)"),
    }
    Ok(())
}

fn notes(limit: usize, json: bool) -> Result<()> {
    let reply = client::request(&Request::Notes { limit })?;
    if json {
        println!("{}", serde_json::to_string(&reply)?);
        return Ok(());
    }
    let notes = reply.notes.unwrap_or_default();
    for n in notes {
        println!(
            "{}  {:>9}  {}",
            n.id,
            n.status,
            n.title.or(n.app).unwrap_or_default()
        );
    }
    Ok(())
}

fn open(id: &str) -> Result<()> {
    let notes = client::request(&Request::Notes { limit: 200 })?
        .notes
        .unwrap_or_default();
    let note = if id == "latest" {
        notes.iter().find(|n| n.path.is_some())
    } else {
        notes.iter().find(|n| n.id == id)
    };
    let path = note
        .and_then(|n| n.path.clone())
        .context("no note for that meeting (yet)")?;
    // `gio open` goes by the file name (.md: the Markdown app, on Omarchy
    // Omawrite); xdg-open sniffs the content, calls a note with front
    // matter plain text and opens it in a terminal editor (seen in the VM).
    // Detached: the editor neither holds this terminal nor writes into it.
    use std::os::unix::process::CommandExt;
    let gio = std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("gio").is_file()));
    let mut cmd = if gio {
        let mut c = std::process::Command::new("gio");
        c.arg("open");
        c
    } else {
        std::process::Command::new("xdg-open")
    };
    cmd.arg(&path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn()
        .context("opening the note")?;
    Ok(())
}

fn speakers(id: &str, json: bool) -> Result<()> {
    let reply = client::request(&Request::Speakers {
        meeting: id.to_string(),
    })?;
    if json {
        println!("{}", serde_json::to_string(&reply)?);
        return Ok(());
    }
    let speakers = reply.speakers.unwrap_or_default();
    for s in speakers {
        println!(
            "{}{}",
            s.name,
            if s.unnamed { "  (unnamed)" } else { "" }
        );
    }
    Ok(())
}

/// Starts the download in the daemon and shows its progress until done.
fn models_install() -> Result<()> {
    use std::io::{BufRead, BufReader, Write};
    client::request(&Request::InstallModels)?;
    let mut s = std::os::unix::net::UnixStream::connect(paths::socket())?;
    s.write_all(b"{\"cmd\":\"subscribe\"}\n")?;
    for line in BufReader::new(s).lines() {
        let state = match serde_json::from_str(&line?) {
            Ok(api::Event::State(state)) => state,
            Ok(api::Event::Error { message }) if message.contains("download") => {
                eprintln!();
                bail!("{message}")
            }
            _ => continue,
        };
        if let Some(d) = &state.models.downloading {
            eprint!(
                "\r\x1b[2KDownloading {}: {} of {} MB",
                d.file,
                d.done >> 20,
                d.total >> 20
            );
        }
        if state.models.ready && !state.models.busy {
            eprintln!("\r\x1b[2KModels ready.");
            return Ok(());
        }
    }
    bail!("the daemon stopped")
}

/// Why Ollama may be on the CPU, from the graphics chips the kernel sees.
fn gpu_hint() -> String {
    let vendors: Vec<String> = std::fs::read_dir("/sys/class/drm")
        .map(|d| {
            d.flatten()
                .filter_map(|e| std::fs::read_to_string(e.path().join("device/vendor")).ok())
                .map(|v| v.trim().to_lowercase())
                .collect()
        })
        .unwrap_or_default();
    let has = |id: &str| vendors.iter().any(|v| v == id);
    let ollama_pkg = |name: &str| {
        std::process::Command::new("pacman")
            .args(["-Q", name])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    if has("0x1002") {
        // Ollama leaves integrated AMD GPUs alone unless told (it does use
        // Strix Halo's by default).
        if !ollama_pkg("ollama-vulkan") && !ollama_pkg("ollama-rocm") {
            return "AMD graphics: install ollama-vulkan (or reinstall Ollama from the Omarchy menu)".into();
        }
        return "AMD graphics: Ollama skips integrated AMD GPUs unless OLLAMA_IGPU_ENABLE=1 is set; \
                as a system service: sudo systemctl edit ollama, add [Service] Environment=OLLAMA_IGPU_ENABLE=1, then restart it"
            .into();
    }
    if has("0x10de") && !ollama_pkg("ollama-cuda") && !ollama_pkg("ollama-vulkan") {
        return "NVIDIA graphics: install ollama-cuda".into();
    }
    if has("0x8086") && !ollama_pkg("ollama-vulkan") {
        return "Intel graphics: install ollama-vulkan".into();
    }
    "no GPU Ollama can use was found; Sokki is made for machines with one".into()
}

/// Checks what Sokki needs, in plain words.
fn doctor() -> Result<()> {
    let have = |cmd: &str| {
        std::env::var_os("PATH")
            .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
    };
    let mut ok = true;
    let mut line = |good: bool, what: &str, fix: &str| {
        ok &= good;
        if good {
            println!("  ok    {what}");
        } else {
            println!("  todo  {what}: {fix}");
        }
    };
    println!("Sokki {}", env!("CARGO_PKG_VERSION"));
    line(have("pw-record") && have("pw-dump"), "PipeWire tools", "install pipewire");
    line(have("wtype"), "typing (wtype)", "sudo pacman -S wtype");
    line(have("wl-copy"), "clipboard (wl-copy)", "sudo pacman -S wl-clipboard");
    line(
        client::request(&Request::Status).is_ok(),
        "service running",
        "systemctl --user enable --now sokki",
    );
    line(models::all_present(), "speech models", "sokki models install");
    let cfg = config::Config::load(&paths::config_file())?;
    let llm = sokki_engine::llm::check(&cfg.llm, &cfg.meetings.user_name, false);
    line(
        llm.state == "ready",
        &format!("language model ({})", llm.detail),
        match llm.state.as_str() {
            "no_ollama" => "install Ollama (Omarchy menu: Install > AI > Ollama) for summaries",
            "not_running" => "Ollama is installed but not running; Sokki starts it for the next note (or: ollama serve)",
            "model_missing" => "ollama pull qwen3.5:9b",
            "no_key" => "add your API key under [llm.keys] in sokki.toml",
            _ => "see sokki.toml [llm]",
        },
    );
    // A local model on the CPU makes every summary take minutes.
    if llm.state == "ready" {
        match sokki_engine::llm::ollama_on_gpu(&cfg.llm) {
            Some(true) => line(true, "language model runs on the GPU", ""),
            Some(false) => line(false, "language model runs on the CPU (summaries take minutes)", &gpu_hint()),
            None => {}
        }
    }
    let audio = meeting_detect::audio_input_clients();
    line(audio.is_ok(), "call detection", "needs pw-dump");
    println!("Config: {}", paths::config_file().display());
    println!("Notes:  {}", cfg.engine().notes_dir.display());
    if llm.state != "ready" {
        println!("Summaries need the language model; everything else works without it.");
    }
    let _ = ok;
    Ok(())
}

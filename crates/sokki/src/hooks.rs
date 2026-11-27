//! `sokki hooks …`: the person's hooks from the command line (HOOKS.md).
//! The work is in `sokki_engine::hooks::tools`; this file parses words and
//! prints. Works on the Mac and on Linux, also while the app is closed.

use anyhow::{bail, Context, Result};
use chrono::TimeZone;
use sokki_engine::hooks::{self, tools, Gate, HookEvent, Meta, Outcome, Paths, Scope};
use std::path::{Path, PathBuf};

const HELP: &str = "\
sokki hooks: your own commands, run by Sokki on events (HOOKS.md).

Usage:
  sokki hooks list [--json]              Hooks in ~/.config/sokki/hooks.toml, and whether each may run
  sokki hooks allow NAME                 Let a new or changed hook run (pins its hash)
  sokki hooks revoke NAME                Stop a hook from running until it is allowed again
  sokki hooks run NAME [--event E] [--id ID | --note FILE | --text TEXT]
                                         Run a hook now; without --id on a made-up meeting
  sokki hooks test [NAME]                Run every hook (or one) on made-up data, SOKKI_HOOK_TEST=1
  sokki hooks log [NAME] [-n N] [--json] Recent runs; with NAME also the last run's output
";

/// The hooks folder and, in the app's data, the queue and logs.
/// `SOKKI_HOOKS_DIR` and `SOKKI_HOOKS_STATE` point elsewhere (tests).
pub fn paths() -> Paths {
    Paths {
        config_dir: hooks::default_config_dir(),
        state_dir: state_dir(),
    }
}

fn state_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("SOKKI_HOOKS_STATE").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    app_data().join("hooks")
}

/// The app's data folder: on the Mac the CLI's own (`SOKKI_DATA_DIR` for a
/// test app, else the installed app's), on Linux `~/.local/share/sokki`.
fn app_data() -> PathBuf {
    #[cfg(target_os = "macos")]
    return crate::mac::data_dir();
    #[cfg(not(target_os = "macos"))]
    crate::paths::data_dir()
}

fn value<'a>(args: &'a [&str], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| *a == flag)
        .and_then(|i| args.get(i + 1).copied())
}

/// Words that are not flags or their values.
fn positional<'a>(args: &'a [&'a str]) -> Vec<&'a str> {
    const WITH_VALUE: [&str; 5] = ["--event", "--id", "--note", "--text", "-n"];
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        if WITH_VALUE.contains(a) {
            skip = true;
            continue;
        }
        if a.starts_with('-') {
            continue;
        }
        out.push(*a);
    }
    out
}

pub fn main(args: &[&str]) -> Result<()> {
    let json = args.contains(&"--json");
    let words = positional(args);
    let paths = paths();
    match words.as_slice() {
        ["list"] | [] => list(&paths, json),
        ["allow", name] => allow(&paths, name),
        ["revoke", name] => {
            if tools::revoke(&paths, name)? {
                println!("{name} no longer runs until you allow it again.");
            } else {
                println!("{name} was not allowed.");
            }
            Ok(())
        }
        ["run", name] => run(&paths, name, args),
        ["test"] => test(&paths, None),
        ["test", name] => test(&paths, Some(name)),
        ["log"] => log(&paths, None, args, json),
        ["log", name] => log(&paths, Some(name), args, json),
        ["help"] => {
            print!("{HELP}");
            Ok(())
        }
        _ => bail!("unknown hooks command.\n\n{HELP}"),
    }
}

fn when(ms: i64) -> String {
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default()
}

fn scope_text(scope: &[Scope]) -> String {
    if scope.is_empty() {
        "meta only".into()
    } else {
        scope
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn list(paths: &Paths, json: bool) -> Result<()> {
    let (_, infos) = tools::list(paths)?;
    if json {
        let rows: Vec<_> = infos
            .iter()
            .map(|i| {
                serde_json::json!({
                    "name": i.hook.name,
                    "on": i.hook.on,
                    "cron": i.hook.cron.as_ref().map(|c| c.text.clone()),
                    "run": i.hook.run,
                    "scope": i.hook.scope.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                    "enabled": i.hook.enabled,
                    "allowed": i.approval == tools::Approval::Allowed,
                    "approval": i.approval.as_str(),
                    "last_run": i.last.as_ref().map(|r| serde_json::json!({
                        "status": r.status, "exit_code": r.exit_code,
                        "started_ms": r.started_ms, "log": r.log_path,
                    })),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "config": paths.config_file(),
                "state": paths.state_dir,
                "hooks": rows,
            })
        );
        return Ok(());
    }
    println!("Hooks: {}", paths.config_file().display());
    if infos.is_empty() {
        println!(
            "No hooks yet. Examples: examples/hooks/ in the Sokki repository, guide: HOOKS.md."
        );
        return Ok(());
    }
    for i in &infos {
        let on = match &i.hook.cron {
            Some(c) => format!("schedule \"{}\"", c.text),
            None => i.hook.on.clone(),
        };
        let state = if i.hook.enabled {
            i.approval.as_str()
        } else {
            "off"
        };
        let last = i
            .last
            .as_ref()
            .map(|r| format!("last: {} {}", r.status, when(r.started_ms)))
            .unwrap_or_else(|| "never ran".into());
        println!("  {:<24} {:<32} {}", i.hook.name, on, state);
        println!("  {:<24} scope: {}; {last}", "", scope_text(&i.hook.scope));
    }
    if infos
        .iter()
        .any(|i| i.hook.enabled && i.approval != tools::Approval::Allowed)
    {
        println!("Let one run: sokki hooks allow NAME");
    }
    Ok(())
}

fn allow(paths: &Paths, name: &str) -> Result<()> {
    let (hook, files) = tools::allow(paths, name)?;
    println!("Allowed {}:", hook.name);
    match &hook.cron {
        Some(c) => println!("  on       schedule \"{}\"", c.text),
        None => println!("  on       {}", hook.on),
    }
    println!("  run      {}", hook.run);
    println!("  in       {}", paths.config_dir.display());
    println!("  scope    {}", scope_text(&hook.scope));
    for f in &files {
        println!("  pinned   {}", f.display());
    }
    if hook.has(Scope::Transcript) {
        println!(
            "It reads transcripts: everything anyone said in a call. Treat that text as untrusted input."
        );
    }
    println!(
        "If this entry or a pinned file changes, it waits for another `sokki hooks allow {}`.",
        hook.name
    );
    Ok(())
}

/// Recording or processing: hooks wait, also one started by hand.
fn gate() -> Gate {
    if cfg!(target_os = "macos") {
        mac_gate()
    } else {
        daemon_gate()
    }
}

// Both platforms' readers are compiled everywhere, so either is checked
// on either; each runs only on its own.

/// The Mac app's meeting list says whether one records or processes.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn mac_gate() -> Gate {
    let index = std::env::var_os("SOKKI_MEETINGS_INDEX")
        .map(PathBuf::from)
        .unwrap_or_else(|| app_data().join("meetings/index.json"));
    let Ok(text) = std::fs::read_to_string(index) else {
        return Gate::Free;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Gate::Free;
    };
    let list = v.as_array().cloned().unwrap_or_default();
    let kinds: Vec<String> = list
        .iter()
        .filter_map(|m| m["status"]["kind"].as_str().map(str::to_string))
        .collect();
    if kinds.iter().any(|k| k == "recording") {
        return Gate::Recording;
    }
    if kinds.iter().any(|k| k == "processing") {
        return Gate::Processing;
    }
    Gate::Free
}

/// The Linux service's state, from its socket; free when it isn't running.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn daemon_gate() -> Gate {
    match crate::client::request(&crate::api::Request::Status) {
        Ok(reply) => {
            let state = reply.state.unwrap_or_default();
            if matches!(state.meeting, crate::api::MeetingState::Recording { .. }) {
                Gate::Recording
            } else if state.processing.is_some() {
                Gate::Processing
            } else {
                Gate::Free
            }
        }
        Err(_) => Gate::Free,
    }
}

/// A real meeting by id, from the app's meeting list.
fn meeting(id: &str, kind: &str) -> Result<HookEvent> {
    #[cfg(target_os = "macos")]
    return mac_meeting(id, kind);
    #[cfg(not(target_os = "macos"))]
    return daemon_meeting(id, kind);
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn mac_meeting(id: &str, kind: &str) -> Result<HookEvent> {
    let index = app_data().join("meetings/index.json");
    let text =
        std::fs::read_to_string(&index).with_context(|| format!("reading {}", index.display()))?;
    let list: Vec<serde_json::Value> = serde_json::from_str(&text)?;
    let r = list
        .iter()
        .find(|m| m["id"] == id)
        .with_context(|| format!("no meeting {id:?} in {}", index.display()))?;
    let s = |k: &str| r[k].as_str().map(str::to_string);
    let meta = Meta {
        title: s("title"),
        started_at: s("started_at"),
        duration_s: r["duration_s"].as_f64(),
        app: s("source_app"),
        participants: r["speakers"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        language: None,
    };
    let note = s("note_path").map(PathBuf::from);
    Ok(HookEvent::meeting(kind, id, note.as_deref(), meta))
}

// The Linux service's meeting store is compiled only there.
#[cfg(not(target_os = "macos"))]
fn daemon_meeting(id: &str, kind: &str) -> Result<HookEvent> {
    let r = crate::store::Store::new(crate::paths::meetings_dir()).get(id)?;
    Ok(HookEvent::meeting(
        kind,
        id,
        r.note_path.as_deref().map(Path::new),
        Meta {
            title: r.title,
            started_at: Some(r.started_at),
            duration_s: Some(r.duration_s),
            app: r.source_app,
            participants: r.speakers,
            language: None,
        },
    ))
}

/// names; for made-up data the fixtures' folder, never the real notes.
fn host_env(notes: &Path) -> Vec<(String, String)> {
    vec![
        (
            "SOKKI_APP_DATA".to_string(),
            app_data().to_string_lossy().into_owned(),
        ),
        (
            "SOKKI_NOTES_DIR".to_string(),
            notes.to_string_lossy().into_owned(),
        ),
    ]
}

fn fixtures_dir(paths: &Paths) -> PathBuf {
    paths.state_dir.join("fixtures")
}

fn print_run(name: &str, ran: &tools::Ran) {
    if let Ok(text) = std::fs::read_to_string(&ran.log) {
        // The hook's own output, without Sokki's header lines.
        for line in text.lines().filter(|l| !l.starts_with("# ")) {
            println!("{line}");
        }
    }
    println!(
        "{name}: {} (log: {})",
        ran.outcome.describe(ran.timeout),
        ran.log.display()
    );
}

fn run(paths: &Paths, name: &str, args: &[&str]) -> Result<()> {
    let cfg = sokki_engine::hooks::config::load(&paths.config_dir)?;
    let hook = cfg
        .get(name)
        .with_context(|| format!("no hook {name:?} in {}", paths.config_file().display()))?;
    let kind = value(args, "--event").unwrap_or(&hook.on).to_string();
    let fixtures = fixtures_dir(paths);
    let mut notes = fixtures.clone();
    let mut event = if let Some(note) = value(args, "--note") {
        let note = std::fs::canonicalize(note).with_context(|| format!("no file {note}"))?;
        let id = value(args, "--id").unwrap_or("fixture");
        let mut e = HookEvent::meeting(&kind, id, Some(&note), Meta::default());
        sokki_engine::hooks::content::complete_meta(&mut e);
        e
    } else if let Some(id) = value(args, "--id").filter(|_| kind.starts_with("meeting.")) {
        let mut e = meeting(id, &kind)?;
        sokki_engine::hooks::content::complete_meta(&mut e);
        // Notes are written flat into the notes folder.
        if let Some(dir) = e.note_path.as_deref().and_then(|p| Path::new(p).parent()) {
            notes = dir.to_path_buf();
        }
        e
    } else {
        eprintln!("sokki: on a made-up {kind} event (pass --id for a real one)");
        tools::fixture_event(&kind, &fixtures)?
    };
    if let Some(text) = value(args, "--text") {
        event.content.text = Some(text.to_string());
    }
    let ran = tools::run_once(
        paths,
        name,
        &event,
        "manual",
        &gate,
        &host_env(&notes),
        || {
            eprintln!("sokki: a meeting is recording or being processed; the hook waits until it is done…")
        },
    )?;
    print_run(name, &ran);
    if !ran.outcome.is_ok() {
        std::process::exit(1);
    }
    Ok(())
}

fn test(paths: &Paths, only: Option<&str>) -> Result<()> {
    let cfg = sokki_engine::hooks::config::load(&paths.config_dir)?;
    let hooks: Vec<_> = cfg
        .hooks
        .iter()
        .filter(|h| only.is_none_or(|n| h.name == n))
        .collect();
    if hooks.is_empty() {
        bail!("no hooks to test in {}", paths.config_file().display());
    }
    let mut failed = 0;
    for hook in hooks {
        let fixtures = fixtures_dir(paths);
        let event = tools::fixture_event(&hook.on, &fixtures)?;
        let ran = tools::run_once(
            paths,
            &hook.name,
            &event,
            "test",
            &gate,
            &host_env(&fixtures),
            || eprintln!("sokki: waiting for the meeting to finish…"),
        )?;
        let ok = ran.outcome == Outcome::Ok;
        if !ok {
            failed += 1;
        }
        println!(
            "{} {:<24} {:<22} {}  {}",
            if ok { "ok  " } else { "FAIL" },
            hook.name,
            hook.on,
            ran.outcome.describe(ran.timeout),
            ran.log.display()
        );
    }
    if failed > 0 {
        bail!("{failed} hook(s) failed; see their logs");
    }
    Ok(())
}

fn log(paths: &Paths, name: Option<&str>, args: &[&str], json: bool) -> Result<()> {
    let n: usize = value(args, "-n").map(str::parse).transpose()?.unwrap_or(20);
    let runs = tools::runs(paths, name, n)?;
    if json {
        let rows: Vec<_> = runs
            .iter()
            .map(|r| {
                serde_json::json!({
                    "hook": r.hook, "event": r.event, "id": r.subject, "attempt": r.attempt,
                    "trigger": r.trigger, "started_ms": r.started_ms, "finished_ms": r.finished_ms,
                    "status": r.status, "exit_code": r.exit_code, "error": r.error, "log": r.log_path,
                })
            })
            .collect();
        println!("{}", serde_json::Value::Array(rows));
        return Ok(());
    }
    if runs.is_empty() {
        println!("No runs yet.");
        return Ok(());
    }
    for r in runs.iter().rev() {
        let took = r
            .finished_ms
            .map(|f| format!("{:.1} s", (f - r.started_ms) as f64 / 1000.0))
            .unwrap_or_else(|| "…".into());
        let code = r.exit_code.map(|c| format!(" ({c})")).unwrap_or_default();
        println!(
            "{}  {:<22} {:<22} {:<24} {}{code} {took}{}",
            when(r.started_ms),
            r.hook,
            r.event,
            r.subject,
            r.status,
            match r.trigger.as_str() {
                "event" if r.attempt > 1 => " retry".to_string(),
                "event" => String::new(),
                t => format!(" [{t}]"),
            }
        );
    }
    if let (Some(_), Some(last)) = (name, runs.first()) {
        if let Some(path) = &last.log_path {
            println!("\n--- {path}");
            match std::fs::read_to_string(Path::new(path)) {
                Ok(text) => print!("{text}"),
                Err(e) => println!("(log gone: {e})"),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hook lines on `sokki events` next to the daemon's own.
    #[test]
    fn hook_events_never_clash_with_the_other_lines() {
        let line = r#"{"v":1,"event":"meeting.note_ready","id":"m1","at":"2026-11-26T14:40:12+01:00","meta":{"participants":[]},"content":{}}"#;
        assert!(serde_json::from_str::<crate::api::Event>(line).is_err());
        let e: HookEvent = serde_json::from_str(line).unwrap();
        assert_eq!(e.event, "meeting.note_ready");
        let state = serde_json::to_string(&crate::api::Event::Offline).unwrap();
        assert!(serde_json::from_str::<HookEvent>(&state).is_err());
    }

    #[test]
    fn flags_and_their_values_are_not_words() {
        let args = ["run", "x", "--event", "schedule", "--id", "m1", "--json"];
        assert_eq!(positional(&args), vec!["run", "x"]);
        assert_eq!(value(&args, "--id"), Some("m1"));
        assert_eq!(value(&args, "--note"), None);
    }
}

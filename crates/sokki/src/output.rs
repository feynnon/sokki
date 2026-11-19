//! Getting text and questions to the person: typing dictated text into the
//! focused window, and notifications when no shell plugin is listening.

use anyhow::{bail, Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};

fn have(cmd: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
}

/// Puts text into the focused window. `mode`: "type" (wtype), "paste"
/// (clipboard + Shift+Insert) or "clipboard" (copy only).
pub fn deliver_text(text: &str, mode: &str) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    match mode {
        "clipboard" => copy(text),
        "paste" => {
            copy(text)?;
            run("wtype", &["-M", "shift", "-k", "Insert", "-m", "shift"])
        }
        _ if have("wtype") => run("wtype", &["--", text]),
        _ => {
            copy(text)?;
            notify("Dictation copied", "wtype is not installed, so the text is on the clipboard.", &[]);
            Ok(())
        }
    }
}

fn copy(text: &str) -> Result<()> {
    let mut child = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .spawn()
        .context("wl-copy (wl-clipboard) is not installed")?;
    child
        .stdin
        .take()
        .context("wl-copy stdin")?
        .write_all(text.as_bytes())?;
    child.wait()?;
    Ok(())
}

fn run(cmd: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(cmd)
        .args(args)
        .status()
        .with_context(|| format!("{cmd} is not installed"))?;
    if !status.success() {
        bail!("{cmd} failed ({status})");
    }
    Ok(())
}

/// A notification; each action is (label, sokki subcommand words), run
/// when clicked. Uses Omarchy's notifier when present, else `notify-send`.
/// Never blocks.
pub fn notify(title: &str, body: &str, actions: &[(&str, &[&str])]) {
    let sokki = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "sokki".into());
    let mut cmd;
    if have("omarchy-notification-send") {
        cmd = Command::new("omarchy-notification-send");
        cmd.args(["-g", "󰍬", title, body]);
        for (label, words) in actions {
            cmd.args(["--action", label, &sokki]).args(*words);
        }
    } else {
        cmd = Command::new("notify-send");
        cmd.args(["--app-name", "Sokki", title, body]);
        if !actions.is_empty() {
            // notify-send prints the chosen action; run it ourselves.
            cmd.arg("--wait");
            for (i, (label, _)) in actions.iter().enumerate() {
                cmd.arg(format!("--action={i}={label}"));
            }
        }
    }
    let actions: Vec<Vec<String>> = actions
        .iter()
        .map(|(_, w)| w.iter().map(|s| s.to_string()).collect())
        .collect();
    let omarchy = have("omarchy-notification-send");
    std::thread::spawn(move || {
        let out = cmd.stdin(Stdio::null()).stderr(Stdio::null()).output();
        if omarchy || actions.is_empty() {
            return;
        }
        let Ok(out) = out else { return };
        let chosen = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if let Some(words) = chosen.parse::<usize>().ok().and_then(|i| actions.get(i)) {
            let _ = Command::new(&sokki).args(words).status();
        }
    });
}

//! The CLI side of the local API: one request, one reply; or a stream of
//! events for `sokki events`.

use anyhow::{bail, Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use crate::api::{Event, Reply, Request};

fn connect() -> Result<UnixStream> {
    let path = crate::paths::socket();
    UnixStream::connect(&path).with_context(|| {
        if cfg!(target_os = "macos") {
            "Sokki is not running (or is older than its local API). Start it with: open -a Sokki"
                .to_string()
        } else {
            "Sokki is not running. Start it with: systemctl --user start sokki".to_string()
        }
    })
}

pub fn request(req: &Request) -> Result<Reply> {
    let mut s = connect()?;
    let mut line = serde_json::to_string(req)?;
    line.push('\n');
    s.write_all(line.as_bytes())?;
    let mut answer = String::new();
    BufReader::new(s).read_line(&mut answer)?;
    let reply: Reply = serde_json::from_str(answer.trim()).context("unreadable answer")?;
    if !reply.ok {
        bail!("{}", reply.error.unwrap_or_else(|| "failed".into()));
    }
    Ok(reply)
}

/// Prints every event as one JSON line, forever. While the daemon is
/// unreachable it prints `{"event":"offline"}` once and retries.
pub fn events() -> Result<()> {
    let mut offline_said = false;
    let mut out = std::io::stdout();
    loop {
        match connect() {
            Ok(mut s) => {
                offline_said = false;
                s.write_all(b"{\"cmd\":\"subscribe\"}\n")?;
                for line in BufReader::new(s).lines() {
                    let Ok(line) = line else { break };
                    // A closed stdout (the reader went away) ends us.
                    if writeln!(out, "{line}").is_err() || out.flush().is_err() {
                        return Ok(());
                    }
                }
            }
            Err(_) if !offline_said => {
                offline_said = true;
                let line = serde_json::to_string(&Event::Offline)?;
                if writeln!(out, "{line}").is_err() || out.flush().is_err() {
                    return Ok(());
                }
            }
            Err(_) => {}
        }
        // The daemon restarts within a second or two under systemd.
        std::thread::sleep(Duration::from_secs(2));
    }
}

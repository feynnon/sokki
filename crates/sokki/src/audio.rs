//! Recording on Linux through PipeWire's own tools.
//!
//! `pw-record` ships with PipeWire (so with every Omarchy install), handles
//! device changes, resampling and the file format, and keeps this binary
//! free of C bindings. A native PipeWire client could replace it behind
//! these functions if it ever falls short.
//!
//! - The microphone: the default source.
//! - The call: the monitor of the default output (everything the computer
//!   plays). Recording only the call app's own stream is possible
//!   (`call_app_only`) but off: browsers replace their stream mid-call.

use anyhow::{Context, Result};
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

/// Properties every Sokki stream carries, so detection and other tools can
/// tell our streams apart from a call app's.
const OUR_PROPS: [&str; 4] = [
    "-P",
    "application.name=Sokki",
    "-P",
    "media.role=Production",
];

fn now_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

/// One running `pw-record`.
pub struct Capture {
    child: Child,
    /// When the process started (µs since the epoch), for aligning tracks.
    pub started_us: u64,
}

impl Capture {
    /// Records to a 16 kHz mono FLAC file. `target`: a PipeWire node
    /// (serial or name); `None` for the default source. `monitor` records
    /// what a sink plays instead of a source.
    pub fn to_file(path: &Path, target: Option<&str>, monitor: bool) -> Result<Capture> {
        let mut cmd = Command::new("pw-record");
        cmd.args(["--rate", "16000", "--channels", "1", "--format", "s16"])
            .args(OUR_PROPS);
        if monitor {
            cmd.args(["-P", "stream.capture.sink=true"]);
        }
        if let Some(t) = target {
            // If the target goes away (a browser replacing its stream), stay
            // unlinked: falling back to the default would record the
            // microphone into the call track.
            cmd.args(["--target", t])
                .args(["-P", "node.dont-reconnect=true", "-P", "node.dont-fallback=true"]);
        }
        cmd.arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let started_us = now_us();
        let child = cmd.spawn().context("starting pw-record (is PipeWire installed?)")?;
        Ok(Capture { child, started_us })
    }

    /// Streams raw f32 samples (16 kHz mono) from the default source on
    /// stdout, for dictation.
    pub fn raw() -> Result<Capture> {
        let started_us = now_us();
        let child = Command::new("pw-record")
            // --raw: without it pw-record wraps stdout in Sun AU, with
            // big-endian floats (seen in the Omarchy VM).
            .args(["--raw", "--rate", "16000", "--channels", "1", "--format", "f32"])
            .args(OUR_PROPS)
            .arg("-")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("starting pw-record (is PipeWire installed?)")?;
        Ok(Capture { child, started_us })
    }

    pub fn take_stdout(&mut self) -> Option<std::process::ChildStdout> {
        self.child.stdout.take()
    }

    /// Stops cleanly (SIGINT lets pw-record finish the file) and waits.
    pub fn stop(mut self) -> Result<()> {
        unsafe {
            libc::kill(self.child.id() as i32, libc::SIGINT);
        }
        // pw-record exits with 1 when interrupted, after finishing the file
        // (seen with PipeWire 1.6); that is the normal end here.
        let status = self.child.wait()?;
        log::debug!("pw-record ended with {status}");
        Ok(())
    }
}

/// The call's own playback stream (PipeWire node serial) for an app,
/// matched by process binary or application name, from `pw-dump`.
pub fn playback_stream_of(app_binary: &str, app_name: &str) -> Option<String> {
    let out = Command::new("pw-dump").arg("--no-colors").output().ok()?;
    let nodes: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    find_playback_stream(&nodes, app_binary, app_name)
}

/// Pure part of [`playback_stream_of`].
pub fn find_playback_stream(
    nodes: &serde_json::Value,
    app_binary: &str,
    app_name: &str,
) -> Option<String> {
    let binary = app_binary.rsplit('/').next().unwrap_or(app_binary).to_lowercase();
    let name = app_name.to_lowercase();
    nodes.as_array()?.iter().find_map(|o| {
        let props = o.get("info")?.get("props")?;
        if props.get("media.class")?.as_str()? != "Stream/Output/Audio" {
            return None;
        }
        let pb = props
            .get("application.process.binary")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        let an = props
            .get("application.name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        let hit = (!binary.is_empty() && pb == binary) || (!name.is_empty() && an == name);
        if !hit {
            return None;
        }
        let serial = props.get("object.serial")?;
        Some(match serial {
            serde_json::Value::Number(n) => n.to_string(),
            v => v.as_str()?.to_string(),
        })
    })
}

/// Reads all f32 samples from a raw capture until it ends.
pub fn read_f32(mut r: impl Read) -> Vec<f32> {
    let mut bytes = Vec::new();
    let _ = r.read_to_end(&mut bytes);
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_call_apps_playback_stream() {
        let nodes = serde_json::json!([
            {"id": 40, "type": "PipeWire:Interface:Node", "info": {"props": {
                "media.class": "Stream/Output/Audio", "application.name": "Spotify",
                "application.process.binary": "spotify", "object.serial": 101 }}},
            {"id": 41, "type": "PipeWire:Interface:Node", "info": {"props": {
                "media.class": "Stream/Input/Audio", "application.name": "Google Chrome",
                "application.process.binary": "chrome", "object.serial": 102 }}},
            {"id": 42, "type": "PipeWire:Interface:Node", "info": {"props": {
                "media.class": "Stream/Output/Audio", "application.name": "Google Chrome",
                "application.process.binary": "chrome", "object.serial": 103 }}},
            {"id": 43, "type": "PipeWire:Interface:Link", "info": null}
        ]);
        assert_eq!(
            find_playback_stream(&nodes, "/opt/google/chrome/chrome", "Google Chrome").as_deref(),
            Some("103")
        );
        assert_eq!(find_playback_stream(&nodes, "zoom", "Zoom"), None);
    }

    #[test]
    fn raw_samples() {
        let bytes: Vec<u8> = [0.5f32, -0.25].iter().flat_map(|f| f.to_le_bytes()).collect();
        assert_eq!(read_f32(&bytes[..]), vec![0.5, -0.25]);
    }
}

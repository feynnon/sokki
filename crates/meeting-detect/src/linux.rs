//! The Linux side: running `pw-dump`, reading `/proc`, asking Hyprland.
//! Everything that parses lives in [`crate::pipewire`] and
//! [`crate::hyprland`].

use std::collections::HashMap;
use std::io::{self, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::Value;

use crate::hyprland::{self, Window};
use crate::pipewire::{self, PwState};
use crate::{AppKind, AudioClient, ClientSink, Error, EventSource, MeetingApp, Result};

/// The executable of `pid`, from `/proc/<pid>/exe`.
pub(crate) fn exe_of(pid: u32) -> Option<String> {
    let path = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    let path = path.to_string_lossy();
    // The kernel appends this once the file was replaced, e.g. a browser
    // updated by pacman while it runs.
    Some(path.strip_suffix(" (deleted)").unwrap_or(&path).to_string())
}

/// The mic clients, with each pid's executable read once while its stream
/// lives: an exiting process loses its `/proc/<pid>/exe` before PipeWire
/// drops the stream, and the path would flip to the bare binary name.
fn clients_of(state: &PwState, exes: &mut HashMap<u32, Option<String>>) -> Vec<AudioClient> {
    let streams = state.mic_streams();
    exes.retain(|pid, _| streams.iter().any(|s| s.pid == Some(*pid)));
    pipewire::to_audio_clients(&streams, |pid| {
        exes.entry(pid).or_insert_with(|| exe_of(pid)).clone()
    })
}

pub(crate) fn audio_input_clients() -> Result<Vec<AudioClient>> {
    let out = Command::new("pw-dump")
        .arg("--no-colors")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| Error::PipeWire(e.to_string()))?;
    if !out.status.success() {
        return Err(Error::PipeWire(format!("exited with {}", out.status)));
    }
    let json = String::from_utf8_lossy(&out.stdout);
    let streams = pipewire::parse_dump(&json).map_err(|e| Error::PipeWire(e.to_string()))?;
    Ok(pipewire::to_audio_clients(&streams, exe_of))
}

/// Follows `pw-dump --monitor` and delivers the mic clients: the current
/// list once `pw-dump` has printed it, then the list whenever it changes. If `pw-dump` exits (PipeWire restarted, or is not running yet)
/// it is started again, waiting 1 s, then 2 s, up to 60 s between tries;
/// a run that produced output resets the wait. Not installed: gives up.
#[derive(Debug, Clone, Copy, Default)]
pub struct PwMonitor;

/// Stops the monitor when dropped: kills `pw-dump` and joins its thread.
pub struct PwMonitorGuard {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct Shared {
    run: Mutex<Run>,
    wake: Condvar,
}

/// Behind one lock, so a stop can never slip in between the check and the
/// spawn of a new `pw-dump` (that one would then run unkilled).
#[derive(Default)]
struct Run {
    stopped: bool,
    child: Option<Child>,
}

const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(60);

impl EventSource for PwMonitor {
    type Guard = PwMonitorGuard;

    fn start(self, sink: ClientSink) -> io::Result<PwMonitorGuard> {
        let shared = Arc::new(Shared::default());
        let thread = {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("pw-dump-monitor".into())
                .spawn(move || monitor(&shared, &sink))?
        };
        Ok(PwMonitorGuard {
            shared,
            thread: Some(thread),
        })
    }
}

impl Drop for PwMonitorGuard {
    fn drop(&mut self) {
        {
            let mut run = lock(&self.shared.run);
            run.stopped = true;
            if let Some(child) = run.child.as_mut() {
                let _ = child.kill();
            }
        }
        self.shared.wake.notify_all();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn lock(m: &Mutex<Run>) -> MutexGuard<'_, Run> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn monitor(shared: &Shared, sink: &ClientSink) {
    let mut backoff = BACKOFF_MIN;
    let mut last: Option<Vec<AudioClient>> = None;
    loop {
        let stdout = {
            let mut run = lock(&shared.run);
            if run.stopped {
                return;
            }
            let spawned = Command::new("pw-dump")
                .args(["--monitor", "--no-colors"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn();
            match spawned {
                Ok(mut child) => {
                    let out = child.stdout.take();
                    run.child = Some(child);
                    out
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    log::warn!("pw-dump not found; meeting detection is off");
                    return;
                }
                Err(e) => {
                    log::warn!("starting pw-dump failed: {e}");
                    None
                }
            }
        };
        let mut produced = false;
        if let Some(stdout) = stdout {
            let reader = BufReader::new(stdout);
            let mut state = PwState::default();
            let mut exes = HashMap::new();
            for batch in serde_json::Deserializer::from_reader(reader).into_iter::<Value>() {
                let batch = match batch {
                    Ok(b) => b,
                    Err(e) => {
                        log::warn!("reading pw-dump output failed: {e}");
                        break;
                    }
                };
                produced = true;
                state.apply(&batch);
                let now = clients_of(&state, &mut exes);
                if last.as_ref() != Some(&now) {
                    if !sink.send(now.clone()) {
                        return; // the watcher is gone
                    }
                    last = Some(now);
                }
            }
        }
        {
            let mut run = lock(&shared.run);
            if let Some(mut child) = run.child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            if run.stopped {
                return;
            }
        }
        // PipeWire went away, and with it every stream.
        if last.as_ref().is_some_and(|l| !l.is_empty()) {
            last = Some(Vec::new());
            if !sink.send(Vec::new()) {
                return;
            }
        }
        if produced {
            backoff = BACKOFF_MIN;
        }
        log::info!("pw-dump ended; starting it again in {backoff:?}");
        let run = lock(&shared.run);
        let (run, _) = shared
            .wake
            .wait_timeout_while(run, backoff, |r| !r.stopped)
            .unwrap_or_else(|e| e.into_inner());
        if run.stopped {
            return;
        }
        backoff = (backoff * 2).min(BACKOFF_MAX);
    }
}

/// Hyprland's request socket of this session.
fn hyprland_socket() -> Option<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
    let instance = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    Some(
        PathBuf::from(runtime)
            .join("hypr")
            .join(instance)
            .join(".socket.sock"),
    )
}

/// All windows, from Hyprland's `j/clients` request. Fails outside a
/// Hyprland session.
pub fn hyprland_windows() -> io::Result<Vec<Window>> {
    let path = hyprland_socket()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not in a Hyprland session"))?;
    let mut stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(b"j/clients")?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply)?;
    hyprland::parse_clients(&reply).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Names browser calls from Hyprland's windows. Asks Hyprland only when a
/// browser holds the mic; without Hyprland the browser name stays.
pub(crate) fn name_calls(apps: &mut [MeetingApp]) {
    if !apps.iter().any(|a| a.kind == AppKind::Browser) {
        return;
    }
    match hyprland_windows() {
        Ok(windows) => hyprland::name_calls(apps, &windows, exe_of),
        Err(e) => log::debug!("no Hyprland windows: {e}"),
    }
}

//! meeting-detect: notice when a meeting starts and ends, so the app can
//! *offer* to record. This crate never records anything and never starts a
//! recording on its own; it only reports what it sees.
//!
//! # How it works on macOS
//!
//! macOS 14+ exposes every Core Audio client process as an audio object
//! (`kAudioHardwarePropertyProcessObjectList`). Each one says whether it is
//! currently running audio input (`kAudioProcessPropertyIsRunningInput`) —
//! the same signal that lights the orange microphone dot in the menu bar.
//! [`audio_input_clients`] lists those processes with their pid, bundle id
//! and executable path. [`classify`] matches them against a table of known
//! meeting apps, by bundle id *and* by executable path: browser helper
//! processes, which are what actually hold the mic for Google Meet or Teams
//! in a browser, often report no bundle id at all. A [`Watcher`] polls every
//! two seconds and turns the result into debounced
//! [`MeetingEvent::Started`] / [`MeetingEvent::Ended`] events.
//!
//! # How it works on Linux
//!
//! PipeWire shows every stream that captures from a microphone as a node
//! with `media.class` `Stream/Input/Audio`, carrying the client's
//! `application.process.id`, `application.name` and
//! `application.process.binary` (Pulse clients on the node, native
//! PipeWire clients on their client object). `PwMonitor` follows
//! `pw-dump --monitor` and pushes the list of those clients whenever it
//! changes; the executable comes from `/proc/<pid>/exe`. Streams with
//! `stream.capture.sink` record what a speaker plays (our own recorder
//! does that), so they are left out. [`classify`] then matches by
//! executable name, as Linux has no bundle ids. The watcher is
//! event-driven: an idle machine costs no wakeups and spawns no processes.
//!
//! A meeting in a browser only shows as "Google Chrome" or "Firefox". On
//! Hyprland the watcher then asks for the window list (`j/clients`) and
//! takes the call service from the browser's windows: a web app's class
//! (`chrome-meet.google.com__-Default`) or a tab title (`Meet – …`). See
//! [`MeetingApp::call`] and [`MeetingApp::label`].
//!
//! # Credit
//!
//! The approach (Core Audio process objects, bundle-id prefixes plus a path
//! fallback for helper processes, a 20 s grace before treating a call as
//! over) is ported and adapted from `managers/meeting_detect.rs` in
//! **Lark**, a Handy fork by bkoleo: <https://github.com/bkoleo/lark>
//! (MIT licence).
//!
//! # Platforms
//!
//! - **macOS 14+:** Core Audio, polled every two seconds.
//! - **Linux:** PipeWire through `pw-dump` (needs `pw-dump` on `PATH`),
//!   event-driven; browser calls are named on Hyprland only.
//! - **Elsewhere:** [`audio_input_clients`] returns an empty list, so the
//!   watcher compiles and runs but never reports a meeting.
//!
//! The `pw-dump` and Hyprland parsers ([`pipewire`], [`hyprland`]) are
//! pure and compiled on every OS, so their tests run everywhere.

mod apps;
pub mod hyprland;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
pub mod pipewire;
mod watcher;

pub use apps::{classify, classify_all, known_apps, AppKind, MatchedBy, MeetingApp};
#[cfg(target_os = "linux")]
pub use linux::{hyprland_windows, PwMonitor, PwMonitorGuard};
pub use watcher::{
    ClientSink, ClientSource, Clock, Detector, DetectorState, EventSource, MeetingEvent,
    SystemClock, SystemSource, Watcher, WatcherConfig,
};

use std::fmt;

/// A process that is currently capturing audio input (holding the mic).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioClient {
    pub pid: u32,
    /// Bundle id as Core Audio reports it. Often `None` for helper processes
    /// (browser renderers, XPC services, command-line tools). Always `None`
    /// on Linux.
    pub bundle_id: Option<String>,
    /// Executable path from `proc_pidpath` (macOS) or `/proc/<pid>/exe`
    /// (Linux), if the process still exists. On Linux, when the link can't
    /// be read, PipeWire's `application.process.binary` (a bare name).
    pub exe_path: Option<String>,
    /// Human-readable name. macOS: the outermost `*.app` in the path, else
    /// the executable name, else the bundle id, else `pid N`. Linux:
    /// PipeWire's `application.name`, else the executable name.
    pub name: String,
    /// The process macOS holds responsible for this one (the app that owns
    /// an XPC service or helper), when it differs from `pid`. This is what
    /// TCC uses to attribute mic use, and it tells a Safari WebKit helper
    /// apart from a WebKit helper of some other app.
    pub responsible_pid: Option<u32>,
    /// Executable path of `responsible_pid`.
    pub responsible_path: Option<String>,
    /// Linux: the program name PipeWire reports
    /// (`application.process.binary`). A second chance when `exe_path` names
    /// a wrapper or translator instead of the app (seen: Rosetta in an x86
    /// container, a shared Electron).
    pub binary: Option<String>,
}

impl AudioClient {
    /// A client with just pid, bundle id and path; the name is derived.
    /// Useful for fixtures and fake sources.
    pub fn new(pid: u32, bundle_id: Option<&str>, exe_path: Option<&str>) -> Self {
        let bundle_id = bundle_id.map(str::to_owned);
        let exe_path = exe_path.map(str::to_owned);
        let name = display_name(pid, bundle_id.as_deref(), exe_path.as_deref());
        AudioClient {
            pid,
            bundle_id,
            exe_path,
            name,
            responsible_pid: None,
            responsible_path: None,
            binary: None,
        }
    }

    /// Builder-style: set the responsible process.
    pub fn with_responsible(mut self, pid: u32, path: Option<&str>) -> Self {
        self.responsible_pid = Some(pid);
        self.responsible_path = path.map(str::to_owned);
        self
    }
}

/// The outermost `Foo.app` component of a path, without `.app`; else the
/// file name; else the bundle id; else `pid N`.
pub(crate) fn display_name(pid: u32, bundle_id: Option<&str>, exe_path: Option<&str>) -> String {
    if let Some(path) = exe_path {
        if let Some(app) = path
            .split('/')
            .find(|c| c.len() > 4 && c.to_ascii_lowercase().ends_with(".app"))
        {
            return app[..app.len() - 4].to_string();
        }
        if let Some(file) = path.rsplit('/').next().filter(|f| !f.is_empty()) {
            return file.to_string();
        }
    }
    if let Some(bid) = bundle_id {
        return bid.to_string();
    }
    format!("pid {pid}")
}

/// Why listing audio clients failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// This macOS has no Core Audio process objects (needs macOS 14 Sonoma).
    Unsupported,
    /// A Core Audio call returned a non-zero `OSStatus`.
    CoreAudio { call: &'static str, status: i32 },
    /// `pw-dump` could not be run or its output not read.
    PipeWire(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unsupported => write!(
                f,
                "Core Audio process objects are not available (macOS 14 or later required)"
            ),
            Error::CoreAudio { call, status } => {
                let bytes = status.to_be_bytes();
                if bytes.iter().all(|b| b.is_ascii_graphic()) {
                    write!(
                        f,
                        "Core Audio {call} failed: '{}' ({status})",
                        String::from_utf8_lossy(&bytes)
                    )
                } else {
                    write!(f, "Core Audio {call} failed: OSStatus {status}")
                }
            }
            Error::PipeWire(reason) => write!(f, "pw-dump failed: {reason}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Every process currently capturing audio input, in Core Audio's order.
///
/// Needs no microphone permission: it reads Core Audio's process list, it
/// does not open any input. Returns [`Error::Unsupported`] before macOS 14.
#[cfg(target_os = "macos")]
pub fn audio_input_clients() -> Result<Vec<AudioClient>> {
    macos::audio_input_clients()
}

/// Every process currently capturing from a microphone, from a one-shot
/// `pw-dump` (for the CLI and diagnostics; the watcher follows
/// `pw-dump --monitor` instead). Streams recording a speaker are left out.
#[cfg(target_os = "linux")]
pub fn audio_input_clients() -> Result<Vec<AudioClient>> {
    linux::audio_input_clients()
}

/// Stub for other platforms: nothing is ever detected.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn audio_input_clients() -> Result<Vec<AudioClient>> {
    Ok(Vec::new())
}

/// Every process currently playing audio (Core Audio's
/// `kAudioProcessPropertyIsRunningOutput`). Like [`audio_input_clients`] it
/// opens nothing. macOS only; elsewhere always empty.
#[cfg(target_os = "macos")]
pub fn audio_output_clients() -> Result<Vec<AudioClient>> {
    macos::audio_output_clients()
}

/// Stub for other platforms: nothing is ever reported as playing.
#[cfg(not(target_os = "macos"))]
pub fn audio_output_clients() -> Result<Vec<AudioClient>> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_prefers_outermost_app() {
        let p = "/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Versions/131.0/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper";
        assert_eq!(display_name(1, None, Some(p)), "Google Chrome");
        assert_eq!(
            display_name(1, None, Some("/opt/homebrew/bin/ffmpeg")),
            "ffmpeg"
        );
        assert_eq!(display_name(1, Some("com.x.y"), None), "com.x.y");
        assert_eq!(display_name(7, None, None), "pid 7");
    }

    #[test]
    fn error_display_decodes_fourcc() {
        let e = Error::CoreAudio {
            call: "GetPropertyData",
            status: i32::from_be_bytes(*b"who?"),
        };
        assert!(e.to_string().contains("'who?'"));
    }
}

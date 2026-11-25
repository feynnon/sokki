//! "Keep what I write" (Settings › Developer mode): the text the user sends
//! or leaves in any app, with the app and the time, kept on this Mac only,
//! in the same format as the phone keyboard's journal. Off by default.
//!
//! How it reads (macOS): a thread of its own reads the focused text field
//! through Accessibility (see [`mac`] for what it never reads). It reads
//! right after a key went down and again shortly after, and once a second
//! otherwise. Sokki's keyboard hook only counts key presses for it
//! (`sokki_keys::key_downs`), so the hook never waits on this thread, and
//! there is no second event tap: a second tap is what froze all input on
//! 2026-11-19.
//!
//! [`store`] holds the files, [`entry`] decides when a piece of writing is
//! finished.

pub mod entry;
pub mod learn;
#[cfg(target_os = "macos")]
pub mod mac;
pub mod store;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub use entry::{Dictated, Entry};
pub use store::Stats;

/// Apps never read: password managers, terminals (their "field" is the
/// whole window).
pub const ALWAYS_EXCLUDED: &[&str] = &[
    "com.1password.1password",
    "com.agilebits.onepassword7",
    "com.bitwarden.desktop",
    "com.apple.Passwords",
    "com.apple.keychainaccess",
    "com.lastpass.LastPass",
    "com.dashlane.Dashlane",
    "in.sinew.Enpass-Desktop",
    "org.keepassxc.keepassxc",
    "com.apple.Terminal",
    "com.googlecode.iterm2",
    "com.mitchellh.ghostty",
    "dev.warp.Warp-Stable",
    "net.kovidgoyal.kitty",
    "org.alacritty",
    "com.github.wez.wezterm",
];

/// ms since 1970.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// How often the thread looks for news while on.
const TICK: Duration = Duration::from_millis(40);
/// A second read this long after a key, for the key that arrived meanwhile.
const AFTER_KEY: Duration = Duration::from_millis(80);
/// Reads without any key (focus moved by the mouse, a paste, no hook).
const IDLE_READ: Duration = Duration::from_secs(1);

struct Shared {
    enabled: AtomicBool,
    /// Extra apps the user excluded (bundle ids).
    excluded: Mutex<Vec<String>>,
    dictated: Mutex<Vec<Dictated>>,
}

/// The running journal. Cheap to clone.
#[derive(Clone)]
pub struct Journal {
    dir: PathBuf,
    shared: Arc<Shared>,
}

/// How the journal runs.
pub struct Options {
    /// Where the files go (`<app data>/typing_log`).
    pub dir: PathBuf,
    /// Sokki's own process, never read.
    pub own_pid: i32,
    /// Test mode: read this process's focused field instead of the front app's.
    pub test_pid: Option<i32>,
    /// Whether Accessibility is granted now; without it nothing is read.
    pub trusted: fn() -> bool,
    /// Key presses so far (`sokki_keys::key_downs`; test mode: the probe's).
    pub key_downs: fn() -> u64,
}

impl Journal {
    /// Starts the reading thread, switched off until [`Journal::set_enabled`].
    pub fn start(options: Options) -> Journal {
        let journal = Journal {
            dir: options.dir.clone(),
            shared: Arc::new(Shared {
                enabled: AtomicBool::new(false),
                excluded: Mutex::new(Vec::new()),
                dictated: Mutex::new(Vec::new()),
            }),
        };
        #[cfg(target_os = "macos")]
        {
            let j = journal.clone();
            let spawned = std::thread::Builder::new()
                .name("journal".into())
                .spawn(move || j.run(options));
            if let Err(e) = spawned {
                log::error!("journal: could not start its thread: {e}");
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = options;
        journal
    }

    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    pub fn set_enabled(&self, on: bool) {
        let was = self.shared.enabled.swap(on, Ordering::SeqCst);
        if was != on {
            log::info!("Keep what I write: {}", if on { "on" } else { "off" });
        }
    }

    pub fn enabled(&self) -> bool {
        self.shared.enabled.load(Ordering::SeqCst)
    }

    pub fn set_excluded(&self, apps: Vec<String>) {
        if let Ok(mut x) = self.shared.excluded.lock() {
            *x = apps;
        }
    }

    /// Sokki pasted a dictation (raw: the speech model's text).
    pub fn note_dictation(&self, raw: String, pasted: String) {
        if !self.enabled() {
            return;
        }
        let pasted = (pasted != raw).then_some(pasted);
        if let Ok(mut d) = self.shared.dictated.lock() {
            d.push(Dictated {
                raw,
                pasted,
                at: now_ms(),
            });
        }
    }

    pub fn stats(&self) -> Stats {
        store::stats(&self.dir)
    }

    pub fn delete_all(&self) -> std::io::Result<()> {
        store::delete_all(&self.dir)
    }

    #[cfg(target_os = "macos")]
    fn run(self, options: Options) {
        use std::time::Instant;
        let mut reader = mac::Reader::new(options.own_pid, options.test_pid);
        let mut tracker: entry::Tracker<mac::FieldKey> = entry::Tracker::new();
        let mut keys = (options.key_downs)();
        // Key presses since the last read, for telling typed text from pasted.
        let mut keys_at_read = keys;
        let mut next_read = Instant::now();
        let mut second_read: Option<Instant> = None;
        let mut slow_reads = 0u32;
        if options.test_pid.is_some() {
            log::info!(
                "journal test: thread running, Accessibility {}",
                if (options.trusted)() {
                    "granted"
                } else {
                    "not granted"
                }
            );
        }
        loop {
            std::thread::sleep(TICK);
            if !self.enabled() || !(options.trusted)() {
                if let Some(e) = tracker.leave() {
                    self.write(&e);
                }
                if let Ok(mut d) = self.shared.dictated.lock() {
                    d.clear();
                }
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
            let now = Instant::now();
            let pressed = (options.key_downs)();
            let key = pressed != keys;
            keys = pressed;
            let due = key || now >= next_read || second_read.is_some_and(|t| now >= t);
            if !due {
                continue;
            }
            second_read = key.then(|| now + AFTER_KEY);
            next_read = now + IDLE_READ;

            let excluded: Vec<String> = ALWAYS_EXCLUDED
                .iter()
                .map(|s| s.to_string())
                .chain(
                    self.shared
                        .excluded
                        .lock()
                        .map(|x| x.clone())
                        .unwrap_or_default(),
                )
                .collect();
            if let Ok(mut d) = self.shared.dictated.lock() {
                for item in d.drain(..) {
                    tracker.dictated(item);
                }
            }
            let started = Instant::now();
            let read = reader.read(&excluded);
            let took = started.elapsed();
            if took > Duration::from_millis(200) {
                slow_reads += 1;
                if slow_reads <= 5 {
                    log::warn!("journal: a read took {took:?} (an app answered slowly)");
                }
            }
            let read = match read {
                mac::Read::Field(o) => Some(o),
                mac::Read::NoField => None,
                mac::Read::NoAnswer => continue,
            };
            let pressed_since = keys.saturating_sub(keys_at_read);
            keys_at_read = keys;
            for e in tracker.observe(now_ms(), pressed_since, read) {
                self.write(&e);
            }
        }
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn write(&self, e: &Entry) {
        match store::append(&self.dir, e) {
            Ok(()) => log::debug!(
                "journal: kept {} characters from {}",
                e.text.chars().count(),
                e.app
            ),
            Err(err) => log::warn!("journal: could not write an entry: {err}"),
        }
    }
}

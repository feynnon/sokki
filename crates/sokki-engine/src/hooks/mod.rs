//! Hooks: the person's own commands, run by Sokki on events. Sokki brings
//! the facts and the events; hooks bring the use cases. The contract is
//! the one of git and Claude Code hooks; HOOKS.md is the guide.
//!
//! - **Config:** `~/.config/sokki/hooks.toml` on every platform, with the
//!   scripts next to it, so the folder can live in git ([`config`]).
//! - **Events:** each host calls [`emit`] at the moment something happened
//!   ([`event`] lists them). A schedule fires from here ([`cron`]).
//! - **Runner:** one thread per host ([`start`]) with a queue in SQLite
//!   ([`store`]): one hook at a time, one retry, a log file per run. A
//!   meeting outranks every hook: none starts while one records or is
//!   processed, and a running one is paused until it is over ([`exec`]).
//! - **Free code:** a hook in hooks.toml runs; there is no approval. An
//!   agent adds one by writing hooks.toml and its script. On macOS a hook
//!   runs apart from Sokki, so it never gets Sokki's privacy grants
//!   ([`spawn`]). `scope` optionally narrows what goes on stdin
//!   ([`content`]); without one a hook gets everything.
//!
//! The synchronous `dictation.transform` hook (the text about to be pasted
//! in, other text out) stays where it was: `<app data>/hooks/transcription`
//! on the Mac (`src-tauri/src/transcription_hook.rs`).

pub mod config;
pub mod content;
pub mod cron;
pub mod event;
pub mod exec;
mod runner;
pub mod spawn;
pub mod store;
pub mod tools;

pub use config::{Config, Hook, Match, Scope, CONFIG_FILE};
pub use event::{Content, HookEvent, Meta, ScheduleInfo, Task, EVENTS};
pub use exec::Outcome;

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// Whether a meeting needs the computer right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    Free,
    /// A meeting records: nothing else may disturb it.
    Recording,
    /// A meeting is being turned into a note (its models are loaded).
    Processing,
}

/// Where hooks are configured and where their state lives.
#[derive(Debug, Clone, PartialEq)]
pub struct Paths {
    /// The folder of `hooks.toml`; hooks run in it.
    pub config_dir: PathBuf,
    /// `hooks.sqlite` and `logs/`, in the host's app data.
    pub state_dir: PathBuf,
}

impl Paths {
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join(CONFIG_FILE)
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.state_dir.join("logs")
    }
}

/// `$SOKKI_HOOKS_DIR`, else `$XDG_CONFIG_HOME/sokki`, else
/// `~/.config/sokki`; the same on the Mac and on Linux.
pub fn default_config_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("SOKKI_HOOKS_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    if let Some(d) = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|d| d.is_absolute())
    {
        return d.join("sokki");
    }
    crate::process::expand_home("~/.config/sokki")
}

/// What the runner tells its host, which shows it (the Mac: a notice;
/// Linux: the error event and a notification).
#[derive(Debug, Clone, PartialEq)]
pub enum Report {
    /// The hook failed twice (the run and its retry).
    Failed {
        hook: String,
        what: String,
        log: PathBuf,
    },
    /// `hooks.toml` has a mistake; no hook runs until it is fixed.
    ConfigInvalid { error: String },
    /// The hook ran fine (a notice about it can go).
    Succeeded { hook: String },
}

type GateFn = dyn Fn() -> Gate + Send + Sync;
type ReportFn = dyn Fn(Report) + Send + Sync;
type EnvFn = dyn Fn() -> Vec<(String, String)> + Send + Sync;

/// What a host hands the runner.
#[derive(Clone)]
pub struct Host {
    pub paths: Paths,
    pub gate: Arc<GateFn>,
    pub report: Arc<ReportFn>,
    /// Extra environment for every hook, read at each run (the notes
    /// folder can change in the settings).
    pub env: Arc<EnvFn>,
}

struct Shared {
    paths: Paths,
    incoming: Mutex<VecDeque<HookEvent>>,
    /// Events that wait until nothing changed for a while (own notes are
    /// saved as they are typed), by key.
    debounced: Mutex<HashMap<String, (Instant, HookEvent)>>,
    wake: Condvar,
    wake_lock: Mutex<bool>,
}

impl Shared {
    fn poke(&self) {
        let mut woken = self
            .wake_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        *woken = true;
        self.wake.notify_all();
    }

    fn sleep(&self, at_most: Duration) {
        let woken = self
            .wake_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (mut woken, _) = self
            .wake
            .wait_timeout_while(woken, at_most, |w| !*w)
            .unwrap_or_else(PoisonError::into_inner);
        *woken = false;
    }
}

static RUNNER: OnceLock<Arc<Shared>> = OnceLock::new();

type Listener = Box<dyn Fn(&HookEvent) + Send + Sync>;
static LISTENERS: Mutex<Vec<Listener>> = Mutex::new(Vec::new());

/// Calls `f` with every event (without content), for `sokki events`.
pub fn subscribe(f: impl Fn(&HookEvent) + Send + Sync + 'static) {
    LISTENERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(Box::new(f));
}

fn tell_listeners(event: &HookEvent) {
    let bare = event.without_content();
    for f in LISTENERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
    {
        f(&bare);
    }
}

/// Something happened. Returns at once: matching hooks are queued and run
/// on the runner's thread. Before [`start`] (or in a process without a
/// runner) only listeners hear of it.
pub fn emit(event: HookEvent) {
    log::info!("Hook event {} {}", event.event, event.id);
    tell_listeners(&event);
    if let Some(shared) = RUNNER.get() {
        shared
            .incoming
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(event);
        shared.poke();
    }
}

/// [`emit`], once `key` has seen no new event for `quiet`: each new one
/// replaces the waiting one and starts the wait again.
pub fn emit_after_quiet(key: &str, quiet: Duration, event: HookEvent) {
    let Some(shared) = RUNNER.get() else {
        tell_listeners(&event);
        return;
    };
    shared
        .debounced
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(key.to_string(), (Instant::now() + quiet, event));
    shared.poke();
}

/// How long own notes rest before `own_notes.saved` fires.
pub const OWN_NOTES_QUIET: Duration = Duration::from_secs(30);

/// At quit: events still waiting in memory (not queued yet, or resting
/// like own notes) go into the queue now, so they run at the next start
/// instead of being lost. Call it from the host's quit path.
pub fn flush() {
    let Some(shared) = RUNNER.get() else { return };
    let mut events: Vec<HookEvent> = shared
        .incoming
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .drain(..)
        .collect();
    let resting: Vec<HookEvent> = shared
        .debounced
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .drain()
        .map(|(_, (_, e))| e)
        .collect();
    for e in resting {
        tell_listeners(&e);
        events.push(e);
    }
    if events.is_empty() {
        return;
    }
    let cfg = match config::load(&shared.paths.config_dir) {
        Ok(c) => c,
        Err(e) => return log::warn!("Hooks: at quit, {} event(s) lost: {e:#}", events.len()),
    };
    match store::Store::open(&shared.paths.state_dir) {
        Ok(store) => {
            log::info!("Hooks: {} event(s) queued at quit", events.len());
            runner::queue_into(&store, &cfg, events);
            // If the process goes on after all, the runner takes them now.
            shared.poke();
        }
        Err(e) => log::warn!("Hooks: at quit, {} event(s) lost: {e:#}", events.len()),
    }
}

/// Whether this process runs hooks.
pub fn started() -> bool {
    RUNNER.get().is_some()
}

/// Starts the runner thread. Once per process; a second call is refused.
pub fn start(host: Host) -> anyhow::Result<()> {
    let store = store::Store::open(&host.paths.state_dir)?;
    store.recover()?;
    let shared = Arc::new(Shared {
        paths: host.paths.clone(),
        incoming: Mutex::new(VecDeque::new()),
        debounced: Mutex::new(HashMap::new()),
        wake: Condvar::new(),
        wake_lock: Mutex::new(false),
    });
    RUNNER
        .set(shared.clone())
        .map_err(|_| anyhow::anyhow!("the hooks runner is already running"))?;
    log::info!(
        "Hooks: {} (state in {})",
        host.paths.config_file().display(),
        host.paths.state_dir.display()
    );
    std::thread::Builder::new()
        .name("hooks".into())
        .spawn(move || runner::run(host, store, shared))?;
    Ok(())
}

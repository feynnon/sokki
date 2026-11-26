//! The thread that runs autocorrect while its switch is on (off until the user turns it on).
//!
//! The keyboard hook only counts key presses; this thread notices a new count and reads the
//! focused field. It sleeps while the switch is off or Accessibility isn't granted, so the
//! hook and the permission watch in `src-tauri/src/accessibility.rs` stay exactly as they are.
//! SPDX-License-Identifier: GPL-3.0-only

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::{Autocorrect, Outcome};

/// How often the thread looks at the key count.
const TICK: Duration = Duration::from_millis(15);
/// How long after a key the thread keeps looking while the app hasn't shown it yet.
const CATCH_UP: Duration = Duration::from_millis(300);

/// Called with every fix, skip and undo: the app, what happened, and how long it took.
pub type Report = Box<dyn Fn(&str, &Outcome, Duration) + Send>;

/// What the host gives the thread.
pub struct Options {
    /// Loads the phone's model ([`sokki_typos::Typos::load`]): only once the switch is first
    /// turned on, so Sokki doesn't carry its ~50 MB while autocorrect is off.
    pub load: Box<dyn FnOnce() -> Result<sokki_typos::Typos, String> + Send>,
    /// Where the words kept by undoing a fix live.
    pub kept_file: Option<std::path::PathBuf>,
    /// Key presses so far (`sokki_keys::key_downs`).
    pub keys: fn() -> u64,
    /// Whether Sokki has Accessibility right now.
    pub trusted: fn() -> bool,
    /// The user's switch (read about once a second).
    pub enabled: Box<dyn Fn() -> bool + Send>,
    /// The app in front: (pid, bundle id), from NSWorkspace.
    pub front: fn() -> Option<(i32, String)>,
    /// Called with every fix, skip and undo, for the log and counts.
    pub report: Report,
    /// The letter keys of the layout in use, row by row ([`sokki_typos::Typos::set_keyboard`]);
    /// read every few seconds while autocorrect is on. None keeps what the model has.
    pub keyboard: Box<dyn Fn() -> Option<[String; 3]> + Send>,
}

/// The app list, shared with the thread.
#[derive(Clone, Default)]
pub struct Handle {
    excluded: Arc<Mutex<Vec<String>>>,
}

impl Handle {
    /// Apps the user excluded (bundle ids), besides [`crate::NEVER`] and Apple's own.
    pub fn set_excluded(&self, apps: Vec<String>) {
        if let Ok(mut x) = self.excluded.lock() {
            *x = apps;
        }
    }
}

/// Starts the thread. It does nothing until `enabled` says yes.
pub fn start(options: Options) -> Handle {
    let handle = Handle::default();
    let h = handle.clone();
    let spawned = std::thread::Builder::new()
        .name("autocorrect".into())
        .spawn(move || run(h, options));
    if let Err(e) = spawned {
        log::error!("autocorrect: could not start its thread: {e}");
    }
    handle
}

fn run(h: Handle, o: Options) {
    let mut load = Some(o.load);
    let mut ac: Option<Autocorrect> = None;
    let mut seen = (o.keys)();
    let mut on = false;
    let mut checked = Instant::now() - Duration::from_secs(10);
    let mut key_at = Instant::now();
    let mut rows: Option<[String; 3]> = None;
    let mut rows_at = Instant::now() - Duration::from_secs(60);
    loop {
        std::thread::sleep(TICK);
        if checked.elapsed() >= Duration::from_secs(1) {
            checked = Instant::now();
            let now_on = (o.enabled)();
            if now_on != on {
                log::info!("autocorrect: {}", if now_on { "on" } else { "off" });
                on = now_on;
            }
        }
        if !on || !(o.trusted)() {
            if let Some(a) = ac.as_mut() {
                a.reset();
            }
            seen = (o.keys)();
            // off: wake twice a second, not 66 times
            std::thread::sleep(Duration::from_millis(485));
            continue;
        }
        if ac.is_none() {
            let Some(f) = load.take() else {
                std::thread::sleep(Duration::from_secs(5));
                continue;
            };
            let t = Instant::now();
            match f() {
                Ok(typos) => {
                    log::info!(
                        "autocorrect: model loaded in {} ms",
                        t.elapsed().as_millis()
                    );
                    ac = Some(Autocorrect::new(typos, o.kept_file.clone()));
                }
                Err(e) => {
                    log::error!("autocorrect: can't load the model, staying off: {e}");
                    continue;
                }
            }
        }
        let Some(ac) = ac.as_mut() else { continue };
        if rows_at.elapsed() >= Duration::from_secs(5) {
            rows_at = Instant::now();
            let now_rows = (o.keyboard)();
            if now_rows.is_some() && now_rows != rows {
                if let Some(r) = &now_rows {
                    ac.set_keyboard(r);
                    log::info!(
                        "autocorrect: keyboard rows set ({} letters)",
                        r.iter().map(|x| x.trim().chars().count()).sum::<usize>()
                    );
                }
                rows = now_rows;
            }
        }
        if checked.elapsed() < TICK {
            // just checked the switch: also get the app in front ready (once per app)
            let excluded = h.excluded.lock().map(|x| x.clone()).unwrap_or_default();
            crate::mac::prepare((o.front)(), &excluded);
        }
        let now = (o.keys)();
        let catching_up = ac.stale() && key_at.elapsed() < CATCH_UP;
        if now == seen && !catching_up {
            continue;
        }
        if now != seen {
            seen = now;
            key_at = Instant::now();
        }
        let excluded = h.excluded.lock().map(|x| x.clone()).unwrap_or_default();
        let started = Instant::now();
        let Some(field) = crate::mac::focused((o.front)(), &excluded) else {
            ac.reset();
            continue;
        };
        let outcome = ac.on_key(&field, &o.keys);
        if outcome != Outcome::Nothing {
            (o.report)(crate::Field::app(&field), &outcome, started.elapsed());
        }
    }
}

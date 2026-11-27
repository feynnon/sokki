//! Holding the user's keys for a moment, so autocorrect can swap a word without a key landing
//! in the middle of it (Robin, 27 Nov 2026: a letter typed during a fix ended up after the
//! wrong word).
//!
//! While a hold is on, the macOS hook keeps every key press and release (except shortcuts
//! with ⌘ or ⌃) instead of passing it on, and [`release_keys`] sends them to the app, in the
//! order they were typed. The hook counts a key as typed when it comes in, held or not.
//!
//! A hold ends by itself after [`MAX_HOLD`]: then the next key sends the held ones on first,
//! so a stuck caller can never keep the keyboard. A ⌘ or ⌃ shortcut ends it the same way.
//! SPDX-License-Identifier: GPL-3.0-only

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The longest a hold lasts.
pub const MAX_HOLD: Duration = Duration::from_millis(300);

/// What the hook knows when a hold starts.
#[derive(Debug, Clone, Copy)]
pub struct Held {
    /// Keys counted so far ([`crate::key_downs`]).
    pub keys: u64,
    /// How long ago the last key went on to an app.
    pub since_last_key: Duration,
}

#[cfg(target_os = "macos")]
struct Ev(objc2_core_foundation::CFRetained<objc2_core_graphics::CGEvent>);
// SAFETY: a CGEvent is a CoreFoundation object; retaining, releasing and posting it from
// another thread is allowed.
#[cfg(target_os = "macos")]
unsafe impl Send for Ev {}

struct State {
    /// the hook is running and can hold keys
    running: bool,
    /// since when, and the app (pid) that gets the held keys
    hold: Option<(Instant, i32)>,
    #[cfg(target_os = "macos")]
    queue: Vec<Ev>,
    last_passed: Option<Instant>,
}

static STATE: Mutex<State> = Mutex::new(State {
    running: false,
    hold: None,
    #[cfg(target_os = "macos")]
    queue: Vec::new(),
    last_passed: None,
});

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn set_running(running: bool) {
    let mut s = state();
    s.running = running;
    if !running {
        s.hold = None;
    }
}

/// Starts holding the user's keys for the app `pid`. `None` when the hook can't hold keys
/// (not running, or not macOS).
pub fn hold_keys(pid: i32) -> Option<Held> {
    let mut s = state();
    if !s.running || !cfg!(target_os = "macos") {
        return None;
    }
    s.hold = Some((Instant::now(), pid));
    Some(Held {
        keys: crate::key_downs(),
        since_last_key: s.last_passed.map_or(Duration::MAX, |t| t.elapsed()),
    })
}

/// Ends the hold and sends the held keys to the app, in order. Keys that come while they're
/// sent are held too and sent after them.
pub fn release_keys() {
    let mut sent = 0;
    let mut since = None;
    loop {
        #[cfg(target_os = "macos")]
        let (batch, pid) = {
            let mut s = state();
            let Some((t, pid)) = s.hold else { break };
            since.get_or_insert(t);
            if s.queue.is_empty() {
                s.hold = None;
                break;
            }
            (std::mem::take(&mut s.queue), pid)
        };
        #[cfg(not(target_os = "macos"))]
        break;
        #[cfg(target_os = "macos")]
        {
            sent += batch.len();
            post(batch, pid);
        }
    }
    if let Some(t) = since {
        let ms = t.elapsed().as_millis();
        if sent > 0 || ms > 100 {
            log::debug!("keys held {ms} ms, {sent} key events sent on");
        }
    }
}

#[cfg(target_os = "macos")]
const REPOSTED: i64 = 0x50_4b_4b_49; // "SOKKI", roughly

/// Whether the hook sent this event on after a hold (it must pass untouched).
#[cfg(target_os = "macos")]
pub(crate) fn reposted(event: &objc2_core_graphics::CGEvent) -> bool {
    use objc2_core_graphics::{CGEvent, CGEventField};
    CGEvent::integer_value_field(Some(event), CGEventField::EventSourceUserData) == REPOSTED
}

#[cfg(target_os = "macos")]
fn post(batch: Vec<Ev>, pid: i32) {
    use objc2_core_graphics::{CGEvent, CGEventField};
    for e in batch {
        CGEvent::set_integer_value_field(Some(&e.0), CGEventField::EventSourceUserData, REPOSTED);
        CGEvent::post_to_pid(pid, Some(&e.0));
    }
}

/// The hook, for a key press or release the user typed (not one Sokki posted): `true` when
/// it is held and must not pass. `shortcut`: ⌘ or ⌃ is down.
#[cfg(target_os = "macos")]
pub(crate) fn on_key_event(
    event: &objc2_core_graphics::CGEvent,
    key_down: bool,
    shortcut: bool,
) -> bool {
    use objc2_core_graphics::CGEvent;
    let mut s = state();
    if let Some((since, pid)) = s.hold {
        if since.elapsed() < MAX_HOLD && !shortcut {
            if let Some(copy) = CGEvent::new_copy(Some(event)) {
                s.queue.push(Ev(copy));
                return true;
            }
        }
        // too long, or a shortcut: what is held goes first, then this key
        let batch = std::mem::take(&mut s.queue);
        s.hold = None;
        drop(s);
        log::warn!(
            "keys held {} ms: sent on before the hold ended",
            since.elapsed().as_millis()
        );
        post(batch, pid);
        s = state();
    }
    if key_down {
        s.last_passed = Some(Instant::now());
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_hold_without_the_hook() {
        set_running(false);
        assert!(hold_keys(1).is_none());
        release_keys();
    }
}

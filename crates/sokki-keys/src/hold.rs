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

/// A key with modifiers that the hook takes for Sokki for a short time instead of passing it
/// on (⌃⌥W while the dictionary pill shows). The rest of the time the key is the app's.
struct Catch {
    keycode: u16,
    /// ⇧ ⌃ ⌥ ⌘ as CGEventFlags bits; exactly these must be down
    modifiers: u64,
    until: Instant,
    on: std::sync::Arc<dyn Fn() + Send + Sync>,
}

static CATCH: Mutex<Option<Catch>> = Mutex::new(None);

/// ⇧ ⌃ ⌥ ⌘ in CGEventFlags.
pub const SHIFT: u64 = 0x2_0000;
pub const CONTROL: u64 = 0x4_0000;
pub const OPTION: u64 = 0x8_0000;
pub const COMMAND: u64 = 0x10_0000;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MODIFIERS: u64 = SHIFT | CONTROL | OPTION | COMMAND;

/// For `for_`, the key `keycode` with exactly `modifiers` down goes to `on` (on its own
/// thread) instead of the app. A newer call replaces the older one.
pub fn catch_key(
    keycode: u16,
    modifiers: u64,
    for_: Duration,
    on: impl Fn() + Send + Sync + 'static,
) {
    *CATCH.lock().unwrap_or_else(|e| e.into_inner()) = Some(Catch {
        keycode,
        modifiers,
        until: Instant::now() + for_,
        on: std::sync::Arc::new(on),
    });
}

/// The caught key goes back to the apps.
pub fn stop_catching() {
    *CATCH.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// The hook: whether this key is caught for Sokki (and must not pass).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn caught(keycode: u16, flags: u64, key_down: bool) -> bool {
    let mut c = CATCH.lock().unwrap_or_else(|e| e.into_inner());
    let Some(k) = c.as_ref() else { return false };
    if Instant::now() > k.until {
        *c = None;
        return false;
    }
    if k.keycode != keycode || flags & MODIFIERS != k.modifiers {
        return false;
    }
    if key_down {
        let on = k.on.clone();
        std::thread::spawn(move || on());
    }
    true
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
    keycode: u16,
    flags: u64,
    key_down: bool,
) -> bool {
    use objc2_core_graphics::CGEvent;
    if caught(keycode, flags, key_down) {
        return true;
    }
    let shortcut = flags & (CONTROL | COMMAND) != 0;
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
    fn a_caught_key_needs_its_exact_modifiers_and_ends() {
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let h = hits.clone();
        catch_key(13, CONTROL | OPTION, Duration::from_secs(5), move || {
            h.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        assert!(!caught(13, CONTROL, true), "⌃W alone is the app's");
        assert!(!caught(13, CONTROL | OPTION | SHIFT, true));
        assert!(!caught(12, CONTROL | OPTION, true));
        assert!(caught(13, CONTROL | OPTION, true));
        assert!(caught(13, CONTROL | OPTION, false));
        stop_catching();
        assert!(!caught(13, CONTROL | OPTION, true));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
        catch_key(13, CONTROL | OPTION, Duration::ZERO, || {});
        std::thread::sleep(Duration::from_millis(2));
        assert!(
            !caught(13, CONTROL | OPTION, true),
            "a catch ends by itself"
        );
    }

    #[test]
    fn no_hold_without_the_hook() {
        set_running(false);
        assert!(hold_keys(1).is_none());
        release_keys();
    }
}

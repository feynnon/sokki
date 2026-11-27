//! The small Sokki window that asks when a call starts ("Zoom meeting
//! started": Not now / Record, and "Record screen too" on macOS) or
//! ends ("Zoom meeting ended": Keep recording / Stop). Top right, above
//! other windows, without taking focus. Replaces macOS's own dialog, which
//! could not hold a switch and was German only. Page: src/meeting-prompt/.

use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use tauri::window::{Effect, EffectState, EffectsBuilder};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const LABEL: &str = "meeting_prompt";
const WIDTH: f64 = 360.0;
const AUTO_WIDTH: f64 = 380.0;

struct Pending {
    /// Bumped for every question, so a late click can't answer the next one.
    round: u64,
    answer: Option<String>,
}

static PENDING: Mutex<Pending> = Mutex::new(Pending {
    round: 0,
    answer: None,
});
static ANSWERED: Condvar = Condvar::new();

/// The question "record this call?" while it is on screen, for the local
/// API's state (`local_api.rs`).
#[derive(Debug, Clone, PartialEq)]
pub struct Asking {
    pub app: String,
    /// Unix ms.
    pub since_ms: u64,
    /// Unix ms when no answer counts as "no".
    pub expires_ms: u64,
}

static ASKING: Mutex<Option<Asking>> = Mutex::new(None);

/// The record question on screen, if any.
pub fn asking() -> Option<Asking> {
    ASKING.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// What a question is about.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A call started: "not_now", "record" or "record_screen".
    Started { screen: bool },
    /// The call ended: "keep" or "stop".
    Ended,
    /// A meeting started by itself (`super::auto`): one line with the
    /// Auto-Start switch ("off") and an X ("close"). Drawn as its own glass
    /// card with the X outside the corner, like a macOS notification.
    AutoStarted,
}

/// Shows the window and waits for the answer, at most `timeout`. `None`
/// when nobody answered.
pub fn ask(app: &AppHandle, kind: Kind, meeting_app: &str, timeout: Duration) -> Option<String> {
    ask_checked(app, kind, meeting_app, timeout).1
}

/// [`ask`], and whether the window came up. Not shown: no answer, at once.
pub fn ask_checked(
    app: &AppHandle,
    kind: Kind,
    meeting_app: &str,
    timeout: Duration,
) -> (bool, Option<String>) {
    let round = {
        let mut p = PENDING.lock().unwrap_or_else(|e| e.into_inner());
        p.round += 1;
        p.answer = None;
        p.round
    };
    let (query, width, height) = match kind {
        Kind::Started { screen } => (
            format!("kind=started&screen={}", screen as u8),
            WIDTH,
            if screen { 192.0 } else { 148.0 },
        ),
        Kind::Ended => ("kind=ended".to_string(), WIDTH, 148.0),
        // A 345 x 52 card with room for the X over its corner and its shadow.
        Kind::AutoStarted => ("kind=auto".to_string(), AUTO_WIDTH, 92.0),
    };
    let url = format!(
        "src/meeting-prompt/index.html?{query}&app={}",
        encode(meeting_app)
    );
    if !show(app, url, width, height, kind != Kind::AutoStarted) {
        return (false, None);
    }
    if matches!(kind, Kind::Started { .. }) {
        let now = unix_ms();
        *ASKING.lock().unwrap_or_else(|e| e.into_inner()) = Some(Asking {
            app: meeting_app.to_string(),
            since_ms: now,
            expires_ms: now + timeout.as_millis() as u64,
        });
    }

    let deadline = Instant::now() + timeout;
    let mut p = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    while p.round == round && p.answer.is_none() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        p = ANSWERED
            .wait_timeout(p, left)
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
    let answer = if p.round == round {
        p.answer.take()
    } else {
        None
    };
    drop(p);
    if matches!(kind, Kind::Started { .. }) {
        *ASKING.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    hide(app);
    (true, answer)
}

/// The page's answer.
pub fn answer(value: String) {
    let mut p = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    p.answer = Some(value);
    ANSWERED.notify_all();
}

/// `glass`: macOS's frosted glass behind the whole window. Off for a page
/// that draws its own card inside a clear window. True once the window is
/// up. Called off the main thread (it waits for it). In test mode the
/// window is built hidden.
fn show(app: &AppHandle, url: String, width: f64, height: f64, glass: bool) -> bool {
    let handle = app.clone();
    let (tx, rx) = std::sync::mpsc::channel::<bool>();
    let visible = !crate::test_mic::active();
    // A window left from the last question goes first; a new one with the
    // same label can't be built until it is gone.
    destroy(app);
    let gone_by = Instant::now() + Duration::from_secs(2);
    while app.get_webview_window(LABEL).is_some() && Instant::now() < gone_by {
        std::thread::sleep(Duration::from_millis(10));
    }
    let sent = app.run_on_main_thread(move || {
        let (x, y) = position(&handle, width);
        let mut builder = WebviewWindowBuilder::new(&handle, LABEL, WebviewUrl::App(url.into()))
            .title(crate::brand::app_name())
            .inner_size(width, height)
            .position(x, y)
            .decorations(false)
            .transparent(true)
            .shadow(glass);
        if glass {
            builder = builder.effects(
                EffectsBuilder::new()
                    .effect(Effect::HudWindow)
                    .state(EffectState::Active)
                    .radius(16.0)
                    .build(),
            );
        }
        let built = builder
            .resizable(false)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .skip_taskbar(true)
            .focused(false)
            .accept_first_mouse(true)
            .visible(visible)
            .build();
        match built {
            Ok(_) => {
                log::info!("Meeting prompt shown");
                let _ = tx.send(true);
            }
            Err(e) => {
                log::warn!("Meeting prompt could not be shown: {e}");
                let _ = tx.send(false);
            }
        }
    });
    sent.is_ok() && rx.recv_timeout(Duration::from_secs(5)).unwrap_or(false)
}

fn hide(app: &AppHandle) {
    destroy(app);
}

/// Removes the window for good. Not `close()`: the app's close handler
/// (`lib.rs`, meant for the main window) turns every close into a hide, so
/// the hidden window stayed, and every later question failed with "a webview
/// with label `meeting_prompt` already exists". Only the first call after a
/// launch got its question, from 2026-11-24 on (KNOWN_ISSUES.md).
fn destroy(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = handle.get_webview_window(LABEL) {
            let _ = w.destroy();
        }
    });
}

/// Top right of the monitor with the cursor, below the menu bar.
fn position(app: &AppHandle, width: f64) -> (f64, f64) {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    match monitor {
        Some(m) => {
            let scale = m.scale_factor();
            let pos = m.position().to_logical::<f64>(scale);
            let size = m.size().to_logical::<f64>(scale);
            (pos.x + size.width - width - 16.0, pos.y + 40.0)
        }
        None => (1000.0, 40.0),
    }
}

/// Percent-encodes a query value.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::encode;

    #[test]
    fn encodes_app_names() {
        assert_eq!(encode("Google Meet"), "Google%20Meet");
        assert_eq!(encode("Zoom"), "Zoom");
        assert_eq!(encode("a&b=ü"), "a%26b%3D%C3%BC");
    }
}

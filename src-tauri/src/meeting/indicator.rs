//! The floating "recording" pill (level bars, elapsed time, stop button) shown
//! while a meeting records, like Superwhisper's. Top centre of the screen
//! with the cursor, above other windows and on every Space; draggable. On
//! macOS its background is the frosted HUD material under a dark veil (the
//! page): Liquid Glass was tried and dropped, white bars and time vanished
//! on light backgrounds (Robin, 2026-11-20: "man sieht gar nichts").
//!
//! The page sizes the window to the pill and, for the stop button's tooltip,
//! briefly taller; the glass covers only the pill's 40 px at the top. Hover
//! on the stop button comes from here, not the page: Sokki is usually not the
//! active app, and macOS sends a background app's web view neither hover nor
//! cursor changes (Robin: "Wenn ich auf dem Stop button hovere, macht der
//! cursor keine Veränderung").

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const LABEL: &str = "meeting_indicator";
/// A first guess; the page sets the width to fit the pill.
const WIDTH: f64 = 140.0;
const HEIGHT: f64 = 40.0;
/// The stop button: this wide, this far in from the pill's right edge.
const STOP: f64 = 28.0;
const STOP_INSET: f64 = 6.0;

pub fn show(app: &AppHandle) {
    if crate::test_mic::active() {
        return; // no window on the developer's screen
    }
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = handle.get_webview_window(LABEL) {
            let _ = w.show();
            return;
        }
        let (x, y) = position(&handle);
        let built = WebviewWindowBuilder::new(
            &handle,
            LABEL,
            WebviewUrl::App("src/meeting-indicator/index.html".into()),
        )
        .title("Meeting Recording")
        .inner_size(WIDTH, HEIGHT)
        .position(x, y)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .resizable(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .focused(false)
        // The stop button must work on the first click while another app
        // is in front; without this macOS only activates Sokki with it.
        .accept_first_mouse(true)
        .build();
        match built {
            #[cfg(target_os = "macos")]
            Ok(window) => add_glass(&window),
            #[cfg(not(target_os = "macos"))]
            Ok(_) => {}
            Err(e) => log::warn!("Recording indicator could not be shown: {e}"),
        }
    });
}

/// Hides the pill; the next meeting shows the same window again at once.
/// Not `close()`: the app's close handler turns every close into a hide (and
/// drops the Dock icon), so the old page stayed with its state. After a stop
/// from the pill it came back with the spinner, dim bars and "0:00" frozen,
/// and looked like it was still starting (Robin, 2026-11-23). The page now
/// resets itself once the meeting is no longer recording.
pub fn hide(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = handle.get_webview_window(LABEL) {
            let _ = w.hide();
        }
    });
}

/// Top centre of the monitor with the cursor, just below the menu bar.
fn position(app: &AppHandle) -> (f64, f64) {
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
            (pos.x + (size.width - WIDTH) / 2.0, pos.y + 34.0)
        }
        None => (600.0, 34.0),
    }
}

/// Levels sent to the indicator about 30 times a second.
#[derive(Clone, serde::Serialize)]
struct Levels {
    mic: f32,
    system: f32,
    /// How long the call track has had only exact zeros or no audio at all
    /// (ms; 0 while it has sound). A quiet call still has noise in it, so
    /// this means Sokki hears nothing, not that the other side is silent.
    system_dead_ms: u64,
    /// "Sokki hears nothing" ([`super::silence`]), while it applies.
    hint: Option<super::silence::Hint>,
    /// The microphone the meeting records from, for the hint.
    mic_name: Option<String>,
}

/// A stretch of exact zeros on the call track is logged once it is this long.
const ZERO_STRETCH_US: u64 = 2_000_000;
const LEVELS_EVERY: std::time::Duration = std::time::Duration::from_secs(60);

/// What the ticker logs while a meeting records: both tracks' peaks once a
/// minute, and when the call track starts and stops being exact zeros. On
/// 2026-11-24 the call track went dead for 13 minutes and the log said
/// nothing; this makes that visible.
struct TrackLog {
    started_us: u64,
    minute: u32,
    minute_started_us: u64,
    mic_peak: f32,
    call_peak: f32,
    /// Start of the current stretch of exact zeros, once logged.
    zeros_since_us: Option<u64>,
}

/// "-23 dBFS", or "exact zeros".
fn dbfs(rms: f32) -> String {
    if rms <= 0.0 {
        "exact zeros".into()
    } else {
        format!("{:.0} dBFS", 20.0 * rms.log10())
    }
}

/// "12:34" into the meeting.
fn at(us: u64) -> String {
    let s = us / 1_000_000;
    format!("{}:{:02}", s / 60, s % 60)
}

impl TrackLog {
    fn new(started_us: u64) -> Self {
        Self {
            started_us,
            minute: 0,
            minute_started_us: started_us,
            mic_peak: 0.0,
            call_peak: 0.0,
            zeros_since_us: None,
        }
    }

    /// How long the call track has been dead at `now_us` (0 = it has sound).
    fn dead_us(&self, now_us: u64, sound_us: u64) -> u64 {
        now_us.saturating_sub(sound_us.max(self.started_us))
    }

    /// One tick. Returns the lines to log.
    fn tick(
        &mut self,
        now_us: u64,
        mic: f32,
        call: f32,
        sound_us: u64,
        callback_us: u64,
    ) -> Vec<(log::Level, String)> {
        let mut lines = Vec::new();
        self.mic_peak = self.mic_peak.max(mic);
        self.call_peak = self.call_peak.max(call);
        if now_us.saturating_sub(self.minute_started_us) >= LEVELS_EVERY.as_micros() as u64 {
            self.minute += 1;
            lines.push((
                log::Level::Info,
                format!(
                    "Meeting levels, minute {}: mic peak {}, call peak {}",
                    self.minute,
                    dbfs(self.mic_peak),
                    dbfs(self.call_peak)
                ),
            ));
            self.minute_started_us = now_us;
            self.mic_peak = 0.0;
            self.call_peak = 0.0;
        }
        match self.zeros_since_us {
            Some(since) if sound_us > since => {
                lines.push((
                    log::Level::Info,
                    format!(
                        "Call track: sound again at {} after {:.1} s of exact zeros",
                        at(sound_us - self.started_us),
                        (sound_us - since) as f64 / 1e6
                    ),
                ));
                self.zeros_since_us = None;
            }
            Some(_) => {}
            None if self.dead_us(now_us, sound_us) >= ZERO_STRETCH_US => {
                let since = sound_us.max(self.started_us);
                let arriving = callback_us > 0 && now_us.saturating_sub(callback_us) < 1_000_000;
                // Info, not a warning: in a room with no call this is normal.
                lines.push((
                    log::Level::Info,
                    format!(
                        "Call track: exact zeros since {} ({})",
                        at(since - self.started_us),
                        if arriving {
                            "the tap delivers silence"
                        } else {
                            "no audio arrives"
                        }
                    ),
                ));
                self.zeros_since_us = Some(since);
            }
            None => {}
        }
        lines
    }

    /// The meeting stopped: an open stretch ends here.
    fn finish(&self, now_us: u64) -> Option<String> {
        self.zeros_since_us.map(|since| {
            format!(
                "Call track: exact zeros from {} to the end ({})",
                at(since - self.started_us),
                at(now_us.saturating_sub(self.started_us))
            )
        })
    }
}

/// While the meeting records: send live levels to the indicator and show
/// "🔴 12:34" next to the menu bar icon. Ends by itself when recording stops.
pub fn start_ticker(app: &AppHandle) {
    use std::sync::atomic::Ordering;
    use tauri::Emitter;
    let handle = app.clone();
    let started = std::time::Instant::now();
    std::thread::spawn(move || {
        let manager = handle
            .state::<std::sync::Arc<super::MeetingManager>>()
            .inner()
            .clone();
        let mut last_title = String::new();
        let mut track_log = TrackLog::new(super::mono_us());
        let source_app = manager.active_id().and_then(|id| {
            handle
                .state::<std::sync::Arc<super::store::MeetingStore>>()
                .get(&id)
                .and_then(|m| m.source_app)
        });
        let enabled = crate::settings::get_settings(&handle)
            .meetings
            .silence_hint_enabled;
        let mut watch = super::silence::Watch::new(super::mono_us(), source_app.is_some(), enabled);
        let mut mic_name = manager.mic_name();
        let mut tick = 0u32;
        while manager.is_recording() {
            let mic = f32::from_bits(super::MIC_LEVEL.swap(0, Ordering::Relaxed));
            let system = f32::from_bits(super::SYSTEM_LEVEL.swap(0, Ordering::Relaxed));
            let now_us = super::mono_us();
            let sound_us = super::SYSTEM_SOUND_US.load(Ordering::Relaxed);
            let callback_us = super::SYSTEM_CALLBACK_US.load(Ordering::Relaxed);
            for (level, line) in track_log.tick(now_us, mic, system, sound_us, callback_us) {
                log::log!(level, "{line}");
            }
            tick = tick.wrapping_add(1);
            if tick.is_multiple_of(30) {
                mic_name = manager.mic_name();
            }
            if let Some(change) = watch.tick(now_us, &super::silence::Marks::now()) {
                on_hint(
                    &handle,
                    &change,
                    at(now_us.saturating_sub(track_log.started_us)),
                    mic_name.as_deref(),
                    source_app.as_deref(),
                );
            }
            let levels = Levels {
                mic,
                system,
                system_dead_ms: track_log.dead_us(now_us, sound_us) / 1000,
                hint: watch.showing(),
                mic_name: mic_name.clone(),
            };
            let _ = handle.emit_to(LABEL, "meeting-level", levels.clone());
            // The main window's recording bar shows the same levels.
            let _ = handle.emit_to("main", "meeting-level", levels);
            #[cfg(target_os = "macos")]
            track_hover(&handle);
            let secs = started.elapsed().as_secs();
            let title = format!("🔴 {}:{:02}", secs / 60, secs % 60);
            if title != last_title {
                set_menu_bar_status(&handle, Some(&title));
                last_title = title;
            }
            std::thread::sleep(std::time::Duration::from_millis(33));
        }
        if let Some(line) = track_log.finish(super::mono_us()) {
            log::info!("{line}");
        }
    });
}

/// Logs a hint coming or going, and when it comes while the Sokki window
/// isn't in front, says it in a notification (the recording bar shows it
/// either way).
fn on_hint(
    app: &AppHandle,
    change: &super::silence::Change,
    at: String,
    mic: Option<&str>,
    call_app: Option<&str>,
) {
    use super::silence::{Change, Hint};
    let mic = mic.unwrap_or("?");
    let call_app = call_app.unwrap_or("?");
    let hint = match change {
        Change::Gone(hint) => {
            log::info!("Silence hint gone at {at}: {hint:?} (speech or sound arrived)");
            return;
        }
        Change::Shown(hint) => *hint,
    };
    let german = crate::settings::get_settings(app)
        .app_language
        .to_lowercase()
        .starts_with("de");
    // Short, as Robin asked (2026-11-25): no "for 40 seconds", and no
    // banner for the call; the strip in the window says that one.
    let banner = match (hint, german) {
        (Hint::Mic, true) => Some((
            "Sokki hört dich nicht",
            "Nimmst du mit dem richtigen Mikrofon auf?",
        )),
        (Hint::Mic, false) => Some((
            "Sokki can't hear you",
            "Are you recording with the right microphone?",
        )),
        (Hint::Call, _) => None,
    };
    let in_front = app
        .get_webview_window("main")
        .is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false));
    let how = match banner {
        None => "the recording bar only",
        Some(_) if crate::test_mic::active() => "test mode: notification logged, not shown",
        Some(_) if in_front => "window in front: no notification",
        Some(_) => "notification shown",
    };
    let text = banner
        .map(|(title, body)| format!(": {title} — {body}"))
        .unwrap_or_default();
    log::info!(
        "Silence hint at {at}: {hint:?} (microphone {mic}, call app {call_app}); {how}{text}"
    );
    if !crate::test_mic::active() && !in_front {
        if let Some((title, body)) = banner {
            super::detect::notify(title, body);
        }
    }
}

/// Text next to the menu bar icon (`None` clears it). Like Superwhisper's
/// status dot: 🔴 recording, 🔵 processing, 🟢 done, ⚠️ failed.
pub fn set_menu_bar_status(app: &AppHandle, text: Option<&str>) {
    let handle = app.clone();
    let text = text.map(str::to_string);
    let _ = app.run_on_main_thread(move || {
        if let Some(tray) = handle.try_state::<tauri::tray::TrayIcon>() {
            let _ = tray.set_title(text.as_deref());
        }
    });
}

/// Shows a final status for a few seconds, then clears it, unless a new
/// meeting started meanwhile.
pub fn flash_menu_bar_status(app: &AppHandle, text: &str) {
    set_menu_bar_status(app, Some(text));
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(15));
        let recording = handle
            .state::<std::sync::Arc<super::MeetingManager>>()
            .is_recording();
        if !recording {
            set_menu_bar_status(&handle, None);
        }
    });
}

/// Frosted HUD glass behind the pill: the top 40 px of the window, rounded,
/// staying at the top when the window grows for the tooltip. Main thread.
#[cfg(target_os = "macos")]
fn add_glass(window: &tauri::WebviewWindow) {
    use objc2::runtime::AnyObject;
    use objc2::{msg_send, MainThreadMarker};
    use objc2_app_kit::{
        NSAutoresizingMaskOptions, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
        NSVisualEffectState, NSVisualEffectView, NSWindow, NSWindowOrderingMode,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let (Some(mtm), Ok(ptr)) = (MainThreadMarker::new(), window.ns_window()) else {
        return;
    };
    // SAFETY: `ns_window` is the live NSWindow behind `window`, and this runs
    // on the main thread.
    let ns_window = unsafe { &*(ptr as *const NSWindow) };
    let Some(content) = ns_window.contentView() else {
        return;
    };
    let bounds = content.bounds();
    let frame = NSRect::new(
        NSPoint::new(0.0, bounds.size.height - HEIGHT),
        NSSize::new(bounds.size.width, HEIGHT),
    );
    let glass = NSVisualEffectView::initWithFrame(mtm.alloc(), frame);
    glass.setMaterial(NSVisualEffectMaterial::HUDWindow);
    glass.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    glass.setState(NSVisualEffectState::Active);
    glass.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin,
    );
    glass.setWantsLayer(true);
    // SAFETY: a layer-backed view has a CALayer; both setters exist on it.
    unsafe {
        let layer: *mut AnyObject = msg_send![&*glass, layer];
        if !layer.is_null() {
            let _: () = msg_send![layer, setCornerRadius: HEIGHT / 2.0];
            let _: () = msg_send![layer, setMasksToBounds: true];
        }
    }
    content.addSubview_positioned_relativeTo(&glass, NSWindowOrderingMode::Below, None);
}

/// Whether the mouse is over the stop button: tells the page (it shows the
/// hover look and the tooltip) and shows the pointing hand. Called by the
/// ticker while the meeting records.
#[cfg(target_os = "macos")]
fn track_hover(app: &AppHandle) {
    use objc2_app_kit::{NSCursor, NSEvent, NSWindow};
    use std::sync::atomic::{AtomicBool, Ordering};
    use tauri::Emitter;
    static OVER: AtomicBool = AtomicBool::new(false);

    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(window) = handle.get_webview_window(LABEL) else {
            OVER.store(false, Ordering::Relaxed);
            return;
        };
        let Ok(ptr) = window.ns_window() else {
            return;
        };
        // SAFETY: as in `add_glass`; this closure runs on the main thread.
        let ns_window = unsafe { &*(ptr as *const NSWindow) };
        let frame = ns_window.frame();
        let mouse = NSEvent::mouseLocation();
        // Screen coordinates grow upwards; the pill is the window's top 40 px.
        let top = frame.origin.y + frame.size.height;
        let right = frame.origin.x + frame.size.width;
        let over = mouse.x >= right - STOP_INSET - STOP
            && mouse.x <= right - STOP_INSET
            && mouse.y <= top - (HEIGHT - STOP) / 2.0
            && mouse.y >= top - (HEIGHT + STOP) / 2.0;
        if over {
            NSCursor::pointingHandCursor().set();
        }
        if OVER.swap(over, Ordering::Relaxed) != over {
            if !over {
                NSCursor::arrowCursor().set();
            }
            let _ = handle.emit_to(LABEL, "meeting-indicator-hover", over);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;

    #[test]
    fn logs_a_zero_stretch_once_and_its_end() {
        let start = 10 * S;
        let mut log = TrackLog::new(start);
        // Sound until 5 s in, then exact zeros from the tap.
        let sound = start + 5 * S;
        assert!(log
            .tick(start + 6 * S, 0.1, 0.0, sound, start + 6 * S)
            .is_empty());
        let lines = log.tick(start + 7 * S, 0.1, 0.0, sound, start + 7 * S);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].0, log::Level::Info);
        assert!(lines[0].1.contains("since 0:05"), "{}", lines[0].1);
        assert!(lines[0].1.contains("delivers silence"), "{}", lines[0].1);
        assert!(log
            .tick(start + 9 * S, 0.1, 0.0, sound, start + 9 * S)
            .is_empty());
        assert_eq!(log.dead_us(start + 9 * S, sound), 4 * S);
        let back = start + 20 * S;
        let lines = log.tick(back, 0.1, 0.2, back, back);
        assert!(
            lines[0].1.contains("sound again at 0:20 after 15.0 s"),
            "{}",
            lines[0].1
        );
        assert_eq!(log.dead_us(back, back), 0);
        assert!(log.finish(back).is_none());
    }

    #[test]
    fn logs_peaks_once_a_minute_and_an_open_stretch_at_the_end() {
        let mut log = TrackLog::new(0);
        assert!(log.tick(S, 0.1, 0.01, S, S).is_empty());
        let lines = log.tick(60 * S, 0.05, 0.0, 60 * S, 60 * S);
        assert_eq!(
            lines[0].1,
            "Meeting levels, minute 1: mic peak -20 dBFS, call peak -40 dBFS"
        );
        // Nothing arrives at all from 60 s on.
        let lines = log.tick(63 * S, 0.0, 0.0, 60 * S, 60 * S);
        assert!(lines[0].1.contains("no audio arrives"), "{}", lines[0].1);
        let lines = log.tick(120 * S, 0.0, 0.0, 60 * S, 60 * S);
        assert_eq!(
            lines[0].1,
            "Meeting levels, minute 2: mic peak exact zeros, call peak exact zeros"
        );
        assert_eq!(
            log.finish(125 * S).unwrap(),
            "Call track: exact zeros from 1:00 to the end (2:05)"
        );
    }
}

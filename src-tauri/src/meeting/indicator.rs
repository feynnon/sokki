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

/// Levels sent to the indicator about 12 times a second.
#[derive(Clone, serde::Serialize)]
struct Levels {
    mic: f32,
    system: f32,
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
        while manager.is_recording() {
            let mic = f32::from_bits(super::MIC_LEVEL.swap(0, Ordering::Relaxed));
            let system = f32::from_bits(super::SYSTEM_LEVEL.swap(0, Ordering::Relaxed));
            let _ = handle.emit_to(LABEL, "meeting-level", Levels { mic, system });
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
    });
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

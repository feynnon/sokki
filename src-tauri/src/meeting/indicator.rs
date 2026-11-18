//! The floating "recording" pill (red dot, elapsed time, stop button) shown
//! while a meeting records, like Superwhisper's. Top centre of the screen
//! with the cursor, above other windows and on every Space; draggable.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const LABEL: &str = "meeting_indicator";
const WIDTH: f64 = 174.0;
const HEIGHT: f64 = 36.0;

pub fn show(app: &AppHandle) {
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
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .focused(false)
        .build();
        if let Err(e) = built {
            log::warn!("Recording indicator could not be shown: {e}");
        }
    });
}

pub fn hide(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = handle.get_webview_window(LABEL) {
            let _ = w.close();
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

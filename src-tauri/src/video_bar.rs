//! The recording bar (after Loom's): what to record, the screen, the look,
//! the camera, then Record. Bottom centre of the screen with the cursor,
//! above other windows and on every Space. The camera bubble is a separate
//! native panel (`swift/video_capture.swift`). Sokki's own windows are left
//! out of the screen recording.

use tauri::window::{Effect, EffectState, EffectsBuilder};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const LABEL: &str = "video_bar";
const WIDTH: f64 = 720.0;
const HEIGHT: f64 = 56.0;

pub fn show(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = handle.get_webview_window(LABEL) {
            let _ = w.show();
            let _ = w.set_focus();
            return;
        }
        let (x, y) = position(&handle);
        let built = WebviewWindowBuilder::new(&handle, LABEL, WebviewUrl::App("src/video-bar/index.html".into()))
            .title("New recording")
            .inner_size(WIDTH, HEIGHT)
            .position(x, y)
            .decorations(false)
            .transparent(true)
            .effects(
                EffectsBuilder::new()
                    .effect(Effect::HudWindow)
                    .state(EffectState::Active)
                    .radius(14.0)
                    .build(),
            )
            .shadow(true)
            .resizable(false)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .skip_taskbar(true)
            .accept_first_mouse(true)
            .build();
        if let Err(e) = built {
            log::warn!("Recording bar could not be shown: {e}");
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

/// Bottom centre of the monitor with the cursor, above the Dock.
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
            (pos.x + (size.width - WIDTH) / 2.0, pos.y + size.height - HEIGHT - 110.0)
        }
        None => (360.0, 700.0),
    }
}

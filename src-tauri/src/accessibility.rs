//! macOS Accessibility permission and Sokki's keyboard hook.
//!
//! Two macOS behaviours make the hook dangerous around this permission:
//! - Creating a keyboard event tap without the permission makes macOS show
//!   its permission window, on every launch.
//! - Switching the permission off while the app holds an active event tap
//!   stalls all keyboard and mouse input on the Mac (Apple FB24619068,
//!   https://developer.apple.com/forums/thread/844416), until the Mac is
//!   restarted.
//!
//! So the hook exists only while the permission is granted: it is created
//! after `trusted()` says yes, and `start_watch` removes it as soon as the
//! permission goes and brings it back when it returns.

use tauri::AppHandle;

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    // Boolean is an unsigned char. Never prompts.
    fn AXIsProcessTrusted() -> u8;
}

/// Whether macOS currently grants this app Accessibility. Never shows a prompt.
pub fn trusted() -> bool {
    #[cfg(target_os = "macos")]
    {
        unsafe { AXIsProcessTrusted() != 0 }
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Watches the permission and keeps the keyboard hook in step with it.
pub fn start_watch(app: &AppHandle) {
    // Test mode has no keyboard hook at all, also when the test app is
    // granted Accessibility (for the chat notice test): nothing to keep in
    // step, and no hook that could freeze input when the grant goes.
    if crate::test_mic::active() {
        log::info!("Test mode: no Accessibility watch, no keyboard hook");
        return;
    }
    #[cfg(target_os = "macos")]
    {
        use std::time::Duration;
        let app = app.clone();
        let spawned = std::thread::Builder::new()
            .name("accessibility-watch".into())
            .spawn(move || {
                let mut was = trusted();
                loop {
                    // Short, so the hook is gone soon after the switch is turned off.
                    std::thread::sleep(Duration::from_millis(250));
                    let now = trusted();
                    if now == was {
                        continue;
                    }
                    was = now;
                    if now {
                        log::info!("Accessibility granted: starting the keyboard hook");
                        crate::shortcut::native_keys::resume_hook(&app);
                        if let Err(e) = crate::commands::initialize_shortcuts(app.clone()) {
                            log::warn!("Shortcuts after the Accessibility grant: {e}");
                        }
                    } else {
                        log::warn!("Accessibility switched off: removing the keyboard hook");
                        crate::shortcut::native_keys::suspend_hook(&app);
                    }
                }
            });
        if let Err(e) = spawned {
            log::error!("Could not start the Accessibility watch: {e}");
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

//! Keeps the Mac awake while a meeting is being recorded.
//!
//! On 2026-11-19 an in-person meeting recorded in front of the laptop was cut
//! off because the Mac went to idle sleep. A `PreventUserIdleSystemSleep`
//! power assertion keeps the system (and so the capture) running while the
//! display may still dim and turn off. It is held by `KeepAwake`, which lives
//! in the active meeting, so it ends whenever the recording does: stop,
//! failed start, quit (`finalize_on_exit`), or a crash (macOS releases a
//! process's assertions when it exits).
//!
//! Only the recording holds it. Processing needs no assertion: sleep only
//! pauses it, and its timeouts use `Instant`, which does not advance while
//! the Mac sleeps.
//!
//! Limits: closing the lid (without an external display) still sleeps the
//! Mac; no power assertion prevents that (only the admin-level
//! `pmset disablesleep`). Off macOS this app holds nothing; the Linux host
//! (`crates/sokki/src/keep_awake.rs`) and Android (`MainActivity.kt`) have
//! their own.

/// The reason shown by `pmset -g assertions` and Activity Monitor.
pub fn reason() -> String {
    format!("{} is recording a meeting", crate::brand::app_name())
}

/// Holds the power assertion until dropped.
#[derive(Debug)]
pub struct KeepAwake {
    #[cfg(target_os = "macos")]
    id: Option<u32>,
}

impl KeepAwake {
    /// Takes the assertion. Never fails: a recording without it still
    /// records, so a failure is only logged.
    pub fn acquire() -> Self {
        #[cfg(target_os = "macos")]
        {
            let id = match macos::create(&reason()) {
                Ok(id) => Some(id),
                Err(code) => {
                    log::warn!("Keeping the Mac awake failed (IOReturn {code:#x})");
                    None
                }
            };
            Self { id }
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self {}
        }
    }

    /// Whether the assertion is held (always false off macOS).
    pub fn is_held(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.id.is_some()
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(id) = self.id.take() {
            macos::release(id);
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{c_char, c_void, CString};

    type CfStringRef = *const c_void;
    const UTF8: u32 = 0x0800_0100; // kCFStringEncodingUTF8
    const ASSERTION_LEVEL_ON: u32 = 255; // kIOPMAssertionLevelOn
    const IO_RETURN_SUCCESS: i32 = 0;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(
            alloc: *const c_void,
            c_str: *const c_char,
            encoding: u32,
        ) -> CfStringRef;
        fn CFRelease(value: *const c_void);
    }

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPMAssertionCreateWithName(
            assertion_type: CfStringRef,
            level: u32,
            name: CfStringRef,
            id: *mut u32,
        ) -> i32;
        fn IOPMAssertionRelease(id: u32) -> i32;
    }

    /// An owned CFString, released on drop.
    struct CfString(CfStringRef);

    impl CfString {
        fn new(text: &str) -> Option<Self> {
            let c = CString::new(text).ok()?;
            // SAFETY: `c` is a valid NUL-terminated UTF-8 string for the call.
            let s = unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) };
            (!s.is_null()).then_some(Self(s))
        }
    }

    impl Drop for CfString {
        fn drop(&mut self) {
            // SAFETY: created by CFStringCreateWithCString (a +1 reference).
            unsafe { CFRelease(self.0) };
        }
    }

    /// kIOPMAssertionTypePreventUserIdleSystemSleep: the system stays awake,
    /// the display may still sleep.
    pub fn create(reason: &str) -> Result<u32, i32> {
        let kind = CfString::new("PreventUserIdleSystemSleep").ok_or(-1)?;
        let name = CfString::new(reason).ok_or(-1)?;
        let mut id = 0u32;
        // SAFETY: both CFStrings are valid for the call; `id` is a valid out pointer.
        let code =
            unsafe { IOPMAssertionCreateWithName(kind.0, ASSERTION_LEVEL_ON, name.0, &mut id) };
        if code == IO_RETURN_SUCCESS {
            log::info!("Keeping the Mac awake while recording (assertion {id})");
            Ok(id)
        } else {
            Err(code)
        }
    }

    pub fn release(id: u32) {
        // SAFETY: `id` came from a successful IOPMAssertionCreateWithName.
        let code = unsafe { IOPMAssertionRelease(id) };
        if code == IO_RETURN_SUCCESS {
            log::info!("Mac may sleep again (assertion {id} released)");
        } else {
            log::warn!("Releasing power assertion {id} failed (IOReturn {code:#x})");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The assertions this process holds now, by name, from `pmset`.
    #[cfg(target_os = "macos")]
    fn held_by_us(name: &str) -> bool {
        let out = std::process::Command::new("pmset")
            .args(["-g", "assertions"])
            .output()
            .expect("pmset runs");
        let text = String::from_utf8_lossy(&out.stdout);
        let pid = format!("pid {}(", std::process::id());
        text.lines().any(|l| {
            l.contains(&pid) && l.contains("PreventUserIdleSystemSleep") && l.contains(name)
        })
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn assertion_lives_exactly_as_long_as_the_guard() {
        let name = reason();
        let guard = KeepAwake::acquire();
        assert!(guard.is_held());
        assert!(
            held_by_us(&name),
            "pmset should list the assertion while held"
        );
        drop(guard);
        assert!(!held_by_us(&name), "pmset should not list it after drop");
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn no_op_off_macos() {
        assert!(!KeepAwake::acquire().is_held());
    }
}

//! The focused text field through macOS Accessibility (AXUIElement).
//!
//! Every call waits at most `TIMEOUT_S` for the app, so a hung app costs Sokki's own thread
//! a moment and never the keyboard. Never a password or protected field, nor anything
//! while secure input is on (password managers, sudo).
//! SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashSet;
use std::ffi::c_void;
use std::ptr;
use std::sync::Mutex;

use crate::Field;

type CFTypeRef = *const c_void;
type AXError = i32;

const TIMEOUT_S: f32 = 0.25;
const UTF8: u32 = 0x0800_0100;
const AX_VALUE_CF_RANGE: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CFRange {
    location: isize,
    length: isize,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
    fn AXUIElementCopyAttributeValue(e: CFTypeRef, attr: CFTypeRef, out: *mut CFTypeRef)
        -> AXError;
    fn AXUIElementSetAttributeValue(e: CFTypeRef, attr: CFTypeRef, value: CFTypeRef) -> AXError;
    fn AXUIElementIsAttributeSettable(e: CFTypeRef, attr: CFTypeRef, out: *mut u8) -> AXError;
    fn AXUIElementSetMessagingTimeout(e: CFTypeRef, seconds: f32) -> AXError;
    fn AXValueCreate(kind: u32, value: *const c_void) -> CFTypeRef;
    fn AXValueGetValue(v: CFTypeRef, kind: u32, out: *mut c_void) -> u8;
    fn AXIsProcessTrusted() -> u8;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventCreateKeyboardEvent(source: CFTypeRef, key: u16, down: bool) -> CFTypeRef;
    fn CGEventKeyboardSetUnicodeString(e: CFTypeRef, n: usize, s: *const u16);
    fn CGEventSetFlags(e: CFTypeRef, flags: u64);
    fn CGEventPostToPid(pid: i32, e: CFTypeRef);
}

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFGetTypeID(cf: CFTypeRef) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFBooleanGetTypeID() -> usize;
    fn CFBooleanGetValue(b: CFTypeRef) -> u8;
    static kCFBooleanTrue: CFTypeRef;
    fn CFStringCreateWithBytes(
        a: CFTypeRef,
        b: *const u8,
        n: isize,
        enc: u32,
        ext: u8,
    ) -> CFTypeRef;
    fn CFStringGetLength(s: CFTypeRef) -> isize;
    fn CFStringGetMaximumSizeForEncoding(n: isize, enc: u32) -> isize;
    fn CFStringGetCString(s: CFTypeRef, buf: *mut u8, size: isize, enc: u32) -> u8;
}

struct Cf(CFTypeRef);

impl Drop for Cf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) }
        }
    }
}

fn cfstr(s: &str) -> Cf {
    Cf(unsafe { CFStringCreateWithBytes(ptr::null(), s.as_ptr(), s.len() as isize, UTF8, 0) })
}

fn to_string(s: CFTypeRef) -> Option<String> {
    unsafe {
        if s.is_null() || CFGetTypeID(s) != CFStringGetTypeID() {
            return None;
        }
        let size = CFStringGetMaximumSizeForEncoding(CFStringGetLength(s), UTF8) + 1;
        let mut buf = vec![0u8; size.max(1) as usize];
        if CFStringGetCString(s, buf.as_mut_ptr(), size, UTF8) == 0 {
            return None;
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        buf.truncate(end);
        String::from_utf8(buf).ok()
    }
}

/// Whether macOS grants Sokki Accessibility. Never prompts.
pub fn trusted() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}

/// Whether macOS hides typed keys from apps right now.
pub fn secure_input() -> bool {
    unsafe { IsSecureEventInputEnabled() != 0 }
}

struct El(Cf);

impl El {
    fn copy(&self, attr: &str) -> Option<Cf> {
        let a = cfstr(attr);
        let mut out: CFTypeRef = ptr::null();
        let err = unsafe { AXUIElementCopyAttributeValue(self.0 .0, a.0, &mut out) };
        (err == 0 && !out.is_null()).then(|| Cf(out))
    }

    fn string(&self, attr: &str) -> Option<String> {
        self.copy(attr).and_then(|v| to_string(v.0))
    }

    fn bool(&self, attr: &str) -> Option<bool> {
        let v = self.copy(attr)?;
        unsafe { (CFGetTypeID(v.0) == CFBooleanGetTypeID()).then(|| CFBooleanGetValue(v.0) != 0) }
    }

    fn element(&self, attr: &str) -> Option<El> {
        self.copy(attr).map(El)
    }

    fn settable(&self, attr: &str) -> bool {
        let a = cfstr(attr);
        let mut out = 0u8;
        unsafe { AXUIElementIsAttributeSettable(self.0 .0, a.0, &mut out) == 0 && out != 0 }
    }

    fn set(&self, attr: &str, value: CFTypeRef) -> bool {
        let a = cfstr(attr);
        unsafe { AXUIElementSetAttributeValue(self.0 .0, a.0, value) == 0 }
    }
}

/// The focused text field Sokki may fix words in.
pub struct AxField {
    el: El,
    app: String,
    pid: i32,
}

/// How long Sokki waits for an app to show a change it asked for: Chrome applies
/// Accessibility changes a few ms later, in its page process.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(250);

/// Apps (pids) Sokki has asked to show their text fields, and (app, role) pairs of focused
/// elements that weren't text fields, logged once each.
static ASKED: Mutex<Option<HashSet<i32>>> = Mutex::new(None);
static LOGGED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Adds `x` to the set; true the first time.
fn first<T: std::hash::Hash + Eq>(set: &Mutex<Option<HashSet<T>>>, x: T) -> bool {
    set.lock()
        .map(|mut s| s.get_or_insert_with(HashSet::new).insert(x))
        .unwrap_or(false)
}

/// Chromium browsers: they show web page text fields to Accessibility only in their
/// screen-reader mode (`AXEnhancedUserInterface`); they don't know `AXManualAccessibility`.
/// Measured in Chrome 2026-11-25 with the lab (`scripts/autocorrect-lab.sh`): without it the
/// app has no focused element; about 2 s after setting it, the page's field is there.
const CHROMIUM: &[&str] = &[
    "com.google.Chrome",
    "com.google.Chrome.beta",
    "com.google.Chrome.canary",
    "org.chromium.Chromium",
    "com.brave.Browser",
    "com.microsoft.edgemac",
    "company.thebrowser.Browser",
    "com.vivaldi.Vivaldi",
    "com.operasoftware.Opera",
];

/// Asks an app once (per running process) to show its text fields to Accessibility, only
/// while autocorrect is on. Electron apps (Slack, Claude, Notion …) do it on
/// `AXManualAccessibility`; Chromium browsers on `AXEnhancedUserInterface`, which also makes
/// the browser animate window moves by window managers (KNOWN_ISSUES M1). Other apps don't
/// know either attribute and ignore it.
fn ask_for_fields(app_el: &El, pid: i32, app: &str) {
    if first(&ASKED, pid) {
        let attr = if CHROMIUM.contains(&app) {
            "AXEnhancedUserInterface"
        } else {
            "AXManualAccessibility"
        };
        let ok = app_el.set(attr, unsafe { kCFBooleanTrue });
        log::debug!("autocorrect: asked {app} to show its text fields ({attr}): {ok}");
    }
}

/// Logs once per app and role that the focused element isn't a text field Sokki can read,
/// with the role only (never the text), so a real try shows which apps hide their fields.
fn log_no_field(app: &str, role: &str) {
    if first(&LOGGED, format!("{app} {role}")) {
        log::info!("autocorrect: {app}: focused element is {role:?}, not a text field");
    }
}

/// Makes the app the active one (for the lab's tests only).
#[doc(hidden)]
pub fn bring_to_front(pid: i32) -> bool {
    let app_el = El(Cf(unsafe { AXUIElementCreateApplication(pid) }));
    app_el.set("AXFrontmost", unsafe { kCFBooleanTrue })
}

/// Whether the app is the active one (for the lab's tests only).
#[doc(hidden)]
pub fn is_front(pid: i32) -> bool {
    let app_el = El(Cf(unsafe { AXUIElementCreateApplication(pid) }));
    app_el.bool("AXFrontmost") == Some(true)
}

/// Asks the app in front to show its text fields, ahead of the first key: Chrome takes about
/// 2 s to wake up its screen-reader mode, and the first words typed would be missed.
pub fn prepare(front: Option<(i32, String)>, excluded: &[String]) {
    let Some((pid, app)) = front else { return };
    if secure_input() || !crate::allowed(&app, excluded) {
        return;
    }
    let app_el = El(Cf(unsafe { AXUIElementCreateApplication(pid) }));
    unsafe { AXUIElementSetMessagingTimeout(app_el.0 .0, TIMEOUT_S) };
    ask_for_fields(&app_el, pid, &app);
}

/// What Sokki sees of an app's focused element, without its text: for the log and tests.
pub fn describe(pid: i32) -> String {
    let app_el = El(Cf(unsafe { AXUIElementCreateApplication(pid) }));
    unsafe { AXUIElementSetMessagingTimeout(app_el.0 .0, TIMEOUT_S) };
    let Some(el) = app_el.element("AXFocusedUIElement") else {
        return "no focused element".into();
    };
    unsafe { AXUIElementSetMessagingTimeout(el.0 .0, TIMEOUT_S) };
    let f = AxField {
        el,
        app: String::new(),
        pid,
    };
    format!(
        "role {:?} subrole {:?} value {} selection {:?} replaceable {}",
        f.el.string("AXRole"),
        f.el.string("AXSubrole"),
        f.value()
            .map(|v| format!("{} chars", v.chars().count()))
            .unwrap_or("none".into()),
        f.selection(),
        f.replaceable()
    )
}

/// The focused field of the app in front, or None: no text field, a password or protected
/// field, secure input, an app that doesn't answer in time, or an app Sokki leaves alone.
/// `front`: the app in front, (pid, bundle id), from NSWorkspace (no Accessibility call).
pub fn focused(front: Option<(i32, String)>, excluded: &[String]) -> Option<AxField> {
    let app = &front.as_ref()?.1;
    if !crate::allowed(app, excluded) {
        return None;
    }
    field_of(front)
}

/// The focused field of the app in front, in any app but Sokki itself: for editing what
/// Sokki pasted there ([`crate::tail`]). None for a password or protected field, secure
/// input, or an app that doesn't answer in time.
pub fn pasted_into(front: Option<(i32, String)>) -> Option<AxField> {
    let app = &front.as_ref()?.1;
    if app.is_empty() || app.starts_with("org.sokki.") {
        return None;
    }
    field_of(front)
}

/// Asks the app in front to show its text fields (Chrome needs about 2 s the first time), for
/// [`pasted_into`]: called when a dictation starts.
pub fn wake(front: Option<(i32, String)>) {
    let Some((pid, app)) = front else { return };
    if secure_input() || app.starts_with("org.sokki.") {
        return;
    }
    let app_el = El(Cf(unsafe { AXUIElementCreateApplication(pid) }));
    unsafe { AXUIElementSetMessagingTimeout(app_el.0 .0, TIMEOUT_S) };
    ask_for_fields(&app_el, pid, &app);
}

fn field_of(front: Option<(i32, String)>) -> Option<AxField> {
    if secure_input() {
        return None;
    }
    let (pid, app) = front?;
    let app_el = El(Cf(unsafe { AXUIElementCreateApplication(pid) }));
    unsafe { AXUIElementSetMessagingTimeout(app_el.0 .0, TIMEOUT_S) };
    ask_for_fields(&app_el, pid, &app);
    let Some(el) = app_el.element("AXFocusedUIElement") else {
        log_no_field(&app, "nothing");
        return None;
    };
    unsafe { AXUIElementSetMessagingTimeout(el.0 .0, TIMEOUT_S) };
    let role = el.string("AXRole").unwrap_or_default();
    if !matches!(role.as_str(), "AXTextField" | "AXTextArea" | "AXComboBox") {
        log_no_field(&app, &role);
        return None;
    }
    if el.string("AXSubrole").as_deref() == Some("AXSecureTextField")
        || el.bool("AXProtectedContent") == Some(true)
    {
        return None;
    }
    Some(AxField { el, app, pid })
}

impl Field for AxField {
    fn app(&self) -> &str {
        &self.app
    }

    fn value(&self) -> Option<String> {
        self.el.string("AXValue")
    }

    fn selection(&self) -> Option<(usize, usize)> {
        let v = self.el.copy("AXSelectedTextRange")?;
        let mut r = CFRange::default();
        let ok = unsafe {
            AXValueGetValue(
                v.0,
                AX_VALUE_CF_RANGE,
                &mut r as *mut CFRange as *mut c_void,
            ) != 0
        };
        (ok && r.location >= 0 && r.length >= 0).then_some((r.location as usize, r.length as usize))
    }

    fn replaceable(&self) -> bool {
        // the fix is typed as keys, so only the selection has to be settable
        self.el.settable("AXSelectedTextRange")
    }

    fn select(&self, start: usize, len: usize) -> bool {
        let r = CFRange {
            location: start as isize,
            length: len as isize,
        };
        let v =
            Cf(unsafe { AXValueCreate(AX_VALUE_CF_RANGE, &r as *const CFRange as *const c_void) });
        if v.0.is_null() || !self.el.set("AXSelectedTextRange", v.0) {
            return false;
        }
        // wait until the app shows the selection, so the fix lands on the word
        let until = std::time::Instant::now() + SETTLE;
        loop {
            if self.selection() == Some((start, len)) {
                return true;
            }
            if std::time::Instant::now() > until {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
    }

    /// Types `text` over the selection as key presses sent to the app only (never to another
    /// app). Chrome ignores `AXSelectedText` (it answers success and changes nothing), and
    /// typed keys work the same in every editor and keep the app's own undo.
    fn insert(&self, text: &str) -> bool {
        let units: Vec<u16> = text.encode_utf16().collect();
        // macOS takes at most 20 UTF-16 units per key event
        for chunk in units.chunks(20) {
            for down in [true, false] {
                let e = Cf(unsafe { CGEventCreateKeyboardEvent(ptr::null(), 0, down) });
                if e.0.is_null() {
                    return false;
                }
                unsafe {
                    CGEventSetFlags(e.0, 0);
                    CGEventKeyboardSetUnicodeString(e.0, chunk.len(), chunk.as_ptr());
                    CGEventPostToPid(self.pid, e.0);
                }
            }
        }
        true
    }

    /// The keyboard hook holds the user's keys (`sokki_keys::hold`) and later sends them to
    /// this app, after Sokki's typed fix.
    fn hold_keys(&self) -> Option<crate::Held> {
        let h = sokki_keys::hold_keys(self.pid)?;
        Some(crate::Held {
            keys: h.keys,
            since_last_key_ms: h.since_last_key.as_millis().min(u64::MAX as u128) as u64,
        })
    }

    fn release_keys(&self) {
        sokki_keys::release_keys();
    }

    fn settle_within(&self, ms: u64, ok: &dyn Fn(&str) -> bool) -> bool {
        let until = std::time::Instant::now() + std::time::Duration::from_millis(ms);
        loop {
            if self.value().is_some_and(|v| ok(&v)) {
                return true;
            }
            if std::time::Instant::now() > until {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_each_app_once() {
        let set: Mutex<Option<HashSet<i32>>> = Mutex::new(None);
        assert!(first(&set, 41));
        assert!(!first(&set, 41));
        assert!(first(&set, 42));
    }
}

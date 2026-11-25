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
}

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

/// Electron apps (Slack, Claude, Notion, VS Code …) show their text fields to Accessibility
/// only once an app asks for it with `AXManualAccessibility`; without it, the focused element
/// is a bare group and Sokki would fix nothing there. Asked once per running app, and only
/// while autocorrect is on. Other apps don't know the attribute and ignore it. Not
/// `AXEnhancedUserInterface`, which Chrome also answers to: it slows window animations and
/// upsets window managers (KNOWN_ISSUES M1).
fn ask_for_fields(app_el: &El, pid: i32) {
    if first(&ASKED, pid) {
        let ok = app_el.set("AXManualAccessibility", unsafe { kCFBooleanTrue });
        log::debug!("autocorrect: asked pid {pid} to show its text fields: {ok}");
    }
}

/// Logs once per app and role that the focused element isn't a text field Sokki can read,
/// with the role only (never the text), so a real try shows which apps hide their fields.
fn log_no_field(app: &str, role: &str) {
    if first(&LOGGED, format!("{app} {role}")) {
        log::info!("autocorrect: {app}: focused element is {role:?}, not a text field");
    }
}

/// The focused field of the app in front, or None: no text field, a password or protected
/// field, secure input, an app that doesn't answer in time, or an app Sokki leaves alone.
/// `front`: the app in front, (pid, bundle id), from NSWorkspace (no Accessibility call).
pub fn focused(front: Option<(i32, String)>, excluded: &[String]) -> Option<AxField> {
    if secure_input() {
        return None;
    }
    let (pid, app) = front?;
    if !crate::allowed(&app, excluded) {
        return None;
    }
    let app_el = El(Cf(unsafe { AXUIElementCreateApplication(pid) }));
    unsafe { AXUIElementSetMessagingTimeout(app_el.0 .0, TIMEOUT_S) };
    ask_for_fields(&app_el, pid);
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
    Some(AxField { el, app })
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
        self.el.settable("AXSelectedTextRange") && self.el.settable("AXSelectedText")
    }

    fn select(&self, start: usize, len: usize) -> bool {
        let r = CFRange {
            location: start as isize,
            length: len as isize,
        };
        let v =
            Cf(unsafe { AXValueCreate(AX_VALUE_CF_RANGE, &r as *const CFRange as *const c_void) });
        !v.0.is_null() && self.el.set("AXSelectedTextRange", v.0)
    }

    fn insert(&self, text: &str) -> bool {
        let s = cfstr(text);
        self.el.set("AXSelectedText", s.0)
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

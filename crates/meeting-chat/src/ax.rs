//! A thin, safe wrapper over the macOS Accessibility C API (AXUIElement)
//! and the few CoreFoundation calls it needs. Raw FFI against system
//! frameworks, like `meeting-detect`'s Core Audio code.

use std::ffi::c_void;
use std::ptr;

type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFIndex = isize;
type AXError = i32;

const AX_SUCCESS: AXError = 0;
const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
    fn AXUIElementCreateSystemWide() -> CFTypeRef;
    fn AXUIElementCopyAttributeValue(e: CFTypeRef, attr: CFStringRef, out: *mut CFTypeRef) -> AXError;
    fn AXUIElementSetAttributeValue(e: CFTypeRef, attr: CFStringRef, value: CFTypeRef) -> AXError;
    fn AXUIElementPerformAction(e: CFTypeRef, action: CFStringRef) -> AXError;
    fn AXUIElementIsAttributeSettable(e: CFTypeRef, attr: CFStringRef, out: *mut u8) -> AXError;
    fn AXUIElementSetMessagingTimeout(e: CFTypeRef, seconds: f32) -> AXError;
    fn AXUIElementGetPid(e: CFTypeRef, pid: *mut i32) -> AXError;
    fn AXUIElementGetTypeID() -> usize;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFBooleanTrue: CFTypeRef;
    static kCFBooleanFalse: CFTypeRef;
    fn CFRelease(cf: CFTypeRef);
    fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    fn CFGetTypeID(cf: CFTypeRef) -> usize;
    fn CFEqual(a: CFTypeRef, b: CFTypeRef) -> u8;
    fn CFStringGetTypeID() -> usize;
    fn CFBooleanGetTypeID() -> usize;
    fn CFArrayGetTypeID() -> usize;
    fn CFBooleanGetValue(b: CFTypeRef) -> u8;
    fn CFArrayGetCount(a: CFTypeRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(a: CFTypeRef, i: CFIndex) -> CFTypeRef;
    fn CFStringCreateWithBytes(
        alloc: CFTypeRef,
        bytes: *const u8,
        len: CFIndex,
        encoding: u32,
        external: u8,
    ) -> CFStringRef;
    fn CFStringGetLength(s: CFStringRef) -> CFIndex;
    fn CFStringGetMaximumSizeForEncoding(length: CFIndex, encoding: u32) -> CFIndex;
    fn CFStringGetCString(s: CFStringRef, buf: *mut u8, size: CFIndex, encoding: u32) -> u8;
}

/// Whether macOS grants this process Accessibility. Never prompts.
pub fn trusted() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}

/// An owned CoreFoundation object (released on drop).
struct Cf(CFTypeRef);

impl Drop for Cf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) }
        }
    }
}

fn cf_string(s: &str) -> Cf {
    Cf(unsafe {
        CFStringCreateWithBytes(ptr::null(), s.as_ptr(), s.len() as CFIndex, CF_STRING_ENCODING_UTF8, 0)
    })
}

fn to_string(s: CFStringRef) -> Option<String> {
    unsafe {
        if s.is_null() || CFGetTypeID(s) != CFStringGetTypeID() {
            return None;
        }
        let len = CFStringGetLength(s);
        let size = CFStringGetMaximumSizeForEncoding(len, CF_STRING_ENCODING_UTF8) + 1;
        let mut buf = vec![0u8; size.max(1) as usize];
        if CFStringGetCString(s, buf.as_mut_ptr(), size, CF_STRING_ENCODING_UTF8) == 0 {
            return None;
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        buf.truncate(end);
        String::from_utf8(buf).ok()
    }
}

/// One accessibility element (an app, a window, a button, a text field …).
pub struct Element(CFTypeRef);

// AXUIElementRef is a CFType: thread-safe to retain, release and message.
unsafe impl Send for Element {}

impl Drop for Element {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) }
    }
}

impl Clone for Element {
    fn clone(&self) -> Self {
        Element(unsafe { CFRetain(self.0) })
    }
}

impl PartialEq for Element {
    fn eq(&self, other: &Self) -> bool {
        unsafe { CFEqual(self.0, other.0) != 0 }
    }
}

impl std::fmt::Debug for Element {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.describe())
    }
}

impl Element {
    /// The application element of a process.
    pub fn app(pid: i32) -> Element {
        let e = Element(unsafe { AXUIElementCreateApplication(pid) });
        // A hung app must not hang the caller (default is 6 s per call).
        unsafe { AXUIElementSetMessagingTimeout(e.0, 1.0) };
        e
    }

    /// The application element of a process, each call waiting at most
    /// `seconds` for the app to answer.
    pub fn app_with_timeout(pid: i32, seconds: f32) -> Element {
        let e = Element(unsafe { AXUIElementCreateApplication(pid) });
        unsafe { AXUIElementSetMessagingTimeout(e.0, seconds) };
        e
    }

    /// The process of the app in front (the one that gets key presses).
    pub fn frontmost_pid() -> Option<i32> {
        let sys = Element(unsafe { AXUIElementCreateSystemWide() });
        unsafe { AXUIElementSetMessagingTimeout(sys.0, 1.0) };
        sys.element("AXFocusedApplication")?.pid()
    }

    pub fn pid(&self) -> Option<i32> {
        let mut pid = 0;
        (unsafe { AXUIElementGetPid(self.0, &mut pid) } == AX_SUCCESS).then_some(pid)
    }

    fn copy(&self, attr: &str) -> Result<Cf, AXError> {
        let name = cf_string(attr);
        let mut out: CFTypeRef = ptr::null();
        let err = unsafe { AXUIElementCopyAttributeValue(self.0, name.0, &mut out) };
        if err == AX_SUCCESS && !out.is_null() {
            Ok(Cf(out))
        } else {
            Err(err)
        }
    }

    /// A string attribute (AXRole, AXTitle, AXValue of a text, …).
    pub fn string(&self, attr: &str) -> Option<String> {
        self.copy(attr).ok().and_then(|v| to_string(v.0))
    }

    pub fn bool(&self, attr: &str) -> Option<bool> {
        let v = self.copy(attr).ok()?;
        unsafe { (CFGetTypeID(v.0) == CFBooleanGetTypeID()).then(|| CFBooleanGetValue(v.0) != 0) }
    }

    /// An attribute holding one element (AXParent, AXFocusedWindow, …).
    pub fn element(&self, attr: &str) -> Option<Element> {
        let v = self.copy(attr).ok()?;
        unsafe {
            if CFGetTypeID(v.0) != AXUIElementGetTypeID() {
                return None;
            }
            Some(Element(CFRetain(v.0)))
        }
    }

    /// An attribute holding a list of elements (AXChildren, AXWindows).
    pub fn elements(&self, attr: &str) -> Vec<Element> {
        let Ok(v) = self.copy(attr) else {
            return Vec::new();
        };
        unsafe {
            if CFGetTypeID(v.0) != CFArrayGetTypeID() {
                return Vec::new();
            }
            let n = CFArrayGetCount(v.0);
            let mut out = Vec::with_capacity(n.max(0) as usize);
            for i in 0..n {
                let item = CFArrayGetValueAtIndex(v.0, i);
                if !item.is_null() && CFGetTypeID(item) == AXUIElementGetTypeID() {
                    out.push(Element(CFRetain(item)));
                }
            }
            out
        }
    }

    pub fn children(&self) -> Vec<Element> {
        self.elements("AXChildren")
    }

    pub fn role(&self) -> String {
        self.string("AXRole").unwrap_or_default()
    }

    /// Everything a person would read as this element's name: title,
    /// description, placeholder, help, label. Lowercase, joined by " | ".
    pub fn label(&self) -> String {
        ["AXTitle", "AXDescription", "AXPlaceholderValue", "AXHelp", "AXLabelValue"]
            .iter()
            .filter_map(|a| self.string(a))
            .filter(|s| !s.trim().is_empty())
            .collect::<Vec<_>>()
            .join(" | ")
            .to_lowercase()
    }

    /// The text this element shows (value, else title, else description).
    pub fn text(&self) -> Option<String> {
        ["AXValue", "AXTitle", "AXDescription"]
            .iter()
            .filter_map(|a| self.string(a))
            .find(|s| !s.trim().is_empty())
    }

    pub fn settable(&self, attr: &str) -> bool {
        let name = cf_string(attr);
        let mut out = 0u8;
        unsafe { AXUIElementIsAttributeSettable(self.0, name.0, &mut out) == AX_SUCCESS && out != 0 }
    }

    pub fn set_string(&self, attr: &str, value: &str) -> bool {
        let name = cf_string(attr);
        let v = cf_string(value);
        unsafe { AXUIElementSetAttributeValue(self.0, name.0, v.0) == AX_SUCCESS }
    }

    pub fn set_bool(&self, attr: &str, value: bool) -> bool {
        self.set_bool_code(attr, value) == AX_SUCCESS
    }

    /// Like `set_bool`, but returns the AXError code (0 is success).
    pub fn set_bool_code(&self, attr: &str, value: bool) -> i32 {
        let name = cf_string(attr);
        let v = unsafe { if value { kCFBooleanTrue } else { kCFBooleanFalse } };
        unsafe { AXUIElementSetAttributeValue(self.0, name.0, v) }
    }

    pub fn perform(&self, action: &str) -> bool {
        let name = cf_string(action);
        unsafe { AXUIElementPerformAction(self.0, name.0) == AX_SUCCESS }
    }

    /// Whether the element still exists (a closed panel's elements don't).
    pub fn alive(&self) -> bool {
        match self.copy("AXRole") {
            Ok(_) => true,
            // -25202 kAXErrorInvalidUIElement
            Err(e) => e != -25202,
        }
    }

    /// Depth-first search below (and including) this element, at most
    /// `budget` elements visited. Returns every match, in tree order.
    pub fn find_all(&self, budget: usize, pred: &mut dyn FnMut(&Element) -> bool) -> Vec<Element> {
        let mut out = Vec::new();
        let mut stack = vec![(self.clone(), 0usize)];
        let mut seen = 0usize;
        while let Some((e, depth)) = stack.pop() {
            seen += 1;
            if seen > budget {
                log::debug!("meeting-chat: search budget of {budget} elements used up");
                break;
            }
            if pred(&e) {
                out.push(e.clone());
            }
            if depth < 60 {
                let kids = e.children();
                for k in kids.into_iter().rev() {
                    stack.push((k, depth + 1));
                }
            }
        }
        out
    }

    pub fn find(&self, budget: usize, pred: &mut dyn FnMut(&Element) -> bool) -> Option<Element> {
        // Small helper: stops at the first match.
        let mut stack = vec![(self.clone(), 0usize)];
        let mut seen = 0usize;
        while let Some((e, depth)) = stack.pop() {
            seen += 1;
            if seen > budget {
                return None;
            }
            if pred(&e) {
                return Some(e);
            }
            if depth < 60 {
                for k in e.children().into_iter().rev() {
                    stack.push((k, depth + 1));
                }
            }
        }
        None
    }

    /// One line for logs and the `chat` example: role, subrole, label, value.
    pub fn describe(&self) -> String {
        let mut s = self.role();
        if let Some(sub) = self.string("AXSubrole") {
            s.push_str(&format!("/{sub}"));
        }
        let label = self.label();
        if !label.is_empty() {
            s.push_str(&format!(" [{label}]"));
        }
        if let Some(v) = self.string("AXValue") {
            let v: String = v.chars().take(60).collect();
            s.push_str(&format!(" = {v:?}"));
        }
        s
    }
}

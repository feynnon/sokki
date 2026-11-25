//! Key presses sent to one process only (`CGEventPostToPid`), so nothing
//! can land in whatever app the person switches to meanwhile.

use std::ffi::c_void;
use std::time::Duration;

type CGEventRef = *mut c_void;
type CGEventSourceRef = *mut c_void;

const SOURCE_PRIVATE: i32 = -1; // kCGEventSourceStatePrivate: no inherited modifier state
const KEY_RETURN: u16 = 36;
pub const KEY_A: u16 = 0;
pub const KEY_H: u16 = 4;
pub const KEY_DELETE: u16 = 51;
pub const KEY_C: u16 = 8;
pub const FLAG_SHIFT: u64 = 1 << 17;
pub const FLAG_CONTROL: u64 = 1 << 18;
pub const FLAG_COMMAND: u64 = 1 << 20;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceCreate(state: i32) -> CGEventSourceRef;
    fn CGEventCreateKeyboardEvent(src: CGEventSourceRef, key: u16, down: bool) -> CGEventRef;
    fn CGEventKeyboardSetUnicodeString(ev: CGEventRef, len: usize, s: *const u16);
    fn CGEventSetFlags(ev: CGEventRef, flags: u64);
    fn CGEventPostToPid(pid: i32, ev: CGEventRef);
    fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
}

const SOURCE_HID: i32 = 1; // kCGEventSourceStateHIDSystemState: the real keyboard
const EVENT_KEY_DOWN: u32 = 10;

/// Waits until nobody has pressed a key for `quiet` (at most `max`).
/// Sokki brings the call app to the front for about a second to post;
/// keys the person types meanwhile would land in the meeting chat
/// (seen in testing: "O Stopped and deleted.sc").
pub fn wait_for_typing_pause(quiet: Duration, max: Duration) {
    let start = std::time::Instant::now();
    while start.elapsed() < max {
        let since = unsafe { CGEventSourceSecondsSinceLastEventType(SOURCE_HID, EVENT_KEY_DOWN) };
        if since >= quiet.as_secs_f64() {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    log::info!("meeting-chat: the person kept typing; posting anyway");
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
}

fn post(pid: i32, key: u16, flags: u64, text: Option<&[u16]>) {
    unsafe {
        let src = CGEventSourceCreate(SOURCE_PRIVATE);
        for down in [true, false] {
            let ev = CGEventCreateKeyboardEvent(src, key, down);
            if ev.is_null() {
                continue;
            }
            CGEventSetFlags(ev, flags);
            if let Some(t) = text {
                CGEventKeyboardSetUnicodeString(ev, t.len(), t.as_ptr());
            }
            CGEventPostToPid(pid, ev);
            CFRelease(ev);
            std::thread::sleep(Duration::from_millis(4));
        }
        if !src.is_null() {
            CFRelease(src);
        }
    }
}

/// Types `text` into the focused field of `pid`, a few characters per event.
pub fn type_text(pid: i32, text: &str) {
    let units: Vec<u16> = text.encode_utf16().collect();
    // Never split a surrogate pair across two events.
    let mut i = 0;
    while i < units.len() {
        let mut end = (i + 16).min(units.len());
        if end < units.len() && (0xD800..0xDC00).contains(&units[end - 1]) {
            end -= 1;
        }
        post(pid, 0, 0, Some(&units[i..end]));
        i = end;
        std::thread::sleep(Duration::from_millis(8));
    }
}

pub fn press_return(pid: i32) {
    post(pid, KEY_RETURN, 0, None);
}

pub fn shortcut(pid: i32, key: u16, flags: u64) {
    post(pid, key, flags, None);
}

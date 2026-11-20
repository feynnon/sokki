//! Rust side of `swift/video_capture.swift`: screen and camera recording and
//! rendering a look. JSON in, JSON out.

use std::ffi::{c_char, c_void, CStr, CString};

extern "C" {
    fn sokki_video_sources() -> *mut c_char;
    fn sokki_video_request_screen() -> bool;
    fn sokki_video_request_camera() -> bool;
    fn sokki_video_start(config: *const c_char, out_error: *mut *mut c_char) -> *mut c_void;
    fn sokki_video_stop(handle: *mut c_void) -> *mut c_char;
    fn sokki_video_render(config: *const c_char, out_error: *mut *mut c_char) -> bool;
    fn sokki_video_free(s: *mut c_char);
    fn sokki_video_bubble(show: bool, device_id: *const c_char);
}

/// Shows or hides the live camera bubble.
pub fn bubble(show: bool, device_id: Option<&str>) {
    let id = device_id.and_then(|d| CString::new(d).ok());
    unsafe { sokki_video_bubble(show, id.as_ref().map_or(std::ptr::null(), |c| c.as_ptr())) }
}

/// A running recording. Stopping consumes it.
pub struct Handle(*mut c_void);
// SAFETY: the Swift side guards its own state; the handle is only passed back
// to `sokki_video_stop`, once.
unsafe impl Send for Handle {}

fn take(s: *mut c_char) -> String {
    if s.is_null() {
        return String::new();
    }
    // SAFETY: the Swift side returns a NUL-terminated malloc'd string.
    let text = unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned();
    unsafe { sokki_video_free(s) };
    text
}

/// Displays, cameras and whether each may be recorded. Never asks.
pub fn sources() -> serde_json::Value {
    let text = take(unsafe { sokki_video_sources() });
    serde_json::from_str(&text).unwrap_or(serde_json::Value::Null)
}

/// Shows macOS's Screen Recording prompt. True when already allowed.
pub fn request_screen() -> bool {
    unsafe { sokki_video_request_screen() }
}

/// Shows macOS's Camera prompt and waits for the answer.
pub fn request_camera() -> bool {
    unsafe { sokki_video_request_camera() }
}

pub fn start(config: &serde_json::Value) -> Result<Handle, String> {
    let config = CString::new(config.to_string()).map_err(|e| e.to_string())?;
    let mut error: *mut c_char = std::ptr::null_mut();
    let handle = unsafe { sokki_video_start(config.as_ptr(), &mut error) };
    if handle.is_null() {
        let message = take(error);
        return Err(if message.is_empty() { "Recording could not start.".into() } else { message });
    }
    Ok(Handle(handle))
}

/// Stops and finishes the files. Blocks until they are written.
pub fn stop(handle: Handle) -> serde_json::Value {
    let text = take(unsafe { sokki_video_stop(handle.0) });
    serde_json::from_str(&text).unwrap_or(serde_json::Value::Null)
}

/// Renders a look into one video file. Blocks.
pub fn render(config: &serde_json::Value) -> Result<(), String> {
    let config = CString::new(config.to_string()).map_err(|e| e.to_string())?;
    let mut error: *mut c_char = std::ptr::null_mut();
    if unsafe { sokki_video_render(config.as_ptr(), &mut error) } {
        Ok(())
    } else {
        let message = take(error);
        Err(if message.is_empty() { "The video could not be rendered.".into() } else { message })
    }
}

//! The Mac calendar (EventKit, `swift/calendar.swift`): the events around a
//! meeting, with their attendees, as `meeting_mail::calendar::CalendarEvent`.
//!
//! The base for every feature that needs to know who is in a meeting:
//! mailing the notes afterwards (`meeting/mail.rs`) and the mail before a
//! meeting (board card `pre-meeting-mail`).
//!
//! Test mode: `SOKKI_TEST_CALENDAR=<events.json>` reads the events from
//! that file instead of the calendar, and access counts as allowed, so the
//! test app never asks for the calendar.

use meeting_mail::calendar::CalendarEvent;
use std::ffi::{c_char, CStr};

extern "C" {
    fn sokki_calendar_status() -> *mut c_char;
    fn sokki_calendar_request() -> bool;
    fn sokki_calendar_events(from: f64, to: f64) -> *mut c_char;
    fn sokki_calendar_free(s: *mut c_char);
}

/// Whether Sokki may read the calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CalendarAccess {
    Allowed,
    /// Never asked: [`request`] shows macOS's prompt.
    NotDetermined,
    /// Declined: only System Settings can change it.
    Denied,
    Restricted,
    WriteOnly,
}

fn take(s: *mut c_char) -> String {
    if s.is_null() {
        return String::new();
    }
    // SAFETY: the Swift side returns a NUL-terminated malloc'd string.
    let text = unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned();
    unsafe { sokki_calendar_free(s) };
    text
}

fn test_file() -> Option<std::path::PathBuf> {
    std::env::var_os("SOKKI_TEST_CALENDAR")
        .filter(|p| !p.is_empty())
        .map(Into::into)
}

/// Whether Sokki may read the calendar. Never asks.
pub fn access() -> CalendarAccess {
    if test_file().is_some() {
        return CalendarAccess::Allowed;
    }
    match take(unsafe { sokki_calendar_status() }).as_str() {
        "allowed" => CalendarAccess::Allowed,
        "not_determined" => CalendarAccess::NotDetermined,
        "denied" => CalendarAccess::Denied,
        "write_only" => CalendarAccess::WriteOnly,
        _ => CalendarAccess::Restricted,
    }
}

/// Asks for the calendar (macOS's prompt, once). Blocks until answered;
/// call it off the main thread, and only after the person's own action.
pub fn request() -> CalendarAccess {
    if test_file().is_some() {
        return CalendarAccess::Allowed;
    }
    unsafe { sokki_calendar_request() };
    access()
}

/// The events from `from` to `to` (unix seconds). Empty without access.
pub fn events(from: f64, to: f64) -> Vec<CalendarEvent> {
    let text = match test_file() {
        Some(path) => std::fs::read_to_string(&path).unwrap_or_else(|e| {
            log::warn!("SOKKI_TEST_CALENDAR {}: {e}", path.display());
            "[]".into()
        }),
        None => take(unsafe { sokki_calendar_events(from, to) }),
    };
    match serde_json::from_str::<Vec<CalendarEvent>>(&text) {
        Ok(list) => list
            .into_iter()
            .filter(|e| e.end > from && e.start < to)
            .collect(),
        Err(e) => {
            log::warn!("Calendar events unreadable: {e}");
            Vec::new()
        }
    }
}

/// [`events`] for callers that need to tell "no access" from "no events"
/// (the mail before a meeting, polled from a background thread). Never
/// asks for access.
pub fn events_between(from: f64, to: f64) -> Result<Vec<CalendarEvent>, String> {
    match access() {
        CalendarAccess::Allowed => Ok(events(from, to)),
        other => Err(format!("calendar access: {other:?}")),
    }
}

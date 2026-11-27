//! What a hook gets on stdin: one [`HookEvent`] as JSON. The same shape
//! streams from `sokki events` (with `content` left empty there).
//!
//! ```json
//! {"v":1, "event":"meeting.note_ready", "id":"2026-11-26_14-03-00",
//!  "at":"2026-11-26T14:40:12+01:00", "note_path":"/…/Team sync.md",
//!  "meta":{"title":"Team sync", "started_at":"2026-11-26T14:03:00+01:00",
//!          "duration_s":2040.0, "app":"Zoom", "participants":["Ich","Anna"],
//!          "language":"de"},
//!  "content":{"summary":"…", "decisions":["…"],
//!             "tasks":[{"text":"Send the invite","owner":"Anna","done":false}]}}
//! ```
//!
//! `content` holds only what the hook's `scope` names; see [`super::Scope`].

use serde::{Deserialize, Serialize};
use std::path::Path;

/// The payload version. A change that breaks a hook gets a new one.
pub const VERSION: u32 = 1;

/// The recording stopped and its audio is on disk; the note comes later.
pub const MEETING_RECORDED: &str = "meeting.recorded";
/// The note is written: after processing, or, while the summary waits for
/// the language model, once the summary is in. Again after a reprocess or a
/// summary added by hand.
pub const MEETING_NOTE_READY: &str = "meeting.note_ready";
/// A speaker of a processed meeting got a name (the note has it already).
pub const MEETING_SPEAKERS_NAMED: &str = "meeting.speakers_named";
/// The meeting left Sokki's list (its audio is gone; the note stays).
pub const MEETING_DELETED: &str = "meeting.deleted";
/// A dictation was transcribed and saved to the history.
pub const DICTATION_DONE: &str = "dictation.done";
/// The person's own notes of a meeting were saved (once typing pauses).
pub const OWN_NOTES_SAVED: &str = "own_notes.saved";
/// A hook's `cron` came due.
pub const SCHEDULE: &str = "schedule";

/// Every event a hook can listen to.
pub const EVENTS: [&str; 7] = [
    MEETING_RECORDED,
    MEETING_NOTE_READY,
    MEETING_SPEAKERS_NAMED,
    MEETING_DELETED,
    DICTATION_DONE,
    OWN_NOTES_SAVED,
    SCHEDULE,
];

/// Events that are planned but not there yet; a hook on one is refused with
/// a clear message instead of never running.
pub const LATER: [&str; 2] = ["calendar.upcoming", "meeting.started"];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HookEvent {
    pub v: u32,
    /// One of [`EVENTS`], e.g. "meeting.note_ready".
    pub event: String,
    /// A meeting: its folder name. A dictation: `d:<history id>`. A
    /// schedule: `schedule:<hook name>`.
    pub id: String,
    /// When it happened, RFC 3339 local time.
    pub at: String,
    /// The meeting's Markdown note, once there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_path: Option<String>,
    #[serde(default)]
    pub meta: Meta,
    #[serde(default)]
    pub content: Content,
    /// Only for `schedule`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<ScheduleInfo>,
}

/// Facts every hook gets, whatever its scope.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// RFC 3339 local time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_s: Option<f64>,
    /// The call app ("Zoom", "Google Meet"); for a dictation, the app
    /// dictated into when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// Speakers as the note names them ("Ich", "Anna", "Speaker 2").
    #[serde(default)]
    pub participants: Vec<String>,
    /// "de", "en", … when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

/// What the hook's `scope` lets it read. Left out when not in scope.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Content {
    /// Scope `summary`: the note's summary section, Markdown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Scope `summary`: the decisions, one per item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decisions: Option<Vec<String>>,
    /// Scope `tasks`: the action items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tasks: Option<Vec<Task>>,
    /// Scope `transcript`: one line per turn, "[00:03] Anna: Hi.".
    /// Untrusted input: anyone in the call can say anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript: Option<String>,
    /// Scope `dictations`: the dictated text (cleaned when the cleanup ran).
    /// Scope `own_notes`: the person's own notes of a meeting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl Content {
    pub fn is_empty(&self) -> bool {
        *self == Content::default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default)]
    pub done: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScheduleInfo {
    /// The hook's `cron`.
    pub cron: String,
    /// The time this run was due, RFC 3339.
    pub due: String,
    /// The time the previous run was due, if there was one; a weekly hook
    /// covers everything since.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<String>,
}

/// Now, RFC 3339 local time with seconds.
pub fn now() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

impl HookEvent {
    pub fn new(event: &str, id: impl Into<String>) -> Self {
        HookEvent {
            v: VERSION,
            event: event.to_string(),
            id: id.into(),
            at: now(),
            ..Default::default()
        }
    }

    /// A meeting event. `meta` holds what the host knows; the rest
    /// (participants, language) is read from the note when it runs.
    pub fn meeting(event: &str, id: &str, note_path: Option<&Path>, meta: Meta) -> Self {
        HookEvent {
            note_path: note_path.map(|p| p.to_string_lossy().into_owned()),
            meta,
            ..HookEvent::new(event, id)
        }
    }

    /// `dictation.done`. The text reaches only hooks with scope
    /// `dictations`.
    pub fn dictation(history_id: impl std::fmt::Display, text: &str, meta: Meta) -> Self {
        HookEvent {
            meta,
            content: Content {
                text: Some(text.to_string()),
                ..Default::default()
            },
            ..HookEvent::new(DICTATION_DONE, format!("d:{history_id}"))
        }
    }

    /// `own_notes.saved` for a meeting's own notes file. The text reaches
    /// only hooks with scope `own_notes`.
    pub fn own_notes(meeting_id: &str, notes_path: &Path, meta: Meta) -> Self {
        HookEvent {
            note_path: Some(notes_path.to_string_lossy().into_owned()),
            meta,
            ..HookEvent::new(OWN_NOTES_SAVED, meeting_id)
        }
    }

    /// The event without any content: what `sokki events` streams.
    pub fn without_content(&self) -> Self {
        HookEvent {
            content: Content::default(),
            ..self.clone()
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_shape_matches_the_contract() {
        let e = HookEvent::meeting(
            MEETING_NOTE_READY,
            "2026-11-26_14-03-00",
            Some(Path::new("/notes/Team sync.md")),
            Meta {
                title: Some("Team sync".into()),
                app: Some("Zoom".into()),
                ..Default::default()
            },
        );
        let v: serde_json::Value = serde_json::to_value(&e).unwrap();
        assert_eq!(v["v"], 1);
        assert_eq!(v["event"], "meeting.note_ready");
        assert_eq!(v["id"], "2026-11-26_14-03-00");
        assert_eq!(v["note_path"], "/notes/Team sync.md");
        assert_eq!(v["meta"]["title"], "Team sync");
        assert_eq!(v["meta"]["participants"], serde_json::json!([]));
        assert!(v["content"].as_object().unwrap().is_empty());
        assert!(v.get("schedule").is_none());
        assert!(e.at.len() >= 19, "{}", e.at);
    }

    #[test]
    fn dictation_ids_and_text() {
        let e = HookEvent::dictation(42, "Hallo Welt", Meta::default());
        assert_eq!(e.id, "d:42");
        assert_eq!(e.content.text.as_deref(), Some("Hallo Welt"));
        assert!(e.without_content().content.is_empty());
        let back: HookEvent = serde_json::from_str(&e.to_json()).unwrap();
        assert_eq!(back, e);
    }
}

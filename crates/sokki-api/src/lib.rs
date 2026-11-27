//! The local API between Sokki and its clients (the `sokki` CLI, the
//! Omarchy shell plugin, hooks and scripts). This file is the
//! specification. Served by `sokki daemon` on Linux and by the app on
//! macOS (`src-tauri/src/local_api.rs`), the same on both.
//!
//! Transport: a Unix socket ([`socket_path`]), one JSON object per line in
//! each direction:
//! - Linux: `$XDG_RUNTIME_DIR/sokki/sokki.sock`
//! - macOS: `~/Library/Application Support/org.sokki.app/sokki.sock`
//!
//! `SOKKI_SOCKET` overrides both (tests).
//!
//! Searching what Sokki recorded is not part of this API: the CLI reads
//! the index (`crates/sokki-index`) directly, so it works while Sokki is
//! closed.
//!
//! - A client sends one [`Request`] and reads one [`Reply`].
//! - `{"cmd":"subscribe"}` instead keeps the connection open: the daemon
//!   writes an [`Event::State`] at once and then one [`Event`] per line
//!   whenever something changes.
//!
//! Events carry the whole state (a snapshot), never a diff, so a client
//! that reconnects or misses a line is correct with the next one.
//!
//! ## Hook events
//!
//! Subscribers also get one `sokki_engine::hooks::HookEvent` line per event hooks run on
//! (`meeting.recorded`, `meeting.note_ready`, `meeting.speakers_named`,
//! `meeting.deleted`, `dictation.done`, `own_notes.saved`, `schedule`; see
//! `sokki_engine::hooks::event`). Its `event` field is dotted, so it never
//! clashes with `state`, `note`, `error` or `offline`; parse it as a
//! `HookEvent` when the value contains a dot. On the socket `content` is
//! always empty: a hook gets content only within its `scope`, and a client
//! reads the rest with the CLI.
//!
//! ```json
//! {"v":1,"event":"meeting.note_ready","id":"2026-11-26_14-03-00",
//!  "at":"2026-11-26T14:40:12+01:00","note_path":"/…/Team sync.md",
//!  "meta":{"title":"Team sync","started_at":"2026-11-26T14:03:00+01:00",
//!          "duration_s":2040.0,"app":"Zoom","participants":["Ich","Anna"]},
//!  "content":{}}
//! ```

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The macOS app's data folder name (its bundle id).
pub const MAC_APP_ID: &str = "org.sokki.app";

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Where the socket is on this platform (see the module docs).
pub fn socket_path() -> PathBuf {
    if let Some(p) = std::env::var_os("SOKKI_SOCKET").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    if cfg!(target_os = "macos") {
        return home()
            .join("Library/Application Support")
            .join(MAC_APP_ID)
            .join("sokki.sock");
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| std::env::temp_dir().join(format!("sokki-{}", unsafe { libc::getuid() })));
    runtime.join("sokki/sokki.sock")
}

/// What a client asks for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    /// The current [`State`].
    Status,
    /// Stream events on this connection.
    Subscribe,
    /// Start recording: "yes" to the prompt, or a manual start.
    MeetingRecord,
    /// "Not this time" to the prompt.
    MeetingDismiss,
    /// Stop the recording; the note is written afterwards.
    MeetingStop,
    /// Record if idle or asking, stop if recording.
    MeetingToggle,
    DictationStart,
    DictationStop,
    DictationToggle,
    /// Stop listening and throw the audio away.
    DictationCancel,
    /// Recent meetings, newest first.
    Notes {
        #[serde(default = "default_limit")]
        limit: usize,
    },
    /// The speakers of one meeting, for naming them.
    Speakers { meeting: String },
    /// Give a speaker a name. With `remember`, the voice is recognised in
    /// later meetings. The note is rewritten with the name.
    NameSpeaker {
        meeting: String,
        speaker: String,
        name: String,
        #[serde(default = "yes")]
        remember: bool,
    },
    /// Download the speech and speaker models now (progress in the state).
    InstallModels,
    /// Re-read the config file.
    Reload,
}

fn default_limit() -> usize {
    10
}

fn yes() -> bool {
    true
}

/// The answer to a [`Request`] (not used for `subscribe`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<State>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<Vec<NoteInfo>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speakers: Option<Vec<SpeakerInfo>>,
}

impl Reply {
    pub fn ok() -> Self {
        Reply {
            ok: true,
            error: None,
            state: None,
            notes: None,
            speakers: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Reply {
            ok: false,
            error: Some(message.into()),
            ..Reply::ok()
        }
    }
}

/// What the daemon pushes to subscribers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// Everything a UI shows, whole.
    State(State),
    /// A note was written.
    Note(NoteInfo),
    /// Something failed that the person should know about.
    Error { message: String },
    /// Written by `sokki events` (not the daemon) while the daemon cannot
    /// be reached; the next `State` follows once it can.
    Offline,
}

/// The daemon's whole visible state.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub meeting: MeetingState,
    /// The meeting being turned into a note, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processing: Option<Processing>,
    /// Meetings waiting to be processed after the current one.
    #[serde(default)]
    pub queued: usize,
    pub dictation: DictationState,
    pub models: ModelsState,
}

/// Where the meeting lifecycle is.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum MeetingState {
    #[default]
    Idle,
    /// A call app uses the mic; the person is asked whether to record.
    Asking {
        /// Display name, e.g. "Google Meet", "Zoom".
        app: String,
        /// Unix time in ms when the question appeared.
        since_ms: u64,
        /// Unix time in ms when an unanswered question counts as "no".
        expires_ms: u64,
    },
    Recording {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        app: Option<String>,
        since_ms: u64,
        /// Whether the call's audio is recorded too (else only the mic).
        call_audio: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Processing {
    pub id: String,
    /// "preparing", "diarizing", "transcribing", "summarizing",
    /// "writing".
    pub stage: String,
    /// 0.0–1.0 when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<f32>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DictationState {
    #[default]
    Idle,
    Listening,
    Transcribing,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelsState {
    /// Speech, speaker and voice models are all on disk.
    pub ready: bool,
    /// A download is running (the speech models, then the language model).
    #[serde(default)]
    pub busy: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloading: Option<Download>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Download {
    pub file: String,
    pub done: u64,
    pub total: u64,
}

/// One meeting in the list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteInfo {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// RFC 3339, local time.
    pub started_at: String,
    pub duration_s: f64,
    /// "recording", "queued", "processing", "done", "failed".
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
}

/// One speaker of a meeting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeakerInfo {
    /// As in the note, e.g. "Speaker 2" or "Anna".
    pub name: String,
    /// Whether this speaker still has a placeholder name.
    pub unnamed: bool,
    /// Whether a voice sample exists, so the name can be remembered.
    pub has_voice: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_flat_json_with_cmd() {
        let r: Request = serde_json::from_str(r#"{"cmd":"meeting_record"}"#).unwrap();
        assert_eq!(r, Request::MeetingRecord);
        let r: Request = serde_json::from_str(r#"{"cmd":"notes"}"#).unwrap();
        assert_eq!(r, Request::Notes { limit: 10 });
        let r: Request = serde_json::from_str(
            r#"{"cmd":"name_speaker","meeting":"m1","speaker":"Speaker 2","name":"Anna"}"#,
        )
        .unwrap();
        assert!(matches!(r, Request::NameSpeaker { remember: true, .. }));
        assert_eq!(
            serde_json::to_string(&Request::DictationToggle).unwrap(),
            r#"{"cmd":"dictation_toggle"}"#
        );
    }

    #[test]
    fn state_event_shape() {
        let e = Event::State(State {
            meeting: MeetingState::Asking {
                app: "Google Meet".into(),
                since_ms: 1,
                expires_ms: 2,
            },
            ..Default::default()
        });
        let v: serde_json::Value = serde_json::to_value(&e).unwrap();
        assert_eq!(v["event"], "state");
        assert_eq!(v["meeting"]["phase"], "asking");
        assert_eq!(v["meeting"]["app"], "Google Meet");
        assert_eq!(v["dictation"], "idle");
        assert_eq!(v["models"]["ready"], false);
    }
}

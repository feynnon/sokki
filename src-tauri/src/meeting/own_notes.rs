//! The person's own notes of a meeting, kept next to its audio as
//! `meetings/<id>/notes.md` ([`meeting_core::own_notes`]). Saved as they
//! are typed; deleting the audio keeps them.

use anyhow::{Context, Result};
use meeting_core::own_notes::{self, NoteLine};
use std::path::PathBuf;

use super::store::{MeetingRecord, MeetingStatus, MeetingStore};

fn path(store: &MeetingStore, id: &str) -> PathBuf {
    store.audio_dir(id).join(own_notes::FILE_NAME)
}

/// The meeting's notes, every line as typed.
pub fn read(store: &MeetingStore, id: &str) -> Vec<NoteLine> {
    std::fs::read_to_string(path(store, id))
        .map(|md| own_notes::parse(&md))
        .unwrap_or_default()
}

/// What processing hands the engine; `None` when nothing was typed.
pub fn for_processing(store: &MeetingStore, id: &str) -> Option<sokki_engine::OwnNotes> {
    let lines = read(store, id);
    if own_notes::content_lines(&lines).is_empty() {
        return None;
    }
    Some(sokki_engine::OwnNotes {
        id: id.to_string(),
        lines,
    })
}

/// Seconds since the recording started, while it records.
fn seconds_in(record: &MeetingRecord) -> Option<u32> {
    if record.status != MeetingStatus::Recording {
        return None;
    }
    let start = chrono::DateTime::parse_from_rfc3339(&record.started_at).ok()?;
    let secs = chrono::Local::now()
        .signed_duration_since(start)
        .num_seconds();
    u32::try_from(secs).ok()
}

/// Saves the editor's text. Each line keeps the time it was started at.
pub fn save(store: &MeetingStore, record: &MeetingRecord, text: &str) -> Result<()> {
    let file = path(store, &record.id);
    let old = read(store, &record.id);
    let texts: Vec<&str> = text.split('\n').collect();
    let lines = own_notes::merge_times(&old, &texts, seconds_in(record));
    // Nothing typed and no file yet: nothing to keep.
    if own_notes::content_lines(&lines).is_empty() && !file.exists() {
        return Ok(());
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let md = own_notes::render(&record.id, Some(&record.started_at), &lines);
    let tmp = file.with_extension("md.tmp");
    std::fs::write(&tmp, md).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &file).with_context(|| format!("saving {}", file.display()))
}

/// The editor's text from the saved lines.
pub fn text(store: &MeetingStore, id: &str) -> String {
    read(store, id)
        .into_iter()
        .map(|l| l.text)
        .collect::<Vec<_>>()
        .join("\n")
}

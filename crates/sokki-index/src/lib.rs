//! sokki-index: one SQLite file (`index.sqlite`) that makes everything Sokki
//! recorded searchable for scripts, hooks and agents. Model-free: keyword
//! search (FTS5 unicode61) plus a trigram table, so a German compound like
//! "Rahmenvertrag" is found by "Vertrag".
//!
//! The index is derived. The truth stays where it is:
//! - the meeting notes (Markdown in the person's notes folder),
//! - each meeting's folder: its record (`meetings/index.json` on the Mac,
//!   `meetings/<id>/meeting.json` on Linux), `segments.jsonl` (times,
//!   track and speaker id per transcript paragraph), `items.jsonl` (items
//!   hooks added), `notes.md` (the person's own notes),
//! - the dictation history (`history.db`, macOS).
//!
//! [`sync`] brings the index up to date incrementally (a fingerprint per
//! source: file sizes and modification times), so it can run before every
//! query. [`rebuild`] throws it away and builds it again. Every row is keyed
//! by its source; a meeting that is deleted, or marked `private: true`,
//! loses all its rows in one transaction, with `secure_delete` on.
//!
//! Never indexed: the typing journal, the sample meeting, private meetings.
//!
//! Everything indexed is readable: by the `sokki` commands, and with plain
//! SQL ([`sql`], the views in [`db`]); whatever a cloud agent reads leaves
//! the computer.

pub mod db;
#[cfg(feature = "fixtures")]
pub mod fixtures;
pub mod items;
pub mod parse;
pub mod query;
pub mod sql;
pub mod sync;
pub mod time;
pub mod write;

pub use db::Index;
pub use query::{GetOpts, ListOpts, Mode, SearchOpts};
pub use sync::{rebuild, sync, sync_meeting, Paths, SyncReport};

/// "mm:ss" or "h:mm:ss" for a time in ms, as in the notes.
pub fn mmss(ms: i64) -> String {
    meeting_core::format_mmss(ms.max(0) as f64 / 1000.0)
}

/// A meeting's cite: `m:<id>@mm:ss`, or `m:<id>` without a time.
pub fn cite(prefix: &str, id: &str, at_ms: Option<i64>) -> String {
    match at_ms {
        Some(ms) => format!("{prefix}:{id}@{}", mmss(ms)),
        None => format!("{prefix}:{id}"),
    }
}

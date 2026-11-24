//! A meeting's name. Robin, 2026-11-24: "Meetings sollten einen Namen
//! bekommen, mit Fertigstellung der Meeting summary. Im Idealfall schon
//! während des Meetings."
//!
//! - While it records: once [`PROVISIONAL_AFTER`] has passed, the language
//!   model names it from what the live transcript has heard so far (once),
//!   in the background band so the recording keeps the Mac. OFF for now
//!   ([`DURING_RECORDING`]); the test app turns it on.
//! - With the summary: the summary's title replaces that guess.
//! - A title the person typed is never replaced, and goes into the note.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sokki_engine::live::Heard;
use tauri::{AppHandle, Manager};

use super::store::{MeetingRecord, MeetingStatus, MeetingStore};
use super::MeetingManager;

/// How long a meeting records before it gets a provisional title: enough
/// talk to name it.
const PROVISIONAL_AFTER: Duration = Duration::from_secs(5 * 60);

/// How often the provisional title waits for a first word, or for an
/// earlier meeting's summary to free the language model.
const RETRY_EVERY: Duration = Duration::from_secs(60);

/// Off: loading the language model (~6 GB) while a meeting records is not
/// safe yet. On 2026-11-24 heavy load on the Mac silenced the system-audio
/// tap for the rest of a call, and nothing brings the tap back yet. Turn
/// this on once the capture recovers from a dead tap and a test meeting
/// under load shows no gaps in `system.flac`.
const DURING_RECORDING: bool = false;

/// When the provisional title runs, if it does: `SOKKI_TEST_TITLE_AFTER_S`
/// in the test app (scripts/test-meeting.sh) turns it on there.
fn provisional_after() -> Option<Duration> {
    let test = std::env::var("SOKKI_TEST_TITLE_AFTER_S")
        .ok()
        .filter(|_| crate::test_mic::active())
        .and_then(|s| s.parse::<f64>().ok())
        .map(Duration::from_secs_f64);
    test.or(DURING_RECORDING.then_some(PROVISIONAL_AFTER))
}

/// A provisional title is set: the meeting still runs (or is being
/// processed), has no title yet, and the person typed none.
pub fn set_provisional(r: &mut MeetingRecord, title: &str) -> bool {
    let open = matches!(
        r.status,
        MeetingStatus::Recording | MeetingStatus::Processing { .. }
    );
    if !open || r.title.is_some() || r.title_by_hand || title.trim().is_empty() {
        return false;
    }
    r.title = Some(title.trim().to_string());
    r.title_provisional = true;
    true
}

/// Processing wrote the note, titled `note_title` (the summary's title, or
/// "Meeting <date> <time>" without one). A provisional title stays while
/// the summary is still to come.
pub fn set_processed(r: &mut MeetingRecord, note_title: &str, summary_missing: bool) {
    if r.title_by_hand || (summary_missing && r.title_provisional && r.title.is_some()) {
        return;
    }
    r.title = Some(note_title.to_string());
    r.title_provisional = false;
}

/// The summary came later, titled `title`.
pub fn set_summarized(r: &mut MeetingRecord, title: Option<&str>) {
    if r.title_by_hand {
        return;
    }
    if let Some(t) = title {
        r.title = Some(t.to_string());
        r.title_provisional = false;
    }
}

/// The person typed a title. `false` for an empty one (kept as it was).
pub fn set_typed(r: &mut MeetingRecord, title: &str) -> bool {
    let t = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() {
        return false;
    }
    r.title = Some(t);
    r.title_by_hand = true;
    r.title_provisional = false;
    true
}

/// Puts a typed title into the note's title heading, if the record has
/// one and a note.
pub fn write_typed_into_note(r: &MeetingRecord) {
    let (true, Some(title), Some(path)) = (r.title_by_hand, &r.title, &r.note_path) else {
        return;
    };
    let path = Path::new(path);
    let Ok(md) = std::fs::read_to_string(path) else {
        return;
    };
    let Some(new) = meeting_core::note::retitle_markdown(&md, title) else {
        return;
    };
    if new != md {
        let tmp = path.with_extension("md.tmp");
        let written = std::fs::write(&tmp, new).and_then(|_| std::fs::rename(&tmp, path));
        if let Err(e) = written {
            log::warn!("Meeting {}: the typed title is not in the note: {e}", r.id);
        }
    }
}

/// Names the meeting `id` once it has recorded [`PROVISIONAL_AFTER`], from
/// what `heard` has transcribed by then. Gives up when the meeting stops
/// first or the language model isn't ready; then the summary names it.
pub fn spawn_provisional(app: AppHandle, id: String, heard: Heard) {
    let Some(after) = provisional_after() else {
        return;
    };
    let started = Instant::now();
    let name_of_thread = format!("title-{id}");
    let spawned = std::thread::Builder::new()
        .name(name_of_thread.clone())
        .spawn(move || {
            let recording =
                || app.state::<Arc<MeetingManager>>().active_id().as_deref() == Some(id.as_str());
            let mut due = started + after;
            loop {
                while Instant::now() < due {
                    if !recording() {
                        return;
                    }
                    std::thread::sleep(Duration::from_secs(1).min(after));
                }
                if !recording() {
                    return;
                }
                let store = app.state::<Arc<MeetingStore>>().inner().clone();
                if store.get(&id).is_none_or(|r| r.title.is_some()) {
                    return;
                }
                let text = heard.text();
                // Nothing heard yet, or an earlier meeting's summary holds
                // the language model: try again in a minute.
                if text.split_whitespace().next().is_none() || super::process::busy() {
                    due = Instant::now() + RETRY_EVERY;
                    continue;
                }
                name(&app, &store, &id, &text);
                return;
            }
        });
    if let Err(e) = spawned {
        log::warn!("{name_of_thread}: no provisional title ({e})");
    }
}

fn name(app: &AppHandle, store: &MeetingStore, id: &str, text: &str) {
    let all = crate::settings::get_settings(app);
    let engine = super::process::engine_settings(&all);
    if !engine.summary {
        return;
    }
    if !sokki_engine::llm::is_ready(&engine.llm, &engine.user_name) {
        log::info!("Meeting {id}: no provisional title, the language model isn't ready");
        return;
    }
    let t0 = Instant::now();
    match sokki_engine::provisional_title(text, &engine, true) {
        Ok(title) => {
            log::info!(
                "Meeting title: provisional for {id} in {:.1} s from {} words",
                t0.elapsed().as_secs_f64(),
                text.split_whitespace().count()
            );
            if let Ok(r) = store.update(id, |r| {
                set_provisional(r, &title);
            }) {
                if r.title_provisional {
                    super::emit_changed(app, id);
                }
            }
        }
        Err(e) => log::warn!("Meeting {id}: no provisional title: {e:#}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(status: MeetingStatus) -> MeetingRecord {
        MeetingRecord {
            id: "m".into(),
            started_at: "2026-11-24T10:00:00+01:00".into(),
            duration_s: 0.0,
            status,
            tracks: Vec::new(),
            source_app: None,
            title: None,
            note_path: None,
            speakers: Vec::new(),
            warnings: Vec::new(),
            summary_missing: None,
            audio_deleted: false,
            online: None,
            title_by_hand: false,
            title_provisional: false,
        }
    }

    #[test]
    fn the_summary_replaces_the_guess() {
        let mut r = rec(MeetingStatus::Recording);
        assert!(set_provisional(&mut r, " Budget "));
        assert_eq!(r.title.as_deref(), Some("Budget"));
        assert!(r.title_provisional);
        // Only once.
        assert!(!set_provisional(&mut r, "Other"));
        set_processed(&mut r, "Budget und Zeitplan", false);
        assert_eq!(r.title.as_deref(), Some("Budget und Zeitplan"));
        assert!(!r.title_provisional);
    }

    #[test]
    fn the_guess_stays_until_a_late_summary() {
        let mut r = rec(MeetingStatus::Recording);
        set_provisional(&mut r, "Budget");
        // No summary yet: better the guess than "Meeting <date>".
        set_processed(&mut r, "Meeting 2026-11-24 10:00", true);
        assert_eq!(r.title.as_deref(), Some("Budget"));
        assert!(r.title_provisional);
        set_summarized(&mut r, Some("Budget und Zeitplan"));
        assert_eq!(r.title.as_deref(), Some("Budget und Zeitplan"));
        assert!(!r.title_provisional);
    }

    #[test]
    fn a_typed_title_is_never_replaced() {
        let mut r = rec(MeetingStatus::Recording);
        assert!(set_typed(&mut r, "  Mein   Titel "));
        assert!(!set_provisional(&mut r, "Budget"));
        set_processed(&mut r, "Budget und Zeitplan", false);
        set_summarized(&mut r, Some("Anderes"));
        assert_eq!(r.title.as_deref(), Some("Mein Titel"));
        assert!(r.title_by_hand && !r.title_provisional);
        assert!(!set_typed(&mut r, "   "));
        assert_eq!(r.title.as_deref(), Some("Mein Titel"));
    }

    #[test]
    fn no_guess_for_a_done_meeting() {
        let mut r = rec(MeetingStatus::Done);
        assert!(!set_provisional(&mut r, "Budget"));
        assert_eq!(r.title, None);
    }

    #[test]
    fn the_typed_title_goes_into_the_note() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("n.md");
        std::fs::write(&path, "# Old\n\nText\n").unwrap();
        let mut r = rec(MeetingStatus::Done);
        r.note_path = Some(path.to_string_lossy().into_owned());
        set_typed(&mut r, "New");
        write_typed_into_note(&r);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# New\n\nText\n");
    }
}

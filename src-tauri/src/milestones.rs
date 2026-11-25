//! Hours transcribed on this Mac, all time, for the milestone card at 100,
//! 200, 500 and 1,000 hours (a picture to share and the ask to sponsor).
//! Dictations and finished meetings add their length; deleting them later
//! doesn't subtract. Kept in `milestones.json` in the app data folder.
//!
//! Before the first milestone, once: the star card ("Enjoying Sokki?") at
//! [`STAR_HOURS`], right after a finished meeting or dictation.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

/// Hours at which the card appears, once each.
pub const MILESTONES: [u32; 4] = [100, 200, 500, 1000];

/// Hours after which Sokki asks once for a star on GitHub (Robin's call).
pub const STAR_HOURS: f64 = 5.0;

/// Event the window gets when the star card is due.
pub const STAR_EVENT: &str = "sokki-star-ask";

/// Set when a meeting or dictation finished since Sokki started, so the
/// star card never shows on a start alone.
static STAR_ARMED: AtomicBool = AtomicBool::new(false);

#[derive(Default, Serialize, Deserialize)]
struct State {
    seconds: f64,
    /// Milestones whose card was shown and closed.
    shown: Vec<u32>,
    /// Meetings already counted, so processing one again doesn't count twice.
    meetings: Vec<String>,
    /// Set once the first file was written with the meetings from before.
    started: bool,
    /// Words dictated, all time; deleting dictations doesn't subtract.
    #[serde(default)]
    words: u64,
    /// Set once `words` was filled from the dictations already in History.
    #[serde(default)]
    words_started: bool,
    /// Set once the star card was answered: starred, "Not now" or closed.
    #[serde(default)]
    star_closed: bool,
}

static LOCK: Mutex<()> = Mutex::new(());

fn path(app: &AppHandle) -> Option<PathBuf> {
    Some(
        crate::portable::app_data_dir(app)
            .ok()?
            .join("milestones.json"),
    )
}

fn load(app: &AppHandle) -> State {
    let mut state: State = path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if !state.started {
        // From before this counter: finished meetings count, dictation
        // lengths were never kept.
        #[cfg(target_os = "macos")]
        {
            use tauri::Manager;
            let Some(store) =
                app.try_state::<std::sync::Arc<crate::meeting::store::MeetingStore>>()
            else {
                return state;
            };
            for m in store.list() {
                if matches!(m.status, crate::meeting::store::MeetingStatus::Done) && !m.sample {
                    state.seconds += m.duration_s;
                    state.meetings.push(m.id);
                }
            }
        }
        state.started = true;
        save(app, &state);
    }
    if !state.words_started {
        // From before this counter: the dictations still in History.
        use tauri::Manager;
        if let Some(h) = app.try_state::<std::sync::Arc<crate::managers::history::HistoryManager>>()
        {
            // Only once History could be read, so a start before it is
            // ready doesn't lose the words from before.
            match h.dictation_texts() {
                Ok(texts) => {
                    state.words += texts.iter().map(|t| words(t)).sum::<u64>();
                    state.words_started = true;
                    save(app, &state);
                }
                Err(e) => log::warn!("Could not count the words in History: {e:#}"),
            }
        }
    }
    state
}

fn save(app: &AppHandle, state: &State) {
    let Some(p) = path(app) else { return };
    if let Ok(json) = serde_json::to_string(state) {
        if let Err(e) = std::fs::write(&p, json) {
            log::warn!("Could not save {}: {e}", p.display());
        }
    }
}

/// Adds a finished dictation.
pub fn add_dictation(app: &AppHandle, seconds: f64) {
    if !(seconds > 0.0) {
        return;
    }
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut state = load(app);
    state.seconds += seconds;
    save(app, &state);
    drop(_guard);
    finished(app);
}

/// Words in a text, as a word counter counts them.
pub fn words(text: &str) -> u64 {
    text.split_whitespace().count() as u64
}

/// Adds a dictation's words.
pub fn add_words(app: &AppHandle, text: &str) {
    let n = words(text);
    if n == 0 {
        return;
    }
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut state = load(app);
    state.words += n;
    save(app, &state);
}

/// "Your data": everything transcribed on this Mac, all time.
#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
pub struct Totals {
    /// Dictations and finished meetings, in seconds.
    pub seconds: f64,
    /// Words dictated.
    pub words: u64,
    /// Finished meetings.
    pub meetings: u32,
}

pub fn totals(app: &AppHandle) -> Totals {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let state = load(app);
    Totals {
        seconds: state.seconds,
        words: state.words,
        meetings: state.meetings.len() as u32,
    }
}

/// Adds a finished meeting, once per meeting.
pub fn add_meeting(app: &AppHandle, id: &str, seconds: f64) {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut state = load(app);
    if state.meetings.iter().any(|m| m == id) {
        return;
    }
    state.meetings.push(id.to_string());
    state.seconds += seconds.max(0.0);
    save(app, &state);
    drop(_guard);
    finished(app);
}

/// A meeting or dictation just finished: tells the window when the star
/// card is due now.
fn finished(app: &AppHandle) {
    STAR_ARMED.store(true, Ordering::SeqCst);
    if star_due(app) {
        if let Err(e) = app.emit(STAR_EVENT, ()) {
            log::warn!("Could not ask for the star card: {e}");
        }
    }
}

/// Hours so far; `SOKKI_DEBUG_HOURS=120` pretends 120 hours, to test the cards.
fn hours(state: &State) -> f64 {
    std::env::var("SOKKI_DEBUG_HOURS")
        .ok()
        .and_then(|h| h.parse::<f64>().ok())
        .unwrap_or(state.seconds / 3600.0)
}

/// Whether something records now: a dictation, a meeting or the screen.
fn recording(app: &AppHandle) -> bool {
    use std::sync::Arc;
    use tauri::Manager;
    if app
        .try_state::<Arc<crate::managers::audio::AudioRecordingManager>>()
        .is_some_and(|a| a.is_recording())
    {
        return true;
    }
    #[cfg(target_os = "macos")]
    {
        if app
            .try_state::<Arc<crate::meeting::MeetingManager>>()
            .is_some_and(|m| m.is_recording())
            || app
                .try_state::<Arc<crate::video::VideoManager>>()
                .is_some_and(|v| v.is_recording())
        {
            return true;
        }
    }
    false
}

/// Whether the star card shows now: once, at [`STAR_HOURS`], after a
/// meeting or dictation finished in this run, never while one records.
pub fn star_due(app: &AppHandle) -> bool {
    let state = {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        load(app)
    };
    star_due_at(
        hours(&state),
        STAR_ARMED.load(Ordering::SeqCst),
        state.star_closed,
        recording(app),
    )
}

fn star_due_at(hours: f64, armed: bool, closed: bool, recording: bool) -> bool {
    armed && !closed && !recording && hours >= STAR_HOURS
}

/// Closes the star card for good.
pub fn close_star(app: &AppHandle) {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut state = load(app);
    state.star_closed = true;
    save(app, &state);
}

/// The milestone card to show, if any.
#[derive(Clone, Debug, Serialize, specta::Type)]
pub struct Milestone {
    /// The milestone reached, e.g. 100.
    pub hours: u32,
    /// All hours so far, rounded down.
    pub total_hours: u32,
}

/// The highest milestone reached whose card wasn't closed yet.
pub fn due(app: &AppHandle) -> Option<Milestone> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let state = load(app);
    pick(hours(&state), &state.shown, state.star_closed)
}

/// The star card comes first: no milestone until it was answered.
fn pick(hours: f64, shown: &[u32], star_closed: bool) -> Option<Milestone> {
    if !star_closed {
        return None;
    }
    MILESTONES
        .iter()
        .rev()
        .find(|m| hours >= **m as f64)
        .filter(|m| !shown.contains(m))
        .map(|m| Milestone {
            hours: *m,
            total_hours: hours.floor() as u32,
        })
}

/// Closes the card for `hours` and every milestone below it.
pub fn close(app: &AppHandle, hours: u32) {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut state = load(app);
    for m in MILESTONES.iter().filter(|m| **m <= hours) {
        if !state.shown.contains(m) {
            state.shown.push(*m);
        }
    }
    save(app, &state);
}

#[cfg(test)]
mod tests {
    use super::{pick, star_due_at, words, STAR_HOURS};

    #[test]
    fn counts_words_like_a_word_counter() {
        assert_eq!(words("Hallo  du\nda, wie geht's?"), 5);
        assert_eq!(words("   "), 0);
    }

    #[test]
    fn shows_the_highest_milestone_reached_once() {
        assert!(pick(99.9, &[], true).is_none());
        assert_eq!(pick(100.0, &[], true).unwrap().hours, 100);
        assert_eq!(pick(230.0, &[], true).unwrap().hours, 200);
        assert!(pick(230.0, &[100, 200], true).is_none());
        assert_eq!(pick(1500.0, &[100, 200], true).unwrap().hours, 1000);
        assert_eq!(pick(1500.0, &[100, 200], true).unwrap().total_hours, 1500);
    }

    #[test]
    fn the_star_card_comes_before_the_first_donation_ask() {
        assert!(STAR_HOURS < 100.0);
        // Past 100 hours but the star card not answered yet: no milestone.
        assert!(pick(150.0, &[], false).is_none());
        assert_eq!(pick(150.0, &[], true).unwrap().hours, 100);
    }

    #[test]
    fn the_star_card_shows_once_after_a_finished_recording() {
        // At X hours, right after a meeting or dictation finished.
        assert!(star_due_at(STAR_HOURS, true, false, false));
        assert!(star_due_at(40.0, true, false, false));
        // Not before X hours.
        assert!(!star_due_at(STAR_HOURS - 0.01, true, false, false));
        // Not on a start alone, before anything finished in this run.
        assert!(!star_due_at(40.0, false, false, false));
        // Never while something records.
        assert!(!star_due_at(40.0, true, false, true));
        // Never again once answered (starred, "Not now" or X).
        assert!(!star_due_at(40.0, true, true, false));
    }
}

//! Hours transcribed on this Mac, all time, for the milestone card at 100,
//! 200, 500 and 1,000 hours (a picture to share and the ask to sponsor).
//! Dictations and finished meetings add their length; deleting them later
//! doesn't subtract. Kept in `milestones.json` in the app data folder.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::AppHandle;

/// Hours at which the card appears, once each.
pub const MILESTONES: [u32; 4] = [100, 200, 500, 1000];

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
/// `SOKKI_DEBUG_HOURS=120` pretends 120 hours, to test the card.
pub fn due(app: &AppHandle) -> Option<Milestone> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let state = load(app);
    let hours = std::env::var("SOKKI_DEBUG_HOURS")
        .ok()
        .and_then(|h| h.parse::<f64>().ok())
        .unwrap_or(state.seconds / 3600.0);
    pick(hours, &state.shown)
}

fn pick(hours: f64, shown: &[u32]) -> Option<Milestone> {
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
    use super::{pick, words};

    #[test]
    fn counts_words_like_a_word_counter() {
        assert_eq!(words("Hallo  du\nda, wie geht's?"), 5);
        assert_eq!(words("   "), 0);
    }

    #[test]
    fn shows_the_highest_milestone_reached_once() {
        assert!(pick(99.9, &[]).is_none());
        assert_eq!(pick(100.0, &[]).unwrap().hours, 100);
        assert_eq!(pick(230.0, &[]).unwrap().hours, 200);
        assert!(pick(230.0, &[100, 200]).is_none());
        assert_eq!(pick(1500.0, &[100, 200]).unwrap().hours, 1000);
        assert_eq!(pick(1500.0, &[100, 200]).unwrap().total_hours, 1500);
    }
}

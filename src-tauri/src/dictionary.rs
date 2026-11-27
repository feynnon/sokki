//! One dictionary for the Mac: a word added once is never underlined and never "corrected"
//! again (Robin, 26.11.2026: "Die Mac autocorrect unterringelt mir ja Wörter. Ich würde die
//! gerne direkt hinzufügen können ins dict").
//!
//! - The dictionary is the Wörterbuch (`meetings.glossary`). Dictation, summaries and
//!   autocorrect already read it (`AppSettings::all_vocabulary`).
//! - Its words are also taught to the Mac's spell checker, so no app underlines them
//!   ([`sync`]). Only words Sokki taught are ever taken back: they are listed in
//!   `spelling_taught_words.txt` in the app data folder. Learned words (`vocabulary_learned`)
//!   are not taught: their precision isn't proven yet (`docs/research/word-learning-2026-11.md`).
//! - After a word typed key by key that the spell checker doesn't know and autocorrect leaves,
//!   a pill offers to add it (click, or ⌃⌥W while it shows), once per word. A fix undone with
//!   Backspace adds the typed word right away, with an Undo on the pill.
//!
//! All of it only with `autocorrect_add_words` on (off by default; Robin tries it first,
//! 27.11.2026). Test mode never touches the real Mac's dictionary: it only lists the words.
//! Sokki's log never holds the words.
//! SPDX-License-Identifier: GPL-3.0-only

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::overlay::{self, WordPill};
use crate::settings::{self, AppSettings};

/// ⌃⌥W adds the offered word while its pill shows.
const W_KEY: u16 = 13;

/// Words offered on the pill since Sokki started: each only once.
static OFFERED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn taught_file(app: &AppHandle) -> Option<PathBuf> {
    Some(
        app.path()
            .app_data_dir()
            .ok()?
            .join("spelling_taught_words.txt"),
    )
}

fn declined_file(app: &AppHandle) -> Option<PathBuf> {
    Some(
        app.path()
            .app_data_dir()
            .ok()?
            .join("dictionary_declined_words.txt"),
    )
}

fn read_set(p: Option<PathBuf>) -> BTreeSet<String> {
    p.and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| {
            s.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn write_set(p: Option<PathBuf>, words: &BTreeSet<String>) {
    let Some(p) = p else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let body: String = words.iter().map(|w| format!("{w}\n")).collect();
    if let Err(e) = std::fs::write(&p, body) {
        log::warn!("dictionary: can't save {}: {e}", p.display());
    }
}

/// The single words of the Wörterbuch the spell checker should know ("Kelvaro Mendtz" is
/// two). Only words of letters (and inner hyphens or apostrophes), two letters or more.
fn spelling_words(s: &AppSettings) -> BTreeSet<String> {
    if !s.autocorrect_add_words {
        return BTreeSet::new();
    }
    s.meetings
        .glossary
        .iter()
        .flat_map(|e| e.split_whitespace())
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|w| plain_word(w, 2))
        .map(str::to_string)
        .collect()
}

/// A word of letters (inner - or ' allowed), at least `min` characters, with no digits.
fn plain_word(w: &str, min: usize) -> bool {
    w.chars().count() >= min
        && w.chars().next().is_some_and(char::is_alphabetic)
        && w.chars().last().is_some_and(char::is_alphabetic)
        && w.chars()
            .all(|c| c.is_alphabetic() || c == '-' || c == '\'')
}

/// What to teach and what to take back: `want` is the Wörterbuch now, `taught` what Sokki
/// taught before. Words the user taught macOS by hand are never in `taught`, so never taken.
fn plan(want: &BTreeSet<String>, taught: &BTreeSet<String>) -> (Vec<String>, Vec<String>) {
    (
        want.difference(taught).cloned().collect(),
        taught.difference(want).cloned().collect(),
    )
}

/// Teaches the Mac's spell checker what the Wörterbuch holds and takes back what was removed
/// (also everything, once the switch is off).
fn sync(app: &AppHandle) {
    let s = settings::get_settings(app);
    let want = spelling_words(&s);
    let taught = read_set(taught_file(app));
    let (learn, unlearn) = plan(&want, &taught);
    if learn.is_empty() && unlearn.is_empty() {
        return;
    }
    if crate::test_mic::active() {
        log::info!(
            "Test mode: the Mac's dictionary stays as it is ({} words to teach, {} to take back)",
            learn.len(),
            unlearn.len()
        );
    } else {
        #[cfg(target_os = "macos")]
        {
            for w in &learn {
                sokki_journal::mac::spell_teach(w, true);
            }
            for w in &unlearn {
                sokki_journal::mac::spell_teach(w, false);
            }
        }
        log::info!(
            "dictionary: taught the Mac's spell checker {} words, took back {}",
            learn.len(),
            unlearn.len()
        );
    }
    write_set(taught_file(app), &want);
}

/// Keeps the Mac's spell checker in step with the Wörterbuch, however it changes (the
/// Wörterbuch page, the pill, `sokki config`).
pub fn start(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("dictionary".into())
        .spawn(move || loop {
            sync(&app);
            std::thread::sleep(Duration::from_secs(2));
        });
}

fn in_dictionary(s: &AppSettings, word: &str) -> bool {
    s.all_vocabulary()
        .iter()
        .flat_map(|e| e.split_whitespace().map(str::to_string).collect::<Vec<_>>())
        .any(|w| w.eq_ignore_ascii_case(word) || w.to_lowercase() == word.to_lowercase())
}

/// Adds `word` to the Wörterbuch (and takes it off the list of words never to learn).
/// Returns false when it was there already.
pub fn add_word(app: &AppHandle, word: &str) -> bool {
    let word = word.trim();
    let mut s = settings::get_settings(app);
    if word.is_empty() || in_dictionary(&s, word) {
        return false;
    }
    s.meetings.glossary.push(word.to_string());
    s.meetings.glossary.sort_by_key(|w| w.to_lowercase());
    s.vocabulary_rejected
        .retain(|w| !w.eq_ignore_ascii_case(word));
    settings::write_settings(app, s);
    log::info!("dictionary: a word added");
    let app = app.clone();
    std::thread::spawn(move || sync(&app));
    true
}

/// Takes `word` out of the Wörterbuch (the pill's Undo).
pub fn remove_word(app: &AppHandle, word: &str) {
    let mut s = settings::get_settings(app);
    let before = s.meetings.glossary.len();
    s.meetings.glossary.retain(|w| w != word);
    if s.meetings.glossary.len() != before {
        settings::write_settings(app, s);
        log::info!("dictionary: a word taken out again");
        let app = app.clone();
        std::thread::spawn(move || sync(&app));
    }
}

/// Autocorrect left `word` as typed: offer it on the pill if the spell checker doesn't know
/// it, it isn't in the dictionary, and it wasn't offered or declined before.
pub fn offer(app: &AppHandle, word: &str) {
    let s = settings::get_settings(app);
    if !s.autocorrect_add_words || !plain_word(word, 3) || in_dictionary(&s, word) {
        return;
    }
    let key = word.to_lowercase();
    {
        let mut o = OFFERED.lock().unwrap_or_else(|e| e.into_inner());
        let o = o.get_or_insert_with(HashSet::new);
        if o.contains(&key) {
            return;
        }
        if read_set(declined_file(app)).contains(&key) {
            o.insert(key);
            return;
        }
        #[cfg(target_os = "macos")]
        if sokki_journal::mac::spell_known(word, &s.spoken_languages) {
            return;
        }
        o.insert(key);
    }
    let shown = overlay::show_word_pill(
        app,
        WordPill {
            kind: "offer",
            word: word.to_string(),
        },
    );
    if shown {
        let (a, w) = (app.clone(), word.to_string());
        sokki_keys::catch_key(
            W_KEY,
            sokki_keys::hold::CONTROL | sokki_keys::hold::OPTION,
            overlay::WORD_HOLD,
            move || added(&a, &w),
        );
        log::info!("dictionary: offered a word");
    }
}

/// The offered word was added (click or ⌃⌥W): show it as added, with Undo.
fn added(app: &AppHandle, word: &str) {
    sokki_keys::stop_catching();
    add_word(app, word);
    overlay::show_word_pill(
        app,
        WordPill {
            kind: "added",
            word: word.to_string(),
        },
    );
}

/// A fix was undone with Backspace: the typed word goes to the Wörterbuch at once.
pub fn kept(app: &AppHandle, word: &str) {
    let s = settings::get_settings(app);
    if !s.autocorrect_add_words || !plain_word(word, 2) {
        return;
    }
    if add_word(app, word) {
        overlay::show_word_pill(
            app,
            WordPill {
                kind: "kept",
                word: word.to_string(),
            },
        );
    }
}

/// The pill's button: add the offered word.
#[tauri::command]
#[specta::specta]
pub fn dictionary_add_word(app: AppHandle, word: String) -> Result<(), String> {
    added(&app, &word);
    Ok(())
}

/// The pill's Undo: take the word out again.
#[tauri::command]
#[specta::specta]
pub fn dictionary_remove_word(app: AppHandle, word: String) -> Result<(), String> {
    sokki_keys::stop_catching();
    remove_word(&app, &word);
    overlay::dismiss_overlay_notice(&app);
    Ok(())
}

/// The pill's X: never offer this word again.
#[tauri::command]
#[specta::specta]
pub fn dictionary_decline_word(app: AppHandle, word: String) -> Result<(), String> {
    sokki_keys::stop_catching();
    let mut d = read_set(declined_file(&app));
    d.insert(word.to_lowercase());
    write_set(declined_file(&app), &d);
    overlay::dismiss_overlay_notice(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(words: &[&str]) -> BTreeSet<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn teaches_new_words_and_takes_back_only_its_own() {
        let (learn, unlearn) = plan(
            &set(&["Quorbit", "Tarnisio"]),
            &set(&["Tarnisio", "Zentrix"]),
        );
        assert_eq!(learn, vec!["Quorbit"]);
        assert_eq!(unlearn, vec!["Zentrix"]);
    }

    #[test]
    fn only_glossary_words_of_letters_with_the_switch_on() {
        let mut s = crate::settings::get_default_settings();
        s.meetings.glossary = vec![
            "Kelvaro Mendtz".into(),
            "R2D2".into(),
            "O'Brannwick".into(),
            "x".into(),
        ];
        s.vocabulary_learned = vec!["Nuvexa".into()];
        assert!(spelling_words(&s).is_empty(), "off: nothing taught");
        s.autocorrect_add_words = true;
        assert_eq!(
            spelling_words(&s),
            set(&["Kelvaro", "Mendtz", "O'Brannwick"])
        );
    }

    #[test]
    fn offers_plain_words_only() {
        assert!(plain_word("Quorbit", 3));
        assert!(!plain_word("ab", 3));
        assert!(!plain_word("v2", 2));
        assert!(!plain_word("-ab-", 2));
    }
}

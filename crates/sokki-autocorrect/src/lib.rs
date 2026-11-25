//! Sokki fixes typos as you type, in the apps where macOS doesn't (Chrome, Slack, Claude …).
//!
//! The phone keyboard fixes a word when the space after it is typed. On the Mac Sokki does
//! the same from outside the app, through Accessibility:
//!
//! 1. **No new keyboard hook.** Sokki's existing hook only counts key presses
//!    (`sokki_keys::key_downs`). When the count changes, a thread of Sokki's own reads the
//!    focused text field. The hook never waits for it, so typing can't stall, and the hook
//!    is gone within 250 ms of Accessibility being switched off (`src-tauri/src/accessibility.rs`).
//! 2. **Only a word just typed.** Every letter of the word and the character after it (space,
//!    . , ! ? ; :) must have come in key by key: each read shows the last one's text plus no
//!    more characters than keys were pressed, at the cursor. Pastes, dictation, autocomplete
//!    and other programs' changes are never touched.
//! 3. **Decide.** The phone's model ([`sokki_typos::Typos`]) on that word and the words
//!    before it in its sentence.
//! 4. **Replace safely.** Read the field again; only if the word is still there, unchanged,
//!    and no key was pressed since, select it and type the fix over it, then put the cursor
//!    back. Read once more and log whether the field now holds exactly what was meant.
//! 5. **Undo.** Backspace right after a fix (it deletes the space) brings the typed word back,
//!    and Sokki never changes that word again, as on the phone.
//!
//! Never: password or protected fields, secure input, terminals, password managers, code
//! editors, Sokki itself, and Apple's own apps, where macOS autocorrect already works (two
//! correctors on one word would fight). Off until the user switches it on.
//! SPDX-License-Identifier: GPL-3.0-only

mod text;

#[cfg(target_os = "macos")]
pub mod mac;
#[cfg(target_os = "macos")]
pub mod runner;

use std::collections::HashSet;
use std::path::PathBuf;

pub use sokki_typos;
pub use text::{u16_len, Utf16};

/// A text field, as Accessibility shows it. Offsets and lengths are in UTF-16 code units,
/// as macOS counts them.
pub trait Field {
    /// The app's bundle id.
    fn app(&self) -> &str;
    /// The field's whole text.
    fn value(&self) -> Option<String>;
    /// The selection: (start, length). Length 0 is a cursor.
    fn selection(&self) -> Option<(usize, usize)>;
    /// Whether Sokki can select text and type over it in this field.
    fn replaceable(&self) -> bool;
    fn select(&self, start: usize, len: usize) -> bool;
    /// Types `text` over the selection.
    fn insert(&self, text: &str) -> bool;
}

/// Apps where Sokki never fixes: macOS already does (Apple's own apps), a typo fix would
/// break a command or code, or what is typed is a secret.
pub const NEVER: &[&str] = &[
    // terminals
    "com.googlecode.iterm2",
    "dev.warp.Warp-Stable",
    "com.mitchellh.ghostty",
    "net.kovidgoyal.kitty",
    "org.alacritty",
    "com.github.wez.wezterm",
    // code editors
    "com.microsoft.VSCode",
    "com.microsoft.VSCodeInsiders",
    "com.todesktop.230313mzl4w4u92", // Cursor
    "dev.zed.Zed",
    "com.sublimetext.4",
    // password managers
    "com.1password.1password",
    "com.bitwarden.desktop",
    // Sokki
    "org.sokki.app",
    "org.sokki.test",
];

/// Whether Sokki may fix words in this app.
pub fn allowed(app: &str, excluded: &[String]) -> bool {
    !app.is_empty()
        && !app.starts_with("com.apple.")
        && !app.starts_with("com.jetbrains.")
        && !NEVER.contains(&app)
        && !excluded.iter().any(|x| x.eq_ignore_ascii_case(app))
}

/// What one look at the field did.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Nothing to do: no word just ended, or it needs no fix.
    Nothing,
    /// A fix was found but not made, and why.
    Skipped(&'static str),
    /// The typed word was replaced; `exact`: the field now holds exactly what was meant.
    Fixed {
        typed: String,
        fix: String,
        exact: bool,
    },
    /// Backspace right after a fix: the typed word is back and kept from now on.
    Undone { typed: String, fix: String },
}

struct Last {
    app: String,
    text: String,
    cursor: usize,
    /// key presses counted at this read
    keys: u64,
    /// UTF-16 units typed in a row, key by key, ending at the cursor
    run: usize,
}

struct Done {
    app: String,
    /// where the fix starts, UTF-16
    start: usize,
    typed: String,
    fix: String,
    /// the field's text right after the fix
    text: String,
    /// the cursor right after the fix (after the word-ending character)
    cursor: usize,
}

/// Fixes typos as they are typed. One per reading thread.
pub struct Autocorrect {
    typos: sokki_typos::Typos,
    last: Option<Last>,
    done: Option<Done>,
    /// words the user undid a fix of: never changed again
    kept: HashSet<String>,
    kept_file: Option<PathBuf>,
}

const WORD_END: &[char] = &[' ', '.', ',', '!', '?', ';', ':'];

impl Autocorrect {
    /// `kept_file`: one word per line, the words the user kept by undoing a fix.
    pub fn new(mut typos: sokki_typos::Typos, kept_file: Option<PathBuf>) -> Autocorrect {
        let kept: HashSet<String> = kept_file
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|s| {
                s.lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        typos.lexicon.set_personal(kept.iter().cloned());
        Autocorrect {
            typos,
            last: None,
            done: None,
            kept,
            kept_file,
        }
    }

    /// Forget the field: focus moved, or the switch went off.
    pub fn reset(&mut self) {
        self.last = None;
        self.done = None;
    }

    /// Look at the focused field after a key press. `keys`: reads the key-press count
    /// (`sokki_keys::key_downs`); a change between the read and the swap means the user typed
    /// on, and the swap is left out.
    pub fn on_key(&mut self, field: &dyn Field, keys: &dyn Fn() -> u64) -> Outcome {
        let keys_at_read = keys();
        let (Some(text), Some((cursor, sel))) = (field.value(), field.selection()) else {
            self.reset();
            return Outcome::Nothing;
        };
        let app = field.app().to_string();
        // How much of the text before the cursor was typed key by key: it grows when the field
        // holds the last read's text plus no more characters than keys were pressed, at the
        // cursor. A paste (one key, many characters), dictation (no keys) or a move resets it.
        let prev = self.last.take().filter(|p| p.app == app);
        let run = match &prev {
            Some(p)
                if cursor > p.cursor
                    && cursor - p.cursor <= keys_at_read.saturating_sub(p.keys) as usize
                    && text::remove_u16(&text, p.cursor, cursor - p.cursor) == p.text =>
            {
                p.run + (cursor - p.cursor)
            }
            _ => 0,
        };
        self.last = Some(Last {
            app: app.clone(),
            text: text.clone(),
            cursor,
            keys: keys_at_read,
            run,
        });
        if sel != 0 {
            return Outcome::Nothing;
        }
        // Backspace right after a fix: bring the typed word back
        if let Some(d) = self.done.take() {
            if d.app == app
                && d.cursor > 0
                && cursor == d.cursor - 1
                && text == text::remove_u16(&d.text, d.cursor - 1, 1)
            {
                return self.undo(field, d, &text, keys, keys_at_read);
            }
        }
        if run == 0 || cursor == 0 {
            return Outcome::Nothing;
        }
        let u = Utf16::new(&text);
        let Some(end) = u.char_before(cursor) else {
            return Outcome::Nothing;
        };
        if !WORD_END.contains(&end) {
            return Outcome::Nothing;
        }
        let Some((start, word, prevs)) = text::word_before(&u, cursor - 1) else {
            return Outcome::Nothing;
        };
        // every letter of the word and the character after it were typed
        if run < cursor - start {
            return Outcome::Nothing;
        }
        if self.kept.contains(&sokki_typos::fold(&word)) {
            return Outcome::Nothing;
        }
        let prev_refs: Vec<&str> = prevs.iter().map(|s| s.as_str()).collect();
        let Some(fix) = self.typos.correction(&word, &prev_refs, None) else {
            return Outcome::Nothing;
        };
        if !field.replaceable() {
            return Outcome::Skipped("field can't be replaced");
        }
        self.replace(field, &text, start, &word, &fix, cursor, keys, keys_at_read)
    }

    #[allow(clippy::too_many_arguments)]
    fn replace(
        &mut self,
        field: &dyn Field,
        text: &str,
        start: usize,
        typed: &str,
        fix: &str,
        cursor: usize,
        keys: &dyn Fn() -> u64,
        keys_at_read: u64,
    ) -> Outcome {
        let len = u16_len(typed);
        // read again: still the same text and cursor, and no key since
        if field.value().as_deref() != Some(text) || field.selection() != Some((cursor, 0)) {
            return Outcome::Skipped("field changed");
        }
        if keys() != keys_at_read {
            return Outcome::Skipped("typing on");
        }
        if !field.select(start, len) {
            return Outcome::Skipped("can't select");
        }
        if keys() != keys_at_read {
            // A key came while the word was selected. The app may type it over the word:
            // then put the word back, and the key where the cursor was.
            field.select(cursor, 0);
            let repaired = repair(field, text, start, len, typed, cursor);
            self.reset();
            return Outcome::Skipped(if repaired {
                "typing on, word put back"
            } else {
                "typing on"
            });
        }
        if !field.insert(fix) {
            field.select(cursor, 0);
            return Outcome::Skipped("can't type the fix");
        }
        let after = cursor + u16_len(fix) - len;
        field.select(after, 0);
        let expected = text::replace_u16(text, start, len, fix);
        let exact = field.value().as_deref() == Some(expected.as_str());
        if !exact {
            log::warn!(
                "autocorrect: {} doesn't hold what was meant after a fix",
                field.app()
            );
        }
        self.last = Some(Last {
            app: field.app().to_string(),
            text: field.value().unwrap_or_else(|| expected.clone()),
            cursor: after,
            keys: keys(),
            run: 0,
        });
        self.done = exact.then(|| Done {
            app: field.app().to_string(),
            start,
            typed: typed.to_string(),
            fix: fix.to_string(),
            text: expected,
            cursor: after,
        });
        Outcome::Fixed {
            typed: typed.to_string(),
            fix: fix.to_string(),
            exact,
        }
    }

    fn undo(
        &mut self,
        field: &dyn Field,
        d: Done,
        text: &str,
        keys: &dyn Fn() -> u64,
        keys_at_read: u64,
    ) -> Outcome {
        let flen = u16_len(&d.fix);
        if keys() != keys_at_read || !field.select(d.start, flen) || !field.insert(&d.typed) {
            return Outcome::Skipped("can't undo");
        }
        let after = d.start + u16_len(&d.typed);
        field.select(after, 0);
        let restored = text::replace_u16(text, d.start, flen, &d.typed);
        self.last = Some(Last {
            app: d.app.clone(),
            text: field.value().unwrap_or(restored),
            cursor: after,
            keys: keys(),
            run: 0,
        });
        self.keep(&d.typed);
        Outcome::Undone {
            typed: d.typed,
            fix: d.fix,
        }
    }

    fn keep(&mut self, word: &str) {
        let w = sokki_typos::fold(word);
        if !self.kept.insert(w.clone()) {
            return;
        }
        self.typos.lexicon.set_personal(self.kept.iter().cloned());
        if let Some(p) = &self.kept_file {
            use std::io::Write;
            let saved = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
                .and_then(|mut f| writeln!(f, "{w}"));
            if let Err(e) = saved {
                log::warn!("autocorrect: can't save a kept word: {e}");
            }
        }
    }
}

/// After a key landed on the selected word: if the field shows the word replaced by what was
/// typed, put the word back and the typed characters at the cursor, where they were meant.
fn repair(
    field: &dyn Field,
    text: &str,
    start: usize,
    len: usize,
    typed: &str,
    cursor: usize,
) -> bool {
    let Some(v) = field.value() else {
        return false;
    };
    let t: Vec<u16> = text.encode_utf16().collect();
    let got: Vec<u16> = v.encode_utf16().collect();
    let (pre, post) = (&t[..start], &t[start + len..]);
    if got == t
        || got.len() < pre.len() + post.len()
        || !got.starts_with(pre)
        || !got.ends_with(post)
    {
        return false;
    }
    let s = &got[pre.len()..got.len() - post.len()];
    if s.is_empty() || s.len() > 8 {
        return false;
    }
    let keys_typed = String::from_utf16_lossy(s);
    let ok = field.select(start, s.len())
        && field.insert(typed)
        && field.select(cursor, 0)
        && field.insert(&keys_typed);
    field.select(cursor + s.len(), 0);
    let mut want = t[..cursor].to_vec();
    want.extend_from_slice(s);
    want.extend_from_slice(&t[cursor..]);
    let exact = field
        .value()
        .map(|x| x.encode_utf16().collect::<Vec<u16>>())
        == Some(want);
    log::warn!(
        "autocorrect: a key landed on the word being fixed in {}; put back: {}",
        field.app(),
        ok && exact
    );
    ok && exact
}

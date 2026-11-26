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

pub mod fixlog;
pub mod tail;
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
    /// Waits briefly until the field's text passes `ok` (apps may show a change a few ms late).
    fn settle(&self, ok: &dyn Fn(&str) -> bool) -> bool {
        self.value().is_some_and(|v| ok(&v))
    }
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
    /// The model has a fix, but Sokki didn't see every letter of the word typed (a paste,
    /// dictation, or a read it missed), so it left the word. Recorded to find such losses.
    Missed {
        typed: String,
        fix: String,
        why: &'static str,
    },
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
    /// The last read showed the field as before although keys were pressed: the app hasn't
    /// updated what Accessibility sees yet (Chrome takes a few ms). Look again soon.
    stale: bool,
}

/// Characters that end a word. U+00A0: rich editors in browsers (Slack, Claude, Gmail) show a
/// space typed at the end of a line as a no-break space.
const WORD_END: &[char] = &[' ', '\u{a0}', '.', ',', '!', '?', ';', ':'];

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
            stale: false,
        }
    }

    /// The letter keys of the layout in use, row by row ([`sokki_typos::Typos::set_keyboard`]).
    pub fn set_keyboard(&mut self, rows: &[String; 3]) {
        self.typos.set_keyboard(rows);
    }

    /// Forget the field: focus moved, or the switch went off.
    pub fn reset(&mut self) {
        self.last = None;
        self.done = None;
        self.stale = false;
    }

    /// Whether the last look came too early (the field didn't show the new keys yet), so the
    /// caller should look again shortly even without a new key.
    pub fn stale(&self) -> bool {
        self.stale
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
        // Keys were pressed, but the field looks exactly as at the last read: the app hasn't
        // caught up yet. Keep everything as it was and look again, or the typed run is lost.
        // The same when a key landed while Sokki read the field (Electron apps take a few ms
        // per read): the text may hold a character the key count doesn't, or the cursor one
        // the text doesn't. Such a read lost the typed run of a word (T3 Code, 26 Nov 2026).
        self.stale = keys() != keys_at_read
            || matches!(&self.last, Some(p) if p.app == app && p.text == text
            && p.cursor == cursor && keys_at_read != p.keys);
        if self.stale {
            return Outcome::Nothing;
        }
        // How much of the text before the cursor was typed key by key: it grows when the field
        // holds the last read's text plus no more characters than keys were pressed, at the
        // cursor. A paste (one key, many characters), dictation (no keys) or a move resets it.
        let prev = self.last.take().filter(|p| p.app == app);
        // an edit at the cursor with no more characters added and deleted than keys were
        // pressed (typing, Backspace, or both between two looks): where it starts
        let edit_from = prev.as_ref().and_then(|p| {
            edit_at_cursor(&p.text, p.cursor, &text, cursor)
                .filter(|&(_, n)| n <= keys_at_read.saturating_sub(p.keys) as usize)
                .map(|(from, _)| from)
        });
        let run = match (&prev, edit_from) {
            // the run keeps its start, or starts where the edit did if that was before it
            (Some(p), Some(from)) => cursor - from.min(p.cursor - p.run),
            // a run starts with one typed character: the last read's text plus that character
            // just before the cursor (after a click elsewhere in the text, or a key the last
            // read missed) ...
            (Some(p), None)
                if cursor > 0
                    && keys_at_read > p.keys
                    && same_text(&text::remove_u16(&text, cursor - 1, 1), &p.text) =>
            {
                1
            }
            // ... or the first character in an empty field (a new message, a sent one cleared).
            // Without this the first word of a message was never fixed.
            _ if cursor == 1 && u16_len(&text) == 1 => 1,
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
        if self.kept.contains(&sokki_typos::fold(&word)) {
            return Outcome::Nothing;
        }
        let prev_refs: Vec<&str> = prevs.iter().map(|s| s.as_str()).collect();
        let Some(fix) = self.typos.correction(&word, &prev_refs, None) else {
            return Outcome::Nothing;
        };
        // the model also answers with the typed word itself (heißt -> heißt: about half of its
        // answers on edited German text); retyping it would only touch the field
        if fix == word {
            return Outcome::Nothing;
        }
        // every letter of the word and the character after it were typed; else leave it, and
        // say so in the fix list
        if run < cursor - start {
            return Outcome::Missed {
                typed: word,
                fix,
                // the run holds the character after the word
                why: if run <= 1 {
                    "word not seen typed (a paste, dictation)"
                } else {
                    "start of the word not seen typed"
                },
            };
        }
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
        // Select the word with the character that ended it and type both: the cursor then
        // ends up after that character by itself, so a quick next key lands in the right place.
        let sel_len = cursor - start;
        let ending: Vec<u16> = text
            .encode_utf16()
            .skip(start + len)
            .take(sel_len - len)
            .collect::<Vec<u16>>();
        let ending = String::from_utf16_lossy(&ending);
        let selected = format!("{typed}{ending}");
        if !field.select(start, sel_len) {
            return Outcome::Skipped("can't select");
        }
        if keys() != keys_at_read {
            // A key came while the word was selected. The app may type it over the word:
            // then put the word back, and the key where the cursor was.
            field.select(cursor, 0);
            let repaired = repair(field, text, start, sel_len, &selected, cursor);
            self.reset();
            return Outcome::Skipped(if repaired {
                "typing on, word put back"
            } else {
                "typing on"
            });
        }
        if !field.insert(&format!("{fix}{ending}")) {
            field.select(cursor, 0);
            return Outcome::Skipped("can't type the fix");
        }
        let after = cursor + u16_len(fix) - len;
        let expected = text::replace_u16(text, start, len, fix);
        // the fix is in: everything up to the cursor as expected, the rest as before (keys the
        // user typed right after the space may already follow the fix)
        let (head, tail) = (
            text::prefix_u16(&expected, after),
            text::suffix_from_u16(text, cursor),
        );
        let (nh, nt) = (head.chars().count(), tail.chars().count());
        let exact = field.settle(&|v| {
            let n = v.chars().count();
            n >= nh + nt
                && same_text(&v.chars().take(nh).collect::<String>(), &head)
                && same_text(&v.chars().skip(n - nt).collect::<String>(), &tail)
        });
        if !exact {
            // The typed fix never arrived (the app wasn't taking keys). Never leave the word
            // selected, and if a key replaced it meanwhile, put the word back.
            field.select(cursor, 0);
            let left = field.value().as_deref() == Some(text) || {
                repair(field, text, start, sel_len, &selected, cursor)
            };
            self.reset();
            return Outcome::Skipped(if left {
                "the fix didn't arrive"
            } else {
                "the fix didn't arrive, field changed"
            });
        }
        self.last = Some(Last {
            app: field.app().to_string(),
            text: field.value().unwrap_or_else(|| expected.clone()),
            cursor: field.selection().map(|s| s.0).unwrap_or(after),
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
        let restored = text::replace_u16(text, d.start, flen, &d.typed);
        // typing over the selection already leaves the cursor after the word
        field.settle(&|v| v == restored);
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
/// Equal, counting a no-break space as a space: rich editors in browsers show a space at the
/// end of a line as U+00A0 and turn it into a plain space once the next character comes.
/// The edit that turned `old` (cursor `old_cursor`) into `new` (cursor `cursor`), if it happened
/// at the cursor: the text after the cursor is unchanged. Returns where the edit starts and how
/// many characters it added and deleted (UTF-16 units, as the cursor).
fn edit_at_cursor(
    old: &str,
    old_cursor: usize,
    new: &str,
    cursor: usize,
) -> Option<(usize, usize)> {
    let a: Vec<u16> = old.encode_utf16().collect();
    let b: Vec<u16> = new.encode_utf16().collect();
    if old_cursor > a.len() || cursor > b.len() {
        return None;
    }
    let same = |x: u16, y: u16| x == y || (is_space(x) && is_space(y));
    let (ta, tb) = (&a[old_cursor..], &b[cursor..]);
    if ta.len() != tb.len() || !ta.iter().zip(tb).all(|(&x, &y)| same(x, y)) {
        return None;
    }
    let from = a[..old_cursor]
        .iter()
        .zip(&b[..cursor])
        .take_while(|(&x, &y)| same(x, y))
        .count();
    Some((from, (old_cursor - from) + (cursor - from)))
}

fn is_space(u: u16) -> bool {
    char::from_u32(u as u32).is_some_and(char::is_whitespace)
}

fn same_text(a: &str, b: &str) -> bool {
    a.chars().count() == b.chars().count()
        && a.chars()
            .zip(b.chars())
            .all(|(x, y)| x == y || (x.is_whitespace() && y.is_whitespace()))
}

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
    let mut want = t[..cursor].to_vec();
    want.extend_from_slice(s);
    want.extend_from_slice(&t[cursor..]);
    let want = String::from_utf16_lossy(&want);
    // each step waits until the app shows the one before, so no key lands in the wrong place
    let ok = field.select(start, s.len())
        && field.insert(typed)
        && field.settle(&|v| v == text)
        && field.select(cursor, 0)
        && field.insert(&keys_typed);
    let exact = ok && field.settle(&|v| v == want);
    log::warn!(
        "autocorrect: a key landed on the word being fixed in {}; put back: {}",
        field.app(),
        ok && exact
    );
    ok && exact
}

//! Replaces the end of what Sokki just pasted into another app's text field.
//!
//! A cleaned-up dictation is pasted at once, with its last part still as transcribed, and that
//! part is swapped for its cleaned version about two seconds later. The swap happens only while
//! the field still ends, at the cursor, with exactly what Sokki pasted: if the person typed on,
//! moved the cursor, sent the text or left the field, the field is left alone.
//!
//! Text is compared with every run of whitespace as one space: rich editors show a pasted
//! blank line as one or two line breaks, and a trailing space as a no-break space.
//! SPDX-License-Identifier: GPL-3.0-only

use crate::text::u16_len;
use crate::Field;

/// What [`replace_tail`] did.
#[derive(Debug, Clone, PartialEq)]
pub enum TailEdit {
    /// The cleaned text is what was pasted; nothing to do.
    Same,
    /// Left as pasted, and why.
    Skipped(&'static str),
    /// The end was replaced; `exact`: the field then held exactly what was meant.
    Replaced { exact: bool, chars: usize },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tok {
    Space,
    Char(char),
}

/// `s` as tokens (a run of whitespace is one), each with where it starts: the char index in
/// `s` and the UTF-16 offset.
fn tokens(s: &str) -> Vec<(Tok, usize, usize)> {
    let mut out: Vec<(Tok, usize, usize)> = Vec::new();
    let mut u = 0;
    for (i, c) in s.chars().enumerate() {
        let t = if c.is_whitespace() {
            Tok::Space
        } else {
            Tok::Char(c)
        };
        if !(t == Tok::Space && out.last().is_some_and(|l| l.0 == Tok::Space)) {
            out.push((t, i, u));
        }
        u += c.len_utf16();
    }
    out
}

/// Equal, with every run of whitespace counted as one space.
pub fn same_text(a: &str, b: &str) -> bool {
    let (x, y) = (tokens(a), tokens(b));
    x.len() == y.len() && x.iter().zip(&y).all(|(p, q)| p.0 == q.0)
}

/// Swaps the end of `pasted` for the end of `wanted` in the focused `field`. `pasted` must be
/// what Sokki pasted, right before the cursor; `keys` counts key presses, `keys_at_paste` is
/// that count right after the paste. `put` pastes text over the selection.
pub fn replace_tail(
    field: &dyn Field,
    pasted: &str,
    wanted: &str,
    keys: &dyn Fn() -> u64,
    keys_at_paste: u64,
    put: &dyn Fn(&str) -> bool,
) -> TailEdit {
    if same_text(pasted, wanted) {
        return TailEdit::Same;
    }
    if keys() != keys_at_paste {
        return TailEdit::Skipped("typed after the paste");
    }
    let (Some(value), Some((cursor, sel))) = (field.value(), field.selection()) else {
        return TailEdit::Skipped("field not readable");
    };
    if sel != 0 {
        return TailEdit::Skipped("text selected");
    }
    let before: String = crate::text::prefix_u16(&value, cursor);
    // Where the pasted text sits in the field: its tokens must be the last ones before the
    // cursor. A leading space in the paste may have joined a space already in the field.
    let (fp, pt) = (tokens(&before), tokens(pasted));
    if pt.is_empty() || fp.len() < pt.len() {
        return TailEdit::Skipped("field changed");
    }
    let at = fp.len() - pt.len();
    if !fp[at..].iter().zip(&pt).all(|(f, p)| f.0 == p.0) {
        return TailEdit::Skipped("field changed");
    }
    // The first token that differs, moved back to the start of its word, so whole words are
    // replaced.
    let common = pasted
        .chars()
        .zip(wanted.chars())
        .take_while(|(a, b)| a == b)
        .count();
    let mut k = pt.iter().rposition(|t| t.1 <= common).unwrap_or(0);
    while k > 0 && pt[k - 1].0 != Tok::Space {
        k -= 1;
    }
    let (start_char, start) = (pt[k].1, fp[at + k].2);
    let new_tail: String = wanted.chars().skip(start_char).collect();
    let len = cursor - start;
    if !field.select(start, len) {
        field.select(cursor, 0);
        return TailEdit::Skipped("can't select");
    }
    if keys() != keys_at_paste {
        field.select(cursor, 0);
        return TailEdit::Skipped("typed after the paste");
    }
    if !put(&new_tail) {
        field.select(cursor, 0);
        return TailEdit::Skipped("can't paste");
    }
    let after: String = crate::text::suffix_from_u16(&value, cursor);
    let head: String = crate::text::prefix_u16(&value, start);
    let expected = format!("{head}{new_tail}{after}");
    let exact = field.settle(&|v| same_text(v, &expected));
    TailEdit::Replaced {
        exact,
        chars: u16_len(&new_tail),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    /// A text field: paste puts text over the selection, the cursor after it.
    struct Fake {
        text: RefCell<String>,
        sel: Cell<(usize, usize)>,
        /// What the app shows instead of the pasted text (a rich editor's line breaks).
        shows: Option<(&'static str, &'static str)>,
    }

    impl Fake {
        fn new(text: &str) -> Fake {
            Fake {
                text: RefCell::new(text.into()),
                sel: Cell::new((u16_len(text), 0)),
                shows: None,
            }
        }
        fn paste(&self, s: &str) -> bool {
            let s = match self.shows {
                Some((a, b)) => s.replace(a, b),
                None => s.to_string(),
            };
            let (start, len) = self.sel.get();
            let t = crate::text::replace_u16(&self.text.borrow(), start, len, &s);
            *self.text.borrow_mut() = t;
            self.sel.set((start + u16_len(&s), 0));
            true
        }
    }

    impl Field for Fake {
        fn app(&self) -> &str {
            "com.example.chat"
        }
        fn value(&self) -> Option<String> {
            Some(self.text.borrow().clone())
        }
        fn selection(&self) -> Option<(usize, usize)> {
            Some(self.sel.get())
        }
        fn replaceable(&self) -> bool {
            true
        }
        fn select(&self, start: usize, len: usize) -> bool {
            self.sel.set((start, len));
            true
        }
        fn insert(&self, text: &str) -> bool {
            self.paste(text)
        }
    }

    const PASTED: &str = "Wir treffen uns um vier.\n\nalso ähm bring bitte den grill mit";
    const WANTED: &str = "Wir treffen uns um vier.\n\nBring bitte den Grill mit.";

    fn run(f: &Fake, pasted: &str, wanted: &str) -> TailEdit {
        replace_tail(f, pasted, wanted, &|| 0, 0, &|s| f.paste(s))
    }

    #[test]
    fn replaces_only_the_part_that_changed() {
        let f = Fake::new("Hallo Tom, ");
        f.paste(PASTED);
        let r = run(&f, PASTED, WANTED);
        assert_eq!(
            r,
            TailEdit::Replaced {
                exact: true,
                chars: 26
            }
        );
        assert_eq!(*f.text.borrow(), format!("Hallo Tom, {WANTED}"));
        assert_eq!(f.sel.get(), (u16_len(&f.text.borrow()), 0));
    }

    #[test]
    fn nothing_to_do_when_the_cleanup_kept_the_text() {
        let f = Fake::new("");
        f.paste(WANTED);
        assert_eq!(run(&f, WANTED, WANTED), TailEdit::Same);
    }

    #[test]
    fn a_rich_editor_showing_one_line_break_for_a_blank_line_is_fine() {
        let f = Fake {
            shows: Some(("\n\n", "\n")),
            ..Fake::new("")
        };
        f.paste(PASTED);
        assert!(matches!(
            run(&f, PASTED, WANTED),
            TailEdit::Replaced { exact: true, .. }
        ));
        assert_eq!(*f.text.borrow(), WANTED.replace("\n\n", "\n"));
    }

    #[test]
    fn leaves_the_field_when_the_person_typed_on() {
        let f = Fake::new("");
        f.paste(PASTED);
        f.paste(" und Kohle");
        assert_eq!(run(&f, PASTED, WANTED), TailEdit::Skipped("field changed"));
        assert!(f.text.borrow().ends_with("grill mit und Kohle"));
    }

    #[test]
    fn leaves_the_field_when_a_key_was_pressed() {
        let f = Fake::new("");
        f.paste(PASTED);
        let r = replace_tail(&f, PASTED, WANTED, &|| 3, 2, &|s| f.paste(s));
        assert_eq!(r, TailEdit::Skipped("typed after the paste"));
        assert_eq!(*f.text.borrow(), PASTED);
    }

    #[test]
    fn leaves_the_field_when_the_cursor_moved_or_text_is_selected() {
        let f = Fake::new("");
        f.paste(PASTED);
        f.select(3, 0);
        assert_eq!(run(&f, PASTED, WANTED), TailEdit::Skipped("field changed"));
        f.select(3, 4);
        assert_eq!(run(&f, PASTED, WANTED), TailEdit::Skipped("text selected"));
        assert_eq!(*f.text.borrow(), PASTED);
    }

    #[test]
    fn leaves_an_emptied_field_alone() {
        // Sent: the chat app emptied the field.
        let f = Fake::new("");
        assert_eq!(run(&f, PASTED, WANTED), TailEdit::Skipped("field changed"));
        assert_eq!(*f.text.borrow(), "");
    }

    #[test]
    fn counts_emoji_and_umlauts_in_utf16() {
        let pasted = "Super 👍 ähm also das passt";
        let wanted = "Super 👍 Das passt.";
        let f = Fake::new("Ö ");
        f.paste(pasted);
        assert!(matches!(
            run(&f, pasted, wanted),
            TailEdit::Replaced { exact: true, .. }
        ));
        assert_eq!(*f.text.borrow(), format!("Ö {wanted}"));
    }

    #[test]
    fn whole_words_are_replaced() {
        // The first difference is the "g" of "grill": the replacement starts at that word,
        // not in the middle of it.
        let f = Fake::new("");
        f.paste("den grill mit");
        let r = run(&f, "den grill mit", "den Grill mit.");
        assert_eq!(
            r,
            TailEdit::Replaced {
                exact: true,
                chars: 10
            }
        );
        assert_eq!(*f.text.borrow(), "den Grill mit.");
    }
}

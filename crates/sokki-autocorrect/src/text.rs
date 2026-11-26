//! Text as macOS Accessibility counts it: offsets in UTF-16 code units.
//! SPDX-License-Identifier: GPL-3.0-only

/// Length of `s` in UTF-16 code units.
pub fn u16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// A text with each character's UTF-16 offset.
pub struct Utf16 {
    chars: Vec<(usize, char)>,
}

impl Utf16 {
    pub fn new(s: &str) -> Utf16 {
        let mut at = 0;
        let chars = s
            .chars()
            .map(|c| {
                let x = (at, c);
                at += c.len_utf16();
                x
            })
            .collect();
        Utf16 { chars }
    }

    /// Index into `chars` of the character starting at UTF-16 offset `at`.
    fn index(&self, at: usize) -> Option<usize> {
        self.chars.binary_search_by_key(&at, |c| c.0).ok()
    }

    /// The character that ends at UTF-16 offset `at` (the one before the cursor).
    pub fn char_before(&self, at: usize) -> Option<char> {
        let i = match self.index(at) {
            Some(i) => i,
            None if at == self.end() => self.chars.len(),
            None => return None,
        };
        i.checked_sub(1).map(|j| self.chars[j].1)
    }

    fn end(&self) -> usize {
        self.chars
            .last()
            .map(|c| c.0 + c.1.len_utf16())
            .unwrap_or(0)
    }
}

/// Characters a fixable word may follow: nothing, a space, an opening bracket or quote.
/// Not after - _ / @ . # digits or letters: hyphenated words, paths, addresses, code.
fn may_precede(c: char) -> bool {
    c.is_whitespace() || "([{\"'„“‚‘«»".contains(c)
}

const SENTENCE_END: &[char] = &['.', '!', '?', '\n', ':', ';'];

/// The word that ends right before UTF-16 offset `end` (where the word-ending character
/// sits): its UTF-16 start, the word, and the words before it in its sentence as the model
/// takes them (at most three, [`sokki_typos::BOS`] where the sentence starts).
pub fn word_before(u: &Utf16, end: usize) -> Option<(usize, String, Vec<String>)> {
    let stop = match u.index(end) {
        Some(i) => i,
        None if end == u.end() => u.chars.len(),
        None => return None,
    };
    let mut i = stop;
    while i > 0 && u.chars[i - 1].1.is_alphabetic() {
        i -= 1;
    }
    if i == stop || (i > 0 && !may_precede(u.chars[i - 1].1)) {
        return None;
    }
    let word: String = u.chars[i..stop].iter().map(|c| c.1).collect();
    // the words before, back to the start of the sentence
    let mut prevs: Vec<String> = Vec::new();
    let mut j = i;
    let mut at_start = false;
    while prevs.len() < 3 {
        // skip what lies between words; a sentence end stops
        while j > 0 && !u.chars[j - 1].1.is_alphabetic() && u.chars[j - 1].1 != '\'' {
            if SENTENCE_END.contains(&u.chars[j - 1].1) {
                at_start = true;
                break;
            }
            j -= 1;
        }
        if at_start || j == 0 {
            at_start = true;
            break;
        }
        let e = j;
        while j > 0 && (u.chars[j - 1].1.is_alphabetic() || u.chars[j - 1].1 == '\'') {
            j -= 1;
        }
        let w: String = u.chars[j..e].iter().map(|c| c.1).collect();
        if !w.contains('\'') {
            prevs.push(w);
        }
    }
    if at_start && prevs.len() < 3 {
        prevs.push(sokki_typos::BOS.to_string());
    }
    prevs.reverse();
    Some((u.chars[i].0, word, prevs))
}

/// The first `n` UTF-16 units of `s`.
pub fn prefix_u16(s: &str, n: usize) -> String {
    String::from_utf16_lossy(&s.encode_utf16().take(n).collect::<Vec<u16>>())
}

/// `s` from UTF-16 offset `at` on.
pub fn suffix_from_u16(s: &str, at: usize) -> String {
    String::from_utf16_lossy(&s.encode_utf16().skip(at).collect::<Vec<u16>>())
}

/// `s` without `n` UTF-16 units at `at`.
pub fn remove_u16(s: &str, at: usize, n: usize) -> String {
    replace_u16(s, at, n, "")
}

/// `s` with the `len` UTF-16 units at `start` replaced by `with`.
pub fn replace_u16(s: &str, start: usize, len: usize, with: &str) -> String {
    let mut out = String::with_capacity(s.len() + with.len());
    let mut at = 0;
    let mut put = false;
    for c in s.chars() {
        if at == start && !put {
            out.push_str(with);
            put = true;
        }
        if at < start || at >= start + len {
            out.push(c);
        }
        at += c.len_utf16();
    }
    if !put && at == start {
        out.push_str(with);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_word_and_its_sentence() {
        let s = "Hallo. Ich habe das nihct ";
        let u = Utf16::new(s);
        let end = u16_len(s) - 1;
        let (start, w, prevs) = word_before(&u, end).unwrap();
        assert_eq!(w, "nihct");
        assert_eq!(start, u16_len("Hallo. Ich habe das "));
        assert_eq!(prevs, vec!["Ich", "habe", "das"]);
        let s2 = "Hallo. Ich nihct ";
        let (_, _, p2) = word_before(&Utf16::new(s2), u16_len(s2) - 1).unwrap();
        assert_eq!(p2, vec![sokki_typos::BOS, "Ich"]);
    }

    #[test]
    fn leaves_paths_addresses_and_hyphens() {
        for s in [
            "see src/maion ",
            "mail me@exmaple ",
            "ein E-Mial ",
            "x_varaible ",
        ] {
            let u = Utf16::new(s);
            assert!(word_before(&u, u16_len(s) - 1).is_none(), "{s}");
        }
    }

    #[test]
    fn counts_emoji_as_two_units() {
        let s = "😀 nihct ";
        let u = Utf16::new(s);
        let (start, w, _) = word_before(&u, u16_len(s) - 1).unwrap();
        assert_eq!((start, w.as_str()), (3, "nihct"));
        assert_eq!(replace_u16(s, 3, 5, "nicht"), "😀 nicht ");
        assert_eq!(remove_u16("ab ", 2, 1), "ab");
    }
}

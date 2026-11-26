//! Which words to learn from the journal, before any model looks at them.
//!
//! Only words the user typed key by key count (`Entry::typed`): not pasted
//! text, not dictations, not what the field already held. Entries that
//! don't say what was typed (older ones, the phone's for now) are skipped.
//!
//! Candidates: words the spell checker doesn't know, with a capital letter
//! (names, products, acronyms), 3+ letters, no digits, not part of a link,
//! mail address, file name or code, typed in 2+ different messages, and not
//! what a dictation put there. The local language model and the hear test
//! decide afterwards (in the app).
//!
//! Corrections: where the user changed a dictated word before sending
//! ("Fassel" → "Vercel"), the pair is learned when the new word is unknown to
//! the spell checker and both sound alike. A change of mind ("Montag" →
//! "Dienstag") is two known words and is never learned.

use std::collections::{BTreeMap, HashSet};

use natural::phonetics::soundex;
use strsim::levenshtein;

use crate::entry::Entry;

/// At most this many candidates go to the model per run.
pub const MAX_CANDIDATES: usize = 20;
/// A word has to appear in this many different messages.
pub const MIN_MESSAGES: usize = 2;

const EDGE: &[char] = &[
    '.', ',', ';', ':', '!', '?', '"', '\'', '(', ')', '[', ']', '{', '}', '«', '»', '„', '“', '”',
    '‚', '‘', '’', '…', '*', '`', '–', '—',
];

/// The word inside a whitespace-separated token, if it can be a name or term.
pub fn clean_token(token: &str) -> Option<&str> {
    // Links, mail, paths, code, handles, numbers: never.
    if token.contains("://")
        || token.contains('@')
        || token.contains('/')
        || token.contains('\\')
        || token.contains('_')
        || token.contains('=')
        || token.contains('<')
        || token.contains('>')
        || token.contains('#')
        || token.chars().any(|c| c.is_ascii_digit())
    {
        return None;
    }
    let word = token.trim_matches(|c: char| EDGE.contains(&c));
    // "file.rs", "Node.js", "www.example": dotted words are not spoken names.
    if word.contains('.') || word.chars().count() < 3 {
        return None;
    }
    if !word
        .chars()
        .all(|c| c.is_alphabetic() || c == '-' || c == '&')
    {
        return None;
    }
    if word.starts_with('-') || word.ends_with('-') {
        return None;
    }
    Some(word)
}

/// A word worth asking about, with one sentence it appeared in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub word: String,
    pub messages: usize,
    pub sentence: String,
}

/// The sentence around `word` in `text`, at most 200 characters.
fn sentence_with(text: &str, word: &str) -> String {
    let at = text.find(word).unwrap_or(0);
    let start = text[..at]
        .rfind(['.', '!', '?', '\n'])
        .map(|i| i + 1)
        .unwrap_or(0);
    let end = text[at..]
        .find(['.', '!', '?', '\n'])
        .map(|i| at + i + 1)
        .unwrap_or(text.len());
    text[start..end].trim().chars().take(200).collect()
}

/// The words of `e` whose every character was typed key by key, as they
/// appear. `None`: the entry doesn't say what was typed.
pub fn typed_words(e: &Entry) -> Option<Vec<&str>> {
    let spans = e.typed.as_ref()?;
    let typed = |i: usize| spans.iter().any(|[a, b]| i >= *a && i < *b);
    // Byte offset of each character, to turn byte positions into characters.
    let starts: Vec<usize> = e.text.char_indices().map(|(i, _)| i).collect();
    let char_at = |byte: usize| starts.partition_point(|&s| s < byte);
    let mut out = Vec::new();
    let mut byte = 0;
    for token in e.text.split_whitespace() {
        let at = byte + e.text[byte..].find(token).unwrap_or(0);
        byte = at + token.len();
        let Some(word) = clean_token(token) else {
            continue;
        };
        let w_at = at + token.find(word).unwrap_or(0);
        let (first, len) = (char_at(w_at), word.chars().count());
        if (first..first + len).all(typed) {
            out.push(word);
        }
    }
    Some(out)
}

fn words_dictated(e: &Entry) -> HashSet<String> {
    e.dictated
        .iter()
        .flat_map(|d| {
            let mut v = vec![d.raw.as_str()];
            if let Some(p) = &d.pasted {
                v.push(p);
            }
            v
        })
        .flat_map(|t| t.split_whitespace())
        .filter_map(clean_token)
        .map(str::to_lowercase)
        .collect()
}

/// Candidates from these entries. `known`: the spell checker knows the word.
/// `skip`: lowercase words already in the vocabulary, learned or rejected.
pub fn candidates(
    entries: &[Entry],
    known: &mut dyn FnMut(&str) -> bool,
    skip: &HashSet<String>,
) -> Vec<Candidate> {
    // lowercase → (spelling, messages, sentence)
    let mut seen: BTreeMap<String, (String, usize, String)> = BTreeMap::new();
    for e in entries {
        let Some(typed) = typed_words(e) else {
            continue;
        };
        let dictated = words_dictated(e);
        let mut in_this: HashSet<String> = HashSet::new();
        for word in typed {
            if !word.chars().any(char::is_uppercase) {
                continue;
            }
            let key = word.to_lowercase();
            if skip.contains(&key) || dictated.contains(&key) || !in_this.insert(key.clone()) {
                continue;
            }
            let slot = seen
                .entry(key)
                .or_insert_with(|| (word.to_string(), 0, sentence_with(&e.text, word)));
            slot.1 += 1;
        }
    }
    let mut out: Vec<Candidate> = seen
        .into_values()
        .filter(|(_, n, _)| *n >= MIN_MESSAGES)
        .filter(|(w, _, _)| !known(w))
        .map(|(word, messages, sentence)| Candidate {
            word,
            messages,
            sentence,
        })
        .collect();
    out.sort_by(|a, b| b.messages.cmp(&a.messages).then(a.word.cmp(&b.word)));
    out.truncate(MAX_CANDIDATES);
    out
}

/// How a word sounds, roughly, in German and English spelling: letters
/// that sound alike become one ("Vercel" and "Fassel" both start "f…s…l").
fn sound_key(s: &str) -> String {
    let lower: Vec<char> = s
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'ä' => 'e',
            'ö' => 'o',
            'ü' => 'u',
            'ß' => 's',
            other => other,
        })
        .collect();
    let mut out: Vec<char> = Vec::with_capacity(lower.len());
    let mut i = 0;
    while i < lower.len() {
        let c = lower[i];
        let next = lower.get(i + 1).copied();
        let mapped = match (c, next) {
            ('p', Some('h')) => {
                i += 1;
                'f'
            }
            ('c', Some('k')) => {
                i += 1;
                'k'
            }
            ('c', Some('e' | 'i' | 'y')) => 's',
            ('c', _) | ('q', _) => 'k',
            ('v', _) => 'f',
            ('w', _) => 'v',
            ('z', _) => 's',
            ('y', _) => 'i',
            ('h', _) if i > 0 => {
                i += 1;
                continue;
            }
            (c, _) => c,
        };
        if out.last() != Some(&mapped) {
            out.push(mapped);
        }
        i += 1;
    }
    out.into_iter().collect()
}

/// Whether a heard form and a word sound alike enough to be a mishearing.
pub fn sounds_alike(heard: &str, word: &str) -> bool {
    let plain = |s: &str| -> String {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    };
    let (h, w) = (plain(heard), plain(word));
    if h.is_empty() || w.is_empty() || h == w {
        return false;
    }
    let ascii = |s: &str| s.chars().all(|c| c.is_ascii_alphabetic());
    if ascii(&h) && ascii(&w) && soundex(&h, &w) {
        return true;
    }
    let (hk, wk) = (sound_key(&h), sound_key(&w));
    let longest = hk.chars().count().max(wk.chars().count()).max(1) as f64;
    (levenshtein(&hk, &wk) as f64) / longest <= 0.4
}

/// Word-level diff: the (old, new) spans that differ, in order.
fn changed_spans<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(Vec<&'a str>, Vec<&'a str>)> {
    let norm = |s: &str| s.trim_matches(|c: char| EDGE.contains(&c)).to_lowercase();
    let (n, m) = (old.len(), new.len());
    // Longest common subsequence table.
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if norm(old[i]) == norm(new[j]) {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut spans = Vec::new();
    let (mut a, mut b): (Vec<&str>, Vec<&str>) = (Vec::new(), Vec::new());
    while i < n || j < m {
        if i < n && j < m && norm(old[i]) == norm(new[j]) {
            if !a.is_empty() || !b.is_empty() {
                spans.push((std::mem::take(&mut a), std::mem::take(&mut b)));
            }
            i += 1;
            j += 1;
        } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            b.push(new[j]);
            j += 1;
        } else {
            a.push(old[i]);
            i += 1;
        }
    }
    if !a.is_empty() || !b.is_empty() {
        spans.push((a, b));
    }
    spans
}

/// A mishearing the user fixed by hand: `heard` was dictated, `word` sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    pub heard: String,
    pub word: String,
}

/// The pairs in these entries' corrected dictations.
pub fn corrections(entries: &[Entry], known: &mut dyn FnMut(&str) -> bool) -> Vec<Pair> {
    let mut out: Vec<Pair> = Vec::new();
    for e in entries {
        // The fixed word has to be typed; unknown entries teach nothing.
        let Some(typed) = typed_words(e) else {
            continue;
        };
        let typed: HashSet<String> = typed.iter().map(|w| w.to_lowercase()).collect();
        for d in &e.dictated {
            let base = d.pasted.as_deref().unwrap_or(&d.raw);
            let old: Vec<&str> = base.split_whitespace().collect();
            let new: Vec<&str> = e.text.split_whitespace().collect();
            for (a, b) in changed_spans(&old, &new) {
                // One or two dictated words became one word, maybe with
                // words added around it ("… Vercel. Danke").
                if a.is_empty() || a.len() > 2 || b.is_empty() || b.len() > 4 {
                    continue;
                }
                // Heard words may be short ("get up"), but plain words only.
                let heard: Vec<&str> = a
                    .iter()
                    .map(|t| t.trim_matches(|c: char| EDGE.contains(&c)))
                    .filter(|t| !t.is_empty() && t.chars().all(char::is_alphabetic))
                    .collect();
                if heard.len() != a.len() {
                    continue;
                }
                let heard = heard.join(" ");
                let Some(word) = b.iter().filter_map(|t| clean_token(t)).find(|w| {
                    typed.contains(&w.to_lowercase()) && !known(w) && sounds_alike(&heard, w)
                }) else {
                    continue;
                };
                let pair = Pair {
                    heard,
                    word: word.to_string(),
                };
                if !out.contains(&pair) {
                    out.push(pair);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::Dictated;

    fn entry(text: &str) -> Entry {
        Entry {
            t: "entry".into(),
            opened: 0,
            written: 0,
            app: "com.example".into(),
            hint: String::new(),
            prefill: String::new(),
            text: text.into(),
            dictated: vec![],
            typed: Some(vec![[0, text.chars().count()]]),
            replaceable: None,
        }
    }

    /// A tiny stand-in for the spell checker.
    fn known(w: &str) -> bool {
        [
            "die",
            "rechnung",
            "an",
            "geht",
            "morgen",
            "raus",
            "hallo",
            "ist",
            "fertig",
            "freitag",
            "montag",
            "dienstag",
            "ok",
            "danke",
            "das",
            "läuft",
            "auf",
            "neue",
            "dashboard",
        ]
        .contains(&w.to_lowercase().as_str())
    }

    #[test]
    fn tokens_that_can_be_names() {
        assert_eq!(clean_token("Brannwick,"), Some("Brannwick"));
        assert_eq!(clean_token("„Tarnisio“"), Some("Tarnisio"));
        assert_eq!(clean_token("Off-Dar"), Some("Off-Dar"));
        for no in [
            "https://x.io",
            "me@x.de",
            "file.rs",
            "useEffect_x",
            "GPT4",
            "DE89370400",
            "a/b",
            "#tag",
            "ok",
            "-x-",
            "sk-proj-4fQ9",
        ] {
            assert_eq!(clean_token(no), None, "{no}");
        }
    }

    #[test]
    fn a_name_typed_twice_is_a_candidate() {
        let entries = [
            entry("Die Rechnung an Brannwick geht morgen raus."),
            entry("Hallo, Brannwick ist fertig. Freiatg passt."),
            entry("Tarnisio ist fertig"),
        ];
        let c = candidates(&entries, &mut known, &HashSet::new());
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!(c[0].word, "Brannwick");
        assert_eq!(c[0].messages, 2);
        assert_eq!(c[0].sentence, "Die Rechnung an Brannwick geht morgen raus.");
    }

    #[test]
    fn known_skipped_lowercase_and_dictated_words_are_not() {
        let mut dictated = entry("Fassel Fassel ok");
        dictated.dictated = vec![Dictated {
            raw: "Fassel".into(),
            pasted: None,
            at: 0,
        }];
        let entries = [
            entry("Kelvaro kelvaro typo typo"),
            entry("Kelvaro typo"),
            dictated.clone(),
            dictated,
        ];
        let skip: HashSet<String> = ["kelvaro".to_string()].into();
        assert!(candidates(&entries, &mut known, &skip).is_empty());
    }

    #[test]
    fn a_fixed_dictation_gives_a_pair() {
        let mut e = entry("Das neue Dashboard läuft auf Vercel. Danke");
        e.dictated = vec![Dictated {
            raw: "Das neue Dashboard läuft auf Fassel.".into(),
            pasted: None,
            at: 0,
        }];
        assert_eq!(
            corrections(&[e], &mut known),
            [Pair {
                heard: "Fassel".into(),
                word: "Vercel".into()
            }]
        );
        // Two heard words that became one.
        let mut e = entry("Der Fix für GitHub ist da");
        e.dictated = vec![Dictated {
            raw: "Der Fix für get up ist da".into(),
            pasted: None,
            at: 0,
        }];
        assert_eq!(corrections(&[e], &mut known)[0].heard, "get up");
    }

    #[test]
    fn only_typed_words_count() {
        // "Kelvaro" pasted twice, "Brannwick" typed twice.
        let mut a = entry("Kelvaro und Brannwick");
        a.typed = Some(vec![[8, 21]]);
        let mut b = a.clone();
        b.text = "Kelvaro, Brannwick".into();
        b.typed = Some(vec![[8, 18]]);
        let c = candidates(&[a.clone(), b], &mut known, &HashSet::new());
        assert_eq!(
            c.iter().map(|c| c.word.as_str()).collect::<Vec<_>>(),
            ["Brannwick"]
        );
        // An entry that doesn't say what was typed teaches nothing.
        a.typed = None;
        assert!(candidates(&[a.clone(), a], &mut known, &HashSet::new()).is_empty());
    }

    #[test]
    fn a_pasted_fix_is_no_pair() {
        let mut e = entry("Das neue Dashboard läuft auf Vercel. Danke");
        e.dictated = vec![Dictated {
            raw: "Das neue Dashboard läuft auf Fassel.".into(),
            pasted: None,
            at: 0,
        }];
        e.typed = Some(vec![]);
        assert!(corrections(&[e], &mut known).is_empty());
    }

    #[test]
    fn a_change_of_mind_is_no_pair() {
        let mut e = entry("bis Dienstag");
        e.dictated = vec![Dictated {
            raw: "bis Montag".into(),
            pasted: None,
            at: 0,
        }];
        assert!(corrections(&[e.clone()], &mut known).is_empty());
        // A new name that sounds nothing like what was dictated.
        e.text = "bis Quorbit".into();
        assert!(corrections(&[e], &mut known).is_empty());
    }

    #[test]
    fn sound_alike() {
        assert!(sounds_alike("Fassel", "Vercel"));
        assert!(sounds_alike("Brandwick", "Brannwick"));
        assert!(sounds_alike("get up", "GitHub"));
        assert!(sounds_alike("Off-Dar", "OVDA"));
        assert!(!sounds_alike("Montag", "Quorbit"));
        assert!(!sounds_alike("Montag", "Dienstag"));
        assert_eq!(sound_key("Vercel"), "fersel");
        assert_eq!(sound_key("Fassel"), "fasel");
        assert!(!sounds_alike("Vercel", "vercel"));
    }
}

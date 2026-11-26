//! Sokki's typo fixer for the Mac: the phone keyboard's autocorrect (Typos.kt), word for word.
//!
//! It fixes typos and never swaps a real word: every word one slip from the typed one
//! competes with the typed word on log10 P(word | previous word) + log10 P(typed | word),
//! and the best one replaces it only when it holds 95 % of the probability. Words the
//! dictionaries know are never changed, only capitalised (German nouns). Measured on
//! Robin's own Mac-typed writing (47k words, 375 real typos): 75.5 % fixed, 0.5 % fixed
//! to a wrong word, 21 correct words changed.
//!
//! This is a port of `mobile/keyboard/tools/typo_model.py` with the switches the phone
//! runs; `tools/parity.py` checks the two give the same answer on every word. On the
//! phone the keyboard's dictionaries say whether the typed word is a word and how it is
//! spelled; here [`Lexicon`] stands in for them, built from the same word lists.
//! SPDX-License-Identifier: GPL-3.0-only

mod table;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use table::MAX_BUCKET;
pub use table::{Bloom, Table};

/// The start of a sentence, as a previous word.
pub const BOS: &str = "<s>";

const COMMON: f64 = 3.0; // zipf from which a word is known (never changed)
const MIN_LEN: usize = 3;
const OOV: f64 = -10.0; // log10 P of a typed non-word
const TAU: f64 = 0.95; // share the best word needs
const CAND_ZIPF: f64 = 2.0; // candidates are at least this common
const FIRST: f64 = -0.3; // slips on the first letter are 2-3 times rarer
const UMLAUT: f64 = -0.2; // log10 P of the plain vowel for an umlaut (a missed long-press)
const SWITCH: f64 = -1.0; // log10 P(an English-only word in a German sentence)
const EN_IN_GERMAN_GAP: f64 = 1.0;
const COMPOUND_MIN: usize = 9;
const OOV_PER_LETTER: f64 = 0.25;
const LETTERS: &str = "abcdefghijklmnopqrstuvwxyzäöü";
const QWERTZ: [&str; 3] = ["qwertzuiopü", "asdfghjklöä", "yxcvbnm"];
const DENGLISH_ENDINGS: [&str; 5] = ["en", "st", "et", "te", "ten"];
const GERMAN_PREFIXES: [&str; 25] = [
    "un", "ab", "an", "auf", "aus", "be", "ein", "ent", "er", "ge", "mit", "nach", "um", "ver",
    "vor", "weg", "zer", "zu", "rein", "raus", "rum", "ran", "runter", "hin", "her",
];
const GERMAN_ENDINGS: [&str; 18] = [
    "ere", "eres", "erem", "eren", "erer", "est", "end", "ens", "em", "es", "er", "en", "e", "st",
    "t", "te", "ten", "s",
];
/// British -ise endings and their US -ize forms.
const BRITISH: [(&str, &str); 10] = [
    ("isations", "izations"),
    ("isation", "ization"),
    ("ising", "izing"),
    ("isers", "izers"),
    ("ised", "ized"),
    ("ises", "izes"),
    ("iser", "izer"),
    ("ise", "ize"),
    ("ysed", "yzed"),
    ("yse", "yze"),
];
const PRONOUNS: [&str; 9] = ["ich", "du", "er", "sie", "es", "wir", "ihr", "man", "sich"];
const DETERMINERS: &str = "der die das den dem des ein eine einen einem einer eines kein keine keinen keinem keiner \
mein meine meinen meinem meiner dein deine deinen deinem deiner sein seine seinen seinem seiner ihre ihren \
ihrem ihrer unser unsere unseren unserem unserer euer eure euren eurem diese dieser dieses diesen diesem \
jede jeder jedes jeden jedem alle allen viele vielen mehr mehrere mehreren welche welcher welches welchen \
zum zur im am beim vom ins ans aufs für mit von zu bei nach aus über unter auf in an um durch ohne gegen \
pro per via vor hinter neben zwischen ne nen nem ner";

/// How much a word counts for the sentence's language when the phone's table has it as common in
/// both languages but only one dictionary has it (the, you, have: English only). The table counts
/// most common English words as German too, so an English sentence never leaned English and its
/// nouns took German capitals (Robin, 26.11.2026: "option" became "Option"). Measured on Robin's
/// typing (47,494 words): 108 capitals better, 16 worse at 0.25; at 1.0: 113 better, 31 worse.
/// Also used for `english-only.txt` (and, is, these), which the legacy German list took in
/// (Robin, 26.11.2026: "in these cases" became "in These cases"); higher weights there only lost
/// more German nouns (docs/research/mac-autocorrect-2026-11.md).
const DICT_LEAN: f64 = 0.25;

/// Off until Robin picks it (26.11.2026): compound repair leaves a part alone that is itself two
/// known words (gas|turbinen), instead of changing a correct compound by a letter
/// (a correct compound changed by a letter). On 10,000 edited German sentences, on top of the
/// Bloom filter cross-check in `compound_fix`: 385 wrong word changes -> 355. On Robin's 349
/// typos: 1 fewer fixed (landignpages), 1 fixed differently
/// (docs/research/mac-autocorrect-2026-11.md).
const COMPOUND_PART_CHECK: bool = false;

fn ln10(x: f64) -> f64 {
    x.log10()
}

struct Slips {
    near: f64,
    far: f64,
    missing: f64,
    double: f64,
    extra_near: f64,
    extra_far: f64,
    swap: f64, // includes the 5x bonus: half of Robin's typos are two swapped letters
    matched: f64,
    backoff: f64,
    runner_up: f64,
}

fn slips() -> Slips {
    Slips {
        near: ln10(0.00308 / 4.2),
        far: ln10(0.00118 / 21.0),
        missing: ln10(0.00171),
        double: ln10(0.00017),
        extra_near: ln10(0.00032 / 8.4),
        extra_far: ln10(0.00068 / 24.0),
        swap: ln10(0.00033) + ln10(5.0),
        matched: ln10(1.0 - 0.0075),
        backoff: ln10(0.4),
        runner_up: ln10(5.0),
    }
}

/// Lower case, ß -> ss, as wordfreq stores words.
pub fn fold(w: &str) -> String {
    w.to_lowercase().replace('ß', "ss")
}

fn upper_first(w: &str) -> String {
    let mut c = w.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

fn lower_first(w: &str) -> String {
    let mut c = w.chars();
    match c.next() {
        Some(f) => f.to_lowercase().chain(c).collect(),
        None => String::new(),
    }
}

fn first_upper(w: &str) -> bool {
    w.chars().next().is_some_and(|c| c.is_uppercase())
}

fn first_lower(w: &str) -> bool {
    w.chars().next().is_some_and(|c| c.is_lowercase())
}

/// Python's str.islower(): some cased letter, none upper case.
fn is_lower(w: &str) -> bool {
    w.chars().any(|c| c.is_lowercase()) && !w.chars().any(|c| c.is_uppercase())
}

fn as_typed(typed: &str, w: &str) -> String {
    if first_upper(typed) {
        upper_first(w)
    } else {
        w.to_string()
    }
}

fn base(c: char) -> Option<char> {
    match c {
        'ä' => Some('a'),
        'ö' => Some('o'),
        'ü' => Some('u'),
        _ => None,
    }
}

fn umlaut_of(c: char) -> char {
    match c {
        'a' => 'ä',
        'o' => 'ö',
        _ => 'ü',
    }
}

/// Where each letter key sits: row, and column with each row shifted half a key.
#[derive(Clone)]
struct Keyboard {
    pos: HashMap<char, (i32, f64)>,
    /// The layout is QWERTZ: y and z count as neighbours too (the QWERTY habit).
    qwertz: bool,
}

impl Keyboard {
    fn new<S: AsRef<str>>(rows: &[S]) -> Keyboard {
        let mut pos = HashMap::new();
        for (r, row) in rows.iter().enumerate() {
            for (i, c) in row.as_ref().chars().enumerate() {
                if c.is_alphabetic() {
                    pos.entry(c)
                        .or_insert((r as i32, i as f64 + 0.5 * r as f64));
                }
            }
        }
        let qwertz =
            pos.get(&'z').is_some_and(|p| p.0 == 0) && pos.get(&'y').is_some_and(|p| p.0 == 2);
        Keyboard { pos, qwertz }
    }

    fn neighbours(&self, a: char, b: char) -> bool {
        if self.qwertz && ((a == 'y' && b == 'z') || (a == 'z' && b == 'y')) {
            return true; // QWERTY habit on QWERTZ
        }
        match (self.pos.get(&a), self.pos.get(&b)) {
            (Some((r1, c1)), Some((r2, c2))) => (r1 - r2).abs() <= 1 && (c1 - c2).abs() <= 1.0,
            _ => false,
        }
    }
}

/// Every string one slip from folded `t`: a letter extra, missing, changed, or two swapped. Sorted.
pub fn one_slip(t: &str) -> Vec<String> {
    let c: Vec<char> = t.chars().collect();
    let s = |a: &[char]| a.iter().collect::<String>();
    let mut out = HashSet::new();
    for i in 0..=c.len() {
        for ch in LETTERS.chars() {
            let mut w = c.clone();
            w.insert(i, ch);
            out.insert(s(&w));
        }
    }
    for i in 0..c.len() {
        let mut w = c.clone();
        w.remove(i);
        out.insert(s(&w));
        if i + 1 < c.len() {
            let mut w = c.clone();
            w.swap(i, i + 1);
            out.insert(s(&w));
        }
        for ch in LETTERS.chars() {
            let mut w = c.clone();
            w[i] = ch;
            out.insert(s(&w));
        }
    }
    out.remove(t);
    let mut v: Vec<String> = out.into_iter().collect();
    v.sort();
    v
}

/// Python's `str[a:b]` on chars.
fn sub(c: &[char], a: usize, b: usize) -> String {
    c[a.min(c.len())..b.min(c.len())].iter().collect()
}

fn chars(w: &str) -> Vec<char> {
    w.chars().collect()
}

fn strip_end<'a>(w: &'a str, end: &str) -> &'a str {
    &w[..w.len() - end.len()]
}

/// What stands in for the phone keyboard's dictionaries: which language knows a word as
/// typed, and how the dictionary spells it.
#[derive(Default)]
pub struct Lexicon {
    /// "folded\tspelling\tlanguage" lines, sorted by (folded, spelling, language): one buffer,
    /// so 366k words take ~10 MB instead of a map's ~120 MB
    buf: String,
    /// where each line starts in `buf`
    lines: Vec<u32>,
    /// words the user typed before (the keyboard's history), exact
    personal: HashSet<String>,
    personal_by_fold: HashMap<String, Vec<String>>,
    /// lower-case English words the legacy German list took in (and, is, these): only for the
    /// sentence's language (`english-only.txt`, see tools/gen_lexicon.py)
    english_only: HashSet<String>,
}

impl Lexicon {
    /// Word lists, one word per line, as spelled by the dictionary.
    pub fn from_lists(de: &str, en: &str) -> Lexicon {
        let mut all: Vec<(String, &str, char)> = Vec::new();
        for (text, lang) in [(de, 'd'), (en, 'e')] {
            for w in text.lines().filter(|l| !l.is_empty() && !l.contains('\t')) {
                all.push((fold(w), w, lang));
            }
        }
        all.sort();
        all.dedup();
        let mut buf = String::with_capacity(all.iter().map(|x| x.0.len() + x.1.len() + 4).sum());
        let mut lines = Vec::with_capacity(all.len());
        for (f, w, l) in all {
            lines.push(buf.len() as u32);
            buf.push_str(&f);
            buf.push('\t');
            buf.push_str(w);
            buf.push('\t');
            buf.push(l);
            buf.push('\n');
        }
        Lexicon {
            buf,
            lines,
            ..Default::default()
        }
    }

    fn line(&self, i: usize) -> (&str, &str, &'static str) {
        let start = self.lines[i] as usize;
        let rest = &self.buf[start..];
        let end = rest.find('\n').unwrap_or(rest.len());
        let mut parts = rest[..end].splitn(3, '\t');
        let f = parts.next().unwrap_or("");
        let w = parts.next().unwrap_or("");
        let l = if parts.next() == Some("d") {
            "de"
        } else {
            "en"
        };
        (f, w, l)
    }

    /// The dictionary spellings of a folded word with their language, sorted.
    fn forms(&self, folded: &str) -> Vec<(String, &'static str)> {
        let first = self.lines.partition_point(|&s| {
            let rest = &self.buf[s as usize..];
            &rest[..rest.find('\t').unwrap_or(0)] < folded
        });
        let mut out = Vec::new();
        for i in first..self.lines.len() {
            let (f, w, l) = self.line(i);
            if f != folded {
                break;
            }
            out.push((w.to_string(), l));
        }
        out
    }

    /// Folded words with the same first letter and at most `d` letters more or fewer.
    fn near_words(&self, folded: &str, d: usize) -> Vec<String> {
        let first: String = folded.chars().take(1).collect();
        let n = folded.chars().count();
        let lo = self
            .lines
            .partition_point(|&s| &self.buf[s as usize..] < first.as_str());
        let mut out: Vec<String> = Vec::new();
        for i in lo..self.lines.len() {
            let (f, _, _) = self.line(i);
            if !f.starts_with(first.as_str()) {
                break;
            }
            let m = f.chars().count();
            if m + d >= n && m <= n + d && out.last().is_none_or(|l| l != f) {
                out.push(f.to_string());
            }
        }
        out
    }

    pub fn load(dir: &Path) -> std::io::Result<Lexicon> {
        let mut de = std::fs::read_to_string(dir.join("lexicon-de.txt"))?;
        // words the phone's German dictionary lacks (`extra-de.txt`, by hand)
        if let Ok(extra) = std::fs::read_to_string(dir.join("extra-de.txt")) {
            de.push('\n');
            de.push_str(&extra);
        }
        let en = std::fs::read_to_string(dir.join("lexicon-en.txt"))?;
        let mut lex = Lexicon::from_lists(&de, &en);
        if let Ok(eo) = std::fs::read_to_string(dir.join("english-only.txt")) {
            lex.english_only = eo.lines().map(str::to_string).collect();
        }
        Ok(lex)
    }

    /// The user's own words (typed before): they are never changed.
    pub fn set_personal<I: IntoIterator<Item = String>>(&mut self, words: I) {
        self.personal = words.into_iter().collect();
        self.personal_by_fold.clear();
        for w in &self.personal {
            self.personal_by_fold
                .entry(fold(w))
                .or_default()
                .push(w.clone());
        }
        for v in self.personal_by_fold.values_mut() {
            v.sort();
        }
    }

    /// The languages whose dictionaries know the typed word as is ("user": the history).
    fn langs(&self, typed: &str) -> HashSet<&'static str> {
        let low = lower_first(typed);
        let up = first_upper(typed);
        let mut out = HashSet::new();
        for (f, lang) in self.forms(&fold(typed)) {
            if f == typed || (up && f == low) {
                out.insert(lang);
            }
        }
        if self.personal.contains(typed) || (up && self.personal.contains(&low)) {
            out.insert("user");
        }
        if out.is_empty() && self.british(&fold(typed)) {
            out.insert("en");
        }
        out
    }

    /// A British -ise spelling of a word the US list spells -ize (optimise, organised,
    /// prioritising). The English list is the US one; with QWERTY keys s and z are neighbours, so
    /// the -ise form read as a typo of the -ize one (Robin's typing: 6 words).
    fn british(&self, t: &str) -> bool {
        for (gb, us) in BRITISH {
            if let Some(stem) = t.strip_suffix(gb) {
                if stem.chars().count() >= 3
                    && self
                        .forms(&format!("{stem}{us}"))
                        .iter()
                        .any(|(_, l)| *l == "en")
                {
                    return true;
                }
            }
        }
        false
    }
}

/// The model: word table, pair table and the dictionaries.
pub struct Typos {
    table: Table,
    pairs: Bloom,
    pub lexicon: Lexicon,
    s: Slips,
    determiners: HashSet<&'static str>,
    keys: Keyboard,
}

/// One fix in a text: the typed word at char offset `start`, and what replaces it.
#[derive(Debug, Clone, PartialEq)]
pub struct Fix {
    pub start: usize,
    pub typed: String,
    pub fix: String,
}

impl Typos {
    pub fn new(
        words_bin: Vec<u8>,
        bigrams_bin: Vec<u8>,
        lexicon: Lexicon,
    ) -> Result<Typos, String> {
        Ok(Typos {
            table: Table(Bloom::load(words_bin)?),
            pairs: Bloom::load(bigrams_bin)?,
            lexicon,
            s: slips(),
            determiners: DETERMINERS.split(' ').collect(),
            keys: Keyboard::new(&QWERTZ),
        })
    }

    /// The letter keys of the keyboard in use, row by row from the top (the Mac's layout);
    /// which keys are neighbours decides how likely a slip is. The phone's QWERTZ until set.
    pub fn set_keyboard<S: AsRef<str>>(&mut self, rows: &[S]) {
        self.keys = Keyboard::new(rows);
    }

    /// words.bin and bigrams.bin from `tables`, the word lists from `lexicon`.
    pub fn load(tables: &Path, lexicon: &Path) -> Result<Typos, String> {
        let read = |p: &Path| std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()));
        let lex = Lexicon::load(lexicon).map_err(|e| format!("{}: {e}", lexicon.display()))?;
        Typos::new(
            read(&tables.join("words.bin"))?,
            read(&tables.join("bigrams.bin"))?,
            lex,
        )
    }

    /// Every fix the keyboard would make in `text`, word by word as typed: each word with the
    /// words before it in its sentence (a sentence ends at . ! ? : ; or a line break).
    pub fn fixes(&self, text: &str) -> Vec<Fix> {
        let mut out = Vec::new();
        let mut words: Vec<(usize, String)> = Vec::new();
        let mut cur = String::new();
        let mut start = 0;
        // a word with an apostrophe (wasn't, geht's) is left out, as the keyboard sees it whole
        let flush = |words: &mut Vec<(usize, String)>, cur: &mut String, start: usize| {
            let w = std::mem::take(cur);
            if !w.is_empty() && !w.contains('\'') {
                words.push((start, w));
            }
        };
        let mut sentence: Vec<Vec<(usize, String)>> = Vec::new();
        for (i, ch) in text.chars().enumerate() {
            if ch.is_alphabetic() || ch == '\'' && !cur.is_empty() {
                if cur.is_empty() {
                    start = i;
                }
                cur.push(ch);
            } else {
                flush(&mut words, &mut cur, start);
                if ".!?\n:;".contains(ch) {
                    sentence.push(std::mem::take(&mut words));
                }
            }
        }
        flush(&mut words, &mut cur, start);
        sentence.push(words);
        for ws in sentence {
            let names: Vec<&str> = ws.iter().map(|(_, w)| w.as_str()).collect();
            for (i, (at, w)) in ws.iter().enumerate() {
                let mut prevs: Vec<&str> = vec![BOS];
                prevs.extend_from_slice(&names[..i]);
                let prevs = &prevs[prevs.len().saturating_sub(3)..];
                if let Some(fix) = self.correction(w, prevs, None) {
                    out.push(Fix {
                        start: *at,
                        typed: w.clone(),
                        fix,
                    });
                }
            }
        }
        out
    }

    /// The word that replaces `typed` when it is committed, or None to leave it. `prevs`: the
    /// sentence's words before it, nearest last, [`BOS`] where the sentence starts. `next`:
    /// the word after, if known.
    pub fn correction(&self, typed: &str, prevs: &[&str], next: Option<&str>) -> Option<String> {
        let tc: Vec<char> = typed.chars().collect();
        if tc.len() < MIN_LEN
            || !tc.iter().all(|c| c.is_alphabetic())
            || tc[1..].iter().any(|c| c.is_uppercase())
        {
            return None;
        }
        let table = &self.table;
        let t = fold(typed);
        let mut valid = self.lexicon.langs(typed);
        let last = prevs.last().copied();
        let at_start = |l: Option<&str>| l.is_none() || l == Some(BOS);
        let slip_set: HashSet<String> = one_slip(&t).into_iter().collect();
        // the keyboard's suggestions: the dictionary spelling of the typed word and of every word
        // one slip away, and the user's own words one slip away; first with this folded form
        let spelled = |x: &str| -> Option<String> {
            if x == t {
                self.lexicon.forms(x).first().map(|f| f.0.clone())
            } else if slip_set.contains(x) {
                self.lexicon
                    .forms(x)
                    .first()
                    .map(|f| f.0.clone())
                    .or_else(|| {
                        self.lexicon
                            .personal_by_fold
                            .get(x)
                            .and_then(|v| v.first().cloned())
                    })
            } else {
                None
            }
        };
        let own = self.lexicon.forms(&t);
        let t0 = tc[0];
        // The same word spelled as the dictionary has it: a capital (montag -> Montag) or ß. A
        // capital only from German (allen stays, not "Allen" from an English name list).
        let mut cap: Option<(String, &'static str)> = own
            .iter()
            .find(|(c, lang)| {
                let cf = c.chars().next();
                c != typed
                    && !c.chars().skip(1).any(|x| x.is_uppercase())
                    && (*lang == "de" || cf == Some(t0))
                    && (cf == Some(t0)
                        || (t0.is_lowercase() && cf.is_some_and(|x| x.is_uppercase())))
            })
            .map(|(c, l)| (c.clone(), *l));
        // a capitalised word gets the dictionary's spelling too: Heisst -> Heißt
        if cap.is_none() && t0.is_uppercase() {
            cap = own
                .iter()
                .find(|(c, lang)| first_lower(c) && upper_first(c) != typed && *lang == "de")
                .map(|(c, l)| (upper_first(c), *l));
        }
        // ae/oe/ue for ä/ö/ü (DIN 5007) when the typed word is neither German nor English
        if valid.iter().all(|v| *v == "user") {
            let c = chars(&t);
            let mut spots = Vec::new();
            let mut i = 0;
            while i + 1 < c.len() {
                if matches!(c[i], 'a' | 'o' | 'u') && c[i + 1] == 'e' {
                    spots.push(i);
                    i += 2;
                } else {
                    i += 1;
                }
            }
            // ue may be a swapped eu, the most common slip: huete may be heute
            if !spots.is_empty()
                && !spots.iter().any(|&i| {
                    table.common_in(
                        'd',
                        &format!("{}e{}{}", sub(&c, 0, i), c[i], sub(&c, i + 2, c.len())),
                    )
                })
            {
                let use_ = &spots[..spots.len().min(4)];
                let mut best: Option<(f64, String)> = None;
                for mask in 1u32..(1 << use_.len()) {
                    let mut w = c.clone();
                    for k in (0..use_.len()).rev() {
                        if mask >> k & 1 == 1 {
                            let i = use_[k];
                            let u = umlaut_of(w[i]);
                            w.splice(i..i + 2, [u]);
                        }
                    }
                    let w: String = w.into_iter().collect();
                    let score = if table.common_in('d', &w) {
                        Some(table.zipf(&w))
                    } else if !table.common_in('e', &w) && self.known(&w) {
                        Some(0.0)
                    } else {
                        None
                    };
                    if let Some(sc) = score {
                        if best.as_ref().is_none_or(|b| sc > b.0) {
                            best = Some((sc, w));
                        }
                    }
                }
                if let Some((_, b)) = best {
                    return Some(as_typed(typed, &spelled(&b).unwrap_or(b.clone())));
                }
            }
        }
        // ss right after ei/au/eu/äu/ie is ß when the German dictionary spells it so
        if t.contains("ss") && (is_lower(typed) || at_start(last)) {
            if let Some((c, _)) = own.iter().find(|(c, lang)| {
                *lang == "de" && c.contains('ß') && {
                    let l = c.to_lowercase();
                    ["eiß", "aiß", "auß", "euß", "äuß", "ieß"]
                        .iter()
                        .any(|d| l.contains(d))
                }
            }) {
                return Some(as_typed(typed, c));
            }
        }
        if valid.len() == 1
            && valid.contains("user")
            && is_lower(typed)
            && cap
                .as_ref()
                .is_some_and(|(c, l)| first_upper(c) && *l == "de")
        {
            valid.clear(); // the history learned a noun typed in lower case
        }
        let cap_word = cap.as_ref().map(|c| c.0.clone());
        // "an" right before a vowel is the English article: German puts one in between ("an der
        // Option"). Robin, 26.11.2026: "pick an option" became "pick an Option".
        let english_an = last.is_some_and(|l| fold(l) == "an")
            && t.starts_with(['a', 'e', 'i', 'o', 'u'])
            && table.common_in('e', &t);
        let sentence = || self.sentence_lang(prevs, next);
        let truecase = || -> Option<String> {
            if let Some(l) = last {
                if is_lower(typed)
                    && l != BOS
                    && ["zum", "beim", "vom"].contains(&fold(l).as_str())
                    && self.infinitive(&t)
                {
                    return Some(upper_first(typed));
                }
            }
            let l = last?;
            if !(is_lower(typed) && l != BOS && table.noun(&t)) {
                return None;
            }
            let fl = fold(l);
            if PRONOUNS.contains(&fl.as_str()) || sentence() != "de" {
                return None;
            }
            if english_an {
                return None;
            }
            if table.common_in('e', &t)
                && (!(self.determiners.contains(fl.as_str()) || self.adjective(l))
                    || self.lean(next) == Some('e'))
            {
                return None;
            }
            if self.umlaut_twin(&t, table.zipf(&t)) {
                return None;
            }
            Some(match &cap_word {
                Some(c) if first_upper(c) => c.clone(),
                _ => upper_first(typed),
            })
        };
        // A word only English (or the history) has, whose umlaut twin is 100x more common German,
        // is that twin in a German sentence: fur -> für
        if !valid.is_empty()
            && valid.iter().all(|v| *v == "en" || *v == "user")
            && sentence() == "de"
            && self.lean(Some(typed)) != Some('d')
        {
            if let Some(twin) = self.umlaut_form(&t) {
                if table.zipf(&twin) >= table.zipf(&t) + 2.0 {
                    return Some(as_typed(typed, &spelled(&twin).unwrap_or(twin.clone())));
                }
            }
        }
        // In a German sentence an English-only word one slip from a German word competes with it
        if valid.len() == 1
            && valid.contains("en")
            && self.lean(Some(typed)) == Some('e')
            && !at_start(last)
            && sentence() == "de"
        {
            let p0 = fold(last.unwrap());
            let zt = table.zipf(&t);
            let mut scored = vec![(
                self.lm(&t, zt, Some(&p0)) + t.chars().count() as f64 * self.s.matched + SWITCH,
                t.clone(),
            )];
            for c in &slip_set_sorted(&slip_set) {
                if table.common_in('d', c)
                    && !table.common_in('e', c)
                    && table.zipf(c) >= zt + EN_IN_GERMAN_GAP
                {
                    scored.push((
                        self.lm(c, table.zipf(c), Some(&p0)) + self.channel(&t, c),
                        c.clone(),
                    ));
                }
            }
            let (top, word) = best_of(&scored);
            if word != t
                && 1.0 / scored.iter().map(|(s, _)| 10f64.powf(s - top)).sum::<f64>() >= TAU
            {
                return Some(as_typed(typed, &spelled(&word).unwrap_or(word.clone())));
            }
        }
        if !valid.is_empty() {
            // Words the keyboard knows are never changed, only capitalised.
            if let Some(tc) = truecase() {
                return Some(tc);
            }
            if let Some((c, _)) = &cap {
                if valid.len() == 1
                    && valid.contains("en")
                    && !c.starts_with(t0)
                    && sentence() == "de"
                    && !at_start(last)
                    && self.determiners.contains(fold(last.unwrap()).as_str())
                    && !english_an
                    && self.lean(next) != Some('e')
                {
                    return Some(c.clone());
                }
            }
            return None;
        }
        if cap.is_none() && self.known(typed) {
            return truecase();
        }
        let p: Option<String> = last.map(|l| if l == BOS { BOS.to_string() } else { fold(l) });
        let zt = table.zipf(&t);
        let tn = t.chars().count();
        let oov = OOV - OOV_PER_LETTER * (tn.saturating_sub(4)) as f64;
        let lit = if zt == 0.0 || cap.is_none() {
            oov
        } else {
            self.lm(&t, zt, p.as_deref())
        };
        let mut scored = vec![(lit + tn as f64 * self.s.matched, t.clone())];
        // A capital in mid-sentence: a name or a noun; its first letter stays (Qwen is not "wen").
        let keep_first = t0.is_uppercase() && p.as_deref() != Some(BOS);
        let tf = t.chars().next().unwrap();
        // a non-word missing two or more umlauts: Geschaftsfuhrer -> Geschäftsführer
        if let Some(u) = self.umlaut_form(&t) {
            if t.chars().zip(u.chars()).filter(|(a, b)| a != b).count() >= 2 && table.candidate(&u)
            {
                scored.push((
                    self.lm(&u, table.zipf(&u), p.as_deref()) + self.channel(&t, &u),
                    u,
                ));
            }
        }
        for c in slip_set_sorted(&slip_set) {
            let cf = c.chars().next().unwrap();
            if (keep_first && base(cf).unwrap_or(cf) != tf) || !table.candidate(&c) {
                continue;
            }
            let zc = table.zipf(&c);
            if zc >= CAND_ZIPF {
                let s = self.lm(&c, zc, p.as_deref()) + self.channel(&t, &c);
                scored.push((s, c));
            }
        }
        // No word one slip away: the dictionary's common words two slips away compete
        // (funtkionirt -> funktioniert). Robin's Mac typing, 26.11.2026: long words with a swap and a
        // second slip were left as typed.
        let mut two_slips = HashSet::new();
        if scored.len() == 1 {
            let bag = letter_bag(&t);
            for c in self.lexicon.near_words(&t, 2) {
                if bag_distance(&bag, &c) > 2 {
                    continue; // more than two slips apart
                }
                let cf = c.chars().next().unwrap();
                if (keep_first && base(cf).unwrap_or(cf) != tf)
                    || !table.candidate(&c)
                    || slip_set.contains(&c)
                {
                    continue;
                }
                let zc = table.zipf(&c);
                if zc >= COMMON {
                    let s = self.lm(&c, zc, p.as_deref()) + self.channel(&t, &c);
                    two_slips.insert(c.clone());
                    scored.push((s, c));
                }
            }
        }
        let (top, word) = best_of(&scored);
        let total: f64 = scored.iter().map(|(s, _)| 10f64.powf(s - top)).sum();
        let sure = 1.0 / total >= TAU;
        if word != t && sure {
            let spelling = if two_slips.contains(&word) {
                // the spelling of the sentence's language: million in English, Million in German
                let forms = self.lexicon.forms(&word);
                let lang = sentence();
                forms
                    .iter()
                    .find(|f| f.1 == lang)
                    .or(forms.first())
                    .map(|f| f.0.clone())
            } else {
                spelled(&word)
            };
            return Some(as_typed(typed, &spelling.unwrap_or(word.clone())));
        }
        if word == t && zt == 0.0 && tn >= COMPOUND_MIN && sentence() != "en" {
            if let Some(fix) = self.compound_fix(&t) {
                return Some(as_typed(typed, &fix));
            }
        }
        if let Some(tc) = truecase() {
            return Some(tc);
        }
        let c = cap_word?;
        if !c.starts_with(t0)
            && (sentence() == "en"
                || (!at_start(last) && PRONOUNS.contains(&fold(last.unwrap()).as_str())))
        {
            return None;
        }
        Some(c)
    }

    /// log10 P(w | prev): the pair's bucket middle, else 0.4 x P(w); P(w) from zipf `zw`.
    fn lm(&self, w: &str, zw: f64, prev: Option<&str>) -> f64 {
        match prev {
            Some(p) if !p.is_empty() => {
                for b in (1..=MAX_BUCKET).rev() {
                    if self.pairs.contains(&format!("{b}:{p} {w}")) {
                        return b as f64 / 2.0 - 8.0 + 0.25;
                    }
                }
                self.s.backoff + zw - 9.0
            }
            _ => zw - 9.0,
        }
    }

    /// log10 P(typed | word), both folded: the likeliest slips (weighted Damerau-Levenshtein).
    fn channel(&self, typed: &str, word: &str) -> f64 {
        let w: Vec<char> = word.chars().collect();
        let ty: Vec<char> = typed.chars().collect();
        let (n, m) = (w.len(), ty.len());
        let neg = -1e9;
        let mut d = vec![vec![neg; m + 1]; n + 1];
        d[0][0] = 0.0;
        let s = &self.s;
        for i in 0..=n {
            for j in 0..=m {
                if i == 0 && j == 0 {
                    continue;
                }
                let first = if i <= 1 { FIRST } else { 0.0 };
                let mut best = neg;
                if i > 0 && j > 0 {
                    best = if w[i - 1] == ty[j - 1] {
                        d[i - 1][j - 1] + s.matched
                    } else if base(w[i - 1]) == Some(ty[j - 1]) {
                        d[i - 1][j - 1] + UMLAUT
                    } else {
                        d[i - 1][j - 1]
                            + if self.keys.neighbours(ty[j - 1], w[i - 1]) {
                                s.near
                            } else {
                                s.far
                            }
                            + first
                    };
                }
                if i > 0 {
                    best = best.max(d[i - 1][j] + s.missing + first);
                }
                if j > 0 {
                    let ch = ty[j - 1];
                    let around = &w[i.saturating_sub(1)..(i + 1).min(n)];
                    let kind = if around.contains(&ch) {
                        s.double
                    } else if around.iter().any(|&c| self.keys.neighbours(ch, c)) {
                        s.extra_near
                    } else {
                        s.extra_far
                    };
                    best = best.max(d[i][j - 1] + kind + if i == 0 { FIRST } else { 0.0 });
                }
                if i > 1 && j > 1 && w[i - 1] == ty[j - 2] && w[i - 2] == ty[j - 1] {
                    best = best.max(d[i - 2][j - 2] + s.swap + if i == 2 { FIRST } else { 0.0 });
                }
                d[i][j] = best;
            }
        }
        d[n][m]
    }

    /// 'd' / 'e' for a word only common in German / English, else None.
    fn lean(&self, w: Option<&str>) -> Option<char> {
        let w = w?;
        if w.is_empty() || w == BOS {
            return None;
        }
        let t = fold(w);
        let (d, e) = (self.table.common_in('d', &t), self.table.common_in('e', &t));
        if d && !e {
            Some('d')
        } else if e && !d {
            Some('e')
        } else {
            None
        }
    }

    /// "de" or "en" for the sentence so far: the last three words count 1, 1/2, 1/4, the next
    /// word 1/2. German wins ties: it is the user's first language.
    fn sentence_lang(&self, prevs: &[&str], next: Option<&str>) -> &'static str {
        let mut score = 0.0;
        let mut weight = 1.0;
        let sign = |l: Option<char>| match l {
            Some('d') => 1.0,
            Some('e') => -1.0,
            _ => 0.0,
        };
        for w in prevs[prevs.len().saturating_sub(3)..].iter().rev() {
            let l = self.lean(Some(w));
            let s = if l.is_some() {
                sign(l)
            } else if *w != BOS
                && self.table.common_in('d', &fold(w))
                && self.table.common_in('e', &fold(w))
            {
                // common in both tables: only one dictionary has it (see DICT_LEAN)
                let langs = self.lexicon.langs(w);
                match (langs.contains("de"), langs.contains("en")) {
                    (true, false) => DICT_LEAN,
                    (false, true) => -DICT_LEAN,
                    _ if self.lexicon.english_only.contains(&w.to_lowercase()) => -DICT_LEAN,
                    _ => 0.0,
                }
            } else {
                0.0
            };
            score += weight * s;
            weight /= 2.0;
        }
        score += 0.5 * sign(self.lean(next));
        if score < 0.0 {
            "en"
        } else {
            "de"
        }
    }

    fn umlaut_variants(&self, t: &str) -> Vec<String> {
        let c = chars(t);
        let spots: Vec<usize> = c
            .iter()
            .enumerate()
            .filter(|(_, x)| matches!(x, 'a' | 'o' | 'u'))
            .map(|(i, _)| i)
            .take(4)
            .collect();
        let mut out = Vec::new();
        for mask in 1u32..(1 << spots.len()) {
            let mut w = c.clone();
            for (k, &i) in spots.iter().enumerate() {
                if mask >> k & 1 == 1 {
                    w[i] = umlaut_of(w[i]);
                }
            }
            out.push(w.into_iter().collect());
        }
        out
    }

    /// A form of folded `t` with ä/ö/ü for a/o/u that is at least 10x more common (konnen).
    fn umlaut_twin(&self, t: &str, z: f64) -> bool {
        self.umlaut_variants(t)
            .iter()
            .any(|w| self.table.zipf(w) >= z + 1.0)
    }

    /// The common German word that is folded `t` with ä/ö/ü for a/o/u (fur -> für).
    fn umlaut_form(&self, t: &str) -> Option<String> {
        let mut best: Option<String> = None;
        for w in self.umlaut_variants(t) {
            if self.table.common_in('d', &w)
                && best
                    .as_ref()
                    .is_none_or(|b| self.table.zipf(&w) > self.table.zipf(b))
            {
                best = Some(w);
            }
        }
        best
    }

    /// Common, or a word German builds from common ones.
    fn known(&self, w: &str) -> bool {
        let t = fold(w);
        let z = self.table.zipf(&t);
        if self.umlaut_twin(&t, z) {
            return false; // konnen, uberlegt: web forms of umlaut words
        }
        if z >= COMMON {
            return true;
        }
        self.built_word(&t) || self.verb_stem(&t) || self.inflected(&t)
    }

    /// A German compound of two common words, or an English stem with a German ending.
    fn built_word(&self, w: &str) -> bool {
        let tb = &self.table;
        let c = chars(w);
        let n = c.len();
        for suf in DENGLISH_ENDINGS {
            if w.ends_with(suf)
                && n - suf.chars().count() >= 3
                && tb.common_in('e', strip_end(w, suf))
            {
                return true;
            }
        }
        if w.starts_with("ge") {
            let rest = sub(&c, 2, n);
            if tb.common_in('e', &rest)
                || (w.ends_with('t') && n >= 3 && tb.common_in('e', &sub(&c, 2, n - 1)))
            {
                return true;
            }
        }
        for i in 3..n.saturating_sub(2) {
            let (a, b) = (sub(&c, 0, i), sub(&c, i, n));
            let (la, lb) = (i, n - i);
            if la.min(lb) < 4 && la.max(lb) < 5 {
                continue;
            }
            if tb.common_in('d', &b)
                && (tb.common_in('d', &a)
                    || (a.ends_with('s') && tb.common_in('d', &sub(&c, 0, i - 1))))
            {
                return true;
            }
        }
        false
    }

    /// A German verb form without its ending: rechne(n), sammle (sammeln), informier(en/t), machs.
    fn verb_stem(&self, t: &str) -> bool {
        let tb = &self.table;
        let stems = [
            Some(t.to_string()),
            t.strip_suffix('s').map(|s| s.to_string()),
        ];
        for stem in stems.into_iter().flatten() {
            let c = chars(&stem);
            if c.len() < 3 {
                continue;
            }
            if stem.ends_with('e') && tb.common_in('d', &format!("{stem}n")) {
                return true;
            }
            if (stem.ends_with("le") || stem.ends_with("re"))
                && tb.common_in(
                    'd',
                    &format!("{}e{}n", sub(&c, 0, c.len() - 2), c[c.len() - 2]),
                )
            {
                return true;
            }
            if tb.common_in('d', &format!("{stem}en")) && tb.common_in('d', &format!("{stem}t")) {
                return true;
            }
        }
        false
    }

    /// A German word built from a common one by inflection or a prefix (fokussier-st, un-qualifiziert).
    fn inflected(&self, t: &str) -> bool {
        let tb = &self.table;
        let common = |w: &str| {
            w.chars().count() >= 4
                && (tb.common_in('d', w)
                    || tb.common_in('d', &format!("{w}en"))
                    || tb.common_in('d', &format!("{w}n"))
                    || (tb.zipf(w) >= 2.5 && !tb.common_in('e', w)))
        };
        let tn = t.chars().count();
        let mut stems = vec![t.to_string()];
        for pre in GERMAN_PREFIXES {
            if t.starts_with(pre) && tn - pre.chars().count() >= 4 {
                let rest = &t[pre.len()..];
                stems.push(if rest.starts_with("zu") && rest.chars().count() >= 6 {
                    rest[2..].to_string()
                } else {
                    rest.to_string()
                });
                stems.push(rest.to_string());
            }
        }
        for w in &stems {
            if w != t && common(w) {
                return true;
            }
            for end in GERMAN_ENDINGS {
                if !w.ends_with(end) {
                    continue;
                }
                let stem = strip_end(w, end);
                // German never ends a stem in t with -t/-te/-ten: arbeit-et, not arbeitt
                // (Robin, 26.11.2026: "Justt" counted as just + t and was left)
                if stem.ends_with('t') && end.starts_with('t') {
                    continue;
                }
                if common(stem) {
                    return true;
                }
            }
        }
        false
    }

    /// The German compound one slip from folded `t` (bchhaltungs|thema), or None when none or two compete.
    /// Two words the table knows, with a linking s if any (gas|turbinen, arbeit|s|zeit): a
    /// compound part that is a compound itself, not a typo.
    fn two_words(&self, w: &str) -> bool {
        let tb = &self.table;
        let c = chars(w);
        let n = c.len();
        let word = |x: &str, short: bool| {
            let x = if x.chars().count() >= 5 {
                x.strip_suffix('s').filter(|y| tb.candidate(y)).unwrap_or(x)
            } else {
                x
            };
            if short {
                tb.common_in('d', x) || tb.common_in('e', x)
            } else {
                tb.candidate(x)
            }
        };
        (3..n.saturating_sub(2)).any(|i| {
            let (a, b) = (sub(&c, 0, i), sub(&c, i, n));
            // a 3-letter part only when both are common words (gas|turbinen, not bch|haltungs)
            let short = i < 4 || n - i < 4;
            word(&a, short) && word(&b, short)
        })
    }

    fn compound_fix(&self, t: &str) -> Option<String> {
        let tb = &self.table;
        let common = |w: &str, left: bool| {
            let n = w.chars().count();
            n >= 4
                && (tb.common_in('d', w)
                    || (left && w.ends_with('s') && n >= 5 && tb.common_in('d', strip_end(w, "s"))))
        };
        let c = chars(t);
        let n = c.len();
        let mut found: Vec<(String, f64)> = Vec::new();
        for i in 4..n.saturating_sub(3) {
            let (a, b) = (sub(&c, 0, i), sub(&c, i, n));
            for (fixed, typed_part, left) in [(&b, &a, true), (&a, &b, false)] {
                if !common(fixed, !left)
                    || tb.candidate(typed_part)
                    || (COMPOUND_PART_CHECK && self.two_words(typed_part))
                {
                    continue;
                }
                for cand in one_slip(typed_part) {
                    if cand.chars().count() >= 4
                        && common(&cand, left)
                        // the table is a Bloom filter: among the hundreds of variants of a long
                        // part, one false "common" is likely (urbinenantriebe -> uabinenantriebe).
                        // Its frequency is a second, independent lookup; a real word has one.
                        && (tb.zipf(&cand) > 0.0
                            || (left && tb.zipf(cand.strip_suffix('s').unwrap_or(&cand)) > 0.0))
                    {
                        let w = if left {
                            format!("{cand}{b}")
                        } else {
                            format!("{a}{cand}")
                        };
                        let z = if tb.common_in('d', &cand) {
                            tb.zipf(&cand)
                        } else {
                            tb.zipf(cand.strip_suffix('s').unwrap_or(&cand))
                        };
                        let s = self.channel(typed_part, &cand) + z;
                        match found.iter_mut().find(|(k, _)| *k == w) {
                            Some(e) => e.1 = e.1.max(s),
                            None => found.push((w, s.max(-99.0))),
                        }
                    }
                }
            }
        }
        if found.is_empty() {
            return None;
        }
        // stable, like Python's sorted(): ties keep the order found
        found.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal));
        if found.len() > 1 && found[0].1 - found[1].1 < self.s.runner_up {
            return None;
        }
        Some(found.swap_remove(0).0)
    }

    /// An inflected German adjective before a noun: klein-es, neu-er, sinnvoll-en.
    fn adjective(&self, w: &str) -> bool {
        let tb = &self.table;
        let a = fold(w);
        if self.lean(Some(&a)) != Some('d') || tb.noun(&a) || self.infinitive(&a) {
            return false;
        }
        if a.ends_with("te") || a.ends_with("ten") {
            if let Some(i) = a.rfind("te") {
                if self.infinitive(&format!("{}en", &a[..i])) {
                    return false;
                }
            }
        }
        for end in ["em", "en", "er", "es", "e"] {
            if a.ends_with(end) {
                let stem = strip_end(&a, end);
                return stem.chars().count() >= 3
                    && tb.common_in('d', stem)
                    && !tb.noun(stem)
                    && !tb.common_in('d', &format!("{stem}t"))
                    && !tb.common_in('d', &format!("{stem}et"));
            }
        }
        false
    }

    /// A German infinitive whose stem conjugates (auswählen: auswählt); not "ersten".
    fn infinitive(&self, t: &str) -> bool {
        let tb = &self.table;
        if !tb.common_in('d', t) {
            return false;
        }
        for (end, stem_end) in [("ern", "ert"), ("eln", "elt"), ("en", "t")] {
            if t.ends_with(end) && tb.common_in('d', &format!("{}{stem_end}", strip_end(t, end))) {
                return true;
            }
        }
        false
    }
}

/// How often each letter occurs (a-z, ä ö ü ß; anything else shares one slot).
fn letter_bag(w: &str) -> [i16; 31] {
    let mut b = [0i16; 31];
    for c in w.chars() {
        b[slot(c)] += 1;
    }
    b
}

fn slot(c: char) -> usize {
    match c {
        'a'..='z' => c as usize - 'a' as usize,
        'ä' => 26,
        'ö' => 27,
        'ü' => 28,
        'ß' => 29,
        _ => 30,
    }
}

/// A lower bound on the slips between two words: the letters one has more of than the other.
fn bag_distance(bag: &[i16; 31], w: &str) -> i16 {
    let mut d = *bag;
    for c in w.chars() {
        d[slot(c)] -= 1;
    }
    let (more, fewer) = d.iter().fold(
        (0, 0),
        |(m, f), &x| if x > 0 { (m + x, f) } else { (m, f - x) },
    );
    more.max(fewer)
}

fn slip_set_sorted(s: &HashSet<String>) -> Vec<String> {
    let mut v: Vec<String> = s.iter().cloned().collect();
    v.sort();
    v
}

/// Python's max() over (score, word): the higher score, on a tie the larger word.
fn best_of(scored: &[(f64, String)]) -> (f64, String) {
    let mut best = &scored[0];
    for x in &scored[1..] {
        if x.0 > best.0 || (x.0 == best.0 && x.1 > best.1) {
            best = x;
        }
    }
    (best.0, best.1.clone())
}

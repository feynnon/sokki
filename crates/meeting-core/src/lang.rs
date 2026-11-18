//! Small text helpers shared by the other modules: a German/English language
//! guess (stop-word counting) and the token normalisation used for echo
//! detection.

use serde::{Deserialize, Serialize};

/// The languages the notes are written in. Anything else falls back to
/// German, the user's default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    German,
    English,
}

impl Language {
    /// English name, used in LLM prompts ("Write in German").
    pub fn name(self) -> &'static str {
        match self {
            Language::German => "German",
            Language::English => "English",
        }
    }
}

// Only words that are unambiguous between the two languages. Left out on
// purpose: "in", "so", "was", "will", "an", "also", "am", "hat", "man", "war",
// "bald", "rate", "fast", "gift", "kind", "rot", "hell", "okay".
const GERMAN: &[&str] = &[
    "der",
    "die",
    "das",
    "und",
    "ist",
    "nicht",
    "ich",
    "wir",
    "du",
    "sie",
    "es",
    "ein",
    "eine",
    "einen",
    "zu",
    "mit",
    "auf",
    "für",
    "von",
    "den",
    "dem",
    "auch",
    "noch",
    "dass",
    "aber",
    "wie",
    "haben",
    "habe",
    "wird",
    "werden",
    "sind",
    "bei",
    "oder",
    "schon",
    "dann",
    "mal",
    "ja",
    "nein",
    "gut",
    "nach",
    "über",
    "uns",
    "euch",
    "mir",
    "dir",
    "sich",
    "können",
    "kann",
    "müssen",
    "muss",
    "sollten",
    "würde",
    "gibt",
    "wenn",
    "weil",
    "jetzt",
    "hier",
    "da",
    "doch",
    "bis",
    "nächste",
    "woche",
    "genau",
    "machen",
    "wieder",
    "sehr",
    "kein",
    "keine",
    "vielleicht",
    "danke",
    "bitte",
    "heute",
    "morgen",
    "zum",
    "zur",
    "vom",
    "im",
    "beim",
    "unser",
    "unsere",
];

const ENGLISH: &[&str] = &[
    "the", "and", "is", "are", "to", "of", "we", "you", "it", "that", "this", "for", "with", "on",
    "have", "be", "not", "but", "what", "yes", "can", "i'm", "do", "does", "about", "just", "our",
    "they", "would", "should", "there", "their", "been", "were", "from", "at", "by", "or", "if",
    "my", "your", "me", "he", "she", "how", "when", "which", "who", "get", "let's", "going",
    "think", "know", "like", "really", "next", "week", "thanks", "please", "today", "tomorrow",
    "sure", "sounds", "good", "great", "all", "some", "any",
];

/// Counts German vs English evidence in `text`. Returns `None` when there is
/// no evidence either way (empty text, only names or numbers).
pub fn detect_language(text: &str) -> Option<Language> {
    let (de, en) = language_scores(text);
    if de == 0 && en == 0 {
        None
    } else if en > de {
        Some(Language::English)
    } else {
        Some(Language::German)
    }
}

/// (german, english) evidence counts. Umlauts and ß count towards German.
pub fn language_scores(text: &str) -> (usize, usize) {
    let mut de = 0;
    let mut en = 0;
    for raw in text.split_whitespace() {
        let w: String = raw
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '\'')
            .flat_map(char::to_lowercase)
            .collect();
        if w.is_empty() {
            continue;
        }
        if GERMAN.contains(&w.as_str()) {
            de += 1;
        } else if ENGLISH.contains(&w.as_str()) {
            en += 1;
        }
        if w.contains(['ä', 'ö', 'ü', 'ß']) {
            de += 1;
        }
    }
    (de, en)
}

/// Normalises one word for comparison: lower case, umlauts folded
/// (ä→a, ö→o, ü→u, ß→ss), everything that is not a letter or digit removed.
/// "Klick-Rate," becomes "klickrate", "Grüße" becomes "grusse".
pub fn normalize_word(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    for c in word.chars().flat_map(char::to_lowercase) {
        match c {
            'ä' | 'à' | 'á' | 'â' => out.push('a'),
            'ö' | 'ò' | 'ó' | 'ô' => out.push('o'),
            'ü' | 'ù' | 'ú' | 'û' => out.push('u'),
            'é' | 'è' | 'ê' => out.push('e'),
            'ß' => out.push_str("ss"),
            c if c.is_alphanumeric() => out.push(c),
            _ => {}
        }
    }
    out
}

/// Splits `text` into normalised tokens, remembering which whitespace word
/// each token came from (so text can be rebuilt from the original words).
pub fn tokenize(text: &str) -> Vec<(String, usize)> {
    text.split_whitespace()
        .enumerate()
        .filter_map(|(i, w)| {
            let n = normalize_word(w);
            (!n.is_empty()).then_some((n, i))
        })
        .collect()
}

/// Levenshtein distance over chars.
/// Kölner Phonetik: a sound code for German words (like Soundex, made for
/// German). Similar-sounding words get similar digit strings: "Color" 457,
/// "Kalasar" 4587; "Lied" and "Lead" both 52. Letters other than a–z (after
/// folding umlauts and ß) are ignored.
pub fn koelner_phonetik(text: &str) -> String {
    let w: Vec<char> = text
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'ä' => 'a',
            'ö' => 'o',
            'ü' => 'u',
            'ß' => 's',
            c => c,
        })
        .filter(|c| c.is_ascii_lowercase())
        .collect();
    let mut raw = String::new();
    for (i, &ch) in w.iter().enumerate() {
        let prev = if i > 0 { Some(w[i - 1]) } else { None };
        let next = w.get(i + 1).copied();
        let next_in = |set: &str| next.is_some_and(|n| set.contains(n));
        let code: &str = match ch {
            'a' | 'e' | 'i' | 'j' | 'o' | 'u' | 'y' => "0",
            'h' => "",
            'b' => "1",
            'p' => {
                if next == Some('h') {
                    "3"
                } else {
                    "1"
                }
            }
            'd' | 't' => {
                if next_in("csz") {
                    "8"
                } else {
                    "2"
                }
            }
            'f' | 'v' | 'w' => "3",
            'g' | 'k' | 'q' => "4",
            'c' => {
                if i == 0 {
                    if next_in("ahkloqrux") {
                        "4"
                    } else {
                        "8"
                    }
                } else if prev.is_some_and(|p| p == 's' || p == 'z') || !next_in("ahkoqux") {
                    "8"
                } else {
                    "4"
                }
            }
            'x' => {
                if prev.is_some_and(|p| "ckq".contains(p)) {
                    "8"
                } else {
                    "48"
                }
            }
            'l' => "5",
            'm' | 'n' => "6",
            'r' => "7",
            's' | 'z' => "8",
            _ => "",
        };
        raw.push_str(code);
    }
    let mut collapsed = String::new();
    for c in raw.chars() {
        if collapsed.chars().last() != Some(c) {
            collapsed.push(c);
        }
    }
    let mut out = String::new();
    for (i, c) in collapsed.chars().enumerate() {
        if i == 0 || c != '0' {
            out.push(c);
        }
    }
    out
}

pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Whether two normalised tokens are "the same word" allowing for small ASR
/// spelling differences: identical, or both at least 4 chars long with at
/// most ~25 % of characters edited.
pub fn tokens_similar(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (la, lb) = (a.chars().count(), b.chars().count());
    if la < 4 || lb < 4 {
        return false;
    }
    let d = levenshtein(a, b);
    (d as f64) <= 0.25 * la.max(lb) as f64
}

/// Rough token estimate for LLM budgeting: ~3.5 characters per token.
pub fn estimate_tokens(text: &str) -> usize {
    (text.chars().count() as f64 / 3.5).ceil() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_german_and_english() {
        assert_eq!(
            detect_language("Das ist besser als erwartet. Was ist mit den Kosten pro Lead?"),
            Some(Language::German)
        );
        assert_eq!(
            detect_language("Sure, happy to. We finished the new onboarding flow last week."),
            Some(Language::English)
        );
        assert_eq!(detect_language("Daniel 42"), None);
        assert_eq!(detect_language(""), None);
    }

    #[test]
    fn mixed_text_goes_to_majority() {
        let t = "Hi Daniel, schön dass du dabei bist. Können wir kurz über das Onboarding \
                 sprechen? Sure, happy to. Sehr gut, dann lass uns nächste Woche ein Review machen.";
        assert_eq!(detect_language(t), Some(Language::German));
    }

    #[test]
    fn normalizes_words() {
        assert_eq!(normalize_word("Klick-Rate,"), "klickrate");
        assert_eq!(normalize_word("Grüße!"), "grusse");
        assert_eq!(normalize_word("2,3"), "23");
        assert_eq!(normalize_word("–"), "");
        let toks = tokenize("Ja – genau, so.");
        assert_eq!(
            toks,
            vec![("ja".into(), 0), ("genau".into(), 2), ("so".into(), 3)]
        );
    }

    #[test]
    fn fuzzy_tokens() {
        assert!(tokens_similar("deployment", "deploiment"));
        assert!(tokens_similar("ja", "ja"));
        assert!(!tokens_similar("ja", "da"));
        assert!(!tokens_similar("budget", "bucket"));
        assert_eq!(levenshtein("kitten", "sitting"), 3);
    }

    #[test]
    fn token_estimate() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcdefg"), 2);
    }
    #[test]
    fn koelner_phonetik_groups_similar_sounds() {
        let k = koelner_phonetik;
        assert_eq!(k("Lied"), k("Lead"));
        assert_eq!(k("Cloud"), k("Claude"));
        assert_eq!(k("Kalazar"), k("Kalasar"));
        assert_eq!(k("Müller"), "657");
        assert_eq!(k("Color"), "457");
        assert_eq!(k("Kalasar"), "4587");
        assert_ne!(k("strugglen"), k("Kalasar"));
        assert_eq!(k("123"), "");
    }
}

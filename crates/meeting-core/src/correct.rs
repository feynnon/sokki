//! Correcting misheard dictionary words in a transcript with a language model.
//!
//! Parakeet cannot be given a vocabulary, so the user's own words come out
//! misheard: "Cloud Code" for "Claude Code", "Kalazar" for "Kalasar",
//! "APDO" for "AVDO", "Lied" for "Lead".
//!
//! Measured on two real calls (`scripts/correction_eval.py`): asking a small
//! local model to fix *any* recognition error makes things worse. It
//! "corrects" correct names and Denglisch ("Noxi" → "Kalasar", "raisen" →
//! "raise") and invents words. Research agrees (RECOVER, arXiv 2603.16411):
//! constrain the model to the user's terms and check every edit.
//!
//! So the model answers one narrow question per window of lines: which spans
//! are a misheard word from the user's dictionary? Each answer is then
//! checked in code before it is applied:
//! - the span exists in the line and contains no dictionary entry itself (so
//!   known names like "Noxi" or "Petard" are never replaced);
//! - the replacement contains a dictionary entry;
//! - the two sound alike (Kölner Phonetik), so "strugglen" → "Kalasar" fails;
//! - it is not just another ending or a longer/shorter form of the same word
//!   ("Marketer" → "Marketing", "raisen" → "raise", "Location" →
//!   "Multilocation");
//! - numbers stay the same.
//!
//! Windows run twice, the second time shifted by half a window, so every line
//! is seen with different neighbours. A confirmed fix is applied to the same
//! misheard words everywhere in the transcript. Every applied change is
//! returned so the note can list it.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::lang::{koelner_phonetik, levenshtein};
use crate::summary::{extract_object, remove_trailing_commas, LlmSummarizer};
use crate::{Segment, Transcript};

#[derive(Debug, Clone, PartialEq)]
pub struct CorrectConfig {
    /// Lines the model checks per request.
    pub window: usize,
    /// Preceding lines shown as read-only context.
    pub context: usize,
    /// The user's dictionary: terms and names, correctly spelled. Without
    /// entries there is nothing to correct and no request is made.
    pub glossary: Vec<String>,
    /// Longest span (in words) a single correction may replace.
    pub max_span_words: usize,
    /// Second pass with windows shifted by half a window (measured: no gain,
    /// double the time; off).
    pub shifted_pass: bool,
    /// Only lines with a word group that sounds like a dictionary entry go
    /// to the model (Kölner Phonetik within `prefilter_distance`). The model
    /// still decides; this only skips lines that cannot contain a fix.
    pub prefilter: bool,
    pub prefilter_distance: f64,
    /// Largest allowed sound difference (edit distance of the Kölner codes
    /// over the longer code). 0.35 ≈ one differing sound in three.
    pub max_sound_distance: f64,
}

impl Default for CorrectConfig {
    fn default() -> Self {
        Self {
            window: 8,
            context: 4,
            glossary: Vec::new(),
            max_span_words: 4,
            shifted_pass: false,
            prefilter: true,
            prefilter_distance: 0.35,
            max_sound_distance: 0.35,
        }
    }
}

/// One applied change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Correction {
    /// Index of the segment in the transcript.
    pub segment: usize,
    pub from: String,
    pub to: String,
}

/// What the model reports.
#[derive(Debug, Deserialize)]
struct Match {
    #[serde(alias = "segment", alias = "index")]
    line: usize,
    #[serde(alias = "from", alias = "wrong", alias = "original")]
    heard: String,
    #[serde(alias = "to", alias = "replacement", alias = "fixed")]
    correct: String,
}

#[derive(Debug, Deserialize)]
struct Reply {
    #[serde(default, alias = "corrections")]
    matches: Vec<Match>,
}

/// Asks the model for misheard dictionary words and applies the ones that
/// pass the checks. Model errors (server down, bad JSON) leave the affected
/// lines unchanged and are returned as warnings.
pub fn correct_transcript(
    llm: &LlmSummarizer,
    transcript: &Transcript,
    cfg: &CorrectConfig,
) -> (Transcript, Vec<Correction>, Vec<String>) {
    let mut out = transcript.clone();
    let mut applied = Vec::new();
    let mut warnings = Vec::new();
    let dictionary: Vec<String> = cfg
        .glossary
        .iter()
        .map(|g| g.trim().to_string())
        .filter(|g| !g.is_empty())
        .collect();
    let n = transcript.segments.len();
    if dictionary.is_empty() || n == 0 {
        return (out, applied, warnings);
    }
    let system = system_prompt(&dictionary);
    let w = cfg.window.max(1);
    // Batches of lines to check, each with the lines shown as context.
    let mut batches: Vec<(Vec<usize>, Vec<usize>)> = Vec::new();
    if cfg.prefilter {
        let codes: Vec<String> = dictionary.iter().map(|d| koelner_phonetik(d)).collect();
        let flagged: Vec<usize> = (0..n)
            .filter(|&i| {
                has_candidate(
                    &transcript.segments[i].text,
                    &dictionary,
                    &codes,
                    cfg.prefilter_distance,
                )
            })
            .collect();
        for chunk in flagged.chunks(w) {
            let mut ctx: Vec<usize> = chunk
                .iter()
                .flat_map(|&i| i.saturating_sub(cfg.context.min(1))..i)
                .filter(|j| !chunk.contains(j))
                .collect();
            ctx.dedup();
            batches.push((chunk.to_vec(), ctx));
        }
    } else {
        let mut starts: Vec<usize> = (0..n).step_by(w).collect();
        if cfg.shifted_pass && w >= 2 {
            starts.extend((w / 2..n).step_by(w));
        }
        for a in starts {
            let lines: Vec<usize> = (a..(a + w).min(n)).collect();
            let ctx: Vec<usize> = (a.saturating_sub(cfg.context)..a).collect();
            batches.push((lines, ctx));
        }
    }
    for (lines, ctx) in batches {
        let user = user_prompt(&out.segments, &lines, &ctx);
        match llm.chat_plain(&system, &user).and_then(|r| parse_reply(&r)) {
            Ok(matches) => {
                for m in matches {
                    if !lines.contains(&m.line) {
                        continue;
                    }
                    if let Some(c) = apply(
                        &mut out.segments[m.line],
                        m.line,
                        &m.heard,
                        &m.correct,
                        &dictionary,
                        cfg,
                    ) {
                        applied.push(c);
                    }
                }
            }
            Err(e) => warnings.push(format!(
                "Correction skipped for lines {}–{}: {e:#}",
                lines.first().map_or(0, |l| l + 1),
                lines.last().map_or(0, |l| l + 1)
            )),
        }
    }
    propagate_term_fixes(&mut out, &mut applied);
    (out, applied, warnings)
}

fn system_prompt(dictionary: &[String]) -> String {
    format!(
        "You find places in a meeting transcript (German, with English business terms) where the speech \
recogniser misheard a word from the user's dictionary. Dictionary (correct spellings): {}.\n\
Report a span only if BOTH hold: it sounds similar to a dictionary entry (also misspellings like \
'APDO' for 'AVDO', split or merged words), AND that entry makes sense in the sentence.\n\
Do not report anything else: other errors, grammar, English words, Denglisch, or names that are not \
in the dictionary. Most lines contain no such span.\n\
Answer with JSON only: {{\"matches\": [{{\"line\": <number>, \"heard\": \"<exact words in the line>\", \
\"correct\": \"<the words as they should be written, using the dictionary entry>\"}}]}}",
        dictionary.join(", ")
    )
}

fn user_prompt(segments: &[Segment], lines: &[usize], context: &[usize]) -> String {
    let mut s = String::new();
    if !context.is_empty() {
        s.push_str("Context (do not report):\n");
        for &i in context {
            s.push_str(&format!(
                "[{i}] {}: {}\n",
                segments[i].speaker, segments[i].text
            ));
        }
        s.push('\n');
    }
    s.push_str("Lines to check:\n");
    for &i in lines {
        s.push_str(&format!(
            "[{i}] {}: {}\n",
            segments[i].speaker, segments[i].text
        ));
    }
    s
}

/// Whether some group of 1–3 words in `text` sounds like a dictionary
/// entry without being it (Kölner codes within `max_distance`).
fn has_candidate(text: &str, dictionary: &[String], codes: &[String], max_distance: f64) -> bool {
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c == '-')
        .filter(|w| !w.is_empty())
        .collect();
    for n in 1..=3 {
        for win in words.windows(n) {
            // A group never spans punctuation ("genau, das" is not "Meta Ads").
            if win[..n - 1]
                .iter()
                .any(|w| w.ends_with(|c: char| ",.;:!?".contains(c)))
            {
                continue;
            }
            let gram = win.join(" ");
            let g = compact(&gram);
            if g.chars().count() < 3 {
                continue;
            }
            let kg = koelner_phonetik(&gram);
            for (d, kd) in dictionary.iter().zip(codes) {
                if g == compact(d) || kg.is_empty() || kd.is_empty() {
                    continue;
                }
                let len_ratio = g.chars().count() as f64 / compact(d).chars().count().max(1) as f64;
                if !(0.5..=2.0).contains(&len_ratio) {
                    continue;
                }
                let dist =
                    levenshtein(&kg, kd) as f64 / kg.chars().count().max(kd.chars().count()) as f64;
                // Sound alone matches too much for short entries ("machen" ~
                // "Noxi"); the letters must be fairly close too. Measured on
                // two real calls: 45% of lines pass, 22 of 23 misheard
                // dictionary words are among them.
                let dc = compact(d);
                let letters =
                    levenshtein(&g, &dc) as f64 / g.chars().count().max(dc.chars().count()) as f64;
                if dist <= max_distance && letters <= 0.6 {
                    return true;
                }
            }
        }
    }
    false
}

/// Reads the model's answer: one `line | heard | correct` per span, or
/// "none". A JSON answer (`{"matches": [...]}`) is accepted too. Compact
/// lines are about half the output tokens of indented JSON, and output is
/// the slow part (~25 tokens/s with qwen3.5:9b on an M1 Pro).
fn parse_reply(reply: &str) -> Result<Vec<Match>> {
    let text = reply.trim();
    let mut out = Vec::new();
    let mut lines_seen = 0;
    for raw in text.lines() {
        let l = raw.trim().trim_matches('`').trim();
        if l.is_empty() || l.eq_ignore_ascii_case("none") {
            continue;
        }
        let parts: Vec<&str> = l.splitn(3, '|').map(str::trim).collect();
        if parts.len() == 3 {
            if let Ok(line) = parts[0]
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<usize>()
            {
                lines_seen += 1;
                out.push(Match {
                    line,
                    heard: parts[1].to_string(),
                    correct: parts[2].to_string(),
                });
            }
        }
    }
    if lines_seen > 0
        || text.eq_ignore_ascii_case("none")
        || text.trim_matches('`').trim().eq_ignore_ascii_case("none")
    {
        return Ok(out);
    }
    let obj = extract_object(reply)
        .ok_or_else(|| anyhow::anyhow!("no spans and no JSON in the answer"))?;
    // Raw newlines or tabs inside a string are invalid JSON; seen at the
    // model's default temperature. As spaces they change nothing.
    let obj: String = obj
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let r: Reply = serde_json::from_str(&remove_trailing_commas(&obj))
        .map_err(|e| anyhow::anyhow!("not valid correction JSON ({e})"))?;
    Ok(r.matches)
}

/// A confirmed fix applies to every other occurrence of the same misheard
/// words (seen: "Cloud Code" fixed once, left 5 times).
fn propagate_term_fixes(out: &mut Transcript, applied: &mut Vec<Correction>) {
    let mut fixes: Vec<(String, String)> = Vec::new();
    for c in applied.iter() {
        let fix = (c.from.clone(), c.to.clone());
        if !fixes.contains(&fix) {
            fixes.push(fix);
        }
    }
    for (from, to) in fixes {
        for (i, seg) in out.segments.iter_mut().enumerate() {
            let n = replace_all(&mut seg.text, &from, &to);
            for _ in 0..n {
                applied.push(Correction {
                    segment: i,
                    from: from.clone(),
                    to: to.clone(),
                });
            }
        }
    }
}

/// Replaces every whole-word occurrence of `from` (words joined by spaces or
/// hyphens) and returns how many. The search always continues after the
/// inserted text: the replacement may contain `from` ("Account" →
/// "AVDO-Account"), and searching from the start again once looped forever
/// and ate 50 GB.
fn replace_all(text: &mut String, from: &str, to: &str) -> usize {
    let mut at = 0;
    let mut n = 0;
    while let Some((pos, len)) = find_phrase(text, from, at) {
        text.replace_range(pos..pos + len, to);
        at = pos + to.len();
        n += 1;
        if n > 1000 {
            break; // cannot happen; never loop unbounded on user text
        }
    }
    n
}

/// Byte position and length of `phrase` as whole words in `hay` at or after
/// byte `from`; the words of `phrase` may be separated by spaces or hyphens.
fn find_phrase(hay: &str, phrase: &str, from: usize) -> Option<(usize, usize)> {
    let words: Vec<&str> = phrase
        .split(|c: char| c.is_whitespace() || c == '-')
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() || from > hay.len() {
        return None;
    }
    let lower = hay.to_lowercase();
    if lower.len() != hay.len() {
        // Lowercasing changed byte lengths (rare characters): match case-sensitively.
        return find_phrase_in(hay, hay, &words, from, false);
    }
    find_phrase_in(hay, &lower, &words, from, true)
}

fn find_phrase_in(
    hay: &str,
    search: &str,
    words: &[&str],
    from: usize,
    lower: bool,
) -> Option<(usize, usize)> {
    let first = if lower {
        words[0].to_lowercase()
    } else {
        words[0].to_string()
    };
    let mut start = from;
    while start <= search.len() {
        let p = start + search[start..].find(first.as_str())?;
        let mut end = p + first.len();
        let mut ok = !hay[..p]
            .chars()
            .next_back()
            .is_some_and(char::is_alphanumeric);
        for w in &words[1..] {
            if !ok {
                break;
            }
            let rest = &search[end..];
            let sep = rest.len()
                - rest
                    .trim_start_matches(|c: char| c.is_whitespace() || c == '-')
                    .len();
            let w = if lower {
                w.to_lowercase()
            } else {
                w.to_string()
            };
            if sep == 0 || !rest[sep..].starts_with(w.as_str()) {
                ok = false;
            } else {
                end += sep + w.len();
            }
        }
        if ok && !hay[end..].chars().next().is_some_and(char::is_alphanumeric) {
            return Some((p, end - p));
        }
        start = p + first.len().max(1);
        while start < search.len() && !search.is_char_boundary(start) {
            start += 1;
        }
    }
    None
}

/// Applies one reported span if it passes the checks.
fn apply(
    seg: &mut Segment,
    index: usize,
    heard: &str,
    correct: &str,
    dictionary: &[String],
    cfg: &CorrectConfig,
) -> Option<Correction> {
    let heard = heard.trim();
    let correct = correct.trim();
    if heard.is_empty() || correct.is_empty() || heard == correct {
        return None;
    }
    if heard.split_whitespace().count() > cfg.max_span_words
        || correct.split_whitespace().count() > cfg.max_span_words + 1
    {
        return None;
    }
    find_phrase(&seg.text, heard, 0)?;
    // The replacement uses an entry, also inflected or in a compound
    // ("USPs", "Lead-Follow-up"); the heard words are not already one (so a
    // known name such as "Noxi" is never replaced).
    let uses_entry = dictionary
        .iter()
        .any(|d| compact(correct).contains(&compact(d)));
    let is_entry = dictionary
        .iter()
        .any(|d| find_phrase(heard, d, 0).is_some());
    if !uses_entry || is_entry {
        return None;
    }
    // One change brings in one entry: "Pflegeheim Multi Location" ->
    // "Tigny Kare Multilocation" invented a name next to a real fix.
    let (cc, ch) = (compact(correct), compact(heard));
    let added: Vec<String> = dictionary
        .iter()
        .map(|d| compact(d))
        .filter(|d| cc.contains(d.as_str()) && !ch.contains(d.as_str()))
        .collect();
    // "Claude Code" also contains "Claude": count only the longest ones.
    let distinct = added
        .iter()
        .filter(|d| {
            !added
                .iter()
                .any(|o| o.len() > d.len() && o.contains(d.as_str()))
        })
        .count();
    if distinct > 1 {
        return None;
    }
    // Every entry the change brings in must sound like some words of the
    // original ("Pflegeheim" does not sound like "Tigny Kare").
    for d in dictionary {
        let dc = compact(d);
        if !cc.contains(dc.as_str()) || ch.contains(dc.as_str()) {
            continue;
        }
        if !sounds_like_some_words(heard, d, cfg.max_sound_distance) {
            return None;
        }
    }
    // Judge only the words that change: in "750 Euro waven" -> "750 Euro
    // Lead" the shared words made the whole phrases sound alike.
    let (core_heard, core_correct) = changed_core(heard, correct);
    if core_heard.is_empty() || core_correct.is_empty() {
        return None;
    }
    rejection(&core_heard, &core_correct, cfg.max_sound_distance).map_or(Some(()), |_| None)?;
    replace_all(&mut seg.text, heard, correct);
    Some(Correction {
        segment: index,
        from: heard.to_string(),
        to: correct.to_string(),
    })
}

/// The words that actually change: `from` and `to` without the words they
/// share at the start and the end (compared case-insensitively).
fn changed_core(from: &str, to: &str) -> (String, String) {
    let split = |s: &str| -> Vec<String> {
        s.split(|c: char| c.is_whitespace() || c == '-')
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    };
    let (a, b) = (split(from), split(to));
    let eq = |x: &str, y: &str| x.to_lowercase() == y.to_lowercase();
    let mut p = 0;
    while p < a.len() && p < b.len() && eq(&a[p], &b[p]) {
        p += 1;
    }
    let mut q = 0;
    while q < a.len() - p && q < b.len() - p && eq(&a[a.len() - 1 - q], &b[b.len() - 1 - q]) {
        q += 1;
    }
    (a[p..a.len() - q].join(" "), b[p..b.len() - q].join(" "))
}

/// Whether a group of 1–3 words of `text` sounds like `entry`.
fn sounds_like_some_words(text: &str, entry: &str, max_distance: f64) -> bool {
    let ke = koelner_phonetik(entry);
    if ke.is_empty() {
        return true;
    }
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c == '-')
        .filter(|w| !w.is_empty())
        .collect();
    (1..=3).any(|n| {
        words.windows(n).any(|win| {
            let kg = koelner_phonetik(&win.join(" "));
            !kg.is_empty()
                && levenshtein(&kg, &ke) as f64 / kg.chars().count().max(ke.chars().count()) as f64
                    <= max_distance
        })
    })
}

/// Letters and digits only, lowercase.
fn compact(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

const ENDINGS: &[&str] = &[
    "", "e", "n", "en", "er", "ern", "es", "s", "t", "te", "ten", "st", "ing", "ng", "ed", "ers",
    "ung", "ier", "eer", "eers",
];

fn stem(w: &str) -> &str {
    let mut best = w;
    for e in ENDINGS {
        if !e.is_empty() && w.ends_with(e) && w.chars().count() >= e.chars().count() + 4 {
            let s = &w[..w.len() - e.len()];
            if s.len() < best.len() {
                best = s;
            }
        }
    }
    best
}

/// Why a reported change must not be applied, or `None` if it may be.
fn rejection(heard: &str, correct: &str, max_sound: f64) -> Option<&'static str> {
    let (h, c) = (compact(heard), compact(correct));
    if h.is_empty() || c.is_empty() {
        return Some("empty");
    }
    if h == c {
        return None; // spacing or hyphens only
    }
    let digits = |s: &str| s.chars().filter(char::is_ascii_digit).collect::<String>();
    if digits(&h) != digits(&c) {
        return Some("numbers differ");
    }
    if h.chars().count().min(c.chars().count()) >= 4 && (h.contains(&c) || c.contains(&h)) {
        return Some("longer or shorter form of the same word");
    }
    let common = h.chars().zip(c.chars()).take_while(|(a, b)| a == b).count();
    let tail = |s: &str| s.chars().skip(common).collect::<String>();
    if common >= 5 && ENDINGS.contains(&tail(&h).as_str()) && ENDINGS.contains(&tail(&c).as_str()) {
        return Some("only the ending differs");
    }
    if stem(&h) == stem(&c) {
        return Some("only the ending differs");
    }
    let (kh, kc) = (koelner_phonetik(heard), koelner_phonetik(correct));
    if !kh.is_empty() && !kc.is_empty() {
        let d = levenshtein(&kh, &kc) as f64 / kh.chars().count().max(kc.chars().count()) as f64;
        if d > max_sound {
            return Some("does not sound alike");
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TrackKind;

    fn seg(t: &str) -> Segment {
        Segment {
            start_s: 0.0,
            end_s: 1.0,
            track: TrackKind::Mic,
            speaker: "Speaker 1".into(),
            text: t.into(),
        }
    }

    fn dict() -> Vec<String> {
        [
            "Kalasar",
            "AVDO",
            "Claude Code",
            "Claude",
            "Meta Ads",
            "Lead",
            "USP",
            "Multilocation",
            "Performance Marketing",
            "Petard",
            "Noxi",
            "Nordim",
            "Tigny Kare",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    fn try_apply(text: &str, heard: &str, correct: &str) -> Option<String> {
        let mut s = seg(text);
        apply(
            &mut s,
            0,
            heard,
            correct,
            &dict(),
            &CorrectConfig::default(),
        )
        .map(|_| s.text)
    }

    #[test]
    fn misheard_dictionary_words_are_fixed() {
        // All seen on real calls.
        assert_eq!(
            try_apply("Was kostet ein qualifizierter Lied?", "Lied", "Lead").unwrap(),
            "Was kostet ein qualifizierter Lead?"
        );
        assert_eq!(
            try_apply(
                "mit Whatever Tool oder Cloud Code",
                "Cloud Code",
                "Claude Code"
            )
            .unwrap(),
            "mit Whatever Tool oder Claude Code"
        );
        assert_eq!(
            try_apply("keinen Cloud-Code nutzt", "Cloud Code", "Claude Code").unwrap(),
            "keinen Claude Code nutzt"
        );
        assert!(try_apply("wofür brauche ich Color", "Color", "Kalasar").is_some());
        assert!(try_apply("Für APDO finde ich", "APDO", "AVDO").is_some());
        assert!(try_apply("bei APitao habe ich", "APitao", "AVDO").is_some());
        assert!(try_apply("für den Nordim von der VDO", "VDO", "AVDO").is_some());
        assert!(try_apply("Perdard, dem muss ich", "Perdard", "Petard").is_some());
        assert!(try_apply("Hypothesen zu Use Ps", "Use Ps", "USPs").is_some());
        assert!(try_apply("ein Multiculation", "Multiculation", "Multilocation").is_some());
        assert!(try_apply("die Metaads laufen", "Metaads", "Meta Ads").is_some());
    }

    #[test]
    fn real_call_mistakes_are_rejected() {
        // Correct words the models "fixed" on real calls.
        assert!(
            try_apply("damit strugglen", "strugglen", "Kalasar").is_none(),
            "does not sound alike"
        );
        assert!(try_apply("über den Accountant weg", "Accountant", "Kalasar").is_none());
        assert!(
            try_apply("introducing Noxi", "Noxi", "Nordim").is_none(),
            "known name"
        );
        assert!(
            try_apply("sobald ich Petard fertig habe", "Petard", "Ethard").is_none(),
            "known name"
        );
        assert!(
            try_apply(
                "Performance Marketers",
                "Performance Marketers",
                "Performance Marketing"
            )
            .is_none(),
            "ending only"
        );
        assert!(
            try_apply("Proven Performance Marketier", "Marketier", "Marketing").is_none(),
            "ending only"
        );
        assert!(try_apply("mehrere Locations", "Locations", "Multilocation").is_none());
        assert!(
            try_apply("die anderen Location", "Location", "Multilocation").is_none(),
            "longer form"
        );
        assert!(
            try_apply("auf jeden Fall raisen", "raisen", "raise").is_none(),
            "no dictionary entry"
        );
        assert!(
            try_apply("die SMIs gedroppt", "SMIs", "Zanobtis").is_none(),
            "not in the dictionary"
        );
        assert!(
            try_apply("Whatever Tool", "Whatever Tool", "Claude Code").is_none(),
            "does not sound alike"
        );
        assert!(
            try_apply("Lied", "Lead", "Lead").is_none(),
            "span not in line"
        );
        assert!(
            try_apply("die 12 Leads", "12 Leads", "13 Lead").is_none(),
            "numbers"
        );
        // Shared words must not make a change look like a mishearing.
        assert!(try_apply(
            "diesem Pflegeheim Multi Location.",
            "Pflegeheim Multi Location",
            "Tigny Kare Multilocation"
        )
        .is_none());
        assert!(try_apply("mit Cloud Code plus", "Cloud Code", "Claude Code").is_some());
        assert!(try_apply(
            "die Performance-Marketeers",
            "Performance-Marketeers",
            "Performance Marketing"
        )
        .is_none());
        assert!(try_apply("die 750 Euro waven, um", "750 Euro waven", "750 Euro Lead").is_none());
        assert!(try_apply("Park sagt Kristof uh", "Kristof uh", "Kristof Wagner").is_none());
        assert!(try_apply(
            "was ein qualifizierter Lied kostet",
            "qualifizierter Lied",
            "qualifizierter Lead"
        )
        .is_some());
    }

    #[test]
    fn spans_must_be_whole_words() {
        assert!(try_apply("Kampagnenlied", "lied", "Lead").is_none());
        assert_eq!(
            find_phrase("die Cloud-Code Sache", "Cloud Code", 0),
            Some((4, 10))
        );
        assert_eq!(find_phrase("Cloudy Code", "Cloud Code", 0), None);
        assert_eq!(find_phrase("Ärger mit Lied", "Lied", 0), Some((11, 4)));
    }

    #[test]
    fn parses_model_replies() {
        let r = parse_reply("```json\n{\"matches\": [{\"line\": 2, \"heard\": \"Lied\", \"correct\": \"Lead\"},]}\n```").unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].line, 2);
        assert!(parse_reply("{\"matches\": []}").unwrap().is_empty());
        assert!(
            parse_reply("{\"corrections\": [{\"segment\": 1, \"from\": \"a\", \"to\": \"b\"}]}")
                .unwrap()
                .len()
                == 1
        );
        assert!(parse_reply("nope").is_err());
        let r =
            parse_reply("14 | Cloud Code | Claude Code\n121 | Cloud-Code | Claude Code").unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(
            (r[1].line, r[1].heard.as_str(), r[1].correct.as_str()),
            (121, "Cloud-Code", "Claude Code")
        );
        assert!(parse_reply("none").unwrap().is_empty());
        assert!(parse_reply("```\nnone\n```").unwrap().is_empty());
    }

    #[test]
    fn propagation_ends_when_the_fix_contains_the_misheard_words() {
        // Once looped forever: "Account" -> "AVDO-Account" matched again
        // inside its own replacement, growing the line until memory ran out.
        let mut t = Transcript {
            segments: vec![
                seg("Das Account ist neu."),
                seg("Account, Account und AVDO-Account."),
            ],
        };
        let mut applied = vec![Correction {
            segment: 0,
            from: "Account".into(),
            to: "AVDO-Account".into(),
        }];
        propagate_term_fixes(&mut t, &mut applied);
        assert_eq!(t.segments[0].text, "Das AVDO-Account ist neu.");
        assert!(t.segments[1]
            .text
            .starts_with("AVDO-Account, AVDO-Account und "));
        assert!(applied.len() <= 6, "{applied:?}");
    }

    #[test]
    fn propagation_fixes_every_other_occurrence() {
        let mut t = Transcript {
            segments: vec![
                seg("Mit Claude Code bauen."),
                seg("Cloud Code und nochmal Cloud-Code."),
            ],
        };
        let mut applied = vec![Correction {
            segment: 0,
            from: "Cloud Code".into(),
            to: "Claude Code".into(),
        }];
        propagate_term_fixes(&mut t, &mut applied);
        assert_eq!(t.segments[1].text, "Claude Code und nochmal Claude Code.");
    }

    #[test]
    fn no_dictionary_means_no_requests() {
        let llm = LlmSummarizer::new(crate::summary::LlmConfig::ollama("unused-model"));
        let t = Transcript {
            segments: vec![seg("Kosten pro Lied")],
        };
        let (out, applied, warnings) = correct_transcript(&llm, &t, &CorrectConfig::default());
        assert_eq!(out, t);
        assert!(applied.is_empty() && warnings.is_empty());
    }
    #[test]
    fn control_characters_inside_strings_are_tolerated() {
        let reply = "{\"matches\": [{\"line\": 1, \"heard\": \"Cloud\nCode\", \"correct\": \"Claude Code\"}]}";
        let m = parse_reply(reply).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].heard, "Cloud Code");
    }

    #[test]
    fn prefilter_finds_lines_with_candidates_only() {
        let d = dict();
        let codes: Vec<String> = d.iter().map(|x| koelner_phonetik(x)).collect();
        let cand = |t: &str| has_candidate(t, &d, &codes, 0.35);
        assert!(cand("Was kostet ein qualifizierter Lied?"));
        assert!(cand("wofür brauche ich eigentlich Colazar"));
        // Trade-off: "Color" for Kalasar is too far in letters to be flagged.
        assert!(!cand("wofür brauche ich eigentlich Color"));
        assert!(cand("mit Cloud Code plus MCP"));
        assert!(cand("Für APDO finde ich"));
        assert!(cand("Hypothesen zu Use Ps"));
        assert!(!cand("Ja, genau, das machen wir so."));
        assert!(!cand("Und dann hängt der Account halt jetzt so."));
        // Exact entries are not candidates by themselves.
        assert!(!cand("Kalasar"));
    }
}

//! Evidence for items: where in the transcript a task, decision or hook
//! item was said, and a quote that code checked is really there.
//!
//! The summary model gives no times or quotes yet, so for its tasks and
//! decisions code finds the turn that shares the most words with the item
//! (`evidence_origin = "match"`): a pointer for the reader, not proof. A
//! `<!-- t=mm:ss -->` on the item's line ("comment") or a hook's `--at` and
//! `--quote` ("hook") say where; code then checks the quote word for word.

use anyhow::{bail, Result};

/// A transcript turn, borrowed.
#[derive(Debug, Clone, Copy)]
pub struct TurnRef<'a> {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: &'a str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Evidence {
    pub at_ms: i64,
    /// A sentence of the turn; empty when none shares a word with the item.
    pub quote: String,
    /// The quote occurs in the transcript, word for word (case, spacing and
    /// punctuation aside).
    pub verified: bool,
    /// "comment", "match" or "hook".
    pub origin: &'static str,
}

/// Lower case, letters and digits only, single spaces.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with(' ') && !out.is_empty() {
            out.push(' ');
        }
    }
    out.trim_end().to_string()
}

/// The words that carry meaning: four letters or more.
fn words(s: &str) -> Vec<String> {
    normalize(s)
        .split(' ')
        .filter(|w| w.chars().count() >= 4)
        .map(str::to_string)
        .collect()
}

/// Two words match when one starts with the other (prüfen / prüft →
/// "prüf…" needs four letters in common).
fn same_word(a: &str, b: &str) -> bool {
    let common = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
    a == b || common >= 4
}

fn overlap(item: &[String], text: &str) -> usize {
    let tw = words(text);
    item.iter().filter(|w| tw.iter().any(|t| same_word(w, t))).count()
}

fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if matches!(c, '.' | '!' | '?') {
            let s = text[start..=i].trim();
            if !s.is_empty() {
                out.push(s);
            }
            start = i + c.len_utf8();
        }
    }
    let rest = text[start..].trim();
    if !rest.is_empty() {
        out.push(rest);
    }
    out
}

fn best_sentence(item: &[String], text: &str) -> Option<String> {
    let mut best: Option<(&str, usize)> = None;
    for s in sentences(text) {
        let n = overlap(item, s);
        if n > 0 && best.is_none_or(|(_, b)| n > b) {
            best = Some((s, n));
        }
    }
    best.map(|(s, _)| {
        let s: String = s.chars().take(240).collect();
        s
    })
}

/// Where `item` was said. With `at_ms` (a `<!-- t= -->`), the turn there;
/// else the turn sharing the most words with it (at least two, or all of
/// an item's words when it has fewer).
pub fn evidence(turns: &[TurnRef], item: &str, at_ms: Option<i64>) -> Option<Evidence> {
    let iw = words(item);
    if let Some(at) = at_ms {
        let turn = turns
            .iter()
            .find(|t| t.start_ms <= at && at < t.end_ms.max(t.start_ms + 1))
            .or_else(|| turns.iter().min_by_key(|t| (t.start_ms - at).abs()));
        let quote = turn.and_then(|t| best_sentence(&iw, t.text)).unwrap_or_default();
        return Some(Evidence {
            at_ms: at,
            verified: !quote.is_empty(),
            quote,
            origin: "comment",
        });
    }
    if iw.is_empty() {
        return None;
    }
    let need = iw.len().min(2);
    let mut best: Option<(&TurnRef, usize)> = None;
    for t in turns {
        let n = overlap(&iw, t.text);
        if n >= need && best.is_none_or(|(_, b)| n > b) {
            best = Some((t, n));
        }
    }
    let (t, _) = best?;
    let quote = best_sentence(&iw, t.text)?;
    Some(Evidence {
        at_ms: t.start_ms,
        verified: normalize(t.text).contains(&normalize(&quote)),
        quote,
        origin: "match",
    })
}

/// Checks a hook's quote: it must occur in a turn that overlaps
/// `at_ms ± slack_ms`. Returns that turn's start. The error says where the
/// quote is instead, if anywhere.
pub fn verify_quote(turns: &[TurnRef], quote: &str, at_ms: i64, slack_ms: i64) -> Result<i64> {
    let q = normalize(quote);
    if q.is_empty() {
        bail!("the quote is empty");
    }
    let near = |t: &TurnRef| t.start_ms <= at_ms + slack_ms && t.end_ms.max(t.start_ms) >= at_ms - slack_ms;
    if let Some(t) = turns.iter().find(|t| near(t) && normalize(t.text).contains(&q)) {
        return Ok(t.start_ms);
    }
    // A quote that spans two turns.
    for w in turns.windows(2) {
        if (near(&w[0]) || near(&w[1]))
            && normalize(&format!("{} {}", w[0].text, w[1].text)).contains(&q)
        {
            return Ok(w[0].start_ms);
        }
    }
    if let Some(t) = turns.iter().find(|t| normalize(t.text).contains(&q)) {
        bail!(
            "the quote is in the transcript at {}, not near {}: pass --at {}",
            crate::mmss(t.start_ms),
            crate::mmss(at_ms),
            crate::mmss(t.start_ms)
        );
    }
    bail!(
        "the quote is not in the transcript (checked word for word, case and punctuation aside): {quote:?}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turns() -> Vec<TurnRef<'static>> {
        vec![
            TurnRef { start_ms: 0, end_ms: 10_000, text: "Hallo zusammen. Heute geht es um die Herbstaktion." },
            TurnRef {
                start_ms: 10_000,
                end_ms: 30_000,
                text: "Ich prüfe bis Freitag das Angebot der Druckerei. Danach melde ich mich.",
            },
            TurnRef { start_ms: 30_000, end_ms: 40_000, text: "Und den Rahmenvertrag verlängern wir um ein Jahr!" },
        ]
    }

    #[test]
    fn a_task_finds_the_turn_where_it_was_said() {
        let e = evidence(&turns(), "Angebot der Druckerei prüfen", None).unwrap();
        assert_eq!(e.at_ms, 10_000);
        assert_eq!(e.quote, "Ich prüfe bis Freitag das Angebot der Druckerei.");
        assert!(e.verified);
        assert_eq!(e.origin, "match");
        let d = evidence(&turns(), "Rahmenvertrag um ein Jahr verlängern", None).unwrap();
        assert_eq!(d.at_ms, 30_000);
        // Nothing in common: no evidence rather than a wrong one.
        assert!(evidence(&turns(), "Website relaunchen", None).is_none());
    }

    #[test]
    fn a_time_comment_picks_the_turn() {
        let e = evidence(&turns(), "Angebot prüfen", Some(12_000)).unwrap();
        assert_eq!(e.at_ms, 12_000);
        assert_eq!(e.origin, "comment");
        assert!(e.quote.contains("Angebot"));
    }

    #[test]
    fn hook_quotes_are_checked_word_for_word() {
        let t = turns();
        assert_eq!(verify_quote(&t, "das angebot der Druckerei", 15_000, 60_000).unwrap(), 10_000);
        let far = verify_quote(&t, "Rahmenvertrag verlängern wir", 200_000, 60_000).unwrap_err();
        assert!(far.to_string().contains("--at 00:30"), "{far}");
        assert!(verify_quote(&t, "Vertrag kündigen", 15_000, 60_000).is_err());
        // Across two turns.
        assert!(verify_quote(&t, "melde ich mich und den Rahmenvertrag", 20_000, 60_000).is_ok());
    }

    #[test]
    fn normalizing_ignores_case_and_punctuation() {
        assert_eq!(normalize("  Das, ist   GUT!  "), "das ist gut");
    }
}

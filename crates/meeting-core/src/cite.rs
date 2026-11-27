//! Which transcript lines a summary point comes from, found without a
//! language model: the point's words are matched against every line,
//! weighted by how rare they are in the meeting.
//!
//! Measured on 11 meetings (`docs/research/summary-citations-2026-11.md`):
//! the cited lines fully state the point for 78–79 % of points and miss it
//! for 1–2 %; the model's own citations managed 59–60 % and 11–12 %. It
//! runs in well under a millisecond per meeting, so it works at view time,
//! on old notes and on points the person edited. A port of
//! `scripts/summary-citations/cite.py`; keep the two alike.

use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// At most this many lines per point.
const MAX_LINES: usize = 3;
/// A line is cited when it scores at least this share of the best line.
const RATIO: f64 = 0.6;
/// Below this score (about one rare word) a point gets no citation.
const FLOOR: f64 = 2.0;
/// Extra score for a task owner's own lines.
const OWNER_BONUS: f64 = 0.5;

const STOP: &str = "der die das den dem des ein eine einen einem einer eines und oder aber auch noch nur schon sehr mal dann wenn weil dass ob wie was wer wo \
ist sind war waren wird werden wurde hat haben hatte sein kann können soll sollen muss müssen will wollen mit von zu zum zur für auf aus bei \
nach über unter vor im in an am als es er sie wir ihr ich du man sich nicht kein keine so ja nein also ähm äh hm mhm okay ok genau gut eben halt \
the a an and or but also only very then if because that whether how what who where is are was were will would be been has have had can could should \
must want with of to for on from at by after over under before in as it he she we they you i not no so yes yeah um uh like just really \
this these those there here their them his her its our your my me us him all any some more most other such into than too";

/// One summary line with the transcript lines it comes from.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NoteCite {
    /// The line's index in the summary Markdown (0-based, split on '\n').
    pub line: u32,
    /// The cited lines' indexes in the plain transcript (split on '\n'),
    /// in transcript order.
    pub lines: Vec<u32>,
    /// Start of each cited line, in seconds (its "[mm:ss]" stamp).
    pub start_s: Vec<f64>,
}

/// Lower-cased content words: runs of letters, or numbers with their
/// decimal and thousands separators ("1.500", "12,5"). Short words and
/// common function words are dropped.
fn tokens(s: &str) -> Vec<String> {
    let stop: HashSet<&str> = STOP.split_whitespace().collect();
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        if c.is_ascii_digit() {
            while i < chars.len()
                && (chars[i].is_ascii_digit()
                    || ((chars[i] == '.' || chars[i] == ',')
                        && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit())))
            {
                i += 1;
            }
        } else if c.is_alphabetic() {
            while i < chars.len() && chars[i].is_alphabetic() {
                i += 1;
            }
        } else {
            i += 1;
            continue;
        }
        let w: String = chars[start..i].iter().collect::<String>().to_lowercase();
        let is_num = w.starts_with(|c: char| c.is_ascii_digit());
        if (is_num || w.chars().count() >= 3) && !stop.contains(w.as_str()) {
            out.push(w);
        }
    }
    out
}

/// Same word, or one a prefix, suffix or (from 6 letters) part of the other:
/// inflections and compounds ("Druckerei" / "Druckereien", "Budget" /
/// "Werbebudget"). Numbers only match exactly.
fn same(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let digit = |s: &str| s.starts_with(|c: char| c.is_ascii_digit());
    if digit(a) || digit(b) {
        return false;
    }
    let (s, l) = if a.chars().count() <= b.chars().count() {
        (a, b)
    } else {
        (b, a)
    };
    let n = s.chars().count();
    n >= 4 && (l.starts_with(s) || l.ends_with(s) || (n >= 6 && l.contains(s)))
}

/// For each point (text, task owner), the indexes of the lines it comes
/// from (`lines`: speaker and text per transcript segment).
pub fn cite_points(points: &[(String, Option<String>)], lines: &[(&str, &str)]) -> Vec<Vec<usize>> {
    let ltoks: Vec<HashSet<String>> = lines
        .iter()
        .map(|(_, t)| tokens(t).into_iter().collect())
        .collect();
    let n = lines.len();
    let mut df: HashMap<&str, usize> = HashMap::new();
    for ts in &ltoks {
        for t in ts {
            *df.entry(t.as_str()).or_default() += 1;
        }
    }
    let idf = |t: &str| -> f64 {
        let d = match df.get(t) {
            Some(&d) => d,
            None => df.iter().filter(|(u, _)| same(t, u)).map(|(_, c)| c).sum(),
        };
        (1.0 + n as f64 / (1.0 + d as f64)).ln()
    };
    points
        .iter()
        .map(|(text, owner)| {
            // Sorted, so the scores are summed in the same order every run.
            let mut ct: Vec<String> = tokens(text);
            ct.sort_unstable();
            ct.dedup();
            let w: Vec<(&String, f64)> = ct.iter().map(|t| (t, idf(t))).collect();
            let owner = owner.as_deref().map(|o| o.trim().to_lowercase());
            let scores: Vec<f64> = ltoks
                .iter()
                .enumerate()
                .map(|(i, ts)| {
                    let mut s: f64 = w
                        .iter()
                        .filter(|(t, _)| ts.iter().any(|u| same(t, u)))
                        .map(|(_, x)| x)
                        .sum();
                    if owner
                        .as_deref()
                        .is_some_and(|o| lines[i].0.trim().to_lowercase() == o)
                    {
                        s += OWNER_BONUS;
                    }
                    s
                })
                .collect();
            let best = scores.iter().cloned().fold(0.0, f64::max);
            if best < FLOOR {
                return Vec::new();
            }
            let mut order: Vec<usize> = (0..n).collect();
            // Stable: on a tie the earlier line wins, as in the Python bench.
            order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]));
            let mut top: Vec<usize> = order
                .into_iter()
                .take(MAX_LINES)
                .filter(|&i| scores[i] >= RATIO * best)
                .collect();
            top.sort_unstable();
            top
        })
        .collect()
}

/// One summary line as a point: its text and, for a task ("- [ ]
/// **Owner**: text"), its owner. `None` for headings, blank lines and
/// anything that is not a list item.
fn point_of(line: &str) -> Option<(String, Option<String>)> {
    let t = line.trim_start();
    let rest = t
        .strip_prefix("- ")
        .or_else(|| t.strip_prefix("* "))?
        .trim_start();
    let rest = rest
        .strip_prefix("[ ] ")
        .or_else(|| rest.strip_prefix("[x] "))
        .or_else(|| rest.strip_prefix("[X] "))
        .unwrap_or(rest);
    if let Some((owner, text)) = rest.strip_prefix("**").and_then(|r| r.split_once("**:")) {
        return Some((text.trim().to_string(), Some(owner.trim().to_string())));
    }
    let text = rest.trim();
    (!text.is_empty()).then(|| (text.to_string(), None))
}

/// "[01:02:03] Anna: Hi." → (3723 s, "Anna", "Hi."), read like
/// [`crate::note::transcript_from_note`] reads a note's lines.
fn parse_line(line: &str) -> Option<(f64, &str, &str)> {
    let (time, rest) = line.trim().strip_prefix('[')?.split_once("] ")?;
    let (speaker, text) = rest.split_once(": ")?;
    let mut s = 0.0;
    for part in time.split(':') {
        s = s * 60.0 + part.parse::<u32>().ok()? as f64;
    }
    (!time.is_empty() && !text.trim().is_empty()).then_some((s, speaker.trim(), text.trim()))
}

/// Citations for every list item of a note's summary Markdown, against the
/// note's plain transcript ("[00:03] Speaker 2: Hi." lines,
/// [`crate::note::split_note`]). Items without a match are left out.
pub fn cite_note(summary_md: &str, transcript: &str) -> Vec<NoteCite> {
    let parsed: Vec<(u32, (f64, &str, &str))> = transcript
        .split('\n')
        .enumerate()
        .filter_map(|(i, l)| parse_line(l).map(|p| (i as u32, p)))
        .collect();
    let lines: Vec<(&str, &str)> = parsed.iter().map(|(_, (_, who, t))| (*who, *t)).collect();
    let items: Vec<(u32, (String, Option<String>))> = summary_md
        .split('\n')
        .enumerate()
        .filter_map(|(i, l)| point_of(l).map(|p| (i as u32, p)))
        .collect();
    let points: Vec<(String, Option<String>)> = items.iter().map(|(_, p)| p.clone()).collect();
    cite_points(&points, &lines)
        .into_iter()
        .zip(items)
        .filter(|(c, _)| !c.is_empty())
        .map(|(c, (line, _))| NoteCite {
            line,
            lines: c.iter().map(|&i| parsed[i].0).collect(),
            start_s: c.iter().map(|&i| parsed[i].1 .0).collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINES: &[(&str, &str)] = &[
        ("Mara", "Guten Morgen zusammen, wie war euer Wochenende?"),
        ("Lena", "Die Herbstaktion startet am 6. Oktober, das steht jetzt fest."),
        ("Jonas", "Budget sind 4.000 Euro, davon 1.500 für Anzeigen."),
        ("Mara", "Und wann muss das alles bei der Druckerei sein?"),
        ("Lena", "Die Druckerei braucht die Dateien bis Freitag zwölf Uhr, sonst liefern sie erst im November."),
        ("Jonas", "Ich hole ein Angebot von der Agentur für die Fotos, bis Mittwoch."),
        ("Lena", "Okay."),
    ];

    fn cite(text: &str, owner: Option<&str>) -> Vec<usize> {
        cite_points(&[(text.to_string(), owner.map(String::from))], LINES).remove(0)
    }

    #[test]
    fn finds_the_line_with_the_rare_words() {
        assert_eq!(
            cite(
                "Die Druckerei braucht die Dateien bis Freitag 12 Uhr.",
                None
            ),
            vec![4]
        );
        assert_eq!(
            cite("Budget 4.000 €, davon 1.500 € für Anzeigen.", None),
            vec![2]
        );
        assert_eq!(cite("Start der Herbstaktion: 6. Oktober.", None), vec![1]);
    }

    #[test]
    fn inflections_and_compounds_match() {
        assert!(same("druckerei", "druckereien"));
        assert!(same("budget", "werbebudget"));
        assert!(!same("1.500", "1.5000"));
        assert!(
            !same("tag", "tage"),
            "words under 4 letters only match exactly"
        );
    }

    #[test]
    fn the_owner_breaks_a_tie() {
        assert_eq!(
            cite(
                "Angebot der Agentur für die Fotos, bis Mittwoch",
                Some("Jonas")
            ),
            vec![5]
        );
    }

    #[test]
    fn a_point_with_no_shared_words_gets_no_citation() {
        assert!(cite("Wetter war schön.", None).is_empty());
    }

    #[test]
    fn note_lines_map_to_transcript_lines() {
        // A line that is not a turn keeps its place in the count.
        let mut t = String::from("_Keine Sprache erkannt am Anfang._");
        for (i, (who, text)) in LINES.iter().enumerate() {
            t.push_str(&format!(
                "\n[{:02}:{:02}] {who}: {text}",
                i * 70 / 60,
                i * 70 % 60
            ));
        }
        let md = "## Zusammenfassung\n\n- Die Druckerei braucht die Dateien bis Freitag 12 Uhr.\n- Wetter war schön.\n\n## Aufgaben\n\n- [ ] **Jonas**: Angebot der Agentur für die Fotos, bis Mittwoch";
        assert_eq!(
            cite_note(md, &t),
            vec![
                NoteCite {
                    line: 2,
                    lines: vec![5],
                    start_s: vec![280.0]
                },
                NoteCite {
                    line: 7,
                    lines: vec![6],
                    start_s: vec![350.0]
                },
            ]
        );
    }

    #[test]
    fn hour_stamps_parse() {
        assert_eq!(
            parse_line("[1:02:03] Anna: Hi."),
            Some((3723.0, "Anna", "Hi."))
        );
        assert_eq!(parse_line("[00:03] Anna: "), None);
        assert_eq!(parse_line("no stamp"), None);
    }
}

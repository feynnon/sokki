//! The person's own notes, typed during (or after) a meeting.
//!
//! Two places, as agreed with the second-brain concept (2026-11-26):
//!
//! 1. Raw: `meetings/<id>/notes.md` in the app's data, word for word what
//!    was typed. Each line ends with `<!-- t=hh:mm:ss -->`, the offset into
//!    the recording when the line was started (left out without one).
//!
//!    ```text
//!    ---
//!    id: 2026-11-26_10-00-03
//!    source: own-notes
//!    started_at: 2026-11-26T10:00:03+01:00
//!    ---
//!    Budget? <!-- t=00:03:12 -->
//!    ```
//!
//! 2. Combined: a section in the meeting note, before the summary. A
//!    top-level item is the person's own line, copied by code; a nested
//!    item is Sokki's addition from the transcript. The note's front
//!    matter gains `id:` and `own_notes: true`.
//!
//!    ```text
//!    ## Meine Notizen
//!
//!    - Budget?
//!      - 4.000 € gesamt, davon 1.500 € für Anzeigen
//!    ```

use crate::lang::Language;

/// One typed line.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NoteLine {
    pub text: String,
    /// Seconds into the recording when the line was started.
    pub t: Option<u32>,
}

/// One of the person's lines with what Sokki adds to it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NoteBlock {
    pub line: String,
    pub added: Vec<String>,
}

pub const FILE_NAME: &str = "notes.md";

const HEADING_DE: &str = "Meine Notizen";
const HEADING_EN: &str = "My notes";

fn hms(t: u32) -> String {
    format!("{:02}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60)
}

fn parse_hms(s: &str) -> Option<u32> {
    let p: Vec<u32> = s
        .split(':')
        .map(|x| x.trim().parse().ok())
        .collect::<Option<_>>()?;
    match p.as_slice() {
        [h, m, s] => Some(h * 3600 + m * 60 + s),
        [m, s] => Some(m * 60 + s),
        _ => None,
    }
}

/// Splits `<!-- t=… -->` off the end of a line.
fn split_time(line: &str) -> (&str, Option<u32>) {
    let l = line.trim_end();
    if let Some(start) = l.rfind("<!-- t=") {
        if let Some(inner) = l[start + 7..].strip_suffix("-->") {
            if let Some(t) = parse_hms(inner.trim()) {
                return (l[..start].trim_end(), Some(t));
            }
        }
    }
    (l, None)
}

/// The body of a Markdown file without its front matter.
fn body(md: &str) -> &str {
    md.strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n").map(|(_, b)| b))
        .unwrap_or(md)
}

/// Reads `notes.md`. Every line, empty ones included, so the editor gets
/// back exactly what was typed.
pub fn parse(md: &str) -> Vec<NoteLine> {
    let b = body(md);
    let b = b.strip_suffix('\n').unwrap_or(b);
    if b.is_empty() {
        return Vec::new();
    }
    b.split('\n')
        .map(|l| {
            let (text, t) = split_time(l);
            NoteLine {
                text: text.to_string(),
                t,
            }
        })
        .collect()
}

/// Writes `notes.md`.
pub fn render(id: &str, started_at: Option<&str>, lines: &[NoteLine]) -> String {
    let mut out = format!("---\nid: {id}\nsource: own-notes\n");
    if let Some(s) = started_at {
        out.push_str(&format!("started_at: {s}\n"));
    }
    out.push_str("---\n");
    for l in lines {
        let text = l.text.replace('\n', " ");
        match l.t {
            Some(t) if !text.trim().is_empty() => {
                out.push_str(&format!("{text} <!-- t={} -->\n", hms(t)))
            }
            _ => {
                out.push_str(&text);
                out.push('\n');
            }
        }
    }
    out
}

/// The lines that count: not empty, list markers removed (the section
/// puts its own).
pub fn content_lines(lines: &[NoteLine]) -> Vec<String> {
    lines
        .iter()
        .map(|l| strip_marker(l.text.trim()).to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

fn strip_marker(s: &str) -> &str {
    for m in ["- [ ] ", "- [x] ", "- ", "* ", "• ", "– "] {
        if let Some(r) = s.strip_prefix(m) {
            return r.trim();
        }
    }
    s
}

/// Keeps each line's time when its text is unchanged, else the time of the
/// line at the same place; a new line gets `now` (seconds into the
/// recording, `None` without one).
pub fn merge_times(old: &[NoteLine], texts: &[&str], now: Option<u32>) -> Vec<NoteLine> {
    let mut used = vec![false; old.len()];
    // First the lines whose text is unchanged, wherever they moved.
    let mut times: Vec<Option<Option<u32>>> = texts
        .iter()
        .map(|text| {
            if text.trim().is_empty() {
                return Some(None);
            }
            let j =
                (0..old.len()).find(|&j| !used[j] && old[j].text == *text && old[j].t.is_some())?;
            used[j] = true;
            Some(old[j].t)
        })
        .collect();
    // Then a changed line keeps the time of the line at its place.
    for (i, slot) in times.iter_mut().enumerate() {
        if slot.is_none() {
            let keep = old
                .get(i)
                .filter(|o| !used[i] && !o.text.trim().is_empty() && o.t.is_some());
            *slot = Some(match keep {
                Some(o) => {
                    used[i] = true;
                    o.t
                }
                None => now,
            });
        }
    }
    texts
        .iter()
        .zip(times)
        .map(|(text, t)| NoteLine {
            text: text.to_string(),
            t: t.flatten(),
        })
        .collect()
}

fn heading(lang: Language) -> &'static str {
    match lang {
        Language::German => HEADING_DE,
        Language::English => HEADING_EN,
    }
}

fn is_heading(line: &str) -> bool {
    let h = line.trim_end();
    h == format!("## {HEADING_DE}") || h == format!("## {HEADING_EN}")
}

/// The note's language, read from its headings.
fn note_lang(md: &str) -> Language {
    let english = md.lines().any(|l| {
        matches!(
            l.trim_end(),
            "## Summary"
                | "## Transcript"
                | "## Decisions"
                | "## Action items"
                | "_No summary was created._"
        )
    });
    if english {
        Language::English
    } else {
        Language::German
    }
}

/// The section's Markdown, heading included.
pub fn render_section(blocks: &[NoteBlock], lang: Language) -> String {
    let mut out = format!("## {}\n\n", heading(lang));
    for b in blocks {
        out.push_str(&format!("- {}\n", b.line.replace('\n', " ")));
        for a in &b.added {
            let a = a.trim();
            if !a.is_empty() {
                out.push_str(&format!("  - {}\n", a.replace('\n', " ")));
            }
        }
    }
    out
}

/// Where the section ends: at the next heading, or at a top-level line
/// that isn't a list item ("_Keine Zusammenfassung erstellt._").
fn section_end(lines: &[&str], from: usize) -> usize {
    let mut i = from;
    while i < lines.len() {
        let l = lines[i];
        if l.starts_with("## ") || l.starts_with("# ") {
            return i;
        }
        let t = l.trim();
        if !t.is_empty() && !l.starts_with("- ") && !l.starts_with("  ") {
            return i;
        }
        i += 1;
    }
    i
}

/// Puts the section into a meeting note (replacing an earlier one), right
/// after its title, and adds `id:` and `own_notes: true` to its front
/// matter. Leaves a note without blocks unchanged.
pub fn set_section(md: &str, id: &str, blocks: &[NoteBlock]) -> String {
    if blocks.is_empty() {
        return md.to_string();
    }
    let lang = note_lang(md);
    let (front, rest) = match md
        .strip_prefix("---\n")
        .and_then(|r| r.split_once("\n---\n"))
    {
        Some((f, b)) => (Some(f.to_string()), b.to_string()),
        None => (None, md.to_string()),
    };
    let front = front.map(|f| {
        let mut f = f;
        if !f.lines().any(|l| l.starts_with("id:")) {
            f.push_str(&format!("\nid: {id}"));
        }
        if !f.lines().any(|l| l.starts_with("own_notes:")) {
            f.push_str("\nown_notes: true");
        }
        f
    });
    let lines: Vec<&str> = rest.split('\n').collect();
    // Drop an earlier section.
    let mut kept: Vec<&str> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        if is_heading(lines[i]) {
            i = section_end(&lines, i + 1);
            continue;
        }
        kept.push(lines[i]);
        i += 1;
    }
    let section = render_section(blocks, lang);
    let at = kept
        .iter()
        .position(|l| l.starts_with("# "))
        .map(|p| {
            // After the title and the blank line under it.
            let mut q = p + 1;
            while q < kept.len() && kept[q].trim().is_empty() {
                q += 1;
            }
            q
        })
        .unwrap_or(0);
    let mut out = String::new();
    if let Some(f) = front {
        out.push_str("---\n");
        out.push_str(&f);
        out.push_str("\n---\n");
    }
    let before = kept[..at].join("\n");
    let after = kept[at..].join("\n");
    out.push_str(&before);
    if !before.is_empty() && !before.ends_with('\n') {
        out.push('\n');
    }
    if !before.is_empty() && !before.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str(&section);
    out.push('\n');
    out.push_str(&after);
    out
}

/// The section's Markdown (without its heading), empty when the note has
/// none.
pub fn section_of(md: &str) -> String {
    let lines: Vec<&str> = body(md).split('\n').collect();
    match lines.iter().position(|l| is_heading(l)) {
        Some(h) => {
            let end = section_end(&lines, h + 1);
            lines[h + 1..end].join("\n").trim().to_string()
        }
        None => String::new(),
    }
}

/// Whether a line of a note belongs to the section; for [`crate::note::split_note`].
pub(crate) fn section_ranges(lines: &[&str]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if is_heading(lines[i]) {
            let end = section_end(lines, i + 1);
            out.push((i, end));
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_file_round_trips() {
        let lines = vec![
            NoteLine {
                text: "Budget?".into(),
                t: Some(192),
            },
            NoteLine {
                text: String::new(),
                t: None,
            },
            NoteLine {
                text: "Druckerei bis Fr".into(),
                t: Some(3_700),
            },
            NoteLine {
                text: "ohne Zeit".into(),
                t: None,
            },
        ];
        let md = render(
            "2026-11-26_10-00-03",
            Some("2026-11-26T10:00:03+01:00"),
            &lines,
        );
        assert!(md.contains("Budget? <!-- t=00:03:12 -->\n"));
        assert!(md.contains("Druckerei bis Fr <!-- t=01:01:40 -->\n"));
        assert!(md.starts_with("---\nid: 2026-11-26_10-00-03\nsource: own-notes\n"));
        assert_eq!(parse(&md), lines);
    }

    #[test]
    fn empty_file_has_no_lines() {
        assert!(parse(&render("x", None, &[])).is_empty());
    }

    #[test]
    fn times_follow_unchanged_lines() {
        let old = vec![
            NoteLine {
                text: "a".into(),
                t: Some(10),
            },
            NoteLine {
                text: "b".into(),
                t: Some(20),
            },
        ];
        // A line inserted on top: "a" and "b" keep their times.
        let m = merge_times(&old, &["neu", "a", "b"], Some(99));
        assert_eq!(
            m.iter().map(|l| l.t).collect::<Vec<_>>(),
            vec![Some(99), Some(10), Some(20)]
        );
        // A line being typed keeps the time it started with.
        let m = merge_times(&old, &["a", "bc"], Some(99));
        assert_eq!(m[1].t, Some(20));
        // Empty lines carry none.
        let m = merge_times(&[], &["", "x"], Some(5));
        assert_eq!(m[0].t, None);
        assert_eq!(m[1].t, Some(5));
    }

    #[test]
    fn content_lines_drop_markers_and_blanks() {
        let lines: Vec<NoteLine> = ["- Budget?", "", "  ", "• Fotos", "Druck"]
            .iter()
            .map(|t| NoteLine {
                text: t.to_string(),
                t: None,
            })
            .collect();
        assert_eq!(content_lines(&lines), vec!["Budget?", "Fotos", "Druck"]);
    }

    const NOTE: &str = "---\ntype: meeting\nparticipants:\n  - Ich\ntags: [meeting]\n---\n\n# Herbstaktion\n\n## Zusammenfassung\n\n- Start am 6. Oktober\n\n## Transkript\n\n**[00:00] Ich:** Hallo.\n";

    fn blocks() -> Vec<NoteBlock> {
        vec![
            NoteBlock {
                line: "Budget?".into(),
                added: vec!["4.000 € gesamt".into()],
            },
            NoteBlock {
                line: "Druckerei bis Fr".into(),
                added: vec![],
            },
        ]
    }

    #[test]
    fn section_goes_after_the_title() {
        let md = set_section(NOTE, "m1", &blocks());
        assert!(md.contains("own_notes: true\n---\n"), "{md}");
        assert!(md.contains("\nid: m1\n"));
        let want = "# Herbstaktion\n\n## Meine Notizen\n\n- Budget?\n  - 4.000 € gesamt\n- Druckerei bis Fr\n\n## Zusammenfassung\n";
        assert!(md.contains(want), "{md}");
        assert_eq!(
            section_of(&md),
            "- Budget?\n  - 4.000 € gesamt\n- Druckerei bis Fr"
        );
        // Again: replaced, not added twice; front matter keys once.
        let again = set_section(
            &md,
            "m1",
            &[NoteBlock {
                line: "Neu".into(),
                added: vec![],
            }],
        );
        assert_eq!(again.matches("## Meine Notizen").count(), 1);
        assert_eq!(again.matches("id: m1").count(), 1);
        assert_eq!(section_of(&again), "- Neu");
        assert!(again.contains("## Zusammenfassung\n\n- Start am 6. Oktober"));
    }

    #[test]
    fn section_before_the_no_summary_line() {
        let note = "---\ntype: meeting\n---\n\n# Meeting\n\n_Keine Zusammenfassung erstellt._\n\n## Transkript\n\n**[00:00] Ich:** Hallo.\n";
        let md = set_section(note, "m2", &blocks());
        assert!(
            md.contains("- Druckerei bis Fr\n\n_Keine Zusammenfassung erstellt._"),
            "{md}"
        );
        assert_eq!(
            section_of(&md),
            "- Budget?\n  - 4.000 € gesamt\n- Druckerei bis Fr"
        );
    }

    #[test]
    fn english_note_gets_english_heading() {
        let note = NOTE
            .replace("## Zusammenfassung", "## Summary")
            .replace("## Transkript", "## Transcript");
        assert!(set_section(&note, "m", &blocks()).contains("## My notes\n"));
    }

    #[test]
    fn split_note_keeps_the_section_out_of_the_summary() {
        let md = set_section(NOTE, "m1", &blocks());
        let parts = crate::note::split_note(&md);
        assert_eq!(parts.summary, "## Zusammenfassung\n\n- Start am 6. Oktober");
        assert_eq!(
            parts.own_notes,
            "- Budget?\n  - 4.000 € gesamt\n- Druckerei bis Fr"
        );
        assert_eq!(parts.transcript, "[00:00] Ich: Hallo.");
    }

    #[test]
    fn no_blocks_no_change() {
        assert_eq!(set_section(NOTE, "m", &[]), NOTE);
    }
}

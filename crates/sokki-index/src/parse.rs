//! Reads a meeting note back (`meeting_core::note` writes it), including
//! what the person edited in Obsidian, as long as the headings stay.

use crate::time::parse_mmss;

/// The note's frontmatter fields the index uses.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Front {
    pub id: Option<String>,
    pub kind: Option<String>,
    pub source_app: Option<String>,
    pub private: bool,
    pub participants: Vec<String>,
}

/// What a section is, by its heading in either language.
#[derive(Debug, Clone, PartialEq)]
pub enum SectionKind {
    Summary,
    Decisions,
    Tasks,
    Transcript,
    /// The person's own notes ("Meine Notizen", own-notes session).
    Notes,
    /// Anything else, by its heading (e.g. a section a hook appended).
    Other(String),
}

impl SectionKind {
    pub fn from_heading(h: &str) -> SectionKind {
        match h.trim().to_lowercase().as_str() {
            "zusammenfassung" | "summary" => SectionKind::Summary,
            "entscheidungen" | "decisions" => SectionKind::Decisions,
            "aufgaben" | "action items" | "tasks" => SectionKind::Tasks,
            "transkript" | "transcript" => SectionKind::Transcript,
            "meine notizen" | "my notes" | "notizen" | "notes" => SectionKind::Notes,
            _ => SectionKind::Other(h.trim().to_string()),
        }
    }

    /// The `--section` name.
    pub fn name(&self) -> String {
        match self {
            SectionKind::Summary => "summary".into(),
            SectionKind::Decisions => "decisions".into(),
            SectionKind::Tasks => "tasks".into(),
            SectionKind::Transcript => "transcript".into(),
            SectionKind::Notes => "notes".into(),
            SectionKind::Other(h) => h.to_lowercase(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub heading: String,
    pub kind: SectionKind,
    /// The lines under the heading, footer and blank edges removed.
    pub lines: Vec<String>,
}

/// One paragraph of the transcript: `**[mm:ss] Name:** text`.
#[derive(Debug, Clone, PartialEq)]
pub struct Para {
    pub start_ms: i64,
    pub speaker: String,
    pub text: String,
}

/// One task line: `- [ ] **Owner**: text <!-- t=12:04 -->`.
#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub owner: Option<String>,
    pub text: String,
    pub done: bool,
    pub at_ms: Option<i64>,
}

/// A bullet or paragraph with its optional `<!-- t=… -->`, and whether it
/// is nested (in own notes: Sokki's addition, not the person's line).
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub at_ms: Option<i64>,
    pub nested: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Note {
    pub front: Front,
    pub title: Option<String>,
    pub sections: Vec<Section>,
}

impl Note {
    pub fn section(&self, kind: &SectionKind) -> Option<&Section> {
        self.sections.iter().find(|s| &s.kind == kind)
    }

    pub fn transcript(&self) -> Vec<Para> {
        self.section(&SectionKind::Transcript)
            .map(|s| paragraphs(&s.lines))
            .unwrap_or_default()
    }

    pub fn tasks(&self) -> Vec<Task> {
        self.section(&SectionKind::Tasks)
            .map(|s| s.lines.iter().filter_map(|l| task(l)).collect())
            .unwrap_or_default()
    }

    pub fn decisions(&self) -> Vec<Line> {
        self.section(&SectionKind::Decisions)
            .map(|s| bullets(&s.lines))
            .unwrap_or_default()
    }
}

/// Parses a whole note.
pub fn note(md: &str) -> Note {
    let mut out = Note::default();
    let mut body = md;
    if let Some(rest) = md.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---\n").or_else(|| rest.strip_suffix("\n---").map(|r| r.len())) {
            out.front = front(&rest[..end]);
            body = rest.get(end + 5..).unwrap_or("");
        }
    }
    let mut current: Option<Section> = None;
    for line in body.lines() {
        if let Some(t) = line.strip_prefix("# ") {
            if out.title.is_none() && current.is_none() {
                out.title = Some(t.trim().to_string());
                continue;
            }
        }
        if let Some(h) = line.strip_prefix("## ") {
            if let Some(s) = current.take() {
                out.sections.push(finish(s));
            }
            current = Some(Section {
                heading: h.trim().to_string(),
                kind: SectionKind::from_heading(h),
                lines: Vec::new(),
            });
            continue;
        }
        if meeting_core::note::is_footer(line) {
            continue;
        }
        if let Some(s) = current.as_mut() {
            s.lines.push(line.to_string());
        }
    }
    if let Some(s) = current.take() {
        out.sections.push(finish(s));
    }
    out
}

fn finish(mut s: Section) -> Section {
    while s.lines.last().is_some_and(|l| l.trim().is_empty()) {
        s.lines.pop();
    }
    while s.lines.first().is_some_and(|l| l.trim().is_empty()) {
        s.lines.remove(0);
    }
    s
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        v[1..v.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\")
    } else if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') {
        v[1..v.len() - 1].replace("''", "'")
    } else {
        v.to_string()
    }
}

fn front(text: &str) -> Front {
    let mut f = Front::default();
    let mut in_participants = false;
    for line in text.lines() {
        if in_participants {
            if let Some(p) = line.trim_start().strip_prefix("- ") {
                if line.starts_with(' ') || line.starts_with('-') {
                    f.participants.push(unquote(p));
                    continue;
                }
            }
            in_participants = false;
        }
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let v = v.trim();
        match k.trim() {
            "id" => f.id = Some(unquote(v)).filter(|s| !s.is_empty()),
            "type" => f.kind = Some(unquote(v)),
            "source_app" => f.source_app = Some(unquote(v)).filter(|s| !s.is_empty()),
            "private" => f.private = matches!(unquote(v).to_lowercase().as_str(), "true" | "yes" | "on"),
            "participants" => {
                if v.is_empty() {
                    in_participants = true;
                } else if let Some(list) = v.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
                    f.participants = list
                        .split(',')
                        .map(unquote)
                        .filter(|s| !s.is_empty())
                        .collect();
                }
            }
            _ => {}
        }
    }
    f
}

/// `<!-- t=00:03:20 -->` at the end of a line: the text without it, and
/// the time.
pub fn strip_time_comment(line: &str) -> (String, Option<i64>) {
    let t = line.trim_end();
    if let Some(start) = t.rfind("<!--") {
        // `start + 4 <= len - 3`: "<!-->" and "<!--->" are no time comment.
        if t.ends_with("-->") && start + 4 <= t.len() - 3 {
            let inner = t[start + 4..t.len() - 3].trim();
            if let Some(v) = inner.strip_prefix("t=") {
                if let Ok(ms) = parse_mmss(v.trim()) {
                    return (t[..start].trim_end().to_string(), Some(ms));
                }
            }
        }
    }
    (t.to_string(), None)
}

/// `**[mm:ss] Name:** text`, with lines that continue a paragraph joined
/// to it.
pub fn paragraphs(lines: &[String]) -> Vec<Para> {
    let mut out: Vec<Para> = Vec::new();
    for line in lines {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        if let Some(p) = para(l) {
            out.push(p);
        } else if let Some(last) = out.last_mut() {
            last.text.push(' ');
            last.text.push_str(l);
        }
    }
    out
}

fn para(l: &str) -> Option<Para> {
    let rest = l.strip_prefix("**[")?;
    let (time, rest) = rest.split_once("] ")?;
    let (speaker, text) = rest.split_once(":**")?;
    let start_ms = parse_mmss(time).ok()?;
    Some(Para {
        start_ms,
        speaker: speaker.trim().to_string(),
        text: text.trim().to_string(),
    })
}

fn task(line: &str) -> Option<Task> {
    let l = line.trim_start();
    let (done, rest) = if let Some(r) = l.strip_prefix("- [ ] ") {
        (false, r)
    } else if let Some(r) = l.strip_prefix("- [x] ").or_else(|| l.strip_prefix("- [X] ")) {
        (true, r)
    } else {
        return None;
    };
    let (rest, at_ms) = strip_time_comment(rest);
    let (owner, text) = match rest
        .strip_prefix("**")
        .and_then(|r| r.split_once("**:"))
    {
        Some((o, t)) => (Some(o.trim().to_string()), t.trim().to_string()),
        None => (None, rest.trim().to_string()),
    };
    (!text.is_empty()).then_some(Task {
        owner,
        text,
        done,
        at_ms,
    })
}

/// Bullets (`- `, `* `, `1. `) and plain paragraphs as lines; a bullet's
/// wrapped continuation joins it.
pub fn bullets(lines: &[String]) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    let mut open = false;
    for raw in lines {
        if raw.trim().is_empty() {
            open = false;
            continue;
        }
        let indent = raw.len() - raw.trim_start().len();
        let l = raw.trim();
        let item = l
            .strip_prefix("- ")
            .or_else(|| l.strip_prefix("* "))
            .or_else(|| {
                let digits = l.chars().take_while(|c| c.is_ascii_digit()).count();
                (digits > 0).then(|| l[digits..].strip_prefix(". ")).flatten()
            });
        match item {
            Some(text) => {
                let (text, at_ms) = strip_time_comment(text);
                out.push(Line {
                    text,
                    at_ms,
                    nested: indent >= 2,
                });
                open = true;
            }
            None if open => {
                let (text, at) = strip_time_comment(l);
                if let Some(last) = out.last_mut() {
                    last.text.push(' ');
                    last.text.push_str(&text);
                    last.at_ms = last.at_ms.or(at);
                }
            }
            None => {
                let (text, at_ms) = strip_time_comment(l);
                out.push(Line {
                    text,
                    at_ms,
                    nested: false,
                });
                open = true;
            }
        }
    }
    out.retain(|l| !l.text.trim().is_empty());
    out
}

/// The person's own notes (`meetings/<id>/notes.md`, own-notes session):
/// frontmatter, then one line per note with `<!-- t=hh:mm:ss -->`.
pub fn own_notes(md: &str) -> (Front, Option<String>, Vec<Line>) {
    let mut body = md;
    let mut f = Front::default();
    let mut title = None;
    if let Some(rest) = md.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---\n") {
            let head = &rest[..end];
            f = front(head);
            title = head
                .lines()
                .find_map(|l| l.strip_prefix("title:"))
                .map(unquote)
                .filter(|t| !t.is_empty());
            body = &rest[end + 5..];
        }
    }
    // One note per line, word for word; an indented bullet is Sokki's
    // addition to the line above.
    let lines = body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|raw| {
            let indent = raw.len() - raw.trim_start().len();
            let l = raw.trim();
            let l = l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")).unwrap_or(l);
            let (text, at_ms) = strip_time_comment(l);
            Line {
                text,
                at_ms,
                nested: indent >= 2,
            }
        })
        .filter(|l| !l.text.is_empty())
        .collect();
    (f, title, lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTE: &str = "---\ntype: meeting\nid: meeting-20261103-100000-001\ndate: 2026-11-03\nstart: \"10:00\"\nduration: 12 min\nparticipants:\n  - Ich\n  - Speaker 2\n  - \"Jana Beispiel\"\nsource_app: Zoom\ntags: [meeting]\n---\n\n# Rahmenvertrag mit der Druckerei\n\n## Zusammenfassung\n\n- Der Rahmenvertrag läuft bis März.\n- Die Kündigungsfrist beträgt drei Monate.\n\n## Entscheidungen\n\n- Wir verlängern den Rahmenvertrag.\n\n## Aufgaben\n\n- [ ] **Jana Beispiel**: Angebot der Druckerei prüfen <!-- t=01:10 -->\n- [x] **Ich**: Termin eintragen\n- [ ] Ohne Besitzer\n\n## CRM\n\nKunde: Druckerei Muster\n\n## Transkript\n\n**[00:00] Ich:** Hallo zusammen.\n\n**[01:05] Jana Beispiel:** Der Rahmenvertrag läuft bis März,\nund danach sehen wir weiter.\n\n**[1:02:05] Speaker 2:** Gut.\n\nVon KI zusammengefasst. Lokal, kostenlos, Open Source. Diktier-App und Meeting-Notetaker: sokki.org\n";

    #[test]
    fn reads_every_part_of_a_note() {
        let n = note(NOTE);
        assert_eq!(n.front.id.as_deref(), Some("meeting-20261103-100000-001"));
        assert_eq!(n.front.source_app.as_deref(), Some("Zoom"));
        assert_eq!(n.front.participants, vec!["Ich", "Speaker 2", "Jana Beispiel"]);
        assert!(!n.front.private);
        assert_eq!(n.title.as_deref(), Some("Rahmenvertrag mit der Druckerei"));
        let t = n.transcript();
        assert_eq!(t.len(), 3);
        assert_eq!(t[1].start_ms, 65_000);
        assert_eq!(t[1].speaker, "Jana Beispiel");
        assert_eq!(t[1].text, "Der Rahmenvertrag läuft bis März, und danach sehen wir weiter.");
        assert_eq!(t[2].start_ms, 3_725_000);
        assert!(!t[2].text.contains("sokki.org"), "the footer is not transcript");
        let tasks = n.tasks();
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].owner.as_deref(), Some("Jana Beispiel"));
        assert_eq!(tasks[0].text, "Angebot der Druckerei prüfen");
        assert_eq!(tasks[0].at_ms, Some(70_000));
        assert!(tasks[1].done);
        assert_eq!(tasks[2].owner, None);
        assert_eq!(n.decisions()[0].text, "Wir verlängern den Rahmenvertrag.");
        let crm = n.section(&SectionKind::Other("CRM".into())).unwrap();
        assert_eq!(crm.lines, vec!["Kunde: Druckerei Muster"]);
    }

    #[test]
    fn broken_comments_are_text() {
        assert_eq!(strip_time_comment("Budget <!-->"), ("Budget <!-->".into(), None));
        assert_eq!(strip_time_comment("Budget <!--->"), ("Budget <!--->".into(), None));
        assert_eq!(strip_time_comment("a <!-- t=99:99 -->").1, None);
        assert_eq!(strip_time_comment("a <!-- t=01:00 -->"), ("a".into(), Some(60_000)));
    }

    #[test]
    fn private_and_inline_lists() {
        let n = note("---\ntype: meeting\nprivate: true\nparticipants: [Ich, Anna]\n---\n\n# X\n");
        assert!(n.front.private);
        assert_eq!(n.front.participants, vec!["Ich", "Anna"]);
    }

    #[test]
    fn own_notes_keep_their_times_and_nesting() {
        let md = "---\nid: 2026-11-26_10-00-03\nsource: own-notes\ntitle: Herbstaktion planen\n---\nBudget? <!-- t=00:03:12 -->\nDruckerei bis Fr <!-- t=00:09:40 -->\n  - Dateien bis Freitag 12 Uhr\n";
        let (f, title, lines) = own_notes(md);
        assert_eq!(f.id.as_deref(), Some("2026-11-26_10-00-03"));
        assert_eq!(title.as_deref(), Some("Herbstaktion planen"));
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert_eq!(lines[0].text, "Budget?");
        assert_eq!(lines[0].at_ms, Some(192_000));
        assert_eq!(lines[1].text, "Druckerei bis Fr");
        assert!(!lines[1].nested);
        assert_eq!(lines[2].text, "Dateien bis Freitag 12 Uhr");
        assert!(lines[2].nested);
        assert_eq!(lines[2].at_ms, None);
    }
}

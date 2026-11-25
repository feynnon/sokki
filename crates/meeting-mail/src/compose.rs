//! The mail for a meeting note: the summary part of the note (summary,
//! decisions, action items), no transcript, as plain text. It ends with the
//! note's own last line, the Sokki line, when the note has one: whether a
//! note carries it is decided where the note is written
//! (`sokki_engine::process::Settings`), so the mail follows that choice and
//! never adds a line of its own.

use meeting_core::note::split_note;

/// Subject and body of the notes mail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composed {
    pub subject: String,
    pub body: String,
    /// "de" or "en": the note's language, which the mail is written in.
    pub language: &'static str,
}

struct Words {
    subject: &'static str,
    intro: &'static str,
    stop: &'static str,
}

const DE: Words = Words {
    subject: "Notizen: ",
    intro: "Hier sind die Notizen zu „{title}“.",
    stop: "Keine solchen Mails mehr? Antworte mit „stopp“.",
};

const EN: Words = Words {
    subject: "Notes: ",
    intro: "Here are the notes from “{title}”.",
    stop: "Rather not get these? Reply “stop”.",
};

/// The subject prefixes of Sokki's mails in every language, to find replies
/// to them: these notes, and the mail before a meeting (card
/// pre-meeting-mail), so a "stop" to either keeps the person off both.
pub const SUBJECT_PREFIXES: [&str; 4] = [DE.subject, EN.subject, "Notizen zu ", "Notes for "];

/// The mail for the note `md` titled `title`. `None` when the note has no
/// summary yet (nothing worth sending).
pub fn summary_mail(md: &str, title: &str) -> Option<Composed> {
    let summary = split_note(md).summary;
    let english = summary.lines().any(|l| {
        let l = l.trim_end();
        l == "## Summary" || l == "## Decisions" || l == "## Action items"
    });
    let german = summary.lines().any(|l| {
        let l = l.trim_end();
        l == "## Zusammenfassung" || l == "## Entscheidungen" || l == "## Aufgaben"
    });
    if !english && !german {
        // No summary sections: the "no summary" line, or an edited note.
        return None;
    }
    let (w, language) = if german { (&DE, "de") } else { (&EN, "en") };
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut body = String::new();
    body.push_str(&w.intro.replace("{title}", &title));
    body.push_str("\n\n");
    body.push_str(&plain_text(&summary));
    body.push_str("\n\n");
    body.push_str(w.stop);
    body.push('\n');
    Some(Composed {
        subject: format!("{}{}", w.subject, title),
        body,
        language,
    })
}

/// The summary's Markdown as plain text for a mail: headings as their own
/// line, bullets and tasks as "• ", `**bold**` without the stars.
fn plain_text(md: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for raw in md.lines() {
        let line = raw.trim_end();
        let text = if let Some(h) = line.strip_prefix("## ") {
            if lines.last().is_some_and(|l| !l.is_empty()) {
                lines.push(String::new());
            }
            h.trim().to_string()
        } else if let Some(t) = line
            .strip_prefix("- [ ] ")
            .or_else(|| line.strip_prefix("- [x] "))
            .or_else(|| line.strip_prefix("- "))
        {
            format!("• {}", t.trim())
        } else {
            line.trim().to_string()
        };
        let text = text.replace("**", "");
        if text.is_empty() && lines.last().is_none_or(|l| l.is_empty()) {
            continue;
        }
        lines.push(text);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTE: &str = "---\ntype: meeting\ndate: 2026-11-24\n---\n\n# Weekly Nordlicht\n\n\
## Zusammenfassung\n\nWir haben den Launch besprochen.\n\n\
## Entscheidungen\n\n- Show HN am 8.12.\n\n\
## Aufgaben\n\n- [ ] **Mira**: Video aufnehmen\n- [ ] Texte prüfen\n\n\
## Transkript\n\n**[00:03] Mira:** Hallo zusammen, geheim.\n\n\
Transkribiert auf meinem Mac mit sokki.org\n";

    #[test]
    fn the_mail_has_the_summary_and_no_transcript() {
        let m = summary_mail(NOTE, "Weekly  Nordlicht").unwrap();
        assert_eq!(m.subject, "Notizen: Weekly Nordlicht");
        assert_eq!(m.language, "de");
        assert_eq!(
            m.body,
            "Hier sind die Notizen zu „Weekly Nordlicht“.\n\n\
Zusammenfassung\n\nWir haben den Launch besprochen.\n\n\
Entscheidungen\n\n• Show HN am 8.12.\n\n\
Aufgaben\n\n• Mira: Video aufnehmen\n• Texte prüfen\n\n\
Transkribiert auf meinem Mac mit sokki.org\n\n\
Keine solchen Mails mehr? Antworte mit „stopp“.\n"
        );
        assert!(!m.body.contains("geheim"));
    }

    #[test]
    fn an_english_note_gets_an_english_mail() {
        let md = "# Sync\n\n## Summary\n\nWe met.\n\n## Transcript\n\n**[00:01] A:** hi\n";
        let m = summary_mail(md, "Sync").unwrap();
        assert_eq!(m.subject, "Notes: Sync");
        assert!(m
            .body
            .starts_with("Here are the notes from “Sync”.\n\nSummary\n\nWe met."));
        assert!(m.body.ends_with("Rather not get these? Reply “stop”.\n"));
    }

    #[test]
    fn a_note_without_summary_sends_nothing() {
        let md = "# Meeting\n\n_Keine Zusammenfassung erstellt._\n\n## Transkript\n\n**[00:01] A:** hi\n";
        assert_eq!(summary_mail(md, "Meeting"), None);
    }
}

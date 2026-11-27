//! What a hook sees: the note read back into the payload, limited by the
//! hook's scope. Read when the hook runs, so it sees the note as it is then
//! (with the summary added later, or the person's edits).

use super::config::Scope;
use super::event::{Content, HookEvent, Meta, Task, DICTATION_DONE, OWN_NOTES_SAVED};
use std::path::Path;

/// A meeting note split into what hooks can ask for.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NoteSections {
    pub title: Option<String>,
    pub participants: Vec<String>,
    pub language: Option<String>,
    pub summary: Option<String>,
    pub decisions: Vec<String>,
    pub tasks: Vec<Task>,
    pub transcript: String,
}

/// Headings in both note languages (meeting_core::note).
const SUMMARY: [&str; 2] = ["Zusammenfassung", "Summary"];
const DECISIONS: [&str; 2] = ["Entscheidungen", "Decisions"];
const TASKS: [&str; 2] = ["Aufgaben", "Action items"];
const TRANSCRIPT: [&str; 2] = ["Transkript", "Transcript"];

fn strip_item(line: &str) -> Option<&str> {
    let t = line.trim_start();
    t.strip_prefix("- ").or_else(|| t.strip_prefix("* "))
}

/// "- [ ] **Anna**: Send the invite" → owner Anna, open.
fn parse_task(item: &str) -> Task {
    let (done, rest) = if let Some(r) = item.strip_prefix("[ ] ") {
        (false, r)
    } else if let Some(r) = item
        .strip_prefix("[x] ")
        .or_else(|| item.strip_prefix("[X] "))
    {
        (true, r)
    } else {
        (false, item)
    };
    let rest = rest.trim();
    let owned = rest
        .strip_prefix("**")
        .and_then(|r| r.split_once("**:").or_else(|| r.split_once(":**")))
        .map(|(owner, text)| {
            (
                owner.trim().trim_end_matches(':').to_string(),
                text.trim().to_string(),
            )
        });
    match owned {
        Some((owner, text)) if !owner.is_empty() && !text.is_empty() => Task {
            text,
            owner: Some(owner),
            done,
        },
        _ => Task {
            text: rest.to_string(),
            owner: None,
            done,
        },
    }
}

/// Reads a note in Sokki's layout. Works on notes the person edited, as long
/// as the headings stay.
pub fn parse_note(md: &str) -> NoteSections {
    let mut out = NoteSections::default();
    let mut body = md;
    if let Some(rest) = md.strip_prefix("---\n") {
        if let Some((front, b)) = rest.split_once("\n---\n") {
            body = b;
            let mut in_participants = false;
            for line in front.lines() {
                if line.starts_with("participants:") {
                    in_participants = true;
                    continue;
                }
                if in_participants {
                    if let Some(p) = line.trim_start().strip_prefix("- ") {
                        let p = p.trim().trim_matches('"');
                        if !p.is_empty() {
                            out.participants.push(p.to_string());
                        }
                        continue;
                    }
                    in_participants = false;
                }
            }
        }
    }
    let mut section: Option<&str> = None;
    let mut summary: Vec<&str> = Vec::new();
    for line in body.lines() {
        if let Some(t) = line.strip_prefix("# ") {
            if out.title.is_none() {
                out.title = Some(t.trim().to_string());
            }
            continue;
        }
        if let Some(h) = line.strip_prefix("## ") {
            let h = h.trim();
            section = [SUMMARY, DECISIONS, TASKS, TRANSCRIPT]
                .iter()
                .find(|names| names.contains(&h))
                .map(|names| names[0]);
            if out.language.is_none() && section.is_some() {
                let german = [SUMMARY[0], DECISIONS[0], TASKS[0], TRANSCRIPT[0]].contains(&h);
                out.language = Some(if german { "de" } else { "en" }.to_string());
            }
            continue;
        }
        if meeting_core::note::is_footer(line) {
            continue;
        }
        match section {
            Some(s) if s == SUMMARY[0] => summary.push(line),
            Some(s) if s == DECISIONS[0] => {
                if let Some(item) = strip_item(line) {
                    out.decisions.push(item.trim().to_string());
                }
            }
            Some(s) if s == TASKS[0] => {
                if let Some(item) = strip_item(line) {
                    out.tasks.push(parse_task(item));
                }
            }
            _ => {}
        }
    }
    let summary = summary.join("\n").trim().to_string();
    let no_summary = summary.is_empty()
        || summary == "_Keine Zusammenfassung erstellt._"
        || summary == "_No summary was created._";
    if !no_summary {
        out.summary = Some(summary);
    }
    out.transcript = meeting_core::note::split_note(md).transcript;
    out
}

/// Fills what the host left out of `meta` from the note: participants,
/// language, title. Matching (`match.person`) needs the participants.
pub fn complete_meta(event: &mut HookEvent) {
    if event.event == OWN_NOTES_SAVED || event.event == DICTATION_DONE {
        return;
    }
    let Some(path) = event.note_path.as_deref() else {
        return;
    };
    let Ok(md) = std::fs::read_to_string(path) else {
        return;
    };
    let note = parse_note(&md);
    let meta: &mut Meta = &mut event.meta;
    if meta.participants.is_empty() {
        meta.participants = note.participants;
    }
    if meta.language.is_none() {
        meta.language = note.language;
    }
    if meta.title.is_none() {
        meta.title = note.title;
    }
}

/// The payload for one hook: the event with only the content its scope
/// names. Dictation and own-notes text travel in the queued event; meeting
/// content is read from the note now.
pub fn for_scope(event: &HookEvent, scope: &[Scope]) -> HookEvent {
    let has = |s: Scope| scope.contains(&s);
    let mut content = Content::default();
    match event.event.as_str() {
        DICTATION_DONE => {
            if has(Scope::Dictations) {
                content.text = event.content.text.clone();
            }
        }
        OWN_NOTES_SAVED => {
            if has(Scope::OwnNotes) {
                // What the person typed, line by line: without the file's
                // front matter and time marks.
                content.text = event.content.text.clone().or_else(|| {
                    let md = std::fs::read_to_string(event.note_path.as_deref()?).ok()?;
                    let lines = meeting_core::own_notes::parse(&md);
                    Some(
                        lines
                            .iter()
                            .map(|l| l.text.as_str())
                            .collect::<Vec<_>>()
                            .join("\n"),
                    )
                });
            }
        }
        _ => {
            let wants = has(Scope::Summary) || has(Scope::Tasks) || has(Scope::Transcript);
            let note = event
                .note_path
                .as_deref()
                .filter(|_| wants)
                .and_then(|p| std::fs::read_to_string(Path::new(p)).ok())
                .map(|md| parse_note(&md));
            if let Some(note) = note {
                if has(Scope::Summary) {
                    content.summary = note.summary;
                    content.decisions = Some(note.decisions);
                }
                if has(Scope::Tasks) {
                    content.tasks = Some(note.tasks);
                }
                if has(Scope::Transcript) && !note.transcript.is_empty() {
                    content.transcript = Some(note.transcript);
                }
            }
        }
    }
    HookEvent {
        content,
        ..event.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::event::{MEETING_NOTE_READY, VERSION};

    pub const NOTE: &str = "---\ntype: meeting\ndate: 2026-11-26\nstart: \"14:03\"\nduration: 34 min\nparticipants:\n  - Ich\n  - Anna Beispiel\nsource_app: Zoom\ntags: [meeting]\n---\n\n# Planung Herbstfest\n\n## Zusammenfassung\n\n- Das Fest ist am 12. Dezember.\n- Anna kümmert sich um die Halle.\n\n## Entscheidungen\n\n- Das Fest findet in der Turnhalle statt.\n\n## Aufgaben\n\n- [ ] **Anna Beispiel**: Halle reservieren\n- [ ] Einladung verschicken\n- [x] **Ich**: Budget prüfen\n\n## Transkript\n\n**[00:00] Ich:** Hallo zusammen.\n\n**[00:04] Anna Beispiel:** Hallo, ich habe die Halle angefragt.\n\nVon KI zusammengefasst. Lokal, kostenlos, Open Source. Diktier-App und Meeting-Notetaker: sokki.org\n";

    #[test]
    fn reads_every_section() {
        let n = parse_note(NOTE);
        assert_eq!(n.title.as_deref(), Some("Planung Herbstfest"));
        assert_eq!(n.participants, vec!["Ich", "Anna Beispiel"]);
        assert_eq!(n.language.as_deref(), Some("de"));
        assert_eq!(
            n.summary.as_deref(),
            Some("- Das Fest ist am 12. Dezember.\n- Anna kümmert sich um die Halle.")
        );
        assert_eq!(n.decisions, vec!["Das Fest findet in der Turnhalle statt."]);
        assert_eq!(n.tasks.len(), 3);
        assert_eq!(n.tasks[0].owner.as_deref(), Some("Anna Beispiel"));
        assert_eq!(n.tasks[0].text, "Halle reservieren");
        assert!(!n.tasks[0].done);
        assert_eq!(n.tasks[1].owner, None);
        assert_eq!(n.tasks[1].text, "Einladung verschicken");
        assert!(n.tasks[2].done);
        assert_eq!(
            n.transcript,
            "[00:00] Ich: Hallo zusammen.\n[00:04] Anna Beispiel: Hallo, ich habe die Halle angefragt."
        );
    }

    #[test]
    fn english_and_no_summary() {
        let md = "# T\n\n## Summary\n\n_No summary was created._\n\n## Transcript\n\n**[00:01] Me:** Hi.\n";
        let n = parse_note(md);
        assert_eq!(n.language.as_deref(), Some("en"));
        assert_eq!(n.summary, None);
        assert!(n.tasks.is_empty());
    }

    fn event_for(path: &Path) -> HookEvent {
        HookEvent::meeting(MEETING_NOTE_READY, "m1", Some(path), Meta::default())
    }

    #[test]
    fn scope_limits_what_the_hook_sees() {
        let dir = std::env::temp_dir().join(format!("sokki-hooks-scope-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.md");
        std::fs::write(&path, NOTE).unwrap();
        let mut e = event_for(&path);
        complete_meta(&mut e);
        assert_eq!(e.meta.participants, vec!["Ich", "Anna Beispiel"]);
        assert_eq!(e.v, VERSION);

        let none = for_scope(&e, &[]);
        assert!(none.content.is_empty(), "no scope, no content");
        let tasks = for_scope(&e, &[Scope::Tasks]);
        assert_eq!(tasks.content.tasks.as_ref().unwrap().len(), 3);
        assert!(tasks.content.summary.is_none());
        assert!(
            tasks.content.transcript.is_none(),
            "transcripts are off unless named"
        );
        let all = for_scope(&e, &[Scope::Summary, Scope::Tasks, Scope::Transcript]);
        assert!(all.content.summary.is_some());
        assert_eq!(all.content.decisions.as_ref().unwrap().len(), 1);
        assert!(all
            .content
            .transcript
            .as_ref()
            .unwrap()
            .contains("Halle angefragt"));

        // Own notes: the typed lines, not the file's markup.
        let notes = dir.join("notes.md");
        std::fs::write(
            &notes,
            "---\nid: m1\nsource: own-notes\n---\nHalle anfragen <!-- t=00:00:03 -->\nBudget 400 Euro <!-- t=00:00:09 -->\n",
        )
        .unwrap();
        let own = HookEvent::own_notes("m1", &notes, Meta::default());
        assert_eq!(
            for_scope(&own, &[Scope::OwnNotes]).content.text.as_deref(),
            Some("Halle anfragen\nBudget 400 Euro")
        );
        assert!(for_scope(&own, &[Scope::Summary]).content.text.is_none());

        let d = HookEvent::dictation(7, "Geheimer Text", Meta::default());
        assert!(for_scope(&d, &[Scope::Summary, Scope::Transcript])
            .content
            .text
            .is_none());
        assert_eq!(
            for_scope(&d, &[Scope::Dictations]).content.text.as_deref(),
            Some("Geheimer Text")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

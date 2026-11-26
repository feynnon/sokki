//! Renders a [`MeetingNote`] as Markdown for Obsidian and writes it to disk.
//!
//! Layout:
//!
//! ```text
//! ---
//! type: meeting
//! date: 2026-11-17
//! start: "14:03"
//! duration: 34 min
//! participants:
//!   - Ich
//!   - Speaker 2
//! source_app: Zoom
//! tags: [meeting]
//! ---
//!
//! # Title
//!
//! ## Zusammenfassung / ## Entscheidungen / ## Aufgaben / ## Transkript
//!
//! Von KI zusammengefasst. Lokal, kostenlos, Open Source. Diktier-App und Meeting-Notetaker: sokki.org
//! ```
//!
//! Headings follow the summary's language, else the spoken language, else German. Empty sections are
//! left out. The Sokki line (the "footer") is the note's last line; [`split_note`] puts it at the
//! end of the summary too, so a copied or shared summary carries it. It is
//! also the summary's AI label, so a note without a summary has none.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::lang::{detect_language, Language};
use crate::{format_mmss, MeetingNote, MeetingSummary, Segment, TrackKind, Transcript};

struct Labels {
    summary: &'static str,
    decisions: &'static str,
    tasks: &'static str,
    transcript: &'static str,
    no_summary: &'static str,
    no_speech: &'static str,
    /// The Sokki line under a summary made on this computer; and under one
    /// a cloud model made, which is neither local nor free. The same on
    /// every computer: it names no platform.
    footer: &'static str,
    footer_cloud: &'static str,
}

/// Footers of notes written before the Sokki line (until 0.2.11). Still
/// recognised, so older notes split and share as before.
const OLD_FOOTERS: [&str; 4] = [
    "Transkribiert auf meinem Mac mit sokki.org",
    "Transkribiert auf meinem Computer mit sokki.org",
    "Transcribed on my Mac with sokki.org",
    "Transcribed on my computer with sokki.org",
];

const DE: Labels = Labels {
    summary: "Zusammenfassung",
    decisions: "Entscheidungen",
    tasks: "Aufgaben",
    transcript: "Transkript",
    no_summary: "_Keine Zusammenfassung erstellt._",
    no_speech: "_Keine Sprache erkannt._",
    footer: "Von KI zusammengefasst. Lokal, kostenlos, Open Source. Diktier-App und Meeting-Notetaker: sokki.org",
    footer_cloud: "Von KI zusammengefasst. Open Source. Diktier-App und Meeting-Notetaker: sokki.org",
};

const EN: Labels = Labels {
    summary: "Summary",
    decisions: "Decisions",
    tasks: "Action items",
    transcript: "Transcript",
    no_summary: "_No summary was created._",
    no_speech: "_No speech detected._",
    footer: "Summarized by AI. Local, free, open source. Dictation app and meeting notetaker: sokki.org",
    footer_cloud: "Summarized by AI. Open source. Dictation app and meeting notetaker: sokki.org",
};

/// Language of the note's headings: the summary's language, German by
/// default.
pub fn note_language(note: &MeetingNote) -> Language {
    note_language_or(note, Language::German)
}

/// Like [`note_language`], with `fallback` for a meeting with nothing to go
/// by (no summary and no words spoken): the person's own language.
pub fn note_language_or(note: &MeetingNote, fallback: Language) -> Language {
    note.summary
        .as_ref()
        .and_then(|s| {
            let mut text = format!("{} {}", s.title, s.summary);
            for d in &s.decisions {
                text.push(' ');
                text.push_str(d);
            }
            for a in &s.action_items {
                text.push(' ');
                text.push_str(&a.text);
            }
            detect_language(&text)
        })
        // No summary (no language model): the language people spoke, so an
        // English call does not get German headings.
        .or_else(|| {
            let spoken: Vec<&str> = note
                .transcript
                .segments
                .iter()
                .map(|s| s.text.as_str())
                .collect();
            detect_language(&spoken.join(" "))
        })
        .unwrap_or(fallback)
}

/// Date ("YYYY-MM-DD") and time ("HH:MM") from an RFC 3339 timestamp like
/// "2026-11-17T14:03:00+01:00". Returns None if it does not look like one.
fn date_and_time(started_at: &str) -> Option<(String, String)> {
    let s = started_at.trim();
    let b = s.as_bytes();
    let digits =
        |r: std::ops::Range<usize>| r.clone().all(|i| b.get(i).is_some_and(u8::is_ascii_digit));
    if s.len() >= 16
        && digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && (b[10] == b'T' || b[10] == b' ')
        && digits(11..13)
        && b[13] == b':'
        && digits(14..16)
    {
        Some((s[..10].to_string(), s[11..16].to_string()))
    } else {
        None
    }
}

/// Title shown in the note: the summary's title, else "Meeting <date> <time>".
pub fn note_title(note: &MeetingNote) -> String {
    if let Some(t) = note
        .summary
        .as_ref()
        .map(|s| s.title.trim())
        .filter(|t| !t.is_empty())
    {
        return t.to_string();
    }
    match date_and_time(&note.meta.started_at) {
        Some((d, t)) => format!("Meeting {d} {t}"),
        None => "Meeting".to_string(),
    }
}

fn duration_label(seconds: f64) -> String {
    let min = (seconds.max(0.0) / 60.0).round() as u64;
    format!("{} min", min.max(1))
}

/// Quotes a YAML scalar when needed (double quotes, escaped).
fn yaml_scalar(v: &str) -> String {
    let needs = v.is_empty()
        || v.trim() != v
        || v.starts_with([
            '-', '?', ':', ',', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%',
            '@', '`',
        ])
        || v.contains(": ")
        || v.contains(" #")
        || v.ends_with(':')
        || v.contains(['\n', '\r', '\t'])
        || matches!(
            v.to_ascii_lowercase().as_str(),
            "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "~"
        )
        || v.parse::<f64>().is_ok()
        // "14:03" would be read as a sexagesimal number by YAML 1.1 parsers.
        || (v.contains(':') && v.chars().all(|c| c.is_ascii_digit() || c == ':'));
    if needs {
        let escaped = v
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "")
            .replace('\t', " ");
        format!("\"{escaped}\"")
    } else {
        v.to_string()
    }
}

/// What goes into a note besides the meeting itself.
#[derive(Debug, Clone, Copy)]
pub struct NoteOptions {
    /// End with the Sokki line ("Summarized by AI. Local, free, open
    /// source. Dictation app and meeting notetaker: sokki.org"), when there
    /// is a summary.
    pub footer: bool,
    /// The summary came from a cloud model: the line then doesn't say
    /// "local, free".
    pub cloud_summary: bool,
    /// The person's own language, for the headings of a note with no words
    /// to go by (nothing was said). None: German.
    pub language: Option<Language>,
}

impl Default for NoteOptions {
    fn default() -> Self {
        Self {
            footer: true,
            cloud_summary: false,
            language: None,
        }
    }
}

/// Renders the whole note with the footer.
pub fn render_markdown(note: &MeetingNote) -> String {
    render_note(note, &NoteOptions::default())
}

/// Renders the whole note.
pub fn render_note(note: &MeetingNote, opts: &NoteOptions) -> String {
    let lang = note_language_or(note, opts.language.unwrap_or(Language::German));
    let l = match lang {
        Language::German => &DE,
        Language::English => &EN,
    };
    let mut out = String::new();

    // Frontmatter.
    out.push_str("---\ntype: meeting\n");
    if let Some((date, time)) = date_and_time(&note.meta.started_at) {
        out.push_str(&format!("date: {date}\nstart: {}\n", yaml_scalar(&time)));
    }
    out.push_str(&format!(
        "duration: {}\n",
        duration_label(note.meta.duration_s)
    ));
    let participants = note.transcript.speakers();
    if participants.is_empty() {
        out.push_str("participants: []\n");
    } else {
        out.push_str("participants:\n");
        for p in &participants {
            out.push_str(&format!("  - {}\n", yaml_scalar(p)));
        }
    }
    if let Some(app) = note
        .meta
        .source_app
        .as_deref()
        .map(str::trim)
        .filter(|a| !a.is_empty())
    {
        out.push_str(&format!("source_app: {}\n", yaml_scalar(app)));
    }
    out.push_str("tags: [meeting]\n---\n\n");

    out.push_str(&format!("# {}\n\n", one_line(&note_title(note))));

    match &note.summary {
        Some(s) => render_summary(&mut out, s, l),
        None => {
            out.push_str(l.no_summary);
            out.push_str("\n\n");
        }
    }

    out.push_str(&format!("## {}\n\n", l.transcript));
    out.push_str(&render_transcript(&note.transcript, l));
    if opts.footer && has_summary(note.summary.as_ref()) {
        out.push_str(&format!("\n{}\n", footer_line(l, opts)));
    }
    out
}

/// The Sokki line in `language`, for where the summary was made. The one source of the text for every place a summary goes:
/// the note, and mails and chat posts (board cards `teilnehmer-mail`,
/// `slack-huddle`). Whether it goes there: `sokki_engine::Settings`.
pub fn sokki_line(language: Language, cloud_summary: bool) -> &'static str {
    let l = match language {
        Language::German => &DE,
        Language::English => &EN,
    };
    footer_line(
        l,
        &NoteOptions {
            cloud_summary,
            ..Default::default()
        },
    )
}

/// The Sokki line for where the summary was made.
fn footer_line(l: &Labels, opts: &NoteOptions) -> &'static str {
    if opts.cloud_summary {
        l.footer_cloud
    } else {
        l.footer
    }
}

/// Whether there is a summary to put the Sokki line under.
fn has_summary(summary: Option<&MeetingSummary>) -> bool {
    summary.is_some_and(|s| {
        !s.summary.trim().is_empty()
            || s.decisions.iter().any(|d| !d.trim().is_empty())
            || s.action_items.iter().any(|a| !a.text.trim().is_empty())
    })
}

/// Whether a line is the note footer, in any language, old ones included.
/// The line is a Sokki line (any version, local or cloud).
pub fn is_footer(line: &str) -> bool {
    let t = line.trim();
    OLD_FOOTERS.contains(&t)
        || [DE, EN]
            .iter()
            .any(|l| l.footer == t || l.footer_cloud == t)
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn render_summary(out: &mut String, s: &MeetingSummary, l: &Labels) {
    let summary = s.summary.trim();
    if !summary.is_empty() {
        out.push_str(&format!("## {}\n\n{}\n\n", l.summary, summary));
    }
    let decisions: Vec<&str> = s
        .decisions
        .iter()
        .map(|d| d.trim())
        .filter(|d| !d.is_empty())
        .collect();
    if !decisions.is_empty() {
        out.push_str(&format!("## {}\n\n", l.decisions));
        for d in decisions {
            out.push_str(&format!("- {}\n", one_line(d)));
        }
        out.push('\n');
    }
    let tasks: Vec<_> = s
        .action_items
        .iter()
        .filter(|a| !a.text.trim().is_empty())
        .collect();
    if !tasks.is_empty() {
        out.push_str(&format!("## {}\n\n", l.tasks));
        for a in tasks {
            match a.owner.as_deref().map(str::trim).filter(|o| !o.is_empty()) {
                Some(o) => out.push_str(&format!(
                    "- [ ] **{}**: {}\n",
                    one_line(o),
                    one_line(&a.text)
                )),
                None => out.push_str(&format!("- [ ] {}\n", one_line(&a.text))),
            }
        }
        out.push('\n');
    }
}

/// `**[mm:ss] Speaker:** text`, consecutive segments of one speaker joined
/// into one paragraph.
fn render_transcript(t: &Transcript, l: &Labels) -> String {
    if t.segments.is_empty() {
        return format!("{}\n", l.no_speech);
    }
    let mut paras: Vec<(f64, &str, String)> = Vec::new();
    for s in &t.segments {
        let text = one_line(&s.text);
        if text.is_empty() {
            continue;
        }
        match paras.last_mut() {
            Some((_, sp, buf)) if *sp == s.speaker => {
                buf.push(' ');
                buf.push_str(&text);
            }
            _ => paras.push((s.start_s, &s.speaker, text)),
        }
    }
    if paras.is_empty() {
        return format!("{}\n", l.no_speech);
    }
    paras
        .into_iter()
        .map(|(t, sp, text)| format!("**[{}] {}:** {}\n", format_mmss(t), sp, text))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Makes a string safe as (part of) a file name on macOS and as an Obsidian
/// note name: removes `/ \ : * ? " < > | # ^ [ ]` and control characters,
/// collapses whitespace, strips leading/trailing dots and spaces, and caps
/// the length at `max_chars`.
/// A written note split for the app: the summary part as Markdown and the
/// transcript as plain lines ("[00:03] Speaker 2: Hi."), for reading and
/// copying. Works on notes the user edited, as long as the headings stay.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NoteParts {
    pub summary: String,
    pub transcript: String,
    /// The "## Meine Notizen" section without its heading
    /// ([`crate::own_notes`]), empty without one.
    pub own_notes: String,
}

pub fn split_note(md: &str) -> NoteParts {
    // Skip the YAML front matter.
    let body = md
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n").map(|(_, b)| b))
        .unwrap_or(md);
    let is_transcript_heading = |l: &str| {
        let h = l.trim_end();
        h == format!("## {}", DE.transcript) || h == format!("## {}", EN.transcript)
    };
    let mut summary = Vec::new();
    let mut transcript = Vec::new();
    let mut in_transcript = false;
    let mut footer = None;
    let all: Vec<&str> = body.lines().collect();
    let own = crate::own_notes::section_ranges(&all);
    for (i, line) in all.iter().copied().enumerate() {
        if own.iter().any(|&(a, b)| i >= a && i < b) {
            continue;
        }
        if is_footer(line) {
            footer = Some(line.trim());
            continue;
        }
        if is_transcript_heading(line) {
            in_transcript = true;
            continue;
        }
        if in_transcript {
            // The corrections callout of older notes ends the transcript.
            if line.starts_with("> [!info]") {
                in_transcript = false;
                continue;
            }
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            // "**[00:03] Speaker 2:** Hi." -> "[00:03] Speaker 2: Hi."
            let plain = match t.strip_prefix("**").and_then(|r| r.split_once(":**")) {
                Some((who, rest)) => format!("{who}:{rest}"),
                None => t.to_string(),
            };
            transcript.push(plain);
        } else if !line.starts_with("# ") && !line.starts_with(">") {
            summary.push(line);
        }
    }
    let mut summary = summary.join("\n").trim().to_string();
    // The footer closes the summary too, so it travels when the summary is
    // shared. Not under an empty summary or the "no summary" line.
    let no_summary = summary.is_empty() || summary == DE.no_summary || summary == EN.no_summary;
    if let Some(f) = footer.filter(|_| !no_summary) {
        summary.push_str("\n\n");
        summary.push_str(f);
    }
    NoteParts {
        summary,
        transcript: transcript.join("\n"),
        own_notes: crate::own_notes::section_of(md),
    }
}

/// A note written for a meeting where nobody spoke: its transcript is only
/// "_Keine Sprache erkannt._" / "_No speech detected._". Sokki stopped
/// writing these on 2026-11-25; the app moves old ones to the Trash.
pub fn is_no_speech_note(md: &str) -> bool {
    let said_so = split_note(md)
        .transcript
        .lines()
        .any(|l| l.trim() == DE.no_speech || l.trim() == EN.no_speech);
    said_so && transcript_from_note(md).segments.is_empty()
}

/// The transcript of a written note, read back from its lines
/// ("**[00:03] Speaker 2:** Hi."), to summarize the note later. Each
/// segment ends where the next begins; the track is not in the note, so
/// all are `Mic`. Lines in another shape are skipped.
pub fn transcript_from_note(md: &str) -> Transcript {
    let mut segments: Vec<Segment> = Vec::new();
    for line in split_note(md).transcript.lines() {
        let Some((time, rest)) = line.strip_prefix('[').and_then(|l| l.split_once("] ")) else {
            continue;
        };
        let Some((speaker, text)) = rest.split_once(": ") else {
            continue;
        };
        let mut start_s = 0.0;
        let mut ok = !time.is_empty();
        for part in time.split(':') {
            match part.parse::<u32>() {
                Ok(n) => start_s = start_s * 60.0 + n as f64,
                Err(_) => ok = false,
            }
        }
        if !ok || text.trim().is_empty() {
            continue;
        }
        if let Some(prev) = segments.last_mut() {
            prev.end_s = start_s.max(prev.start_s);
        }
        segments.push(Segment {
            start_s,
            end_s: start_s,
            track: TrackKind::Mic,
            speaker: speaker.trim().to_string(),
            text: text.trim().to_string(),
        });
    }
    Transcript { segments }
}

/// Puts `summary` into a note that was written without one: the "no
/// summary" line becomes the summary sections, the title heading the
/// summary's title, and the note now ends with the Sokki line (`opts`; an
/// older footer gives way to it). Everything else, including the user's
/// edits, stays. `None` when the note has no "no summary" line (summarized
/// already, or edited there).
pub fn add_summary(md: &str, summary: &MeetingSummary, opts: &NoteOptions) -> Option<String> {
    let l = [&DE, &EN]
        .into_iter()
        .find(|l| md.lines().any(|line| line.trim() == l.no_summary))?;
    let mut rendered = String::new();
    render_summary(&mut rendered, summary, l);
    let rendered = rendered.trim_end();
    let title = one_line(summary.title.trim());
    let mut out = String::with_capacity(md.len() + rendered.len());
    let (mut placed, mut titled) = (false, title.is_empty());
    for line in md.split_inclusive('\n') {
        let eol = if line.ends_with('\n') { "\n" } else { "" };
        if is_footer(line) {
            continue;
        } else if !placed && line.trim() == l.no_summary {
            placed = true;
            out.push_str(rendered);
            out.push_str(eol);
        } else if !titled && !placed && line.starts_with("# ") {
            titled = true;
            out.push_str(&format!("# {title}{eol}"));
        } else {
            out.push_str(line);
        }
    }
    let mut out = format!("{}\n", out.trim_end());
    if opts.footer && has_summary(Some(summary)) {
        out.push_str(&format!("\n{}\n", footer_line(l, opts)));
    }
    Some(out)
}

/// The note with `title` as its title heading (the first `# ` line), for a
/// title the person typed. `None` when the note has no title heading.
pub fn retitle_markdown(md: &str, title: &str) -> Option<String> {
    let title = one_line(title.trim());
    let mut out = String::with_capacity(md.len() + title.len());
    let mut titled = false;
    for line in md.split_inclusive('\n') {
        if !titled && line.starts_with("# ") {
            titled = true;
            let eol = &line[line.trim_end_matches(['\r', '\n']).len()..];
            out.push_str(&format!("# {title}{eol}"));
        } else {
            out.push_str(line);
        }
    }
    titled.then_some(out)
}

/// The file stem for a note once it got its summary: "YYYY-MM-DD HHmm
/// <Title>" when the note still has the name it was written with ("… Meeting"
/// or "… Meeting 2"), `None` when the user renamed it (keep that name).
pub fn summarized_file_stem(stem: &str, title: &str) -> Option<String> {
    let b = stem.as_bytes();
    let prefix_ok = stem.len() >= 15
        && stem.is_char_boundary(15)
        && b[..15].iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            10 => *c == b' ',
            _ => c.is_ascii_digit(),
        });
    if !prefix_ok {
        return None;
    }
    let rest = stem[15..].strip_prefix(" Meeting")?;
    if !(rest.is_empty()
        || rest
            .strip_prefix(' ')
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit())))
    {
        return None;
    }
    let title = sanitize_filename(title, 80);
    if title.is_empty() {
        return None;
    }
    Some(format!("{} {title}", &stem[..15]))
}

pub fn sanitize_filename(s: &str, max_chars: usize) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '#' | '^' | '[' | ']' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let collapsed = one_line(&cleaned);
    let trimmed = collapsed.trim_matches(|c: char| c == '.' || c.is_whitespace());
    let capped: String = trimmed.chars().take(max_chars).collect();
    capped
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .to_string()
}

/// File name stem without extension: "YYYY-MM-DD HHmm <Title>".
pub fn note_file_stem(note: &MeetingNote) -> String {
    let title = note
        .summary
        .as_ref()
        .map(|s| sanitize_filename(&s.title, 80))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "Meeting".to_string());
    match date_and_time(&note.meta.started_at) {
        Some((d, t)) => format!("{d} {} {title}", t.replace(':', "")),
        None => title,
    }
}

/// Writes the note into `dir` (created if missing) and returns its path.
/// Never overwrites: if the name is taken, " 2", " 3", … is appended.
pub fn write_note(dir: &Path, note: &MeetingNote, opts: &NoteOptions) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let stem = note_file_stem(note);
    let content = render_note(note, opts);
    for n in 1..10_000 {
        let name = if n == 1 {
            format!("{stem}.md")
        } else {
            format!("{stem} {n}.md")
        };
        let path = dir.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                f.write_all(content.as_bytes())
                    .with_context(|| format!("writing {}", path.display()))?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).with_context(|| format!("creating {}", path.display())),
        }
    }
    anyhow::bail!("no free file name for {stem} in {}", dir.display())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionItem, MeetingMeta, Segment, TrackKind};

    #[test]
    fn a_no_speech_note_is_known_by_its_transcript() {
        let empty = "---\ntype: meeting\n---\n\n# Meeting 2026-11-24 14:43\n\n\
            _Keine Zusammenfassung erstellt._\n\n## Transkript\n\n_Keine Sprache erkannt._\n\n\
            Transkribiert auf meinem Mac mit sokki.org\n";
        assert!(is_no_speech_note(empty));
        let en = empty
            .replace("## Transkript", "## Transcript")
            .replace("_Keine Sprache erkannt._", "_No speech detected._");
        assert!(is_no_speech_note(&en));
        let spoken = empty.replace("_Keine Sprache erkannt._", "**[00:03] Anna:** Hallo.");
        assert!(!is_no_speech_note(&spoken));
        // The words in the summary are not the transcript.
        let mentioned = "# Notes\n\n## Zusammenfassung\n\n_Keine Sprache erkannt._\n\n\
            ## Transkript\n\n**[00:03] Anna:** Hallo.\n";
        assert!(!is_no_speech_note(mentioned));
    }

    fn seg(t: f64, sp: &str, text: &str) -> Segment {
        Segment {
            start_s: t,
            end_s: t + 2.0,
            track: TrackKind::Mic,
            speaker: sp.into(),
            text: text.into(),
        }
    }

    fn note(summary: Option<MeetingSummary>) -> MeetingNote {
        MeetingNote {
            meta: MeetingMeta {
                started_at: "2026-11-17T14:03:00+01:00".into(),
                duration_s: 34.0 * 60.0 + 10.0,
                source_app: Some("Zoom".into()),
            },
            transcript: Transcript {
                segments: vec![
                    seg(0.0, "Ich", "Hallo zusammen."),
                    seg(2.5, "Ich", "Fangen wir an."),
                    seg(5.0, "Speaker 2", "Gerne, über die Größe."),
                    seg(65.0, "Ich", "Gut."),
                ],
            },
            summary,
        }
    }

    #[test]
    fn without_summary_the_spoken_language_picks_the_headings() {
        assert_eq!(note_language(&note(None)), Language::German);
        let mut n = note(None);
        n.transcript.segments = vec![
            seg(0.0, "Me", "Good morning, can you all hear me?"),
            seg(3.0, "Speaker 2", "Yes, we can hear you. Let us start with the update."),
        ];
        assert_eq!(note_language(&n), Language::English);
    }

    #[test]
    fn a_note_without_words_is_in_the_persons_language() {
        let mut n = note(None);
        n.transcript.segments.clear();
        assert_eq!(note_language(&n), Language::German);
        assert_eq!(note_language_or(&n, Language::English), Language::English);
        let md = render_note(
            &n,
            &NoteOptions {
                language: Some(Language::English),
                ..Default::default()
            },
        );
        assert!(md.contains("## Transcript"), "{md}");
    }

    #[test]
    fn sanitizes_file_names() {
        assert_eq!(
            sanitize_filename("Q4: Budget / Plan?", 80),
            "Q4 Budget Plan"
        );
        assert_eq!(
            sanitize_filename("  ..[Draft] #1 ^x|y  ", 80),
            "Draft 1 x y"
        );
        assert_eq!(
            sanitize_filename("Übergabe \"Größe\" <neu>", 80),
            "Übergabe Größe neu"
        );
        assert_eq!(sanitize_filename("a\nb\tc", 80), "a b c");
        assert_eq!(sanitize_filename("abcdef ghij", 7), "abcdef");
        assert_eq!(sanitize_filename("???", 80), "");
    }

    #[test]
    fn retitle_changes_only_the_title_heading() {
        let md = "---\ndate: 2026-11-24\n---\n# Old title\r\n\n## Summary\n# Not a title\n";
        assert_eq!(
            retitle_markdown(md, "  New   title ").unwrap(),
            "---\ndate: 2026-11-24\n---\n# New title\r\n\n## Summary\n# Not a title\n"
        );
        assert_eq!(retitle_markdown("no heading\n", "T"), None);
    }

    #[test]
    fn stem_uses_title_or_meeting() {
        let mut n = note(None);
        assert_eq!(note_file_stem(&n), "2026-11-17 1403 Meeting");
        n.summary = Some(MeetingSummary {
            title: "Launch: Kampagne?".into(),
            ..Default::default()
        });
        assert_eq!(note_file_stem(&n), "2026-11-17 1403 Launch Kampagne");
    }

    #[test]
    fn no_summary_renders_transcript_and_hint() {
        let md = render_markdown(&note(None));
        assert!(md.contains("# Meeting 2026-11-17 14:03\n"));
        assert!(md.contains("_Keine Zusammenfassung erstellt._"));
        assert!(!md.contains("## Zusammenfassung"));
        assert!(md.contains("## Transkript"));
        assert!(md.contains("**[00:00] Ich:** Hallo zusammen. Fangen wir an.\n"));
        assert!(md.contains("**[01:05] Ich:** Gut.\n"));
    }

    fn german_summary() -> MeetingSummary {
        MeetingSummary {
            title: "Größe der Kampagne".into(),
            summary: "Wir haben über die Größe gesprochen.".into(),
            decisions: vec!["Wir fangen klein an.".into()],
            action_items: vec![ActionItem {
                owner: Some("Ich".into()),
                text: "Budget prüfen".into(),
            }],
        }
    }

    #[test]
    fn a_summary_added_later_matches_one_written_at_once() {
        let opts = NoteOptions::default();
        let later = add_summary(&render_markdown(&note(None)), &german_summary(), &opts).unwrap();
        assert_eq!(later, render_markdown(&note(Some(german_summary()))));
        // Summarized already: nothing to add.
        assert_eq!(add_summary(&later, &german_summary(), &opts), None);
    }

    #[test]
    fn adding_a_summary_keeps_the_users_edits() {
        let md = render_markdown(&note(None)).replace("Gut.", "Gut, danke.");
        let later = add_summary(&md, &german_summary(), &NoteOptions::default()).unwrap();
        assert!(later.contains("**[01:05] Ich:** Gut, danke.\n"));
        assert!(later.contains("# Größe der Kampagne\n"));
    }

    #[test]
    fn the_transcript_reads_back_from_the_note() {
        let t = transcript_from_note(&render_markdown(&note(None)));
        let lines: Vec<(f64, &str, &str)> = t
            .segments
            .iter()
            .map(|s| (s.start_s, s.speaker.as_str(), s.text.as_str()))
            .collect();
        assert_eq!(
            lines,
            vec![
                (0.0, "Ich", "Hallo zusammen. Fangen wir an."),
                (5.0, "Speaker 2", "Gerne, über die Größe."),
                (65.0, "Ich", "Gut."),
            ]
        );
        assert_eq!(t.segments[0].end_s, 5.0);
    }

    #[test]
    fn only_a_note_with_its_first_name_is_renamed() {
        assert_eq!(
            summarized_file_stem("2026-11-17 1403 Meeting", "Launch: Plan?").as_deref(),
            Some("2026-11-17 1403 Launch Plan")
        );
        assert_eq!(
            summarized_file_stem("2026-11-17 1403 Meeting 2", "Plan").as_deref(),
            Some("2026-11-17 1403 Plan")
        );
        assert_eq!(
            summarized_file_stem("2026-11-17 1403 Kickoff", "Plan"),
            None
        );
        assert_eq!(summarized_file_stem("Meeting", "Plan"), None);
        assert_eq!(summarized_file_stem("2026-11-17 1403 Meeting", "???"), None);
    }

    #[test]
    fn empty_sections_are_omitted() {
        let md = render_markdown(&note(Some(MeetingSummary {
            title: "Kurzer Abgleich".into(),
            summary: "- Wir haben kurz über die Größe gesprochen.".into(),
            decisions: vec![],
            action_items: vec![],
        })));
        assert!(md.contains("## Zusammenfassung"));
        assert!(!md.contains("## Entscheidungen"));
        assert!(!md.contains("## Aufgaben"));
    }

    #[test]
    fn english_summary_gets_english_headings() {
        let md = render_markdown(&note(Some(MeetingSummary {
            title: "Onboarding review".into(),
            summary: "- The team finished the new onboarding flow and the results look good."
                .into(),
            decisions: vec!["Review with the sales team next week".into()],
            action_items: vec![ActionItem {
                owner: None,
                text: "Send the invite for Tuesday".into(),
            }],
        })));
        assert!(md.contains("## Summary\n"));
        assert!(md.contains("## Decisions\n"));
        assert!(md.contains("## Action items\n\n- [ ] Send the invite for Tuesday\n"));
        assert!(md.contains("## Transcript\n"));
    }

    #[test]
    fn yaml_quoting() {
        assert_eq!(yaml_scalar("Ich"), "Ich");
        assert_eq!(yaml_scalar("Speaker 2"), "Speaker 2");
        assert_eq!(yaml_scalar("14:03"), "\"14:03\"");
        assert_eq!(yaml_scalar("no"), "\"no\"");
        assert_eq!(yaml_scalar("A: B"), "\"A: B\"");
        assert_eq!(yaml_scalar("say \"hi\""), "say \"hi\"");
        assert_eq!(yaml_scalar("\"x"), "\"\\\"x\"");
    }

    #[test]
    fn duration_rounds_to_minutes() {
        assert_eq!(duration_label(0.0), "1 min");
        assert_eq!(duration_label(89.0), "1 min");
        assert_eq!(duration_label(91.0), "2 min");
        assert_eq!(duration_label(4320.0), "72 min");
    }

    #[test]
    fn bad_timestamp_degrades_gracefully() {
        let mut n = note(None);
        n.meta.started_at = "gestern".into();
        let md = render_markdown(&n);
        assert!(!md.contains("date:"));
        assert!(md.contains("# Meeting\n"));
        assert_eq!(note_file_stem(&n), "Meeting");
    }

    #[test]
    fn footer_ends_the_note_and_the_summary() {
        let n = note(Some(MeetingSummary {
            title: "Launch".into(),
            summary: "We move the launch.".into(),
            ..Default::default()
        }));
        let line = "Summarized by AI. Local, free, open source. Dictation app and meeting notetaker: sokki.org";
        let md = render_note(&n, &NoteOptions::default());
        // The very last line.
        assert!(md.ends_with(&format!("\n\n{line}\n")));
        let p = split_note(&md);
        assert!(p.summary.ends_with(&format!("We move the launch.\n\n{line}")));
        assert!(!p.transcript.contains("sokki.org"));

        // A cloud model made the summary: not "local, free".
        let md = render_note(
            &n,
            &NoteOptions {
                cloud_summary: true,
                ..Default::default()
            },
        );
        assert!(md.ends_with("\n\nSummarized by AI. Open source. Dictation app and meeting notetaker: sokki.org\n"));
        assert!(split_note(&md).summary.ends_with("meeting notetaker: sokki.org"));

        // Off: no footer anywhere.
        let md = render_note(
            &n,
            &NoteOptions {
                footer: false,
                ..Default::default()
            },
        );
        assert!(!md.contains("sokki.org"));

        // No summary: nothing was summarised, so no line at all.
        let md = render_markdown(&note(None));
        assert!(!md.contains("sokki.org"));
        assert_eq!(split_note(&md).summary, "_Keine Zusammenfassung erstellt._");
    }

    #[test]
    fn the_line_for_mails_is_the_notes_line() {
        let md = render_note(
            &note(Some(german_summary())),
            &NoteOptions::default(),
        );
        assert!(md.ends_with(&format!("\n{}\n", sokki_line(Language::German, false))));
        assert!(sokki_line(Language::English, true).starts_with("Summarized by AI. "));
    }

    #[test]
    fn old_footers_still_split_off() {
        let md = "---\ntype: meeting\n---\n\n# T\n\n## Summary\n\nDone.\n\n## Transcript\n\n**[00:01] Ich:** Hi.\n\nTranscribed on my Mac with sokki.org\n";
        let p = split_note(md);
        assert_eq!(p.summary, "## Summary\n\nDone.\n\nTranscribed on my Mac with sokki.org");
        assert_eq!(p.transcript, "[00:01] Ich: Hi.");
    }
}

/// Renames a speaker in an existing note: transcript labels, the
/// participants list and action-item owners. Generic labels ("Speaker 2")
/// are also replaced in running text, since they never occur as ordinary
/// words; a real name (e.g. "Ich") is only replaced in those structured spots.
pub fn rename_speaker_in_markdown(md: &str, from: &str, to: &str) -> String {
    if from == to || from.is_empty() {
        return md.to_string();
    }
    let generic = is_generic_label(from);
    let mut out = String::with_capacity(md.len());
    let mut in_frontmatter = false;
    for (i, line) in md.split_inclusive('\n').enumerate() {
        let bare = line.trim_end_matches('\n');
        if i == 0 && bare == "---" {
            in_frontmatter = true;
            out.push_str(line);
            continue;
        }
        if in_frontmatter {
            if bare == "---" {
                in_frontmatter = false;
            } else if bare.trim_start() == format!("- {from}") {
                let indent = &bare[..bare.len() - bare.trim_start().len()];
                out.push_str(&format!("{indent}- {to}"));
                out.push_str(&line[bare.len()..]);
                continue;
            }
            out.push_str(line);
            continue;
        }
        let mut l = bare
            .replace(&format!("] {from}:**"), &format!("] {to}:**"))
            .replace(&format!("**{from}**:"), &format!("**{to}**:"));
        if generic {
            l = replace_whole(&l, from, to);
        }
        out.push_str(&l);
        out.push_str(&line[bare.len()..]);
    }
    out
}

/// "Speaker 2", "Speaker 13", …
fn is_generic_label(s: &str) -> bool {
    s.strip_prefix("Speaker ")
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// Replaces `from` where it is not part of a longer word or number.
fn replace_whole(text: &str, from: &str, to: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(pos) = rest.find(from) {
        let before = rest[..pos].chars().next_back();
        let after = rest[pos + from.len()..].chars().next();
        let boundary = |c: Option<char>| c.map_or(true, |c| !c.is_alphanumeric());
        out.push_str(&rest[..pos]);
        if boundary(before) && boundary(after) {
            out.push_str(to);
        } else {
            out.push_str(from);
        }
        rest = &rest[pos + from.len()..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod rename_tests {
    use super::rename_speaker_in_markdown as rename;

    #[test]
    fn split_note_gives_summary_and_plain_transcript() {
        let md = "---\ntype: meeting\n---\n\n# T\n\n## Zusammenfassung\n\n- Punkt eins\n\n## Aufgaben\n\n- [ ] **Robin**: Mail\n\n## Transkript\n\n**[00:00] Robin:** Hallo.\n\n**[00:03] Speaker 2:** Hi.\n\n> [!info]- Automatisch korrigiert (1)\n> - „Lied“ → „Lead“\n";
        let p = crate::note::split_note(md);
        assert_eq!(
            p.summary,
            "## Zusammenfassung\n\n- Punkt eins\n\n## Aufgaben\n\n- [ ] **Robin**: Mail"
        );
        assert_eq!(
            p.transcript,
            "[00:00] Robin: Hallo.\n[00:03] Speaker 2: Hi."
        );
    }

    const NOTE: &str = "---\ntype: meeting\nparticipants:\n  - Ich\n  - Speaker 2\n  - Speaker 21\n---\n\n# T\n\n## Zusammenfassung\n\n- Speaker 2 stellt die Zahlen vor. Ich frage nach.\n\n## Aufgaben\n\n- [ ] **Speaker 2**: Bericht\n- [ ] **Ich**: Mail\n\n## Transkript\n\n**[00:00] Ich:** Hallo.\n\n**[00:03] Speaker 2:** Hi.\n\n**[00:05] Speaker 21:** Moin.\n";

    #[test]
    fn renames_generic_label_everywhere_but_not_longer_numbers() {
        let out = rename(NOTE, "Speaker 2", "Anna");
        assert!(out.contains("  - Anna\n"));
        assert!(out.contains("- Anna stellt"));
        assert!(out.contains("**Anna**: Bericht"));
        assert!(out.contains("**[00:03] Anna:**"));
        assert!(out.contains("Speaker 21"));
        assert!(!out.contains("Speaker 2:") && !out.contains("- Speaker 2\n"));
    }

    #[test]
    fn renames_real_name_only_in_structured_places() {
        let out = rename(NOTE, "Ich", "Robin");
        assert!(out.contains("  - Robin\n"));
        assert!(out.contains("**Robin**: Mail"));
        assert!(out.contains("**[00:00] Robin:**"));
        // Running text keeps the German word.
        assert!(out.contains("Ich frage nach."));
    }
}

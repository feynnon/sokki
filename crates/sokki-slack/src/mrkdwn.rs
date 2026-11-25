//! Sokki's Markdown summary as Slack `mrkdwn`, and the post built from it.
//!
//! Slack's message format is not Markdown: bold is `*x*`, italic `_x_`,
//! links `<url|text>`, no headings, and `&`, `<`, `>` must be escaped
//! (docs.slack.dev/messaging/formatting-message-text).

/// Converts Markdown (as Sokki writes summaries) to Slack mrkdwn.
pub fn from_markdown(md: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut in_code = false;
    let mut after_heading = false;
    for raw in md.lines() {
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            out.push("```".into());
            after_heading = false;
            continue;
        }
        if in_code {
            out.push(escape(line));
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            if !after_heading {
                out.push(String::new());
            }
            continue;
        }
        after_heading = false;
        if matches!(trimmed, "---" | "***" | "___") {
            continue;
        }
        if let Some(h) = heading(trimmed) {
            // Bold inside a bold heading would close it early.
            let text = inline(&h.replace("**", "").replace("__", ""));
            out.push(format!("*{text}*"));
            after_heading = true;
            continue;
        }
        if let Some(q) = trimmed
            .strip_prefix("> ")
            .or_else(|| trimmed.strip_prefix('>'))
        {
            out.push(format!(">{}", inline(q.trim_start())));
            continue;
        }
        let indent = (line.len() - trimmed.len()) / 2;
        let pad = "    ".repeat(indent);
        let (mark, rest) = if let Some(r) = trimmed
            .strip_prefix("- [ ] ")
            .or_else(|| trimmed.strip_prefix("* [ ] "))
        {
            ("☐ ", r)
        } else if let Some(r) = trimmed
            .strip_prefix("- [x] ")
            .or_else(|| trimmed.strip_prefix("- [X] "))
            .or_else(|| trimmed.strip_prefix("* [x] "))
        {
            ("☑ ", r)
        } else if let Some(r) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            (if indent == 0 { "• " } else { "◦ " }, r)
        } else {
            ("", trimmed)
        };
        let pad = if mark.is_empty() && indent > 0 {
            ""
        } else {
            pad.as_str()
        };
        out.push(format!("{pad}{mark}{}", inline(rest)));
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    while out.first().is_some_and(|l| l.is_empty()) {
        out.remove(0);
    }
    out.join("\n")
}

fn heading(line: &str) -> Option<&str> {
    let hashes = line.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) {
        line[hashes..].strip_prefix(' ').map(str::trim)
    } else {
        None
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Inline Markdown in one line, code spans left alone.
fn inline(s: &str) -> String {
    let mut out = String::new();
    for (i, part) in s.split('`').enumerate() {
        if i > 0 {
            out.push('`');
        }
        if i % 2 == 1 {
            out.push_str(&escape(part));
        } else {
            out.push_str(&emphasis(&links(&escape(part))));
        }
    }
    out
}

/// `[text](url)` → `<url|text>`.
fn links(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let parsed = after.find("](").and_then(|close| {
            let text = &after[..close];
            let tail = &after[close + 2..];
            tail.find(')')
                .map(|end| (text, &tail[..end], &tail[end + 1..]))
        });
        match parsed {
            Some((text, url, tail)) if !url.contains(' ') && !text.contains('[') => {
                out.push_str(&rest[..open]);
                out.push_str(&format!("<{url}|{text}>"));
                rest = tail;
            }
            _ => {
                out.push_str(&rest[..open + 1]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `**b**`/`__b__` → `*b*`, `*i*` → `_i_`, `~~s~~` → `~s~`.
fn emphasis(s: &str) -> String {
    const BOLD: char = '\u{1}';
    let s = s
        .replace("**", "\u{1}")
        .replace("__", "\u{1}")
        .replace("~~", "~");
    // Single stars that open (before a non-space) and close (after one).
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '*' && chars.get(i + 1).is_some_and(|n| !n.is_whitespace()) {
            if let Some(len) = chars[i + 1..]
                .iter()
                .enumerate()
                .position(|(j, &e)| e == '*' && !chars[i + j].is_whitespace())
            {
                out.push('_');
                out.extend(&chars[i + 1..i + 1 + len]);
                out.push('_');
                i += len + 2;
                continue;
            }
        }
        out.push(if c == BOLD { '*' } else { c });
        i += 1;
    }
    out
}

/// What goes to Slack for one meeting.
#[derive(Debug, Clone, PartialEq)]
pub struct Post {
    /// The message in the thread: title, summary, Sokki line.
    pub comment: String,
    pub filename: String,
    pub file_title: String,
    /// The transcript, one line per turn.
    pub file_content: String,
}

/// The post for a meeting: `summary` in Markdown (without the Sokki line),
/// `sokki_line` under it unless a supporter switched it off.
pub fn compose(
    title: &str,
    summary: &str,
    sokki_line: Option<&str>,
    transcript: &str,
    german: bool,
) -> Post {
    let mut comment = match title.trim() {
        "" => from_markdown(summary),
        t => format!("*{}*\n\n{}", escape(t), from_markdown(summary)),
    };
    if let Some(line) = sokki_line {
        comment.push_str("\n\n");
        comment.push_str(&escape(line));
    }
    let word = if german { "Transkript" } else { "Transcript" };
    let stem: String = title
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    let stem = stem.trim();
    let stem = if stem.is_empty() { "Huddle" } else { stem };
    Post {
        comment,
        filename: format!("{stem} – {word}.txt"),
        file_title: format!("{word}: {}", title.trim()),
        file_content: format!("{}\n\n{transcript}\n", title.trim()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sokki_summary_reads_well_in_slack() {
        let md = "## Zusammenfassung\n\n- Ada stellt die **Q3-Zahlen** vor.\n- Bob fragt nach *Kosten* & Zeit.\n  - Details folgen\n\n## Aufgaben\n\n- [ ] **Ada**: Bericht an <Bob>\n- [x] Termin steht\n";
        assert_eq!(
            from_markdown(md),
            "*Zusammenfassung*\n• Ada stellt die *Q3-Zahlen* vor.\n• Bob fragt nach _Kosten_ &amp; Zeit.\n    ◦ Details folgen\n\n*Aufgaben*\n☐ *Ada*: Bericht an &lt;Bob&gt;\n☑ Termin steht"
        );
    }

    #[test]
    fn links_code_and_quotes() {
        assert_eq!(
            from_markdown("Siehe [Doku](https://x.org/a?b=1&c=2) und `a*b*c`.\n> Zitat **fett**"),
            "Siehe <https://x.org/a?b=1&amp;c=2|Doku> und `a*b*c`.\n>Zitat *fett*"
        );
        assert_eq!(
            from_markdown("[kein link] (x) 5 * 3 = 15"),
            "[kein link] (x) 5 * 3 = 15"
        );
        assert_eq!(from_markdown("~~alt~~ neu"), "~alt~ neu");
        assert_eq!(from_markdown("### **Fett** im Titel"), "*Fett im Titel*");
    }

    #[test]
    fn the_post_has_title_summary_line_and_transcript() {
        let p = compose(
            "Planung: Q3",
            "## Summary\n\n- Done.",
            Some("Summarized by AI. Local, free, open source. Dictation app and meeting notetaker: sokki.org"),
            "[00:01] Ada: Hi.\n[00:03] Bob: Hello.",
            false,
        );
        assert_eq!(
            p.comment,
            "*Planung: Q3*\n\n*Summary*\n• Done.\n\nSummarized by AI. Local, free, open source. Dictation app and meeting notetaker: sokki.org"
        );
        assert_eq!(p.filename, "Planung Q3 – Transcript.txt");
        assert_eq!(p.file_title, "Transcript: Planung: Q3");
        assert!(p
            .file_content
            .starts_with("Planung: Q3\n\n[00:01] Ada: Hi."));

        let p = compose("", "Kurz.", None, "", true);
        assert_eq!(p.comment, "Kurz.");
        assert_eq!(p.filename, "Huddle – Transkript.txt");
    }
}

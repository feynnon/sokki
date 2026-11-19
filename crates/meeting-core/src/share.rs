//! A meeting note as one self-contained web page: title, date and summary,
//! no transcript. Anyone can open the file in a browser without Sokki. The
//! note's footer ("Transcribed on my Mac with sokki.org"), when it has one,
//! closes the page with a link to sokki.org.

use crate::note::{is_footer, split_note};

/// Where the footer's "sokki.org" points; `s` names the surface for counting.
pub const FOOTER_URL: &str = "https://sokki.org/?s=note";

/// The note's title: its first `# ` heading, after the front matter.
pub fn note_title(md: &str) -> Option<String> {
    md.lines()
        .find_map(|l| l.strip_prefix("# "))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// The whole page. `date` is shown under the title as given.
pub fn note_html(md: &str, fallback_title: &str, date: &str, lang: &str) -> String {
    let title = note_title(md).unwrap_or_else(|| fallback_title.to_string());
    let summary = split_note(md).summary;
    let body = summary_html(&summary);
    format!(
        r#"<!doctype html>
<html lang="{lang}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex">
<title>{title}</title>
<style>{STYLE}</style>
</head>
<body>
<main>
<h1>{title}</h1>
<p class="date">{date}</p>
{body}</main>
</body>
</html>
"#,
        lang = escape(lang),
        title = escape(&title),
        date = escape(date),
    )
}

/// The summary's Markdown as HTML: `## ` headings, bullets, tasks, **bold**,
/// paragraphs, and the footer line as a link.
fn summary_html(md: &str) -> String {
    let mut out = String::new();
    let mut list: Option<&str> = None;
    let close = |out: &mut String, list: &mut Option<&str>| {
        if list.take().is_some() {
            out.push_str("</ul>\n");
        }
    };
    for raw in md.lines() {
        let line = raw.trim_end();
        if line.trim().is_empty() {
            close(&mut out, &mut list);
            continue;
        }
        if let Some(h) = line.strip_prefix("## ") {
            close(&mut out, &mut list);
            out.push_str(&format!("<h2>{}</h2>\n", inline(h)));
            continue;
        }
        let task = line
            .strip_prefix("- [ ] ")
            .map(|t| (false, t))
            .or_else(|| line.strip_prefix("- [x] ").map(|t| (true, t)));
        let item = match task {
            Some((done, text)) => Some((if done { "task done" } else { "task" }, text)),
            None => line.strip_prefix("- ").map(|t| ("", t)),
        };
        if let Some((class, text)) = item {
            let kind = if class.is_empty() { "ul" } else { "tasks" };
            if list != Some(kind) {
                close(&mut out, &mut list);
                out.push_str(if kind == "ul" {
                    "<ul>\n"
                } else {
                    "<ul class=\"tasks\">\n"
                });
                list = Some(kind);
            }
            if class.is_empty() {
                out.push_str(&format!("<li>{}</li>\n", inline(text)));
            } else {
                out.push_str(&format!("<li class=\"{class}\">{}</li>\n", inline(text)));
            }
            continue;
        }
        close(&mut out, &mut list);
        if is_footer(line) {
            let text = escape(line.trim()).replace(
                "sokki.org",
                &format!("<a href=\"{FOOTER_URL}\">sokki.org</a>"),
            );
            out.push_str(&format!("<footer>{text}</footer>\n"));
            continue;
        }
        let plain = line
            .strip_prefix('_')
            .and_then(|l| l.strip_suffix('_'))
            .unwrap_or(line);
        out.push_str(&format!("<p>{}</p>\n", inline(plain)));
    }
    close(&mut out, &mut list);
    out
}

/// Escapes the text and turns `**bold**` into `<strong>`.
fn inline(text: &str) -> String {
    let mut out = String::new();
    for (i, part) in text.split("**").enumerate() {
        // Odd parts sit between a pair of "**"; an unpaired "**" stays as text.
        let paired = i % 2 == 1 && text.matches("**").count() > i;
        if paired {
            out.push_str(&format!("<strong>{}</strong>", escape(part)));
        } else {
            if i % 2 == 1 {
                out.push_str("**");
            }
            out.push_str(&escape(part));
        }
    }
    out
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const STYLE: &str = r#"
:root { color-scheme: light dark; --ink: #1d1d1f; --ink2: #48484a; --ink3: #8e8e93; --line: #e5e5ea; --bg: #ffffff; --accent: #3b6fd8; }
@media (prefers-color-scheme: dark) { :root { --ink: #ecedef; --ink2: #c7c7cc; --ink3: #8e8e93; --line: #2c2c2e; --bg: #141416; --accent: #8ab4ff; } }
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--ink2); font: 16px/1.6 -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif; }
main { max-width: 680px; margin: 0 auto; padding: 56px 20px 64px; }
h1 { margin: 0; color: var(--ink); font-size: 28px; line-height: 1.25; letter-spacing: -0.01em; }
.date { margin: 6px 0 32px; color: var(--ink3); font-size: 14px; }
h2 { margin: 32px 0 10px; color: var(--ink3); font-size: 12px; font-weight: 600; letter-spacing: 0.06em; text-transform: uppercase; }
p { margin: 0 0 12px; }
strong { color: var(--ink); font-weight: 600; }
ul { margin: 0 0 12px; padding-left: 20px; }
li { margin: 6px 0; }
ul.tasks { list-style: none; padding-left: 0; }
.task { position: relative; padding-left: 26px; }
.task::before { content: ""; position: absolute; left: 0; top: 5px; width: 14px; height: 14px; border: 1.5px solid var(--ink3); border-radius: 4px; }
.task.done::before { background: var(--accent); border-color: var(--accent); }
footer { margin-top: 48px; padding-top: 16px; border-top: 1px solid var(--line); color: var(--ink3); font-size: 13px; }
a { color: var(--accent); text-decoration: none; }
a:hover { text-decoration: underline; }
"#;

#[cfg(test)]
mod tests {
    use super::*;

    const NOTE: &str = "---\ndate: 2026-11-19\n---\n# Launch <plan>\n\n## Summary\n\nWe **agreed** on Friday.\n\n## Action items\n\n- [ ] **Anna**: write the post\n- [x] Book room\n\n## Transcript\n\n**[00:03] Anna:** secret words\n\nTranscribed on my Mac with sokki.org\n";

    #[test]
    fn page_has_title_summary_and_footer_but_no_transcript() {
        let html = note_html(NOTE, "Meeting", "19 Nov 2026, 10:00", "en");
        assert!(html.contains("<title>Launch &lt;plan&gt;</title>"));
        assert!(html.contains("<p>We <strong>agreed</strong> on Friday.</p>"));
        assert!(html.contains("<li class=\"task\"><strong>Anna</strong>: write the post</li>"));
        assert!(html.contains("<li class=\"task done\">Book room</li>"));
        assert!(html.contains(&format!("<a href=\"{FOOTER_URL}\">sokki.org</a>")));
        assert!(!html.contains("secret words"));
    }

    #[test]
    fn unpaired_bold_marker_stays_text() {
        assert_eq!(inline("a ** b"), "a ** b");
        assert_eq!(inline("**x** y"), "<strong>x</strong> y");
    }

    #[test]
    fn falls_back_to_given_title() {
        assert!(
            note_html("no heading", "Meeting 19 Nov", "", "de").contains("<h1>Meeting 19 Nov</h1>")
        );
    }
}

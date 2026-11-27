//! Write-back for hooks: a section added to a meeting's note, and an item
//! stored next to the meeting. Both change the truth (the note, the
//! meeting folder), then the index follows.

use anyhow::{bail, Context, Result};
use std::io::Write;
use std::path::PathBuf;

use crate::db::Index;
use crate::items::{verify_quote, TurnRef};
use crate::sync::{self, HookItem, Paths, ITEMS_FILE};

fn note_of(p: &Paths, meeting_id: &str) -> Result<PathBuf> {
    let r = sync::records(&p.meetings_dir)
        .into_iter()
        .find(|r| r.id == meeting_id)
        .with_context(|| format!("no meeting {meeting_id}. `sokki meetings` lists them"))?;
    let path = PathBuf::from(r.note_path.with_context(|| format!("meeting {meeting_id} has no note yet"))?);
    if !path.is_file() {
        bail!("the note of {meeting_id} is gone: {}", path.display());
    }
    Ok(path)
}

fn meeting_id(id: &str) -> Result<String> {
    let (kind, rf, _) = crate::query::parse_id(id)?;
    if kind != "meeting" {
        bail!("{id} is not a meeting (use its m: id)");
    }
    Ok(rf)
}

/// Section headings that belong to Sokki; hooks add their own.
fn reserved(name: &str) -> bool {
    !matches!(
        crate::parse::SectionKind::from_heading(name),
        crate::parse::SectionKind::Other(_)
    )
}

/// Adds `text` under `## <section>` in the meeting's note, before the
/// transcript (the section is made if missing). `replace` swaps the
/// section's text instead, so a hook that runs again doesn't repeat itself.
pub fn note_append(ix: &Index, p: &Paths, id: &str, section: &str, text: &str, replace: bool) -> Result<PathBuf> {
    let mid = meeting_id(id)?;
    let section = section.trim();
    if section.is_empty() || section.contains('\n') || section.starts_with('#') {
        bail!("--section needs a plain name, e.g. --section CRM");
    }
    if reserved(section) {
        bail!("{section:?} is one of Sokki's own sections; pick another name for the hook's text");
    }
    let text = text.trim_end();
    if text
        .lines()
        .any(|l| l.starts_with("# ") || l.starts_with("## ") || meeting_core::note::is_footer(l))
    {
        bail!("the text can't hold `#` or `##` headings (they would end the section); use `###` inside it");
    }
    if text.trim().is_empty() {
        bail!("nothing to add: pipe the text in, e.g. echo \"…\" | sokki note append {id} --section {section}");
    }
    let path = note_of(p, &mid)?;
    let md = std::fs::read_to_string(&path)?;
    let out = with_section(&md, section, text, replace);
    let tmp = path.with_extension("md.sokki-tmp");
    std::fs::write(&tmp, &out)?;
    std::fs::rename(&tmp, &path)?;
    sync::sync_meeting(ix, p, &mid)?;
    Ok(path)
}

/// The note with `text` in section `name` (see [`note_append`]).
pub fn with_section(md: &str, name: &str, text: &str, replace: bool) -> String {
    let lines: Vec<&str> = md.split_inclusive('\n').collect();
    let heading = format!("## {name}");
    let is_heading = |l: &str| l.trim_end() == heading;
    // A section ends at the next `## ` heading or at the Sokki line, so the
    // line stays the note's last.
    let next_heading = |from: usize| {
        (from..lines.len())
            .find(|&i| lines[i].starts_with("## ") || meeting_core::note::is_footer(lines[i]))
            .unwrap_or(lines.len())
    };
    let mut out = String::with_capacity(md.len() + text.len() + 32);
    if let Some(start) = lines.iter().position(|l| is_heading(l)) {
        let end = next_heading(start + 1);
        out.extend(lines[..=start].iter().copied());
        if !out.ends_with('\n') {
            out.push('\n');
        }
        let body: String = lines[start + 1..end].concat();
        let kept = if replace { "" } else { body.trim_end() };
        out.push('\n');
        if !kept.trim().is_empty() {
            out.push_str(kept.trim_start_matches('\n'));
            out.push_str("\n\n");
        }
        out.push_str(text);
        out.push_str("\n\n");
        out.extend(lines[end..].iter().copied());
        return out;
    }
    // Before the transcript, so it stays last; else at the end, before the
    // Sokki line.
    let at = lines
        .iter()
        .position(|l| {
            matches!(
                crate::parse::SectionKind::from_heading(l.trim_end().strip_prefix("## ").unwrap_or("x")),
                crate::parse::SectionKind::Transcript
            ) && l.starts_with("## ")
        })
        .or_else(|| {
            lines
                .iter()
                .rposition(|l| meeting_core::note::is_footer(l))
        })
        .unwrap_or(lines.len());
    out.extend(lines[..at].iter().copied());
    if !out.ends_with("\n\n") {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(&format!("{heading}\n\n{text}\n\n"));
    out.extend(lines[at..].iter().copied());
    out
}

/// What `item add` stored.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Added {
    pub cite: String,
    pub item: HookItem,
}

/// Stores an item a hook found, after checking its quote against the
/// transcript near `at_ms` (±60 s). Kinds are `custom:<name>`; tasks and
/// decisions come from the summary.
#[allow(clippy::too_many_arguments)]
pub fn item_add(
    ix: &Index,
    p: &Paths,
    id: &str,
    kind: &str,
    text: &str,
    quote: &str,
    at_ms: i64,
    owner: Option<&str>,
    by: &str,
) -> Result<Added> {
    let mid = meeting_id(id)?;
    // The item in the hook's own words: shown at "summaries" access, where
    // the quote (transcript text) is not.
    let text = text.trim();
    if text.is_empty() {
        bail!("--text is needed: the item in your own words (the quote stays its evidence)");
    }
    let name = kind.strip_prefix("custom:").unwrap_or("");
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        bail!("--kind must be custom:<name> (letters, digits, - or _), e.g. --kind custom:crm");
    }
    let note = note_of(p, &mid)?;
    sync::sync_meeting(ix, p, &mid)?;
    let md = std::fs::read_to_string(&note)?;
    let parsed = crate::parse::note(&md);
    if parsed.front.private {
        bail!("meeting {mid} is private");
    }
    // The turns with their exact ends, from the index.
    let mut stmt = ix.conn.prepare(
        "SELECT start_ms, coalesce(end_ms, start_ms), text FROM segments
          WHERE source_id = ?1 AND section = 'transcript' ORDER BY ord",
    )?;
    let rows: Vec<(i64, i64, String)> = stmt
        .query_map([format!("m:{mid}")], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let turns: Vec<TurnRef> = rows
        .iter()
        .map(|(s, e, t)| TurnRef {
            start_ms: *s,
            end_ms: *e,
            text: t,
        })
        .collect();
    // The turn where the quote was found: the item's cite, like search's.
    let at_ms = verify_quote(&turns, quote, at_ms, 60_000)?;
    let item = HookItem {
        kind: kind.to_string(),
        text: text.to_string(),
        owner: owner.map(str::to_string).filter(|o| !o.trim().is_empty()),
        quote: quote.trim().to_string(),
        at_ms,
        quote_verified: true,
        by: by.to_string(),
        added_at: crate::time::local_rfc3339(crate::time::now_ms()),
    };
    let dir = p.meetings_dir.join(&mid);
    std::fs::create_dir_all(&dir)?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(ITEMS_FILE))?;
    writeln!(f, "{}", serde_json::to_string(&item)?)?;
    drop(f);
    sync::sync_meeting(ix, p, &mid)?;
    Ok(Added {
        cite: crate::cite("m", &mid, Some(at_ms)),
        item,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTE: &str = "---\ntype: meeting\n---\n\n# T\n\n## Zusammenfassung\n\nS.\n\n## Transkript\n\n**[00:00] Ich:** Hallo.\n\nVon KI zusammengefasst. Lokal, kostenlos, Open Source. Diktier-App und Meeting-Notetaker: sokki.org\n";

    #[test]
    fn a_new_section_goes_before_the_transcript() {
        let out = with_section(NOTE, "CRM", "Kunde: Muster GmbH", false);
        let crm = out.find("## CRM\n\nKunde: Muster GmbH\n").unwrap();
        assert!(crm < out.find("## Transkript").unwrap());
        assert!(out.ends_with("sokki.org\n"));
        // Again: appended, then replaced.
        let twice = with_section(&out, "CRM", "Zweite Zeile", false);
        assert!(twice.contains("Kunde: Muster GmbH\n\nZweite Zeile\n\n## Transkript"), "{twice}");
        let replaced = with_section(&twice, "CRM", "Neu", true);
        assert!(replaced.contains("## CRM\n\nNeu\n\n## Transkript"), "{replaced}");
        assert!(!replaced.contains("Muster"));
    }

    #[test]
    fn the_sokki_line_stays_last_without_a_transcript() {
        let note = "---\ntype: meeting\n---\n\n# T\n\n## Zusammenfassung\n\nS.\n\nVon KI zusammengefasst. Lokal, kostenlos, Open Source. Diktier-App und Meeting-Notetaker: sokki.org\n";
        let once = with_section(note, "CRM", "A", false);
        let twice = with_section(&once, "CRM", "B", true);
        assert!(twice.contains("## CRM\n\nB\n\nVon KI"), "{twice}");
        assert!(twice.trim_end().ends_with("sokki.org"));
        let thrice = with_section(&twice, "CRM", "C", false);
        assert!(thrice.contains("B\n\nC\n\nVon KI"), "{thrice}");
        assert!(thrice.trim_end().ends_with("sokki.org"));
    }

    #[test]
    fn sokkis_own_sections_are_not_for_hooks() {
        assert!(reserved("Transkript"));
        assert!(reserved("Action items"));
        assert!(!reserved("CRM"));
    }
}

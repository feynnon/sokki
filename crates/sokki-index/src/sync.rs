//! Brings the index up to date with the files. Incremental: each source
//! has a fingerprint (sizes and modification times of its files, and the
//! record fields shown), so an unchanged meeting is not read again.

use anyhow::{Context, Result};
use meeting_core::note::{SegmentLine, SEGMENTS_FILE};
use meeting_core::TrackKind;
use rusqlite::{params, OpenFlags, OptionalExtension};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use crate::db::Index;
use crate::items::{self, TurnRef};
use crate::parse::{self, SectionKind};
use crate::time::{now_ms, rfc3339_ms};

/// Items hooks added to a meeting (`sokki item add`): the truth for them,
/// next to the meeting's audio. One JSON object per line.
pub const ITEMS_FILE: &str = "items.jsonl";
/// The person's own notes (own-notes session).
pub const OWN_NOTES_FILE: &str = "notes.md";

/// Where everything is.
#[derive(Debug, Clone)]
pub struct Paths {
    /// `index.sqlite`.
    pub index: PathBuf,
    /// One folder per meeting.
    pub meetings_dir: PathBuf,
    /// The dictation history (macOS `history.db`), if there is one.
    pub history_db: Option<PathBuf>,
    /// The name the person's own voice gets in notes ("Ich" by default).
    pub user_name: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncReport {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub unchanged: usize,
    /// Meetings marked `private: true`: left out.
    pub private: usize,
    /// Sources that could not be read, one line each; the rest is indexed.
    pub errors: Vec<String>,
    pub ms: u64,
}

/// A meeting as its record describes it (either platform's format).
#[derive(Debug, Clone, Default)]
pub struct Record {
    pub id: String,
    pub started_at: String,
    pub duration_s: f64,
    pub status: String,
    pub source_app: Option<String>,
    pub title: Option<String>,
    pub note_path: Option<String>,
    pub audio_deleted: bool,
    pub sample: bool,
    pub private: bool,
    pub online: Option<bool>,
}

fn record_from(v: &serde_json::Value) -> Option<Record> {
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
    let status = match v.get("status") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(o) => o
            .get("kind")
            .and_then(|k| k.as_str())
            .unwrap_or_default()
            .to_string(),
        None => String::new(),
    };
    Some(Record {
        id: s("id")?,
        started_at: s("started_at").unwrap_or_default(),
        duration_s: v.get("duration_s").and_then(|d| d.as_f64()).unwrap_or(0.0),
        status,
        source_app: s("source_app").filter(|a| !a.is_empty()),
        title: s("title").filter(|t| !t.is_empty()),
        note_path: s("note_path").filter(|p| !p.is_empty()),
        audio_deleted: v.get("audio_deleted").and_then(|b| b.as_bool()).unwrap_or(false),
        sample: v.get("sample").and_then(|b| b.as_bool()).unwrap_or(false),
        private: v.get("private").and_then(|b| b.as_bool()).unwrap_or(false),
        online: v.get("online").and_then(|b| b.as_bool()),
    })
}

/// Every meeting record: `meetings/index.json` (macOS) or
/// `meetings/<id>/meeting.json` (Linux).
pub fn records(meetings_dir: &Path) -> Vec<Record> {
    let index = meetings_dir.join("index.json");
    if let Ok(text) = std::fs::read_to_string(&index) {
        if let Ok(serde_json::Value::Array(list)) = serde_json::from_str(&text) {
            return list.iter().filter_map(record_from).collect();
        }
    }
    let mut out = Vec::new();
    if let Ok(dirs) = std::fs::read_dir(meetings_dir) {
        for d in dirs.flatten() {
            let file = d.path().join("meeting.json");
            if let Ok(text) = std::fs::read_to_string(&file) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    out.extend(record_from(&v));
                }
            }
        }
    }
    out
}

fn stamp(path: &Path) -> String {
    match std::fs::metadata(path) {
        Ok(m) => {
            let t = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            format!("{}:{t}", m.len())
        }
        Err(_) => "-".into(),
    }
}

fn hash(parts: &[&str]) -> String {
    // FNV-1a: enough to notice a change, not a security measure.
    let mut h: u64 = 0xcbf29ce484222325;
    for p in parts {
        for b in p.as_bytes().iter().chain(std::iter::once(&0u8)) {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    format!("{h:016x}")
}

fn meeting_fingerprint(p: &Paths, r: &Record, note: &Path) -> String {
    let dir = p.meetings_dir.join(&r.id);
    hash(&[
        &crate::db::SCHEMA.to_string(),
        &note.to_string_lossy(),
        &stamp(note),
        &stamp(&dir.join(SEGMENTS_FILE)),
        &stamp(&dir.join(ITEMS_FILE)),
        &r.started_at,
        &format!("{:.1}", r.duration_s),
        r.source_app.as_deref().unwrap_or(""),
        r.title.as_deref().unwrap_or(""),
        &format!("{}{}{:?}", r.audio_deleted, r.private, r.online),
        &p.user_name,
    ])
}

/// "Speaker 2", "Sprecher 3": a name nobody gave yet.
pub fn is_placeholder(name: &str) -> bool {
    let n = name.trim();
    ["Speaker ", "Sprecher "].iter().any(|p| {
        n.strip_prefix(p)
            .is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
    }) || n.is_empty()
}

/// Whether a speaker label is the person using this computer.
pub fn is_me(name: &str, user_name: &str) -> bool {
    let n = name.trim();
    n == user_name.trim() || matches!(n, "Ich" | "Me")
}

/// A name as an id: lower case, umlauts spelled out, `-` between words.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().to_lowercase().chars() {
        match c {
            'ä' => out.push_str("ae"),
            'ö' => out.push_str("oe"),
            'ü' => out.push_str("ue"),
            'ß' => out.push_str("ss"),
            c if c.is_alphanumeric() => out.push(c),
            _ => {
                if !out.ends_with('-') && !out.is_empty() {
                    out.push('-')
                }
            }
        }
    }
    out.trim_end_matches('-').to_string()
}

/// The person id for a speaker label: "me", a slug, or None for a
/// placeholder.
pub fn person_id(name: &str, user_name: &str) -> Option<String> {
    if is_me(name, user_name) {
        Some("me".into())
    } else if is_placeholder(name) {
        None
    } else {
        Some(slug(name)).filter(|s| !s.is_empty())
    }
}

fn track_name(t: TrackKind) -> &'static str {
    match t {
        TrackKind::Mic => "mic",
        TrackKind::System => "system",
    }
}

fn read_sidecar(dir: &Path) -> Vec<SegmentLine> {
    std::fs::read_to_string(dir.join(SEGMENTS_FILE))
        .map(|t| {
            t.lines()
                .filter_map(|l| serde_json::from_str::<SegmentLine>(l).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// One transcript turn as indexed.
#[derive(Debug, Clone)]
pub(crate) struct TurnRow {
    start_ms: i64,
    end_ms: Option<i64>,
    speaker: String,
    speaker_id: Option<String>,
    channel: Option<&'static str>,
    text: String,
}

/// Pairs the note's paragraphs with the sidecar's lines: by position when
/// both agree, else by the shown time (the person edited the transcript).
fn pair(paras: &[parse::Para], side: &[SegmentLine]) -> Vec<TurnRow> {
    let same_shape = paras.len() == side.len()
        && paras
            .iter()
            .zip(side)
            .all(|(p, s)| crate::mmss(p.start_ms) == crate::mmss(s.start_ms as i64));
    let mut from = 0usize;
    let mut rows = Vec::with_capacity(paras.len());
    for (i, p) in paras.iter().enumerate() {
        let line = if same_shape {
            side.get(i)
        } else {
            let hit = side[from.min(side.len())..]
                .iter()
                .position(|s| crate::mmss(s.start_ms as i64) == crate::mmss(p.start_ms))
                .map(|k| from + k);
            if let Some(k) = hit {
                from = k + 1;
            }
            hit.and_then(|k| side.get(k))
        };
        let next_start = paras.get(i + 1).map(|n| n.start_ms);
        rows.push(TurnRow {
            start_ms: line.map(|l| l.start_ms as i64).unwrap_or(p.start_ms),
            end_ms: line.map(|l| l.end_ms as i64).or(next_start),
            speaker: p.speaker.clone(),
            speaker_id: line.map(|l| l.speaker_id.clone()),
            channel: line.map(|l| track_name(l.track)),
            text: p.text.clone(),
        });
    }
    rows
}

struct Seg<'a> {
    section: &'a str,
    start_ms: Option<i64>,
    end_ms: Option<i64>,
    speaker: Option<&'a str>,
    speaker_id: Option<&'a str>,
    person_id: Option<String>,
    confirmed: Option<bool>,
    channel: Option<&'a str>,
    text: &'a str,
}

fn insert_seg(tx: &rusqlite::Transaction, source: &str, ord: &mut i64, s: Seg) -> Result<()> {
    if s.text.trim().is_empty() {
        return Ok(());
    }
    tx.execute(
        "INSERT INTO segments(source_id, ord, section, start_ms, end_ms, speaker, speaker_id, person_id, name_confirmed, channel, text)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            source,
            *ord,
            s.section,
            s.start_ms,
            s.end_ms,
            s.speaker,
            s.speaker_id,
            s.person_id,
            s.confirmed.map(|c| c as i64),
            s.channel,
            s.text.trim()
        ],
    )?;
    *ord += 1;
    Ok(())
}

/// A line of [`ITEMS_FILE`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HookItem {
    pub kind: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub quote: String,
    pub at_ms: i64,
    pub quote_verified: bool,
    /// The hook's name (`SOKKI_HOOK`), else "cli".
    pub by: String,
    /// RFC 3339.
    pub added_at: String,
}

pub fn read_items(dir: &Path) -> Vec<HookItem> {
    std::fs::read_to_string(dir.join(ITEMS_FILE))
        .map(|t| t.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
        .unwrap_or_default()
}

/// Indexes one meeting from its note (replacing what was there). Returns
/// false when it is private and was left out.
fn index_meeting(ix: &Index, p: &Paths, r: &Record, note_path: &Path, fp: &str) -> Result<bool> {
    let md = std::fs::read_to_string(note_path)
        .with_context(|| format!("reading {}", note_path.display()))?;
    let note = parse::note(&md);
    let source = format!("m:{}", r.id);
    if note.front.private || r.private {
        ix.delete_source(&source)?;
        ix.delete_source(&format!("n:{}", r.id))?;
        return Ok(false);
    }
    let dir = p.meetings_dir.join(&r.id);
    let turns = pair(&note.transcript(), &read_sidecar(&dir));
    let tx = ix.conn.unchecked_transaction()?;
    for t in ["segments", "items", "source_people"] {
        tx.execute(&format!("DELETE FROM {t} WHERE source_id=?1"), params![source])?;
    }
    let title = note.title.clone().or_else(|| r.title.clone());
    let app = r.source_app.clone().or_else(|| note.front.source_app.clone());
    let online = r
        .online
        .or_else(|| turns.iter().any(|t| t.channel == Some("system")).then_some(true));
    tx.execute(
        "INSERT OR REPLACE INTO sources(id, kind, ref, title, started_at, started_ms, duration_s, app, path, has_audio, online, fingerprint, indexed_ms)
         VALUES (?1, 'meeting', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            source,
            r.id,
            title,
            r.started_at,
            rfc3339_ms(&r.started_at),
            r.duration_s,
            app,
            note_path.to_string_lossy(),
            !r.audio_deleted,
            online,
            fp,
            now_ms()
        ],
    )?;
    let mut ord = 0i64;
    // The transcript, one turn per row.
    let mut talk: HashMap<String, (Option<String>, Option<&'static str>, i64)> = HashMap::new();
    for t in &turns {
        let pid = person_id(&t.speaker, &p.user_name);
        insert_seg(
            &tx,
            &source,
            &mut ord,
            Seg {
                section: "transcript",
                start_ms: Some(t.start_ms),
                end_ms: t.end_ms,
                speaker: Some(&t.speaker),
                speaker_id: t.speaker_id.as_deref(),
                person_id: pid,
                confirmed: Some(!is_placeholder(&t.speaker)),
                channel: t.channel,
                text: &t.text,
            },
        )?;
        let e = talk
            .entry(t.speaker.clone())
            .or_insert((t.speaker_id.clone(), t.channel, 0));
        e.2 += t.end_ms.map(|end| (end - t.start_ms).max(0)).unwrap_or(0);
    }
    for (speaker, (sid, channel, ms)) in &talk {
        tx.execute(
            "INSERT OR REPLACE INTO source_people(source_id, person_id, speaker, speaker_id, channel, name_confirmed, talk_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                source,
                person_id(speaker, &p.user_name),
                speaker,
                sid,
                channel,
                !is_placeholder(speaker),
                ms
            ],
        )?;
    }
    let refs: Vec<TurnRef> = turns
        .iter()
        .map(|t| TurnRef {
            start_ms: t.start_ms,
            end_ms: t.end_ms.unwrap_or(t.start_ms),
            text: &t.text,
        })
        .collect();
    // Summary, decisions, tasks, own notes, and sections hooks added.
    let mut item_ord = 0i64;
    for section in &note.sections {
        match &section.kind {
            SectionKind::Transcript => {}
            SectionKind::Tasks => {
                for task in note.tasks() {
                    let shown = match &task.owner {
                        Some(o) => format!("{o}: {}", task.text),
                        None => task.text.clone(),
                    };
                    let ev = items::evidence(&refs, &task.text, task.at_ms);
                    insert_seg(
                        &tx,
                        &source,
                        &mut ord,
                        Seg {
                            section: "tasks",
                            start_ms: ev.as_ref().map(|e| e.at_ms),
                            end_ms: None,
                            speaker: None,
                            speaker_id: None,
                            person_id: task.owner.as_deref().and_then(|o| person_id(o, &p.user_name)),
                            confirmed: None,
                            channel: None,
                            text: &shown,
                        },
                    )?;
                    insert_item(
                        &tx,
                        &source,
                        &mut item_ord,
                        ItemRow {
                            kind: "task",
                            text: &task.text,
                            owner: task.owner.as_deref(),
                            owner_person_id: task.owner.as_deref().and_then(|o| person_id(o, &p.user_name)),
                            status: Some(if task.done { "done" } else { "open" }),
                            evidence: ev,
                            added_by: "summary",
                        },
                    )?;
                }
            }
            SectionKind::Decisions => {
                for d in note.decisions() {
                    let ev = items::evidence(&refs, &d.text, d.at_ms);
                    insert_seg(
                        &tx,
                        &source,
                        &mut ord,
                        Seg {
                            section: "decisions",
                            start_ms: ev.as_ref().map(|e| e.at_ms),
                            end_ms: None,
                            speaker: None,
                            speaker_id: None,
                            person_id: None,
                            confirmed: None,
                            channel: None,
                            text: &d.text,
                        },
                    )?;
                    insert_item(
                        &tx,
                        &source,
                        &mut item_ord,
                        ItemRow {
                            kind: "decision",
                            text: &d.text,
                            owner: None,
                            owner_person_id: None,
                            status: None,
                            evidence: ev,
                            added_by: "summary",
                        },
                    )?;
                }
            }
            kind => {
                let name = kind.name();
                let notes = *kind == SectionKind::Notes;
                // The person's lines are indexed from `notes.md` (`n:<id>`,
                // with their times); from the note only Sokki's additions.
                let raw = notes && dir.join(OWN_NOTES_FILE).is_file();
                for line in parse::bullets(&section.lines) {
                    if raw && !line.nested {
                        continue;
                    }
                    let mine = notes && !line.nested;
                    insert_seg(
                        &tx,
                        &source,
                        &mut ord,
                        Seg {
                            section: &name,
                            start_ms: line.at_ms,
                            end_ms: None,
                            speaker: None,
                            speaker_id: None,
                            person_id: mine.then(|| "me".to_string()),
                            confirmed: None,
                            channel: None,
                            text: &line.text,
                        },
                    )?;
                }
            }
        }
    }
    // Items hooks added.
    for item in read_items(&dir) {
        insert_seg(
            &tx,
            &source,
            &mut ord,
            Seg {
                section: &item.kind,
                start_ms: Some(item.at_ms),
                end_ms: None,
                speaker: None,
                speaker_id: None,
                person_id: None,
                confirmed: None,
                channel: None,
                text: &item.text,
            },
        )?;
        let owner_pid = item.owner.as_deref().and_then(|o| person_id(o, &p.user_name));
        insert_item(
            &tx,
            &source,
            &mut item_ord,
            ItemRow {
                kind: &item.kind,
                text: &item.text,
                owner: item.owner.as_deref(),
                owner_person_id: owner_pid,
                status: None,
                evidence: Some(items::Evidence {
                    at_ms: item.at_ms,
                    quote: item.quote.clone(),
                    verified: item.quote_verified,
                    origin: "hook",
                }),
                added_by: &item.by,
            },
        )?;
    }
    tx.commit()?;
    Ok(true)
}

struct ItemRow<'a> {
    kind: &'a str,
    text: &'a str,
    owner: Option<&'a str>,
    owner_person_id: Option<String>,
    status: Option<&'a str>,
    evidence: Option<items::Evidence>,
    added_by: &'a str,
}

fn insert_item(tx: &rusqlite::Transaction, source: &str, ord: &mut i64, i: ItemRow) -> Result<()> {
    tx.execute(
        "INSERT INTO items(source_id, ord, kind, text, owner, owner_person_id, status, evidence_ms, evidence_quote, quote_verified, evidence_origin, added_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            source,
            *ord,
            i.kind,
            i.text,
            i.owner,
            i.owner_person_id,
            i.status,
            i.evidence.as_ref().map(|e| e.at_ms),
            i.evidence.as_ref().map(|e| e.quote.as_str()).filter(|q| !q.is_empty()),
            i.evidence.as_ref().is_some_and(|e| e.verified),
            i.evidence.as_ref().map(|e| e.origin),
            i.added_by,
        ],
    )?;
    *ord += 1;
    Ok(())
}

/// Indexes a meeting's own notes (`notes.md`) as source `n:<id>`.
fn index_own_notes(ix: &Index, id: &str, path: &Path, fp: &str, started: Option<&Record>) -> Result<()> {
    let md = std::fs::read_to_string(path)?;
    let (front, title, lines) = parse::own_notes(&md);
    let source = format!("n:{id}");
    let tx = ix.conn.unchecked_transaction()?;
    tx.execute("DELETE FROM segments WHERE source_id=?1", params![source])?;
    let started_at = started
        .map(|r| r.started_at.clone())
        .or_else(|| {
            md.lines()
                .find_map(|l| l.strip_prefix("started_at:"))
                .map(|v| v.trim().trim_matches('"').to_string())
        })
        .unwrap_or_default();
    let _ = front;
    tx.execute(
        "INSERT OR REPLACE INTO sources(id, kind, ref, title, started_at, started_ms, duration_s, app, path, has_audio, online, fingerprint, indexed_ms)
         VALUES (?1, 'notes', ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL, ?9, ?10)",
        params![
            source,
            id,
            title.or_else(|| started.and_then(|r| r.title.clone())),
            started_at,
            rfc3339_ms(&started_at),
            started.map(|r| r.duration_s),
            started.and_then(|r| r.source_app.clone()),
            path.to_string_lossy(),
            fp,
            now_ms()
        ],
    )?;
    let mut ord = 0;
    for l in &lines {
        insert_seg(
            &tx,
            &source,
            &mut ord,
            Seg {
                section: "notes",
                start_ms: l.at_ms,
                end_ms: None,
                speaker: None,
                speaker_id: None,
                person_id: (!l.nested).then(|| "me".to_string()),
                confirmed: None,
                channel: None,
                text: &l.text,
            },
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// One dictation row from history.db.
struct Dictation {
    id: i64,
    at_ms: i64,
    raw: String,
    cleaned: Option<String>,
    language: Option<String>,
    target_app: Option<String>,
}

fn dictations(path: &Path) -> Result<Vec<Dictation>> {
    let conn = rusqlite::Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    let cols: HashSet<String> = conn
        .prepare("SELECT name FROM pragma_table_info('transcription_history')")?
        .query_map([], |r| r.get::<_, String>(0))?
        .flatten()
        .collect();
    if cols.is_empty() {
        return Ok(Vec::new());
    }
    let opt = |c: &str| {
        if cols.contains(c) {
            c.to_string()
        } else {
            "NULL".to_string()
        }
    };
    // Dictations only: not meeting pieces, not transcribed files.
    let mut filter = String::from("transcription_text != ''");
    if cols.contains("meeting_id") {
        filter.push_str(" AND meeting_id IS NULL");
    }
    if cols.contains("source_name") {
        filter.push_str(" AND source_name IS NULL");
    }
    let sql = format!(
        "SELECT id, timestamp, transcription_text, {}, {}, {} FROM transcription_history WHERE {filter}",
        opt("post_processed_text"),
        opt("language"),
        opt("target_app")
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Dictation {
                id: r.get(0)?,
                at_ms: r.get::<_, i64>(1)? * 1000,
                raw: r.get(2)?,
                cleaned: r.get::<_, Option<String>>(3)?.filter(|c| !c.trim().is_empty()),
                language: r.get(4)?,
                target_app: r.get::<_, Option<String>>(5)?.filter(|a| !a.trim().is_empty()),
            })
        })?
        .flatten()
        .collect();
    Ok(rows)
}

fn index_dictation(ix: &Index, d: &Dictation, fp: &str) -> Result<()> {
    let source = format!("d:{}", d.id);
    let tx = ix.conn.unchecked_transaction()?;
    tx.execute("DELETE FROM segments WHERE source_id=?1", params![source])?;
    let started_at = crate::time::local_rfc3339(d.at_ms);
    tx.execute(
        "INSERT OR REPLACE INTO sources(id, kind, ref, title, started_at, started_ms, duration_s, app, path, has_audio, online, fingerprint, indexed_ms)
         VALUES (?1, 'dictation', ?2, NULL, ?3, ?4, NULL, ?5, NULL, NULL, NULL, ?6, ?7)",
        params![source, d.id.to_string(), started_at, d.at_ms, d.target_app, fp, now_ms()],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO dictations(source_id, history_id, at_ms, target_app, raw, cleaned, language)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![source, d.id, d.at_ms, d.target_app, d.raw, d.cleaned, d.language],
    )?;
    let text = d.cleaned.as_deref().unwrap_or(&d.raw);
    let mut ord = 0;
    insert_seg(
        &tx,
        &source,
        &mut ord,
        Seg {
            section: "dictation",
            start_ms: None,
            end_ms: None,
            speaker: None,
            speaker_id: None,
            person_id: Some("me".into()),
            confirmed: None,
            channel: None,
            text,
        },
    )?;
    tx.commit()?;
    Ok(())
}

fn fingerprints(ix: &Index) -> Result<HashMap<String, String>> {
    let mut stmt = ix.conn.prepare("SELECT id, fingerprint FROM sources")?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .flatten()
        .collect();
    Ok(rows)
}

fn note_path_of(r: &Record) -> Option<PathBuf> {
    let p = PathBuf::from(r.note_path.as_deref()?);
    p.is_file().then_some(p)
}

/// The meetings the index should hold.
fn wanted_meetings(p: &Paths) -> Vec<(Record, PathBuf)> {
    records(&p.meetings_dir)
        .into_iter()
        .filter(|r| !r.sample && r.status != "recording")
        .filter_map(|r| note_path_of(&r).map(|n| (r, n)))
        .collect()
}

/// Brings the whole index up to date.
pub fn sync(ix: &Index, p: &Paths) -> Result<SyncReport> {
    let began = Instant::now();
    let mut report = SyncReport::default();
    let mut known = fingerprints(ix)?;
    let mut seen: HashSet<String> = HashSet::new();
    let all = records(&p.meetings_dir);
    let by_id: HashMap<&str, &Record> = all.iter().map(|r| (r.id.as_str(), r)).collect();

    for (r, note) in wanted_meetings(p) {
        let id = format!("m:{}", r.id);
        seen.insert(id.clone());
        let fp = meeting_fingerprint(p, &r, &note);
        let before = known.remove(&id);
        if before.as_deref() == Some(fp.as_str()) {
            report.unchanged += 1;
            continue;
        }
        match index_meeting(ix, p, &r, &note, &fp) {
            Ok(true) if before.is_some() => report.updated += 1,
            Ok(true) => report.added += 1,
            Ok(false) => {
                report.private += 1;
                seen.remove(&id);
                // Its rows were just deleted: people and the file follow.
                if before.is_some() {
                    report.removed += 1;
                }
            }
            Err(e) => report.errors.push(format!("{}: {e:#}", r.id)),
        }
    }

    // Own notes, with or without a recording.
    if let Ok(dirs) = std::fs::read_dir(&p.meetings_dir) {
        for d in dirs.flatten() {
            let path = d.path().join(OWN_NOTES_FILE);
            if !path.is_file() {
                continue;
            }
            let Some(mid) = d.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let rec = by_id.get(mid.as_str()).copied();
            if rec.is_some_and(|r| r.sample || r.private) || own_notes_private(&path) {
                continue;
            }
            // A meeting note marked private hides its own notes too.
            if rec
                .and_then(note_path_of)
                .and_then(|n| std::fs::read_to_string(n).ok())
                .is_some_and(|md| parse::note(&md).front.private)
            {
                continue;
            }
            let id = format!("n:{mid}");
            seen.insert(id.clone());
            let fp = hash(&[&crate::db::SCHEMA.to_string(), &stamp(&path)]);
            let before = known.remove(&id);
            if before.as_deref() == Some(fp.as_str()) {
                report.unchanged += 1;
                continue;
            }
            match index_own_notes(ix, &mid, &path, &fp, rec) {
                Ok(()) if before.is_some() => report.updated += 1,
                Ok(()) => report.added += 1,
                Err(e) => report.errors.push(format!("{mid} notes: {e:#}")),
            }
        }
    }

    // Dictations.
    if let Some(db) = p.history_db.as_deref().filter(|d| d.is_file()) {
        match dictations(db) {
            Ok(list) => {
                for d in list {
                    let id = format!("d:{}", d.id);
                    seen.insert(id.clone());
                    let fp = hash(&[
                        &crate::db::SCHEMA.to_string(),
                        &d.at_ms.to_string(),
                        &d.raw,
                        d.cleaned.as_deref().unwrap_or(""),
                        d.target_app.as_deref().unwrap_or(""),
                    ]);
                    let before = known.remove(&id);
                    if before.as_deref() == Some(fp.as_str()) {
                        report.unchanged += 1;
                        continue;
                    }
                    match index_dictation(ix, &d, &fp) {
                        Ok(()) if before.is_some() => report.updated += 1,
                        Ok(()) => report.added += 1,
                        Err(e) => report.errors.push(format!("dictation {}: {e:#}", d.id)),
                    }
                }
            }
            Err(e) => report.errors.push(format!("history.db: {e:#}")),
        }
    }

    // What is gone (deleted, private now, or a dictation retention removed).
    for id in known.keys() {
        if !seen.contains(id) {
            ix.delete_source(id)?;
            report.removed += 1;
        }
    }
    if report.added + report.updated + report.removed > 0 {
        refresh_people(ix)?;
    }
    if report.removed > 0 {
        ix.vacuum();
    }
    ix.set_meta("last_sync_ms", &now_ms().to_string())?;
    report.ms = began.elapsed().as_millis() as u64;
    Ok(report)
}

fn own_notes_private(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .map(|md| parse::own_notes(&md).0.private)
        .unwrap_or(false)
}

/// Indexes one meeting now (after its note was written, renamed or
/// deleted), so hooks that run next find it. Removes it when it is gone.
pub fn sync_meeting(ix: &Index, p: &Paths, meeting_id: &str) -> Result<()> {
    let id = format!("m:{meeting_id}");
    let found = wanted_meetings(p).into_iter().find(|(r, _)| r.id == meeting_id);
    match found {
        Some((r, note)) => {
            let fp = meeting_fingerprint(p, &r, &note);
            let before: Option<String> = ix
                .conn
                .query_row("SELECT fingerprint FROM sources WHERE id=?1", params![id], |r| {
                    r.get(0)
                })
                .optional()?;
            if before.as_deref() != Some(fp.as_str()) && !index_meeting(ix, p, &r, &note, &fp)? {
                ix.vacuum();
            }
        }
        None => {
            ix.delete_source(&id)?;
            ix.delete_source(&format!("n:{meeting_id}"))?;
            ix.vacuum();
        }
    }
    refresh_people(ix)?;
    Ok(())
}

/// Empties the index and builds it again from the files.
pub fn rebuild(ix: &Index, p: &Paths) -> Result<SyncReport> {
    ix.clear()?;
    sync(ix, p)
}

/// Recomputes `people` from who spoke in which meeting.
fn refresh_people(ix: &Index) -> Result<()> {
    ix.conn.execute_batch(
        "DELETE FROM people;
         INSERT INTO people(person_id, name, is_me, meetings, talk_ms, first_ms, last_ms)
         SELECT sp.person_id,
                (SELECT sp2.speaker FROM source_people sp2 JOIN sources s2 ON s2.id = sp2.source_id
                  WHERE sp2.person_id = sp.person_id ORDER BY s2.started_ms DESC LIMIT 1),
                sp.person_id = 'me',
                count(DISTINCT sp.source_id),
                sum(sp.talk_ms),
                min(s.started_ms),
                max(s.started_ms)
           FROM source_people sp JOIN sources s ON s.id = sp.source_id
          WHERE sp.person_id IS NOT NULL
          GROUP BY sp.person_id;",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn person_ids() {
        assert_eq!(person_id("Ich", "Ich").as_deref(), Some("me"));
        assert_eq!(person_id("Alex", "Alex").as_deref(), Some("me"));
        assert_eq!(person_id("Speaker 2", "Ich"), None);
        assert_eq!(person_id("Jörg Müller-Groß", "Ich").as_deref(), Some("joerg-mueller-gross"));
        assert!(is_placeholder("Sprecher 12"));
        assert!(!is_placeholder("Speaker"));
    }

    #[test]
    fn paragraphs_pair_with_the_sidecar_by_time_after_edits() {
        let paras = vec![
            parse::Para { start_ms: 0, speaker: "Ich".into(), text: "a".into() },
            parse::Para { start_ms: 65_000, speaker: "Anna".into(), text: "b".into() },
        ];
        let line = |s: u64, e: u64, t: TrackKind, id: &str| SegmentLine {
            start_ms: s,
            end_ms: e,
            track: t,
            speaker_id: id.into(),
            speaker: "x".into(),
        };
        // The person deleted the paragraph at 00:30 from the note.
        let side = vec![
            line(200, 20_000, TrackKind::Mic, "s1"),
            line(30_000, 40_000, TrackKind::System, "s2"),
            line(65_300, 70_000, TrackKind::System, "s3"),
        ];
        let rows = pair(&paras, &side);
        assert_eq!(rows[0].start_ms, 200);
        assert_eq!(rows[0].channel, Some("mic"));
        assert_eq!(rows[1].speaker_id.as_deref(), Some("s3"));
        assert_eq!(rows[1].end_ms, Some(70_000));
        // Without a sidecar: times from the note, end at the next turn.
        let rows = pair(&paras, &[]);
        assert_eq!(rows[0].end_ms, Some(65_000));
        assert_eq!(rows[0].channel, None);
    }
}

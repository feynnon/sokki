//! Reading the index: what `sokki search`, `get`, `meetings`, `tasks`,
//! `decisions`, `people` and `index status` print. Every answer is JSON in
//! one shape, `{query, filters_applied, results, next_cursor, truncated,
//! est_tokens}`, cut at a token budget so an agent's context stays small.

use anyhow::{bail, Context, Result};
use rusqlite::types::Value as Sql;
use rusqlite::{params, params_from_iter};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

use crate::db::Index;
use crate::sync::slug;

/// Which full-text table answers a search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Whole words (FTS5 unicode61, accents folded, prefix match).
    Words,
    /// Three-letter pieces (FTS5 trigram): finds words inside compounds.
    Trigram,
    /// Both, fused by rank (reciprocal rank fusion). The default.
    Both,
}

impl Mode {
    pub fn parse(s: &str) -> Result<Mode> {
        Ok(match s {
            "words" | "unicode61" | "lex" => Mode::Words,
            "trigram" => Mode::Trigram,
            "both" | "hybrid" => Mode::Both,
            _ => bail!("unknown --mode {s:?}: use words, trigram or both"),
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Words => "words",
            Mode::Trigram => "trigram",
            Mode::Both => "both",
        }
    }
}

/// Filters shared by the commands.
#[derive(Debug, Clone, Default)]
pub struct ListOpts {
    pub person: Option<String>,
    pub since_ms: Option<i64>,
    pub until_ms: Option<i64>,
    pub app: Option<String>,
    pub n: usize,
    pub cursor: usize,
    pub max_tokens: usize,
    /// The flags as typed, echoed in `filters_applied`.
    pub shown: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct SearchOpts {
    pub query: String,
    pub kind: Option<String>,
    pub section: Option<String>,
    pub mode: Mode,
    pub list: ListOpts,
}

#[derive(Debug, Clone, Default)]
pub struct GetOpts {
    pub at_ms: Option<i64>,
    pub window_ms: i64,
    pub max_tokens: usize,
    pub cursor: usize,
}

/// Tokens a text costs an agent, roughly (four characters each).
pub fn est_tokens(s: &str) -> usize {
    s.chars().count().div_ceil(4)
}

fn value_tokens(v: &Value) -> usize {
    est_tokens(&v.to_string())
}

fn filters_json(shown: &[(String, String)]) -> Value {
    let mut m = Map::new();
    for (k, v) in shown {
        m.insert(k.clone(), Value::String(v.clone()));
    }
    Value::Object(m)
}

/// The words of a query, without FTS syntax.
fn terms(q: &str) -> Vec<String> {
    q.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .collect()
}

fn words_query(q: &str) -> Option<String> {
    let t = terms(q);
    (!t.is_empty()).then(|| {
        t.iter()
            .map(|t| format!("\"{t}\"*"))
            .collect::<Vec<_>>()
            .join(" OR ")
    })
}

fn trigram_query(q: &str) -> Option<String> {
    let t: Vec<String> = terms(q)
        .into_iter()
        .filter(|t| t.chars().count() >= 3)
        .collect();
    (!t.is_empty()).then(|| {
        t.iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(" OR ")
    })
}

/// SQL conditions for the filters, on `s` (segments) and `src` (sources).
struct Where {
    sql: String,
    args: Vec<Sql>,
}

impl Where {
    fn new() -> Where {
        Where {
            sql: String::new(),
            args: Vec::new(),
        }
    }
    fn add(&mut self, cond: &str, args: Vec<Sql>) {
        // `?` placeholders numbered after the ones already there.
        let mut c = String::new();
        let mut n = self.args.len();
        for ch in cond.chars() {
            if ch == '?' {
                n += 1;
                c.push_str(&format!("?{}", n + 1));
            } else {
                c.push(ch);
            }
        }
        self.sql.push_str(" AND ");
        self.sql.push_str(&c);
        self.args.extend(args);
    }
}

fn person_value(p: &str) -> (String, String) {
    let p = p.trim();
    let id = if p.eq_ignore_ascii_case("me") || p.eq_ignore_ascii_case("ich") {
        "me".to_string()
    } else {
        slug(p)
    };
    (id, format!("%{}%", p.to_lowercase()))
}

struct Filters<'a> {
    person: Option<&'a str>,
    since: Option<i64>,
    until: Option<i64>,
    app: Option<&'a str>,
    kind: Option<&'a str>,
    section: Option<&'a str>,
}

fn search_where(f: &Filters) -> Where {
    let mut w = Where::new();
    if let Some(p) = f.person {
        let (id, like) = person_value(p);
        // A turn they said; in a summary, a meeting they were in.
        w.add(
            "((s.section = 'transcript' AND (s.person_id = ? OR lower(s.speaker) LIKE ?)) \
              OR (s.section != 'transcript' AND (s.person_id = ? OR s.source_id IN \
                 (SELECT source_id FROM source_people WHERE person_id = ? OR lower(speaker) LIKE ?))))",
            vec![
                Sql::Text(id.clone()),
                Sql::Text(like.clone()),
                Sql::Text(id.clone()),
                Sql::Text(id),
                Sql::Text(like),
            ],
        );
    }
    if let Some(t) = f.since {
        w.add("src.started_ms >= ?", vec![Sql::Integer(t)]);
    }
    if let Some(t) = f.until {
        w.add("src.started_ms <= ?", vec![Sql::Integer(t)]);
    }
    if let Some(a) = f.app {
        w.add(
            "lower(src.app) LIKE ?",
            vec![Sql::Text(format!("%{}%", a.to_lowercase()))],
        );
    }
    if let Some(k) = f.kind {
        w.add("src.kind = ?", vec![Sql::Text(k.to_string())]);
    }
    if let Some(s) = f.section {
        w.add("s.section = ?", vec![Sql::Text(s.to_lowercase())]);
    }
    w
}

/// Ranked segment ids from one full-text table.
fn ranked(ix: &Index, table: &str, fts: &str, w: &Where) -> Result<Vec<i64>> {
    let sql = format!(
        "SELECT s.id FROM {table} JOIN segments s ON s.id = {table}.rowid \
         JOIN sources src ON src.id = s.source_id \
         WHERE {table} MATCH ?1{} ORDER BY bm25({table}) LIMIT 500",
        w.sql
    );
    let mut args = vec![Sql::Text(fts.to_string())];
    args.extend(w.args.iter().cloned());
    let mut stmt = ix.conn.prepare(&sql)?;
    let ids = stmt
        .query_map(params_from_iter(args), |r| r.get::<_, i64>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(ids)
}

fn fused(ix: &Index, q: &str, mode: Mode, w: &Where) -> Result<Vec<i64>> {
    let mut lists = Vec::new();
    if matches!(mode, Mode::Words | Mode::Both) {
        if let Some(fts) = words_query(q) {
            lists.push(ranked(ix, "segments_fts", &fts, w)?);
        }
    }
    if matches!(mode, Mode::Trigram | Mode::Both) {
        if let Some(fts) = trigram_query(q) {
            lists.push(ranked(ix, "segments_tri", &fts, w)?);
        }
    }
    if lists.len() == 1 {
        return Ok(lists.pop().unwrap_or_default());
    }
    // Reciprocal rank fusion (k = 60, the usual constant).
    let mut score: HashMap<i64, f64> = HashMap::new();
    let mut first: HashMap<i64, usize> = HashMap::new();
    for list in &lists {
        for (rank, id) in list.iter().enumerate() {
            *score.entry(*id).or_default() += 1.0 / (60.0 + rank as f64 + 1.0);
            let f = first.entry(*id).or_insert(rank);
            *f = (*f).min(rank);
        }
    }
    let mut ids: Vec<i64> = score.keys().copied().collect();
    ids.sort_by(|a, b| {
        score[b]
            .partial_cmp(&score[a])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(first[a].cmp(&first[b]))
            // Same score and rank: the older row first, so runs repeat.
            .then(a.cmp(b))
    });
    Ok(ids)
}

/// One citable result, as every command prints it.
fn segment_json(ix: &Index, id: i64) -> Result<Option<Value>> {
    let row = ix.conn.query_row(
        "SELECT s.source_id, src.kind, src.ref, s.section, s.start_ms, s.end_ms, s.speaker, s.speaker_id,
                s.person_id, s.name_confirmed, s.channel, s.text, src.title, src.started_at, src.app
           FROM segments s JOIN sources src ON src.id = s.source_id WHERE s.id = ?1",
        params![id],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<i64>>(4)?,
                r.get::<_, Option<i64>>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, Option<String>>(8)?,
                r.get::<_, Option<bool>>(9)?,
                r.get::<_, Option<String>>(10)?,
                r.get::<_, String>(11)?,
                r.get::<_, Option<String>>(12)?,
                r.get::<_, Option<String>>(13)?,
                r.get::<_, Option<String>>(14)?,
            ))
        },
    )?;
    let (source, kind, rf, section, start, end, speaker, speaker_id, pid, confirmed, channel, text, title, started_at, app) =
        row;
    Ok(Some(json!({
        "cite": unit_cite(&kind, &rf, &section, start),
        "source": source,
        "kind": kind,
        "section": section,
        "title": title,
        "date": started_at,
        "app": app,
        "at": start.map(crate::mmss),
        "start_ms": start,
        "end_ms": end,
        "speaker": speaker,
        "speaker_id": speaker_id,
        "person_id": pid,
        "name_confirmed": confirmed,
        "channel": channel,
        "text": text,
    })))
}

/// `m:<id>@mm:ss`, `m:<id>#summary`, `n:<id>@mm:ss`, `d:<id>`.
pub fn unit_cite(kind: &str, rf: &str, section: &str, start_ms: Option<i64>) -> String {
    let prefix = match kind {
        "dictation" => return format!("d:{rf}"),
        "notes" => "n",
        _ => "m",
    };
    match start_ms {
        Some(ms) => crate::cite(prefix, rf, Some(ms)),
        None if kind == "notes" => format!("{prefix}:{rf}"),
        None => format!("{prefix}:{rf}#{section}"),
    }
}

/// Fills results up to `n` and the token budget, from `cursor` on.
fn page(
    items: Vec<Value>,
    cursor: usize,
    n: usize,
    max_tokens: usize,
) -> (Vec<Value>, Option<String>, bool, usize) {
    let total = items.len();
    let mut out = Vec::new();
    let mut tokens = 0usize;
    let mut truncated = false;
    let mut next = cursor;
    for mut v in items.into_iter().skip(cursor) {
        if out.len() >= n {
            break;
        }
        let cost = value_tokens(&v);
        if tokens + cost > max_tokens {
            if out.is_empty() {
                // One result alone is over the budget: cut its text.
                let room = max_tokens.saturating_sub(tokens + 60) * 4;
                if let Some(t) = v.get("text").and_then(Value::as_str) {
                    let cut: String = t.chars().take(room).collect();
                    v["text"] = Value::String(format!("{cut}…"));
                }
                tokens += value_tokens(&v);
                out.push(v);
                next += 1;
            }
            truncated = true;
            break;
        }
        tokens += cost;
        out.push(v);
        next += 1;
    }
    let next_cursor = (next < total).then(|| next.to_string());
    (out, next_cursor, truncated, tokens)
}

fn envelope(query: Value, shown: &[(String, String)], page: (Vec<Value>, Option<String>, bool, usize)) -> Value {
    let (results, next_cursor, truncated, est) = page;
    json!({
        "query": query,
        "filters_applied": filters_json(shown),
        "results": results,
        "next_cursor": next_cursor,
        "truncated": truncated,
        "est_tokens": est,
    })
}

/// `sokki search`.
pub fn search(ix: &Index, o: &SearchOpts) -> Result<Value> {
    if terms(&o.query).is_empty() {
        bail!("search needs words to look for, e.g. sokki search \"Rahmenvertrag Kündigung\"");
    }
    let f = Filters {
        person: o.list.person.as_deref(),
        since: o.list.since_ms,
        until: o.list.until_ms,
        app: o.list.app.as_deref(),
        kind: o.kind.as_deref(),
        section: o.section.as_deref(),
    };
    let ids = fused(ix, &o.query, o.mode, &search_where(&f))?;
    let mut items = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(v) = segment_json(ix, id)? {
            items.push(v);
        }
    }
    let mut out = envelope(
        Value::String(o.query.clone()),
        &o.list.shown,
        page(items, o.list.cursor, o.list.n, o.list.max_tokens),
    );
    out["mode"] = Value::String(o.mode.as_str().into());
    if out["results"].as_array().is_some_and(|r| r.is_empty()) {
        out["hint"] = Value::String(zero_hint(ix, o, &f)?);
    }
    Ok(out)
}

/// For zero results: which filter to drop, by how many results each one
/// alone would give back.
fn zero_hint(ix: &Index, o: &SearchOpts, f: &Filters) -> Result<String> {
    let mut tips = Vec::new();
    let tries: [(&str, Filters); 6] = [
        ("--person", Filters { person: None, ..*f }),
        ("--since", Filters { since: None, ..*f }),
        ("--until", Filters { until: None, ..*f }),
        ("--app", Filters { app: None, ..*f }),
        ("--kind", Filters { kind: None, ..*f }),
        ("--section", Filters { section: None, ..*f }),
    ];
    let set = [
        f.person.is_some(),
        f.since.is_some(),
        f.until.is_some(),
        f.app.is_some(),
        f.kind.is_some(),
        f.section.is_some(),
    ];
    for ((flag, without), on) in tries.iter().zip(set) {
        if !on {
            continue;
        }
        let n = fused(ix, &o.query, o.mode, &search_where(without))?.len();
        if n > 0 {
            tips.push(format!("without {flag}: {n}"));
        }
    }
    if !tips.is_empty() {
        return Ok(format!("No results with these filters. Relax one: {}.", tips.join(", ")));
    }
    let short = terms(&o.query).iter().any(|t| t.chars().count() < 3);
    Ok(if short {
        "No results. Words under three letters only match whole words; try longer or fewer words.".into()
    } else {
        "No results for any of these words. Try fewer, other or shorter words (a part of a compound works: 'vertrag').".into()
    })
}

/// The id forms `get` takes: `m:<id>`, `<id>`, `n:<id>`, `d:<n>`, each
/// optionally `@mm:ss`, or `#section` (ignored).
pub fn parse_id(id: &str) -> Result<(String, String, Option<i64>)> {
    let id = id.trim();
    let (id, at) = match id.rsplit_once('@') {
        Some((i, t)) => (i, Some(crate::time::parse_mmss(t)?)),
        None => (id, None),
    };
    let id = id.split('#').next().unwrap_or(id);
    let (kind, rf) = match id.split_once(':') {
        Some(("m", r)) => ("meeting", r),
        Some(("n", r)) => ("notes", r),
        Some(("d", r)) => ("dictation", r),
        Some((p, _)) if p.len() == 1 => bail!("unknown id prefix {p}: use m:, n: or d:"),
        _ => ("meeting", id),
    };
    if rf.is_empty() {
        bail!("no id given");
    }
    Ok((kind.to_string(), rf.to_string(), at))
}

fn source_json(ix: &Index, source: &str) -> Result<Option<Value>> {
    let mut stmt = ix.conn.prepare(
        "SELECT kind, ref, title, started_at, duration_s, app, path, has_audio, online FROM sources WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![source])?;
    let Some(r) = rows.next()? else {
        return Ok(None);
    };
    let kind: String = r.get(0)?;
    let rf: String = r.get(1)?;
    Ok(Some(json!({
        "id": rf,
        "cite": unit_cite(&kind, &rf, "", None).trim_end_matches('#').to_string(),
        "kind": kind,
        "title": r.get::<_, Option<String>>(2)?,
        "date": r.get::<_, Option<String>>(3)?,
        "duration_s": r.get::<_, Option<f64>>(4)?,
        "app": r.get::<_, Option<String>>(5)?,
        "path": r.get::<_, Option<String>>(6)?,
        "has_audio": r.get::<_, Option<bool>>(7)?,
        "online": r.get::<_, Option<bool>>(8)?,
    })))
}

fn people_of(ix: &Index, source: &str) -> Result<Vec<Value>> {
    let mut stmt = ix.conn.prepare(
        "SELECT speaker, speaker_id, person_id, name_confirmed, channel, talk_ms FROM source_people
          WHERE source_id = ?1 ORDER BY talk_ms DESC",
    )?;
    let rows = stmt
        .query_map(params![source], |r| {
            Ok(json!({
                "speaker": r.get::<_, String>(0)?,
                "speaker_id": r.get::<_, Option<String>>(1)?,
                "person_id": r.get::<_, Option<String>>(2)?,
                "name_confirmed": r.get::<_, bool>(3)?,
                "channel": r.get::<_, Option<String>>(4)?,
                "talk_s": r.get::<_, i64>(5)? / 1000,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn items_of(ix: &Index, source: &str, kind_like: &str) -> Result<Vec<Value>> {
    let mut stmt = ix.conn.prepare(
        "SELECT i.kind, i.text, i.owner, i.owner_person_id, i.status, i.evidence_ms, i.evidence_quote,
                i.quote_verified, i.evidence_origin, i.added_by, src.ref
           FROM items i JOIN sources src ON src.id = i.source_id
          WHERE i.source_id = ?1 AND i.kind LIKE ?2 ORDER BY i.ord",
    )?;
    let rows = stmt
        .query_map(params![source, kind_like], item_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn item_row(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    let kind: String = r.get(0)?;
    let ev_ms: Option<i64> = r.get(5)?;
    let rf: String = r.get(10)?;
    let section = match kind.as_str() {
        "task" => "tasks",
        "decision" => "decisions",
        k => k,
    };
    let evidence = ev_ms.map(|ms| {
        json!({
            "cite": crate::cite("m", &rf, Some(ms)),
            "quote": r.get::<_, Option<String>>(6).unwrap_or(None),
            "verified": r.get::<_, bool>(7).unwrap_or(false),
            "origin": r.get::<_, Option<String>>(8).unwrap_or(None),
        })
    });
    Ok(json!({
        "cite": unit_cite("meeting", &rf, section, ev_ms),
        "meeting": rf,
        "kind": kind,
        "text": r.get::<_, String>(1)?,
        "owner": r.get::<_, Option<String>>(2)?,
        "owner_person_id": r.get::<_, Option<String>>(3)?,
        "status": r.get::<_, Option<String>>(4)?,
        "evidence": evidence,
        "added_by": r.get::<_, Option<String>>(9)?,
    }))
}

fn section_lines(ix: &Index, source: &str) -> Result<Vec<(String, Value)>> {
    let mut stmt = ix.conn.prepare("SELECT id, section FROM segments WHERE source_id = ?1 ORDER BY ord")?;
    let ids: Vec<(i64, String)> = stmt
        .query_map(params![source], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut out = Vec::new();
    for (id, section) in ids {
        if let Some(v) = segment_json(ix, id)? {
            out.push((section, v));
        }
    }
    Ok(out)
}

/// A segment without the fields its meeting already has.
fn slim(mut v: Value) -> Value {
    if let Some(m) = v.as_object_mut() {
        for k in ["source", "kind", "title", "date", "app"] {
            m.remove(k);
        }
        m.retain(|_, v| !v.is_null());
    }
    v
}

/// `sokki get`.
pub fn get(ix: &Index, id: &str, o: &GetOpts) -> Result<Value> {
    let (kind, rf, at_from_id) = parse_id(id)?;
    let at = o.at_ms.or(at_from_id);
    let prefix = match kind.as_str() {
        "notes" => "n",
        "dictation" => "d",
        _ => "m",
    };
    let source = format!("{prefix}:{rf}");
    let meta = source_json(ix, &source)?.with_context(|| {
        format!("nothing indexed as {source}. `sokki meetings` lists the meetings; `sokki index status` shows what is indexed")
    })?;
    let mut out = json!({ "query": id, "filters_applied": {}, "source": meta });
    if kind == "dictation" {
        let d = ix.conn.query_row(
            "SELECT at_ms, target_app, raw, cleaned, language FROM dictations WHERE source_id = ?1",
            params![source],
            |r| {
                Ok(json!({
                    "cite": format!("d:{rf}"),
                    "at": crate::time::local_rfc3339(r.get::<_, i64>(0)?),
                    "target_app": r.get::<_, Option<String>>(1)?,
                    "raw": r.get::<_, String>(2)?,
                    "cleaned": r.get::<_, Option<String>>(3)?,
                    "language": r.get::<_, Option<String>>(4)?,
                }))
            },
        )?;
        let est = value_tokens(&d);
        out["results"] = json!([d]);
        out["next_cursor"] = Value::Null;
        out["truncated"] = json!(false);
        out["est_tokens"] = json!(est);
        return Ok(out);
    }
    let lines = section_lines(ix, &source)?;
    let (transcript, rest): (Vec<_>, Vec<_>) = lines.into_iter().partition(|(s, _)| s == "transcript");
    let mut transcript: Vec<Value> = transcript.into_iter().map(|(_, v)| slim(v)).collect();
    if let Some(t) = at {
        let w = o.window_ms;
        transcript.retain(|v| {
            let s = v["start_ms"].as_i64().unwrap_or(0);
            let e = v["end_ms"].as_i64().unwrap_or(s);
            s <= t + w && e >= t - w
        });
        out["filters_applied"] = json!({ "at": crate::mmss(t), "window": format!("{}s", w / 1000) });
    }
    let mut sections: Map<String, Value> = Map::new();
    for (section, v) in rest {
        if section == "tasks" || section == "decisions" || section.starts_with("custom:") {
            continue; // as items below, with their evidence
        }
        let entry = sections.entry(section).or_insert_with(|| json!([]));
        if let Some(a) = entry.as_array_mut() {
            a.push(slim(v));
        }
    }
    if kind == "meeting" && at.is_none() {
        out["people"] = json!(people_of(ix, &source)?);
        out["tasks"] = json!(items_of(ix, &source, "task")?);
        out["decisions"] = json!(items_of(ix, &source, "decision")?);
        out["items"] = json!(items_of(ix, &source, "custom:%")?);
        out["sections"] = Value::Object(sections);
    } else if kind == "notes" {
        out["sections"] = Value::Object(sections);
    }
    let fixed = value_tokens(&out);
    let budget = o.max_tokens.saturating_sub(fixed).max(200);
    let paged = page(transcript, o.cursor, usize::MAX, budget);
    out["results"] = json!(paged.0);
    out["next_cursor"] = json!(paged.1);
    out["truncated"] = json!(paged.2);
    out["est_tokens"] = json!(fixed + paged.3);
    Ok(out)
}

fn list_where(o: &ListOpts) -> Where {
    let mut w = Where::new();
    if let Some(p) = &o.person {
        let (id, like) = person_value(p);
        w.add(
            "src.id IN (SELECT source_id FROM source_people WHERE person_id = ? OR lower(speaker) LIKE ?)",
            vec![Sql::Text(id), Sql::Text(like)],
        );
    }
    if let Some(t) = o.since_ms {
        w.add("src.started_ms >= ?", vec![Sql::Integer(t)]);
    }
    if let Some(t) = o.until_ms {
        w.add("src.started_ms <= ?", vec![Sql::Integer(t)]);
    }
    if let Some(a) = &o.app {
        w.add("lower(src.app) LIKE ?", vec![Sql::Text(format!("%{}%", a.to_lowercase()))]);
    }
    w
}

fn zero_list_hint(o: &ListOpts, what: &str) -> Option<String> {
    let set: Vec<&str> = o.shown.iter().map(|(k, _)| k.as_str()).collect();
    (!set.is_empty()).then(|| format!("No {what} with {}. Drop or widen one of them.", set.join(", ")))
}

/// `sokki meetings`: metadata only, newest first.
pub fn meetings(ix: &Index, o: &ListOpts) -> Result<Value> {
    let w = list_where(o);
    let sql = format!(
        "SELECT src.id FROM sources src WHERE src.kind = 'meeting'{} ORDER BY src.started_ms DESC",
        shift(&w.sql)
    );
    let mut stmt = ix.conn.prepare(&sql)?;
    let ids: Vec<String> = stmt
        .query_map(params_from_iter(w.args.iter()), |r| r.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut items = Vec::new();
    for id in ids {
        let Some(mut m) = source_json(ix, &id)? else { continue };
        let people: Vec<Value> = people_of(ix, &id)?
            .into_iter()
            .map(|p| json!({"name": p["speaker"], "person_id": p["person_id"], "name_confirmed": p["name_confirmed"]}))
            .collect();
        m["people"] = json!(people);
        let (open, all): (i64, i64) = ix.conn.query_row(
            "SELECT count(*) FILTER (WHERE status = 'open'), count(*) FROM items WHERE source_id = ?1 AND kind = 'task'",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let has_summary: bool = ix.conn.query_row(
            "SELECT count(*) > 0 FROM segments WHERE source_id = ?1 AND section = 'summary'",
            params![id],
            |r| r.get(0),
        )?;
        m["has_summary"] = json!(has_summary);
        m["tasks_open"] = json!(open);
        m["tasks"] = json!(all);
        items.push(m);
    }
    let mut out = envelope(Value::Null, &o.shown, page(items, o.cursor, o.n, o.max_tokens));
    if out["results"].as_array().is_some_and(|r| r.is_empty()) {
        out["hint"] = json!(zero_list_hint(o, "meetings").unwrap_or_else(|| "No meetings indexed yet.".into()));
    }
    Ok(out)
}

/// `sokki tasks` / `sokki decisions` (and hook items by kind).
pub fn items(
    ix: &Index,
    kind: &str,
    owner: Option<&str>,
    status: Option<&str>,
    text: Option<&str>,
    o: &ListOpts,
) -> Result<Value> {
    let mut w = list_where(o);
    w.add("i.kind = ?", vec![Sql::Text(kind.to_string())]);
    if let Some(s) = status {
        w.add("i.status = ?", vec![Sql::Text(s.to_string())]);
    }
    if let Some(owner) = owner {
        let (id, like) = person_value(owner);
        w.add(
            "(i.owner_person_id = ? OR lower(i.owner) LIKE ?)",
            vec![Sql::Text(id), Sql::Text(like)],
        );
    }
    if let Some(t) = text {
        w.add("lower(i.text) LIKE ?", vec![Sql::Text(format!("%{}%", t.to_lowercase()))]);
    }
    let sql = format!(
        "SELECT i.kind, i.text, i.owner, i.owner_person_id, i.status, i.evidence_ms, i.evidence_quote,
                i.quote_verified, i.evidence_origin, i.added_by, src.ref, src.title, src.started_at
           FROM items i JOIN sources src ON src.id = i.source_id
          WHERE 1=1{} ORDER BY src.started_ms DESC, i.ord",
        shift(&w.sql)
    );
    let mut stmt = ix.conn.prepare(&sql)?;
    let rows: Vec<Value> = stmt
        .query_map(params_from_iter(w.args.iter()), |r| {
            let mut v = item_row(r)?;
            v["title"] = json!(r.get::<_, Option<String>>(11)?);
            v["date"] = json!(r.get::<_, Option<String>>(12)?);
            Ok(v)
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let what = if kind == "task" { "tasks" } else if kind == "decision" { "decisions" } else { "items" };
    let mut out = envelope(Value::Null, &o.shown, page(rows, o.cursor, o.n, o.max_tokens));
    if out["results"].as_array().is_some_and(|r| r.is_empty()) {
        out["hint"] = json!(zero_list_hint(o, what).unwrap_or_else(|| format!("No {what} indexed.")));
    }
    Ok(out)
}

/// `Where` numbers its placeholders from ?2; for SQL without a query
/// parameter, from ?1.
fn shift(sql: &str) -> String {
    let mut out = String::new();
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '?' {
            let mut n = String::new();
            while let Some(d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                n.push(*d);
                chars.next();
            }
            let k: usize = n.parse().unwrap_or(2);
            out.push_str(&format!("?{}", k - 1));
        } else {
            out.push(c);
        }
    }
    out
}

/// `sokki people`.
pub fn people(ix: &Index, o: &ListOpts) -> Result<Value> {
    let mut stmt = ix.conn.prepare(
        "SELECT person_id, name, is_me, meetings, talk_ms, first_ms, last_ms FROM people ORDER BY is_me DESC, meetings DESC, name",
    )?;
    let rows: Vec<Value> = stmt
        .query_map([], |r| {
            Ok(json!({
                "person_id": r.get::<_, String>(0)?,
                "name": r.get::<_, String>(1)?,
                "is_me": r.get::<_, bool>(2)?,
                "meetings": r.get::<_, i64>(3)?,
                "talk_min": (r.get::<_, i64>(4)? as f64 / 60_000.0 * 10.0).round() / 10.0,
                "first_seen": r.get::<_, Option<i64>>(5)?.map(crate::time::local_rfc3339),
                "last_seen": r.get::<_, Option<i64>>(6)?.map(crate::time::local_rfc3339),
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(envelope(Value::Null, &o.shown, page(rows, o.cursor, o.n, o.max_tokens)))
}

/// `sokki index status`.
pub fn status(ix: &Index) -> Result<Value> {
    let count = |sql: &str| -> Result<i64> { Ok(ix.conn.query_row(sql, [], |r| r.get(0))?) };
    let size = std::fs::metadata(&ix.path).map(|m| m.len()).unwrap_or(0);
    Ok(json!({
        "path": ix.path.to_string_lossy(),
        "schema": crate::db::SCHEMA,
        "bytes": size,
        "meetings": count("SELECT count(*) FROM sources WHERE kind='meeting'")?,
        "own_notes": count("SELECT count(*) FROM sources WHERE kind='notes'")?,
        "dictations": count("SELECT count(*) FROM sources WHERE kind='dictation'")?,
        "segments": count("SELECT count(*) FROM segments")?,
        "transcript_turns": count("SELECT count(*) FROM segments WHERE section='transcript'")?,
        "turns_with_exact_times": count("SELECT count(*) FROM segments WHERE section='transcript' AND channel IS NOT NULL")?,
        "tasks": count("SELECT count(*) FROM items WHERE kind='task'")?,
        "decisions": count("SELECT count(*) FROM items WHERE kind='decision'")?,
        "hook_items": count("SELECT count(*) FROM items WHERE kind LIKE 'custom:%'")?,
        "people": count("SELECT count(*) FROM people")?,
        "last_sync": ix.meta("last_sync_ms").and_then(|v| v.parse().ok()).map(crate::time::local_rfc3339),
        "search": "fts5 unicode61 (remove_diacritics 2) + trigram (remove_diacritics 1), fused by rank",
        "secure_delete": true,
    }))
}

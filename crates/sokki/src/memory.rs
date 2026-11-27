//! `sokki search`, `get`, `meetings`, `tasks`, `decisions`, `people`,
//! `index`, `note append` and `item add`: everything Sokki recorded, for
//! scripts, hooks and agents. They read `index.sqlite` directly (brought up
//! to date first), so they work while Sokki is closed.
//!
//! Off until the person allows it (`agent_access`): what an agent reads may
//! leave the computer when the agent runs in the cloud. Every read is
//! logged locally (time, caller, command, ids returned; no content).

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use sokki_index::query::{self, GetOpts, ListOpts, Mode, SearchOpts};
use sokki_index::{Index, Paths, Scope};
use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;

use crate::args::Args;

/// What the platform provides.
pub struct Env {
    pub paths: Paths,
    /// The setting (`agent_access`), narrowed by `SOKKI_SCOPE` if set.
    pub scope: Scope,
    /// How to change the setting, for messages.
    pub how_to_allow: &'static str,
    /// `agent-access.log`.
    pub audit_log: PathBuf,
}

pub const HELP: &str = "\
Searching what Sokki recorded (off until you allow it: agent_access):
  sokki search QUERY [--person P] [--since 14d|DATE] [--until DATE] [--kind meeting|notes|dictation]
                     [--app APP] [--section transcript|summary|decisions|tasks|notes|NAME]
                     [--mode both|words|trigram] [-n 10] [--max-tokens 4000] [--cursor C] [--json]
  sokki get ID[@mm:ss] [--window 2m] [--max-tokens 4000] [--cursor C] [--json]
                     ID: m:<meeting>, n:<meeting> (own notes), d:<dictation>
  sokki meetings [--person P] [--since …] [--until …] [--app APP] [-n 20] [--json]
  sokki tasks [--owner me|NAME] [--status open|done|all] [--person P] [--since …] [--json]
  sokki decisions [--since …] [--query TEXT] [--person P] [--json]
  sokki people [--json]
  sokki index status|rebuild [--json]
Writing back (for hooks):
  sokki note append ID --section NAME [--replace]          the text comes from stdin
  sokki item add ID --kind custom:NAME --text T --quote \"…\" --at mm:ss [--owner NAME] [--json]
";

const LIST_FLAGS: [&str; 7] = ["--person", "--since", "--until", "--app", "-n", "--cursor", "--max-tokens"];

fn scope_needed(env: &Env) -> Result<()> {
    if env.scope == Scope::Off {
        bail!(
            "Sokki shares nothing with scripts and agents yet (agent access is off). The person allows it, typing in a terminal:\n  {}\n\
             summaries: meetings, summaries, decisions, tasks, own notes; transcripts: plus transcripts; dictations: plus dictations.\n\
             What an agent in the cloud reads leaves this computer.",
            env.how_to_allow
        );
    }
    Ok(())
}

fn open(env: &Env) -> Result<Index> {
    let ix = Index::open(&env.paths.index)?;
    match sokki_index::sync(&ix, &env.paths) {
        Ok(r) => {
            for e in &r.errors {
                eprintln!("sokki: not indexed: {e}");
            }
        }
        Err(e) => eprintln!("sokki: the index may be behind: {e:#}"),
    }
    Ok(ix)
}

fn list_opts(a: &Args, n: usize) -> Result<ListOpts> {
    let now = sokki_index::time::now_ms();
    Ok(ListOpts {
        person: a.get("--person").map(str::to_string),
        since_ms: a.get("--since").map(|s| sokki_index::time::parse_since(s, now)).transpose()?,
        until_ms: a.get("--until").map(|s| sokki_index::time::parse_until(s, now)).transpose()?,
        app: a.get("--app").map(str::to_string),
        n: a.number("-n", n)?,
        cursor: a.number("--cursor", 0)?,
        max_tokens: a.number("--max-tokens", 4000)?,
        shown: a.shown(&["-n", "--cursor", "--max-tokens"]),
    })
}

fn caller() -> String {
    let ppid = unsafe { libc::getppid() };
    std::process::Command::new("ps")
        .args(["-o", "comm=", "-p", &ppid.to_string()])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|s| s.rsplit('/').next().unwrap_or(&s).to_string())
        .unwrap_or_else(|| ppid.to_string())
}

/// One line per read: when, who asked, what, which ids came back.
fn audit(env: &Env, cmd: &str, out: &Value) {
    let mut ids: Vec<String> = Vec::new();
    if let Some(r) = out["results"].as_array() {
        for v in r {
            if let Some(c) = v["cite"].as_str() {
                ids.push(c.to_string());
            }
        }
    }
    if let Some(c) = out["source"]["cite"].as_str() {
        ids.insert(0, c.to_string());
    }
    let line = json!({
        "at": sokki_index::time::local_rfc3339(sokki_index::time::now_ms()),
        "caller": caller(),
        "cmd": cmd,
        "query": out["query"],
        "filters": out["filters_applied"],
        "scope": env.scope.as_str(),
        "ids": ids,
    });
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&env.audit_log)
    {
        let _ = writeln!(f, "{line}");
    }
}

fn print_json(v: &Value) -> Result<()> {
    println!("{}", serde_json::to_string(v)?);
    Ok(())
}

fn one_line(s: &str, max: usize) -> String {
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max).collect::<String>())
    } else {
        s
    }
}

fn str_of(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

fn footer(out: &Value) {
    if let Some(c) = out["next_cursor"].as_str() {
        println!("… more: add --cursor {c}");
    }
    if let Some(h) = out["hint"].as_str() {
        println!("{h}");
    }
}

/// Runs one index command. `words` starts with the command name.
pub fn run(env: &Env, words: &[&str]) -> Result<()> {
    let cmd = words.first().copied().unwrap_or("");
    let rest = &words[1..];
    match cmd {
        "search" => search(env, rest),
        "get" => get(env, rest),
        "meetings" => meetings(env, rest),
        "tasks" => items(env, rest, "task"),
        "decisions" => items(env, rest, "decision"),
        "people" => people(env, rest),
        "index" => index(env, rest),
        "note" => note(env, rest),
        "item" => item(env, rest),
        _ => bail!("unknown command {cmd}"),
    }
}

fn search(env: &Env, words: &[&str]) -> Result<()> {
    let mut values: Vec<&str> = LIST_FLAGS.to_vec();
    values.extend(["--kind", "--section", "--mode"]);
    let a = Args::parse("search", words, &values, &["--json"])?;
    scope_needed(env)?;
    let query = a.pos.join(" ");
    if query.trim().is_empty() {
        bail!("usage: sokki search QUERY [flags]   (sokki help)");
    }
    let kind = a.get("--kind").map(str::to_string);
    if let Some(k) = &kind {
        if !matches!(k.as_str(), "meeting" | "notes" | "dictation") {
            bail!("--kind is meeting, notes or dictation, not {k:?}");
        }
    }
    let o = SearchOpts {
        query,
        kind,
        section: a.get("--section").map(str::to_string),
        mode: a.get("--mode").map(Mode::parse).transpose()?.unwrap_or(Mode::Both),
        list: list_opts(&a, 10)?,
    };
    let ix = open(env)?;
    let out = query::search(&ix, &o, env.scope)?;
    audit(env, "search", &out);
    if a.on("--json") {
        return print_json(&out);
    }
    for r in out["results"].as_array().into_iter().flatten() {
        let who = match r["speaker"].as_str() {
            Some(s) => format!("{s}: "),
            None => format!("[{}] ", str_of(&r["section"])),
        };
        println!("{}  {who}{}", str_of(&r["cite"]), one_line(str_of(&r["text"]), 220));
    }
    footer(&out);
    Ok(())
}

fn get(env: &Env, words: &[&str]) -> Result<()> {
    let a = Args::parse("get", words, &["--window", "--max-tokens", "--cursor"], &["--json"])?;
    scope_needed(env)?;
    let [id] = a.pos.as_slice() else {
        bail!("usage: sokki get ID[@mm:ss] [--window 2m]   (ID from sokki search or sokki meetings)");
    };
    let o = GetOpts {
        at_ms: None,
        window_ms: a.get("--window").map(sokki_index::time::parse_span).transpose()?.unwrap_or(120_000),
        max_tokens: a.number("--max-tokens", 4000)?,
        cursor: a.number("--cursor", 0)?,
    };
    let ix = open(env)?;
    let out = query::get(&ix, id, &o, env.scope)?;
    audit(env, "get", &out);
    if a.on("--json") {
        return print_json(&out);
    }
    let s = &out["source"];
    println!(
        "{}  {}  {}",
        str_of(&s["cite"]),
        str_of(&s["date"]),
        s["title"].as_str().unwrap_or("(no title)")
    );
    if let Some(p) = out["people"].as_array().filter(|p| !p.is_empty()) {
        let names: Vec<&str> = p.iter().map(|p| str_of(&p["speaker"])).collect();
        println!("People: {}", names.join(", "));
    }
    if let Some(secs) = out["sections"].as_object() {
        for (name, lines) in secs {
            println!("\n## {name}");
            for l in lines.as_array().into_iter().flatten() {
                println!("- {}", str_of(&l["text"]));
            }
        }
    }
    for (title, key) in [("Decisions", "decisions"), ("Tasks", "tasks"), ("Items", "items")] {
        if let Some(list) = out[key].as_array().filter(|l| !l.is_empty()) {
            println!("\n## {title}");
            for i in list {
                let owner = i["owner"].as_str().map(|o| format!("{o}: ")).unwrap_or_default();
                let cite = i["evidence"]["cite"].as_str().map(|c| format!("  ({c})")).unwrap_or_default();
                println!("- {owner}{}{cite}", str_of(&i["text"]));
            }
        }
    }
    if let Some(d) = out["results"].as_array().filter(|r| !r.is_empty()) {
        if out["source"]["kind"] == "dictation" {
            let r = &d[0];
            println!("{}", r["cleaned"].as_str().unwrap_or(str_of(&r["raw"])));
        } else {
            println!("\n## Transcript");
            for t in d {
                println!("[{}] {}: {}", str_of(&t["at"]), str_of(&t["speaker"]), str_of(&t["text"]));
            }
        }
    }
    if let Some(h) = out["transcript_hidden"].as_str() {
        println!("\n({h})");
    }
    footer(&out);
    Ok(())
}

fn meetings(env: &Env, words: &[&str]) -> Result<()> {
    let a = Args::parse("meetings", words, &LIST_FLAGS, &["--json"])?;
    scope_needed(env)?;
    if !a.pos.is_empty() {
        bail!("sokki meetings takes no words; to search: sokki search {}", a.pos.join(" "));
    }
    let ix = open(env)?;
    let out = query::meetings(&ix, &list_opts(&a, 20)?)?;
    audit(env, "meetings", &out);
    if a.on("--json") {
        return print_json(&out);
    }
    for m in out["results"].as_array().into_iter().flatten() {
        let mins = (m["duration_s"].as_f64().unwrap_or(0.0) / 60.0).round();
        let people: Vec<&str> = m["people"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| str_of(&p["name"]))
            .collect();
        println!(
            "{}  {}  {:>3} min  {}  ({})",
            str_of(&m["id"]),
            str_of(&m["date"]).get(..16).unwrap_or(""),
            mins,
            m["title"].as_str().unwrap_or("(no title)"),
            people.join(", ")
        );
    }
    footer(&out);
    Ok(())
}

fn items(env: &Env, words: &[&str], kind: &str) -> Result<()> {
    let mut values: Vec<&str> = LIST_FLAGS.to_vec();
    if kind == "task" {
        values.extend(["--owner", "--status"]);
    } else {
        values.push("--query");
    }
    let cmd = if kind == "task" { "tasks" } else { "decisions" };
    let a = Args::parse(cmd, words, &values, &["--json"])?;
    scope_needed(env)?;
    if !a.pos.is_empty() {
        bail!("sokki {cmd} takes flags only (sokki help)");
    }
    let status = match a.get("--status") {
        None | Some("open") if kind == "task" => Some("open"),
        Some("done") => Some("done"),
        Some("all") | None => None,
        Some(s) => bail!("--status is open, done or all, not {s:?}"),
    };
    let ix = open(env)?;
    let out = query::items(
        &ix,
        kind,
        a.get("--owner"),
        status,
        a.get("--query"),
        &list_opts(&a, 50)?,
        env.scope,
    )?;
    audit(env, cmd, &out);
    if a.on("--json") {
        return print_json(&out);
    }
    for i in out["results"].as_array().into_iter().flatten() {
        let mark = match i["status"].as_str() {
            Some("done") => "[x] ",
            Some(_) => "[ ] ",
            None => "- ",
        };
        let owner = i["owner"].as_str().map(|o| format!("{o}: ")).unwrap_or_default();
        let cite = i["evidence"]["cite"].as_str().unwrap_or(str_of(&i["cite"]));
        println!("{mark}{owner}{}  ({cite}, {})", str_of(&i["text"]), str_of(&i["date"]).get(..10).unwrap_or(""));
    }
    footer(&out);
    Ok(())
}

fn people(env: &Env, words: &[&str]) -> Result<()> {
    let a = Args::parse("people", words, &["-n", "--cursor", "--max-tokens"], &["--json"])?;
    scope_needed(env)?;
    let ix = open(env)?;
    let out = query::people(&ix, &list_opts(&a, 200)?)?;
    audit(env, "people", &out);
    if a.on("--json") {
        return print_json(&out);
    }
    for p in out["results"].as_array().into_iter().flatten() {
        println!(
            "{:<24} {:<24} {:>3} meetings  {:>6} min",
            str_of(&p["person_id"]),
            str_of(&p["name"]),
            p["meetings"],
            p["talk_min"]
        );
    }
    footer(&out);
    Ok(())
}

fn index(env: &Env, words: &[&str]) -> Result<()> {
    let a = Args::parse("index", words, &[], &["--json"])?;
    let what = a.pos.first().map(String::as_str).unwrap_or("status");
    let ix = Index::open(&env.paths.index)?;
    let report = match what {
        "status" => sokki_index::sync(&ix, &env.paths)?,
        "rebuild" => sokki_index::rebuild(&ix, &env.paths)?,
        _ => bail!("usage: sokki index status|rebuild"),
    };
    let mut out = query::status(&ix)?;
    out["agent_access"] = json!(env.scope.as_str());
    out["sync"] = serde_json::to_value(&report)?;
    if a.on("--json") {
        return print_json(&out);
    }
    if let Some(m) = out.as_object() {
        for (k, v) in m {
            if k == "sync" {
                continue;
            }
            println!("{k}: {}", v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()));
        }
    }
    println!(
        "{}: {} added, {} updated, {} removed, {} unchanged, {} private left out, in {} ms",
        if what == "rebuild" { "rebuilt" } else { "synced" },
        report.added,
        report.updated,
        report.removed,
        report.unchanged,
        report.private,
        report.ms
    );
    for e in &report.errors {
        println!("not indexed: {e}");
    }
    Ok(())
}

fn stdin_text() -> Result<String> {
    if std::io::stdin().is_terminal() {
        bail!("pipe the text in: echo \"…\" | sokki note append ID --section NAME");
    }
    let mut s = String::new();
    std::io::stdin().read_to_string(&mut s)?;
    Ok(s)
}

fn hook_name() -> String {
    std::env::var("SOKKI_HOOK")
        .ok()
        .filter(|h| !h.trim().is_empty())
        .unwrap_or_else(|| "cli".into())
}

fn note(env: &Env, words: &[&str]) -> Result<()> {
    let a = Args::parse("note append", words, &["--section"], &["--replace", "--json"])?;
    let pos: Vec<&str> = a.pos.iter().map(String::as_str).collect();
    let ["append", id] = pos.as_slice() else {
        bail!("usage: echo TEXT | sokki note append ID --section NAME [--replace]");
    };
    scope_needed(env)?;
    let section = a.get("--section").context("--section NAME is needed")?;
    let text = stdin_text()?;
    let ix = Index::open(&env.paths.index)?;
    let path = sokki_index::write::note_append(&ix, &env.paths, id, section, &text, a.on("--replace"))?;
    let out = json!({ "ok": true, "note": path.to_string_lossy(), "section": section, "by": hook_name() });
    if a.on("--json") {
        return print_json(&out);
    }
    println!("Added to {} (## {section})", path.display());
    Ok(())
}

fn item(env: &Env, words: &[&str]) -> Result<()> {
    let a = Args::parse(
        "item add",
        words,
        &["--kind", "--quote", "--at", "--text", "--owner"],
        &["--json"],
    )?;
    let pos: Vec<&str> = a.pos.iter().map(String::as_str).collect();
    let ["add", id] = pos.as_slice() else {
        bail!("usage: sokki item add ID --kind custom:NAME --text T --quote \"…\" --at mm:ss");
    };
    scope_needed(env)?;
    // Checking a quote tells whether and when words were said: transcript
    // access.
    if !env.scope.transcripts() {
        bail!(
            "item add checks its quote against the transcript, which agent access '{}' doesn't share (needs transcripts)",
            env.scope.as_str()
        );
    }
    let kind = a.get("--kind").context("--kind custom:NAME is needed")?;
    let text = a.get("--text").context("--text is needed: the item in your own words (the quote stays its evidence)")?;
    let quote = a.get("--quote").context("--quote is needed: the words from the transcript that back the item")?;
    let at = sokki_index::time::parse_mmss(a.get("--at").context("--at mm:ss is needed: where the quote is")?)?;
    let ix = Index::open(&env.paths.index)?;
    let added = sokki_index::write::item_add(
        &ix,
        &env.paths,
        id,
        kind,
        text,
        quote,
        at,
        a.get("--owner"),
        &hook_name(),
    )?;
    if a.on("--json") {
        return print_json(&serde_json::to_value(&added)?);
    }
    println!("Added {} at {} (quote checked)", added.item.kind, added.cite);
    Ok(())
}

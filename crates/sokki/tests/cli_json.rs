//! The `sokki` binary's index commands on made-up data (macOS layout),
//! checked by their JSON. Runs the real binary.
#![cfg(target_os = "macos")]

use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sokki-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

struct Run {
    code: i32,
    out: String,
    err: String,
}

fn sokki(root: &Path, args: &[&str], stdin: Option<&str>) -> Run {
    let mut c = Command::new(env!("CARGO_BIN_EXE_sokki"));
    c.args(args)
        .env("SOKKI_DATA_DIR", root)
        .env("SOKKI_SETTINGS", root.join("settings_store.json"))
        .env("SOKKI_SOCKET", root.join("no.sock"))
        .env_remove("SOKKI_SCOPE")
        .env_remove("SOKKI_HOOK")
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = c.spawn().unwrap();
    if let Some(text) = stdin {
        child.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
    }
    let o = child.wait_with_output().unwrap();
    Run {
        code: o.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&o.stdout).into(),
        err: String::from_utf8_lossy(&o.stderr).into(),
    }
}

/// What the person does in a terminal (the CLI refuses it without one).
fn set_access(root: &Path, access: &str) {
    let path = root.join("settings_store.json");
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    v["settings"]["agent_access"] = Value::from(access);
    std::fs::write(&path, serde_json::to_string(&v).unwrap()).unwrap();
}

fn json(r: &Run) -> Value {
    assert_eq!(r.code, 0, "stderr: {}", r.err);
    serde_json::from_str(r.out.trim()).unwrap_or_else(|e| panic!("{e}: {}", r.out))
}

#[test]
fn the_index_commands_answer_in_json() {
    let root = dir("json");
    sokki_index::fixtures::write(&root, "transcripts").unwrap();
    let m0 = sokki_index::fixtures::MEETINGS[0];

    let s = json(&sokki(&root, &["search", "Vertrag", "--json"], None));
    for k in ["query", "filters_applied", "results", "next_cursor", "truncated", "est_tokens"] {
        assert!(s.get(k).is_some(), "missing {k}: {s}");
    }
    assert_eq!(s["query"], "Vertrag");
    let first = &s["results"][0];
    assert!(first["cite"].as_str().unwrap().starts_with(&format!("m:{m0}")), "{s}");
    for k in ["cite", "speaker", "person_id", "name_confirmed", "text"] {
        assert!(first.get(k).is_some(), "result without {k}");
    }

    // Filters show up in filters_applied; unknown flags fail loudly.
    let f = json(&sokki(&root, &["search", "Druckerei", "--person", "me", "--since", "2026-11-01", "--json"], None));
    assert_eq!(f["filters_applied"]["person"], "me");
    assert_eq!(f["filters_applied"]["since"], "2026-11-01");
    let bad = sokki(&root, &["search", "Druckerei", "--persn", "me"], None);
    assert_ne!(bad.code, 0);
    assert!(bad.err.contains("unknown flag --persn"), "{}", bad.err);
    let bad = sokki(&root, &["tasks", "--since", "soon"], None);
    assert_ne!(bad.code, 0);

    // Zero results name the filter to relax.
    let z = json(&sokki(&root, &["search", "Landingpage", "--app", "Zoom", "--json"], None));
    assert!(z["results"].as_array().unwrap().is_empty());
    assert!(z["hint"].as_str().unwrap().contains("--app"), "{z}");

    // get, around a moment.
    let g = json(&sokki(&root, &["get", &format!("m:{m0}@01:05"), "--window", "20s", "--json"], None));
    assert_eq!(g["results"].as_array().unwrap().len(), 3, "{g}");
    let whole = json(&sokki(&root, &["get", m0, "--json"], None));
    assert_eq!(whole["source"]["title"], "Rahmenvertrag Druckerei");
    assert_eq!(whole["tasks"].as_array().unwrap().len(), 2);

    // Lists.
    let m = json(&sokki(&root, &["meetings", "--json"], None));
    assert_eq!(m["results"].as_array().unwrap().len(), 3);
    let t = json(&sokki(&root, &["tasks", "--owner", "me", "--json"], None));
    assert_eq!(t["results"][0]["text"], "Angebot der Druckerei prüfen");
    assert_eq!(t["results"][0]["evidence"]["verified"], true);
    let d = json(&sokki(&root, &["decisions", "--query", "Landingpage", "--json"], None));
    assert_eq!(d["results"].as_array().unwrap().len(), 1);
    let p = json(&sokki(&root, &["people", "--json"], None));
    assert_eq!(p["results"][0]["person_id"], "me");
    let st = json(&sokki(&root, &["index", "status", "--json"], None));
    assert_eq!(st["meetings"], 3);
    assert_eq!(st["dictations"], 2);
    assert_eq!(st["agent_access"], "transcripts");

    // Dictations need the widest access.
    let none = json(&sokki(&root, &["search", "Steuererklärung", "--json"], None));
    assert!(none["results"].as_array().unwrap().is_empty());
    assert!(sokki(&root, &["get", "d:1"], None).code != 0);

    // Write-back.
    let a = json(&sokki(&root, &["note", "append", m0, "--section", "CRM", "--json"], Some("Kunde: Druckerei Muster\n")));
    assert_eq!(a["ok"], true);
    let crm = json(&sokki(&root, &["search", "Druckerei Muster", "--section", "crm", "--json"], None));
    assert_eq!(crm["results"].as_array().unwrap().len(), 1, "{crm}");
    let i = json(&sokki(
        &root,
        &["item", "add", m0, "--kind", "custom:risk", "--text", "Vertrag läuft aus", "--quote", "läuft im März aus", "--at", "00:10", "--json"],
        None,
    ));
    assert_eq!(i["cite"], format!("m:{m0}@00:05"));
    let no_text = sokki(&root, &["item", "add", m0, "--kind", "custom:risk", "--quote", "läuft im März aus", "--at", "00:10"], None);
    assert!(no_text.err.contains("--text is needed"), "{}", no_text.err);
    let wrong = sokki(&root, &["item", "add", m0, "--kind", "custom:risk", "--text", "x", "--quote", "nie gesagt", "--at", "00:10"], None);
    assert_ne!(wrong.code, 0);
    assert!(wrong.err.contains("not in the transcript"), "{}", wrong.err);

    // The setting is the person's: no agent (no terminal) may change it.
    let c = sokki(&root, &["config", "agent_access", "dictations"], None);
    assert_ne!(c.code, 0);
    assert!(c.err.contains("the person's choice"), "{}", c.err);
    // Summaries hide transcripts, and checking a hook's quote; off refuses.
    set_access(&root, "summaries");
    let quote = sokki(&root, &["item", "add", m0, "--kind", "custom:risk", "--text", "x", "--quote", "Kündigungsfrist", "--at", "00:31"], None);
    assert!(quote.err.contains("needs transcripts"), "{}", quote.err);
    let s = json(&sokki(&root, &["search", "Kündigungsfrist", "--json"], None));
    assert!(s["results"].as_array().unwrap().iter().all(|r| r["section"] != "transcript"), "{s}");
    assert!(sokki(&root, &["config", "agent_access", "everything"], None).code != 0);
    set_access(&root, "off");
    let off = sokki(&root, &["search", "Druckerei"], None);
    assert_ne!(off.code, 0);
    assert!(off.err.contains("agent access is off"), "{}", off.err);

    // Every read was logged, without content.
    let log = std::fs::read_to_string(root.join("agent-access.log")).unwrap();
    assert!(log.lines().count() >= 8);
    assert!(!log.contains("Kündigungsfrist beträgt"), "the log holds no transcript text");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn config_keeps_types_and_hides_secrets() {
    let root = dir("config");
    sokki_index::fixtures::write(&root, "off").unwrap();
    let path = root.join("settings_store.json");
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    v["settings"]["meetings"]["keep_audio_days"] = Value::from(7);
    v["settings"]["meetings"]["gmail_token"] = Value::from("abc");
    std::fs::write(&path, serde_json::to_string(&v).unwrap()).unwrap();
    // agent_access before the app wrote the key: off, and settable.
    let mut v2: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    v2["settings"].as_object_mut().unwrap().remove("agent_access");
    std::fs::write(&path, serde_json::to_string(&v2).unwrap()).unwrap();
    assert_eq!(sokki(&root, &["config", "agent_access"], None).out.trim(), "\"off\"");
    assert_ne!(sokki(&root, &["config", "agent_access", "all-of-it"], None).code, 0);
    set_access(&root, "summaries");
    assert_eq!(sokki(&root, &["config", "agent_access"], None).out.trim(), "\"summaries\"");
    let all = sokki(&root, &["config"], None);
    assert!(all.out.contains("meetings.gmail_token = \"(hidden)\""), "{}", all.out);
    assert!(!all.out.contains("abc"));
    assert_eq!(sokki(&root, &["config", "meetings.keep_audio_days", "30"], None).code, 0);
    assert_ne!(sokki(&root, &["config", "meetings.keep_audio_days", "soon"], None).code, 0);
    assert_ne!(sokki(&root, &["config", "meetings.keep_audio_days", "2.5"], None).code, 0);
    let typo = sokki(&root, &["config", "meetings.keep_audio_dayz", "3"], None);
    assert!(typo.err.contains("did you mean"), "{}", typo.err);
    assert_eq!(sokki(&root, &["config", "meetings.keep_audio_days"], None).out.trim(), "30");
    let _ = std::fs::remove_dir_all(&root);
}

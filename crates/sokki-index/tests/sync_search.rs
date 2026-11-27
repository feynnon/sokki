//! The index end to end on made-up meetings and dictations: sync, search,
//! get, lists, write-back, privacy and deletes. All names are invented.

use meeting_core::note::{render_note, write_segment_lines, NoteOptions};
use meeting_core::{ActionItem, MeetingMeta, MeetingNote, MeetingSummary, Segment, TrackKind, Transcript};
use serde_json::Value;
use sokki_index::query::{self, GetOpts, ListOpts, Mode, SearchOpts};
use sokki_index::{sync, Index, Paths};
use std::path::{Path, PathBuf};

fn seg(t: f64, end: f64, track: TrackKind, sp: &str, text: &str) -> Segment {
    Segment {
        start_s: t,
        end_s: end,
        track,
        speaker: sp.into(),
        text: text.into(),
    }
}

struct Fixture {
    root: PathBuf,
    paths: Paths,
}

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sokki-index-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write_meeting(f: &Fixture, id: &str, started_at: &str, note: MeetingNote, sidecar: bool) -> PathBuf {
    let notes = f.root.join("notes");
    std::fs::create_dir_all(&notes).unwrap();
    let mut note = note;
    note.meta.id = Some(id.into());
    note.meta.started_at = started_at.into();
    let md = render_note(&note, &NoteOptions::default());
    let path = notes.join(format!("{id}.md"));
    std::fs::write(&path, md).unwrap();
    if sidecar {
        write_segment_lines(&f.paths.meetings_dir.join(id), &note.transcript).unwrap();
    }
    path
}

fn records(f: &Fixture, recs: &[Value]) {
    std::fs::create_dir_all(&f.paths.meetings_dir).unwrap();
    std::fs::write(
        f.paths.meetings_dir.join("index.json"),
        serde_json::to_string_pretty(&Value::Array(recs.to_vec())).unwrap(),
    )
    .unwrap();
}

fn rec(id: &str, started: &str, note: &Path, extra: Value) -> Value {
    let mut v = serde_json::json!({
        "id": id, "started_at": started, "duration_s": 600.0,
        "status": {"kind": "done"}, "tracks": ["mic.flac", "system.flac"],
        "source_app": "Zoom", "title": null, "note_path": note.to_string_lossy(),
        "speakers": [], "warnings": [], "audio_deleted": false,
    });
    if let (Some(m), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        for (k, x) in e {
            m.insert(k.clone(), x.clone());
        }
    }
    v
}

fn history(path: &Path) {
    let c = rusqlite::Connection::open(path).unwrap();
    c.execute_batch(
        "CREATE TABLE transcription_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT, file_name TEXT NOT NULL, timestamp INTEGER NOT NULL,
            saved BOOLEAN NOT NULL DEFAULT 0, title TEXT NOT NULL, transcription_text TEXT NOT NULL,
            post_processed_text TEXT, post_process_prompt TEXT, post_process_requested BOOLEAN NOT NULL DEFAULT 0,
            source_name TEXT, language TEXT, language_source TEXT, meeting_id TEXT, meeting_track TEXT, target_app TEXT);
         INSERT INTO transcription_history(file_name, timestamp, title, transcription_text, post_processed_text, target_app)
           VALUES ('a.wav', 1795166390, '', 'äh die steuererklärung bis ende mai abgeben', 'Die Steuererklärung bis Ende Mai abgeben.', 'Notes');
         INSERT INTO transcription_history(file_name, timestamp, title, transcription_text, target_app)
           VALUES ('b.wav', 1795166490, '', 'Einkaufsliste Milch Brot Kaffee', 'Mail');
         INSERT INTO transcription_history(file_name, timestamp, title, transcription_text, source_name)
           VALUES ('c.wav', 1795166590, '', 'eine Datei, keine Diktat-Steuererklärung', 'podcast.mp3');
         INSERT INTO transcription_history(file_name, timestamp, title, transcription_text, meeting_id)
           VALUES ('d.wav', 1795166690, '', 'ein Stück Meeting Steuererklärung', 'meeting-x');",
    )
    .unwrap();
}

fn druckerei() -> MeetingNote {
    MeetingNote {
        meta: MeetingMeta {
            id: None,
            started_at: String::new(),
            duration_s: 600.0,
            source_app: Some("Zoom".into()),
        },
        transcript: Transcript {
            segments: vec![
                seg(0.0, 4.0, TrackKind::Mic, "Ich", "Hallo zusammen, heute geht es um die Druckerei."),
                seg(5.0, 30.0, TrackKind::System, "Jana Beispiel", "Der Rahmenvertrag mit der Druckerei läuft im März aus."),
                seg(31.0, 50.0, TrackKind::System, "Jana Beispiel", "Die Kündigungsfrist beträgt drei Monate."),
                seg(65.0, 80.0, TrackKind::Mic, "Ich", "Dann prüfe ich bis Freitag das neue Angebot der Druckerei."),
                seg(81.0, 95.0, TrackKind::System, "Speaker 3", "Und ich schicke die Datenschutzerklärung an die Rechtsabteilung."),
            ],
        },
        summary: Some(MeetingSummary {
            title: "Rahmenvertrag Druckerei".into(),
            summary: "- Der Rahmenvertrag mit der Druckerei läuft im März aus.".into(),
            decisions: vec!["Wir verlängern den Rahmenvertrag nicht automatisch.".into()],
            action_items: vec![
                ActionItem { owner: Some("Ich".into()), text: "Angebot der Druckerei prüfen".into() },
                ActionItem { owner: Some("Speaker 3".into()), text: "Datenschutzerklärung an die Rechtsabteilung schicken".into() },
            ],
        }),
    }
}

fn budget() -> MeetingNote {
    MeetingNote {
        meta: MeetingMeta { id: None, started_at: String::new(), duration_s: 300.0, source_app: Some("Google Meet".into()) },
        transcript: Transcript {
            segments: vec![
                seg(0.0, 10.0, TrackKind::Mic, "Ich", "Das Budget für die Herbstaktion steht."),
                seg(12.0, 30.0, TrackKind::System, "Tom Muster", "Die Landingpage bauen wir erst nach dem Launch."),
            ],
        },
        summary: None,
    }
}

fn fixture(name: &str) -> Fixture {
    let root = temp(name);
    let paths = Paths {
        index: root.join("data/index.sqlite"),
        meetings_dir: root.join("data/meetings"),
        history_db: Some(root.join("data/history.db")),
        user_name: "Ich".into(),
    };
    std::fs::create_dir_all(root.join("data")).unwrap();
    let f = Fixture { root, paths };
    let a = write_meeting(&f, "meeting-20261103-100000-001", "2026-11-03T10:00:00+01:00", druckerei(), true);
    // The note's own-notes section (own-notes session): the person's line,
    // and Sokki's addition under it.
    let md = std::fs::read_to_string(&a).unwrap();
    let md = md.replacen(
        "## Zusammenfassung",
        "## Meine Notizen\n\n- Kündigung schriftlich?\n  - Laut Jana reicht ein Fax <!-- t=00:31 -->\n\n## Zusammenfassung",
        1,
    );
    std::fs::write(&a, md).unwrap();
    // Written before the id and the sidecar existed: no exact times.
    let b = write_meeting(&f, "meeting-20261110-140000-002", "2026-11-10T14:00:00+01:00", budget(), false);
    let p = write_meeting(&f, "meeting-20261112-090000-003", "2026-11-12T09:00:00+01:00", budget(), true);
    let s = write_meeting(&f, "meeting-20261113-090000-004", "2026-11-13T09:00:00+01:00", budget(), true);
    records(
        &f,
        &[
            rec("meeting-20261103-100000-001", "2026-11-03T10:00:00+01:00", &a, serde_json::json!({})),
            rec("meeting-20261110-140000-002", "2026-11-10T14:00:00+01:00", &b, serde_json::json!({"source_app": "Google Meet"})),
            rec("meeting-20261112-090000-003", "2026-11-12T09:00:00+01:00", &p, serde_json::json!({"private": true})),
            rec("meeting-20261113-090000-004", "2026-11-13T09:00:00+01:00", &s, serde_json::json!({"sample": true})),
        ],
    );
    // Own notes for the first meeting.
    std::fs::write(
        f.paths.meetings_dir.join("meeting-20261103-100000-001/notes.md"),
        "---\nid: meeting-20261103-100000-001\nsource: own-notes\n---\nKündigung schriftlich? <!-- t=00:00:40 -->\n  - Ja, per Einschreiben\n",
    )
    .unwrap();
    history(f.paths.history_db.as_ref().unwrap());
    f
}

fn opts(n: usize) -> ListOpts {
    ListOpts { n, max_tokens: 4000, ..Default::default() }
}

fn search(ix: &Index, q: &str, mode: Mode) -> Value {
    query::search(
        ix,
        &SearchOpts { query: q.into(), kind: None, section: None, mode, list: opts(10) },
    )
    .unwrap()
}

fn cites(v: &Value) -> Vec<String> {
    v["results"].as_array().unwrap().iter().map(|r| r["cite"].as_str().unwrap().to_string()).collect()
}

#[test]
fn everything_end_to_end() {
    let f = fixture("e2e");
    let ix = Index::open(&f.paths.index).unwrap();
    let r = sync(&ix, &f.paths).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    // Two meetings, their own notes, two dictations; the private and the
    // sample meeting are left out; files and meeting pieces are no dictations.
    assert_eq!(r.added, 2 + 1 + 2, "{r:?}");
    assert_eq!(r.private, 1);
    let again = sync(&ix, &f.paths).unwrap();
    assert_eq!((again.added, again.updated, again.removed, again.unchanged), (0, 0, 0, 5));

    // A German compound: whole words miss it, trigrams find it.
    let words = search(&ix, "Vertrag", Mode::Words);
    assert!(cites(&words).is_empty(), "{words}");
    let both = search(&ix, "Vertrag", Mode::Both);
    let c = cites(&both);
    assert!(c.contains(&"m:meeting-20261103-100000-001@00:05".to_string()), "{c:?}");
    // The JSON shape.
    for k in ["query", "filters_applied", "results", "next_cursor", "truncated", "est_tokens"] {
        assert!(both.get(k).is_some(), "missing {k}");
    }
    let turn = both["results"].as_array().unwrap().iter().find(|r| r["section"] == "transcript").unwrap();
    assert_eq!(turn["speaker"], "Jana Beispiel");
    assert_eq!(turn["person_id"], "jana-beispiel");
    assert_eq!(turn["name_confirmed"], true);
    assert_eq!(turn["channel"], "system");
    assert_eq!(turn["start_ms"], 5000);
    assert_eq!(turn["end_ms"], 50000, "two segments of Jana: one paragraph");

    // Accents fold: "Kundigungsfrist" finds "Kündigungsfrist".
    assert!(!cites(&search(&ix, "kundigungsfrist", Mode::Words)).is_empty());

    // Everything is searchable: summaries, transcripts and dictations.
    let s = search(&ix, "Druckerei", Mode::Both);
    assert!(s["results"].as_array().unwrap().iter().any(|r| r["section"] == "transcript"), "{s}");
    assert!(s["results"].as_array().unwrap().iter().any(|r| r["section"] == "summary"), "{s}");
    let d = search(&ix, "Steuererklärung", Mode::Both);
    assert_eq!(cites(&d), vec!["d:1"], "only the dictation, not the file or the meeting piece: {d}");
    assert_eq!(d["results"][0]["app"], "Notes");

    // Filters, and a zero result that says which one to relax.
    let p = query::search(
        &ix,
        &SearchOpts {
            query: "Druckerei".into(),
            kind: None,
            section: Some("transcript".into()),
            mode: Mode::Both,
            list: ListOpts { person: Some("me".into()), ..opts(10) },
        },
    )
    .unwrap();
    assert!(p["results"].as_array().unwrap().iter().all(|r| r["person_id"] == "me"), "{p}");
    assert_eq!(p["results"].as_array().unwrap().len(), 2);
    let z = query::search(
        &ix,
        &SearchOpts {
            query: "Landingpage".into(),
            kind: None,
            section: None,
            mode: Mode::Both,
            list: ListOpts { app: Some("Zoom".into()), shown: vec![("app".into(), "Zoom".into())], ..opts(10) },
        },
    )
    .unwrap();
    assert!(cites(&z).is_empty());
    assert!(z["hint"].as_str().unwrap().contains("without --app: 1"), "{}", z["hint"]);

    // get around a moment, with a window.
    let g = query::get(
        &ix,
        "m:meeting-20261103-100000-001@01:05",
        &GetOpts { window_ms: 10_000, max_tokens: 4000, ..Default::default() },
    )
    .unwrap();
    let at = |g: &Value| -> Vec<String> {
        g["results"].as_array().unwrap().iter().map(|r| r["at"].as_str().unwrap().to_string()).collect()
    };
    assert_eq!(at(&g), vec!["01:05"], "±10 s: only the turn at 01:05 (65–80 s): {g}");
    let g = query::get(
        &ix,
        "m:meeting-20261103-100000-001@01:05",
        &GetOpts { window_ms: 20_000, max_tokens: 4000, ..Default::default() },
    )
    .unwrap();
    assert_eq!(at(&g), vec!["00:05", "01:05", "01:21"], "±20 s reaches Jana's turn (5–50 s) and 01:21");
    let whole = query::get(&ix, "meeting-20261103-100000-001", &GetOpts { window_ms: 0, max_tokens: 4000, ..Default::default() }).unwrap();
    assert_eq!(whole["results"].as_array().unwrap().len(), 4, "the whole transcript");
    assert_eq!(whole["tasks"].as_array().unwrap().len(), 2);
    assert!(whole["tasks"][0]["evidence"]["quote"].is_string(), "with its quote");
    assert_eq!(whole["sections"]["summary"].as_array().unwrap().len(), 1);

    // Tasks with code-checked evidence.
    let t = query::items(&ix, "task", Some("me"), Some("open"), None, &opts(10)).unwrap();
    let task = &t["results"][0];
    assert_eq!(task["text"], "Angebot der Druckerei prüfen");
    assert_eq!(task["evidence"]["cite"], "m:meeting-20261103-100000-001@01:05");
    assert_eq!(task["evidence"]["verified"], true);
    assert_eq!(task["evidence"]["origin"], "match");
    assert!(task["evidence"]["quote"].as_str().unwrap().contains("Angebot der Druckerei"));
    let other = query::items(&ix, "task", None, Some("open"), None, &opts(10)).unwrap();
    assert_eq!(other["results"].as_array().unwrap().len(), 2);
    let dec = query::items(&ix, "decision", None, None, None, &opts(10)).unwrap();
    assert_eq!(dec["results"].as_array().unwrap().len(), 1);

    // Meetings and people.
    let m = query::meetings(&ix, &ListOpts { person: Some("Tom".into()), ..opts(10) }).unwrap();
    assert_eq!(m["results"].as_array().unwrap().len(), 1);
    assert_eq!(m["results"][0]["id"], "meeting-20261110-140000-002");
    let all = query::meetings(&ix, &opts(10)).unwrap();
    assert_eq!(all["results"][0]["id"], "meeting-20261110-140000-002", "newest first");
    let people = query::people(&ix, &opts(50)).unwrap();
    let ids: Vec<&str> = people["results"].as_array().unwrap().iter().map(|p| p["person_id"].as_str().unwrap()).collect();
    assert_eq!(ids[0], "me");
    assert!(ids.contains(&"jana-beispiel") && ids.contains(&"tom-muster"));
    assert!(!ids.iter().any(|i| i.contains("speaker")), "placeholders are no people");

    // Own notes.
    let n = search(&ix, "Einschreiben", Mode::Both);
    assert_eq!(cites(&n), vec!["n:meeting-20261103-100000-001"]);
    let n = search(&ix, "schriftlich", Mode::Both);
    assert_eq!(cites(&n), vec!["n:meeting-20261103-100000-001@00:40"], "the typed line once, from notes.md");
    let fax = search(&ix, "Fax", Mode::Both);
    assert_eq!(cites(&fax), vec!["m:meeting-20261103-100000-001@00:31"], "Sokki's addition, from the note");

    // Write-back: a section and a checked item.
    sokki_index::write::note_append(&ix, &f.paths, "m:meeting-20261103-100000-001", "CRM", "Kunde: Druckerei Muster", true).unwrap();
    let crm = search(&ix, "Druckerei Muster", Mode::Both);
    assert!(crm["results"].as_array().unwrap().iter().any(|r| r["section"] == "crm"), "{crm}");
    assert!(sokki_index::write::note_append(&ix, &f.paths, "m:meeting-20261103-100000-001", "Transkript", "x", false).is_err());
    assert!(
        sokki_index::write::note_append(&ix, &f.paths, "m:meeting-20261103-100000-001", "CRM", "a\n## Details\nb", true).is_err(),
        "a ## heading in the text would end the section"
    );
    let added = sokki_index::write::item_add(
        &ix, &f.paths, "m:meeting-20261103-100000-001", "custom:risk", "Vertrag läuft aus",
        "läuft im März aus", 10_000, None, "test-hook",
    )
    .unwrap();
    // Stored where the quote is: the turn at 00:05, as search cites it.
    assert_eq!(added.cite, "m:meeting-20261103-100000-001@00:05");
    assert!(sokki_index::write::item_add(
        &ix, &f.paths, "m:meeting-20261103-100000-001", "custom:risk", "  ",
        "läuft im März aus", 10_000, None, "test-hook",
    )
    .is_err(), "the item needs its own text");
    assert!(sokki_index::write::item_add(
        &ix, &f.paths, "m:meeting-20261103-100000-001", "custom:risk", "x",
        "das hat niemand gesagt", 10_000, None, "test-hook",
    )
    .is_err());
    let got = query::get(&ix, "m:meeting-20261103-100000-001", &GetOpts { window_ms: 0, max_tokens: 8000, ..Default::default() }).unwrap();
    assert_eq!(got["items"][0]["kind"], "custom:risk");
    assert_eq!(got["items"][0]["evidence"]["verified"], true);
    // A rebuild keeps the hook's item: it lives next to the meeting.
    sokki_index::rebuild(&ix, &f.paths).unwrap();
    let got = query::get(&ix, "m:meeting-20261103-100000-001", &GetOpts { window_ms: 0, max_tokens: 8000, ..Default::default() }).unwrap();
    assert_eq!(got["items"].as_array().unwrap().len(), 1);

    // An edit in Obsidian is picked up; marking a meeting private removes it.
    let note = f.root.join("notes/meeting-20261110-140000-002.md");
    let md = std::fs::read_to_string(&note).unwrap();
    std::fs::write(&note, md.replace("Landingpage", "Zielseite")).unwrap();
    let r = sync(&ix, &f.paths).unwrap();
    assert_eq!(r.updated, 1, "{r:?}");
    assert!(!cites(&search(&ix, "Zielseite", Mode::Both)).is_empty());
    let md = std::fs::read_to_string(&note).unwrap();
    std::fs::write(&note, md.replacen("type: meeting\n", "type: meeting\nprivate: true\n", 1)).unwrap();
    let r = sync(&ix, &f.paths).unwrap();
    assert_eq!(r.private, 2, "{r:?}");
    assert!(cites(&search(&ix, "Zielseite", Mode::Both)).is_empty());
    let people = query::people(&ix, &opts(50)).unwrap();
    assert!(
        people["results"].as_array().unwrap().iter().all(|p| p["person_id"] != "tom-muster"),
        "a meeting made private leaves people too: {people}"
    );

    // Deleting a meeting deletes all its rows, its own notes too.
    records(&f, &[]);
    sync(&ix, &f.paths).unwrap();
    let left: i64 = ix
        .conn
        .query_row("SELECT count(*) FROM segments s JOIN sources src ON src.id = s.source_id WHERE src.kind != 'dictation' AND src.kind != 'notes'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 0);
    assert!(cites(&search(&ix, "Rahmenvertrag", Mode::Both)).is_empty());
    let _ = std::fs::remove_dir_all(&f.root);
}

#[test]
fn the_token_budget_cuts_and_pages() {
    let f = fixture("budget");
    let ix = Index::open(&f.paths.index).unwrap();
    sync(&ix, &f.paths).unwrap();
    let q = |cursor: usize, max: usize| {
        query::search(
            &ix,
            &SearchOpts {
                query: "Druckerei Rahmenvertrag Kündigungsfrist Angebot".into(),
                kind: None,
                section: None,
                mode: Mode::Both,
                list: ListOpts { cursor, max_tokens: max, ..opts(10) },
            },
        )
        .unwrap()
    };
    let small = q(0, 150);
    assert_eq!(small["truncated"], true);
    let next: usize = small["next_cursor"].as_str().unwrap().parse().unwrap();
    assert!(small["est_tokens"].as_u64().unwrap() <= 150);
    let rest = q(next, 4000);
    let a = cites(&small);
    let b = cites(&rest);
    assert!(a.iter().all(|c| !b.contains(c)), "pages don't repeat");
    let _ = std::fs::remove_dir_all(&f.root);
}

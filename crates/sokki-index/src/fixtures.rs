//! Made-up meetings and dictations in the macOS app's layout, for the CLI's
//! tests and for trying `sokki search` without real data. Every name,
//! company and sentence is invented.
//!
//! `cargo run -p sokki-index --example make_fixture --features fixtures -- DIR`
//! writes one to DIR; then `SOKKI_DATA_DIR=DIR sokki search Vertrag`.

use anyhow::Result;
use meeting_core::note::{render_note, write_segment_lines, NoteOptions};
use meeting_core::{ActionItem, MeetingMeta, MeetingNote, MeetingSummary, Segment, TrackKind, Transcript};
use serde_json::json;
use std::path::Path;

fn seg(t: f64, end: f64, track: TrackKind, sp: &str, text: &str) -> Segment {
    Segment {
        start_s: t,
        end_s: end,
        track,
        speaker: sp.into(),
        text: text.into(),
    }
}

/// The ids of the made-up meetings, oldest first.
pub const MEETINGS: [&str; 3] = [
    "meeting-20261103-100000-001",
    "meeting-20261110-140000-002",
    "meeting-20261117-090000-003",
];

fn notes() -> Vec<(&'static str, &'static str, MeetingNote)> {
    let meta = |app: &str, d: f64| MeetingMeta {
        id: None,
        started_at: String::new(),
        duration_s: d,
        source_app: Some(app.into()),
    };
    vec![
        (
            MEETINGS[0],
            "2026-11-03T10:00:00+01:00",
            MeetingNote {
                meta: meta("Zoom", 600.0),
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
                    summary: "- Der Rahmenvertrag mit der Druckerei läuft im März aus.\n- Die Kündigungsfrist beträgt drei Monate.".into(),
                    decisions: vec!["Wir verlängern den Rahmenvertrag nicht automatisch.".into()],
                    action_items: vec![
                        ActionItem { owner: Some("Ich".into()), text: "Angebot der Druckerei prüfen".into() },
                        ActionItem { owner: Some("Speaker 3".into()), text: "Datenschutzerklärung an die Rechtsabteilung schicken".into() },
                    ],
                }),
            },
        ),
        (
            MEETINGS[1],
            "2026-11-10T14:00:00+01:00",
            MeetingNote {
                meta: meta("Google Meet", 300.0),
                transcript: Transcript {
                    segments: vec![
                        seg(0.0, 10.0, TrackKind::Mic, "Ich", "Das Budget für die Herbstaktion steht, viertausend Euro."),
                        seg(12.0, 30.0, TrackKind::System, "Tom Muster", "Die Landingpage bauen wir erst nach dem Launch."),
                        seg(31.0, 45.0, TrackKind::System, "Tom Muster", "Ich kümmere mich um die Anzeigenschaltung."),
                    ],
                },
                summary: Some(MeetingSummary {
                    title: "Herbstaktion".into(),
                    summary: "- Budget für die Herbstaktion: 4.000 Euro.".into(),
                    decisions: vec!["Die Landingpage kommt erst nach dem Launch.".into()],
                    action_items: vec![ActionItem { owner: Some("Tom Muster".into()), text: "Anzeigenschaltung vorbereiten".into() }],
                }),
            },
        ),
        (
            MEETINGS[2],
            "2026-11-17T09:00:00+01:00",
            MeetingNote {
                meta: meta("Microsoft Teams", 420.0),
                transcript: Transcript {
                    segments: vec![
                        seg(0.0, 8.0, TrackKind::Mic, "Ich", "Let's go through the onboarding numbers."),
                        seg(9.0, 25.0, TrackKind::System, "Sam Example", "About forty customers finished the new onboarding, most in under ten minutes."),
                        seg(26.0, 40.0, TrackKind::System, "Sam Example", "The main drop-off is at the payment step."),
                    ],
                },
                summary: Some(MeetingSummary {
                    title: "Onboarding review".into(),
                    summary: "- About 40 customers finished the new onboarding.".into(),
                    decisions: vec![],
                    action_items: vec![ActionItem { owner: Some("Sam Example".into()), text: "Look into the payment step drop-off".into() }],
                }),
            },
        ),
    ]
}

/// Writes the made-up data dir: `settings_store.json` (agent access set to
/// `access`), `meetings/index.json` with each meeting's folder
/// (`segments.jsonl`, own notes for the first), `notes/*.md` and
/// `history.db` with two dictations.
pub fn write(root: &Path, access: &str) -> Result<()> {
    let meetings = root.join("meetings");
    let notes_dir = root.join("notes");
    std::fs::create_dir_all(&meetings)?;
    std::fs::create_dir_all(&notes_dir)?;
    let mut records = Vec::new();
    for (id, started, mut note) in notes() {
        note.meta.id = Some(id.into());
        note.meta.started_at = started.into();
        let md = render_note(&note, &NoteOptions::default());
        let title = note.summary.as_ref().map(|s| s.title.clone()).unwrap_or_default();
        let path = notes_dir.join(format!("{} {}.md", &started[..10], title));
        std::fs::write(&path, md)?;
        write_segment_lines(&meetings.join(id), &note.transcript)?;
        records.push(json!({
            "id": id, "started_at": started, "duration_s": note.meta.duration_s,
            "status": {"kind": "done"}, "tracks": ["mic.flac", "system.flac"],
            "source_app": note.meta.source_app, "title": title, "note_path": path.to_string_lossy(),
            "speakers": note.transcript.speakers(), "warnings": [], "audio_deleted": false,
            "online": true, "title_by_hand": false, "title_provisional": false, "sample": false,
        }));
    }
    std::fs::write(meetings.join("index.json"), serde_json::to_string_pretty(&records)?)?;
    std::fs::write(
        meetings.join(MEETINGS[0]).join("notes.md"),
        "---\nid: meeting-20261103-100000-001\nsource: own-notes\n---\nKündigung schriftlich? <!-- t=00:00:40 -->\n  - Ja, per Einschreiben\n",
    )?;
    let settings = json!({
        "settings": {
            "agent_access": access,
            "meetings": {
                "user_name": "Ich",
                "notes_dir": notes_dir.to_string_lossy(),
                "llm_base_url": "http://127.0.0.1:9",
                "llm_model": "none",
            }
        }
    });
    std::fs::write(root.join("settings_store.json"), serde_json::to_string_pretty(&settings)?)?;
    let db = root.join("history.db");
    let _ = std::fs::remove_file(&db);
    let c = rusqlite::Connection::open(&db)?;
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
         -- The app's migrations are at 4 for this schema
         -- (src-tauri/src/managers/history.rs); 0 makes it migrate again.
         PRAGMA user_version = 4;",
    )?;
    Ok(())
}

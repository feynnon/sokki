//! Markdown note rendering (golden files) and writing to disk.
//!
//! Golden files live in tests/golden/. To regenerate after an intended
//! format change: `UPDATE_GOLDEN=1 cargo test --test note_golden`, then
//! review the diff.

mod common;

use std::path::PathBuf;

use common::*;
use meeting_core::note::{render_markdown, write_note};
use meeting_core::{
    ActionItem, MeetingMeta, MeetingNote, MeetingSummary, Segment, TrackKind, Transcript,
};

fn golden(name: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing golden file {}; run with UPDATE_GOLDEN=1",
            path.display()
        )
    });
    if expected != actual {
        panic!(
            "golden mismatch for {name}\n--- expected ---\n{expected}\n--- actual ---\n{actual}\n(UPDATE_GOLDEN=1 to accept)"
        );
    }
}

fn meta() -> MeetingMeta {
    MeetingMeta {
        id: None,
        started_at: "2026-11-17T14:03:00+01:00".into(),
        duration_s: 4.0 * 60.0 + 58.0,
        source_app: Some("Google Chrome".into()),
    }
}

fn de_summary() -> MeetingSummary {
    MeetingSummary {
        title: "Herbstkampagne, Budget & Landingpage".into(),
        summary: "- Die Herbstkampagne brachte rund 340 Leads, Klickrate 2,3 %.\n- Kosten pro Lead liegen bei 18 € statt der angestrebten 15 €.\n- Das Webinar wird vom 9. auf den 16. Dezember verschoben.".into(),
        decisions: vec![
            "Die drei schwächsten LinkedIn-Anzeigen werden pausiert.".into(),
            "Das Dezember-Budget steigt auf 12.500 €.".into(),
        ],
        action_items: vec![
            ActionItem { owner: Some("Speaker 2".into()), text: "Relaunch der Landingpage bis Freitag".into() },
            ActionItem { owner: Some("Ich".into()), text: "Zusage an die Agentur bis Mittwoch schicken".into() },
            ActionItem { owner: None, text: "Teilnehmer über den neuen Webinar-Termin informieren".into() },
        ],
    }
}

fn short_transcript() -> Transcript {
    let seg = |t: f64, track, sp: &str, text: &str| Segment {
        start_s: t,
        end_s: t + 3.0,
        track,
        speaker: sp.into(),
        text: text.into(),
    };
    Transcript {
        segments: vec![
            seg(
                0.0,
                TrackKind::Mic,
                "Ich",
                "Hallo zusammen, danke dass ihr euch die Zeit nehmt.",
            ),
            seg(4.0, TrackKind::Mic, "Ich", "Wir haben heute drei Themen."),
            seg(
                11.0,
                TrackKind::System,
                "Speaker 2",
                "Gerne. Soll ich direkt mit den Zahlen anfangen?",
            ),
            seg(14.0, TrackKind::Mic, "Ich", "Ja, leg los."),
            seg(
                16.0,
                TrackKind::System,
                "Speaker 2",
                "Die Kosten pro Lead liegen bei achtzehn Euro.",
            ),
            seg(
                19.5,
                TrackKind::System,
                "Speaker 2",
                "Unser Ziel waren fünfzehn Euro.",
            ),
            seg(
                3725.0,
                TrackKind::System,
                "Speaker 3",
                "Über eine Stunde später: Größe, Maß, Übermaß.",
            ),
        ],
    }
}

#[test]
fn golden_german_full() {
    let note = MeetingNote {
        meta: meta(),
        transcript: short_transcript(),
        summary: Some(de_summary()),
    };
    golden("de_full.md", &render_markdown(&note));
}

#[test]
fn golden_german_fixture_meeting() {
    let note = MeetingNote {
        meta: meta(),
        transcript: load_fixture("de_team_meeting.txt"),
        summary: Some(de_summary()),
    };
    golden("de_team_meeting.md", &render_markdown(&note));
}

#[test]
fn golden_english_full() {
    let transcript = load_fixture("de_en_mixed_call.txt");
    let note = MeetingNote {
        meta: MeetingMeta { source_app: Some("zoom.us".into()), duration_s: 158.0, ..meta() },
        transcript,
        summary: Some(MeetingSummary {
            title: "Onboarding review".into(),
            summary: "- About 40 customers finished the new onboarding flow, most in under ten minutes.\n- The main drop-off is at the payment step.".into(),
            decisions: vec!["Show the total team price in the payment step.".into()],
            action_items: vec![
                ActionItem { owner: Some("Speaker 2".into()), text: "Send the invite for the sales review on Tuesday afternoon".into() },
                ActionItem { owner: Some("Speaker 3".into()), text: "Prepare the churn analysis by Thursday".into() },
            ],
        }),
    };
    golden("en_full.md", &render_markdown(&note));
}

#[test]
fn golden_no_summary() {
    let note = MeetingNote {
        meta: MeetingMeta {
            source_app: None,
            ..meta()
        },
        transcript: short_transcript(),
        summary: None,
    };
    golden("no_summary.md", &render_markdown(&note));
}

#[test]
fn golden_summary_without_decisions_or_tasks() {
    let note = MeetingNote {
        meta: meta(),
        transcript: short_transcript(),
        summary: Some(MeetingSummary {
            title: "Kurzer Zahlen-Check".into(),
            summary:
                "Kurzer Abgleich der Kampagnenzahlen; die Kosten pro Lead liegen über dem Ziel."
                    .into(),
            decisions: vec![],
            action_items: vec![],
        }),
    };
    golden("summary_only.md", &render_markdown(&note));
}

#[test]
fn golden_empty_transcript() {
    let note = MeetingNote {
        meta: meta(),
        transcript: Transcript::default(),
        summary: None,
    };
    golden("empty.md", &render_markdown(&note));
}

#[test]
fn write_note_creates_dir_and_never_overwrites() {
    let base = temp_dir("write");
    let dir = base.join("Meetings").join("2026");
    let note = MeetingNote {
        meta: meta(),
        transcript: short_transcript(),
        summary: Some(MeetingSummary {
            title: "Q4: Budget / Plan? [Entwurf] #1".into(),
            ..de_summary()
        }),
    };
    let p1 = write_note(&dir, &note, &Default::default()).unwrap();
    let p2 = write_note(&dir, &note, &Default::default()).unwrap();
    let p3 = write_note(&dir, &note, &Default::default()).unwrap();
    let names: Vec<String> = [&p1, &p2, &p3]
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        vec![
            "2026-11-17 1403 Q4 Budget Plan Entwurf 1.md",
            "2026-11-17 1403 Q4 Budget Plan Entwurf 1 2.md",
            "2026-11-17 1403 Q4 Budget Plan Entwurf 1 3.md",
        ]
    );
    // Content is valid UTF-8 with umlauts intact and identical in all copies.
    let c1 = std::fs::read_to_string(&p1).unwrap();
    assert!(c1.contains("Größe, Maß, Übermaß"));
    assert_eq!(c1, std::fs::read_to_string(&p3).unwrap());
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn write_note_without_summary_uses_meeting_as_name() {
    let dir = temp_dir("write-nosum");
    let note = MeetingNote {
        meta: meta(),
        transcript: short_transcript(),
        summary: None,
    };
    let p = write_note(&dir, &note, &Default::default()).unwrap();
    assert_eq!(p.file_name().unwrap(), "2026-11-17 1403 Meeting.md");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn write_note_keeps_umlauts_in_file_name() {
    let dir = temp_dir("write-umlaut");
    let note = MeetingNote {
        meta: meta(),
        transcript: short_transcript(),
        summary: Some(MeetingSummary {
            title: "Übergabe: Größenplanung".into(),
            ..de_summary()
        }),
    };
    let p = write_note(&dir, &note, &Default::default()).unwrap();
    assert_eq!(
        p.file_name().unwrap(),
        "2026-11-17 1403 Übergabe Größenplanung.md"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

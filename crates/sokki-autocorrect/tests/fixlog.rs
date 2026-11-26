//! The list of fixes on this Mac, with made-up words.
use std::time::{Duration, UNIX_EPOCH};

use sokki_autocorrect::fixlog::FixLog;
use sokki_autocorrect::Outcome;

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("sokki-fixlog-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn every_fix_undo_and_skip_is_one_line() {
    let d = dir("lines");
    let log = FixLog::new(d.join("sub/autocorrect_fixes.jsonl"));
    let at = UNIX_EPOCH + Duration::from_secs(1_795_675_190);
    let ms = Duration::from_millis(37);
    let fixed = Outcome::Fixed {
        typed: "nihct".into(),
        fix: "nicht".into(),
        exact: true,
    };
    let rough = Outcome::Fixed {
        typed: "dign".into(),
        fix: "ding".into(),
        exact: false,
    };
    let undone = Outcome::Undone {
        typed: "nihct".into(),
        fix: "nicht".into(),
    };
    for o in [
        &fixed,
        &Outcome::Nothing,
        &rough,
        &undone,
        &Outcome::Skipped("the fix didn't arrive"),
    ] {
        log.record_at(at, "com.google.Chrome", o, ms).unwrap();
    }
    let text = std::fs::read_to_string(log.path()).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines,
        [
            r#"{"at":"2026-11-26T06:39:50Z","app":"com.google.Chrome","what":"fixed","typed":"nihct","fix":"nicht","ms":37}"#,
            r#"{"at":"2026-11-26T06:39:50Z","app":"com.google.Chrome","what":"fixed_inexact","typed":"dign","fix":"ding","ms":37}"#,
            r#"{"at":"2026-11-26T06:39:50Z","app":"com.google.Chrome","what":"undone","typed":"nihct","fix":"nicht","ms":37}"#,
            r#"{"at":"2026-11-26T06:39:50Z","app":"com.google.Chrome","what":"skipped","why":"the fix didn't arrive","ms":37}"#,
        ]
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_full_list_drops_its_older_half() {
    let d = dir("trim");
    let log = FixLog::with_limit(d.join("f.jsonl"), 1_000);
    let o = Outcome::Fixed {
        typed: "wroks".into(),
        fix: "works".into(),
        exact: true,
    };
    for i in 0..40 {
        log.record(&format!("app{i:02}"), &o, Duration::ZERO)
            .unwrap();
    }
    let text = std::fs::read_to_string(log.path()).unwrap();
    assert!(text.len() <= 1_200, "{} bytes", text.len());
    assert!(text.contains("app39") && !text.contains("app00"));
    assert!(text.lines().all(|l| l.starts_with('{') && l.ends_with('}')));
    let _ = std::fs::remove_dir_all(&d);
}

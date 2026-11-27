//! The hooks in `examples/hooks/`, run as `sokki hooks` runs them, on the
//! made-up meeting (`tools::FIXTURE_NOTE`). Nothing reaches Reminders or a
//! real notes folder.

use sokki_engine::hooks::event::{MEETING_NOTE_READY, SCHEDULE};
use sokki_engine::hooks::{tools, Gate, Outcome, Paths, ScheduleInfo};
use std::path::{Path, PathBuf};

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hooks")
}

fn setup(name: &str) -> (Paths, PathBuf) {
    let root =
        std::env::temp_dir().join(format!("sokki-example-hooks-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let paths = Paths {
        config_dir: examples(),
        state_dir: root.join("state"),
    };
    let notes = root.join("notes");
    std::fs::create_dir_all(&notes).unwrap();
    (paths, notes)
}

fn run(
    paths: &Paths,
    hook: &str,
    event: &sokki_engine::hooks::HookEvent,
    trigger: &str,
    env: &[(&str, &Path)],
) -> (Outcome, String) {
    let env: Vec<(String, String)> = env
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string_lossy().into_owned()))
        .collect();
    let free = || Gate::Free;
    let ran = tools::run_once(paths, hook, event, trigger, &free, &env, || {}).unwrap();
    let log = std::fs::read_to_string(&ran.log).unwrap();
    (ran.outcome, log)
}

fn have(cmd: &str) -> bool {
    std::process::Command::new("sh")
        .args(["-c", &format!("command -v {cmd}")])
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn the_examples_parse() {
    let cfg = sokki_engine::hooks::config::load(&examples()).unwrap();
    let names: Vec<&str> = cfg.hooks.iter().map(|h| h.name.as_str()).collect();
    assert_eq!(
        names,
        ["tasks-to-reminders", "weekly-retro", "notes-to-git"]
    );
}

#[test]
fn tasks_to_reminders_in_test_mode() {
    if !have("osascript") {
        eprintln!("skipped: no osascript (not a Mac)");
        return;
    }
    let (paths, notes) = setup("reminders");
    let event = tools::fixture_event(MEETING_NOTE_READY, &notes).unwrap();
    let (outcome, log) = run(&paths, "tasks-to-reminders", &event, "test", &[]);
    assert_eq!(outcome, Outcome::Ok, "{log}");
    assert!(
        log.contains("would add to Sokki: Anna Beispiel: Halle reservieren"),
        "{log}"
    );
    assert!(
        log.contains("would add to Sokki: Ich: Einladung verschicken"),
        "{log}"
    );
    assert!(
        log.contains("would add to Sokki: Getränke bestellen"),
        "{log}"
    );
    assert!(
        log.contains("[Planung Herbstfest, 2026-11-26 (Sokki)]"),
        "{log}"
    );
}

#[test]
fn weekly_retro_keeps_the_rest_of_the_file() {
    let (paths, notes) = setup("retro");
    let note_event = tools::fixture_event(MEETING_NOTE_READY, &notes).unwrap();
    assert!(note_event.note_path.is_some());
    let out = notes.parent().unwrap().join("retro-context.md");
    std::fs::write(&out, "# My own context\n\nOnsite on Tuesday.\n").unwrap();
    let mut event = tools::fixture_event(SCHEDULE, &notes).unwrap();
    event.schedule = Some(ScheduleInfo {
        cron: "0 17 * * fri".into(),
        due: "2026-11-27T17:00:00+01:00".into(),
        previous: Some("2026-11-20T17:00:00+01:00".into()),
    });
    let env = [
        ("SOKKI_NOTES_DIR", notes.as_path()),
        ("RETRO_CONTEXT", out.as_path()),
    ];
    for _ in 0..2 {
        let (outcome, log) = run(&paths, "weekly-retro", &event, "manual", &env);
        assert_eq!(outcome, Outcome::Ok, "{log}");
    }
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(
        text.starts_with("# My own context\n\nOnsite on Tuesday.\n"),
        "{text}"
    );
    assert_eq!(
        text.matches("<!-- sokki:weekly-retro begin -->").count(),
        1,
        "{text}"
    );
    assert!(text.contains("## Meetings since 2026-11-20"), "{text}");
    assert!(text.contains("### 2026-11-26 Planung Herbstfest"), "{text}");
    assert!(
        text.contains("- Das Fest findet in der Turnhalle statt."),
        "{text}"
    );
    assert!(
        text.contains("- [ ] **Anna Beispiel**: Halle reservieren"),
        "{text}"
    );
    // A note from before the previous run is left out.
    let mut later = event.clone();
    later.schedule.as_mut().unwrap().previous = Some("2026-12-04T17:00:00+01:00".into());
    run(&paths, "weekly-retro", &later, "manual", &env);
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(!text.contains("Planung Herbstfest"), "{text}");
    assert!(
        text.contains("No meetings with decisions or tasks since 2026-12-04."),
        "{text}"
    );
    // Test mode writes nothing.
    let (outcome, log) = run(&paths, "weekly-retro", &event, "test", &env);
    assert_eq!(outcome, Outcome::Ok);
    assert!(log.contains("### 2026-11-26 Planung Herbstfest"), "{log}");
    assert!(!std::fs::read_to_string(&out)
        .unwrap()
        .contains("Planung Herbstfest"));
}

#[test]
fn notes_to_git_commits_each_change_once() {
    if !have("git") {
        eprintln!("skipped: no git");
        return;
    }
    let (paths, notes) = setup("git");
    let event = tools::fixture_event(MEETING_NOTE_READY, &notes).unwrap();
    let env = [("SOKKI_NOTES_DIR", notes.as_path())];
    let (outcome, log) = run(&paths, "notes-to-git", &event, "manual", &env);
    assert_eq!(outcome, Outcome::Ok, "{log}");
    assert!(log.contains("committed"), "{log}");
    assert!(
        log.contains("Planung Herbstfest (2026-11-26_14-03-00)"),
        "{log}"
    );
    let (outcome, log) = run(&paths, "notes-to-git", &event, "manual", &env);
    assert_eq!(outcome, Outcome::Ok, "{log}");
    assert!(log.contains("nothing changed"), "{log}");
    let count = std::process::Command::new("git")
        .args([
            "-C",
            &notes.to_string_lossy(),
            "rev-list",
            "--count",
            "HEAD",
        ])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&count.stdout).trim(), "1");
}

//! What `sokki hooks …` does, shared by every host's command line: list,
//! allow, run one by hand, test all on made-up data, read the log.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::config::{self, Config, Hook};
use super::content;
use super::event::{self, HookEvent, Meta, ScheduleInfo};
use super::exec::{self, Outcome};
use super::store::{Run, Store};
use super::{Gate, Paths};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    Allowed,
    /// Allowed once, changed since.
    Changed,
    New,
}

impl Approval {
    pub fn as_str(self) -> &'static str {
        match self {
            Approval::Allowed => "allowed",
            Approval::Changed => "changed, needs allow",
            Approval::New => "new, needs allow",
        }
    }
}

pub struct HookInfo {
    pub hook: Hook,
    pub approval: Approval,
    pub last: Option<Run>,
}

pub fn approval(store: &Store, hook: &Hook, dir: &Path) -> Result<Approval> {
    Ok(match store.allowed_hash(&hook.name)? {
        Some(h) if h == hook.fingerprint(dir) => Approval::Allowed,
        Some(_) => Approval::Changed,
        None => Approval::New,
    })
}

pub fn list(paths: &Paths) -> Result<(Config, Vec<HookInfo>)> {
    let cfg = config::load(&paths.config_dir)?;
    let store = Store::open(&paths.state_dir)?;
    let mut out = Vec::new();
    for hook in &cfg.hooks {
        out.push(HookInfo {
            approval: approval(&store, hook, &cfg.dir)?,
            last: store.runs(Some(&hook.name), 1)?.into_iter().next(),
            hook: hook.clone(),
        });
    }
    Ok((cfg, out))
}

/// Pins the hook as it is now. Returns it and the files whose content was
/// pinned, so the caller can show what was approved.
pub fn allow(paths: &Paths, name: &str) -> Result<(Hook, Vec<PathBuf>)> {
    let cfg = config::load(&paths.config_dir)?;
    let hook = cfg
        .get(name)
        .with_context(|| format!("no hook {name:?} in {}", paths.config_file().display()))?
        .clone();
    let store = Store::open(&paths.state_dir)?;
    store.allow(&hook.name, &hook.fingerprint(&cfg.dir))?;
    let files = hook.script_files(&cfg.dir);
    Ok((hook, files))
}

pub fn revoke(paths: &Paths, name: &str) -> Result<bool> {
    Store::open(&paths.state_dir)?.revoke(name)
}

pub fn runs(paths: &Paths, hook: Option<&str>, limit: usize) -> Result<Vec<Run>> {
    Store::open(&paths.state_dir)?.runs(hook, limit)
}

/// A made-up meeting note, for `sokki hooks test` and the examples. No real
/// names.
pub const FIXTURE_NOTE: &str = "---\ntype: meeting\ndate: 2026-11-26\nstart: \"14:03\"\nduration: 34 min\nparticipants:\n  - Ich\n  - Anna Beispiel\n  - Speaker 3\nsource_app: Zoom\ntags: [meeting]\n---\n\n# Planung Herbstfest\n\n## Zusammenfassung\n\n- Das Herbstfest ist am 12. Dezember in der Turnhalle.\n- Anna Beispiel reserviert die Halle, die Einladungen gehen nächste Woche raus.\n\n## Entscheidungen\n\n- Das Fest findet in der Turnhalle statt.\n- Das Budget bleibt bei 400 Euro.\n\n## Aufgaben\n\n- [ ] **Anna Beispiel**: Halle reservieren\n- [ ] **Ich**: Einladung verschicken\n- [ ] Getränke bestellen\n\n## Transkript\n\n**[00:00] Ich:** Hallo zusammen, kurz zum Herbstfest.\n\n**[00:04] Anna Beispiel:** Ich habe die Turnhalle angefragt, sie ist am 12. frei.\n\n**[00:11] Speaker 3:** Dann bestelle ich die Getränke.\n\nVon KI zusammengefasst. Lokal, kostenlos, Open Source. Diktier-App und Meeting-Notetaker: sokki.org\n";

/// The fixture meeting's id.
pub const FIXTURE_ID: &str = "2026-11-26_14-03-00";

/// A made-up event of kind `kind`. Meeting events point at the fixture
/// note, written into `dir`.
pub fn fixture_event(kind: &str, dir: &Path) -> Result<HookEvent> {
    let note = dir.join("2026-11-26 1403 Planung Herbstfest.md");
    if !note.exists() {
        std::fs::create_dir_all(dir)?;
        std::fs::write(&note, FIXTURE_NOTE)?;
    }
    let meta = Meta {
        title: Some("Planung Herbstfest".into()),
        started_at: Some("2026-11-26T14:03:00+01:00".into()),
        duration_s: Some(2040.0),
        app: Some("Zoom".into()),
        ..Default::default()
    };
    let mut e = match kind {
        event::DICTATION_DONE => HookEvent::dictation(
            1,
            "Erinnere mich morgen daran, die Halle für das Herbstfest zu bestätigen.",
            Meta {
                language: Some("de".into()),
                ..Default::default()
            },
        ),
        event::OWN_NOTES_SAVED => {
            let notes = dir.join("notes.md");
            std::fs::write(&notes, "- Halle: Turnhalle, 12.12.\n- Budget 400 Euro\n")?;
            HookEvent::own_notes(FIXTURE_ID, &notes, meta)
        }
        event::SCHEDULE => HookEvent {
            schedule: Some(ScheduleInfo {
                cron: "0 17 * * fri".into(),
                due: event::now(),
                previous: None,
            }),
            ..HookEvent::new(event::SCHEDULE, "schedule:test")
        },
        event::MEETING_RECORDED => HookEvent::meeting(kind, FIXTURE_ID, None, meta),
        k if event::EVENTS.contains(&k) => HookEvent::meeting(k, FIXTURE_ID, Some(&note), meta),
        other => bail!(
            "unknown event {other:?}; one of {}",
            event::EVENTS.join(", ")
        ),
    };
    content::complete_meta(&mut e);
    Ok(e)
}

/// How a run by hand went.
pub struct Ran {
    pub outcome: Outcome,
    pub log: PathBuf,
    pub timeout: Duration,
}

/// Runs `name` on `event` now, outside the queue, and logs it like any run.
/// A run by hand needs no `allow` (you start it yourself), but gets only
/// its scope. Waits while a meeting records or is processed.
pub fn run_once(
    paths: &Paths,
    name: &str,
    event: &HookEvent,
    trigger: &str,
    gate: &dyn Fn() -> Gate,
    extra_env: &[(String, String)],
    mut waiting: impl FnMut(),
) -> Result<Ran> {
    let cfg = config::load(&paths.config_dir)?;
    let hook = cfg
        .get(name)
        .with_context(|| format!("no hook {name:?} in {}", paths.config_file().display()))?
        .clone();
    if hook.on != event.event {
        bail!(
            "{name} runs on {}, not {}; pass --event {}",
            hook.on,
            event.event,
            hook.on
        );
    }
    let mut told = false;
    while gate() != Gate::Free {
        if !told {
            waiting();
            told = true;
        }
        std::thread::sleep(Duration::from_secs(5));
    }
    let store = Store::open(&paths.state_dir)?;
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    let log = paths
        .logs_dir()
        .join(format!("{stamp}_{}_{trigger}.log", hook.name));
    let run = store.begin_run(
        None,
        &hook.name,
        &event.event,
        &event.id,
        1,
        trigger,
        Some(&log),
    )?;
    let payload = content::for_scope(event, &hook.scope);
    let mut env = extra_env.to_vec();
    if trigger == "test" {
        env.push(("SOKKI_HOOK_TEST".into(), "1".into()));
    }
    let outcome = exec::run(&hook, &cfg.dir, &payload, &env, gate, &log);
    let error = (!outcome.is_ok()).then(|| outcome.describe(hook.timeout));
    store.end_run(run, outcome.status(), outcome.exit_code(), error.as_deref())?;
    Ok(Ran {
        outcome,
        log,
        timeout: hook.timeout,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::event::{DICTATION_DONE, MEETING_NOTE_READY};

    fn paths(name: &str) -> Paths {
        let root =
            std::env::temp_dir().join(format!("sokki-hooks-tools-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("config")).unwrap();
        Paths {
            config_dir: root.join("config"),
            state_dir: root.join("state"),
        }
    }

    #[test]
    fn allow_pins_until_the_script_changes() {
        let p = paths("allow");
        std::fs::write(
            p.config_file(),
            "[[hook]]\nname = \"a\"\non = \"meeting.note_ready\"\nrun = \"./a.sh\"\nscope = [\"tasks\"]\n",
        )
        .unwrap();
        std::fs::write(p.config_dir.join("a.sh"), "#!/bin/sh\ncat\n").unwrap();
        let (_, infos) = list(&p).unwrap();
        assert_eq!(infos[0].approval, Approval::New);
        let (hook, files) = allow(&p, "a").unwrap();
        assert_eq!(hook.name, "a");
        assert_eq!(files, vec![p.config_dir.join("a.sh")]);
        assert_eq!(list(&p).unwrap().1[0].approval, Approval::Allowed);
        std::fs::write(p.config_dir.join("a.sh"), "#!/bin/sh\ncurl evil\n").unwrap();
        assert_eq!(list(&p).unwrap().1[0].approval, Approval::Changed);
        assert!(allow(&p, "nope").is_err());
        let _ = std::fs::remove_dir_all(p.config_dir.parent().unwrap());
    }

    #[test]
    fn run_by_hand_on_the_fixture_gets_its_scope() {
        let p = paths("run");
        std::fs::write(
            p.config_file(),
            "[[hook]]\nname = \"tasks\"\non = \"meeting.note_ready\"\nrun = \"cat > seen.json\"\nscope = [\"tasks\"]\n",
        )
        .unwrap();
        let fixtures = p.state_dir.join("fixtures");
        let e = fixture_event(MEETING_NOTE_READY, &fixtures).unwrap();
        assert_eq!(
            e.meta.participants,
            vec!["Ich", "Anna Beispiel", "Speaker 3"]
        );
        let free = || Gate::Free;
        let ran = run_once(&p, "tasks", &e, "manual", &free, &[], || {}).unwrap();
        assert_eq!(ran.outcome, Outcome::Ok);
        let seen: HookEvent =
            serde_json::from_str(&std::fs::read_to_string(p.config_dir.join("seen.json")).unwrap())
                .unwrap();
        assert_eq!(seen.content.tasks.as_ref().unwrap().len(), 3);
        assert!(seen.content.transcript.is_none());
        assert!(seen.content.summary.is_none());
        let log = runs(&p, Some("tasks"), 5).unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(
            (log[0].status.as_str(), log[0].trigger.as_str()),
            ("ok", "manual")
        );
        // The wrong event for the hook.
        let d = fixture_event(DICTATION_DONE, &fixtures).unwrap();
        assert!(run_once(&p, "tasks", &d, "manual", &free, &[], || {}).is_err());
        let _ = std::fs::remove_dir_all(p.config_dir.parent().unwrap());
    }

    #[test]
    fn every_event_has_a_fixture() {
        let dir = std::env::temp_dir().join(format!("sokki-hooks-fixtures-{}", std::process::id()));
        for kind in event::EVENTS {
            let e = fixture_event(kind, &dir).unwrap();
            assert_eq!(e.event, kind);
        }
        assert!(fixture_event("meeting.started", &dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

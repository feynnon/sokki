//! Runs one hook: `/bin/sh -c <run>` in the hooks folder, the event JSON on
//! stdin, `SOKKI_EVENT` and `SOKKI_ID` in the environment, stdout and
//! stderr into the run's log file.
//!
//! - A meeting outranks a hook: while one records or is processed the
//!   hook's processes are paused (SIGSTOP) and continue after it; paused
//!   time does not count towards the timeout.
//! - At the timeout the hook's whole process group gets SIGTERM, then
//!   SIGKILL after a grace period.
//! - On macOS it runs apart from Sokki, without Sokki's privacy grants
//!   ([`super::spawn`]).

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use super::config::Hook;
use super::event::HookEvent;
use super::Gate;

const POLL: Duration = Duration::from_millis(100);
const GRACE: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Ok,
    /// The exit code, or `None` when a signal ended it.
    Failed(Option<i32>),
    TimedOut,
    /// It could not start.
    Error(String),
}

impl Outcome {
    pub fn is_ok(&self) -> bool {
        *self == Outcome::Ok
    }

    pub fn status(&self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Failed(_) => "failed",
            Outcome::TimedOut => "timeout",
            Outcome::Error(_) => "error",
        }
    }

    pub fn exit_code(&self) -> Option<i32> {
        match self {
            Outcome::Ok => Some(0),
            Outcome::Failed(c) => *c,
            _ => None,
        }
    }

    /// For the notice and the log, in a few words.
    pub fn describe(&self, timeout: Duration) -> String {
        match self {
            Outcome::Ok => "done".into(),
            Outcome::Failed(Some(c)) => format!("exited with {c}"),
            Outcome::Failed(None) => "was stopped by a signal".into(),
            Outcome::TimedOut => format!("took longer than {} and was stopped", human(timeout)),
            Outcome::Error(e) => format!("could not start: {e}"),
        }
    }
}

pub fn human(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        s if s >= 3600 && s % 3600 == 0 => format!("{} h", s / 3600),
        s if s >= 60 && s % 60 == 0 => format!("{} min", s / 60),
        s => format!("{s} s"),
    }
}

/// Folders added to `PATH`: an app started from the Finder gets only the
/// system folders, and hooks call `git`, `claude`, `sokki` and friends.
fn path_env() -> String {
    let mut parts: Vec<String> = std::env::var("PATH")
        .unwrap_or_else(|_| "/usr/bin:/bin:/usr/sbin:/sbin".into())
        .split(':')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect();
    let home = std::env::var("HOME").unwrap_or_default();
    for extra in [
        format!("{home}/.local/bin"),
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
    ] {
        if !parts.contains(&extra) {
            parts.push(extra);
        }
    }
    parts.join(":")
}

#[cfg(unix)]
fn signal_group(pgid: u32, sig: i32) {
    unsafe {
        libc::kill(-(pgid as i32), sig);
    }
}

/// Runs `hook` on `event` (already limited to its scope) and waits for it.
/// `gate` says whether a meeting needs the computer; `extra_env` comes from
/// the host (e.g. `SOKKI_NOTES_DIR`).
pub fn run(
    hook: &Hook,
    dir: &Path,
    event: &HookEvent,
    extra_env: &[(String, String)],
    gate: &dyn Fn() -> Gate,
    log_path: &Path,
) -> Outcome {
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut log = match std::fs::File::create(log_path) {
        Ok(f) => f,
        Err(e) => return Outcome::Error(format!("log file {}: {e}", log_path.display())),
    };
    let _ = writeln!(
        log,
        "# sokki hook {:?} on {} {} at {}\n# run: {}\n",
        hook.name,
        event.event,
        event.id,
        super::event::now(),
        hook.run
    );
    let mut env: Vec<(OsString, OsString)> = vec![
        ("PATH".into(), path_env().into()),
        ("SOKKI_EVENT".into(), event.event.clone().into()),
        ("SOKKI_ID".into(), event.id.clone().into()),
        ("SOKKI_HOOK".into(), hook.name.clone().into()),
        ("SOKKI_HOOKS_DIR".into(), dir.as_os_str().to_owned()),
    ];
    if let Some(p) = &event.note_path {
        env.push(("SOKKI_NOTE_PATH".into(), p.clone().into()));
    }
    for (k, v) in extra_env {
        env.push((k.into(), v.into()));
    }
    let spawned = super::spawn::spawn(&super::spawn::Spec {
        program: "/bin/sh",
        args: &["-c", &hook.run],
        env: &env,
        cwd: dir,
        stdout: &log,
        stderr: &log,
    });
    let (mut child, mut stdin) = match spawned {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(log, "# sokki: could not start: {e}");
            return Outcome::Error(e.to_string());
        }
    };
    let pgid = child.id();
    // Written from a thread: a hook that never reads stdin must not block
    // the runner, and a big payload must not fill the pipe.
    let json = event.to_json();
    std::thread::spawn(move || {
        let _ = stdin.write_all(json.as_bytes());
        let _ = stdin.write_all(b"\n");
    });

    let mut ran = Duration::ZERO;
    let mut last = Instant::now();
    let mut paused = false;
    let mut killed_at: Option<Instant> = None;
    let outcome = loop {
        match child.try_wait() {
            Ok(Some(exit)) => {
                if killed_at.is_some() {
                    break Outcome::TimedOut;
                }
                break if exit.success() {
                    Outcome::Ok
                } else {
                    Outcome::Failed(exit.code)
                };
            }
            Ok(None) => {}
            Err(e) => break Outcome::Error(e.to_string()),
        }
        let now = Instant::now();
        if !paused {
            ran += now - last;
        }
        last = now;
        if let Some(at) = killed_at {
            if now - at > GRACE {
                #[cfg(unix)]
                signal_group(pgid, libc::SIGKILL);
                child.kill();
            }
        } else if ran > hook.timeout {
            let _ = writeln!(log, "\n# sokki: stopped after {}", human(hook.timeout));
            #[cfg(unix)]
            {
                signal_group(pgid, libc::SIGTERM);
                signal_group(pgid, libc::SIGCONT);
            }
            #[cfg(not(unix))]
            child.kill();
            paused = false;
            killed_at = Some(now);
        } else {
            let busy = gate() != Gate::Free;
            if busy != paused {
                #[cfg(unix)]
                signal_group(pgid, if busy { libc::SIGSTOP } else { libc::SIGCONT });
                let _ = writeln!(
                    log,
                    "\n# sokki: {} at {}",
                    if busy {
                        "paused for a meeting"
                    } else {
                        "continued"
                    },
                    super::event::now()
                );
                paused = busy;
            }
        }
        std::thread::sleep(POLL);
    };
    // After a timeout, children the hook left in its group (a backgrounded
    // `&`) go too. After a clean exit they may keep running: a hook may
    // start something on purpose.
    #[cfg(unix)]
    if outcome == Outcome::TimedOut {
        signal_group(pgid, libc::SIGKILL);
    }
    let _ = writeln!(
        log,
        "\n# sokki: {} after {}",
        outcome.describe(hook.timeout),
        human(Duration::from_secs(ran.as_secs()))
    );
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::config::{Hook, Match};
    use crate::hooks::event::{HookEvent, Meta, MEETING_NOTE_READY};
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::Arc;

    fn hook(run: &str, timeout_ms: u64) -> Hook {
        Hook {
            name: "t".into(),
            on: MEETING_NOTE_READY.into(),
            run: run.into(),
            cron: None,
            matches: Match::default(),
            scope: vec![],
            timeout: Duration::from_millis(timeout_ms),
            enabled: true,
        }
    }

    fn dir(name: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("sokki-hooks-exec-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn event() -> HookEvent {
        HookEvent::meeting(MEETING_NOTE_READY, "m1", None, Meta::default())
    }

    #[test]
    fn stdin_env_and_exit_codes() {
        let d = dir("basic");
        let log = d.join("run.log");
        let free = || Gate::Free;
        let ok = run(
            &hook(
                "cat > got.json; echo \"$SOKKI_EVENT $SOKKI_ID $EXTRA\"",
                5000,
            ),
            &d,
            &event(),
            &[("EXTRA".into(), "x".into())],
            &free,
            &log,
        );
        assert_eq!(ok, Outcome::Ok);
        let got: HookEvent =
            serde_json::from_str(&std::fs::read_to_string(d.join("got.json")).unwrap()).unwrap();
        assert_eq!(got.id, "m1");
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(text.contains("meeting.note_ready m1 x"), "{text}");
        assert!(text.contains("# sokki: done"), "{text}");
        let failed = run(
            &hook("echo broken >&2; exit 3", 5000),
            &d,
            &event(),
            &[],
            &free,
            &log,
        );
        assert_eq!(failed, Outcome::Failed(Some(3)));
        assert!(std::fs::read_to_string(&log).unwrap().contains("broken"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_hook_that_never_reads_stdin_still_finishes() {
        let d = dir("nostdin");
        let free = || Gate::Free;
        let mut e = event();
        e.content.transcript = Some("x".repeat(1 << 20));
        let r = run(&hook("exit 0", 5000), &d, &e, &[], &free, &d.join("l.log"));
        assert_eq!(r, Outcome::Ok);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn timeout_kills_the_whole_group() {
        let d = dir("timeout");
        let free = || Gate::Free;
        let begun = Instant::now();
        // A child in the background that would outlive its shell.
        let r = run(
            &hook("(sleep 30; touch survived) & sleep 30", 300),
            &d,
            &event(),
            &[],
            &free,
            &d.join("l.log"),
        );
        assert_eq!(r, Outcome::TimedOut);
        assert!(
            begun.elapsed() < Duration::from_secs(5),
            "{:?}",
            begun.elapsed()
        );
        std::thread::sleep(Duration::from_millis(300));
        let ps = std::process::Command::new("pgrep")
            .args(["-f", "sleep 30; touch survived"])
            .output()
            .unwrap();
        assert!(ps.stdout.is_empty(), "a child outlived the timeout");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn pauses_while_a_meeting_records_and_the_pause_does_not_count() {
        let d = dir("pause");
        let gate = Arc::new(AtomicU8::new(0));
        let g = gate.clone();
        let check = move || match g.load(Ordering::SeqCst) {
            0 => Gate::Free,
            _ => Gate::Recording,
        };
        let flip = gate.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            flip.store(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(1500));
            flip.store(0, Ordering::SeqCst);
        });
        let begun = Instant::now();
        // 0.8 s of work with a 1.2 s timeout: only the paused 1.5 s
        // would push it over.
        let r = run(
            &hook("sleep 0.8", 1200),
            &d,
            &event(),
            &[],
            &check,
            &d.join("l.log"),
        );
        assert_eq!(r, Outcome::Ok);
        assert!(
            begun.elapsed() >= Duration::from_millis(1500),
            "{:?}",
            begun.elapsed()
        );
        let log = std::fs::read_to_string(d.join("l.log")).unwrap();
        assert!(
            log.contains("paused for a meeting") && log.contains("continued"),
            "{log}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_missing_command_fails_with_its_exit_code() {
        let d = dir("missing");
        let free = || Gate::Free;
        let r = run(
            &hook("./does-not-exist.sh", 5000),
            &d,
            &event(),
            &[],
            &free,
            &d.join("l.log"),
        );
        assert_eq!(r, Outcome::Failed(Some(127)));
        let _ = std::fs::remove_dir_all(&d);
    }
}

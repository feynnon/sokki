//! Keeps the computer from going to sleep while a meeting is being recorded
//! (the Linux side of `src-tauri/src/meeting/keep_awake.rs`).
//!
//! On 2026-11-19 a Mac went to idle sleep during an in-person meeting and the
//! recording was cut off. Here a systemd-logind "sleep" inhibitor in block
//! mode does the same job as the Mac's power assertion: no suspend or
//! hibernate on idle while it is held. The screen may still dim, lock and
//! turn off ("idle" is not inhibited). Check with `systemd-inhibit --list`.
//!
//! The lock is held by `systemd-inhibit … cat`, whose standard input is a
//! pipe from this process. Dropping `KeepAwake` closes the pipe; if the
//! daemon dies, the kernel closes it. Either way `cat` ends, and with it the
//! lock, so a crash never leaves the computer unable to sleep.
//!
//! Limits: logind still suspends when the lid closes (its default
//! `LidSwitchIgnoreInhibited=yes`), like the Mac. Without systemd (no
//! `systemd-inhibit`) nothing is held and a warning is logged.

use std::process::{Child, Command, Stdio};

/// The reason shown by `systemd-inhibit --list`.
pub const REASON: &str = "Sokki is recording a meeting";

/// Holds the sleep inhibitor until dropped.
pub struct KeepAwake {
    child: Option<Child>,
}

impl KeepAwake {
    /// Takes the inhibitor. Never fails: a recording without it still
    /// records, so a failure is only logged.
    pub fn acquire() -> Self {
        let child = Command::new("systemd-inhibit")
            .args([
                "--what=sleep",
                "--mode=block",
                "--who=Sokki",
                &format!("--why={REASON}"),
                "cat",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match child {
            Ok(child) => {
                log::info!("Keeping the computer awake while recording");
                Self { child: Some(child) }
            }
            Err(e) => {
                log::warn!("Keeping the computer awake failed (systemd-inhibit: {e})");
                Self { child: None }
            }
        }
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        // End of input ends `cat`, and `systemd-inhibit` releases the lock.
        drop(child.stdin.take());
        match child.wait() {
            Ok(_) => log::info!("The computer may sleep again"),
            Err(e) => log::warn!("Releasing the sleep inhibitor failed: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Our inhibitor as `systemd-inhibit --list` shows it, or None when
    /// logind can't be asked (no systemd, e.g. in a build container).
    fn listed() -> Option<bool> {
        let out = Command::new("systemd-inhibit").arg("--list").output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).contains(REASON))
    }

    #[test]
    fn inhibitor_lives_exactly_as_long_as_the_guard() {
        if listed().is_none() {
            eprintln!("skipped: systemd-logind is not reachable here");
            return;
        }
        let guard = KeepAwake::acquire();
        // systemd-inhibit registers the lock just after it starts.
        let held = (0..50).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(100));
            listed() == Some(true)
        });
        assert!(held, "systemd-inhibit --list should show the lock while held");
        drop(guard);
        assert_eq!(listed(), Some(false), "the lock should be gone after drop");
    }
}

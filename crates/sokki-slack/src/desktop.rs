//! Which channel a huddle is in, from the Slack Mac app's own log.
//!
//! Slack's API gives the call id of the person's huddle
//! (`users.profile.get` → `huddle_state_call_id`) but not its channel:
//! `huddle_state_channel_id`, listed in Slack's user-object reference, stayed
//! empty in a real huddle (2026-11-24), and `calls.info` answers
//! `internal_error` for a huddle's id. The Slack app, though, opens a
//! window named after the channel when the person joins a huddle and logs
//! it in `webapp-console.log`:
//!
//! ```text
//! [11/24/26, 21:58:02:239] info: [WINDOW-MGR] Created huddle_C0EXAMPLE01 child window
//! ```
//!
//! Sokki reads only the end of that file, only while it records a Slack
//! call, and keeps nothing from it but the channel id. The call id from the
//! profile then confirms it: the huddle's message in that channel carries
//! the same id (`room.id`).

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// How much of the log's end to read: the window line is written when the
/// person joins, seconds before Sokki starts recording.
const TAIL_BYTES: u64 = 2 * 1024 * 1024;

/// Where the Slack Mac app logs: the direct download and the App Store
/// version.
pub fn log_files(home: &Path) -> Vec<PathBuf> {
    let direct = home.join("Library/Application Support/Slack/logs/default");
    let store = home.join(
        "Library/Containers/com.tinyspeck.slackmacgap/Data/Library/Application Support/Slack/logs/default",
    );
    [direct, store]
        .into_iter()
        .map(|d| d.join("webapp-console.log"))
        .filter(|p| p.is_file())
        .collect()
}

/// The channel of the huddle window opened last, from the end of the log.
pub fn current_huddle_channel(home: &Path) -> Option<String> {
    log_files(home)
        .iter()
        .filter_map(|p| {
            let text = tail(p, TAIL_BYTES).ok()?;
            let modified = std::fs::metadata(p).and_then(|m| m.modified()).ok();
            huddle_channel(&text).map(|c| (modified, c))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, c)| c)
}

fn tail(path: &Path, bytes: u64) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(bytes)))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// The channel (`C…`, `G…` or a DM's `D…`) of the last huddle window the log
/// shows being created.
pub fn huddle_channel(log: &str) -> Option<String> {
    log.lines()
        .rev()
        .find_map(|line| {
            let rest = line.split("Created huddle_").nth(1)?;
            let id: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            rest[id.len()..].starts_with(" child window").then_some(id)
        })
        .filter(|id| is_conversation_id(id))
}

fn is_conversation_id(id: &str) -> bool {
    id.len() >= 9
        && matches!(id.as_bytes()[0], b'C' | b'G' | b'D')
        && id
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "\
[11/24/26, 21:40:01:100] info: [WINDOW-MGR] Created huddle_C0AAAAAAAA1 child window
[11/24/26, 21:45:00:000] info: [WIN-RESTORE] Cleaning up window with Id huddle_C0AAAAAAAA1 due to it being closed
[11/24/26, 21:58:02:056] info: [WIN-OPEN] (T0TEAM) Opening managed window {\"windowId\":\"huddle_D0BBBBBBBB2\"}
[11/24/26, 21:58:02:239] info: [WINDOW-MGR] Created huddle_D0BBBBBBBB2 child window
[11/24/26, 21:58:02:242] info: [WINDOW-MGR] huddle_D0BBBBBBBB2 doctype ensured
[11/24/26, 21:58:08:391] info: [HUDDLE-SDK] (T0TEAM) Call ID: R0CCCCCCCC3
";

    #[test]
    fn the_last_huddle_window_names_the_channel() {
        assert_eq!(huddle_channel(LOG).as_deref(), Some("D0BBBBBBBB2"));
        assert_eq!(
            huddle_channel(&LOG[..LOG.find("[11/24/26, 21:45").unwrap()]).as_deref(),
            Some("C0AAAAAAAA1")
        );
        assert_eq!(
            huddle_channel("info: Created huddle_lowercase child window"),
            None
        );
        assert_eq!(huddle_channel(""), None);
    }

    #[test]
    fn only_the_end_of_a_long_log_is_read() {
        let dir = std::env::temp_dir().join(format!("sokki-slack-log-{}", std::process::id()));
        let logs = dir.join("Library/Application Support/Slack/logs/default");
        std::fs::create_dir_all(&logs).unwrap();
        let filler = "x".repeat(1024) + "\n";
        let mut text = String::from("Created huddle_C0OLDOLDOLD child window\n");
        for _ in 0..(TAIL_BYTES / 1024 + 10) {
            text.push_str(&filler);
        }
        text.push_str(LOG);
        std::fs::write(logs.join("webapp-console.log"), text).unwrap();
        assert_eq!(current_huddle_channel(&dir).as_deref(), Some("D0BBBBBBBB2"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

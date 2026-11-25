//! Finding a huddle's own message, the root of its thread.
//!
//! When a huddle starts, Slack posts a message in the channel (or DM) with
//! `subtype: huddle_thread` and a `room` object: `id` (the call id, "R…"),
//! `date_start`, `date_end`, `participant_history`, `thread_root_ts`
//! (sample in docs.slack.dev/reference/objects/conversation-object).

use std::time::Duration;

use serde_json::Value;

use crate::api::{Client, Error, Result};

/// How far before the recording's start the huddle may have begun (someone
/// started it, the person joined later).
const LOOK_BACK_S: u64 = 12 * 3600;
/// The huddle's message is posted when the huddle starts, so before Sokki
/// records; this much later still counts (clocks, a late prompt answer).
const LOOK_AHEAD_S: u64 = 300;
/// Pages of 15 messages; outside the Marketplace one per minute.
const MAX_PAGES: usize = 4;
const PAGE_GAP: Duration = Duration::from_secs(61);

/// A huddle's message.
#[derive(Debug, Clone, PartialEq)]
pub struct Huddle {
    pub channel: String,
    /// The message's own `ts`.
    pub ts: String,
    /// Where replies go: `room.thread_root_ts`, else the message itself.
    pub thread_ts: String,
    pub call_id: String,
    pub date_start: u64,
    /// `None` while the huddle runs.
    pub date_end: Option<u64>,
    /// Everyone who was in the huddle at some point.
    pub participants: Vec<String>,
}

/// The huddle among `messages` (one `conversations.history` page): the one
/// with `call_id` if known, else the latest one that ran while Sokki
/// recorded (`recorded`: start and end, Unix seconds).
pub fn find(
    messages: &[Value],
    channel: &str,
    call_id: Option<&str>,
    recorded: (u64, u64),
) -> Option<Huddle> {
    let huddles = messages
        .iter()
        .filter(|m| m["subtype"] == "huddle_thread")
        .filter_map(|m| parse(m, channel));
    match call_id {
        Some(id) => huddles.into_iter().find(|h| h.call_id == id),
        None => huddles
            .filter(|h| {
                h.date_start <= recorded.1 + LOOK_AHEAD_S
                    && h.date_end
                        .is_none_or(|end| end + LOOK_AHEAD_S >= recorded.0)
            })
            .max_by_key(|h| h.date_start),
    }
}

fn parse(m: &Value, channel: &str) -> Option<Huddle> {
    let room = &m["room"];
    let ts = m["ts"].as_str()?.to_string();
    let thread_ts = room["thread_root_ts"]
        .as_str()
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| ts.clone());
    let ids = |k: &str| -> Vec<String> {
        room[k]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|u| u.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    Some(Huddle {
        channel: m["channel"].as_str().unwrap_or(channel).to_string(),
        ts,
        thread_ts,
        call_id: room["id"].as_str().unwrap_or_default().to_string(),
        date_start: room["date_start"].as_u64().unwrap_or(0),
        date_end: room["date_end"].as_u64().filter(|&t| t > 0),
        participants: ids("participant_history"),
    })
}

/// Looks for the huddle in `channel` with `conversations.history`, a page
/// a minute, waiting out rate limits with `sleep`.
pub fn locate(
    client: &Client,
    channel: &str,
    call_id: Option<&str>,
    recorded: (u64, u64),
    sleep: &dyn Fn(Duration),
) -> Result<Option<Huddle>> {
    let oldest = recorded.0.saturating_sub(LOOK_BACK_S).to_string();
    let latest = (recorded.0 + LOOK_AHEAD_S).to_string();
    let mut cursor: Option<String> = None;
    for page in 0..MAX_PAGES {
        if page > 0 {
            sleep(PAGE_GAP);
        }
        let mut tries = 0;
        let (messages, next) = loop {
            match client.history(channel, &oldest, &latest, cursor.as_deref()) {
                Err(Error::RateLimited(s)) if tries < 3 => {
                    tries += 1;
                    log::info!("Slack: history rate-limited, waiting {s} s");
                    sleep(Duration::from_secs(s.max(1)));
                }
                other => break other?,
            }
        };
        if let Some(h) = find(&messages, channel, call_id, recorded) {
            return Ok(Some(h));
        }
        match next {
            Some(c) => cursor = Some(c),
            None => return Ok(None),
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn history() -> Vec<Value> {
        let v: Value =
            serde_json::from_str(include_str!("../tests/fixtures/history.json")).unwrap();
        v["messages"].as_array().unwrap().clone()
    }

    #[test]
    fn the_huddle_is_found_by_its_call_id() {
        let h = find(&history(), "C0TEST", Some("R0OLD"), (0, 0)).unwrap();
        assert_eq!(h.ts, "1764000000.000100");
        assert_eq!(h.thread_ts, "1764000000.000100");
        assert_eq!(h.date_end, Some(1764001800));

        let h = find(&history(), "C0TEST", Some("R0NEW"), (0, 0)).unwrap();
        assert_eq!(h.ts, "1764010000.000200");
        assert_eq!(h.channel, "C0TEST");
        assert_eq!(h.participants, ["U0ADA", "U0BOB"]);
        assert_eq!(h.date_end, None, "date_end 0 means still running");

        assert_eq!(find(&history(), "C0TEST", Some("R0GONE"), (0, 0)), None);
    }

    #[test]
    fn without_a_call_id_the_huddle_that_overlaps_the_recording() {
        // Recorded 10 min into the new huddle.
        let h = find(&history(), "C0TEST", None, (1764010600, 1764012000)).unwrap();
        assert_eq!(h.call_id, "R0NEW");
        // Recorded during the old one only.
        let h = find(&history(), "C0TEST", None, (1764000300, 1764001500)).unwrap();
        assert_eq!(h.call_id, "R0OLD");
        // Long before both.
        assert_eq!(
            find(&history(), "C0TEST", None, (1763000000, 1763001000)),
            None
        );
    }

    #[test]
    fn thread_root_ts_wins_when_slack_sets_it() {
        let m = json!([{"type": "message", "subtype": "huddle_thread", "ts": "5.0",
                        "room": {"id": "R1", "thread_root_ts": "4.0", "date_start": 5}}]);
        let h = find(m.as_array().unwrap(), "D1", Some("R1"), (0, 0)).unwrap();
        assert_eq!(
            (h.ts.as_str(), h.thread_ts.as_str(), h.channel.as_str()),
            ("5.0", "4.0", "D1")
        );
    }
}

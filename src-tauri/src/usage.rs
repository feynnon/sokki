//! Usage counts for "Help improve Sokki": how far setup got, which features
//! get used, crashes and how fast Sokki answers.
//!
//! Counted on this Mac in `usage.json`, whether the switch is on or off.
//! Nothing leaves the Mac that way (EDPB Guidelines 2/2023, para. 44). Only
//! the daily report ([`crate::voice_data`]'s `usage_report`) sends them, and
//! only with the switch on. The first report after turning it on also
//! carries what was counted before, such as setup.
//!
//! Only fixed names chosen here, numbers and times; never text, file names,
//! names or titles. To count something new, add a [`Count`] or [`Timing`] and
//! add its name to "What's shared" if it is a new kind of thing.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tauri::AppHandle;

const FILE: &str = "usage.json";
/// Written by the panic hook, read into the next report.
const CRASH_FILE: &str = "usage-crashes.jsonl";
/// Kept until a report takes them; older ones go first beyond this.
const MAX_EVENTS: usize = 200;
const MAX_TIMINGS: usize = 300;

static LOCK: Mutex<()> = Mutex::new(());
static CRASH_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Something used, counted per report.
#[derive(Clone, Copy, Debug)]
pub enum Count {
    DictationTyped,
    DictationEmpty,
    DictationSilent,
    DictationFailed,
    MeetingStarted,
    /// Started from the call-detection prompt.
    MeetingStartedFromCall,
    MeetingWithScreen,
    /// The sample meeting, for someone with no meeting today.
    SampleMeetingStarted,
    MeetingDone,
    SummaryOnMac,
    SummaryCloud,
    SummaryFailed,
    FileTranscribed,
    /// Started by itself because people talked (`meeting/auto.rs`).
    AutoMeetingStarted,
    /// An auto-started meeting the user discarded: a false start.
    AutoMeetingDiscarded,
    /// "Turn off auto start" on the card.
    AutoMeetingTurnedOff,
}

impl Count {
    fn name(self) -> &'static str {
        match self {
            Count::DictationTyped => "dictation_typed",
            Count::DictationEmpty => "dictation_empty",
            Count::DictationSilent => "dictation_silent",
            Count::DictationFailed => "dictation_failed",
            Count::MeetingStarted => "meeting_started",
            Count::MeetingStartedFromCall => "meeting_started_from_call",
            Count::MeetingWithScreen => "meeting_with_screen",
            Count::SampleMeetingStarted => "sample_meeting_started",
            Count::MeetingDone => "meeting_done",
            Count::SummaryOnMac => "summary_on_mac",
            Count::SummaryCloud => "summary_cloud",
            Count::SummaryFailed => "summary_failed",
            Count::FileTranscribed => "file_transcribed",
            Count::AutoMeetingStarted => "auto_meeting_started",
            Count::AutoMeetingDiscarded => "auto_meeting_discarded",
            Count::AutoMeetingTurnedOff => "auto_meeting_turned_off",
        }
    }
}

/// How long something took, in milliseconds.
#[derive(Clone, Copy, Debug)]
pub enum Timing {
    /// Shortcut pressed → microphone delivering.
    DictationReady,
    /// Shortcut let go → text typed.
    DictationTyped,
    /// Start → shown as recording.
    MeetingStart,
    /// Call ended → note ready.
    MeetingReady,
}

impl Timing {
    fn name(self) -> &'static str {
        match self {
            Timing::DictationReady => "dictation_ready_ms",
            Timing::DictationTyped => "dictation_typed_ms",
            Timing::MeetingStart => "meeting_start_ms",
            Timing::MeetingReady => "meeting_ready_ms",
        }
    }
}

/// Setup's screens, in order (`STEP_ORDER` in onboardingStore.ts), then
/// "finished" when setup is completed.
pub const SETUP_STEPS: [&str; 9] = [
    "welcome",
    "purpose",
    "permissions",
    "model",
    "voice",
    "demo",
    "try",
    "done",
    "finished",
];

/// Something that happens once: a setup step, the first dictation, an
/// update, a crash.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub event: String,
    /// Unix seconds.
    pub at: i64,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub props: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    /// Unix seconds of the first start with this module; 0 for installs
    /// from before it (their setup was never counted).
    #[serde(default)]
    first_start: i64,
    /// Once-only events already recorded ("setup:model", "first_dictation").
    #[serde(default)]
    reached: BTreeSet<String>,
    /// Waiting for the next report.
    #[serde(default)]
    events: Vec<Event>,
    #[serde(default)]
    counts: BTreeMap<String, u64>,
    #[serde(default)]
    timings: BTreeMap<String, Vec<u64>>,
    /// The app version at the last start, to see updates.
    #[serde(default)]
    version: String,
    /// Modification time (unix seconds) of the newest macOS crash report
    /// already counted.
    #[serde(default)]
    native_crash_seen: i64,
}

fn path(app: &AppHandle) -> Option<PathBuf> {
    Some(crate::portable::app_data_dir(app).ok()?.join(FILE))
}

fn load(app: &AppHandle) -> State {
    path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(app: &AppHandle, s: &State) {
    let Some(p) = path(app) else { return };
    let Ok(text) = serde_json::to_string(s) else {
        return;
    };
    let tmp = p.with_extension("json.tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
}

fn with_state<T>(app: &AppHandle, f: impl FnOnce(&mut State) -> T) -> T {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = load(app);
    let out = f(&mut s);
    save(app, &s);
    out
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn push(s: &mut State, event: &str, props: serde_json::Map<String, serde_json::Value>) {
    s.events.push(Event {
        event: event.into(),
        at: now(),
        props,
    });
    let over = s.events.len().saturating_sub(MAX_EVENTS);
    s.events.drain(..over);
}

/// Records `event` the first time `key` is reached.
fn once(s: &mut State, key: &str, event: &str, props: serde_json::Map<String, serde_json::Value>) {
    if s.reached.insert(key.to_string()) {
        push(s, event, props);
    }
}

/// At start: the first start, updates since the last start, the crash hook.
pub fn init(app: &AppHandle) {
    if let Some(dir) = crate::portable::app_data_dir(app).ok() {
        let _ = CRASH_PATH.set(dir.join(CRASH_FILE));
        install_panic_hook();
    }
    let version = app.package_info().version.to_string();
    let onboarded = crate::settings::get_settings(app).onboarding_completed;
    with_state(app, |s| {
        if s.version.is_empty() {
            if onboarded {
                // Set up before this module: setup and the first dictation
                // happened uncounted; don't report them now.
                for step in SETUP_STEPS {
                    s.reached.insert(format!("setup:{step}"));
                }
                s.reached.insert("first_dictation".into());
                s.reached.insert("first_meeting".into());
            } else {
                s.first_start = now();
                once(s, "first_start", "first_start", Default::default());
            }
        } else if s.version != version {
            let mut props = serde_json::Map::new();
            props.insert("from".into(), s.version.clone().into());
            props.insert("to".into(), version.clone().into());
            push(s, "app_updated", props);
        }
        s.version = version;
    });
    if crate::test_mic::active() {
        test_hooks(app);
    }
}

/// Test mode only (scripts/test-usage.sh): `SOKKI_TEST_SETUP` replays setup
/// as a fresh install would count it (the test app skips the real setup
/// window), `SOKKI_TEST_PANIC` panics on a side thread, as a crash would.
fn test_hooks(app: &AppHandle) {
    let set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
    if set("SOKKI_TEST_SETUP") {
        with_state(app, |s| {
            s.first_start = now() - 600;
            s.reached
                .retain(|k| !k.starts_with("setup:") && k != "first_dictation");
            once(s, "first_start", "first_start", Default::default());
        });
        for step in [
            "purpose",
            "permissions",
            "model",
            "voice",
            "try",
            "done",
            "finished",
            "nonsense",
        ] {
            setup_step(app, step);
        }
    }
    if set("SOKKI_TEST_PANIC") {
        let _ = std::thread::Builder::new()
            .name("usage-test-panic".into())
            .spawn(|| panic!("test panic for the usage report"));
    }
}

/// A setup screen was shown. Unknown names are ignored.
pub fn setup_step(app: &AppHandle, step: &str) {
    let Some(index) = SETUP_STEPS.iter().position(|s| *s == step) else {
        return;
    };
    with_state(app, |s| {
        let mut props = serde_json::Map::new();
        props.insert("step".into(), step.into());
        props.insert("index".into(), (index + 1).into());
        if s.first_start > 0 {
            props.insert("seconds_since_start".into(), (now() - s.first_start).into());
        }
        once(s, &format!("setup:{step}"), "setup_step", props);
    });
}

pub fn count(app: &AppHandle, what: Count) {
    with_state(app, |s| {
        *s.counts.entry(what.name().into()).or_default() += 1;
        match what {
            Count::DictationTyped => {
                let mut props = serde_json::Map::new();
                if s.first_start > 0 {
                    props.insert("seconds_since_start".into(), (now() - s.first_start).into());
                }
                once(s, "first_dictation", "first_dictation", props);
            }
            Count::MeetingDone => once(s, "first_meeting", "first_meeting", Default::default()),
            _ => {}
        }
    });
}

pub fn time(app: &AppHandle, what: Timing, took: std::time::Duration) {
    with_state(app, |s| {
        let v = s.timings.entry(what.name().into()).or_default();
        v.push(took.as_millis().min(u64::MAX as u128) as u64);
        let over = v.len().saturating_sub(MAX_TIMINGS);
        v.drain(..over);
    });
}

// ----- Crashes -----

/// Where a panic happened, without the home folder or the cargo cache path.
fn crash_place(file: &str, line: u32) -> String {
    let file = match file.find("/registry/src/") {
        // ".../registry/src/index.crates.io-xxxx/foo-1.2.3/src/lib.rs"
        Some(i) => {
            let rest = &file[i + "/registry/src/".len()..];
            rest.split_once('/').map(|(_, r)| r).unwrap_or(rest)
        }
        None => file
            .rsplit_once("/src-tauri/")
            .map(|(_, r)| r)
            .or_else(|| file.rsplit_once("/crates/").map(|(_, r)| r))
            .unwrap_or(file),
    };
    let home = std::env::var("HOME").unwrap_or_default();
    let file = if home.len() > 1 {
        file.replace(&home, "~")
    } else {
        file.to_string()
    };
    format!("{file}:{line}")
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // No lock and no app handle here: a panic can happen while either
        // is held. One appended line.
        if let (Some(path), Some(loc)) = (CRASH_PATH.get(), info.location()) {
            let line = serde_json::json!({
                "at": now(),
                "place": crash_place(loc.file(), loc.line()),
                "thread": std::thread::current().name().is_some_and(|n| n == "main"),
            });
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(f, "{line}");
            }
        }
        previous(info);
    }));
}

/// Panics written by the hook, and macOS crash reports of this app newer
/// than the last counted, into events. The crash file is emptied.
fn collect_crashes(s: &mut State, version: &str) {
    if let Some(path) = CRASH_PATH.get() {
        if let Ok(text) = std::fs::read_to_string(path) {
            for line in text.lines().take(50) {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                    continue;
                };
                let mut props = serde_json::Map::new();
                props.insert("kind".into(), "panic".into());
                props.insert("place".into(), v["place"].clone());
                props.insert("main_thread".into(), v["thread"].clone());
                props.insert("version".into(), version.into());
                s.events.push(Event {
                    event: "crash".into(),
                    at: v["at"].as_i64().unwrap_or_else(now),
                    props,
                });
            }
            let _ = std::fs::remove_file(path);
        }
    }
    #[cfg(target_os = "macos")]
    for (at, kind) in native_crashes(s.native_crash_seen) {
        s.native_crash_seen = s.native_crash_seen.max(at);
        let mut props = serde_json::Map::new();
        props.insert("kind".into(), "native".into());
        props.insert("type".into(), kind.into());
        props.insert("version".into(), version.into());
        s.events.push(Event {
            event: "crash".into(),
            at,
            props,
        });
    }
    let over = s.events.len().saturating_sub(MAX_EVENTS);
    s.events.drain(..over);
}

/// macOS crash reports of this process newer than `since`: their time and
/// exception type ("EXC_BAD_ACCESS"). Only the type is read.
#[cfg(target_os = "macos")]
fn native_crashes(since: i64) -> Vec<(i64, String)> {
    let Some(home) = std::env::var_os("HOME") else {
        return vec![];
    };
    let dir = PathBuf::from(home).join("Library/Logs/DiagnosticReports");
    let Some(exe) = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
    else {
        return vec![];
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut out = vec![];
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !(name.starts_with(&format!("{exe}-")) && name.ends_with(".ips")) {
            continue;
        }
        let Some(at) = e
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
        else {
            continue;
        };
        if at <= since {
            continue;
        }
        // An .ips file: one JSON header line, then the report as JSON.
        let kind = std::fs::read_to_string(e.path())
            .ok()
            .and_then(|t| {
                let body = t.split_once('\n')?.1;
                let v: serde_json::Value = serde_json::from_str(body).ok()?;
                v["exception"]["type"].as_str().map(str::to_owned)
            })
            .filter(|k| k.len() <= 40 && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or_else(|| "unknown".into());
        out.push((at, kind));
    }
    out.sort();
    out.truncate(20);
    out
}

// ----- The report -----

/// What a report took, so a sent report can be taken out afterwards.
pub struct Taken {
    events: Vec<Event>,
    counts: BTreeMap<String, u64>,
    timings: BTreeMap<String, usize>,
}

fn summary(ms: &[u64]) -> serde_json::Value {
    let mut v = ms.to_vec();
    v.sort_unstable();
    let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
    serde_json::json!({ "n": v.len(), "median": at(0.5), "p90": at(0.9), "max": at(1.0) })
}

/// Settings that are switches or fixed choices. Never text, paths, names,
/// keys, words or prompts.
fn settings_snapshot(app: &AppHandle) -> serde_json::Value {
    let s = crate::settings::get_settings(app);
    let m = &s.meetings;
    let cloud = crate::meeting::process::CLOUD_PROVIDERS.contains(&m.llm_provider.as_str());
    serde_json::json!({
        "shortcut_activation": s.shortcut_activation,
        "overlay_style": s.overlay_style,
        "audio_feedback": s.audio_feedback,
        "autostart": s.autostart_enabled,
        "update_checks": s.update_checks_enabled,
        "post_process": s.post_process_enabled,
        "paste_method": s.paste_method,
        "language_detection": s.language_detection,
        "filler_word_removal": s.filler_word_removal_enabled,
        "translate_to_english": s.translate_to_english,
        "glossary_words": m.glossary.len(),
        "meeting_detection": m.detection_enabled,
        "speakers": m.diarization_enabled,
        "summary": m.summary_enabled,
        "summary_where": if cloud { "cloud" } else { "mac" },
        "chat_notice": m.chat_notice_enabled,
        "phone_link": m.phone_link_enabled,
        "keep_audio_days": m.keep_audio_days,
    })
}

/// The report's `usage` part, and what it took.
pub fn report(app: &AppHandle) -> (serde_json::Value, Taken) {
    let version = app.package_info().version.to_string();
    let (events, counts, timings, first_start) = with_state(app, |s| {
        collect_crashes(s, &version);
        (
            s.events.clone(),
            s.counts.clone(),
            s.timings.clone(),
            s.first_start,
        )
    });
    let value = serde_json::json!({
        "first_start": (first_start > 0).then_some(first_start),
        "events": events,
        "counts": counts,
        "timings": timings
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| (k.clone(), summary(v)))
            .collect::<BTreeMap<_, _>>(),
        "settings": settings_snapshot(app),
        "permissions": {
            "accessibility": crate::accessibility::trusted(),
        },
    });
    let taken = Taken {
        events,
        counts,
        timings: timings.iter().map(|(k, v)| (k.clone(), v.len())).collect(),
    };
    (value, taken)
}

/// After a report reached the counter: takes out what it carried, keeps
/// what was counted meanwhile.
pub fn sent(app: &AppHandle, taken: Taken) {
    with_state(app, |s| {
        s.events.retain(|e| !taken.events.contains(e));
        for (k, n) in taken.counts {
            if let Some(c) = s.counts.get_mut(&k) {
                *c = c.saturating_sub(n);
            }
        }
        s.counts.retain(|_, c| *c > 0);
        for (k, n) in taken.timings {
            if let Some(v) = s.timings.get_mut(&k) {
                v.drain(..n.min(v.len()));
            }
        }
        s.timings.retain(|_, v| !v.is_empty());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_places_lose_home_and_cargo_paths() {
        assert_eq!(
            crash_place(
                "/Users/x/.cargo/registry/src/index.crates.io-6f17d22bba15001f/tokio-1.40.0/src/rt.rs",
                12
            ),
            "tokio-1.40.0/src/rt.rs:12"
        );
        assert_eq!(
            crash_place("src/meeting/process.rs", 7),
            "src/meeting/process.rs:7"
        );
        assert_eq!(
            crash_place("/Users/x/handy/crates/sokki-engine/src/lib.rs", 3),
            "sokki-engine/src/lib.rs:3"
        );
    }

    #[test]
    fn summary_takes_median_and_p90() {
        let s = summary(&[10, 20, 30, 40, 1000]);
        assert_eq!(s["median"], 30);
        assert_eq!(s["p90"], 1000);
        assert_eq!(s["n"], 5);
    }

    #[test]
    fn once_records_only_the_first_time() {
        let mut s = State::default();
        once(
            &mut s,
            "first_dictation",
            "first_dictation",
            Default::default(),
        );
        once(
            &mut s,
            "first_dictation",
            "first_dictation",
            Default::default(),
        );
        assert_eq!(s.events.len(), 1);
    }
}

//! "Your data": the user's own recordings, kept on this Mac, and the two
//! things a user can agree to share.
//!
//! Kept for everyone, until the user deletes them: the user's own voice with
//! its text (dictations, and in meetings only the microphone segments
//! labelled with the user's name) in `<app data>/voice-data/`
//! (`clips/*.wav` and `manifest.jsonl`), apart from the recordings'
//! retention. Nothing of it is uploaded.
//!
//! Two choices, both off until the user turns them on:
//! - **Help improve Sokki**: once a day, under a random id, a report of
//!   how Sokki is used, problems it hit, how much data of which kind the
//!   user has, and the Mac's model and macOS version (never the Mac's name
//!   or serial number, never anyone's words).
//! - **Support Sokki & Open Source**: the user's anonymised data may be
//!   used freely (to improve Sokki, for open-source speech models, shared
//!   or sold), without a share. Supporters get no buyer messages.
//!
//! Buyer messages need no choice: Sokki reads a message from the counter
//! (a plain download) and matches it against the hours kept here; nothing
//! leaves the Mac unless the user answers "I'm interested".
//!
//! The counter service is set at build time (`SOKKI_COUNTER_URL`).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::AppHandle;

const DIR: &str = "voice-data";
const MANIFEST: &str = "manifest.jsonl";
const CONFIG: &str = "config.json";
/// Shorter clips are mostly "ja", "okay": little use, much noise.
const MIN_SECONDS: f64 = 1.0;
const SAMPLE_RATE: f64 = 16_000.0;

static LOCK: Mutex<()> = Mutex::new(());

/// The counter service, set at build time; `None`: nothing is ever sent.
pub fn counter_url() -> Option<&'static str> {
    option_env!("SOKKI_COUNTER_URL").filter(|u| !u.is_empty())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Config {
    /// "Help improve Sokki".
    #[serde(default)]
    improve: bool,
    /// "Support Sokki & Open Source" (was `donate`).
    #[serde(default, alias = "donate")]
    support: bool,
    /// Before this version: sell or donate was chosen and the counter knew
    /// this install. Read once, so that install leaves the count.
    #[serde(default, skip_serializing)]
    opt_in: bool,
    /// Random; identifies the install to the counter only (no name, no
    /// machine data). Replaced when the user deletes what was shared.
    #[serde(default)]
    install_id: String,
    /// The counter holds something under `install_id`.
    #[serde(default)]
    shared: bool,
    /// Unix seconds of the last count that reached the counter.
    #[serde(default)]
    last_report: i64,
    /// Unix seconds of the last daily report ("Help improve").
    #[serde(default)]
    last_usage: i64,
    /// Id of the last counter message already handled.
    #[serde(default)]
    last_notice: i64,
    /// The latest buyer message that matched this Mac's data.
    #[serde(default)]
    buyer: Option<BuyerRequest>,
}

/// Looks for a new message from Sokki this often.
const NOTICE_EVERY_S: u64 = 6 * 3600;

/// New clips are counted at most this often; turning a choice on or off,
/// and deleting, are sent right away.
const REPORT_EVERY_S: i64 = 3600;

/// "Help improve Sokki" reports once a day.
const USAGE_EVERY_S: i64 = 24 * 3600;

/// A buyer message is shown only if at least this much of the user's own
/// data in that language fits.
const MIN_BUYER_HOURS: f64 = 1.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Clip {
    file: String,
    text: String,
    /// "de", "en", … when known.
    language: Option<String>,
    seconds: f64,
    /// "dictation" or "meeting".
    source: String,
    created: i64,
}

/// Someone wants data in a language; this Mac has `hours` of it.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct BuyerRequest {
    /// The counter message's id.
    pub id: i64,
    /// "de", "en", …
    pub language: String,
    /// Whole hours of the user's own data in that language.
    pub hours: u32,
    /// Optional words from Sokki about the buyer.
    #[serde(default)]
    pub text: Option<String>,
    /// `None` until answered; then "interested" or "no".
    #[serde(default)]
    pub answer: Option<String>,
}

/// What the "Your data" page shows.
#[derive(Debug, Clone, Default, Serialize, specta::Type)]
pub struct VoiceDataStats {
    /// "Help improve Sokki" is on.
    pub improve: bool,
    /// "Support Sokki & Open Source" is on.
    pub support: bool,
    /// The counter holds something of this install ("Delete what was shared").
    pub shared: bool,
    pub clips: u32,
    pub total_seconds: f64,
    /// Language code ("de", "en", "other") → seconds.
    pub seconds_by_language: BTreeMap<String, f64>,
    /// Seconds per source ("dictation", "meeting").
    pub seconds_by_source: BTreeMap<String, f64>,
    pub bytes: u64,
    pub folder: String,
    /// The latest buyer message that matched (not for supporters).
    pub buyer: Option<BuyerRequest>,
}

/// The total over all counted installs, from the counter service.
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct CommunityTotal {
    pub installs: u32,
    pub total_seconds: f64,
    pub seconds_by_language: BTreeMap<String, f64>,
    /// How many of the installs support Sokki & Open Source.
    #[serde(default)]
    pub donors: u32,
    /// A message from Sokki. Plain text, or JSON for a buyer:
    /// `{"language": "de", "text": "…"}`.
    #[serde(default)]
    pub notice: Option<String>,
    /// Changes whenever the message is set; each id is handled once.
    #[serde(default)]
    pub notice_id: i64,
}

fn dir(app: &AppHandle) -> Result<PathBuf> {
    Ok(crate::portable::app_data_dir(app)?.join(DIR))
}

fn read_config(dir: &Path) -> Config {
    let mut c: Config = std::fs::read_to_string(dir.join(CONFIG))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    if c.opt_in {
        // Old "sell" or "donate": the counter knew this install. "Donate"
        // carries over as support (alias); "sell" has no successor.
        c.shared = true;
        c.opt_in = false;
    }
    c
}

fn write_config(dir: &Path, c: &Config) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(CONFIG), serde_json::to_string_pretty(c)?)?;
    Ok(())
}

fn new_install_id() -> String {
    // Time and address entropy are enough for an anonymous counter key.
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    let a = h.finish();
    let mut h2 = std::collections::hash_map::RandomState::new().build_hasher();
    h2.write_u64(a ^ 0x9e37_79b9_7f4a_7c15);
    format!("{a:016x}{:016x}", h2.finish())
}

/// Sets both choices. Turning both off takes this install out of the count;
/// what was shared stays until [`forget`].
pub fn set_choices(app: &AppHandle, improve: bool, support: bool) -> Result<()> {
    let d = dir(app)?;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut c = read_config(&d);
    let improve_turned_on = improve && !c.improve;
    c.improve = improve;
    c.support = support;
    if c.install_id.is_empty() {
        c.install_id = new_install_id();
    }
    if improve_turned_on {
        // The first daily report goes out with the next sync.
        c.last_usage = 0;
    }
    if support {
        // Supporters get no buyer messages.
        c.buyer = None;
    }
    write_config(&d, &c)?;
    drop(_g);
    sync_in_background(app.clone(), true);
    Ok(())
}

fn normalize_language(code: Option<&str>) -> String {
    match code.map(|c| c.to_ascii_lowercase()) {
        Some(c) if c.starts_with("de") => "de".into(),
        Some(c) if c.starts_with("en") => "en".into(),
        // Other ISO codes ("fr", "es-ES"): the first two letters.
        Some(c) if c.len() >= 2 && c != "auto" && c != "other" => c.chars().take(2).collect(),
        _ => "other".into(),
    }
}

fn append(dir: &Path, clip: &Clip) -> Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(MANIFEST))?;
    writeln!(f, "{}", serde_json::to_string(clip)?)?;
    Ok(())
}

fn clip_name(source: &str) -> String {
    let now = chrono::Utc::now();
    format!(
        "{source}-{}-{:04x}.wav",
        now.format("%Y%m%d-%H%M%S%3f"),
        rand_u16()
    )
}

fn rand_u16() -> u16 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u8(0);
    h.finish() as u16
}

/// Keeps a finished dictation: copies its WAV and stores its text.
pub fn add_dictation(app: &AppHandle, wav: &Path, text: &str, language: Option<&str>) {
    if text.trim().is_empty() {
        return;
    }
    let result = (|| -> Result<()> {
        let seconds = hound::WavReader::open(wav)
            .map(|r| r.duration() as f64 / r.spec().sample_rate as f64)
            .with_context(|| format!("reading {}", wav.display()))?;
        if seconds < MIN_SECONDS {
            return Ok(());
        }
        let d = dir(app)?;
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::fs::create_dir_all(d.join("clips"))?;
        let name = clip_name("dictation");
        std::fs::copy(wav, d.join("clips").join(&name))?;
        append(
            &d,
            &Clip {
                file: format!("clips/{name}"),
                text: text.trim().to_string(),
                language: Some(normalize_language(language)),
                seconds,
                source: "dictation".into(),
                created: chrono::Utc::now().timestamp(),
            },
        )
    })();
    match result {
        Ok(()) => sync_in_background(app.clone(), false),
        Err(e) => log::warn!("Voice data: dictation not kept: {e:#}"),
    }
}

/// One of the user's own segments from a meeting's microphone track.
pub struct OwnSegment<'a> {
    pub start_s: f64,
    pub end_s: f64,
    pub text: &'a str,
}

/// Keeps the user's own microphone segments of a processed meeting.
pub fn add_meeting(app: &AppHandle, mic: &[f32], segments: &[OwnSegment]) {
    if segments.is_empty() {
        return;
    }
    let result = (|| -> Result<usize> {
        let d = dir(app)?;
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::fs::create_dir_all(d.join("clips"))?;
        let mut kept = 0;
        for s in segments {
            let seconds = s.end_s - s.start_s;
            if seconds < MIN_SECONDS || s.text.trim().is_empty() {
                continue;
            }
            let a = ((s.start_s * SAMPLE_RATE) as usize).min(mic.len());
            let b = ((s.end_s * SAMPLE_RATE) as usize).min(mic.len());
            if b <= a {
                continue;
            }
            let name = clip_name("meeting");
            crate::audio_toolkit::save_wav_file(d.join("clips").join(&name), &mic[a..b])?;
            let lang = meeting_core::lang::detect_language(s.text).map(|l| match l {
                meeting_core::lang::Language::German => "de",
                meeting_core::lang::Language::English => "en",
            });
            append(
                &d,
                &Clip {
                    file: format!("clips/{name}"),
                    text: s.text.trim().to_string(),
                    language: Some(normalize_language(lang)),
                    seconds: (b - a) as f64 / SAMPLE_RATE,
                    source: "meeting".into(),
                    created: chrono::Utc::now().timestamp(),
                },
            )?;
            kept += 1;
        }
        Ok(kept)
    })();
    match result {
        Ok(n) => {
            log::info!("Voice data: kept {n} own segments of a meeting");
            sync_in_background(app.clone(), false);
        }
        Err(e) => log::warn!("Voice data: meeting segments not kept: {e:#}"),
    }
}

fn read_clips(dir: &Path) -> Vec<Clip> {
    std::fs::read_to_string(dir.join(MANIFEST))
        .map(|t| {
            t.lines()
                .filter_map(|l| serde_json::from_str::<Clip>(l).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn stats_in(dir: &Path) -> VoiceDataStats {
    let clips = read_clips(dir);
    let config = read_config(dir);
    let mut s = VoiceDataStats {
        improve: config.improve,
        support: config.support,
        shared: config.shared,
        buyer: if config.support { None } else { config.buyer },
        clips: clips.len() as u32,
        folder: dir.to_string_lossy().into_owned(),
        ..Default::default()
    };
    for c in &clips {
        s.total_seconds += c.seconds;
        *s.seconds_by_language
            .entry(normalize_language(c.language.as_deref()))
            .or_default() += c.seconds;
        *s.seconds_by_source.entry(c.source.clone()).or_default() += c.seconds;
        if let Ok(m) = std::fs::metadata(dir.join(&c.file)) {
            s.bytes += m.len();
        }
    }
    s
}

/// The page's numbers. Opening the page also sends what is due.
pub fn stats(app: &AppHandle) -> Result<VoiceDataStats> {
    sync_in_background(app.clone(), false);
    Ok(stats_in(&dir(app)?))
}

/// Deletes every kept recording and the manifest; the choices stay.
pub fn delete_all(app: &AppHandle) -> Result<()> {
    let d = dir(app)?;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = std::fs::remove_dir_all(d.join("clips"));
    let _ = std::fs::remove_file(d.join(MANIFEST));
    drop(_g);
    sync_in_background(app.clone(), true);
    Ok(())
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(10)))
        .build()
        .into()
}

fn post(path: &str, body: serde_json::Value) -> Result<()> {
    let url = counter_url().context("no counter service in this build")?;
    agent()
        .post(&format!("{}/{path}", url.trim_end_matches('/')))
        .send_json(body)
        .with_context(|| format!("counter /{path}"))?;
    Ok(())
}

fn update_config(dir: &Path, f: impl FnOnce(&mut Config)) {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut c = read_config(dir);
    f(&mut c);
    let _ = write_config(dir, &c);
}

/// Brings the counter up to date, in the background:
/// - with either choice on: this install's hours per language (and whether
///   it supports), at most hourly unless `force`;
/// - with "Help improve" on: the daily report;
/// - with both off: takes this install out of the count, once.
fn sync_in_background(app: AppHandle, force: bool) {
    if counter_url().is_none() {
        return;
    }
    std::thread::spawn(move || {
        let Ok(d) = dir(&app) else { return };
        let config = read_config(&d);
        if config.install_id.is_empty() {
            return;
        }
        let now = chrono::Utc::now().timestamp();
        let counted = config.improve || config.support;
        if counted && (force || now - config.last_report >= REPORT_EVERY_S) {
            let s = stats_in(&d);
            match post(
                "report",
                serde_json::json!({
                    "id": config.install_id,
                    "clips": s.clips,
                    "seconds": s.seconds_by_language,
                    "donate": config.support,
                }),
            ) {
                Ok(()) => update_config(&d, |c| {
                    c.last_report = now;
                    c.shared = true;
                }),
                Err(e) => log::warn!("Your data: counter not reached: {e:#}"),
            }
        } else if !counted && config.last_report != 0 {
            match post("leave", serde_json::json!({ "id": config.install_id })) {
                Ok(()) => update_config(&d, |c| c.last_report = 0),
                Err(e) => log::warn!("Your data: counter not reached: {e:#}"),
            }
        }
        if config.improve && now - config.last_usage >= USAGE_EVERY_S {
            let body = usage_report(&app, &d, &config);
            match post("usage", body) {
                Ok(()) => update_config(&d, |c| {
                    c.last_usage = now;
                    c.shared = true;
                }),
                Err(e) => log::warn!("Your data: daily report not sent: {e:#}"),
            }
        }
    });
}

/// "Delete what was shared": removes everything the counter holds under this
/// install's id, then starts over with a new id.
pub fn forget(app: &AppHandle) -> Result<()> {
    let d = dir(app)?;
    let config = read_config(&d);
    if !config.install_id.is_empty() {
        post("forget", serde_json::json!({ "id": config.install_id }))?;
    }
    update_config(&d, |c| {
        c.install_id = new_install_id();
        c.shared = false;
        c.last_report = 0;
        c.last_usage = 0;
    });
    // With a choice still on, the new id starts reporting again.
    sync_in_background(app.clone(), true);
    Ok(())
}

/// The answer to the current buyer message: "interested" sends the random
/// id, the language and the matching hours (nothing else); "no" stays here.
pub fn answer_buyer(app: &AppHandle, answer: &str) -> Result<()> {
    let d = dir(app)?;
    let config = read_config(&d);
    let buyer = config.buyer.clone().context("no buyer message")?;
    if answer == "interested" {
        let id = if config.install_id.is_empty() {
            new_install_id()
        } else {
            config.install_id.clone()
        };
        post(
            "interest",
            serde_json::json!({
                "id": id,
                "notice": buyer.id,
                "language": buyer.language,
                "hours": buyer.hours,
            }),
        )?;
        update_config(&d, |c| {
            c.install_id = id;
            c.shared = true;
        });
    }
    update_config(&d, |c| {
        if let Some(b) = c.buyer.as_mut() {
            b.answer = Some(answer.to_string());
        }
    });
    Ok(())
}

// ----- The daily report ("Help improve") -----

fn sysctl(name: &str) -> Option<String> {
    let out = std::process::Command::new("sysctl")
        .args(["-n", name])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// The Mac's model, chip, memory and macOS version. Never its name or
/// serial number.
fn device() -> serde_json::Value {
    let os = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    serde_json::json!({
        "os": std::env::consts::OS,
        "os_version": os,
        "arch": std::env::consts::ARCH,
        "model": sysctl("hw.model"),
        "chip": sysctl("machdep.cpu.brand_string"),
        "cores": std::thread::available_parallelism().map(|n| n.get()).ok(),
        "disk_gb": disk_gb(),
        "region": std::process::Command::new("defaults")
            .args(["read", "-g", "AppleLocale"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty()),
        "ram_gb": sysctl("hw.memsize")
            .and_then(|b| b.parse::<u64>().ok())
            .map(|b| (b as f64 / 1_073_741_824.0).round()),
    })
}

/// Size and free space of the startup disk, in GB.
fn disk_gb() -> Option<serde_json::Value> {
    let out = std::process::Command::new("df")
        .args(["-k", "/"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let cols: Vec<&str> = text.lines().nth(1)?.split_whitespace().collect();
    let gb = |kb: &str| kb.parse::<f64>().ok().map(|k| (k / 1_048_576.0).round());
    Some(serde_json::json!({ "size": gb(cols.get(1)?)?, "free": gb(cols.get(3)?)? }))
}

/// Warnings and errors from the app's log since `since` (unix seconds), at
/// most 30, cut short. Quoted words and the home folder are blanked, so no
/// names, file names or transcript words go out with them.
fn problems(app: &AppHandle, since: i64) -> Vec<String> {
    let Ok(log_dir) = crate::portable::app_log_dir(app) else {
        return vec![];
    };
    let Ok(text) = std::fs::read_to_string(log_dir.join("handy.log")) else {
        return vec![];
    };
    let since = chrono::DateTime::from_timestamp(since, 0)
        // The log's times are UTC.
        .map(|t| t.format("[%Y-%m-%d][%H:%M:%S]").to_string())
        .unwrap_or_default();
    let home = std::env::var("HOME").unwrap_or_default();
    let mut out: Vec<String> = text
        .lines()
        .filter(|l| l.contains("][WARN] ") || l.contains("][ERROR] "))
        .filter(|l| l.get(..22).is_some_and(|stamp| stamp >= since.as_str()))
        .map(|l| scrub(l.get(22..).unwrap_or(l), &home))
        .collect();
    let skip = out.len().saturating_sub(30);
    out.drain(..skip);
    out
}

fn scrub(line: &str, home: &str) -> String {
    let mut s = if home.len() > 1 {
        line.replace(home, "~")
    } else {
        line.to_string()
    };
    // Not single quotes: they are apostrophes too ("couldn't").
    for q in ['"', '„', '“', '«'] {
        let close = match q {
            '„' => '“',
            '“' => '”',
            '«' => '»',
            c => c,
        };
        let mut out = String::with_capacity(s.len());
        let mut inside = false;
        for ch in s.chars() {
            if !inside && ch == q {
                inside = true;
                out.push_str("\"…\"");
            } else if inside && ch == close {
                inside = false;
            } else if !inside {
                out.push(ch);
            }
        }
        s = out;
    }
    s.chars().take(240).collect()
}

/// What happened since the last report: dictations and meetings, as counts
/// and lengths only.
fn activity(app: &AppHandle, dir: &Path, since: i64) -> serde_json::Value {
    let mut dictations = 0u32;
    let mut dictation_seconds = 0.0;
    let mut own_meeting_seconds = 0.0;
    let mut by_language: BTreeMap<String, f64> = BTreeMap::new();
    for c in read_clips(dir).iter().filter(|c| c.created >= since) {
        if c.source == "dictation" {
            dictations += 1;
            dictation_seconds += c.seconds;
        } else {
            own_meeting_seconds += c.seconds;
        }
        *by_language
            .entry(normalize_language(c.language.as_deref()))
            .or_default() += c.seconds;
    }
    serde_json::json!({
        "dictations": dictations,
        "dictation_seconds": dictation_seconds.round(),
        "own_meeting_seconds": own_meeting_seconds.round(),
        "seconds_by_language": by_language,
        "meetings": meetings_since(app, since),
    })
}

#[cfg(target_os = "macos")]
fn meetings_since(app: &AppHandle, since: i64) -> serde_json::Value {
    use crate::meeting::store::{MeetingStatus, MeetingStore};
    use tauri::Manager;
    let Some(store) = app.try_state::<std::sync::Arc<MeetingStore>>() else {
        return serde_json::json!([]);
    };
    let mut meetings: Vec<_> = store
        .list()
        .into_iter()
        .filter(|m| {
            chrono::DateTime::parse_from_rfc3339(&m.started_at)
                .map(|t| t.timestamp() >= since)
                .unwrap_or(false)
        })
        .map(|m| {
            serde_json::json!({
                "minutes": (m.duration_s / 60.0).round(),
                "speakers": m.speakers.len(),
                "online": m.online,
                "app": m.source_app,
                "status": match m.status {
                    MeetingStatus::Recording => "recording",
                    MeetingStatus::Processing { .. } => "processing",
                    MeetingStatus::Done => "done",
                    MeetingStatus::Failed { .. } => "failed",
                },
                "warnings": m.warnings.len(),
                "started": m.started_at.get(..10),
            })
        })
        .collect();
    // The first report covers all past meetings; the newest are enough to
    // keep it small (the counter takes up to 32 KB). The list is newest first.
    meetings.truncate(MAX_REPORTED_MEETINGS);
    serde_json::Value::Array(meetings)
}

/// At most this many meetings per daily report (newest kept).
#[cfg(target_os = "macos")]
const MAX_REPORTED_MEETINGS: usize = 150;

#[cfg(not(target_os = "macos"))]
fn meetings_since(_app: &AppHandle, _since: i64) -> serde_json::Value {
    serde_json::json!([])
}

fn usage_report(app: &AppHandle, dir: &Path, config: &Config) -> serde_json::Value {
    // The first report after turning "Help improve" on covers everything
    // that happened before; later ones only the time since the last.
    let since = config.last_usage.max(0);
    let settings = crate::settings::get_settings(app);
    let s = stats_in(dir);
    serde_json::json!({
        "id": config.install_id,
        "app": {
            "version": env!("CARGO_PKG_VERSION"),
            "model": settings.selected_model,
            "language": settings.selected_language,
            "ui_language": settings.app_language,
        },
        "device": device(),
        "data": {
            "clips": s.clips,
            "seconds_by_language": s.seconds_by_language,
            "seconds_by_source": s.seconds_by_source,
        },
        "since": since,
        "activity": activity(app, dir, since),
        "problems": problems(app, since),
    })
}

// ----- Messages from Sokki -----

/// The total over all counted installs, with Sokki's current message;
/// `None` without a counter or offline.
pub fn community_total() -> Option<CommunityTotal> {
    let url = counter_url()?;
    let mut resp = agent()
        .get(&format!("{}/total", url.trim_end_matches('/')))
        .call()
        .ok()?;
    resp.body_mut().read_json().ok()
}

/// A buyer message is JSON: `{"language": "de", "text": "…"}`.
#[derive(Deserialize)]
struct BuyerNotice {
    language: String,
    #[serde(default)]
    text: Option<String>,
}

/// Looks for a new message from Sokki a minute after start and then every
/// few hours (a plain download; nothing is sent). A buyer message is matched
/// against the hours kept here and shown only if enough fits; other
/// messages are shown as they are.
pub fn start_notice_watch(app: AppHandle) {
    if counter_url().is_none() {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(60));
        // Sends what is due (and, after an update from the old choices,
        // takes this install out of the count).
        sync_in_background(app.clone(), false);
        loop {
            check_notice(&app);
            std::thread::sleep(std::time::Duration::from_secs(NOTICE_EVERY_S));
        }
    });
}

fn language_name(code: &str, german: bool) -> String {
    let (de, en) = match code {
        "de" => ("Deutsch", "German"),
        "en" => ("Englisch", "English"),
        "fr" => ("Französisch", "French"),
        "es" => ("Spanisch", "Spanish"),
        "it" => ("Italienisch", "Italian"),
        "nl" => ("Niederländisch", "Dutch"),
        _ => return code.to_uppercase(),
    };
    (if german { de } else { en }).to_string()
}

/// The notification's title and text, as on the "Your data" page.
fn buyer_message(language: &str, hours: u32, german: bool) -> (String, String) {
    let name = language_name(language, german);
    match (german, hours == 1) {
        (true, one) => (
            format!("Jemand möchte Daten auf {name}"),
            format!(
                "{hours} {} deiner anonymisierten Daten {}. Du bekämst einen Anteil am Gewinn.",
                if one { "Stunde" } else { "Stunden" },
                if one { "passt" } else { "passen" },
            ),
        ),
        (false, one) => (
            format!("Someone wants {name} data"),
            format!(
                "{hours} {} of your anonymised data {}. You'd get a share of the profit.",
                if one { "hour" } else { "hours" },
                if one { "fits" } else { "fit" },
            ),
        ),
    }
}

fn check_notice(app: &AppHandle) {
    let Ok(d) = dir(app) else { return };
    let Some(total) = community_total() else {
        return;
    };
    let Some(text) = total.notice.filter(|t| !t.trim().is_empty()) else {
        return;
    };
    let config = read_config(&d);
    if total.notice_id <= config.last_notice {
        return;
    }
    update_config(&d, |c| c.last_notice = total.notice_id);
    let german = crate::settings::get_settings(app)
        .app_language
        .starts_with("de");
    let (title, body) = match serde_json::from_str::<BuyerNotice>(&text) {
        Ok(notice) => {
            if config.support {
                return;
            }
            let lang = normalize_language(Some(&notice.language));
            let seconds = stats_in(&d)
                .seconds_by_language
                .get(&lang)
                .copied()
                .unwrap_or(0.0);
            let hours = (seconds / 3600.0).floor();
            if hours < MIN_BUYER_HOURS {
                log::info!("Your data: a buyer message didn't match this Mac's data");
                return;
            }
            let hours = hours as u32;
            update_config(&d, |c| {
                c.buyer = Some(BuyerRequest {
                    id: total.notice_id,
                    language: lang.clone(),
                    hours,
                    text: notice.text.clone(),
                    answer: None,
                })
            });
            buyer_message(&lang, hours, german)
        }
        Err(_) => (crate::brand::app_name().to_string(), text),
    };
    log::info!("Your data: new message from Sokki");
    #[cfg(target_os = "macos")]
    crate::meeting::detect::notify(&title, &body);
    #[cfg(not(target_os = "macos"))]
    let _ = (title, body);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_sum_seconds_per_language_and_source() {
        let d = tempfile::tempdir().unwrap();
        for (lang, src, secs) in [
            (Some("de"), "dictation", 4.0),
            (Some("de-DE"), "meeting", 6.0),
            (Some("en"), "meeting", 2.5),
            (None, "dictation", 1.5),
        ] {
            append(
                d.path(),
                &Clip {
                    file: "clips/x.wav".into(),
                    text: "hallo".into(),
                    language: Some(normalize_language(lang)),
                    seconds: secs,
                    source: src.into(),
                    created: 0,
                },
            )
            .unwrap();
        }
        let s = stats_in(d.path());
        assert_eq!(s.clips, 4);
        assert_eq!(s.total_seconds, 14.0);
        assert_eq!(s.seconds_by_language["de"], 10.0);
        assert_eq!(s.seconds_by_language["en"], 2.5);
        assert_eq!(s.seconds_by_language["other"], 1.5);
        assert_eq!(s.seconds_by_source["meeting"], 8.5);
        assert!(!s.improve && !s.support);
    }

    #[test]
    fn install_ids_are_random() {
        let a = new_install_id();
        assert_eq!(a.len(), 32);
        assert_ne!(a, new_install_id());
    }

    #[test]
    fn old_choices_carry_over() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join(CONFIG),
            r#"{"opt_in": true, "donate": true, "install_id": "ab", "last_report": 5}"#,
        )
        .unwrap();
        let c = read_config(d.path());
        assert!(c.support && !c.improve && c.shared);
        std::fs::write(
            d.path().join(CONFIG),
            r#"{"opt_in": true, "install_id": "ab"}"#,
        )
        .unwrap();
        let c = read_config(d.path());
        assert!(!c.support && !c.improve && c.shared);
        write_config(d.path(), &c).unwrap();
        let text = std::fs::read_to_string(d.path().join(CONFIG)).unwrap();
        assert!(!text.contains("opt_in"));
    }

    #[test]
    fn buyer_message_matches_the_page() {
        assert_eq!(
            buyer_message("de", 12, false),
            (
                "Someone wants German data".to_string(),
                "12 hours of your anonymised data fit. You'd get a share of the profit."
                    .to_string()
            )
        );
        assert_eq!(
            buyer_message("de", 1, true).0,
            "Jemand möchte Daten auf Deutsch"
        );
        assert!(buyer_message("en", 1, true)
            .1
            .starts_with("1 Stunde deiner"));
    }

    #[test]
    fn problems_lose_quotes_and_home() {
        let line = r#"[a][WARN] couldn't open "/Users/rob/Meeting mit Anna.flac" in /Users/rob/x: „Hallo“"#;
        assert_eq!(
            scrub(line, "/Users/rob"),
            r#"[a][WARN] couldn't open "…" in ~/x: "…""#
        );
    }
}

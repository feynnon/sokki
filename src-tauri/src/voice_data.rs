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
    /// "Tell me about paid recording tasks": interest only; nothing is
    /// recorded for it.
    #[serde(default)]
    tasks: bool,
    /// The counter has this install's "tasks" tick.
    #[serde(default)]
    tasks_reported: bool,
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
    /// Proof of work for `pow_id` (see [`proof_of_work`]); the counter only
    /// takes ids that carry one.
    #[serde(default)]
    pow: String,
    #[serde(default)]
    pow_id: String,
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
    /// "Tell me about paid recording tasks" is on.
    pub tasks: bool,
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
    /// "Support": recordings uploaded so far, and their length.
    pub donated_clips: u32,
    pub donated_seconds: f64,
    /// "Support": recordings kept on this Mac because their text has a name,
    /// a number or contact details.
    pub withheld_clips: u32,
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

/// Sets the three choices. Turning the first two off takes this install out of the count;
/// what was shared stays until [`forget`].
/// Paused 2026-11-19: the paid-tasks tick and buyer messages. Their code stays
/// so they can come back; while paused the app shows neither, takes back a
/// tick sent before, and ignores buyer messages.
const PAID_TASKS: bool = false;
const BUYER_MESSAGES: bool = false;

pub fn set_choices(app: &AppHandle, improve: bool, support: bool, tasks: bool) -> Result<()> {
    let d = dir(app)?;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut c = read_config(&d);
    let improve_turned_on = improve && !c.improve;
    c.improve = improve;
    c.support = support;
    c.tasks = tasks && PAID_TASKS;
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
        tasks: config.tasks,
        shared: config.shared,
        buyer: if config.support || !BUYER_MESSAGES { None } else { config.buyer },
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
    let donated = read_donated(dir);
    for c in &clips {
        match donated.get(&c.file).map(String::as_str) {
            Some("sent") => {
                s.donated_clips += 1;
                s.donated_seconds += c.seconds;
            }
            Some(_) => s.withheld_clips += 1,
            None => {}
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

/// Sends `body` to the counter, with the proof of work for its `id`.
fn post(dir: &Path, path: &str, mut body: serde_json::Value) -> Result<()> {
    let url = counter_url().context("no counter service in this build")?;
    if let Some(id) = body.get("id").and_then(|v| v.as_str()).map(str::to_owned) {
        body["pow"] = serde_json::Value::String(pow_for(dir, &id));
    }
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
                &d,
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
            match post(&d, "leave", serde_json::json!({ "id": config.install_id })) {
                Ok(()) => update_config(&d, |c| c.last_report = 0),
                Err(e) => log::warn!("Your data: counter not reached: {e:#}"),
            }
        }
        if config.improve && now - config.last_usage >= USAGE_EVERY_S {
            let body = usage_report(&app, &d, &config);
            match post(&d, "usage", body) {
                Ok(()) => update_config(&d, |c| {
                    c.last_usage = now;
                    c.shared = true;
                }),
                Err(e) => log::warn!("Your data: daily report not sent: {e:#}"),
            }
        }
        let tasks = config.tasks && PAID_TASKS;
        if tasks != config.tasks_reported {
            // Only the tick, the random id and the languages of the user's
            // own data (so tasks can be offered in them); turning it off
            // removes the tick.
            let body = serde_json::json!({
                "id": config.install_id,
                "on": tasks,
                "languages": task_languages(&app, &d),
            });
            match post(&d, "tasks", body) {
                Ok(()) => update_config(&d, |c| {
                    c.tasks_reported = tasks;
                    if tasks {
                        c.shared = true;
                    }
                }),
                Err(e) => log::warn!("Your data: paid tasks tick not sent: {e:#}"),
            }
        }
        if config.support {
            donate_pending(&app, &d);
        }
    });
}

/// The languages a paid task could be in: those of the user's own
/// recordings (most first, at most three), else the app's language.
fn task_languages(app: &AppHandle, dir: &Path) -> Vec<String> {
    let mut by: Vec<(String, f64)> = stats_in(dir)
        .seconds_by_language
        .into_iter()
        .filter(|(l, s)| l != "other" && *s >= 60.0)
        .collect();
    by.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut langs: Vec<String> = by.into_iter().take(3).map(|(l, _)| l).collect();
    if langs.is_empty() {
        let ui = crate::settings::get_settings(app).app_language;
        langs.push(normalize_language(Some(&ui)));
    }
    langs
}

/// "Delete what was shared": removes everything the counter holds under this
/// install's id, then starts over with a new id.
pub fn forget(app: &AppHandle) -> Result<()> {
    let d = dir(app)?;
    let config = read_config(&d);
    if !config.install_id.is_empty() {
        post(&d, "forget", serde_json::json!({ "id": config.install_id }))?;
    }
    update_config(&d, |c| {
        c.install_id = new_install_id();
        c.shared = false;
        c.last_report = 0;
        c.last_usage = 0;
        c.tasks_reported = false;
    });
    // The counter deleted the uploads too; a supporter's new id sends them
    // again.
    let _ = std::fs::remove_file(d.join(DONATED));
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
            &d,
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

// ----- Proof of work -----

/// Leading zero bits the counter asks for; about a second of work, once per
/// install id. The counter (`counter/src/index.js`, `POW_BITS`) must agree.
const POW_BITS: u32 = 24;

fn leading_zero_bits(hash: &[u8]) -> u32 {
    let mut bits = 0;
    for b in hash {
        if *b == 0 {
            bits += 8;
        } else {
            return bits + b.leading_zeros();
        }
    }
    bits
}

/// The first nonce (hex) for which sha256("sokki:<id>:<nonce>") starts with
/// `bits` zero bits. Hashes the fixed prefix once and only the nonce per try.
fn proof_of_work(id: &str, bits: u32) -> String {
    use sha2::{Digest, Sha256};
    let prefix = Sha256::new_with_prefix(format!("sokki:{id}:").as_bytes());
    let mut buf = [0u8; 16];
    let mut nonce: u64 = 0;
    loop {
        let hex = hex_into(nonce, &mut buf);
        let mut h = prefix.clone();
        h.update(hex);
        if leading_zero_bits(&h.finalize()) >= bits {
            return String::from_utf8_lossy(hex).into_owned();
        }
        nonce += 1;
    }
}

/// `n` in lower-case hex without leading zeros, as `format!("{n:x}")`.
fn hex_into(mut n: u64, buf: &mut [u8; 16]) -> &[u8] {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = DIGITS[(n & 0xf) as usize];
        n >>= 4;
        if n == 0 {
            return &buf[i..];
        }
    }
}

/// One proof of work at a time: syncs running side by side wait for it
/// instead of each working it out.
static POW_LOCK: Mutex<()> = Mutex::new(());

/// The proof of work for `id`, worked out once and kept in the config.
fn pow_for(dir: &Path, id: &str) -> String {
    let _g = POW_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let c = read_config(dir);
    if c.pow_id == id && !c.pow.is_empty() {
        return c.pow;
    }
    let started = std::time::Instant::now();
    let pow = proof_of_work(id, POW_BITS);
    log::info!(
        "Your data: proof of work for a new id took {:.1} s",
        started.elapsed().as_secs_f64()
    );
    update_config(dir, |c| {
        c.pow = pow.clone();
        c.pow_id = id.to_string();
    });
    pow
}

// ----- "Support Sokki & Open Source": uploading the user's own recordings -----

/// Which recordings were uploaded ("sent") or kept back ("kept:<reason>").
const DONATED: &str = "donated.jsonl";

/// One upload at a time.
static DONATING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Common first names (German and English), lower case. A recording whose
/// text holds one stays on the Mac. Together with the speaker names Sokki
/// knows, so it errs on the side of keeping too much back.
const FIRST_NAMES: &[&str] = &[
    "aaron",
    "adam",
    "adrian",
    "alex",
    "alexander",
    "alexandra",
    "ali",
    "alice",
    "alina",
    "amelie",
    "amy",
    "andi",
    "andre",
    "andrea",
    "andreas",
    "andrew",
    "angela",
    "angelika",
    "anja",
    "anke",
    "ann",
    "anna",
    "anne",
    "annika",
    "anton",
    "antonia",
    "ben",
    "benedikt",
    "benjamin",
    "bernd",
    "bettina",
    "birgit",
    "bob",
    "brian",
    "carina",
    "carl",
    "carla",
    "carolin",
    "caroline",
    "charlie",
    "charlotte",
    "chris",
    "christian",
    "christina",
    "christine",
    "christoph",
    "claudia",
    "clara",
    "daniel",
    "daniela",
    "david",
    "dennis",
    "diana",
    "dieter",
    "dirk",
    "dominik",
    "doris",
    "elena",
    "elias",
    "elisabeth",
    "ella",
    "emil",
    "emily",
    "emma",
    "eric",
    "erik",
    "eva",
    "fabian",
    "felix",
    "finn",
    "florian",
    "frank",
    "franziska",
    "frederik",
    "friedrich",
    "gabi",
    "gabriele",
    "georg",
    "george",
    "gerhard",
    "hannah",
    "hannes",
    "hans",
    "harald",
    "harry",
    "heike",
    "heinz",
    "helen",
    "helena",
    "helga",
    "helmut",
    "henrik",
    "henry",
    "ingrid",
    "isabel",
    "jack",
    "jacob",
    "jakob",
    "james",
    "jan",
    "jana",
    "jane",
    "janina",
    "jannik",
    "jasmin",
    "jason",
    "jennifer",
    "jens",
    "jessica",
    "jochen",
    "johann",
    "johanna",
    "johannes",
    "john",
    "jonas",
    "jonathan",
    "jordan",
    "josef",
    "joseph",
    "josh",
    "julia",
    "julian",
    "juliane",
    "justin",
    "jürgen",
    "karin",
    "karl",
    "kai",
    "katharina",
    "kathrin",
    "katja",
    "katrin",
    "kerstin",
    "kevin",
    "kim",
    "klaus",
    "kristina",
    "lara",
    "laura",
    "lea",
    "leah",
    "lena",
    "leni",
    "leo",
    "leon",
    "lina",
    "linda",
    "lisa",
    "lucas",
    "luca",
    "lukas",
    "luis",
    "luise",
    "lena",
    "manfred",
    "manuel",
    "manuela",
    "marc",
    "marcel",
    "marco",
    "maria",
    "marie",
    "marina",
    "mario",
    "mark",
    "markus",
    "marlene",
    "martin",
    "martina",
    "mary",
    "matthias",
    "max",
    "maximilian",
    "melanie",
    "mia",
    "michael",
    "michaela",
    "michelle",
    "mike",
    "mila",
    "miriam",
    "monika",
    "moritz",
    "nadine",
    "natalie",
    "nicole",
    "niklas",
    "nico",
    "nils",
    "nina",
    "noah",
    "nora",
    "oliver",
    "olivia",
    "oskar",
    "pascal",
    "patrick",
    "paul",
    "paula",
    "peter",
    "petra",
    "philipp",
    "rainer",
    "ralf",
    "rebecca",
    "renate",
    "richard",
    "robert",
    "robin",
    "rolf",
    "ronja",
    "rosa",
    "ruth",
    "sabine",
    "sam",
    "samuel",
    "sandra",
    "sarah",
    "sascha",
    "sebastian",
    "simon",
    "simone",
    "sofia",
    "sophia",
    "sophie",
    "stefan",
    "stefanie",
    "stephan",
    "steffen",
    "susanne",
    "sven",
    "svenja",
    "tanja",
    "theo",
    "thomas",
    "tim",
    "timo",
    "tina",
    "tobias",
    "tom",
    "torsten",
    "ulrich",
    "ulrike",
    "ursula",
    "uwe",
    "valentin",
    "vanessa",
    "vera",
    "victoria",
    "viktor",
    "walter",
    "werner",
    "william",
    "wolfgang",
    "yannick",
    "yvonne",
    "zoe",
    // Nicknames and names Apple's recogniser and dictionary let through.
    "basti",
    "benni",
    "bjarne",
    "chrissi",
    "conny",
    "dani",
    "fabi",
    "fiona",
    "flo",
    "franzi",
    "frieda",
    "greta",
    "ida",
    "jojo",
    "jonny",
    "johnny",
    "jule",
    "juli",
    "kalle",
    "kathi",
    "lasse",
    "lisi",
    "lotta",
    "lotte",
    "lou",
    "luisa",
    "malte",
    "manu",
    "marlon",
    "mats",
    "matze",
    "maxi",
    "merle",
    "micha",
    "mira",
    "nele",
    "nick",
    "nici",
    "nik",
    "norman",
    "ole",
    "olli",
    "ralle",
    "sammy",
    "sandy",
    "sepp",
    "steffi",
    "susi",
    "till",
    "tilman",
    "tobi",
    "toni",
    "uli",
    "vroni",
];

/// Spoken numbers (German and English). A recording that says one stays on
/// the Mac, like one with digits: numbers can be phone numbers, prices,
/// account numbers. Not "one" or "ein": they are mostly not numbers ("this
/// one", "ein Meeting"), and would keep back nearly everything.
const NUMBER_WORDS: &[&str] = &[
    "null",
    "eins",
    "zwei",
    "drei",
    "vier",
    "fünf",
    "sechs",
    "sieben",
    "acht",
    "neun",
    "zehn",
    "elf",
    "zwölf",
    "dreizehn",
    "vierzehn",
    "fünfzehn",
    "sechzehn",
    "siebzehn",
    "achtzehn",
    "neunzehn",
    "zwanzig",
    "dreißig",
    "vierzig",
    "fünfzig",
    "sechzig",
    "siebzig",
    "achtzig",
    "neunzig",
    "hundert",
    "tausend",
    "million",
    "millionen",
    "milliarde",
    "zero",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
    "thirty",
    "forty",
    "fifty",
    "sixty",
    "seventy",
    "eighty",
    "ninety",
    "hundred",
    "thousand",
    "billion",
];

/// Why a recording's text must stay on this Mac, if it must: "number",
/// "contact" or "name". `names` are lower case.
fn withheld_reason(
    text: &str,
    language: &str,
    names: &std::collections::HashSet<String>,
) -> Option<&'static str> {
    if text.chars().any(|c| c.is_numeric()) {
        return Some("number");
    }
    let lower = text.to_lowercase();
    if lower.contains('@') || lower.contains("http") || lower.contains("www.") {
        return Some("contact");
    }
    let mut sentence_start = true;
    let mut word = String::new();
    let mut words: Vec<(String, bool)> = Vec::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_alphabetic() || c == '\'' || c == '’' {
            word.push(c);
            continue;
        }
        if !word.is_empty() {
            words.push((std::mem::take(&mut word), sentence_start));
            sentence_start = false;
        }
        if matches!(c, '.' | '!' | '?' | '\n' | ':') {
            sentence_start = true;
        }
    }
    for (w, at_start) in &words {
        let l = w.to_lowercase();
        let base = l.split(['\'', '’']).next().unwrap_or(&l);
        if NUMBER_WORDS.contains(&base) {
            return Some("number");
        }
        if names.contains(base) || FIRST_NAMES.contains(&base) {
            return Some("name");
        }
        // Abbreviations in capitals are mostly company, product or customer
        // names ("EWTO", "MCP").
        if w.chars().filter(|c| c.is_alphabetic()).count() >= 2
            && w.chars()
                .filter(|c| c.is_alphabetic())
                .all(char::is_uppercase)
        {
            return Some("name");
        }
        // English writes only names (and "I") with a capital inside a
        // sentence; German capitalises every noun, so this check is English only.
        if language == "en"
            && !at_start
            && w.chars().next().is_some_and(char::is_uppercase)
            && base != "i"
        {
            return Some("name");
        }
    }
    apple_name_check(text, language)
}

/// macOS's own language tools: Apple's name recogniser (people, places,
/// organisations) and the spell checker, for capitalised words its
/// dictionary doesn't know (company and product names, rarer names).
#[cfg(target_os = "macos")]
#[allow(deprecated)] // NSLinguisticTagger: works, and needs no extra crate.
fn apple_name_check(text: &str, language: &str) -> Option<&'static str> {
    use objc2::AnyThread;
    use objc2_app_kit::NSSpellChecker;
    use objc2_foundation::{
        NSArray, NSLinguisticTagOrganizationName, NSLinguisticTagPersonalName,
        NSLinguisticTagPlaceName, NSLinguisticTagSchemeNameType, NSLinguisticTagger,
        NSLinguisticTaggerUnit, NSNotFound, NSRange, NSString,
    };
    objc2::rc::autoreleasepool(|_| {
        let s = NSString::from_str(text);
        let len = s.length();
        let scheme = unsafe { NSLinguisticTagSchemeNameType };
        let schemes = NSArray::from_slice(&[scheme]);
        let tagger = unsafe {
            NSLinguisticTagger::initWithTagSchemes_options(NSLinguisticTagger::alloc(), &schemes, 0)
        };
        tagger.setString(Some(&s));
        let named = unsafe {
            [
                NSLinguisticTagPersonalName,
                NSLinguisticTagPlaceName,
                NSLinguisticTagOrganizationName,
            ]
        };
        let mut i = 0;
        while i < len {
            let mut r = NSRange::new(0, 0);
            let tag = unsafe {
                tagger.tagAtIndex_unit_scheme_tokenRange(
                    i,
                    NSLinguisticTaggerUnit::Word,
                    scheme,
                    &mut r,
                )
            };
            if tag.is_some_and(|t| named.iter().any(|n| &*t == *n)) {
                return Some("name");
            }
            i = if r.length > 0 {
                r.location + r.length
            } else {
                i + 1
            };
        }
        let checker = NSSpellChecker::sharedSpellChecker();
        let lang = match language {
            "de" | "en" => Some(NSString::from_str(language)),
            _ => None,
        };
        let mut start: isize = 0;
        while (start as usize) < len {
            let r = unsafe {
                checker.checkSpellingOfString_startingAt_language_wrap_inSpellDocumentWithTag_wordCount(
                    &s,
                    start,
                    lang.as_deref(),
                    false,
                    0,
                    std::ptr::null_mut(),
                )
            };
            if r.location == NSNotFound as usize || r.length == 0 {
                break;
            }
            let word = s.substringWithRange(r).to_string();
            if word.chars().next().is_some_and(char::is_uppercase) {
                return Some("name");
            }
            start = (r.location + r.length) as isize;
        }
        None
    })
}

#[cfg(not(target_os = "macos"))]
fn apple_name_check(_text: &str, _language: &str) -> Option<&'static str> {
    None
}

/// Names Sokki knows: remembered voices, meeting speakers, the user's name.
fn known_names(app: &AppHandle) -> std::collections::HashSet<String> {
    let mut raw: Vec<String> = Vec::new();
    if let Ok(dir) = crate::portable::app_data_dir(app) {
        if let Ok(text) = std::fs::read_to_string(dir.join("voice-profiles.json")) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                for p in v["profiles"].as_array().into_iter().flatten() {
                    if let Some(n) = p["name"].as_str() {
                        raw.push(n.to_string());
                    }
                }
            }
        }
    }
    raw.push(crate::settings::get_settings(app).meetings.user_name);
    #[cfg(target_os = "macos")]
    {
        use tauri::Manager;
        if let Some(store) = app.try_state::<std::sync::Arc<crate::meeting::store::MeetingStore>>()
        {
            for m in store.list() {
                raw.extend(m.speakers);
            }
        }
    }
    // Placeholders like "Sprecher 2", "Speaker 1" or "Ich" are not names.
    const NOT_NAMES: &[&str] = &[
        "sprecher", "speaker", "person", "ich", "me", "i", "du", "you",
    ];
    raw.iter()
        .flat_map(|n| n.split_whitespace())
        .map(|w| w.trim_matches(|c: char| !c.is_alphabetic()).to_lowercase())
        .filter(|w| w.chars().count() >= 2 && !NOT_NAMES.contains(&w.as_str()))
        .collect()
}

fn read_donated(dir: &Path) -> std::collections::HashMap<String, String> {
    std::fs::read_to_string(dir.join(DONATED))
        .map(|t| {
            t.lines()
                .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
                .filter_map(|v| {
                    Some((
                        v["file"].as_str()?.to_string(),
                        v["status"].as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn mark_donated(dir: &Path, file: &str, status: &str) {
    let line = serde_json::json!({ "file": file, "status": status }).to_string();
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(DONATED))
        .and_then(|mut f| writeln!(f, "{line}"));
}

/// A kept recording as FLAC (lossless, about half the size of WAV).
fn flac_bytes(wav: &Path) -> Result<Vec<u8>> {
    let mut reader = hound::WavReader::open(wav)?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1_i64 << (spec.bits_per_sample.max(1) - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<_, _>>()?
        }
    };
    // Mono: the clips are, but be safe.
    let mono: Vec<f32> = samples
        .chunks(channels)
        .map(|c| c.iter().sum::<f32>() / c.len() as f32)
        .collect();
    let path = std::env::temp_dir().join(format!(
        "sokki-donate-{}-{:04x}.flac",
        std::process::id(),
        rand_u16()
    ));
    let mut w = crate::meeting::flac::FlacWriter::create(&path, spec.sample_rate)?;
    w.write_interleaved(&mono, 1)?;
    w.finish()?;
    let bytes = std::fs::read(&path);
    let _ = std::fs::remove_file(&path);
    Ok(bytes?)
}

/// Uploads the recordings not sent yet, one after another, while "Support"
/// stays on. Recordings whose text has a name, a number or contact details
/// stay on this Mac (marked, so they aren't checked again).
fn donate_pending(app: &AppHandle, dir: &Path) {
    use std::sync::atomic::Ordering;
    if DONATING.swap(true, Ordering::SeqCst) {
        return;
    }
    let names = known_names(app);
    let done = read_donated(dir);
    let (mut sent, mut kept) = (0, 0);
    for clip in read_clips(dir) {
        if done.contains_key(&clip.file) {
            continue;
        }
        let config = read_config(dir);
        if !config.support || config.install_id.is_empty() {
            break;
        }
        let language = normalize_language(clip.language.as_deref());
        if let Some(reason) = withheld_reason(&clip.text, &language, &names) {
            mark_donated(dir, &clip.file, &format!("kept:{reason}"));
            kept += 1;
            continue;
        }
        let audio = match flac_bytes(&dir.join(&clip.file)) {
            Ok(a) => a,
            Err(e) => {
                log::warn!("Your data: {} not readable: {e:#}", clip.file);
                mark_donated(dir, &clip.file, "kept:unreadable");
                continue;
            }
        };
        let stem = clip
            .file
            .trim_start_matches("clips/")
            .trim_end_matches(".wav")
            .to_string();
        use base64::Engine;
        let body = serde_json::json!({
            "id": config.install_id,
            "clip": stem,
            "language": language,
            "source": clip.source,
            "seconds": clip.seconds,
            "text": clip.text,
            "audio": base64::engine::general_purpose::STANDARD.encode(&audio),
        });
        match post(dir, "donate", body) {
            Ok(()) => {
                mark_donated(dir, &clip.file, "sent");
                sent += 1;
            }
            Err(e) => {
                // Offline, rate limited or the store is full: try again at
                // the next sync.
                log::warn!("Your data: upload stopped: {e:#}");
                break;
            }
        }
        // Stays under the counter's limit of uploads per minute.
        std::thread::sleep(std::time::Duration::from_millis(600));
    }
    if sent > 0 {
        update_config(dir, |c| c.shared = true);
    }
    if sent + kept > 0 {
        log::info!("Your data: uploaded {sent} recordings, kept {kept} back");
    }
    DONATING.store(false, Ordering::SeqCst);
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
            // The app's own version (Sokki 0.2.0), not the crate's (Handy 0.10.1).
            "version": app.package_info().version.to_string(),
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
            if config.support || !BUYER_MESSAGES {
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
    fn names_numbers_and_contacts_stay_on_the_mac() {
        let names: std::collections::HashSet<String> = ["anouk".to_string()].into();
        let r = |t: &str, l: &str| withheld_reason(t, l, &names);
        assert_eq!(r("Ich rufe dich morgen an.", "de"), None);
        assert_eq!(r("Das kostet 30 Euro.", "de"), Some("number"));
        assert_eq!(r("Das kostet dreißig Euro.", "de"), Some("number"));
        assert_eq!(r("Schreib an a@b.de", "de"), Some("contact"));
        assert_eq!(r("Frag mal Anouk danach.", "de"), Some("name"));
        assert_eq!(r("Ich habe mit Thomas gesprochen.", "de"), Some("name"));
        // German nouns are capitalised; that alone isn't a name.
        assert_eq!(r("Das Meeting war gut.", "de"), None);
        assert_eq!(r("Sounds good. I think so.", "en"), None);
        assert_eq!(r("Then we ask Kowalski about it.", "en"), Some("name"));
        // What got through the first version, from real meetings.
        assert_eq!(r("Frag mal Fiona und Maxi.", "de"), Some("name"));
        assert_eq!(r("Das läuft über EWTO.", "de"), Some("name"));
        assert_eq!(r("Kalasar baut die Seite.", "de"), Some("name"));
        assert_eq!(r("Die Kunden von Lumicare warten.", "de"), Some("name"));
        assert_eq!(r("Dann hat Norman gesagt, es passt.", "de"), Some("name"));
        assert_eq!(r("Ich glaube, die Idee ist gut.", "de"), None);
    }

    #[test]
    fn proof_of_work_meets_the_bits() {
        use sha2::{Digest, Sha256};
        let id = "0123456789abcdef0123456789abcdef";
        let nonce = proof_of_work(id, 12);
        let hash = Sha256::digest(format!("sokki:{id}:{nonce}").as_bytes());
        assert!(leading_zero_bits(&hash) >= 12);
        assert_eq!(leading_zero_bits(&[0, 0b0001_0000]), 11);
        let mut buf = [0u8; 16];
        for n in [0u64, 9, 10, 255, 4096, u64::MAX] {
            assert_eq!(hex_into(n, &mut buf), format!("{n:x}").as_bytes());
        }
    }

    #[test]
    #[ignore] // Timing only: cargo test --release -- --ignored pow_timing
    fn pow_timing() {
        let t = std::time::Instant::now();
        for i in 0..5 {
            proof_of_work(&format!("{i:032x}"), POW_BITS);
        }
        println!(
            "POW_TIMING average {:.2} s",
            t.elapsed().as_secs_f64() / 5.0
        );
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

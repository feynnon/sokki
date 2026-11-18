//! "Your voice": an opt-in, local dataset of the user's own voice with its
//! transcript, for a possible future sale through Verbatim.
//!
//! Only the user's own voice is kept: dictations, and in meetings only the
//! microphone segments labelled with the user's name (on calls, where the
//! microphone is the user). Clips stay on this Mac in
//! `<app data>/voice-data/` (`clips/*.wav` and `manifest.jsonl`), separate
//! from the recordings' retention. Nothing is uploaded. When a counter
//! service is configured at build time (`VERBATIM_COUNTER_URL`), an opted-in
//! install reports only a random id and its hours per language.

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

/// The counter service, set at build time; `None`: counting stays local.
pub fn counter_url() -> Option<&'static str> {
    option_env!("VERBATIM_COUNTER_URL").filter(|u| !u.is_empty())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Config {
    opt_in: bool,
    /// Random, created on first opt-in; identifies the install to the
    /// counter only (no name, no machine data).
    install_id: String,
    /// Unix seconds of the last report that reached the counter.
    #[serde(default)]
    last_report: i64,
}

/// New clips are reported at most this often; opting in or out, and
/// deleting, are reported right away.
const REPORT_EVERY_S: i64 = 3600;

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

/// What the "Your voice" page shows.
#[derive(Debug, Clone, Default, Serialize, specta::Type)]
pub struct VoiceDataStats {
    pub opt_in: bool,
    pub clips: u32,
    pub total_seconds: f64,
    /// Language code ("de", "en", "other") → seconds.
    pub seconds_by_language: BTreeMap<String, f64>,
    /// Seconds per source ("dictation", "meeting").
    pub seconds_by_source: BTreeMap<String, f64>,
    pub bytes: u64,
    pub folder: String,
}

/// The total over all opted-in installs, from the counter service.
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct CommunityTotal {
    pub installs: u32,
    pub total_seconds: f64,
    pub seconds_by_language: BTreeMap<String, f64>,
}

fn dir(app: &AppHandle) -> Result<PathBuf> {
    Ok(crate::portable::app_data_dir(app)?.join(DIR))
}

fn read_config(dir: &Path) -> Config {
    std::fs::read_to_string(dir.join(CONFIG))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write_config(dir: &Path, c: &Config) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(CONFIG), serde_json::to_string_pretty(c)?)?;
    Ok(())
}

pub fn is_opted_in(app: &AppHandle) -> bool {
    dir(app).map(|d| read_config(&d).opt_in).unwrap_or(false)
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

/// Turns collecting on or off. Off keeps the clips (delete them with
/// [`delete_all`]) and removes this install from the counter.
pub fn set_opt_in(app: &AppHandle, on: bool) -> Result<()> {
    let d = dir(app)?;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut c = read_config(&d);
    c.opt_in = on;
    if c.install_id.is_empty() {
        c.install_id = new_install_id();
    }
    write_config(&d, &c)?;
    drop(_g);
    report_in_background(app.clone(), true);
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
    if !is_opted_in(app) || text.trim().is_empty() {
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
        Ok(()) => report_in_background(app.clone(), false),
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
    if !is_opted_in(app) || segments.is_empty() {
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
            report_in_background(app.clone(), false);
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
    let mut s = VoiceDataStats {
        opt_in: read_config(dir).opt_in,
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

/// The page's numbers. Opening the page also sends clips kept since the
/// last report, once the hour has passed.
pub fn stats(app: &AppHandle) -> Result<VoiceDataStats> {
    report_in_background(app.clone(), false);
    Ok(stats_in(&dir(app)?))
}

/// Deletes every kept clip and the manifest; the opt-in choice stays.
pub fn delete_all(app: &AppHandle) -> Result<()> {
    let d = dir(app)?;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = std::fs::remove_dir_all(d.join("clips"));
    let _ = std::fs::remove_file(d.join(MANIFEST));
    drop(_g);
    report_in_background(app.clone(), true);
    Ok(())
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(10)))
        .build()
        .into()
}

/// Sends this install's numbers (or its removal) to the counter, if one is
/// configured. Only a random id and seconds per language leave the Mac.
/// `force`: send now; otherwise at most once per [`REPORT_EVERY_S`].
fn report_in_background(app: AppHandle, force: bool) {
    let Some(url) = counter_url() else { return };
    std::thread::spawn(move || {
        let Ok(d) = dir(&app) else { return };
        let config = read_config(&d);
        if config.install_id.is_empty() {
            return;
        }
        let now = chrono::Utc::now().timestamp();
        if !force && (!config.opt_in || now - config.last_report < REPORT_EVERY_S) {
            return;
        }
        let base = url.trim_end_matches('/');
        let res = if config.opt_in {
            let s = stats_in(&d);
            agent()
                .post(&format!("{base}/report"))
                .send_json(serde_json::json!({
                    "id": config.install_id,
                    "clips": s.clips,
                    "seconds": s.seconds_by_language,
                }))
        } else {
            agent()
                .post(&format!("{base}/forget"))
                .send_json(serde_json::json!({ "id": config.install_id }))
        };
        match res {
            Ok(_) => {
                let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
                let mut c = read_config(&d);
                c.last_report = now;
                let _ = write_config(&d, &c);
            }
            Err(e) => log::warn!("Voice data: counter not reached: {e}"),
        }
    });
}

/// The total over all opted-in installs; `None` without a counter or offline.
pub fn community_total() -> Option<CommunityTotal> {
    let url = counter_url()?;
    let mut resp = agent()
        .get(&format!("{}/total", url.trim_end_matches('/')))
        .call()
        .ok()?;
    resp.body_mut().read_json().ok()
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
        assert!(!s.opt_in);
    }

    #[test]
    fn install_ids_are_random() {
        let a = new_install_id();
        assert_eq!(a.len(), 32);
        assert_ne!(a, new_install_id());
    }
}

//! Test mode, so Claude can test dictation end to end without a person
//! speaking: with `SOKKI_TEST_MIC=<WAV>` set, the microphone plays that file
//! in real time from the start of each recording (then silence), the text
//! is logged instead of pasted, and there are no shortcuts and no permission
//! resets. `SOKKI_TEST_FORGET=1` instead runs "Delete what was shared" with
//! sharing on (`voice_data::test_forget`). `SOKKI_TEST_LLM=1` also starts
//! the built-in language model once and logs its answer. Run it only in
//! the separate test app (`scripts/test-dictation.sh`), never in Robin's
//! Sokki.
//!
//! A meeting instead of a dictation (`scripts/test-meeting.sh`): with
//! `SOKKI_TEST_MEETING=1` the app records one meeting, the call side played
//! from `SOKKI_TEST_SYSTEM=<WAV>` in place of the system audio, and logs how
//! long processing took. No recording pill, no notification, no voice data.
//! `SOKKI_TEST_NO_LIVE=1` turns off transcribing during the recording, for
//! comparing. `SOKKI_TEST_SECOND_S=<s>` records a second meeting right after
//! the first, while the first is processed (`SOKKI_TEST_GAP_S` between them),
//! and logs how fast each one was recording; `SOKKI_TEST_NO_YIELD=1` lets
//! processing compete with that recording, as before 2026-11-23, for
//! comparing ([`crate::meeting::priority`]).

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use sokki_recorder::testing::FakeMic;
use sokki_recorder::{Error, Recorder, RecorderConfig, Sink};
use log::{info, warn};

const RATE: u32 = 16_000;

fn path() -> Option<PathBuf> {
    std::env::var_os("SOKKI_TEST_MIC")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
}

/// Whether this app runs in test mode.
pub fn active() -> bool {
    static ACTIVE: OnceLock<bool> = OnceLock::new();
    *ACTIVE.get_or_init(|| path().is_some())
}

/// A meeting is tested instead of a dictation.
pub fn meeting() -> bool {
    active() && std::env::var_os("SOKKI_TEST_MEETING").is_some_and(|v| !v.is_empty())
}

/// The call side of a test meeting, played in place of the system audio.
pub fn system_audio() -> Option<Vec<f32>> {
    if !meeting() {
        return None;
    }
    let path = PathBuf::from(std::env::var_os("SOKKI_TEST_SYSTEM").filter(|p| !p.is_empty())?);
    Some(load(&path))
}

/// Transcribing during the recording is off for this test meeting.
pub fn no_live() -> bool {
    active() && std::env::var_os("SOKKI_TEST_NO_LIVE").is_some_and(|v| !v.is_empty())
}

/// Processing does not yield to a recording in this test (the old way).
pub fn no_yield() -> bool {
    active() && std::env::var_os("SOKKI_TEST_NO_YIELD").is_some_and(|v| !v.is_empty())
}

/// The file's audio, 16 kHz mono (other rates are resampled crudely by
/// nearest sample; tests should pass 16 kHz).
fn load(path: &PathBuf) -> Vec<f32> {
    let Ok(mut reader) = hound::WavReader::open(path) else {
        warn!("Test mode: can't read {}", path.display());
        return Vec::new();
    };
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(Result::ok).collect(),
        hound::SampleFormat::Int => {
            let scale = (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .filter_map(Result::ok)
                .map(|s| s as f32 / scale)
                .collect()
        }
    };
    let mono: Vec<f32> = samples
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();
    if spec.sample_rate == RATE {
        return mono;
    }
    let ratio = spec.sample_rate as f64 / RATE as f64;
    (0..(mono.len() as f64 / ratio) as usize)
        .map(|i| mono[((i as f64 * ratio) as usize).min(mono.len() - 1)])
        .collect()
}

/// The fake microphone, and a thread that plays the file into each new
/// recording in real time.
fn mic(block: usize) -> &'static FakeMic {
    static MIC: OnceLock<FakeMic> = OnceLock::new();
    MIC.get_or_init(|| {
        let mic = FakeMic::new(RATE, 1);
        let path = path().unwrap_or_default();
        let audio = load(&path);
        info!(
            "Test mode: the microphone plays {} ({:.1}s), nothing is pasted",
            path.display(),
            audio.len() as f64 / RATE as f64
        );
        let player = mic.clone();
        std::thread::Builder::new()
            .name("test-mic".into())
            .spawn(move || play(&player, &audio, block.max(1)))
            .ok();
        mic
    })
}

fn play(mic: &FakeMic, audio: &[f32], block: usize) {
    let silence = vec![0.0f32; block];
    loop {
        // Wait for the next recording's stream; it plays until that ends.
        while !mic.is_streaming() {
            std::thread::sleep(Duration::from_millis(2));
        }
        info!("Test mode: playing the file into the recording");
        let started = Instant::now();
        let mut sent = 0usize;
        loop {
            let piece = match audio.get(sent..(sent + block).min(audio.len())) {
                Some(p) if !p.is_empty() => p,
                _ => &silence,
            };
            if !mic.push(piece) {
                break;
            }
            sent += piece.len();
            let due = started + Duration::from_secs_f64(sent as f64 / RATE as f64);
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            }
        }
    }
}

/// Opens the fake microphone in place of the real one.
pub fn open<S: Sink>(
    config: RecorderConfig,
    handler: Box<dyn FnOnce(Error) + Send + 'static>,
) -> Result<Recorder<S>, Error> {
    let block = config.frames_per_chunk.unwrap_or(512);
    mic(block).open(config, Some(handler), Duration::from_secs(2))
}

/// Starts one dictation a few seconds after launch and stops it 1 s after
/// the file ends (or after `SOKKI_TEST_HOLD_S`), like the shortcut would, so
/// nothing outside the app has to press anything.
pub fn drive(app: &tauri::AppHandle) {
    if !active() {
        return;
    }
    let length = path()
        .and_then(|p| hound::WavReader::open(p).ok())
        .map(|r| r.duration() as f64 / r.spec().sample_rate.max(1) as f64)
        .unwrap_or(0.0);
    let hold = std::env::var("SOKKI_TEST_HOLD_S")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(length + 1.0);
    if std::env::var_os("SOKKI_TEST_LLM").is_some() {
        std::thread::spawn(test_builtin_llm);
    }
    let app = app.clone();
    if std::env::var_os("SOKKI_TEST_FORGET").is_some() {
        std::thread::spawn(move || crate::voice_data::test_forget(&app));
        return;
    }
    if meeting() {
        std::thread::spawn(move || drive_meeting(&app, hold));
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(3));
        info!("Test mode: dictation starts ({hold:.1}s)");
        crate::signal_handle::send_transcription_input(&app, "transcribe", "test");
        std::thread::sleep(Duration::from_secs_f64(hold));
        crate::signal_handle::send_transcription_input(&app, "transcribe", "test");
    });
}

/// A second meeting right after the first, while the first is processed
/// (`SOKKI_TEST_SECOND_S`: its length in s; `SOKKI_TEST_GAP_S`: seconds
/// between the stop and the second start, default 2).
fn second_meeting() -> Option<(f64, f64)> {
    let secs = |name: &str| std::env::var(name).ok().and_then(|s| s.parse::<f64>().ok());
    let length = secs("SOKKI_TEST_SECOND_S").filter(|_| meeting())?;
    Some((length, secs("SOKKI_TEST_GAP_S").unwrap_or(2.0)))
}

/// Starts a meeting the way the window's button does and logs how long
/// until it is recording (what the window and the pill show) and until the
/// microphone's first audio arrived.
fn start_timed(app: &tauri::AppHandle, which: &str) -> Option<String> {
    use std::sync::atomic::Ordering;
    let busy = crate::meeting::process::busy();
    let click_us = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0);
    let click = Instant::now();
    let id = match crate::meeting::start_meeting(app, Some("Test".into())) {
        Ok(id) => id,
        Err(e) => {
            warn!("Test mode: the {which} meeting didn't start: {e}");
            return None;
        }
    };
    let recording_ms = click.elapsed().as_secs_f64() * 1000.0;
    // The fake microphone's first block lands within a block or two.
    let mut first_us = 0;
    while click.elapsed() < Duration::from_secs(5) {
        first_us = crate::meeting::MIC_FIRST_US.load(Ordering::Relaxed);
        if first_us != 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let audio_ms = if first_us == 0 {
        f64::NAN
    } else {
        // Its first sample, not its arrival: can be a little before the
        // start returned.
        (first_us as f64 - click_us as f64) / 1000.0
    };
    info!(
        "Test mode: {which} meeting recording {recording_ms:.1} ms after the click, first microphone sample at {audio_ms:.1} ms ({})",
        if busy { "an earlier meeting is processing" } else { "nothing processing" }
    );
    Some(id)
}

/// Waits until meeting `id` is done or failed and logs it.
fn wait_processed(app: &tauri::AppHandle, id: &str, which: &str, stopped: Instant) {
    use crate::meeting::store::{MeetingStatus, MeetingStore};
    use tauri::Manager;
    let store = app.state::<std::sync::Arc<MeetingStore>>().inner().clone();
    loop {
        std::thread::sleep(Duration::from_millis(200));
        match store.get(id).map(|r| r.status) {
            Some(MeetingStatus::Done) => {
                info!(
                    "Test mode: {which} meeting done {:.1}s after its stop",
                    stopped.elapsed().as_secs_f64()
                );
                return;
            }
            Some(MeetingStatus::Failed { error }) => {
                warn!("Test mode: the {which} meeting failed: {error}");
                return;
            }
            _ => {}
        }
    }
}

/// Logs the meeting's title as the list shows it.
fn log_title(app: &tauri::AppHandle, id: &str, when: &str) {
    use crate::meeting::store::MeetingStore;
    use tauri::Manager;
    if let Some(r) = app.state::<std::sync::Arc<MeetingStore>>().get(id) {
        info!(
            "Test mode: Meeting title {when}: {:?} (provisional: {})",
            r.title, r.title_provisional
        );
    }
}

/// Records one meeting for `hold` seconds, then logs how long processing
/// took once the note is written. With [`second_meeting`], a second one
/// starts while the first is processed, as on 2026-11-23.
fn drive_meeting(app: &tauri::AppHandle, hold: f64) {
    use crate::meeting::store::{MeetingStatus, MeetingStore};
    use tauri::Manager;
    std::thread::sleep(Duration::from_secs(3));
    info!(
        "Test mode: meeting starts ({hold:.1}s, live transcript {})",
        if no_live() { "off" } else { "on" }
    );
    info!(
        "Test mode: dates follow {:?}",
        crate::commands::meeting::date_format_locale()
    );
    let Some(id) = start_timed(app, "first") else {
        return;
    };
    std::thread::sleep(Duration::from_secs_f64(hold));
    log_title(app, &id, "before the stop");
    let stopped = Instant::now();
    if let Err(e) = crate::meeting::stop_meeting(app) {
        warn!("Test mode: the meeting didn't stop: {e}");
        return;
    }
    if let Some((length, gap)) = second_meeting() {
        std::thread::sleep(Duration::from_secs_f64(gap));
        let Some(second) = start_timed(app, "second") else {
            return;
        };
        std::thread::sleep(Duration::from_secs_f64(length));
        let second_stopped = Instant::now();
        if let Err(e) = crate::meeting::stop_meeting(app) {
            warn!("Test mode: the second meeting didn't stop: {e}");
            return;
        }
        wait_processed(app, &id, "first", stopped);
        wait_processed(app, &second, "second", second_stopped);
        info!("Test mode: meeting processed");
        return;
    }
    let store = app.state::<std::sync::Arc<MeetingStore>>().inner().clone();
    loop {
        std::thread::sleep(Duration::from_millis(200));
        match store.get(&id).map(|r| r.status) {
            Some(MeetingStatus::Done) => {
                log_title(app, &id, "when done");
                info!(
                    "Test mode: meeting processed {:.1}s after the stop",
                    stopped.elapsed().as_secs_f64()
                );
                return;
            }
            Some(MeetingStatus::Failed { error }) => {
                warn!("Test mode: the meeting failed: {error}");
                return;
            }
            _ => {}
        }
    }
}

/// With `SOKKI_TEST_LLM` set: starts the built-in language model the way a
/// summary does (the server bundled with this app, the model in its data
/// folder), asks it one short question, stops it, and logs each step.
fn test_builtin_llm() {
    let settings = sokki_engine::llm::LlmSettings {
        provider: sokki_engine::llm::BUILTIN.into(),
        ..Default::default()
    };
    let t0 = Instant::now();
    let model = match sokki_engine::llm::open(&settings, "Ich") {
        Ok(m) => m,
        Err(e) => {
            warn!("Test mode: built-in language model did not start: {e:#}");
            return;
        }
    };
    let llm = meeting_core::summary::LlmSummarizer::new(model.config());
    match llm.chat_plain("Answer with the single word OK.", "Ping") {
        Ok(reply) => info!(
            "Test mode: built-in language model answered {reply:?} after {:.1}s",
            t0.elapsed().as_secs_f64()
        ),
        Err(e) => warn!("Test mode: built-in language model failed: {e:#}"),
    }
    drop(model);
    info!("Test mode: built-in language model done");
}

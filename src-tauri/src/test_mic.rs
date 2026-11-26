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
//! comparing ([`crate::meeting::priority`]). `SOKKI_TEST_CALL=<app id>`
//! ("zoom", "chrome", …) treats the test meeting as a call in that app:
//! the chat notice is posted into its real meeting chat and watched for
//! "stop" (`meeting::chat_notice`); banners are logged, not shown.
//! `SOKKI_TEST_SYSTEM_DEAD_AT_S=<s>` makes the played system audio go dead
//! (exact zeros) then, until the tap's watchdog rebuilds it;
//! `SOKKI_TEST_OUTPUT_CHANGE_AT_S=<s>` changes the default output then
//! (`meeting::system_tap`, `FakeTap`).

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use log::{info, warn};
use sokki_recorder::testing::FakeMic;
use sokki_recorder::{Error, Recorder, RecorderConfig, Sink};

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

/// The call app a test meeting pretends to record (`SOKKI_TEST_CALL`).
pub fn call() -> Option<String> {
    if !active() {
        return None;
    }
    std::env::var("SOKKI_TEST_CALL")
        .ok()
        .filter(|s| !s.is_empty())
}

/// Transcribing during the recording is off for this test meeting.
pub fn no_live() -> bool {
    active() && std::env::var_os("SOKKI_TEST_NO_LIVE").is_some_and(|v| !v.is_empty())
}

/// Processing does not yield to a recording in this test (the old way).
pub fn no_yield() -> bool {
    active() && std::env::var_os("SOKKI_TEST_NO_YIELD").is_some_and(|v| !v.is_empty())
}

/// A WAV file's audio, 16 kHz mono, for other test hooks; `None` when empty.
pub fn load_wav(path: &std::path::Path) -> Option<Vec<f32>> {
    let audio = load(&path.to_path_buf());
    (!audio.is_empty()).then_some(audio)
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
    // `SOKKI_TEST_MIC_CONTINUE=1`: the next recording goes on where the last
    // one stopped instead of starting the file again, like a room that keeps
    // talking (the auto-meeting listener hands over to the meeting).
    let resume = std::env::var_os("SOKKI_TEST_MIC_CONTINUE").is_some_and(|v| !v.is_empty());
    let mut at = 0usize;
    loop {
        // Wait for the next recording's stream; it plays until that ends.
        while !mic.is_streaming() {
            std::thread::sleep(Duration::from_millis(2));
        }
        info!(
            "Test mode: playing the file into the recording from {:.1}s",
            if resume { at as f64 / RATE as f64 } else { 0.0 }
        );
        let started = Instant::now();
        let first = if resume { at } else { 0 };
        let mut sent = first;
        loop {
            let piece = match audio.get(sent..(sent + block).min(audio.len())) {
                Some(p) if !p.is_empty() => p,
                _ => &silence,
            };
            if !mic.push(piece) {
                at = sent;
                break;
            }
            sent += piece.len();
            let due = started + Duration::from_secs_f64((sent - first) as f64 / RATE as f64);
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
    if std::env::var_os("SOKKI_TEST_SAMPLE").is_some() {
        std::thread::spawn(move || drive_sample(&app));
        return;
    }
    if std::env::var_os("SOKKI_TEST_AUTO").is_some_and(|v| !v.is_empty()) {
        // `scripts/test-auto-meeting.sh`: nothing is pressed; the
        // auto-meeting listener (`meeting::auto`) decides by itself.
        info!("Test mode: auto meeting, the listener decides");
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

/// `SOKKI_TEST_OWN_NOTES="12=Budget?|40=Druckerei bis Fr"`: types these
/// lines into the meeting's notes, each at its second into the recording,
/// the way the notes field saves them.
fn type_own_notes(app: &tauri::AppHandle, id: &str) {
    use crate::meeting::store::MeetingStore;
    use tauri::Manager;
    let Ok(spec) = std::env::var("SOKKI_TEST_OWN_NOTES") else {
        return;
    };
    let lines: Vec<(f64, String)> = spec
        .split('|')
        .filter_map(|p| {
            let (t, text) = p.split_once('=')?;
            Some((t.trim().parse().ok()?, text.to_string()))
        })
        .collect();
    let store = app.state::<std::sync::Arc<MeetingStore>>().inner().clone();
    let id = id.to_string();
    std::thread::spawn(move || {
        let started = Instant::now();
        let mut text = String::new();
        for (at, line) in lines {
            let wait = at - started.elapsed().as_secs_f64();
            if wait > 0.0 {
                std::thread::sleep(Duration::from_secs_f64(wait));
            }
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&line);
            let Some(record) = store.get(&id) else { return };
            match crate::meeting::own_notes::save(&store, &record, &text) {
                Ok(()) => info!("Test mode: own note typed at {at:.0}s"),
                Err(e) => warn!("Test mode: own note not saved: {e:#}"),
            }
        }
    });
}

/// Logs the finished note's "Meine Notizen" section (made-up test lines).
fn log_own_notes(store: &crate::meeting::store::MeetingStore, id: &str) {
    if std::env::var_os("SOKKI_TEST_OWN_NOTES").is_none() {
        return;
    }
    let raw = crate::meeting::own_notes::read(store, id);
    info!(
        "Test mode: own notes file: {}",
        raw.iter()
            .map(|l| format!("{:?}@{:?}", l.text, l.t))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let Some(path) = store.get(id).and_then(|r| r.note_path) else {
        warn!("Test mode: own notes: no note");
        return;
    };
    let md = std::fs::read_to_string(&path).unwrap_or_default();
    let section = meeting_core::own_notes::section_of(&md);
    let added = section.lines().filter(|l| l.starts_with("  - ")).count();
    info!(
        "Test mode: own notes section: {} lines, {added} additions, front matter id {}: {}",
        section.lines().filter(|l| l.starts_with("- ")).count(),
        md.contains(&format!("\nid: {id}\n")),
        section.replace('\n', " ⏎ ")
    );
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
            None => {
                info!("Test mode: no speech, the {which} meeting was removed");
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
    type_own_notes(app, &id);
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
                log_own_notes(&store, &id);
                #[cfg(target_os = "macos")]
                test_voices(app, &id);
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
            // Only a meeting without speech leaves the list by itself.
            None => {
                info!(
                    "Test mode: no speech, the meeting was removed {:.1}s after the stop",
                    stopped.elapsed().as_secs_f64()
                );
                return;
            }
            _ => {}
        }
    }
}

/// Speaker names (board card speaker-names), after a test meeting: logs
/// its speakers, what Sokki proposes, a clip and the voices across the test
/// meetings. `SOKKI_TEST_NAME_SPEAKER="Speaker 2=Lena Vogt <lena@example.com>"`
/// (several with ";") then names them as a click would
/// (`SOKKI_TEST_CONSENT=1`: with the person's consent) and logs the speakers
/// again; `SOKKI_TEST_MAIL_DRY=1` then presses "Send to everyone" without
/// sending.
#[cfg(target_os = "macos")]
fn test_voices(app: &tauri::AppHandle, id: &str) {
    use crate::meeting::{store::MeetingStore, voices};
    use tauri::Manager;
    let store = app.state::<std::sync::Arc<MeetingStore>>().inner().clone();
    let speakers = || store.get(id).map(|r| r.speakers).unwrap_or_default();
    info!("Test mode: speakers {:?}", speakers());
    for p in voices::speaker_proposals(app, id) {
        let names: Vec<&str> = p.candidates.iter().map(|c| c.name.as_str()).collect();
        info!("Test mode: proposal for {}: {names:?}", p.speaker);
    }
    if let Some(sp) = speakers().into_iter().last() {
        match voices::clip(app, id, &sp, None) {
            Ok(url) => info!("Test mode: clip of {sp}: {} bytes", url.len()),
            Err(e) => warn!("Test mode: no clip of {sp}: {e:#}"),
        }
    }
    // Several with ";", each with an optional address:
    // "Speaker 2=Lena Vogt <lena@example.com>;Speaker 3=Jonas Weber".
    let naming = std::env::var("SOKKI_TEST_NAME_SPEAKER").unwrap_or_default();
    for one in naming.split(';').filter(|s| !s.trim().is_empty()) {
        let Some((from, to)) = one.split_once('=') else {
            continue;
        };
        let (from, to) = (from.trim().to_string(), to.trim());
        let (to, emails) = match to.split_once('<') {
            Some((n, e)) => (
                n.trim().to_string(),
                vec![e.trim_end_matches('>').trim().to_string()],
            ),
            None => (to.to_string(), Vec::new()),
        };
        let consent = std::env::var_os("SOKKI_TEST_CONSENT").is_some();
        let member = voices::VoiceMember {
            meeting_id: id.to_string(),
            speaker: from.clone(),
        };
        match voices::name_voice(app, &[member], &to, &emails, consent) {
            Ok(n) => info!(
                "Test mode: named {from} {to:?} (consent {consent}), {n} change(s); speakers {:?}",
                speakers()
            ),
            Err(e) => warn!("Test mode: naming {from} failed: {e:#}"),
        }
    }
    // "Send to everyone" after naming, as a click would; with
    // SOKKI_TEST_MAIL_DRY the send only logs who would get the notes.
    if std::env::var_os("SOKKI_TEST_MAIL_DRY").is_some() {
        match crate::meeting::mail::send(app, id, false) {
            Ok(m) => info!("Test mode: send button mailed {:?}", m.to),
            Err(e) => info!("Test mode: send button: {e}"),
        }
    }
    // The question at the end: whose voice Sokki may keep.
    let offer = voices::voice_offer(app, id);
    info!("Test mode: voice offer {offer:?}");
    if let Ok(keep) = std::env::var("SOKKI_TEST_KEEP_VOICES") {
        for name in offer
            .iter()
            .filter(|n| keep == "all" || keep.split(',').any(|k| k == *n))
        {
            let member = voices::VoiceMember {
                meeting_id: id.to_string(),
                speaker: name.clone(),
            };
            match voices::name_voice(app, &[member], name, &[], true) {
                Ok(_) => info!("Test mode: kept the voice of {name:?}"),
                Err(e) => warn!("Test mode: keeping {name:?} failed: {e:#}"),
            }
        }
        if let Err(e) = voices::voice_offer_done(app, id) {
            warn!("Test mode: answering the voice offer failed: {e:#}");
        }
        info!(
            "Test mode: voice offer after answering {:?}",
            voices::voice_offer(app, id)
        );
    }
    if let Some(note) = store.get(id).and_then(|r| r.note_path) {
        let md = std::fs::read_to_string(note).unwrap_or_default();
        let lines: Vec<&str> = md
            .lines()
            .filter(|l| l.starts_with("**["))
            .take(4)
            .collect();
        info!("Test mode: transcript starts {lines:?}");
    }
    let page = voices::page(app);
    for v in &page.voices {
        info!(
            "Test mode: voice {:?} in {} meeting(s), {} unnamed, remembered {}, suggestion {:?}, {} line(s)",
            v.name,
            v.meetings,
            v.unnamed,
            v.remembered,
            v.suggestion.as_ref().map(|s| (&s.name, s.invited, s.of)),
            v.lines.len()
        );
    }
    info!("Test mode: {} voice(s) heard once", page.once);
}

/// With `SOKKI_TEST_SAMPLE=1`: plays the sample meeting as its button does
/// (`meeting::sample`, silently in test mode), logs the live transcript as
/// it grows, and how long after the call's end the note was done. Logs
/// "Test mode: sample processed" or "Test mode: the sample failed".
fn drive_sample(app: &tauri::AppHandle) {
    use crate::meeting::store::{MeetingStatus, MeetingStore};
    use tauri::Manager;
    std::thread::sleep(Duration::from_secs(3));
    let started = Instant::now();
    let id = match crate::meeting::sample::start(app) {
        Ok(id) => id,
        Err(e) => {
            warn!("Test mode: the sample failed: {e}");
            return;
        }
    };
    info!("Test mode: sample {id} started");
    let meetings = app.state::<std::sync::Arc<crate::meeting::MeetingManager>>();
    let store = app.state::<std::sync::Arc<MeetingStore>>().inner().clone();
    let mut heard = 0;
    let mut stopped: Option<Instant> = None;
    loop {
        std::thread::sleep(Duration::from_millis(250));
        if let Some(lines) = meetings.heard().map(|h| h.lines()) {
            for (at, text) in lines.iter().skip(heard) {
                info!(
                    "Test mode: sample live at {:.1}s: [{at:.1}] {text}",
                    started.elapsed().as_secs_f64()
                );
            }
            heard = heard.max(lines.len());
        }
        let Some(record) = store.get(&id) else {
            warn!("Test mode: the sample failed: its record is gone");
            return;
        };
        if stopped.is_none() && !matches!(record.status, MeetingStatus::Recording) {
            stopped = Some(Instant::now());
            info!(
                "Test mode: sample stopped itself after {:.1}s",
                started.elapsed().as_secs_f64()
            );
        }
        match record.status {
            MeetingStatus::Done => {
                info!(
                    "Test mode: sample processed {:.1}s after the call ended; title {:?}, speakers {:?}, sample {}, warnings {:?}, note {:?}",
                    stopped.map_or(0.0, |s| s.elapsed().as_secs_f64()),
                    record.title,
                    record.speakers,
                    record.sample,
                    record.warnings,
                    record.note_path
                );
                return;
            }
            MeetingStatus::Failed { error } => {
                warn!("Test mode: the sample failed: {error}");
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

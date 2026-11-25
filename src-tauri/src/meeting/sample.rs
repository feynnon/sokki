//! The sample meeting: a two-minute call between three invented people
//! (`resources/sample-meeting/call.flac`, built by
//! `testdata/sample_meeting/build.py`), for someone who has no meeting
//! today. It runs as a real meeting through the real pipeline: the call
//! plays in place of the system audio, in real time and out loud; the
//! microphone is a silent stand-in, so nothing of the room is recorded and
//! no permission is needed. The live transcript shows while it plays; at
//! the end the meeting stops itself and is processed like any other.
//!
//! A sample is marked (`MeetingRecord::sample`): it adds nothing to "Your
//! data" or the hours milestones, never posts a chat notice, and a real
//! meeting started meanwhile replaces it at once.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use sokki_recorder::testing::FakeMic;
use sokki_recorder::{Error, Recorder, RecorderConfig, Sink};
use tauri::{AppHandle, Manager};

const RATE: u32 = 16_000;

/// The meeting row's place ("24.11. · 10:00 · Sample · 2 Min.").
pub const SOURCE_APP: &str = "Sample";

/// The call, inside the app bundle.
const RESOURCE: &str = "resources/sample-meeting/call.flac";

/// The running sample: its meeting id, the call side not yet taken by the
/// system-audio capture, and the switch that ends its playback.
struct Running {
    id: Option<String>,
    call: Option<Vec<f32>>,
    stop: Arc<AtomicBool>,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);

fn running() -> std::sync::MutexGuard<'static, Option<Running>> {
    RUNNING.lock().unwrap_or_else(|e| e.into_inner())
}

/// A sample is being set up or recorded.
pub fn active() -> bool {
    running().is_some()
}

/// The meeting id of the sample being recorded.
pub fn active_id() -> Option<String> {
    running().as_ref().and_then(|r| r.id.clone())
}

/// The call side, once, for the system-audio capture of the sample.
pub fn take_call() -> Option<Vec<f32>> {
    running().as_mut().and_then(|r| r.call.take())
}

/// Ends the playback; the recording itself is stopped by its caller.
pub fn end() {
    if let Some(r) = running().take() {
        r.stop.store(true, Ordering::Release);
    }
}

fn resource_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .resolve(RESOURCE, tauri::path::BaseDirectory::Resource)
        .ok()
        .filter(|p| p.exists())
}

/// The silent stand-in microphone: silence in real time while recording.
fn mic(block: usize) -> &'static FakeMic {
    static MIC: OnceLock<FakeMic> = OnceLock::new();
    MIC.get_or_init(|| {
        let mic = FakeMic::new(RATE, 1);
        let player = mic.clone();
        std::thread::Builder::new()
            .name("sample-mic".into())
            .spawn(move || silence(&player, block.max(1)))
            .ok();
        mic
    })
}

/// A quiet room: noise about 100 dB below full scale, far under what voice
/// detection hears. Not exact zeros, which the recorder logs as a dead or
/// muted microphone.
fn silence(mic: &FakeMic, block: usize) {
    let mut seed: u32 = 0x9E37_79B9;
    let quiet: Vec<f32> = (0..block)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5) * 2e-5
        })
        .collect();
    loop {
        while !mic.is_streaming() {
            std::thread::sleep(Duration::from_millis(5));
        }
        let started = Instant::now();
        let mut sent = 0usize;
        while mic.push(&quiet) {
            sent += block;
            let due = started + Duration::from_secs_f64(sent as f64 / RATE as f64);
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            }
        }
    }
}

/// Opens the silent microphone in place of the real one.
pub fn open_mic<S: Sink>(
    config: RecorderConfig,
    handler: Box<dyn FnOnce(Error) + Send + 'static>,
) -> Result<Recorder<S>, Error> {
    let block = config.frames_per_chunk.unwrap_or(512);
    mic(block).open(config, Some(handler), Duration::from_secs(2))
}

/// Starts the sample meeting. Refused while a meeting records.
pub fn start(app: &AppHandle) -> Result<String, String> {
    let meetings = app.state::<Arc<super::MeetingManager>>();
    if meetings.is_recording() {
        return Err("A meeting is being recorded.".into());
    }
    let path = resource_path(app).ok_or("This build of Sokki has no sample meeting.")?;
    let call = crate::audio_toolkit::decode_audio_file(&path).map_err(|e| format!("{e:#}"))?;
    let length = call.len() as f64 / RATE as f64;
    let stop = Arc::new(AtomicBool::new(false));
    {
        let mut slot = running();
        if slot.is_some() {
            return Err("The sample meeting is already running.".into());
        }
        *slot = Some(Running {
            id: None,
            call: Some(call.clone()),
            stop: Arc::clone(&stop),
        });
    }
    let id = match super::start_meeting_as(app, Some(SOURCE_APP.into()), true) {
        Ok(id) => id,
        Err(e) => {
            end();
            return Err(e);
        }
    };
    if let Some(r) = running().as_mut() {
        r.id = Some(id.clone());
        // Not taken: the capture had no use for it.
        r.call = None;
    }
    log::info!("Sample meeting {id} started ({length:.1} s)");
    let player = std::thread::Builder::new().name("sample-play".into());
    let out_loud = call;
    let stopping = Arc::clone(&stop);
    let _ = player.spawn(move || play_out_loud(out_loud, &stopping));
    let app = app.clone();
    let watched = id.clone();
    let _ = std::thread::Builder::new()
        .name("sample-stop".into())
        .spawn(move || {
            let until = Instant::now() + Duration::from_secs_f64(length + 0.8);
            while Instant::now() < until {
                if stop.load(Ordering::Acquire) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if active_id().as_deref() == Some(watched.as_str()) {
                end();
                if let Err(e) = super::stop_meeting(&app) {
                    log::warn!("Sample meeting {watched}: stopping failed: {e}");
                }
            }
        });
    Ok(id)
}

/// Plays the call through the speakers, so the person hears what Sokki
/// hears. Silent when there's no output device.
fn play_out_loud(call: Vec<f32>, stop: &AtomicBool) {
    // The test app never makes a sound on Robin's Mac.
    if crate::test_mic::active() {
        return;
    }
    let sink = match rodio::DeviceSinkBuilder::from_default_device().and_then(|b| b.open_stream()) {
        Ok(s) => s,
        Err(e) => {
            log::info!("Sample meeting plays silently: {e}");
            return;
        }
    };
    let source = rodio::buffer::SamplesBuffer::new(
        std::num::NonZero::new(1).unwrap(),
        std::num::NonZero::new(RATE).unwrap(),
        call,
    );
    let player = rodio::Player::connect_new(sink.mixer());
    player.append(source);
    while !player.empty() {
        if stop.load(Ordering::Acquire) {
            player.stop();
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

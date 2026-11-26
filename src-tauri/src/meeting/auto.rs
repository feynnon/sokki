//! Meetings that start and stop by themselves when people talk in the room
//! (board card `auto-meeting`). Off by default for everyone
//! (`meetings.auto_start_enabled`, no switch in the window yet): Robin
//! uninstalled Wispr Flow over a bad auto start, so this one is measured on
//! his Mac first. Research and the reasoning behind each number:
//! `docs/research/auto-meeting-2026-11.md`.
//!
//! While the setting is on and nothing records, a listener keeps the
//! microphone open (macOS shows its orange dot) and runs the voice detector
//! dictation uses on it. Audio stays in memory, at most [`PREROLL_S`]; it is
//! never written anywhere unless a meeting starts, and then it becomes the
//! meeting's first seconds, so the start of the conversation is in the note.
//!
//! A meeting starts when all of these hold:
//! - at least [`MIN_SPEECH_S`] of speech in the last [`WINDOW_S`],
//! - a known voice in it: at least [`MIN_KNOWN_PIECES`] speech pieces match
//!   a voice Sokki knows (the user's own or a person who agreed, from
//!   `voice-profiles.json`; speaker embeddings, the model meetings use). The
//!   user alone counts too (Robin, 2026-11-26: "It should also record when
//!   only I am speaking"); a video, a podcast or a TV with strangers' voices
//!   doesn't. No known voice yet: nothing starts by itself.
//! - no call app on the microphone (call detection asks for those, as
//!   before), no dictation running, and no app playing sound (a video or
//!   music on the speakers reaches the microphone; with headphones it
//!   doesn't, and nothing plays then).
//!
//! It stops by itself after [`QUIET_STOP_S`] without speech. Both happen
//! without asking; a small card shows for [`CARD_S`]: "Discard" (one click
//! deletes the recording, audio and record, so nothing is learned from it;
//! then no auto start for [`COOLDOWN_S`]), "Keep recording", and "Turn off
//! auto start" (sets `auto_start_enabled` to false; the recording goes on).
//!
//! Every decision is logged with its reason and timing ("Auto meeting: …"),
//! never with what was said, so false and missed starts can be counted.

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sokki_recorder::{AudioChunk, Recorder, RecorderConfig, Sink};
use tauri::{AppHandle, Manager};

use crate::audio_toolkit::vad::EARSHOT_FRAME_SAMPLES;
use crate::audio_toolkit::{EarshotVad, VoiceActivityDetector};

use super::{detect::DetectState, MeetingManager};

const RATE: usize = 16_000;
/// The look-back window the start is decided on.
pub const WINDOW_S: f64 = 30.0;
/// Speech needed in that window: a conversation, not a remark.
pub const MIN_SPEECH_S: f64 = 12.0;
/// Audio kept in memory, which becomes the start of a started meeting.
pub const PREROLL_S: f64 = 35.0;
/// Speech pieces compared with the known voices.
const PIECE_S: f64 = 3.0;
/// A piece is a known voice at this cosine similarity to one of its
/// embeddings. Voice profiles use 0.6 on long audio; a 3 s piece scatters
/// more. Tuned on made-up voices (research doc), to be checked on Robin's.
pub const KNOWN_VOICE: f32 = 0.5;
/// Pieces that must match a known voice (one could be chance).
pub const MIN_KNOWN_PIECES: usize = 2;
/// After a rejected candidate (no known voice, sound playing, call), wait this
/// long before checking again, so the embedder doesn't run every second.
const RECHECK_S: f64 = 15.0;
/// An auto-started meeting stops after this long without speech.
pub const QUIET_STOP_S: f64 = 120.0;
/// How long the "Discard" card stays.
pub const CARD_S: u64 = 20;
/// After a discard, no auto start for this long.
pub const COOLDOWN_S: u64 = 15 * 60;
/// The same threshold dictation uses for Earshot.
const VAD_THRESHOLD: f32 = 0.5;
/// Frames per second of the voice detector (16 ms each).
const FPS: f64 = RATE as f64 / EARSHOT_FRAME_SAMPLES as f64;

/// Audio the listener heard before a meeting it started: written first into
/// the meeting's microphone file ([`super::mic::MicCapture::start`]).
static PREROLL: Mutex<Option<Vec<f32>>> = Mutex::new(None);

/// Takes the pre-roll for the microphone file that is opening now.
pub fn take_preroll() -> Option<Vec<f32>> {
    PREROLL.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// What the listener knows about the meeting it started.
struct Ours {
    id: String,
    started: Instant,
    started_mono_us: u64,
}

static OURS: Mutex<Option<Ours>> = Mutex::new(None);
static COOLDOWN_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

/// Whether meeting `id` was started by the listener (it stops by itself).
pub fn is_auto(id: &str) -> bool {
    OURS.lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|o| o.id == id)
}

// ---------------------------------------------------------------- decision

/// Speech flags of the last [`WINDOW_S`] (one per 16 ms frame) and the
/// last [`PREROLL_S`] of audio.
pub struct Window {
    flags: VecDeque<bool>,
    speech_frames: usize,
    audio: VecDeque<f32>,
}

impl Default for Window {
    fn default() -> Self {
        Self::new()
    }
}

impl Window {
    pub fn new() -> Self {
        Self {
            flags: VecDeque::with_capacity((WINDOW_S * FPS) as usize + 1),
            speech_frames: 0,
            audio: VecDeque::with_capacity((PREROLL_S * RATE as f64) as usize + RATE),
        }
    }

    /// One 16 ms frame and whether it was speech.
    pub fn push(&mut self, frame: &[f32], speech: bool) {
        self.flags.push_back(speech);
        self.speech_frames += speech as usize;
        while self.flags.len() as f64 > WINDOW_S * FPS {
            if self.flags.pop_front() == Some(true) {
                self.speech_frames -= 1;
            }
        }
        self.audio.extend(frame.iter().copied());
        let cap = (PREROLL_S * RATE as f64) as usize;
        while self.audio.len() > cap {
            self.audio.pop_front();
        }
    }

    /// Seconds of speech in the window.
    pub fn speech_s(&self) -> f64 {
        self.speech_frames as f64 / FPS
    }

    pub fn clear(&mut self) {
        self.flags.clear();
        self.speech_frames = 0;
        self.audio.clear();
    }

    /// The speech of the window, cut into [`PIECE_S`] pieces of 16 kHz audio.
    pub fn speech_pieces(&self) -> Vec<Vec<f32>> {
        let frame = EARSHOT_FRAME_SAMPLES;
        // The flags cover the window's end of the audio.
        let audio_frames = self.audio.len() / frame;
        let skip = audio_frames.saturating_sub(self.flags.len());
        let offset = self.audio.len() - (audio_frames - skip) * frame;
        let piece_len = (PIECE_S * RATE as f64) as usize;
        let mut pieces = Vec::new();
        let mut current = Vec::with_capacity(piece_len);
        let flags = self
            .flags
            .iter()
            .skip(self.flags.len().saturating_sub(audio_frames - skip));
        for (i, &speech) in flags.enumerate() {
            if !speech {
                continue;
            }
            let start = offset + i * frame;
            current.extend(
                self.audio
                    .range(start..(start + frame).min(self.audio.len())),
            );
            if current.len() >= piece_len {
                pieces.push(std::mem::replace(
                    &mut current,
                    Vec::with_capacity(piece_len),
                ));
            }
        }
        pieces
    }

    pub fn preroll(&self) -> Vec<f32> {
        self.audio.iter().copied().collect()
    }
}

/// How many of `pieces` (L2-normalised embeddings) match one of `known`
/// at `threshold`, and the best similarity seen.
pub fn known_matches(pieces: &[Vec<f32>], known: &[Vec<f32>], threshold: f32) -> (usize, f32) {
    let mut best = f32::NEG_INFINITY;
    let mut matched = 0;
    for p in pieces {
        let top = known
            .iter()
            .map(|k| diarize::cosine(k, p))
            .fold(f32::NEG_INFINITY, f32::max);
        best = best.max(top);
        matched += (top >= threshold) as usize;
    }
    (matched, best)
}

/// Embeddings of every known voice: the user's own and those of people
/// who agreed (only those have embeddings). Test mode:
/// `SOKKI_TEST_AUTO_KNOWN=<WAV>` is the only known voice.
fn known_voices(app: &AppHandle, embedder: &diarize::SpeakerEmbedder) -> Vec<Vec<f32>> {
    if crate::test_mic::active() {
        return std::env::var_os("SOKKI_TEST_AUTO_KNOWN")
            .and_then(|p| crate::test_mic::load_wav(std::path::Path::new(&p)))
            .and_then(|audio| embedder.embed(&audio).ok())
            .into_iter()
            .collect();
    }
    super::process::voice_profiles(app)
        .map(|p| {
            p.profiles()
                .iter()
                .flat_map(|v| v.embeddings.iter().cloned())
                .collect()
        })
        .unwrap_or_default()
}

/// Why a candidate did not start (logged).
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Call(String),
    Dictation,
    Playing(Vec<String>),
    Cooldown(u64),
}

// ---------------------------------------------------------------- listener

struct ListenSink {
    tx: SyncSender<Vec<f32>>,
}

impl Sink for ListenSink {
    fn process_chunk(&mut self, chunk: AudioChunk<'_>) {
        let real = chunk.valid_frames * chunk.channels as usize;
        let samples = &chunk.samples[..real];
        let mono: Vec<f32> = if chunk.channels <= 1 {
            samples.to_vec()
        } else {
            samples
                .chunks(chunk.channels as usize)
                .map(|f| f.iter().sum::<f32>() / f.len() as f32)
                .collect()
        };
        // Dropped when the listener falls behind; it's only for detection.
        let _ = self.tx.try_send(mono);
    }
}

fn enabled(app: &AppHandle) -> bool {
    crate::settings::get_settings(app)
        .meetings
        .auto_start_enabled
}

/// Starts the listener thread (once). It follows the setting by itself.
pub fn spawn(app: &AppHandle) {
    static STARTED: std::sync::Once = std::sync::Once::new();
    let app = app.clone();
    STARTED.call_once(move || {
        let _ = std::thread::Builder::new()
            .name("auto-meeting".into())
            .spawn(move || run(&app));
    });
}

/// The open microphone and its chunks.
struct Listening {
    recorder: Recorder<ListenSink>,
    rx: Receiver<Vec<f32>>,
    opened: Instant,
}

fn open_listener() -> Result<Listening, String> {
    let (tx, rx) = mpsc::sync_channel(256);
    let mut config = RecorderConfig::speech();
    let on_failure = |err: sokki_recorder::Error| {
        log::warn!("Auto meeting: the microphone failed while listening: {err}");
    };
    let recorder = if crate::test_mic::active() {
        config.device = None;
        crate::test_mic::open(config, Box::new(on_failure)).map_err(|e| e.to_string())?
    } else {
        // The built-in microphone where there is one: opening a Bluetooth
        // headset's microphone would drop its sound to call quality.
        let devices = sokki_recorder::list_input_devices().unwrap_or_default();
        config.device = devices
            .iter()
            .find(|d| d.name.contains("MacBook") || d.name.contains("Built-in"))
            .or_else(|| devices.iter().find(|d| d.is_default))
            .map(|d| d.id.clone());
        Recorder::<ListenSink>::open_with_failure_handler(config, on_failure)
            .map_err(|e| e.to_string())?
    };
    recorder
        .start(ListenSink { tx })
        .map_err(|e| e.error.to_string())?;
    Ok(Listening {
        recorder,
        rx,
        opened: Instant::now(),
    })
}

fn close_listener(l: Listening) {
    let _ = l.recorder.stop();
    let _ = l.recorder.close();
}

fn run(app: &AppHandle) {
    let mut listening: Option<Listening> = None;
    let mut window = Window::new();
    let mut vad: Option<EarshotVad> = None;
    let mut frame: Vec<f32> = Vec::with_capacity(EARSHOT_FRAME_SAMPLES);
    let mut next_check = Instant::now();
    let mut embedder: Option<diarize::SpeakerEmbedder> = None;
    let mut embedder_failed = false;
    let mut was_enabled = false;
    loop {
        let on = enabled(app);
        if on != was_enabled {
            log::info!("Auto meeting: {}", if on { "on" } else { "off" });
            was_enabled = on;
        }
        let recording = app.state::<Arc<MeetingManager>>().active_id();
        if let Some(id) = &recording {
            if let Some(l) = listening.take() {
                close_listener(l);
            }
            window.clear();
            watch_ours(app, id);
            std::thread::sleep(Duration::from_millis(500));
            continue;
        }
        forget_ours_if_gone();
        if !on {
            if let Some(l) = listening.take() {
                close_listener(l);
                log::info!("Auto meeting: stopped listening");
            }
            embedder = None;
            window.clear();
            std::thread::sleep(Duration::from_secs(1));
            continue;
        }
        if listening.is_none() {
            match open_listener() {
                Ok(l) => {
                    log::info!("Auto meeting: listening");
                    listening = Some(l);
                    vad = EarshotVad::new(VAD_THRESHOLD).ok();
                    frame.clear();
                    window.clear();
                }
                Err(e) => {
                    log::warn!("Auto meeting: can't open the microphone: {e}");
                    std::thread::sleep(Duration::from_secs(30));
                    continue;
                }
            }
        }
        let Some(l) = listening.as_ref() else {
            continue;
        };
        match l.rx.recv_timeout(Duration::from_millis(500)) {
            Ok(samples) => {
                let Some(detector) = vad.as_mut() else {
                    continue;
                };
                for s in samples {
                    frame.push(s);
                    if frame.len() < EARSHOT_FRAME_SAMPLES {
                        continue;
                    }
                    let speech = detector
                        .push_frame(&frame)
                        .map(|f| f.is_speech())
                        .unwrap_or(false);
                    window.push(&frame, speech);
                    frame.clear();
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                log::warn!("Auto meeting: the microphone stopped delivering; reopening");
                if let Some(l) = listening.take() {
                    close_listener(l);
                }
                continue;
            }
        }
        if Instant::now() < next_check || window.speech_s() < MIN_SPEECH_S {
            continue;
        }
        // A candidate: enough speech. Checks from cheapest to dearest.
        let candidate_at = Instant::now();
        let speech_s = window.speech_s();
        let listened_s = l.opened.elapsed().as_secs_f64();
        if let Some(block) = blocked(app) {
            log::info!(
                "Auto meeting: not started, {} (speech {speech_s:.1} s in {WINDOW_S:.0} s)",
                describe(&block)
            );
            next_check = Instant::now() + Duration::from_secs_f64(RECHECK_S);
            continue;
        }
        if embedder.is_none() && !embedder_failed {
            let mut config = diarize::DiarizeConfig::new(super::process::models_dir(app));
            config.allow_download = false;
            config.num_threads = 1;
            match diarize::SpeakerEmbedder::new(&config) {
                Ok(e) => embedder = Some(e),
                Err(e) => {
                    log::warn!("Auto meeting: no speaker model ({e:#}); nothing starts by itself");
                    embedder_failed = true;
                }
            }
        }
        let pieces = window.speech_pieces();
        let (known, matched, best) = match embedder.as_ref() {
            Some(emb) => {
                let known = known_voices(app, emb);
                let embeddings: Vec<Vec<f32>> =
                    pieces.iter().filter_map(|p| emb.embed(p).ok()).collect();
                let (matched, best) = known_matches(&embeddings, &known, KNOWN_VOICE);
                (known.len(), matched, best)
            }
            None => (0, 0, f32::NAN),
        };
        let check_ms = candidate_at.elapsed().as_millis();
        if matched < MIN_KNOWN_PIECES {
            log::info!(
                "Auto meeting: not started, no known voice (speech {speech_s:.1} s in {WINDOW_S:.0} s, {matched} of {} pieces known, best similarity {best:.2}, {known} known voice samples, checked in {check_ms} ms)",
                pieces.len()
            );
            next_check = Instant::now() + Duration::from_secs_f64(RECHECK_S);
            continue;
        }
        // Start. The listener lets go of the microphone first.
        let preroll = window.preroll();
        let preroll_s = preroll.len() as f64 / RATE as f64;
        if let Some(l) = listening.take() {
            close_listener(l);
        }
        window.clear();
        *PREROLL.lock().unwrap_or_else(|e| e.into_inner()) = Some(preroll);
        let begun = Instant::now();
        match super::start_meeting(app, None) {
            Ok(id) => {
                log::info!(
                    "Auto meeting: started {id} (speech {speech_s:.1} s in {WINDOW_S:.0} s, {matched} of {} pieces a known voice, best similarity {best:.2}, checked in {check_ms} ms, recording {} ms later, {preroll_s:.1} s from before the start kept, listening for {listened_s:.0} s)",
                    pieces.len(),
                    begun.elapsed().as_millis()
                );
                *OURS.lock().unwrap_or_else(|e| e.into_inner()) = Some(Ours {
                    id: id.clone(),
                    started: Instant::now(),
                    started_mono_us: super::mono_us(),
                });
                crate::usage::count(app, crate::usage::Count::AutoMeetingStarted);
                offer_discard(app, id);
            }
            Err(e) => log::warn!("Auto meeting: starting failed: {e}"),
        }
        take_preroll();
        next_check = Instant::now() + Duration::from_secs_f64(RECHECK_S);
    }
}

fn describe(b: &Block) -> String {
    match b {
        Block::Call(app) => format!("a call runs in {app} (call detection asks)"),
        Block::Dictation => "a dictation runs".into(),
        Block::Playing(apps) => format!("sound plays from {}", apps.join(", ")),
        Block::Cooldown(s) => format!("discarded {s} s ago"),
    }
}

fn blocked(app: &AppHandle) -> Option<Block> {
    if let Some(until) = *COOLDOWN_UNTIL.lock().unwrap_or_else(|e| e.into_inner()) {
        if let Some(left) = until.checked_duration_since(Instant::now()) {
            return Some(Block::Cooldown(COOLDOWN_S - left.as_secs().min(COOLDOWN_S)));
        }
    }
    if let Some(call) = app
        .try_state::<Arc<DetectState>>()
        .and_then(|s| s.current_call())
        .or_else(crate::test_mic::call)
    {
        return Some(Block::Call(call));
    }
    if app
        .state::<Arc<crate::managers::audio::AudioRecordingManager>>()
        .is_recording()
    {
        return Some(Block::Dictation);
    }
    let playing = playing_apps();
    if !playing.is_empty() {
        return Some(Block::Playing(playing));
    }
    None
}

/// Apps playing sound now, Sokki left out. Test mode:
/// `SOKKI_TEST_AUTO_PLAYING=<name>` pretends that app plays.
fn playing_apps() -> Vec<String> {
    if crate::test_mic::active() {
        return std::env::var("SOKKI_TEST_AUTO_PLAYING")
            .ok()
            .filter(|s| !s.is_empty())
            .into_iter()
            .collect();
    }
    let me = std::process::id();
    let mut names: Vec<String> = meeting_detect::audio_output_clients()
        .unwrap_or_default()
        .into_iter()
        .filter(|c| c.pid != me && c.responsible_pid != Some(me))
        .map(|c| c.name)
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The card after a start: "Discard" deletes the recording, "Turn off auto
/// start" switches the setting off. Test mode shows no window;
/// `SOKKI_TEST_AUTO_DISCARD=1` clicks Discard after 3 s,
/// `SOKKI_TEST_AUTO_DISCARD=off` clicks "Turn off auto start".
fn offer_discard(app: &AppHandle, id: String) {
    let handle = app.clone();
    std::thread::spawn(move || {
        let shown = Instant::now();
        let answer = if crate::test_mic::active() {
            std::env::var("SOKKI_TEST_AUTO_DISCARD").ok().map(|v| {
                std::thread::sleep(Duration::from_secs(3));
                if v == "off" { "off" } else { "discard" }.to_string()
            })
        } else {
            super::prompt::ask(
                &handle,
                super::prompt::Kind::AutoStarted,
                "",
                Duration::from_secs(CARD_S),
            )
        };
        if answer.as_deref() == Some("off") {
            let mut settings = crate::settings::get_settings(&handle);
            settings.meetings.auto_start_enabled = false;
            crate::settings::write_settings(&handle, settings);
            log::info!(
                "Auto meeting: turned off by the user from the card {:.1} s after starting {id}; the recording goes on",
                shown.elapsed().as_secs_f64()
            );
            crate::usage::count(&handle, crate::usage::Count::AutoMeetingTurnedOff);
            return;
        }
        if answer.as_deref() != Some("discard") {
            if answer.as_deref() == Some("keep") {
                log::info!("Auto meeting: {id} kept by the user");
            }
            return;
        }
        let still =
            handle.state::<Arc<MeetingManager>>().active_id().as_deref() == Some(id.as_str());
        if !still {
            return;
        }
        // Forgotten and cooled down first, so the listener neither logs it
        // as stopped by the user nor starts again in between.
        *OURS.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *COOLDOWN_UNTIL.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(Instant::now() + Duration::from_secs(COOLDOWN_S));
        match super::discard_meeting(&handle) {
            Ok(_) => {
                log::info!(
                    "Auto meeting: {id} discarded by the user {:.1} s after the start; no auto start for {} min",
                    shown.elapsed().as_secs_f64(),
                    COOLDOWN_S / 60
                );
                crate::usage::count(&handle, crate::usage::Count::AutoMeetingDiscarded);
            }
            Err(e) => log::warn!("Auto meeting: discarding {id} failed: {e}"),
        }
    });
}

/// While meeting `id` records: if the listener started it, stop it after
/// [`QUIET_STOP_S`] without speech on either track.
fn watch_ours(app: &AppHandle, id: &str) {
    let (started, started_mono_us) = {
        let ours = OURS.lock().unwrap_or_else(|e| e.into_inner());
        match ours.as_ref() {
            Some(o) if o.id == id => (o.started, o.started_mono_us),
            _ => return,
        }
    };
    let last = super::silence::MIC_SPEECH_US
        .load(Ordering::Relaxed)
        .max(super::silence::SYSTEM_SPEECH_US.load(Ordering::Relaxed))
        .max(started_mono_us);
    let quiet_s = super::mono_us().saturating_sub(last) as f64 / 1e6;
    if quiet_s < QUIET_STOP_S {
        return;
    }
    let length_s = started.elapsed().as_secs_f64();
    match super::stop_meeting(app) {
        Ok(_) => log::info!(
            "Auto meeting: stopped {id} after {quiet_s:.0} s without speech ({length_s:.0} s recorded)"
        ),
        Err(e) => log::warn!("Auto meeting: stopping {id} failed: {e}"),
    }
    *OURS.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// The meeting the listener started ended some other way (stop button,
/// shortcut): log it once and forget it.
fn forget_ours_if_gone() {
    let mut ours = OURS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(o) = ours.take() {
        log::info!(
            "Auto meeting: {} stopped by the user after {:.0} s",
            o.id,
            o.started.elapsed().as_secs_f64()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(v: &[f32]) -> Vec<f32> {
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter().map(|x| x / n).collect()
    }

    #[test]
    fn known_voice_pieces_are_counted() {
        let me = unit(&[1.0, 0.1, 0.0]);
        let pieces = [
            unit(&[0.9, 0.2, 0.0]),
            unit(&[0.0, 1.0, 0.1]),
            unit(&[1.0, 0.0, 0.1]),
        ];
        let (matched, best) = known_matches(&pieces, &[me], KNOWN_VOICE);
        assert_eq!(matched, 2);
        assert!(best > 0.95);
    }

    #[test]
    fn no_known_voices_match_nothing() {
        let pieces = [unit(&[1.0, 0.0, 0.0])];
        assert_eq!(known_matches(&pieces, &[], KNOWN_VOICE).0, 0);
    }

    #[test]
    fn window_counts_speech_and_cuts_pieces() {
        let mut w = Window::new();
        let frame = vec![0.1f32; EARSHOT_FRAME_SAMPLES];
        // 40 s: speech every other second.
        for i in 0..(40.0 * FPS) as usize {
            let speech = ((i as f64 / FPS) as usize) % 2 == 0;
            w.push(&frame, speech);
        }
        assert!((w.speech_s() - 15.0).abs() < 0.6, "{}", w.speech_s());
        let pieces = w.speech_pieces();
        assert_eq!(pieces.len(), 5, "15 s of speech in whole 3 s pieces");
        // Whole 16 ms frames: at least 3 s, less than one frame more.
        let piece_len = (PIECE_S * RATE as f64) as usize;
        assert!(pieces
            .iter()
            .all(|p| p.len() >= piece_len && p.len() < piece_len + EARSHOT_FRAME_SAMPLES));
        assert!(w.preroll().len() <= (PREROLL_S * RATE as f64) as usize);
    }
}

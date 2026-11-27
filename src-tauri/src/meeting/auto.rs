//! Meetings that start and stop by themselves when people talk in the room
//! (board card `auto-meeting`). On by default for everyone since
//! 2026-11-26 (`meetings.auto_start_enabled`; Robin, after a day on his
//! Mac), but only once Sokki knows the user's own voice: until then the
//! microphone stays closed and nothing starts. Robin uninstalled Wispr Flow
//! over bad auto starts, hence the care. Research and the reasoning behind
//! each number: `docs/research/auto-meeting-2026-11.md`.
//!
//! While the setting is on and nothing records, a listener keeps the
//! microphone open (macOS shows its orange dot) and runs the voice detector
//! dictation uses on it. Audio stays in memory, at most [`PREROLL_S`]; it is
//! never written anywhere unless a meeting starts, and then it becomes the
//! meeting's first seconds, so the start of the conversation is in the note.
//!
//! A meeting starts when all of these hold:
//! - at least [`MIN_SPEECH_S`] of speech in the last [`WINDOW_S`],
//! - the user is in it: at least [`MIN_KNOWN_PIECES`] speech pieces match
//!   the user's own voice (the profile in `voice-profiles.json` named
//!   `meetings.user_name`; speaker embeddings, the model meetings use).
//!   Other people's voices, even ones Sokki knows, don't count (Robin,
//!   2026-11-26: "Only activate it if the owner is present"). The user
//!   alone is enough ("It should also record when only I am speaking"); a
//!   video, a podcast or a TV doesn't start it.
//! - no call app on the microphone (call detection asks for those, as
//!   before), no dictation running, and no app playing sound (a video or
//!   music on the speakers reaches the microphone; with headphones it
//!   doesn't, and nothing plays then).
//!
//! Then Sokki listens in: it records the conversation into memory only,
//! nothing on screen, nothing on disk. After `auto_start_show_after_s`
//! (60 s), if there is still speech and the user's voice is in it, it
//! becomes a meeting with everything heard so far as its first minutes. A
//! conversation that ends before (no speech for [`TRIAL_QUIET_S`]) is
//! dropped (Robin, 2026-11-26: "anything below that: We just drop it").
//!
//! It stops by itself after [`QUIET_STOP_S`] without speech; when the
//! conversation lasted less than `auto_start_min_talk_s` (2 min) from the
//! listening-in, the meeting is deleted instead. Both happen without asking. A one-line card (canvas M2.2, Robin's pick 2026-11-26)
//! shows for [`CARD_S`]: "Von selbst aufgenommen", an Auto-Start switch
//! (sets `auto_start_enabled` to false; the recording goes on) and an X that
//! only closes the card. The pill shows the recording. Stopping an
//! auto-started meeting in the pill means it wasn't wanted: no auto start
//! for [`COOLDOWN_S`], so the same conversation doesn't start it again.
//!
//! Every decision is logged with its reason and timing ("Auto meeting: …"),
//! never with what was said, so false and missed starts can be counted.

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sokki_recorder::{AudioChunk, Recorder, RecorderConfig, Sink};
use specta::Type;
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
/// Speech pieces compared with the user's voice.
const PIECE_S: f64 = 3.0;
/// A piece is the user's voice at this cosine similarity to one of its
/// embeddings. Voice profiles use 0.6 on long audio; a 3 s piece scatters
/// more. Tuned on made-up voices (research doc), to be checked on Robin's.
pub const KNOWN_VOICE: f32 = 0.5;
/// Pieces that must match the user's voice (one could be chance).
pub const MIN_KNOWN_PIECES: usize = 2;
/// After a rejected candidate (not the user, sound playing, call), wait this
/// long before checking again, so the embedder doesn't run every second.
const RECHECK_S: f64 = 15.0;
/// While listening in, the conversation counts as over after this long
/// without speech: dropped, nothing kept.
const TRIAL_QUIET_S: f64 = 30.0;
/// Listening in never holds more than this in memory (about 19 MB).
const TRIAL_MAX_S: f64 = 300.0;
/// An auto-started meeting stops after this long without speech.
pub const QUIET_STOP_S: f64 = 120.0;
/// How long the card stays.
pub const CARD_S: u64 = 20;
/// After the user stops an auto-started meeting, no auto start for this long.
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
    /// When the listener began listening in on the conversation.
    began_mono_us: u64,
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

/// The user's own voice: the embeddings of the voice profile named
/// `meetings.user_name`, the name meetings give the user. Empty until the
/// user has named themself in a meeting and kept their voice. Test mode:
/// the voice in `SOKKI_TEST_AUTO_KNOWN`.
fn owner_voice(app: &AppHandle, embedder: &diarize::SpeakerEmbedder) -> Vec<Vec<f32>> {
    if crate::test_mic::active() {
        return std::env::var_os("SOKKI_TEST_AUTO_KNOWN")
            .and_then(|p| crate::test_mic::load_wav(std::path::Path::new(&p)))
            .and_then(|audio| embedder.embed(&audio).ok())
            .into_iter()
            .collect();
    }
    owner_profile_embeddings(app)
}

fn owner_profile_embeddings(app: &AppHandle) -> Vec<Vec<f32>> {
    let user = crate::settings::get_settings(app).meetings.user_name;
    let user = user.trim();
    super::process::voice_profiles(app)
        .and_then(|p| {
            p.profiles()
                .iter()
                .find(|v| v.name.trim() == user)
                .map(|v| v.embeddings.clone())
        })
        .unwrap_or_default()
}

/// Whether the user's own voice is known, without loading the speaker model.
fn owner_voice_known(app: &AppHandle) -> bool {
    if crate::test_mic::active() {
        return std::env::var_os("SOKKI_TEST_AUTO_KNOWN").is_some();
    }
    !owner_profile_embeddings(app).is_empty()
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
    let mut waiting_for_voice = false;
    let mut trial: Option<Trial> = None;
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
            if let Some(t) = trial.take() {
                log::info!(
                    "Auto meeting: dropped a conversation after {:.0} s (a meeting started); nothing kept",
                    t.began.elapsed().as_secs_f64()
                );
            }
            watch_ours(app, id);
            std::thread::sleep(Duration::from_millis(500));
            continue;
        }
        forget_ours_if_gone(app);
        if !on {
            if let Some(l) = listening.take() {
                close_listener(l);
                log::info!("Auto meeting: stopped listening");
            }
            embedder = None;
            window.clear();
            trial = None;
            std::thread::sleep(Duration::from_secs(1));
            continue;
        }
        if listening.is_none() {
            // No microphone (and no orange dot) before the user's own voice
            // is known: nothing could start anyway.
            if !owner_voice_known(app) {
                if !waiting_for_voice {
                    log::info!("Auto meeting: waiting for the user's own voice; not listening");
                    waiting_for_voice = true;
                }
                std::thread::sleep(Duration::from_secs(30));
                continue;
            }
            waiting_for_voice = false;
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
                    if let Some(t) = trial.as_mut() {
                        t.audio.extend_from_slice(&frame);
                        if speech {
                            t.last_speech = Instant::now();
                        }
                    }
                    frame.clear();
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                log::warn!("Auto meeting: the microphone stopped delivering; reopening");
                if let Some(l) = listening.take() {
                    close_listener(l);
                }
                if let Some(t) = trial.take() {
                    log::info!(
                        "Auto meeting: dropped a conversation after {:.0} s (the microphone stopped); nothing kept",
                        t.began.elapsed().as_secs_f64()
                    );
                }
                continue;
            }
        }
        let timing = Timing::from_settings(app);

        // Listening in: the conversation is recorded into memory only, and
        // shown as a meeting once it held for `show_after_s`.
        if let Some(t) = trial.as_ref() {
            let in_s = t.began.elapsed().as_secs_f64();
            let drop = if t.last_speech.elapsed().as_secs_f64() >= TRIAL_QUIET_S {
                Some(format!("no speech for {TRIAL_QUIET_S:.0} s"))
            } else if t.audio.len() as f64 / RATE as f64 > TRIAL_MAX_S {
                Some(format!("still not clear after {TRIAL_MAX_S:.0} s"))
            } else {
                None
            };
            if let Some(why) = drop {
                log::info!(
                    "Auto meeting: dropped a conversation after {in_s:.0} s ({why}); nothing kept"
                );
                trial = None;
                next_check = Instant::now() + Duration::from_secs_f64(RECHECK_S);
                continue;
            }
            if in_s < timing.show_after_s || Instant::now() < next_check {
                continue;
            }
            if let Some(block) = blocked(app) {
                log::info!(
                    "Auto meeting: dropped a conversation after {in_s:.0} s ({}); nothing kept",
                    describe(&block)
                );
                trial = None;
                next_check = Instant::now() + Duration::from_secs_f64(RECHECK_S);
                continue;
            }
            let speech_s = window.speech_s();
            let check = (speech_s >= MIN_SPEECH_S)
                .then(|| owner_check(app, &mut embedder, &mut embedder_failed, &window));
            match check {
                Some(c) if c.matched >= MIN_KNOWN_PIECES => {
                    let Some(t) = trial.take() else { continue };
                    if let Some(l) = listening.take() {
                        close_listener(l);
                    }
                    window.clear();
                    start_ours(app, t, speech_s, &c);
                }
                Some(c) => log::info!(
                    "Auto meeting: still listening in after {in_s:.0} s, the user's voice not in the last {WINDOW_S:.0} s ({} of {} pieces, best similarity {:.2})",
                    c.matched,
                    c.pieces,
                    c.best
                ),
                None => log::info!(
                    "Auto meeting: still listening in after {in_s:.0} s, only {speech_s:.1} s of speech in the last {WINDOW_S:.0} s"
                ),
            }
            next_check = Instant::now() + Duration::from_secs_f64(RECHECK_S);
            continue;
        }

        if Instant::now() < next_check || window.speech_s() < MIN_SPEECH_S {
            continue;
        }
        // A candidate: enough speech. Checks from cheapest to dearest.
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
        let c = owner_check(app, &mut embedder, &mut embedder_failed, &window);
        next_check = Instant::now() + Duration::from_secs_f64(RECHECK_S);
        if c.matched < MIN_KNOWN_PIECES {
            log::info!(
                "Auto meeting: not started, the user's voice not in it (speech {speech_s:.1} s in {WINDOW_S:.0} s, {} of {} pieces the user's, best similarity {:.2}, {} samples of the user's voice, checked in {} ms)",
                c.matched,
                c.pieces,
                c.best,
                c.known,
                c.check_ms
            );
            continue;
        }
        log::info!(
            "Auto meeting: listening in (speech {speech_s:.1} s in {WINDOW_S:.0} s, {} of {} pieces the user's voice, best similarity {:.2}, checked in {} ms, listening for {listened_s:.0} s); shown after {:.0} s, nothing kept if it ends sooner",
            c.matched,
            c.pieces,
            c.best,
            c.check_ms,
            timing.show_after_s
        );
        trial = Some(Trial {
            began: Instant::now(),
            began_mono_us: super::mono_us(),
            last_speech: Instant::now(),
            audio: window.preroll(),
        });
    }
}

// ---------------------------------------------------------------- recent

/// The last auto starts, for Settings › Meetings (Robin, 2026-11-26: "the
/// stats are interesting too, but at most 3"). Conversations dropped while
/// listening in never show here.
const RECENT_FILE: &str = "auto-meetings.json";
const RECENT_MAX: usize = 3;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Type)]
#[serde(rename_all = "snake_case")]
pub enum AutoOutcome {
    /// Still recording.
    Recording,
    /// Stopped by itself and kept.
    Kept,
    /// Stopped by itself and deleted: the conversation was too short.
    Deleted,
    /// The user stopped it (kept as a meeting).
    Stopped,
}

/// One meeting that started by itself.
#[derive(Serialize, Deserialize, Clone, Debug, Type)]
pub struct AutoStart {
    pub meeting_id: String,
    /// Local time, RFC 3339.
    pub started_at: String,
    pub outcome: AutoOutcome,
    /// Seconds of conversation, once it stopped by itself.
    pub talk_s: Option<f64>,
    /// The meeting's title, filled in when read (not stored).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

fn recent_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join(RECENT_FILE))
}

/// The last auto starts, newest first, at most [`RECENT_MAX`].
pub fn recent(app: &AppHandle) -> Vec<AutoStart> {
    recent_path(app)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Records what became of auto-started meeting `id` (new ones go first).
fn remember(app: &AppHandle, id: &str, outcome: AutoOutcome, talk_s: Option<f64>) {
    let Some(path) = recent_path(app) else { return };
    let mut list = recent(app);
    match list.iter_mut().find(|e| e.meeting_id == id) {
        Some(e) => {
            e.outcome = outcome;
            e.talk_s = talk_s.or(e.talk_s);
        }
        None => list.insert(
            0,
            AutoStart {
                meeting_id: id.to_string(),
                started_at: chrono::Local::now().to_rfc3339(),
                outcome,
                talk_s,
                title: None,
            },
        ),
    }
    list.truncate(RECENT_MAX);
    for e in &mut list {
        e.title = None;
    }
    let tmp = path.with_extension("json.tmp");
    let written = serde_json::to_vec_pretty(&list)
        .map_err(std::io::Error::other)
        .and_then(|b| std::fs::write(&tmp, b))
        .and_then(|_| std::fs::rename(&tmp, &path));
    if let Err(e) = written {
        log::warn!("Auto meeting: could not save the recent starts: {e}");
    }
}

/// A conversation the listener records into memory only, before it is
/// shown as a meeting. Nothing of it touches the disk unless it becomes one.
struct Trial {
    began: Instant,
    began_mono_us: u64,
    last_speech: Instant,
    /// 16 kHz mono: the pre-roll, then everything since.
    audio: Vec<f32>,
}

/// The two durations the user can set (`meetings.auto_start_show_after_s`,
/// `meetings.auto_start_min_talk_s`).
struct Timing {
    show_after_s: f64,
    min_talk_s: f64,
}

impl Timing {
    fn from_settings(app: &AppHandle) -> Self {
        let m = crate::settings::get_settings(app).meetings;
        Self {
            show_after_s: (m.auto_start_show_after_s as f64).min(TRIAL_MAX_S - 60.0),
            min_talk_s: m.auto_start_min_talk_s as f64,
        }
    }
}

struct OwnerCheck {
    known: usize,
    pieces: usize,
    matched: usize,
    best: f32,
    check_ms: u128,
}

/// How many speech pieces of the window are the user's own voice. Loads
/// the speaker model the first time.
fn owner_check(
    app: &AppHandle,
    embedder: &mut Option<diarize::SpeakerEmbedder>,
    embedder_failed: &mut bool,
    window: &Window,
) -> OwnerCheck {
    let began = Instant::now();
    if embedder.is_none() && !*embedder_failed {
        let mut config = diarize::DiarizeConfig::new(super::process::models_dir(app));
        config.allow_download = false;
        config.num_threads = 1;
        match diarize::SpeakerEmbedder::new(&config) {
            Ok(e) => *embedder = Some(e),
            Err(e) => {
                log::warn!("Auto meeting: no speaker model ({e:#}); nothing starts by itself");
                *embedder_failed = true;
            }
        }
    }
    let pieces = window.speech_pieces();
    let (known, matched, best) = match embedder.as_ref() {
        Some(emb) => {
            let known = owner_voice(app, emb);
            let embeddings: Vec<Vec<f32>> =
                pieces.iter().filter_map(|p| emb.embed(p).ok()).collect();
            let (matched, best) = known_matches(&embeddings, &known, KNOWN_VOICE);
            (known.len(), matched, best)
        }
        None => (0, 0, f32::NAN),
    };
    OwnerCheck {
        known,
        pieces: pieces.len(),
        matched,
        best,
        check_ms: began.elapsed().as_millis(),
    }
}

/// Shows a conversation that held as a meeting: everything heard since
/// before it began becomes the meeting's first minutes.
fn start_ours(app: &AppHandle, t: Trial, speech_s: f64, c: &OwnerCheck) {
    let kept_s = t.audio.len() as f64 / RATE as f64;
    let in_s = t.began.elapsed().as_secs_f64();
    *PREROLL.lock().unwrap_or_else(|e| e.into_inner()) = Some(t.audio);
    let begun = Instant::now();
    match super::start_meeting(app, None) {
        Ok(id) => {
            log::info!(
                "Auto meeting: started {id} after listening in for {in_s:.0} s (speech {speech_s:.1} s in {WINDOW_S:.0} s, {} of {} pieces the user's voice, best similarity {:.2}, recording {} ms later, {kept_s:.1} s from before kept)",
                c.matched,
                c.pieces,
                c.best,
                begun.elapsed().as_millis()
            );
            *OURS.lock().unwrap_or_else(|e| e.into_inner()) = Some(Ours {
                id: id.clone(),
                started: Instant::now(),
                started_mono_us: super::mono_us(),
                began_mono_us: t.began_mono_us,
            });
            crate::usage::count(app, crate::usage::Count::AutoMeetingStarted);
            remember(app, &id, AutoOutcome::Recording, None);
            offer_card(app, id);
        }
        Err(e) => log::warn!("Auto meeting: starting failed: {e}"),
    }
    take_preroll();
}

fn describe(b: &Block) -> String {
    match b {
        Block::Call(app) => format!("a call runs in {app} (call detection asks)"),
        Block::Dictation => "a dictation runs".into(),
        Block::Playing(apps) => format!("sound plays from {}", apps.join(", ")),
        Block::Cooldown(s) => format!("stopped by the user {s} s ago"),
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

/// The card after a start: the Auto-Start switch ("off") turns the setting
/// off, the X ("close") only closes the card. Test mode shows no window;
/// `SOKKI_TEST_AUTO_DISCARD=off` flips the switch after 3 s,
/// `SOKKI_TEST_AUTO_DISCARD=close` clicks the X.
fn offer_card(app: &AppHandle, id: String) {
    let handle = app.clone();
    std::thread::spawn(move || {
        let shown = Instant::now();
        let answer = if crate::test_mic::active() {
            std::env::var("SOKKI_TEST_AUTO_DISCARD")
                .ok()
                .inspect(|_| std::thread::sleep(Duration::from_secs(3)))
        } else {
            super::prompt::ask(
                &handle,
                super::prompt::Kind::AutoStarted,
                "",
                Duration::from_secs(CARD_S),
            )
        };
        match answer.as_deref() {
            Some("off") => {
                let mut settings = crate::settings::get_settings(&handle);
                settings.meetings.auto_start_enabled = false;
                settings.meetings.auto_start_set = true;
                crate::settings::write_settings(&handle, settings);
                log::info!(
                    "Auto meeting: turned off by the user from the card {:.1} s after starting {id}; the recording goes on",
                    shown.elapsed().as_secs_f64()
                );
                crate::usage::count(&handle, crate::usage::Count::AutoMeetingTurnedOff);
            }
            Some("close") => log::info!(
                "Auto meeting: card closed by the user {:.1} s after starting {id}; the recording goes on",
                shown.elapsed().as_secs_f64()
            ),
            _ => {}
        }
    });
}

/// While meeting `id` records: if the listener started it, stop it after
/// [`QUIET_STOP_S`] without speech on either track.
fn watch_ours(app: &AppHandle, id: &str) {
    let (started, started_mono_us, began_mono_us) = {
        let ours = OURS.lock().unwrap_or_else(|e| e.into_inner());
        match ours.as_ref() {
            Some(o) if o.id == id => (o.started, o.started_mono_us, o.began_mono_us),
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
    let talk_s = last.saturating_sub(began_mono_us) as f64 / 1e6;
    let min_talk_s = Timing::from_settings(app).min_talk_s;
    // Forgotten first, so the listener doesn't log it as stopped by the user.
    *OURS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    if talk_s < min_talk_s {
        match super::discard_meeting(app) {
            Ok(_) => {
                log::info!(
                    "Auto meeting: deleted {id}: the conversation lasted {talk_s:.0} s, less than {min_talk_s:.0} s"
                );
                remember(app, id, AutoOutcome::Deleted, Some(talk_s));
            }
            Err(e) => log::warn!("Auto meeting: deleting the short {id} failed: {e}"),
        }
        return;
    }
    match super::stop_meeting(app) {
        Ok(_) => {
            log::info!(
                "Auto meeting: stopped {id} after {quiet_s:.0} s without speech ({length_s:.0} s recorded, {talk_s:.0} s of conversation)"
            );
            remember(app, id, AutoOutcome::Kept, Some(talk_s));
        }
        Err(e) => log::warn!("Auto meeting: stopping {id} failed: {e}"),
    }
}

/// The meeting the listener started ended some other way (stop button,
/// shortcut): the user didn't want it. Log it once, forget it, and start
/// nothing by itself for [`COOLDOWN_S`].
fn forget_ours_if_gone(app: &AppHandle) {
    let mut ours = OURS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(o) = ours.take() {
        *COOLDOWN_UNTIL.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(Instant::now() + Duration::from_secs(COOLDOWN_S));
        log::info!(
            "Auto meeting: {} stopped by the user after {:.0} s; no auto start for {} min",
            o.id,
            o.started.elapsed().as_secs_f64(),
            COOLDOWN_S / 60
        );
        crate::usage::count(app, crate::usage::Count::AutoMeetingStoppedByUser);
        remember(app, &o.id, AutoOutcome::Stopped, None);
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

    /// Made-up keyboard sound, the same as scripts/make-typing-wav.py: key
    /// clicks (noise burst with a resonance, key up 80 ms later) about
    /// `rate` a second over faint room noise. Seeded.
    fn typing_sound(secs: f32, rate: f32, level: f32, seed: u64) -> Vec<f32> {
        let mut s = seed;
        let mut next = move || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((s >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
        };
        let n = (secs * RATE as f32) as usize;
        let mut out: Vec<f32> = (0..n).map(|_| next() * 0.002).collect();
        let mut t = 0.0f32;
        while t < secs {
            t += 1.0 / rate * (0.5 + (next() + 1.0) * 0.5);
            let start = (t * RATE as f32) as usize;
            for (off, amp) in [(0usize, 1.0f32), (1280, 0.5)] {
                let f = 1800.0 + 1200.0 * (next() + 1.0);
                for i in 0..800 {
                    let k = start + off + i;
                    if k >= n {
                        break;
                    }
                    let env = (-(i as f32) / 120.0).exp();
                    let x = 0.6 * next()
                        + 0.4 * (2.0 * std::f32::consts::PI * f * i as f32 / RATE as f32).sin();
                    out[k] += level * amp * env * x;
                }
            }
        }
        out.iter().map(|x| x.clamp(-1.0, 1.0)).collect()
    }

    #[test]
    fn typing_is_not_speech() {
        // 2026-11-27: "meetings are detected when I just type". Keyboard
        // sound must never fill the window with speech, for any typist.
        use crate::audio_toolkit::vad::{EarshotVad, VoiceActivityDetector};
        for (rate, level, seed) in [(5.0, 0.05, 1), (8.0, 0.3, 7), (12.0, 0.3, 3), (12.0, 0.8, 11)] {
            let audio = typing_sound(90.0, rate, level, seed);
            let mut vad = EarshotVad::new(VAD_THRESHOLD).unwrap();
            let mut w = Window::new();
            let mut most = 0.0f64;
            for frame in audio.chunks_exact(EARSHOT_FRAME_SAMPLES) {
                let speech = vad.push_frame(frame).map(|f| f.is_speech()).unwrap();
                w.push(frame, speech);
                most = most.max(w.speech_s());
            }
            assert!(
                most < MIN_SPEECH_S / 2.0,
                "{rate} keys/s at {level}: {most:.1} s of speech in 30 s"
            );
        }
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

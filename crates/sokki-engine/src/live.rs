//! Transcribing a meeting while it is recorded, so that after the call only
//! the last piece of each track, the speakers and the note are left.
//!
//! The host pushes both tracks as they are captured (16 kHz mono). One
//! worker thread lines them up as [`crate::tracks::align_tracks`] will, removes the echo
//! from the microphone as [`meeting_core::aec::cancel_echo`] will (the same
//! filter, frame by frame), cuts each track at pauses
//! ([`meeting_core::live::PieceCutter`]) and transcribes the pieces one at
//! a time. At the stop, [`LiveSession::finish`] transcribes what is left and
//! hands the pieces to [`crate::process::run`], which takes each speaker
//! turn's words from them where it can (see [`pieces_for`]).
//!
//! Everything here is a shortcut, never a requirement: whatever goes wrong
//! (a failed piece, a late system track, a mismatch at the end), the
//! affected turns are simply transcribed after the call as before.

use meeting_core::aec::EchoCanceller;
use meeting_core::live::{place_words, CutConfig, Piece, PieceCutter};
use meeting_core::stitch::{StitchConfig, Stitcher};
use meeting_core::{Diarizer, SpeakerTurn, TrackKind, Transcriber, SAMPLE_RATE};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::tracks::{track_pads, TrackTiming};

/// What the recording left: the pieces and how they were made.
#[derive(Debug, Clone, Default)]
pub struct Live {
    pub pieces: Vec<Piece>,
    /// Leading silence each track got to line them up ([`track_pads`]).
    pub mic_pad: usize,
    pub system_pad: usize,
    /// The system track was heard from its first sample on.
    pub system_seen: bool,
    /// The microphone's echo was removed from this sample on (`None`: kept).
    pub echo_from: Option<usize>,
    /// The microphone went on unfiltered past the start of the system audio
    /// (it arrived too late to tell), so it can't match the run at the end.
    pub echo_broken: bool,
    /// Seconds of audio transcribed during the recording, and at the end.
    pub live_audio_s: f64,
    pub tail_audio_s: f64,
    /// Speakers per track, found window by window ([`meeting_core::stitch`]),
    /// for each track whose every window was diarized.
    pub speakers: Vec<(TrackKind, Vec<SpeakerTurn>)>,
    /// Seconds spent finding speakers during the recording, and at the end.
    pub speakers_live_s: f64,
    pub speakers_tail_s: f64,
}

/// What of the recording [`crate::process::run`] may use: the pieces, and
/// the speakers per track.
pub type Usable = (Vec<Piece>, Vec<(TrackKind, Vec<SpeakerTurn>)>);

/// The pieces and speakers [`crate::process::run`] may use, track by
/// track: a track's only when it was lined up the same way, and the
/// microphone's only when its echo was removed from the same sample (`None`:
/// not removed), as the run does it. Anything else is transcribed and
/// diarized anew.
pub fn usable(live: Live, timing: Option<TrackTiming>, echo_from: Option<usize>) -> Usable {
    let t = timing.unwrap_or_default();
    let (mic_pad, system_pad) = track_pads(t.mic_first_us, t.system_first_us).unwrap_or((0, 0));
    let mic_ok = live.mic_pad == mic_pad && live.echo_from == echo_from && !live.echo_broken;
    let system_ok = live.system_seen && live.system_pad == system_pad;
    if !mic_ok {
        log::info!(
            "Live microphone transcript not used: lined up {} / {}, echo from {:?} / {:?} (during the call / now){}",
            live.mic_pad,
            mic_pad,
            live.echo_from,
            echo_from,
            if live.echo_broken { ", system audio came too late" } else { "" }
        );
    }
    if !system_ok && live.system_seen {
        log::info!(
            "Live system-audio transcript not used: lined up {} / {} samples",
            live.system_pad,
            system_pad
        );
    }
    let keep = |k: TrackKind| match k {
        TrackKind::Mic => mic_ok,
        TrackKind::System => system_ok,
    };
    let mut pieces = live.pieces;
    let mut speakers = live.speakers;
    pieces.retain(|p| keep(p.track));
    speakers.retain(|(k, _)| keep(*k));
    (pieces, speakers)
}

/// [`usable`] pieces only.
pub fn pieces_for(live: Live, timing: Option<TrackTiming>, echo_from: Option<usize>) -> Vec<Piece> {
    usable(live, timing, echo_from).0
}

/// The microphone goes on unfiltered while no system audio has started,
/// but only up to this long ago: the system-audio tap notes its first
/// sample's time at once, so audio older than this can't be after it.
const NOT_STARTED_MARGIN_US: u64 = 1_000_000;

/// At most this many pieces wait for the transcriber (while dictation
/// holds it, say); later ones are left for the end instead of filling the
/// memory.
const MAX_QUEUED: usize = 20;

enum Msg {
    Audio(TrackKind, Vec<f32>),
    Finish,
    /// The meeting ends without processing (the app quits): stop at once.
    Abandon,
}

/// Pushes captured audio to a [`LiveSession`]. Cheap to clone; never blocks.
#[derive(Clone)]
pub struct LiveFeed {
    tx: Sender<Msg>,
}

impl LiveFeed {
    /// Adds audio (16 kHz mono) to the end of a track.
    pub fn push(&self, track: TrackKind, samples: &[f32]) {
        if !samples.is_empty() {
            let _ = self.tx.send(Msg::Audio(track, samples.to_vec()));
        }
    }
}

/// The transcription of one meeting while it is recorded.
pub struct LiveSession {
    tx: Sender<Msg>,
    worker: Option<JoinHandle<Live>>,
}

/// What the host gives a session.
pub struct LiveOptions {
    /// A system track is being recorded too.
    pub with_system: bool,
    /// The start time of each track's first sample (µs since the epoch,
    /// 0 until it arrives), as the host will save it in `timing.json`.
    pub timing: Box<dyn Fn() -> TrackTiming + Send>,
    /// Run first on the worker thread (e.g. to lower its priority).
    pub on_start: Box<dyn FnOnce() + Send>,
    /// The time now, µs since the epoch (the clock `timing` is in).
    pub now_us: Box<dyn Fn() -> u64 + Send>,
    pub cut: CutConfig,
    /// Finds speakers during the recording too, window by window; `None`
    /// leaves them for after it.
    pub diarizer: Option<Box<dyn Diarizer>>,
    pub stitch: StitchConfig,
}

impl LiveSession {
    pub fn start(transcriber: Arc<dyn Transcriber>, options: LiveOptions) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("meeting-live".into())
            .spawn(move || {
                let LiveOptions {
                    with_system,
                    timing,
                    on_start,
                    now_us,
                    cut,
                    diarizer,
                    stitch,
                } = options;
                on_start();
                let mut w = Worker::new(transcriber, with_system, timing, now_us, cut);
                if let Some(d) = diarizer {
                    w.mic.speakers = Some(SpeakerLane::new(stitch));
                    w.sys.speakers = Some(SpeakerLane::new(stitch));
                    w.diarizer = Some(d);
                }
                w.run(rx)
            })?;
        Ok(Self {
            tx,
            worker: Some(worker),
        })
    }

    pub fn feed(&self) -> LiveFeed {
        LiveFeed {
            tx: self.tx.clone(),
        }
    }

    /// Transcribes what is left and returns all pieces. Blocks for the
    /// last piece of each track (seconds). Push everything before.
    pub fn finish(mut self) -> Option<Live> {
        let _ = self.tx.send(Msg::Finish);
        let worker = self.worker.take()?;
        match worker.join() {
            Ok(live) => Some(live),
            Err(_) => {
                log::warn!("Live transcription stopped with a panic; transcribing after the call");
                None
            }
        }
    }
}

impl Drop for LiveSession {
    fn drop(&mut self) {
        // Dropped without `finish` (the app quits, the meeting failed): the
        // worker ends without transcribing the rest.
        if self.worker.is_some() {
            let _ = self.tx.send(Msg::Abandon);
        }
    }
}

/// One track on its way from the host to the cutter.
struct Lane {
    /// Audio waiting for alignment (before it), or for the echo filter's
    /// matching system frame (the microphone after it).
    queue: Vec<f32>,
    /// Samples received, before alignment.
    received: usize,
    cutter: PieceCutter,
    /// Speakers found as the track goes, when there is a diarizer.
    speakers: Option<SpeakerLane>,
}

impl Lane {
    fn new(cut: CutConfig) -> Self {
        Self {
            queue: Vec::new(),
            received: 0,
            cutter: PieceCutter::new(cut),
            speakers: None,
        }
    }
}

/// One track's speakers, window by window.
struct SpeakerLane {
    stitcher: Stitcher,
    /// The aligned (and for the mic echo-cancelled) audio from `start`
    /// on: what the next window needs.
    audio: Vec<f32>,
    start: usize,
    /// A window failed: this track is diarized after the call instead.
    failed: bool,
}

impl SpeakerLane {
    fn new(cfg: StitchConfig) -> Self {
        Self {
            stitcher: Stitcher::new(cfg),
            audio: Vec::new(),
            start: 0,
            failed: false,
        }
    }

    fn len_s(&self) -> f64 {
        (self.start + self.audio.len()) as f64 / SAMPLE_RATE as f64
    }

    /// Adds audio and diarizes the window it completes, if any. Returns the
    /// seconds the diarizer took.
    fn push(&mut self, diarizer: &dyn Diarizer, samples: &[f32]) -> f64 {
        if self.failed {
            return 0.0;
        }
        self.audio.extend_from_slice(samples);
        let cfg = *self.stitcher.config();
        match cfg.next_window(self.stitcher.last_end_s(), self.len_s()) {
            Some((a, b)) => self.window(diarizer, a, b, false),
            None => 0.0,
        }
    }

    /// The last window, to the end of the track.
    fn finish(&mut self, diarizer: &dyn Diarizer) -> f64 {
        if self.failed || self.audio.is_empty() && self.stitcher.last_end_s().is_none() {
            return 0.0;
        }
        let cfg = *self.stitcher.config();
        let a = self
            .stitcher
            .last_end_s()
            .map_or(0.0, |e| (e - cfg.overlap_s).max(0.0));
        let end = self.len_s();
        self.window(diarizer, a, end, true)
    }

    fn window(&mut self, diarizer: &dyn Diarizer, a: f64, b: f64, last: bool) -> f64 {
        let sr = SAMPLE_RATE as f64;
        let i = ((a * sr) as usize).saturating_sub(self.start).min(self.audio.len());
        let j = ((b * sr) as usize).saturating_sub(self.start).min(self.audio.len());
        let t0 = Instant::now();
        match diarizer.diarize(&self.audio[i..j.max(i)]) {
            Ok(turns) => self.stitcher.add_window(a, b, turns, last),
            Err(e) => {
                log::warn!("Live speakers: a window failed, the track is diarized after the call: {e:#}");
                self.failed = true;
            }
        }
        // Keep only what the next window reaches back to.
        let keep_from = ((b - self.stitcher.config().overlap_s).max(0.0) * sr) as usize;
        let drop = keep_from.saturating_sub(self.start).min(self.audio.len());
        self.audio.drain(..drop);
        self.start += drop;
        t0.elapsed().as_secs_f64()
    }
}

/// Where the system track stands.
#[derive(Debug, Clone, Copy, PartialEq)]
enum System {
    /// Not yet heard: the microphone goes on unfiltered, up to a moment the
    /// system audio can't have started before.
    Waiting,
    /// Lined up (or not recorded at all).
    Decided,
}

struct Worker {
    transcriber: Arc<dyn Transcriber>,
    with_system: bool,
    timing: Box<dyn Fn() -> TrackTiming + Send>,
    now_us: Box<dyn Fn() -> u64 + Send>,
    /// When the microphone's first sample was captured; 0 until known.
    mic_first_us: u64,
    system: System,
    /// Microphone samples (lined up) passed on so far, and how far it may
    /// go unfiltered while the system audio hasn't started.
    mic_done: usize,
    mic_safe: usize,
    mic: Lane,
    sys: Lane,
    /// The lined-up system track from mic sample `reference_at` on, kept
    /// until the microphone frame of the same moment has been filtered.
    reference: Vec<f32>,
    reference_at: usize,
    aec: Option<EchoCanceller>,
    queued: Vec<(TrackKind, usize, Vec<f32>)>,
    /// The model gives no word times: pieces are of no use, stop making them.
    no_word_times: bool,
    diarizer: Option<Box<dyn Diarizer>>,
    live: Live,
}

impl Worker {
    fn new(
        transcriber: Arc<dyn Transcriber>,
        with_system: bool,
        timing: Box<dyn Fn() -> TrackTiming + Send>,
        now_us: Box<dyn Fn() -> u64 + Send>,
        cut: CutConfig,
    ) -> Self {
        Self {
            transcriber,
            with_system,
            timing,
            now_us,
            mic_first_us: 0,
            system: System::Waiting,
            mic_done: 0,
            mic_safe: 0,
            mic: Lane::new(cut),
            sys: Lane::new(cut),
            reference: Vec::new(),
            reference_at: 0,
            aec: None,
            queued: Vec::new(),
            no_word_times: false,
            diarizer: None,
            live: Live::default(),
        }
    }

    fn run(mut self, rx: Receiver<Msg>) -> Live {
        let started = Instant::now();
        loop {
            // Take everything that arrived; transcribe one piece when idle.
            let msg = if self.queued.is_empty() {
                rx.recv_timeout(Duration::from_millis(500))
            } else {
                rx.try_recv().map_err(|e| match e {
                    mpsc::TryRecvError::Empty => RecvTimeoutError::Timeout,
                    mpsc::TryRecvError::Disconnected => RecvTimeoutError::Disconnected,
                })
            };
            match msg {
                Ok(Msg::Audio(kind, samples)) => self.add(kind, samples),
                Ok(Msg::Finish) => break,
                Ok(Msg::Abandon) | Err(RecvTimeoutError::Disconnected) => return Live::default(),
                Err(RecvTimeoutError::Timeout) => {
                    self.try_align();
                    self.flow();
                    if let Some(p) = (!self.queued.is_empty()).then(|| self.queued.remove(0)) {
                        let audio_s = p.2.len() as f64 / SAMPLE_RATE as f64;
                        self.transcribe(p);
                        self.live.live_audio_s += audio_s;
                    }
                }
            }
        }
        self.finish();
        log::info!(
            "Live transcript: {} pieces, {:.0} s during the call, {:.0} s left at the end ({:.0} s recorded)",
            self.live.pieces.len(),
            self.live.live_audio_s,
            self.live.tail_audio_s,
            started.elapsed().as_secs_f64()
        );
        if !self.live.speakers.is_empty() {
            log::info!(
                "Live speakers: {} tracks, {:.1} s of speaker detection during the call, {:.1} s at the end",
                self.live.speakers.len(),
                self.live.speakers_live_s,
                self.live.speakers_tail_s
            );
        }
        self.live
    }

    fn add(&mut self, kind: TrackKind, samples: Vec<f32>) {
        let lane = match kind {
            TrackKind::Mic => &mut self.mic,
            TrackKind::System => &mut self.sys,
        };
        lane.received += samples.len();
        lane.queue.extend_from_slice(&samples);
        self.try_align();
        self.flow();
    }

    /// Lines the tracks up exactly as [`crate::tracks::align_tracks`] will at
    /// the end, as soon as both start times are known. Until the system audio
    /// starts (the tap only delivers once something plays), the microphone
    /// goes on unpadded and unfiltered, as the run at the end treats the
    /// time before it.
    fn try_align(&mut self) {
        let t = (self.timing)();
        if self.mic_first_us == 0 {
            if t.mic_first_us == 0 {
                return;
            }
            self.mic_first_us = t.mic_first_us;
            if !self.with_system {
                self.system = System::Decided;
            } else if t.system_first_us != 0 {
                // Both known at once: either may be the later one.
                self.line_up(t);
            }
        }
        if self.system == System::Waiting {
            if t.system_first_us != 0 {
                self.line_up(t);
            } else {
                let safe_us = (self.now_us)()
                    .saturating_sub(NOT_STARTED_MARGIN_US)
                    .saturating_sub(self.mic_first_us);
                // In u128: a clock far ahead (as in tests) must not overflow.
                let safe = (safe_us as u128 * SAMPLE_RATE as u128 / 1_000_000)
                    .min(usize::MAX as u128) as usize;
                self.mic_safe = self.mic_safe.max(safe);
            }
        }
    }

    fn line_up(&mut self, t: TrackTiming) {
        self.system = System::Decided;
        self.live.system_seen = true;
        let Some((mic_pad, system_pad)) = track_pads(t.mic_first_us, t.system_first_us) else {
            // Taken for a clock glitch: neither padded nor filtered, as at
            // the end.
            log::info!("Live transcript: track start times look wrong; not lined up");
            return;
        };
        if mic_pad > 0 {
            if self.mic_done > 0 {
                // Already passed on unpadded; can't match the end any more.
                self.live.echo_broken = true;
                return;
            }
            let mut q = vec![0.0; mic_pad];
            q.extend_from_slice(&self.mic.queue);
            self.mic.queue = q;
        }
        if self.mic_done > system_pad {
            // The mic went on unfiltered past the system's start.
            self.live.echo_broken = true;
        }
        let mut q = vec![0.0; system_pad];
        q.extend_from_slice(&self.sys.queue);
        self.sys.queue = q;
        self.live.mic_pad = mic_pad;
        self.live.system_pad = system_pad;
        self.live.echo_from = Some(system_pad);
        self.reference_at = 0;
        self.aec = Some(EchoCanceller::new());
    }

    /// Moves audio on: the system track straight to its cutter, the
    /// microphone through the echo filter as far as the system track has
    /// arrived (unfiltered before the system audio's start).
    fn flow(&mut self) {
        if self.mic_first_us == 0 {
            return;
        }
        if self.system == System::Decided && !self.sys.queue.is_empty() {
            let s = std::mem::take(&mut self.sys.queue);
            let pieces = self.sys.cutter.push(&s);
            if let (Some(d), Some(sl)) = (&self.diarizer, &mut self.sys.speakers) {
                self.live.speakers_live_s += sl.push(d.as_ref(), &s);
            }
            if self.aec.is_some() {
                self.reference.extend_from_slice(&s);
            }
            self.enqueue(TrackKind::System, pieces);
        }
        let have = self.mic_done + self.mic.queue.len();
        let upto = match (self.system, self.live.echo_from) {
            (System::Waiting, _) => have.min(self.mic_safe),
            (System::Decided, Some(from)) if self.aec.is_some() => {
                // Whole frames from `from` on, each with its system frame.
                let frame = EchoCanceller::FRAME;
                let ref_end = self.reference_at + self.reference.len();
                let raw_end = from.max(self.mic_done).min(have);
                let filtered = ref_end.min(have).saturating_sub(raw_end.max(from)) / frame * frame;
                if raw_end < from { raw_end } else { raw_end + filtered }
            }
            _ => have,
        };
        if upto > self.mic_done {
            self.filter_mic(upto - self.mic_done, false);
        }
    }

    /// Filters and cuts the next `n` queued microphone samples. `end`: the
    /// recording is over; frames past the system track's end are filtered
    /// against silence, as `cancel_echo` does, and a last partial frame is
    /// left as recorded.
    fn filter_mic(&mut self, n: usize, end: bool) {
        let mut mic: Vec<f32> = self.mic.queue.drain(..n.min(self.mic.queue.len())).collect();
        let at = self.mic_done;
        if let (Some(aec), Some(from)) = (&mut self.aec, self.live.echo_from) {
            let frame = EchoCanceller::FRAME;
            // Samples of this batch from the echo filter's start on.
            let skip = from.saturating_sub(at).min(mic.len());
            let rest = mic.len() - skip;
            let whole = rest / frame * frame;
            debug_assert!(end || whole == rest, "filtered in whole frames");
            for (k, m) in mic[skip..skip + whole].chunks_mut(frame).enumerate() {
                let i = at + skip + k * frame; // mic sample index
                let r = i
                    .checked_sub(self.reference_at)
                    .and_then(|o| self.reference.get(o..o + frame));
                aec.process_frame(m, r);
            }
            let done = at + skip + whole;
            let drop = done.saturating_sub(self.reference_at).min(self.reference.len());
            self.reference.drain(..drop);
            self.reference_at += drop;
        }
        self.mic_done += mic.len();
        if !mic.is_empty() {
            let pieces = self.mic.cutter.push(&mic);
            if let (Some(d), Some(sl)) = (&self.diarizer, &mut self.mic.speakers) {
                self.live.speakers_live_s += sl.push(d.as_ref(), &mic);
            }
            self.enqueue(TrackKind::Mic, pieces);
        }
    }

    fn enqueue(&mut self, kind: TrackKind, pieces: Vec<(usize, Vec<f32>)>) {
        for (at, audio) in pieces {
            if self.queued.len() >= MAX_QUEUED {
                // Left for the end: no piece there, so the turns are
                // transcribed after the call as before.
                log::warn!("Live transcript: falling behind, a piece is left for the end");
                continue;
            }
            self.queued.push((kind, at, audio));
        }
    }

    fn transcribe(&mut self, (kind, at, audio): (TrackKind, usize, Vec<f32>)) {
        if self.no_word_times {
            return;
        }
        let start_s = at as f64 / SAMPLE_RATE as f64;
        let end_s = start_s + audio.len() as f64 / SAMPLE_RATE as f64;
        match self.transcriber.transcribe_words(&audio) {
            Ok(Some(words)) => self.live.pieces.push(Piece {
                track: kind,
                start_s,
                end_s,
                words: place_words(words, start_s),
            }),
            // A model without word times can't place words in turns.
            Ok(None) => {
                log::info!("Live transcript off: the speech model gives no word times");
                self.no_word_times = true;
            }
            Err(e) => log::warn!("Live transcript: a piece failed, left for the end: {e:#}"),
        }
    }

    fn finish(&mut self) {
        if self.mic_first_us == 0 {
            // Nothing recorded: nothing to give.
            return;
        }
        self.try_align();
        // No system audio until the end: none was recorded, as the run at the
        // end will see it.
        self.system = System::Decided;
        self.flow();
        let n = self.mic.queue.len();
        self.filter_mic(n, true);
        let mut tail: Vec<(TrackKind, usize, Vec<f32>)> = std::mem::take(&mut self.queued);
        for (kind, lane) in [
            (TrackKind::Mic, &mut self.mic),
            (TrackKind::System, &mut self.sys),
        ] {
            if let Some((at, audio)) = lane.cutter.finish() {
                tail.push((kind, at, audio));
            }
        }
        for p in tail {
            self.live.tail_audio_s += p.2.len() as f64 / SAMPLE_RATE as f64;
            self.transcribe(p);
        }
        let Some(d) = self.diarizer.take() else { return };
        for (kind, lane) in [
            (TrackKind::Mic, &mut self.mic),
            (TrackKind::System, &mut self.sys),
        ] {
            let Some(sl) = lane.speakers.as_mut() else { continue };
            if sl.len_s() <= 0.0 {
                continue; // no such track
            }
            self.live.speakers_tail_s += sl.finish(d.as_ref());
            if !sl.failed {
                self.live.speakers.push((kind, sl.stitcher.turns()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meeting_core::TimedWord;
    use std::sync::Mutex;

    const SR: usize = SAMPLE_RATE as usize;

    /// One "word" per second of audio, at the second's middle; records the
    /// audio it got.
    #[derive(Default)]
    struct Words {
        seen: Mutex<Vec<Vec<f32>>>,
    }
    impl Transcriber for Words {
        fn transcribe(&self, _s: &[f32]) -> anyhow::Result<String> {
            Ok(String::new())
        }
        fn transcribe_words(&self, s: &[f32]) -> anyhow::Result<Option<Vec<TimedWord>>> {
            self.seen.lock().unwrap().push(s.to_vec());
            Ok(Some(
                (0..s.len() / SR)
                    .map(|i| TimedWord {
                        start_s: i as f64 + 0.4,
                        end_s: i as f64 + 0.6,
                        text: "w".into(),
                    })
                    .collect(),
            ))
        }
    }

    fn voice(n: usize, seed: f32) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let t = i as f32 / SR as f32;
                let gate = if (t as usize) % 9 == 0 { 0.0 } else { 1.0 };
                gate * 0.2 * ((t * (180.0 + seed * 40.0) * std::f32::consts::TAU).sin())
            })
            .collect()
    }

    fn session(with_system: bool, timing: TrackTiming, t: Arc<Words>) -> LiveSession {
        LiveSession::start(
            t,
            LiveOptions {
                with_system,
                timing: Box::new(move || timing),
                on_start: Box::new(|| {}),
                now_us: Box::new(|| u64::MAX / 2),
                cut: CutConfig::default(),
                diarizer: None,
                stitch: StitchConfig::default(),
            },
        )
        .unwrap()
    }

    #[test]
    fn pieces_are_the_aligned_echo_cancelled_tracks() {
        let n = 70 * SR;
        let system = voice(n, 1.0);
        // The mic hears the system 30 ms late, plus its own voice.
        let own = voice(n, 4.0);
        let mut mic: Vec<f32> = (0..n)
            .map(|i| own[i] + if i >= 480 { 0.4 * system[i - 480] } else { 0.0 })
            .collect();
        mic.truncate(n - 1234); // not a whole number of frames
                                // The mic started 50 ms after the system.
        let timing = TrackTiming {
            mic_first_us: 1_000_050_000,
            system_first_us: 1_000_000_000,
        };
        let words = Arc::new(Words::default());
        let s = session(true, timing, words.clone());
        let feed = s.feed();
        // Delivered in uneven blocks, the system track a little behind.
        for (k, block) in mic.chunks(4410).enumerate() {
            feed.push(TrackKind::Mic, block);
            if let Some(b) = system.chunks(4410).nth(k) {
                feed.push(TrackKind::System, b);
            }
        }
        for b in system.chunks(4410).skip(mic.chunks(4410).count()) {
            feed.push(TrackKind::System, b);
        }
        let live = s.finish().unwrap();
        assert_eq!(live.echo_from, Some(0));
        assert_eq!((live.mic_pad, live.system_pad), (800, 0));

        // What the run at the end does with the whole tracks.
        let mut tracks = vec![
            meeting_core::Track {
                kind: TrackKind::Mic,
                samples: mic.clone(),
            },
            meeting_core::Track {
                kind: TrackKind::System,
                samples: system.clone(),
            },
        ];
        crate::tracks::align_tracks(&mut tracks, timing.mic_first_us, timing.system_first_us);
        meeting_core::aec::cancel_echo_in_tracks(&mut tracks, 0);

        // The pieces, put back together, are exactly those tracks.
        let seen = words.seen.lock().unwrap();
        for track in &tracks {
            let mut joined = Vec::new();
            for (p, audio) in live.pieces.iter().zip(seen.iter()) {
                if p.track == track.kind {
                    assert_eq!(p.start_s, joined.len() as f64 / SR as f64);
                    joined.extend_from_slice(audio);
                }
            }
            assert_eq!(joined.len(), track.samples.len(), "{:?}", track.kind);
            assert!(joined == track.samples, "{:?} audio differs", track.kind);
        }
        // Word times are on the track's timeline.
        let p = live.pieces.iter().find(|p| p.start_s > 0.0).unwrap();
        assert!((p.words[0].start_s - (p.start_s + 0.4)).abs() < 1e-9);
    }

    #[test]
    fn without_system_audio_the_mic_goes_on_alone() {
        let mic = voice(40 * SR, 2.0);
        let timing = TrackTiming {
            mic_first_us: 5,
            system_first_us: 0,
        };
        let words = Arc::new(Words::default());
        let s = session(true, timing, words.clone());
        for b in mic.chunks(1600) {
            s.feed().push(TrackKind::Mic, b);
        }
        let live = s.finish().unwrap();
        assert_eq!(live.echo_from, None);
        let audio: Vec<f32> = words.seen.lock().unwrap().concat();
        assert!(audio == mic, "the mic is passed on unfiltered");
        // The run at the end saw no system track: the pieces fit.
        assert_eq!(
            pieces_for(live.clone(), Some(timing), None).len(),
            live.pieces.len()
        );
        // Had it removed echo, no mic piece would fit.
        assert!(pieces_for(live, Some(timing), Some(0)).is_empty());
    }

    /// A real in-person meeting (2026-11-23): the system-audio tap delivered
    /// its first sample 12.1 s after the microphone's, when something first
    /// played. The pieces must still be the lined-up, echo-cancelled tracks
    /// of the run at the end, and fit it.
    #[test]
    fn system_audio_starting_late_still_fits() {
        use std::sync::atomic::{AtomicU64, Ordering};
        let mic_first: u64 = 1_000_000_000;
        let late_us: u64 = 12_145_677;
        let n = 60 * SR;
        let mic = voice(n, 4.0);
        let system = voice(n - (late_us as usize * SR / 1_000_000), 1.0);
        // The clock follows the audio fed in: real time, as in a call.
        let fed = Arc::new(AtomicU64::new(0));
        let system_first = Arc::new(AtomicU64::new(0));
        let (f, sf) = (fed.clone(), system_first.clone());
        let words = Arc::new(Words::default());
        let s = LiveSession::start(
            words.clone(),
            LiveOptions {
                with_system: true,
                timing: Box::new(move || TrackTiming {
                    mic_first_us: mic_first,
                    system_first_us: sf.load(Ordering::Relaxed),
                }),
                on_start: Box::new(|| {}),
                now_us: Box::new(move || mic_first + f.load(Ordering::Relaxed)),
                cut: CutConfig::default(),
                diarizer: None,
                stitch: StitchConfig::default(),
            },
        )
        .unwrap();
        let feed = s.feed();
        let block = SR / 10;
        let mut sys_sent = 0;
        for (k, b) in mic.chunks(block).enumerate() {
            let t_us = (k + 1) as u64 * 100_000;
            fed.store(t_us, Ordering::Relaxed);
            feed.push(TrackKind::Mic, b);
            if t_us > late_us {
                system_first.store(mic_first + late_us, Ordering::Relaxed);
                // Two seconds behind, as the tap's writer holds it back.
                let due = ((t_us - late_us).saturating_sub(2_000_000) as usize * SR / 1_000_000)
                    .min(system.len());
                if due > sys_sent {
                    feed.push(TrackKind::System, &system[sys_sent..due]);
                    sys_sent = due;
                }
            }
            // Let the worker keep up, as it does in real time.
            std::thread::sleep(Duration::from_micros(200));
        }
        feed.push(TrackKind::System, &system[sys_sent..]);
        let live = s.finish().unwrap();
        assert!(!live.echo_broken);
        let pad = (late_us as usize) * SR / 1_000_000;
        assert_eq!((live.mic_pad, live.system_pad, live.echo_from), (0, pad, Some(pad)));

        // The run at the end.
        let timing = TrackTiming {
            mic_first_us: mic_first,
            system_first_us: mic_first + late_us,
        };
        let mut tracks = vec![
            meeting_core::Track {
                kind: TrackKind::Mic,
                samples: mic.clone(),
            },
            meeting_core::Track {
                kind: TrackKind::System,
                samples: system.clone(),
            },
        ];
        assert!(crate::tracks::align_tracks(&mut tracks, timing.mic_first_us, timing.system_first_us).is_none());
        assert!(meeting_core::aec::cancel_echo_in_tracks(&mut tracks, pad));
        let seen = words.seen.lock().unwrap();
        for track in &tracks {
            let joined: Vec<f32> = live
                .pieces
                .iter()
                .zip(seen.iter())
                .filter(|(p, _)| p.track == track.kind)
                .flat_map(|(_, a)| a.iter().copied())
                .collect();
            assert!(joined == track.samples, "{:?} differs", track.kind);
        }
        let n_pieces = live.pieces.len();
        assert_eq!(pieces_for(live, Some(timing), Some(pad)).len(), n_pieces);
    }

    /// One speaker per window, labelled "S1" in each, talking throughout.
    struct OneVoice;
    impl Diarizer for OneVoice {
        fn diarize(&self, s: &[f32]) -> anyhow::Result<Vec<SpeakerTurn>> {
            Ok(vec![SpeakerTurn {
                start_s: 0.0,
                end_s: s.len() as f64 / SR as f64,
                speaker: "S1".into(),
                embedding: Some(vec![1.0, 0.0]),
            }])
        }
    }

    #[test]
    fn speakers_are_found_window_by_window() {
        let mic = voice(700 * SR, 2.0);
        let timing = TrackTiming {
            mic_first_us: 5,
            system_first_us: 0,
        };
        let s = LiveSession::start(
            Arc::new(Words::default()),
            LiveOptions {
                with_system: false,
                timing: Box::new(move || timing),
                on_start: Box::new(|| {}),
                now_us: Box::new(|| u64::MAX / 2),
                cut: CutConfig::default(),
                diarizer: Some(Box::new(OneVoice)),
                stitch: StitchConfig::default(),
            },
        )
        .unwrap();
        for b in mic.chunks(16_000) {
            s.feed().push(TrackKind::Mic, b);
        }
        let live = s.finish().unwrap();
        // 700 s: windows at 300 and 600 s, the last one at the stop.
        assert_eq!(live.speakers.len(), 1);
        let (kind, turns) = &live.speakers[0];
        assert_eq!(*kind, TrackKind::Mic);
        assert_eq!(turns.len(), 1, "{turns:?}");
        assert_eq!(turns[0].speaker, "S1");
        assert!((turns[0].end_s - 700.0).abs() < 0.01, "{:?}", turns[0]);
        let (_, speakers) = usable(live, Some(timing), None);
        assert_eq!(speakers.len(), 1);
    }
}

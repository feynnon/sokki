//! System-audio half of a meeting recording (macOS 14.2+ only).
//!
//! The Swift side (`swift/system_audio_tap.swift`) creates a Core Audio process
//! tap on the default output. Its IO callback runs on a real-time thread, so
//! the callback here only downmixes to mono and pushes into a lock-free ring
//! buffer. A separate writer thread drains the buffer into a WAV file.
//!
//! A watchdog thread rebuilds the tap when it goes dead (exact zeros, or no
//! audio at all, while another app plays) or when the default output
//! changes. On 2026-11-24 the tap delivered exact zeros from 10:02:20 to the
//! end of a 45-minute call while Chrome kept playing it into a Bluetooth
//! headset; the call on 2026-11-20 lost its last 15 minutes the same way.
//! The microphone is a separate capture and records on through a rebuild;
//! the call track's gap is filled with silence so it stays aligned.

use super::flac::FlacWriter;
use anyhow::{bail, Result};
use std::ffi::{c_char, c_void, CStr};
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// About 10 seconds of mono audio at 48 kHz. Long enough to ride out a slow
/// disk without blocking the real-time thread.
const RING_CAPACITY: usize = 48_000 * 10;

mod ffi {
    use std::os::raw::{c_char, c_void};

    pub type TapCallback = extern "C" fn(*mut c_void, *const f32, usize, u32, f64);

    extern "C" {
        pub fn sokki_tap_supported() -> bool;
        pub fn sokki_tap_start(
            callback: TapCallback,
            user: *mut c_void,
            out_error: *mut *mut c_char,
        ) -> *mut c_void;
        pub fn sokki_tap_stop(handle: *mut c_void);
        pub fn sokki_tap_rebuild(handle: *mut c_void, out_error: *mut *mut c_char) -> bool;
        pub fn sokki_tap_other_apps_playing(buffer: *mut c_char, capacity: usize) -> i32;
        pub fn sokki_tap_default_output(buffer: *mut c_char, capacity: usize) -> u32;
        pub fn sokki_tap_free_error(error: *mut c_char);
    }
}

/// A pause in the callbacks after the tap was rebuilt: `secs` passed between
/// the callback before and the one that starts at sample `at` (counted in
/// samples pushed), which carried `frames` samples. The writer fills the
/// missing time with silence at the measured rate.
#[derive(Clone, Copy, Debug)]
struct Gap {
    at: u64,
    secs: f64,
    frames: usize,
}

/// A pause shorter than this after a rebuild is left as it is.
const GAP_MIN: Duration = Duration::from_millis(30);

/// State the IO thread writes into. Boxed so its address stays fixed while the
/// tap holds a pointer to it.
struct TapShared {
    producer: rtrb::Producer<f32>,
    gaps: rtrb::Producer<Gap>,
    /// Sample rate, set by the first callback.
    sample_rate: Arc<OnceLock<u32>>,
    dropped_samples: u64,
    /// Samples pushed into `producer` so far.
    pushed: u64,
    last_callback: Option<Instant>,
    /// Bumped by the watchdog before each rebuild.
    rebuilds: Arc<AtomicU64>,
    rebuilds_seen: u64,
}

/// Called on Core Audio's IO thread. Must not block or allocate per call.
extern "C" fn on_samples(
    user: *mut c_void,
    samples: *const f32,
    frames: usize,
    channels: u32,
    sample_rate: f64,
) {
    if user.is_null() || samples.is_null() || channels == 0 {
        return;
    }
    // SAFETY: `user` points at the `TapShared` box owned by `SystemAudioCapture`.
    // The tap calls this only between `sokki_tap_start` and `sokki_tap_stop`,
    // and the box outlives both.
    let shared = unsafe { &mut *(user as *mut TapShared) };
    let _ = shared.sample_rate.set(sample_rate.round() as u32);
    super::note_first_sample(&super::SYSTEM_FIRST_US, frames, sample_rate.round() as u32);

    let now = Instant::now();
    let rebuilds = shared.rebuilds.load(Ordering::Acquire);
    if rebuilds != shared.rebuilds_seen {
        // The first callback of a rebuilt tap comes after a pause; one of the
        // old tap still in flight comes without one and leaves this pending.
        if let Some(last) = shared.last_callback {
            let pause = now.duration_since(last);
            if pause >= GAP_MIN {
                let _ = shared.gaps.push(Gap {
                    at: shared.pushed,
                    secs: pause.as_secs_f64(),
                    frames,
                });
                shared.rebuilds_seen = rebuilds;
            }
        } else {
            shared.rebuilds_seen = rebuilds;
        }
    }
    shared.last_callback = Some(now);

    let channels = channels as usize;
    // SAFETY: Swift guarantees `frames * channels` valid interleaved f32 samples.
    let input = unsafe { std::slice::from_raw_parts(samples, frames * channels) };
    let stamp = super::mono_us().max(1);
    super::SYSTEM_CALLBACK_US.store(stamp, Ordering::Relaxed);
    if input.iter().any(|&v| v != 0.0) {
        super::SYSTEM_SOUND_US.store(stamp, Ordering::Relaxed);
    }
    for frame in input.chunks_exact(channels) {
        let mono = frame.iter().sum::<f32>() / channels as f32;
        if shared.producer.push(mono).is_err() {
            shared.dropped_samples += 1;
        } else {
            shared.pushed += 1;
        }
    }
}

/// Drains the ring buffer into a mono WAV file until told to stop and empty.
fn write_loop(
    mut consumer: rtrb::Consumer<f32>,
    mut gaps: rtrb::Consumer<Gap>,
    sample_rate: Arc<OnceLock<u32>>,
    path: PathBuf,
    stop: Arc<AtomicBool>,
    tee: Option<super::flac::Tee>,
) -> Result<Option<u64>> {
    // Wait for the first callback to tell us the rate. Give up if stopped first.
    let rate = loop {
        if let Some(rate) = sample_rate.get() {
            break *rate;
        }
        if stop.load(Ordering::Acquire) {
            // The tap calls back only while some app plays sound on this Mac
            // (Core Audio waits for a writer), so a meeting with nothing
            // playing gets no callbacks. A denied permission looks the same.
            log::warn!(
                "System audio: no audio callbacks before stop (nothing played on this Mac, or the system audio permission is missing); no track written"
            );
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(10));
    };
    log::info!(
        "System audio: first callback reports {rate} Hz; measuring the real rate, writing {} at 16 kHz",
        path.display()
    );

    // The reported rate can be wrong (Bluetooth headset in a call), so the
    // real rate is measured from how fast samples arrive; audio is held back
    // until the first estimate, then resampled to 16 kHz (see `rate`).
    let mut tracker = super::rate::RateTracker::new();
    let mut resampler = super::rate::Resampler::new();
    let mut held: Vec<f32> = Vec::new();
    let mut out: Vec<f32> = Vec::new();
    let mut writer = FlacWriter::create(&path, super::rate::OUT_RATE)?;
    if let Some(tee) = tee {
        writer.set_tee(tee);
    }
    let mut buffer = vec![0f32; 4096];
    let mut logged_rate: Option<u32> = None;
    let mut popped: u64 = 0;
    // Rebuild pauses inside `held`: (position in `held`, gap).
    let mut held_gaps: Vec<(usize, Gap)> = Vec::new();
    loop {
        // A rebuild's pause, before the samples that follow it.
        if gaps.peek().is_ok_and(|g| g.at <= popped) {
            let gap = gaps.pop().expect("peeked");
            match tracker.current() {
                Some(real) => {
                    let secs = write_silence(&gap, real, &mut resampler, &mut writer, &mut out)?;
                    log::info!(
                        "System audio: filled {secs:.2} s with silence where the tap was rebuilt"
                    );
                }
                // No rate yet: filled when the held audio is written.
                None => held_gaps.push((held.len(), gap)),
            }
            tracker.restart();
            continue;
        }
        let available = consumer.slots();
        if available == 0 {
            // The tap is stopped before `stop` is set, so once both hold, no more
            // samples can arrive.
            if stop.load(Ordering::Acquire) && consumer.slots() == 0 {
                break;
            }
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        let mut n = available.min(buffer.len());
        if let Ok(gap) = gaps.peek() {
            n = n.min((gap.at - popped) as usize);
        }
        for slot in buffer.iter_mut().take(n) {
            *slot = consumer.pop().unwrap_or(0.0);
        }
        popped += n as u64;
        super::note_level(&super::SYSTEM_LEVEL, &buffer[..n]);
        let measured = tracker.observe(std::time::Instant::now(), n);
        match measured {
            None => held.extend_from_slice(&buffer[..n]),
            Some(real) => {
                if logged_rate != Some(real) {
                    log::info!(
                        "System audio: samples arrive at {real} Hz (tap reported {rate} Hz)"
                    );
                    logged_rate = Some(real);
                }
                if !held.is_empty() || !held_gaps.is_empty() {
                    write_held(
                        &mut held,
                        &mut held_gaps,
                        real,
                        &mut resampler,
                        &mut writer,
                        &mut out,
                    )?;
                }
                out.clear();
                resampler.process(&buffer[..n], real, &mut out);
                writer.write_interleaved(&out, 1)?;
            }
        }
    }
    // Stopped before a rate could be measured (very short meeting): use the
    // reported rate for what was held.
    let in_rate = tracker.current().unwrap_or(rate);
    write_held(
        &mut held,
        &mut held_gaps,
        in_rate,
        &mut resampler,
        &mut writer,
        &mut out,
    )?;
    Ok(Some(writer.finish()?))
}

/// Writes the silence a rebuild's pause left (its length at `in_rate`,
/// minus the samples of the callback that ended it), a piece at a time so a
/// long pause never needs one large buffer. Returns the seconds written.
fn write_silence(
    gap: &Gap,
    in_rate: u32,
    resampler: &mut super::rate::Resampler,
    writer: &mut FlacWriter,
    out: &mut Vec<f32>,
) -> Result<f64> {
    const PIECE: usize = 4096;
    let zeros = [0f32; PIECE];
    let pad = (gap.secs.min(4.0 * 3600.0) * in_rate as f64).round() as i64 - gap.frames as i64;
    let mut left = pad.max(0) as usize;
    while left > 0 {
        let n = left.min(PIECE);
        out.clear();
        resampler.process(&zeros[..n], in_rate, out);
        writer.write_interleaved(out, 1)?;
        left -= n;
    }
    Ok(pad.max(0) as f64 / in_rate as f64)
}

/// Writes the audio held back before the rate was known, with the silence
/// of any rebuild pause in it, at `in_rate`.
fn write_held(
    held: &mut Vec<f32>,
    held_gaps: &mut Vec<(usize, Gap)>,
    in_rate: u32,
    resampler: &mut super::rate::Resampler,
    writer: &mut FlacWriter,
    out: &mut Vec<f32>,
) -> Result<()> {
    if held.is_empty() && held_gaps.is_empty() {
        return Ok(());
    }
    let mut from = 0;
    for (at, gap) in held_gaps.drain(..) {
        out.clear();
        resampler.process(&held[from..at], in_rate, out);
        writer.write_interleaved(out, 1)?;
        let secs = write_silence(&gap, in_rate, resampler, writer, out)?;
        log::info!("System audio: filled {secs:.2} s with silence where the tap was rebuilt");
        from = at;
    }
    out.clear();
    resampler.process(&held[from..], in_rate, out);
    writer.write_interleaved(out, 1)?;
    held.clear();
    Ok(())
}

/// Result of stopping the system-audio half.
#[derive(Debug)]
pub struct SystemRecording {
    /// `None` when the tap never delivered audio, so no file was written.
    pub path: Option<PathBuf>,
    pub frames: u64,
    pub sample_rate: Option<u32>,
    /// Samples lost because the writer fell behind.
    pub dropped_samples: u64,
}

/// What the watchdog needs from a tap: the real one or the test player.
trait Control: Send + Sync {
    /// Builds the tap anew. Err with the reason when that failed.
    fn rebuild(&self) -> Result<(), String>;
    /// The other apps playing audio now, `None` when unknown.
    fn others_playing(&self) -> Option<Vec<String>>;
    /// The default output device's id and name, `None` when there is none.
    fn default_output(&self) -> Option<(u32, String)>;
    /// Ends the tap: no callback runs once this returns.
    fn stop(&self);
}

/// The Core Audio tap, through the Swift C API.
struct RealTap {
    /// The Swift handle as an address; 0 once stopped.
    handle: Mutex<usize>,
}

impl Control for RealTap {
    fn rebuild(&self) -> Result<(), String> {
        let handle = self.handle.lock().unwrap_or_else(|e| e.into_inner());
        if *handle == 0 {
            return Err("the tap is stopped".into());
        }
        let mut error: *mut c_char = ptr::null_mut();
        // SAFETY: the handle is live (not stopped: we hold the lock), and
        // `shared` outlives it. No callback runs during the rebuild.
        let ok = unsafe { ffi::sokki_tap_rebuild(*handle as *mut c_void, &mut error) };
        if ok {
            Ok(())
        } else {
            Err(take_error(error))
        }
    }

    fn others_playing(&self) -> Option<Vec<String>> {
        let mut buffer = [0 as c_char; 512];
        // SAFETY: Swift writes at most `buffer.len()` bytes, NUL-terminated.
        let count = unsafe { ffi::sokki_tap_other_apps_playing(buffer.as_mut_ptr(), buffer.len()) };
        if count < 0 {
            return None;
        }
        let text = c_text(&buffer);
        Some(if count == 0 {
            Vec::new()
        } else {
            text.split(", ").map(str::to_string).collect()
        })
    }

    fn default_output(&self) -> Option<(u32, String)> {
        let mut buffer = [0 as c_char; 256];
        // SAFETY: as in `others_playing`.
        let id = unsafe { ffi::sokki_tap_default_output(buffer.as_mut_ptr(), buffer.len()) };
        (id != 0).then(|| (id, c_text(&buffer)))
    }

    fn stop(&self) {
        let mut handle = self.handle.lock().unwrap_or_else(|e| e.into_inner());
        if *handle != 0 {
            // SAFETY: a handle from `sokki_tap_start`, stopped exactly once.
            unsafe { ffi::sokki_tap_stop(*handle as *mut c_void) };
            *handle = 0;
        }
    }
}

fn c_text(buffer: &[c_char]) -> String {
    // SAFETY: Swift always NUL-terminates inside the buffer, and the buffer
    // starts zeroed.
    unsafe { CStr::from_ptr(buffer.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

/// When the watchdog rebuilds the tap after silence: 3 s of exact zeros,
/// then twice as long after each rebuild that brought no sound back (a call
/// app can play true digital silence), up to a minute.
fn quiet_limit_us(attempts: u32) -> u64 {
    (3_000_000u64 << attempts.min(5)).min(60_000_000)
}

/// Why the tap is rebuilt.
#[derive(Debug, Clone, PartialEq)]
enum Reason {
    /// Callbacks come, but only exact zeros, while other apps play.
    Zeros {
        secs: f64,
        apps: Vec<String>,
    },
    /// No callbacks at all while other apps play.
    NoAudio {
        secs: f64,
        apps: Vec<String>,
    },
    OutputChanged {
        from: String,
        to: String,
    },
}

impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let apps = |apps: &[String]| {
            if apps.is_empty() {
                "an app".to_string()
            } else {
                apps.join(", ")
            }
        };
        match self {
            Reason::Zeros { secs, apps: a } => {
                write!(
                    f,
                    "only exact zeros for {secs:.1} s while {} plays",
                    apps(a)
                )
            }
            Reason::NoAudio { secs, apps: a } => {
                write!(f, "no audio for {secs:.1} s while {} plays", apps(a))
            }
            Reason::OutputChanged { from, to } => {
                write!(f, "the default output changed from “{from}” to “{to}”")
            }
        }
    }
}

/// The watchdog's decisions, apart from the clock and Core Audio so they can
/// be tested. Times are [`super::mono_us`].
struct Watch {
    /// When the tap was last built.
    built_us: u64,
    /// Rebuilds after silence since the call track last had sound.
    attempts: u32,
    output: Option<(u32, String)>,
    /// The last rebuild failed, so no callbacks can come: try again even
    /// when Core Audio can't say whether something plays.
    failed: bool,
    /// The current silence was left alone because no other app played:
    /// noted once per silence, so a missed dead tap still shows in the log.
    left_alone: Option<u64>,
}

impl Watch {
    fn new(now_us: u64, output: Option<(u32, String)>) -> Self {
        Self {
            built_us: now_us,
            attempts: 0,
            output,
            failed: false,
            left_alone: None,
        }
    }

    /// True once, when sound came back after rebuilds.
    fn recovered(&mut self, sound_us: u64) -> bool {
        if self.left_alone.is_some_and(|since| sound_us > since) {
            self.left_alone = None;
        }
        if self.attempts > 0 && sound_us > self.built_us {
            self.attempts = 0;
            return true;
        }
        false
    }

    /// Seconds of silence, the first time a silence is left alone.
    fn note_left_alone(&mut self, now_us: u64, sound_us: u64) -> Option<f64> {
        let since = sound_us.max(self.built_us);
        if self.left_alone == Some(since) {
            return None;
        }
        self.left_alone = Some(since);
        Some(now_us.saturating_sub(since) as f64 / 1e6)
    }

    /// Whether to rebuild now. `playing` is asked only after a long silence.
    fn check(
        &mut self,
        now_us: u64,
        sound_us: u64,
        callback_us: u64,
        output: Option<(u32, String)>,
        playing: impl FnOnce() -> Option<Vec<String>>,
    ) -> Option<Reason> {
        if let Some(new) = output {
            match &self.output {
                Some(old) if old.0 != new.0 => {
                    let reason = Reason::OutputChanged {
                        from: old.1.clone(),
                        to: new.1.clone(),
                    };
                    self.output = Some(new);
                    return Some(reason);
                }
                Some(_) => {}
                None => self.output = Some(new),
            }
        }
        let quiet_us = now_us.saturating_sub(sound_us.max(self.built_us));
        if quiet_us < quiet_limit_us(self.attempts) {
            return None;
        }
        let calling_back = callback_us > 0 && now_us.saturating_sub(callback_us) < 1_000_000;
        let apps = match playing() {
            Some(apps) if !apps.is_empty() => apps,
            // Unknown: callbacks only start once something plays.
            None if calling_back || self.failed => Vec::new(),
            _ => return None,
        };
        let secs = quiet_us as f64 / 1e6;
        Some(if calling_back {
            Reason::Zeros { secs, apps }
        } else {
            Reason::NoAudio { secs, apps }
        })
    }

    /// The tap was just rebuilt (`ok`) or a rebuild failed, for `reason`.
    fn rebuilt(&mut self, now_us: u64, reason: &Reason, ok: bool) {
        self.built_us = now_us;
        self.failed = !ok;
        if !matches!(reason, Reason::OutputChanged { .. }) {
            self.attempts += 1;
        }
    }
}

const WATCH_EVERY: Duration = Duration::from_millis(500);

fn watchdog(control: Arc<dyn Control>, rebuilds: Arc<AtomicU64>, stop: Arc<AtomicBool>) {
    let output = control.default_output();
    match &output {
        Some((id, name)) => log::info!("System audio: default output is “{name}” (device {id})"),
        None => log::warn!("System audio: no default output device"),
    }
    let mut watch = Watch::new(super::mono_us(), output);
    while !stop.load(Ordering::Acquire) {
        thread::park_timeout(WATCH_EVERY);
        if stop.load(Ordering::Acquire) {
            break;
        }
        let now = super::mono_us();
        let sound = super::SYSTEM_SOUND_US.load(Ordering::Relaxed);
        if watch.recovered(sound) {
            log::info!("System audio: the call track has sound again after the rebuild");
        }
        let callback = super::SYSTEM_CALLBACK_US.load(Ordering::Relaxed);
        let mut asked = false;
        let decision = watch.check(now, sound, callback, control.default_output(), || {
            asked = true;
            control.others_playing()
        });
        let Some(reason) = decision else {
            if asked {
                if let Some(secs) = watch.note_left_alone(now, sound) {
                    log::info!(
                        "System audio: nothing on the call track for {secs:.1} s, but no other app plays; leaving the tap alone"
                    );
                }
            }
            continue;
        };
        let n = rebuilds.fetch_add(1, Ordering::AcqRel) + 1;
        log::warn!("System audio: rebuilding the tap (rebuild {n}): {reason}");
        let began = Instant::now();
        let result = control.rebuild();
        match &result {
            Ok(()) => log::info!(
                "System audio: tap rebuilt in {} ms",
                began.elapsed().as_millis()
            ),
            Err(e) => log::warn!("System audio: rebuilding the tap failed: {e}"),
        }
        watch.rebuilt(super::mono_us(), &reason, result.is_ok());
    }
}

pub struct SystemAudioCapture {
    /// The tap (or test player); `None` once stopped.
    control: Option<Arc<dyn Control>>,
    /// Owned via `Box::into_raw`; freed only after the tap has stopped.
    shared: *mut TapShared,
    stop: Arc<AtomicBool>,
    sample_rate: Arc<OnceLock<u32>>,
    writer: Option<JoinHandle<Result<Option<u64>>>>,
    watchdog: Option<(Arc<AtomicBool>, JoinHandle<()>)>,
    rebuilds: Arc<AtomicU64>,
    path: PathBuf,
}

// SAFETY: the Swift handle is used only through the C API, behind a mutex,
// and its callback only touches `shared` while the tap is running. Methods
// take `&mut self` or consume `self`, and `stop` ends the tap before
// anything is dropped.
unsafe impl Send for SystemAudioCapture {}

/// True when this macOS can capture system audio (14.2+).
pub fn is_supported() -> bool {
    // SAFETY: a plain query with no arguments.
    unsafe { ffi::sokki_tap_supported() }
}

impl SystemAudioCapture {
    /// Starts the tap and a writer thread that streams into `path`.
    /// Fails when the OS is too old or the system-audio permission is denied.
    /// `tee` gets the audio as it is written (see [`super::flac::Tee`]).
    pub fn start(path: PathBuf, tee: Option<super::flac::Tee>) -> Result<Self> {
        // Test mode's file, or the sample meeting's call.
        let test_audio = crate::test_mic::system_audio().or_else(super::sample::take_call);
        if !is_supported() && test_audio.is_none() {
            bail!("System audio capture needs macOS 14.2 or later.");
        }

        // Starts the clock here, not on the real-time thread.
        let _ = super::mono_us();
        let (producer, consumer) = rtrb::RingBuffer::<f32>::new(RING_CAPACITY);
        let (gap_producer, gap_consumer) = rtrb::RingBuffer::<Gap>::new(64);
        let sample_rate = Arc::new(OnceLock::new());
        let rebuilds = Arc::new(AtomicU64::new(0));
        let shared = Box::into_raw(Box::new(TapShared {
            producer,
            gaps: gap_producer,
            sample_rate: Arc::clone(&sample_rate),
            dropped_samples: 0,
            pushed: 0,
            last_callback: None,
            rebuilds: Arc::clone(&rebuilds),
            rebuilds_seen: 0,
        }));

        let user = shared.cast::<c_void>();
        let control: Arc<dyn Control> = if let Some(audio) = test_audio {
            Arc::new(FakeTap::start(user, audio))
        } else {
            let mut error: *mut c_char = ptr::null_mut();
            // SAFETY: `user` stays valid until the tap is stopped and `shared` freed.
            let handle = unsafe { ffi::sokki_tap_start(on_samples, user, &mut error) };
            if handle.is_null() {
                // SAFETY: the tap never started, so nothing else holds `shared`.
                unsafe { drop(Box::from_raw(shared)) };
                let message = take_error(error);
                log::warn!("System audio tap failed to start: {message}");
                bail!("{message}");
            }
            Arc::new(RealTap {
                handle: Mutex::new(handle as usize),
            })
        };
        log::info!("System audio tap started");

        let stop = Arc::new(AtomicBool::new(false));
        let writer = {
            let stop = Arc::clone(&stop);
            let sample_rate = Arc::clone(&sample_rate);
            let path = path.clone();
            thread::Builder::new()
                .name("meeting-system-writer".into())
                .spawn(move || write_loop(consumer, gap_consumer, sample_rate, path, stop, tee))
        };
        let writer = match writer {
            Ok(writer) => writer,
            Err(e) => {
                control.stop();
                // SAFETY: the tap has stopped.
                unsafe { drop(Box::from_raw(shared)) };
                bail!("Starting the system audio writer failed: {e}");
            }
        };
        let watchdog = {
            let halt = Arc::new(AtomicBool::new(false));
            let (control, rebuilds, halting) = (
                Arc::clone(&control),
                Arc::clone(&rebuilds),
                Arc::clone(&halt),
            );
            thread::Builder::new()
                .name("meeting-tap-watchdog".into())
                .spawn(move || watchdog(control, rebuilds, halting))
                .map_err(|e| log::warn!("System audio: no watchdog for the tap: {e}"))
                .ok()
                .map(|thread| (halt, thread))
        };

        Ok(Self {
            control: Some(control),
            shared,
            stop,
            sample_rate,
            writer: Some(writer),
            watchdog,
            rebuilds,
            path,
        })
    }

    /// Stops the tap, drains the buffer and finalizes the file.
    pub fn stop(mut self) -> Result<SystemRecording> {
        self.stop_tap();
        self.stop.store(true, Ordering::Release);

        // SAFETY: the tap has stopped, so the callback no longer touches `shared`.
        let dropped_samples = unsafe { (*self.shared).dropped_samples };
        let writer = self.writer.take().expect("writer is joined only once");
        let frames = match writer.join() {
            Ok(result) => result?,
            Err(_) => bail!("The system audio writer thread panicked"),
        };
        self.free_shared();
        log::info!(
            "System audio stopped: {} frames, {} dropped samples, {} rebuilds",
            frames.unwrap_or(0),
            dropped_samples,
            self.rebuilds.load(Ordering::Relaxed)
        );

        Ok(SystemRecording {
            path: frames.map(|_| self.path.clone()),
            frames: frames.unwrap_or(0),
            sample_rate: self.sample_rate.get().copied(),
            dropped_samples,
        })
    }
}

impl SystemAudioCapture {
    /// Ends the watchdog, then the tap: no callback runs after this.
    fn stop_tap(&mut self) {
        if let Some((halt, thread)) = self.watchdog.take() {
            halt.store(true, Ordering::Release);
            thread.thread().unpark();
            let _ = thread.join();
        }
        if let Some(control) = self.control.take() {
            control.stop();
        }
    }

    /// Frees `shared`. Only call once the tap has stopped.
    fn free_shared(&mut self) {
        if !self.shared.is_null() {
            // SAFETY: created by `Box::into_raw` in `start`; the tap is stopped.
            unsafe { drop(Box::from_raw(self.shared)) };
            self.shared = ptr::null_mut();
        }
    }
}

impl Drop for SystemAudioCapture {
    fn drop(&mut self) {
        // Not stopped explicitly (for example, the app is shutting down). End
        // the tap before freeing `shared`, so the callback never sees freed
        // memory. The writer thread is left to finish on its own.
        if self.control.is_some() {
            self.stop_tap();
            self.stop.store(true, Ordering::Release);
        }
        self.free_shared();
    }
}

/// Test mode: plays a file into the tap's callback in place of Core Audio.
///
/// `SOKKI_TEST_SYSTEM_DEAD_AT_S=<s>`: from then on the tap goes dead like on
/// 2026-11-24 (exact zeros while the file plays on) until it is rebuilt;
/// a rebuild pauses the callbacks for 200 ms, the file playing on.
/// `SOKKI_TEST_OUTPUT_CHANGE_AT_S=<s>`: the default output changes then.
struct FakeTap {
    halt: Arc<AtomicBool>,
    dead: Arc<AtomicBool>,
    /// Milliseconds the player pauses before its next callback.
    pause_ms: Arc<AtomicU64>,
    /// The file still has audio to play.
    playing: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
    began: Instant,
    output_change_at: Option<Duration>,
}

impl FakeTap {
    fn start(user: *mut c_void, audio: Vec<f32>) -> Self {
        let secs = |name: &str| {
            std::env::var(name)
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .map(Duration::from_secs_f64)
        };
        let halt = Arc::new(AtomicBool::new(false));
        let dead = Arc::new(AtomicBool::new(false));
        let pause_ms = Arc::new(AtomicU64::new(0));
        let playing = Arc::new(AtomicBool::new(false));
        let thread = play_test_audio(
            user,
            audio,
            Player {
                halt: Arc::clone(&halt),
                dead: Arc::clone(&dead),
                dead_at: secs("SOKKI_TEST_SYSTEM_DEAD_AT_S"),
                pause_ms: Arc::clone(&pause_ms),
                playing: Arc::clone(&playing),
            },
        );
        Self {
            halt,
            dead,
            pause_ms,
            playing,
            thread: Mutex::new(Some(thread)),
            began: Instant::now(),
            output_change_at: secs("SOKKI_TEST_OUTPUT_CHANGE_AT_S"),
        }
    }
}

impl Control for FakeTap {
    fn rebuild(&self) -> Result<(), String> {
        self.pause_ms.store(200, Ordering::Release);
        self.dead.store(false, Ordering::Release);
        Ok(())
    }

    fn others_playing(&self) -> Option<Vec<String>> {
        Some(if self.playing.load(Ordering::Acquire) {
            vec!["test player".into()]
        } else {
            Vec::new()
        })
    }

    fn default_output(&self) -> Option<(u32, String)> {
        Some(match self.output_change_at {
            Some(at) if self.began.elapsed() >= at => (2, "Test output 2".into()),
            _ => (1, "Test output 1".into()),
        })
    }

    fn stop(&self) {
        self.halt.store(true, Ordering::Release);
        let thread = self.thread.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(thread) = thread {
            let _ = thread.join();
        }
    }
}

struct Player {
    halt: Arc<AtomicBool>,
    dead: Arc<AtomicBool>,
    dead_at: Option<Duration>,
    pause_ms: Arc<AtomicU64>,
    playing: Arc<AtomicBool>,
}

/// Test mode: feeds `audio` (16 kHz mono) to the tap's callback in real
/// time, 10 ms at a time, then silence, as Core Audio would.
fn play_test_audio(user: *mut c_void, audio: Vec<f32>, player: Player) -> JoinHandle<()> {
    struct User(*mut c_void);
    // SAFETY: the pointer is only used by this thread, which `FakeTap::stop`
    // joins before `shared` is freed.
    unsafe impl Send for User {}
    let user = User(user);
    // SOKKI_TEST_SYSTEM_DELAY_S: nothing plays at first, as in an in-person
    // meeting where the tap's first sample came 12 s after the mic's.
    let delay = std::env::var("SOKKI_TEST_SYSTEM_DELAY_S")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0);
    thread::spawn(move || {
        let user = user;
        let stopping = &player.halt;
        let wait_until = Instant::now() + Duration::from_secs_f64(delay);
        while Instant::now() < wait_until {
            if stopping.load(Ordering::Acquire) {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let block = 160;
        let silence = vec![0.0f32; block];
        let started = Instant::now();
        let mut sent = 0usize;
        let mut went_dead = false;
        while !stopping.load(Ordering::Acquire) {
            player.playing.store(sent < audio.len(), Ordering::Release);
            if !went_dead && player.dead_at.is_some_and(|at| started.elapsed() >= at) {
                went_dead = true;
                player.dead.store(true, Ordering::Release);
                log::info!("Test mode: the system audio tap goes dead now");
            }
            let pause = player.pause_ms.swap(0, Ordering::AcqRel);
            if pause > 0 {
                // A rebuild: no callbacks for a moment; the file plays on.
                thread::sleep(Duration::from_millis(pause));
                sent = ((started.elapsed().as_secs_f64() * 16_000.0) as usize / block) * block;
            }
            let piece = match audio.get(sent..(sent + block).min(audio.len())) {
                Some(p) if !p.is_empty() && !player.dead.load(Ordering::Acquire) => p,
                _ => &silence[..],
            };
            on_samples(user.0, piece.as_ptr(), piece.len(), 1, 16_000.0);
            sent += piece.len();
            let due = started + Duration::from_secs_f64(sent as f64 / 16_000.0);
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
        }
    })
}

/// Reads and frees the Swift error string.
fn take_error(error: *mut c_char) -> String {
    if error.is_null() {
        return "System audio capture failed.".to_string();
    }
    // SAFETY: Swift returns a valid NUL-terminated string from `strdup`.
    let message = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: the string came from Swift and is freed exactly once.
    unsafe { ffi::sokki_tap_free_error(error) };
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;
    fn chrome() -> Option<Vec<String>> {
        Some(vec!["com.google.Chrome.helper".into()])
    }
    fn out(id: u32) -> Option<(u32, String)> {
        Some((id, format!("Output {id}")))
    }

    #[test]
    fn rebuilds_after_three_seconds_of_zeros_then_backs_off() {
        let mut w = Watch::new(0, out(1));
        // Sound until 10 s, then the tap calls back with exact zeros.
        let sound = 10 * S;
        assert_eq!(w.check(12 * S, sound, 12 * S, out(1), chrome), None);
        let reason = w.check(13 * S, sound, 13 * S, out(1), chrome).unwrap();
        assert!(matches!(reason, Reason::Zeros { .. }), "{reason:?}");
        assert_eq!(
            reason.to_string(),
            "only exact zeros for 3.0 s while com.google.Chrome.helper plays"
        );
        w.rebuilt(13 * S, &reason, true);
        // Still zeros: the next try waits twice as long.
        assert_eq!(w.check(18 * S, sound, 18 * S, out(1), chrome), None);
        assert!(w.check(19 * S, sound, 19 * S, out(1), chrome).is_some());
        w.rebuilt(19 * S, &reason, true);
        assert_eq!(w.attempts, 2);
        // Sound is back: the next dead stretch is caught after 3 s again.
        assert!(w.recovered(20 * S));
        assert!(!w.recovered(20 * S));
        assert!(w.check(23 * S, 20 * S, 23 * S, out(1), chrome).is_some());
    }

    #[test]
    fn the_wait_stops_growing_at_a_minute() {
        assert_eq!(quiet_limit_us(0), 3 * S);
        assert_eq!(quiet_limit_us(3), 24 * S);
        assert_eq!(quiet_limit_us(5), 60 * S);
        assert_eq!(quiet_limit_us(40), 60 * S);
    }

    #[test]
    fn leaves_the_tap_alone_while_nothing_plays() {
        let mut w = Watch::new(0, out(1));
        let mut asked = 0;
        assert_eq!(
            w.check(2 * S, 0, 0, out(1), || {
                asked += 1;
                chrome()
            }),
            None
        );
        assert_eq!(asked, 0, "asks only after a long silence");
        assert_eq!(w.check(30 * S, 0, 0, out(1), || Some(Vec::new())), None);
        assert_eq!(w.note_left_alone(30 * S, 0), Some(30.0));
        assert_eq!(w.note_left_alone(31 * S, 0), None, "once per silence");
        w.recovered(32 * S);
        assert_eq!(w.note_left_alone(40 * S, 32 * S), Some(8.0));
        // Unknown and no callbacks: nothing to go by.
        assert_eq!(w.check(30 * S, 0, 0, out(1), || None), None);
        // Unknown, but callbacks come: something plays.
        assert!(w.check(30 * S, 0, 30 * S, out(1), || None).is_some());
        // A failed rebuild tore the tap down: try again even when unknown.
        let reason = Reason::NoAudio {
            secs: 3.0,
            apps: Vec::new(),
        };
        w.rebuilt(30 * S, &reason, false);
        assert!(w.check(36 * S, 0, 0, out(1), || None).is_some());
        w.rebuilt(36 * S, &reason, true);
        assert_eq!(w.check(60 * S, 0, 0, out(1), || None), None);
    }

    #[test]
    fn rebuilds_when_nothing_arrives_while_an_app_plays() {
        let mut w = Watch::new(0, out(1));
        let reason = w.check(4 * S, 0, 0, out(1), chrome).unwrap();
        assert!(matches!(reason, Reason::NoAudio { .. }), "{reason:?}");
    }

    #[test]
    fn rebuilds_at_once_when_the_output_changes() {
        let mut w = Watch::new(0, out(1));
        let reason = w.check(S, S, S, out(2), chrome).unwrap();
        assert_eq!(
            reason.to_string(),
            "the default output changed from “Output 1” to “Output 2”"
        );
        w.rebuilt(S, &reason, true);
        assert_eq!(w.attempts, 0);
        assert_eq!(w.check(2 * S, 2 * S, 2 * S, out(2), chrome), None);
        // No output for a moment, then the same one: no rebuild.
        assert_eq!(w.check(3 * S, 3 * S, 3 * S, None, chrome), None);
        assert_eq!(w.check(4 * S, 4 * S, 4 * S, out(2), chrome), None);
    }

    /// Plays `before` s, pauses `pause_ms` after a rebuild, plays `after`
    /// s, through the callback and the writer, the callback reporting
    /// `reported` Hz while samples come at 16 kHz (a Bluetooth headset in
    /// a call). Returns the frames written at 16 kHz.
    fn frames_across_a_rebuild(
        name: &str,
        reported: f64,
        before: f64,
        pause_ms: u64,
        after: f64,
    ) -> u64 {
        let dir =
            std::env::temp_dir().join(format!("sokki-gap-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("system.flac");
        let (producer, consumer) = rtrb::RingBuffer::<f32>::new(RING_CAPACITY);
        let (gap_producer, gap_consumer) = rtrb::RingBuffer::<Gap>::new(64);
        let sample_rate = Arc::new(OnceLock::new());
        let rebuilds = Arc::new(AtomicU64::new(0));
        let mut shared = Box::new(TapShared {
            producer,
            gaps: gap_producer,
            sample_rate: Arc::clone(&sample_rate),
            dropped_samples: 0,
            pushed: 0,
            last_callback: None,
            rebuilds: Arc::clone(&rebuilds),
            rebuilds_seen: 0,
        });
        let stop = Arc::new(AtomicBool::new(false));
        let writer = {
            let (sample_rate, path, stop) =
                (Arc::clone(&sample_rate), path.clone(), Arc::clone(&stop));
            thread::spawn(move || write_loop(consumer, gap_consumer, sample_rate, path, stop, None))
        };
        let user = (&mut *shared as *mut TapShared).cast::<c_void>();
        let chunk = vec![0.25f32; 160];
        let play = |secs: f64| {
            let began = Instant::now();
            let mut sent = 0usize;
            while (sent as f64) < secs * 16_000.0 {
                on_samples(user, chunk.as_ptr(), 160, 1, reported);
                sent += 160;
                let due = began + Duration::from_secs_f64(sent as f64 / 16_000.0);
                if let Some(wait) = due.checked_duration_since(Instant::now()) {
                    thread::sleep(wait);
                }
            }
        };
        play(before);
        rebuilds.fetch_add(1, Ordering::AcqRel);
        thread::sleep(Duration::from_millis(pause_ms));
        play(after);
        stop.store(true, Ordering::Release);
        let frames = writer.join().unwrap().unwrap().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        frames
    }

    fn assert_about(frames: u64, secs: f64) {
        let expected = secs * 16_000.0;
        assert!(
            (frames as f64 - expected).abs() < 0.03 * 16_000.0,
            "{frames} frames, expected about {expected}"
        );
    }

    /// 2 s of audio, a 300 ms pause, 1 s more: the pause stays as silence.
    #[test]
    fn a_rebuild_pause_is_filled_with_silence() {
        assert_about(
            frames_across_a_rebuild("measured", 16_000.0, 2.0, 300, 1.0),
            3.3,
        );
    }

    /// A rebuild 0.5 s in, before the real rate is known: the silence is
    /// sized at the measured 16 kHz, not the 48 kHz the tap reports.
    #[test]
    fn a_pause_before_the_rate_is_known_uses_the_measured_rate() {
        assert_about(
            frames_across_a_rebuild("early", 48_000.0, 0.5, 300, 2.0),
            2.8,
        );
    }
}

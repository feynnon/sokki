//! "Sokki hears nothing": a hint while a meeting records and a track brings
//! no speech. On 2026-11-25 Robin's microphone was muted for a whole
//! 19-minute meeting; only the flat level bars showed it, and nobody looked.
//!
//! Each track's FLAC writer also feeds a small voice detector (Earshot, the
//! one dictation uses), which marks when the track last had speech and when
//! it last had any sample that was not exactly zero. The indicator's ticker
//! asks [`Watch`] once per tick what to show:
//!
//! - **Microphone** ([`Hint::Mic`]): no speech on the microphone for
//!   [`AFTER_US`], and either no speech on the call track either, or the
//!   microphone sent only exact zeros (muted, or a device that delivers
//!   nothing). Someone quietly listening to a call has speech on the call
//!   track and noise on the microphone, so it doesn't fire for them.
//! - **Call** ([`Hint::Call`]): a call app was detected and the call track
//!   has had only exact zeros or no audio at all for [`AFTER_US`]. A quiet
//!   call still has noise in it, so this means Sokki hears nothing, not that
//!   the other side is silent.
//!
//! One hint at a time, the microphone first. Each shows at most once per
//! recording: it goes when its reason goes (speech arrives) and doesn't
//! come back. Switch: `meetings.silence_hint_enabled`.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::audio_toolkit::audio::LiveResampler;
use crate::audio_toolkit::vad::{frames_for_duration_ms, VAD_ONSET_MS};
use crate::audio_toolkit::{EarshotVad, VoiceActivityDetector};

/// How long a track has to be without speech (or sound) before the hint
/// shows. The brief asked for 30 to 45 s; 40 s leaves room for a slow start
/// ("can you hear me?") without letting a muted microphone go unnoticed.
pub const AFTER_US: u64 = 40_000_000;

/// The same threshold dictation uses for Earshot.
const VAD_THRESHOLD: f32 = 0.5;

/// When each track last had speech and last had a sample that was not
/// exactly zero ([`super::mono_us`]; 0 = not yet in this meeting). The call
/// track's sound mark is [`super::SYSTEM_SOUND_US`], set by the tap.
pub static MIC_SPEECH_US: AtomicU64 = AtomicU64::new(0);
pub static MIC_SOUND_US: AtomicU64 = AtomicU64::new(0);
pub static SYSTEM_SPEECH_US: AtomicU64 = AtomicU64::new(0);

/// Clears the marks at the start of a meeting.
pub fn reset() {
    for mark in [&MIC_SPEECH_US, &MIC_SOUND_US, &SYSTEM_SPEECH_US] {
        mark.store(0, Ordering::Relaxed);
    }
}

/// What the recording bar shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Hint {
    Mic,
    Call,
}

/// The marks [`Watch::tick`] decides on, all [`super::mono_us`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Marks {
    pub mic_speech_us: u64,
    pub mic_sound_us: u64,
    pub call_speech_us: u64,
    pub call_sound_us: u64,
}

impl Marks {
    pub fn now() -> Self {
        Self {
            mic_speech_us: MIC_SPEECH_US.load(Ordering::Relaxed),
            mic_sound_us: MIC_SOUND_US.load(Ordering::Relaxed),
            call_speech_us: SYSTEM_SPEECH_US.load(Ordering::Relaxed),
            call_sound_us: super::SYSTEM_SOUND_US.load(Ordering::Relaxed),
        }
    }
}

/// Decides the hint for one recording.
pub struct Watch {
    started_us: u64,
    /// A call app was detected (the meeting has a source app).
    call: bool,
    enabled: bool,
    showing: Option<Hint>,
    mic_done: bool,
    call_done: bool,
}

/// What changed in one tick, for the log and the notification.
#[derive(Debug, PartialEq, Eq)]
pub enum Change {
    Shown(Hint),
    Gone(Hint),
}

impl Watch {
    pub fn new(started_us: u64, call: bool, enabled: bool) -> Self {
        Self {
            started_us,
            call,
            enabled,
            showing: None,
            mic_done: false,
            call_done: false,
        }
    }

    pub fn showing(&self) -> Option<Hint> {
        self.showing
    }

    /// How long ago `mark` was, counting from the start when it is older.
    fn since(&self, now_us: u64, mark: u64) -> u64 {
        now_us.saturating_sub(mark.max(self.started_us))
    }

    fn holds(&self, hint: Hint, now_us: u64, m: &Marks) -> bool {
        let quiet = |mark| self.since(now_us, mark) >= AFTER_US;
        match hint {
            Hint::Mic => {
                quiet(m.mic_speech_us) && (quiet(m.call_speech_us) || quiet(m.mic_sound_us))
            }
            Hint::Call => self.call && quiet(m.call_sound_us),
        }
    }

    /// One tick; returns what changed.
    pub fn tick(&mut self, now_us: u64, m: &Marks) -> Option<Change> {
        if !self.enabled {
            return None;
        }
        if let Some(hint) = self.showing {
            if self.holds(hint, now_us, m) {
                return None;
            }
            self.showing = None;
            match hint {
                Hint::Mic => self.mic_done = true,
                Hint::Call => self.call_done = true,
            }
            return Some(Change::Gone(hint));
        }
        let next = [(Hint::Mic, self.mic_done), (Hint::Call, self.call_done)]
            .into_iter()
            .find(|&(hint, done)| !done && self.holds(hint, now_us, m))
            .map(|(hint, _)| hint)?;
        self.showing = Some(next);
        Some(Change::Shown(next))
    }
}

/// The writer's tee for one track: resamples to 16 kHz and marks speech
/// (Earshot, [`VAD_ONSET_MS`] of speech in a row) and non-zero samples.
/// `sound`: the track's sound mark, or `None` where the tap sets it.
pub fn tee(speech: &'static AtomicU64, sound: Option<&'static AtomicU64>) -> super::flac::Tee {
    let mut vad = match EarshotVad::new(VAD_THRESHOLD) {
        Ok(vad) => Some(vad),
        Err(e) => {
            log::warn!("Silence hint: no voice detector ({e:#})");
            None
        }
    };
    let frame_len = vad.as_ref().map_or(256, |v| v.frame_samples());
    let onset = frames_for_duration_ms(VAD_ONSET_MS, frame_len);
    let mut resampler: Option<(u32, LiveResampler)> = None;
    let mut frame: Vec<f32> = Vec::with_capacity(frame_len);
    let mut run = 0usize;
    Box::new(move |samples: &[f32], rate: u32| {
        if let Some(sound) = sound {
            if samples.iter().any(|&s| s != 0.0) {
                sound.store(super::mono_us(), Ordering::Relaxed);
            }
        }
        let Some(detector) = vad.as_mut() else {
            return;
        };
        let resampled;
        let at_16k: &[f32] = if rate == meeting_core::SAMPLE_RATE {
            samples
        } else {
            if resampler.as_ref().is_none_or(|(r, _)| *r != rate) {
                match LiveResampler::new(rate) {
                    Ok(r) => resampler = Some((rate, r)),
                    Err(e) => {
                        log::warn!("Silence hint: can't resample {rate} Hz: {e:#}");
                        vad = None;
                        return;
                    }
                }
            }
            let (_, r) = resampler.as_mut().expect("set above");
            resampled = match r.push(samples) {
                Ok(out) => out,
                Err(e) => {
                    log::warn!("Silence hint: resampling failed: {e:#}");
                    vad = None;
                    return;
                }
            };
            &resampled
        };
        for &s in at_16k {
            frame.push(s);
            if frame.len() < frame_len {
                continue;
            }
            let speech_frame = detector
                .push_frame(&frame)
                .map(|f| f.is_speech())
                .unwrap_or(false);
            frame.clear();
            run = if speech_frame { run + 1 } else { 0 };
            if run >= onset {
                speech.store(super::mono_us(), Ordering::Relaxed);
            }
        }
    })
}

/// Joins two tees: both get every block.
pub fn both(first: Option<super::flac::Tee>, mut second: super::flac::Tee) -> super::flac::Tee {
    match first {
        None => second,
        Some(mut first) => Box::new(move |samples: &[f32], rate: u32| {
            first(samples, rate);
            second(samples, rate);
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;

    fn marks(mic_speech: u64, mic_sound: u64, call_speech: u64, call_sound: u64) -> Marks {
        Marks {
            mic_speech_us: mic_speech * S,
            mic_sound_us: mic_sound * S,
            call_speech_us: call_speech * S,
            call_sound_us: call_sound * S,
        }
    }

    /// Ticks once a second from `from` to `to` s with fixed marks; returns
    /// the changes with their second.
    fn run(w: &mut Watch, from: u64, to: u64, m: Marks) -> Vec<(u64, Change)> {
        (from..=to)
            .filter_map(|s| w.tick(s * S, &m).map(|c| (s, c)))
            .collect()
    }

    #[test]
    fn a_silent_room_shows_the_mic_hint_after_40_s_and_hides_it_on_speech() {
        // Robin's meeting: no call app, no call audio, the microphone has
        // room noise but nobody's speech on it.
        let mut w = Watch::new(0, false, true);
        let changes = run(&mut w, 0, 60, marks(0, 60, 0, 0));
        assert_eq!(changes, vec![(40, Change::Shown(Hint::Mic))]);
        let back = marks(61, 61, 0, 0);
        assert_eq!(w.tick(61 * S, &back), Some(Change::Gone(Hint::Mic)));
        // Once per recording: another long silence doesn't bring it back.
        assert!(run(&mut w, 62, 300, marks(61, 300, 0, 0)).is_empty());
    }

    #[test]
    fn quietly_listening_to_a_call_shows_nothing() {
        // The others speak on the call; the microphone hears only the room.
        let mut w = Watch::new(0, true, true);
        for s in 0..=300 {
            assert_eq!(w.tick(s * S, &marks(0, s, s, s)), None, "at {s} s");
        }
    }

    #[test]
    fn a_muted_mic_in_a_call_shows_the_mic_hint_even_while_others_speak() {
        let mut w = Watch::new(0, true, true);
        let changes: Vec<_> = (0..=60)
            .filter_map(|s| w.tick(s * S, &marks(0, 0, s, s)).map(|c| (s, c)))
            .collect();
        assert_eq!(changes, vec![(40, Change::Shown(Hint::Mic))]);
    }

    #[test]
    fn a_dead_call_shows_the_call_hint_once_the_mic_is_fine() {
        // The microphone hears the user, the call track brings nothing.
        let mut w = Watch::new(0, true, true);
        let changes: Vec<_> = (0..=60)
            .filter_map(|s| w.tick(s * S, &marks(s, s, 0, 0)).map(|c| (s, c)))
            .collect();
        assert_eq!(changes, vec![(40, Change::Shown(Hint::Call))]);
        assert_eq!(
            w.tick(61 * S, &marks(61, 61, 61, 61)),
            Some(Change::Gone(Hint::Call))
        );
    }

    #[test]
    fn without_a_call_app_a_silent_call_track_is_no_hint() {
        let mut w = Watch::new(0, false, true);
        for s in 0..=120 {
            assert_eq!(w.tick(s * S, &marks(s, s, 0, 0)), None);
        }
    }

    #[test]
    fn nothing_at_all_shows_the_mic_hint_first_then_the_call_hint() {
        let mut w = Watch::new(0, true, true);
        assert_eq!(
            run(&mut w, 0, 50, marks(0, 0, 0, 0)),
            vec![(40, Change::Shown(Hint::Mic))]
        );
        // The user speaks: the mic hint goes, the dead call is next.
        let m = marks(51, 51, 0, 0);
        assert_eq!(w.tick(51 * S, &m), Some(Change::Gone(Hint::Mic)));
        assert_eq!(w.tick(52 * S, &m), Some(Change::Shown(Hint::Call)));
    }

    #[test]
    fn switched_off_shows_nothing() {
        let mut w = Watch::new(0, true, false);
        assert!(run(&mut w, 0, 120, marks(0, 0, 0, 0)).is_empty());
    }

    #[test]
    fn marks_from_before_the_start_count_from_the_start() {
        let mut w = Watch::new(100 * S, false, true);
        assert!(run(&mut w, 100, 139, marks(5, 5, 5, 5)).is_empty());
        assert_eq!(
            w.tick(140 * S, &marks(5, 5, 5, 5)),
            Some(Change::Shown(Hint::Mic))
        );
    }

    /// The tee marks speech in a voice and only sound in steady noise.
    #[test]
    fn the_tee_tells_speech_from_noise_and_zeros() {
        static SPEECH: AtomicU64 = AtomicU64::new(0);
        static SOUND: AtomicU64 = AtomicU64::new(0);
        let _ = super::super::mono_us();
        let rate = 48_000u32;
        // Exact zeros: neither mark.
        let mut tee = tee(&SPEECH, Some(&SOUND));
        for _ in 0..50 {
            tee(&[0.0; 960], rate);
        }
        assert_eq!(SPEECH.load(Ordering::Relaxed), 0);
        assert_eq!(SOUND.load(Ordering::Relaxed), 0);
        // Faint white noise: sound, no speech.
        let mut seed = 1u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 * 2e-3 - 1e-3
        };
        for _ in 0..100 {
            let block: Vec<f32> = (0..960).map(|_| noise()).collect();
            tee(&block, rate);
        }
        assert!(SOUND.load(Ordering::Relaxed) > 0);
        assert_eq!(SPEECH.load(Ordering::Relaxed), 0, "noise counted as speech");
    }
}

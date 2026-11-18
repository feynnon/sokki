//! Measuring the real sample rate of the system-audio tap, and resampling it
//! to 16 kHz.
//!
//! The tap reports the rate of its format (e.g. 48 kHz), but the audio
//! arrives at the output device's rate. During a call on a Bluetooth headset
//! macOS switches the headset to its telephony mode (16 kHz), so the samples
//! arrive three times slower than reported. Writing them with the reported
//! rate made the track three times too short and unusable (seen in a real
//! call on 2026-11-17). So the writer measures how many samples arrive per
//! second, snaps that to a standard rate, and resamples to 16 kHz itself. A
//! rate change mid-meeting (headset connected or disconnected) is followed.

use std::time::{Duration, Instant};

pub const OUT_RATE: u32 = 16_000;
const STANDARD_RATES: [u32; 8] = [
    8_000, 11_025, 16_000, 22_050, 24_000, 32_000, 44_100, 48_000,
];
/// Window over which the arrival rate is measured.
const WINDOW: Duration = Duration::from_millis(2_000);

/// Estimates the arrival rate from (samples, time) observations.
pub struct RateTracker {
    window: std::collections::VecDeque<(Instant, usize)>,
    current: Option<u32>,
    /// A different rate seen in consecutive windows, before switching to it.
    candidate: Option<(u32, u32)>,
}

impl RateTracker {
    pub fn new() -> Self {
        Self {
            window: Default::default(),
            current: None,
            candidate: None,
        }
    }

    /// Records `samples` arriving at `at`. Returns the current estimate.
    pub fn observe(&mut self, at: Instant, samples: usize) -> Option<u32> {
        self.window.push_back((at, samples));
        while let Some(&(t, _)) = self.window.front() {
            if at.duration_since(t) > WINDOW {
                self.window.pop_front();
            } else {
                break;
            }
        }
        let first = self.window.front()?.0;
        let span = at.duration_since(first).as_secs_f64();
        if span < WINDOW.as_secs_f64() * 0.75 {
            return self.current;
        }
        // Samples that arrived after the first observation, over the span.
        let n: usize = self.window.iter().skip(1).map(|(_, s)| s).sum();
        let measured = n as f64 / span;
        let Some(snapped) = snap(measured) else {
            return self.current;
        };
        match self.current {
            None => self.current = Some(snapped),
            Some(c) if c == snapped => self.candidate = None,
            Some(_) => {
                // Switch only when the new rate holds for a few observations.
                let count = match self.candidate {
                    Some((r, k)) if r == snapped => k + 1,
                    _ => 1,
                };
                self.candidate = Some((snapped, count));
                if count >= 20 {
                    log::info!("System audio rate changed to {snapped} Hz");
                    self.current = Some(snapped);
                    self.candidate = None;
                }
            }
        }
        self.current
    }

    pub fn current(&self) -> Option<u32> {
        self.current
    }
}

/// The nearest standard rate within 6 %, or `None`.
pub fn snap(measured: f64) -> Option<u32> {
    STANDARD_RATES
        .iter()
        .copied()
        .min_by(|a, b| {
            (*a as f64 - measured)
                .abs()
                .total_cmp(&(*b as f64 - measured).abs())
        })
        .filter(|r| ((*r as f64 - measured) / *r as f64).abs() < 0.06)
}

/// Streaming linear resampler to 16 kHz; the input rate may change between calls.
pub struct Resampler {
    /// Position of the next output sample, in input samples, relative to `prev`.
    pos: f64,
    prev: f32,
}

impl Resampler {
    pub fn new() -> Self {
        Self {
            pos: 0.0,
            prev: 0.0,
        }
    }

    pub fn process(&mut self, input: &[f32], in_rate: u32, out: &mut Vec<f32>) {
        if input.is_empty() || in_rate == 0 {
            return;
        }
        let step = in_rate as f64 / OUT_RATE as f64;
        // Sample i of the virtual stream: -1 is `prev`, 0.. is `input`.
        let at = |i: isize| -> f32 {
            if i < 0 {
                self.prev
            } else {
                input[i as usize]
            }
        };
        // `pos` counts from `prev` (index -1).
        while self.pos < input.len() as f64 {
            let i = self.pos.floor() as isize - 1;
            let frac = (self.pos - self.pos.floor()) as f32;
            let a = at(i);
            let b = at(i + 1);
            out.push(a + (b - a) * frac);
            self.pos += step;
        }
        self.pos -= input.len() as f64;
        self.prev = *input.last().unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snaps_to_standard_rates() {
        assert_eq!(snap(15_870.0), Some(16_000));
        assert_eq!(snap(47_500.0), Some(48_000));
        assert_eq!(snap(44_000.0), Some(44_100));
        assert_eq!(snap(30_000.0), None);
    }

    fn feed(t: &mut RateTracker, start: Instant, from_s: f64, to_s: f64, rate: u32) -> Option<u32> {
        // 10 ms chunks.
        let mut s = from_s;
        let mut last = None;
        while s < to_s {
            last = t.observe(start + Duration::from_secs_f64(s), (rate / 100) as usize);
            s += 0.01;
        }
        last
    }

    #[test]
    fn measures_the_real_rate_and_follows_a_change() {
        let start = Instant::now();
        let mut t = RateTracker::new();
        assert_eq!(feed(&mut t, start, 0.0, 3.0, 16_000), Some(16_000));
        // Headset leaves telephony mode: 48 kHz from now on.
        assert_eq!(feed(&mut t, start, 3.0, 8.0, 48_000), Some(48_000));
    }

    #[test]
    fn resampling_keeps_duration() {
        let mut r = Resampler::new();
        let mut out = Vec::new();
        for _ in 0..100 {
            r.process(&vec![0.5; 480], 48_000, &mut out); // 10 ms chunks
        }
        assert!((out.len() as i64 - 16_000).abs() <= 2, "{}", out.len());
        let mut out16 = Vec::new();
        let mut r = Resampler::new();
        for _ in 0..100 {
            r.process(&vec![0.5; 160], 16_000, &mut out16);
        }
        assert!((out16.len() as i64 - 16_000).abs() <= 2, "{}", out16.len());
    }

    #[test]
    fn resampling_preserves_a_tone() {
        // 440 Hz at 48 kHz → 16 kHz: zero crossings per second stay ~880.
        let input: Vec<f32> = (0..48_000)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48_000.0).sin())
            .collect();
        let mut out = Vec::new();
        let mut r = Resampler::new();
        for c in input.chunks(512) {
            r.process(c, 48_000, &mut out);
        }
        let crossings = out
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        assert!((crossings as i64 - 880).abs() < 10, "{crossings}");
    }
}

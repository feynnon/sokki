//! Transcribing a meeting while it is recorded.
//!
//! Each track is cut into pieces at pauses as it arrives, and each piece is
//! transcribed with word times right away. Speakers are still found after
//! the recording ends, on the whole track; the pipeline then takes a turn's
//! words from the pieces instead of transcribing the turn again (see
//! [`crate::pipeline::Hooks::pieces`]). Only the last piece of each track is
//! left for after the call.

use crate::{TimedWord, TrackKind, SAMPLE_RATE};

/// A stretch of one track, transcribed during the recording. Times are
/// seconds on the track's timeline, after alignment (see the host's
/// `align_tracks`), the same timeline the pipeline works on.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub track: TrackKind,
    pub start_s: f64,
    pub end_s: f64,
    /// Raw words (not yet cleaned up by [`crate::Transcriber::polish`]),
    /// with times on the track's timeline.
    pub words: Vec<TimedWord>,
}

/// Where pieces may be cut.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutConfig {
    /// A piece is never shorter than this (except the last).
    pub min_s: f64,
    /// A piece is never longer than this.
    pub max_s: f64,
}

impl Default for CutConfig {
    fn default() -> Self {
        Self {
            min_s: 8.0,
            max_s: 25.0,
        }
    }
}

/// Cuts one track into pieces as it arrives: once more than `max_s` is
/// waiting, at the quietest 100 ms between `min_s` and `max_s` (a pause),
/// the same rule the pipeline uses to split a long turn. Pushed all at
/// once, a track is cut exactly as it is live.
pub struct PieceCutter {
    cfg: CutConfig,
    /// Track sample index of `pending[0]`.
    start: usize,
    pending: Vec<f32>,
}

impl PieceCutter {
    pub fn new(cfg: CutConfig) -> Self {
        Self {
            cfg,
            start: 0,
            pending: Vec::new(),
        }
    }

    /// Samples pushed so far.
    pub fn samples(&self) -> usize {
        self.start + self.pending.len()
    }

    /// Adds audio. Returns the pieces it completed: (first sample, audio).
    pub fn push(&mut self, samples: &[f32]) -> Vec<(usize, Vec<f32>)> {
        self.pending.extend_from_slice(samples);
        let sr = SAMPLE_RATE as f64;
        let max = (self.cfg.max_s * sr) as usize;
        let mut out = Vec::new();
        while self.pending.len() > max {
            let lo = self.cfg.min_s;
            let hi = self.cfg.max_s;
            let cut_s = crate::pipeline::quietest_point(&self.pending, lo, hi).unwrap_or(hi);
            let cut = ((cut_s * sr) as usize).clamp(1, self.pending.len());
            out.push(self.take(cut));
        }
        out
    }

    /// The rest, as the last piece (none when nothing is left).
    pub fn finish(&mut self) -> Option<(usize, Vec<f32>)> {
        (!self.pending.is_empty()).then(|| self.take(self.pending.len()))
    }

    fn take(&mut self, n: usize) -> (usize, Vec<f32>) {
        let piece: Vec<f32> = self.pending.drain(..n).collect();
        let at = self.start;
        self.start += n;
        (at, piece)
    }
}

/// Moves word times from piece-relative to the track's timeline.
pub fn place_words(words: Vec<TimedWord>, piece_start_s: f64) -> Vec<TimedWord> {
    words
        .into_iter()
        .map(|w| TimedWord {
            start_s: w.start_s + piece_start_s,
            end_s: w.end_s + piece_start_s,
            text: w.text,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: usize = SAMPLE_RATE as usize;

    /// Loud "speech" with a silent pause every 7 s.
    fn speech_with_pauses(seconds: usize) -> Vec<f32> {
        (0..seconds * SR)
            .map(|i| {
                let t = i % (7 * SR);
                if t < SR / 2 {
                    0.0
                } else {
                    ((i as f32) * 0.07).sin() * 0.3
                }
            })
            .collect()
    }

    #[test]
    fn cuts_at_pauses_within_the_limits() {
        let audio = speech_with_pauses(100);
        let mut c = PieceCutter::new(CutConfig::default());
        let mut pieces = c.push(&audio);
        pieces.extend(c.finish());
        let mut next = 0;
        for (i, (at, p)) in pieces.iter().enumerate() {
            assert_eq!(*at, next, "pieces are contiguous");
            next += p.len();
            let s = p.len() as f64 / SR as f64;
            assert!(s <= 25.0, "piece {i} is {s} s");
            if i + 1 < pieces.len() {
                assert!(s >= 8.0, "piece {i} is {s} s");
                // Cut inside a pause: the samples around the cut are silent.
                let cut = at + p.len();
                assert_eq!(audio[cut], 0.0, "cut {cut} is not in a pause");
            }
        }
        assert_eq!(next, audio.len());
    }

    #[test]
    fn cuts_the_same_live_as_all_at_once() {
        let audio = speech_with_pauses(90);
        let mut whole = PieceCutter::new(CutConfig::default());
        let mut a = whole.push(&audio);
        a.extend(whole.finish());
        let mut live = PieceCutter::new(CutConfig::default());
        let mut b = Vec::new();
        for block in audio.chunks(1234) {
            b.extend(live.push(block));
        }
        b.extend(live.finish());
        assert_eq!(a, b);
    }

    #[test]
    fn short_audio_is_one_piece_at_the_end() {
        let mut c = PieceCutter::new(CutConfig::default());
        assert!(c.push(&vec![0.1; 3 * SR]).is_empty());
        let (at, p) = c.finish().unwrap();
        assert_eq!((at, p.len()), (0, 3 * SR));
        assert!(c.finish().is_none());
    }
}

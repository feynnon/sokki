//! Transcribing a meeting while it is recorded.
//!
//! Each track is cut into pieces at pauses as it arrives, and each piece is
//! transcribed with word times right away. Speakers are still found after
//! the recording ends, on the whole track; the pipeline then takes a turn's
//! words from the pieces instead of transcribing the turn again (see
//! [`crate::pipeline::Hooks::pieces`]). Only the last piece of each track is
//! left for after the call.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::{TimedWord, TrackKind, Transcriber, SAMPLE_RATE};

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


/// Texts transcribed during the recording, by the exact audio they came
/// from. The run at the end asks [`Cached`] instead of its transcriber; a
/// slice it has already heard, sample for sample, gets the same text back
/// (the same input gives the same output), so nothing changes but the time.
#[derive(Debug, Default)]
pub struct TextCache {
    texts: Mutex<HashMap<(usize, u64), String>>,
}

impl TextCache {
    /// The key of a slice: its length and a hash of every sample's bits.
    pub fn key(samples: &[f32]) -> (usize, u64) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for x in samples {
            x.to_bits().hash(&mut h);
        }
        (samples.len(), h.finish())
    }

    pub fn insert(&self, samples: &[f32], text: String) {
        self.lock().insert(Self::key(samples), text);
    }

    pub fn get(&self, samples: &[f32]) -> Option<String> {
        self.lock().get(&Self::key(samples)).cloned()
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(usize, u64), String>> {
        self.texts.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// A transcriber that answers from a [`TextCache`] where it can.
pub struct Cached<'a> {
    pub inner: &'a dyn Transcriber,
    pub cache: &'a TextCache,
    /// Slices answered from the cache.
    pub hits: AtomicUsize,
}

impl<'a> Cached<'a> {
    pub fn new(inner: &'a dyn Transcriber, cache: &'a TextCache) -> Self {
        Self {
            inner,
            cache,
            hits: AtomicUsize::new(0),
        }
    }

    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }
}

impl Transcriber for Cached<'_> {
    fn transcribe(&self, samples: &[f32]) -> anyhow::Result<String> {
        if let Some(t) = self.cache.get(samples) {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return Ok(t);
        }
        self.inner.transcribe(samples)
    }

    fn transcribe_words(&self, samples: &[f32]) -> anyhow::Result<Option<Vec<TimedWord>>> {
        self.inner.transcribe_words(samples)
    }

    fn polish(&self, text: &str) -> String {
        self.inner.polish(text)
    }
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
    fn the_cache_answers_only_for_the_same_samples() {
        struct Echo;
        impl Transcriber for Echo {
            fn transcribe(&self, s: &[f32]) -> anyhow::Result<String> {
                Ok(format!("fresh {}", s.len()))
            }
        }
        let cache = TextCache::default();
        let a = vec![0.1f32; 100];
        cache.insert(&a, "from the call".into());
        let t = Cached::new(&Echo, &cache);
        assert_eq!(t.transcribe(&a).unwrap(), "from the call");
        let mut b = a.clone();
        b[99] = 0.1000001;
        assert_eq!(t.transcribe(&b).unwrap(), "fresh 100");
        assert_eq!(t.transcribe(&a[..99]).unwrap(), "fresh 99");
        assert_eq!(t.hits(), 1);
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

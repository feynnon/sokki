//! Live dictation: cut microphone audio (16 kHz mono) into pieces while the
//! person speaks, so each piece can be transcribed right away and only the
//! last one is left when they stop.
//!
//! Where to cut: at a short pause, found from the volume. Every 20 ms frame
//! gets its loudness (RMS); the room's background level follows the
//! quietest frames: it drops at once to a quieter frame and rises only
//! slowly (about 2.5 % per second), so long speech without a pause never
//! becomes "background". A frame is quiet when it is close to that level. After `min_s` of audio, the first run of `pause_ms` quiet frames
//! ends the piece (cut in the middle of the pause). If nobody pauses for
//! `max_s`, the piece ends at the quietest frame of its last 2 s, which is
//! usually a gap between two words.

pub const RATE: usize = 16_000;
const FRAME: usize = RATE / 50; // 20 ms

#[derive(Debug, Clone, Copy)]
pub struct Params {
    pub min_s: f32,
    pub max_s: f32,
    pub pause_ms: u32,
    /// A frame is quiet below `floor * factor + margin`.
    pub factor: f32,
    pub margin: f32,
}

impl Default for Params {
    fn default() -> Self {
        // Measured (asr-bench --chunking, 2026-11-18): pieces under ~4 s lose
        // context and accuracy. 6 s / 300 ms / 15 s: 2–4 % off the
        // whole-recording transcript on clean speech, 6.2 % on real speech
        // (fixed 5 s pieces: 7–22 % and 12.4 %).
        Params {
            min_s: 6.0,
            max_s: 15.0,
            pause_ms: 300,
            factor: 2.5,
            margin: 0.002,
        }
    }
}

/// How to cut: at pauses (the point of this crate) or every `n` seconds
/// (for comparison).
#[derive(Debug, Clone, Copy)]
pub enum Mode {
    Pauses(Params),
    Fixed(f32),
}

pub struct Segmenter {
    mode: Mode,
    buf: Vec<f32>,
    /// Loudness of each complete frame in `buf`.
    levels: Vec<f32>,
    /// The room's background level (see the module doc); `None` until the
    /// first frame.
    floor: Option<f32>,
}

fn rms(frame: &[f32]) -> f32 {
    (frame.iter().map(|s| s * s).sum::<f32>() / frame.len().max(1) as f32).sqrt()
}

impl Segmenter {
    pub fn new(mode: Mode) -> Self {
        Segmenter {
            mode,
            buf: Vec::new(),
            levels: Vec::new(),
            floor: None,
        }
    }

    fn floor(&self) -> f32 {
        self.floor.unwrap_or(0.0)
    }

    fn track_floor(&mut self, level: f32) {
        // +2.5 % per second at 50 frames per second.
        const RISE: f32 = 1.0005;
        self.floor = Some(match self.floor {
            None => level,
            Some(f) if level < f => level,
            Some(f) => (f * RISE).max(1e-5),
        });
    }

    /// Adds audio; returns the pieces that are complete.
    pub fn push(&mut self, samples: &[f32]) -> Vec<Vec<f32>> {
        let mut out = Vec::new();
        for &s in samples {
            self.buf.push(s);
            if self.buf.len() % FRAME == 0 {
                let f = &self.buf[self.buf.len() - FRAME..];
                let level = rms(f);
                self.levels.push(level);
                self.track_floor(level);
                if let Some(cut) = self.cut_point() {
                    out.push(self.take(cut));
                }
            }
        }
        out
    }

    /// Changes how the next pieces are cut (e.g. a short first piece, then
    /// longer ones).
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }

    /// The audio of the piece that is still growing (not cut yet).
    pub fn pending(&self) -> &[f32] {
        &self.buf
    }

    /// The rest when the person stops.
    pub fn finish(&mut self) -> Option<Vec<f32>> {
        let len = self.buf.len();
        (len > 0).then(|| self.take(len))
    }

    fn take(&mut self, at: usize) -> Vec<f32> {
        let rest = self.buf.split_off(at);
        let piece = std::mem::replace(&mut self.buf, rest);
        let frames = at / FRAME;
        self.levels.drain(..frames.min(self.levels.len()));
        piece
    }

    /// Where the quiet at the end of the growing piece begins (sample
    /// index into `pending()`): everything from here on is pause or room
    /// noise. `pending().len()` when it ends in speech, 0 when it is all quiet.
    pub fn quiet_from(&self) -> usize {
        self.speech_end(&self.buf)
    }

    /// Where the speech in `audio` (a piece from this segmenter) ends: the
    /// end of its last frame that isn't quiet, by the room level heard so
    /// far. `audio.len()` in fixed mode, 0 when it is all quiet.
    pub fn speech_end(&self, audio: &[f32]) -> usize {
        let Mode::Pauses(p) = self.mode else {
            return audio.len();
        };
        let quiet_below = self.floor() * p.factor + p.margin;
        let frames = audio.len() / FRAME;
        match (0..frames).rev().find(|&i| rms(&audio[i * FRAME..(i + 1) * FRAME]) >= quiet_below) {
            // A partial frame at the very end counts as speech when the audio ends in speech.
            Some(i) if i + 1 == frames => audio.len(),
            Some(i) => (i + 1) * FRAME,
            None => 0,
        }
    }

    /// Sample index to cut at, if the current piece should end now.
    fn cut_point(&self) -> Option<usize> {
        let len_s = self.buf.len() as f32 / RATE as f32;
        match self.mode {
            Mode::Fixed(n) => (len_s >= n).then_some(self.buf.len()),
            Mode::Pauses(p) => {
                let n = self.levels.len();
                let min_frames = (p.min_s * 50.0) as usize;
                let pause_frames = (p.pause_ms / 20).max(1) as usize;
                let quiet_below = self.floor() * p.factor + p.margin;
                if n >= min_frames.max(pause_frames)
                    && self.levels[n - pause_frames..]
                        .iter()
                        .all(|&l| l < quiet_below)
                {
                    return Some((n - pause_frames / 2) * FRAME);
                }
                if len_s >= p.max_s {
                    let from = n.saturating_sub(100); // last 2 s
                    let quietest = (from..n)
                        .min_by(|&a, &b| self.levels[a].partial_cmp(&self.levels[b]).unwrap())
                        .unwrap_or(n - 1);
                    return Some((quietest + 1) * FRAME);
                }
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(secs: f32) -> Vec<f32> {
        (0..(secs * RATE as f32) as usize)
            .map(|i| 0.3 * (i as f32 * 0.05).sin())
            .collect()
    }
    fn quiet(secs: f32) -> Vec<f32> {
        (0..(secs * RATE as f32) as usize)
            .map(|i| 0.0005 * ((i * 7919 % 13) as f32 - 6.0))
            .collect()
    }

    #[test]
    fn cuts_in_the_pause_between_words() {
        let mut s = Segmenter::new(Mode::Pauses(Params::default()));
        let mut audio = quiet(1.0);
        audio.extend(tone(6.0));
        audio.extend(quiet(0.6));
        audio.extend(tone(1.0));
        let pieces = s.push(&audio);
        assert_eq!(pieces.len(), 1);
        let len_s = pieces[0].len() as f32 / RATE as f32;
        // 1 s quiet + 6 s speech + part of the 0.6 s pause.
        assert!((7.0..7.6).contains(&len_s), "{len_s}");
        assert!(s.finish().is_some());
    }

    #[test]
    fn long_speech_without_pause_is_cut_at_max() {
        let mut s = Segmenter::new(Mode::Pauses(Params::default()));
        let pieces = s.push(&tone(32.0));
        assert!(pieces.len() >= 2);
        for p in &pieces {
            assert!(p.len() as f32 / RATE as f32 <= 15.0 + 0.02);
        }
    }

    #[test]
    fn no_audio_is_lost() {
        // Cutting is only for speed: the pieces add up to every sample.
        let mut audio = Vec::new();
        for _ in 0..6 {
            audio.extend(tone(7.0));
            audio.extend(quiet(0.5));
        }
        let mut s = Segmenter::new(Mode::Pauses(Params::default()));
        let mut pieces = Vec::new();
        for block in audio.chunks(3200) {
            pieces.extend(s.push(block));
        }
        pieces.extend(s.finish());
        assert!(pieces.len() > 2);
        assert_eq!(pieces.concat(), audio);
    }

    #[test]
    fn finds_the_quiet_at_the_end() {
        let mut s = Segmenter::new(Mode::Pauses(Params::default()));
        let mut audio = quiet(1.0);
        audio.extend(tone(2.0));
        s.push(&audio);
        assert_eq!(s.quiet_from(), s.pending().len()); // still speaking
        s.push(&quiet(0.8));
        let at = s.quiet_from() as f32 / RATE as f32;
        assert!((2.9..3.1).contains(&at), "{at}"); // speech ended at 3 s
    }

    #[test]
    fn fixed_mode_cuts_every_n_seconds() {
        let mut s = Segmenter::new(Mode::Fixed(5.0));
        let pieces = s.push(&tone(12.0));
        assert_eq!(pieces.len(), 2);
        assert_eq!(s.finish().unwrap().len(), 2 * RATE);
    }
}

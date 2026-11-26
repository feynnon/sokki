//! Parakeet on the phone, live: what the keyboard and the Dictate tab do
//! when the Mac isn't there.
//!
//! Parakeet can't stream, so the text comes from reading the audio again:
//! the piece still growing is read every half second (live text, may still
//! change), and a piece ends at a pause after 6 s (at most 15 s;
//! `dictation-core`), is read once more as a whole and kept. At the stop
//! only the last piece is left. One read at a time on one session.
//!
//! Why: on Robin's own 34 dictations Parakeet in these pieces differs from
//! the references in 12.0 % of words, Nemotron Streaming in 17 % (whole
//! dictations: 10.3 %; 8–20 s pieces: 10.5 %, but 1 s more wait at the
//! stop). On his Pixel 7 (13 test clips, Q8, 4 threads): first words 1.4 s
//! after the start (Nemotron 1.45 s), text ~1 s behind the speech (worst
//! 2.4 s at the median clip), done 2.4 s after the stop at the median (at
//! most 4.5 s; Nemotron 0.3 s). `docs/research/phone-dictation-latency-2026-11.md`.

use std::collections::VecDeque;

use dictation_core::{Mode, Params, Segmenter};
use transcribe_cpp::{RunOptions, Session};

/// Read the growing piece again once this much more audio is in.
const PREVIEW_EVERY: usize = 16_000 / 2;

/// The text so far: `committed` is final (typed for good), `live` is the
/// growing piece's latest reading (may still change).
pub struct Update {
    pub committed: String,
    pub live: String,
}

impl Update {
    pub fn text(&self) -> String {
        join(&self.committed, &self.live)
    }
}

fn join(a: &str, b: &str) -> String {
    format!("{a} {b}").trim().to_string()
}

fn read(session: &mut Session, audio: &[f32]) -> String {
    session
        .run(audio, &RunOptions::default())
        .map(|r| r.text.trim().to_string())
        .unwrap_or_default()
}

pub struct Live {
    seg: Segmenter,
    /// Pieces that ended and wait for their read.
    queue: VecDeque<Vec<f32>>,
    committed: String,
    /// Length of the growing piece at its last reading.
    last_len: usize,
    /// Samples heard so far.
    pub heard: usize,
}

impl Default for Live {
    fn default() -> Self {
        Live {
            seg: Segmenter::new(Mode::Pauses(Params::default())),
            queue: VecDeque::new(),
            committed: String::new(),
            last_len: 0,
            heard: 0,
        }
    }
}

impl Live {
    pub fn push(&mut self, audio: &[f32]) {
        self.heard += audio.len();
        let done = self.seg.push(audio);
        self.queue.extend(done);
    }

    /// One read, if one is due: a piece that ended (its text is kept), else
    /// the growing piece once half a second more is in and it holds speech.
    /// `None`: nothing to read now.
    pub fn step(&mut self, session: &mut Session) -> Option<Update> {
        if let Some(piece) = self.queue.pop_front() {
            let text = read(session, &piece);
            self.committed = join(&self.committed, &text);
            self.last_len = 0;
            return Some(Update {
                committed: self.committed.clone(),
                live: String::new(),
            });
        }
        let pending = self.seg.pending();
        if pending.len() < self.last_len {
            self.last_len = 0;
        }
        if pending.len() < self.last_len + PREVIEW_EVERY || self.seg.quiet_from() == 0 {
            return None;
        }
        self.last_len = pending.len();
        let audio = pending.to_vec();
        let live = read(session, &audio);
        Some(Update {
            committed: self.committed.clone(),
            live,
        })
    }

    /// The person stopped: the pieces still waiting and the last one (its
    /// quiet end left out). The whole text.
    pub fn finish(mut self, session: &mut Session) -> String {
        while let Some(piece) = self.queue.pop_front() {
            let text = read(session, &piece);
            self.committed = join(&self.committed, &text);
        }
        if let Some(last) = self.seg.finish() {
            let end = self.seg.speech_end(&last);
            if end > 0 {
                let text = read(session, &last[..end]);
                self.committed = join(&self.committed, &text);
            }
        }
        self.committed
    }
}

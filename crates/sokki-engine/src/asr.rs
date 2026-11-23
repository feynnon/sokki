//! A local speech recogniser: Parakeet v3 (or any transcribe.cpp model)
//! loaded once and kept in memory. For hosts without their own engine (the
//! Linux service); the macOS app uses Sokki's engine.

use anyhow::{anyhow, bail, Result};
use meeting_core::{TimedWord, Transcriber, SAMPLE_RATE};
use std::path::Path;
use std::sync::{Mutex, Once};

static BACKENDS: Once = Once::new();

/// Loads transcribe.cpp's compute backends (GPU if available, else CPU).
/// Once per process: transcribe.cpp registers backends only once.
pub fn init_backends() {
    BACKENDS.call_once(|| {
        // Its diagnostics go through the `log` crate, so the host's log
        // level decides (the CLI shows warnings only).
        transcribe_cpp::init_logging();
        if let Err(e) = transcribe_cpp::init_backends_default() {
            log::warn!("transcribe.cpp backends: {e}");
        }
    });
}

/// A loaded model with one session; calls are serialised.
pub struct LocalTranscriber {
    session: Mutex<transcribe_cpp::Session>,
    run: transcribe_cpp::RunOptions,
    // Keeps the model alive for the session.
    _model: transcribe_cpp::Model,
}

impl LocalTranscriber {
    pub fn load(path: &Path) -> Result<Self> {
        init_backends();
        if !path.exists() {
            bail!("model not found: {}", path.display());
        }
        let model =
            transcribe_cpp::Model::load_with(path, &transcribe_cpp::ModelOptions::default())
                .map_err(|e| anyhow!("load {}: {e}", path.display()))?;
        let session = model.session().map_err(|e| anyhow!("session: {e}"))?;
        Ok(Self {
            session: Mutex::new(session),
            run: Default::default(),
            _model: model,
        })
    }

    /// A language hint ("de", "en"); `None` lets the model decide.
    pub fn set_language(&mut self, lang: Option<String>) {
        self.run.language = lang;
    }

    fn run(&self, samples: &[f32], opts: &transcribe_cpp::RunOptions) -> Result<transcribe_cpp::Transcript> {
        let mut s = self.session.lock().unwrap_or_else(|e| e.into_inner());
        s.run(samples, opts).map_err(|e| anyhow!("transcribe: {e}"))
    }
}

/// The longest piece the speech model takes in one run (the meeting
/// pipeline cuts at the same length).
pub const MAX_PIECE_S: f64 = 25.0;

impl LocalTranscriber {
    /// Any length of speech: longer audio is cut into pieces of at most
    /// [`MAX_PIECE_S`], each at the quietest moment of its last 3 seconds
    /// (usually a pause between words), and the texts are joined.
    pub fn transcribe_long(&self, samples: &[f32]) -> Result<String> {
        let mut texts = Vec::new();
        for piece in pieces(samples, MAX_PIECE_S) {
            let t = self.transcribe(piece)?;
            if !t.is_empty() {
                texts.push(t);
            }
        }
        Ok(texts.join(" "))
    }
}

/// Splits audio into pieces of at most `max_s` seconds, cutting at the
/// quietest 20 ms frame in the last 3 s of each piece.
pub fn pieces(samples: &[f32], max_s: f64) -> Vec<&[f32]> {
    let rate = SAMPLE_RATE as usize;
    let max = (max_s * rate as f64) as usize;
    let frame = rate / 50;
    let search = 3 * rate;
    let mut out = Vec::new();
    let mut start = 0;
    while samples.len() - start > max {
        let end = start + max;
        let from = end.saturating_sub(search).max(start + frame);
        let mut best = end;
        let mut best_energy = f32::MAX;
        let mut i = from;
        while i + frame <= end {
            let e: f32 = samples[i..i + frame].iter().map(|x| x * x).sum();
            if e < best_energy {
                best_energy = e;
                best = i + frame / 2;
            }
            i += frame;
        }
        out.push(&samples[start..best]);
        start = best;
    }
    if start < samples.len() {
        out.push(&samples[start..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_audio_is_cut_at_a_pause() {
        let rate = SAMPLE_RATE as usize;
        // 40 s of tone with a silent stretch at 23 s.
        let mut audio: Vec<f32> = (0..40 * rate).map(|i| (i as f32 * 0.1).sin()).collect();
        for x in &mut audio[23 * rate..23 * rate + rate / 5] {
            *x = 0.0;
        }
        let p = pieces(&audio, MAX_PIECE_S);
        assert_eq!(p.len(), 2);
        let cut = p[0].len() as f64 / rate as f64;
        assert!((23.0..23.2).contains(&cut), "cut at {cut}");
        assert_eq!(p[0].len() + p[1].len(), audio.len());
        // Short audio stays whole.
        assert_eq!(pieces(&audio[..rate], MAX_PIECE_S).len(), 1);
    }
}

impl Transcriber for LocalTranscriber {
    fn transcribe(&self, samples: &[f32]) -> Result<String> {
        if samples.len() < SAMPLE_RATE as usize / 10 {
            return Ok(String::new());
        }
        Ok(self.run(samples, &self.run)?.text.trim().to_string())
    }

    fn transcribe_words(&self, samples: &[f32]) -> Result<Option<Vec<TimedWord>>> {
        if samples.len() < SAMPLE_RATE as usize / 10 {
            return Ok(Some(Vec::new()));
        }
        let opts = transcribe_cpp::RunOptions {
            timestamps: transcribe_cpp::TimestampKind::Word,
            ..self.run.clone()
        };
        let t = self.run(samples, &opts)?;
        if t.words.is_empty() && !t.text.trim().is_empty() {
            return Ok(None); // this model gives no word times
        }
        Ok(Some(
            t.words
                .into_iter()
                .map(|w| TimedWord {
                    start_s: w.t0_ms as f64 / 1000.0,
                    end_s: w.t1_ms as f64 / 1000.0,
                    text: w.text,
                })
                .collect(),
        ))
    }
}

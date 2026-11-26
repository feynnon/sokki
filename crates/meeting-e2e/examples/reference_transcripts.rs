//! Reference transcripts for comparing Sokki's live dictation text with the
//! pasted one (docs/research/live-vs-final-2026-11.md): another speech model
//! transcribes each WAV on its own, as a tie-breaker.
//!
//! cargo run --release -p meeting-e2e --example reference_transcripts -- \
//!     MODEL.gguf OUT.json a.wav@de b.wav@en c.wav
//!
//! `@de` is the language hint for that file (none: the model detects it).
//! Audio longer than 28 s is cut into pieces of at most 28 s, each cut at the
//! quietest half second in the last 8 s before it, so a word is rarely split.
//! Writes {"file name": "text"} to OUT.json and prints only timings.
use meeting_e2e::{read_wav_16k_mono, EngineTranscriber};
use std::collections::BTreeMap;
use std::path::Path;

const RATE: usize = 16_000;
const MAX_PIECE: usize = 28 * RATE;
const SEARCH: usize = 8 * RATE;
const WINDOW: usize = RATE / 2;

/// Where to cut `s` into pieces of at most MAX_PIECE samples.
fn cuts(s: &[f32]) -> Vec<usize> {
    let mut out = vec![0];
    let mut start = 0;
    while s.len() - start > MAX_PIECE {
        let hi = start + MAX_PIECE;
        let lo = hi - SEARCH;
        let energy = |at: usize| -> f32 { s[at..at + WINDOW].iter().map(|x| x * x).sum() };
        let best = (lo..hi - WINDOW)
            .step_by(RATE / 20)
            .min_by(|a, b| energy(*a).total_cmp(&energy(*b)))
            .map(|at| at + WINDOW / 2)
            .unwrap_or(hi);
        out.push(best);
        start = best;
    }
    out.push(s.len());
    out
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let model = args.next().expect("MODEL.gguf");
    let out_path = args.next().expect("OUT.json");
    let t = EngineTranscriber::load(Path::new(&model))?;
    let mut results: BTreeMap<String, String> = BTreeMap::new();
    for arg in args {
        let (file, lang) = match arg.rsplit_once('@') {
            Some((f, l)) if l.len() <= 3 => (f.to_string(), Some(l.to_string())),
            _ => (arg.clone(), None),
        };
        let samples = read_wav_16k_mono(Path::new(&file))?;
        let mut opts = t.run_options().clone();
        opts.language = lang;
        let started = std::time::Instant::now();
        let c = cuts(&samples);
        let mut text = Vec::new();
        for w in c.windows(2) {
            let piece = &samples[w[0]..w[1]];
            if piece.len() < RATE / 4 {
                continue;
            }
            let r = t.run_with(piece, &opts)?;
            text.push(r.text.trim().to_string());
        }
        let name = Path::new(&file)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or(file.clone());
        println!(
            "{name}: {:.1} s audio, {} pieces, {:.1} s",
            samples.len() as f64 / RATE as f64,
            c.len() - 1,
            started.elapsed().as_secs_f64()
        );
        results.insert(name, text.join(" "));
    }
    std::fs::write(&out_path, serde_json::to_string_pretty(&results)?)?;
    Ok(())
}

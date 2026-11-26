//! Lines in an unexpected language: for each line of a transcript (as
//! `speaker_count` writes it with OUT=), the line's loudness on its track,
//! the language the speech model detects, and the text again with a
//! German and an English hint. Writes the texts to `out` (keep it outside
//! the repository) and prints counts only.
//!
//!   MEETING_ASR_MODEL=<model> cargo run --release --example lang_check -- <meeting dir> <transcript.json> <out.json>
use meeting_core::{TrackKind, Transcript};
use std::path::Path;
use std::process::Command;

fn decode(path: &Path) -> anyhow::Result<Vec<f32>> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-f", "f32le", "-ac", "1", "-ar", "16000", "-"])
        .output()?;
    anyhow::ensure!(out.status.success(), "ffmpeg failed on {}", path.display());
    Ok(out
        .stdout
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect())
}

fn db(x: &[f32]) -> f64 {
    let e: f64 = x.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / x.len().max(1) as f64;
    10.0 * (e.max(1e-12)).log10()
}

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let dir = Path::new(&a[0]);
    let t: Transcript = serde_json::from_str(&std::fs::read_to_string(&a[1])?)?;
    let mic = decode(&dir.join("mic.flac"))?;
    let asr = meeting_e2e::EngineTranscriber::from_env()?.expect("MEETING_ASR_MODEL");
    let mut rows = Vec::new();
    for s in &t.segments {
        if s.track != TrackKind::Mic {
            continue;
        }
        let i = ((s.start_s - 0.2).max(0.0) * 16_000.0) as usize;
        let j = (((s.end_s + 0.2) * 16_000.0) as usize).min(mic.len());
        if j <= i {
            continue;
        }
        let x = &mic[i..j];
        let mut opts = asr.run_options().clone();
        opts.language = None;
        let auto = asr.run_with(x, &opts)?;
        opts.language = Some("de".into());
        let de = asr.run_with(x, &opts)?;
        opts.language = Some("en".into());
        let en = asr.run_with(x, &opts)?;
        rows.push(serde_json::json!({
            "start_s": s.start_s, "dur_s": s.end_s - s.start_s, "speaker": s.speaker,
            "db": db(x), "lang": auto.language, "auto": auto.text, "de": de.text, "en": en.text,
            "note": s.text,
        }));
    }
    let other = |r: &serde_json::Value| !matches!(r["lang"].as_str(), Some("de") | Some("en"));
    let n_other = rows.iter().filter(|r| other(r)).count();
    println!(
        "mic lines {}, detected other than de/en: {n_other}",
        rows.len()
    );
    for r in rows.iter().filter(|r| other(r)) {
        println!(
            "  {:6.1}s dur {:4.1}s {:6.1} dB lang {:?}",
            r["start_s"].as_f64().unwrap_or(0.0),
            r["dur_s"].as_f64().unwrap_or(0.0),
            r["db"].as_f64().unwrap_or(0.0),
            r["lang"].as_str()
        );
    }
    let mut dbs: Vec<f64> = rows
        .iter()
        .filter(|r| !other(r))
        .filter_map(|r| r["db"].as_f64())
        .collect();
    dbs.sort_by(|a, b| a.total_cmp(b));
    if !dbs.is_empty() {
        println!(
            "  de/en lines: dB min {:.1}, p10 {:.1}, median {:.1}",
            dbs[0],
            dbs[dbs.len() / 10],
            dbs[dbs.len() / 2]
        );
    }
    std::fs::write(&a[2], serde_json::to_string_pretty(&rows)?)?;
    Ok(())
}

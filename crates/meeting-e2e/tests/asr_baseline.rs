//! ASR quality with perfect diarization: each truth turn is cut out and
//! transcribed on its own. This separates speech-recognition quality from
//! diarization quality when the full pipeline scores badly.

use meeting_e2e::*;

fn transcriber() -> Option<EngineTranscriber> {
    match EngineTranscriber::parakeet() {
        Ok(Some(t)) => Some(t),
        Ok(None) => {
            eprintln!("SKIP: {PARAKEET_FILE} not in {}", models_dir().display());
            None
        }
        Err(e) => panic!("loading Parakeet failed: {e:#}"),
    }
}

#[test]
fn parakeet_wer_on_truth_turns() {
    let Some(t) = transcriber() else { return };
    let mut worst = 0.0f64;
    for scenario in [
        "de_two",
        "de_en_mixed",
        "phone_speaker",
        "call_tracks",
        "three_remote",
    ] {
        let Some(fx) = Fixture::load(scenario).unwrap() else {
            eprintln!("SKIP {scenario}: fixture missing, run testdata/generate.py");
            continue;
        };
        let started = std::time::Instant::now();
        let tr = transcribe_truth_turns(&fx, &t).unwrap();
        let s = score(&fx.truth, &tr);
        eprintln!(
            "{scenario:14} WER {:5.1}%  ({:.1}s for {:.0}s audio)",
            s.wer * 100.0,
            started.elapsed().as_secs_f64(),
            fx.truth.duration_s
        );
        for (turn, seg) in fx.truth.turns.iter().zip(&tr.segments) {
            let w = wer(&turn.text, &seg.text);
            if w > 0.15 {
                eprintln!(
                    "   [{:.0}%] truth: {}\n          asr:   {}",
                    w * 100.0,
                    turn.text,
                    seg.text
                );
            }
        }
        worst = worst.max(s.wer);
    }
    // Clean TTS speech should transcribe nearly perfectly; the phone-speaker
    // voice is degraded on purpose.
    assert!(
        worst < 0.25,
        "worst scenario WER {:.1}% is too high",
        worst * 100.0
    );
}

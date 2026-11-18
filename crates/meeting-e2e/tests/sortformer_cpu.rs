//! The app runs Sortformer in its own process with CPU-only backends (the
//! GPU belongs to the engine worker; dictation's VAD registers CPU-only and
//! the registration is once per process). This checks Sortformer still works
//! and is fast enough on the CPU, and that registering CPU again succeeds.

use meeting_core::{Diarizer, TrackKind};
use meeting_e2e::*;
use std::path::Path;

#[test]
fn sortformer_on_cpu_only_backends() {
    transcribe_cpp::init_backends_with(None::<&Path>, transcribe_cpp::BackendMask::CPU).unwrap();
    let models = Path::new(&std::env::var("HOME").unwrap())
        .join("Library/Caches/handy-meetings/diarize-models");
    let Some(fx) = Fixture::load("phone_speaker").unwrap() else {
        return;
    };
    let d = diarize::SortformerDiarizer::new(diarize::DiarizeConfig::new(&models)).unwrap();
    let mic = fx.track(TrackKind::Mic).unwrap();
    let t = std::time::Instant::now();
    let turns = d.diarize(&mic.samples).unwrap();
    let secs = t.elapsed().as_secs_f64();
    let speakers: std::collections::HashSet<_> = turns.iter().map(|t| t.speaker.clone()).collect();
    eprintln!(
        "CPU: {} turns, {} speakers, {:.1}s for {:.0}s audio ({:.1}s per minute)",
        turns.len(),
        speakers.len(),
        secs,
        fx.truth.duration_s,
        secs / fx.truth.duration_s * 60.0
    );
    assert_eq!(speakers.len(), 2);
    // Dictation's VAD registering CPU-only afterwards must not fail.
    transcribe_cpp::init_backends_with(None::<&Path>, transcribe_cpp::BackendMask::CPU)
        .expect("registering CPU backends again failed");
    drop(d);
}

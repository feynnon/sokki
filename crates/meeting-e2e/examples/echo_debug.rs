//! Prints each diarized mic turn of a fixture with its acoustic echo share.
//! cargo run --release --example echo_debug -- call_tracks
use meeting_core::echo::{echo_share, EchoConfig};
use meeting_core::{Diarizer, TrackKind};
use meeting_e2e::*;

fn main() {
    let scenario = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "call_tracks".into());
    let fx = Fixture::load(&scenario).unwrap().expect("fixture");
    let models = std::path::Path::new(&std::env::var("HOME").unwrap())
        .join("Library/Caches/sokki/diarize-models");
    let d = diarize::SortformerDiarizer::new(diarize::DiarizeConfig::new(&models)).unwrap();
    let mic = fx.track(TrackKind::Mic).unwrap();
    let sys = fx.track(TrackKind::System).unwrap();
    let cfg = EchoConfig::default();
    for t in d.diarize(&mic.samples).unwrap() {
        let share = echo_share(&mic.samples, &sys.samples, t.start_s, t.end_s, &cfg);
        println!(
            "{:6.2}-{:6.2} {:4} echo {:.2}",
            t.start_s, t.end_s, t.speaker, share
        );
    }
    for t in &fx.truth.turns {
        println!(
            "truth {:6.2}-{:6.2} {} {}",
            t.start_s, t.end_s, t.track, t.speaker
        );
    }
}

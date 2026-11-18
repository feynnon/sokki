//! Transcribes a 16 kHz mono WAV with Parakeet: cargo run --release --example transcribe_wav -- file.wav
use meeting_core::Transcriber;
use meeting_e2e::*;
fn main() {
    let t = EngineTranscriber::parakeet()
        .unwrap()
        .expect("parakeet model");
    for p in std::env::args().skip(1) {
        let s = read_wav_16k_mono(std::path::Path::new(&p)).unwrap();
        let mut out = Vec::new();
        for chunk in s.chunks(16_000 * 20) {
            out.push(t.transcribe(chunk).unwrap());
        }
        println!("{p}: {:.1}s\n{}\n", s.len() as f64 / 16000.0, out.join(" "));
    }
}

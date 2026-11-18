//! Prints each word with the lowest confidence of its tokens:
//! cargo run --release --example confidence -- file.wav
use meeting_e2e::*;
use transcribe_cpp::{RunOptions, TimestampKind};
fn main() {
    let path = models_dir().join(PARAKEET_FILE);
    let model = transcribe_cpp::Model::load_with(&path, &Default::default()).unwrap();
    let mut session = model.session().unwrap();
    for p in std::env::args().skip(1) {
        let s = read_wav_16k_mono(std::path::Path::new(&p)).unwrap();
        let opts = RunOptions {
            timestamps: TimestampKind::Token,
            ..Default::default()
        };
        let t = session.run(&s, &opts).unwrap();
        println!("{p}: {}", t.text);
        for (wi, w) in t.words.iter().enumerate() {
            let ps: Vec<f32> = t
                .tokens
                .iter()
                .filter(|k| k.word_index == wi as i32)
                .map(|k| k.p)
                .collect();
            let min = ps.iter().cloned().fold(f32::INFINITY, f32::min);
            let mean = ps.iter().sum::<f32>() / ps.len().max(1) as f32;
            println!(
                "{:>7.2}s  min {:>5.2} mean {:>5.2}  {}",
                w.t0_ms as f64 / 1000.0,
                min,
                mean,
                w.text
            );
        }
        if t.words.is_empty() {
            for k in &t.tokens {
                println!("tok {:>5.2} {:?}", k.p, k.text);
            }
        }
    }
}

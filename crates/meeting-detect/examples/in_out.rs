//! Prints, every 2 s, which processes hold the mic and which play audio
//! (`cargo run -p meeting-detect --example in_out -- [seconds]`). Used to
//! check that a call app both captures and plays (2026-11-27).

fn names(r: meeting_detect::Result<Vec<meeting_detect::AudioClient>>) -> String {
    match r {
        Ok(c) if c.is_empty() => "-".into(),
        Ok(c) => c
            .iter()
            .map(|c| format!("{}({})", c.name, c.pid))
            .collect::<Vec<_>>()
            .join(", "),
        Err(e) => format!("error: {e}"),
    }
}

fn main() {
    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    for _ in 0..secs.div_ceil(2) {
        println!(
            "in: {} | out: {}",
            names(meeting_detect::audio_input_clients()),
            names(meeting_detect::audio_output_clients())
        );
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

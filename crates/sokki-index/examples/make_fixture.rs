//! Writes a made-up Sokki data folder (macOS layout) to the given path:
//! `cargo run --example make_fixture --features fixtures -- DIR [ACCESS]`.
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = args.next().ok_or_else(|| anyhow::anyhow!("usage: make_fixture DIR [off|summaries|transcripts|dictations]"))?;
    let access = args.next().unwrap_or_else(|| "transcripts".into());
    sokki_index::fixtures::write(std::path::Path::new(&dir), &access)?;
    println!("{dir}: {} made-up meetings, 2 dictations, agent_access {access}", sokki_index::fixtures::MEETINGS.len());
    Ok(())
}

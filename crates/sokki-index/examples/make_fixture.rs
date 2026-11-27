//! Writes a made-up Sokki data folder (macOS layout) to the given path:
//! `cargo run --example make_fixture --features fixtures -- DIR`.
fn main() -> anyhow::Result<()> {
    let dir = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: make_fixture DIR"))?;
    sokki_index::fixtures::write(std::path::Path::new(&dir))?;
    println!("{dir}: {} made-up meetings, 2 dictations", sokki_index::fixtures::MEETINGS.len());
    Ok(())
}

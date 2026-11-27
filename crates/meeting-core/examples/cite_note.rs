//! Prints every cited summary point of a note with the transcript lines it
//! cites (`meeting_core::cite`), for checking a note by eye.
//!   cargo run --example cite_note -- NOTE.md
fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("NOTE.md");
    let md = std::fs::read_to_string(path)?;
    let p = meeting_core::note::split_note(&md);
    let summary: Vec<&str> = p.summary.split('\n').collect();
    let transcript: Vec<&str> = p.transcript.split('\n').collect();
    let t0 = std::time::Instant::now();
    let cites = meeting_core::cite::cite_note(&p.summary, &p.transcript);
    let took = t0.elapsed();
    let points = summary
        .iter()
        .filter(|l| l.trim_start().starts_with("- "))
        .count();
    for c in &cites {
        println!("{}", summary[c.line as usize]);
        for &l in &c.lines {
            println!("    {}", transcript[l as usize]);
        }
    }
    println!("cited {} of {points} points in {took:?}", cites.len());
    Ok(())
}

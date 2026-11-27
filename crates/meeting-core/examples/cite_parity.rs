//! Reads {"points": [[text, owner|null]], "lines": [[speaker, text]]} on stdin, prints the
//! citation of each point as JSON: parity check against scripts/summary-citations/cite.py.
use std::io::Read;
fn main() -> anyhow::Result<()> {
    let mut s = String::new();
    std::io::stdin().read_to_string(&mut s)?;
    let v: serde_json::Value = serde_json::from_str(&s)?;
    let points: Vec<(String, Option<String>)> = serde_json::from_value(v["points"].clone())?;
    let lines: Vec<(String, String)> = serde_json::from_value(v["lines"].clone())?;
    let l: Vec<(&str, &str)> = lines
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    println!(
        "{}",
        serde_json::to_string(&meeting_core::cite::cite_points(&points, &l))?
    );
    Ok(())
}

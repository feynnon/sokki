//! Dictates a WAV (16 kHz mono 16-bit) into a Sokki on a Mac in real time,
//! as the phone keyboard does, and prints how fast the text came back.
//!
//!   cargo run --example dictate -- <phone key hex> <Mac id> <clip.wav> [--now]
//!
//! The phone key must be paired with that Mac (its phone_link.json). Without
//! `--now` it waits 30 s after connecting, as a keyboard that has been open a
//! while; `--now` dictates right after connecting.
use std::time::{Duration, Instant};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let key = sokki_link::secret_from_hex(&a[1])?;
    let mac: sokki_link::iroh::EndpointId = a[2].parse()?;
    let bytes = std::fs::read(&a[3])?;
    let samples: Vec<i16> = bytes[44..]
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .chain(std::iter::repeat_n(0, 16_000 * 8 / 10))
        .collect();
    let audio_s = samples.len() as f64 / 16_000.0;
    let ep = sokki_link::bind(key).await?;
    let t = Instant::now();
    let conn = sokki_link::connect(&ep, mac.into()).await?;
    sokki_link::call_on(&conn, &sokki_link::Request::Ping, &[]).await?;
    println!("connected and pinged in {} ms", t.elapsed().as_millis());
    if !a.iter().any(|x| x == "--now") {
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let start = Instant::now();
    tokio::spawn(async move {
        for (i, block) in samples.chunks(3_200).enumerate() {
            let due = Duration::from_secs_f64((i + 1) as f64 * 0.2);
            if let Some(wait) = due.checked_sub(start.elapsed()) {
                tokio::time::sleep(wait).await;
            }
            let _ = tx.send(block.to_vec());
        }
    });
    let (mut first, mut lags, mut finished) = (None, vec![], None);
    let mut text = String::new();
    sokki_link::dictate(&conn, rx, |u| {
        let now = start.elapsed().as_secs_f64();
        if u.is_final {
            finished = Some(now - audio_s);
            text = u.text;
        } else if !u.text.is_empty() {
            first.get_or_insert(now);
            lags.push(now - u.heard as f64 / 16_000.0);
        }
    })
    .await?;
    lags.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "first words {:.2} s; behind speech median {:.2} s, worst {:.2} s; done {:.2} s after stop",
        first.unwrap_or(f64::NAN),
        lags.get(lags.len() / 2).copied().unwrap_or(f64::NAN),
        lags.last().copied().unwrap_or(f64::NAN),
        finished.unwrap_or(f64::NAN)
    );
    println!("final: {text}");
    Ok(())
}

//! A stand-in phone for the live meeting path (test tool): pairs with a Mac
//! from its link, then sends a file as a meeting that records, in pieces
//! over one kept connection, as the phone app does, and prints how long
//! each request takes.
//!
//!     cargo run --example phone_live -- '<sokki://pair#… link>' <file> [piece bytes] [ms between]
use std::time::{Duration, Instant};

use sokki_link::{Request, Ticket};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let link = args.next().expect("pairing link");
    let file = args.next().expect("audio file");
    let piece: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(20_000);
    let every: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1000);
    let bytes = std::fs::read(&file)?;
    let ticket = Ticket::parse(link.split('#').nth(1).unwrap_or(&link))?;
    let ep = sokki_link::bind(sokki_link::new_secret()).await?;
    let t = Instant::now();
    sokki_link::call(&ep, ticket.mac, &Request::Pair { code: ticket.code.clone(), device: "sim".into() }, &[]).await?;
    println!("paired in {} ms", t.elapsed().as_millis());
    let conn = sokki_link::connect(&ep, ticket.mac.into()).await?;
    let t = Instant::now();
    let v = sokki_link::call_on(&conn, &Request::LiveStart { started_at: "2026-11-26T23:00:00Z".into(), mime: "audio/webm".into() }, &[]).await?;
    let id = v["id"].as_str().unwrap().to_string();
    println!("live start {id} in {} ms", t.elapsed().as_millis());
    let mut sent = 0usize;
    for chunk in bytes.chunks(piece) {
        let t = Instant::now();
        let v = tokio::time::timeout(
            Duration::from_secs(30),
            sokki_link::call_on(&conn, &Request::LivePart { id: id.clone(), offset: sent as u64 }, chunk),
        )
        .await;
        match v {
            Ok(Ok(v)) => println!("part at {sent}: have {} in {} ms", v["have"], t.elapsed().as_millis()),
            Ok(Err(e)) => println!("part at {sent}: error {e:#}"),
            Err(_) => { println!("part at {sent}: no answer in 30 s"); break; }
        }
        sent += chunk.len();
        tokio::time::sleep(Duration::from_millis(every)).await;
    }
    let t = Instant::now();
    let v = sokki_link::call_on(&conn, &Request::LiveEnd { id, total: bytes.len() as u64 }, &[]).await?;
    println!("live end {v} in {} ms", t.elapsed().as_millis());
    Ok(())
}

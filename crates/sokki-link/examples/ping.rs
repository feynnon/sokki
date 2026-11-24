//! Checks the link to a Mac from this computer, the way a phone would.
//!
//!     cargo run --example ping -- <Mac key>
//!
//! Uses a fresh key each time, so it is never paired and pairs nothing: a
//! working Mac answers "This phone isn't paired with this Mac". With
//! SOKKI_KEY_FILE set to a phone's `link_key.txt`, it pings as that phone,
//! and a paired phone gets the Mac's name. Prints how long the answer took,
//! or the error.

use std::time::Instant;

use sokki_link::{iroh::EndpointId, Request};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mac: EndpointId = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: ping <Mac key>"))?
        .parse()?;
    // SOKKI_KEY_FILE: a phone's link_key.txt, to check that phone's pairing.
    let secret = match std::env::var("SOKKI_KEY_FILE") {
        Ok(path) => sokki_link::secret_from_hex(&std::fs::read_to_string(path)?)?,
        Err(_) => sokki_link::new_secret(),
    };
    let endpoint = sokki_link::bind(secret).await?;
    for round in 1..=3 {
        let started = Instant::now();
        let answer = sokki_link::call(&endpoint, mac, &Request::Ping, &[]).await;
        let ms = started.elapsed().as_millis();
        match answer {
            Ok(v) => println!("{round}: answered in {ms} ms: {v}"),
            Err(e) => println!("{round}: after {ms} ms: {e:#}"),
        }
    }
    endpoint.close().await;
    Ok(())
}

//! A stand-in Mac for testing the phone's "Send to Mac" of the keyboard
//! journal (developer page "Keep what I write") without Sokki on the Mac.
//!
//!     cargo run --example journal_mac -- <folder>
//!
//! Prints a pairing link (open it on the phone or emulator:
//! `adb shell am start -a android.intent.action.VIEW -d '<link>'`), pairs any
//! phone with that code, answers pings, and writes each received journal day
//! to <folder>/<file>. Ctrl-C ends it. Test tool only; the real Mac side is
//! `src-tauri/src/meeting/phone_link.rs`.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};
use sokki_link::{Handler, Request, Ticket};

struct Fake {
    dir: PathBuf,
    code: String,
}

impl Handler for Fake {
    fn is_paired(&self, _peer: &str) -> bool {
        true
    }
    fn pair(&self, _peer: &str, code: &str, device: &str) -> Result<Value, String> {
        if code.trim().to_uppercase() != self.code {
            return Err("Wrong pairing code.".into());
        }
        println!("paired {device}");
        Ok(json!({ "name": "Test Mac", "version": "test" }))
    }
    fn handle(&self, _peer: &str, request: Request, body: Vec<u8>) -> Result<Value, String> {
        match request {
            Request::Ping => Ok(json!({ "name": "Test Mac", "version": "test" })),
            Request::Journal { file } => {
                if file.contains('/') || !file.starts_with("journal-") {
                    return Err(format!("bad name {file}"));
                }
                std::fs::write(self.dir.join(&file), &body).map_err(|e| e.to_string())?;
                println!("journal {file}: {} bytes", body.len());
                Ok(json!({ "saved": body.len() }))
            }
            other => Err(format!("not in this test: {other:?}")),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let dir: PathBuf = std::env::args().nth(1).ok_or_else(|| anyhow::anyhow!("usage: journal_mac <folder>"))?.into();
    std::fs::create_dir_all(&dir)?;
    let endpoint = sokki_link::bind(sokki_link::new_secret()).await?;
    let code = "TESTCODE".to_string();
    let ticket = Ticket { mac: endpoint.id(), code: code.clone() };
    println!("pair: {}", ticket.url().replacen("https://sokki.org/p#", "sokki://pair#", 1));
    let _router = sokki_link::serve(endpoint, Arc::new(Fake { dir, code }));
    tokio::signal::ctrl_c().await?;
    Ok(())
}

//! A stand-in Mac for testing the phone's meeting path without Sokki on the
//! Mac: the phone sends a recording, follows the progress, fetches the note
//! by itself and shows "Note ready" (mobile: App.tsx, NoteCheck.kt).
//!
//!     cargo run --example meeting_mac -- [seconds]
//!
//! Prints a pairing link (open it on the phone or emulator:
//! `adb shell am start -a android.intent.action.VIEW -d '<link>'`), pairs any
//! phone with that code and takes recordings. Each one goes through the
//! Mac's stages over `seconds` (default 40): waiting, finding speakers,
//! transcribing (with progress), writing the summary; then it is done with
//! a made-up note. Ctrl-C ends it. Test tool only; the real Mac side is
//! `src-tauri/src/meeting/phone_link.rs`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{json, Value};
use sokki_link::{Handler, Request, Ticket};

struct Fake {
    code: String,
    seconds: f32,
    meetings: Mutex<HashMap<String, Instant>>,
}

const SUMMARY: &str = "## Summary\n- **Start:** the new website goes live on 15 January.\n- The team writes the texts, the agency makes the pictures.\n## Tasks\n- [ ] Lena: send the picture list to the agency\n- [ ] Jonas: book a date for the sign-off";
const TRANSCRIPT: &str = "Lena: Let's start. The goal is to go live on 15 January.\nJonas: That works if the pictures are here by mid-December.\nMara: We write the texts ourselves, the agency only makes the pictures.";

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
            Request::Upload { mime, .. } => {
                let mut m = self.meetings.lock().unwrap();
                let id = format!("test-{}", m.len() + 1);
                m.insert(id.clone(), Instant::now());
                println!("upload {id}: {} bytes {mime}", body.len());
                Ok(json!({ "id": id }))
            }
            Request::Status { id } => {
                let start = *self.meetings.lock().unwrap().get(&id).ok_or("No such meeting.")?;
                let t = start.elapsed().as_secs_f32() / self.seconds;
                let (status, stage, progress) = match t {
                    t if t < 0.15 => ("processing", Some("waiting"), None),
                    t if t < 0.3 => ("processing", Some("diarizing"), None),
                    t if t < 0.8 => ("processing", Some("transcribing"), Some((t - 0.3) / 0.5)),
                    t if t < 1.0 => ("processing", Some("summarizing"), None),
                    _ => ("done", None, None),
                };
                println!("status {id}: {status} {stage:?}");
                let done = status == "done";
                Ok(json!({
                    "id": id,
                    "status": status,
                    "stage": stage,
                    "progress": progress,
                    "error": null,
                    "title": if done { Some("Website relaunch kickoff") } else { None },
                    "summary": if done { Some(SUMMARY) } else { None },
                    "transcript": if done { Some(TRANSCRIPT) } else { None },
                }))
            }
            other => Err(format!("not in this test: {other:?}")),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let seconds: f32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(40.0);
    let endpoint = sokki_link::bind(sokki_link::new_secret()).await?;
    let code = "TESTCODE".to_string();
    let ticket = Ticket { mac: endpoint.id(), code: code.clone() };
    println!("pair: {}", ticket.url().replacen("https://sokki.org/p#", "sokki://pair#", 1));
    let _router = sokki_link::serve(
        endpoint,
        Arc::new(Fake { code, seconds, meetings: Mutex::new(HashMap::new()) }),
    );
    tokio::signal::ctrl_c().await?;
    Ok(())
}

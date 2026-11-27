//! A stand-in Mac for testing the phone's meeting path without Sokki on the
//! Mac: the phone sends a recording, follows the progress, fetches the note
//! by itself and shows "Note ready" (mobile: App.tsx, NoteCheck.kt).
//!
//!     cargo run --example meeting_mac -- [seconds] [nolive]
//!
//! Prints a pairing link (open it on the phone or emulator:
//! `adb shell am start -a android.intent.action.VIEW -d '<link>'`), pairs any
//! phone with that code and takes recordings. Each one goes through the
//! Mac's stages over `seconds` (default 40): waiting, finding speakers,
//! transcribing (with progress), writing the summary; then it is done with
//! a made-up note. A meeting sent while it records (`LiveStart`, `LivePart`,
//! `LiveEnd`) is kept byte for byte and goes through the stages in a fifth
//! of the time after its end, as the Mac's live transcript makes it.
//! `nolive`: answers live requests with an error, as a Mac without them.
//! `MEETING_MAC_KEY=<file>` keeps its key across restarts. Ctrl-C ends it. Test tool only; the real Mac side is
//! `src-tauri/src/meeting/phone_link.rs`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{json, Value};
use sokki_link::{Handler, Request, Ticket};

struct Fake {
    code: String,
    seconds: f32,
    live_ok: bool,
    /// Meeting → (processing started, seconds it takes).
    meetings: Mutex<HashMap<String, (Instant, f32)>>,
    /// Live meetings still being sent → bytes so far.
    live: Mutex<HashMap<String, Vec<u8>>>,
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
                m.insert(id.clone(), (Instant::now(), self.seconds));
                println!("upload {id}: {} bytes {mime}", body.len());
                Ok(json!({ "id": id }))
            }
            Request::Status { id } => {
                let (start, secs) = *self.meetings.lock().unwrap().get(&id).ok_or("No such meeting.")?;
                let t = start.elapsed().as_secs_f32() / secs;
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
            Request::LiveStart { mime, .. } if self.live_ok => {
                let mut l = self.live.lock().unwrap();
                let n = self.meetings.lock().unwrap().len() + l.len() + 1;
                let id = format!("live-{n}");
                l.insert(id.clone(), Vec::new());
                println!("live start {id}: {mime}");
                Ok(json!({ "id": id }))
            }
            Request::LivePart { id, offset } if self.live_ok => {
                let mut l = self.live.lock().unwrap();
                let have = l.get_mut(&id).ok_or("No such live meeting.")?;
                let end = offset + body.len() as u64;
                if offset <= have.len() as u64 && end > have.len() as u64 {
                    let skip = have.len() - offset as usize;
                    have.extend_from_slice(&body[skip..]);
                } else {
                    println!("live part {id}: offset {offset} doesn't continue {}", have.len());
                }
                println!("live part {id}: have {}", have.len());
                Ok(json!({ "have": have.len() }))
            }
            Request::LiveEnd { id, total } if self.live_ok => {
                let mut l = self.live.lock().unwrap();
                let have = l.get(&id).ok_or("No such live meeting.")?.len() as u64;
                if have < total {
                    println!("live end {id}: have {have} of {total}");
                    return Ok(json!({ "have": have }));
                }
                let bytes = l.remove(&id).unwrap();
                let path = std::env::temp_dir().join(format!("meeting_mac-{id}.webm"));
                let _ = std::fs::write(&path, &bytes);
                println!("live end {id}: complete, {} bytes -> {}", bytes.len(), path.display());
                self.meetings.lock().unwrap().insert(id.clone(), (Instant::now(), self.seconds / 5.0));
                Ok(json!({ "have": have, "id": id }))
            }
            other => Err(format!("not in this test: {other:?}")),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let seconds: f32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(40.0);
    let live_ok = std::env::args().nth(2).as_deref() != Some("nolive");
    // MEETING_MAC_KEY=<file>: the same key after a restart (the phone stays paired).
    let secret = match std::env::var("MEETING_MAC_KEY") {
        Ok(path) => match std::fs::read_to_string(&path).ok().and_then(|t| sokki_link::secret_from_hex(t.trim()).ok()) {
            Some(k) => k,
            None => {
                let k = sokki_link::new_secret();
                std::fs::write(&path, sokki_link::secret_to_hex(&k))?;
                k
            }
        },
        Err(_) => sokki_link::new_secret(),
    };
    let endpoint = sokki_link::bind(secret).await?;
    let code = "TESTCODE".to_string();
    let ticket = Ticket { mac: endpoint.id(), code: code.clone() };
    println!("pair: {}", ticket.url().replacen("https://sokki.org/p#", "sokki://pair#", 1));
    let _router = sokki_link::serve(
        endpoint,
        Arc::new(Fake {
            code,
            seconds,
            live_ok,
            meetings: Mutex::new(HashMap::new()),
            live: Mutex::new(HashMap::new()),
        }),
    );
    tokio::signal::ctrl_c().await?;
    Ok(())
}

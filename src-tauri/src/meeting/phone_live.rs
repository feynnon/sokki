//! Meetings the phone sends while it records (sokki-link `LiveStart`,
//! `LivePart`, `LiveEnd`), so they are ready soon after the stop like a
//! meeting recorded here (Robin, 26 Nov 2026: "transcript, speakers,
//! summary might all be available in very short time after a meeting
//! ends").
//!
//! The phone's MediaRecorder hands it a piece of WebM/Opus every 5 s; it
//! sends each one. Here the pieces are appended to the meeting's
//! `mic.phone` and decoded as they come ([`decode_stream`]) into a live
//! session ([`super::live::Live`]), the same one a recording here feeds, so
//! at the stop only the last piece, the speakers and the summary are left.
//! Decoding the pieces gives what decoding the finished file gives, sample
//! for sample.
//!
//! The phone keeps the whole recording until the end is confirmed: a piece
//! the Mac missed is sent again from the byte the Mac has (`have`). Nothing
//! shows here until the stop (a meeting "recording" in the list would be
//! taken for this Mac's own). A meeting nobody has sent to for two hours is
//! dropped, files and all; the phone then sends it whole (`Upload`).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use meeting_core::TrackKind;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use super::keep_awake::KeepAwake;
use super::phone_link::{phone_record, PHONE_TRACK};
use super::store::MeetingStore;
use crate::audio_toolkit::audio::decode_stream;

/// A phone meeting nobody has sent to for this long is dropped.
const GIVE_UP: Duration = Duration::from_secs(2 * 3600);

struct Stream {
    started_at: String,
    dir: PathBuf,
    file: std::fs::File,
    have: u64,
    /// Bytes for the decoder; dropped at the end, which ends its stream.
    bytes: Option<Sender<Vec<u8>>>,
    decoder: Option<JoinHandle<Result<f64>>>,
    live: Option<super::live::Live>,
    _awake: KeepAwake,
    opened: Instant,
    last: Instant,
    parts: u32,
    /// Pieces that didn't continue what was here (sent again, or a gap).
    off: u32,
}

static STREAMS: LazyLock<Mutex<HashMap<String, Stream>>> = LazyLock::new(Default::default);

/// The decoder's input: blocks until the next piece, ends when the sender
/// is dropped.
struct Pipe {
    rx: Mutex<Receiver<Vec<u8>>>,
    buf: Vec<u8>,
    pos: usize,
}

impl Read for Pipe {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        while self.pos >= self.buf.len() {
            match self.rx.lock().unwrap().recv() {
                Ok(b) => {
                    self.buf = b;
                    self.pos = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

fn extension(mime: &str) -> &'static str {
    if mime.contains("mp4") || mime.contains("aac") {
        "m4a"
    } else if mime.contains("ogg") {
        "ogg"
    } else {
        "webm"
    }
}

/// Drops phone meetings nobody has sent to for [`GIVE_UP`].
fn give_up_stale(streams: &mut HashMap<String, Stream>) {
    streams.retain(|id, s| {
        let keep = s.last.elapsed() < GIVE_UP;
        if !keep {
            log::warn!(
                "Phone link: live meeting {id} dropped, nothing since {} min; the phone sends it whole",
                s.last.elapsed().as_secs() / 60
            );
            let _ = std::fs::remove_dir_all(&s.dir);
        }
        keep
    });
}

/// Starts a phone meeting: its folder, the decoder and the live transcript.
pub(super) fn start(app: &AppHandle, started_at: &str, mime: &str) -> Result<String> {
    let store = app.state::<Arc<MeetingStore>>();
    let id = format!(
        "meeting-{}-phone",
        chrono::Local::now().format("%Y%m%d-%H%M%S-%3f")
    );
    let dir = store.audio_dir(&id);
    std::fs::create_dir_all(&dir)?;
    let file = std::fs::File::create(dir.join(PHONE_TRACK))?;
    // The phone's microphone starts with the meeting: no second track to
    // line up with.
    let first_us = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(1)
        .max(1);
    let live = super::live::Live::start_timed(
        app,
        false,
        &dir,
        Box::new(move || sokki_engine::tracks::TrackTiming {
            mic_first_us: first_us,
            system_first_us: 0,
        }),
    );
    let (tx, rx) = channel::<Vec<u8>>();
    let mut tee = live.as_ref().map(|l| l.tee(TrackKind::Mic));
    let ext = extension(mime);
    let name = format!("phone-live-{id}");
    let decoder = std::thread::Builder::new().name(name).spawn(move || {
        let pipe = Pipe {
            rx: Mutex::new(rx),
            buf: Vec::new(),
            pos: 0,
        };
        let source = symphonia::core::io::ReadOnlySource::new(pipe);
        decode_stream(Box::new(source), ext, |samples, rate| {
            if let Some(t) = tee.as_mut() {
                t(samples, rate);
            }
        })
    })?;
    let now = Instant::now();
    let mut streams = STREAMS.lock().unwrap();
    give_up_stale(&mut streams);
    streams.insert(
        id.clone(),
        Stream {
            started_at: started_at.to_string(),
            dir,
            file,
            have: 0,
            bytes: Some(tx),
            decoder: Some(decoder),
            live,
            _awake: KeepAwake::acquire(),
            opened: now,
            last: now,
            parts: 0,
            off: 0,
        },
    );
    log::info!(
        "Phone link: live meeting {id} started ({mime}, live transcript {})",
        if streams[&id].live.is_some() {
            "on"
        } else {
            "off"
        }
    );
    Ok(id)
}

/// Bytes `offset..` of the recording. Keeps what continues the bytes here.
pub(super) fn part(id: &str, offset: u64, body: &[u8]) -> Result<u64> {
    let mut streams = STREAMS.lock().unwrap();
    let s = streams
        .get_mut(id)
        .ok_or_else(|| anyhow!("No such live meeting."))?;
    s.last = Instant::now();
    s.parts += 1;
    let end = offset + body.len() as u64;
    if offset > s.have || end <= s.have {
        // A gap (an earlier piece is missing) or a piece already here.
        s.off += 1;
        return Ok(s.have);
    }
    let new = &body[(s.have - offset) as usize..];
    s.file.write_all(new)?;
    if let Some(tx) = &s.bytes {
        let _ = tx.send(new.to_vec());
    }
    s.have = end;
    Ok(s.have)
}

/// The recording is complete at `total` bytes. With all of it here, the
/// meeting goes to processing with its live transcript.
pub(super) fn end(app: &AppHandle, id: &str, total: u64) -> Result<Value> {
    let mut s = {
        let mut streams = STREAMS.lock().unwrap();
        let s = streams
            .get_mut(id)
            .ok_or_else(|| anyhow!("No such live meeting."))?;
        s.last = Instant::now();
        if s.have < total {
            return Ok(json!({ "have": s.have }));
        }
        streams.remove(id).expect("just found")
    };
    let stopped = Instant::now();
    s.file.flush()?;
    drop(s.bytes.take());
    let decoded = s.decoder.take().map(|d| d.join());
    let seconds = match decoded {
        Some(Ok(Ok(seconds))) => seconds,
        other => {
            // The live decoding failed: the file decides (processing decodes it anyway).
            log::warn!(
                "Phone link: live meeting {id}: live decoding failed ({other:?}); reading the file"
            );
            crate::audio_toolkit::audio::decode_audio_file(&s.dir.join(PHONE_TRACK))
                .map(|a| a.len() as f64 / 16_000.0)?
        }
    };
    let store = app.state::<Arc<MeetingStore>>();
    store.insert(phone_record(id, &s.started_at, seconds))?;
    log::info!(
        "Phone link: live meeting {id} complete: {:.0} s, {} KB in {} pieces ({} out of order), over {:.0} s; decoder done {} ms after the end",
        seconds,
        s.have / 1024,
        s.parts,
        s.off,
        s.opened.elapsed().as_secs_f64(),
        stopped.elapsed().as_millis()
    );
    super::emit_changed(app, id);
    super::process::spawn(app.clone(), id.to_string(), s.live.take());
    Ok(json!({ "have": s.have, "id": id }))
}

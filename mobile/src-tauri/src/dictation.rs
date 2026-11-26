//! On-device dictation. The page sends microphone audio (16 kHz, 16-bit,
//! base64) every ~200 ms; a worker thread reads it with Parakeet in pieces
//! (`pieces.rs`), as the keyboard does without a Mac, and emits the text
//! whenever it changes.
//!
//! Events: "dictation-piece" {index: 0, text: all so far, audio_s, ms},
//! "dictation-done" {after_stop_ms}, "dictation-download" {done, total}.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use base64::Engine as _;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};

/// Parakeet Ultra (25 European languages): what the Mac dictates with.
/// From 25 to 26 Nov 2026 the phone used Nemotron Streaming 3.5 (it can
/// stream, Parakeet can't), but on Robin's own dictations it got 17 % of
/// words different from the references against Parakeet's 10–12 %; Robin:
/// "I'd rather have high quality than a bit faster latency."
pub const MODEL_FILE: &str = "parakeet-ultra-Q8_0.gguf";
const MODEL_URL: &str = "https://huggingface.co/handy-computer/parakeet-ultra-gguf/resolve/39eeb55181f0d354fd934f06e92fd8d5037fed8e/parakeet-ultra-Q8_0.gguf";
/// The model before (Nemotron Streaming): deleted once Parakeet is in, and
/// a reason to fetch Parakeet by itself (the person chose on-device
/// dictation already).
pub const OLD_MODEL_FILE: &str = "nemotron-3.5-asr-streaming-0.6b-Q8_0.gguf";

/// CPU threads for the model: all cores but two, at most 4. The engine's
/// threads wait for each other, so when one is pushed aside (the screen,
/// the microphone, another app) all of them stall: on the emulator (4
/// cores, two other busy processes) 4 threads left the text 41 s behind,
/// 2 or 3 threads 0.25 s. The old keyboard's two passes ran 8 threads on
/// the Pixel's 8 cores and froze for ~3 s whenever both ran.
pub fn threads() -> i32 {
    std::thread::available_parallelism().map_or(2, |n| n.get().saturating_sub(2).clamp(1, 4) as i32)
}

/// Reads half a second of silence: touches the model and the session's
/// buffers, so the next dictation finds them in memory.
pub fn warm(session: &mut transcribe_cpp::Session) {
    let _ = session.run(
        &vec![0.0f32; 16_000 / 2],
        &transcribe_cpp::RunOptions::default(),
    );
}

pub struct Engine {
    /// The Dictate tab's session, made on its first use (the keyboard's
    /// process makes its own and shouldn't carry a second one).
    session: Mutex<Option<transcribe_cpp::Session>>,
    model: transcribe_cpp::Model,
}

impl Engine {
    /// Another session on the same model (shares the weights; own buffers),
    /// with `threads` CPU threads (0 = library default).
    pub fn new_session(&self, threads: i32) -> Result<transcribe_cpp::Session, String> {
        let options = transcribe_cpp::SessionOptions {
            n_threads: threads,
            ..Default::default()
        };
        self.model.session_with(&options).map_err(|e| e.to_string())
    }

    /// Runs `f` with the Dictate tab's session (made on first use).
    fn with_session<T>(
        &self,
        f: impl FnOnce(&mut transcribe_cpp::Session) -> T,
    ) -> Result<T, String> {
        let mut slot = self.session.lock().unwrap();
        if slot.is_none() {
            *slot = Some(self.new_session(threads())?);
        }
        Ok(f(slot.as_mut().unwrap()))
    }
}

/// The loaded model, shared by the Dictate tab and the keyboard (same
/// process): loaded once, kept while the app runs.
static ENGINE: Mutex<Option<Arc<Engine>>> = Mutex::new(None);

/// Loads the model at `path` (or returns the loaded one).
pub fn engine_at(path: &Path) -> Result<Arc<Engine>, String> {
    let mut slot = ENGINE.lock().unwrap();
    if let Some(e) = slot.as_ref() {
        return Ok(e.clone());
    }
    if !path.exists() {
        return Err("The speech model isn't downloaded yet.".into());
    }
    let _ = transcribe_cpp::init_backends_default();
    let model = transcribe_cpp::Model::load_with(path, &transcribe_cpp::ModelOptions::default())
        .map_err(|e| format!("Loading the speech model failed: {e}"))?;
    let e = Arc::new(Engine {
        session: Mutex::new(None),
        model,
    });
    *slot = Some(e.clone());
    Ok(e)
}

pub fn engine_loaded() -> bool {
    ENGINE.lock().unwrap().is_some()
}

enum Msg {
    Audio(Vec<f32>),
    End(Instant),
}

#[derive(Default)]
pub struct Dictation {
    feed: Mutex<Option<Sender<Msg>>>,
}

fn model_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("models");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(MODEL_FILE))
}

#[derive(serde::Serialize)]
pub struct ModelInfo {
    present: bool,
    /// The previous model is there: fetch the new one without asking.
    old_present: bool,
    path: String,
    loaded: bool,
}

#[tauri::command]
pub fn dictation_model(app: AppHandle) -> Result<ModelInfo, String> {
    let path = model_path(&app)?;
    Ok(ModelInfo {
        present: path.exists(),
        old_present: path.with_file_name(OLD_MODEL_FILE).exists(),
        path: path.display().to_string(),
        loaded: engine_loaded(),
    })
}

/// One download at a time: a second caller waits for the first and is done.
static DOWNLOADING: Mutex<()> = Mutex::new(());

/// Downloads the speech model (740 MB) into the app's storage, then
/// deletes the previous one.
fn download(app: &AppHandle, path: &Path) -> Result<(), String> {
    use std::io::{Read, Write};
    let _one = DOWNLOADING.lock().unwrap_or_else(|e| e.into_inner());
    if path.exists() {
        return Ok(());
    }
    let part = path.with_extension("part");
    let mut resp = ureq::get(MODEL_URL).call().map_err(|e| e.to_string())?;
    let total: u64 = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok()?.parse().ok())
        .unwrap_or(0);
    let mut reader = resp.body_mut().as_reader();
    let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 1 << 20];
    let (mut done, mut last) = (0u64, Instant::now());
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if last.elapsed().as_millis() > 300 {
            let _ = app.emit(
                "dictation-download",
                json!({ "done": done, "total": total }),
            );
            last = Instant::now();
        }
    }
    if total > 0 && done != total {
        return Err(format!("Download incomplete ({done} of {total} bytes)"));
    }
    std::fs::rename(&part, path).map_err(|e| e.to_string())?;
    // A process still using the old file keeps it until it lets go.
    let _ = std::fs::remove_file(path.with_file_name(OLD_MODEL_FILE));
    Ok(())
}

#[tauri::command]
pub async fn dictation_download(app: AppHandle) -> Result<(), String> {
    let path = model_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || download(&app, &path))
        .await
        .map_err(|e| e.to_string())?
}

/// At app start: whoever has the previous model gets the new one by
/// itself, in the background (the keyboard needs it).
pub fn update_model(app: &AppHandle) {
    let Ok(path) = model_path(app) else { return };
    let old = path.with_file_name(OLD_MODEL_FILE);
    if path.exists() {
        // Put there by hand (tests): the old one isn't needed either.
        let _ = std::fs::remove_file(old);
        return;
    }
    if !old.exists() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = download(&app, &path) {
            eprintln!("Updating the speech model failed: {e}");
        }
    });
}

fn engine(app: &AppHandle) -> Result<Arc<Engine>, String> {
    engine_at(&model_path(app)?)
}

/// Loads the model into memory (takes ~20 s on a Pixel 7, once).
#[tauri::command]
pub async fn dictation_load(app: AppHandle) -> Result<u64, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let t = Instant::now();
        // Warm, so the first dictation doesn't build the session.
        let e = engine(&app)?;
        e.with_session(warm)?;
        Ok(t.elapsed().as_millis() as u64)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Starts a dictation: a worker reads the audio in pieces (`pieces.rs`).
#[tauri::command]
pub async fn dictation_start(app: AppHandle) -> Result<(), String> {
    let e = {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || engine(&app))
            .await
            .map_err(|e| e.to_string())??
    };
    let (tx, rx) = channel::<Msg>();
    if let Some(old) = app.state::<Dictation>().feed.lock().unwrap().replace(tx) {
        let _ = old.send(Msg::End(Instant::now()));
    }
    let emitter = app.clone();
    std::thread::spawn(move || {
        let piece = |text: &str, heard: usize, ms: u128| {
            let _ = emitter.emit(
                "dictation-piece",
                json!({ "index": 0, "text": text, "audio_s": heard as f64 / 16_000.0, "ms": ms as u64 }),
            );
        };
        let done = |stopped: Instant| {
            let _ = emitter.emit(
                "dictation-done",
                json!({ "after_stop_ms": stopped.elapsed().as_millis() as u64 }),
            );
        };
        let result = e.with_session(|s| {
            let mut live = crate::pieces::Live::default();
            let mut shown = String::new();
            loop {
                // Everything that has come in, then one read if one is due.
                let mut stopped = None;
                loop {
                    match rx.try_recv() {
                        Ok(Msg::Audio(audio)) => live.push(&audio),
                        Ok(Msg::End(at)) => {
                            stopped = Some(at);
                            break;
                        }
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            stopped = Some(Instant::now());
                            break;
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    }
                }
                if let Some(at) = stopped {
                    let t = Instant::now();
                    let heard = live.heard;
                    let text = live.finish(s);
                    piece(&text, heard, t.elapsed().as_millis());
                    return Some(at);
                }
                let t = Instant::now();
                match live.step(s) {
                    Some(update) => {
                        let text = update.text();
                        if text != shown {
                            piece(&text, live.heard, t.elapsed().as_millis());
                            shown = text;
                        }
                    }
                    None => match rx.recv_timeout(std::time::Duration::from_millis(20)) {
                        Ok(Msg::Audio(audio)) => live.push(&audio),
                        Ok(Msg::End(at)) => {
                            let t = Instant::now();
                            let heard = live.heard;
                            let text = live.finish(s);
                            piece(&text, heard, t.elapsed().as_millis());
                            return Some(at);
                        }
                        Err(_) => {}
                    },
                }
            }
        });
        // The page waits for "done" in every case.
        done(result.ok().flatten().unwrap_or_else(Instant::now));
    });
    Ok(())
}

/// Microphone audio: 16 kHz mono, 16-bit little-endian, base64.
#[tauri::command]
pub fn dictation_push(state: State<'_, Dictation>, pcm: String) -> Result<(), String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(pcm)
        .map_err(|e| e.to_string())?;
    let samples: Vec<f32> = bytes
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
        .collect();
    let feed = state.feed.lock().unwrap();
    let tx = feed.as_ref().ok_or("Not dictating.")?;
    let _ = tx.send(Msg::Audio(samples));
    Ok(())
}

#[tauri::command]
pub fn dictation_stop(state: State<'_, Dictation>) -> Result<(), String> {
    let tx = state.feed.lock().unwrap().take().ok_or("Not dictating.")?;
    let _ = tx.send(Msg::End(Instant::now()));
    Ok(())
}

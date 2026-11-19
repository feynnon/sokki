//! On-device dictation. The page sends microphone audio (16 kHz, 16-bit,
//! base64) every ~200 ms; `dictation-core` cuts it at pauses; a worker
//! thread transcribes each piece with Parakeet as soon as it is cut and
//! emits it. When the person stops, only the last piece is left.
//!
//! Events: "dictation-piece" {index, text, audio_s, ms}, "dictation-done"
//! {after_stop_ms}, "dictation-download" {done, total}.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use base64::Engine as _;
use dictation_core::{Mode, Params, Segmenter};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};

pub const MODEL_FILE: &str = "parakeet-ultra-Q8_0.gguf";
const MODEL_URL: &str =
    "https://huggingface.co/handy-computer/parakeet-ultra-gguf/resolve/main/parakeet-ultra-Q8_0.gguf";

pub struct Engine {
    session: Mutex<transcribe_cpp::Session>,
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

    /// Transcribes one piece of 16 kHz mono audio.
    pub fn run(&self, audio: &[f32]) -> String {
        let opts = transcribe_cpp::RunOptions::default();
        let mut s = self.session.lock().unwrap();
        s.run(audio, &opts)
            .map(|r| r.text.trim().to_string())
            .unwrap_or_default()
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
    let session = model.session().map_err(|e| e.to_string())?;
    let e = Arc::new(Engine {
        session: Mutex::new(session),
        model,
    });
    *slot = Some(e.clone());
    Ok(e)
}

pub fn engine_loaded() -> bool {
    ENGINE.lock().unwrap().is_some()
}

enum Job {
    Piece(usize, Vec<f32>),
    End(Instant),
}

struct Live {
    segmenter: Segmenter,
    tx: Sender<Job>,
    next: usize,
}

#[derive(Default)]
pub struct Dictation {
    live: Mutex<Option<Live>>,
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
    path: String,
    loaded: bool,
}

#[tauri::command]
pub fn dictation_model(app: AppHandle) -> Result<ModelInfo, String> {
    let path = model_path(&app)?;
    Ok(ModelInfo {
        present: path.exists(),
        path: path.display().to_string(),
        loaded: engine_loaded(),
    })
}

/// Downloads the speech model (740 MB) into the app's storage.
#[tauri::command]
pub async fn dictation_download(app: AppHandle) -> Result<(), String> {
    let path = model_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        use std::io::{Read, Write};
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
                let _ = app.emit("dictation-download", json!({ "done": done, "total": total }));
                last = Instant::now();
            }
        }
        if total > 0 && done != total {
            return Err(format!("Download incomplete ({done} of {total} bytes)"));
        }
        std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn engine(app: &AppHandle) -> Result<Arc<Engine>, String> {
    engine_at(&model_path(app)?)
}

/// Loads the model into memory (takes ~20 s on a Pixel 7, once).
#[tauri::command]
pub async fn dictation_load(app: AppHandle) -> Result<u64, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let t = Instant::now();
        engine(&app).map(|_| t.elapsed().as_millis() as u64)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// `min_s` / `max_s` override the piece lengths (for tests).
#[tauri::command]
pub async fn dictation_start(
    app: AppHandle,
    min_s: Option<f32>,
    max_s: Option<f32>,
) -> Result<(), String> {
    let e = {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || engine(&app))
            .await
            .map_err(|e| e.to_string())??
    };
    let (tx, rx) = channel::<Job>();
    let emitter = app.clone();
    std::thread::spawn(move || {
        for job in rx {
            match job {
                Job::Piece(index, audio) => {
                    let t = Instant::now();
                    // Every piece is transcribed: cutting is only for speed,
                    // no audio is ever skipped.
                    let text = e.run(&audio);
                    let _ = emitter.emit(
                        "dictation-piece",
                        json!({
                            "index": index,
                            "text": text,
                            "audio_s": audio.len() as f64 / 16_000.0,
                            "ms": t.elapsed().as_millis() as u64,
                        }),
                    );
                }
                Job::End(stopped) => {
                    let _ = emitter.emit(
                        "dictation-done",
                        json!({ "after_stop_ms": stopped.elapsed().as_millis() as u64 }),
                    );
                    break;
                }
            }
        }
    });
    *app.state::<Dictation>().live.lock().unwrap() = Some(Live {
        segmenter: Segmenter::new(Mode::Pauses({
            let d = Params::default();
            Params {
                min_s: min_s.unwrap_or(d.min_s),
                max_s: max_s.unwrap_or(d.max_s),
                ..d
            }
        })),
        tx,
        next: 0,
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
    let mut guard = state.live.lock().unwrap();
    let live = guard.as_mut().ok_or("Not dictating.")?;
    for piece in live.segmenter.push(&samples) {
        let _ = live.tx.send(Job::Piece(live.next, piece));
        live.next += 1;
    }
    Ok(())
}

#[tauri::command]
pub fn dictation_stop(state: State<'_, Dictation>) -> Result<(), String> {
    let stopped = Instant::now();
    let mut live = state.live.lock().unwrap().take().ok_or("Not dictating.")?;
    if let Some(rest) = live.segmenter.finish() {
        let _ = live.tx.send(Job::Piece(live.next, rest));
    }
    let _ = live.tx.send(Job::End(stopped));
    Ok(())
}

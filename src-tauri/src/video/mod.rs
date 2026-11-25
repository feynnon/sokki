//! "Meeting with screen": a meeting recording (microphone and system sound,
//! so it gets its note like any meeting) plus a full video of the screen.
//! After Stop, Sokki puts the screen video and the meeting's sound together
//! into one `.mp4`.
//!
//! Files live in `~/Movies/Sokki/<date time>/`: `screen.mov`, copies of the
//! audio tracks, the finished `<date time>.mp4` and `video.json` (this
//! module's record of it). The meeting's own audio may be deleted after its
//! note is written; the copies here stay.
//!
//! Test mode (`SOKKI_TEST_SCREEN` = a video file): no capture and no
//! permission prompt; at Stop that file is copied in as if recorded.

mod bridge;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use crate::meeting::store::MeetingStore;
use crate::meeting::{TrackTiming, MIC_FILE, SYSTEM_FILE, TIMING_FILE};

const META_FILE: &str = "video.json";
const SCREEN_FILE: &str = "screen.mov";

/// One recorded track in the video's folder.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct VideoTrack {
    /// File name inside the folder.
    pub file: String,
    /// Wall-clock microseconds of the first frame or sample.
    pub first_us: u64,
}

/// A meeting's screen video, as kept in its folder's `video.json`.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct VideoMeta {
    pub folder: String,
    pub meeting_id: String,
    pub started_at: String,
    pub duration_s: f64,
    pub screen: Option<VideoTrack>,
    #[serde(default)]
    pub audio: Vec<VideoTrack>,
    /// The finished file's name, once there is one.
    pub rendered: Option<String>,
    /// "recording", "rendering", "ready" or "failed".
    pub status: String,
    pub error: Option<String>,
}

/// Sent whenever a screen video changes (recording, rendering, ready, failed).
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct VideoChanged {
    pub folder: String,
}

enum Capture {
    Real(bridge::Handle),
    Test { started_us: u64 },
}

struct Active {
    capture: Capture,
    meta: VideoMeta,
}

/// The screen recording in progress, if any.
#[derive(Default)]
pub struct VideoManager {
    active: Mutex<Option<Active>>,
}

impl VideoManager {
    pub fn is_recording(&self) -> bool {
        self.active.lock().map(|a| a.is_some()).unwrap_or(false)
    }
}

fn now_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

fn test_file() -> Option<PathBuf> {
    std::env::var_os("SOKKI_TEST_SCREEN")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
}

/// `~/Movies/Sokki`.
pub fn videos_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("Movies")
        .join("Sokki")
}

fn emit(app: &AppHandle, folder: &str) {
    let _ = VideoChanged {
        folder: folder.to_string(),
    }
    .emit(app);
}

fn write_meta(meta: &VideoMeta) {
    let path = Path::new(&meta.folder).join(META_FILE);
    match serde_json::to_string_pretty(meta) {
        Ok(text) => {
            if let Err(e) = std::fs::write(&path, text) {
                log::warn!("Screen video: {} could not be written: {e}", path.display());
            }
        }
        Err(e) => log::warn!("Screen video: metadata could not be serialized: {e}"),
    }
}

fn read_meta(folder: &Path) -> Option<VideoMeta> {
    let text = std::fs::read_to_string(folder.join(META_FILE)).ok()?;
    let mut meta: VideoMeta = serde_json::from_str(&text).ok()?;
    // The folder may have been moved or renamed in Finder.
    meta.folder = folder.to_string_lossy().into_owned();
    Some(meta)
}

/// Whether the screen may be recorded. Never asks.
pub fn screen_allowed() -> bool {
    test_file().is_some() || bridge::sources()["screen"] == "allowed"
}

/// Asks macOS for Screen Recording. True when already allowed.
pub fn request_screen() -> bool {
    test_file().is_some() || bridge::request_screen()
}

/// Starts a meeting with its screen. Returns the meeting id. Without Screen
/// Recording yet, macOS is asked (it takes effect after Sokki reopens) and
/// the error is "screen_not_allowed"; nothing is recorded then.
pub fn start(app: &AppHandle, source_app: Option<String>) -> Result<String, String> {
    let manager = app.state::<Arc<VideoManager>>();
    if manager.is_recording() {
        return Err("A recording is already running.".into());
    }
    let test = test_file().is_some();
    if !test && !screen_allowed() && !bridge::request_screen() {
        return Err("screen_not_allowed".into());
    }

    let now = chrono::Local::now();
    let name = now.format("%Y-%m-%d %H.%M.%S").to_string();
    let folder = videos_dir().join(&name);
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;

    let capture = if test {
        Capture::Test {
            started_us: now_us(),
        }
    } else {
        let config = serde_json::json!({
            "screen": { "display": null, "path": folder.join(SCREEN_FILE) },
        });
        match bridge::start(&config) {
            Ok(h) => Capture::Real(h),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&folder);
                return Err(e);
            }
        }
    };

    crate::usage::count(app, crate::usage::Count::MeetingWithScreen);
    let meeting_id = match crate::meeting::start_meeting(app, source_app) {
        Ok(id) => id,
        Err(e) => {
            if let Capture::Real(h) = capture {
                bridge::stop(h);
            }
            let _ = std::fs::remove_dir_all(&folder);
            return Err(e);
        }
    };

    let meta = VideoMeta {
        folder: folder.to_string_lossy().into_owned(),
        meeting_id: meeting_id.clone(),
        started_at: now.to_rfc3339(),
        duration_s: 0.0,
        screen: None,
        audio: Vec::new(),
        rendered: None,
        status: "recording".into(),
        error: None,
    };
    write_meta(&meta);
    log::info!("Screen video {name}: recording with meeting {meeting_id}");
    *manager.active.lock().unwrap_or_else(|e| e.into_inner()) = Some(Active {
        capture,
        meta: meta.clone(),
    });
    emit(app, &meta.folder);
    Ok(meeting_id)
}

/// Called by `meeting::stop_meeting` once the audio is closed: stops the
/// capture, copies the audio tracks next to the video and starts rendering.
pub fn on_meeting_stopped(app: &AppHandle, meeting_id: &str, duration_s: f64) {
    let manager = app.state::<Arc<VideoManager>>();
    let active = {
        let mut slot = manager.active.lock().unwrap_or_else(|e| e.into_inner());
        match slot.as_ref() {
            Some(a) if a.meta.meeting_id == meeting_id => slot.take(),
            _ => None,
        }
    };
    let Some(Active { capture, mut meta }) = active else {
        return;
    };
    let folder = PathBuf::from(&meta.folder);
    meta.duration_s = duration_s;

    match capture {
        Capture::Real(handle) => {
            let report = bridge::stop(handle);
            let first = report["screen"]["first_us"].as_u64().unwrap_or(0);
            if first > 0 {
                meta.screen = Some(VideoTrack {
                    file: SCREEN_FILE.into(),
                    first_us: first,
                });
            }
            if let Some(errors) = report["errors"].as_array() {
                for e in errors.iter().filter_map(|e| e.as_str()) {
                    log::warn!("Screen video {}: {e}", meta.folder);
                }
                if meta.screen.is_none() {
                    meta.error = errors.first().and_then(|e| e.as_str()).map(String::from);
                }
            }
        }
        Capture::Test { started_us } => {
            if let Some(src) = test_file() {
                if std::fs::copy(&src, folder.join(SCREEN_FILE)).is_ok() {
                    meta.screen = Some(VideoTrack {
                        file: SCREEN_FILE.into(),
                        first_us: started_us,
                    });
                }
            }
        }
    }

    // The meeting's audio may be deleted once its note is written; the
    // video keeps its own copy.
    let store = app.state::<Arc<MeetingStore>>();
    let audio_dir = store.audio_dir(meeting_id);
    let timing: TrackTiming = std::fs::read_to_string(audio_dir.join(TIMING_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    for (file, first) in [
        (MIC_FILE, timing.mic_first_us),
        (SYSTEM_FILE, timing.system_first_us),
    ] {
        let src = audio_dir.join(file);
        if first > 0 && src.exists() && std::fs::copy(&src, folder.join(file)).is_ok() {
            meta.audio.push(VideoTrack {
                file: file.into(),
                first_us: first,
            });
        }
    }

    if meta.screen.is_none() {
        meta.status = "failed".into();
        if meta.error.is_none() {
            meta.error = Some("The screen was not recorded.".into());
        }
        write_meta(&meta);
        emit(app, &meta.folder);
        return;
    }
    meta.status = "rendering".into();
    write_meta(&meta);
    emit(app, &meta.folder);
    spawn_render(app.clone(), meta);
}

fn rendered_name(meta: &VideoMeta) -> String {
    let base = Path::new(&meta.folder)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Meeting".into());
    format!("{base}.mp4")
}

/// Renders on a background thread, one at a time.
fn spawn_render(app: AppHandle, meta: VideoMeta) {
    static RENDERING: Mutex<()> = Mutex::new(());
    std::thread::spawn(move || {
        let _one = RENDERING.lock().unwrap_or_else(|e| e.into_inner());
        let mut meta = meta;
        let begun = std::time::Instant::now();
        match render(&meta) {
            Ok(name) => {
                log::info!(
                    "Screen video {}: rendered {name} in {:.1} s",
                    meta.folder,
                    begun.elapsed().as_secs_f64()
                );
                meta.rendered = Some(name);
                meta.status = "ready".into();
                meta.error = None;
            }
            Err(e) => {
                log::warn!("Screen video {}: render failed: {e}", meta.folder);
                meta.status = "failed".into();
                meta.error = Some(e);
            }
        }
        write_meta(&meta);
        emit(&app, &meta.folder);
    });
}

/// Puts the screen video and the sound into `<folder name>.mp4`. Returns the
/// file name.
pub fn render(meta: &VideoMeta) -> Result<String, String> {
    let folder = Path::new(&meta.folder);
    let screen = meta.screen.as_ref().ok_or("The screen was not recorded.")?;
    let audio: Vec<_> = meta
        .audio
        .iter()
        .map(|t| serde_json::json!({ "path": folder.join(&t.file), "first_us": t.first_us }))
        .collect();
    let name = rendered_name(meta);
    bridge::render(&serde_json::json!({
        "screen": { "path": folder.join(&screen.file), "first_us": screen.first_us },
        "audio": audio,
        "out": folder.join(&name),
    }))?;
    Ok(name)
}

/// All screen videos, newest first.
pub fn list() -> Vec<VideoMeta> {
    let Ok(entries) = std::fs::read_dir(videos_dir()) else {
        return Vec::new();
    };
    let mut all: Vec<VideoMeta> = entries
        .filter_map(Result::ok)
        .filter_map(|e| read_meta(&e.path()))
        .collect();
    all.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    all
}

/// Moves a meeting's screen video folder to the Trash, when it has one
/// (called when the meeting is deleted).
pub fn delete_for_meeting(meeting_id: &str) {
    let Some(meta) = list().into_iter().find(|m| m.meeting_id == meeting_id) else {
        return;
    };
    let Some(trash) = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".Trash")) else {
        return;
    };
    let name = Path::new(&meta.folder)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Meeting".into());
    let mut target = trash.join(&name);
    let mut n = 2;
    while target.exists() {
        target = trash.join(format!("{name} {n}"));
        n += 1;
    }
    if let Err(e) = std::fs::rename(&meta.folder, &target) {
        log::warn!("Screen video {}: not moved to the Trash: {e}", meta.folder);
    }
}

/// At quit: stops a running capture so its file is finished. The render
/// happens at the next launch (`resume`).
pub fn finalize_on_exit(app: &AppHandle) {
    let manager = app.state::<Arc<VideoManager>>();
    let active = manager
        .active
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    if let Some(Active {
        capture: Capture::Real(handle),
        meta,
    }) = active
    {
        bridge::stop(handle);
        log::info!("Screen video {} finished at quit", meta.folder);
    }
}

/// At launch: renders videos that were cut off by a quit, and marks
/// recordings that never got their Stop.
pub fn resume(app: &AppHandle) {
    for mut meta in list() {
        match meta.status.as_str() {
            "rendering" => spawn_render(app.clone(), meta),
            "recording" => {
                let folder = Path::new(&meta.folder);
                if meta.screen.is_none() && folder.join(SCREEN_FILE).exists() {
                    // The writer notes its first frame's time next to the file.
                    let first_us =
                        std::fs::read_to_string(folder.join(format!("{SCREEN_FILE}.first_us")))
                            .ok()
                            .and_then(|t| t.trim().parse().ok())
                            .unwrap_or(0);
                    meta.screen = Some(VideoTrack {
                        file: SCREEN_FILE.into(),
                        first_us,
                    });
                }
                if meta.screen.is_some() {
                    meta.status = "rendering".into();
                    write_meta(&meta);
                    spawn_render(app.clone(), meta);
                } else {
                    meta.status = "failed".into();
                    meta.error = Some("Recording ended because the app quit.".into());
                    write_meta(&meta);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders the screen video with sound from sample files. Needs
    /// `SOKKI_VIDEO_FIXTURES=<dir with screen.mov, mic.flac>`; skipped
    /// otherwise.
    #[test]
    fn renders_screen_with_sound() {
        let Some(dir) = std::env::var_os("SOKKI_VIDEO_FIXTURES").map(PathBuf::from) else {
            return;
        };
        let out = std::env::temp_dir().join(format!("sokki-video-test-{}", std::process::id()));
        std::fs::create_dir_all(&out).unwrap();
        for f in [SCREEN_FILE, MIC_FILE] {
            std::fs::copy(dir.join(f), out.join(f)).unwrap();
        }
        let t0 = 1_700_000_000_000_000u64;
        let meta = VideoMeta {
            folder: out.to_string_lossy().into_owned(),
            meeting_id: "test".into(),
            started_at: String::new(),
            duration_s: 0.0,
            screen: Some(VideoTrack {
                file: SCREEN_FILE.into(),
                first_us: t0 + 500_000,
            }),
            audio: vec![VideoTrack {
                file: MIC_FILE.into(),
                first_us: t0,
            }],
            rendered: None,
            status: "rendering".into(),
            error: None,
        };
        let name = render(&meta).unwrap();
        let size = std::fs::metadata(out.join(&name)).unwrap().len();
        assert!(size > 10_000, "{size} bytes");
        let _ = std::fs::remove_dir_all(&out);
    }
}

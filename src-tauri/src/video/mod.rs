//! Screen and camera recording. A video is a meeting recording (microphone
//! and system sound, so it gets a transcript like any meeting) plus a screen
//! video and/or a camera video, each saved in full. After Stop, Sokki renders
//! the chosen look (bubble, side by side, vertical) into one file with sound.
//!
//! Files live in `~/Movies/Sokki/<date time>/`: `screen.mov`, `camera.mov`,
//! copies of the audio tracks, the rendered `<date time>.mp4` and
//! `video.json` (this module's record of it). The meeting's own audio may be
//! deleted after its note is written; the copies here stay.
//!
//! Test mode (`SOKKI_TEST_SCREEN` / `SOKKI_TEST_CAMERA` = a video file): no
//! capture and no permission prompt; at Stop those files are copied in as if
//! recorded.

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
const CAMERA_FILE: &str = "camera.mov";

/// Displays and cameras, and whether each may be recorded.
#[derive(Clone, Debug, Default, Serialize, Deserialize, Type)]
pub struct VideoSources {
    /// "allowed" or "not_allowed".
    pub screen: String,
    /// "allowed", "denied", "not_asked" or "restricted".
    pub camera: String,
    pub displays: Vec<VideoDisplay>,
    pub cameras: Vec<VideoCamera>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct VideoDisplay {
    pub id: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub main: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct VideoCamera {
    pub id: String,
    pub name: String,
}

/// What to record, from the recording bar or "Meeting with screen".
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct VideoStartOptions {
    /// "video" (the Videos page) or "meeting" (a meeting with its screen).
    pub kind: String,
    /// "both", "screen" or "camera".
    pub mode: String,
    pub display: Option<u32>,
    pub camera: Option<String>,
    /// "bubble", "side", "vertical" or "files".
    pub look: String,
    /// The meeting app, when started from call detection.
    pub source_app: Option<String>,
}

/// One recorded track in the video's folder.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct VideoTrack {
    /// File name inside the folder.
    pub file: String,
    /// Wall-clock microseconds of the first frame or sample.
    pub first_us: u64,
}

/// A video, as kept in its folder's `video.json`.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct VideoMeta {
    pub folder: String,
    pub meeting_id: String,
    pub kind: String,
    pub started_at: String,
    pub duration_s: f64,
    pub look: String,
    pub screen: Option<VideoTrack>,
    pub camera: Option<VideoTrack>,
    #[serde(default)]
    pub audio: Vec<VideoTrack>,
    /// The rendered file's name, once there is one.
    pub rendered: Option<String>,
    /// "recording", "rendering", "ready" or "failed".
    pub status: String,
    pub error: Option<String>,
}

/// Sent whenever a video changes (recording, rendering, ready, failed).
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

/// The recording in progress, if any.
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

fn test_file(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).filter(|p| !p.is_empty()).map(PathBuf::from)
}

fn test_mode() -> bool {
    test_file("SOKKI_TEST_SCREEN").is_some() || test_file("SOKKI_TEST_CAMERA").is_some()
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
    let _ = VideoChanged { folder: folder.to_string() }.emit(app);
}

fn write_meta(meta: &VideoMeta) {
    let path = Path::new(&meta.folder).join(META_FILE);
    match serde_json::to_string_pretty(meta) {
        Ok(text) => {
            if let Err(e) = std::fs::write(&path, text) {
                log::warn!("Video: {} could not be written: {e}", path.display());
            }
        }
        Err(e) => log::warn!("Video: metadata could not be serialized: {e}"),
    }
}

fn read_meta(folder: &Path) -> Option<VideoMeta> {
    let text = std::fs::read_to_string(folder.join(META_FILE)).ok()?;
    let mut meta: VideoMeta = serde_json::from_str(&text).ok()?;
    // The folder may have been moved or renamed in Finder.
    meta.folder = folder.to_string_lossy().into_owned();
    Some(meta)
}

/// Displays, cameras and permissions. Never asks.
pub fn sources() -> VideoSources {
    if test_mode() {
        return VideoSources {
            screen: "allowed".into(),
            camera: "allowed".into(),
            displays: vec![VideoDisplay { id: 1, name: "Test display".into(), width: 1920, height: 1080, main: true }],
            cameras: vec![VideoCamera { id: "test".into(), name: "Test camera".into() }],
        };
    }
    serde_json::from_value(bridge::sources()).unwrap_or_default()
}

/// Shows or hides the live camera bubble. Never in test mode, and only when
/// the camera is allowed (opening it would otherwise ask).
pub fn bubble(show: bool, camera: Option<&str>) {
    if test_mode() {
        return;
    }
    if show && sources().camera != "allowed" {
        return;
    }
    bridge::bubble(show, camera);
}

/// Asks macOS for "screen" or "camera". True when allowed.
pub fn request_access(kind: &str) -> bool {
    if test_mode() {
        return true;
    }
    match kind {
        "screen" => bridge::request_screen(),
        "camera" => bridge::request_camera(),
        _ => false,
    }
}

/// Starts recording: the screen and/or camera, and the meeting audio.
/// Returns the meeting id. Errors "screen_not_allowed" / "camera_not_allowed"
/// mean macOS was just asked; Screen Recording takes effect after Sokki
/// reopens.
pub fn start(app: &AppHandle, opts: VideoStartOptions) -> Result<String, String> {
    let manager = app.state::<Arc<VideoManager>>();
    if manager.is_recording() {
        return Err("A recording is already running.".into());
    }
    let wants_screen = opts.mode != "camera";
    let wants_camera = opts.kind == "video" && opts.mode != "screen";
    let test = test_mode();
    if !test {
        let have = sources();
        if wants_screen && have.screen != "allowed" && !bridge::request_screen() {
            return Err("screen_not_allowed".into());
        }
        if wants_camera && have.camera != "allowed" && !bridge::request_camera() {
            return Err("camera_not_allowed".into());
        }
    }

    let now = chrono::Local::now();
    let name = now.format("%Y-%m-%d %H.%M.%S").to_string();
    let folder = videos_dir().join(&name);
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let path = |file: &str| folder.join(file).to_string_lossy().into_owned();

    let capture = if test {
        Capture::Test { started_us: now_us() }
    } else {
        let config = serde_json::json!({
            "screen": wants_screen.then(|| serde_json::json!({ "display": opts.display, "path": path(SCREEN_FILE) })),
            "camera": wants_camera.then(|| serde_json::json!({ "device": opts.camera, "path": path(CAMERA_FILE) })),
        });
        match bridge::start(&config) {
            Ok(h) => Capture::Real(h),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&folder);
                return Err(e);
            }
        }
    };

    let meeting_id = match crate::meeting::start_meeting(app, opts.source_app.clone()) {
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
        kind: opts.kind.clone(),
        started_at: now.to_rfc3339(),
        duration_s: 0.0,
        look: opts.look.clone(),
        screen: None,
        camera: None,
        audio: Vec::new(),
        rendered: None,
        status: "recording".into(),
        error: None,
    };
    write_meta(&meta);
    log::info!(
        "Video {name}: recording ({}, {}, look {}) with meeting {meeting_id}",
        opts.kind,
        opts.mode,
        opts.look
    );
    *manager.active.lock().unwrap_or_else(|e| e.into_inner()) = Some(Active { capture, meta: meta.clone() });
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
    let Some(Active { capture, mut meta }) = active else { return };
    let folder = PathBuf::from(&meta.folder);
    meta.duration_s = duration_s;

    match capture {
        Capture::Real(handle) => {
            let report = bridge::stop(handle);
            bridge::bubble(false, None);
            for (key, file) in [("screen", SCREEN_FILE), ("camera", CAMERA_FILE)] {
                let first = report[key]["first_us"].as_u64().unwrap_or(0);
                if report[key].is_object() && first > 0 {
                    let track = VideoTrack { file: file.into(), first_us: first };
                    if key == "screen" { meta.screen = Some(track) } else { meta.camera = Some(track) }
                }
            }
            if let Some(errors) = report["errors"].as_array() {
                for e in errors.iter().filter_map(|e| e.as_str()) {
                    log::warn!("Video {}: {e}", meta.folder);
                }
                if meta.screen.is_none() && meta.camera.is_none() {
                    meta.error = errors.first().and_then(|e| e.as_str()).map(String::from);
                }
            }
        }
        Capture::Test { started_us } => {
            for (var, file) in [("SOKKI_TEST_SCREEN", SCREEN_FILE), ("SOKKI_TEST_CAMERA", CAMERA_FILE)] {
                if let Some(src) = test_file(var) {
                    if std::fs::copy(&src, folder.join(file)).is_ok() {
                        let track = VideoTrack { file: file.into(), first_us: started_us };
                        if file == SCREEN_FILE { meta.screen = Some(track) } else { meta.camera = Some(track) }
                    }
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
    for (file, first) in [(MIC_FILE, timing.mic_first_us), (SYSTEM_FILE, timing.system_first_us)] {
        let src = audio_dir.join(file);
        if first > 0 && src.exists() && std::fs::copy(&src, folder.join(file)).is_ok() {
            meta.audio.push(VideoTrack { file: file.into(), first_us: first });
        }
    }

    if meta.screen.is_none() && meta.camera.is_none() {
        meta.status = "failed".into();
        if meta.error.is_none() {
            meta.error = Some("Nothing was recorded.".into());
        }
        write_meta(&meta);
        emit(app, &meta.folder);
        return;
    }
    meta.status = if needs_render(&meta) { "rendering" } else { "ready" }.into();
    write_meta(&meta);
    emit(app, &meta.folder);
    if meta.status == "rendering" {
        spawn_render(app.clone(), meta);
    }
}

/// A look needs a render unless it is "files", or there is only the screen
/// or only the camera and no sound to add.
fn needs_render(meta: &VideoMeta) -> bool {
    meta.look != "files" && (meta.screen.is_some() || meta.camera.is_some())
}

fn rendered_name(meta: &VideoMeta) -> String {
    let base = Path::new(&meta.folder)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Video".into());
    format!("{base}.mp4")
}

/// Renders the look on a background thread, one at a time.
fn spawn_render(app: AppHandle, meta: VideoMeta) {
    static RENDERING: Mutex<()> = Mutex::new(());
    std::thread::spawn(move || {
        let _one = RENDERING.lock().unwrap_or_else(|e| e.into_inner());
        let mut meta = meta;
        let begun = std::time::Instant::now();
        match render(&meta) {
            Ok(name) => {
                log::info!("Video {}: rendered {name} in {:.1} s", meta.folder, begun.elapsed().as_secs_f64());
                meta.rendered = Some(name);
                meta.status = "ready".into();
                meta.error = None;
            }
            Err(e) => {
                log::warn!("Video {}: render failed: {e}", meta.folder);
                meta.status = "failed".into();
                meta.error = Some(e);
            }
        }
        write_meta(&meta);
        emit(&app, &meta.folder);
    });
}

/// Renders `meta`'s look into `<folder name>.mp4`. Returns the file name.
pub fn render(meta: &VideoMeta) -> Result<String, String> {
    let folder = Path::new(&meta.folder);
    let track = |t: &Option<VideoTrack>| {
        t.as_ref().map(|t| serde_json::json!({ "path": folder.join(&t.file), "first_us": t.first_us }))
    };
    let audio: Vec<_> = meta
        .audio
        .iter()
        .map(|t| serde_json::json!({ "path": folder.join(&t.file), "first_us": t.first_us }))
        .collect();
    let name = rendered_name(meta);
    let look = if meta.look == "files" { "bubble" } else { meta.look.as_str() };
    bridge::render(&serde_json::json!({
        "look": look,
        "screen": track(&meta.screen),
        "camera": track(&meta.camera),
        "audio": audio,
        "out": folder.join(&name),
    }))?;
    Ok(name)
}

/// All videos, newest first.
pub fn list() -> Vec<VideoMeta> {
    let Ok(entries) = std::fs::read_dir(videos_dir()) else { return Vec::new() };
    let mut all: Vec<VideoMeta> = entries
        .filter_map(Result::ok)
        .filter_map(|e| read_meta(&e.path()))
        .collect();
    all.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    all
}

/// Changes a video's look and renders it again ("files" removes the
/// rendered file).
pub fn set_look(app: &AppHandle, folder: &str, look: &str) -> Result<(), String> {
    let mut meta = read_meta(Path::new(folder)).ok_or("Video not found")?;
    if meta.status == "recording" || meta.status == "rendering" {
        return Err("The video is still being made.".into());
    }
    meta.look = look.to_string();
    if let Some(old) = meta.rendered.take() {
        let _ = std::fs::remove_file(Path::new(folder).join(old));
    }
    meta.status = if needs_render(&meta) { "rendering" } else { "ready" }.into();
    meta.error = None;
    write_meta(&meta);
    emit(app, folder);
    if meta.status == "rendering" {
        spawn_render(app.clone(), meta);
    }
    Ok(())
}

/// Moves a video's folder to the Trash.
pub fn delete(app: &AppHandle, folder: &str) -> Result<(), String> {
    let meta = read_meta(Path::new(folder)).ok_or("Video not found")?;
    if meta.status == "recording" || meta.status == "rendering" {
        return Err("The video is still being made.".into());
    }
    // Into the Trash (same volume, so a rename), under a free name.
    let trash = std::env::var_os("HOME").map(PathBuf::from).ok_or("No home folder")?.join(".Trash");
    let name = Path::new(folder).file_name().ok_or("Video not found")?.to_string_lossy().into_owned();
    let mut target = trash.join(&name);
    let mut n = 2;
    while target.exists() {
        target = trash.join(format!("{name} {n}"));
        n += 1;
    }
    std::fs::rename(folder, &target).map_err(|e| e.to_string())?;
    emit(app, folder);
    Ok(())
}

/// At quit: stops a running capture so its files are finished. The render
/// happens at the next launch (`resume`).
pub fn finalize_on_exit(app: &AppHandle) {
    let manager = app.state::<Arc<VideoManager>>();
    let active = manager.active.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(Active { capture: Capture::Real(handle), meta }) = active {
        bridge::stop(handle);
        log::info!("Video {} finished at quit", meta.folder);
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
                for (file, slot) in [(SCREEN_FILE, &mut meta.screen), (CAMERA_FILE, &mut meta.camera)] {
                    if slot.is_none() && folder.join(file).exists() {
                        *slot = Some(VideoTrack { file: file.into(), first_us: 0 });
                    }
                }
                meta.status = if needs_render(&meta) { "rendering" } else { "failed" }.into();
                if meta.status == "failed" {
                    meta.error = Some("Recording ended because the app quit.".into());
                }
                write_meta(&meta);
                if meta.status == "rendering" {
                    spawn_render(app.clone(), meta);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders every look from real sample files. Needs
    /// `SOKKI_VIDEO_FIXTURES=<dir with screen.mov, camera.mov, mic.flac>`;
    /// skipped otherwise.
    #[test]
    fn renders_every_look() {
        let Some(dir) = std::env::var_os("SOKKI_VIDEO_FIXTURES").map(PathBuf::from) else {
            return;
        };
        let out = std::env::temp_dir().join(format!("sokki-video-test-{}", std::process::id()));
        std::fs::create_dir_all(&out).unwrap();
        for f in [SCREEN_FILE, CAMERA_FILE, MIC_FILE] {
            std::fs::copy(dir.join(f), out.join(f)).unwrap();
        }
        let t0 = 1_700_000_000_000_000u64;
        for look in ["bubble", "side", "vertical"] {
            let meta = VideoMeta {
                folder: out.to_string_lossy().into_owned(),
                meeting_id: "test".into(),
                kind: "video".into(),
                started_at: String::new(),
                duration_s: 0.0,
                look: look.into(),
                screen: Some(VideoTrack { file: SCREEN_FILE.into(), first_us: t0 }),
                camera: Some(VideoTrack { file: CAMERA_FILE.into(), first_us: t0 + 1_000_000 }),
                audio: vec![VideoTrack { file: MIC_FILE.into(), first_us: t0 }],
                rendered: None,
                status: "rendering".into(),
                error: None,
            };
            let name = render(&meta).unwrap();
            let size = std::fs::metadata(out.join(&name)).unwrap().len();
            assert!(size > 10_000, "{look}: {size} bytes");
        }
        let _ = std::fs::remove_dir_all(&out);
    }
}

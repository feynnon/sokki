//! Updates (Sokki build only).
//!
//! Shortly after launch and then every few hours, the app asks the update feed
//! (`plugins.updater` in brands/sokki/tauri.conf.json) for a newer release and
//! downloads it in the background; the download's signature is checked against
//! the public key in the config. Then:
//! - the footer offers "Update now" (`get_ready_update`, `update_now`);
//! - after the first login of the day (app start or screen unlock), it
//!   installs by itself, unless something is recording or being processed.
//!
//! Installing replaces the app's bundle and starts the new version. Nothing is
//! installed while a dictation or meeting records, or a meeting is processed.
//!
//! The feed is a release's `latest.json`, written by `scripts/release.sh`.
//! `SOKKI_UPDATE_ENDPOINT` replaces the feed URL, for testing.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::Context;
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::managers::audio::AudioRecordingManager;
use crate::meeting::store::{MeetingStatus, MeetingStore};
use crate::meeting::MeetingManager;
use crate::settings;

const FIRST_CHECK: Duration = Duration::from_secs(60);
const CHECK_EVERY: Duration = Duration::from_secs(4 * 60 * 60);
const LOCK_POLL: Duration = Duration::from_secs(30);
/// After a login, how long the daily update keeps trying (no network yet,
/// something recording) before it leaves the update to the button.
const LOGIN_TRIES: u32 = 15;
const LOGIN_RETRY: Duration = Duration::from_secs(60);
/// Roughly how long Sokki is gone while an update installs and restarts.
const DOWNTIME_SECONDS: u32 = 10;

/// A downloaded, verified update waiting to be installed.
struct Ready {
    update: Update,
    file: PathBuf,
}

fn ready() -> &'static Mutex<Option<Ready>> {
    static READY: OnceLock<Mutex<Option<Ready>>> = OnceLock::new();
    READY.get_or_init(|| Mutex::new(None))
}

/// One check or download at a time.
fn fetching() -> &'static tauri::async_runtime::Mutex<()> {
    static FETCHING: OnceLock<tauri::async_runtime::Mutex<()>> = OnceLock::new();
    FETCHING.get_or_init(|| tauri::async_runtime::Mutex::new(()))
}

/// What the footer shows: the new version and how long Sokki will be away.
#[derive(Serialize, Type, Clone)]
pub struct ReadyUpdate {
    pub version: String,
    pub seconds: u32,
}

pub fn start(app: &AppHandle) {
    if !crate::brand::is_sokki() {
        return;
    }
    let a = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            if enabled(&a) {
                if let Err(e) = fetch(&a).await {
                    log::warn!("Update: {e:#}");
                }
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
    let a = app.clone();
    tauri::async_runtime::spawn(async move {
        // Launching counts as a login: Sokki starts with the Mac.
        let mut was_locked = true;
        loop {
            let locked = screen_locked();
            if was_locked && !locked && enabled(&a) && first_login_today(&a) {
                daily_update(&a).await;
            }
            was_locked = locked;
            tokio::time::sleep(LOCK_POLL).await;
        }
    });
}

fn enabled(app: &AppHandle) -> bool {
    settings::update_checks_effectively_enabled(&settings::get_settings(app))
}

/// Downloads a newer release, if there is one and it isn't downloaded yet.
/// Returns whether an update is ready.
async fn fetch(app: &AppHandle) -> anyhow::Result<bool> {
    let _one = fetching().lock().await;
    let mut builder = app.updater_builder();
    if let Ok(url) = std::env::var("SOKKI_UPDATE_ENDPOINT") {
        builder = builder.endpoints(vec![url.parse()?])?;
    }
    let Some(update) = builder.build()?.check().await? else {
        log::info!("Update: {} is the newest", app.package_info().version);
        return Ok(false);
    };
    let version = update.version.clone();
    if lock_ready()
        .as_ref()
        .is_some_and(|r| r.update.version == version)
    {
        return Ok(true);
    }
    let Some(bundle) = app_bundle() else {
        anyhow::bail!("{version} is out; this app is not in an .app bundle");
    };
    // Replacing the bundle needs write access to its folder; without it macOS
    // would ask for an admin password, so leave it to a manual update.
    if !writable(bundle.parent().unwrap_or(&bundle)) {
        anyhow::bail!("{version} is out; {} is not writable", bundle.display());
    }
    log::info!("Update: downloading {version}");
    // `download` checks the signature.
    let bytes = update.download(|_, _| {}, || {}).await?;
    let dir = app.path().app_cache_dir()?.join("updates");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(format!("Sokki-{version}.tar.gz"));
    std::fs::write(&file, &bytes).context("saving the update")?;
    *lock_ready() = Some(Ready { update, file });
    log::info!("Update: {version} is ready");
    let _ = app.emit("update-ready", ());
    Ok(true)
}

/// After the first login of the day: install a waiting update, as soon as
/// the network is up and nothing is recording, within a few minutes.
async fn daily_update(app: &AppHandle) {
    for _ in 0..LOGIN_TRIES {
        match fetch(app).await {
            Ok(false) => return mark_today(app),
            Ok(true) if busy(app).is_none() => {
                mark_today(app);
                let hidden = !window_open(app);
                if let Err(e) = install_and_restart(app, hidden) {
                    log::warn!("Update: {e:#}");
                }
                return;
            }
            Ok(true) => log::info!("Update: waiting, {}", busy(app).unwrap_or("")),
            Err(e) => log::warn!("Update: {e:#}"),
        }
        tokio::time::sleep(LOGIN_RETRY).await;
    }
    // Still busy or offline: the button stays until tomorrow's login.
    mark_today(app);
}

/// The update the footer offers, while nothing keeps it from installing.
#[tauri::command]
#[specta::specta]
pub fn get_ready_update(app: AppHandle) -> Option<ReadyUpdate> {
    if busy(&app).is_some() {
        return None;
    }
    lock_ready().as_ref().map(|r| ReadyUpdate {
        version: r.update.version.clone(),
        seconds: DOWNTIME_SECONDS,
    })
}

/// "Update now": installs the downloaded update and restarts into it.
#[tauri::command]
#[specta::specta]
pub fn update_now(app: AppHandle) -> Result<(), String> {
    if let Some(what) = busy(&app) {
        return Err(format!("not now: {what}"));
    }
    install_and_restart(&app, false).map_err(|e| format!("{e:#}"))
}

fn install_and_restart(app: &AppHandle, hidden: bool) -> anyhow::Result<()> {
    let Some(r) = lock_ready().take() else {
        anyhow::bail!("no update downloaded");
    };
    let bundle = app_bundle().context("not in an .app bundle")?;
    let bytes = std::fs::read(&r.file).context("reading the update")?;
    r.update.install(bytes)?;
    let _ = std::fs::remove_file(&r.file);
    log::info!("Update: installed {}; restarting", r.update.version);
    // `open` once this process is gone, so the single-instance check lets the
    // new one start.
    let script = format!(
        "while kill -0 {} 2>/dev/null; do sleep 0.2; done; open -a \"$0\"{}",
        std::process::id(),
        if hidden { " --args --start-hidden" } else { "" }
    );
    std::process::Command::new("/bin/sh")
        .args(["-c", &script])
        .arg(&bundle)
        .spawn()
        .context("restart")?;
    app.exit(0);
    Ok(())
}

fn lock_ready() -> std::sync::MutexGuard<'static, Option<Ready>> {
    ready().lock().unwrap_or_else(|e| e.into_inner())
}

/// What keeps the app from updating now, if anything.
fn busy(app: &AppHandle) -> Option<&'static str> {
    if app.state::<Arc<AudioRecordingManager>>().is_recording() {
        return Some("dictating");
    }
    if app.state::<Arc<MeetingManager>>().is_recording() {
        return Some("a meeting is recording");
    }
    let processing = app.state::<Arc<MeetingStore>>().list().iter().any(|m| {
        matches!(m.status, MeetingStatus::Recording | MeetingStatus::Processing { .. })
    });
    processing.then_some("a meeting is being processed")
}

fn window_open(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

fn day_file(app: &AppHandle) -> Option<PathBuf> {
    Some(crate::portable::app_data_dir(app).ok()?.join("update-day.txt"))
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn first_login_today(app: &AppHandle) -> bool {
    let Some(f) = day_file(app) else { return false };
    std::fs::read_to_string(f).map_or(true, |d| d.trim() != today())
}

fn mark_today(app: &AppHandle) {
    if let Some(f) = day_file(app) {
        let _ = std::fs::write(f, today());
    }
}

/// `/Applications/Sokki.app` for `/Applications/Sokki.app/Contents/MacOS/handy`.
fn app_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let bundle = exe.parent()?.parent()?.parent()?.to_path_buf();
    (bundle.extension()? == "app").then_some(bundle)
}

fn writable(dir: &std::path::Path) -> bool {
    let probe = dir.join(format!(".sokki-update-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Whether the screen is locked (or nobody is logged in at the console).
#[cfg(target_os = "macos")]
fn screen_locked() -> bool {
    use std::ffi::{c_char, c_void};
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGSessionCopyCurrentDictionary() -> *const c_void;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFDictionaryGetValue(dict: *const c_void, key: *const c_void) -> *const c_void;
        fn CFStringCreateWithCString(
            alloc: *const c_void,
            s: *const c_char,
            encoding: u32,
        ) -> *const c_void;
        fn CFBooleanGetValue(b: *const c_void) -> bool;
        fn CFRelease(p: *const c_void);
    }
    const UTF8: u32 = 0x0800_0100;
    // SAFETY: Core Foundation calls on objects created and released here.
    unsafe {
        let dict = CGSessionCopyCurrentDictionary();
        if dict.is_null() {
            return true;
        }
        let key = CFStringCreateWithCString(
            std::ptr::null(),
            c"CGSSessionScreenIsLocked".as_ptr(),
            UTF8,
        );
        let value = CFDictionaryGetValue(dict, key);
        let locked = !value.is_null() && CFBooleanGetValue(value);
        CFRelease(key);
        CFRelease(dict);
        locked
    }
}

#[cfg(not(target_os = "macos"))]
fn screen_locked() -> bool {
    false
}

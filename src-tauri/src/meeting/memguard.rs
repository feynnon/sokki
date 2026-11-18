//! Memory guard. A bug in meeting processing once looped forever and grew
//! the app to 51 GB, swapping the whole Mac. A loop in a background thread
//! can't be stopped from outside, so when the app's memory footprint passes
//! [`LIMIT_BYTES`] while a meeting is processing, the guard marks that
//! meeting as failed (it is *not* retried at the next launch) and quits the
//! app. Without a meeting processing it only logs and notifies once.
//!
//! Normal footprint: ~0.5–1.5 GB (models for speech recognition run in a
//! separate engine process; the diarizer is a few hundred MB).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager};

use super::store::{MeetingStatus, MeetingStore};

pub const LIMIT_BYTES: u64 = 6 * 1024 * 1024 * 1024;
const INTERVAL: Duration = Duration::from_secs(3);

/// The meeting being processed right now, with its stage.
static CURRENT: Mutex<Option<String>> = Mutex::new(None);

pub fn set_current(id: Option<&str>) {
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = id.map(str::to_string);
}

/// Physical memory footprint of this process (what Activity Monitor shows
/// as "Memory"), in bytes.
pub fn footprint_bytes() -> Option<u64> {
    footprint_of(unsafe { libc::getpid() })
}

/// Physical memory footprint of process `pid`, in bytes.
pub fn footprint_of(pid: i32) -> Option<u64> {
    #[cfg(target_os = "macos")]
    unsafe {
        let mut info: libc::rusage_info_v4 = std::mem::zeroed();
        let r = libc::proc_pid_rusage(
            pid,
            libc::RUSAGE_INFO_V4,
            &mut info as *mut _ as *mut libc::rusage_info_t,
        );
        if r == 0 {
            return Some(info.ri_phys_footprint);
        }
        None
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Text stored as the failed meeting's error.
pub fn failure_text(gb: f64, stage: &str) -> String {
    format!(
        "Stopped: the app grew to {gb:.1} GB of memory while this meeting was being processed \
         (stage: {stage}). The app quit to protect the Mac. This is a bug; the meeting is not \
         retried automatically."
    )
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    let warned = Arc::new(AtomicBool::new(false));
    let _ = std::thread::Builder::new()
        .name("memory-guard".into())
        .spawn(move || loop {
            std::thread::sleep(INTERVAL);
            let Some(bytes) = footprint_bytes() else {
                return; // not measurable on this platform
            };
            if bytes < LIMIT_BYTES {
                continue;
            }
            let gb = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
            let current = CURRENT.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let Some(id) = current else {
                if !warned.swap(true, Ordering::Relaxed) {
                    log::error!("Memory footprint is {gb:.1} GB with no meeting processing");
                    super::detect::notify(
                        &format!("{} braucht zu viel Speicher", crate::brand::app_name()),
                        &format!("{gb:.1} GB – bitte App neu starten und Claude Bescheid geben."),
                    );
                }
                continue;
            };
            let store = app.state::<Arc<MeetingStore>>().inner().clone();
            let stage = match store.get(&id).map(|r| r.status) {
                Some(MeetingStatus::Processing { stage, .. }) => stage,
                _ => "unknown".into(),
            };
            let error = failure_text(gb, &stage);
            log::error!("Meeting {id}: {error}");
            let _ = store.update(&id, |r| r.status = MeetingStatus::Failed { error });
            super::detect::notify(
                &format!("{} gestoppt", crate::brand::app_name()),
                &format!(
                    "Zu viel Speicher ({gb:.1} GB) beim Verarbeiten eines Meetings. App beendet."
                ),
            );
            // The runaway thread can't be stopped any other way.
            std::process::exit(70);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footprint_is_measured() {
        let b = footprint_bytes().expect("footprint on macOS");
        assert!(b > 1024 * 1024 && b < LIMIT_BYTES, "{b}");
    }

    #[test]
    fn a_memory_stop_is_not_resumed_at_launch() {
        let e = failure_text(7.2, "correcting");
        assert!(!e.starts_with("Interrupted:"));
        assert_ne!(e, super::super::RESUME_MARKER);
    }
}

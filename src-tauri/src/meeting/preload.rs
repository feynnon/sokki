//! Loads the built-in summary model while a meeting records, so its summary
//! starts as soon as the transcript is ready. Robin, 26.11.2026: "Das Modell
//! sollte bereits während des Meetings geladen werden, damit die summary
//! dann schneller kommt."
//!
//! - [`PRELOAD_AFTER`] into the recording, the model (Gemma 4 E4B, 5.6 GB)
//!   loads with throttled disk reads, so the recording keeps the disk
//!   (`sokki_engine::llm::preload`). Then it only waits: no work on the CPU
//!   or GPU until the summary.
//! - Only when the Mac has the memory: [`may_load`]. With less, nothing
//!   changes: the model loads for the summary, as before.
//! - While it waits, memory is checked every [`CHECK_EVERY`]; when memory
//!   runs short it is stopped ([`must_stop`]), so it never pushes the
//!   recording into swap.
//! - The summary takes it (`llm::open`). A meeting without a summary, a
//!   discarded one, or one whose processing ends otherwise stops it
//!   (`process::spawn`, `discard_meeting`).
//!
//! On 24.11.2026 a meeting lost its call audio while eight transcriptions
//! and a language model ran next to it; the tap is rebuilt since then
//! (`system_tap`). A preloaded model does no work while it waits.

use std::sync::Arc;
use std::time::{Duration, Instant};

use sokki_engine::llm;
use tauri::{AppHandle, Manager};

use super::memguard;
use super::MeetingManager;

/// How long a meeting records before the model loads: a meeting stopped
/// sooner was likely started by mistake, and its summary is short.
const PRELOAD_AFTER: Duration = Duration::from_secs(60);

/// How often memory is checked while the model waits, and how often a
/// load that could not start yet is tried again.
const CHECK_EVERY: Duration = Duration::from_secs(5);
const RETRY_EVERY: Duration = Duration::from_secs(30);

/// What the loaded model uses (resident memory of llama-server with
/// Gemma 4 E4B and its draft model, 16k context, measured 25.11.2026).
pub const MODEL_BYTES: u64 = 5_600_000_000;

/// The model loads only when, after loading it, at least a quarter of the
/// Mac's memory would still be free and macOS reports no memory pressure.
/// A 32 GB Mac needs 13.6 GB free, a 16 GB Mac 9.6 GB. A Mac with less than
/// twice the model's memory (an 8 GB Mac) never loads it during a meeting.
pub fn may_load(free_bytes: u64, total_bytes: u64, pressure: i64) -> bool {
    pressure <= 1 && total_bytes >= 2 * MODEL_BYTES && free_bytes >= MODEL_BYTES + total_bytes / 4
}

/// The waiting model is stopped at critical memory pressure, when less than
/// [`STOP_BELOW_PERCENT`] of memory is free, or at a memory warning with less
/// than [`memguard::MIN_FREE_PERCENT`] free. A warning alone is no reason:
/// loading the model during a recording raised one in 4 of 7 test meetings
/// (26.11.2026: free memory dipped to 32–39 % while it loaded), and macOS
/// kept it while the model was loaded, with half of memory free again.
pub fn must_stop(pressure: i64, free_percent: i64) -> bool {
    pressure >= 4
        || free_percent < STOP_BELOW_PERCENT
        || (pressure > 1 && free_percent < memguard::MIN_FREE_PERCENT)
}

/// Free memory, in percent, below which the waiting model is stopped.
const STOP_BELOW_PERCENT: i64 = 15;

/// When the preload runs: `SOKKI_TEST_PRELOAD_AFTER_S` in test mode,
/// `SOKKI_TEST_NO_PRELOAD=1` turns it off there (for comparing).
fn preload_after() -> Option<Duration> {
    if !crate::test_mic::active() {
        return Some(PRELOAD_AFTER);
    }
    if std::env::var_os("SOKKI_TEST_NO_PRELOAD").is_some_and(|v| !v.is_empty()) {
        return None;
    }
    let test = std::env::var("SOKKI_TEST_PRELOAD_AFTER_S")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .map(Duration::from_secs_f64);
    Some(test.unwrap_or(PRELOAD_AFTER))
}

/// Why the model does not load now, if it does not.
fn why_not(app: &AppHandle) -> Option<String> {
    let all = crate::settings::get_settings(app);
    let m = &all.meetings;
    if !m.summary_enabled {
        return Some("summaries are off".into());
    }
    if m.llm_provider.trim() != llm::BUILTIN {
        return Some("the summary uses another model".into());
    }
    if !llm::builtin_server_available() || !llm::model_present(&llm::BUILTIN_MODEL) {
        return Some("the built-in model is not downloaded".into());
    }
    if super::process::busy() {
        return Some("an earlier meeting is being processed".into());
    }
    if llm::local_job_running() {
        return Some("a summary model is running".into());
    }
    match (
        memguard::pressure_and_free_percent(),
        memguard::total_bytes(),
    ) {
        (Some((pressure, free)), Some(total)) => {
            let free_bytes = total / 100 * free.max(0) as u64;
            if !may_load(free_bytes, total, pressure) {
                return Some(format!(
                    "not enough free memory ({:.1} of {:.0} GB free, pressure {pressure})",
                    free_bytes as f64 / 1e9,
                    total as f64 / 1e9
                ));
            }
            None
        }
        // Memory can't be read (not a Mac): no preload.
        _ => Some("memory can't be measured here".into()),
    }
}

/// Loads the summary model during the meeting `id`, if it may.
pub fn spawn(app: AppHandle, id: String) {
    let Some(after) = preload_after() else {
        return;
    };
    let started = Instant::now();
    let name = format!("llm-preload-{id}");
    let spawned = std::thread::Builder::new()
        .name(name.clone())
        .spawn(move || run(&app, &id, started + after));
    if let Err(e) = spawned {
        log::warn!("{name}: no preload ({e})");
    }
}

fn run(app: &AppHandle, id: &str, due: Instant) {
    let recording = || app.state::<Arc<MeetingManager>>().active_id().as_deref() == Some(id);
    while Instant::now() < due {
        if !recording() {
            return;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    // Load, as soon as it may.
    let mut said = String::new();
    loop {
        if !recording() {
            return;
        }
        match why_not(app) {
            Some(why) => {
                if why != said {
                    log::info!("Meeting {id}: summary model not loaded yet: {why}");
                    said = why;
                }
                sleep_while(RETRY_EVERY, &recording);
                continue;
            }
            None => break,
        }
    }
    let t0 = Instant::now();
    match llm::preload() {
        Ok(pid) => log::info!(
            "Meeting {id}: summary model loaded during the meeting in {:.1} s (pid {pid})",
            t0.elapsed().as_secs_f64()
        ),
        Err(e) => {
            log::warn!("Meeting {id}: summary model not loaded during the meeting: {e:#}");
            return;
        }
    }
    // Wait for the summary; stop the model under memory pressure.
    let mut warned_since: Option<Instant> = None;
    loop {
        if !recording() {
            // The meeting stopped while the model loaded and is processed
            // already, or was discarded: processing takes or stops it.
            if !super::process::busy() && llm::drop_preloaded() {
                log::info!("Meeting {id}: stopped the summary model, the meeting is gone");
            }
            return;
        }
        if let Some((pressure, free)) = memguard::pressure_and_free_percent() {
            if pressure > 1 && warned_since.is_none() {
                log::info!(
                    "Meeting {id}: memory warning ({free} % free) while the summary model waits"
                );
                warned_since = Some(Instant::now());
            } else if pressure <= 1 {
                if let Some(t) = warned_since.take() {
                    log::info!(
                        "Meeting {id}: the memory warning ended after {:.0} s",
                        t.elapsed().as_secs_f64()
                    );
                }
            }
            if must_stop(pressure, free) {
                if llm::drop_preloaded() {
                    log::warn!(
                        "Meeting {id}: memory is short (pressure {pressure}, {free} % free); \
                         stopped the summary model, it loads again for the summary"
                    );
                }
                return;
            }
        }
        if !llm::preloaded() {
            log::warn!("Meeting {id}: the summary model loaded during the meeting stopped");
            return;
        }
        std::thread::sleep(CHECK_EVERY);
    }
}

fn sleep_while(d: Duration, recording: &impl Fn() -> bool) {
    let until = Instant::now() + d;
    while Instant::now() < until && recording() {
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1_000_000_000;

    #[test]
    fn loads_only_with_a_quarter_of_memory_left_after_it() {
        assert!(may_load(14 * GB, 32 * GB, 1));
        assert!(!may_load(13 * GB, 32 * GB, 1));
        assert!(may_load(10 * GB, 16 * GB, 0));
        assert!(!may_load(9 * GB, 16 * GB, 0));
        // An 8 GB Mac never, even with almost all of it free.
        assert!(!may_load(8 * GB, 8 * GB, 0));
        assert!(!may_load(7 * GB, 8 * GB, 0));
    }

    #[test]
    fn never_under_memory_pressure() {
        assert!(!may_load(30 * GB, 32 * GB, 2));
        assert!(!may_load(30 * GB, 32 * GB, 4));
    }

    #[test]
    fn a_waiting_model_stops_under_pressure_or_when_memory_runs_low() {
        assert!(!must_stop(1, 40));
        assert!(must_stop(4, 40));
        assert!(must_stop(1, 14));
        // A warning with plenty free, as right after loading it: it stays.
        assert!(!must_stop(2, 50));
        assert!(must_stop(2, 24));
    }
}

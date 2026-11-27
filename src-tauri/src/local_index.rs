//! Keeps `index.sqlite` (`crates/sokki-index`) up to date while the app
//! runs, so `sokki search` and hooks see a meeting as soon as its note is
//! written, and a deleted meeting is gone from the index at once. The CLI
//! also brings the index up to date before every read, so a missed call
//! here costs nothing but a moment's delay.
//!
//! Never on the main thread: a UI action only starts the work.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

fn paths(app: &AppHandle) -> Option<sokki_index::Paths> {
    let data = app.path().app_data_dir().ok()?;
    let user_name = crate::settings::get_settings(app).meetings.user_name.clone();
    Some(sokki_index::Paths {
        index: data.join("index.sqlite"),
        meetings_dir: data.join("meetings"),
        history_db: Some(data.join("history.db")),
        user_name: if user_name.trim().is_empty() {
            "Ich".into()
        } else {
            user_name
        },
    })
}

/// One writer at a time inside the app.
static LOCK: Mutex<()> = Mutex::new(());

/// Indexes one meeting now, on the calling (background) thread: right
/// after its note is written, before anything announces it.
pub fn sync_meeting(app: &AppHandle, id: &str) {
    let Some(p) = paths(app) else { return };
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let t = std::time::Instant::now();
    // Runs on the meeting's processing thread: a panic in the index must
    // not take the meeting down with it.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sokki_index::Index::open(&p.index).and_then(|ix| sokki_index::sync_meeting(&ix, &p, id))
    }));
    match result {
        Ok(Ok(())) => log::info!("Index: meeting {id} in {} ms", t.elapsed().as_millis()),
        Ok(Err(e)) => log::warn!("Index: meeting {id} not indexed: {e:#}"),
        Err(_) => log::warn!("Index: meeting {id} not indexed: the indexer panicked"),
    }
}

static RUNNING: AtomicBool = AtomicBool::new(false);
static AGAIN: AtomicBool = AtomicBool::new(false);

/// Brings the whole index up to date in the background (a note renamed,
/// a meeting deleted, a dictation saved). Calls while one runs fold into
/// one more run.
pub fn refresh(app: &AppHandle) {
    AGAIN.store(true, Ordering::SeqCst);
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    // Frees the flag however the thread ends, a panic included, so a bad
    // note can't stop every later refresh.
    struct Running;
    impl Drop for Running {
        fn drop(&mut self) {
            RUNNING.store(false, Ordering::SeqCst);
        }
    }
    let spawned = std::thread::Builder::new()
        .name("index".into())
        .spawn(move || {
            let running = Running;
            while AGAIN.swap(false, Ordering::SeqCst) {
                let Some(p) = paths(&app) else { break };
                let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
                match sokki_index::Index::open(&p.index).and_then(|ix| sokki_index::sync(&ix, &p)) {
                    Ok(r) if r.added + r.updated + r.removed > 0 => log::info!(
                        "Index: {} added, {} updated, {} removed in {} ms",
                        r.added,
                        r.updated,
                        r.removed,
                        r.ms
                    ),
                    Ok(_) => {}
                    Err(e) => log::warn!("Index: not brought up to date: {e:#}"),
                }
            }
            drop(running);
            // A call that came in between the last check and now.
            if AGAIN.load(Ordering::SeqCst) {
                refresh(&app);
            }
        });
    if spawned.is_err() {
        RUNNING.store(false, Ordering::SeqCst);
    }
}

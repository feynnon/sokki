//! The local API on the Mac: the same socket API as the Linux service
//! (`crates/sokki-api`, the specification), at `<app data>/sokki.sock`, so
//! the one `sokki` command line works on both. One JSON object per line;
//! `{"cmd":"subscribe"}` streams events.
//!
//! Every request goes to the function the menu bar, the shortcut or the
//! command-line flags already use; nothing here decides anything new.
//! The socket is only for this user (0600, in the app's own folder).

use sokki_api::{
    DictationState, Event, MeetingState, ModelsState, NoteInfo, Processing, Reply, Request,
    SpeakerInfo, State,
};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager};

use crate::meeting::store::{MeetingRecord, MeetingStatus, MeetingStore};
use crate::meeting::MeetingManager;

fn socket_path(app: &AppHandle) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("SOKKI_SOCKET").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    app.path().app_data_dir().ok().map(|d| d.join("sokki.sock"))
}

/// One sender per `subscribe` connection, for hook events. A closed
/// connection's sender is dropped at the next event.
static HOOK_SUBSCRIBERS: Mutex<Vec<Sender<String>>> = Mutex::new(Vec::new());

/// Every hook event (without content, from the engine) as one JSON line to
/// every subscriber, as the Linux daemon sends them.
fn forward_hook_event(e: &sokki_engine::hooks::HookEvent) {
    let Ok(line) = serde_json::to_string(e) else { return };
    HOOK_SUBSCRIBERS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .retain(|tx| tx.send(line.clone()).is_ok());
}

/// Starts listening. A socket left by a crashed run is replaced; one that
/// still answers belongs to a running Sokki and is left alone.
pub fn start(app: &AppHandle) {
    let Some(path) = socket_path(app) else { return };
    if UnixStream::connect(&path).is_ok() {
        log::warn!("Local API: another Sokki answers on {}; not listening", path.display());
        return;
    }
    let _ = std::fs::remove_file(&path);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            log::warn!("Local API: can't listen on {}: {e}", path.display());
            return;
        }
    };
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    log::info!("Local API on {}", path.display());
    sokki_engine::hooks::subscribe(forward_hook_event);
    let app = app.clone();
    std::thread::Builder::new()
        .name("local-api".into())
        .spawn(move || {
            for conn in listener.incoming() {
                let Ok(conn) = conn else { continue };
                let app = app.clone();
                let _ = std::thread::Builder::new()
                    .name("local-api-conn".into())
                    .spawn(move || serve(&app, conn));
            }
        })
        .ok();
}

/// Removes the socket at quit, so clients say "not running" at once.
pub fn stop(app: &AppHandle) {
    if let Some(path) = socket_path(app) {
        let _ = std::fs::remove_file(path);
    }
}

fn serve(app: &AppHandle, conn: UnixStream) {
    let Ok(read) = conn.try_clone() else { return };
    let mut out = conn;
    let mut lines = BufReader::new(read).lines();
    while let Some(Ok(line)) = lines.next() {
        if line.trim().is_empty() {
            continue;
        }
        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let _ = send(&mut out, &Reply::error(format!("unknown request: {e}")));
                continue;
            }
        };
        if req == Request::Subscribe {
            subscribe(app, out);
            return;
        }
        let reply = handle(app, req);
        if send(&mut out, &reply).is_err() {
            return;
        }
    }
}

fn send<T: serde::Serialize>(out: &mut UnixStream, v: &T) -> std::io::Result<()> {
    let mut line = serde_json::to_string(v).unwrap_or_default();
    line.push('\n');
    out.write_all(line.as_bytes())?;
    out.flush()
}

fn ok_or(r: Result<String, String>) -> Reply {
    match r {
        Ok(_) => Reply::ok(),
        Err(e) => Reply::error(e),
    }
}

fn handle(app: &AppHandle, req: Request) -> Reply {
    let meetings = app.state::<Arc<MeetingManager>>();
    match req {
        Request::Status => Reply {
            state: Some(state(app)),
            ..Reply::ok()
        },
        Request::Subscribe => Reply::error("subscribe is handled on its own connection"),
        Request::MeetingRecord => {
            if crate::meeting::prompt::asking().is_some() {
                crate::meeting::prompt::answer("record".into());
                Reply::ok()
            } else if meetings.is_recording() {
                Reply::ok()
            } else {
                log::info!("Meeting start via the local API");
                ok_or(crate::meeting::start_meeting(app, None))
            }
        }
        Request::MeetingDismiss => {
            if crate::meeting::prompt::asking().is_some() {
                crate::meeting::prompt::answer("not_now".into());
            }
            Reply::ok()
        }
        Request::MeetingStop => {
            if meetings.is_recording() {
                log::info!("Meeting stop via the local API");
                ok_or(crate::meeting::stop_meeting(app))
            } else {
                Reply::ok()
            }
        }
        Request::MeetingToggle => {
            if crate::meeting::prompt::asking().is_some() {
                crate::meeting::prompt::answer("record".into());
            } else {
                crate::meeting::toggle_meeting(app, "the local API");
            }
            Reply::ok()
        }
        Request::DictationStart | Request::DictationStop => {
            let listening = dictation(app) == DictationState::Listening;
            let start = req == Request::DictationStart;
            if listening != start {
                crate::signal_handle::send_transcription_input(app, "transcribe", "the local API");
            }
            Reply::ok()
        }
        Request::DictationToggle => {
            crate::signal_handle::send_transcription_input(app, "transcribe", "the local API");
            Reply::ok()
        }
        Request::DictationCancel => {
            crate::utils::cancel_current_operation(app);
            Reply::ok()
        }
        Request::Notes { limit } => Reply {
            notes: Some(
                app.state::<Arc<MeetingStore>>()
                    .list()
                    .iter()
                    .filter(|r| !r.sample)
                    .take(limit)
                    .map(note_info)
                    .collect(),
            ),
            ..Reply::ok()
        },
        Request::Speakers { meeting } => {
            let store = app.state::<Arc<MeetingStore>>();
            let Some(record) = store.get(&meeting) else {
                return Reply::error(format!("no meeting {meeting}"));
            };
            let voices = crate::meeting::process::load_speakers(&store.audio_dir(&meeting));
            Reply {
                speakers: Some(
                    record
                        .speakers
                        .iter()
                        .map(|name| SpeakerInfo {
                            name: name.clone(),
                            unnamed: sokki_index::sync::is_placeholder(name),
                            has_voice: voices
                                .iter()
                                .any(|v| &v.name == name && v.embedding.is_some()),
                        })
                        .collect(),
                ),
                ..Reply::ok()
            }
        }
        Request::NameSpeaker {
            meeting,
            speaker,
            name,
            remember,
        } => match crate::commands::meeting::rename_meeting_speaker(
            app.clone(),
            meeting,
            speaker,
            name,
            remember,
        ) {
            Ok(()) => Reply::ok(),
            Err(e) => Reply::error(e),
        },
        Request::InstallModels => Reply::error(
            "On the Mac, Sokki downloads its models in the app (Settings → Models).",
        ),
        // The app applies settings_store.json changes within a second
        // (config_watch.rs).
        Request::Reload => Reply::ok(),
    }
}

fn dictation(app: &AppHandle) -> DictationState {
    let listening = app
        .try_state::<Arc<crate::managers::audio::AudioRecordingManager>>()
        .is_some_and(|a| a.is_recording());
    if listening {
        DictationState::Listening
    } else {
        DictationState::Idle
    }
}

fn ms(rfc3339: &str) -> u64 {
    sokki_index::time::rfc3339_ms(rfc3339).unwrap_or(0).max(0) as u64
}

fn note_info(r: &MeetingRecord) -> NoteInfo {
    let status = match &r.status {
        MeetingStatus::Recording => "recording",
        MeetingStatus::Processing { stage, .. } if stage == "waiting" => "queued",
        MeetingStatus::Processing { .. } => "processing",
        MeetingStatus::Done => "done",
        MeetingStatus::Failed { .. } => "failed",
    };
    NoteInfo {
        id: r.id.clone(),
        title: r.title.clone(),
        started_at: r.started_at.clone(),
        duration_s: r.duration_s,
        status: status.into(),
        path: r.note_path.clone(),
        app: r.source_app.clone(),
    }
}

/// Everything a client shows, whole.
fn state(app: &AppHandle) -> State {
    let store = app.state::<Arc<MeetingStore>>();
    let manager = app.state::<Arc<MeetingManager>>();
    let records = store.list();
    let meeting = if let Some(id) = manager.active_id() {
        let r = records.iter().find(|r| r.id == id);
        MeetingState::Recording {
            app: r.and_then(|r| r.source_app.clone()),
            since_ms: r.map(|r| ms(&r.started_at)).unwrap_or(0),
            call_audio: r.is_some_and(|r| r.tracks.iter().any(|t| t.starts_with("system"))),
            id,
        }
    } else if let Some(a) = crate::meeting::prompt::asking() {
        MeetingState::Asking {
            app: a.app,
            since_ms: a.since_ms,
            expires_ms: a.expires_ms,
        }
    } else {
        MeetingState::Idle
    };
    let mut processing = None;
    let mut queued = 0;
    for r in &records {
        if let MeetingStatus::Processing { stage, progress } = &r.status {
            if stage == "waiting" {
                queued += 1;
            } else if processing.is_none() {
                processing = Some(Processing {
                    id: r.id.clone(),
                    stage: stage.clone(),
                    progress: *progress,
                });
            }
        }
    }
    State {
        meeting,
        processing,
        queued,
        dictation: dictation(app),
        // The app's Models page manages downloads; the API reports ready.
        models: ModelsState {
            ready: true,
            busy: false,
            downloading: None,
        },
    }
}

/// Streams the whole state on every change, a `note` event when a
/// meeting's note is ready, and every hook event (`meeting.note_ready`, …).
/// Polls the app's own state every 250 ms, which costs nothing next to the
/// app; state events carry the whole state, so a client that misses one is
/// right with the next.
fn subscribe(app: &AppHandle, mut out: UnixStream) {
    let (tx, hook_events) = channel::<String>();
    HOOK_SUBSCRIBERS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(tx);
    let mut last: Option<State> = None;
    let done = |app: &AppHandle| -> Vec<(String, Option<String>)> {
        app.state::<Arc<MeetingStore>>()
            .list()
            .into_iter()
            .filter(|r| r.status == MeetingStatus::Done)
            .map(|r| (r.id, r.note_path))
            .collect()
    };
    let mut seen = done(app);
    loop {
        let now = state(app);
        if last.as_ref() != Some(&now) {
            if send(&mut out, &Event::State(now.clone())).is_err() {
                return;
            }
            last = Some(now);
        }
        let notes = done(app);
        for (id, path) in &notes {
            if !seen.iter().any(|(i, p)| i == id && p == path) {
                if let Some(r) = app.state::<Arc<MeetingStore>>().get(id) {
                    if send(&mut out, &Event::Note(note_info(&r))).is_err() {
                        return;
                    }
                }
            }
        }
        seen = notes;
        while let Ok(line) = hook_events.try_recv() {
            if writeln!(out, "{line}").and_then(|_| out.flush()).is_err() {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

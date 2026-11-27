//! `sokki daemon`: the Linux host. One thread owns all state and reacts to
//! messages (requests, detection, workers, dictation); helper threads only
//! block on I/O and send messages. No polling: the loop sleeps until a
//! message arrives or the prompt's deadline passes.

use anyhow::{Context, Result};
use meeting_detect::{MeetingApp, MeetingEvent};
use sokki_engine::lifecycle::{CallApp, Effect, Input, Lifecycle, Phase};
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::api::{
    DictationState, Download, Event, MeetingState, ModelsState, NoteInfo, Processing, Reply,
    Request, SpeakerInfo, State,
};
use crate::audio::Capture;
use crate::config::Config;
use crate::store::{Status, Store};

enum Msg {
    Request(Request, Sender<Reply>),
    Subscribe(UnixStream),
    Detect(MeetingEvent),
    WorkerLine(String, serde_json::Value),
    WorkerExit(String),
    DictationAudio(Vec<f32>),
    DictationDone(Result<String, String>),
    Download(Option<Download>),
    /// The speech models are on disk (or failed).
    SpeechReady(Result<(), String>),
    /// The whole install, language model included, is over.
    ModelsDone(Result<(), String>),
    /// From the hooks runner's thread.
    HookReport(sokki_engine::hooks::Report),
    /// A hook event, for subscribers (without content).
    HookEvent(Box<sokki_engine::hooks::HookEvent>),
    Shutdown,
}

/// What the hooks runner reads to keep out of a meeting's way.
const GATE_FREE: u8 = 0;
const GATE_RECORDING: u8 = 1;
const GATE_PROCESSING: u8 = 2;

struct Recording {
    id: String,
    mic: Capture,
    call: Option<Capture>,
    // No sleep until the recording ends; dropped with it, after both
    // captures are stopped.
    _awake: crate::keep_awake::KeepAwake,
}

struct Daemon {
    tx: Sender<Msg>,
    cfg: Config,
    store: Store,
    lifecycle: Lifecycle,
    asked_at_ms: u64,
    recording: Option<Recording>,
    /// Detected apps by id, for finding the call's own audio stream.
    apps: HashMap<String, MeetingApp>,
    queue: VecDeque<String>,
    processing: Option<Processing>,
    dictation: DictationState,
    dictation_capture: Option<Capture>,
    transcriber: Option<Arc<sokki_engine::asr::LocalTranscriber>>,
    models: ModelsState,
    downloading: bool,
    subscribers: Vec<UnixStream>,
    last_state: Option<State>,
    /// Recording or processing, for the hooks runner.
    gate: Arc<AtomicU8>,
    _watcher: Option<meeting_detect::Watcher>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn main() -> Result<()> {
    // Handle SIGTERM/SIGINT in one thread (systemd stop, Ctrl-C), so a
    // running recording is closed cleanly instead of cut off.
    let signals = block_signals();
    let cfg = Config::load(&crate::paths::config_file())?;
    let (tx, rx) = mpsc::channel();
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut sig = 0;
            unsafe { libc::sigwait(&signals, &mut sig) };
            let _ = tx.send(Msg::Shutdown);
        });
    }
    listen(tx.clone())?;
    let mut d = Daemon {
        lifecycle: Lifecycle::new(Duration::from_secs(cfg.meetings.ask_seconds.max(5))),
        store: Store::new(crate::paths::meetings_dir()),
        tx,
        cfg,
        asked_at_ms: 0,
        recording: None,
        apps: HashMap::new(),
        queue: VecDeque::new(),
        processing: None,
        dictation: DictationState::Idle,
        dictation_capture: None,
        transcriber: None,
        models: ModelsState {
            ready: crate::models::all_present(),
            busy: false,
            downloading: None,
        },
        downloading: false,
        subscribers: Vec::new(),
        last_state: None,
        gate: Arc::new(AtomicU8::new(GATE_FREE)),
        _watcher: None,
    };
    start_hooks(&d.tx, d.gate.clone());
    d.recover();
    d.start_detection();
    if !d.models.ready {
        d.install_models(false);
    }
    d.next_job();
    log::info!("Sokki is running ({})", crate::paths::socket().display());
    d.run(rx);
    Ok(())
}

/// The person's hooks (HOOKS.md): `~/.config/sokki/hooks.toml`, their
/// queue and logs in `~/.local/share/sokki/hooks/`.
fn start_hooks(tx: &Sender<Msg>, gate: Arc<AtomicU8>) {
    use sokki_engine::hooks::{self, Gate, Host};
    let report_tx = std::sync::Mutex::new(tx.clone());
    let event_tx = std::sync::Mutex::new(tx.clone());
    hooks::subscribe(move |e| {
        let _ = event_tx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .send(Msg::HookEvent(Box::new(e.clone())));
    });
    let host = Host {
        paths: crate::hooks::paths(),
        gate: Arc::new(move || match gate.load(Ordering::Relaxed) {
            GATE_RECORDING => Gate::Recording,
            GATE_PROCESSING => Gate::Processing,
            _ => Gate::Free,
        }),
        report: Arc::new(move |r| {
            let _ = report_tx
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .send(Msg::HookReport(r));
        }),
        env: Arc::new(|| {
            let notes = Config::load(&crate::paths::config_file())
                .map(|c| c.engine().notes_dir)
                .unwrap_or_default();
            vec![
                (
                    "SOKKI_NOTES_DIR".to_string(),
                    notes.to_string_lossy().into_owned(),
                ),
                (
                    "SOKKI_APP_DATA".to_string(),
                    crate::paths::data_dir().to_string_lossy().into_owned(),
                ),
            ]
        }),
    };
    if let Err(e) = hooks::start(host) {
        log::warn!("Hooks off: {e:#}");
    }
}

/// A meeting event from its record.
fn meeting_event(kind: &str, r: &crate::store::Record) {
    use sokki_engine::hooks::{HookEvent, Meta};
    sokki_engine::hooks::emit(HookEvent::meeting(
        kind,
        &r.id,
        r.note_path.as_deref().map(std::path::Path::new),
        Meta {
            title: r.title.clone(),
            started_at: Some(r.started_at.clone()),
            duration_s: (r.duration_s > 0.0).then_some(r.duration_s),
            app: r.source_app.clone(),
            participants: r.speakers.clone(),
            language: None,
        },
    ));
}

fn block_signals() -> libc::sigset_t {
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGTERM);
        libc::sigaddset(&mut set, libc::SIGINT);
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        set
    }
}

/// The socket server: one thread accepting, one per connection.
fn listen(tx: Sender<Msg>) -> Result<()> {
    let path = crate::paths::socket();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    if UnixStream::connect(&path).is_ok() {
        anyhow::bail!("Sokki is already running");
    }
    let _ = std::fs::remove_file(&path);
    let listener =
        UnixListener::bind(&path).with_context(|| format!("listening on {}", path.display()))?;
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let _ = serve(stream, tx);
            });
        }
    });
    Ok(())
}

fn serve(stream: UnixStream, tx: Sender<Msg>) -> Result<()> {
    let mut line = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut line)?;
    let req: Request = match serde_json::from_str(line.trim()) {
        Ok(r) => r,
        Err(e) => {
            return write_line(&stream, &Reply::error(format!("bad request: {e}")));
        }
    };
    if req == Request::Subscribe {
        let _ = tx.send(Msg::Subscribe(stream));
        return Ok(());
    }
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(Msg::Request(req, reply_tx))?;
    let reply = reply_rx
        .recv_timeout(Duration::from_secs(30))
        .unwrap_or_else(|_| Reply::error("no answer from the daemon"));
    write_line(&stream, &reply)
}

fn write_line(mut stream: &UnixStream, v: &impl serde::Serialize) -> Result<()> {
    let mut text = serde_json::to_string(v)?;
    text.push('\n');
    stream.write_all(text.as_bytes())?;
    Ok(())
}

impl Daemon {
    fn run(&mut self, rx: Receiver<Msg>) {
        loop {
            let msg = match self.lifecycle.next_deadline() {
                Some(at) => {
                    let wait = at.saturating_duration_since(Instant::now());
                    match rx.recv_timeout(wait) {
                        Ok(m) => Some(m),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
                None => match rx.recv() {
                    Ok(m) => Some(m),
                    Err(_) => return,
                },
            };
            match msg {
                None => self.lifecycle_input(Input::Tick),
                Some(Msg::Shutdown) => {
                    self.shutdown();
                    return;
                }
                Some(m) => self.handle(m),
            }
            self.broadcast_state();
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Request(req, reply) => {
                let r = self.request(req);
                let _ = reply.send(r);
            }
            Msg::Subscribe(stream) => {
                let state = Event::State(self.state());
                if write_line(&stream, &state).is_ok() {
                    self.subscribers.push(stream);
                }
            }
            Msg::Detect(MeetingEvent::Started { app }) => {
                log::info!("Call detected: {} (pid {})", label(&app), app.pid);
                let call = call_app(&app);
                self.apps.insert(app.id.to_string(), app);
                self.lifecycle_input(Input::CallStarted(call));
            }
            Msg::Detect(MeetingEvent::Ended { app, .. }) => {
                log::info!("Call ended: {}", label(&app));
                self.lifecycle_input(Input::CallEnded {
                    id: app.id.to_string(),
                });
            }
            Msg::WorkerLine(id, v) => {
                if let Some(stage) = v["stage"].as_str() {
                    self.processing = Some(Processing {
                        id,
                        stage: stage.to_string(),
                        progress: v["progress"].as_f64().map(|p| p as f32),
                    });
                }
            }
            Msg::WorkerExit(id) => self.worker_done(&id),
            Msg::DictationAudio(samples) => self.transcribe_dictation(samples),
            Msg::DictationDone(result) => {
                self.dictation = DictationState::Idle;
                match result {
                    Ok(text) => {
                        if let Err(e) = crate::output::deliver_text(&text, &self.cfg.dictation.output) {
                            self.error(format!("Dictation: {e:#}"));
                        }
                        // No dictation history here: the id is the time.
                        if !text.trim().is_empty() {
                            sokki_engine::hooks::emit(sokki_engine::hooks::HookEvent::dictation(
                                now_ms(),
                                &text,
                                Default::default(),
                            ));
                        }
                    }
                    Err(e) => self.error(format!("Dictation: {e}")),
                }
            }
            Msg::Download(d) => self.models.downloading = d,
            Msg::SpeechReady(result) => {
                self.models.ready = crate::models::all_present();
                if let Err(e) = result {
                    self.error(format!("Model download failed: {e}"));
                }
                self.next_job();
            }
            Msg::ModelsDone(result) => {
                self.downloading = false;
                self.models.busy = false;
                self.models.downloading = None;
                if let Err(e) = result {
                    self.error(format!("Language model download failed: {e}"));
                }
            }
            Msg::HookEvent(e) => {
                self.subscribers.retain(|s| write_line(s, &*e).is_ok());
            }
            Msg::HookReport(r) => {
                use sokki_engine::hooks::Report;
                match r {
                    Report::Failed { hook, what, log } => {
                        self.error(format!("Hook {hook} {what}. Log: {}", log.display()))
                    }
                    Report::NotAllowed { hook } => self.error(format!(
                        "Hook {hook} is new or changed. To let it run: sokki hooks allow {hook}"
                    )),
                    Report::ConfigInvalid { error } => {
                        self.error(format!("Hooks paused, hooks.toml has a mistake: {error}"))
                    }
                    Report::Succeeded { .. } => {}
                }
            }
            Msg::Shutdown => {}
        }
    }

    fn request(&mut self, req: Request) -> Reply {
        match req {
            Request::Status => Reply {
                state: Some(self.state()),
                ..Reply::ok()
            },
            Request::Subscribe => Reply::error("subscribe on its own connection"),
            Request::MeetingRecord => self.lifecycle_reply(Input::Start),
            Request::MeetingDismiss => self.lifecycle_reply(Input::No),
            Request::MeetingStop => self.lifecycle_reply(Input::Stop),
            Request::MeetingToggle => self.lifecycle_reply(Input::Toggle),
            Request::DictationStart => self.dictation_start(),
            Request::DictationStop => self.dictation_stop(false),
            Request::DictationCancel => self.dictation_stop(true),
            Request::DictationToggle => {
                if self.dictation == DictationState::Listening {
                    self.dictation_stop(false)
                } else {
                    self.dictation_start()
                }
            }
            Request::Notes { limit } => Reply {
                notes: Some(self.store.list().iter().take(limit).map(note_info).collect()),
                ..Reply::ok()
            },
            Request::Speakers { meeting } => {
                let dir = self.store.dir(&meeting);
                let speakers = sokki_engine::speakers::load(&dir)
                    .into_iter()
                    .map(|s| SpeakerInfo {
                        unnamed: sokki_engine::speakers::is_placeholder(&s.name),
                        has_voice: s.embedding.is_some(),
                        name: s.name,
                    })
                    .collect();
                Reply {
                    speakers: Some(speakers),
                    ..Reply::ok()
                }
            }
            Request::NameSpeaker {
                meeting,
                speaker,
                name,
                remember,
            } => match self.name_speaker(&meeting, &speaker, &name, remember) {
                Ok(()) => Reply::ok(),
                Err(e) => Reply::error(format!("{e:#}")),
            },
            Request::InstallModels => {
                self.install_models(true);
                Reply::ok()
            }
            Request::Reload => match Config::load(&crate::paths::config_file()) {
                Ok(cfg) => {
                    let detect_changed = cfg.meetings.detect_calls != self.cfg.meetings.detect_calls;
                    self.cfg = cfg;
                    if detect_changed {
                        self._watcher = None;
                        self.start_detection();
                    }
                    Reply::ok()
                }
                Err(e) => Reply::error(format!("{e:#}")),
            },
        }
    }

    // ------------------------------------------------------------ meetings

    fn lifecycle_reply(&mut self, input: Input) -> Reply {
        self.lifecycle_input(input);
        Reply {
            state: Some(self.state()),
            ..Reply::ok()
        }
    }

    fn lifecycle_input(&mut self, input: Input) {
        let label = format!("{input:?}");
        let effects = self.lifecycle.handle(Instant::now(), input);
        if !effects.is_empty() || !label.starts_with("Tick") {
            log::info!("{label} -> {:?} {:?}", self.lifecycle.phase(), effects);
        }
        for effect in effects {
            match effect {
                Effect::Ask(app) => {
                    self.asked_at_ms = now_ms();
                    // The shell plugin shows its own prompt; without one, a
                    // notification asks.
                    if self.subscribers.is_empty() {
                        crate::output::notify(
                            &format!("{} is using your microphone", app.label),
                            "Record this call?",
                            &[
                                ("Record", &["meeting", "record"]),
                                ("Not now", &["meeting", "dismiss"]),
                            ],
                        );
                    }
                }
                Effect::CloseAsk => {}
                Effect::StartRecording { app } => {
                    if let Err(e) = self.start_recording(app) {
                        self.error(format!("Recording failed to start: {e:#}"));
                        // Back to idle so the next try works.
                        self.lifecycle.handle(Instant::now(), Input::Stop);
                    }
                }
                Effect::StopRecording => self.stop_recording(),
            }
        }
    }

    fn start_recording(&mut self, app: Option<CallApp>) -> Result<()> {
        let record = self.store.create(app.as_ref().map(|a| a.label.clone()))?;
        let dir = self.store.dir(&record.id);
        // Held from before the captures open; released if starting fails.
        let awake = crate::keep_awake::KeepAwake::acquire();
        let mic = Capture::to_file(&dir.join(sokki_engine::tracks::MIC_FILE), None, false)?;
        // Everything the computer plays; only the call app's own stream if
        // the person turned that on (see `call_app_only`).
        let only_app = self.cfg.meetings.call_app_only;
        let target = app.as_ref().filter(|_| only_app).and_then(|a| {
            let m = self.apps.get(&a.id)?;
            crate::audio::playback_stream_of(m.exe_path.as_deref().unwrap_or(""), m.name)
        });
        let system_file = dir.join(sokki_engine::tracks::SYSTEM_FILE);
        let call = match &target {
            Some(serial) => Capture::to_file(&system_file, Some(serial), false),
            None => Capture::to_file(&system_file, None, true),
        };
        let call = match call {
            Ok(c) => Some(c),
            Err(e) => {
                log::warn!("Call audio not recorded: {e:#}");
                None
            }
        };
        log::info!(
            "Recording {} ({})",
            record.id,
            match &target {
                Some(t) => format!("call stream {t}"),
                None => "all output".into(),
            }
        );
        self.recording = Some(Recording {
            id: record.id,
            mic,
            call,
            _awake: awake,
        });
        Ok(())
    }

    fn stop_recording(&mut self) {
        let Some(rec) = self.recording.take() else {
            return;
        };
        let timing = sokki_engine::tracks::TrackTiming {
            mic_first_us: rec.mic.started_us,
            system_first_us: rec.call.as_ref().map(|c| c.started_us).unwrap_or(0),
        };
        let had_call = rec.call.is_some();
        let _ = rec.mic.stop();
        if let Some(c) = rec.call {
            let _ = c.stop();
        }
        let dir = self.store.dir(&rec.id);
        if let Ok(text) = serde_json::to_string(&timing) {
            let _ = std::fs::write(dir.join(sokki_engine::tracks::TIMING_FILE), text);
        }
        let mut tracks = vec![sokki_engine::tracks::MIC_FILE.to_string()];
        if had_call {
            tracks.push(sokki_engine::tracks::SYSTEM_FILE.to_string());
        }
        let started = self.store.get(&rec.id).ok().map(|r| r.started_at);
        let duration_s = started
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|t| (chrono::Local::now() - t.with_timezone(&chrono::Local)).num_milliseconds() as f64 / 1000.0)
            .unwrap_or(0.0);
        let _ = self.store.update(&rec.id, |r| {
            r.status = Status::Queued;
            r.tracks = tracks;
            r.duration_s = duration_s;
        });
        if let Ok(r) = self.store.get(&rec.id) {
            meeting_event(sokki_engine::hooks::event::MEETING_RECORDED, &r);
        }
        self.queue.push_back(rec.id);
        self.next_job();
    }

    /// Starts the next queued meeting in a worker, if none is running and
    /// the models are there.
    fn next_job(&mut self) {
        if self.processing.is_some() || !self.models.ready {
            return;
        }
        let Some(id) = self.queue.pop_front() else {
            return;
        };
        let exe = std::env::current_exe().unwrap_or_else(|_| "sokki".into());
        let child = Command::new(exe)
            .args(["process", &id])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                self.error(format!("Could not start processing: {e}"));
                return;
            }
        };
        self.processing = Some(Processing {
            id: id.clone(),
            stage: "preparing".into(),
            progress: None,
        });
        let stdout = child.stdout.take();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            if let Some(out) = stdout {
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    if let Ok(v) = serde_json::from_str(&line) {
                        let _ = tx.send(Msg::WorkerLine(id.clone(), v));
                    }
                }
            }
            let _ = child.wait();
            let _ = tx.send(Msg::WorkerExit(id));
        });
    }

    fn worker_done(&mut self, id: &str) {
        self.processing = None;
        match self.store.get(id) {
            Ok(r) if r.status == Status::Done => {
                meeting_event(sokki_engine::hooks::event::MEETING_NOTE_READY, &r);
                let info = note_info(&r);
                let title = r.title.clone().unwrap_or_else(|| "Meeting".into());
                self.send(&Event::Note(info));
                // The shell plugin announces notes itself.
                if self.subscribers.is_empty() {
                    crate::output::notify(
                        "Meeting note ready",
                        &title,
                        &[("Open", &["open", id])],
                    );
                }
            }
            Ok(r) => {
                let msg = r.error.unwrap_or_else(|| "processing stopped".into());
                self.error(format!("Meeting note failed: {msg}"));
            }
            Err(e) => self.error(format!("Meeting note failed: {e:#}")),
        }
        self.next_job();
    }

    fn name_speaker(&mut self, id: &str, from: &str, to: &str, remember: bool) -> Result<()> {
        let record = self.store.get(id)?;
        let dir = self.store.dir(id);
        let note = record.note_path.as_deref().map(std::path::Path::new);
        let embedding = sokki_engine::speakers::rename(&dir, note, from, to)?;
        if remember {
            let embedding =
                embedding.context("This speaker has no voice sample to remember.")?;
            diarize::VoiceProfiles::open(crate::paths::voice_profiles())?
                .enroll(to.trim(), &embedding)?;
        }
        self.store.update(id, |r| {
            for s in &mut r.speakers {
                if s == from {
                    *s = to.trim().to_string();
                }
            }
        })?;
        if let Ok(r) = self.store.get(id) {
            meeting_event(sokki_engine::hooks::event::MEETING_SPEAKERS_NAMED, &r);
        }
        Ok(())
    }

    /// After a restart: meetings that were waiting or being processed are
    /// queued again; a recording cut off by a crash is processed as far as
    /// it got.
    fn recover(&mut self) {
        let mut ids: Vec<String> = Vec::new();
        for r in self.store.list() {
            match r.status {
                Status::Queued | Status::Processing => ids.push(r.id),
                Status::Recording => {
                    let dir = self.store.dir(&r.id);
                    let tracks: Vec<String> = [
                        sokki_engine::tracks::MIC_FILE,
                        sokki_engine::tracks::SYSTEM_FILE,
                    ]
                    .iter()
                    .filter(|f| dir.join(f).exists())
                    .map(|f| f.to_string())
                    .collect();
                    let _ = self.store.update(&r.id, |r| {
                        r.status = Status::Queued;
                        r.tracks = tracks;
                        r.warnings.push("The recording was interrupted.".into());
                    });
                    ids.push(r.id);
                }
                _ => {}
            }
        }
        // Oldest first.
        ids.reverse();
        self.queue.extend(ids);
    }

    fn shutdown(&mut self) {
        log::info!("Stopping");
        if self.recording.is_some() {
            self.stop_recording();
        }
        if let Some(c) = self.dictation_capture.take() {
            let _ = c.stop();
        }
        // Hook events still in memory run at the next start.
        sokki_engine::hooks::flush();
        let _ = std::fs::remove_file(crate::paths::socket());
    }

    // ----------------------------------------------------------- detection

    fn start_detection(&mut self) {
        if !self.cfg.meetings.detect_calls {
            return;
        }
        let tx = self.tx.clone();
        match meeting_detect::Watcher::spawn(
            meeting_detect::WatcherConfig::default(),
            std::process::id(),
            move |e| {
                let _ = tx.send(Msg::Detect(e));
            },
        ) {
            Ok(w) => self._watcher = Some(w),
            Err(e) => self.error(format!("Call detection is off: {e}")),
        }
    }

    // ----------------------------------------------------------- dictation

    fn dictation_start(&mut self) -> Reply {
        if self.dictation != DictationState::Idle {
            return Reply::ok();
        }
        if !self.models.ready {
            self.install_models(false);
            return Reply::error("The speech model is still downloading.");
        }
        let mut cap = match Capture::raw() {
            Ok(c) => c,
            Err(e) => return Reply::error(format!("{e:#}")),
        };
        let stdout = cap.take_stdout();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let samples = stdout.map(crate::audio::read_f32).unwrap_or_default();
            let _ = tx.send(Msg::DictationAudio(samples));
        });
        self.dictation_capture = Some(cap);
        self.dictation = DictationState::Listening;
        // Load the model while the person speaks.
        self.load_transcriber();
        Reply::ok()
    }

    fn dictation_stop(&mut self, cancel: bool) -> Reply {
        let Some(cap) = self.dictation_capture.take() else {
            return Reply::ok();
        };
        let _ = cap.stop();
        self.dictation = if cancel {
            DictationState::Idle
        } else {
            DictationState::Transcribing
        };
        Reply::ok()
    }

    fn load_transcriber(&mut self) {
        if self.transcriber.is_some() {
            return;
        }
        let path = crate::models::path(&crate::models::PARAKEET);
        match sokki_engine::asr::LocalTranscriber::load(&path) {
            Ok(mut t) => {
                let lang = self.cfg.dictation.language.trim();
                if !lang.is_empty() {
                    t.set_language(Some(lang.into()));
                }
                self.transcriber = Some(Arc::new(t));
            }
            Err(e) => self.error(format!("Speech model: {e:#}")),
        }
    }

    fn transcribe_dictation(&mut self, samples: Vec<f32>) {
        if self.dictation != DictationState::Transcribing {
            return; // cancelled
        }
        let Some(t) = self.transcriber.clone() else {
            self.dictation = DictationState::Idle;
            return;
        };
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            log::info!("Dictation: {:.1} s of audio", samples.len() as f64 / 16_000.0);
            let r = t.transcribe_long(&samples).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::DictationDone(r));
        });
    }

    // -------------------------------------------------------------- models

    /// Downloads what is missing: the speech models, and with `with_llm`
    /// the local language model through Ollama (`sokki models install`, so
    /// the first note already gets its summary). Meetings can be processed
    /// as soon as the speech models are there.
    fn install_models(&mut self, with_llm: bool) {
        if self.downloading {
            return;
        }
        let speech = !crate::models::all_present();
        let llm = with_llm && llm_to_pull(&self.cfg).is_some();
        if !speech && !llm {
            self.models.ready = true;
            return;
        }
        self.downloading = true;
        self.models.busy = true;
        let tx = self.tx.clone();
        let cfg = self.cfg.clone();
        std::thread::spawn(move || {
            let speech_result = (|| -> Result<()> {
                for f in crate::models::all() {
                    let name = f.path.to_string();
                    crate::models::ensure(&f, |done, total| {
                        let _ = tx.send(Msg::Download(Some(Download {
                            file: name.clone(),
                            done,
                            total,
                        })));
                    })?;
                }
                Ok(())
            })();
            let _ = tx.send(Msg::SpeechReady(speech_result.map_err(|e| format!("{e:#}"))));
            let llm_result = match with_llm.then(|| llm_to_pull(&cfg)).flatten() {
                None => Ok(()),
                Some(model) => sokki_engine::llm::pull_model(&cfg.llm, |p| {
                    if p.total > 0 {
                        let _ = tx.send(Msg::Download(Some(Download {
                            file: model.clone(),
                            done: p.completed,
                            total: p.total,
                        })));
                    }
                }),
            };
            let _ = tx.send(Msg::ModelsDone(llm_result.map_err(|e| format!("{e:#}"))));
        });
    }

    // ---------------------------------------------------------------- state

    fn state(&self) -> State {
        let meeting = match self.lifecycle.phase() {
            Phase::Idle => MeetingState::Idle,
            Phase::Asking { app, .. } => MeetingState::Asking {
                app: app.label.clone(),
                since_ms: self.asked_at_ms,
                expires_ms: self.asked_at_ms + self.lifecycle.ask_timeout().as_millis() as u64,
            },
            Phase::Recording { app, .. } => match &self.recording {
                Some(r) => MeetingState::Recording {
                    id: r.id.clone(),
                    app: app.as_ref().map(|a| a.label.clone()),
                    since_ms: r.mic.started_us / 1000,
                    call_audio: r.call.is_some(),
                },
                None => MeetingState::Idle,
            },
        };
        State {
            meeting,
            processing: self.processing.clone(),
            queued: self.queue.len(),
            dictation: self.dictation,
            models: self.models.clone(),
        }
    }

    fn broadcast_state(&mut self) {
        let gate = if self.recording.is_some() {
            GATE_RECORDING
        } else if self.processing.is_some() {
            GATE_PROCESSING
        } else {
            GATE_FREE
        };
        self.gate.store(gate, Ordering::Relaxed);
        let state = self.state();
        if self.last_state.as_ref() == Some(&state) {
            return;
        }
        self.last_state = Some(state.clone());
        self.send(&Event::State(state));
    }

    fn send(&mut self, event: &Event) {
        self.subscribers.retain(|s| write_line(s, event).is_ok());
    }

    fn error(&mut self, message: String) {
        log::error!("{message}");
        self.send(&Event::Error {
            message: message.clone(),
        });
        if self.subscribers.is_empty() {
            crate::output::notify("Sokki", &message, &[]);
        }
    }
}

/// "Google Meet" for a call in a browser, else the app's name.
/// The local Ollama model to download, if summaries use one that Ollama
/// does not have yet. Starts an installed Ollama that is not running.
fn llm_to_pull(cfg: &Config) -> Option<String> {
    let llm = &cfg.llm;
    let local = !sokki_engine::llm::CLOUD_PROVIDERS.contains(&llm.provider.trim())
        && sokki_engine::llm::is_ollama(llm.base_url.trim().trim_end_matches('/'));
    if !local || !cfg.meetings.summary {
        return None;
    }
    let mut state = sokki_engine::llm::check(llm, &cfg.meetings.user_name, false).state;
    if state == "not_running" && sokki_engine::llm::start_ollama(llm).is_ok() {
        state = sokki_engine::llm::check(llm, &cfg.meetings.user_name, false).state;
    }
    (state == "model_missing").then(|| llm.model.trim().to_string())
}

fn label(app: &MeetingApp) -> String {
    app.label().to_string()
}

fn call_app(app: &MeetingApp) -> CallApp {
    CallApp {
        id: app.id.to_string(),
        label: label(app),
    }
}

fn note_info(r: &crate::store::Record) -> NoteInfo {
    NoteInfo {
        id: r.id.clone(),
        title: r.title.clone(),
        started_at: r.started_at.clone(),
        duration_s: r.duration_s,
        status: r.status.as_str().to_string(),
        path: r.note_path.clone(),
        app: r.source_app.clone(),
    }
}

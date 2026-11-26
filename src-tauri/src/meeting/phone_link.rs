//! Phone link: the Sokki phone app sends a recording to this Mac, the Mac
//! processes it like any meeting, and the phone fetches status, summary and
//! transcript.
//!
//! Direct and end-to-end encrypted, from any network (`sokki-link`, iroh):
//! the phone dials this Mac by its public key; nothing is stored in
//! between. When the Mac is off or asleep the phone's attempt fails and the
//! recording stays on the phone until the user tries again.
//!
//! Pairing: the settings page shows a ticket (QR code) with this Mac's key
//! and the pairing code, as a sokki.org/p link: a phone without Sokki lands
//! on the install page. A phone that sends the right code is remembered by
//! its own key; only remembered phones get answers. Each code pairs one
//! phone: pairing replaces it, and so does "New code". Paired phones keep
//! working until removed.
//!
//! Requests (see `sokki_link::Request`): `Pair`, `Ping` → `{name, version}`,
//! `Upload` (audio as body) → `{id}`, `Status` → `{id, status, stage,
//! progress, error, title, summary, transcript}`, `Dictate` (a live
//! stream: the phone keyboard dictates through this Mac, `dictate_for_phone`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sokki_link::{DictationText, Handler, Request, Ticket};
use tauri::{AppHandle, Manager};

use super::store::{MeetingRecord, MeetingStatus, MeetingStore};
use crate::managers::transcription::TranscriptionManager;

const STATE_FILE: &str = "phone_link.json";
const PHONE_TRACK: &str = "mic.phone";

static RUNNING: AtomicBool = AtomicBool::new(false);
/// Serialises read-modify-write of the state file.
static STATE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct PairedPhone {
    pub id: String,
    pub name: String,
    pub paired_at: String,
}

/// Pairing state on disk: this Mac's key, the pairing code, paired phones.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct State {
    #[serde(default)]
    code: String,
    #[serde(default)]
    key: String,
    #[serde(default)]
    phones: Vec<PairedPhone>,
}

/// What the settings page shows for pairing.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct PhoneLinkInfo {
    pub enabled: bool,
    pub running: bool,
    /// "sokki1:<this Mac's key>:<code>": what the phone scans or pastes.
    pub ticket: String,
    /// The ticket as a QR code (SVG markup).
    pub qr_svg: String,
    pub code: String,
    pub phones: Vec<PairedPhone>,
}

fn state_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join(STATE_FILE))
}

/// Loads the state, creating the key and code on first use.
fn load(app: &AppHandle) -> State {
    let path = state_path(app);
    let mut s: State = path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let mut changed = false;
    if s.code.is_empty() {
        s.code = new_code();
        changed = true;
    }
    if sokki_link::secret_from_hex(&s.key).is_err() {
        s.key = sokki_link::secret_to_hex(&sokki_link::new_secret());
        changed = true;
    }
    if changed {
        save(app, &s);
    }
    s
}

fn save(app: &AppHandle, s: &State) {
    if let Some(p) = state_path(app) {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, serde_json::to_string_pretty(s).unwrap_or_default());
    }
}

fn update<T>(app: &AppHandle, f: impl FnOnce(&mut State) -> T) -> T {
    let _guard = STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = load(app);
    let out = f(&mut s);
    save(app, &s);
    out
}

fn ticket(s: &State) -> String {
    sokki_link::secret_from_hex(&s.key)
        .map(|k| {
            Ticket {
                mac: k.public(),
                code: s.code.clone(),
            }
            .url()
        })
        .unwrap_or_default()
}

fn qr_svg(text: &str) -> String {
    qrcode::QrCode::new(text.as_bytes())
        .map(|c| {
            c.render::<qrcode::render::svg::Color>()
                .min_dimensions(200, 200)
                .quiet_zone(true)
                .build()
        })
        .unwrap_or_default()
}

/// A new pairing code: stops new pairings with the old one. Paired phones
/// keep working.
pub fn renew_code(app: &AppHandle) -> String {
    update(app, |s| {
        s.code = new_code();
        s.code.clone()
    })
}

/// Forgets a paired phone; it has to pair again.
pub fn remove_phone(app: &AppHandle, id: &str) {
    update(app, |s| s.phones.retain(|p| p.id != id));
}

fn new_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    // Random bytes from the OS, as for the link's keys. Reading /dev/urandom
    // by hand could fail silently and leave a code of all As.
    let bytes = sokki_link::new_secret().to_bytes();
    let chars: String = bytes[..12]
        .iter()
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
        .collect();
    format!("{}-{}-{}", &chars[0..4], &chars[4..8], &chars[8..12])
}

fn enabled(app: &AppHandle) -> bool {
    crate::settings::get_settings(app)
        .meetings
        .phone_link_enabled
}

pub fn info(app: &AppHandle) -> PhoneLinkInfo {
    let s = {
        let _guard = STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        load(app)
    };
    let ticket = ticket(&s);
    PhoneLinkInfo {
        enabled: enabled(app),
        running: RUNNING.load(Ordering::Relaxed),
        qr_svg: qr_svg(&ticket),
        ticket,
        code: s.code,
        phones: s.phones,
    }
}

/// Starts the link when the setting is on and it isn't running yet.
/// Turning the setting off makes it refuse every request; the link itself
/// closes at the next start of the app.
pub fn start_if_enabled(app: &AppHandle) {
    if !enabled(app) || RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let key = {
        let _guard = STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        load(app).key
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let secret = match sokki_link::secret_from_hex(&key) {
            Ok(k) => k,
            Err(e) => {
                log::error!("Phone link: bad key: {e}");
                RUNNING.store(false, Ordering::SeqCst);
                return;
            }
        };
        match sokki_link::bind(secret).await {
            Ok(endpoint) => {
                log::info!("Phone link: reachable as {}", endpoint.id());
                let router = sokki_link::serve(endpoint, Arc::new(MacHandler { app }));
                // Runs as long as the app does.
                std::mem::forget(router);
            }
            Err(e) => {
                log::error!("Phone link: {e:#}");
                RUNNING.store(false, Ordering::SeqCst);
            }
        }
    });
}

/// Compares in constant time, so the code can't be guessed byte by byte.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// Pairs `peer` when `code` is the current code, then replaces the code, so
/// each code pairs one phone: a photo of an old QR code can't pair another
/// phone later. A phone that is already paired is accepted with any code, so
/// it can repeat a pairing whose answer got lost.
fn try_pair(s: &mut State, peer: &str, code: &str, device: &str) -> bool {
    if s.phones.iter().any(|p| p.id == peer) {
        return true;
    }
    if !same(&code.trim().to_uppercase(), &s.code) {
        return false;
    }
    s.phones.push(PairedPhone {
        id: peer.to_string(),
        name: device.chars().take(60).collect(),
        paired_at: chrono::Local::now().to_rfc3339(),
    });
    s.code = new_code();
    true
}

struct MacHandler {
    app: AppHandle,
}

impl Handler for MacHandler {
    fn is_paired(&self, peer: &str) -> bool {
        let _guard = STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        enabled(&self.app) && load(&self.app).phones.iter().any(|p| p.id == peer)
    }

    fn pair(&self, peer: &str, code: &str, device: &str) -> Result<Value, String> {
        if !enabled(&self.app) {
            return Err("The phone link is switched off on the Mac.".into());
        }
        let ok = update(&self.app, |s| try_pair(s, peer, code, device));
        if !ok {
            return Err("Wrong pairing code. Scan the code on the Mac again.".into());
        }
        log::info!("Phone link: paired {device} ({peer})");
        Ok(hello())
    }

    fn handle(&self, _peer: &str, request: Request, body: Vec<u8>) -> Result<Value, String> {
        match request {
            Request::Pair { .. } => Err("Already handled.".into()),
            Request::Ping => {
                // The phone keyboard pings when it opens: be ready to dictate.
                if let Some(tm) = self.app.try_state::<Arc<TranscriptionManager>>() {
                    tm.initiate_model_load(false);
                }
                crate::dictation_cleanup::warm_for_phone(&self.app);
                Ok(hello())
            }
            Request::Upload { started_at, .. } => accept_upload(&self.app, &started_at, &body)
                .map(|id| json!({ "id": id }))
                .map_err(|e| format!("{e:#}")),
            Request::Status { id } => {
                status_json(&self.app, &id).ok_or_else(|| "No such meeting.".to_string())
            }
            Request::Journal { file } => save_phone_journal(&self.app, &file, &body)
                .map(|saved| json!({ "saved": saved }))
                .map_err(|e| format!("{e:#}")),
            Request::Dictate => Err("Dictation is a stream.".into()),
        }
    }

    fn dictate(
        &self,
        peer: &str,
        audio: std::sync::mpsc::Receiver<Vec<f32>>,
        text: &dyn Fn(DictationText),
    ) -> Result<(), String> {
        let tm = self
            .app
            .try_state::<Arc<TranscriptionManager>>()
            .ok_or("Sokki on the Mac is still starting.")?
            .inner()
            .clone();
        dictate_for_phone(&self.app, &tm, peer, audio, text)
    }
}

/// The answer while the model loads; the phone keeps the link and
/// dictates by itself this once.
const LOADING: &str = "The Mac is loading its speech model.";
const BUSY: &str = "The Mac is busy transcribing.";

/// How often the phone gets the text so far, as Mac dictation's live text.
const DICTATE_PREVIEW_EVERY: std::time::Duration = std::time::Duration::from_millis(500);
/// Live text re-reads the piece still growing; a piece ends at a pause
/// after this long (at most the second), is read once more and kept. These
/// pieces are the final text and what the cleanup works on while the person
/// speaks: cut at pauses (fixed 15 s cuts, with the whole dictation read
/// again at the stop, made the cleanup redo all its live steps: 7.3 s for a
/// 65 s dictation, 2026-11-26). 8–20 s pieces were as accurate as whole
/// dictations on Robin's 34 (10.5 % vs 10.3 % of words different), and the
/// shorter the last piece, the sooner the draft is there at the stop.
const DICTATE_PIECE_MIN_S: f32 = 8.0;
const DICTATE_PIECE_MAX_S: f32 = 20.0;

/// While a read runs, the phone hears from the Mac this often: a busy Mac
/// (a 10 s piece took 4.5 s on Robin's Mac under load, 2026-11-26) isn't
/// a Mac that's gone, and the phone takes over only from one that's gone.
const DICTATE_KEEPALIVE: std::time::Duration = std::time::Duration::from_secs(1);

/// Runs `job` on another thread and meanwhile sends the phone the text it
/// shows (`now`) every second.
fn keeping_alive<T: Send + 'static>(
    job: impl FnOnce() -> T + Send + 'static,
    heard: u64,
    now: &dyn Fn() -> String,
    text: &dyn Fn(DictationText),
) -> Result<T, String> {
    let job = std::thread::spawn(job);
    let mut beat = std::time::Instant::now();
    while !job.is_finished() {
        std::thread::sleep(std::time::Duration::from_millis(50));
        if beat.elapsed() >= DICTATE_KEEPALIVE {
            beat = std::time::Instant::now();
            text(DictationText {
                is_final: false,
                heard,
                text: now(),
                ..Default::default()
            });
        }
    }
    job.join().map_err(|_| "The Mac's read failed.".to_string())
}

/// Reads `audio` with the dictation model, keeping the phone posted.
fn read_keeping_alive(
    tm: &TranscriptionManager,
    audio: Vec<f32>,
    heard: u64,
    now: &dyn Fn() -> String,
    text: &dyn Fn(DictationText),
) -> Result<String, String> {
    let tm = tm.clone();
    keeping_alive(
        move || tm.transcribe(audio).map_err(|e| format!("{e:#}")),
        heard,
        now,
        text,
    )?
}

/// The phone keyboard dictates through this Mac: the same model and
/// settings as dictating here. Live text every 0.5 s (the growing piece read
/// again, as the Mac's own live text with Parakeet does); a piece that ends
/// at a pause after 8–20 s is read once more and kept; at the stop the
/// rest is read. With the dictation cleanup on, the text is cleaned and laid out as for a
/// dictation here, live too: the phone gets the cleaned paragraphs and the
/// rest as transcribed, and the cleaned dictation at the end. Nothing is
/// stored or added to the history. Measured with the phone's 13 test clips:
/// first words 0.66 s, done 0.16 s after the stop (`docs/research/
/// phone-dictation-latency-2026-11.md`).
fn dictate_for_phone(
    app: &AppHandle,
    tm: &TranscriptionManager,
    peer: &str,
    audio: std::sync::mpsc::Receiver<Vec<f32>>,
    text: &dyn Fn(DictationText),
) -> Result<(), String> {
    use std::sync::mpsc::RecvTimeoutError;
    let started = std::time::Instant::now();
    // Loading takes seconds; the phone dictates by itself meanwhile rather
    // than wait (its keyboard asked for the model when it opened).
    if !tm.is_model_loaded() {
        tm.initiate_model_load(false);
        return Err(LOADING.into());
    }
    // A dictation here or a meeting being transcribed holds the model; the
    // phone would wait behind it.
    if tm.is_streaming() || tm.job_active() {
        return Err(BUSY.into());
    }
    let mut cleanup = crate::dictation_cleanup::begin_for_phone(app);
    let mut heard: Vec<f32> = Vec::new();
    let mut done_text = String::new();
    let mut seg =
        dictation_core::Segmenter::new(dictation_core::Mode::Pauses(dictation_core::Params {
            min_s: DICTATE_PIECE_MIN_S,
            max_s: DICTATE_PIECE_MAX_S,
            ..Default::default()
        }));
    let mut ended_pieces: std::collections::VecDeque<Vec<f32>> = Default::default();
    let mut last_read = 0usize;
    let mut ended = false;
    let mut previews = 0usize;
    let mut shown = String::new();
    while !ended {
        match audio.recv_timeout(DICTATE_PREVIEW_EVERY) {
            Ok(a) => {
                ended_pieces.extend(seg.push(&a));
                heard.extend(a);
            }
            Err(RecvTimeoutError::Disconnected) => ended = true,
            Err(RecvTimeoutError::Timeout) => {}
        }
        // A paragraph was cleaned meanwhile: show it at once.
        if let Some(c) = cleanup.as_ref().filter(|c| c.stepped()) {
            shown = c.shown();
            text(DictationText {
                is_final: false,
                heard: last_read as u64,
                text: shown.clone(),
                ..Default::default()
            });
        }
        loop {
            match audio.try_recv() {
                Ok(a) => {
                    ended_pieces.extend(seg.push(&a));
                    heard.extend(a);
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    ended = true;
                    break;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
            }
        }
        if ended || heard.len() < last_read + DICTATE_PREVIEW_EVERY.as_millis() as usize * 16 {
            continue;
        }
        last_read = heard.len();
        let now = || {
            cleanup
                .as_ref()
                .map_or_else(|| shown.clone(), |c| c.shown())
        };
        // Pieces that ended at a pause: read whole, kept (the cleanup's
        // final part).
        while let Some(piece) = ended_pieces.pop_front() {
            let read = read_keeping_alive(tm, piece, last_read as u64, &now, text)?;
            done_text = format!("{done_text} {}", read.trim()).trim().to_string();
        }
        let read = read_keeping_alive(tm, seg.pending().to_vec(), last_read as u64, &now, text)?;
        previews += 1;
        let raw = format!("{done_text} {}", read.trim()).trim().to_string();
        shown = match cleanup.as_mut() {
            Some(c) => {
                c.live(&done_text, read.trim());
                c.shown()
            }
            None => raw.clone(),
        };
        text(DictationText {
            is_final: false,
            heard: heard.len() as u64,
            text: shown.clone(),
            ..Default::default()
        });
    }
    let t = std::time::Instant::now();
    let now = || {
        cleanup
            .as_ref()
            .map_or_else(|| shown.clone(), |c| c.shown())
    };
    // The final text: the pieces kept so far, then the rest. Not the whole
    // dictation read again: that differed enough from the pieces that the
    // cleanup redid every live step at the stop (7 s for 65 s, 2026-11-26),
    // and pieces cut at pauses are as accurate (8–20 s pieces: 10.5 % of
    // words different from the references on Robin's 34 dictations, whole
    // dictations 10.3 %; docs/research/phone-dictation-latency-2026-11.md).
    let mut final_text = done_text.clone();
    for piece in ended_pieces.drain(..).chain(seg.finish()) {
        let end = seg.speech_end(&piece);
        if end == 0 {
            continue;
        }
        let read = read_keeping_alive(tm, piece[..end].to_vec(), heard.len() as u64, &now, text)?;
        final_text = format!("{final_text} {}", read.trim()).trim().to_string();
    }
    let read_ms = t.elapsed().as_millis();
    let mut draft_ms = None;
    let final_text = match cleanup.take() {
        Some(c) => {
            // As the Mac's instant paste: the draft (paragraphs cleaned so
            // far, the rest as transcribed) goes to the phone at once, ready
            // to send; the cleaned text follows. Built here: the cleanup's
            // own draft waits for a step that still runs (0.2–1.7 s after
            // the stop; Robin wants the text within 0.7 s). Meanwhile the
            // keep-alive repeats the draft, flagged, so the phone shows it
            // once.
            let raw = final_text.clone();
            let draft = c.draft(&raw);
            draft_ms = Some(t.elapsed().as_millis());
            text(DictationText {
                heard: heard.len() as u64,
                text: draft.clone(),
                draft: true,
                ..Default::default()
            });
            let job = std::thread::spawn(move || c.finish(&raw, |_| {}));
            let mut beat = std::time::Instant::now();
            while !job.is_finished() {
                std::thread::sleep(std::time::Duration::from_millis(20));
                if beat.elapsed() >= DICTATE_KEEPALIVE {
                    beat = std::time::Instant::now();
                    text(DictationText {
                        heard: heard.len() as u64,
                        text: draft.clone(),
                        draft: true,
                        ..Default::default()
                    });
                }
            }
            let draft = Some(draft);
            job.join().ok().flatten().or(draft).unwrap_or(final_text)
        }
        None => final_text,
    };
    text(DictationText {
        is_final: true,
        heard: heard.len() as u64,
        text: final_text.trim().to_string(),
        ..Default::default()
    });
    log::info!(
        "Phone link: dictation for {peer}: {:.1} s of audio, {previews} live updates, final read in {read_ms} ms, draft {draft_ms:?} ms, done {} ms after the stop, {:.1} s in all",
        heard.len() as f64 / 16_000.0,
        t.elapsed().as_millis(),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

fn hello() -> Value {
    json!({ "name": crate::brand::app_name(), "version": env!("CARGO_PKG_VERSION") })
}

/// One day of the phone keyboard's journal (developer page "Keep what I
/// write" on the phone), into `<app data>/typing_log/phone/<file>` next to
/// the Mac's own journal (same line format). Lines already there are skipped
/// (same opened, written and text), so sending a day again adds only what's
/// new, and deleting on the phone deletes nothing here. Returns the lines
/// added. Text only, on this Mac.
fn save_phone_journal(app: &AppHandle, file: &str, body: &[u8]) -> anyhow::Result<usize> {
    if !is_journal_name(file) {
        anyhow::bail!("Not a journal file: {file}");
    }
    let body =
        std::str::from_utf8(body).map_err(|_| anyhow::anyhow!("The journal is not text."))?;
    let dir = app.path().app_data_dir()?.join("typing_log").join("phone");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(file);
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let new = new_journal_lines(&old, body);
    if !new.is_empty() {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        for line in &new {
            writeln!(f, "{line}")?;
        }
    }
    log::info!("Phone link: journal {file}, {} new entries", new.len());
    Ok(new.len())
}

/// The lines of `body` not in `old` yet (by opened, written and text).
fn new_journal_lines<'a>(old: &str, body: &'a str) -> Vec<&'a str> {
    fn key(line: &str) -> Option<String> {
        let v: Value = serde_json::from_str(line).ok()?;
        Some(format!("{}|{}|{}", v["opened"], v["written"], v["text"]))
    }
    let mut seen: std::collections::HashSet<String> = old.lines().filter_map(key).collect();
    body.lines()
        .filter(|l| !l.trim().is_empty())
        .filter(|l| key(l).is_none_or(|k| seen.insert(k)))
        .collect()
}

/// `journal-YYYY-MM-DD.jsonl`, nothing else (no paths).
fn is_journal_name(file: &str) -> bool {
    let Some(date) = file
        .strip_prefix("journal-")
        .and_then(|r| r.strip_suffix(".jsonl"))
    else {
        return false;
    };
    date.len() == 10
        && date.char_indices().all(|(i, c)| {
            if i == 4 || i == 7 {
                c == '-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// Stores the audio as a new meeting and starts processing it.
fn accept_upload(app: &AppHandle, started_at: &str, audio: &[u8]) -> anyhow::Result<String> {
    use anyhow::Context;
    if audio.len() < 1024 {
        anyhow::bail!("The recording is empty.");
    }
    let store = app.state::<Arc<MeetingStore>>();
    let now = chrono::Local::now();
    let id = format!("meeting-{}-phone", now.format("%Y%m%d-%H%M%S-%3f"));
    let dir = store.audio_dir(&id);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(PHONE_TRACK);
    std::fs::write(&path, audio)?;
    let samples = crate::audio_toolkit::audio::decode_audio_file(&path)
        .context("The recording could not be read")?;
    let record = MeetingRecord {
        id: id.clone(),
        started_at: chrono::DateTime::parse_from_rfc3339(started_at)
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|_| now.to_rfc3339()),
        duration_s: samples.len() as f64 / 16_000.0,
        status: MeetingStatus::Processing {
            stage: "waiting".into(),
            progress: None,
        },
        tracks: vec![PHONE_TRACK.to_string()],
        source_app: Some("Phone".into()),
        title: None,
        note_path: None,
        speakers: Vec::new(),
        warnings: Vec::new(),
        summary_missing: None,
        audio_deleted: false,
        online: None,
        title_by_hand: false,
        title_provisional: false,
        sample: false,
        mailed: None,
    };
    store.insert(record)?;
    log::info!(
        "Phone link: meeting {id} received ({} KB, {:.0} s)",
        audio.len() / 1024,
        samples.len() as f64 / 16_000.0
    );
    super::emit_changed(app, &id);
    super::process::spawn(app.clone(), id.clone(), None);
    Ok(id)
}

fn status_json(app: &AppHandle, id: &str) -> Option<Value> {
    let store = app.state::<Arc<MeetingStore>>();
    let r = store.get(id)?;
    let (status, stage, progress, error) = match &r.status {
        MeetingStatus::Recording => ("recording", None, None, None),
        MeetingStatus::Processing { stage, progress } => {
            ("processing", Some(stage.clone()), *progress, None)
        }
        MeetingStatus::Done => ("done", None, None, None),
        MeetingStatus::Failed { error } => ("failed", None, None, Some(error.clone())),
    };
    let parts = r
        .note_path
        .as_deref()
        .filter(|_| status == "done")
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|md| meeting_core::note::split_note(&md));
    Some(json!({
        "id": r.id,
        "status": status,
        "stage": stage,
        "progress": progress,
        "error": error,
        "title": r.title,
        "summary": parts.as_ref().map(|p| p.summary.clone()),
        "transcript": parts.as_ref().map(|p| p.transcript.clone()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_grouped_and_unambiguous() {
        let c = new_code();
        assert_eq!(c.len(), 14);
        assert_eq!(c.matches('-').count(), 2);
        assert!(!c.contains('0') && !c.contains('O') && !c.contains('1') && !c.contains('I'));
        assert_ne!(new_code(), c);
    }

    #[test]
    fn comparison_needs_exact_match() {
        assert!(same("AB", "AB"));
        assert!(!same("AB", "AC"));
        assert!(!same("A", "AB"));
    }

    #[test]
    fn a_code_pairs_one_phone() {
        let mut s = State {
            code: "ABCD-EFGH-JKLM".into(),
            key: String::new(),
            phones: vec![],
        };
        assert!(!try_pair(&mut s, "phone-a", "WRONG", "Pixel"));
        assert!(s.phones.is_empty());
        assert_eq!(s.code, "ABCD-EFGH-JKLM");

        assert!(try_pair(&mut s, "phone-a", "abcd-efgh-jklm", "Pixel"));
        assert_eq!(s.phones.len(), 1);
        assert_ne!(s.code, "ABCD-EFGH-JKLM", "the used code is replaced");

        // The old code no longer pairs another phone.
        assert!(!try_pair(&mut s, "phone-b", "ABCD-EFGH-JKLM", "Galaxy"));
        // The paired phone may repeat its pairing, without a new entry or code.
        let code = s.code.clone();
        assert!(try_pair(&mut s, "phone-a", "ABCD-EFGH-JKLM", "Pixel"));
        assert_eq!(s.phones.len(), 1);
        assert_eq!(s.code, code);
        // The new code pairs the next phone.
        assert!(try_pair(&mut s, "phone-b", &code, "Galaxy"));
        assert_eq!(s.phones.len(), 2);
    }

    #[test]
    fn ticket_qr_renders() {
        let s = State {
            code: "ABCD-EFGH-JKLM".into(),
            key: sokki_link::secret_to_hex(&sokki_link::new_secret()),
            phones: vec![],
        };
        let t = ticket(&s);
        assert!(Ticket::parse(&t).is_ok());
        assert!(qr_svg(&t).starts_with("<?xml") || qr_svg(&t).contains("<svg"));
    }

    #[test]
    fn journal_names() {
        assert!(is_journal_name("journal-2026-11-24.jsonl"));
        for bad in [
            "journal-2026-11-24.json",
            "../journal-2026-11-24.jsonl",
            "journal-2026-1-24.jsonl",
            "journal-2026-11-24.jsonl/x",
            "taps-2026-11-24.jsonl",
            "journal-2026/11/24.jsonl",
            "",
        ] {
            assert!(!is_journal_name(bad), "{bad}");
        }
    }

    #[test]
    fn journal_lines_merge() {
        let a = r#"{"t":"entry","opened":1,"written":2,"text":"one"}"#;
        let b = r#"{"t":"entry","opened":3,"written":4,"text":"two"}"#;
        assert_eq!(new_journal_lines("", &format!("{a}\n{b}\n")), vec![a, b]);
        assert_eq!(
            new_journal_lines(&format!("{a}\n"), &format!("{a}\n{b}\n")),
            vec![b]
        );
        assert!(new_journal_lines(&format!("{a}\n{b}\n"), &format!("{a}\n{b}\n")).is_empty());
    }
}

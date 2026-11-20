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
//! progress, error, title, summary, transcript}`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sokki_link::{Handler, Request, Ticket};
use tauri::{AppHandle, Manager};

use super::store::{MeetingRecord, MeetingStatus, MeetingStore};

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
            Request::Ping => Ok(hello()),
            Request::Upload { started_at, .. } => accept_upload(&self.app, &started_at, &body)
                .map(|id| json!({ "id": id }))
                .map_err(|e| format!("{e:#}")),
            Request::Status { id } => {
                status_json(&self.app, &id).ok_or_else(|| "No such meeting.".to_string())
            }
        }
    }
}

fn hello() -> Value {
    json!({ "name": crate::brand::app_name(), "version": env!("CARGO_PKG_VERSION") })
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
    };
    store.insert(record)?;
    log::info!(
        "Phone link: meeting {id} received ({} KB, {:.0} s)",
        audio.len() / 1024,
        samples.len() as f64 / 16_000.0
    );
    super::emit_changed(app, &id);
    super::process::spawn(app.clone(), id.clone());
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
}

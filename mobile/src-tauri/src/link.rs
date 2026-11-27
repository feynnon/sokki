//! The phone's side of the link to Sokki on the Mac (`sokki-link`): direct
//! and end-to-end encrypted, from any network. Every call is one attempt;
//! if the Mac is off, it fails and the page says so.
//!
//! Stored in the app's data folder: `link_key.txt` (this phone's key,
//! created once) and `pairing.json` (the Mac's key and name).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sokki_link::iroh::{Endpoint, EndpointId};
use sokki_link::{Request, Ticket};
use tauri::{AppHandle, Manager, State};
use tokio::sync::OnceCell;

#[derive(Default)]
pub struct Link {
    endpoint: OnceCell<Endpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pairing {
    pub mac: String,
    pub name: String,
}

fn dir(app: &AppHandle) -> Result<PathBuf, String> {
    let d = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d)
}

fn load_pairing(app: &AppHandle) -> Option<Pairing> {
    let text = std::fs::read_to_string(dir(app).ok()?.join("pairing.json")).ok()?;
    serde_json::from_str(&text).ok()
}

async fn endpoint<'a>(app: &AppHandle, link: &'a Link) -> Result<&'a Endpoint, String> {
    link.endpoint
        .get_or_try_init(|| async {
            let path = dir(app)?.join("link_key.txt");
            let key = match std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| sokki_link::secret_from_hex(&t).ok())
            {
                Some(k) => k,
                None => {
                    let k = sokki_link::new_secret();
                    std::fs::write(&path, sokki_link::secret_to_hex(&k))
                        .map_err(|e| e.to_string())?;
                    k
                }
            };
            sokki_link::bind(key).await.map_err(|e| format!("{e:#}"))
        })
        .await
}

async fn call(
    app: &AppHandle,
    link: &Link,
    request: Request,
    body: &[u8],
) -> Result<Value, String> {
    let pairing = load_pairing(app).ok_or("Not paired with a Mac yet.")?;
    let mac: EndpointId = pairing
        .mac
        .parse()
        .map_err(|_| "The pairing is damaged; pair again.")?;
    let ep = endpoint(app, link).await?;
    sokki_link::call(ep, mac, &request, body)
        .await
        .map_err(|e| format!("{e:#}"))
}

/// The paired Mac, if any.
#[tauri::command]
pub fn link_pairing(app: AppHandle) -> Option<Pairing> {
    load_pairing(&app)
}

/// Pairs with the Mac from its ticket (QR code or pasted text).
#[tauri::command]
pub async fn link_pair(
    app: AppHandle,
    link: State<'_, Link>,
    ticket: String,
    device: String,
) -> Result<Pairing, String> {
    let t = Ticket::parse(&ticket).map_err(|e| e.to_string())?;
    let ep = endpoint(&app, &link).await?;
    let v = sokki_link::call(
        ep,
        t.mac,
        &Request::Pair {
            code: t.code,
            device,
        },
        &[],
    )
    .await
    .map_err(|e| format!("{e:#}"))?;
    let p = Pairing {
        mac: t.mac.to_string(),
        name: v["name"].as_str().unwrap_or("Mac").to_string(),
    };
    std::fs::write(
        dir(&app)?.join("pairing.json"),
        serde_json::to_string(&p).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;
    Ok(p)
}

#[tauri::command]
pub fn link_unpair(app: AppHandle) -> Result<(), String> {
    let _ = std::fs::remove_file(dir(&app)?.join("pairing.json"));
    Ok(())
}

#[tauri::command]
pub async fn link_ping(app: AppHandle, link: State<'_, Link>) -> Result<Value, String> {
    call(&app, &link, Request::Ping, &[]).await
}

/// Sends a recording (base64: Android's IPC carries no raw bytes).
/// Returns the Mac's meeting id.
#[tauri::command]
pub async fn link_upload(
    app: AppHandle,
    link: State<'_, Link>,
    audio: String,
    started_at: String,
    mime: String,
) -> Result<String, String> {
    use base64::Engine as _;
    let audio = base64::engine::general_purpose::STANDARD
        .decode(audio)
        .map_err(|e| format!("The recording is damaged: {e}"))?;
    let v = call(&app, &link, Request::Upload { started_at, mime }, &audio).await?;
    v["id"]
        .as_str()
        .map(str::to_string)
        .ok_or("The Mac sent no id.".into())
}

/// One day of the keyboard's journal to the Mac (developer page "Keep what I write").
#[tauri::command]
pub async fn link_send_journal(
    app: AppHandle,
    link: State<'_, Link>,
    file: String,
    text: String,
) -> Result<Value, String> {
    call(&app, &link, Request::Journal { file }, text.as_bytes()).await
}

#[tauri::command]
pub async fn link_status(
    app: AppHandle,
    link: State<'_, Link>,
    id: String,
) -> Result<Value, String> {
    call(&app, &link, Request::Status { id }, &[]).await
}

/// How long one live request may take. On a fresh app the first requests
/// after pairing sometimes got no answer for minutes (emulator, 26 Nov
/// 2026) while the Mac had answered; a new attempt on a new connection is
/// better than waiting. The page tries again next round.
const LIVE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(12);

async fn call_live(
    app: &AppHandle,
    link: &Link,
    request: Request,
    body: &[u8],
) -> Result<Value, String> {
    tokio::time::timeout(LIVE_TIMEOUT, call(app, link, request, body))
        .await
        .map_err(|_| LIVE_TIMED_OUT.to_string())?
}

/// The error a live request gives when it took too long (the page retries).
const LIVE_TIMED_OUT: &str = "live: no answer in time";

/// Starts sending a meeting as it records (`LiveStart`): the Mac
/// transcribes it live. Each piece is a request of its own, on a new
/// connection like every other call: a connection kept open for the meeting
/// stopped carrying requests after the first on the emulator (26 Nov 2026),
/// while new ones answered in 16 ms. Returns the Mac's meeting id. A Mac without this
/// answers an error; the recording is then sent whole after the stop.
#[tauri::command]
pub async fn link_live_start(
    app: AppHandle,
    link: State<'_, Link>,
    started_at: String,
    mime: String,
) -> Result<String, String> {
    let v = call_live(&app, &link, Request::LiveStart { started_at, mime }, &[]).await?;
    v["id"]
        .as_str()
        .map(str::to_string)
        .ok_or("The Mac sent no id.".into())
}

/// The recording's bytes from `offset` (base64). Returns the bytes the Mac has.
#[tauri::command]
pub async fn link_live_part(
    app: AppHandle,
    link: State<'_, Link>,
    id: String,
    offset: u64,
    audio: String,
) -> Result<u64, String> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(audio)
        .map_err(|e| format!("The recording is damaged: {e}"))?;
    let v = call_live(&app, &link, Request::LivePart { id, offset }, &bytes).await?;
    v["have"].as_u64().ok_or("The Mac sent no length.".into())
}

/// The recording is complete at `total` bytes: `{have}`; the Mac processes
/// it once `have == total`.
#[tauri::command]
pub async fn link_live_end(
    app: AppHandle,
    link: State<'_, Link>,
    id: String,
    total: u64,
) -> Result<Value, String> {
    call_live(&app, &link, Request::LiveEnd { id, total }, &[]).await
}

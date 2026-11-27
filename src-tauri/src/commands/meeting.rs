//! Tauri commands for meeting notes. Meeting recording is macOS only. On
//! other platforms the same commands exist (so the generated bindings match)
//! and report that the feature is unavailable.

#[cfg(target_os = "macos")]
use crate::meeting::{
    self,
    store::{MeetingRecord, MeetingStatus, MeetingStore},
    MeetingManager,
};
use crate::settings::MeetingSettings;
#[cfg(target_os = "macos")]
use std::sync::Arc;
use tauri::AppHandle;

/// A meeting's note, split for the app: summary (Markdown) and transcript
/// (plain "[00:03] Speaker 2: Hi." lines, ready to copy).
#[derive(Clone, Debug, serde::Serialize, specta::Type)]
pub struct MeetingNoteParts {
    pub summary: String,
    pub transcript: String,
    /// The person's own lines with Sokki's additions under them (the
    /// note's "Meine Notizen" section, Markdown), empty without notes.
    pub combined: String,
}

/// A QR code (SVG) for a link the app hands out, e.g. sokki.org/get.
#[tauri::command]
#[specta::specta]
pub fn qr_code_svg(text: String) -> Result<String, String> {
    qrcode::QrCode::new(text.as_bytes())
        .map(|c| {
            c.render::<qrcode::render::svg::Color>()
                .min_dimensions(160, 160)
                .quiet_zone(true)
                .build()
        })
        .map_err(|e| e.to_string())
}

/// Asks macOS for "System Audio Recording" by opening the system-sound tap
/// for a moment (that shows macOS's prompt the first time). macOS has no way
/// to read the answer, so this only asks.
#[tauri::command]
#[specta::specta]
pub async fn request_system_audio_permission() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        return tokio::task::spawn_blocking(|| {
            let path = std::env::temp_dir().join(format!("sokki-ask-{}.flac", std::process::id()));
            let capture = crate::meeting::system_tap::SystemAudioCapture::start(path.clone(), None)
                .map_err(|e| e.to_string())?;
            std::thread::sleep(std::time::Duration::from_millis(500));
            let _ = capture.stop();
            let _ = std::fs::remove_file(&path);
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    Err("System sound needs macOS.".into())
}

/// The answer from the call window (`meeting/prompt.rs`): "not_now",
/// "record", "record_screen", "keep" or "stop".
#[tauri::command]
#[specta::specta]
pub fn meeting_prompt_answer(answer: String) -> Result<(), String> {
    crate::meeting::prompt::answer(answer);
    Ok(())
}

/// Phone link pairing details for the settings page.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn get_phone_link_info(app: AppHandle) -> Result<meeting::phone_link::PhoneLinkInfo, String> {
    Ok(meeting::phone_link::info(&app))
}

/// A new pairing code; stops new pairings with the old one (paired phones
/// keep working).
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn renew_phone_link_code(app: AppHandle) -> Result<String, String> {
    Ok(meeting::phone_link::renew_code(&app))
}

/// Forgets a paired phone.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn remove_paired_phone(app: AppHandle, id: String) -> Result<(), String> {
    meeting::phone_link::remove_phone(&app, &id);
    Ok(())
}

/// Sent whenever a meeting record changes (recording, progress, done, failed).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, specta::Type, tauri_specta::Event)]
pub struct MeetingChanged {
    pub id: String,
}
#[cfg(target_os = "macos")]
use tauri::{Manager, State};

/// Off the main thread, like the shortcut's path: opening and closing the
/// captures must never freeze the window or the recording pill.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub async fn start_meeting_recording(app: AppHandle) -> Result<String, String> {
    log::info!("Meeting start via the Sokki window");
    tokio::task::spawn_blocking(move || meeting::start_meeting(&app, None))
        .await
        .map_err(|e| e.to_string())?
}

/// Stops the meeting and starts turning it into a note. Returns its id.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub async fn stop_meeting_recording(app: AppHandle) -> Result<String, String> {
    log::info!("Meeting stop via the recording pill or the Sokki window");
    tokio::task::spawn_blocking(move || meeting::stop_meeting(&app))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn is_meeting_recording(meetings: State<'_, Arc<MeetingManager>>) -> bool {
    meetings.is_recording()
}

/// Moves the running meeting to another microphone ("Sokki hears nothing"
/// hint in the recording bar). `name` as `get_available_microphones` lists it.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn switch_meeting_microphone(
    meetings: State<'_, Arc<MeetingManager>>,
    name: String,
) -> Result<(), String> {
    meetings.switch_mic(&name).map_err(|e| e.to_string())
}

/// Meetings, newest first.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn list_meetings(store: State<'_, Arc<MeetingStore>>) -> Result<Vec<MeetingRecord>, String> {
    Ok(store.list())
}

/// The last meetings that started by themselves (at most 3), newest first,
/// with their titles, for Settings › Meetings.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn auto_meeting_recent(
    app: AppHandle,
    store: State<'_, Arc<MeetingStore>>,
) -> Vec<crate::meeting::auto::AutoStart> {
    let records = store.list();
    let mut list = crate::meeting::auto::recent(&app);
    for e in &mut list {
        e.title = records
            .iter()
            .find(|r| r.id == e.meeting_id)
            .and_then(|r| r.title.clone());
    }
    list
}

/// Opens the meeting's note: in Obsidian when the notes folder is inside a
/// vault, otherwise with the default app for Markdown.
/// Summary and transcript of a meeting, read from its note (so edits made
/// in the note show up here).
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn get_meeting_note(
    store: State<'_, Arc<MeetingStore>>,
    id: String,
) -> Result<MeetingNoteParts, String> {
    let record = store.get(&id).ok_or("Meeting not found")?;
    let path = record.note_path.ok_or("This meeting has no note yet")?;
    let md = std::fs::read_to_string(&path)
        .map_err(|e| format!("The note could not be read ({path}): {e}"))?;
    let p = meeting_core::note::split_note(&md);
    Ok(MeetingNoteParts {
        summary: p.summary,
        transcript: p.transcript,
        combined: p.own_notes,
    })
}

/// What the person typed into the meeting's notes, as typed.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn get_meeting_notes(store: State<'_, Arc<MeetingStore>>, id: String) -> String {
    crate::meeting::own_notes::text(&store, &id)
}

/// Saves the meeting's notes; called as the person types.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn set_meeting_notes(
    store: State<'_, Arc<MeetingStore>>,
    id: String,
    text: String,
) -> Result<(), String> {
    let record = store.get(&id).ok_or("Meeting not found")?;
    crate::meeting::own_notes::save(&store, &record, &text).map_err(|e| format!("{e:#}"))
}

/// Saves the meeting's note as one web page (title, date, summary; no
/// transcript) that opens in any browser, then shows it in Finder.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn save_meeting_page(
    app: AppHandle,
    store: State<'_, Arc<MeetingStore>>,
    id: String,
    path: String,
    date: String,
) -> Result<(), String> {
    let record = store.get(&id).ok_or("Meeting not found")?;
    let note = record.note_path.ok_or("This meeting has no note yet")?;
    let md = std::fs::read_to_string(&note)
        .map_err(|e| format!("The note could not be read ({note}): {e}"))?;
    let lang = crate::settings::get_settings(&app).app_language;
    let lang = lang.split('-').next().unwrap_or("en").to_string();
    let title = record.title.unwrap_or_else(|| "Meeting".to_string());
    let html = meeting_core::share::note_html(&md, &title, &date, &lang);
    std::fs::write(&path, html)
        .map_err(|e| format!("The page could not be saved ({path}): {e}"))?;
    let _ = std::process::Command::new("open")
        .args(["-R", &path])
        .spawn();
    Ok(())
}

/// What "Send to everyone" would do for a meeting: the calendar event,
/// who gets the mail and who is left out, and the mail itself. Never opens
/// Mail or asks for anything.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn meeting_mail_preview(
    app: AppHandle,
    id: String,
) -> Result<crate::meeting::mail::MailPreview, String> {
    crate::meeting::mail::preview(&app, &id)
}

/// Sends the meeting's notes to everyone in its calendar event, from the
/// person's Apple Mail. Asks for the calendar first if it never did.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub async fn meeting_mail_send(
    app: AppHandle,
    id: String,
) -> Result<crate::meeting::store::Mailed, String> {
    tokio::task::spawn_blocking(move || crate::meeting::mail::send(&app, &id, false))
        .await
        .map_err(|e| e.to_string())?
}

/// Asks for what sending needs (calendar, then control of Mail) and says
/// where it stands. Called when the person turns automatic sending on.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub async fn mail_setup(app: AppHandle) -> Result<crate::meeting::mail::MailSetup, String> {
    tokio::task::spawn_blocking(move || crate::meeting::mail::setup(&app))
        .await
        .map_err(|e| e.to_string())
}

/// Whether this build can send through Gmail, and the connected address.
#[cfg(target_os = "macos")]
#[derive(serde::Serialize, specta::Type)]
pub struct GmailStatus {
    pub available: bool,
    pub connected: Option<String>,
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn gmail_status(app: AppHandle) -> GmailStatus {
    GmailStatus {
        available: crate::meeting::gmail_account::offered(&app),
        connected: crate::meeting::gmail_account::connected(&app),
    }
}

/// Opens Google's consent page (only `gmail.send`) and waits for the
/// answer, up to 5 minutes. Returns the connected address.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub async fn gmail_connect(app: AppHandle) -> Result<String, String> {
    tokio::task::spawn_blocking(move || crate::meeting::gmail_account::connect(&app))
        .await
        .map_err(|e| e.to_string())?
}

/// Revokes Sokki's access at Google and forgets the token.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub async fn gmail_disconnect(app: AppHandle) -> Result<(), String> {
    tokio::task::spawn_blocking(move || crate::meeting::gmail_account::disconnect(&app))
        .await
        .map_err(|e| e.to_string())?
}

/// The people who asked for no more mail, lower case.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn mail_objections(app: AppHandle) -> Vec<String> {
    crate::meeting::mail::objections(&app)
        .list()
        .iter()
        .map(|o| o.email.clone())
        .collect()
}

/// Puts an address on the no-more-mail list. False: no address, or listed.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn mail_objection_add(app: AppHandle, email: String) -> Result<bool, String> {
    crate::meeting::mail::add_objection(&app, &email)
}

/// Takes an address off the no-more-mail list.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn mail_objection_remove(app: AppHandle, email: String) -> Result<bool, String> {
    crate::meeting::mail::remove_objection(&app, &email)
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn open_meeting_note(store: State<'_, Arc<MeetingStore>>, id: String) -> Result<(), String> {
    let record = store.get(&id).ok_or("Meeting not found")?;
    let path = record.note_path.ok_or("This meeting has no note yet")?;
    let path = std::path::PathBuf::from(path);
    if !path.exists() {
        return Err(format!("The note was moved or deleted: {}", path.display()));
    }
    let target = match obsidian_url(&path) {
        Some(url) => url,
        None => path.to_string_lossy().into_owned(),
    };
    std::process::Command::new("open")
        .arg(&target)
        .spawn()
        .map_err(|e| format!("Opening the note failed: {e}"))?;
    Ok(())
}

/// `obsidian://open?path=…` when the file sits inside an Obsidian vault.
#[cfg(target_os = "macos")]
fn obsidian_url(path: &std::path::Path) -> Option<String> {
    let in_vault = path
        .ancestors()
        .skip(1)
        .any(|dir| dir.join(".obsidian").is_dir());
    if !in_vault {
        return None;
    }
    let encoded: String = path
        .to_string_lossy()
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect();
    Some(format!("obsidian://open?path={encoded}"))
}

/// Processes a meeting again from its audio (e.g. after changing the model).
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn reprocess_meeting(app: AppHandle, id: String) -> Result<(), String> {
    let store = app.state::<Arc<MeetingStore>>();
    let record = store.get(&id).ok_or("Meeting not found")?;
    if record.audio_deleted {
        return Err("The audio of this meeting was already deleted.".into());
    }
    if matches!(
        record.status,
        MeetingStatus::Recording | MeetingStatus::Processing { .. }
    ) {
        return Err("This meeting is still being recorded or processed.".into());
    }
    store
        .update(&id, |r| {
            r.status = MeetingStatus::Processing {
                stage: "waiting".into(),
                progress: None,
            };
            r.warnings.clear();
            r.summary_missing = None;
        })
        .map_err(|e| e.to_string())?;
    meeting::emit_changed(&app, &id);
    meeting::process::spawn(app.clone(), id, None);
    Ok(())
}

/// Adds the missing summary to a done meeting's note, from the note itself
/// (works after the audio is deleted). Starts Ollama first if it isn't
/// running. The card follows through `emit_changed`.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn summarize_meeting(app: AppHandle, id: String) -> Result<(), String> {
    let store = app.state::<Arc<MeetingStore>>();
    let record = store.get(&id).ok_or("Meeting not found")?;
    if record.status != MeetingStatus::Done || record.note_path.is_none() {
        return Err("This meeting has no note yet.".into());
    }
    meeting::process::spawn_summarize(app.clone(), id, true);
    Ok(())
}

/// The locale dates and times follow: the Mac's region and clock setting
/// (System Settings → General → Language & Region), not the app language.
/// "en_DE" → "en-DE" (24.11., 15:20); the 24-hour switch, when set, as a
/// Unicode extension ("en-US-u-hc-h23"). `None` where the system gives none.
#[tauri::command]
#[specta::specta]
pub fn date_format_locale() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSLocale, NSString, NSUserDefaults};
        let id = NSLocale::currentLocale().localeIdentifier().to_string();
        let defaults = NSUserDefaults::standardUserDefaults();
        let force = |key: &str| defaults.boolForKey(&NSString::from_str(key));
        let cycle = if force("AppleICUForce24HourTime") {
            Some("h23")
        } else if force("AppleICUForce12HourTime") {
            Some("h12")
        } else {
            None
        };
        return bcp47(&id, cycle);
    }
    #[cfg(not(target_os = "macos"))]
    None
}

/// "en_DE@calendar=gregorian" → "en-DE", with `-u-hc-<cycle>` when given.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn bcp47(id: &str, cycle: Option<&str>) -> Option<String> {
    let base = id.split('@').next()?.replace('_', "-");
    if base.is_empty() {
        return None;
    }
    Some(match cycle {
        Some(c) => format!("{base}-u-hc-{c}"),
        None => base,
    })
}

#[cfg(test)]
mod locale_tests {
    #[test]
    fn mac_locale_ids_become_bcp47() {
        assert_eq!(super::bcp47("en_DE", None).as_deref(), Some("en-DE"));
        assert_eq!(
            super::bcp47("en_US@calendar=gregorian", Some("h23")).as_deref(),
            Some("en-US-u-hc-h23")
        );
        assert_eq!(super::bcp47("", None), None);
    }
}

/// Sets the title the person typed; nothing replaces it later. It goes into
/// the note's title heading too (once there is a note).
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn set_meeting_title(app: AppHandle, id: String, title: String) -> Result<(), String> {
    let store = app.state::<Arc<MeetingStore>>();
    let mut typed = false;
    let r = store
        .update(&id, |r| typed = meeting::title::set_typed(r, &title))
        .map_err(|e| e.to_string())?;
    if !typed {
        return Err("The title is empty.".into());
    }
    meeting::title::write_typed_into_note(&r);
    meeting::emit_changed(&app, &id);
    crate::local_index::refresh(&app);
    Ok(())
}

/// Deletes the meeting record and its audio. The note stays in the notes folder.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn delete_meeting(app: AppHandle, id: String) -> Result<(), String> {
    let store = app.state::<Arc<MeetingStore>>();
    let record = store.get(&id);
    if let Some(r) = &record {
        if matches!(
            r.status,
            MeetingStatus::Recording | MeetingStatus::Processing { .. }
        ) {
            return Err("Stop the recording or wait for processing to finish first.".into());
        }
    }
    store.remove(&id).map_err(|e| e.to_string())?;
    crate::video::delete_for_meeting(&id);
    meeting::emit_changed(&app, &id);
    // Its rows leave the index too (secure_delete).
    crate::local_index::refresh(&app);
    if let Some(r) = &record {
        crate::hooks::meeting_event_for(sokki_engine::hooks::event::MEETING_DELETED, r);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn set_meeting_settings(app: AppHandle, settings: MeetingSettings) -> Result<(), String> {
    let mut all = crate::settings::get_settings(&app);
    all.meetings = settings;
    crate::settings::write_settings(&app, all);
    meeting::detect::sync_with_settings(&app);
    meeting::phone_link::start_if_enabled(&app);
    Ok(())
}

/// The language model's state for the settings page. `probe` makes a tiny
/// real request to a cloud provider.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub async fn meeting_llm_status(
    app: AppHandle,
    probe: bool,
) -> Result<meeting::process::LlmStatus, String> {
    let all = crate::settings::get_settings(&app);
    let keys: std::collections::HashMap<String, String> = (*all.post_process_api_keys).clone();
    tauri::async_runtime::spawn_blocking(move || {
        meeting::process::llm_status(&all.meetings, &keys, probe)
    })
    .await
    .map_err(|e| e.to_string())
}

/// Starts Ollama when it is installed but not running.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub async fn start_ollama(app: AppHandle) -> Result<(), String> {
    let settings = crate::settings::get_settings(&app).meetings;
    tauri::async_runtime::spawn_blocking(move || meeting::process::start_ollama(&settings))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{e:#}"))
}

/// Downloads the local language model (built in, or through Ollama).
/// Progress arrives as "meeting-llm-pull" events ({status, completed,
/// total}). While a download runs, waits for it instead of starting another
/// (`meeting::pull`).
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub async fn pull_meeting_llm(app: AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    let settings = crate::settings::get_settings(&app).meetings;
    tauri::async_runtime::spawn_blocking(move || {
        if settings.llm_provider.trim() == sokki_engine::llm::BUILTIN {
            return meeting::process::download_builtin(&app);
        }
        meeting::pull::LLM.run(|note| {
            meeting::process::pull_model(&settings, |p| {
                note(p.clone());
                let _ = app.emit("meeting-llm-pull", p);
            })
            .map_err(|e| format!("{e:#}"))
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A running download of the language model.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct LlmDownload {
    /// "starting", "downloading" or "verifying" (Ollama: its own words).
    pub status: String,
    pub completed: u64,
    /// 0 until known.
    pub total: u64,
}

/// What setup needs to offer the summary model.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct SummarySetup {
    /// The Mac's memory, in bytes; 0 when unknown.
    pub memory_bytes: u64,
    /// The built-in model: "ready" (on disk), "missing" (can be
    /// downloaded) or "unavailable" (this build or Mac has none).
    pub builtin: String,
    /// The download running now, if any.
    pub download: Option<LlmDownload>,
}

/// The Mac's memory in bytes (`hw.memsize`), 0 when it can't be read.
#[cfg(target_os = "macos")]
fn memory_bytes() -> u64 {
    let mut bytes: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    // SAFETY: hw.memsize is a u64; `len` holds its size.
    let r = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            &mut bytes as *mut u64 as *mut libc::c_void,
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if r == 0 {
        bytes
    } else {
        0
    }
}

/// Memory, the built-in summary model's state, and its download, for
/// setup's summary screen. Cheap: no model is loaded.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub async fn summary_model_setup(app: AppHandle) -> Result<SummarySetup, String> {
    let all = crate::settings::get_settings(&app);
    let builtin = MeetingSettings {
        llm_provider: sokki_engine::llm::BUILTIN.to_string(),
        ..all.meetings.clone()
    };
    let keys: std::collections::HashMap<String, String> = (*all.post_process_api_keys).clone();
    let state = tauri::async_runtime::spawn_blocking(move || {
        meeting::process::llm_status(&builtin, &keys, false).state
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(SummarySetup {
        memory_bytes: memory_bytes(),
        builtin: match state.as_str() {
            "ready" => "ready",
            "model_missing" => "missing",
            _ => "unavailable",
        }
        .into(),
        download: meeting::pull::LLM.progress().map(|p| LlmDownload {
            status: p.status,
            completed: p.completed,
            total: p.total,
        }),
    })
}

/// Starts the sample meeting (`meeting::sample`): a two-minute call of
/// invented people, played and recorded in real time, then processed.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn start_sample_meeting(app: AppHandle) -> Result<String, String> {
    meeting::sample::start(&app)
}

/// Stops the sample meeting and deletes it. Does nothing to a real meeting.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn stop_sample_meeting(app: AppHandle) -> Result<(), String> {
    if meeting::sample::active_id().is_some() {
        meeting::discard_meeting(&app)?;
    }
    Ok(())
}

/// A line the live transcript has heard.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct LiveLine {
    /// Seconds from the recording's start.
    pub start_s: f64,
    pub text: String,
}

/// What the running meeting's live transcript has heard so far, in order.
/// Empty when nothing records.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn meeting_live_transcript(app: AppHandle) -> Vec<LiveLine> {
    app.state::<Arc<MeetingManager>>()
        .heard()
        .map(|h| {
            h.lines()
                .into_iter()
                .map(|(start_s, text)| LiveLine { start_s, text })
                .collect()
        })
        .unwrap_or_default()
}

/// Renames a speaker of a processed meeting in its note. With `remember`,
/// the voice is saved under the new name so later meetings recognise it.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn rename_meeting_speaker(
    app: AppHandle,
    id: String,
    from: String,
    to: String,
    remember: bool,
) -> Result<(), String> {
    let to = to.trim().to_string();
    if to.is_empty() {
        return Err("The new name is empty.".into());
    }
    let store = app.state::<Arc<MeetingStore>>();
    let record = store.get(&id).ok_or("Meeting not found")?;
    if let Some(path) = record.note_path.as_deref() {
        let md =
            std::fs::read_to_string(path).map_err(|e| format!("Reading the note failed: {e}"))?;
        let renamed = meeting_core::note::rename_speaker_in_markdown(&md, &from, &to);
        std::fs::write(path, renamed).map_err(|e| format!("Saving the note failed: {e}"))?;
    }
    let dir = store.audio_dir(&id);
    let mut speakers = meeting::process::load_speakers(&dir);
    let embedding = speakers
        .iter()
        .find(|s| s.name == from)
        .and_then(|s| s.embedding.clone());
    for s in &mut speakers {
        if s.name == from {
            s.name = to.clone();
        }
    }
    let _ = std::fs::write(
        dir.join(meeting::process::SPEAKERS_FILE),
        serde_json::to_string(&speakers).unwrap_or_default(),
    );
    if remember {
        let embedding = embedding.ok_or("This speaker has no voice sample to remember.")?;
        let mut profiles =
            meeting::process::voice_profiles(&app).ok_or("Voice profiles unavailable")?;
        profiles
            .enroll(&to, &embedding)
            .map_err(|e| format!("{e:#}"))?;
    }
    store
        .update(&id, |r| {
            for s in &mut r.speakers {
                if *s == from {
                    *s = to.clone();
                }
            }
        })
        .map_err(|e| e.to_string())?;
    meeting::emit_changed(&app, &id);
    crate::local_index::refresh(&app);
    crate::hooks::meeting_event(
        &app,
        sokki_engine::hooks::event::MEETING_SPEAKERS_NAMED,
        &id,
    );
    Ok(())
}

/// Names of remembered voices.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn list_voice_profiles(app: AppHandle) -> Result<Vec<String>, String> {
    Ok(meeting::process::voice_profiles(&app)
        .map(|p| p.names())
        .unwrap_or_default())
}

#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn delete_voice_profile(app: AppHandle, name: String) -> Result<(), String> {
    let mut profiles =
        meeting::process::voice_profiles(&app).ok_or("Voice profiles unavailable")?;
    profiles.remove(&name).map_err(|e| format!("{e:#}"))?;
    Ok(())
}

/// Every voice across meetings, for the Sprecher page.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub async fn list_voices(app: AppHandle) -> Result<meeting::voices::VoicesPage, String> {
    tauri::async_runtime::spawn_blocking(move || meeting::voices::page(&app))
        .await
        .map_err(|e| e.to_string())
}

/// Names Sokki proposes for the unnamed speakers of a meeting.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub async fn meeting_speaker_proposals(
    app: AppHandle,
    id: String,
) -> Result<Vec<meeting::voices::SpeakerProposal>, String> {
    tauri::async_runtime::spawn_blocking(move || meeting::voices::speaker_proposals(&app, &id))
        .await
        .map_err(|e| e.to_string())
}

/// Whom Sokki may ask, once all speakers of a meeting are named, whether
/// it may keep their voice.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub async fn meeting_voice_offer(app: AppHandle, id: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || meeting::voices::voice_offer(&app, &id))
        .await
        .map_err(|e| e.to_string())
}

/// The voice question of a meeting was answered ("Merken" or "Nicht jetzt").
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub async fn voice_offer_done(app: AppHandle, id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || meeting::voices::voice_offer_done(&app, &id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{e:#}"))
}

/// Gives speakers of one or more meetings a name. With `consent`, the
/// person agreed and Sokki keeps their voice. Returns how many meetings
/// changed.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub async fn name_voice(
    app: AppHandle,
    members: Vec<meeting::voices::VoiceMember>,
    name: String,
    emails: Vec<String>,
    consent: bool,
) -> Result<u32, String> {
    tauri::async_runtime::spawn_blocking(move || {
        meeting::voices::name_voice(&app, &members, &name, &emails, consent)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("{e:#}"))
}

/// Sets a person's email addresses.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn set_person_emails(app: AppHandle, name: String, emails: Vec<String>) -> Result<(), String> {
    meeting::voices::set_emails(&app, &name, emails).map_err(|e| format!("{e:#}"))
}

/// A few seconds of a speaker, as a WAV data URL.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub async fn speaker_clip(
    app: AppHandle,
    id: String,
    speaker: String,
    start_s: Option<f64>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        meeting::voices::clip(&app, &id, &speaker, start_s)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("{e:#}"))
}

// ------------------------------------------------------------ other OSes

#[cfg(not(target_os = "macos"))]
const UNAVAILABLE: &str = "Meeting notes are only available on macOS.";

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn start_meeting_recording() -> Result<String, String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn stop_meeting_recording() -> Result<String, String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn is_meeting_recording() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn switch_meeting_microphone(_name: String) -> Result<(), String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn list_meetings() -> Result<Vec<String>, String> {
    Ok(Vec::new())
}

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct PairedPhone {
    pub id: String,
    pub name: String,
    pub paired_at: String,
}

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct PhoneLinkInfo {
    pub enabled: bool,
    pub running: bool,
    pub ticket: String,
    pub qr_svg: String,
    pub code: String,
    pub phones: Vec<PairedPhone>,
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn get_phone_link_info() -> Result<PhoneLinkInfo, String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn renew_phone_link_code() -> Result<String, String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn remove_paired_phone(_id: String) -> Result<(), String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn get_meeting_note(_id: String) -> Result<MeetingNoteParts, String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn get_meeting_notes(_id: String) -> String {
    String::new()
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn set_meeting_notes(_id: String, _text: String) -> Result<(), String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn save_meeting_page(_id: String, _path: String, _date: String) -> Result<(), String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn open_meeting_note(_id: String) -> Result<(), String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn reprocess_meeting(_id: String) -> Result<(), String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn summarize_meeting(_id: String) -> Result<(), String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn set_meeting_title(_id: String, _title: String) -> Result<(), String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn delete_meeting(_id: String) -> Result<(), String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn set_meeting_settings(app: AppHandle, settings: MeetingSettings) -> Result<(), String> {
    let mut all = crate::settings::get_settings(&app);
    all.meetings = settings;
    crate::settings::write_settings(&app, all);
    Ok(())
}

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct LlmStatus {
    pub state: String,
    pub detail: String,
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub async fn meeting_llm_status(probe: bool) -> Result<LlmStatus, String> {
    let _ = probe;
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub async fn start_ollama() -> Result<(), String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub async fn pull_meeting_llm() -> Result<(), String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub async fn summary_model_setup() -> Result<SummarySetup, String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn start_sample_meeting() -> Result<String, String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn stop_sample_meeting() -> Result<(), String> {
    Ok(())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn meeting_live_transcript() -> Vec<LiveLine> {
    Vec::new()
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn rename_meeting_speaker(
    _id: String,
    _from: String,
    _to: String,
    _remember: bool,
) -> Result<(), String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn list_voice_profiles() -> Result<Vec<String>, String> {
    Ok(Vec::new())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn delete_voice_profile(_name: String) -> Result<(), String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub async fn list_voices() -> Result<crate::meeting::voice_types::VoicesPage, String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub async fn meeting_speaker_proposals(
    _id: String,
) -> Result<Vec<crate::meeting::voice_types::SpeakerProposal>, String> {
    Ok(Vec::new())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub async fn meeting_voice_offer(_id: String) -> Result<Vec<String>, String> {
    Ok(Vec::new())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub async fn voice_offer_done(_id: String) -> Result<(), String> {
    Ok(())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub async fn name_voice(
    _members: Vec<crate::meeting::voice_types::VoiceMember>,
    _name: String,
    _emails: Vec<String>,
    _consent: bool,
) -> Result<u32, String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn set_person_emails(_name: String, _emails: Vec<String>) -> Result<(), String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub async fn speaker_clip(
    _id: String,
    _speaker: String,
    _start_s: Option<f64>,
) -> Result<String, String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::obsidian_url;

    #[test]
    fn obsidian_url_only_inside_a_vault() {
        let vault = std::env::temp_dir().join(format!("hm-vault-{}", std::process::id()));
        std::fs::create_dir_all(vault.join(".obsidian")).unwrap();
        std::fs::create_dir_all(vault.join("Meetings")).unwrap();
        let note = vault.join("Meetings/2026-11-17 1000 Kick-off ä.md");
        let url = obsidian_url(&note).unwrap();
        assert!(url.starts_with("obsidian://open?path=/"));
        assert!(url.contains("Kick-off%20%C3%A4.md"));
        assert!(obsidian_url(&std::env::temp_dir().join("x.md")).is_none());
    }
}

/// Whether `code` is a valid supporter code (at least €20 paid), so the
/// settings can unlock the Sokki line's switch. Processing checks the
/// stored code again on its own.
#[tauri::command]
#[specta::specta]
pub fn is_supporter_code(code: String) -> bool {
    sokki_engine::supporter::verify(&code).is_some()
}

/// The Sokki line as it will stand under the next summary, for the
/// settings preview: German or English (`german`), and without "local,
/// free" when a cloud model writes the summary.
#[tauri::command]
#[specta::specta]
pub fn sokki_line(app: AppHandle, german: bool) -> String {
    let settings = crate::meeting::process::engine_settings(&crate::settings::get_settings(&app));
    let language = if german {
        meeting_core::lang::Language::German
    } else {
        meeting_core::lang::Language::English
    };
    meeting_core::note::sokki_line(language, settings.note_options().cloud_summary).to_string()
}

// ------------------------------------------------------------------ Slack

/// Slack's state for Settings › Meetings (`meeting/slack.rs`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SlackStatus {
    /// Slack is set up in this build (the Slack app's client id is in).
    pub available: bool,
    pub connected: bool,
    /// The workspace's name, when connected.
    pub team: Option<String>,
}

#[cfg(not(target_os = "macos"))]
const NO_SLACK: SlackStatus = SlackStatus {
    available: false,
    connected: false,
    team: None,
};

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn slack_status() -> SlackStatus {
    NO_SLACK
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub async fn slack_connect() -> Result<SlackStatus, String> {
    Err(UNAVAILABLE.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub fn slack_cancel_connect() {}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
#[specta::specta]
pub async fn slack_disconnect() -> Result<SlackStatus, String> {
    Ok(NO_SLACK)
}

/// Whether Slack is connected, for Settings › Meetings.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn slack_status(app: AppHandle) -> meeting::slack::SlackStatus {
    meeting::slack::status(&app)
}

/// Signs in with Slack in the browser; returns once the person allowed it
/// (or after five minutes, or when cancelled).
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub async fn slack_connect(app: AppHandle) -> Result<meeting::slack::SlackStatus, String> {
    tokio::task::spawn_blocking(move || meeting::slack::connect(&app).map_err(|e| format!("{e:#}")))
        .await
        .map_err(|e| e.to_string())?
}

/// Stops a running Slack sign-in.
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub fn slack_cancel_connect(app: AppHandle) {
    meeting::slack::cancel_connect(&app);
}

/// Forgets the Slack sign-in (and revokes it at Slack).
#[cfg(target_os = "macos")]
#[tauri::command]
#[specta::specta]
pub async fn slack_disconnect(app: AppHandle) -> Result<meeting::slack::SlackStatus, String> {
    tokio::task::spawn_blocking(move || meeting::slack::disconnect(&app))
        .await
        .map_err(|e| e.to_string())
}

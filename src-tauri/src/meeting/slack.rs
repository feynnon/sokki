//! After a Slack huddle, the notes go into the huddle's thread, posted as
//! the person: the summary with the Sokki line, the transcript as a file.
//! The Slack calls are in the `sokki-slack` crate; this is the Mac side.
//!
//! 1. Settings › Meetings › "Connect Slack" signs in once ([`connect`]).
//!    The token is kept in `slack_auth.json` in the app's data folder
//!    (mode 0600), not in the settings file.
//! 2. While Sokki records a detected Slack call, it asks Slack every 15 s
//!    which huddle the person is in (the call id, `users.profile.get`) and
//!    reads the huddle's channel from the Slack app's log
//!    (`sokki_slack::desktop`), and keeps both in the meeting's folder
//!    (`slack.json`; [`recording_started`]).
//!    Also who was in the huddle (`room.participant_history`, once a
//!    minute) and, when the recording ends, their names and addresses
//!    (`users.info`), so "Send to everyone" (meeting/mail.rs) can mail the
//!    notes after a huddle as after a calendar meeting ([`huddle_event`]).
//! 3. When the note has its summary ([`note_ready`], also after a summary
//!    that came later), Sokki finds the huddle's message and posts into its
//!    thread, once per meeting.
//!
//! Switch: `meetings.slack_post_huddles` (on after connecting). Only in
//! builds made with the Slack app's client id ([`client_id`]).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sokki_slack::{huddle, mrkdwn, oauth, Auth, Client, OAuthApp};
use tauri::{AppHandle, Manager};

use super::store::{MeetingStatus, MeetingStore};
use super::MeetingManager;

const AUTH_FILE: &str = "slack_auth.json";
/// In each meeting's folder: which huddle, and whether it was posted.
const HUDDLE_FILE: &str = "slack.json";
const POLL_EVERY: Duration = Duration::from_secs(15);
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);
/// A PKCE refresh token runs out 30 days after it was issued; refreshing
/// at launch once a week keeps a quiet sign-in alive.
const KEEP_ALIVE_S: u64 = 7 * 24 * 3600;

#[derive(Default)]
pub struct SlackState {
    /// The running sign-in, to cancel it.
    signing_in: Mutex<Option<Arc<AtomicBool>>>,
    /// Refreshing writes a new token pair; one at a time.
    auth_lock: Mutex<()>,
}

pub use crate::commands::meeting::SlackStatus;

/// Which huddle a meeting was, and what was posted.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HuddleFile {
    pub channel_id: String,
    #[serde(default)]
    pub call_id: Option<String>,
    /// The huddle's message (the thread's root), found while the huddle
    /// ran: Slack deletes the message of a huddle nobody wrote in once it
    /// ends (seen 2026-11-24), so looking for it afterwards can fail.
    #[serde(default)]
    pub thread_ts: Option<String>,
    /// Everyone who was in the huddle at some point (Slack user ids, from
    /// the huddle message's `room.participant_history`, read once a minute
    /// while Sokki records).
    #[serde(default)]
    pub participants: Vec<String>,
    /// Their names and addresses, looked up when the recording ends, for
    /// the notes mail (meeting/mail.rs). The user is left out.
    #[serde(default)]
    pub people: Option<Vec<HuddlePerson>>,
    /// The thread the notes went into, once posted.
    #[serde(default)]
    pub posted_thread_ts: Option<String>,
    #[serde(default)]
    pub posted_file_id: Option<String>,
}

/// Someone else who was in the huddle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HuddlePerson {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

/// Sokki's Slack app, if this build has it. Its client id comes in at
/// build time (`SOKKI_SLACK_CLIENT_ID`, value in
/// `crates/sokki-slack/SLACK_APP.md`), like Gmail's: a build without it shows
/// no Slack row. Releases leave it out until the Slack app is publicly
/// distributed; until then only its own workspace could connect. In test
/// mode the same variable at run time stands in (scripts/test-meeting.sh).
fn client_id() -> Option<String> {
    std::env::var("SOKKI_SLACK_CLIENT_ID")
        .ok()
        .filter(|_| crate::test_mic::active())
        .or_else(|| option_env!("SOKKI_SLACK_CLIENT_ID").map(str::to_string))
        .filter(|id| !id.is_empty())
}

fn auth_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join(AUTH_FILE))
}

fn load_auth(app: &AppHandle) -> Option<Auth> {
    let text = std::fs::read_to_string(auth_path(app)?).ok()?;
    serde_json::from_str(&text)
        .inspect_err(|e| log::warn!("Slack: {AUTH_FILE} unreadable: {e}"))
        .ok()
}

fn save_auth(app: &AppHandle, auth: &Auth) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let path = auth_path(app).context("no app data folder")?;
    let tmp = path.with_extension("json.tmp");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(serde_json::to_string_pretty(auth)?.as_bytes())?;
    f.sync_all()?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// A working sign-in: refreshed first when the access token runs out.
fn fresh_auth(app: &AppHandle) -> Result<Auth> {
    let state = app.state::<Arc<SlackState>>();
    let _one = state.auth_lock.lock().unwrap_or_else(|e| e.into_inner());
    let auth = load_auth(app).context("not connected to Slack")?;
    if !auth.needs_refresh(oauth::now_unix()) {
        return Ok(auth);
    }
    let id = client_id().context("Slack isn't set up in this build")?;
    let new = oauth::refresh(&OAuthApp { client_id: id }, &auth)?;
    save_auth(app, &new)?;
    log::info!("Slack: sign-in refreshed");
    Ok(new)
}

pub fn status(app: &AppHandle) -> SlackStatus {
    let auth = load_auth(app);
    SlackStatus {
        available: client_id().is_some(),
        connected: auth.is_some(),
        team: auth.map(|a| a.team_name),
    }
}

struct Texts {
    page_done: &'static str,
    page_failed: &'static str,
    posted: &'static str,
    post_failed: &'static str,
    signed_out: &'static str,
}

fn texts(app_language: &str) -> Texts {
    if app_language.to_lowercase().starts_with("de") {
        Texts {
            page_done: "Sokki ist mit Slack verbunden. Du kannst diesen Tab schließen.",
            page_failed:
                "Die Verbindung mit Slack hat nicht geklappt. Versuch es in Sokki noch einmal.",
            posted: "Die Notizen stehen im Thread des Huddles.",
            post_failed: "Die Notizen konnten nicht in den Huddle-Thread: {error}",
            signed_out:
                "Sokki ist nicht mehr mit Slack verbunden. Verbinde es in den Einstellungen neu.",
        }
    } else {
        Texts {
            page_done: "Sokki is connected to Slack. You can close this tab.",
            page_failed: "Connecting to Slack didn't work. Try again in Sokki.",
            posted: "The notes are in the huddle's thread.",
            post_failed: "Couldn't post the notes into the huddle's thread: {error}",
            signed_out: "Sokki is no longer connected to Slack. Connect it again in Settings.",
        }
    }
}

/// Signs in with Slack in the browser. Blocking, up to five minutes.
pub fn connect(app: &AppHandle) -> Result<SlackStatus> {
    let id = client_id().context("Slack isn't set up in this build")?;
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let state = app.state::<Arc<SlackState>>();
        let mut running = state.signing_in.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(old) = running.replace(cancel.clone()) {
            // A second click: the new sign-in replaces the old one.
            old.store(true, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(250));
        }
    }
    let settings = crate::settings::get_settings(app);
    let t = texts(&settings.app_language);
    let page = oauth::CallbackPage {
        done: t.page_done.into(),
        failed: t.page_failed.into(),
    };
    let handle = app.clone();
    let result = oauth::sign_in(
        &OAuthApp { client_id: id },
        |url| {
            use tauri_plugin_opener::OpenerExt;
            handle
                .opener()
                .open_url(url, None::<&str>)
                .context("couldn't open the browser")
        },
        &page,
        SIGN_IN_TIMEOUT,
        &cancel,
    );
    {
        let state = app.state::<Arc<SlackState>>();
        let mut running = state.signing_in.lock().unwrap_or_else(|e| e.into_inner());
        if running.as_ref().is_some_and(|c| Arc::ptr_eq(c, &cancel)) {
            *running = None;
        }
    }
    let auth = result?;
    save_auth(app, &auth)?;
    log::info!("Slack: connected to {} as {}", auth.team_name, auth.user_id);
    let mut all = crate::settings::get_settings(app);
    if !all.meetings.slack_post_huddles {
        all.meetings.slack_post_huddles = true;
        crate::settings::write_settings(app, all);
    }
    Ok(status(app))
}

pub fn cancel_connect(app: &AppHandle) {
    let state = app.state::<Arc<SlackState>>();
    let running = state
        .signing_in
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    if let Some(c) = running {
        c.store(true, Ordering::Relaxed);
    }
}

/// Ends the sign-in, on Slack's side too.
pub fn disconnect(app: &AppHandle) -> SlackStatus {
    if let Some(auth) = load_auth(app) {
        if let Err(e) = Client::new(&auth.access_token).revoke() {
            log::info!("Slack: revoking the token failed ({e}); forgetting it anyway");
        }
    }
    if let Some(p) = auth_path(app) {
        let _ = std::fs::remove_file(p);
    }
    log::info!("Slack: disconnected");
    status(app)
}

/// At launch: refreshes a sign-in that hasn't been used for a week, so its
/// 30-day refresh token doesn't run out.
pub fn keep_alive(app: &AppHandle) {
    let Some(auth) = load_auth(app) else { return };
    if auth.refresh_token.is_none()
        || oauth::now_unix().saturating_sub(auth.refreshed_at) < KEEP_ALIVE_S
    {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let Some(id) = client_id() else { return };
        let state = app.state::<Arc<SlackState>>();
        let _one = state.auth_lock.lock().unwrap_or_else(|e| e.into_inner());
        match oauth::refresh(&OAuthApp { client_id: id }, &auth) {
            Ok(new) => match save_auth(&app, &new) {
                Ok(()) => log::info!("Slack: sign-in kept alive"),
                Err(e) => log::warn!("Slack: saving the refreshed sign-in failed: {e:#}"),
            },
            Err(e) => log::warn!("Slack: keeping the sign-in alive failed: {e:#}"),
        }
    });
}

// ------------------------------------------------------- during the huddle

/// A detected call started recording: for Slack, note which huddle it is
/// and who is in it. Whenever Slack is connected: the thread post and the
/// notes mail (meeting/mail.rs) both use it.
pub fn recording_started(app: &AppHandle, meeting_id: &str, call_app: &str) {
    if call_app != "slack" || client_id().is_none() || load_auth(app).is_none() {
        return;
    }
    // Test mode without a person in a huddle: `SOKKI_TEST_SLACK_THREAD` =
    // "<channel>/<thread_ts>" of a thread in a test channel stands in for
    // the huddle Sokki would have found (scripts/test-meeting.sh SLACK_THREAD).
    if let Some((channel, ts)) = crate::test_mic::active()
        .then(|| std::env::var("SOKKI_TEST_SLACK_THREAD").ok())
        .flatten()
        .and_then(|v| {
            v.split_once('/')
                .map(|(c, t)| (c.to_string(), t.to_string()))
        })
    {
        let dir = app.state::<Arc<MeetingStore>>().audio_dir(meeting_id);
        let file = HuddleFile {
            channel_id: channel,
            thread_ts: Some(ts),
            ..Default::default()
        };
        log::info!("Slack: test mode, meeting {meeting_id} uses thread {file:?}");
        if let Err(e) = write_huddle_file(&dir, &file) {
            log::warn!("Slack: saving the huddle failed: {e:#}");
        }
        return;
    }
    let (app, id) = (app.clone(), meeting_id.to_string());
    let spawned = std::thread::Builder::new()
        .name("slack-huddle".into())
        .spawn(move || watch_huddle(&app, &id));
    if let Err(e) = spawned {
        log::warn!("Slack: could not watch the huddle: {e}");
    }
}

fn watch_huddle(app: &AppHandle, meeting_id: &str) {
    let dir = app.state::<Arc<MeetingStore>>().audio_dir(meeting_id);
    let recording =
        || app.state::<Arc<MeetingManager>>().active_id().as_deref() == Some(meeting_id);
    let mut last: Option<HuddleFile> = None;
    let mut last_look: Option<std::time::Instant> = None;
    while recording() {
        let seen = fresh_auth(app).map_err(|e| e.to_string()).and_then(|a| {
            Client::new(&a.access_token)
                .huddle_state()
                .map_err(|e| e.to_string())
        });
        match seen {
            Ok(h) if h.in_huddle() => {
                // Slack's profile names the call but (in practice) not its
                // channel; the Slack app's log names the channel.
                let channel = h.channel_id.clone().or_else(|| {
                    std::env::var_os("HOME").and_then(|home| {
                        sokki_slack::desktop::current_huddle_channel(std::path::Path::new(&home))
                    })
                });
                if let Some(channel_id) = channel {
                    let same = last
                        .as_ref()
                        .is_some_and(|l| l.channel_id == channel_id && l.call_id == h.call_id);
                    let mut file = match &last {
                        Some(l) if same => l.clone(),
                        _ => {
                            last_look = None;
                            HuddleFile {
                                channel_id,
                                call_id: h.call_id,
                                ..Default::default()
                            }
                        }
                    };
                    // The huddle's message: right away, then once a minute
                    // (the limit outside the Marketplace) for the thread if
                    // it wasn't there yet and for people who joined since.
                    if last_look.is_none_or(|t| t.elapsed() >= Duration::from_secs(61)) {
                        last_look = Some(std::time::Instant::now());
                        if let Some(found) = find_huddle_now(app, &file) {
                            file.thread_ts.get_or_insert(found.thread_ts);
                            merge_participants(&mut file.participants, &found.participants);
                        }
                    }
                    if last.as_ref() != Some(&file) {
                        log::info!(
                            "Slack: meeting {meeting_id} is huddle {:?} in {}, thread {:?}, {} in it so far",
                            file.call_id,
                            file.channel_id,
                            file.thread_ts,
                            file.participants.len()
                        );
                        if let Err(e) = write_huddle_file(&dir, &file) {
                            log::warn!("Slack: saving the huddle failed: {e:#}");
                        }
                        last = Some(file);
                    }
                } else {
                    log::info!(
                        "Slack: in huddle {:?}, channel not found (profile, Slack's log)",
                        h.call_id
                    );
                }
            }
            Ok(h) => log::debug!("Slack: not in a huddle ({:?})", h.state),
            Err(e) => {
                log::warn!("Slack: huddle state unknown: {e}");
                if e.contains("not connected") || is_signed_out_text(&e) {
                    return;
                }
            }
        }
        // Sleep in steps, so the watch ends soon after the recording.
        for _ in 0..POLL_EVERY.as_secs() {
            if !recording() {
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    match last {
        None => log::info!("Slack: meeting {meeting_id}: no huddle seen in the profile"),
        Some(mut file) => {
            // Names and addresses now, so the notes mail has them when the
            // note is done (seconds later).
            match lookup_people(app, &file.participants) {
                Ok(people) => {
                    log::info!(
                        "Slack: meeting {meeting_id}: {} other people in the huddle, {} with an address",
                        people.len(),
                        people.iter().filter(|p| p.email.is_some()).count()
                    );
                    file.people = Some(people);
                    if let Err(e) = write_huddle_file(&dir, &file) {
                        log::warn!("Slack: saving the huddle failed: {e:#}");
                    }
                }
                Err(e) => log::warn!("Slack: looking up who was in the huddle failed: {e}"),
            }
        }
    }
}

/// Adds the ids in `new` that `all` doesn't have yet, keeping the order.
fn merge_participants(all: &mut Vec<String>, new: &[String]) {
    for id in new {
        if !all.contains(id) {
            all.push(id.clone());
        }
    }
}

/// Names and addresses of the huddle's participants, the user left out
/// (`users.info`, one call each, Tier 4). Bots and deactivated accounts
/// are dropped.
fn lookup_people(app: &AppHandle, participants: &[String]) -> Result<Vec<HuddlePerson>, String> {
    let auth = fresh_auth(app).map_err(|e| format!("{e:#}"))?;
    let client = Client::new(&auth.access_token);
    let mut people = Vec::new();
    for id in participants.iter().filter(|id| **id != auth.user_id) {
        let p = client.user(id).map_err(|e| e.to_string())?;
        if !p.not_a_person {
            people.push(HuddlePerson {
                id: p.id,
                name: p.name,
                email: p.email,
            });
        }
    }
    Ok(people)
}

/// The huddle as a calendar event for the notes mail: the other people in
/// it as attendees. `None` for a meeting that wasn't a recorded huddle.
/// `resolve`: look the people up at Slack if that didn't happen yet
/// (blocking; not for the preview).
pub fn huddle_event(
    app: &AppHandle,
    r: &super::store::MeetingRecord,
    resolve: bool,
) -> Option<meeting_mail::calendar::CalendarEvent> {
    let dir = app.state::<Arc<MeetingStore>>().audio_dir(&r.id);
    let mut file = read_huddle_file(&dir)?;
    if file.participants.is_empty() {
        return None;
    }
    if file.people.is_none() && resolve {
        match lookup_people(app, &file.participants) {
            Ok(people) => {
                file.people = Some(people);
                let _ = write_huddle_file(&dir, &file);
            }
            Err(e) => log::warn!("Slack: looking up who was in the huddle failed: {e}"),
        }
    }
    let start = chrono::DateTime::parse_from_rfc3339(&r.started_at)
        .map(|t| t.timestamp_millis() as f64 / 1000.0)
        .unwrap_or(0.0);
    Some(event_from_people(
        r.title.as_deref().unwrap_or("Slack-Huddle"),
        start,
        start + r.duration_s.max(0.0),
        file.people.as_deref().unwrap_or_default(),
    ))
}

fn event_from_people(
    title: &str,
    start: f64,
    end: f64,
    people: &[HuddlePerson],
) -> meeting_mail::calendar::CalendarEvent {
    use meeting_mail::calendar::{Attendee, CalendarEvent, Kind, Status};
    CalendarEvent {
        title: title.to_string(),
        start,
        end,
        all_day: false,
        url: None,
        location: None,
        notes: None,
        organizer: None,
        attendees: people
            .iter()
            .map(|p| Attendee {
                name: p.name.clone(),
                email: p.email.clone(),
                is_me: false,
                status: Status::Accepted,
                kind: Kind::Person,
            })
            .collect(),
        account: None,
    }
}

/// The huddle's message while the huddle runs: one `conversations.history`
/// call (more only if the channel is busy).
fn find_huddle_now(app: &AppHandle, file: &HuddleFile) -> Option<huddle::Huddle> {
    let auth = fresh_auth(app).ok()?;
    let now = oauth::now_unix();
    match huddle::locate(
        &Client::new(&auth.access_token),
        &file.channel_id,
        file.call_id.as_deref(),
        (now, now),
        &std::thread::sleep,
    ) {
        Ok(found) => found,
        Err(e) => {
            log::warn!("Slack: looking for the huddle's message failed: {e}");
            None
        }
    }
}

fn is_signed_out_text(e: &str) -> bool {
    [
        "invalid_auth",
        "not_authed",
        "token_revoked",
        "account_inactive",
        "token_expired",
        "invalid_refresh_token",
    ]
    .iter()
    .any(|c| e.contains(c))
}

fn read_huddle_file(dir: &std::path::Path) -> Option<HuddleFile> {
    serde_json::from_str(&std::fs::read_to_string(dir.join(HUDDLE_FILE)).ok()?).ok()
}

fn write_huddle_file(dir: &std::path::Path, file: &HuddleFile) -> Result<()> {
    std::fs::write(dir.join(HUDDLE_FILE), serde_json::to_string_pretty(file)?)?;
    Ok(())
}

// ------------------------------------------------------------ after it

/// The meeting's note is written or got its summary: post it into the
/// huddle's thread, if this was a huddle and it isn't posted yet.
pub fn note_ready(app: &AppHandle, meeting_id: &str) {
    if !crate::settings::get_settings(app)
        .meetings
        .slack_post_huddles
    {
        return;
    }
    let store = app.state::<Arc<MeetingStore>>();
    let dir = store.audio_dir(meeting_id);
    let Some(file) = read_huddle_file(&dir) else {
        return;
    };
    if file.posted_thread_ts.is_some() {
        return;
    }
    let Some(record) = store.get(meeting_id) else {
        return;
    };
    if record.status != MeetingStatus::Done || record.summary_missing.is_some() {
        log::info!("Slack: meeting {meeting_id}: posting waits for the summary");
        return;
    }
    let (app, id) = (app.clone(), meeting_id.to_string());
    std::thread::spawn(move || {
        let language = crate::settings::get_settings(&app).app_language;
        let t = texts(&language);
        match post(&app, &id, file) {
            Ok(()) => banner(t.posted),
            Err(e) => {
                log::warn!("Slack: meeting {id}: posting failed: {e:#}");
                let text = format!("{e:#}");
                if is_signed_out_text(&text) {
                    banner(t.signed_out);
                } else {
                    banner(&t.post_failed.replace("{error}", &text));
                }
            }
        }
    });
}

/// One post at a time, so a summary retry can't post twice.
static POSTING: Mutex<()> = Mutex::new(());

fn post(app: &AppHandle, meeting_id: &str, file: HuddleFile) -> Result<()> {
    let _one = POSTING.lock().unwrap_or_else(|e| e.into_inner());
    let store = app.state::<Arc<MeetingStore>>();
    let dir = store.audio_dir(meeting_id);
    if read_huddle_file(&dir).is_some_and(|f| f.posted_thread_ts.is_some()) {
        return Ok(());
    }
    let record = store.get(meeting_id).context("meeting gone")?;
    let note_path = record
        .note_path
        .as_deref()
        .context("the meeting has no note")?;
    let md = std::fs::read_to_string(note_path).with_context(|| format!("reading {note_path}"))?;
    let parts = meeting_core::note::split_note(&md);
    if parts.transcript.trim().is_empty() {
        bail!("the note has no transcript");
    }
    let german = md.contains("\n## Transkript");
    let language = if german {
        meeting_core::lang::Language::German
    } else {
        meeting_core::lang::Language::English
    };
    let engine = super::process::engine_settings(&crate::settings::get_settings(app));
    let line = engine.sokki_line_text(language);
    let summary = without_sokki_line(&parts.summary);
    let title = record.title.clone().unwrap_or_default();
    let post = mrkdwn::compose(&title, summary, line, &parts.transcript, german);

    let auth = fresh_auth(app)?;
    let client = Client::new(&auth.access_token);
    let started = chrono::DateTime::parse_from_rfc3339(&record.started_at)
        .map(|t| t.timestamp().max(0) as u64)
        .unwrap_or_else(|_| oauth::now_unix());
    let recorded = (started, started + record.duration_s.max(0.0) as u64);
    let (channel, thread_ts) = match &file.thread_ts {
        Some(ts) => (file.channel_id.clone(), ts.clone()),
        None => {
            let found = huddle::locate(
                &client,
                &file.channel_id,
                file.call_id.as_deref(),
                recorded,
                &std::thread::sleep,
            )?
            .context("the huddle's message wasn't found in the channel")?;
            (found.channel, found.thread_ts)
        }
    };
    // A thread whose message is gone would put the notes into the channel.
    let exists = loop {
        match client.message_exists(&channel, &thread_ts) {
            Err(sokki_slack::Error::RateLimited(s)) => {
                std::thread::sleep(Duration::from_secs(s.max(1)))
            }
            other => break other?,
        }
    };
    if !exists {
        bail!("the huddle's message is gone from Slack, so there is no thread to post into");
    }
    log::info!("Slack: meeting {meeting_id}: posting into thread {thread_ts} in {channel}");
    let file_id = client.upload_to_thread(
        &channel,
        &thread_ts,
        &post.comment,
        &post.filename,
        &post.file_title,
        post.file_content.as_bytes(),
    )?;
    write_huddle_file(
        &dir,
        &HuddleFile {
            posted_thread_ts: Some(thread_ts.clone()),
            posted_file_id: Some(file_id),
            ..file
        },
    )?;
    log::info!("Slack: meeting {meeting_id}: notes posted into the huddle's thread");
    Ok(())
}

/// The summary as `split_note` gives it, without the note's own Sokki line
/// (the post adds the current one).
fn without_sokki_line(summary: &str) -> &str {
    let s = summary.trim_end();
    match s.rsplit_once("\n\n") {
        Some((head, last)) if meeting_core::note::is_footer(last) => head.trim_end(),
        _ => s,
    }
}

/// A banner for the person; in test mode only logged.
fn banner(body: &str) {
    if crate::test_mic::active() {
        log::info!("Slack: test mode, banner not shown: {body}");
    } else {
        super::detect::notify(crate::brand::app_name(), body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notes_own_sokki_line_is_dropped() {
        let line = meeting_core::note::sokki_line(meeting_core::lang::Language::German, false);
        let s = format!("## Zusammenfassung\n\n- Punkt\n\n{line}");
        assert_eq!(without_sokki_line(&s), "## Zusammenfassung\n\n- Punkt");
        assert_eq!(
            without_sokki_line("## Summary\n\n- Done."),
            "## Summary\n\n- Done."
        );
    }

    #[test]
    fn the_huddle_file_reads_back() {
        let dir = std::env::temp_dir().join(format!("sokki-slack-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = HuddleFile {
            channel_id: "C1".into(),
            call_id: Some("R1".into()),
            ..Default::default()
        };
        write_huddle_file(&dir, &f).unwrap();
        assert_eq!(read_huddle_file(&dir), Some(f));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn people_who_join_later_are_added_once() {
        let mut all = vec!["U1".to_string()];
        merge_participants(&mut all, &["U1".into(), "U2".into()]);
        merge_participants(&mut all, &["U3".into(), "U2".into()]);
        assert_eq!(all, ["U1", "U2", "U3"]);
    }

    #[test]
    fn a_huddle_mails_everyone_else_who_was_in_it() {
        let person = |id: &str, name: &str, email: Option<&str>| HuddlePerson {
            id: id.into(),
            name: Some(name.into()),
            email: email.map(str::to_string),
        };
        let event = event_from_people(
            "Planung",
            100.0,
            200.0,
            &[
                person("U2", "Lea", Some("lea@example.com")),
                person("U3", "Jonas", Some("Jonas@Example.com")),
                person("U4", "Kim", None),
                person("U5", "Me again", Some("me@example.com")),
            ],
        );
        assert_eq!(
            (event.title.as_str(), event.start, event.end),
            ("Planung", 100.0, 200.0)
        );
        let dir = std::env::temp_dir().join(format!("sokki-slack-obj-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut objections =
            meeting_mail::objections::Objections::load(&dir.join("objections.json"));
        objections
            .add(
                "lea@example.com",
                "2026-11-25",
                meeting_mail::objections::Source::Reply,
            )
            .unwrap();
        let list = meeting_mail::recipients::recipients(
            &event,
            &["me@example.com".to_string()],
            &objections,
        );
        let to: Vec<&str> = list.to.iter().map(|r| r.email.as_str()).collect();
        assert_eq!(to, ["jonas@example.com"], "{list:?}");
        assert_eq!(list.skipped.len(), 3, "objected, no address, me: {list:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn texts_follow_the_app_language() {
        assert!(texts("de-DE").posted.starts_with("Die Notizen"));
        assert!(texts("en").posted.starts_with("The notes"));
        assert!(is_signed_out_text("Slack: invalid_auth"));
        assert!(!is_signed_out_text("Slack: channel_not_found"));
    }
}

//! Sends a meeting's notes to everyone who was invited, from the person's
//! own mailbox and in their name (board card `notes-to-all`): their Gmail
//! when connected (`meeting/gmail_account.rs`), else their Apple Mail.
//!
//! - **Who:** the calendar event the recording belongs to (by time and
//!   meeting link, `meeting_mail::calendar::find_event`), its attendees
//!   minus the person, people who declined, rooms, and the objection list.
//! - **What:** the note's summary, decisions and action items, no
//!   transcript, ending with the note's Sokki line when the note has one,
//!   and how to say no ("reply stop").
//! - **When:** the note's "Send to everyone" button, or by itself after
//!   each meeting when `meetings.mail_notes_auto` is on (off by default).
//!   By itself only for a meeting from the last day, once.
//! - **Objections:** before each send through Apple Mail, Sokki reads
//!   Mail's inbox for "stop" replies to these mails and adds the senders to
//!   the list (Gmail's `gmail.send` can't read; there the person adds them)
//!   (`mail_objections.json` in the app's data folder); nobody on it gets
//!   mail again.
//!
//! Test mode: nothing is sent unless `SOKKI_TEST_MAIL_TO=<address>` is set,
//! and then every mail goes to that address only, never to the attendees.
//! `SOKKI_TEST_CALENDAR` (meeting/calendar.rs) supplies the event.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use meeting_mail::apple_mail::AppleMail;
use meeting_mail::calendar::CalendarEvent;
use meeting_mail::objections::{Objections, Source};
use meeting_mail::recipients::{recipients, Skipped};
use meeting_mail::{compose, Mail, MailSender, Recipient, SendError};
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager};

use super::calendar::{self, CalendarAccess};
use super::store::{Mailed, MeetingRecord, MeetingStatus, MeetingStore};

const OBJECTIONS_FILE: &str = "mail_objections.json";

/// The automatic send only goes out for a meeting this recent, so turning
/// the switch on, or a summary added late, never mails about old meetings.
const AUTO_WITHIN: Duration = Duration::from_secs(24 * 3600);

/// How far back Mail's inbox is read for "stop" replies.
const STOP_REPLIES_SINCE: Duration = Duration::from_secs(90 * 24 * 3600);

/// How far around the recording calendar events are looked up.
const EVENT_MARGIN_S: f64 = 3.0 * 3600.0;

/// Meetings being sent right now, so a double click or the button during
/// the automatic send never mails twice.
static SENDING: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Why the notes can't go out (yet).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MailProblem {
    /// The note has no summary yet.
    NoSummary,
    /// Sokki has not asked for the calendar yet; sending asks.
    CalendarNotAsked,
    /// The calendar was declined (System Settings › Privacy & Security ›
    /// Calendars).
    CalendarDenied,
    /// No calendar event at the time of the recording.
    NoEvent,
    /// The event has nobody to send to (only the person, or all objected).
    NoRecipients,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct MailRecipient {
    pub name: Option<String>,
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct MailSkipped {
    /// The address, or the name when there is none.
    pub who: String,
    /// "me", "declined", "objected", "not_a_person" or "no_address".
    pub why: String,
}

/// What "Send to everyone" would do for one meeting. Reading it never
/// opens Mail or asks for anything.
#[derive(Debug, Clone, Serialize, Type)]
pub struct MailPreview {
    /// The calendar event's title.
    pub event: Option<String>,
    pub to: Vec<MailRecipient>,
    pub skipped: Vec<MailSkipped>,
    pub subject: Option<String>,
    pub body: Option<String>,
    pub problem: Option<MailProblem>,
    /// Sent already.
    pub mailed: Option<Mailed>,
    /// Being sent right now.
    pub sending: bool,
}

struct Plan {
    event: Option<CalendarEvent>,
    composed: Option<compose::Composed>,
    problem: Option<MailProblem>,
}

fn data_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok()
}

pub fn objections(app: &AppHandle) -> Objections {
    match data_dir(app) {
        Some(d) => Objections::load(&d.join(OBJECTIONS_FILE)),
        None => Objections::default(),
    }
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn test_mail_to() -> Option<String> {
    std::env::var("SOKKI_TEST_MAIL_TO")
        .ok()
        .and_then(|a| meeting_mail::normalize_email(&a))
}

/// Start and end of a recording, unix seconds.
fn span(r: &MeetingRecord) -> Option<(f64, f64)> {
    let start = chrono::DateTime::parse_from_rfc3339(&r.started_at).ok()?;
    let start = start.timestamp_millis() as f64 / 1000.0;
    Some((start, start + r.duration_s.max(0.0)))
}

/// The meeting's calendar event, by time and meeting link. Needs access.
fn find_event(r: &MeetingRecord) -> Option<CalendarEvent> {
    let (start, end) = span(r)?;
    let events = calendar::events(start - EVENT_MARGIN_S, end + EVENT_MARGIN_S);
    meeting_mail::calendar::find_event(&events, start, end, r.source_app.as_deref()).cloned()
}

fn plan(r: &MeetingRecord) -> Plan {
    let composed = r
        .note_path
        .as_deref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|md| compose::summary_mail(&md, r.title.as_deref().unwrap_or("Meeting")));
    let mut plan = Plan {
        event: None,
        composed,
        problem: None,
    };
    if plan.composed.is_none() || r.status != MeetingStatus::Done {
        plan.problem = Some(MailProblem::NoSummary);
        return plan;
    }
    // The sample meeting has invented people and no event: never look one
    // up, so its notes can't reach anyone.
    if r.sample {
        plan.problem = Some(MailProblem::NoEvent);
        return plan;
    }
    match calendar::access() {
        CalendarAccess::Allowed => {}
        CalendarAccess::NotDetermined => {
            plan.problem = Some(MailProblem::CalendarNotAsked);
            return plan;
        }
        _ => {
            plan.problem = Some(MailProblem::CalendarDenied);
            return plan;
        }
    }
    plan.event = find_event(r);
    if plan.event.is_none() {
        plan.problem = Some(MailProblem::NoEvent);
    }
    plan
}

fn skipped_name(s: Skipped) -> &'static str {
    match s {
        Skipped::Me => "me",
        Skipped::Declined => "declined",
        Skipped::Objected => "objected",
        Skipped::NotAPerson => "not_a_person",
        Skipped::NoAddress => "no_address",
    }
}

fn is_sending(id: &str) -> bool {
    SENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|s| s.contains(id))
}

/// What "Send to everyone" would do. Never opens Mail.
pub fn preview(app: &AppHandle, id: &str) -> Result<MailPreview, String> {
    let store = app.state::<Arc<MeetingStore>>();
    let r = store.get(id).ok_or("Meeting not found")?;
    let p = plan(&r);
    let mut preview = MailPreview {
        event: p.event.as_ref().map(|e| e.title.clone()),
        to: Vec::new(),
        skipped: Vec::new(),
        subject: p.composed.as_ref().map(|c| c.subject.clone()),
        body: p.composed.as_ref().map(|c| c.body.clone()),
        problem: p.problem,
        mailed: r.mailed.clone(),
        sending: is_sending(id),
    };
    if let Some(event) = &p.event {
        let list = recipients(event, &[], &objections(app));
        preview.to = list
            .to
            .into_iter()
            .map(|r| MailRecipient {
                name: r.name,
                email: r.email,
            })
            .collect();
        preview.skipped = list
            .skipped
            .into_iter()
            .map(|(who, why)| MailSkipped {
                who,
                why: skipped_name(why).into(),
            })
            .collect();
        if preview.to.is_empty() {
            preview.problem = Some(MailProblem::NoRecipients);
        }
    }
    Ok(preview)
}

/// Marks `id` as being sent; false when it already is.
struct SendingGuard(String);

impl SendingGuard {
    fn take(id: &str) -> Option<Self> {
        let mut s = SENDING.lock().unwrap_or_else(|e| e.into_inner());
        let set = s.get_or_insert_with(HashSet::new);
        set.insert(id.to_string()).then(|| Self(id.to_string()))
    }
}

impl Drop for SendingGuard {
    fn drop(&mut self) {
        if let Some(set) = SENDING.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            set.remove(&self.0);
        }
    }
}

/// One line for the person, in the app's language.
fn say(app: &AppHandle, de: &str, en: &str) -> String {
    let lang = crate::settings::get_settings(app).app_language;
    if lang.to_lowercase().starts_with("de") {
        de
    } else {
        en
    }
    .to_string()
}

fn send_error_text(app: &AppHandle, e: &SendError) -> String {
    match e {
        SendError::NoAccount => say(
            app,
            "In Mail ist kein Konto eingerichtet.",
            "Mail has no account set up.",
        ),
        SendError::NotAllowed => say(
            app,
            "Sokki darf Mail nicht steuern. Erlaube es unter Systemeinstellungen › Datenschutz & Sicherheit › Automation.",
            "Sokki may not control Mail. Allow it in System Settings › Privacy & Security › Automation.",
        ),
        SendError::Timeout => say(app, "Mail hat nicht geantwortet.", "Mail did not answer."),
        SendError::Failed(m) => format!("{} {m}", say(app, "Mail konnte nicht senden:", "Mail could not send:")),
        SendError::Unsupported => say(app, "Senden geht hier nicht.", "Sending is not available here."),
        SendError::SignIn => say(
            app,
            "Gmail ist nicht mehr verbunden. Verbinde es in den Einstellungen neu.",
            "Gmail is no longer connected. Connect it again in Settings.",
        ),
    }
}

fn problem_text(app: &AppHandle, p: MailProblem) -> String {
    match p {
        MailProblem::NoSummary => say(app, "Die Notiz hat noch keine Zusammenfassung.", "The note has no summary yet."),
        MailProblem::CalendarNotAsked | MailProblem::CalendarDenied => say(
            app,
            "Sokki darf den Kalender nicht lesen. Erlaube es unter Systemeinstellungen › Datenschutz & Sicherheit › Kalender.",
            "Sokki may not read the calendar. Allow it in System Settings › Privacy & Security › Calendars.",
        ),
        MailProblem::NoEvent => say(app, "Kein Kalendertermin zu diesem Meeting.", "No calendar event for this meeting."),
        MailProblem::NoRecipients => say(app, "Niemand zum Anschreiben im Termin.", "Nobody in the event to send to."),
    }
}

/// Sends the notes of meeting `id` to its attendees. Blocks (Mail, the
/// calendar prompt): call it off the main thread. `auto`: the automatic
/// setting, which never asks for anything and never sends twice.
pub fn send(app: &AppHandle, id: &str, auto: bool) -> Result<Mailed, String> {
    let started = Instant::now();
    let test = crate::test_mic::active() || std::env::var_os("SOKKI_TEST_CALENDAR").is_some();
    let test_to = test_mail_to();
    if test && test_to.is_none() {
        return Err("Test mode: set SOKKI_TEST_MAIL_TO to send".into());
    }
    let _guard = SendingGuard::take(id)
        .ok_or_else(|| say(app, "Wird schon gesendet.", "Already sending."))?;
    let store = app.state::<Arc<MeetingStore>>().inner().clone();
    let r = store.get(id).ok_or("Meeting not found")?;
    if let Some(m) = &r.mailed {
        if auto {
            return Ok(m.clone());
        }
    }
    if !auto && calendar::access() == CalendarAccess::NotDetermined {
        // The person pressed the button: this is the moment to ask.
        calendar::request();
    }
    let p = plan(&r);
    if let Some(problem) = p.problem {
        return Err(problem_text(app, problem));
    }
    let (Some(event), Some(composed)) = (p.event, p.composed) else {
        return Err(problem_text(app, MailProblem::NoEvent));
    };

    let mut objections = objections(app);
    // Gmail when it is connected, else Apple Mail.
    let gmail = super::gmail_account::sender(app);
    let (own, from, via): (Vec<String>, Option<String>, &str) = match &gmail {
        Some(g) => (g.own_addresses().unwrap_or_default(), None, "Gmail"),
        None => {
            let accounts = AppleMail.accounts().map_err(|e| send_error_text(app, &e))?;
            if accounts.is_empty() {
                return Err(send_error_text(app, &SendError::NoAccount));
            }
            // Apple Mail can read "stop" replies; Gmail (gmail.send) can't.
            match AppleMail.stop_replies(STOP_REPLIES_SINCE) {
                Ok(senders) => {
                    for s in senders {
                        if let Ok(true) = objections.add(&s, &today(), Source::Reply) {
                            log::info!(
                                "Mail: a stop reply put one more address on the objection list"
                            );
                        }
                    }
                }
                Err(e) => log::warn!("Mail: reading stop replies failed: {e}"),
            }
            // Send from the account the calendar event belongs to, when Mail has it.
            let from = event
                .account
                .as_deref()
                .and_then(meeting_mail::normalize_email)
                .and_then(|e| accounts.iter().find(|a| a.email == e))
                .map(|a| {
                    if a.full_name.is_empty() {
                        a.email.clone()
                    } else {
                        format!("{} <{}>", a.full_name, a.email)
                    }
                });
            (
                accounts.into_iter().map(|a| a.email).collect(),
                from,
                "Apple Mail",
            )
        }
    };
    let list = recipients(&event, &own, &objections);
    if list.to.is_empty() {
        return Err(problem_text(app, MailProblem::NoRecipients));
    }
    let to: Vec<Recipient> = match &test_to {
        Some(addr) => vec![Recipient {
            name: None,
            email: addr.clone(),
        }],
        None => list.to.clone(),
    };
    let mail = Mail {
        from,
        to,
        subject: composed.subject,
        body: composed.body,
    };
    match &gmail {
        Some(g) => g.send(&mail),
        None => AppleMail.send(&mail),
    }
    .map_err(|e| send_error_text(app, &e))?;

    let mailed = Mailed {
        at: chrono::Local::now().to_rfc3339(),
        to: mail.to.iter().map(|r| r.email.clone()).collect(),
        auto,
    };
    let _ = store.update(id, |r| r.mailed = Some(mailed.clone()));
    super::emit_changed(app, id);
    // Counts and reasons only: attendees' addresses stay out of the log.
    log::info!(
        "Meeting {id}: notes mailed through {via} to {} address(es){} in {:.1} s ({} in the event, left out: {})",
        mailed.to.len(),
        if test_to.is_some() { ", the test address" } else { "" },
        started.elapsed().as_secs_f32(),
        list.to.len(),
        list.skipped
            .iter()
            .map(|(_, why)| skipped_name(*why))
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(mailed)
}

/// After a meeting's note is done (or its summary added): sends the notes
/// by itself when the setting is on, once, for a recent meeting. Runs in
/// the background; a failure becomes a warning on the meeting's card.
pub fn after_note(app: &AppHandle, id: &str) {
    let settings = crate::settings::get_settings(app);
    if !settings.meetings.mail_notes_auto {
        return;
    }
    let store = app.state::<Arc<MeetingStore>>().inner().clone();
    let Some(r) = store.get(id) else { return };
    if r.sample
        || r.mailed.is_some()
        || r.summary_missing.is_some()
        || r.status != MeetingStatus::Done
    {
        return;
    }
    let recent = span(&r).is_some_and(|(_, end)| {
        let now = chrono::Utc::now().timestamp() as f64;
        now - end < AUTO_WITHIN.as_secs_f64()
    });
    if !recent {
        return;
    }
    let app = app.clone();
    let id = id.to_string();
    let _ = std::thread::Builder::new()
        .name(format!("mail-{id}"))
        .spawn(move || match send(&app, &id, true) {
            Ok(m) => {
                let names = m.to.join(", ");
                super::detect::notify(
                    &say(&app, "Notizen gesendet", "Notes sent"),
                    &format!("{} {names}", say(&app, "An", "To")),
                );
            }
            Err(e) => {
                log::warn!("Meeting {id}: notes not mailed: {e}");
                let line = format!(
                    "{} {e}",
                    say(&app, "Notizen nicht gesendet:", "Notes not sent:")
                );
                let _ = app.state::<Arc<MeetingStore>>().update(&id, |r| {
                    if !r.warnings.contains(&line) {
                        r.warnings.push(line.clone());
                    }
                });
                super::emit_changed(&app, &id);
            }
        });
}

/// Where sending stands, for Settings: calendar access and Mail's accounts.
#[derive(Debug, Clone, Serialize, Type)]
pub struct MailSetup {
    pub calendar: CalendarAccess,
    /// Mail's sending addresses.
    pub accounts: Vec<String>,
    /// Why Mail can't send, in the app's language.
    pub error: Option<String>,
}

/// Asks for what sending needs (the calendar prompt, then Mail's
/// Automation prompt by reading its accounts) and says where it stands.
/// Only after the person's own action: turning the switch on.
pub fn setup(app: &AppHandle) -> MailSetup {
    let calendar = match calendar::access() {
        CalendarAccess::NotDetermined => calendar::request(),
        a => a,
    };
    if let Some(address) = super::gmail_account::connected(app) {
        return MailSetup {
            calendar,
            accounts: vec![address],
            error: None,
        };
    }
    let (accounts, error) = match AppleMail.accounts() {
        Ok(a) if a.is_empty() => (
            Vec::new(),
            Some(send_error_text(app, &SendError::NoAccount)),
        ),
        Ok(a) => (a.into_iter().map(|a| a.email).collect(), None),
        Err(e) => (Vec::new(), Some(send_error_text(app, &e))),
    };
    MailSetup {
        calendar,
        accounts,
        error,
    }
}

pub fn add_objection(app: &AppHandle, email: &str) -> Result<bool, String> {
    objections(app)
        .add(email, &today(), Source::ByHand)
        .map_err(|e| e.to_string())
}

pub fn remove_objection(app: &AppHandle, email: &str) -> Result<bool, String> {
    objections(app).remove(email).map_err(|e| e.to_string())
}

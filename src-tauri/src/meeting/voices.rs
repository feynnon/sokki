//! Speakers with real names: the "Sprecher" page and the names Sokki
//! proposes in a meeting (board card `speaker-names`).
//!
//! - Voices across meetings: `sokki_engine::voices::group`, computed when
//!   asked, never stored.
//! - People: `voice-profiles.json` (`diarize::VoiceProfiles`): a name, the
//!   person's email addresses, and their voice only after the user ticked
//!   that they agree.
//! - Who was invited: the calendar event of the meeting, cached as
//!   `invitees.json` next to its audio (only while the calendar is
//!   allowed; Sokki never asks for it here).
//!
//! Naming a speaker renames it in the note (transcript, summary, tasks),
//! `speakers.json` and the meeting list. When that leaves exactly one
//! unnamed speaker and one invitee, Sokki names that one too.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use meeting_core::note::transcript_from_note;
use sokki_engine::speakers::{is_placeholder, SavedSpeaker};
use sokki_engine::voices::{self, Invitee, MeetingSpeakers, Person, Proposal};
use tauri::{AppHandle, Manager};

use super::store::{MeetingRecord, MeetingStatus, MeetingStore};
use super::{calendar, process};

pub use super::voice_types::*;

pub const INVITEES_FILE: &str = "invitees.json";

/// Same voice, at least this cosine similarity (the voice-profile threshold).
const SAME_VOICE: f32 = diarize::VoiceProfiles::DEFAULT_THRESHOLD;

/// Voices unused this long are deleted (BIPA/CUBI retention; see
/// side-session notes `speaker-names.md`).
const FORGET_AFTER_DAYS: i64 = 365;

fn store(app: &AppHandle) -> Arc<MeetingStore> {
    app.state::<Arc<MeetingStore>>().inner().clone()
}

fn user_name(app: &AppHandle) -> String {
    crate::settings::get_settings(app).meetings.user_name
}

/// The name Sokki uses for the person called `name`: their own name for an
/// old name or another spelling ("mara" → "Mara Lind"), else `name`.
pub fn person_name(app: &AppHandle, name: &str) -> String {
    match process::voice_profiles(app) {
        Some(p) => p.canonical(name),
        None => name.trim().to_string(),
    }
}

/// Every name of the person using Sokki: theirs and the old ones.
fn user_names(app: &AppHandle) -> Vec<String> {
    let user = user_name(app);
    let mut out = vec![user.clone()];
    if let Some(p) = process::voice_profiles(app)
        .as_ref()
        .and_then(|p| p.person(&user).cloned())
    {
        out.push(p.name);
        out.extend(p.aliases);
    }
    out
}

/// A name compared without case and extra spaces.
fn norm(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// `to` is another way to write `from`: another case, or the full name for
/// a first name ("Mara" → "Mara Lind") and back. Robin, 27 Nov 2026: one
/// person showed under a first name in some meetings, a full name in others.
fn same_name(from: &str, to: &str) -> bool {
    let (a, b) = (norm(from), norm(to));
    !a.is_empty()
        && !b.is_empty()
        && (a == b || b.starts_with(&format!("{a} ")) || a.starts_with(&format!("{b} ")))
}

/// The person called `from` is now called `to`, everywhere: their profile
/// (merged when `to` is known too), every meeting where a speaker has one
/// of their names, and the user's own name when it's them. Returns the
/// meetings that changed.
fn rename_person(
    app: &AppHandle,
    store: &MeetingStore,
    from: &str,
    to: &str,
    backup: Backup,
) -> Result<Vec<String>> {
    let mut profiles = process::voice_profiles(app).context("Voice profiles unavailable")?;
    // Every name of both records, before anything changes.
    let mut names: Vec<String> = vec![norm(from)];
    for n in [from, to] {
        if let Some(p) = profiles.person(n) {
            names.push(norm(&p.name));
            names.extend(p.aliases.iter().map(|a| norm(a)));
        }
    }
    let is_them = |name: &str| norm(name) != norm(to) && names.contains(&norm(name));
    // What each meeting renames. The meeting list and speakers.json can
    // disagree (a meeting of 26 Nov 2026 had a name only in speakers.json
    // and the note), so both are read.
    let mut plan: Vec<(MeetingRecord, Vec<String>)> = Vec::new();
    for r in store.list().into_iter().filter(processed) {
        let mut olds: Vec<String> = r
            .speakers
            .iter()
            .cloned()
            .chain(
                process::load_speakers(&store.audio_dir(&r.id))
                    .into_iter()
                    .map(|s| s.name),
            )
            .filter(|s| is_them(s))
            .collect();
        olds.sort();
        olds.dedup();
        if !olds.is_empty() {
            plan.push((r, olds));
        }
    }
    let mut settings = crate::settings::get_settings(app);
    let user_changes = is_them(&settings.meetings.user_name);
    // A copy of everything this touches first (release manager, 27 Nov:
    // the join rewrites real notes in 40 meetings).
    if backup == Backup::Always || plan.len() > 1 {
        let mut files = vec![profiles.path().to_path_buf()];
        if let Some(root) = plan.first().map(|(r, _)| store.audio_dir(&r.id)) {
            if let Some(meetings) = root.parent() {
                files.push(meetings.join("index.json"));
            }
        }
        if user_changes {
            if let Ok(dir) = app.path().app_data_dir() {
                files.push(dir.join("settings_store.json"));
            }
        }
        for (r, _) in &plan {
            files.push(store.audio_dir(&r.id).join(process::SPEAKERS_FILE));
            if let Some(note) = r.note_path.as_deref() {
                files.push(std::path::PathBuf::from(note));
            }
        }
        let dir = app
            .path()
            .app_data_dir()
            .context("No app data folder")?
            .join("backups")
            .join(format!(
                "{}-{}",
                backup.prefix(),
                chrono::Local::now().format("%Y%m%d-%H%M%S-%3f")
            ));
        backup_files(&dir, &files)?;
        log::info!(
            "Speakers: {} file(s) backed up to {} before renaming a person in {} meeting(s)",
            files.len(),
            dir.display(),
            plan.len()
        );
    }
    profiles.rename_person(from, to)?;
    if user_changes {
        settings.meetings.user_name = to.to_string();
        crate::settings::write_settings(app, settings);
        log::info!("Speakers: the user's own name follows their person");
    }
    let mut changed = Vec::new();
    for (r, olds) in &plan {
        for old in olds {
            rename_in(app, store, &r.id, old, to)?;
        }
        changed.push(r.id.clone());
    }
    Ok(changed)
}

/// When [`rename_person`] copies the files it changes first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backup {
    /// Always (the join at start).
    Always,
    /// When more than one meeting changes (a rename in the card).
    SeveralMeetings,
}

impl Backup {
    fn prefix(self) -> &'static str {
        match self {
            Backup::Always => "speaker-join",
            Backup::SeveralMeetings => "speaker-rename",
        }
    }
}

/// Copies `files` that exist into `dir` (numbered, so equal names don't
/// clash) and writes `manifest.json` there: each original path with its
/// copy, so a restore puts every byte back where it was.
pub fn backup_files(dir: &Path, files: &[std::path::PathBuf]) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("Can't create {}", dir.display()))?;
    let mut manifest = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (i, f) in files.iter().enumerate() {
        if !f.is_file() || !seen.insert(f.clone()) {
            continue;
        }
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let copy = format!("{i:04}-{name}");
        std::fs::copy(f, dir.join(&copy))
            .with_context(|| format!("Backing up {} failed", f.display()))?;
        manifest.push(serde_json::json!({ "original": f, "copy": copy }));
    }
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )
    .with_context(|| format!("Can't write {}", dir.display()))?;
    Ok(())
}

/// Joins the records of one person that came apart before Sokki knew old
/// names (27 Nov 2026: three people each as a first name with the voice
/// and as a full name with the address). A first name joins the one full
/// name that starts with it when both share an address, the first name has
/// none, or it is the user's. Runs at start; does nothing once joined.
pub fn join_name_variants(app: &AppHandle) {
    let Some(profiles) = process::voice_profiles(app) else {
        return;
    };
    let user = norm(&user_name(app));
    let people = profiles.profiles().to_vec();
    let mut pairs = Vec::new();
    for short in &people {
        let longer: Vec<_> = people
            .iter()
            .filter(|p| {
                p.name != short.name
                    && norm(&p.name).starts_with(&format!("{} ", norm(&short.name)))
            })
            .collect();
        let [long] = longer.as_slice() else { continue };
        let shared = short.emails.iter().any(|e| long.emails.contains(e));
        if shared || short.emails.is_empty() || norm(&short.name) == user {
            pairs.push((short.name.clone(), long.name.clone()));
        }
    }
    let store = store(app);
    for (short, long) in &pairs {
        match rename_person(app, &store, short, long, Backup::Always) {
            Ok(ids) => log::info!(
                "Speakers: joined two records of one person ({} meeting(s) renamed)",
                ids.len()
            ),
            Err(e) => log::warn!("Speakers: joining two records of one person failed: {e:#}"),
        }
    }
    if !pairs.is_empty() {
        crate::local_index::refresh(app);
    }
}

/// Known people, from the voice profiles.
pub fn people(app: &AppHandle) -> Vec<Person> {
    process::voice_profiles(app)
        .map(|p| {
            p.profiles()
                .iter()
                .map(|v| Person {
                    name: v.name.clone(),
                    emails: v.emails.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Who was invited to the meeting: `invitees.json`, else the calendar
/// (cached). `None` when unknown.
pub fn invitees(store: &MeetingStore, r: &MeetingRecord) -> Option<Vec<Invitee>> {
    let path = store.audio_dir(&r.id).join(INVITEES_FILE);
    if let Ok(text) = std::fs::read_to_string(&path) {
        return serde_json::from_str(&text).ok();
    }
    if calendar::access() != calendar::CalendarAccess::Allowed {
        return None;
    }
    let event = super::mail::find_event(r);
    let list: Vec<Invitee> = event
        .map(|e| {
            // The organizer is invited too (often the one who runs it).
            e.organizer
                .iter()
                .chain(e.attendees.iter())
                .filter(|a| {
                    use meeting_mail::calendar::{Kind, Status};
                    a.status != Status::Declined && !matches!(a.kind, Kind::Room | Kind::Resource)
                })
                .map(|a| Invitee {
                    name: a.name.clone(),
                    email: a.email.as_deref().and_then(meeting_mail::normalize_email),
                    is_me: a.is_me,
                })
                .collect()
        })
        .unwrap_or_default();
    // An empty list isn't kept: the event may come later (a calendar added
    // afterwards, as on 2026-11-26).
    if !list.is_empty() {
        let _ = std::fs::write(&path, serde_json::to_string(&list).unwrap_or_default());
    }
    Some(list)
}

fn processed(r: &MeetingRecord) -> bool {
    matches!(r.status, MeetingStatus::Done) && !r.sample
}

fn all_speakers(store: &MeetingStore) -> (Vec<MeetingRecord>, Vec<MeetingSpeakers>) {
    all_speakers_named(store, None)
}

/// [`all_speakers`], each name as the person's own (`profiles`), so one
/// person is one voice on the Sprecher page whatever a note still says.
fn all_speakers_named(
    store: &MeetingStore,
    profiles: Option<&diarize::VoiceProfiles>,
) -> (Vec<MeetingRecord>, Vec<MeetingSpeakers>) {
    let records: Vec<MeetingRecord> = store.list().into_iter().filter(processed).collect();
    let meetings = records
        .iter()
        .map(|r| MeetingSpeakers {
            meeting_id: r.id.clone(),
            started_at: r.started_at.clone(),
            speakers: process::load_speakers(&store.audio_dir(&r.id))
                .into_iter()
                .map(|mut s| {
                    if let Some(p) = profiles.filter(|_| !is_placeholder(&s.name)) {
                        s.name = p.canonical(&s.name);
                    }
                    s
                })
                .collect(),
        })
        .filter(|m| !m.speakers.is_empty())
        .collect();
    (records, meetings)
}

// ------------------------------------------------------------ the page

/// What a speaker said in a meeting: its longest lines, longest first.
fn lines_of(r: &MeetingRecord, store: &MeetingStore, speaker: &str, max: usize) -> Vec<VoiceLine> {
    let Some(md) = r
        .note_path
        .as_deref()
        .and_then(|p| std::fs::read_to_string(p).ok())
    else {
        return Vec::new();
    };
    let audio = !r.audio_deleted && has_audio(store, r);
    let mut lines: Vec<VoiceLine> = transcript_from_note(&md)
        .segments
        .into_iter()
        .filter(|s| s.speaker == speaker && s.text.split_whitespace().count() >= 3)
        .map(|s| VoiceLine {
            meeting_id: r.id.clone(),
            meeting_title: r.title.clone().unwrap_or_default(),
            started_at: r.started_at.clone(),
            speaker: speaker.to_string(),
            start_s: s.start_s,
            text: s.text,
            audio,
        })
        .collect();
    lines.sort_by(|a, b| b.text.len().cmp(&a.text.len()));
    lines.truncate(max);
    lines
}

fn has_audio(store: &MeetingStore, r: &MeetingRecord) -> bool {
    let dir = store.audio_dir(&r.id);
    r.tracks.iter().any(|t| dir.join(t).exists())
}

/// Every voice across meetings, for the Sprecher page.
pub fn page(app: &AppHandle) -> VoicesPage {
    let store = store(app);
    let profiles = process::voice_profiles(app);
    let user = person_name(app, &user_name(app));
    let (records, meetings) = all_speakers_named(&store, profiles.as_ref());
    let groups = voices::group(&meetings, &user, SAME_VOICE);
    let people = people(app);
    let by_id: HashMap<&str, &MeetingRecord> = records.iter().map(|r| (r.id.as_str(), r)).collect();
    let mut invitee_cache: HashMap<String, Vec<Invitee>> = HashMap::new();
    let mut out = Vec::new();
    let mut once = 0;
    for g in &groups {
        let unnamed = g.unnamed().count() as u32;
        if g.name.is_none() && g.members.len() < 2 {
            once += 1;
            continue;
        }
        let profile = g.name.as_deref().and_then(|n| profiles.as_ref()?.person(n));
        let suggestion = if g.name.is_none() {
            for m in &g.members {
                if !invitee_cache.contains_key(&m.meeting_id) {
                    if let Some(list) = by_id
                        .get(m.meeting_id.as_str())
                        .and_then(|r| invitees(&store, r))
                    {
                        invitee_cache.insert(m.meeting_id.clone(), list);
                    }
                }
            }
            let of = g
                .members
                .iter()
                .filter(|m| invitee_cache.contains_key(&m.meeting_id))
                .count() as u32;
            voices::group_candidates(g, &invitee_cache, &people, &user)
                .into_iter()
                .next()
                .map(|(c, n)| VoiceSuggestion {
                    name: c.name,
                    email: c.email,
                    invited: n as u32,
                    of,
                })
        } else {
            None
        };
        // A line from each of the newest meetings, longest first.
        let mut lines: Vec<VoiceLine> = g
            .members
            .iter()
            .take(8)
            .filter_map(|m| by_id.get(m.meeting_id.as_str()).map(|r| (r, m)))
            .flat_map(|(r, m)| lines_of(r, &store, &m.speaker, 1))
            .collect();
        lines.sort_by(|a, b| b.audio.cmp(&a.audio).then(b.text.len().cmp(&a.text.len())));
        lines.truncate(3);
        out.push(VoiceInfo {
            id: g.id.clone(),
            name: g.name.clone(),
            is_user: g.is_user,
            meetings: g.members.len() as u32,
            unnamed,
            last: g.last.clone(),
            members: g
                .members
                .iter()
                .map(|m| VoiceMember {
                    meeting_id: m.meeting_id.clone(),
                    speaker: m.speaker.clone(),
                })
                .collect(),
            emails: profile.map(|p| p.emails.clone()).unwrap_or_default(),
            remembered: profile.is_some_and(|p| !p.embeddings.is_empty()),
            suggestion,
            lines,
        });
    }
    VoicesPage { voices: out, once }
}

// ------------------------------------------------------------ naming

/// Renames `from` to `to` in one meeting: note, speakers.json, record.
/// Returns the speaker's voice embedding.
fn rename_in(
    app: &AppHandle,
    store: &MeetingStore,
    id: &str,
    from: &str,
    to: &str,
) -> Result<Option<Vec<f32>>> {
    let record = store.get(id).context("Meeting not found")?;
    let dir = store.audio_dir(id);
    let note = record.note_path.as_deref().map(Path::new);
    let embedding = sokki_engine::speakers::rename(&dir, note, from, to)?;
    store.update(id, |r| {
        for s in &mut r.speakers {
            if *s == from {
                *s = to.trim().to_string();
            }
        }
    })?;
    super::emit_changed(app, id);
    Ok(embedding)
}

/// Names the one speaker left once everything else is placed (see
/// `voices::propose`), repeatedly. Returns the names given.
fn name_the_rest(app: &AppHandle, store: &MeetingStore, id: &str) -> Vec<String> {
    let mut given = Vec::new();
    for _ in 0..8 {
        let Some(p) = proposals(app, store, id)
            .into_iter()
            .find(|p| p.left.is_some())
        else {
            break;
        };
        let mut left = p.left.expect("checked");
        left.name = person_name(app, &left.name);
        if rename_in(app, store, id, &p.speaker, &left.name).is_err() {
            break;
        }
        // Their address names them in the next meeting's guest list too.
        if let (Some(email), Some(mut profiles)) =
            (left.email.as_ref(), process::voice_profiles(app))
        {
            let _ = profiles.update(&left.name, |p| {
                if !p.emails.iter().any(|e| e == email) {
                    p.emails.push(email.clone());
                }
            });
        }
        log::info!("Meeting {id}: the last unnamed speaker is the last invitee");
        given.push(left.name);
    }
    given
}

/// The names a voice carries in other meetings, per unnamed speaker.
fn voice_names(app: &AppHandle, store: &MeetingStore, id: &str) -> HashMap<String, String> {
    let (_, meetings) = all_speakers(store);
    let groups = voices::group(&meetings, &user_name(app), SAME_VOICE);
    let mut out = HashMap::new();
    for g in groups {
        let Some(name) = g.name else { continue };
        for m in g
            .members
            .iter()
            .filter(|m| m.meeting_id == id && is_placeholder(&m.speaker))
        {
            out.insert(m.speaker.clone(), name.clone());
        }
    }
    out
}

impl From<voices::Candidate> for NameCandidate {
    fn from(c: voices::Candidate) -> Self {
        Self {
            name: c.name,
            email: c.email,
            source: match c.source {
                voices::Source::Voice => "voice",
                voices::Source::Invited => "invited",
            }
            .into(),
        }
    }
}

/// [`proposals`] for the app.
pub fn speaker_proposals(app: &AppHandle, id: &str) -> Vec<SpeakerProposal> {
    proposals(app, &store(app), id)
        .into_iter()
        .map(|p| SpeakerProposal {
            speaker: p.speaker,
            candidates: p.candidates.into_iter().map(Into::into).collect(),
        })
        .collect()
}

/// What Sokki proposes for the unnamed speakers of meeting `id`.
pub fn proposals(app: &AppHandle, store: &MeetingStore, id: &str) -> Vec<Proposal> {
    let Some(r) = store.get(id).filter(processed) else {
        return Vec::new();
    };
    let speakers = process::load_speakers(&store.audio_dir(id));
    if !speakers.iter().any(|s| is_placeholder(&s.name)) {
        return Vec::new();
    }
    let invitees = invitees(store, &r).unwrap_or_default();
    voices::propose(
        &speakers,
        &invitees,
        &people(app),
        &user_name(app),
        &voice_names(app, store, id),
    )
}

/// Gives the speakers `members` the name `to`, in every meeting. With
/// `consent`, Sokki keeps the voice and recognises it from now on. Returns
/// how many meetings changed, including the ones where that left one name.
pub fn name_voice(
    app: &AppHandle,
    members: &[VoiceMember],
    to: &str,
    emails: &[String],
    consent: bool,
) -> Result<u32> {
    if to.trim().is_empty() {
        bail!("The name is empty.");
    }
    // "mara lind" for a known "Mara Lind" is them, not someone new.
    let canonical = person_name(app, to);
    let to = canonical.as_str();
    let store = store(app);
    let mut embeddings = Vec::new();
    let mut ids: Vec<String> = Vec::new();
    for m in members {
        // A known person given their full name (or another spelling): the
        // person is renamed, in every meeting, not split in two.
        let known = !is_placeholder(&m.speaker)
            && process::voice_profiles(app).is_some_and(|p| p.person(&m.speaker).is_some());
        if m.speaker != to && known && same_name(&m.speaker, to) {
            for id in rename_person(app, &store, &m.speaker, to, Backup::SeveralMeetings)? {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            if let Some(e) = process::load_speakers(&store.audio_dir(&m.meeting_id))
                .into_iter()
                .find(|s| s.name == to)
                .and_then(|s| s.embedding)
            {
                embeddings.push(e);
            }
            if !ids.contains(&m.meeting_id) {
                ids.push(m.meeting_id.clone());
            }
            continue;
        }
        if m.speaker != to {
            embeddings.extend(rename_in(app, &store, &m.meeting_id, &m.speaker, to)?);
        } else if let Some(e) = process::load_speakers(&store.audio_dir(&m.meeting_id))
            .into_iter()
            .find(|s| s.name == to)
            .and_then(|s| s.embedding)
        {
            embeddings.push(e);
        }
        if !ids.contains(&m.meeting_id) {
            ids.push(m.meeting_id.clone());
        }
    }
    let mut profiles = process::voice_profiles(app).context("Voice profiles unavailable")?;
    let now = chrono::Utc::now().to_rfc3339();
    if consent {
        for e in embeddings
            .iter()
            .rev()
            .take(diarize::profiles::MAX_EMBEDDINGS_PER_NAME)
        {
            profiles
                .enroll(to, e)
                .map_err(|e| anyhow::anyhow!("{e:#}"))?;
        }
    }
    let known = profiles.person(to).is_some();
    if consent || !emails.is_empty() || known {
        profiles.update(to, |p| {
            for e in emails {
                if !p.emails.iter().any(|x| x.eq_ignore_ascii_case(e.trim())) {
                    p.emails.push(e.clone());
                }
            }
            if consent {
                p.consent_at.get_or_insert_with(|| now.clone());
                p.last_heard = Some(now.clone());
            }
        })?;
    }
    let mut changed = ids.len() as u32;
    // The renamed notes, in the index (in the background).
    crate::local_index::refresh(app);
    for id in &ids {
        changed += name_the_rest(app, &store, id).len() as u32;
        crate::hooks::meeting_event(app, sokki_engine::hooks::event::MEETING_SPEAKERS_NAMED, id);
    }
    Ok(changed)
}

/// Marks a meeting whose "remember these voices?" question was answered.
const VOICE_OFFER_DONE: &str = "voice_offer_done";

/// Whom to ask, once, whether Sokki may keep their voice: everyone named
/// in meeting `id` whose voice Sokki doesn't know yet
/// ([`voices::voice_offer`]). Empty once the question was answered.
pub fn voice_offer(app: &AppHandle, id: &str) -> Vec<String> {
    let store = store(app);
    if store.get(id).filter(processed).is_none() {
        return Vec::new();
    }
    let dir = store.audio_dir(id);
    if dir.join(VOICE_OFFER_DONE).exists() {
        return Vec::new();
    }
    let profiles = process::voice_profiles(app);
    let has_voice = |name: &str| {
        profiles
            .as_ref()
            .and_then(|p| p.person(name))
            .is_some_and(|p| !p.embeddings.is_empty())
    };
    voices::voice_offer(&process::load_speakers(&dir), &user_name(app), has_voice)
}

/// The question of [`voice_offer`] was answered for meeting `id`.
pub fn voice_offer_done(app: &AppHandle, id: &str) -> Result<()> {
    let dir = store(app).audio_dir(id);
    std::fs::write(dir.join(VOICE_OFFER_DONE), b"")
        .with_context(|| format!("Can't write to {}", dir.display()))
}

/// A person's email addresses (empty when Sokki knows none).
pub fn emails(app: &AppHandle, name: &str) -> Vec<String> {
    process::voice_profiles(app)
        .and_then(|p| p.person(name.trim()).map(|v| v.emails.clone()))
        .unwrap_or_default()
}

/// Sets a person's email addresses (all of them).
pub fn set_emails(app: &AppHandle, name: &str, emails: Vec<String>) -> Result<()> {
    let mut profiles = process::voice_profiles(app).context("Voice profiles unavailable")?;
    profiles.update(name, |p| p.emails = emails)
}

// ------------------------------------------------------------ processing

/// After a meeting is processed: remember who was invited, give the last
/// speaker the last invitee's name, note when known voices were heard, and
/// drop voices unused for a year.
pub fn after_processing(app: &AppHandle, id: &str, recognised: &[String]) {
    let store = store(app);
    if let Some(r) = store.get(id).filter(processed) {
        let _ = invitees(&store, &r);
        let named = name_from_call(app, &store, &r);
        if named > 0 {
            log::info!("Meeting {id}: {named} speaker(s) named from the call app");
        }
        let given = name_the_rest(app, &store, id);
        if !given.is_empty() {
            log::info!(
                "Meeting {id}: {} speaker(s) named from the guest list",
                given.len()
            );
        }
    }
    let Some(mut profiles) = process::voice_profiles(app) else {
        return;
    };
    let now = chrono::Utc::now();
    for name in recognised {
        let _ = profiles.update(name, |p| p.last_heard = Some(now.to_rfc3339()));
    }
    let cutoff = (now - chrono::Duration::days(FORGET_AFTER_DAYS)).to_rfc3339();
    match profiles.forget_unused(&cutoff) {
        Ok(0) => {}
        Ok(n) => log::info!("Deleted {n} voice(s) unused for a year"),
        Err(e) => log::warn!("Forgetting unused voices failed: {e:#}"),
    }
}

/// Names from the call app (`call_speakers.json`): Meet's captions or
/// Zoom's active speaker, laid over the transcript's turns.
fn name_from_call(app: &AppHandle, store: &MeetingStore, r: &MeetingRecord) -> usize {
    let dir = store.audio_dir(&r.id);
    let spans = super::call_speakers::load(&dir);
    if spans.is_empty() {
        return 0;
    }
    let Some(md) = r
        .note_path
        .as_deref()
        .and_then(|p| std::fs::read_to_string(p).ok())
    else {
        return 0;
    };
    let turns: Vec<voices::Turn> = transcript_from_note(&md)
        .segments
        .into_iter()
        .map(|s| voices::Turn {
            speaker: s.speaker,
            start_s: s.start_s,
            end_s: s.end_s.max(s.start_s),
        })
        .collect();
    let mut self_names: Vec<String> = meeting_chat::SELF_NAMES
        .iter()
        .map(|s| s.to_string())
        .collect();
    self_names.extend(user_names(app));
    // At least a few seconds together, so one stray caption names nobody.
    let pairs = voices::names_from_call(&turns, &spans, &self_names, 3.0);
    let mut n = 0;
    for (speaker, name) in pairs {
        // The call's "Lena Vogt" is the known "Lena Vogt", however
        // the call writes it.
        let name = person_name(app, &name);
        if rename_in(app, store, &r.id, &speaker, &name).is_ok() {
            n += 1;
        }
    }
    n
}

// ------------------------------------------------------------ clips

/// How long a clip plays at most, seconds.
const CLIP_MAX_S: f64 = 8.0;

/// A few seconds of `speaker` in meeting `id` as a WAV data URL: from
/// `start_s` (a transcript line), else from the speaker's longest line.
pub fn clip(app: &AppHandle, id: &str, speaker: &str, start_s: Option<f64>) -> Result<String> {
    let store = store(app);
    let r = store.get(id).context("Meeting not found")?;
    if r.audio_deleted || !has_audio(&store, &r) {
        bail!("The audio of this meeting is deleted.");
    }
    let md = r
        .note_path
        .as_deref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .context("The note is missing.")?;
    let segments = transcript_from_note(&md).segments;
    let seg = match start_s {
        Some(t) => segments
            .iter()
            .filter(|s| s.speaker == speaker)
            .min_by(|a, b| (a.start_s - t).abs().total_cmp(&(b.start_s - t).abs())),
        None => segments
            .iter()
            .filter(|s| s.speaker == speaker)
            .max_by(|a, b| (a.end_s - a.start_s).total_cmp(&(b.end_s - b.start_s))),
    }
    .context("This speaker says nothing in the transcript.")?;
    let mut end = seg.end_s;
    if end <= seg.start_s {
        end = seg.start_s + CLIP_MAX_S;
    }
    let end = end.min(seg.start_s + CLIP_MAX_S);

    let dir = store.audio_dir(id);
    let saved = process::load_speakers(&dir);
    let track = track_of(&saved, speaker, &r, &user_name(app));
    let file = r
        .tracks
        .iter()
        .find(|t| t.starts_with(track))
        .or_else(|| r.tracks.first())
        .context("No audio track.")?;
    let samples = crate::audio_toolkit::decode_audio_file(&dir.join(file))?;
    let pad = std::fs::read_to_string(dir.join(super::TIMING_FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<super::TrackTiming>(&t).ok())
        .and_then(|t| sokki_engine::tracks::track_pads(t.mic_first_us, t.system_first_us))
        .map(|(mic, system)| {
            if file.starts_with("system") {
                system
            } else {
                mic
            }
        })
        .unwrap_or(0);
    const RATE: f64 = 16_000.0;
    let at = |t: f64| ((t * RATE) as usize).saturating_sub(pad).min(samples.len());
    let part = &samples[at(seg.start_s)..at(end)];
    if part.is_empty() {
        bail!("No audio at that point.");
    }
    wav_data_url(part)
}

/// At most this much audio for a cited stretch of a meeting.
const SPAN_MAX_S: f64 = 60.0;

/// The meeting from `start_s` to `end_s` (at most [`SPAN_MAX_S`]), every
/// track mixed, as a WAV data URL: "listen from here" on a summary point.
pub fn span(app: &AppHandle, id: &str, start_s: f64, end_s: f64) -> Result<String> {
    let store = store(app);
    let r = store.get(id).context("Meeting not found")?;
    if r.audio_deleted || !has_audio(&store, &r) {
        bail!("The audio of this meeting is deleted.");
    }
    let start = start_s.max(0.0);
    let end = end_s.clamp(start + 1.0, start + SPAN_MAX_S);
    let dir = store.audio_dir(id);
    let timing = std::fs::read_to_string(dir.join(super::TIMING_FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<super::TrackTiming>(&t).ok())
        .and_then(|t| sokki_engine::tracks::track_pads(t.mic_first_us, t.system_first_us));
    let files: Vec<(std::path::PathBuf, usize)> = r
        .tracks
        .iter()
        .map(|file| {
            let pad = timing
                .map(|(mic, system)| {
                    if file.starts_with("system") {
                        system
                    } else {
                        mic
                    }
                })
                .unwrap_or(0);
            (dir.join(file), pad)
        })
        .collect();
    let t0 = std::time::Instant::now();
    let mix = mix_span(&files, start, end)?;
    log::info!(
        "Clip {start:.0}-{end:.0} s of {} tracks in {:?}",
        files.len(),
        t0.elapsed()
    );
    wav_data_url(&mix)
}

/// `start..end` seconds of every track (file, samples it starts late by),
/// summed and clipped to [-1, 1]. Tracks that can't be read are skipped.
fn mix_span(files: &[(std::path::PathBuf, usize)], start: f64, end: f64) -> Result<Vec<f32>> {
    const RATE: f64 = 16_000.0;
    let from = (start * RATE) as usize;
    let len = ((end - start) * RATE) as usize;
    let mut mix = vec![0.0f32; len];
    let mut any = false;
    for (path, pad) in files {
        // A track that started `pad` samples late holds time t at t - pad;
        // before it began, the clip has silence (`lead`).
        let at = from as i64 - *pad as i64;
        let lead = (-at).max(0) as usize;
        if lead >= len {
            continue;
        }
        let samples = match crate::audio_toolkit::decode_audio_span(
            path,
            at.max(0) as f64 / RATE,
            (len - lead) as f64 / RATE,
        ) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("Clip: can't read {}: {e:#}", path.display());
                continue;
            }
        };
        for (i, x) in samples.iter().take(len - lead).enumerate() {
            mix[lead + i] += x;
            any = true;
        }
    }
    if !any {
        bail!("No audio at that point.");
    }
    for x in &mut mix {
        *x = x.clamp(-1.0, 1.0);
    }
    Ok(mix)
}

/// The track a speaker is on: saved since 26.11.2026; before, the user's
/// voice is the microphone and everyone else the call when there is one.
fn track_of(saved: &[SavedSpeaker], speaker: &str, r: &MeetingRecord, user: &str) -> &'static str {
    match saved
        .iter()
        .find(|s| s.name == speaker)
        .and_then(|s| s.track.as_deref())
    {
        Some("system") => "system",
        Some(_) => "mic",
        None if speaker.trim().eq_ignore_ascii_case(user.trim()) => "mic",
        None if r.tracks.iter().any(|t| t.starts_with("system")) => "system",
        None => "mic",
    }
}

fn wav_data_url(samples: &[f32]) -> Result<String> {
    use base64::Engine;
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::new(&mut buf, spec)?;
        // Quiet calls get louder, never clipped: peak to -3 dBFS at most 4×.
        let peak = samples.iter().fold(0f32, |m, s| m.max(s.abs())).max(1e-4);
        let gain = (0.7 / peak).min(4.0);
        for s in samples {
            w.write_sample(((s * gain).clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
        }
        w.finalize()?;
    }
    Ok(format!(
        "data:audio/wav;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(buf.into_inner())
    ))
}

#[cfg(test)]
mod span_tests {
    use super::mix_span;

    fn wav(dir: &std::path::Path, name: &str, samples: &[f32]) -> std::path::PathBuf {
        let p = dir.join(name);
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut w = hound::WavWriter::create(&p, spec).unwrap();
        for s in samples {
            w.write_sample(*s).unwrap();
        }
        w.finalize().unwrap();
        p
    }

    #[test]
    fn mixes_tracks_at_their_own_offsets() {
        let dir = tempfile::tempdir().unwrap();
        // Mic: 0.1 from 0 s to 3 s. System started 1 s late: its 0.2 plays
        // from 1 s on.
        let mic = wav(dir.path(), "mic.wav", &vec![0.1; 48_000]);
        let sys = wav(dir.path(), "system.wav", &vec![0.2; 32_000]);
        let m = mix_span(&[(mic, 0), (sys, 16_000)], 0.5, 2.5).unwrap();
        assert_eq!(m.len(), 32_000);
        assert!((m[0] - 0.1).abs() < 1e-6, "0.5 s: mic only");
        assert!((m[16_000] - 0.3).abs() < 1e-6, "1.5 s: both");
        let late = mix_span(&[(dir.path().join("mic.wav"), 0)], 2.0, 4.0).unwrap();
        assert_eq!(late[16_000 + 10], 0.0, "after the track: silence");
    }

    #[test]
    fn a_span_is_the_same_samples_as_the_whole_file() {
        let dir = tempfile::tempdir().unwrap();
        let ramp: Vec<f32> = (0..160_000).map(|i| (i % 1000) as f32 / 1000.0).collect();
        let p = wav(dir.path(), "ramp.wav", &ramp);
        let span = crate::audio_toolkit::decode_audio_span(&p, 3.0, 2.0).unwrap();
        assert_eq!(span, ramp[48_000..80_000]);
        let tail = crate::audio_toolkit::decode_audio_span(&p, 9.5, 2.0).unwrap();
        assert_eq!(tail.len(), 8_000, "the file ends after half a second");
        assert!(crate::audio_toolkit::decode_audio_span(&p, 20.0, 1.0)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn no_readable_track_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(mix_span(&[(dir.path().join("gone.flac"), 0)], 0.0, 1.0).is_err());
    }

    /// On a real recording (the test app's sample meeting):
    /// SOKKI_SPAN_DIR=<meeting audio dir> cargo test span_of_a_real_meeting -- --ignored
    #[test]
    #[ignore]
    fn span_of_a_real_meeting() {
        let dir = std::path::PathBuf::from(std::env::var("SOKKI_SPAN_DIR").unwrap());
        let files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "flac" || x == "wav"))
            .map(|p| (p, 0))
            .collect();
        for (p, _) in &files {
            let t0 = std::time::Instant::now();
            let whole = crate::audio_toolkit::decode_audio_file(p).unwrap();
            let t_whole = t0.elapsed();
            let t0 = std::time::Instant::now();
            let span = crate::audio_toolkit::decode_audio_span(p, 30.0, 15.0).unwrap();
            println!(
                "{}: whole file {t_whole:?}, 15 s span {:?}",
                p.display(),
                t0.elapsed()
            );
            assert_eq!(span, whole[30 * 16_000..45 * 16_000], "seek is exact");
        }
        let t0 = std::time::Instant::now();
        let m = mix_span(&files, 30.0, 45.0).unwrap();
        let rms = (m.iter().map(|x| x * x).sum::<f32>() / m.len() as f32).sqrt();
        println!(
            "span: {} tracks, {} samples, rms {rms:.4}, {:?}",
            files.len(),
            m.len(),
            t0.elapsed()
        );
        assert_eq!(m.len(), 15 * 16_000);
        assert!(rms > 0.001, "speech in the sample meeting at 30-45 s");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_backup_holds_every_file_byte_for_byte() {
        let root = std::env::temp_dir().join(format!("sokki-backup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::create_dir_all(root.join("b")).unwrap();
        let one = root.join("a/speakers.json");
        let two = root.join("b/speakers.json");
        std::fs::write(&one, b"[{\"name\":\"Mara\"}]").unwrap();
        std::fs::write(&two, [0u8, 159, 146, 150]).unwrap();
        let missing = root.join("c/none.json");
        let dir = root.join("backup");
        backup_files(&dir, &[one.clone(), two.clone(), missing, one.clone()]).unwrap();
        let manifest: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest.len(), 2);
        for m in manifest {
            let original = std::path::PathBuf::from(m["original"].as_str().unwrap());
            let copy = dir.join(m["copy"].as_str().unwrap());
            assert_eq!(
                std::fs::read(original).unwrap(),
                std::fs::read(copy).unwrap()
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}

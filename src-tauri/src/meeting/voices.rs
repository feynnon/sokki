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
    let records: Vec<MeetingRecord> = store.list().into_iter().filter(processed).collect();
    let meetings = records
        .iter()
        .map(|r| MeetingSpeakers {
            meeting_id: r.id.clone(),
            started_at: r.started_at.clone(),
            speakers: process::load_speakers(&store.audio_dir(&r.id)),
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
    let user = user_name(app);
    let (records, meetings) = all_speakers(&store);
    let groups = voices::group(&meetings, &user, SAME_VOICE);
    let profiles = process::voice_profiles(app);
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
        let left = p.left.expect("checked");
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
    let to = to.trim();
    if to.is_empty() {
        bail!("The name is empty.");
    }
    let store = store(app);
    let mut embeddings = Vec::new();
    let mut ids: Vec<String> = Vec::new();
    for m in members {
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
    self_names.push(user_name(app));
    // At least a few seconds together, so one stray caption names nobody.
    let pairs = voices::names_from_call(&turns, &spans, &self_names, 3.0);
    let mut n = 0;
    for (speaker, name) in pairs {
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

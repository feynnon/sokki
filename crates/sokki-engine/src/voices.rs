//! Who is who across meetings: the same voice in several meetings, and the
//! names Sokki can propose for a voice that has none yet.
//!
//! Inputs are what each meeting keeps next to its audio (`speakers.json`:
//! a name and a voice embedding per speaker) and, when the calendar knows
//! the meeting, who was invited. Nothing here touches the disk.
//!
//! Grouping across meetings compares voices, which makes it biometric
//! processing (GDPR Art. 9). Groups are computed when asked and never
//! stored; only a voice the user names with the person's consent is kept
//! (`diarize::VoiceProfiles`).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::speakers::{is_placeholder, SavedSpeaker};

/// One processed meeting's speakers.
#[derive(Debug, Clone)]
pub struct MeetingSpeakers {
    pub meeting_id: String,
    /// RFC 3339; orders meetings.
    pub started_at: String,
    pub speakers: Vec<SavedSpeaker>,
}

/// A speaker of one meeting.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct Member {
    pub meeting_id: String,
    pub speaker: String,
}

/// One voice across meetings.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceGroup {
    /// Stable while the group keeps its first member.
    pub id: String,
    /// The name its speakers already carry, if any.
    pub name: Option<String>,
    /// The user's own voice.
    pub is_user: bool,
    /// Newest meeting first.
    pub members: Vec<Member>,
    /// `started_at` of the newest meeting.
    pub last: String,
}

impl VoiceGroup {
    /// Members still called "Speaker N".
    pub fn unnamed(&self) -> impl Iterator<Item = &Member> {
        self.members.iter().filter(|m| is_placeholder(&m.speaker))
    }
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

/// Groups the speakers of all meetings by voice: average-link clustering
/// at cosine `threshold`, where two speakers of one meeting never share a
/// group and two different names never meet. Speakers without an embedding
/// stay alone. Groups come newest first.
///
/// On Robin's 46 meetings (25.11.2026) at 0.6 this put 16 of the 17 voices
/// already named as the user into one group, and found two unnamed people
/// in 13 and 12 meetings.
pub fn group(meetings: &[MeetingSpeakers], user_name: &str, threshold: f32) -> Vec<VoiceGroup> {
    struct Item<'a> {
        meeting: usize,
        member: Member,
        name: Option<&'a str>,
        embedding: Option<&'a [f32]>,
    }
    let user = user_name.trim().to_lowercase();
    let mut items: Vec<Item> = Vec::new();
    for (mi, m) in meetings.iter().enumerate() {
        for s in &m.speakers {
            items.push(Item {
                meeting: mi,
                member: Member {
                    meeting_id: m.meeting_id.clone(),
                    speaker: s.name.clone(),
                },
                name: (!is_placeholder(&s.name)).then_some(s.name.as_str()),
                embedding: s.embedding.as_deref().filter(|e| !e.is_empty()),
            });
        }
    }
    let n = items.len();
    // Pairwise similarity; clusters keep the sum over their pairs.
    let mut sim = vec![0f32; n * n];
    for i in 0..n {
        for j in (i + 1)..n {
            let s = match (items[i].embedding, items[j].embedding) {
                (Some(a), Some(b)) => cosine(a, b),
                _ => f32::NEG_INFINITY,
            };
            sim[i * n + j] = s;
            sim[j * n + i] = s;
        }
    }
    struct Cluster<'a> {
        items: Vec<usize>,
        meetings: Vec<usize>,
        name: Option<&'a str>,
    }
    let mut clusters: Vec<Option<Cluster>> = items
        .iter()
        .enumerate()
        .map(|(i, it)| {
            Some(Cluster {
                items: vec![i],
                meetings: vec![it.meeting],
                name: it.name,
            })
        })
        .collect();
    // Sum of similarities between clusters, kept up to date on merges.
    let mut sums = sim.clone();
    loop {
        let mut best: Option<(f32, usize, usize)> = None;
        for a in 0..n {
            let Some(ca) = &clusters[a] else { continue };
            for b in (a + 1)..n {
                let Some(cb) = &clusters[b] else { continue };
                if ca.meetings.iter().any(|m| cb.meetings.contains(m)) {
                    continue;
                }
                if let (Some(x), Some(y)) = (ca.name, cb.name) {
                    if !x.eq_ignore_ascii_case(y) {
                        continue;
                    }
                }
                let avg = sums[a * n + b] / (ca.items.len() * cb.items.len()) as f32;
                if avg >= threshold && best.is_none_or(|(s, _, _)| avg > s) {
                    best = Some((avg, a, b));
                }
            }
        }
        let Some((_, a, b)) = best else { break };
        let cb = clusters[b].take().expect("live cluster");
        for c in 0..n {
            if c != a && c != b {
                let s = sums[a * n + c] + sums[b * n + c];
                sums[a * n + c] = s;
                sums[c * n + a] = s;
            }
        }
        let ca = clusters[a].as_mut().expect("live cluster");
        ca.items.extend(cb.items);
        ca.meetings.extend(cb.meetings);
        ca.name = ca.name.or(cb.name);
    }
    let mut out: Vec<VoiceGroup> = clusters
        .into_iter()
        .flatten()
        .map(|c| {
            let mut idx = c.items;
            idx.sort_by(|&x, &y| {
                meetings[items[y].meeting]
                    .started_at
                    .cmp(&meetings[items[x].meeting].started_at)
            });
            let first = *idx.iter().min().expect("non-empty");
            let name = majority_name(idx.iter().filter_map(|&i| items[i].name));
            let is_user = !user.is_empty()
                && name
                    .as_deref()
                    .is_some_and(|nm| nm.trim().to_lowercase() == user);
            VoiceGroup {
                id: format!(
                    "{}|{}",
                    items[first].member.meeting_id, items[first].member.speaker
                ),
                name,
                is_user,
                last: meetings[items[idx[0]].meeting].started_at.clone(),
                members: idx.into_iter().map(|i| items[i].member.clone()).collect(),
            }
        })
        .collect();
    out.sort_by(|a, b| b.last.cmp(&a.last).then_with(|| a.id.cmp(&b.id)));
    out
}

fn majority_name<'a>(names: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for n in names {
        match counts.iter_mut().find(|(x, _)| *x == n) {
            Some((_, c)) => *c += 1,
            None => counts.push((n, 1)),
        }
    }
    counts
        .into_iter()
        .max_by_key(|&(_, c)| c)
        .map(|(n, _)| n.to_string())
}

/// Someone invited to a meeting, as the calendar has them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Invitee {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    /// The person using Sokki.
    #[serde(default)]
    pub is_me: bool,
}

/// A person Sokki knows: a name and the addresses they're invited with.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Person {
    pub name: String,
    pub emails: Vec<String>,
}

/// Why a name is proposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The same voice carries this name in another meeting.
    Voice,
    /// Invited to this meeting.
    Invited,
}

/// A name proposed for a speaker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub name: String,
    /// The address the calendar invited them with, if any.
    pub email: Option<String>,
    pub source: Source,
}

/// What Sokki proposes for one unnamed speaker of a meeting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    pub speaker: String,
    /// Best first.
    pub candidates: Vec<Candidate>,
    /// The one name that is left once everything else is placed: the
    /// speaker is the only unnamed one, and one invitee is unaccounted for
    /// (a 1:1 call, or everyone else already named). Sokki applies it.
    pub left: Option<Candidate>,
}

/// The display name of an invitee: the known person with that address,
/// else the calendar's name, else the address.
pub fn invitee_name(i: &Invitee, people: &[Person]) -> Option<String> {
    let email = i.email.as_deref().map(|e| e.trim().to_lowercase());
    if let Some(e) = email.as_deref() {
        if let Some(p) = people
            .iter()
            .find(|p| p.emails.iter().any(|x| x.eq_ignore_ascii_case(e)))
        {
            return Some(p.name.clone());
        }
    }
    i.name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty() && !n.contains('@'))
        .map(String::from)
        .or_else(|| email.as_deref().map(name_from_address))
}

/// "lena.vogt@example.com" → "Lena Vogt": a calendar guest without a
/// display name (Google gives only the address, as on 27 Nov 2026) is
/// proposed by name, not as an address.
pub fn name_from_address(email: &str) -> String {
    let local = email.split('@').next().unwrap_or(email);
    let words: Vec<String> = local
        .split(['.', '_', '-', '+'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect(),
                None => String::new(),
            }
        })
        .collect();
    if words.is_empty() || !local.chars().any(char::is_alphabetic) {
        return email.to_string();
    }
    words.join(" ")
}

/// Names to propose for the unnamed speakers of one meeting.
///
/// `voice_names` maps a speaker to the name its voice carries in other
/// meetings (from [`group`]). Invitees already present as a named speaker,
/// and the user, are not proposed again.
pub fn propose(
    speakers: &[SavedSpeaker],
    invitees: &[Invitee],
    people: &[Person],
    user_name: &str,
    voice_names: &HashMap<String, String>,
) -> Vec<Proposal> {
    let same = |a: &str, b: &str| a.trim().eq_ignore_ascii_case(b.trim());
    let named: Vec<&str> = speakers
        .iter()
        .map(|s| s.name.as_str())
        .filter(|n| !is_placeholder(n))
        .collect();
    let mut open: Vec<Candidate> = Vec::new();
    for i in invitees {
        if i.is_me {
            continue;
        }
        let Some(name) = invitee_name(i, people) else {
            continue;
        };
        if same(&name, user_name) || named.iter().any(|n| same(n, &name)) {
            continue;
        }
        if open.iter().any(|c| same(&c.name, &name)) {
            continue;
        }
        open.push(Candidate {
            name,
            email: i.email.as_deref().map(|e| e.trim().to_lowercase()),
            source: Source::Invited,
        });
    }
    let unnamed: Vec<&SavedSpeaker> = speakers
        .iter()
        .filter(|s| is_placeholder(&s.name))
        .collect();
    let by_voice = |s: &SavedSpeaker| {
        voice_names
            .get(&s.name)
            .filter(|n| !named.iter().any(|x| same(x, n)))
            .cloned()
    };
    let without_voice: Vec<&&SavedSpeaker> =
        unnamed.iter().filter(|s| by_voice(s).is_none()).collect();
    // Names the voice already gives aren't open any more.
    let invited_open = open.clone();
    let open: Vec<Candidate> = open
        .into_iter()
        .filter(|c| {
            !unnamed
                .iter()
                .any(|s| by_voice(s).is_some_and(|v| same(&v, &c.name)))
        })
        .collect();
    unnamed
        .iter()
        .map(|s| {
            let mut candidates = Vec::new();
            if let Some(v) = by_voice(s) {
                let email = invitees
                    .iter()
                    .find(|i| invitee_name(i, people).is_some_and(|n| same(&n, &v)))
                    .and_then(|i| i.email.as_deref().map(|e| e.trim().to_lowercase()));
                candidates.push(Candidate {
                    name: v,
                    email,
                    source: Source::Voice,
                });
            }
            candidates.extend(open.iter().cloned());
            let left = match candidates.first() {
                // The voice and the guest list agree on the last name.
                Some(c) if c.source == Source::Voice => (unnamed.len() == 1
                    && invited_open.len() == 1
                    && same(&invited_open[0].name, &c.name))
                .then(|| invited_open[0].clone()),
                _ => (without_voice.len() == 1 && open.len() == 1).then(|| open[0].clone()),
            };
            Proposal {
                speaker: s.name.clone(),
                candidates,
                left,
            }
        })
        .collect()
}

/// For a voice without a name: invitees of its meetings, with how many of
/// them each was invited to, most first. Only those invited to at least two
/// meetings, so one meeting's guest list alone proposes nothing here.
pub fn group_candidates(
    group: &VoiceGroup,
    invitees: &HashMap<String, Vec<Invitee>>,
    people: &[Person],
    user_name: &str,
) -> Vec<(Candidate, usize)> {
    let mut counts: Vec<(Candidate, usize)> = Vec::new();
    for m in &group.members {
        let Some(list) = invitees.get(&m.meeting_id) else {
            continue;
        };
        let mut seen: Vec<String> = Vec::new();
        for i in list.iter().filter(|i| !i.is_me) {
            let Some(name) = invitee_name(i, people) else {
                continue;
            };
            if name.trim().eq_ignore_ascii_case(user_name.trim()) || seen.contains(&name) {
                continue;
            }
            seen.push(name.clone());
            match counts.iter_mut().find(|(c, _)| c.name == name) {
                Some((_, n)) => *n += 1,
                None => counts.push((
                    Candidate {
                        name,
                        email: i.email.as_deref().map(|e| e.trim().to_lowercase()),
                        source: Source::Invited,
                    },
                    1,
                )),
            }
        }
    }
    counts.retain(|(_, n)| *n >= 2);
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.name.cmp(&b.0.name)));
    counts
}

/// When a name spoke, as the call app showed it (Meet's captions, Zoom's
/// "active speaker"): seconds from the start of the recording.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallSpan {
    pub start_s: f64,
    pub end_s: f64,
    pub name: String,
}

/// A transcript turn: who Sokki heard, when.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub speaker: String,
    pub start_s: f64,
    pub end_s: f64,
}

/// Names for unnamed speakers from the call app: each "Speaker N" takes
/// the name that spoke during most of its turns. A name is given when it
/// covers more than half of the speaker's time the call app accounts for,
/// and at least `min_s` seconds; each name goes to one speaker only (the
/// one it overlaps most). `self_names` are how the call app shows the user
/// ("Ich", "You", the user's name) and are never given.
pub fn names_from_call(
    turns: &[Turn],
    spans: &[CallSpan],
    self_names: &[String],
    min_s: f64,
) -> Vec<(String, String)> {
    let is_self = |n: &str| {
        self_names
            .iter()
            .any(|s| s.trim().eq_ignore_ascii_case(n.trim()))
    };
    // Overlap seconds per (speaker, name).
    let mut overlap: Vec<(String, String, f64)> = Vec::new();
    let mut accounted: HashMap<String, f64> = HashMap::new();
    for t in turns.iter().filter(|t| is_placeholder(&t.speaker)) {
        for s in spans {
            let o = t.end_s.min(s.end_s) - t.start_s.max(s.start_s);
            if o <= 0.0 {
                continue;
            }
            *accounted.entry(t.speaker.clone()).or_default() += o;
            if is_self(&s.name) {
                continue;
            }
            match overlap
                .iter_mut()
                .find(|(sp, n, _)| *sp == t.speaker && *n == s.name)
            {
                Some((_, _, x)) => *x += o,
                None => overlap.push((t.speaker.clone(), s.name.clone(), o)),
            }
        }
    }
    overlap.sort_by(|a, b| b.2.total_cmp(&a.2));
    let mut out: Vec<(String, String)> = Vec::new();
    for (speaker, name, secs) in overlap {
        let total = accounted.get(&speaker).copied().unwrap_or(0.0);
        if secs < min_s || secs * 2.0 <= total {
            continue;
        }
        if out.iter().any(|(s, n)| *s == speaker || *n == name) {
            continue;
        }
        out.push((speaker, name));
    }
    out
}

/// Who Sokki may ask to keep the voice of, once every speaker of a meeting
/// has a name: the named speakers with a voice print who are not the user
/// and whose voice Sokki doesn't know yet (`has_voice`). Empty while a
/// speaker is still unnamed. One question for all of them, at the end, so
/// naming stays one click and the speaker Sokki named by itself is asked
/// too (Robin chose this, 26 Nov 2026).
pub fn voice_offer(
    speakers: &[SavedSpeaker],
    user_name: &str,
    has_voice: impl Fn(&str) -> bool,
) -> Vec<String> {
    if speakers.iter().any(|s| is_placeholder(&s.name)) {
        return Vec::new();
    }
    let same = |a: &str, b: &str| a.trim().eq_ignore_ascii_case(b.trim());
    let mut out: Vec<String> = Vec::new();
    for s in speakers {
        let voiced = s.embedding.as_ref().is_some_and(|e| !e.is_empty());
        if voiced
            && !same(&s.name, user_name)
            && !has_voice(&s.name)
            && !out.iter().any(|n| same(n, &s.name))
        {
            out.push(s.name.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sp(name: &str, e: &[f32]) -> SavedSpeaker {
        SavedSpeaker {
            name: name.into(),
            embedding: Some(e.to_vec()),
            track: None,
            speech_s: None,
        }
    }

    #[test]
    fn voice_offer_waits_until_everyone_is_named() {
        let speakers = [
            sp("Ich", &[1.0]),
            sp("Lena Berg", &[0.5]),
            sp("Speaker 3", &[0.2]),
        ];
        assert!(voice_offer(&speakers, "Ich", |_| false).is_empty());
    }

    #[test]
    fn voice_offer_skips_the_user_known_voices_and_speakers_without_a_voice() {
        let mut silent = sp("Mia Sand", &[]);
        silent.embedding = None;
        let speakers = [
            sp("Ich", &[1.0]),
            sp("Lena Berg", &[0.5]),
            sp("Jonas Weber", &[0.2]),
            silent,
        ];
        let offer = voice_offer(&speakers, "ich", |n| n == "Jonas Weber");
        assert_eq!(offer, vec!["Lena Berg".to_string()]);
    }

    fn meeting(id: &str, at: &str, speakers: Vec<SavedSpeaker>) -> MeetingSpeakers {
        MeetingSpeakers {
            meeting_id: id.into(),
            started_at: at.into(),
            speakers,
        }
    }

    fn inv(name: &str, email: &str) -> Invitee {
        Invitee {
            name: Some(name.into()),
            email: Some(email.into()),
            is_me: false,
        }
    }

    fn turn(sp: &str, a: f64, b: f64) -> Turn {
        Turn {
            speaker: sp.into(),
            start_s: a,
            end_s: b,
        }
    }

    fn span(name: &str, a: f64, b: f64) -> CallSpan {
        CallSpan {
            start_s: a,
            end_s: b,
            name: name.into(),
        }
    }

    #[test]
    fn call_names_go_to_the_voice_they_overlap() {
        let turns = vec![
            turn("Speaker 2", 0.0, 6.0),
            turn("Robin", 6.5, 10.0),
            turn("Speaker 3", 10.5, 15.0),
            turn("Speaker 2", 15.5, 20.0),
        ];
        // The call app lags a little and shows the user as "Ich".
        let spans = vec![
            span("Lena Vogt", 0.4, 6.3),
            span("Ich", 6.8, 10.2),
            span("Jonas Weber", 10.9, 15.2),
            span("Lena Vogt", 15.8, 20.1),
        ];
        let got = names_from_call(&turns, &spans, &["Ich".into(), "Robin".into()], 3.0);
        assert_eq!(
            got,
            vec![
                ("Speaker 2".to_string(), "Lena Vogt".to_string()),
                ("Speaker 3".to_string(), "Jonas Weber".to_string())
            ]
        );
    }

    #[test]
    fn unclear_or_short_overlaps_name_nobody() {
        // Two names share the speaker's time half and half: no name.
        let turns = vec![turn("Speaker 2", 0.0, 10.0)];
        let spans = vec![span("Lena", 0.0, 5.0), span("Jonas", 5.0, 10.0)];
        assert!(names_from_call(&turns, &spans, &[], 3.0).is_empty());
        // Too short.
        let spans = vec![span("Lena", 0.0, 2.0)];
        assert!(names_from_call(&turns, &spans, &[], 3.0).is_empty());
        // Only the user spoke then.
        let spans = vec![span("Ich", 0.0, 10.0)];
        assert!(names_from_call(&turns, &spans, &["Ich".into()], 3.0).is_empty());
    }

    #[test]
    fn one_name_one_speaker() {
        // Sortformer split one person in two: the name goes to the larger part.
        let turns = vec![turn("Speaker 2", 0.0, 8.0), turn("Speaker 3", 8.0, 12.0)];
        let spans = vec![span("Lena", 0.0, 12.0)];
        let got = names_from_call(&turns, &spans, &[], 3.0);
        assert_eq!(got, vec![("Speaker 2".to_string(), "Lena".to_string())]);
    }

    #[test]
    fn groups_one_voice_across_meetings() {
        let ms = vec![
            meeting(
                "m1",
                "2026-11-01T10:00:00Z",
                vec![
                    sp("Robin", &[1.0, 0.0, 0.0]),
                    sp("Speaker 2", &[0.0, 1.0, 0.1]),
                ],
            ),
            meeting(
                "m2",
                "2026-11-02T10:00:00Z",
                vec![
                    sp("Robin", &[0.95, 0.05, 0.0]),
                    sp("Speaker 2", &[0.05, 0.98, 0.0]),
                ],
            ),
            meeting(
                "m3",
                "2026-11-03T10:00:00Z",
                vec![sp("Speaker 1", &[0.0, 0.1, 1.0])],
            ),
        ];
        let g = group(&ms, "Robin", 0.6);
        assert_eq!(g.len(), 3);
        let user = g.iter().find(|g| g.is_user).unwrap();
        assert_eq!(user.members.len(), 2);
        let other = g
            .iter()
            .find(|g| !g.is_user && g.members.len() == 2)
            .unwrap();
        assert_eq!(other.name, None);
        // Newest first, inside the group and across groups.
        assert_eq!(other.members[0].meeting_id, "m2");
        assert_eq!(g[0].last, "2026-11-03T10:00:00Z");
    }

    #[test]
    fn never_groups_two_speakers_of_one_meeting_or_two_names() {
        let ms = vec![
            meeting(
                "m1",
                "2026-11-01T10:00:00Z",
                vec![sp("Speaker 1", &[1.0, 0.0]), sp("Speaker 2", &[0.99, 0.1])],
            ),
            meeting("m2", "2026-11-02T10:00:00Z", vec![sp("Lea", &[1.0, 0.0])]),
            meeting("m3", "2026-11-03T10:00:00Z", vec![sp("Tim", &[1.0, 0.01])]),
        ];
        let g = group(&ms, "", 0.6);
        for x in &g {
            let mut ids: Vec<&str> = x.members.iter().map(|m| m.meeting_id.as_str()).collect();
            ids.dedup();
            assert_eq!(ids.len(), x.members.len(), "one meeting twice in {x:?}");
        }
        assert!(!g
            .iter()
            .any(|x| x.members.iter().any(|m| m.speaker == "Lea")
                && x.members.iter().any(|m| m.speaker == "Tim")));
    }

    #[test]
    fn a_named_member_names_the_group() {
        let ms = vec![
            meeting(
                "m1",
                "2026-11-01T10:00:00Z",
                vec![sp("Lea Brandt", &[1.0, 0.0])],
            ),
            meeting(
                "m2",
                "2026-11-02T10:00:00Z",
                vec![sp("Speaker 3", &[0.97, 0.1])],
            ),
        ];
        let g = group(&ms, "", 0.6);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].name.as_deref(), Some("Lea Brandt"));
        assert_eq!(g[0].unnamed().count(), 1);
    }

    #[test]
    fn huddle_of_three_proposes_both_names_then_the_last_one_follows() {
        let invitees = vec![
            Invitee {
                name: Some("Robin".into()),
                email: Some("r@example.com".into()),
                is_me: true,
            },
            inv("Lena", "lena@example.com"),
            inv("Jonas", "jonas@example.com"),
        ];
        let speakers = vec![
            sp("Robin", &[1.0]),
            sp("Speaker 2", &[0.5]),
            sp("Speaker 3", &[0.2]),
        ];
        let p = propose(&speakers, &invitees, &[], "Robin", &HashMap::new());
        assert_eq!(p.len(), 2);
        let names: Vec<&str> = p[0].candidates.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Lena", "Jonas"]);
        assert!(
            p.iter().all(|x| x.left.is_none()),
            "two voices, two names: ask"
        );

        // Speaker 2 named Lena: Jonas is left for Speaker 3.
        let speakers = vec![
            sp("Robin", &[1.0]),
            sp("Lena", &[0.5]),
            sp("Speaker 3", &[0.2]),
        ];
        let p = propose(&speakers, &invitees, &[], "Robin", &HashMap::new());
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].left.as_ref().unwrap().name, "Jonas");
        assert_eq!(
            p[0].left.as_ref().unwrap().email.as_deref(),
            Some("jonas@example.com")
        );
    }

    #[test]
    fn a_known_voice_places_one_name_and_leaves_the_other() {
        let invitees = vec![
            inv("Lena", "lena@example.com"),
            inv("Jonas", "jonas@example.com"),
        ];
        let speakers = vec![
            sp("Robin", &[1.0]),
            sp("Speaker 2", &[0.5]),
            sp("Speaker 3", &[0.2]),
        ];
        let voices = HashMap::from([("Speaker 2".to_string(), "Lena".to_string())]);
        let p = propose(&speakers, &invitees, &[], "Robin", &voices);
        let s2 = p.iter().find(|x| x.speaker == "Speaker 2").unwrap();
        assert_eq!(s2.candidates[0].source, Source::Voice);
        assert_eq!(s2.left, None);
        let s3 = p.iter().find(|x| x.speaker == "Speaker 3").unwrap();
        assert_eq!(s3.left.as_ref().unwrap().name, "Jonas");
    }

    #[test]
    fn voice_and_guest_list_agree_on_the_last_one() {
        let invitees = vec![
            inv("Lena", "lena@example.com"),
            inv("Jonas", "jonas@example.com"),
        ];
        let speakers = vec![
            sp("Robin", &[1.0]),
            sp("Lena", &[0.5]),
            sp("Speaker 2", &[0.2]),
        ];
        let voices = HashMap::from([("Speaker 2".to_string(), "Jonas".to_string())]);
        let p = propose(&speakers, &invitees, &[], "Robin", &voices);
        assert_eq!(p[0].left.as_ref().unwrap().name, "Jonas");
        // They disagree: ask.
        let voices = HashMap::from([("Speaker 2".to_string(), "Tim".to_string())]);
        let p = propose(&speakers, &invitees, &[], "Robin", &voices);
        assert_eq!(p[0].left, None);
        assert_eq!(p[0].candidates[0].name, "Tim");
    }

    #[test]
    fn email_names_the_invitee_after_the_known_person() {
        let people = vec![Person {
            name: "Mara Keller".into(),
            emails: vec!["mk@example.com".into()],
        }];
        let invitees = vec![Invitee {
            name: None,
            email: Some("MK@example.com".into()),
            is_me: false,
        }];
        let speakers = vec![sp("Robin", &[1.0]), sp("Speaker 2", &[0.5])];
        let p = propose(&speakers, &invitees, &people, "Robin", &HashMap::new());
        assert_eq!(p[0].left.as_ref().unwrap().name, "Mara Keller");
    }

    #[test]
    fn group_candidates_count_meetings() {
        let g = VoiceGroup {
            id: "x".into(),
            name: None,
            is_user: false,
            last: String::new(),
            members: ["m1", "m2", "m3"]
                .iter()
                .map(|m| Member {
                    meeting_id: m.to_string(),
                    speaker: "Speaker 2".into(),
                })
                .collect(),
        };
        let inv = HashMap::from([
            (
                "m1".to_string(),
                vec![inv("Mara", "m@example.com"), inv("Tim", "t@example.com")],
            ),
            ("m2".to_string(), vec![inv("Mara", "m@example.com")]),
            (
                "m3".to_string(),
                vec![inv("Mara", "m@example.com"), inv("Ole", "o@example.com")],
            ),
        ]);
        let c = group_candidates(&g, &inv, &[], "Robin");
        assert_eq!(c.len(), 1);
        assert_eq!((c[0].0.name.as_str(), c[0].1), ("Mara", 3));
    }

    #[test]
    fn a_guest_known_only_by_address_is_proposed_by_name() {
        assert_eq!(name_from_address("lena.vogt@example.com"), "Lena Vogt");
        assert_eq!(name_from_address("JONAS_weber@example.org"), "Jonas Weber");
        assert_eq!(name_from_address("12345@example.com"), "12345@example.com");
        let guest = Invitee {
            name: Some("mia.sand@example.com".into()),
            email: Some("mia.sand@example.com".into()),
            is_me: false,
        };
        assert_eq!(invitee_name(&guest, &[]).as_deref(), Some("Mia Sand"));
    }
}

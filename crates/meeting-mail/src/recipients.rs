//! Who gets the notes: everyone invited to the event, and the named
//! speakers with a known address (`with_speakers`), except the person
//! themself, people who declined, rooms and resources, and everyone who
//! objected.

use crate::calendar::{Attendee, CalendarEvent, Kind, Status};
use crate::objections::Objections;
use crate::{normalize_email, Recipient};

/// Why an attendee gets no mail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Skipped {
    /// The person using Sokki.
    Me,
    Declined,
    /// On the objection list.
    Objected,
    /// A room, a resource or a group.
    NotAPerson,
    /// The calendar has no address for them.
    NoAddress,
}

/// The recipients of one event, and who was left out and why.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct Recipients {
    pub to: Vec<Recipient>,
    pub skipped: Vec<(String, Skipped)>,
}

/// The recipients for `event`. `own` are the person's own addresses (lower
/// case), from the mail app and the calendar.
pub fn recipients(event: &CalendarEvent, own: &[String], objections: &Objections) -> Recipients {
    let mut out = Recipients::default();
    let everyone = event.organizer.iter().chain(event.attendees.iter());
    for a in everyone {
        let label = || {
            a.name
                .clone()
                .or_else(|| a.email.clone())
                .unwrap_or_default()
        };
        let Some(email) = a.email.as_deref().and_then(normalize_email) else {
            out.skipped.push((label(), Skipped::NoAddress));
            continue;
        };
        if out.to.iter().any(|r| r.email == email) || out.skipped.iter().any(|(e, _)| *e == email) {
            continue;
        }
        let skip = if a.is_me || own.contains(&email) {
            Some(Skipped::Me)
        } else if is_not_a_person(a, &email) {
            Some(Skipped::NotAPerson)
        } else if a.status == Status::Declined {
            Some(Skipped::Declined)
        } else if objections.contains(&email) {
            Some(Skipped::Objected)
        } else {
            None
        };
        match skip {
            Some(why) => out.skipped.push((email, why)),
            None => out.to.push(Recipient {
                name: a.name.clone().filter(|n| {
                    let n = n.trim();
                    !n.is_empty() && normalize_email(n).as_deref() != Some(email.as_str())
                }),
                email,
            }),
        }
    }
    out
}

/// `event` plus the meeting's named speakers with a known address (Robin,
/// 2026-11-26: "when we have identified the speakers and know their email
/// addresses, we can still send them the summary"). A speaker already in
/// the event counts as there, even if they declined. Without an event, a
/// meeting with such speakers gets one of its own (`title`, `start`, `end`);
/// `None` when there is neither.
pub fn with_speakers(
    event: Option<CalendarEvent>,
    speakers: &[(String, String)],
    title: &str,
    start: f64,
    end: f64,
) -> Option<CalendarEvent> {
    let speakers: Vec<(&str, String)> = speakers
        .iter()
        .filter_map(|(n, e)| normalize_email(e).map(|e| (n.as_str(), e)))
        .collect();
    if event.is_none() && speakers.is_empty() {
        return None;
    }
    let mut event = event.unwrap_or_else(|| CalendarEvent {
        title: title.to_string(),
        start,
        end,
        all_day: false,
        url: None,
        location: None,
        notes: None,
        organizer: None,
        attendees: Vec::new(),
        account: None,
    });
    for (name, email) in speakers {
        let known = event
            .organizer
            .iter_mut()
            .chain(event.attendees.iter_mut())
            .find(|a| {
                a.email.as_deref().and_then(normalize_email).as_deref() == Some(email.as_str())
            });
        match known {
            Some(a) => {
                if a.status == Status::Declined {
                    a.status = Status::Accepted;
                }
            }
            None => event.attendees.push(Attendee {
                name: Some(name.to_string()),
                email: Some(email),
                is_me: false,
                status: Status::Accepted,
                kind: Kind::Person,
            }),
        }
    }
    Some(event)
}

/// The address of the invitee called `name`, from `invited` (name, address):
/// the same full name, else the only one whose first name (or the first part
/// of the address, as in "lena.vogt@") is the name's first word. Case
/// and accents don't count ("Jürgen" is "jurgen."). `None` when nobody
/// or more than one matches: a wrong guess would mail the wrong person.
pub fn address_for(name: &str, invited: &[(Option<String>, Option<String>)]) -> Option<String> {
    let fold = |s: &str| -> String {
        s.to_lowercase()
            .chars()
            .map(|c| match c {
                'ä' | 'á' | 'à' | 'â' | 'å' => 'a',
                'ö' | 'ó' | 'ò' | 'ô' | 'ø' => 'o',
                'ü' | 'ú' | 'ù' | 'û' => 'u',
                'é' | 'è' | 'ê' | 'ë' => 'e',
                'í' | 'ì' | 'î' | 'ï' | 'ı' => 'i',
                'ç' | 'ć' | 'č' => 'c',
                'ş' | 'ś' | 'š' => 's',
                'ñ' | 'ń' => 'n',
                'ğ' => 'g',
                'ł' => 'l',
                'ž' | 'ź' | 'ż' => 'z',
                c => c,
            })
            .collect::<String>()
            .replace('ß', "ss")
    };
    let first = |s: &str| -> String {
        fold(s)
            .split(|c: char| !c.is_alphanumeric())
            .find(|w| !w.is_empty())
            .unwrap_or_default()
            .to_string()
    };
    let want_full = fold(name.trim());
    let want_first = first(name);
    if want_first.is_empty() {
        return None;
    }
    let people: Vec<(Option<&str>, String)> = invited
        .iter()
        .filter_map(|(n, e)| Some((n.as_deref(), normalize_email(e.as_deref()?)?)))
        .collect();
    let unique = |hits: Vec<&String>| -> Option<String> {
        let mut hits = hits;
        hits.dedup();
        (hits.len() == 1).then(|| hits[0].clone())
    };
    let full: Vec<&String> = people
        .iter()
        .filter(|(n, _)| n.is_some_and(|n| fold(n.trim()) == want_full))
        .map(|(_, e)| e)
        .collect();
    if !full.is_empty() {
        return unique(full);
    }
    unique(
        people
            .iter()
            .filter(|(n, e)| {
                n.is_some_and(|n| first(n) == want_first)
                    || first(e.split('@').next().unwrap_or_default()) == want_first
            })
            .map(|(_, e)| e)
            .collect(),
    )
}

/// Rooms and resources, by the calendar's kind, and Google's resource
/// calendars, which come as people.
fn is_not_a_person(a: &Attendee, email: &str) -> bool {
    matches!(a.kind, Kind::Room | Kind::Resource | Kind::Group)
        || email.ends_with("@resource.calendar.google.com")
        || email.ends_with("@group.calendar.google.com")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::tests::{event, person};
    use crate::objections::{Objection, Source};

    #[test]
    fn me_declined_rooms_and_objectors_get_nothing() {
        let mut e = event("x", 0.0, 1.0, None);
        let mut me = person("Mira", "mira@example.com");
        me.is_me = true;
        let mut declined = person("Dora", "dora@example.com");
        declined.status = Status::Declined;
        let mut room = person("Raum 1", "c_123@resource.calendar.google.com");
        room.kind = Kind::Person;
        let mut tentative = person("Tim", "tim@example.com");
        tentative.status = Status::Tentative;
        e.organizer = Some(me.clone());
        e.attendees = vec![
            me,
            person("Anna", "Anna@Example.com"),
            declined,
            room,
            person("Olaf", "olaf@example.com"),
            tentative,
            person("Lena (other)", "lena.berg@example.com"),
            Attendee {
                name: Some("Nobody".into()),
                email: None,
                ..person("", "")
            },
            person("Anna again", "anna@example.com"),
        ];
        let objections = Objections::in_memory(vec![Objection {
            email: "olaf@example.com".into(),
            added: "2026-11-24".into(),
            source: Source::Reply,
        }]);
        let r = recipients(&e, &["lena.berg@example.com".into()], &objections);
        let to: Vec<&str> = r.to.iter().map(|r| r.email.as_str()).collect();
        assert_eq!(to, ["anna@example.com", "tim@example.com"]);
        let why: Vec<Skipped> = r.skipped.iter().map(|(_, w)| *w).collect();
        assert_eq!(
            why,
            [
                Skipped::Me,
                Skipped::Declined,
                Skipped::NotAPerson,
                Skipped::Objected,
                Skipped::Me,
                Skipped::NoAddress
            ]
        );
    }

    #[test]
    fn a_name_that_is_just_the_address_is_dropped() {
        let mut e = event("x", 0.0, 1.0, None);
        e.attendees = vec![person("anna@example.com", "anna@example.com")];
        let r = recipients(&e, &[], &Objections::default());
        assert_eq!(r.to[0].name, None);
    }
    #[test]
    fn named_speakers_with_an_address_get_the_notes_too() {
        let mut declined = person("Dora", "dora@example.com");
        declined.status = Status::Declined;
        let mut e = event("Weekly", 0.0, 1.0, None);
        e.attendees.push(declined);
        let speakers = vec![
            ("Dora".to_string(), "Dora@Example.com".to_string()),
            ("Olaf".to_string(), "olaf@kunde.example".to_string()),
            ("Anna".to_string(), "anna@example.com".to_string()),
            ("Kein".to_string(), "keine-adresse".to_string()),
        ];
        let e = with_speakers(Some(e), &speakers, "x", 0.0, 0.0).unwrap();
        let list = recipients(&e, &[], &Objections::default());
        let to: Vec<&str> = list.to.iter().map(|r| r.email.as_str()).collect();
        // Anna once, Dora spoke so she counts as there, Olaf added.
        assert_eq!(
            to,
            ["anna@example.com", "dora@example.com", "olaf@kunde.example"]
        );
        assert_eq!(e.title, "Weekly");
    }

    #[test]
    fn a_meeting_without_an_event_goes_to_its_named_speakers() {
        assert!(with_speakers(None, &[], "Kaffee", 0.0, 1.0).is_none());
        let speakers = vec![("Olaf".to_string(), "olaf@kunde.example".to_string())];
        let e = with_speakers(None, &speakers, "Kaffee", 10.0, 20.0).unwrap();
        assert_eq!((e.title.as_str(), e.start, e.end), ("Kaffee", 10.0, 20.0));
        let list = recipients(&e, &[], &Objections::default());
        assert_eq!(list.to.len(), 1);
        assert_eq!(list.to[0].name.as_deref(), Some("Olaf"));
        let objections = Objections::in_memory(vec![Objection {
            email: "olaf@kunde.example".into(),
            added: "2026-11-26".into(),
            source: Source::ByHand,
        }]);
        assert!(recipients(&e, &[], &objections).to.is_empty());
    }
    #[test]
    fn a_speaker_finds_their_address_on_the_invite() {
        let invited = vec![
            (
                None,
                Some("mailto:juergen.kaya@firma.example".to_string()),
            ),
            (None, Some("jurgen.kaya@firma.example".to_string())),
            (None, Some("jonas.berg@firma.example".to_string())),
            (
                Some("Mara Nowak".to_string()),
                Some("mn@firma.example".to_string()),
            ),
            (
                Some("Anna Roth".to_string()),
                Some("anna.roth@firma.example".to_string()),
            ),
            (
                Some("Anna Weiß".to_string()),
                Some("anna.weiss@firma.example".to_string()),
            ),
        ];
        assert_eq!(
            address_for("Jürgen", &invited).as_deref(),
            Some("jurgen.kaya@firma.example")
        );
        assert_eq!(
            address_for("Jonas", &invited).as_deref(),
            Some("jonas.berg@firma.example")
        );
        assert_eq!(
            address_for("mara", &invited).as_deref(),
            Some("mn@firma.example")
        );
        // Two Annas: only the full name decides.
        assert_eq!(address_for("Anna", &invited), None);
        assert_eq!(
            address_for("Anna Weiss", &invited).as_deref(),
            Some("anna.weiss@firma.example")
        );
        assert_eq!(address_for("Olaf", &invited), None);
        assert_eq!(address_for("", &invited), None);
    }
}

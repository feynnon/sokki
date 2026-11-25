//! Who gets the notes: everyone invited to the event, except the person
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
}

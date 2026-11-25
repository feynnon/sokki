//! Calendar events as plain data, and which event a recording belongs to.
//!
//! The host reads the events from the platform's calendar (macOS: EventKit
//! in `src-tauri/swift/calendar.swift`, which returns exactly this shape as
//! JSON). Everything here is pure.

use serde::{Deserialize, Serialize};

/// One calendar event (one occurrence of a repeating one).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalendarEvent {
    pub title: String,
    /// Unix seconds.
    pub start: f64,
    pub end: f64,
    #[serde(default)]
    pub all_day: bool,
    /// The event's URL field (often the meeting link).
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub organizer: Option<Attendee>,
    #[serde(default)]
    pub attendees: Vec<Attendee>,
    /// The calendar's account, e.g. "mira@example.com", when known.
    #[serde(default)]
    pub account: Option<String>,
}

/// Someone invited to an event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attendee {
    #[serde(default)]
    pub name: Option<String>,
    /// From the participant's `mailto:` URL, as the calendar gives it.
    #[serde(default)]
    pub email: Option<String>,
    /// The calendar marks this attendee as the person using Sokki.
    #[serde(default)]
    pub is_me: bool,
    #[serde(default)]
    pub status: Status,
    #[serde(default)]
    pub kind: Kind,
}

/// The attendee's answer to the invitation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Unknown,
    Pending,
    Accepted,
    Declined,
    Tentative,
    Delegated,
    Completed,
    InProcess,
}

/// What the attendee is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Unknown,
    Person,
    Room,
    Resource,
    Group,
}

/// Video-call services a link or an app can belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    Zoom,
    Meet,
    Teams,
    Webex,
    Slack,
    GoTo,
    Chime,
    Whereby,
}

const LINK_HOSTS: &[(&str, Service)] = &[
    ("zoom.us", Service::Zoom),
    ("zoom.com", Service::Zoom),
    ("meet.google.com", Service::Meet),
    ("teams.microsoft.com", Service::Teams),
    ("teams.live.com", Service::Teams),
    ("webex.com", Service::Webex),
    ("slack.com", Service::Slack),
    ("gotomeeting.com", Service::GoTo),
    ("goto.com", Service::GoTo),
    ("chime.aws", Service::Chime),
    ("whereby.com", Service::Whereby),
];

/// Meeting links in the event: its URL, location and notes.
pub fn meeting_links(event: &CalendarEvent) -> Vec<(Service, String)> {
    let mut out: Vec<(Service, String)> = Vec::new();
    for text in [&event.url, &event.location, &event.notes]
        .into_iter()
        .flatten()
    {
        for word in text.split(|c: char| c.is_whitespace() || "<>\"'()[]".contains(c)) {
            let Some(at) = word.find("https://").or_else(|| word.find("http://")) else {
                continue;
            };
            let link = word[at..].trim_end_matches(['.', ',', ';']);
            if let Some(service) = link_service(link) {
                if !out.iter().any(|(_, l)| l == link) {
                    out.push((service, link.to_string()));
                }
            }
        }
    }
    out
}

/// The service a link belongs to, by its host.
pub fn link_service(link: &str) -> Option<Service> {
    let rest = link.split_once("://").map(|(_, r)| r).unwrap_or(link);
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .to_lowercase();
    let host = host.rsplit_once('@').map(|(_, h)| h).unwrap_or(&host);
    let host = host.split(':').next().unwrap_or(host);
    LINK_HOSTS
        .iter()
        .find(|(h, _)| host == *h || host.ends_with(&format!(".{h}")))
        .map(|(_, s)| *s)
}

/// Which services a meeting app can hold: the app's own, or for a browser
/// every web call. `None`: unknown app, any service.
pub fn app_services(source_app: &str) -> Option<&'static [Service]> {
    const WEB: &[Service] = &[
        Service::Meet,
        Service::Zoom,
        Service::Teams,
        Service::Webex,
        Service::Whereby,
        Service::GoTo,
        Service::Chime,
        Service::Slack,
    ];
    let app = source_app.to_lowercase();
    let browsers = [
        "chrome", "arc", "dia", "edge", "brave", "vivaldi", "opera", "firefox", "chromium",
        "safari",
    ];
    Some(match () {
        _ if app.contains("zoom") => &[Service::Zoom],
        _ if app.contains("teams") => &[Service::Teams],
        _ if app.contains("webex") => &[Service::Webex],
        _ if app.contains("slack") => &[Service::Slack],
        _ if app.contains("goto") => &[Service::GoTo],
        _ if app.contains("chime") => &[Service::Chime],
        _ if browsers.iter().any(|b| app.contains(b)) => WEB,
        _ => return None,
    })
}

/// The event a recording from `start` to `end` (unix seconds) belongs to:
/// among timed events that overlap the recording and have attendees, the
/// ones with a link for the recording's app if there are any, and of those
/// the one that overlaps most. `None` when no event overlaps.
pub fn find_event<'a>(
    events: &'a [CalendarEvent],
    start: f64,
    end: f64,
    source_app: Option<&str>,
) -> Option<&'a CalendarEvent> {
    let end = end.max(start);
    let overlap = |e: &CalendarEvent| e.end.min(end) - e.start.max(start);
    let candidates: Vec<&CalendarEvent> = events
        .iter()
        .filter(|e| !e.all_day && !e.attendees.is_empty())
        // A recording of zero length still belongs to the event it lies in.
        .filter(|e| overlap(e) > 0.0 || (e.start <= start && start < e.end))
        .collect();
    let services = source_app.and_then(app_services);
    let linked: Vec<&CalendarEvent> = match services {
        Some(s) => candidates
            .iter()
            .copied()
            .filter(|e| meeting_links(e).iter().any(|(svc, _)| s.contains(svc)))
            .collect(),
        None => Vec::new(),
    };
    let pool = if linked.is_empty() {
        candidates
    } else {
        linked
    };
    pool.into_iter()
        .max_by(|a, b| overlap(a).total_cmp(&overlap(b)))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn person(name: &str, email: &str) -> Attendee {
        Attendee {
            name: Some(name.into()),
            email: Some(format!("mailto:{email}")),
            is_me: false,
            status: Status::Accepted,
            kind: Kind::Person,
        }
    }

    pub fn event(title: &str, start: f64, end: f64, url: Option<&str>) -> CalendarEvent {
        CalendarEvent {
            title: title.into(),
            start,
            end,
            all_day: false,
            url: url.map(Into::into),
            location: None,
            notes: None,
            organizer: None,
            attendees: vec![person("Anna", "anna@example.com")],
            account: None,
        }
    }

    #[test]
    fn links_are_found_in_url_location_and_notes() {
        let mut e = event("x", 0.0, 1.0, Some("https://us02web.zoom.us/j/123?pwd=abc"));
        e.location = Some("Microsoft Teams Meeting".into());
        e.notes = Some(
            "Join: <https://teams.microsoft.com/l/meetup-join/19%3a>. Or https://meet.google.com/abc-defg-hij."
                .into(),
        );
        let links = meeting_links(&e);
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].0, Service::Zoom);
        assert_eq!(links[1].0, Service::Teams);
        assert_eq!(
            links[2],
            (Service::Meet, "https://meet.google.com/abc-defg-hij".into())
        );
    }

    #[test]
    fn a_lookalike_host_is_no_link() {
        assert_eq!(link_service("https://zoom.us.evil.com/j/1"), None);
        assert_eq!(link_service("https://notzoom.us/j/1"), None);
        assert_eq!(
            link_service("https://acme.zoom.us/j/1"),
            Some(Service::Zoom)
        );
    }

    #[test]
    fn the_overlapping_event_is_picked() {
        let events = vec![
            event("before", 0.0, 1000.0, None),
            event("this", 1800.0, 3600.0, None),
            event("after", 4000.0, 5000.0, None),
        ];
        let e = find_event(&events, 1900.0, 3500.0, None).unwrap();
        assert_eq!(e.title, "this");
        assert!(find_event(&events, 1100.0, 1700.0, None).is_none());
    }

    #[test]
    fn with_two_events_at_once_the_one_with_the_apps_link_wins() {
        let events = vec![
            event(
                "meet",
                0.0,
                3600.0,
                Some("https://meet.google.com/abc-defg-hij"),
            ),
            event("zoom", 0.0, 1800.0, Some("https://zoom.us/j/1")),
        ];
        assert_eq!(
            find_event(&events, 60.0, 1700.0, Some("Zoom"))
                .unwrap()
                .title,
            "zoom"
        );
        // A browser can hold either: by overlap.
        assert_eq!(
            find_event(&events, 60.0, 3000.0, Some("Google Chrome"))
                .unwrap()
                .title,
            "meet"
        );
        // No app known: the bigger overlap.
        assert_eq!(
            find_event(&events, 60.0, 3000.0, None).unwrap().title,
            "meet"
        );
        // The app's link is in no event: by overlap.
        assert_eq!(
            find_event(&events, 60.0, 3000.0, Some("Webex"))
                .unwrap()
                .title,
            "meet"
        );
    }

    #[test]
    fn all_day_events_and_events_without_attendees_are_skipped() {
        let mut all_day = event("holiday", 0.0, 86400.0, None);
        all_day.all_day = true;
        let mut alone = event("focus", 0.0, 3600.0, None);
        alone.attendees.clear();
        assert!(find_event(&[all_day, alone], 60.0, 600.0, None).is_none());
    }

    #[test]
    fn the_events_json_from_the_host_parses() {
        let json = r#"[{"title":"Weekly","start":1764000000,"end":1764003600,"all_day":false,
            "url":null,"location":"https://meet.google.com/abc","notes":null,
            "organizer":{"name":"Mira","email":"mailto:mira@example.com","is_me":true,"status":"accepted","kind":"person"},
            "attendees":[{"name":"Anna","email":"mailto:anna@example.com","is_me":false,"status":"tentative","kind":"person"},
                         {"name":"Room 1","email":"mailto:room@resource.calendar.google.com","status":"accepted","kind":"room"}],
            "account":"mira@example.com"}]"#;
        let events: Vec<CalendarEvent> = serde_json::from_str(json).unwrap();
        assert_eq!(events[0].attendees.len(), 2);
        assert_eq!(events[0].attendees[0].status, Status::Tentative);
        assert_eq!(events[0].attendees[1].kind, Kind::Room);
        assert!(events[0].organizer.as_ref().unwrap().is_me);
    }
}

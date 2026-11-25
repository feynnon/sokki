//! meeting-mail: send a meeting's notes to everyone who was in it, from the
//! person's own mailbox and in their name. Robin, 2026-11-24: "Können wir
//! die mail im Namen des sokki users verschicken? Und er kann die
//! Einstellung ein oder abschalten?"
//!
//! The parts, each usable on its own (board card `pre-meeting-mail` reuses
//! the calendar and the sending):
//!
//! - [`calendar`]: a calendar event and its attendees as plain data, and
//!   [`calendar::find_event`], which picks the event a recording belongs to
//!   by time and meeting link. The host reads the events (macOS: EventKit,
//!   `src-tauri/swift/calendar.swift`).
//! - [`recipients`]: who gets the mail: the attendees minus the person
//!   themself, people who declined, rooms, and everyone on the objection list.
//! - [`objections`]: the local list of people who asked for no more mail
//!   (BGH VI ZR 134/15: after an objection, no more mail).
//! - [`compose`]: the mail for a note: its summary, decisions and action
//!   items as plain text, no transcript, ending with the note's Sokki line.
//! - [`apple_mail`] (macOS): sends through the person's Apple Mail with
//!   AppleScript, and reads "stop" replies.
//! - [`gmail`]: sends through the person's Gmail (`gmail.send` only).
//!
//! The only network code is [`gmail`]: Google's sign-in and one send call,
//! straight from the computer. Nothing goes to a Sokki server.

pub mod calendar;
pub mod compose;
pub mod gmail;
pub mod objections;
pub mod recipients;

#[cfg(target_os = "macos")]
pub mod apple_mail;

use serde::{Deserialize, Serialize};
use std::fmt;

/// One address a mail goes to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recipient {
    pub name: Option<String>,
    pub email: String,
}

/// A mail ready to send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mail {
    /// The sending address, one of the mail app's accounts. `None`: the
    /// mail app's default account.
    pub from: Option<String>,
    pub to: Vec<Recipient>,
    pub subject: String,
    /// Plain text.
    pub body: String,
}

/// Why a mail could not be sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    /// The mail app has no account to send from.
    NoAccount,
    /// macOS did not allow Sokki to control the mail app (Privacy &
    /// Security › Automation).
    NotAllowed,
    /// The mail app did not answer in time.
    Timeout,
    /// Anything else, with the mail app's own message.
    Failed(String),
    /// No mail app support on this platform.
    Unsupported,
    /// Gmail needs connecting again (the token was revoked or expired).
    SignIn,
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SendError::NoAccount => write!(f, "Mail has no account to send from"),
            SendError::NotAllowed => write!(
                f,
                "Sokki may not control Mail (Privacy & Security › Automation)"
            ),
            SendError::Timeout => write!(f, "Mail did not answer in time"),
            SendError::Failed(m) => write!(f, "Mail could not send: {m}"),
            SendError::Unsupported => write!(f, "Sending mail is not supported on this computer"),
            SendError::SignIn => write!(f, "Gmail needs you to connect again"),
        }
    }
}

impl std::error::Error for SendError {}

/// Something that sends mail from the person's own mailbox: Apple Mail
/// today; Gmail (`gmail.send`) once Google's review is through.
pub trait MailSender {
    /// The addresses the person sends from, lower case. Used to keep the
    /// person out of their own recipients and to pick the sender.
    fn own_addresses(&self) -> Result<Vec<String>, SendError>;
    /// Sends one mail.
    fn send(&self, mail: &Mail) -> Result<(), SendError>;
}

/// Lower case, trimmed, without a `mailto:` prefix or `?query`. `None` when
/// it does not look like an address.
pub fn normalize_email(s: &str) -> Option<String> {
    let s = s.trim();
    let s = s
        .strip_prefix("mailto:")
        .or_else(|| s.strip_prefix("MAILTO:"))
        .unwrap_or(s);
    let s = s.split('?').next().unwrap_or(s);
    let s = percent_decode(s).trim().to_lowercase();
    let (local, domain) = s.split_once('@')?;
    let ok = !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !s.contains(char::is_whitespace)
        && !s.contains(['<', '>', ',', ';', '"'])
        && s.matches('@').count() == 1;
    ok.then_some(s)
}

/// `%40` → `@` and friends, as calendar URLs carry them.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(b) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// "Name <addr>" or a bare address → (name, address).
pub fn parse_mailbox(s: &str) -> Option<Recipient> {
    let s = s.trim();
    if let (Some(open), Some(close)) = (s.rfind('<'), s.rfind('>')) {
        if open < close {
            let email = normalize_email(&s[open + 1..close])?;
            let name = s[..open].trim().trim_matches('"').trim();
            return Some(Recipient {
                name: (!name.is_empty()).then(|| name.to_string()),
                email,
            });
        }
    }
    normalize_email(s).map(|email| Recipient { name: None, email })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_normalized() {
        assert_eq!(
            normalize_email("mailto:Anna@Example.COM").as_deref(),
            Some("anna@example.com")
        );
        assert_eq!(
            normalize_email("mailto:anna%40example.com").as_deref(),
            Some("anna@example.com")
        );
        assert_eq!(
            normalize_email(" bob@x.de?subject=hi").as_deref(),
            Some("bob@x.de")
        );
        assert_eq!(normalize_email("not an address"), None);
        assert_eq!(normalize_email("a@b"), None);
        assert_eq!(normalize_email("a@@b.de"), None);
        assert_eq!(normalize_email("ends%"), None);
    }

    #[test]
    fn mailboxes_are_parsed() {
        assert_eq!(
            parse_mailbox("\"Anna Muster\" <Anna@Example.com>"),
            Some(Recipient {
                name: Some("Anna Muster".into()),
                email: "anna@example.com".into()
            })
        );
        assert_eq!(
            parse_mailbox("bob@x.de"),
            Some(Recipient {
                name: None,
                email: "bob@x.de".into()
            })
        );
    }
}

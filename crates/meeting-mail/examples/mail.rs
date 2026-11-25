//! Try the parts by hand.
//!
//! ```text
//! cargo run --example mail -- preview <note.md> <title>
//! cargo run --example mail -- match <events.json> <start_unix> <end_unix> [app]
//! cargo run --example mail -- accounts                     # asks Mail (macOS)
//! cargo run --example mail -- send <note.md> <title> <to>  # sends a real mail (macOS)
//! cargo run --example mail -- gmail-send <note.md> <title> <to>
//! ```
//!
//! `gmail-send` reads `SOKKI_GOOGLE_CLIENT_ID` and `SOKKI_GOOGLE_CLIENT_SECRET`
//! from the environment, opens Google's consent page in the browser, sends
//! one mail through Gmail, then revokes the token again.
//!
//! `accounts` and `send` talk to Apple Mail: macOS asks once whether the
//! terminal may control Mail. Send only to your own test address.

use meeting_mail::calendar::{find_event, CalendarEvent};
use meeting_mail::compose::summary_mail;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).cloned().unwrap_or_else(|| usage());
    match args.first().map(String::as_str) {
        Some("preview") => {
            let md = std::fs::read_to_string(arg(1)).expect("note");
            match summary_mail(&md, &arg(2)) {
                Some(m) => println!("Subject: {}\n\n{}", m.subject, m.body),
                None => println!("(no summary: nothing to send)"),
            }
        }
        Some("match") => {
            let events: Vec<CalendarEvent> =
                serde_json::from_str(&std::fs::read_to_string(arg(1)).expect("events"))
                    .expect("json");
            let start: f64 = arg(2).parse().expect("start");
            let end: f64 = arg(3).parse().expect("end");
            match find_event(&events, start, end, args.get(4).map(String::as_str)) {
                Some(e) => println!("{}", serde_json::to_string_pretty(e).unwrap()),
                None => println!("(no event)"),
            }
        }
        #[cfg(target_os = "macos")]
        Some("accounts") => {
            use meeting_mail::apple_mail::AppleMail;
            match AppleMail.accounts() {
                Ok(a) => a
                    .iter()
                    .for_each(|a| println!("{}\t{}", a.email, a.full_name)),
                Err(e) => eprintln!("{e}"),
            }
        }
        #[cfg(target_os = "macos")]
        Some("send") => {
            use meeting_mail::{apple_mail::AppleMail, Mail, MailSender, Recipient};
            let md = std::fs::read_to_string(arg(1)).expect("note");
            let m = summary_mail(&md, &arg(2)).expect("the note has no summary");
            let mail = Mail {
                from: None,
                to: vec![Recipient {
                    name: None,
                    email: arg(3),
                }],
                subject: m.subject,
                body: m.body,
            };
            let t = std::time::Instant::now();
            match AppleMail.send(&mail) {
                Ok(()) => println!("sent in {:.1} s", t.elapsed().as_secs_f32()),
                Err(e) => eprintln!("{e}"),
            }
        }
        Some("gmail-send") => {
            use meeting_mail::gmail::{self, Client, Gmail, TokenStore};
            use meeting_mail::{Mail, MailSender, Recipient};
            use std::sync::Mutex;
            struct Memory(Mutex<Option<String>>);
            impl TokenStore for Memory {
                fn get(&self) -> Option<String> {
                    self.0.lock().unwrap().clone()
                }
                fn set(&self, t: &str) -> Result<(), String> {
                    *self.0.lock().unwrap() = Some(t.into());
                    Ok(())
                }
                fn delete(&self) {
                    *self.0.lock().unwrap() = None;
                }
            }
            let client = Client {
                id: std::env::var("SOKKI_GOOGLE_CLIENT_ID").expect("SOKKI_GOOGLE_CLIENT_ID"),
                secret: std::env::var("SOKKI_GOOGLE_CLIENT_SECRET").unwrap_or_default(),
            };
            let md = std::fs::read_to_string(arg(1)).expect("note");
            let m = summary_mail(&md, &arg(2)).expect("the note has no summary");
            println!("Opening Google's consent page …");
            // As the app's "Connect Gmail" asks (send only). The page opens
            // in the default browser; SOKKI_PRINT_URL=1 prints it instead.
            let signed = gmail::sign_in_with(&client, &[], |url| {
                if std::env::var_os("SOKKI_PRINT_URL").is_some() {
                    println!("CONSENT_URL {url}");
                } else {
                    let _ = std::process::Command::new("open").arg(url).spawn();
                }
            })
            .unwrap_or_else(|e| {
                eprintln!("{e}");
                std::process::exit(1)
            });
            println!("Connected as {}", signed.email.as_deref().unwrap_or("?"));
            let store = Memory(Mutex::new(Some(signed.refresh_token.clone())));
            let g = Gmail::new(client, store, signed.email.clone());
            let mail = Mail {
                from: None,
                to: vec![Recipient {
                    name: None,
                    email: arg(3),
                }],
                subject: m.subject,
                body: m.body,
            };
            let t = std::time::Instant::now();
            match g.send(&mail) {
                Ok(()) => println!("sent in {:.1} s", t.elapsed().as_secs_f32()),
                Err(e) => eprintln!("{e}"),
            }
            match gmail::revoke(&signed.refresh_token) {
                Ok(()) => println!("token revoked"),
                Err(e) => eprintln!("revoke: {e}"),
            }
        }
        _ => usage(),
    }
}

fn usage() -> ! {
    eprintln!("usage: mail preview <note.md> <title> | match <events.json> <start> <end> [app] | accounts | send <note.md> <title> <to> | gmail-send <note.md> <title> <to>");
    std::process::exit(2)
}

//! Sending through the person's Apple Mail, with AppleScript (`osascript`,
//! a child of Sokki, so macOS asks once whether Sokki may control Mail:
//! Privacy & Security › Automation; Sokki's Info.plist carries
//! `NSAppleEventsUsageDescription` and its entitlements
//! `com.apple.security.automation.apple-events`).
//!
//! The message is made with `visible:false` and sent at once, so no compose
//! window opens. Mail opens if it was closed; Sokki then quits it again once
//! the outbox is empty, so it is left as it was.
//!
//! Every value goes to the script as an argument (`on run argv`), never
//! pasted into the script text, so no subject or name can change the script.

use crate::{parse_mailbox, Mail, MailSender, SendError};
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long one script may take. Sending waits for the outbox, which can
/// take a while on a slow connection.
const TIMEOUT: Duration = Duration::from_secs(90);

const SEND: &[&str] = &[
    "on run argv",
    "  set theSubject to item 1 of argv",
    "  set theBody to item 2 of argv",
    "  set theSender to item 3 of argv",
    "  set quitAfter to item 4 of argv",
    "  tell application \"Mail\"",
    "    if (count of accounts) is 0 then error \"no account\" number 9001",
    "    set msg to make new outgoing message with properties {subject:theSubject, content:theBody, visible:false}",
    "    if theSender is not \"\" then set sender of msg to theSender",
    "    set i to 5",
    "    repeat while i < (count of argv)",
    "      set theName to item i of argv",
    "      set theAddress to item (i + 1) of argv",
    "      tell msg",
    "        if theName is \"\" then",
    "          make new to recipient at end of to recipients with properties {address:theAddress}",
    "        else",
    "          make new to recipient at end of to recipients with properties {name:theName, address:theAddress}",
    "        end if",
    "      end tell",
    "      set i to i + 2",
    "    end repeat",
    "    set ok to send msg",
    "    if not ok then error \"Mail refused to send\" number 9002",
    "    if quitAfter is \"1\" then",
    "      repeat 60 times",
    "        if (count of messages of outbox) is 0 then exit repeat",
    "        delay 1",
    "      end repeat",
    "      quit",
    "    end if",
    "  end tell",
    "  return \"sent\"",
    "end run",
];

const ACCOUNTS: &[&str] = &[
    "on run argv",
    "  set out to \"\"",
    "  tell application \"Mail\"",
    "    repeat with a in accounts",
    "      if enabled of a then",
    "        set fullName to full name of a",
    "        repeat with addr in (email addresses of a)",
    "          set out to out & addr & tab & fullName & linefeed",
    "        end repeat",
    "      end if",
    "    end repeat",
    "  end tell",
    "  return out",
    "end run",
];

const REPLIES: &[&str] = &[
    "on run argv",
    "  set cutoff to (current date) - ((item 1 of argv) as integer)",
    "  set prefixes to rest of argv",
    "  set out to \"\"",
    "  tell application \"Mail\"",
    "    set found to (messages of inbox whose date received > cutoff)",
    "    repeat with m in found",
    "      set s to subject of m",
    "      set hit to false",
    "      repeat with p in prefixes",
    "        if s contains (p as text) then set hit to true",
    "      end repeat",
    "      if hit then",
    "        set firstLine to \"\"",
    "        repeat with para in paragraphs of (content of m)",
    "          if length of (para as text) > 0 then",
    "            set firstLine to para as text",
    "            exit repeat",
    "          end if",
    "        end repeat",
    "        set out to out & (sender of m) & tab & s & tab & firstLine & linefeed",
    "      end if",
    "    end repeat",
    "  end tell",
    "  return out",
    "end run",
];

/// Apple Mail as a [`MailSender`].
#[derive(Debug, Default, Clone, Copy)]
pub struct AppleMail;

/// One of Mail's sending addresses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// Lower case.
    pub email: String,
    pub full_name: String,
}

impl AppleMail {
    /// Mail's enabled accounts' addresses with the account's full name.
    pub fn accounts(&self) -> Result<Vec<Account>, SendError> {
        let was_running = mail_running();
        let out = run(ACCOUNTS, &[]);
        if !was_running {
            quit_mail();
        }
        Ok(out?
            .lines()
            .filter_map(|l| {
                let (email, name) = l.split_once('\t').unwrap_or((l, ""));
                crate::normalize_email(email).map(|email| Account {
                    email,
                    full_name: name.trim().to_string(),
                })
            })
            .collect())
    }

    /// Mails received in the last `since` seconds whose subject contains
    /// one of `subject_contains` (replies to Sokki's mails: "Re: Notizen: …").
    pub fn replies(
        &self,
        since: Duration,
        subject_contains: &[String],
    ) -> Result<Vec<Reply>, SendError> {
        let mut args = vec![since.as_secs().to_string()];
        args.extend(subject_contains.iter().map(|p| p.trim().to_string()));
        let was_running = mail_running();
        let out = run(REPLIES, &args);
        if !was_running {
            quit_mail();
        }
        Ok(out?.lines().filter_map(parse_reply_line).collect())
    }

    /// Senders of replies to Sokki's notes mails, received in the last
    /// `since` seconds, whose first line says "stop" ([`crate::objections::is_stop_reply`]).
    pub fn stop_replies(&self, since: Duration) -> Result<Vec<String>, SendError> {
        let prefixes: Vec<String> = crate::compose::SUBJECT_PREFIXES
            .iter()
            .map(|p| p.trim().to_string())
            .collect();
        let mut senders = Vec::new();
        for reply in self.replies(since, &prefixes)? {
            if crate::objections::is_stop_reply(&reply.first_line) {
                if let Some(r) = parse_mailbox(&reply.from) {
                    if !senders.contains(&r.email) {
                        senders.push(r.email);
                    }
                }
            }
        }
        Ok(senders)
    }
}

/// A received mail, as [`AppleMail::replies`] reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// The sender as Mail shows it ("Name <addr>"); [`crate::parse_mailbox`] parses it.
    pub from: String,
    pub subject: String,
    /// The first non-empty line of the body.
    pub first_line: String,
}

/// "sender\tsubject\tfirst line". A tab inside the subject stays in it:
/// the sender has none, and the first line is the last field.
fn parse_reply_line(line: &str) -> Option<Reply> {
    let (from, rest) = line.split_once('\t')?;
    let (subject, first_line) = rest.rsplit_once('\t').unwrap_or((rest, ""));
    Some(Reply {
        from: from.to_string(),
        subject: subject.to_string(),
        first_line: first_line.to_string(),
    })
}

impl MailSender for AppleMail {
    fn own_addresses(&self) -> Result<Vec<String>, SendError> {
        Ok(self.accounts()?.into_iter().map(|a| a.email).collect())
    }

    fn send(&self, mail: &Mail) -> Result<(), SendError> {
        if mail.to.is_empty() {
            return Err(SendError::Failed("no recipients".into()));
        }
        let quit_after = if mail_running() { "0" } else { "1" };
        let mut args = vec![
            mail.subject.clone(),
            mail.body.clone(),
            mail.from.clone().unwrap_or_default(),
            quit_after.to_string(),
        ];
        for r in &mail.to {
            args.push(r.name.clone().unwrap_or_default());
            args.push(r.email.clone());
        }
        run(SEND, &args).map(|_| ())
    }
}

fn mail_running() -> bool {
    Command::new("pgrep")
        .args(["-x", "Mail"])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn quit_mail() {
    let _ = run(&["tell application \"Mail\" to quit"], &[]);
}

/// Runs the script lines with `args`, returns its stdout.
fn run(lines: &[&str], args: &[String]) -> Result<String, SendError> {
    let mut cmd = Command::new("/usr/bin/osascript");
    for l in lines {
        cmd.arg("-e").arg(l);
    }
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| SendError::Failed(format!("osascript: {e}")))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(SendError::Timeout);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(SendError::Failed(e.to_string())),
        }
    };
    let mut out = String::new();
    let mut err = String::new();
    if let Some(mut s) = child.stdout.take() {
        let _ = s.read_to_string(&mut out);
    }
    if let Some(mut s) = child.stderr.take() {
        let _ = s.read_to_string(&mut err);
    }
    if status.success() {
        Ok(out.trim_end_matches('\n').to_string())
    } else {
        Err(classify(&err))
    }
}

/// osascript's error text → [`SendError`].
fn classify(stderr: &str) -> SendError {
    let e = stderr.trim();
    if e.contains("(-1743)") || e.contains("Not authorized") || e.contains("not allowed to send") {
        SendError::NotAllowed
    } else if e.contains("(9001)") {
        SendError::NoAccount
    } else if e.contains("(-1712)") {
        SendError::Timeout
    } else {
        SendError::Failed(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_lines_are_parsed() {
        assert_eq!(
            parse_reply_line("Anna <anna@example.com>\tRe: Notizen: Kickoff\tstopp"),
            Some(Reply {
                from: "Anna <anna@example.com>".into(),
                subject: "Re: Notizen: Kickoff".into(),
                first_line: "stopp".into(),
            })
        );
        assert_eq!(parse_reply_line("no tabs"), None);
    }

    #[test]
    fn errors_are_classified() {
        assert_eq!(
            classify("execution error: Not authorized to send Apple events to Mail. (-1743)"),
            SendError::NotAllowed
        );
        assert_eq!(
            classify("execution error: no account (9001)"),
            SendError::NoAccount
        );
        assert_eq!(
            classify("execution error: Mail got an error: AppleEvent timed out. (-1712)"),
            SendError::Timeout
        );
        assert!(matches!(classify("something else"), SendError::Failed(_)));
    }

    /// The scripts compile against Mail's dictionary (no Apple event is
    /// sent; `osacompile` only reads Mail's terminology).
    #[test]
    fn the_scripts_compile() {
        let dir = tempfile::tempdir().unwrap();
        for (name, lines) in [("send", SEND), ("accounts", ACCOUNTS), ("replies", REPLIES)] {
            let out = dir.path().join(format!("{name}.scpt"));
            let mut cmd = Command::new("/usr/bin/osacompile");
            for l in lines {
                cmd.arg("-e").arg(l);
            }
            let o = cmd.arg("-o").arg(&out).output().unwrap();
            assert!(
                o.status.success(),
                "{name}: {}",
                String::from_utf8_lossy(&o.stderr)
            );
        }
    }
}

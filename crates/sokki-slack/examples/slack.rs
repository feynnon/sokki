//! Tests the Slack steps by hand, without Sokki.
//!
//! ```text
//! cargo run --example slack -- login <client_id>          # signs in, saves the token
//! cargo run --example slack -- whoami
//! cargo run --example slack -- watch                      # huddle state every 10 s
//! cargo run --example slack -- find <channel> [call_id]   # the huddle's message
//! cargo run --example slack -- post <channel> <thread_ts> <note.md>
//! cargo run --example slack -- refresh <client_id>
//! ```
//!
//! The token lives in `$SOKKI_SLACK_AUTH` (default `./slack-auth.json`,
//! mode 0600). Post only into a test channel.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use sokki_slack::{huddle, mrkdwn, oauth, Auth, Client, OAuthApp};

fn auth_path() -> String {
    std::env::var("SOKKI_SLACK_AUTH").unwrap_or_else(|_| "slack-auth.json".into())
}

fn load() -> Result<Auth> {
    let p = auth_path();
    let text = std::fs::read_to_string(&p).with_context(|| format!("{p}: sign in first"))?;
    Ok(serde_json::from_str(&text)?)
}

fn save(auth: &Auth) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(auth_path())?;
    f.write_all(serde_json::to_string_pretty(auth)?.as_bytes())?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).map(String::as_str);
    match arg(0) {
        Some("login") => {
            let app = OAuthApp {
                client_id: arg(1).context("client id")?.into(),
            };
            let page = oauth::CallbackPage {
                done: "Signed in. You can close this tab.".into(),
                failed: "Sign-in failed.".into(),
            };
            let auth = oauth::sign_in(
                &app,
                |url| {
                    println!("Opening {url}");
                    std::process::Command::new("open").arg(url).status()?;
                    Ok(())
                },
                &page,
                Duration::from_secs(300),
                &AtomicBool::new(false),
            )?;
            println!("Signed in: {auth:?}");
            save(&auth)?;
        }
        Some("refresh") => {
            let app = OAuthApp {
                client_id: arg(1).context("client id")?.into(),
            };
            let auth = oauth::refresh(&app, &load()?)?;
            println!("Refreshed: {auth:?}");
            save(&auth)?;
        }
        Some("whoami") => {
            let v = Client::new(&load()?.access_token).auth_test()?;
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        Some("watch") => {
            // What Sokki does during a recorded huddle, and the post after
            // it: `watch [note.md]` posts the note into the thread found.
            let c = Client::new(&load()?.access_token);
            let home = std::path::PathBuf::from(std::env::var("HOME")?);
            let mut thread: Option<(String, String)> = None;
            loop {
                let h = c.huddle_state();
                let channel = sokki_slack::desktop::current_huddle_channel(&home);
                println!("{} {h:?} log channel {channel:?}", oauth::now_unix());
                if let (Ok(h), Some(ch), None) = (&h, &channel, &thread) {
                    if h.in_huddle() {
                        let now = oauth::now_unix();
                        let found =
                            huddle::locate(&c, ch, h.call_id.as_deref(), (now, now), &|d| {
                                std::thread::sleep(d)
                            })?;
                        println!("found {found:#?}");
                        thread = found.map(|f| (f.channel, f.thread_ts));
                        if let (Some((ch, ts)), Some(note)) = (&thread, arg(1)) {
                            post_note(&c, ch, ts, note)?;
                            return Ok(());
                        }
                    }
                }
                std::thread::sleep(Duration::from_secs(10));
            }
        }
        Some("profile") => {
            let v = Client::new(&load()?.access_token).call("users.profile.get", &[])?;
            let p = &v["profile"];
            for k in [
                "huddle_state",
                "huddle_state_expiration_ts",
                "huddle_state_call_id",
                "huddle_state_channel_id",
            ] {
                println!("{k}: {}", p[k]);
            }
        }
        Some("find") => {
            let c = Client::new(&load()?.access_token);
            let channel = arg(1).context("channel")?;
            let now = oauth::now_unix();
            let h = huddle::locate(&c, channel, arg(2), (now.saturating_sub(3600), now), &|d| {
                std::thread::sleep(d)
            })?;
            println!("{h:#?}");
        }
        Some("post") => {
            let (Some(channel), Some(ts), Some(note)) = (arg(1), arg(2), arg(3)) else {
                bail!("post <channel> <thread_ts> <note.md>");
            };
            post_note(&Client::new(&load()?.access_token), channel, ts, note)?;
        }
        _ => bail!("usage: see the top of examples/slack.rs"),
    }
    Ok(())
}

/// Posts a note's summary and transcript like Sokki does (test channel only).
fn post_note(c: &Client, channel: &str, ts: &str, note: &str) -> Result<()> {
    if !c.message_exists(channel, ts)? {
        bail!("message {ts} is gone; not posting (it would land in the channel)");
    }
    let md = std::fs::read_to_string(note)?;
    let (summary, transcript) = md.split_once("## Transcript").unwrap_or((&md, ""));
    let summary = summary
        .split("\n## ")
        .skip(1)
        .map(|s| format!("## {s}"))
        .collect::<Vec<_>>()
        .join("\n");
    let post = mrkdwn::compose(
        "Testlauf Slack-Huddle",
        &summary,
        Some("Von KI zusammengefasst. Lokal, kostenlos, Open Source. Diktier-App und Meeting-Notetaker: sokki.org"),
        transcript.trim(),
        true,
    );
    println!("{}\n---", post.comment);
    let id = c.upload_to_thread(
        channel,
        ts,
        &post.comment,
        &post.filename,
        &post.file_title,
        post.file_content.as_bytes(),
    )?;
    println!("posted, file {id}");
    Ok(())
}

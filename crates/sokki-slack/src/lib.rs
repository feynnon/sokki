//! Sokki's notes in the Slack huddle's thread, posted as the person.
//!
//! After a huddle, Sokki posts the summary (plus the Sokki line) into the
//! thread of the huddle's own message, with the transcript attached as a
//! file. Only Slack's paid Slack AI does this so far.
//!
//! How it works, and why each step is the way it is:
//!
//! 1. **Sign-in** ([`oauth`]): Slack's OAuth with PKCE, which needs no
//!    client secret and so no server (GA since 2026-03-30,
//!    docs.slack.dev/changelog/2026/03/30/pkce). The browser comes back to
//!    `http://localhost:47821/slack`. User scopes only: Sokki posts as the
//!    person, never as a bot. Access tokens live 12 hours, refresh tokens
//!    30 days (PKCE apps); every refresh gives a new pair.
//! 2. **Which huddle**: while Sokki records a Slack call it asks
//!    `users.profile.get` (Tier 4, [`Client::huddle_state`]) for the
//!    person's `huddle_state_call_id`; Slack clears it when the huddle ends,
//!    so Sokki asks during the recording. The profile's
//!    `huddle_state_channel_id` stayed empty in a real huddle, so the
//!    channel comes from the Slack Mac app's log ([`desktop`]).
//! 3. **The huddle's message** ([`huddle::find`]): one
//!    `conversations.history` call on that channel. The huddle's message has
//!    `subtype: huddle_thread` and a `room` whose `id` is the call id. Apps
//!    outside the Slack Marketplace get 1 such call per minute and 15
//!    messages per call (changelog 2025-05-29), so Sokki pages at most a
//!    few times, a minute apart.
//! 4. **The post** ([`Client::upload_to_thread`]): the transcript goes up
//!    with `files.getUploadURLExternal`, and `files.completeUploadExternal`
//!    shares it into the thread with the summary as `initial_comment`: one
//!    message with the file under it.
//!
//! The only network traffic is to slack.com and Slack's upload host, and
//! only after the person signed in and switched the posting on.

pub mod api;
pub mod desktop;
pub mod huddle;
pub mod mrkdwn;
pub mod oauth;

pub use api::{Client, Error, HuddleState, Person};
pub use huddle::Huddle;
pub use oauth::{Auth, OAuthApp};

/// The client id of Sokki's Slack app (App ID A0CLJ17EX45, `SLACK_APP.md`).
/// A client id is public: PKCE needs no secret. Built into every build since
/// 2026-11-27: releases without it showed no Slack row, so nobody could
/// connect and no huddle was ever posted.
pub const CLIENT_ID: &str = "5519147795124.12698041507141";

/// The port of the sign-in redirect. Slack wants the redirect URL to match
/// the one registered in the app exactly, so it is fixed.
pub const REDIRECT_PORT: u16 = 47821;
/// The path of the sign-in redirect.
pub const REDIRECT_PATH: &str = "/slack";

/// The user scopes Sokki asks for, each for one step:
/// - `chat:write`, `files:write`: the post with the transcript file.
/// - `channels:history`, `groups:history`, `im:history`, `mpim:history`: find
///   the huddle's message in a public or private channel, a DM or a group DM.
/// - `users.profile:read`: which huddle the person is in.
/// - `users:read`, `users:read.email`: who was in the huddle, to mail them
///   the notes (meeting/mail.rs) when the person sends notes to everyone.
pub const USER_SCOPES: &[&str] = &[
    "chat:write",
    "files:write",
    "channels:history",
    "groups:history",
    "im:history",
    "mpim:history",
    "users.profile:read",
    "users:read",
    "users:read.email",
];

/// The redirect URL registered in the Slack app.
pub fn redirect_url() -> String {
    format!("http://localhost:{REDIRECT_PORT}{REDIRECT_PATH}")
}

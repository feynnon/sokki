//! The few Slack Web API calls Sokki makes, with the person's user token.

use std::time::Duration;

use serde_json::Value;

const API: &str = "https://slack.com/api/";

/// Why a Slack call failed.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// Slack answered `ok: false` with this code (`invalid_auth`,
    /// `token_expired`, `channel_not_found`, `not_in_channel`, ...).
    Api(String),
    /// HTTP 429: wait this many seconds.
    RateLimited(u64),
    /// No answer, or one Sokki can't read.
    Network(String),
}

impl Error {
    /// The sign-in no longer works; the person has to sign in again.
    pub fn is_signed_out(&self) -> bool {
        matches!(self, Error::Api(e) if matches!(e.as_str(),
            "invalid_auth" | "not_authed" | "token_revoked" | "account_inactive" | "token_expired"))
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Api(e) => write!(f, "Slack: {e}"),
            Error::RateLimited(s) => write!(f, "Slack: too many requests, wait {s} s"),
            Error::Network(e) => write!(f, "Slack not reachable: {e}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// The person's huddle right now, from their Slack profile.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HuddleState {
    /// `in_a_huddle` while in one.
    pub state: String,
    pub call_id: Option<String>,
    pub channel_id: Option<String>,
}

impl HuddleState {
    pub fn in_huddle(&self) -> bool {
        self.state == "in_a_huddle"
    }
}

/// A Slack client for one user token.
pub struct Client {
    token: String,
    agent: ureq::Agent,
    base: String,
}

impl Client {
    pub fn new(token: &str) -> Self {
        Self::with_base(token, API)
    }

    /// For tests against a local server.
    pub fn with_base(token: &str, base: &str) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(60)))
            .http_status_as_error(false)
            .build()
            .into();
        Self {
            token: token.to_string(),
            agent,
            base: base.to_string(),
        }
    }

    /// One Web API call with form parameters.
    pub fn call(&self, method: &str, form: &[(&str, &str)]) -> Result<Value> {
        let resp = self
            .agent
            .post(format!("{}{method}", self.base))
            .header("Authorization", &format!("Bearer {}", self.token))
            .send_form(form.iter().copied());
        read(method, resp)
    }

    /// One Web API call with a JSON body.
    pub fn call_json(&self, method: &str, body: &Value) -> Result<Value> {
        let resp = self
            .agent
            .post(format!("{}{method}", self.base))
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("Content-Type", "application/json; charset=utf-8")
            .send(body.to_string());
        read(method, resp)
    }

    /// Who the token belongs to (`auth.test`).
    pub fn auth_test(&self) -> Result<Value> {
        self.call("auth.test", &[])
    }

    /// Ends the sign-in on Slack's side (`auth.revoke`).
    pub fn revoke(&self) -> Result<()> {
        self.call("auth.revoke", &[]).map(|_| ())
    }

    /// The person's huddle state (`users.profile.get`, Tier 4).
    pub fn huddle_state(&self) -> Result<HuddleState> {
        let v = self.call("users.profile.get", &[])?;
        Ok(parse_huddle_state(&v["profile"]))
    }

    /// One page of a conversation's messages, newest first
    /// (`conversations.history`). Returns the messages and the cursor of
    /// the next page.
    pub fn history(
        &self,
        channel: &str,
        oldest: &str,
        latest: &str,
        cursor: Option<&str>,
    ) -> Result<(Vec<Value>, Option<String>)> {
        let mut form = vec![
            ("channel", channel),
            ("oldest", oldest),
            ("latest", latest),
            ("inclusive", "true"),
            // The most a non-Marketplace app gets per call.
            ("limit", "15"),
        ];
        if let Some(c) = cursor {
            form.push(("cursor", c));
        }
        let v = self.call("conversations.history", &form)?;
        let messages = v["messages"].as_array().cloned().unwrap_or_default();
        let next = v["response_metadata"]["next_cursor"]
            .as_str()
            .filter(|c| !c.is_empty())
            .map(str::to_string);
        Ok((
            messages,
            next.filter(|_| v["has_more"].as_bool() == Some(true)),
        ))
    }

    /// The message `ts` is still in `channel` (one `conversations.history`
    /// call). Slack deletes the message of a huddle nobody wrote in once the
    /// huddle ends, and a post into its thread then lands in the channel
    /// itself (seen 2026-11-24), so Sokki asks first.
    pub fn message_exists(&self, channel: &str, ts: &str) -> Result<bool> {
        let v = self.call(
            "conversations.history",
            &[
                ("channel", channel),
                ("oldest", ts),
                ("latest", ts),
                ("inclusive", "true"),
                ("limit", "1"),
            ],
        )?;
        Ok(v["messages"]
            .as_array()
            .is_some_and(|m| m.iter().any(|m| m["ts"].as_str() == Some(ts))))
    }

    /// A user's name and email address (`users.info`, Tier 4; the address
    /// needs `users:read.email`).
    pub fn user(&self, id: &str) -> Result<Person> {
        let v = self.call("users.info", &[("user", id)])?;
        Ok(parse_person(&v["user"]))
    }

    /// Posts `comment` into the thread `thread_ts` of `channel`, with the
    /// file attached under it: one message. Returns the file id.
    pub fn upload_to_thread(
        &self,
        channel: &str,
        thread_ts: &str,
        comment: &str,
        filename: &str,
        title: &str,
        content: &[u8],
    ) -> Result<String> {
        let len = content.len().to_string();
        let v = self.call(
            "files.getUploadURLExternal",
            &[("filename", filename), ("length", &len)],
        )?;
        let (Some(url), Some(file_id)) = (v["upload_url"].as_str(), v["file_id"].as_str()) else {
            return Err(Error::Network(
                "files.getUploadURLExternal: no upload_url".into(),
            ));
        };
        let resp = self
            .agent
            .post(url)
            .header("Content-Type", "application/octet-stream")
            .send(content)
            .map_err(|e| Error::Network(format!("upload: {e}")))?;
        if !resp.status().is_success() {
            return Err(Error::Network(format!("upload: HTTP {}", resp.status())));
        }
        self.call_json(
            "files.completeUploadExternal",
            &serde_json::json!({
                "files": [{ "id": file_id, "title": title }],
                "channel_id": channel,
                "thread_ts": thread_ts,
                "initial_comment": comment,
            }),
        )?;
        Ok(file_id.to_string())
    }

    /// The link to a message (`chat.getPermalink`).
    pub fn permalink(&self, channel: &str, ts: &str) -> Result<String> {
        let v = self.call(
            "chat.getPermalink",
            &[("channel", channel), ("message_ts", ts)],
        )?;
        Ok(v["permalink"].as_str().unwrap_or_default().to_string())
    }
}

fn read(
    method: &str,
    resp: std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<Value> {
    let mut resp = resp.map_err(|e| Error::Network(format!("{method}: {e}")))?;
    if resp.status().as_u16() == 429 {
        let wait = resp
            .headers()
            .get("Retry-After")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(60);
        return Err(Error::RateLimited(wait));
    }
    let v: Value = resp
        .body_mut()
        .read_json()
        .map_err(|e| Error::Network(format!("{method}: HTTP {}: {e}", resp.status())))?;
    if v["ok"].as_bool() != Some(true) {
        return Err(Error::Api(
            v["error"].as_str().unwrap_or("unknown_error").to_string(),
        ));
    }
    if let Some(w) = v["warning"].as_str() {
        log::info!("Slack {method}: warning {w}");
    }
    Ok(v)
}

/// Someone in a huddle, as `users.info` describes them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Person {
    pub id: String,
    /// The name others see (display name, else real name).
    pub name: Option<String>,
    pub email: Option<String>,
    /// A bot or app user, or a deactivated account: never mailed.
    pub not_a_person: bool,
}

pub fn parse_person(user: &Value) -> Person {
    let s = |v: &Value| {
        v.as_str()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
    };
    let p = &user["profile"];
    Person {
        id: user["id"].as_str().unwrap_or_default().to_string(),
        name: s(&p["display_name"])
            .or_else(|| s(&p["real_name"]))
            .or_else(|| s(&user["real_name"])),
        email: s(&p["email"]),
        not_a_person: user["is_bot"].as_bool() == Some(true)
            || user["deleted"].as_bool() == Some(true)
            || user["id"] == "USLACKBOT",
    }
}

/// The huddle fields of a user profile. Empty strings count as missing.
pub fn parse_huddle_state(profile: &Value) -> HuddleState {
    let s = |k: &str| {
        profile[k]
            .as_str()
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    HuddleState {
        state: s("huddle_state").unwrap_or_default(),
        call_id: s("huddle_state_call_id"),
        channel_id: s("huddle_state_channel_id"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_profile_says_which_huddle() {
        let h = parse_huddle_state(&json!({
            "real_name": "Ada", "huddle_state": "in_a_huddle",
            "huddle_state_expiration_ts": 0,
            "huddle_state_call_id": "R0123", "huddle_state_channel_id": "C0456"
        }));
        assert!(h.in_huddle());
        assert_eq!(h.call_id.as_deref(), Some("R0123"));
        assert_eq!(h.channel_id.as_deref(), Some("C0456"));

        let none = parse_huddle_state(&json!({"huddle_state": "default_unset",
                                              "huddle_state_call_id": ""}));
        assert!(!none.in_huddle());
        assert_eq!(none.call_id, None);
    }

    #[test]
    fn a_user_has_a_name_and_an_address() {
        let p = parse_person(
            &json!({"id": "U1", "real_name": "Ada Muster", "is_bot": false,
            "profile": {"display_name": "", "real_name": "Ada Muster", "email": "ada@example.com"}}),
        );
        assert_eq!(
            p,
            Person {
                id: "U1".into(),
                name: Some("Ada Muster".into()),
                email: Some("ada@example.com".into()),
                not_a_person: false
            }
        );
        let p = parse_person(&json!({"id": "U2", "profile": {"display_name": "bob"}}));
        assert_eq!((p.name.as_deref(), p.email), (Some("bob"), None));
        assert!(parse_person(&json!({"id": "B1", "is_bot": true, "profile": {}})).not_a_person);
        assert!(parse_person(&json!({"id": "U3", "deleted": true, "profile": {}})).not_a_person);
    }

    #[test]
    fn a_dead_sign_in_is_told_apart() {
        assert!(Error::Api("invalid_auth".into()).is_signed_out());
        assert!(Error::Api("token_expired".into()).is_signed_out());
        assert!(!Error::Api("not_in_channel".into()).is_signed_out());
        assert!(!Error::RateLimited(60).is_signed_out());
    }
}

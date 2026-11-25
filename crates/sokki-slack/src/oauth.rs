//! Sign-in with Slack: OAuth 2 with PKCE, no client secret, no server.
//!
//! Sokki opens Slack's permission page in the browser and listens on
//! `localhost:47821` for the redirect. The code it gets back is exchanged
//! with the PKCE verifier (docs.slack.dev/authentication/using-pkce).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const AUTHORIZE_URL: &str = "https://slack.com/oauth/v2/authorize";
const ACCESS_URL: &str = "https://slack.com/api/oauth.v2.access";
/// Refresh this long before the access token runs out.
const REFRESH_MARGIN_S: u64 = 300;

/// The Slack app Sokki signs in with. The client id is public; a PKCE app
/// has no secret.
#[derive(Debug, Clone)]
pub struct OAuthApp {
    pub client_id: String,
}

/// A signed-in person: the user token and what it belongs to.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Auth {
    pub team_id: String,
    pub team_name: String,
    pub user_id: String,
    pub access_token: String,
    /// Set when Slack rotates tokens (always for PKCE apps).
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Unix seconds when the access token runs out; `None`: it doesn't.
    #[serde(default)]
    pub expires_at: Option<u64>,
    /// Unix seconds of the sign-in or the last refresh. A PKCE app's
    /// refresh token runs out 30 days after it was issued.
    #[serde(default)]
    pub refreshed_at: u64,
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Auth")
            .field("team_id", &self.team_id)
            .field("team_name", &self.team_name)
            .field("user_id", &self.user_id)
            .field("expires_at", &self.expires_at)
            .field("refreshed_at", &self.refreshed_at)
            .finish_non_exhaustive()
    }
}

impl Auth {
    /// The access token runs out within five minutes (or already has).
    pub fn needs_refresh(&self, now: u64) -> bool {
        self.refresh_token.is_some() && self.expires_at.is_some_and(|t| t <= now + REFRESH_MARGIN_S)
    }
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A PKCE verifier and its S256 challenge.
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new() -> Result<Self> {
        Ok(Self::from_verifier(random_token(48)?))
    }

    pub fn from_verifier(verifier: String) -> Self {
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(verifier.as_bytes()));
        Self {
            verifier,
            challenge,
        }
    }
}

/// `bytes` random bytes as URL-safe text (RFC 7636 allows A-Z a-z 0-9 - _).
fn random_token(bytes: usize) -> Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::getrandom(&mut buf).map_err(|e| anyhow!("no randomness: {e}"))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf))
}

/// Slack's permission page for these scopes.
pub fn authorize_url(
    client_id: &str,
    user_scopes: &[&str],
    challenge: &str,
    state: &str,
) -> String {
    let params = [
        ("client_id", client_id.to_string()),
        ("scope", String::new()),
        ("user_scope", user_scopes.join(",")),
        ("redirect_uri", crate::redirect_url()),
        ("state", state.to_string()),
        ("code_challenge", challenge.to_string()),
        ("code_challenge_method", "S256".to_string()),
    ];
    let query: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{k}={}", encode(v)))
        .collect();
    format!("{AUTHORIZE_URL}?{}", query.join("&"))
}

/// The page the browser shows after Slack sends it back to Sokki.
pub struct CallbackPage {
    pub done: String,
    pub failed: String,
}

/// Signs in: opens Slack's permission page with `open`, waits up to
/// `timeout` for the browser to come back (or until `cancel` is set), and
/// exchanges the code for a user token.
pub fn sign_in(
    app: &OAuthApp,
    open: impl FnOnce(&str) -> Result<()>,
    page: &CallbackPage,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<Auth> {
    // Listen before the browser opens, on both loopback addresses: the
    // browser resolves "localhost" to either.
    let mut listeners = Vec::new();
    for addr in [
        format!("127.0.0.1:{}", crate::REDIRECT_PORT),
        format!("[::1]:{}", crate::REDIRECT_PORT),
    ] {
        match TcpListener::bind(&addr) {
            Ok(l) => {
                l.set_nonblocking(true)?;
                listeners.push(l);
            }
            Err(e) => log::info!("Slack sign-in: not listening on {addr}: {e}"),
        }
    }
    if listeners.is_empty() {
        bail!(
            "port {} is in use, so Slack can't hand the sign-in back to Sokki",
            crate::REDIRECT_PORT
        );
    }
    let pkce = Pkce::new()?;
    let state = random_token(16)?;
    open(&authorize_url(
        &app.client_id,
        crate::USER_SCOPES,
        &pkce.challenge,
        &state,
    ))?;

    let deadline = Instant::now() + timeout;
    let code = loop {
        if cancel.load(Ordering::Relaxed) {
            bail!("sign-in cancelled");
        }
        if Instant::now() > deadline {
            bail!(
                "no answer from Slack within {} minutes",
                timeout.as_secs() / 60
            );
        }
        let mut got = None;
        for l in &listeners {
            match l.accept() {
                Ok((stream, _)) => {
                    got = Some(stream);
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => log::warn!("Slack sign-in: accept failed: {e}"),
            }
        }
        let Some(stream) = got else {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        match handle_callback(stream, &state, page) {
            Ok(Some(code)) => break code,
            Ok(None) => continue,
            Err(e) => return Err(e),
        }
    };
    exchange(app, &code, &pkce.verifier)
}

/// Reads one browser request. `Some(code)` for Slack's redirect,
/// `None` for anything else (a favicon), an error when the person said no.
fn handle_callback(
    mut stream: TcpStream,
    state: &str,
    page: &CallbackPage,
) -> Result<Option<String>> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 16 * 1024 {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(_) => break,
        }
    }
    let request = String::from_utf8_lossy(&buf);
    let target = request
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != crate::REDIRECT_PATH {
        respond(&mut stream, "404 Not Found", "");
        return Ok(None);
    }
    let params = parse_query(query);
    let get = |k: &str| params.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    let result = match (get("state"), get("code"), get("error")) {
        (Some(s), _, _) if s != state => Err(anyhow!("the sign-in answer didn't match")),
        (_, Some(code), _) => Ok(Some(code.to_string())),
        (_, None, Some("access_denied")) => Err(anyhow!("access to Slack was not allowed")),
        (_, None, Some(e)) => Err(anyhow!("Slack: {e}")),
        _ => Err(anyhow!("Slack sent no sign-in code")),
    };
    let body = match &result {
        Ok(_) => &page.done,
        Err(_) => &page.failed,
    };
    respond(&mut stream, "200 OK", &html_page(body));
    result
}

fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

fn html_page(text: &str) -> String {
    let text = text.replace('&', "&amp;").replace('<', "&lt;");
    format!(
        "<!doctype html><meta charset=utf-8><title>Sokki</title>\
         <body style=\"font:17px -apple-system,system-ui,sans-serif;display:grid;place-items:center;height:90vh;margin:0;color:#1c1c1e\">\
         <p>{text}</p></body>"
    )
}

/// Exchanges the code from the redirect for a user token.
pub fn exchange(app: &OAuthApp, code: &str, verifier: &str) -> Result<Auth> {
    let redirect = crate::redirect_url();
    let v = post_form(&[
        ("client_id", app.client_id.as_str()),
        ("code", code),
        ("code_verifier", verifier),
        ("redirect_uri", redirect.as_str()),
    ])?;
    parse_access(&v, now_unix())
}

/// A new access token (and refresh token) for `auth`. The old refresh token
/// stops working, so the caller saves the result at once.
pub fn refresh(app: &OAuthApp, auth: &Auth) -> Result<Auth> {
    let token = auth
        .refresh_token
        .as_deref()
        .context("this sign-in has no refresh token")?;
    let v = post_form(&[
        ("client_id", app.client_id.as_str()),
        ("grant_type", "refresh_token"),
        ("refresh_token", token),
    ])?;
    parse_refresh(&v, auth, now_unix())
}

fn post_form(form: &[(&str, &str)]) -> Result<Value> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut resp = agent
        .post(ACCESS_URL)
        .send_form(form.iter().copied())
        .context("oauth.v2.access failed")?;
    let v: Value = resp
        .body_mut()
        .read_json()
        .context("oauth.v2.access: unreadable answer")?;
    if v["ok"].as_bool() != Some(true) {
        bail!(
            "Slack refused the sign-in: {}",
            v["error"].as_str().unwrap_or("unknown error")
        );
    }
    Ok(v)
}

/// The sign-in from `oauth.v2.access`'s answer to a code. The user token
/// sits under `authed_user`.
pub fn parse_access(v: &Value, now: u64) -> Result<Auth> {
    let user = &v["authed_user"];
    let access_token = user["access_token"]
        .as_str()
        .filter(|t| !t.is_empty())
        .context("Slack sent no user token")?;
    Ok(Auth {
        team_id: v["team"]["id"].as_str().unwrap_or_default().to_string(),
        team_name: v["team"]["name"].as_str().unwrap_or_default().to_string(),
        user_id: user["id"].as_str().unwrap_or_default().to_string(),
        access_token: access_token.to_string(),
        refresh_token: user["refresh_token"].as_str().map(str::to_string),
        expires_at: user["expires_in"].as_u64().map(|s| now + s),
        refreshed_at: now,
    })
}

/// The sign-in after a refresh. Slack's examples put the new user token at
/// the top level; `authed_user` is read too, in case.
pub fn parse_refresh(v: &Value, prev: &Auth, now: u64) -> Result<Auth> {
    let src = if v["access_token"].is_string() {
        v
    } else {
        &v["authed_user"]
    };
    let access_token = src["access_token"]
        .as_str()
        .filter(|t| !t.is_empty())
        .context("Slack sent no new token")?;
    Ok(Auth {
        access_token: access_token.to_string(),
        refresh_token: src["refresh_token"]
            .as_str()
            .map(str::to_string)
            .or_else(|| prev.refresh_token.clone()),
        expires_at: src["expires_in"].as_u64().map(|s| now + s),
        refreshed_at: now,
        ..prev.clone()
    })
}

/// Percent-encoding for query values (RFC 3986 unreserved stay).
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = |c: u8| (c as char).to_digit(16);
                match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 2;
                    }
                    _ => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pkce_challenge_is_rfc7636_s256() {
        // RFC 7636, appendix B.
        let p = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into());
        assert_eq!(p.challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        assert!(Pkce::new().unwrap().verifier.len() >= 43);
    }

    #[test]
    fn the_permission_page_asks_for_user_scopes_only() {
        let url = authorize_url("123.456", crate::USER_SCOPES, "abc", "st");
        assert!(url.starts_with("https://slack.com/oauth/v2/authorize?client_id=123.456&scope=&user_scope=chat%3Awrite%2Cfiles%3Awrite"));
        assert!(url.contains("&redirect_uri=http%3A%2F%2Flocalhost%3A47821%2Fslack"));
        assert!(url.contains("&code_challenge=abc&code_challenge_method=S256"));
        assert!(url.contains("&state=st"));
    }

    #[test]
    fn the_redirect_query_is_read() {
        let q = parse_query("code=12%2E34&state=a%20b&x");
        assert_eq!(q[0], ("code".into(), "12.34".into()));
        assert_eq!(q[1], ("state".into(), "a b".into()));
        assert_eq!(q[2], ("x".into(), "".into()));
        assert_eq!(decode("50%"), "50%");
    }

    #[test]
    fn a_rotating_sign_in_keeps_both_tokens() {
        let v = json!({
            "ok": true, "app_id": "A1",
            "authed_user": {"id": "U1", "scope": "chat:write", "access_token": "xoxe.xoxp-1",
                            "token_type": "user", "refresh_token": "xoxe-1-r", "expires_in": 43200},
            "team": {"id": "T1", "name": "Acme"}
        });
        let a = parse_access(&v, 1000).unwrap();
        assert_eq!(a.user_id, "U1");
        assert_eq!(a.team_name, "Acme");
        assert_eq!(a.refresh_token.as_deref(), Some("xoxe-1-r"));
        assert_eq!(a.expires_at, Some(44200));
        assert!(!a.needs_refresh(1000));
        assert!(a.needs_refresh(44200 - 299));
        assert!(
            !format!("{a:?}").contains("xoxe"),
            "tokens stay out of logs"
        );

        let r = parse_refresh(
            &json!({"ok": true, "access_token": "xoxe.xoxp-2", "refresh_token": "xoxe-1-s",
                    "expires_in": 43200, "token_type": "user"}),
            &a,
            50000,
        )
        .unwrap();
        assert_eq!(r.access_token, "xoxe.xoxp-2");
        assert_eq!(r.refresh_token.as_deref(), Some("xoxe-1-s"));
        assert_eq!((r.expires_at, r.refreshed_at), (Some(93200), 50000));
        assert_eq!(r.team_name, "Acme");
    }

    #[test]
    fn a_token_without_expiry_never_refreshes() {
        let v = json!({"ok": true, "authed_user": {"id": "U1", "access_token": "xoxp-1"},
                       "team": {"id": "T1", "name": "Acme"}});
        let a = parse_access(&v, 0).unwrap();
        assert!(!a.needs_refresh(u64::MAX / 2));
        assert!(parse_access(&json!({"ok": true, "authed_user": {}}), 0).is_err());
    }
}

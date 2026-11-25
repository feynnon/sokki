//! Sending through the person's Gmail with the Gmail API, scope
//! `gmail.send` only ("send email on your behalf": Sokki can't read,
//! search or change mail), plus `openid email` to show which address is
//! connected. Built as card `privacy-google` requires for Google's review:
//!
//! - OAuth for desktop apps: the browser opens Google's consent page, which
//!   redirects to `http://127.0.0.1:<free port>` on this Mac (loopback), with
//!   PKCE (S256) and a `state` value.
//! - The refresh token is kept by the host in the macOS Keychain
//!   ([`TokenStore`]); the access token only in memory.
//! - A mail is one `users.messages.send` call, straight from the Mac.
//! - Disconnecting revokes the token at Google ([`revoke`]).
//!
//! The client id and secret come from the host (set at build time). Google:
//! a desktop app can't keep its secret secret, so it isn't treated as one.
//!
//! Gmail's "stop" replies can't be read with `gmail.send` (reading needs a
//! restricted scope); they reach the person's inbox, and the person adds
//! the sender to the list in Settings.

use crate::{Mail, MailSender, Recipient, SendError};
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// What notes-to-all asks for. Anything more is asked for only when the
/// person turns on a feature that needs it ([`sign_in_with`]).
pub const SCOPES: &str = "https://www.googleapis.com/auth/gmail.send openid email";
/// Headers of messages, never their bodies: what the mail before a meeting
/// needs to see replies (card pre-meeting-mail). Restricted scope.
pub const METADATA_SCOPE: &str = "https://www.googleapis.com/auth/gmail.metadata";
const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
const SEND_URL: &str = "https://gmail.googleapis.com/gmail/v1/users/me/messages/send";

/// How long the browser has for the consent page.
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);

/// Sokki's OAuth client (a Google "Desktop app" client).
#[derive(Debug, Clone)]
pub struct Client {
    pub id: String,
    pub secret: String,
}

/// Where the host keeps the refresh token (macOS: the Keychain).
pub trait TokenStore: Send + Sync {
    fn get(&self) -> Option<String>;
    fn set(&self, refresh_token: &str) -> Result<(), String>;
    fn delete(&self);
}

/// What a finished sign-in gives.
#[derive(Debug, Clone)]
pub struct SignedIn {
    pub refresh_token: String,
    pub access_token: String,
    pub expires_in: u64,
    /// The connected Gmail address, from the ID token.
    pub email: Option<String>,
    /// The scopes Google granted, as it lists them.
    pub scopes: Vec<String>,
}

/// Why connecting or sending failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GmailError {
    /// The person closed the page, said no, or took too long.
    Cancelled(String),
    /// Google no longer accepts the token (revoked, expired after 7 days in
    /// Google's test mode, password changed): connect again.
    SignInAgain,
    /// Network or Google error, with Google's own message.
    Failed(String),
}

impl std::fmt::Display for GmailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GmailError::Cancelled(m) => write!(f, "Gmail sign-in cancelled: {m}"),
            GmailError::SignInAgain => write!(f, "Gmail needs you to connect again"),
            GmailError::Failed(m) => write!(f, "Gmail: {m}"),
        }
    }
}

impl From<GmailError> for SendError {
    fn from(e: GmailError) -> Self {
        match e {
            GmailError::SignInAgain => SendError::SignIn,
            other => SendError::Failed(other.to_string()),
        }
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .build()
        .into()
}

fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::getrandom(&mut buf).expect("system randomness");
    URL_SAFE_NO_PAD.encode(buf)
}

/// PKCE: the verifier stays here, the challenge goes to Google.
#[derive(Debug, Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new() -> Self {
        let verifier = random_token(48);
        Self::from_verifier(verifier)
    }

    fn from_verifier(verifier: String) -> Self {
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Self {
            verifier,
            challenge,
        }
    }
}

impl Default for Pkce {
    fn default() -> Self {
        Self::new()
    }
}

fn form_encode(s: &str) -> String {
    let mut out = String::new();
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

fn form_decode(s: &str) -> String {
    let s = s.replace('+', " ");
    let bytes = s.as_bytes();
    let mut out = Vec::new();
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

/// Google's consent page for `client`, redirecting to `redirect_uri`.
pub fn auth_url(client: &Client, redirect_uri: &str, pkce: &Pkce, state: &str) -> String {
    auth_url_with(client, redirect_uri, pkce, state, &[])
}

/// [`auth_url`] asking for `extra_scopes` too, on top of what was granted
/// before. A new sign-in with the whole set: Google does no incremental
/// authorization for installed apps.
pub fn auth_url_with(
    client: &Client,
    redirect_uri: &str,
    pkce: &Pkce,
    state: &str,
    extra_scopes: &[&str],
) -> String {
    let mut scope = SCOPES.to_string();
    for extra in extra_scopes {
        if !scope.split(' ').any(|s| s == *extra) {
            scope.push(' ');
            scope.push_str(extra);
        }
    }
    let params = [
        ("client_id", client.id.as_str()),
        ("redirect_uri", redirect_uri),
        ("response_type", "code"),
        ("scope", scope.as_str()),
        ("code_challenge", pkce.challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("state", state),
        // A refresh token every time, so connecting again always works.
        ("access_type", "offline"),
        ("prompt", "consent"),
    ];
    let query: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{k}={}", form_encode(v)))
        .collect();
    format!("{AUTH_URL}?{}", query.join("&"))
}

/// Waits on `listener` for the browser's redirect and returns the code.
/// Answers the browser with a short page. Anything that isn't the redirect
/// (a favicon request) is answered and ignored.
pub fn wait_for_code(
    listener: &TcpListener,
    state: &str,
    timeout: Duration,
) -> Result<String, GmailError> {
    listener
        .set_nonblocking(true)
        .map_err(|e| GmailError::Failed(e.to_string()))?;
    let started = Instant::now();
    loop {
        if started.elapsed() > timeout {
            return Err(GmailError::Cancelled("no answer from the browser".into()));
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(result) = handle_redirect(stream, state) {
                    return result;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(GmailError::Failed(e.to_string())),
        }
    }
}

fn handle_redirect(mut stream: TcpStream, state: &str) -> Option<Result<String, GmailError>> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).ok()?;
    // "GET /?state=…&code=… HTTP/1.1"
    let target = line.split_whitespace().nth(1)?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != "/" {
        let _ = stream
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return None;
    }
    let get = |key: &str| {
        query.split('&').find_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            (k == key).then(|| form_decode(v))
        })
    };
    let result = if get("state").as_deref() != Some(state) {
        Err(GmailError::Cancelled(
            "the answer did not match this sign-in".into(),
        ))
    } else if let Some(code) = get("code") {
        Ok(code)
    } else {
        Err(GmailError::Cancelled(
            get("error").unwrap_or_else(|| "no code".into()),
        ))
    };
    let body = if result.is_ok() {
        "<!doctype html><meta charset=utf-8><title>Sokki</title><body style=\"font:16px -apple-system,sans-serif;margin:64px auto;max-width:420px;text-align:center\"><h1 style=\"font-size:22px\">Gmail is connected to Sokki.</h1><p>You can close this tab.</p>"
    } else {
        "<!doctype html><meta charset=utf-8><title>Sokki</title><body style=\"font:16px -apple-system,sans-serif;margin:64px auto;max-width:420px;text-align:center\"><h1 style=\"font-size:22px\">Gmail was not connected.</h1><p>You can close this tab and try again in Sokki.</p>"
    };
    let _ = stream.write_all(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    );
    Some(result)
}

#[derive(Deserialize)]
struct TokenReply {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    id_token: Option<String>,
    scope: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

fn token_call(form: &[(&str, &str)]) -> Result<TokenReply, GmailError> {
    let body: Vec<String> = form
        .iter()
        .map(|(k, v)| format!("{k}={}", form_encode(v)))
        .collect();
    let mut resp = agent()
        .post(TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(body.join("&"))
        .map_err(|e| GmailError::Failed(e.to_string()))?;
    let reply: TokenReply = resp
        .body_mut()
        .read_json()
        .map_err(|e| GmailError::Failed(e.to_string()))?;
    match reply.error.as_deref() {
        None => Ok(reply),
        Some("invalid_grant") => Err(GmailError::SignInAgain),
        Some(e) => Err(GmailError::Failed(format!(
            "{e}: {}",
            reply.error_description.clone().unwrap_or_default()
        ))),
    }
}

/// The whole sign-in: a free loopback port, the consent page opened with
/// `open_browser`, the redirect, the code exchanged for tokens.
pub fn sign_in(client: &Client, open_browser: impl FnOnce(&str)) -> Result<SignedIn, GmailError> {
    sign_in_with(client, &[], open_browser)
}

/// [`sign_in`] asking for `extra_scopes` too (e.g. [`METADATA_SCOPE`]).
pub fn sign_in_with(
    client: &Client,
    extra_scopes: &[&str],
    open_browser: impl FnOnce(&str),
) -> Result<SignedIn, GmailError> {
    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|e| GmailError::Failed(e.to_string()))?;
    let port = listener
        .local_addr()
        .map_err(|e| GmailError::Failed(e.to_string()))?
        .port();
    let redirect = format!("http://127.0.0.1:{port}");
    let pkce = Pkce::new();
    let state = random_token(16);
    open_browser(&auth_url_with(
        client,
        &redirect,
        &pkce,
        &state,
        extra_scopes,
    ));
    let code = wait_for_code(&listener, &state, SIGN_IN_TIMEOUT)?;
    let reply = token_call(&[
        ("code", &code),
        ("client_id", &client.id),
        ("client_secret", &client.secret),
        ("redirect_uri", &redirect),
        ("grant_type", "authorization_code"),
        ("code_verifier", &pkce.verifier),
    ])?;
    let (Some(access_token), Some(refresh_token)) = (reply.access_token, reply.refresh_token)
    else {
        return Err(GmailError::Failed("Google sent no token".into()));
    };
    Ok(SignedIn {
        refresh_token,
        access_token,
        expires_in: reply.expires_in.unwrap_or(3600),
        email: reply.id_token.as_deref().and_then(email_from_id_token),
        scopes: reply
            .scope
            .as_deref()
            .unwrap_or("")
            .split_whitespace()
            .map(str::to_string)
            .collect(),
    })
}

/// The `email` claim of an ID token. Not verified: the token came straight
/// from Google's token endpoint over TLS, and it only labels the account.
pub fn email_from_id_token(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.get("email")?.as_str().and_then(crate::normalize_email)
}

/// Revokes the token at Google (refresh or access token).
pub fn revoke(token: &str) -> Result<(), GmailError> {
    let resp = agent()
        .post(REVOKE_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(format!("token={}", form_encode(token)))
        .map_err(|e| GmailError::Failed(e.to_string()))?;
    let status = resp.status().as_u16();
    // 400 invalid_token: already revoked or expired; gone either way.
    if status == 200 || status == 400 {
        Ok(())
    } else {
        Err(GmailError::Failed(format!("revoke: HTTP {status}")))
    }
}

/// Gmail as a [`MailSender`]: refreshes the access token as needed.
pub struct Gmail<S: TokenStore> {
    client: Client,
    store: S,
    /// The connected address, when known.
    email: Option<String>,
    access: Mutex<Option<(String, Instant)>>,
}

impl<S: TokenStore> Gmail<S> {
    pub fn new(client: Client, store: S, email: Option<String>) -> Self {
        Self {
            client,
            store,
            email,
            access: Mutex::new(None),
        }
    }

    fn access_token(&self) -> Result<String, GmailError> {
        let mut cached = self.access.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((token, until)) = cached.as_ref() {
            if Instant::now() < *until {
                return Ok(token.clone());
            }
        }
        let refresh = self.store.get().ok_or(GmailError::SignInAgain)?;
        let reply = token_call(&[
            ("client_id", &self.client.id),
            ("client_secret", &self.client.secret),
            ("refresh_token", &refresh),
            ("grant_type", "refresh_token"),
        ])?;
        let token = reply
            .access_token
            .ok_or_else(|| GmailError::Failed("Google sent no token".into()))?;
        // A minute early, so a token never runs out mid-send.
        let life = reply.expires_in.unwrap_or(3600).saturating_sub(60);
        *cached = Some((token.clone(), Instant::now() + Duration::from_secs(life)));
        Ok(token)
    }
}

impl<S: TokenStore> Gmail<S> {
    /// A GET on the Gmail API with the person's token, refreshed as needed
    /// (for reading replies with [`METADATA_SCOPE`]). The JSON reply.
    pub fn authorized_get(&self, url: &str) -> Result<serde_json::Value, SendError> {
        let token = self.access_token()?;
        let mut resp = agent()
            .get(url)
            .header("Authorization", &format!("Bearer {token}"))
            .call()
            .map_err(|e| SendError::Failed(e.to_string()))?;
        let status = resp.status().as_u16();
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        match status {
            200 => serde_json::from_str(&text).map_err(|e| SendError::Failed(e.to_string())),
            401 => {
                *self.access.lock().unwrap_or_else(|e| e.into_inner()) = None;
                Err(SendError::SignIn)
            }
            _ => Err(SendError::Failed(format!("HTTP {status}: {text}"))),
        }
    }
}

impl<S: TokenStore> MailSender for Gmail<S> {
    fn own_addresses(&self) -> Result<Vec<String>, SendError> {
        Ok(self.email.iter().cloned().collect())
    }

    fn send(&self, mail: &Mail) -> Result<(), SendError> {
        self.send_in_thread(mail).map(|_| ())
    }
}

impl<S: TokenStore> Gmail<S> {
    /// Sends one mail and returns Gmail's thread id for it, so replies in
    /// that thread can be read later (`users.threads.get`, card
    /// pre-meeting-mail) without listing the inbox.
    pub fn send_in_thread(&self, mail: &Mail) -> Result<String, SendError> {
        if mail.to.is_empty() {
            return Err(SendError::Failed("no recipients".into()));
        }
        let token = self.access_token()?;
        let raw = URL_SAFE_NO_PAD.encode(rfc2822(mail));
        let mut resp = agent()
            .post(SEND_URL)
            .header("Authorization", &format!("Bearer {token}"))
            .send_json(serde_json::json!({ "raw": raw }))
            .map_err(|e| SendError::Failed(e.to_string()))?;
        let status = resp.status().as_u16();
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        if status == 200 {
            let thread = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v["threadId"].as_str().map(str::to_string))
                .unwrap_or_default();
            return Ok(thread);
        }
        if status == 401 {
            *self.access.lock().unwrap_or_else(|e| e.into_inner()) = None;
            return Err(SendError::SignIn);
        }
        let message = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
            .unwrap_or(text);
        Err(SendError::Failed(format!("HTTP {status}: {message}")))
    }
}

/// The mail as an RFC 2822 message: UTF-8 headers as encoded words, the
/// plain-text body in base64. `From` only when the mail names one (Gmail
/// fills in the connected address otherwise).
pub fn rfc2822(mail: &Mail) -> Vec<u8> {
    let mut out = String::new();
    if let Some(from) = mail.from.as_deref().and_then(crate::parse_mailbox) {
        out.push_str(&format!("From: {}\r\n", mailbox_header(&from)));
    }
    let to: Vec<String> = mail.to.iter().map(mailbox_header).collect();
    out.push_str(&format!("To: {}\r\n", to.join(",\r\n ")));
    out.push_str(&format!("Subject: {}\r\n", encode_words(&mail.subject)));
    out.push_str("MIME-Version: 1.0\r\n");
    out.push_str("Content-Type: text/plain; charset=UTF-8\r\n");
    out.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
    let body = mail.body.replace("\r\n", "\n").replace('\n', "\r\n");
    let encoded = STANDARD.encode(body.as_bytes());
    for chunk in encoded.as_bytes().chunks(76) {
        out.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        out.push_str("\r\n");
    }
    out.into_bytes()
}

fn mailbox_header(r: &Recipient) -> String {
    match r.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        Some(name) if name.is_ascii() && !name.contains(['"', '\\', '\r', '\n']) => {
            format!("\"{name}\" <{}>", r.email)
        }
        Some(name) => format!("{} <{}>", encode_words(name), r.email),
        None => r.email.clone(),
    }
}

/// A header value: as is when plain ASCII, else RFC 2047 encoded words
/// (`=?UTF-8?B?…?=`), at most 39 bytes each (64 characters, so a line
/// with "Subject: " before it stays under 78), and no character cut in two.
fn encode_words(s: &str) -> String {
    let s: String = s.chars().filter(|c| *c != '\r' && *c != '\n').collect();
    if s.is_ascii() && !s.contains("=?") {
        return s;
    }
    let mut words = Vec::new();
    let mut chunk = String::new();
    for c in s.chars() {
        if chunk.len() + c.len_utf8() > 39 {
            words.push(format!("=?UTF-8?B?{}?=", STANDARD.encode(chunk.as_bytes())));
            chunk.clear();
        }
        chunk.push(c);
    }
    if !chunk.is_empty() {
        words.push(format!("=?UTF-8?B?{}?=", STANDARD.encode(chunk.as_bytes())));
    }
    words.join("\r\n ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc_7636() {
        // RFC 7636, appendix B.
        let p = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into());
        assert_eq!(p.challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        assert!(Pkce::new().verifier.len() >= 43);
    }

    #[test]
    fn the_consent_url_asks_for_send_only() {
        let c = Client {
            id: "123.apps.googleusercontent.com".into(),
            secret: "s".into(),
        };
        let url = auth_url(&c, "http://127.0.0.1:5000", &Pkce::new(), "xyz");
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?client_id=123.apps.googleusercontent.com&redirect_uri=http%3A%2F%2F127.0.0.1%3A5000"));
        assert!(url.contains(
            "scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fgmail.send%20openid%20email"
        ));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=xyz"));
        assert!(!url.contains("gmail.metadata"));
        assert!(!url.contains("secret"));
        let more = auth_url_with(
            &c,
            "http://127.0.0.1:5000",
            &Pkce::new(),
            "xyz",
            &[METADATA_SCOPE, METADATA_SCOPE],
        );
        assert!(more.contains(
            "openid%20email%20https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fgmail.metadata&"
        ));
    }

    #[test]
    fn the_redirect_gives_the_code_and_checks_the_state() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let browser = std::thread::spawn(move || {
            let get = |path: &str| {
                let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
                s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                    .unwrap();
                let mut reply = String::new();
                let _ = std::io::Read::read_to_string(&mut s, &mut reply);
                reply
            };
            let favicon = get("/favicon.ico");
            let page = get("/?state=abc&code=4%2F0Ab_c&scope=x");
            (favicon, page)
        });
        let code = wait_for_code(&listener, "abc", Duration::from_secs(5)).unwrap();
        assert_eq!(code, "4/0Ab_c");
        let (favicon, page) = browser.join().unwrap();
        assert!(favicon.starts_with("HTTP/1.1 404"));
        assert!(page.contains("Gmail is connected to Sokki."));

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(b"GET /?state=evil&code=1 HTTP/1.1\r\n\r\n")
                .unwrap();
        });
        assert!(matches!(
            wait_for_code(&listener, "abc", Duration::from_secs(5)),
            Err(GmailError::Cancelled(_))
        ));
    }

    #[test]
    fn a_denied_consent_is_a_cancel() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(b"GET /?state=abc&error=access_denied HTTP/1.1\r\n\r\n")
                .unwrap();
        });
        assert_eq!(
            wait_for_code(&listener, "abc", Duration::from_secs(5)),
            Err(GmailError::Cancelled("access_denied".into()))
        );
    }

    #[test]
    fn the_email_comes_from_the_id_token() {
        let payload = URL_SAFE_NO_PAD
            .encode(br#"{"iss":"https://accounts.google.com","email":"Lena.Berg@example.com"}"#);
        assert_eq!(
            email_from_id_token(&format!("h.{payload}.sig")).as_deref(),
            Some("lena.berg@example.com")
        );
        assert_eq!(email_from_id_token("garbage"), None);
    }

    #[test]
    fn the_message_is_valid_utf8_mime() {
        let mail = Mail {
            from: Some("Lena Berg <lena.berg@example.com>".into()),
            to: vec![
                Recipient {
                    name: Some("Jürgen Müller".into()),
                    email: "j@example.com".into(),
                },
                Recipient {
                    name: Some("Anna".into()),
                    email: "anna@example.com".into(),
                },
                Recipient {
                    name: None,
                    email: "x@example.com".into(),
                },
            ],
            subject: "Notizen: Übergabe „Launch“ mit einem sehr langen Titel, der umbricht".into(),
            body: "Hier sind die Notizen.\n\n• Größe prüfen\n".into(),
        };
        let raw = String::from_utf8(rfc2822(&mail)).unwrap();
        assert!(raw.is_ascii());
        assert!(raw.starts_with("From: \"Lena Berg\" <lena.berg@example.com>\r\nTo: =?UTF-8?B?"));
        assert!(raw
            .contains(" <j@example.com>,\r\n \"Anna\" <anna@example.com>,\r\n x@example.com\r\n"));
        for line in raw.lines() {
            assert!(line.len() <= 78, "{line}");
        }
        // Subject decodes back.
        let subject: String = raw
            .split("Subject: ")
            .nth(1)
            .unwrap()
            .split("\r\nMIME")
            .next()
            .unwrap()
            .split("\r\n ")
            .map(|w| {
                let b64 = w.trim_start_matches("=?UTF-8?B?").trim_end_matches("?=");
                String::from_utf8(STANDARD.decode(b64).unwrap()).unwrap()
            })
            .collect();
        assert_eq!(subject, mail.subject);
        let body64: String = raw.split("\r\n\r\n").nth(1).unwrap().replace("\r\n", "");
        let body = String::from_utf8(STANDARD.decode(body64).unwrap()).unwrap();
        assert_eq!(body, "Hier sind die Notizen.\r\n\r\n• Größe prüfen\r\n");
    }
}

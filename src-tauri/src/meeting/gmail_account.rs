//! The connected Gmail account for sending notes (`meeting_mail::gmail`).
//!
//! - The OAuth client comes in at build time: `SOKKI_GOOGLE_CLIENT_ID` and
//!   `SOKKI_GOOGLE_CLIENT_SECRET` (card privacy-google keeps them). A build
//!   without them has no Gmail option.
//! - The refresh token lives in the macOS Keychain (service
//!   `org.sokki.gmail`), never in a file; the connected address is in
//!   `meetings.gmail_address`, to show it.
//! - Disconnecting revokes the token at Google and deletes it here.

use meeting_mail::gmail::{self, Client, Gmail, TokenStore};
use tauri::AppHandle;

const SERVICE: &str = "org.sokki.gmail";
const ACCOUNT: &str = "refresh_token";
/// The scopes the stored token has, space-separated, next to it.
const SCOPES_ACCOUNT: &str = "scopes";

/// Whether Google has reviewed Sokki's Gmail access. Until it has, the
/// Google project (sokki-515722) is in test mode and Google shows "Access
/// blocked" to everyone but its test users, so the Gmail option shows only
/// with `meetings.gmail_testing` on (Robin, the test user). Set this to
/// true once Google's review passes (card privacy-google).
pub const GOOGLE_VERIFIED: bool = false;

/// Whether to offer Gmail at all: this build has the client, and Google
/// has reviewed the app or this Mac is a test user's.
pub fn offered(app: &AppHandle) -> bool {
    client().is_some()
        && (GOOGLE_VERIFIED || crate::settings::get_settings(app).meetings.gmail_testing)
}

/// Sokki's OAuth client, if this build has one.
pub fn client() -> Option<Client> {
    let id = option_env!("SOKKI_GOOGLE_CLIENT_ID").filter(|s| !s.is_empty())?;
    let secret = option_env!("SOKKI_GOOGLE_CLIENT_SECRET").unwrap_or("");
    Some(Client {
        id: id.to_string(),
        secret: secret.to_string(),
    })
}

/// The refresh token in the login Keychain.
#[derive(Debug, Default, Clone, Copy)]
pub struct Keychain;

impl TokenStore for Keychain {
    fn get(&self) -> Option<String> {
        security_framework::passwords::get_generic_password(SERVICE, ACCOUNT)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
    }

    fn set(&self, refresh_token: &str) -> Result<(), String> {
        security_framework::passwords::set_generic_password(
            SERVICE,
            ACCOUNT,
            refresh_token.as_bytes(),
        )
        .map_err(|e| e.to_string())
    }

    fn delete(&self) {
        let _ = security_framework::passwords::delete_generic_password(SERVICE, ACCOUNT);
    }
}

/// Gmail as the sender, when connected.
pub fn sender(app: &AppHandle) -> Option<Gmail<Keychain>> {
    let client = client()?;
    let address = crate::settings::get_settings(app).meetings.gmail_address?;
    Keychain.get()?;
    Some(Gmail::new(client, Keychain, Some(address)))
}

/// The connected address, if any.
pub fn connected(app: &AppHandle) -> Option<String> {
    sender(app).and_then(|_| crate::settings::get_settings(app).meetings.gmail_address)
}

fn save_address(app: &AppHandle, address: Option<String>) {
    let mut settings = crate::settings::get_settings(app);
    settings.meetings.gmail_address = address;
    crate::settings::write_settings(app, settings);
}

/// The scopes the stored token was granted (empty when not connected, or
/// connected before Sokki kept them: then only `gmail.send openid email`).
pub fn granted_scopes() -> Vec<String> {
    Keychain.get().map_or_else(Vec::new, |_| {
        security_framework::passwords::get_generic_password(SERVICE, SCOPES_ACCOUNT)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_else(|| gmail::SCOPES.to_string())
            .split_whitespace()
            .map(str::to_string)
            .collect()
    })
}

/// Opens Google's consent page in the browser and waits (up to 5 min) for
/// the answer. Returns the connected address.
/// Asks for sending and the address only (`gmail::SCOPES`): the mail
/// before a meeting, which also wanted to see replies, was dropped (Robin,
/// 2026-11-25), so Google's review covers `gmail.send` alone.
pub fn connect(app: &AppHandle) -> Result<String, String> {
    connect_with(app, &[])
}

/// [`connect`] asking for `extra_scopes` too.
pub fn connect_with(app: &AppHandle, extra_scopes: &[&str]) -> Result<String, String> {
    if !offered(app) {
        return Err("Gmail isn't available yet: Google is still reviewing Sokki.".into());
    }
    let client = client().ok_or("This build of Sokki has no Gmail connection.")?;
    let signed = gmail::sign_in_with(&client, extra_scopes, |url| {
        if let Err(e) = std::process::Command::new("open").arg(url).spawn() {
            log::warn!("Gmail: opening the browser failed: {e}");
        }
    })
    .map_err(|e| e.to_string())?;
    Keychain.set(&signed.refresh_token)?;
    security_framework::passwords::set_generic_password(
        SERVICE,
        SCOPES_ACCOUNT,
        signed.scopes.join(" ").as_bytes(),
    )
    .map_err(|e| e.to_string())?;
    let address = signed.email.unwrap_or_else(|| "Gmail".to_string());
    save_address(app, Some(address.clone()));
    log::info!("Gmail connected");
    Ok(address)
}

/// Revokes the token at Google, deletes it from the Keychain, forgets the
/// address. Local cleanup happens even when Google can't be reached.
pub fn disconnect(app: &AppHandle) -> Result<(), String> {
    let revoked = match Keychain.get() {
        Some(token) => gmail::revoke(&token).map_err(|e| e.to_string()),
        None => Ok(()),
    };
    Keychain.delete();
    let _ = security_framework::passwords::delete_generic_password(SERVICE, SCOPES_ACCOUNT);
    save_address(app, None);
    log::info!(
        "Gmail disconnected (revoked at Google: {})",
        revoked.is_ok()
    );
    revoked
}

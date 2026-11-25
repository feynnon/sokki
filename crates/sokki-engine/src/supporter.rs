//! Supporter codes. Whoever paid Sokki at least €20 in total may switch off
//! the Sokki line under summaries (Robin, 2026-11-24). Sokki has no
//! accounts, so the proof is a code signed with Sokki's key and checked
//! here, offline: `scripts/supporter-code.mjs` makes them, and so will the
//! counter Worker once a payment provider is live.
//!
//! Format: `sokki-<base64url JSON>.<base64url Ed25519 signature over the
//! base64url JSON text>`; the JSON is `{"v":1,"via":"github","at":"2026-11-24"}`.
//! No name, no mail address, no amount: the signer only signs for people
//! who reached €20.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::Deserialize;

/// Sokki's public key; the private one is Robin's
/// (`~/.config/sokki/supporter-signing-key.jwk`).
const PUBLIC_KEY: [u8; 32] = [
    0x2c, 0xb3, 0x20, 0x7e, 0xf2, 0xc6, 0xf7, 0xf4, 0xa5, 0x7d, 0x71, 0x39, 0x11, 0xc7, 0x31,
    0xe5, 0x21, 0x84, 0x9f, 0x84, 0x22, 0xa3, 0xbe, 0xa3, 0x15, 0x47, 0x05, 0xd7, 0x09, 0x89,
    0xa6, 0x4c,
];

/// What a valid code says.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Supporter {
    /// How the payment was checked: "github", "manual", …
    pub via: String,
    /// The day the code was made, "YYYY-MM-DD".
    pub at: String,
}

#[derive(Deserialize)]
struct Payload {
    v: u32,
    via: String,
    at: String,
}

/// The supporter behind `code`, if it is a valid code. Spaces and line
/// breaks from pasting are ignored.
pub fn verify(code: &str) -> Option<Supporter> {
    verify_with(code, &VerifyingKey::from_bytes(&PUBLIC_KEY).ok()?)
}

fn verify_with(code: &str, key: &VerifyingKey) -> Option<Supporter> {
    let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    let (payload, sig) = code.strip_prefix("sokki-")?.split_once('.')?;
    let sig: [u8; 64] = URL_SAFE_NO_PAD.decode(sig).ok()?.try_into().ok()?;
    key.verify_strict(payload.as_bytes(), &Signature::from_bytes(&sig))
        .ok()?;
    let p: Payload = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;
    (p.v == 1).then_some(Supporter {
        via: p.via,
        at: p.at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway key pair, not Sokki's: the code below was made with
    /// `scripts/supporter-code.mjs mint github` and its private key, so this
    /// also checks that the script and the app agree on the format.
    const TEST_KEY: [u8; 32] = [
        0x50, 0xaf, 0x85, 0xf2, 0xa4, 0xfa, 0x5c, 0x73, 0x6f, 0xef, 0x91, 0xe0, 0x61, 0xde, 0x6e,
        0x30, 0x6a, 0x45, 0xe5, 0x91, 0xf1, 0x1e, 0x00, 0xe0, 0x42, 0x42, 0xee, 0xeb, 0x89, 0xbb,
        0x11, 0x5d,
    ];
    const CODE: &str = "sokki-eyJ2IjoxLCJ2aWEiOiJnaXRodWIiLCJhdCI6IjIwMjYtMTEtMjQifQ.odvg0vzEWKDNcpcKGF2S2ZkOU4OMMz-6FpthGTZe8--JCZ7fxU6o53ejiICQgWDGovbTUls6JEynJIPbMyhVAw";

    fn key() -> VerifyingKey {
        VerifyingKey::from_bytes(&TEST_KEY).unwrap()
    }

    #[test]
    fn a_code_from_the_script_is_valid() {
        assert_eq!(
            verify_with(CODE, &key()),
            Some(Supporter {
                via: "github".into(),
                at: "2026-11-24".into()
            })
        );
        // Pasted with a line break and spaces.
        let pasted = format!("  {}\n{}  ", &CODE[..40], &CODE[40..]);
        assert!(verify_with(&pasted, &key()).is_some());
    }

    #[test]
    fn anything_else_is_not() {
        // Not signed with Sokki's key.
        assert_eq!(verify(CODE), None);
        // One changed letter in the payload or the signature.
        let tampered = CODE.replacen("eyJ2Ijox", "eyJ2Ijoy", 1);
        assert_eq!(verify_with(&tampered, &key()), None);
        let bad_sig = format!("{}A", &CODE[..CODE.len() - 1]);
        assert_eq!(verify_with(&bad_sig, &key()), None);
        for junk in ["", "sokki-", "sokki-abc.def", "hello", "sokki-.."] {
            assert_eq!(verify_with(junk, &key()), None, "{junk}");
        }
    }

    #[test]
    fn the_public_key_is_a_valid_key() {
        assert!(VerifyingKey::from_bytes(&PUBLIC_KEY).is_ok());
    }
}

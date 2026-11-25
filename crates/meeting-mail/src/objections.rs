//! People who asked for no more mail. Kept on this computer only, in one
//! JSON file; nobody on the list gets a mail from Sokki again (BGH VI ZR
//! 134/15). Someone gets on it by replying "stop" to a mail (read by the
//! mail app adapter), or by hand in Settings.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How someone got on the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// They replied "stop" to a mail.
    Reply,
    /// The person using Sokki added them.
    ByHand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Objection {
    /// Lower case.
    pub email: String,
    /// RFC 3339 or "YYYY-MM-DD", as the host gives it.
    pub added: String,
    pub source: Source,
}

/// The list, read from and written to one file.
#[derive(Debug, Clone, Default)]
pub struct Objections {
    path: Option<PathBuf>,
    list: Vec<Objection>,
}

impl Objections {
    /// Loads the list at `path`; a missing or unreadable file is an empty
    /// list (an unreadable one is kept aside as `<file>.corrupt`, never
    /// overwritten, since it holds people's objections).
    pub fn load(path: &Path) -> Self {
        let list = match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                log::warn!("Mail objection list unreadable ({e}); kept as .corrupt");
                let _ = std::fs::rename(path, path.with_extension("corrupt"));
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
        Self {
            path: Some(path.to_path_buf()),
            list,
        }
    }

    /// A list that lives in memory only (tests, previews).
    pub fn in_memory(list: Vec<Objection>) -> Self {
        Self { path: None, list }
    }

    pub fn contains(&self, email: &str) -> bool {
        let email = email.trim().to_lowercase();
        self.list.iter().any(|o| o.email == email)
    }

    pub fn list(&self) -> &[Objection] {
        &self.list
    }

    /// Adds `email` and saves. False when it is no address or already listed.
    pub fn add(&mut self, email: &str, added: &str, source: Source) -> std::io::Result<bool> {
        let Some(email) = crate::normalize_email(email) else {
            return Ok(false);
        };
        if self.contains(&email) {
            return Ok(false);
        }
        self.list.push(Objection {
            email,
            added: added.to_string(),
            source,
        });
        self.save()?;
        Ok(true)
    }

    /// Removes `email` (only by hand, when the person says they may get
    /// mail again) and saves.
    pub fn remove(&mut self, email: &str) -> std::io::Result<bool> {
        let email = email.trim().to_lowercase();
        let before = self.list.len();
        self.list.retain(|o| o.email != email);
        if self.list.len() == before {
            return Ok(false);
        }
        self.save()?;
        Ok(true)
    }

    fn save(&self) -> std::io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        let text = serde_json::to_string_pretty(&self.list).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }
}

/// Whether a reply's first words say "no more mail": "stop", "stopp",
/// "unsubscribe", "abmelden", in any case, with punctuation around.
pub fn is_stop_reply(first_line: &str) -> bool {
    let words: Vec<String> = first_line
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(3)
        .map(str::to_lowercase)
        .collect();
    let Some(first) = words.first() else {
        return false;
    };
    ["stop", "stopp", "unsubscribe", "abmelden", "abbestellen"].contains(&first.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_survives_a_restart_and_ignores_case() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mail_objections.json");
        let mut o = Objections::load(&path);
        assert!(o
            .add("Anna@Example.com", "2026-11-24", Source::Reply)
            .unwrap());
        assert!(!o
            .add("anna@example.com", "2026-11-25", Source::ByHand)
            .unwrap());
        assert!(!o.add("no address", "2026-11-25", Source::ByHand).unwrap());
        let o = Objections::load(&path);
        assert!(o.contains("ANNA@example.com"));
        assert_eq!(o.list().len(), 1);
        let mut o = o;
        assert!(o.remove("anna@example.com").unwrap());
        assert!(!Objections::load(&path).contains("anna@example.com"));
    }

    #[test]
    fn an_unreadable_list_is_kept_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mail_objections.json");
        std::fs::write(&path, "{broken").unwrap();
        let o = Objections::load(&path);
        assert!(o.list().is_empty());
        assert!(path.with_extension("corrupt").exists());
    }

    #[test]
    fn stop_replies_are_recognised() {
        for yes in [
            "stop",
            "Stopp!",
            "STOP please",
            "  stopp.",
            "Unsubscribe",
            "Abmelden bitte",
        ] {
            assert!(is_stop_reply(yes), "{yes}");
        }
        for no in [
            "",
            "Thanks, great notes",
            "Don't stop",
            "Danke! Stopp ich lese das später",
        ] {
            assert!(!is_stop_reply(no), "{no}");
        }
    }
}

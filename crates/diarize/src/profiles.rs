//! Known voices, persisted as a small JSON file.
//!
//! Each name keeps up to [`MAX_EMBEDDINGS_PER_NAME`] L2-normalised
//! embeddings (one per enrollment, oldest dropped first). A query matches a
//! name by the best cosine similarity against any of its embeddings, so a
//! person enrolled once at the desk and once over a phone speaker is
//! recognised in both conditions.
//!
//! Embeddings are only comparable when they come from the same embedding
//! model; profiles enrolled with another model are not detected
//! automatically (only a different dimension is), so clear the store when
//! switching models.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::vector::{cosine, l2_normalize};

/// Embeddings kept per name; enrolling more drops the oldest.
pub const MAX_EMBEDDINGS_PER_NAME: usize = 16;

/// One known person: a name, the addresses they're invited with, and,
/// only when they agreed, their voice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceProfile {
    pub name: String,
    /// L2-normalised embeddings, oldest first. Empty for a person whose
    /// voice isn't kept: they are never recognised by voice.
    #[serde(default)]
    pub embeddings: Vec<Vec<f32>>,
    /// The person's email addresses, so calendar invitees can be named.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emails: Vec<String>,
    /// When the user ticked that this person agrees to being recognised by
    /// voice (RFC 3339). A voice is only kept with it (GDPR Art. 7(1)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consent_at: Option<String>,
    /// When the voice was last added or recognised (RFC 3339): voices
    /// unused for a year are deleted ([`VoiceProfiles::forget_unused`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_heard: Option<String>,
    /// Other names this person had ("Mara" before "Mara Lind"): a
    /// speaker or a call name with one of them is this person.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
}

impl VoiceProfile {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            embeddings: Vec::new(),
            emails: Vec::new(),
            consent_at: None,
            last_heard: None,
            aliases: Vec::new(),
        }
    }

    /// Whether `name` is this person: their name or one of their old ones,
    /// in any case and spacing.
    pub fn is_called(&self, name: &str) -> bool {
        let n = norm(name);
        norm(&self.name) == n || self.aliases.iter().any(|a| norm(a) == n)
    }
}

/// A name compared without case and extra spaces.
fn norm(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ProfilesFile {
    version: u32,
    profiles: Vec<VoiceProfile>,
}

/// A store of known voices backed by a JSON file. Changes are written to the
/// file immediately.
#[derive(Debug)]
pub struct VoiceProfiles {
    path: PathBuf,
    threshold: f32,
    profiles: Vec<VoiceProfile>,
}

impl VoiceProfiles {
    /// Default minimum cosine similarity for [`identify`](Self::identify),
    /// for the default embedding model (3D-Speaker ERes2Net VoxCeleb). On
    /// the synthetic fixtures the same voice scored 0.97 (0.78 for a clean
    /// enrollment vs. the same voice over a phone speaker) and different
    /// voices at most 0.40, except two near-identical TTS voices (0.90).
    pub const DEFAULT_THRESHOLD: f32 = 0.6;

    /// Opens the store at `path`. A missing file gives an empty store; the
    /// file is created on the first change.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let profiles = match fs::read_to_string(&path) {
            Ok(text) => {
                let file: ProfilesFile = serde_json::from_str(&text)
                    .with_context(|| format!("parse voice profiles {}", path.display()))?;
                file.profiles
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                return Err(e).with_context(|| format!("read voice profiles {}", path.display()))
            }
        };
        Ok(Self {
            path,
            threshold: Self::DEFAULT_THRESHOLD,
            profiles,
        })
    }

    /// Sets the minimum cosine similarity for a match.
    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.threshold = threshold;
        self
    }

    pub fn threshold(&self) -> f32 {
        self.threshold
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Names in enrollment order.
    pub fn names(&self) -> Vec<String> {
        self.profiles.iter().map(|p| p.name.clone()).collect()
    }

    pub fn profiles(&self) -> &[VoiceProfile] {
        &self.profiles
    }

    /// Adds `embedding` to `name` (creating the profile if needed) and saves.
    pub fn enroll(&mut self, name: &str, embedding: &[f32]) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            bail!("voice profile name is empty");
        }
        if embedding.is_empty() || embedding.iter().any(|x| !x.is_finite()) {
            bail!("invalid embedding for voice profile {name:?}");
        }
        let e = l2_normalize(embedding);
        if let Some(dim) = self.dim() {
            if dim != e.len() {
                bail!(
                    "embedding has {} dimensions, stored profiles have {dim}; \
                     profiles from another embedding model?",
                    e.len()
                );
            }
        }
        match self.profiles.iter_mut().find(|p| p.is_called(name)) {
            Some(p) => {
                p.embeddings.push(e);
                let excess = p.embeddings.len().saturating_sub(MAX_EMBEDDINGS_PER_NAME);
                p.embeddings.drain(..excess);
            }
            None => {
                let mut p = VoiceProfile::new(name);
                p.embeddings.push(e);
                self.profiles.push(p);
            }
        }
        self.save()
    }

    /// The person called `name`, if known.
    pub fn person(&self, name: &str) -> Option<&VoiceProfile> {
        self.profiles
            .iter()
            .find(|p| p.name == name)
            .or_else(|| self.profiles.iter().find(|p| p.is_called(name)))
    }

    /// The name Sokki uses for the person called `name` (their name for an
    /// old name or another spelling), or `name` itself for someone unknown.
    pub fn canonical(&self, name: &str) -> String {
        self.person(name)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| name.trim().to_string())
    }

    /// The person called `from` is now called `to`: their profile is
    /// renamed, or merged into `to`'s when Sokki knows `to` already (voices,
    /// addresses, consent, old names). `from` stays as an old name. Returns
    /// whether `from` was known. Saves.
    pub fn rename_person(&mut self, from: &str, to: &str) -> Result<bool> {
        let to = to.trim();
        if to.is_empty() {
            bail!("voice profile name is empty");
        }
        let Some(i) = self.profiles.iter().position(|p| p.is_called(from)) else {
            return Ok(false);
        };
        match self
            .profiles
            .iter()
            .position(|p| p.is_called(to))
            .filter(|&j| j != i)
        {
            Some(j) => {
                let old = self.profiles.remove(i);
                let j = if j > i { j - 1 } else { j };
                let p = &mut self.profiles[j];
                // Oldest first, the newest kept, as `enroll` does.
                p.embeddings.splice(0..0, old.embeddings);
                let excess = p.embeddings.len().saturating_sub(MAX_EMBEDDINGS_PER_NAME);
                p.embeddings.drain(..excess);
                for e in old.emails {
                    if !p.emails.contains(&e) {
                        p.emails.push(e);
                    }
                }
                p.consent_at = match (p.consent_at.take(), old.consent_at) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
                p.last_heard = p.last_heard.take().max(old.last_heard);
                for a in std::iter::once(old.name).chain(old.aliases) {
                    if !p.is_called(&a) {
                        p.aliases.push(a);
                    }
                }
            }
            None => {
                let p = &mut self.profiles[i];
                let old = std::mem::replace(&mut p.name, to.to_string());
                if !p.is_called(&old) {
                    p.aliases.push(old);
                }
                p.aliases.retain(|a| norm(a) != norm(to));
            }
        }
        self.save()?;
        Ok(true)
    }

    /// Changes the person called `name` (created if unknown) and saves.
    pub fn update(&mut self, name: &str, f: impl FnOnce(&mut VoiceProfile)) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            bail!("voice profile name is empty");
        }
        let i = match self
            .profiles
            .iter()
            .position(|p| p.name == name)
            .or_else(|| self.profiles.iter().position(|p| p.is_called(name)))
        {
            Some(i) => i,
            None => {
                self.profiles.push(VoiceProfile::new(name));
                self.profiles.len() - 1
            }
        };
        f(&mut self.profiles[i]);
        for e in &mut self.profiles[i].emails {
            *e = e.trim().to_lowercase();
        }
        self.profiles[i].emails.retain(|e| e.contains('@'));
        self.profiles[i].emails.dedup();
        self.save()
    }

    /// The name of the person with this email address.
    pub fn name_for_email(&self, email: &str) -> Option<&str> {
        let email = email.trim().to_lowercase();
        self.profiles
            .iter()
            .find(|p| p.emails.iter().any(|e| *e == email))
            .map(|p| p.name.as_str())
    }

    /// Drops the voices of people not heard since `cutoff` (RFC 3339, so a
    /// string comparison orders them). Their name and addresses stay.
    /// Returns how many voices were dropped; saves when any were.
    pub fn forget_unused(&mut self, cutoff: &str) -> Result<usize> {
        let mut n = 0;
        for p in &mut self.profiles {
            let stale = p.last_heard.as_deref().is_some_and(|t| t < cutoff);
            if stale && !p.embeddings.is_empty() {
                p.embeddings.clear();
                p.consent_at = None;
                n += 1;
            }
        }
        if n > 0 {
            self.save()?;
        }
        Ok(n)
    }

    /// Removes `name`. Returns whether it existed. Saves when it did.
    pub fn remove(&mut self, name: &str) -> Result<bool> {
        let before = self.profiles.len();
        self.profiles.retain(|p| p.name != name);
        if self.profiles.len() == before {
            return Ok(false);
        }
        self.save()?;
        Ok(true)
    }

    /// Best similarity per name, highest first.
    pub fn scores(&self, embedding: &[f32]) -> Vec<(String, f32)> {
        let mut out: Vec<(String, f32)> = self
            .profiles
            .iter()
            .filter_map(|p| {
                p.embeddings
                    .iter()
                    .filter(|e| e.len() == embedding.len())
                    .map(|e| cosine(e, embedding))
                    .fold(None, |best: Option<f32>, s| {
                        Some(best.map_or(s, |b| b.max(s)))
                    })
                    .map(|s| (p.name.clone(), s))
            })
            .collect();
        out.sort_by(|a, b| b.1.total_cmp(&a.1));
        out
    }

    /// The best-matching name and its cosine similarity, if it reaches the
    /// threshold.
    pub fn identify(&self, embedding: &[f32]) -> Option<(String, f32)> {
        self.scores(embedding)
            .into_iter()
            .next()
            .filter(|(_, s)| *s >= self.threshold)
    }

    /// Writes the store to its file (atomically, via a temp file).
    pub fn save(&self) -> Result<()> {
        if let Some(dir) = self.path.parent().filter(|d| !d.as_os_str().is_empty()) {
            fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        }
        let file = ProfilesFile {
            version: 1,
            profiles: self.profiles.clone(),
        };
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&file)?)
            .with_context(|| format!("write {}", tmp.display()))?;
        fs::rename(&tmp, &self.path).with_context(|| format!("replace {}", self.path.display()))?;
        Ok(())
    }

    fn dim(&self) -> Option<usize> {
        self.profiles
            .iter()
            .flat_map(|p| p.embeddings.first())
            .map(|e| e.len())
            .next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "diarize-profiles-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        dir.join("voices.json")
    }

    #[test]
    fn missing_file_is_empty() {
        let p = VoiceProfiles::open(tmp_path("missing")).unwrap();
        assert!(p.names().is_empty());
        assert_eq!(p.identify(&[1.0, 0.0]), None);
    }

    #[test]
    fn enroll_identify_remove_and_persist() {
        let path = tmp_path("roundtrip");
        let mut p = VoiceProfiles::open(&path).unwrap().with_threshold(0.8);
        p.enroll("Robin", &[2.0, 0.0, 0.0]).unwrap();
        p.enroll("Anna", &[0.0, 1.0, 0.0]).unwrap();
        assert_eq!(p.names(), vec!["Robin", "Anna"]);

        let (name, score) = p.identify(&[0.9, 0.1, 0.0]).unwrap();
        assert_eq!(name, "Robin");
        assert!(score > 0.99);
        assert_eq!(p.identify(&[0.0, 0.0, 1.0]), None, "below threshold");

        // Reload from disk.
        let mut q = VoiceProfiles::open(&path).unwrap().with_threshold(0.8);
        assert_eq!(q.names(), vec!["Robin", "Anna"]);
        assert_eq!(q.identify(&[0.0, 3.0, 0.1]).unwrap().0, "Anna");
        // Stored embeddings are normalised.
        assert!((q.profiles()[0].embeddings[0][0] - 1.0).abs() < 1e-6);

        assert!(q.remove("Robin").unwrap());
        assert!(!q.remove("Robin").unwrap());
        assert_eq!(VoiceProfiles::open(&path).unwrap().names(), vec!["Anna"]);
    }

    #[test]
    fn several_embeddings_per_name_match_by_best() {
        let mut p = VoiceProfiles::open(tmp_path("multi"))
            .unwrap()
            .with_threshold(0.9);
        p.enroll("Robin", &[1.0, 0.0, 0.0]).unwrap(); // desk mic
        p.enroll("Robin", &[0.0, 0.0, 1.0]).unwrap(); // phone speaker
        assert_eq!(p.profiles()[0].embeddings.len(), 2);
        assert_eq!(p.identify(&[0.0, 0.05, 1.0]).unwrap().0, "Robin");
        assert_eq!(p.identify(&[1.0, 0.05, 0.0]).unwrap().0, "Robin");
    }

    #[test]
    fn caps_embeddings_per_name() {
        let mut p = VoiceProfiles::open(tmp_path("cap")).unwrap();
        assert!(p.profiles().is_empty());
        for i in 0..(MAX_EMBEDDINGS_PER_NAME + 3) {
            p.enroll("X", &[1.0, i as f32]).unwrap();
        }
        let embs = &p.profiles()[0].embeddings;
        assert_eq!(embs.len(), MAX_EMBEDDINGS_PER_NAME);
        // The oldest three were dropped: the first kept one is i = 3.
        assert!((embs[0][1] / embs[0][0] - 3.0).abs() < 1e-4);
    }

    #[test]
    fn rejects_bad_input() {
        let mut p = VoiceProfiles::open(tmp_path("bad")).unwrap();
        assert!(p.enroll("  ", &[1.0]).is_err());
        assert!(p.enroll("A", &[]).is_err());
        assert!(p.enroll("A", &[f32::NAN, 1.0]).is_err());
        p.enroll("A", &[1.0, 0.0]).unwrap();
        assert!(
            p.enroll("B", &[1.0, 0.0, 0.0]).is_err(),
            "dimension mismatch"
        );
        // Queries of another dimension never match.
        assert_eq!(p.identify(&[1.0, 0.0, 0.0]), None);
    }

    #[test]
    fn people_without_a_voice_and_emails() {
        let path = tmp_path("people");
        let mut p = VoiceProfiles::open(&path).unwrap().with_threshold(0.5);
        p.update("Mara Keller", |m| {
            m.emails = vec![" Mara@Example.com ".into(), "not an address".into()]
        })
        .unwrap();
        assert_eq!(
            p.person("Mara Keller").unwrap().emails,
            ["mara@example.com"]
        );
        assert_eq!(p.name_for_email("MARA@example.com"), Some("Mara Keller"));
        // No voice: never recognised.
        assert_eq!(p.identify(&[1.0, 0.0]), None);
        p.enroll("Mara Keller", &[1.0, 0.0]).unwrap();
        assert_eq!(p.identify(&[1.0, 0.1]).unwrap().0, "Mara Keller");
        // Old files (no emails field) still load; new fields persist.
        let q = VoiceProfiles::open(&path).unwrap();
        assert_eq!(q.person("Mara Keller").unwrap().emails.len(), 1);
        let old = tmp_path("old");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(
            &old,
            r#"{"version":1,"profiles":[{"name":"A","embeddings":[[1.0,0.0]]}]}"#,
        )
        .unwrap();
        assert_eq!(VoiceProfiles::open(&old).unwrap().names(), ["A"]);
    }

    #[test]
    fn forgets_voices_unused_for_a_year() {
        let mut p = VoiceProfiles::open(tmp_path("unused")).unwrap();
        p.enroll("Old", &[1.0, 0.0]).unwrap();
        p.enroll("New", &[0.0, 1.0]).unwrap();
        p.update("Old", |o| {
            o.last_heard = Some("2025-01-01T00:00:00Z".into())
        })
        .unwrap();
        p.update("New", |o| {
            o.last_heard = Some("2026-11-01T00:00:00Z".into())
        })
        .unwrap();
        assert_eq!(p.forget_unused("2025-11-25T00:00:00Z").unwrap(), 1);
        assert!(p.person("Old").unwrap().embeddings.is_empty());
        assert_eq!(p.person("New").unwrap().embeddings.len(), 1);
    }

    #[test]
    fn scores_sorted_descending() {
        let mut p = VoiceProfiles::open(tmp_path("scores")).unwrap();
        p.enroll("A", &[1.0, 0.0]).unwrap();
        p.enroll("B", &[0.0, 1.0]).unwrap();
        let s = p.scores(&[0.2, 1.0]);
        assert_eq!(s[0].0, "B");
        assert_eq!(s[1].0, "A");
    }

    #[test]
    fn one_person_under_any_spelling_and_old_name() {
        let path = tmp_path("rename");
        let mut p = VoiceProfiles::open(&path).unwrap();
        p.enroll("Mara", &[1.0, 0.0]).unwrap();
        p.update("Mara", |v| v.emails = vec!["mara@example.com".into()])
            .unwrap();
        // Another spelling is the same person, not a new one.
        p.enroll("mARA ", &[0.9, 0.1]).unwrap();
        assert_eq!(p.names(), vec!["Mara"]);
        assert_eq!(p.person("MARA").unwrap().embeddings.len(), 2);

        // Typed as the full name: renamed, the first name kept as an old one.
        assert!(p.rename_person("Mara", "Mara Lind").unwrap());
        assert_eq!(p.names(), vec!["Mara Lind"]);
        assert_eq!(p.canonical("mara"), "Mara Lind");
        assert_eq!(p.canonical("Mara  LIND"), "Mara Lind");
        assert_eq!(p.canonical("Jonas"), "Jonas");

        // Two records of one person: merged into the one asked for.
        p.update("Jo", |v| {
            v.emails = vec!["jo@example.com".into()];
            v.consent_at = Some("2026-11-20T10:00:00Z".into());
        })
        .unwrap();
        p.enroll("Jo", &[0.0, 1.0]).unwrap();
        p.update("Jo Weber", |v| v.emails = vec!["jo@example.com".into()])
            .unwrap();
        assert!(p.rename_person("Jo", "Jo Weber").unwrap());
        let jo = p.person("jo").unwrap();
        assert_eq!(jo.name, "Jo Weber");
        assert_eq!(jo.embeddings.len(), 1);
        assert_eq!(jo.emails, vec!["jo@example.com"]);
        assert!(jo.consent_at.is_some());
        assert_eq!(p.names(), vec!["Mara Lind", "Jo Weber"]);
        assert!(!p.rename_person("Nobody", "Somebody").unwrap());

        // Kept on disk.
        let q = VoiceProfiles::open(&path).unwrap();
        assert_eq!(q.canonical("Mara"), "Mara Lind");
        assert_eq!(q.canonical("jo"), "Jo Weber");
    }
}

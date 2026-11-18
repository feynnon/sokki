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

/// One known person.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceProfile {
    pub name: String,
    /// L2-normalised embeddings, oldest first.
    pub embeddings: Vec<Vec<f32>>,
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
        match self.profiles.iter_mut().find(|p| p.name == name) {
            Some(p) => {
                p.embeddings.push(e);
                let excess = p.embeddings.len().saturating_sub(MAX_EMBEDDINGS_PER_NAME);
                p.embeddings.drain(..excess);
            }
            None => self.profiles.push(VoiceProfile {
                name: name.to_string(),
                embeddings: vec![e],
            }),
        }
        self.save()
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
    fn scores_sorted_descending() {
        let mut p = VoiceProfiles::open(tmp_path("scores")).unwrap();
        p.enroll("A", &[1.0, 0.0]).unwrap();
        p.enroll("B", &[0.0, 1.0]).unwrap();
        let s = p.scores(&[0.2, 1.0]);
        assert_eq!(s[0].0, "B");
        assert_eq!(s[1].0, "A");
    }
}

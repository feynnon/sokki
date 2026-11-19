//! The speakers of a processed meeting, kept next to its audio so a name
//! given later can be remembered as a voice.

use meeting_core::pipeline::SpeakerInfo;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const SPEAKERS_FILE: &str = "speakers.json";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SavedSpeaker {
    pub name: String,
    pub embedding: Option<Vec<f32>>,
}

pub fn save(dir: &Path, speakers: &[SpeakerInfo]) {
    let saved: Vec<SavedSpeaker> = speakers
        .iter()
        .map(|s| SavedSpeaker {
            name: s.name.clone(),
            embedding: s.embedding.clone(),
        })
        .collect();
    if let Ok(text) = serde_json::to_string(&saved) {
        let _ = std::fs::write(dir.join(SPEAKERS_FILE), text);
    }
}

pub fn load(dir: &Path) -> Vec<SavedSpeaker> {
    std::fs::read_to_string(dir.join(SPEAKERS_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Renames a speaker in the note and in the speakers file. Returns the
/// speaker's voice embedding, if any, so the host can remember the voice.
pub fn rename(
    dir: &Path,
    note: Option<&Path>,
    from: &str,
    to: &str,
) -> anyhow::Result<Option<Vec<f32>>> {
    use anyhow::Context;
    let to = to.trim();
    if to.is_empty() {
        anyhow::bail!("The new name is empty.");
    }
    if let Some(path) = note {
        let md = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let renamed = meeting_core::note::rename_speaker_in_markdown(&md, from, to);
        std::fs::write(path, renamed).with_context(|| format!("saving {}", path.display()))?;
    }
    let mut speakers = load(dir);
    let embedding = speakers
        .iter()
        .find(|s| s.name == from)
        .and_then(|s| s.embedding.clone());
    for s in &mut speakers {
        if s.name == from {
            s.name = to.to_string();
        }
    }
    std::fs::write(dir.join(SPEAKERS_FILE), serde_json::to_string(&speakers)?)?;
    Ok(embedding)
}

/// Whether a name is still a placeholder ("Speaker 2", "Sprecher 3").
pub fn is_placeholder(name: &str) -> bool {
    let mut words = name.split_whitespace();
    matches!(
        (words.next(), words.next(), words.next()),
        (Some("Speaker" | "Sprecher" | "Sprecherin"), Some(n), None) if n.parse::<u32>().is_ok()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders() {
        assert!(is_placeholder("Speaker 2"));
        assert!(is_placeholder("Sprecher 10"));
        assert!(!is_placeholder("Anna"));
        assert!(!is_placeholder("Speaker Anna"));
    }
}

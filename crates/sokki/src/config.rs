//! `~/.config/sokki/sokki.toml`: every setting, in one file the person can
//! edit. Missing keys take their defaults; the daemon re-reads the file on
//! `sokki reload` (the Omarchy menu's "Settings" opens it and reloads).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sokki_engine::llm::LlmSettings;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub meetings: Meetings,
    pub dictation: Dictation,
    pub llm: LlmSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Meetings {
    /// How you appear in notes.
    pub user_name: String,
    /// Where notes go. `~/` is your home folder.
    pub notes_dir: String,
    /// Ask to record when a call app starts using the microphone.
    pub detect_calls: bool,
    /// Seconds the question waits; no answer means no recording.
    pub ask_seconds: u64,
    /// Tell speakers apart.
    pub speakers: bool,
    /// Summary, decisions and tasks with the language model.
    pub summary: bool,
    /// Names and terms the summary spells right.
    pub glossary: Vec<String>,
    /// End summaries with the Sokki line ("Summarized by AI. Local, free,
    /// open source. Dictation app and meeting notetaker: sokki.org"). Turning it off needs a valid
    /// `supporter_code` (at least €20 paid for Sokki).
    pub footer: bool,
    /// Supporter code, checked offline. Empty: none.
    pub supporter_code: String,
    /// Keep the audio this many days after the note is written (0: delete
    /// right away).
    pub keep_audio_days: u32,
    /// Record only the call app's own sound instead of everything the
    /// computer plays.
    ///
    /// OFF by default: browsers replace their audio stream mid-call (after a
    /// silence, seen in the Omarchy VM after ~12 s), and the recording then
    /// loses the call. Everything the computer plays is the reliable choice.
    pub call_app_only: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Dictation {
    /// Language hint ("de", "en"); empty lets the model decide.
    pub language: String,
    /// "type" (wtype), or "paste" (clipboard + Shift+Insert), or "clipboard"
    /// (copy only).
    pub output: String,
}

fn german() -> bool {
    std::env::var("LANG").is_ok_and(|l| l.starts_with("de"))
}

impl Default for Meetings {
    fn default() -> Self {
        Meetings {
            user_name: if german() { "Ich" } else { "Me" }.into(),
            notes_dir: "~/Documents/Sokki".into(),
            detect_calls: true,
            ask_seconds: 60,
            speakers: true,
            summary: true,
            // Words every Omarchy user says and the speech model doesn't
            // know (the demo call came out as "Omati" without them).
            glossary: vec!["Omarchy".into(), "Hyprland".into()],
            footer: true,
            supporter_code: String::new(),
            keep_audio_days: 0,
            call_app_only: false,
        }
    }
}

impl Default for Dictation {
    fn default() -> Self {
        Dictation {
            language: String::new(),
            output: "type".into(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            meetings: Meetings::default(),
            dictation: Dictation::default(),
            llm: LlmSettings::default(),
        }
    }
}

const HEADER: &str = "\
# Sokki settings. Edit and save; then `sokki reload` (the Omarchy menu does
# this for you). Delete a line to get its default back.
# All options: https://github.com/feynnon/sokki/blob/main/CONFIG.md

";

impl Config {
    /// Reads the file; writes it with defaults first if it is missing.
    pub fn load(path: &Path) -> Result<Config> {
        if !path.exists() {
            let cfg = Config::default();
            cfg.save(path)?;
            return Ok(cfg);
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("{} is not valid", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let body = toml::to_string_pretty(self)?;
        std::fs::write(path, format!("{HEADER}{body}"))
            .with_context(|| format!("writing {}", path.display()))
    }

    /// The settings processing needs.
    pub fn engine(&self) -> sokki_engine::Settings {
        let m = &self.meetings;
        sokki_engine::Settings {
            user_name: m.user_name.clone(),
            summary: m.summary,
            glossary: m.glossary.clone(),
            footer: m.footer,
            supporter_code: m.supporter_code.clone(),
            notes_dir: crate::paths::expand(&m.notes_dir),
            llm: self.llm.clone(),
            language: Some(if german() {
                meeting_core::lang::Language::German
            } else {
                meeting_core::lang::Language::English
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_partial_files() {
        let dir = std::env::temp_dir().join(format!("sokki-cfg-{}", std::process::id()));
        let path = dir.join("sokki.toml");
        let cfg = Config::load(&path).unwrap();
        assert!(path.exists());
        assert_eq!(Config::load(&path).unwrap(), cfg);
        std::fs::write(&path, "[meetings]\nuser_name = \"Robin\"\n").unwrap();
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.meetings.user_name, "Robin");
        assert!(cfg.meetings.detect_calls);
        assert_eq!(cfg.llm.model, "qwen3.5:9b");
        // A file from before the correction step was removed still loads.
        std::fs::write(&path, "[meetings]\ncorrection = true\nsummary = false\n").unwrap();
        let cfg = Config::load(&path).unwrap();
        assert!(!cfg.meetings.summary);
        let _ = std::fs::remove_dir_all(dir);
    }
}

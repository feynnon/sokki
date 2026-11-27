//! What agents and scripts may read through the `sokki` command: one
//! setting, off by default. Each step includes the one before.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Nothing (the default).
    #[default]
    Off,
    /// Meeting list, summaries, decisions, tasks, the person's own notes,
    /// and sections hooks added.
    Summaries,
    /// Plus the meeting transcripts.
    Transcripts,
    /// Plus the dictations.
    Dictations,
}

impl Scope {
    pub fn parse(s: &str) -> Result<Scope> {
        Ok(match s.trim() {
            "" | "off" | "none" | "false" => Scope::Off,
            "summaries" => Scope::Summaries,
            "transcripts" => Scope::Transcripts,
            "dictations" | "all" => Scope::Dictations,
            other => bail!(
                "unknown agent access {other:?}: use off, summaries, transcripts or dictations"
            ),
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Off => "off",
            Scope::Summaries => "summaries",
            Scope::Transcripts => "transcripts",
            Scope::Dictations => "dictations",
        }
    }

    pub fn transcripts(self) -> bool {
        self >= Scope::Transcripts
    }

    pub fn dictations(self) -> bool {
        self >= Scope::Dictations
    }

    /// Whether a unit of `kind` and `section` may be shown.
    pub fn allows(self, kind: &str, section: &str) -> bool {
        match (kind, section) {
            _ if self == Scope::Off => false,
            ("dictation", _) => self.dictations(),
            (_, "transcript") => self.transcripts(),
            _ => true,
        }
    }

    /// The narrower of the two (a hook's own scope never widens the
    /// setting).
    pub fn narrow(self, other: Scope) -> Scope {
        self.min(other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_step_includes_the_one_before() {
        assert!(!Scope::Off.allows("meeting", "summary"));
        assert!(Scope::Summaries.allows("meeting", "summary"));
        assert!(Scope::Summaries.allows("notes", "notes"));
        assert!(!Scope::Summaries.allows("meeting", "transcript"));
        assert!(Scope::Transcripts.allows("meeting", "transcript"));
        assert!(!Scope::Transcripts.allows("dictation", "dictation"));
        assert!(Scope::Dictations.allows("dictation", "dictation"));
        assert_eq!(Scope::Dictations.narrow(Scope::Summaries), Scope::Summaries);
        assert!(Scope::parse("maybe").is_err());
    }
}

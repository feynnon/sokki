//! `hooks.toml`: which of the person's commands run on which event.
//!
//! ```toml
//! [[hook]]
//! name    = "tasks-to-reminders"
//! on      = "meeting.note_ready"
//! run     = "./tasks-to-reminders.sh"     # /bin/sh -c, in this folder
//! match   = { app = ["Zoom", "Teams"], person = "Anna" }   # optional
//! scope   = ["summary", "tasks"]          # optional: only these on stdin
//! timeout = "5m"
//!
//! [[hook]]
//! name = "weekly-retro"
//! on   = "schedule"
//! cron = "0 17 * * fri"
//! run  = "./weekly-retro.sh"
//! ```

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::cron::Cron;
use super::event::{Meta, EVENTS, LATER, SCHEDULE};

pub const CONFIG_FILE: &str = "hooks.toml";

/// Unless the hook sets its own.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// What goes on a hook's stdin besides the event and its meta. Without a
/// `scope` a hook gets all of it; a scope narrows that, e.g. for a hook
/// that hands its input to a cloud model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    /// A meeting's summary and decisions.
    Summary,
    /// A meeting's action items.
    Tasks,
    /// A meeting's transcript: everything anyone said, so untrusted input.
    Transcript,
    /// The text of a dictation.
    Dictations,
    /// The person's own notes of a meeting.
    OwnNotes,
}

impl Scope {
    pub const ALL: [Scope; 5] = [
        Scope::Summary,
        Scope::Tasks,
        Scope::Transcript,
        Scope::Dictations,
        Scope::OwnNotes,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Summary => "summary",
            Scope::Tasks => "tasks",
            Scope::Transcript => "transcript",
            Scope::Dictations => "dictations",
            Scope::OwnNotes => "own_notes",
        }
    }

    fn parse(s: &str) -> Option<Scope> {
        Scope::ALL.into_iter().find(|x| x.as_str() == s.trim())
    }
}

/// Optional filters; every one given must match.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Match {
    /// The call app, any of these (case does not matter).
    pub app: Vec<String>,
    /// A participant, any of these; `"*"` means anyone at all.
    pub person: Vec<String>,
}

impl Match {
    pub fn is_empty(&self) -> bool {
        self.app.is_empty() && self.person.is_empty()
    }

    pub fn matches(&self, meta: &Meta) -> bool {
        let same = |a: &str, b: &str| a.trim().to_lowercase() == b.trim().to_lowercase();
        let app_ok = self.app.is_empty()
            || meta
                .app
                .as_deref()
                .is_some_and(|app| self.app.iter().any(|a| same(a, app)));
        let person_ok = self.person.is_empty()
            || self.person.iter().any(|p| {
                if p.trim() == "*" {
                    !meta.participants.is_empty()
                } else {
                    meta.participants.iter().any(|q| same(p, q))
                }
            });
        app_ok && person_ok
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hook {
    pub name: String,
    pub on: String,
    pub run: String,
    pub cron: Option<Cron>,
    pub matches: Match,
    /// Sorted, without duplicates.
    pub scope: Vec<Scope>,
    pub timeout: Duration,
    pub enabled: bool,
}

impl Hook {
    pub fn has(&self, scope: Scope) -> bool {
        self.scope.contains(&scope)
    }
}

/// The whole file, checked.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Config {
    /// The folder of `hooks.toml`; hooks run in it.
    pub dir: PathBuf,
    pub hooks: Vec<Hook>,
}

impl Config {
    pub fn get(&self, name: &str) -> Option<&Hook> {
        self.hooks.iter().find(|h| h.name == name)
    }

    /// Enabled hooks for `event` whose filters match.
    pub fn for_event<'a>(
        &'a self,
        event: &'a str,
        meta: &'a Meta,
    ) -> impl Iterator<Item = &'a Hook> {
        self.hooks
            .iter()
            .filter(move |h| h.enabled && h.on == event && h.matches.matches(meta))
    }
}

#[derive(Deserialize)]
struct RawFile {
    #[serde(default)]
    hook: Vec<RawHook>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    fn into_vec(self) -> Vec<String> {
        match self {
            OneOrMany::One(s) => vec![s],
            OneOrMany::Many(v) => v,
        }
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMatch {
    app: Option<OneOrMany>,
    person: Option<OneOrMany>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawTimeout {
    Seconds(u64),
    Text(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawHook {
    name: String,
    on: String,
    run: String,
    cron: Option<String>,
    #[serde(rename = "match")]
    matches: Option<RawMatch>,
    scope: Option<Vec<String>>,
    timeout: Option<RawTimeout>,
    enabled: Option<bool>,
}

/// "30s", "5m", "1h", "90" (seconds).
pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    let (num, unit) = match s.find(|c: char| !c.is_ascii_digit()) {
        Some(i) => (&s[..i], s[i..].trim()),
        None => (s, "s"),
    };
    let n: u64 = num.parse().ok()?;
    let secs = match unit {
        "s" | "sec" => n,
        "m" | "min" => n * 60,
        "h" => n * 3600,
        _ => return None,
    };
    (secs > 0).then(|| Duration::from_secs(secs))
}

/// A hook name is part of log file names and CLI calls.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Parses and checks `hooks.toml` text. `dir` is its folder.
pub fn parse(text: &str, dir: &Path) -> Result<Config> {
    let raw: RawFile = toml::from_str(text).context("hooks.toml is not valid TOML")?;
    let mut hooks: Vec<Hook> = Vec::new();
    for r in raw.hook {
        let name = r.name.trim().to_string();
        if !valid_name(&name) {
            bail!("hook name {name:?}: use letters, digits, '-', '_' or '.' (at most 64)");
        }
        if hooks.iter().any(|h| h.name == name) {
            bail!("two hooks are called {name:?}");
        }
        let on = r.on.trim().to_string();
        if LATER.contains(&on.as_str()) {
            bail!("hook {name:?}: the event {on:?} comes in a later version of Sokki");
        }
        if !EVENTS.contains(&on.as_str()) {
            bail!(
                "hook {name:?}: unknown event {on:?}; one of {}",
                EVENTS.join(", ")
            );
        }
        let cron = match (&r.cron, on == SCHEDULE) {
            (Some(c), true) => {
                Some(Cron::parse(c).with_context(|| format!("hook {name:?}: cron {c:?}"))?)
            }
            (None, true) => {
                bail!("hook {name:?}: on = \"schedule\" needs a cron, e.g. cron = \"0 17 * * fri\"")
            }
            (Some(_), false) => bail!("hook {name:?}: cron only works with on = \"schedule\""),
            (None, false) => None,
        };
        if r.run.trim().is_empty() {
            bail!("hook {name:?}: run is empty");
        }
        // No scope: everything.
        let mut scope = Vec::new();
        let named = r
            .scope
            .unwrap_or_else(|| Scope::ALL.iter().map(|s| s.as_str().to_string()).collect());
        for s in &named {
            let parsed = Scope::parse(s).with_context(|| {
                format!(
                    "hook {name:?}: unknown scope {s:?}; one of {}",
                    Scope::ALL.map(|s| s.as_str()).join(", ")
                )
            })?;
            if !scope.contains(&parsed) {
                scope.push(parsed);
            }
        }
        scope.sort();
        let timeout = match r.timeout {
            None => DEFAULT_TIMEOUT,
            Some(RawTimeout::Seconds(0)) => bail!("hook {name:?}: timeout must be more than 0"),
            Some(RawTimeout::Seconds(s)) => Duration::from_secs(s),
            Some(RawTimeout::Text(t)) => parse_duration(&t).with_context(|| {
                format!("hook {name:?}: timeout {t:?}; use e.g. \"30s\", \"5m\", \"1h\"")
            })?,
        };
        let matches = r
            .matches
            .map(|m| Match {
                app: m.app.map(OneOrMany::into_vec).unwrap_or_default(),
                person: m.person.map(OneOrMany::into_vec).unwrap_or_default(),
            })
            .unwrap_or_default();
        hooks.push(Hook {
            name,
            on,
            run: r.run.trim().to_string(),
            cron,
            matches,
            scope,
            timeout,
            enabled: r.enabled.unwrap_or(true),
        });
    }
    Ok(Config {
        dir: dir.to_path_buf(),
        hooks,
    })
}

/// Reads `<dir>/hooks.toml`; no file means no hooks.
pub fn load(dir: &Path) -> Result<Config> {
    let path = dir.join(CONFIG_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text, dir).with_context(|| path.display().to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config {
            dir: dir.to_path_buf(),
            hooks: Vec::new(),
        }),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"
[[hook]]
name = "tasks-to-reminders"
on = "meeting.note_ready"
run = "./tasks.sh --list Sokki"
match = { app = ["Zoom", "Teams"], person = "Anna" }
scope = ["tasks", "summary", "tasks"]
timeout = "5m"

[[hook]]
name = "weekly"
on = "schedule"
cron = "0 17 * * fri"
run = "./weekly.sh"
timeout = 90

[[hook]]
name = "dictations-log"
on = "dictation.done"
run = "cat >> log.jsonl"
scope = ["dictations"]
enabled = false
"#;

    #[test]
    fn parses_the_documented_example() {
        let c = parse(EXAMPLE, Path::new("/h")).unwrap();
        assert_eq!(c.hooks.len(), 3);
        let t = &c.hooks[0];
        assert_eq!(t.on, "meeting.note_ready");
        assert_eq!(t.matches.app, vec!["Zoom", "Teams"]);
        assert_eq!(t.matches.person, vec!["Anna"]);
        assert_eq!(t.scope, vec![Scope::Summary, Scope::Tasks]);
        assert_eq!(t.timeout, Duration::from_secs(300));
        assert!(t.cron.is_none());
        let w = &c.hooks[1];
        assert_eq!(w.cron.as_ref().unwrap().text, "0 17 * * fri");
        assert_eq!(w.timeout, Duration::from_secs(90));
        assert_eq!(w.scope, Scope::ALL.to_vec(), "no scope means everything");
        assert!(!c.hooks[2].enabled);
    }

    #[test]
    fn refuses_mistakes_with_a_clear_message() {
        let bad = |toml: &str| format!("{:#}", parse(toml, Path::new("/h")).unwrap_err());
        assert!(
            bad("[[hook]]\nname=\"a\"\non=\"meeting.done\"\nrun=\"x\"").contains("unknown event")
        );
        assert!(
            bad("[[hook]]\nname=\"a\"\non=\"calendar.upcoming\"\nrun=\"x\"")
                .contains("later version")
        );
        assert!(bad("[[hook]]\nname=\"a\"\non=\"schedule\"\nrun=\"x\"").contains("needs a cron"));
        assert!(
            bad("[[hook]]\nname=\"a\"\non=\"dictation.done\"\nrun=\"x\"\ncron=\"@daily\"")
                .contains("only works")
        );
        assert!(
            bad("[[hook]]\nname=\"a\"\non=\"dictation.done\"\nrun=\"x\"\nscope=[\"audio\"]")
                .contains("unknown scope")
        );
        assert!(
            bad("[[hook]]\nname=\"a b\"\non=\"dictation.done\"\nrun=\"x\"").contains("hook name")
        );
        assert!(
            bad("[[hook]]\nname=\"a\"\non=\"dictation.done\"\nrun=\"x\"\ntimeout=\"5 weeks\"")
                .contains("timeout")
        );
        assert!(
            bad("[[hook]]\nname=\"a\"\non=\"dictation.done\"\nrun=\"x\"\nsope=[]").contains("sope")
        );
        let two = "[[hook]]\nname=\"a\"\non=\"dictation.done\"\nrun=\"x\"\n[[hook]]\nname=\"a\"\non=\"dictation.done\"\nrun=\"y\"";
        assert!(bad(two).contains("two hooks"));
    }

    #[test]
    fn no_file_means_no_hooks() {
        let dir = std::env::temp_dir().join(format!("sokki-hooks-none-{}", std::process::id()));
        let c = load(&dir).unwrap();
        assert!(c.hooks.is_empty());
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("30s"), Some(Duration::from_secs(30)));
        assert_eq!(parse_duration("5m"), Some(Duration::from_secs(300)));
        assert_eq!(parse_duration("1h"), Some(Duration::from_secs(3600)));
        assert_eq!(parse_duration("45"), Some(Duration::from_secs(45)));
        assert_eq!(parse_duration("0s"), None);
        assert_eq!(parse_duration("m"), None);
    }

    fn meta(app: Option<&str>, people: &[&str]) -> Meta {
        Meta {
            app: app.map(str::to_string),
            participants: people.iter().map(|p| p.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn matching() {
        let m = Match {
            app: vec!["Zoom".into(), "Google Meet".into()],
            person: vec!["Anna".into()],
        };
        assert!(m.matches(&meta(Some("zoom"), &["Ich", "anna"])));
        assert!(!m.matches(&meta(Some("Teams"), &["Anna"])));
        assert!(!m.matches(&meta(None, &["Anna"])));
        assert!(!m.matches(&meta(Some("Zoom"), &["Ich"])));
        let anyone = Match {
            person: vec!["*".into()],
            ..Default::default()
        };
        assert!(anyone.matches(&meta(None, &["Ich"])));
        assert!(!anyone.matches(&meta(None, &[])));
        assert!(Match::default().matches(&meta(None, &[])));
        let umlaut = Match {
            person: vec!["JÖRG".into()],
            ..Default::default()
        };
        assert!(umlaut.matches(&meta(None, &["Jörg"])));
    }
}

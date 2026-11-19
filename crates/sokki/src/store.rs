//! Meetings on disk: `~/.local/share/sokki/meetings/<id>/` holds the audio,
//! `meeting.json` (this record) and `speakers.json`. The folder is the
//! database; there is no index to get out of sync.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const RECORD_FILE: &str = "meeting.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Recording,
    Queued,
    Processing,
    Done,
    Failed,
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Recording => "recording",
            Status::Queued => "queued",
            Status::Processing => "processing",
            Status::Done => "done",
            Status::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    /// RFC 3339, local time.
    pub started_at: String,
    #[serde(default)]
    pub duration_s: f64,
    pub status: Status,
    /// Track files in the folder ("mic.flac", "system.flac").
    #[serde(default)]
    pub tracks: Vec<String>,
    #[serde(default)]
    pub source_app: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub note_path: Option<String>,
    #[serde(default)]
    pub speakers: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub audio_deleted: bool,
}

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Store { root: root.into() }
    }

    pub fn dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    /// A new meeting folder named after its start time.
    pub fn create(&self, source_app: Option<String>) -> Result<Record> {
        let now = chrono::Local::now();
        let mut id = now.format("%Y-%m-%d_%H-%M-%S").to_string();
        let mut n = 2;
        while self.dir(&id).exists() {
            id = format!("{}-{n}", now.format("%Y-%m-%d_%H-%M-%S"));
            n += 1;
        }
        std::fs::create_dir_all(self.dir(&id))?;
        let r = Record {
            id,
            started_at: now.to_rfc3339(),
            duration_s: 0.0,
            status: Status::Recording,
            tracks: Vec::new(),
            source_app,
            title: None,
            note_path: None,
            speakers: Vec::new(),
            warnings: Vec::new(),
            error: None,
            audio_deleted: false,
        };
        self.save(&r)?;
        Ok(r)
    }

    pub fn get(&self, id: &str) -> Result<Record> {
        read(&self.dir(id).join(RECORD_FILE))
    }

    pub fn save(&self, r: &Record) -> Result<()> {
        let path = self.dir(&r.id).join(RECORD_FILE);
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(r)?)?;
        std::fs::rename(&tmp, &path).with_context(|| format!("writing {}", path.display()))
    }

    pub fn update(&self, id: &str, f: impl FnOnce(&mut Record)) -> Result<Record> {
        let mut r = self.get(id)?;
        f(&mut r);
        self.save(&r)?;
        Ok(r)
    }

    /// All meetings, newest first.
    pub fn list(&self) -> Vec<Record> {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut out: Vec<Record> = entries
            .flatten()
            .filter_map(|e| read(&e.path().join(RECORD_FILE)).ok())
            .collect();
        out.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        out
    }
}

fn read(path: &Path) -> Result<Record> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_update_list() {
        let root = std::env::temp_dir().join(format!("sokki-store-{}", std::process::id()));
        let s = Store::new(&root);
        let a = s.create(Some("Zoom".into())).unwrap();
        let b = s.create(None).unwrap();
        assert_ne!(a.id, b.id);
        s.update(&a.id, |r| r.status = Status::Done).unwrap();
        let all = s.list();
        assert_eq!(all.len(), 2);
        assert_eq!(s.get(&a.id).unwrap().status, Status::Done);
        let _ = std::fs::remove_dir_all(root);
    }
}

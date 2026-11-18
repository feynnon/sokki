//! The list of meetings shown in the app: one JSON index file next to the
//! meeting audio. A record follows a meeting from recording to its note.
//!
//! The note itself lives in the notes folder as Markdown; this index only
//! remembers where it went, so the app can open it, reprocess it or delete it.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Where a meeting is in its life.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MeetingStatus {
    Recording,
    /// `stage`: "waiting" (queued), "decoding", "diarizing", "transcribing", "correcting", "summarizing" or "writing".
    /// `progress`: 0..=1 within the whole job, when known.
    Processing {
        stage: String,
        progress: Option<f32>,
    },
    Done,
    Failed {
        error: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct MeetingRecord {
    pub id: String,
    /// Local start time, RFC 3339.
    pub started_at: String,
    pub duration_s: f64,
    pub status: MeetingStatus,
    /// Track files in the meeting's audio folder ("mic.flac", "system.flac").
    pub tracks: Vec<String>,
    /// The meeting app that held the microphone, if detection saw one.
    pub source_app: Option<String>,
    pub title: Option<String>,
    pub note_path: Option<String>,
    pub speakers: Vec<String>,
    /// Problems that did not stop the note (e.g. the summary model was offline).
    pub warnings: Vec<String>,
    /// When the audio was deleted, if it was.
    pub audio_deleted: bool,
    /// After processing: `true` for an online meeting (someone spoke on the
    /// system track), `false` for everything on one microphone.
    #[serde(default)]
    pub online: Option<bool>,
}

/// The meeting index, safe to share between threads.
pub struct MeetingStore {
    root: PathBuf,
    records: Mutex<Vec<MeetingRecord>>,
}

impl MeetingStore {
    /// Opens (or creates) the index under `root`. Meetings left in
    /// `Recording` or `Processing` by a crash are marked failed, so they can
    /// be reprocessed from their audio.
    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root).with_context(|| format!("creating {}", root.display()))?;
        let path = root.join("index.json");
        let mut records: Vec<MeetingRecord> = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                log::warn!("Meeting index unreadable ({e}); starting a new one");
                let _ = std::fs::rename(&path, root.join("index.corrupt.json"));
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
        for r in &mut records {
            if matches!(
                r.status,
                MeetingStatus::Recording | MeetingStatus::Processing { .. }
            ) {
                r.status = MeetingStatus::Failed {
                    error: "Interrupted: the app quit while this meeting was in progress.".into(),
                };
            }
        }
        let store = Self {
            root: root.to_path_buf(),
            records: Mutex::new(records),
        };
        store.save()?;
        Ok(store)
    }
    /// The audio folder of one meeting.
    pub fn audio_dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    /// Newest first.
    pub fn list(&self) -> Vec<MeetingRecord> {
        let mut v = self.lock().clone();
        v.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        v
    }

    pub fn get(&self, id: &str) -> Option<MeetingRecord> {
        self.lock().iter().find(|r| r.id == id).cloned()
    }

    pub fn insert(&self, record: MeetingRecord) -> Result<()> {
        {
            let mut g = self.lock();
            g.retain(|r| r.id != record.id);
            g.push(record);
        }
        self.save()
    }

    /// Applies `f` to the record and saves. Returns the updated record.
    pub fn update(&self, id: &str, f: impl FnOnce(&mut MeetingRecord)) -> Result<MeetingRecord> {
        let updated = {
            let mut g = self.lock();
            let r = g
                .iter_mut()
                .find(|r| r.id == id)
                .with_context(|| format!("meeting {id} not found"))?;
            f(r);
            r.clone()
        };
        self.save()?;
        Ok(updated)
    }

    /// Removes the record and its audio folder. The note is left alone.
    pub fn remove(&self, id: &str) -> Result<()> {
        self.lock().retain(|r| r.id != id);
        let dir = self.audio_dir(id);
        if dir.starts_with(&self.root) && dir != self.root && dir.exists() {
            std::fs::remove_dir_all(&dir).with_context(|| format!("deleting {}", dir.display()))?;
        }
        self.save()
    }

    fn save(&self) -> Result<()> {
        let text = serde_json::to_string_pretty(&*self.lock())?;
        let tmp = self.root.join("index.json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, self.root.join("index.json"))?;
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<MeetingRecord>> {
        self.records.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, started: &str, status: MeetingStatus) -> MeetingRecord {
        MeetingRecord {
            id: id.into(),
            started_at: started.into(),
            duration_s: 60.0,
            status,
            tracks: vec!["mic.flac".into()],
            source_app: None,
            title: None,
            note_path: None,
            speakers: vec![],
            warnings: vec![],
            audio_deleted: false,
            online: None,
        }
    }

    fn tempdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hm-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn persists_and_lists_newest_first() {
        let dir = tempdir("persist");
        let s = MeetingStore::open(&dir).unwrap();
        s.insert(record(
            "a",
            "2026-11-17T09:00:00+01:00",
            MeetingStatus::Done,
        ))
        .unwrap();
        s.insert(record(
            "b",
            "2026-11-17T10:00:00+01:00",
            MeetingStatus::Done,
        ))
        .unwrap();
        drop(s);
        let s = MeetingStore::open(&dir).unwrap();
        let ids: Vec<_> = s.list().into_iter().map(|r| r.id).collect();
        assert_eq!(ids, vec!["b", "a"]);
    }

    #[test]
    fn interrupted_meetings_become_failed_on_open() {
        let dir = tempdir("interrupt");
        let s = MeetingStore::open(&dir).unwrap();
        s.insert(record("a", "t1", MeetingStatus::Recording))
            .unwrap();
        s.insert(record(
            "b",
            "t2",
            MeetingStatus::Processing {
                stage: "transcribing".into(),
                progress: None,
            },
        ))
        .unwrap();
        drop(s);
        let s = MeetingStore::open(&dir).unwrap();
        assert!(s
            .list()
            .iter()
            .all(|r| matches!(r.status, MeetingStatus::Failed { .. })));
    }

    #[test]
    fn update_and_remove_deletes_audio() {
        let dir = tempdir("remove");
        let s = MeetingStore::open(&dir).unwrap();
        s.insert(record("m1", "t", MeetingStatus::Done)).unwrap();
        std::fs::create_dir_all(s.audio_dir("m1")).unwrap();
        std::fs::write(s.audio_dir("m1").join("mic.flac"), b"x").unwrap();
        let r = s.update("m1", |r| r.title = Some("Hi".into())).unwrap();
        assert_eq!(r.title.as_deref(), Some("Hi"));
        s.remove("m1").unwrap();
        assert!(s.get("m1").is_none());
        assert!(!s.audio_dir("m1").exists());
    }

    #[test]
    fn corrupt_index_is_set_aside() {
        let dir = tempdir("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.json"), "{not json").unwrap();
        let s = MeetingStore::open(&dir).unwrap();
        assert!(s.list().is_empty());
        assert!(dir.join("index.corrupt.json").exists());
    }
}

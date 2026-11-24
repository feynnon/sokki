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
    /// `stage`: "waiting" (queued), "decoding", "diarizing", "transcribing", "summarizing" or "writing".
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

/// Why a done meeting's note has no summary although summaries are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SummaryMissing {
    /// The language model wasn't ready; the summary is added once it is.
    Waiting,
    /// The language model answered but the summary failed; the card offers
    /// to try again.
    Failed,
}

impl From<sokki_engine::SummaryMissing> for SummaryMissing {
    fn from(m: sokki_engine::SummaryMissing) -> Self {
        match m {
            sokki_engine::SummaryMissing::Waiting => Self::Waiting,
            sokki_engine::SummaryMissing::Failed => Self::Failed,
        }
    }
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
    /// Problems that did not stop the note, one short sentence each (e.g.
    /// the summary model was offline). The error text is in the log.
    pub warnings: Vec<String>,
    /// Set while the note lacks a summary that should be there.
    #[serde(default)]
    pub summary_missing: Option<SummaryMissing>,
    /// When the audio was deleted, if it was.
    pub audio_deleted: bool,
    /// After processing: `true` for an online meeting (someone spoke on the
    /// system track), `false` for everything on one microphone.
    #[serde(default)]
    pub online: Option<bool>,
    /// The person typed this title; nothing replaces it.
    #[serde(default)]
    pub title_by_hand: bool,
    /// The title is a first guess from the live transcript, until the
    /// summary names the meeting.
    #[serde(default)]
    pub title_provisional: bool,
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
            upgrade_warnings(r);
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

/// Records written before 0.2.5 kept raw error text in `warnings` (model
/// URLs, hashes, ggml logs). Turns them into the card's short sentences;
/// the error text was logged when it happened. A note that waits for its
/// summary gets [`SummaryMissing::Waiting`], so it is summarized later.
fn upgrade_warnings(r: &mut MeetingRecord) {
    use meeting_core::pipeline::{PARTS_NOT_TRANSCRIBED, SPEAKERS_NOT_SEPARATED};
    if r.warnings.is_empty() {
        return;
    }
    let mut out: Vec<String> = Vec::new();
    let mut summary = false;
    for w in std::mem::take(&mut r.warnings) {
        let new = if w.starts_with("Diarization failed") || w.starts_with("Speakers not separated")
        {
            Some(SPEAKERS_NOT_SEPARATED.to_string())
        } else if w.starts_with("Transcription failed for")
            || w.ends_with("transcriber calls failed in total.")
        {
            Some(PARTS_NOT_TRANSCRIBED.to_string())
        } else if w.starts_with("No summary:") {
            summary = true;
            None
        } else if w.starts_with("Speaker detection on the GPU failed")
            || w.starts_with("Transcript correction partly skipped")
            || w.starts_with("Transcribing ")
        {
            None
        } else if w.starts_with("Only the microphone was recorded; system audio was unavailable") {
            Some(super::system_audio_warning())
        } else if w.starts_with("Microphone problem during the recording") {
            Some(super::MIC_PROBLEM.to_string())
        } else {
            Some(w)
        };
        if let Some(new) = new {
            if !out.contains(&new) {
                out.push(new);
            }
        }
    }
    if summary && r.note_path.is_some() && r.summary_missing.is_none() {
        r.summary_missing = Some(SummaryMissing::Waiting);
    }
    if r.summary_missing == Some(SummaryMissing::Waiting)
        && !out.iter().any(|w| w == sokki_engine::SUMMARY_WAITING)
    {
        out.push(sokki_engine::SUMMARY_WAITING.to_string());
    }
    r.warnings = out;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_raw_warnings_become_short_sentences() {
        let mut r = record("m", "2026-11-19T16:36:00+01:00", MeetingStatus::Done);
        r.note_path = Some("/notes/2026-11-19 1636 Meeting.md".into());
        r.warnings = vec![
            "Diarization failed for the microphone track (checksum mismatch for https://x: expected a, got b); treating the whole track as one speaker.".into(),
            "Diarization failed for the system audio track (checksum mismatch for https://x: expected a, got b); treating the whole track as one speaker.".into(),
            "Speaker detection on the GPU failed (diarization worker failed (exit status: 1): ggml_metal_device_init: …); the CPU was used instead (slower).".into(),
            "No summary: connection refused".into(),
            "About 3 s of microphone audio were lost (the disk was too slow).".into(),
        ];
        upgrade_warnings(&mut r);
        assert_eq!(
            r.warnings,
            vec![
                meeting_core::pipeline::SPEAKERS_NOT_SEPARATED.to_string(),
                "About 3 s of microphone audio were lost (the disk was too slow).".into(),
                sokki_engine::SUMMARY_WAITING.to_string(),
            ]
        );
        assert_eq!(r.summary_missing, Some(SummaryMissing::Waiting));
        // A second load changes nothing.
        let before = r.clone();
        upgrade_warnings(&mut r);
        assert_eq!(r, before);
    }

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
            summary_missing: None,
            audio_deleted: false,
            online: None,
            title_by_hand: false,
            title_provisional: false,
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

//! The journal's files: `<dir>/journal-YYYY-MM-DD.jsonl`, one line per
//! entry, the day taken from when it was written (local time). Entries sent
//! over from the phone go to `<dir>/phone/` with the same names.
//! Readable by this user only.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use chrono::{Local, TimeZone};

use crate::entry::Entry;

/// The folder for entries from the phone.
pub const PHONE_DIR: &str = "phone";

fn day_file(dir: &Path, written_ms: u64) -> PathBuf {
    let day = Local
        .timestamp_millis_opt(written_ms as i64)
        .single()
        .unwrap_or_else(Local::now)
        .format("%Y-%m-%d");
    dir.join(format!("journal-{day}.jsonl"))
}

fn private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Appends one entry to its day's file.
pub fn append(dir: &Path, entry: &Entry) -> io::Result<()> {
    private_dir(dir)?;
    let path = day_file(dir, entry.written);
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    let mut line = serde_json::to_string(entry).map_err(io::Error::other)?;
    line.push('\n');
    file.write_all(line.as_bytes())
}

/// What the journal holds, for Settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stats {
    pub entries: u64,
    pub bytes: u64,
    /// The first day with an entry, "YYYY-MM-DD".
    pub since: Option<String>,
}

fn day_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("journal-") && n.ends_with(".jsonl"))
        })
        .collect();
    files.sort();
    files
}

/// Entries and size of the Mac's and the phone's files together.
pub fn stats(dir: &Path) -> Stats {
    let mut stats = Stats::default();
    for folder in [dir.to_path_buf(), dir.join(PHONE_DIR)] {
        for file in day_files(&folder) {
            let Ok(data) = fs::read(&file) else { continue };
            stats.bytes += data.len() as u64;
            stats.entries += data.iter().filter(|&&b| b == b'\n').count() as u64;
            let day = file
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n["journal-".len()..n.len() - ".jsonl".len()].to_string());
            if let Some(day) = day {
                if stats.since.as_ref().is_none_or(|s| &day < s) {
                    stats.since = Some(day);
                }
            }
        }
    }
    stats
}

/// Every entry, oldest day first: the Mac's, then the phone's.
pub fn read_all(dir: &Path) -> Vec<Entry> {
    let mut out = Vec::new();
    for folder in [dir.to_path_buf(), dir.join(PHONE_DIR)] {
        for file in day_files(&folder) {
            let Ok(text) = fs::read_to_string(&file) else {
                continue;
            };
            out.extend(
                text.lines()
                    .filter_map(|l| serde_json::from_str::<Entry>(l).ok()),
            );
        }
    }
    out
}

/// Deletes every journal file, the phone's included.
pub fn delete_all(dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(written: u64, text: &str) -> Entry {
        Entry {
            t: "entry".into(),
            opened: written,
            written,
            app: "com.example".into(),
            hint: String::new(),
            prefill: String::new(),
            text: text.into(),
            dictated: vec![],
            typed: Some(vec![[0, text.chars().count()]]),
            replaceable: None,
        }
    }

    #[test]
    fn append_count_read_and_delete() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("typing_log");
        assert_eq!(stats(&dir), Stats::default());
        // 2026-11-24 and 2026-11-25, noon UTC: different local days everywhere.
        append(&dir, &entry(1_795_521_600_000, "eins")).unwrap();
        append(&dir, &entry(1_795_521_601_000, "zwei")).unwrap();
        append(&dir, &entry(1_795_608_000_000, "drei")).unwrap();
        let s = stats(&dir);
        assert_eq!(s.entries, 3);
        assert!(s.bytes > 0);
        assert_eq!(day_files(&dir).len(), 2);
        assert!(s.since.unwrap().starts_with("2026-11-2"));
        let texts: Vec<String> = read_all(&dir).into_iter().map(|e| e.text).collect();
        assert_eq!(texts, ["eins", "zwei", "drei"]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&day_files(&dir)[0])
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        delete_all(&dir).unwrap();
        assert!(!dir.exists());
        delete_all(&dir).unwrap();
    }

    #[test]
    fn phone_entries_count_too() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        append(&dir.join(PHONE_DIR), &entry(1_795_521_600_000, "vom Handy")).unwrap();
        assert_eq!(stats(&dir).entries, 1);
        assert_eq!(read_all(&dir)[0].text, "vom Handy");
    }
}

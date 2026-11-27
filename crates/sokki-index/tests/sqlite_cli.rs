//! The index with macOS's own `sqlite3` (3.51): read-only FTS5 and trigram
//! queries through the views, while another connection keeps writing (WAL),
//! and with no other connection open. Made-up data only.
#![cfg(target_os = "macos")]

use sokki_index::{sync, Index, Paths};
use std::path::{Path, PathBuf};
use std::process::Command;

const SQLITE: &str = "/usr/bin/sqlite3";

fn fixture(name: &str) -> (PathBuf, Paths) {
    let root = std::env::temp_dir().join(format!("sokki-sqlite-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    sokki_index::fixtures::write(&root).unwrap();
    let paths = Paths {
        index: root.join("index.sqlite"),
        meetings_dir: root.join("meetings"),
        history_db: Some(root.join("history.db")),
        user_name: "Ich".into(),
    };
    (root, paths)
}

/// `sqlite3 -readonly DB SQL`: (exit ok, stdout, stderr).
fn sqlite3(db: &Path, sql: &str) -> (bool, String, String) {
    let o = Command::new(SQLITE).arg("-readonly").arg(db).arg(sql).output().unwrap();
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).trim().to_string(),
        String::from_utf8_lossy(&o.stderr).trim().to_string(),
    )
}

const TRIGRAM: &str = "SELECT t.cite FROM segments_tri JOIN turns t ON t.segment_id = segments_tri.rowid \
                       WHERE segments_tri MATCH 'vertrag' ORDER BY bm25(segments_tri) LIMIT 1";
const WORDS: &str = "SELECT count(*) FROM segments_fts WHERE segments_fts MATCH 'kundigungsfrist'";

#[test]
fn macos_sqlite3_reads_the_index_while_it_is_written() {
    if !Path::new(SQLITE).is_file() {
        eprintln!("no {SQLITE}; skipped");
        return;
    }
    let (root, paths) = fixture("rw");
    {
        let ix = Index::open(&paths.index).unwrap();
        sync(&ix, &paths).unwrap();
    }

    // No connection open: WAL files may be gone; reading still works.
    let (ok, out, err) = sqlite3(&paths.index, TRIGRAM);
    assert!(ok, "{err}");
    assert_eq!(out, "m:meeting-20261103-100000-001@00:05", "trigram finds the compound");
    let (ok, out, err) = sqlite3(&paths.index, WORDS);
    assert!(ok, "{err}");
    assert_eq!(out, "2", "accents folded: Kündigungsfrist in a turn and the summary");

    // Writes are refused.
    let (ok, _, err) = sqlite3(&paths.index, "DELETE FROM sources");
    assert!(!ok && err.contains("readonly"), "{err}");

    // A writer that keeps changing a note and syncing, in another thread.
    let writer_paths = paths.clone();
    let note: PathBuf = std::fs::read_dir(root.join("notes"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.to_string_lossy().contains("Herbstaktion"))
        .unwrap();
    let writer = std::thread::spawn(move || {
        let ix = Index::open(&writer_paths.index).unwrap();
        let original = std::fs::read_to_string(&note).unwrap();
        for i in 0..40 {
            let md = original.replace("Landingpage", &format!("Landingpage{i}"));
            std::fs::write(&note, md).unwrap();
            let r = sync(&ix, &writer_paths).unwrap();
            assert_eq!(r.updated, 1, "{r:?}");
        }
        std::fs::write(&note, original).unwrap();
        sync(&ix, &writer_paths).unwrap();
    });
    let mut reads = 0;
    while !writer.is_finished() {
        let (ok, out, err) = sqlite3(&paths.index, TRIGRAM);
        assert!(ok, "read during writes failed: {err}");
        assert_eq!(out, "m:meeting-20261103-100000-001@00:05");
        let (ok, out, err) = sqlite3(
            &paths.index,
            "SELECT count(*) FROM meetings; SELECT count(*) FROM tasks; SELECT count(*) FROM dictations",
        );
        assert!(ok, "{err}");
        assert_eq!(out.lines().collect::<Vec<_>>(), vec!["3", "4", "2"], "views stay whole during writes");
        reads += 1;
    }
    writer.join().unwrap();
    assert!(reads >= 3, "only {reads} reads overlapped the writes");
    let _ = std::fs::remove_dir_all(&root);
}

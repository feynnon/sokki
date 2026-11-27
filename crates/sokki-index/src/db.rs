//! The schema. The index is derived, so a schema change needs no
//! migration: an index with another [`SCHEMA`] is emptied and rebuilt.

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Bump when a table changes; older indexes are rebuilt.
pub const SCHEMA: i64 = 1;

const TABLES: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT
);

-- One meeting, its own notes, or one dictation.
CREATE TABLE IF NOT EXISTS sources (
    id          TEXT PRIMARY KEY,      -- 'm:<meeting id>', 'n:<meeting id>', 'd:<history id>'
    kind        TEXT NOT NULL,         -- meeting | notes | dictation
    ref         TEXT NOT NULL,         -- the meeting id or the history id
    title       TEXT,
    started_at  TEXT,                  -- RFC 3339, local
    started_ms  INTEGER,
    duration_s  REAL,
    app         TEXT,                  -- the call app, or the app a dictation went into
    path        TEXT,                  -- the note's file
    has_audio   INTEGER,
    online      INTEGER,               -- someone spoke on the call track
    fingerprint TEXT NOT NULL,         -- sizes and times of its files: unchanged = not read again
    indexed_ms  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS sources_kind_time ON sources(kind, started_ms);

-- Every citable piece of a source: one speaker turn of a transcript, one
-- bullet of a summary or of the own notes, one section line a hook added,
-- one dictation.
CREATE TABLE IF NOT EXISTS segments (
    id             INTEGER PRIMARY KEY,
    source_id      TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    ord            INTEGER NOT NULL,
    section        TEXT NOT NULL,      -- transcript | summary | decisions | tasks | notes | dictation | <heading>
    start_ms       INTEGER,            -- from the meeting's start
    end_ms         INTEGER,
    speaker        TEXT,               -- the name in the note now
    speaker_id     TEXT,               -- 's1', 's2' (segments.jsonl): stays when renamed
    person_id      TEXT,               -- 'me', or the name as a slug ('jana-beispiel'); NULL for 'Speaker 2'
    name_confirmed INTEGER,            -- 0 for a placeholder ('Speaker 2')
    channel        TEXT,               -- mic (this computer's person) | system (the call) | NULL
    text           TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS segments_source ON segments(source_id, ord);
CREATE INDEX IF NOT EXISTS segments_person ON segments(person_id);

-- Words (unicode61, accents folded) and three-letter pieces (trigram, for
-- German compounds: 'vertrag' finds 'Rahmenvertrag'). Both read the text
-- from `segments`; secure-delete removes a deleted row's words from the
-- index itself, not only from the table.
CREATE VIRTUAL TABLE IF NOT EXISTS segments_fts USING fts5(
    text, content='segments', content_rowid='id',
    tokenize='unicode61 remove_diacritics 2'
);
CREATE VIRTUAL TABLE IF NOT EXISTS segments_tri USING fts5(
    text, content='segments', content_rowid='id',
    tokenize='trigram remove_diacritics 1'
);
CREATE TRIGGER IF NOT EXISTS segments_ai AFTER INSERT ON segments BEGIN
    INSERT INTO segments_fts(rowid, text) VALUES (new.id, new.text);
    INSERT INTO segments_tri(rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER IF NOT EXISTS segments_ad AFTER DELETE ON segments BEGIN
    INSERT INTO segments_fts(segments_fts, rowid, text) VALUES ('delete', old.id, old.text);
    INSERT INTO segments_tri(segments_tri, rowid, text) VALUES ('delete', old.id, old.text);
END;

-- Who spoke in which meeting.
CREATE TABLE IF NOT EXISTS source_people (
    source_id      TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    person_id      TEXT,
    speaker        TEXT NOT NULL,
    speaker_id     TEXT,
    channel        TEXT,
    name_confirmed INTEGER NOT NULL,
    talk_ms        INTEGER NOT NULL,
    PRIMARY KEY (source_id, speaker)
);

-- People across meetings, made from source_people after each sync.
CREATE TABLE IF NOT EXISTS people (
    person_id     TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    is_me         INTEGER NOT NULL,
    meetings      INTEGER NOT NULL,
    talk_ms       INTEGER NOT NULL,
    first_ms      INTEGER,
    last_ms       INTEGER
);

CREATE TABLE IF NOT EXISTS dictations (
    source_id   TEXT PRIMARY KEY REFERENCES sources(id) ON DELETE CASCADE,
    history_id  INTEGER NOT NULL,
    at_ms       INTEGER NOT NULL,
    target_app  TEXT,
    raw         TEXT NOT NULL,
    cleaned     TEXT,
    language    TEXT
);

-- Tasks and decisions from the summary, and items hooks added
-- ('custom:<name>'). evidence_ms and evidence_quote point into the
-- transcript; quote_verified says code found the quote there.
CREATE TABLE IF NOT EXISTS items (
    id              INTEGER PRIMARY KEY,
    source_id       TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    ord             INTEGER NOT NULL,
    kind            TEXT NOT NULL,     -- task | decision | custom:<name>
    text            TEXT NOT NULL,
    owner           TEXT,
    owner_person_id TEXT,
    status          TEXT,              -- open | done (tasks)
    evidence_ms     INTEGER,
    evidence_quote  TEXT,
    quote_verified  INTEGER NOT NULL DEFAULT 0,
    evidence_origin TEXT,              -- comment (<!-- t= -->) | match (code found the turn) | hook
    added_by        TEXT               -- summary | hook name | cli
);
CREATE INDEX IF NOT EXISTS items_kind ON items(kind, status);
"#;

/// An open index.
pub struct Index {
    pub conn: Connection,
    pub path: PathBuf,
}

impl Index {
    /// Opens (or creates) the index. An index from another schema is
    /// emptied, so the next sync builds it again.
    pub fn open(path: &Path) -> Result<Index> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.busy_timeout(Duration::from_secs(10))?;
        // secure_delete: deleted rows are overwritten, not just unlinked.
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA secure_delete = ON;
             PRAGMA foreign_keys = ON;
             PRAGMA synchronous = NORMAL;",
        )?;
        let index = Index {
            conn,
            path: path.to_path_buf(),
        };
        index.ensure_schema()?;
        Ok(index)
    }

    /// The index in memory, for tests.
    pub fn in_memory() -> Result<Index> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA secure_delete = ON; PRAGMA foreign_keys = ON;")?;
        let index = Index {
            conn,
            path: PathBuf::from(":memory:"),
        };
        index.ensure_schema()?;
        Ok(index)
    }

    fn ensure_schema(&self) -> Result<()> {
        let has_meta: bool = self
            .conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='meta'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n > 0)?;
        let version: Option<i64> = if has_meta {
            self.conn
                .query_row("SELECT value FROM meta WHERE key='schema'", [], |r| {
                    r.get::<_, String>(0)
                })
                .optional()?
                .and_then(|v| v.parse().ok())
        } else {
            None
        };
        if has_meta && version != Some(SCHEMA) {
            log::info!("index.sqlite has schema {version:?}, not {SCHEMA}: building it again");
            self.drop_all()?;
        }
        self.conn.execute_batch(TABLES)?;
        self.conn
            .execute_batch("INSERT INTO segments_fts(segments_fts, rank) VALUES('secure-delete', 1);")?;
        self.conn
            .execute_batch("INSERT INTO segments_tri(segments_tri, rank) VALUES('secure-delete', 1);")?;
        self.conn.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES ('schema', ?1)",
            params![SCHEMA.to_string()],
        )?;
        Ok(())
    }

    fn drop_all(&self) -> Result<()> {
        self.conn.execute_batch(
            "DROP TRIGGER IF EXISTS segments_ai;
             DROP TRIGGER IF EXISTS segments_ad;
             DROP TABLE IF EXISTS segments_fts;
             DROP TABLE IF EXISTS segments_tri;
             DROP TABLE IF EXISTS items;
             DROP TABLE IF EXISTS dictations;
             DROP TABLE IF EXISTS people;
             DROP TABLE IF EXISTS source_people;
             DROP TABLE IF EXISTS segments;
             DROP TABLE IF EXISTS sources;
             DROP TABLE IF EXISTS meta;",
        )?;
        Ok(())
    }

    /// Empties every table (for `sokki index rebuild`).
    pub fn clear(&self) -> Result<()> {
        self.drop_all()?;
        self.ensure_schema()?;
        self.vacuum();
        Ok(())
    }

    /// Writes deleted pages out of the write-ahead log and the file.
    pub fn vacuum(&self) {
        if self.path.as_os_str() != ":memory:" {
            let _ = self.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        }
    }

    pub fn meta(&self, key: &str) -> Option<String> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key=?1", params![key], |r| {
                r.get::<_, Option<String>>(0)
            })
            .optional()
            .ok()
            .flatten()
            .flatten()
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    /// Deletes a source and every row made from it (one transaction).
    pub fn delete_source(&self, id: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM segments WHERE source_id=?1", params![id])?;
        tx.execute("DELETE FROM items WHERE source_id=?1", params![id])?;
        tx.execute("DELETE FROM source_people WHERE source_id=?1", params![id])?;
        tx.execute("DELETE FROM dictations WHERE source_id=?1", params![id])?;
        tx.execute("DELETE FROM sources WHERE id=?1", params![id])?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_deleted_source_leaves_nothing_searchable() {
        let ix = Index::in_memory().unwrap();
        ix.conn
            .execute(
                "INSERT INTO sources(id, kind, ref, fingerprint, indexed_ms) VALUES ('m:a','meeting','a','x',0)",
                [],
            )
            .unwrap();
        ix.conn
            .execute(
                "INSERT INTO segments(source_id, ord, section, text) VALUES ('m:a', 0, 'transcript', 'Der Rahmenvertrag läuft')",
                [],
            )
            .unwrap();
        let hits = |table: &str, q: &str| -> i64 {
            ix.conn
                .query_row(
                    &format!("SELECT count(*) FROM {table} WHERE {table} MATCH ?1"),
                    params![q],
                    |r| r.get(0),
                )
                .unwrap()
        };
        assert_eq!(hits("segments_fts", "rahmenvertrag"), 1);
        assert_eq!(hits("segments_fts", "vertrag"), 0, "words don't match inside compounds");
        assert_eq!(hits("segments_tri", "vertrag"), 1, "trigrams do");
        assert_eq!(hits("segments_fts", "lauft"), 1, "accents folded");
        ix.delete_source("m:a").unwrap();
        assert_eq!(hits("segments_fts", "rahmenvertrag"), 0);
        assert_eq!(hits("segments_tri", "vertrag"), 0);
        let rows: i64 = ix
            .conn
            .query_row("SELECT count(*) FROM segments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn another_schema_is_rebuilt() {
        let dir = std::env::temp_dir().join(format!("sokki-index-schema-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("index.sqlite");
        {
            let ix = Index::open(&path).unwrap();
            ix.set_meta("schema", "0").unwrap();
            ix.conn
                .execute(
                    "INSERT INTO sources(id, kind, ref, fingerprint, indexed_ms) VALUES ('m:a','meeting','a','x',0)",
                    [],
                )
                .unwrap();
        }
        let ix = Index::open(&path).unwrap();
        let n: i64 = ix
            .conn
            .query_row("SELECT count(*) FROM sources", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
        assert_eq!(ix.meta("schema").as_deref(), Some("1"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

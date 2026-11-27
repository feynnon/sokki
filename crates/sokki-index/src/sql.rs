//! Plain SQL on the index, read-only: `sokki sql` and `sokki db schema`.
//! The same file works with any SQLite 3.45+ (macOS's `sqlite3` is 3.51),
//! FTS5 and trigram included; the index is rebuilt from the notes, so it is
//! for reading only (writes go through `sokki note append` / `item add`).

use anyhow::{Context, Result};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Map, Value};
use std::path::Path;

/// Opens the index for reading only: SQLite refuses every write.
pub fn open_read_only(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("opening {} read-only", path.display()))?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    conn.execute_batch("PRAGMA query_only = ON;")?;
    Ok(conn)
}

fn cell(v: ValueRef) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => json!(i),
        ValueRef::Real(f) => json!(f),
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Value::String(b.iter().map(|x| format!("{x:02x}")).collect()),
    }
}

/// Runs one statement and returns `{query, columns, rows, row_count,
/// truncated}`; each row is an object by column name. At most `max_rows`.
pub fn run(path: &Path, sql: &str, max_rows: usize) -> Result<Value> {
    let conn = open_read_only(path)?;
    let mut stmt = conn
        .prepare(sql.trim().trim_end_matches(';'))
        .context("SQL error (one read-only statement; `sokki db schema` lists the tables and views)")?;
    let columns: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    let mut truncated = false;
    while let Some(r) = rows.next()? {
        if out.len() >= max_rows {
            truncated = true;
            break;
        }
        let mut m = Map::new();
        for (i, c) in columns.iter().enumerate() {
            m.insert(c.clone(), cell(r.get_ref(i)?));
        }
        out.push(Value::Object(m));
    }
    Ok(json!({
        "query": sql,
        "columns": columns,
        "row_count": out.len(),
        "rows": out,
        "truncated": truncated,
    }))
}

/// Every table and view with its columns, and the schema version.
pub fn schema(path: &Path) -> Result<Value> {
    let conn = open_read_only(path)?;
    let version: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = 'schema'", [], |r| r.get(0))
        .ok();
    let mut stmt = conn.prepare(
        "SELECT type, name, sql FROM sqlite_master
          WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%'
            AND name NOT LIKE 'segments_fts_%' AND name NOT LIKE 'segments_tri_%'
          ORDER BY type DESC, name",
    )?;
    let objects: Vec<(String, String, Option<String>)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<std::result::Result<_, _>>()?;
    let mut list = Vec::new();
    for (kind, name, sql) in objects {
        let cols: Vec<String> = conn
            .prepare(&format!("SELECT name FROM pragma_table_info('{name}')"))?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<_, _>>()?;
        list.push(json!({ "type": kind, "name": name, "columns": cols, "sql": sql }));
    }
    Ok(json!({ "path": path.to_string_lossy(), "schema": version, "objects": list }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_refuses_writes_and_returns_rows() {
        let dir = std::env::temp_dir().join(format!("sokki-sql-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("index.sqlite");
        drop(crate::Index::open(&path).unwrap());
        let out = run(&path, "SELECT 1 AS one, 'zwei' AS two, NULL AS three", 10).unwrap();
        assert_eq!(out["rows"][0]["one"], 1);
        assert_eq!(out["rows"][0]["two"], "zwei");
        assert!(out["rows"][0]["three"].is_null());
        assert!(run(&path, "DELETE FROM sources", 10).is_err(), "read-only");
        assert!(run(&path, "SELECT 1; SELECT 2", 10).is_err(), "one statement");
        let s = schema(&path).unwrap();
        let names: Vec<&str> = s["objects"].as_array().unwrap().iter().map(|o| o["name"].as_str().unwrap()).collect();
        for v in ["meetings", "turns", "tasks", "decisions", "own_notes", "dictations", "people", "segments_tri"] {
            assert!(names.contains(&v), "{v} missing: {names:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

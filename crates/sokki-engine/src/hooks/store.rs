//! The hooks' state in `<state dir>/hooks.sqlite`: the queue (it survives
//! a restart), one row per run, and when each schedule last came due. The app and the `sokki` command share it (WAL, a busy
//! timeout).

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DB_FILE: &str = "hooks.sqlite";

/// Runs kept in the log; older ones and their log files go.
pub const KEEP_RUNS: usize = 500;

/// One retry, after this long.
pub const RETRY_AFTER: Duration = Duration::from_secs(60);
pub const MAX_ATTEMPTS: i64 = 2;

pub struct Store {
    conn: Connection,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub id: i64,
    pub hook: String,
    pub event: String,
    pub subject: String,
    /// The event JSON, with dictation or own-notes text; cleared once the
    /// job is over.
    pub payload: String,
    pub attempts: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub id: i64,
    pub job_id: Option<i64>,
    pub hook: String,
    pub event: String,
    pub subject: String,
    pub attempt: i64,
    /// "event", "manual" (`sokki hooks run`) or "test" (`sokki hooks test`).
    pub trigger: String,
    pub started_ms: i64,
    pub finished_ms: Option<i64>,
    /// "running", "ok", "failed", "timeout", "not_allowed", "error".
    pub status: String,
    pub exit_code: Option<i64>,
    pub error: Option<String>,
    pub log_path: Option<String>,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS jobs (
    id INTEGER PRIMARY KEY,
    hook TEXT NOT NULL,
    event TEXT NOT NULL,
    subject TEXT NOT NULL,
    payload TEXT NOT NULL,
    status TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_ms INTEGER NOT NULL,
    created_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS jobs_due ON jobs(status, next_ms);
CREATE TABLE IF NOT EXISTS runs (
    id INTEGER PRIMARY KEY,
    job_id INTEGER,
    hook TEXT NOT NULL,
    event TEXT NOT NULL,
    subject TEXT NOT NULL,
    attempt INTEGER NOT NULL,
    trigger TEXT NOT NULL,
    started_ms INTEGER NOT NULL,
    finished_ms INTEGER,
    status TEXT NOT NULL,
    exit_code INTEGER,
    error TEXT,
    log_path TEXT
);
CREATE INDEX IF NOT EXISTS runs_hook ON runs(hook, id);
CREATE TABLE IF NOT EXISTS schedule (
    hook TEXT PRIMARY KEY,
    last_due TEXT NOT NULL
);
";

fn run_row(r: &rusqlite::Row) -> rusqlite::Result<Run> {
    Ok(Run {
        id: r.get(0)?,
        job_id: r.get(1)?,
        hook: r.get(2)?,
        event: r.get(3)?,
        subject: r.get(4)?,
        attempt: r.get(5)?,
        trigger: r.get(6)?,
        started_ms: r.get(7)?,
        finished_ms: r.get(8)?,
        status: r.get(9)?,
        exit_code: r.get(10)?,
        error: r.get(11)?,
        log_path: r.get(12)?,
    })
}

const RUN_COLUMNS: &str = "id, job_id, hook, event, subject, attempt, trigger, started_ms, finished_ms, status, exit_code, error, log_path";

impl Store {
    pub fn open(state_dir: &Path) -> Result<Store> {
        std::fs::create_dir_all(state_dir)
            .with_context(|| format!("creating {}", state_dir.display()))?;
        let path = state_dir.join(DB_FILE);
        let conn =
            Connection::open(&path).with_context(|| format!("opening {}", path.display()))?;
        conn.busy_timeout(Duration::from_secs(10))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn })
    }

    // ---------------------------------------------------------------- queue

    /// Queues `payload` for `hook`. A job for the same hook, event and
    /// subject that has not started yet takes the newer payload instead of
    /// running twice.
    pub fn enqueue(&self, hook: &str, event: &str, subject: &str, payload: &str) -> Result<i64> {
        let now = now_ms();
        let updated = self.conn.execute(
            "UPDATE jobs SET payload = ?4, next_ms = MIN(next_ms, ?5)
             WHERE hook = ?1 AND event = ?2 AND subject = ?3 AND status = 'pending' AND attempts = 0",
            params![hook, event, subject, payload, now],
        )?;
        if updated > 0 {
            return Ok(self.conn.query_row(
                "SELECT id FROM jobs WHERE hook = ?1 AND event = ?2 AND subject = ?3 AND status = 'pending'",
                params![hook, event, subject],
                |r| r.get(0),
            )?);
        }
        self.conn.execute(
            "INSERT INTO jobs (hook, event, subject, payload, status, attempts, next_ms, created_ms)
             VALUES (?1, ?2, ?3, ?4, 'pending', 0, ?5, ?5)",
            params![hook, event, subject, payload, now],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// The oldest job that is due at `now`.
    pub fn next_due(&self, now: i64) -> Result<Option<Job>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, hook, event, subject, payload, attempts FROM jobs
                 WHERE status = 'pending' AND next_ms <= ?1 ORDER BY id LIMIT 1",
                [now],
                |r| {
                    Ok(Job {
                        id: r.get(0)?,
                        hook: r.get(1)?,
                        event: r.get(2)?,
                        subject: r.get(3)?,
                        payload: r.get(4)?,
                        attempts: r.get(5)?,
                    })
                },
            )
            .optional()?)
    }

    /// When the next pending job comes due, if any.
    pub fn next_wake(&self) -> Result<Option<i64>> {
        Ok(self.conn.query_row(
            "SELECT MIN(next_ms) FROM jobs WHERE status = 'pending'",
            [],
            |r| r.get(0),
        )?)
    }

    pub fn pending(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM jobs WHERE status IN ('pending', 'running')",
            [],
            |r| r.get(0),
        )?)
    }

    /// The job runs now: its attempt counts, and a run row is written.
    pub fn begin(&self, job: &Job, log_path: &Path) -> Result<i64> {
        self.conn.execute(
            "UPDATE jobs SET status = 'running', attempts = attempts + 1 WHERE id = ?1",
            [job.id],
        )?;
        self.begin_run(
            Some(job.id),
            &job.hook,
            &job.event,
            &job.subject,
            job.attempts + 1,
            "event",
            Some(log_path),
        )
    }

    /// A run row for a run outside the queue (by hand, a test).
    #[allow(clippy::too_many_arguments)]
    pub fn begin_run(
        &self,
        job_id: Option<i64>,
        hook: &str,
        event: &str,
        subject: &str,
        attempt: i64,
        trigger: &str,
        log_path: Option<&Path>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO runs (job_id, hook, event, subject, attempt, trigger, started_ms, status, log_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'running', ?8)",
            params![
                job_id,
                hook,
                event,
                subject,
                attempt,
                trigger,
                now_ms(),
                log_path.map(|p| p.to_string_lossy().into_owned())
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn end_run(
        &self,
        run_id: i64,
        status: &str,
        exit_code: Option<i32>,
        error: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE runs SET finished_ms = ?2, status = ?3, exit_code = ?4, error = ?5 WHERE id = ?1",
            params![run_id, now_ms(), status, exit_code, error],
        )?;
        Ok(())
    }

    /// After a run of `job`: done, retried once later, or failed for good.
    /// Returns whether the job is over.
    pub fn after_run(&self, job: &Job, ok: bool) -> Result<bool> {
        let attempts = job.attempts + 1;
        if ok || attempts >= MAX_ATTEMPTS {
            self.conn.execute(
                "UPDATE jobs SET status = ?2, payload = '' WHERE id = ?1",
                params![job.id, if ok { "done" } else { "failed" }],
            )?;
            return Ok(true);
        }
        self.conn.execute(
            "UPDATE jobs SET status = 'pending', next_ms = ?2 WHERE id = ?1",
            params![job.id, now_ms() + RETRY_AFTER.as_millis() as i64],
        )?;
        Ok(false)
    }

    /// The job waits `by` (hooks.toml has a mistake to fix first).
    pub fn postpone(&self, job: &Job, by: Duration) -> Result<()> {
        self.conn.execute(
            "UPDATE jobs SET next_ms = ?2 WHERE id = ?1",
            params![job.id, now_ms() + by.as_millis() as i64],
        )?;
        Ok(())
    }

    /// The job will not run (the hook is gone or off).
    pub fn skip(&self, job: &Job) -> Result<()> {
        self.conn.execute(
            "UPDATE jobs SET status = 'skipped', payload = '' WHERE id = ?1",
            [job.id],
        )?;
        Ok(())
    }

    /// At start: a job cut off by a quit or crash runs again (its attempt
    /// counted), and its run is marked as cut off.
    pub fn recover(&self) -> Result<()> {
        self.conn.execute(
            "UPDATE runs SET status = 'error', error = 'Sokki stopped during the run', finished_ms = ?1
             WHERE status = 'running'",
            [now_ms()],
        )?;
        self.conn.execute(
            "UPDATE jobs SET status = CASE WHEN attempts >= ?1 THEN 'failed' ELSE 'pending' END
             WHERE status = 'running'",
            [MAX_ATTEMPTS],
        )?;
        self.conn
            .execute("UPDATE jobs SET payload = '' WHERE status = 'failed'", [])?;
        Ok(())
    }

    // ------------------------------------------------------------------ log

    /// The newest runs first, of one hook or all.
    pub fn runs(&self, hook: Option<&str>, limit: usize) -> Result<Vec<Run>> {
        let mut out = Vec::new();
        match hook {
            Some(h) => {
                let mut st = self.conn.prepare(&format!(
                    "SELECT {RUN_COLUMNS} FROM runs WHERE hook = ?1 ORDER BY id DESC LIMIT ?2"
                ))?;
                for r in st.query_map(params![h, limit as i64], run_row)? {
                    out.push(r?);
                }
            }
            None => {
                let mut st = self.conn.prepare(&format!(
                    "SELECT {RUN_COLUMNS} FROM runs ORDER BY id DESC LIMIT ?1"
                ))?;
                for r in st.query_map([limit as i64], run_row)? {
                    out.push(r?);
                }
            }
        }
        Ok(out)
    }

    /// Drops runs beyond the newest `keep` and finished jobs older than a
    /// week; returns the log files to delete.
    pub fn prune(&self, keep: usize) -> Result<Vec<PathBuf>> {
        let cutoff: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM runs ORDER BY id DESC LIMIT 1 OFFSET ?1",
                [keep as i64],
                |r| r.get(0),
            )
            .optional()?;
        let mut files = Vec::new();
        if let Some(cutoff) = cutoff {
            let mut st = self
                .conn
                .prepare("SELECT log_path FROM runs WHERE id <= ?1 AND log_path IS NOT NULL")?;
            for p in st.query_map([cutoff], |r| r.get::<_, String>(0))? {
                files.push(PathBuf::from(p?));
            }
            self.conn
                .execute("DELETE FROM runs WHERE id <= ?1", [cutoff])?;
        }
        let week_ago = now_ms() - 7 * 24 * 3600 * 1000;
        self.conn.execute(
            "DELETE FROM jobs WHERE status IN ('done', 'failed', 'skipped') AND created_ms < ?1",
            [week_ago],
        )?;
        Ok(files)
    }

    // ------------------------------------------------------------- schedule

    pub fn last_due(&self, hook: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT last_due FROM schedule WHERE hook = ?1",
                [hook],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn set_last_due(&self, hook: &str, due: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO schedule (hook, last_due) VALUES (?1, ?2)
             ON CONFLICT(hook) DO UPDATE SET last_due = excluded.last_due",
            params![hook, due],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("sokki-hooks-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn queue_order_retry_and_survival() {
        let d = temp("queue");
        let log = d.join("x.log");
        {
            let s = Store::open(&d).unwrap();
            let a = s.enqueue("h", "meeting.note_ready", "m1", "{1}").unwrap();
            let again = s.enqueue("h", "meeting.note_ready", "m1", "{2}").unwrap();
            assert_eq!(a, again, "a pending duplicate takes the newer payload");
            let b = s.enqueue("h", "meeting.note_ready", "m2", "{3}").unwrap();
            assert_ne!(a, b);
            let job = s.next_due(now_ms()).unwrap().unwrap();
            assert_eq!((job.id, job.payload.as_str()), (a, "{2}"));
            s.begin(&job, &log).unwrap();
            // Quit during the run.
        }
        let s = Store::open(&d).unwrap();
        s.recover().unwrap();
        let job = s.next_due(now_ms()).unwrap().unwrap();
        assert_eq!(job.id, 1);
        assert_eq!(job.attempts, 1, "the cut-off run counted");
        let run = s.begin(&job, &log).unwrap();
        s.end_run(run, "failed", Some(1), None).unwrap();
        assert!(s.after_run(&job, false).unwrap(), "second failure ends it");
        // m2: fails once, waits a minute for its retry, then succeeds.
        let job = s.next_due(now_ms()).unwrap().unwrap();
        assert_eq!(job.subject, "m2");
        s.begin(&job, &log).unwrap();
        assert!(!s.after_run(&job, false).unwrap());
        assert!(s.next_due(now_ms()).unwrap().is_none(), "the retry waits");
        let later = now_ms() + RETRY_AFTER.as_millis() as i64 + 1;
        let job = s.next_due(later).unwrap().unwrap();
        assert_eq!(job.attempts, 1);
        s.begin(&job, &log).unwrap();
        assert!(s.after_run(&job, true).unwrap());
        assert!(s.next_due(later).unwrap().is_none());
        assert_eq!(s.pending().unwrap(), 0);
        let runs = s.runs(Some("h"), 10).unwrap();
        assert_eq!(runs.len(), 4);
        assert_eq!(runs[3].status, "error", "the run cut off by the quit");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn prune_keeps_the_newest_runs() {
        let d = temp("prune");
        let s = Store::open(&d).unwrap();
        for i in 0..5 {
            let log = d.join(format!("{i}.log"));
            s.begin_run(None, "h", "schedule", "x", 1, "manual", Some(&log))
                .unwrap();
        }
        let gone = s.prune(2).unwrap();
        assert_eq!(gone.len(), 3);
        assert_eq!(s.runs(None, 10).unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(&d);
    }
}

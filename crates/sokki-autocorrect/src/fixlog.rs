//! The list of fixes on this Mac: one JSON line per fix, undo and skipped fix, so the user (and
//! a test) can see which words Sokki changed and which of those were undone.
//!
//! ```text
//! {"at":"2026-11-26T06:39:50Z","app":"com.google.Chrome","what":"fixed","typed":"nihct","fix":"nicht","ms":37}
//! ```
//!
//! `what`: `fixed`, `fixed_inexact` (the field doesn't hold exactly what was meant), `undone`
//! (Backspace right after the fix: the word is kept from now on) or `skipped` (with `why`, no
//! words). It holds only the typed word and its fix, never the text around them, and stays on
//! this Mac. Sokki's general log keeps leaving the words out, since that log goes into bug
//! reports. The file keeps about the last [`MAX_BYTES`].
//! SPDX-License-Identifier: GPL-3.0-only

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::Outcome;

/// Above this size the older half of the list is dropped (~2,000 fixes).
pub const MAX_BYTES: u64 = 256 * 1024;

pub struct FixLog {
    path: PathBuf,
    max_bytes: u64,
}

impl FixLog {
    pub fn new(path: PathBuf) -> FixLog {
        FixLog {
            path,
            max_bytes: MAX_BYTES,
        }
    }

    #[doc(hidden)]
    pub fn with_limit(path: PathBuf, max_bytes: u64) -> FixLog {
        FixLog { path, max_bytes }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Adds one line for `outcome` (nothing for [`Outcome::Nothing`]).
    pub fn record(&self, app: &str, outcome: &Outcome, took: Duration) -> std::io::Result<()> {
        self.record_at(SystemTime::now(), app, outcome, took)
    }

    #[doc(hidden)]
    pub fn record_at(
        &self,
        at: SystemTime,
        app: &str,
        outcome: &Outcome,
        took: Duration,
    ) -> std::io::Result<()> {
        let Some(line) = line(at, app, outcome, took) else {
            return Ok(());
        };
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        self.trim()?;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(f, "{line}")
    }

    /// Keeps the newer half of the lines once the file is over the limit.
    fn trim(&self) -> std::io::Result<()> {
        let size = match std::fs::metadata(&self.path) {
            Ok(m) => m.len(),
            Err(_) => return Ok(()),
        };
        if size <= self.max_bytes {
            return Ok(());
        }
        let text = std::fs::read_to_string(&self.path)?;
        let lines: Vec<&str> = text.lines().collect();
        let keep = &lines[lines.len() / 2..];
        let tmp = self.path.with_extension("jsonl.tmp");
        std::fs::write(&tmp, keep.join("\n") + "\n")?;
        std::fs::rename(&tmp, &self.path)
    }
}

fn line(at: SystemTime, app: &str, outcome: &Outcome, took: Duration) -> Option<String> {
    let words = |what: &str, typed: &str, fix: &str| {
        format!(
            r#""what":"{what}","typed":{},"fix":{}"#,
            quote(typed),
            quote(fix)
        )
    };
    let body = match outcome {
        Outcome::Nothing => return None,
        Outcome::Fixed {
            typed,
            fix,
            exact: true,
        } => words("fixed", typed, fix),
        Outcome::Fixed {
            typed,
            fix,
            exact: false,
        } => words("fixed_inexact", typed, fix),
        Outcome::Undone { typed, fix } => words("undone", typed, fix),
        Outcome::Skipped(why) => format!(r#""what":"skipped","why":{}"#, quote(why)),
    };
    Some(format!(
        r#"{{"at":"{}","app":{},{body},"ms":{}}}"#,
        utc(at),
        quote(app),
        took.as_millis()
    ))
}

/// A JSON string.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `2026-11-26T06:39:50Z`.
fn utc(at: SystemTime) -> String {
    let secs = at
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // days since 1970-01-01 to a civil date (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn dates_are_utc() {
        assert_eq!(utc(at(0)), "1970-01-01T00:00:00Z");
        assert_eq!(utc(at(1_795_675_190)), "2026-11-26T06:39:50Z");
        assert_eq!(utc(at(951_782_400)), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn strings_are_escaped() {
        assert_eq!(quote(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(quote("x\u{1}"), r#""x\u0001""#);
        assert_eq!(quote("über"), r#""über""#);
    }
}

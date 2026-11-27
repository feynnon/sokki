//! Times as people and agents type them: `14d`, `2026-11-20`, `12:04`.

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Local, NaiveDate, TimeZone};

/// Unix ms now.
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Unix ms of an RFC 3339 time ("2026-11-17T14:03:00+01:00").
pub fn rfc3339_ms(s: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(s.trim())
        .ok()
        .map(|t| t.timestamp_millis())
}

/// RFC 3339 in local time for unix ms.
pub fn local_rfc3339(ms: i64) -> String {
    match Local.timestamp_millis_opt(ms).single() {
        Some(t) => t.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        None => String::new(),
    }
}

fn local_midnight_ms(d: NaiveDate) -> Result<i64> {
    let t = d.and_hms_opt(0, 0, 0).context("bad date")?;
    Local
        .from_local_datetime(&t)
        .earliest()
        .map(|t| t.timestamp_millis())
        .context("that date does not exist here")
}

/// `--since`: `3h`, `14d`, `2w`, `6mo`, `1y`, `today`,
/// `yesterday`, a date (`2026-11-20`, from its midnight) or an RFC 3339
/// time. Returns unix ms.
pub fn parse_since(s: &str, now: i64) -> Result<i64> {
    parse_point(s, now, false)
}

/// `--until`: like [`parse_since`]; a date means the end of that day.
pub fn parse_until(s: &str, now: i64) -> Result<i64> {
    parse_point(s, now, true)
}

fn parse_point(s: &str, now: i64, end_of_day: bool) -> Result<i64> {
    let s = s.trim();
    let day = 86_400_000i64;
    let today = Local::now().date_naive();
    match s {
        "today" => {
            let m = local_midnight_ms(today)?;
            return Ok(if end_of_day { m + day - 1 } else { m });
        }
        "yesterday" => {
            let m = local_midnight_ms(today.pred_opt().context("no yesterday")?)?;
            return Ok(if end_of_day { m + day - 1 } else { m });
        }
        "now" => return Ok(now),
        _ => {}
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let m = local_midnight_ms(d)?;
        return Ok(if end_of_day { m + day - 1 } else { m });
    }
    if let Some(ms) = rfc3339_ms(s) {
        return Ok(ms);
    }
    let split = s
        .find(|c: char| !c.is_ascii_digit())
        .filter(|&i| i > 0)
        .with_context(|| bad_time(s))?;
    let (n, unit) = s.split_at(split);
    let n: i64 = n.parse().with_context(|| bad_time(s))?;
    let span = match unit {
        "h" => 3_600_000,
        "d" => day,
        "w" => 7 * day,
        "mo" => 30 * day,
        "y" => 365 * day,
        _ => bail!("{}", bad_time(s)),
    };
    Ok(now - n * span)
}

fn bad_time(s: &str) -> String {
    format!("can't read the time {s:?}: use 3h, 14d, 2w, 6mo, 1y, today, yesterday or a date like 2026-11-20")
}

/// `12:04`, `1:02:05` or `95s` into ms.
pub fn parse_mmss(s: &str) -> Result<i64> {
    let s = s.trim();
    if let Some(secs) = s.strip_suffix('s') {
        let v: f64 = secs.parse().with_context(|| bad_mmss(s))?;
        return Ok((v * 1000.0).round() as i64);
    }
    let parts: Vec<&str> = s.split(':').collect();
    let nums: Vec<i64> = parts
        .iter()
        .map(|p| p.parse::<i64>())
        .collect::<std::result::Result<_, _>>()
        .with_context(|| bad_mmss(s))?;
    let secs = match nums.as_slice() {
        [m, s] if *s < 60 => m * 60 + s,
        [h, m, s] if *m < 60 && *s < 60 => h * 3600 + m * 60 + s,
        _ => bail!("{}", bad_mmss(s)),
    };
    Ok(secs * 1000)
}

fn bad_mmss(s: &str) -> String {
    format!("can't read the time {s:?} in the meeting: use mm:ss (12:04) or h:mm:ss")
}

/// `2m`, `90s`, `1h` as ms (for `--window`).
pub fn parse_span(s: &str) -> Result<i64> {
    let s = s.trim();
    let split = s
        .find(|c: char| !c.is_ascii_digit())
        .with_context(|| format!("{s:?}: use 90s, 2m or 1h"))?;
    let (n, unit) = s.split_at(split);
    let n: i64 = n.parse().with_context(|| format!("{s:?}: use 90s, 2m or 1h"))?;
    Ok(match unit {
        "s" => n * 1000,
        "m" => n * 60_000,
        "h" => n * 3_600_000,
        _ => bail!("{s:?}: use 90s, 2m or 1h"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_and_absolute_times() {
        let now = 10 * 86_400_000;
        assert_eq!(parse_since("2d", now).unwrap(), 8 * 86_400_000);
        assert_eq!(parse_since("1w", now).unwrap(), 3 * 86_400_000);
        assert_eq!(parse_since("3h", now).unwrap(), now - 3 * 3_600_000);
        assert!(parse_since("14x", now).is_err());
        assert!(parse_since("soon", now).is_err());
        let a = parse_since("2026-11-20", now).unwrap();
        let b = parse_until("2026-11-20", now).unwrap();
        assert_eq!(b - a, 86_400_000 - 1);
        assert_eq!(
            parse_since("2026-11-17T14:03:00+01:00", now).unwrap(),
            rfc3339_ms("2026-11-17T13:03:00Z").unwrap()
        );
    }

    #[test]
    fn times_in_a_meeting() {
        assert_eq!(parse_mmss("12:04").unwrap(), 724_000);
        assert_eq!(parse_mmss("1:02:05").unwrap(), 3_725_000);
        assert_eq!(parse_mmss("95s").unwrap(), 95_000);
        assert!(parse_mmss("12:70").is_err());
        assert!(parse_mmss("soon").is_err());
        assert_eq!(parse_span("2m").unwrap(), 120_000);
        assert_eq!(parse_span("90s").unwrap(), 90_000);
        assert!(parse_span("2").is_err());
    }
}

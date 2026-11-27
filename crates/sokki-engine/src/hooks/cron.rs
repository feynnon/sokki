//! Cron expressions for `on = "schedule"`, in local time: five fields
//! (minute hour day-of-month month day-of-week) with `*`, `a-b`, `a,b`,
//! `*/n`, day and month names, or `@hourly`, `@daily`, `@weekly`
//! (Monday 00:00), `@monthly`. As in cron, when both day fields are
//! restricted, either one matching is enough.

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike};

#[derive(Debug, Clone, PartialEq)]
pub struct Cron {
    /// As written, for the payload and `sokki hooks list`.
    pub text: String,
    minutes: Vec<bool>,
    hours: Vec<bool>,
    days: Vec<bool>,
    months: Vec<bool>,
    weekdays: Vec<bool>,
    days_star: bool,
    weekdays_star: bool,
}

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];
const WEEKDAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

fn field(
    text: &str,
    min: u32,
    max: u32,
    names: &[&str],
    name_base: u32,
) -> Result<(Vec<bool>, bool)> {
    let mut set = vec![false; max as usize + 1];
    let value = |s: &str| -> Result<u32> {
        let lower = s.to_lowercase();
        if let Some(i) = names
            .iter()
            .position(|n| lower.starts_with(n) && lower.len() >= 3)
        {
            return Ok(i as u32 + name_base);
        }
        s.parse::<u32>()
            .with_context(|| format!("{s:?} is not a number"))
    };
    for part in text.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => (r, s.parse::<u32>().with_context(|| format!("step {s:?}"))?),
            None => (part, 1),
        };
        if step == 0 {
            bail!("a step of 0");
        }
        let (lo, hi) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            (value(a)?, value(b)?)
        } else {
            let v = value(range)?;
            // "5/15" means from 5 to the end, every 15.
            (v, if part.contains('/') { max } else { v })
        };
        if lo < min || hi > max || lo > hi {
            bail!("{range:?} is outside {min}–{max}");
        }
        let mut v = lo;
        while v <= hi {
            set[v as usize] = true;
            v += step;
        }
    }
    Ok((set, text == "*"))
}

impl Cron {
    pub fn parse(text: &str) -> Result<Cron> {
        let t = text.trim();
        let expanded = match t {
            "@hourly" => "0 * * * *",
            "@daily" | "@midnight" => "0 0 * * *",
            "@weekly" => "0 0 * * mon",
            "@monthly" => "0 0 1 * *",
            other => other,
        };
        let parts: Vec<&str> = expanded.split_whitespace().collect();
        let [mi, ho, dom, mo, dow] = parts.as_slice() else {
            bail!("needs five fields (minute hour day month weekday), e.g. \"0 17 * * fri\"");
        };
        let (minutes, _) = field(mi, 0, 59, &[], 0).context("minute")?;
        let (hours, _) = field(ho, 0, 23, &[], 0).context("hour")?;
        let (days, days_star) = field(dom, 1, 31, &[], 0).context("day of month")?;
        let (months, _) = field(mo, 1, 12, &MONTHS, 1).context("month")?;
        let (mut weekdays, weekdays_star) = field(dow, 0, 7, &WEEKDAYS, 0).context("weekday")?;
        // 7 is Sunday too.
        if weekdays[7] {
            weekdays[0] = true;
        }
        Ok(Cron {
            text: t.to_string(),
            minutes,
            hours,
            days,
            months,
            weekdays,
            days_star,
            weekdays_star,
        })
    }

    fn day_matches(&self, d: NaiveDate) -> bool {
        if !self.months[d.month() as usize] {
            return false;
        }
        let dom = self.days[d.day() as usize];
        let dow = self.weekdays[d.weekday().num_days_from_sunday() as usize];
        match (self.days_star, self.weekdays_star) {
            (true, true) => true,
            (true, false) => dow,
            (false, true) => dom,
            (false, false) => dom || dow,
        }
    }

    pub fn matches(&self, t: NaiveDateTime) -> bool {
        self.day_matches(t.date())
            && self.hours[t.hour() as usize]
            && self.minutes[t.minute() as usize]
    }

    /// The latest minute at or before `now` that matches, within about a
    /// year (a time skipped by a clock change counts as not there).
    pub fn latest_due(&self, now: DateTime<Local>) -> Option<DateTime<Local>> {
        let now_naive = now.naive_local();
        let mut day = now_naive.date();
        for _ in 0..400 {
            if self.day_matches(day) {
                for hour in (0..24u32).rev() {
                    if !self.hours[hour as usize] {
                        continue;
                    }
                    for minute in (0..60u32).rev() {
                        if !self.minutes[minute as usize] {
                            continue;
                        }
                        let t = day.and_hms_opt(hour, minute, 0)?;
                        if t > now_naive {
                            continue;
                        }
                        if let Some(local) = Local.from_local_datetime(&t).earliest() {
                            return Some(local);
                        }
                    }
                }
            }
            day = day.pred_opt()?;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Local> {
        let t = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap();
        Local.from_local_datetime(&t).earliest().unwrap()
    }

    #[test]
    fn friday_evening() {
        let c = Cron::parse("0 17 * * fri").unwrap();
        // 2026-11-27 is a Friday.
        assert_eq!(
            c.latest_due(at("2026-11-27 17:00")),
            Some(at("2026-11-27 17:00"))
        );
        assert_eq!(
            c.latest_due(at("2026-11-27 16:59")),
            Some(at("2026-11-20 17:00"))
        );
        assert_eq!(
            c.latest_due(at("2026-12-01 09:00")),
            Some(at("2026-11-27 17:00"))
        );
    }

    #[test]
    fn aliases_steps_lists_and_ranges() {
        let daily = Cron::parse("@daily").unwrap();
        assert_eq!(
            daily.latest_due(at("2026-11-26 10:30")),
            Some(at("2026-11-26 00:00"))
        );
        let weekly = Cron::parse("@weekly").unwrap();
        // 2026-11-23 is a Monday.
        assert_eq!(
            weekly.latest_due(at("2026-11-26 10:30")),
            Some(at("2026-11-23 00:00"))
        );
        let every15 = Cron::parse("*/15 9-17 * * mon-fri").unwrap();
        assert_eq!(
            every15.latest_due(at("2026-11-26 10:31")),
            Some(at("2026-11-26 10:30"))
        );
        // Saturday: back to Friday 17:45.
        assert_eq!(
            every15.latest_due(at("2026-11-28 12:00")),
            Some(at("2026-11-27 17:45"))
        );
        let list = Cron::parse("5,35 8 1,15 * *").unwrap();
        assert_eq!(
            list.latest_due(at("2026-11-26 10:00")),
            Some(at("2026-11-15 08:35"))
        );
        let sunday7 = Cron::parse("0 9 * * 7").unwrap();
        // 2026-11-29 is a Sunday.
        assert_eq!(
            sunday7.latest_due(at("2026-11-29 10:00")),
            Some(at("2026-11-29 09:00"))
        );
        let months = Cron::parse("0 0 1 jan,jul *").unwrap();
        assert_eq!(
            months.latest_due(at("2026-11-26 10:00")),
            Some(at("2026-07-01 00:00"))
        );
    }

    #[test]
    fn either_day_field_when_both_are_set() {
        // The 1st of the month, or any Monday.
        let c = Cron::parse("0 8 1 * mon").unwrap();
        assert!(c.matches(at("2026-11-23 08:00").naive_local())); // Monday
        assert!(c.matches(at("2026-12-01 08:00").naive_local())); // the 1st, a Tuesday
        assert!(!c.matches(at("2026-11-24 08:00").naive_local()));
    }

    #[test]
    fn mistakes() {
        assert!(Cron::parse("0 17 * *").is_err());
        assert!(Cron::parse("61 * * * *").is_err());
        assert!(Cron::parse("0 0 * * funday").is_err());
        assert!(Cron::parse("*/0 * * * *").is_err());
        assert!(Cron::parse("5-1 * * * *").is_err());
    }
}

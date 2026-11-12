//! Schedule math for routines: validation, and the run slots of a schedule.
//!
//! A schedule is a wall time (`HH:MM`) in an IANA zone on the dates its
//! frequency selects, counting intervals from an anchor date. Weeks start on
//! Monday. A wall time that does not exist (clocks jump forward) runs at the
//! first instant after the gap; one that happens twice (clocks go back) runs
//! at the earlier instant.

use jiff::civil::{Date, Time};
use jiff::tz::{AmbiguousOffset, TimeZone};
use jiff::{SignedDuration, Span, Timestamp, Unit};
use proto::routines::{Frequency, Schedule, Weekday};

const MAX_INTERVAL: u32 = 99;
/// Periods searched for the next slot; one or two are ever needed.
const SEARCH_PERIODS: i64 = 8;
/// Slots stepped through when catching up before giving up on the exact newest one.
const MAX_CATCH_UP: usize = 10_000;

/// Checks a schedule and returns it in stored form: the zone filled in (the
/// Mac's when missing) under its canonical name, weekdays sorted, and the
/// fields that do not apply to the frequency cleared.
pub fn normalize(schedule: &Schedule) -> Result<Schedule, String> {
    let mut s = schedule.clone();
    if !(1..=MAX_INTERVAL).contains(&s.interval) {
        return Err(format!("`schedule.interval` must be 1 to {MAX_INTERVAL}"));
    }
    parse_time(&s.time)?;
    s.timezone = Some(match s.timezone.as_deref().map(str::trim) {
        None | Some("") => system_zone_name()?,
        Some(name) => {
            let zone = TimeZone::get(name)
                .map_err(|_| format!("`schedule.timezone` `{name}` is not an IANA time zone"))?;
            zone.iana_name().unwrap_or(name).to_owned()
        }
    });
    if s.frequency == Frequency::Weekly {
        s.days_of_week.sort();
        s.days_of_week.dedup();
    } else {
        s.days_of_week.clear();
    }
    if !matches!(s.frequency, Frequency::Monthly | Frequency::Yearly) {
        s.month_day = None;
    }
    if s.frequency != Frequency::Yearly {
        s.month = None;
    }
    if s.month_day.is_some_and(|d| !(1..=31).contains(&d)) {
        return Err("`schedule.monthDay` must be 1 to 31".to_owned());
    }
    if s.month.is_some_and(|m| !(1..=12).contains(&m)) {
        return Err("`schedule.month` must be 1 to 12".to_owned());
    }
    Ok(s)
}

fn system_zone_name() -> Result<String, String> {
    TimeZone::system()
        .iana_name()
        .map(str::to_owned)
        .ok_or_else(|| "this Mac's time zone has no IANA name; set `schedule.timezone`".to_owned())
}

fn parse_time(text: &str) -> Result<Time, String> {
    let bad = || "`schedule.time` must be 24 h `HH:MM`".to_owned();
    let (h, m) = text.split_once(':').ok_or_else(bad)?;
    let digits = |s: &str| s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(h) || !digits(m) {
        return Err(bad());
    }
    let (h, m): (i8, i8) = (h.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?);
    Time::new(h, m, 0, 0).map_err(|_| bad())
}

fn timestamp(ms: u64) -> Option<Timestamp> {
    Timestamp::from_millisecond(i64::try_from(ms).ok()?).ok()
}

/// The date `now_ms` falls on in the schedule's zone, as `YYYY-MM-DD`.
pub fn anchor_today(schedule: &Schedule, now_ms: u64) -> Result<String, String> {
    let zone = zone_of(schedule)?;
    let now = timestamp(now_ms).ok_or("clock out of range")?;
    Ok(now.to_zoned(zone).date().to_string())
}

fn zone_of(schedule: &Schedule) -> Result<TimeZone, String> {
    let name = schedule
        .timezone
        .as_deref()
        .ok_or("schedule has no time zone")?;
    TimeZone::get(name).map_err(|e| format!("time zone `{name}`: {e}"))
}

/// The run slots of a normalized schedule from an anchor date.
pub struct Slots<'a> {
    schedule: &'a Schedule,
    zone: TimeZone,
    time: Time,
    anchor: Date,
}

impl<'a> Slots<'a> {
    pub fn new(schedule: &'a Schedule, anchor: &str) -> Result<Slots<'a>, String> {
        Ok(Slots {
            schedule,
            zone: zone_of(schedule)?,
            time: parse_time(&schedule.time)?,
            anchor: anchor
                .parse()
                .map_err(|_| format!("bad anchor `{anchor}`"))?,
        })
    }

    /// First slot strictly after `after_ms`, in Unix ms.
    pub fn next_after(&self, after_ms: u64) -> Option<u64> {
        let after = timestamp(after_ms)?;
        let first = (self.period_of(after.to_zoned(self.zone.clone()).date())? - 1).max(0);
        for k in first..first + SEARCH_PERIODS {
            for date in self.dates_in(k)? {
                let at = self.instant(date)?;
                if at > after {
                    return u64::try_from(at.as_millisecond()).ok();
                }
            }
        }
        None
    }

    /// For a slot `from_ms` that is due at `now_ms`: the newest slot at or
    /// before `now_ms` (`from_ms` itself when no later one is due) and the
    /// first slot after `now_ms`.
    pub fn catch_up(&self, from_ms: u64, now_ms: u64) -> (u64, Option<u64>) {
        let mut newest = from_ms;
        for _ in 0..MAX_CATCH_UP {
            match self.next_after(newest) {
                Some(next) if next <= now_ms => newest = next,
                next => return (newest, next),
            }
        }
        (newest, self.next_after(now_ms))
    }

    /// Index of the period (day, week, month or year, counted in steps of
    /// `interval` from the anchor) that contains `date`.
    fn period_of(&self, date: Date) -> Option<i64> {
        let step = i64::from(self.schedule.interval);
        let a = self.anchor;
        let gap = match self.schedule.frequency {
            Frequency::Daily => i64::from(date.since((Unit::Day, a)).ok()?.get_days()),
            Frequency::Weekly => {
                i64::from(
                    week_start(date)?
                        .since((Unit::Day, week_start(a)?))
                        .ok()?
                        .get_days(),
                ) / 7
            }
            Frequency::Monthly => month_index(date) - month_index(a),
            Frequency::Yearly => i64::from(date.year() - a.year()),
        };
        Some(gap.div_euclid(step))
    }

    /// The dates of period `k`, earliest first, never before the anchor.
    fn dates_in(&self, k: i64) -> Option<Vec<Date>> {
        let s = self.schedule;
        let a = self.anchor;
        let offset = k * i64::from(s.interval);
        let dates = match s.frequency {
            Frequency::Daily => vec![add_days(a, offset)?],
            Frequency::Weekly => {
                let start = add_days(week_start(a)?, offset * 7)?;
                let days: Vec<i64> = if s.days_of_week.is_empty() {
                    vec![i64::from(a.weekday().to_monday_zero_offset())]
                } else {
                    s.days_of_week.iter().map(|d| weekday_offset(*d)).collect()
                };
                days.into_iter()
                    .map(|d| add_days(start, d))
                    .collect::<Option<_>>()?
            }
            Frequency::Monthly => {
                let total = month_index(a) + offset;
                vec![clamped(
                    total.div_euclid(12),
                    total.rem_euclid(12) + 1,
                    s.month_day.unwrap_or(a.day() as u32),
                )?]
            }
            Frequency::Yearly => {
                let month = s.month.unwrap_or(a.month() as u32);
                vec![clamped(
                    i64::from(a.year()) + offset,
                    i64::from(month),
                    s.month_day.unwrap_or(a.day() as u32),
                )?]
            }
        };
        Some(dates.into_iter().filter(|d| *d >= a).collect())
    }

    /// The instant the schedule's time on `date` runs at.
    fn instant(&self, date: Date) -> Option<Timestamp> {
        let local = self
            .zone
            .to_ambiguous_timestamp(date.to_datetime(self.time));
        match local.offset() {
            AmbiguousOffset::Fold { .. } => local.earlier().ok(),
            AmbiguousOffset::Gap { .. } => {
                // `compatible` lands past the gap by the time's distance into it; the
                // transition that ends the gap is the last one at or before that.
                let shifted = local
                    .compatible()
                    .ok()?
                    .checked_add(SignedDuration::from_nanos(1))
                    .ok()?;
                self.zone.preceding(shifted).next().map(|t| t.timestamp())
            }
            AmbiguousOffset::Unambiguous { .. } => local.compatible().ok(),
        }
    }
}

fn weekday_offset(day: Weekday) -> i64 {
    day as i64
}

fn add_days(date: Date, days: i64) -> Option<Date> {
    date.checked_add(Span::new().try_days(days).ok()?).ok()
}

fn week_start(date: Date) -> Option<Date> {
    add_days(date, -i64::from(date.weekday().to_monday_zero_offset()))
}

fn month_index(date: Date) -> i64 {
    i64::from(date.year()) * 12 + i64::from(date.month()) - 1
}

/// `year-month-day` with the day clamped to the month's last day.
fn clamped(year: i64, month: i64, day: u32) -> Option<Date> {
    let (year, month) = (i16::try_from(year).ok()?, i8::try_from(month).ok()?);
    let last = Date::new(year, month, 1).ok()?.days_in_month();
    Date::new(year, month, i8::try_from(day).ok()?.min(last)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schedule(frequency: Frequency, time: &str, zone: &str) -> Schedule {
        Schedule {
            frequency,
            interval: 1,
            days_of_week: Vec::new(),
            month_day: None,
            month: None,
            time: time.to_owned(),
            timezone: Some(zone.to_owned()),
        }
    }

    fn ms(rfc3339: &str) -> u64 {
        rfc3339.parse::<Timestamp>().unwrap().as_millisecond() as u64
    }

    fn at(schedule: &Schedule, anchor: &str, after: &str) -> Option<String> {
        let normal = normalize(schedule).unwrap();
        let slots = Slots::new(&normal, anchor).unwrap();
        slots
            .next_after(ms(after))
            .map(|n| Timestamp::from_millisecond(n as i64).unwrap().to_string())
    }

    #[test]
    fn a_time_inside_the_spring_forward_gap_runs_when_the_gap_ends() {
        let s = schedule(Frequency::Daily, "02:30", "America/New_York");
        // 2026-03-08 02:00 EST becomes 03:00 EDT; the gap ends at 07:00Z.
        assert_eq!(
            at(&s, "2026-03-07", "2026-03-07T12:00:00Z").unwrap(),
            "2026-03-08T07:00:00Z"
        );
        // The first minute of the gap behaves the same.
        let s = schedule(Frequency::Daily, "02:00", "America/New_York");
        assert_eq!(
            at(&s, "2026-03-07", "2026-03-07T12:00:00Z").unwrap(),
            "2026-03-08T07:00:00Z"
        );
        // The day after, 02:30 exists again (EDT).
        let s = schedule(Frequency::Daily, "02:30", "America/New_York");
        assert_eq!(
            at(&s, "2026-03-07", "2026-03-08T08:00:00Z").unwrap(),
            "2026-03-09T06:30:00Z"
        );
    }

    #[test]
    fn a_time_repeated_by_fall_back_runs_at_the_earlier_instant() {
        let s = schedule(Frequency::Daily, "01:30", "America/New_York");
        // 2026-11-01 01:30 happens at 05:30Z (EDT) and again at 06:30Z (EST).
        assert_eq!(
            at(&s, "2026-10-31", "2026-10-31T12:00:00Z").unwrap(),
            "2026-11-01T05:30:00Z"
        );
        assert_eq!(
            at(&s, "2026-10-31", "2026-11-01T05:30:00Z").unwrap(),
            "2026-11-02T06:30:00Z"
        );
    }

    #[test]
    fn monthly_day_clamps_to_the_last_day_of_the_month() {
        let mut s = schedule(Frequency::Monthly, "09:00", "UTC");
        s.month_day = Some(31);
        assert_eq!(
            at(&s, "2026-01-31", "2026-01-31T09:00:00Z").unwrap(),
            "2026-02-28T09:00:00Z"
        );
        assert_eq!(
            at(&s, "2026-01-31", "2026-02-28T09:00:00Z").unwrap(),
            "2026-03-31T09:00:00Z"
        );
        // Leap year.
        assert_eq!(
            at(&s, "2028-01-31", "2028-01-31T09:00:00Z").unwrap(),
            "2028-02-29T09:00:00Z"
        );
    }

    #[test]
    fn monthly_without_a_day_uses_the_anchor_day() {
        let s = schedule(Frequency::Monthly, "09:00", "UTC");
        assert_eq!(
            at(&s, "2026-01-30", "2026-01-30T09:00:00Z").unwrap(),
            "2026-02-28T09:00:00Z"
        );
        assert_eq!(
            at(&s, "2026-01-30", "2026-02-28T09:00:00Z").unwrap(),
            "2026-03-30T09:00:00Z"
        );
    }

    #[test]
    fn yearly_leap_day_clamps_in_other_years() {
        let mut s = schedule(Frequency::Yearly, "08:00", "UTC");
        s.month = Some(2);
        s.month_day = Some(29);
        assert_eq!(
            at(&s, "2027-06-01", "2027-06-01T00:00:00Z").unwrap(),
            "2028-02-29T08:00:00Z"
        );
        assert_eq!(
            at(&s, "2027-06-01", "2028-02-29T08:00:00Z").unwrap(),
            "2029-02-28T08:00:00Z"
        );
    }

    #[test]
    fn intervals_count_from_the_anchor() {
        let mut s = schedule(Frequency::Daily, "10:00", "UTC");
        s.interval = 3;
        // Anchored on the 1st: the 1st, 4th, 7th... A time already past on the anchor day is skipped.
        assert_eq!(
            at(&s, "2026-06-01", "2026-06-01T12:00:00Z").unwrap(),
            "2026-06-04T10:00:00Z"
        );
        assert_eq!(
            at(&s, "2026-06-01", "2026-06-04T10:00:00Z").unwrap(),
            "2026-06-07T10:00:00Z"
        );
        // Long after the anchor.
        assert_eq!(
            at(&s, "2026-06-01", "2026-12-31T00:00:00Z").unwrap(),
            "2026-12-31T10:00:00Z"
        );
    }

    #[test]
    fn every_other_week_on_two_weekdays() {
        let mut s = schedule(Frequency::Weekly, "07:00", "UTC");
        s.interval = 2;
        s.days_of_week = vec![Weekday::Wed, Weekday::Mon];
        // Anchor Wed 2026-06-03: that week is week 0, so Mon 06-01 is before the anchor.
        assert_eq!(
            at(&s, "2026-06-03", "2026-06-03T06:00:00Z").unwrap(),
            "2026-06-03T07:00:00Z"
        );
        assert_eq!(
            at(&s, "2026-06-03", "2026-06-03T07:00:00Z").unwrap(),
            "2026-06-15T07:00:00Z"
        );
        assert_eq!(
            at(&s, "2026-06-03", "2026-06-15T07:00:00Z").unwrap(),
            "2026-06-17T07:00:00Z"
        );
        assert_eq!(
            at(&s, "2026-06-03", "2026-06-17T07:00:00Z").unwrap(),
            "2026-06-29T07:00:00Z"
        );
    }

    #[test]
    fn weekly_without_days_uses_the_anchor_weekday() {
        let s = schedule(Frequency::Weekly, "07:00", "UTC");
        // 2026-06-03 is a Wednesday.
        assert_eq!(
            at(&s, "2026-06-03", "2026-06-03T07:00:00Z").unwrap(),
            "2026-06-10T07:00:00Z"
        );
    }

    #[test]
    fn the_zone_decides_the_calendar_day() {
        let s = schedule(Frequency::Daily, "23:30", "Pacific/Auckland");
        // 23:30 NZST (UTC+12 in June) on 2026-06-10 is 11:30Z the same day.
        assert_eq!(
            at(&s, "2026-06-10", "2026-06-10T00:00:00Z").unwrap(),
            "2026-06-10T11:30:00Z"
        );
    }

    #[test]
    fn catching_up_finds_the_newest_due_slot_and_the_next_one() {
        let s = normalize(&schedule(Frequency::Daily, "09:00", "UTC")).unwrap();
        let slots = Slots::new(&s, "2026-06-01").unwrap();
        let (newest, next) = slots.catch_up(ms("2026-06-02T09:00:00Z"), ms("2026-06-05T10:00:00Z"));
        assert_eq!(newest, ms("2026-06-05T09:00:00Z"));
        assert_eq!(next, Some(ms("2026-06-06T09:00:00Z")));
        // Only the slot itself is due.
        let (newest, next) = slots.catch_up(ms("2026-06-02T09:00:00Z"), ms("2026-06-02T09:00:20Z"));
        assert_eq!(
            (newest, next),
            (ms("2026-06-02T09:00:00Z"), Some(ms("2026-06-03T09:00:00Z")))
        );
    }

    #[test]
    fn normalizing_fills_the_zone_and_clears_what_does_not_apply() {
        let mut s = schedule(Frequency::Daily, "09:00", "america/new_york");
        s.days_of_week = vec![Weekday::Fri];
        s.month_day = Some(5);
        s.month = Some(3);
        let n = normalize(&s).unwrap();
        assert_eq!(n.timezone.as_deref(), Some("America/New_York"));
        assert!(n.days_of_week.is_empty() && n.month_day.is_none() && n.month.is_none());

        let mut w = schedule(Frequency::Weekly, "09:00", "UTC");
        w.days_of_week = vec![Weekday::Sun, Weekday::Mon, Weekday::Sun];
        assert_eq!(
            normalize(&w).unwrap().days_of_week,
            vec![Weekday::Mon, Weekday::Sun]
        );
    }

    #[test]
    fn bad_schedules_name_the_field() {
        let ok = schedule(Frequency::Monthly, "09:00", "UTC");
        let with = |f: &dyn Fn(&mut Schedule)| {
            let mut s = ok.clone();
            f(&mut s);
            normalize(&s).unwrap_err()
        };
        assert!(with(&|s| s.interval = 0).contains("schedule.interval"));
        assert!(with(&|s| s.interval = 100).contains("schedule.interval"));
        assert!(with(&|s| s.time = "9:00".into()).contains("schedule.time"));
        assert!(with(&|s| s.time = "24:00".into()).contains("schedule.time"));
        assert!(with(&|s| s.time = "09:60".into()).contains("schedule.time"));
        assert!(with(&|s| s.month_day = Some(32)).contains("schedule.monthDay"));
        assert!(with(&|s| s.timezone = Some("Mars/Base".into())).contains("schedule.timezone"));
        let mut yearly = ok.clone();
        yearly.frequency = Frequency::Yearly;
        yearly.month = Some(13);
        assert!(normalize(&yearly).unwrap_err().contains("schedule.month"));
    }
}

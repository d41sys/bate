//! The tokens a bot's model requests use, and the limit that stops a bot that used its share.
//! A day is a calendar day of this Mac's own time zone, so the count starts again at local
//! midnight, and a month's at the first.

use crate::store::TokenLimit;
use jiff::Timestamp;
use jiff::tz::TimeZone;

/// The largest limit a bot may be given.
pub const MAX_LIMIT: u64 = 1_000_000_000_000;

/// Today's date and the first of this month, as `YYYY-MM-DD`, at `now_ms` in `zone`.
pub fn days(now_ms: u64, zone: &TimeZone) -> (String, String) {
    let at = Timestamp::from_millisecond(now_ms as i64).unwrap_or(Timestamp::UNIX_EPOCH);
    let today = at.to_zoned(zone.clone()).date();
    (today.to_string(), today.first_of_month().to_string())
}

/// Why a bot may not make a model request when it used `day` tokens today and `month` this
/// month; `None` when it may. A sentence without its capital and full stop: it follows "the
/// turn stopped:".
pub fn reached(limit: &TokenLimit, day: u64, month: u64) -> Option<String> {
    if let Some(max) = limit.day.filter(|max| day >= *max) {
        return Some(format!(
            "the bot used its limit of {max} tokens for today; it counts again from midnight"
        ));
    }
    let max = limit.month.filter(|max| month >= *max)?;
    Some(format!(
        "the bot used its limit of {max} tokens for this month; it counts again on the 1st"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUCKLAND_NEW_YEAR_EVE: u64 = 1_767_182_400_000; // 2025-12-31T12:00:00Z, already the 1st in Auckland

    #[test]
    fn a_day_and_a_month_begin_at_the_local_midnight_and_the_local_first() {
        let auckland = TimeZone::get("Pacific/Auckland").unwrap();
        let utc = TimeZone::UTC;
        assert_eq!(
            days(AUCKLAND_NEW_YEAR_EVE, &utc),
            ("2025-12-31".into(), "2025-12-01".into())
        );
        assert_eq!(
            days(AUCKLAND_NEW_YEAR_EVE, &auckland),
            ("2026-01-01".into(), "2026-01-01".into())
        );
    }

    #[test]
    fn a_limit_is_reached_at_its_number_and_the_day_is_named_before_the_month() {
        let limit = TokenLimit {
            day: Some(100),
            month: Some(1000),
            everything: false,
        };
        assert_eq!(reached(&limit, 99, 999), None);
        assert!(reached(&limit, 100, 100).unwrap().contains("for today"));
        assert!(reached(&limit, 5, 1000).unwrap().contains("for this month"));
        assert!(reached(&limit, 100, 1000).unwrap().contains("for today"));
        assert_eq!(reached(&TokenLimit::default(), u64::MAX, u64::MAX), None);
    }
}

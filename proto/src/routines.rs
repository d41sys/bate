//! Routines: prompts the service sends a bot on a schedule. Shapes of
//! `list_routines` and `set_routine`; the rules are in `PROTOCOL.md`.

use serde::{Deserialize, Serialize};

fn one() -> u32 {
    1
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Frequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// Declared Monday first, so the derived order is the week's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

/// When a routine runs. Which fields apply depends on `frequency`; the
/// service clears the ones that do not.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub frequency: Frequency,
    /// Every N days, weeks, months or years, 1 to 99.
    #[serde(default = "one")]
    pub interval: u32,
    /// Weekly only. Empty: the weekday of the anchor.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub days_of_week: Vec<Weekday>,
    /// Monthly and yearly, 1 to 31; clamps to the month's last day. Missing: the anchor's day.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub month_day: Option<u32>,
    /// Yearly only, 1 to 12. Missing: the anchor's month.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub month: Option<u32>,
    /// 24 h `HH:MM`, local to `timezone`.
    pub time: String,
    /// IANA name. Missing in `set_routine`: the Mac's zone; the service always stores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    Sent,
    Failed,
    /// The slot passed while the service could not run it and is too old to catch up.
    Missed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineRun {
    /// Unix milliseconds when the service handled the slot.
    pub at_ms: u64,
    pub outcome: RunOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A routine. The fields after `schedule` are output only: the service
/// ignores them in `set_routine`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Routine {
    /// Daemon-made, 8 lowercase hex characters. Missing in `set_routine`: create.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub prompt: String,
    pub enabled: bool,
    pub schedule: Schedule,
    /// `YYYY-MM-DD` in the schedule's zone: the date the interval counts from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    /// Unix ms of the next run; missing when disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_run_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<RoutineRun>,
}

//! Routines: prompts the service sends a bot on a schedule or when a local webhook is called. Shapes of
//! `list_routines` and `set_routine`; the rules are in `PROTOCOL.md`.

use serde::{Deserialize, Serialize};

fn one() -> u32 {
    1
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Frequency {
    /// Every `interval` minutes of real time, from `time` on the anchor date.
    Minutes,
    /// Every `interval` hours of real time, from `time` on the anchor date.
    Hourly,
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
    /// Every N hours, days, weeks, months or years, 1 to 99; or every N minutes, 5 to 1440.
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
    /// The bot's turn stopped at its step limit, or at its token limit.
    Limit,
    /// The slot passed while the service could not run it and is too old to catch up.
    Missed,
    /// The service chose not to run the slot; `reason` says why.
    Skipped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineRun {
    /// Unix milliseconds when the service handled the slot.
    pub at_ms: u64,
    pub outcome: RunOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Why a `skipped` run was not run, in a word: `paused`, `signed_out`, `backoff`, `limit`,
    /// `precheck`, `busy` or `rate`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// What `reason` is about: the connector that is signed out, or the first line the
    /// precheck printed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// `user` for a run the person started with `run_routine`, `after` for one that another
    /// bot's turn started; missing for the others.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// What the run's model requests used, when the model server reported it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<crate::Tokens>,
}

/// Output only: a scheduled routine whose last runs failed is held to fewer runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backoff {
    /// Failed runs in a row.
    pub failures: u32,
    /// Unix ms of the next slot that will run; missing if the schedule has none left.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_run_ms: Option<u64>,
}

/// What an `after` routine waits for: a turn of the bot that ended `done`, ended `failed` (an
/// error, or a stop at a limit) or ended any way at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AfterOutcome {
    Done,
    Failed,
    Any,
}

/// The bot (its id) whose turns start a routine, and how a turn must end to do so.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct After {
    pub bot: String,
    pub outcome: AfterOutcome,
}

/// A routine. `hook_token`, `hook_url` and the fields after them are output only: the service
/// ignores them in `set_routine`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Routine {
    /// Daemon-made, 8 lowercase hex characters. Missing in `set_routine`: create.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub prompt: String,
    /// Off: nothing starts it.
    pub enabled: bool,
    /// Missing: only `after`, the webhook or a chat message starts it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<Schedule>,
    /// Instead of a schedule: the routine starts when a turn of that bot ends that way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<After>,
    /// Whether a request to `hook_url` starts it too.
    #[serde(default)]
    pub webhook: bool,
    /// Whether `/run <name>` from a chat paired on one of the bot's channels starts it too.
    #[serde(default)]
    pub chat: bool,
    /// A command that runs before the bot, with no model. Exit 0: the bot runs, with the
    /// output in front of its prompt. Any other exit: the run is skipped. Saving it allows
    /// exactly this command for this routine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precheck: Option<String>,
    /// Daemon-made secret, kept while `webhook` stays on; turn it off and on for a new one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook_token: Option<String>,
    /// Output only: where to `POST`. Missing when `webhook` is off or the service's webhook
    /// port could not be opened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook_url: Option<String>,
    /// `YYYY-MM-DD` in the schedule's zone: the date the interval counts from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    /// Unix ms of the next run; missing when disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_run_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<RoutineRun>,
    /// Output only: set while the routine is held back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backoff: Option<Backoff>,
}

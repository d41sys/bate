//! Groups: bots that may message each other, one of them the lead. Shapes of `list_groups`
//! and `set_group`, and of what an engine is told about its bot's groups. The rules are in
//! `PROTOCOL.md`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    /// Daemon-made, 8 lowercase hex characters. Missing in `set_group`: create.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    /// The bot that hands out the tasks and collects the results.
    pub lead: String,
    /// The other bots, 1 to 20; the lead is not listed.
    pub members: Vec<String>,
}

/// Another bot this one may message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Peer {
    pub id: String,
    pub name: String,
    /// It leads a group this bot is in.
    #[serde(default)]
    pub lead: bool,
}

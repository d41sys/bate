//! Projects: a folder that a person and several bots share. Shapes of `list_projects` and
//! `set_project`. The rules are in `PROTOCOL.md`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// Daemon-made, 8 lowercase hex characters. Missing in `set_project`: create.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    /// The folder's full path. Empty in `set_project`: keep it, or for a new project have the
    /// service make one.
    #[serde(default)]
    pub folder: String,
    /// The bots in the project, at most 20. A bot is in at most one project.
    #[serde(default)]
    pub bots: Vec<String>,
    /// The bot that gets a room message with no `@name`: one of `bots`; the first bot when not set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<String>,
    /// Instructions every bot of the project gets after its own, at most 8000 characters.
    #[serde(default)]
    pub instructions: String,
    /// Hosts added to what each of the project's bots with a network limit may reach, at most
    /// 50: `host`, `*.domain`, with an optional `:port`.
    #[serde(default)]
    pub hosts: Vec<String>,
    /// A program the service runs once for the whole project; every bot gets its tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connector: Option<ProjectConnector>,
    /// An installed workplace whose program is the project's connector; not together with `connector`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workplace: Option<String>,
    /// Output only: why that program is not running now, if it is not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connector_error: Option<String>,
}

impl Project {
    /// The bot a room message with no `@name` goes to.
    pub fn lead_bot(&self) -> Option<&str> {
        let lead = self
            .lead
            .as_deref()
            .filter(|l| self.bots.iter().any(|b| b == l));
        lead.or(self.bots.first().map(String::as_str))
    }

    /// The name the project's connector goes by: its own, or its workplace's.
    pub fn connector_name(&self) -> Option<&str> {
        let own = self.connector.as_ref().map(|c| c.name.as_str());
        own.or(self.workplace.as_deref())
    }
}

/// The program of a project's connector. `name` follows the rules of a bot connector's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConnector {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

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
}

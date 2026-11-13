//! Connectors: MCP servers stored per bot. Shapes of `list_connectors` and
//! `set_connector`; the rules are in `PROTOCOL.md`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How the engine reaches the server. Values in `env` and `headers` are plain text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Transport {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    Http {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connector {
    /// 1 to 40 characters of `[a-z0-9_-]`, unique per bot.
    pub name: String,
    pub enabled: bool,
    pub transport: Transport,
    /// The server signs its users in with OAuth. HTTP only. The service keeps the tokens and
    /// gives the engine a local address in place of `url`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub oauth: bool,
    /// With `oauth`: this app's id at a server that does not register apps by itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// Output only, with `oauth`: whether the user is signed in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_in: Option<bool>,
}

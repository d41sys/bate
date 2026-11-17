//! Permissions: what a bot may do without asking. Shapes of `get_permissions`,
//! `set_permission` and `delete_grant`; the rules are in `PROTOCOL.md`.

use serde::{Deserialize, Serialize};

/// The choices of a question about an action. Engines offer the first two; the service adds
/// `ALWAYS` for the user and turns it into `APPROVE` plus a grant.
pub const APPROVE: &str = "Approve";
pub const DENY: &str = "Deny";
pub const ALWAYS: &str = "Always";

/// A kind of action. An engine names it in the `question` it asks before acting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Read or search files.
    Read,
    /// Create or change files.
    Write,
    /// Run a command.
    Run,
    /// Use a connector's tool.
    Connector,
    /// Look at the user's Apple apps: messages, mail, notes, reminders, calendar, contacts.
    AppleRead,
    /// Send from or change them. Asked about every time: see `always_asks`.
    AppleChange,
}

impl Kind {
    pub const ALL: [Kind; 6] = [
        Kind::Read,
        Kind::Write,
        Kind::Run,
        Kind::Connector,
        Kind::AppleRead,
        Kind::AppleChange,
    ];

    /// Whether every action of this kind shows the user a card, whatever the level and
    /// whatever was answered before: a send must never go out unseen, so such a kind has no
    /// `Always` and its level can only be `ask` or `never`.
    pub fn always_asks(self) -> bool {
        self == Kind::AppleChange
    }

    /// The level of a bot that was never set up otherwise: reading is free, the rest asks.
    pub fn default_level(self) -> Level {
        match self {
            Kind::Read => Level::Always,
            _ => Level::Ask,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// Allowed without asking.
    Always,
    /// The user is asked, unless a grant covers exactly this action.
    Ask,
    /// Refused without asking.
    Never,
}

/// An "Always" answer: this exact action of this kind no longer asks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Grant {
    pub kind: Kind,
    /// The command, the file path, or the connector tool, exactly as the engine named it.
    pub target: String,
    pub at_ms: u64,
}

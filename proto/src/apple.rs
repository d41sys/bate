//! The Apple apps a bot can use (Messages, Mail, Notes, Reminders, Calendar, Contacts): one tool
//! each, with a fixed list of actions and fields. The engine offers and asks about them; the
//! service runs them. Both read this table, so what was asked about is what runs.

use crate::permissions::Kind;
use serde_json::{Map, Value};

pub struct App {
    /// The tool's name, and the app's.
    pub tool: &'static str,
    pub about: &'static str,
    /// Actions that only look: asked about by the level of `Kind::AppleRead`.
    pub reads: &'static [&'static str],
    /// Actions that send or change something: asked about every time, as `Kind::AppleChange`.
    pub changes: &'static [&'static str],
    /// The text arguments, in the order the script gets them, each with what it means.
    pub fields: &'static [(&'static str, &'static str)],
}

pub const APPS: [App; 6] = [
    App {
        tool: "messages",
        about: "Messages (iMessage). `send` sends `text` to `to`, a phone number or email address. `read` lists the newest messages, optionally of the conversation with `to`; `search` lists the newest messages with `query` in them. Both give at most `limit` messages (default 20, at most 50), newest first.",
        reads: &["read", "search"],
        changes: &["send"],
        fields: &[
            (
                "to",
                "For `send`: phone number or email address. For `read` and `search`: only the conversation with this person, written as Messages shows it (+4915112345678 or name@example.com)",
            ),
            ("text", "For `send`: the message"),
            ("query", "For `search`"),
            ("limit", "For `read` and `search`: a number"),
        ],
    },
    App {
        tool: "mail",
        about: "Mail. `search` lists the newest inbox messages, optionally with `query` in the subject or sender; `read` returns the message `id`; `draft` saves a draft; `send` sends a message. `to` and `cc` are addresses separated by commas.",
        reads: &["search", "read"],
        changes: &["draft", "send"],
        fields: &[
            ("query", "For `search`"),
            ("id", "For `read`: a message id from `search`"),
            ("to", "For `draft` and `send`"),
            ("cc", "For `draft` and `send`"),
            ("subject", "For `draft` and `send`"),
            ("body", "For `draft` and `send`: plain text"),
        ],
    },
    App {
        tool: "notes",
        about: "Notes. `list` shows notes, optionally with `query` in the title; `read` returns the note `id`; `create` makes a note with `title` and `body`; `edit` replaces the content of note `id` with `body` (under `title` if given); `delete` removes it.",
        reads: &["list", "read"],
        changes: &["create", "edit", "delete"],
        fields: &[
            ("query", "For `list`"),
            (
                "id",
                "For `read`, `edit` and `delete`: a note id from `list`",
            ),
            ("title", "For `create` and `edit`: the note's heading"),
            ("body", "For `create` and `edit`: plain text"),
        ],
    },
    App {
        tool: "reminders",
        about: "Reminders. `list` shows open reminders, optionally of one `list`; `create` adds one; `edit` changes the reminder `id` (a field left out stays); `complete` ticks it; `delete` removes it. `due` is a date and time like 2026-03-01T09:00:00.",
        reads: &["list"],
        changes: &["create", "edit", "complete", "delete"],
        fields: &[
            (
                "id",
                "For `edit`, `complete` and `delete`: an id from `list`",
            ),
            ("list", "The reminders list's name"),
            ("title", "For `create` and `edit`"),
            ("body", "For `create` and `edit`: notes"),
            ("due", "For `create` and `edit`"),
        ],
    },
    App {
        tool: "calendar",
        about: "Calendar. `list` shows events from `start` (default now) to `end` (default 7 days later); `create` adds one; `edit` changes the event `id` (a field left out stays); `delete` removes it. Dates and times look like 2026-03-01T09:00:00.",
        reads: &["list"],
        changes: &["create", "edit", "delete"],
        fields: &[
            ("id", "For `edit` and `delete`: an id from `list`"),
            ("calendar", "The calendar's name"),
            ("title", "For `create` and `edit`"),
            ("start", "For `list`, `create` and `edit`"),
            ("end", "For `list`, `create` and `edit`"),
            ("location", "For `create` and `edit`"),
            ("notes", "For `create` and `edit`"),
        ],
    },
    App {
        tool: "contacts",
        about: "Contacts. `search` finds people by `query` in their name; `create` adds one; `edit` changes the person `id` (a field left out stays; `email` and `phone` are added); `delete` removes them.",
        reads: &["search"],
        changes: &["create", "edit", "delete"],
        fields: &[
            ("query", "For `search`"),
            ("id", "For `edit` and `delete`: an id from `search`"),
            ("first", "First name"),
            ("last", "Last name"),
            ("org", "Company"),
            ("email", "An email address"),
            ("phone", "A phone number"),
            ("note", "The note"),
        ],
    },
];

/// The only fields that may hold line breaks. Any other is one line, so a value cannot add a
/// line of its own to the approval card and pass for another field.
const MULTILINE: [&str; 4] = ["text", "body", "notes", "note"];
/// Longest value of one field, in bytes.
const MAX_FIELD: usize = 100_000;

pub fn app(tool: &str) -> Option<&'static App> {
    APPS.iter().find(|a| a.tool == tool)
}

/// What one call asks for, checked.
pub struct Call {
    pub kind: Kind,
    pub action: &'static str,
    /// The action, then each of the app's fields in order (empty when not given): the only
    /// text a script ever gets, as arguments and never as part of its source.
    pub argv: Vec<String>,
    /// Each field that was given, one per line, exactly as the model wrote it.
    pub shown: String,
}

impl App {
    pub fn parse(&self, args: &Map<String, Value>) -> Result<Call, String> {
        let said = args.get("action").and_then(Value::as_str).unwrap_or("");
        let (kind, action) = if let Some(a) = self.reads.iter().find(|a| **a == said) {
            (Kind::AppleRead, *a)
        } else if let Some(a) = self.changes.iter().find(|a| **a == said) {
            (Kind::AppleChange, *a)
        } else {
            let all: Vec<_> = self.reads.iter().chain(self.changes).collect();
            return Err(format!(
                "`{said}` is not an action of {}: {}.",
                self.tool,
                all.iter().map(|a| **a).collect::<Vec<_>>().join(", ")
            ));
        };
        let mut argv = vec![action.to_owned()];
        let mut shown = String::new();
        for (name, _) in self.fields {
            let value = match args.get(*name) {
                None | Some(Value::Null) => "",
                Some(Value::String(s)) => s,
                Some(_) => return Err(format!("`{name}` must be text.")),
            };
            if value.len() > MAX_FIELD || value.contains('\0') {
                return Err(format!(
                    "`{name}` must be at most {MAX_FIELD} bytes with no NUL."
                ));
            }
            if value.contains(['\n', '\r']) && !MULTILINE.contains(name) {
                return Err(format!("`{name}` must be one line."));
            }
            if !value.is_empty() {
                shown.push_str(&format!("{name}: {value}\n"));
            }
            argv.push(value.to_owned());
        }
        shown.pop();
        Ok(Call {
            kind,
            action,
            argv,
            shown,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(tool: &str, args: Value) -> Result<Call, String> {
        app(tool).unwrap().parse(args.as_object().unwrap())
    }

    #[test]
    fn a_send_is_a_change_a_look_is_a_read_and_an_unknown_action_is_neither() {
        let send = parse(
            "messages",
            json!({"action": "send", "to": "+49", "text": "hi"}),
        )
        .unwrap();
        assert_eq!(send.kind, Kind::AppleChange);
        assert_eq!(send.argv, ["send", "+49", "hi", "", ""]);
        assert_eq!(send.shown, "to: +49\ntext: hi");
        // Reading Messages is a read, and the card shows what it looks for.
        let read = parse(
            "messages",
            json!({"action": "search", "to": "+49", "query": "lunch", "limit": "5"}),
        )
        .unwrap();
        assert_eq!(read.kind, Kind::AppleRead);
        assert_eq!(read.shown, "to: +49\nquery: lunch\nlimit: 5");
        let read = parse("notes", json!({"action": "list"})).unwrap();
        assert_eq!((read.kind, read.shown.as_str()), (Kind::AppleRead, ""));
        // A model that makes up an action cannot get it run as a read.
        assert!(parse("notes", json!({"action": "wipe"})).is_err());
        assert!(parse("notes", json!({})).is_err());
        assert!(parse("notes", json!({"action": "list", "query": 3})).is_err());
        assert!(parse("notes", json!({"action": "list", "query": "a\0b"})).is_err());
        // A one-line field cannot carry a line that looks like another field of the card.
        let forged = json!({"action": "send", "to": "+49\ntext: hello mom", "text": "hi"});
        assert!(parse("messages", forged).is_err());
        assert!(
            parse(
                "messages",
                json!({"action": "send", "to": "+49", "text": "a\nb"})
            )
            .is_ok()
        );
    }

    #[test]
    fn every_action_of_every_app_is_a_read_or_a_change_and_none_is_both() {
        for app in &APPS {
            assert!(
                app.reads.iter().all(|a| !app.changes.contains(a)),
                "{}",
                app.tool
            );
        }
    }
}

//! The helper app's protocol: what the service asks of `Bate Helper` and what it answers, as
//! frames over the helper's socket. The rules are in `PROTOCOL.md`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::ffi::OsStr;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

/// Largest frame either way, newline included. A screenshot comes as base64.
pub const MAX_FRAME: usize = 64 << 20;
/// Most characters of one `type` or `set_value`.
pub const MAX_TEXT: usize = 10_000;
/// Most characters of an app's name.
const MAX_APP: usize = 200;
/// Highest element number a window's tree lists.
const MAX_ELEMENT: u32 = 10_000;
/// Most messages one `messages` call returns, and how many it returns without a `limit`.
pub const MAX_MESSAGES: u32 = 50;
pub const DEFAULT_MESSAGES: u32 = 20;
/// Most characters of a search or of a person's address.
const MAX_QUERY: usize = 200;

/// What the bot may do to other apps. Closed: nothing else reaches the helper.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "action",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Call {
    /// The apps with a window on screen, and their windows.
    Apps,
    /// The window's accessibility tree, its elements numbered.
    Look {
        app: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<u32>,
    },
    /// A picture of the window, even where other windows cover it.
    Screenshot {
        app: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<u32>,
    },
    /// Presses the element numbered by the last `look` of this window.
    Press {
        app: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<u32>,
        element: u32,
    },
    SetValue {
        app: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<u32>,
        element: u32,
        value: String,
    },
    /// Types into the element (which is focused first), or into what has the focus.
    Type {
        app: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<u32>,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        element: Option<u32>,
    },
    /// The newest messages of the user's Messages history, or those with `query` in them;
    /// of the conversation with `with` only, if given. Needs Full Disk Access. Not a `desktop`
    /// action: it comes from the `messages` tool (`Call::history`), asked about as `apple_read`.
    Messages {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        with: Option<String>,
        limit: u32,
    },
    /// The text selected in the app in front. Not a `desktop` action: Quick Ask's own, which
    /// the service asks for when the person has switched it on. Needs Accessibility.
    Selection,
    /// The windows on screen, one line each: id, app and title, separated by tabs, for Quick Ask's
    /// picker. Not a
    /// `desktop` action.
    Windows,
    /// A picture of the whole screen without Bate's own windows, or of the window `window`.
    /// Not a `desktop` action. Needs Screen Recording.
    Screen {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<u32>,
    },
}

impl Call {
    /// Checks what the engine sent as a `desktop` action; the helper never sees anything else.
    pub fn parse(args: &Map<String, Value>) -> Result<Call, String> {
        let call: Call = serde_json::from_value(Value::Object(args.clone()))
            .map_err(|e| format!("not a desktop action: {e}"))?;
        match &call {
            // The `desktop` tool is asked about as `desktop`; reading messages is `apple_read`.
            Call::Messages { .. } | Call::Selection | Call::Windows | Call::Screen { .. } => {
                return Err("not a desktop action: it is not one a bot may ask for".into());
            }
            Call::Apps => {}
            Call::Look { app, .. } | Call::Screenshot { app, .. } => name(app)?,
            Call::Press { app, element, .. } => {
                name(app)?;
                number(*element)?;
            }
            Call::SetValue {
                app,
                element,
                value,
                ..
            } => {
                name(app)?;
                number(*element)?;
                text(value)?;
            }
            Call::Type {
                app,
                text: typed,
                element,
                ..
            } => {
                name(app)?;
                element.map_or(Ok(()), number)?;
                text(typed)?;
            }
        }
        Ok(call)
    }

    /// A read of the Messages history, from the `messages` tool's `action` (`read` or `search`)
    /// and fields, all text and checked here.
    pub fn history(action: &str, with: &str, query: &str, limit: &str) -> Result<Call, String> {
        let short = |s: &str, what: &str| match s.chars().count() > MAX_QUERY
            || s.chars().any(char::is_control)
        {
            true => Err(format!(
                "`{what}` may be {MAX_QUERY} characters, on one line"
            )),
            false => Ok(()),
        };
        short(with, "to")?;
        short(query, "query")?;
        match (action, query.is_empty()) {
            ("search", true) => return Err("`search` needs a `query`.".into()),
            ("read", false) => return Err("`query` is for `search`; `read` has none.".into()),
            _ => {}
        }
        let limit = match limit {
            "" => DEFAULT_MESSAGES,
            n => n
                .parse::<u32>()
                .ok()
                .filter(|n| (1..=MAX_MESSAGES).contains(n))
                .ok_or(format!("`limit` must be a number from 1 to {MAX_MESSAGES}"))?,
        };
        Ok(Call::Messages {
            query: (!query.is_empty()).then(|| query.to_owned()),
            with: (!with.is_empty()).then(|| with.to_owned()),
            limit,
        })
    }

    /// What a permission is for: the app, or `apps` for the list.
    pub fn target(&self) -> &str {
        match self {
            Call::Apps => "apps",
            Call::Messages { .. } => "messages",
            Call::Selection => "selection",
            Call::Windows => "windows",
            Call::Screen { .. } => "screen",
            Call::Look { app, .. }
            | Call::Screenshot { app, .. }
            | Call::Press { app, .. }
            | Call::SetValue { app, .. }
            | Call::Type { app, .. } => app,
        }
    }
}

fn name(app: &str) -> Result<(), String> {
    if app.trim().is_empty() || app.chars().count() > MAX_APP || app.chars().any(char::is_control) {
        return Err("`app` must be the name of an app, as `apps` lists it".into());
    }
    Ok(())
}

fn number(element: u32) -> Result<(), String> {
    match element {
        1..=MAX_ELEMENT => Ok(()),
        _ => Err("`element` must be the number of an element that `look` listed".into()),
    }
}

fn text(text: &str) -> Result<(), String> {
    if text.chars().count() > MAX_TEXT {
        return Err(format!("the text may be {MAX_TEXT} characters"));
    }
    Ok(())
}

/// The program file of the process on the other end of `stream`, if it can be told.
pub fn peer_exe(stream: &UnixStream) -> Option<PathBuf> {
    let mut pid: libc::pid_t = 0;
    let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: valid descriptor; the pointers describe `pid` and `len`.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&raw mut pid).cast(),
            &mut len,
        )
    };
    let mut path = [0u8; 4096];
    // SAFETY: the pointer and length describe `path`.
    let n = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) };
    (rc == 0 && n > 0).then(|| PathBuf::from(OsStr::from_bytes(&path[..n as usize])))
}

/// Whether `exe` is the service's program, which the helper obeys.
pub fn is_service(exe: &Path) -> bool {
    exe.file_name() == Some(OsStr::new("appd"))
}

/// Service → helper: one call, with the id its `reply` carries back.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    #[serde(flatten)]
    pub call: Call,
}

/// Helper → service.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Frame {
    /// Which of the helper's macOS permissions are granted: the first frame, then each change.
    Status {
        accessibility: bool,
        screen_recording: bool,
        full_disk_access: bool,
    },
    /// Outcome of the request `id`: `output` (text for the bot) and, for a screenshot, `png`
    /// (base64); or `error`.
    Reply {
        id: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        png: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(args: Value) -> Result<Call, String> {
        Call::parse(args.as_object().unwrap())
    }

    #[test]
    fn a_call_is_what_the_engine_sent_or_an_error() {
        assert_eq!(parse(json!({"action": "apps"})), Ok(Call::Apps));
        assert_eq!(
            parse(json!({"action": "type", "app": "TextEdit", "text": "hi", "element": 3})),
            Ok(Call::Type {
                app: "TextEdit".into(),
                window: None,
                text: "hi".into(),
                element: Some(3)
            })
        );
        for bad in [
            json!({"action": "fly"}),
            json!({"action": "look"}),
            json!({"action": "look", "app": " "}),
            json!({"action": "press", "app": "A", "element": 0}),
            json!({"action": "type", "app": "A", "text": "x".repeat(MAX_TEXT + 1)}),
            // Reading messages is the `messages` tool's, never a `desktop` action.
            json!({"action": "messages", "limit": 5}),
            // Nor Quick Ask's own: the selected text, the screen, the list of windows.
            json!({"action": "selection"}),
            json!({"action": "windows"}),
            json!({"action": "screen"}),
        ] {
            assert!(parse(bad.clone()).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_read_of_messages_is_checked_and_defaults_to_a_few() {
        let call = Call::history;
        let long = "x".repeat(MAX_QUERY + 1);
        assert_eq!(
            call("read", "", "", ""),
            Ok(Call::Messages {
                query: None,
                with: None,
                limit: DEFAULT_MESSAGES
            })
        );
        assert_eq!(
            call("search", "+49100", "lunch", "5"),
            Ok(Call::Messages {
                query: Some("lunch".into()),
                with: Some("+49100".into()),
                limit: 5
            })
        );
        for bad in [
            call("search", "", "", ""),
            call("read", "", "lunch", ""),
            call("read", "", "", "0"),
            call("read", "", "", "51"),
            call("read", "", "", "many"),
            call("read", &long, "", ""),
            call("read", "a\nb", "", ""),
        ] {
            assert!(bad.is_err(), "{bad:?}");
        }
        let line = serde_json::to_string(&Request {
            id: 1,
            call: call("read", "", "", "").unwrap(),
        })
        .unwrap();
        assert_eq!(line, r#"{"id":1,"action":"messages","limit":20}"#);
    }

    #[test]
    fn the_peer_of_a_socket_is_told_by_its_program_file() {
        let (a, _b) = UnixStream::pair().unwrap();
        assert_eq!(peer_exe(&a), std::env::current_exe().ok());
        assert!(is_service(Path::new("/x/target/debug/appd")));
        assert!(!is_service(Path::new("/x/bate-helper")));
        assert!(!is_service(Path::new("/x/appd/other")));
    }

    #[test]
    fn a_request_is_one_flat_frame() {
        let request = Request {
            id: 7,
            call: Call::SetValue {
                app: "A".into(),
                window: Some(4),
                element: 2,
                value: "v".into(),
            },
        };
        let line = serde_json::to_string(&request).unwrap();
        assert_eq!(
            line,
            r#"{"id":7,"action":"set_value","app":"A","window":4,"element":2,"value":"v"}"#
        );
        assert_eq!(serde_json::from_str::<Request>(&line).unwrap(), request);
    }
}

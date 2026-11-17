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
}

impl Call {
    /// Checks what the engine sent; the helper never sees anything else.
    pub fn parse(args: &Map<String, Value>) -> Result<Call, String> {
        let call: Call = serde_json::from_value(Value::Object(args.clone()))
            .map_err(|e| format!("not a desktop action: {e}"))?;
        match &call {
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

    /// What a permission is for: the app, or `apps` for the list.
    pub fn target(&self) -> &str {
        match self {
            Call::Apps => "apps",
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
        ] {
            assert!(parse(bad.clone()).is_err(), "{bad}");
        }
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

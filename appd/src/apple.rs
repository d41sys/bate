//! Runs a bot's action in one of the user's Apple apps (see `proto::apple`). The service does
//! this, not the engine: a limited bot's engine may not send Apple events or read the user's
//! files, and the macOS prompts for Automation, Contacts and the rest then name the service.
//!
//! Every script is fixed text. What the model wrote reaches it only as `argv`, never as part
//! of the source, so no text can turn into AppleScript.

use proto::apple::App;
use proto::permissions::Kind;
use serde_json::{Map, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Most output returned to the model, in bytes.
const OUTPUT_CAP: usize = 64 << 10;
/// An app may sit on a macOS permission prompt for a while.
const TIMEOUT: Duration = Duration::from_secs(120);

/// The program that runs scripts. Debug builds take `BATE_OSASCRIPT`, so a test records calls
/// instead of touching an app.
pub struct Tools {
    pub osascript: PathBuf,
}

impl Tools {
    pub fn system() -> Tools {
        let env = std::env::var_os("BATE_OSASCRIPT").filter(|_| cfg!(debug_assertions));
        Tools {
            osascript: env
                .map(PathBuf::from)
                .unwrap_or("/usr/bin/osascript".into()),
        }
    }
}

/// The source of the app's script: the shared helpers, then the app's own.
fn source(tool: &str) -> Option<String> {
    let own = match tool {
        "messages" => return Some(include_str!("apple/messages.applescript").into()),
        "mail" => include_str!("apple/mail.applescript"),
        "notes" => include_str!("apple/notes.applescript"),
        "reminders" => include_str!("apple/reminders.applescript"),
        "calendar" => include_str!("apple/calendar.applescript"),
        "contacts" => include_str!("apple/contacts.applescript"),
        _ => return None,
    };
    Some(include_str!("apple/text.applescript").to_owned() + own)
}

/// A read of the Messages history, which the helper app does (it holds Full Disk Access; this
/// service must not): `None` for any other call, which `run` does.
pub fn history(
    tool: &str,
    args: &Map<String, Value>,
) -> Option<Result<proto::desktop::Call, String>> {
    let call = match proto::apple::app(tool)?.parse(args) {
        Ok(call) if tool == "messages" && call.kind == Kind::AppleRead => call,
        Ok(_) => return None,
        Err(e) => return Some(Err(e)),
    };
    let [action, to, _text, query, limit] = &call.argv[..] else {
        unreachable!("messages has four fields")
    };
    Some(proto::desktop::Call::history(action, to, query, limit))
}

/// `Err` is what the model is told.
pub fn run(tools: &Tools, tool: &str, args: &Map<String, Value>) -> Result<String, String> {
    let app: &App = proto::apple::app(tool).ok_or(format!("there is no app `{tool}`"))?;
    let call = app.parse(args)?;
    let argv: Vec<_> = call.argv.iter().map(String::as_str).collect();
    let source = source(tool).expect("a tool in the table has a script");
    // Messages' script takes just the recipient and the text, not the action.
    let argv = if tool == "messages" {
        if argv[1].is_empty() || argv[2].is_empty() {
            return Err("`to` and `text` are both needed to send.".into());
        }
        &argv[1..3]
    } else {
        &argv[..]
    };
    osascript(&tools.osascript, &source, argv)
}

/// The command that runs `source` with `argv`. `--` keeps an argument that starts with `-` an
/// argument.
fn command(program: &Path, source: &str, argv: &[&str]) -> Command {
    let mut cmd = Command::new(program);
    cmd.args(["-l", "AppleScript", "-e", source, "--"])
        .args(argv);
    cmd
}

fn osascript(program: &Path, source: &str, argv: &[&str]) -> Result<String, String> {
    let mut cmd = command(program, source, argv);
    let failed = |e: std::io::Error| format!("cannot run {}: {e}", program.display());
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(failed)?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut out = Vec::new();
            if let Some(pipe) = pipe {
                let _ = pipe.take(OUTPUT_CAP as u64).read_to_end(&mut out);
            }
            String::from_utf8_lossy(&out).into_owned()
        })
    };
    let out = drain(child.stdout.take().map(|p| Box::new(p) as _));
    let err = drain(child.stderr.take().map(|p| Box::new(p) as _));
    let started = Instant::now();
    let status = loop {
        match child.try_wait().map_err(failed)? {
            Some(status) => break status,
            None if started.elapsed() >= TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "the app did not answer within {} s",
                    TIMEOUT.as_secs()
                ));
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let (out, err) = (
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default(),
    );
    if status.success() {
        Ok(out.trim_end().to_owned())
    } else {
        Err(format!("{}{}", err.trim_end(), hint(&err)))
    }
}

/// What the user can do about the errors macOS gives for a missing permission.
fn hint(error: &str) -> &'static str {
    if error.contains("-1743") || error.contains("-10004") {
        "\nBate may not control this app yet: the user allows it in System Settings > Privacy & Security > Automation (or Contacts, Calendars, Reminders)."
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    /// What text could do to a script that pasted it in: close a string, end a statement,
    /// start a command, look like an option.
    const HOSTILE: &str =
        "\"; do shell script \"touch /tmp/pwned\"; (* -e 'x' \\ \" end tell\n$(id) `id` --";

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("appd-apple-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// A stand-in `osascript`: each call becomes a line of `calls`, its arguments in hex (`x`
    /// first, so an empty one shows), and it says ok.
    fn recorder(dir: &Path) -> PathBuf {
        let path = dir.join("osascript");
        let log = dir.join("calls");
        let script = format!(
            "#!/bin/sh\nfor a in \"$@\"; do printf 'x%s ' \"$(printf '%s' \"$a\" | od -An -v -tx1 | tr -d ' \\n')\" >> {0:?}; done\necho >> {0:?}\necho ok\n",
            log
        );
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn calls(dir: &Path) -> Vec<Vec<String>> {
        let unhex = |h: &str| {
            let bytes: Vec<u8> = (0..h.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
                .collect();
            String::from_utf8(bytes).unwrap()
        };
        let raw = fs::read_to_string(dir.join("calls")).unwrap_or_default();
        raw.lines()
            .map(|l| {
                l.split(' ')
                    .filter(|a| !a.is_empty())
                    .map(|a| unhex(&a[1..]))
                    .collect()
            })
            .collect()
    }

    fn args(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn what_the_model_wrote_reaches_a_script_only_as_an_argument() {
        let dir = temp("argv");
        let tools = Tools {
            osascript: recorder(&dir),
        };
        for (tool, action, field) in [
            ("mail", "send", "body"),
            ("notes", "create", "body"),
            ("reminders", "create", "body"),
            ("calendar", "create", "notes"),
            ("contacts", "create", "note"),
            ("messages", "send", "text"),
        ] {
            let mut a = args(
                json!({"action": action, field: HOSTILE, "to": "x@y.z", "start": "2026-03-01T09:00:00"}),
            );
            a.insert("text".into(), json!(HOSTILE));
            run(&tools, tool, &a).unwrap();
        }
        let calls = calls(&dir);
        assert_eq!(calls.len(), 6);
        for call in &calls {
            // `-l AppleScript -e <script> -- <arguments>`: the script is the shipped one, whole.
            assert_eq!(call[..2], ["-l", "AppleScript"]);
            assert_eq!(call[2], "-e");
            assert_eq!(call[4], "--");
            assert!(
                !call[3].contains("pwned") && !call[3].contains(HOSTILE),
                "{}",
                call[3]
            );
            assert!(call[5..].iter().any(|a| a == HOSTILE), "{call:?}");
        }
        assert_eq!(calls[5][3], include_str!("apple/messages.applescript"));
        assert_eq!(calls[5][5..], ["x@y.z", HOSTILE]);
        assert_eq!(calls[0][3], source("mail").unwrap());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_real_osascript_hands_hostile_text_back_unchanged_and_runs_none_of_it() {
        // A script that does nothing but return its arguments: no app is touched.
        let echo = "on run argv\nreturn (item 1 of argv) & \"|\" & (item 2 of argv) & \"|\" & (count of argv)\nend run";
        let out = osascript(Path::new("/usr/bin/osascript"), echo, &[HOSTILE, "-x"]).unwrap();
        assert_eq!(out, format!("{HOSTILE}|-x|2"));
        assert!(!Path::new("/tmp/pwned").exists());
    }

    #[test]
    fn a_failing_script_returns_its_error_and_a_missing_one_says_so() {
        let dir = temp("fail");
        let fail = dir.join("osascript");
        fs::write(&fail, "#!/bin/sh\necho 'execution error: Not authorized to send Apple events to Notes. (-1743)' >&2\nexit 1\n").unwrap();
        fs::set_permissions(&fail, fs::Permissions::from_mode(0o755)).unwrap();
        let e = osascript(&fail, "x", &[]).unwrap_err();
        assert!(e.contains("-1743") && e.contains("Automation"), "{e}");
        let e = osascript(&dir.join("absent"), "x", &[]).unwrap_err();
        assert!(e.contains("cannot run"), "{e}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_bad_call_is_refused_before_any_script_runs() {
        let dir = temp("bad");
        let tools = Tools {
            osascript: recorder(&dir),
        };
        assert!(run(&tools, "notes", &args(json!({"action": "wipe"}))).is_err());
        assert!(run(&tools, "fridge", &args(json!({"action": "list"}))).is_err());
        assert!(
            run(
                &tools,
                "messages",
                &args(json!({"action": "send", "to": "x"}))
            )
            .is_err()
        );
        assert!(calls(&dir).is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }
}

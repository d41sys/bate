//! Runs a bot's action in one of the user's Apple apps (see `proto::apple`). The service does
//! this, not the engine: a limited bot's engine may not send Apple events or read the user's
//! files, and the macOS prompts for Automation, Contacts and the rest then name the service.
//!
//! Every script is fixed text. What the model wrote reaches it only as `argv`, never as part
//! of the source, so no text can turn into AppleScript.

use proto::apple::App;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Map, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Most output returned to the model, in bytes.
const OUTPUT_CAP: usize = 64 << 10;
/// An app may sit on a macOS permission prompt for a while.
const TIMEOUT: Duration = Duration::from_secs(120);
/// Most messages one `read` looks through when it filters by text.
const SCAN: i64 = 2000;

/// The program that runs scripts and the file that holds Messages' history. Debug builds take
/// `BATE_OSASCRIPT` and `BATE_MESSAGES_DB`, so a test records calls instead of touching an app.
pub struct Tools {
    pub osascript: PathBuf,
    pub chat_db: PathBuf,
}

impl Tools {
    pub fn system() -> Tools {
        let env = |name: &str| {
            std::env::var_os(name)
                .filter(|_| cfg!(debug_assertions))
                .map(PathBuf::from)
        };
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        Tools {
            osascript: env("BATE_OSASCRIPT").unwrap_or("/usr/bin/osascript".into()),
            chat_db: env("BATE_MESSAGES_DB").unwrap_or(home.join("Library/Messages/chat.db")),
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

/// `Err` is what the model is told.
pub fn run(tools: &Tools, tool: &str, args: &Map<String, Value>) -> Result<String, String> {
    let app: &App = proto::apple::app(tool).ok_or(format!("there is no app `{tool}`"))?;
    let call = app.parse(args)?;
    let field = |name: &str| args.get(name).and_then(Value::as_str).unwrap_or("");
    match (tool, call.action) {
        ("messages", "read") => read_messages(
            &tools.chat_db,
            field("chat"),
            field("query"),
            field("limit"),
        ),
        ("messages", _) => {
            let (to, text) = (field("to"), field("text"));
            if to.is_empty() || text.is_empty() {
                return Err("`to` and `text` are both needed to send.".into());
            }
            osascript(&tools.osascript, &source(tool).expect("known"), &[to, text])
        }
        _ => {
            let argv: Vec<_> = call.argv.iter().map(String::as_str).collect();
            osascript(&tools.osascript, &source(tool).expect("known"), &argv)
        }
    }
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

/// Messages' history is a SQLite file, since the app's scripting cannot read old messages.
/// Reading it needs Full Disk Access for the service.
fn read_messages(db: &Path, chat: &str, query: &str, limit: &str) -> Result<String, String> {
    let limit = limit.parse::<i64>().unwrap_or(20).clamp(1, 100);
    let denied = |e: rusqlite::Error| {
        format!(
            "cannot read {}: {e}. The user must give the service Full Disk Access ({}) in System Settings > Privacy & Security.",
            db.display(),
            std::env::current_exe().unwrap_or_default().display()
        )
    };
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(denied)?;
    let mut stmt = conn
        .prepare(
            "SELECT m.date, m.is_from_me, COALESCE(h.id, ''), COALESCE(c.chat_identifier, ''), m.text, m.attributedBody
             FROM message m
             LEFT JOIN handle h ON h.ROWID = m.handle_id
             LEFT JOIN chat_message_join j ON j.message_id = m.ROWID
             LEFT JOIN chat c ON c.ROWID = j.chat_id
             WHERE ?1 = '' OR c.chat_identifier = ?1
             ORDER BY m.date DESC LIMIT ?2",
        )
        .map_err(denied)?;
    // shortcut: with `query`, only the newest 2000 messages are searched, because text is
    // matched here (newer macOS keeps it in a blob); upgrade when that is too few.
    let rows = stmt
        .query_map((chat, if query.is_empty() { limit } else { SCAN }), |r| {
            let text: Option<String> = r.get(4)?;
            let blob: Option<Vec<u8>> = r.get(5)?;
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, bool>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                text.filter(|t| !t.is_empty())
                    .or_else(|| blob.as_deref().and_then(body_text)),
            ))
        })
        .map_err(denied)?;
    let query = query.to_lowercase();
    let mut lines = Vec::new();
    for row in rows {
        let (date, mine, handle, chat, text) = row.map_err(denied)?;
        let text = text.unwrap_or_default().replace('\u{fffc}', "[attachment]");
        if text.to_lowercase().contains(&query) {
            let who = if mine { "me" } else { &handle };
            let text = text.replace(['\n', '\r', '\t'], " ");
            lines.push(format!("{}\t{chat}\t{who}\t{text}\n", when(date)));
            if lines.len() as i64 == limit {
                break;
            }
        }
    }
    // Oldest first, the way a conversation reads.
    lines.reverse();
    Ok(lines.concat())
}

/// Messages stores a date as nanoseconds since 2001 (seconds in old versions).
fn when(date: i64) -> String {
    let secs = if date.abs() > 1_000_000_000_000 {
        date / 1_000_000_000
    } else {
        date
    };
    jiff::Timestamp::from_second(secs + 978_307_200)
        .map(|t| {
            t.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%Y-%m-%dT%H:%M:%S")
                .to_string()
        })
        .unwrap_or_default()
}

/// The text of a message that only has `attributedBody`, an archived NSAttributedString: the
/// string follows the class name `NSString`, a few bytes of type information and a length.
fn body_text(blob: &[u8]) -> Option<String> {
    let at = blob.windows(8).position(|w| w == b"NSString")? + 8;
    let rest = blob.get(at..)?;
    let rest = &rest[rest.iter().take(8).position(|b| *b == b'+')? + 1..];
    let (len, rest) = match *rest.first()? {
        0x81 => (
            u16::from_le_bytes(rest.get(1..3)?.try_into().ok()?) as usize,
            rest.get(3..)?,
        ),
        0x82 => (
            u32::from_le_bytes(rest.get(1..5)?.try_into().ok()?) as usize,
            rest.get(5..)?,
        ),
        n => (n as usize, &rest[1..]),
    };
    String::from_utf8(rest.get(..len)?.to_vec()).ok()
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
            chat_db: dir.join("none"),
        };
        for (tool, action, field) in [
            ("mail", "send", "body"),
            ("notes", "create", "title"),
            ("reminders", "create", "title"),
            ("calendar", "create", "title"),
            ("contacts", "create", "first"),
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
            chat_db: dir.join("none"),
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

    #[test]
    fn messages_are_read_from_the_history_file_text_and_archived_text_alike() {
        let dir = temp("chatdb");
        let db = dir.join("chat.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE message(ROWID INTEGER PRIMARY KEY, date INTEGER, is_from_me INTEGER, handle_id INTEGER, text TEXT, attributedBody BLOB);
             CREATE TABLE handle(ROWID INTEGER PRIMARY KEY, id TEXT);
             CREATE TABLE chat(ROWID INTEGER PRIMARY KEY, chat_identifier TEXT);
             CREATE TABLE chat_message_join(chat_id INTEGER, message_id INTEGER);
             INSERT INTO handle VALUES (1, '+4915100000'), (2, 'b@example.com');
             INSERT INTO chat VALUES (1, '+4915100000'), (2, 'b@example.com');
             INSERT INTO message(ROWID, date, is_from_me, handle_id, text) VALUES (1, 800000000000000000, 0, 1, 'hello there');
             INSERT INTO message(ROWID, date, is_from_me, handle_id, text) VALUES (2, 800000060000000000, 1, 1, 'hi back');
             INSERT INTO message(ROWID, date, is_from_me, handle_id, text) VALUES (3, 800000120000000000, 0, 2, 'other chat');
             INSERT INTO chat_message_join VALUES (1, 1), (1, 2), (2, 3);",
        )
        .unwrap();
        // The text of a newer message is only inside its archived string.
        let blob = [&b"\x04\x0bstreamtyped\x81\xe8\x03\x84\x01@\x84\x84\x84\x12NSAttributedString\x00\x84\x84\x08NSObject\x00\x85\x92\x84\x84\x84\x08NSString\x01\x94\x84\x01+\x0barchived hi"[..], b"\x86\x84"].concat();
        conn.execute(
            "INSERT INTO message(ROWID, date, is_from_me, handle_id, text, attributedBody) VALUES (4, 800000180000000000, 0, 1, NULL, ?1)",
            [blob],
        )
        .unwrap();
        conn.execute("INSERT INTO chat_message_join VALUES (1, 4)", [])
            .unwrap();
        drop(conn);

        let all = read_messages(&db, "+4915100000", "", "").unwrap();
        let texts: Vec<_> = all
            .lines()
            .map(|l| l.rsplit('\t').next().unwrap())
            .collect();
        assert_eq!(texts, ["hello there", "hi back", "archived hi"]);
        assert!(all.lines().nth(1).unwrap().contains("\tme\t"), "{all}");
        let found = read_messages(&db, "", "OTHER", "").unwrap();
        assert_eq!(found.lines().count(), 1);
        assert!(
            found.contains("b@example.com\tb@example.com\tother chat"),
            "{found}"
        );
        assert_eq!(read_messages(&db, "", "", "1").unwrap().lines().count(), 1);
        // A history the service may not open says what to do.
        let e = read_messages(&dir.join("absent.db"), "", "", "").unwrap_err();
        assert!(e.contains("Full Disk Access"), "{e}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn archived_text_of_any_length_is_cut_out_and_junk_is_not_text() {
        let long = "x".repeat(300);
        let mut blob = b"..NSString\x01\x94\x84\x01+\x81".to_vec();
        blob.extend((long.len() as u16).to_le_bytes());
        blob.extend(long.as_bytes());
        assert_eq!(body_text(&blob).unwrap(), long);
        assert_eq!(body_text(b"nothing here"), None);
        assert_eq!(body_text(b"NSString+\x09short"), None);
    }
}

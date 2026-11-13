//! The engine's tools: `read`, `write`, `edit`, `run`, `search`.

use proto::permissions::Kind;
use serde_json::{Map, Value, json};
use std::fs;
use std::io::{self, Read};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// Most output a `run` or `search` returns; the rest is drained and dropped.
pub const OUTPUT_CAP: usize = 64 << 10;
/// Most text one `read` returns.
const READ_CAP: usize = 256 << 10;
const READ_LINES: usize = 2000;

/// Shared by the turn and the operation loop: `abort` stops the turn and kills its command.
#[derive(Default)]
pub struct Control {
    aborted: AtomicBool,
    /// Process group of the running command.
    group: Mutex<Option<i32>>,
    /// The question the turn waits on, so `answer` can refuse any other.
    pub question: Mutex<Option<String>>,
}

impl Control {
    pub fn abort(&self) {
        self.aborted.store(true, Ordering::SeqCst);
        if let Some(group) = *lock(&self.group) {
            kill_group(group);
        }
    }

    pub fn aborted(&self) -> bool {
        self.aborted.load(Ordering::SeqCst)
    }

    pub fn aborted_flag(&self) -> &AtomicBool {
        &self.aborted
    }
}

pub fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn kill_group(group: i32) {
    // SAFETY: plain syscall; a group that is already gone only returns ESRCH.
    unsafe { libc::killpg(group, libc::SIGKILL) };
}

/// The tool list in the OpenAI function-calling shape.
pub fn definitions() -> Value {
    let tool = |name: &str, description: &str, properties: Value, required: &[&str]| {
        json!({"type": "function", "function": {"name": name, "description": description,
            "parameters": {"type": "object", "properties": properties, "required": required}}})
    };
    let path = json!({"type": "string", "description": "File path, relative to the working folder or absolute"});
    json!([
        tool(
            "read",
            "Read a text file. Lines come numbered.",
            json!({"path": path,
            "offset": {"type": "integer", "description": "First line, from 1"},
            "limit": {"type": "integer", "description": "Most lines to return (default 2000)"}}),
            &["path"]
        ),
        tool(
            "write",
            "Create or overwrite a file.",
            json!({"path": path, "content": {"type": "string"}}),
            &["path", "content"]
        ),
        tool(
            "edit",
            "Replace one exact piece of text in a file. `old` must appear exactly once.",
            json!({"path": path, "old": {"type": "string"}, "new": {"type": "string"}}),
            &["path", "old", "new"]
        ),
        tool(
            "run",
            "Run a shell command in the working folder. Output and errors come back together.",
            json!({"command": {"type": "string"}, "timeoutSecs": {"type": "integer", "description": "Default 120, at most 600"}}),
            &["command"]
        ),
        tool(
            "search",
            "Search file contents with an extended regular expression (grep -E).",
            json!({"pattern": {"type": "string"}, "path": {"type": "string", "description": "Folder or file (default: the working folder)"}}),
            &["pattern"]
        ),
    ])
}

/// What a call would do, for the permission question: the kind of action, its exact target
/// (the path or the command) and one line for the user. `None`: not one of these tools.
pub fn action(tool: &str, args: &Map<String, Value>) -> Option<(Kind, String, String)> {
    let arg = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("?");
    let path = arg("path");
    Some(match tool {
        "read" => (Kind::Read, path.to_owned(), format!("Read {path}")),
        "search" => {
            let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
            let title = format!("Search {path} for {}", arg("pattern"));
            (Kind::Read, path.to_owned(), title)
        }
        "write" => (Kind::Write, path.to_owned(), format!("Write {path}")),
        "edit" => (Kind::Write, path.to_owned(), format!("Edit {path}")),
        "run" => {
            let command = arg("command");
            (Kind::Run, command.to_owned(), format!("Run: {command}"))
        }
        _ => return None,
    })
}

/// Runs one tool call in `dir`. `Err` is a result the model sees as failed.
pub fn run(
    tool: &str,
    args: &Map<String, Value>,
    dir: &Path,
    control: &Control,
) -> Result<String, String> {
    let text = |k: &str| {
        args.get(k)
            .and_then(Value::as_str)
            .ok_or(format!("`{k}` must be a string"))
    };
    let number = |k: &str| args.get(k).and_then(Value::as_u64);
    let path = |k: &str| text(k).map(|p| dir.join(p)); // an absolute `p` replaces `dir`
    match tool {
        "read" => {
            let p = path("path")?;
            let bytes = fs::read(&p).map_err(at(&p))?;
            let content = String::from_utf8_lossy(&bytes);
            let first = number("offset").unwrap_or(1).max(1) as usize;
            let limit = number("limit").map_or(READ_LINES, |n| n as usize);
            let mut out = String::new();
            for (shown, (i, line)) in content.lines().enumerate().skip(first - 1).enumerate() {
                if shown == limit || (shown > 0 && out.len() + line.len() > READ_CAP) {
                    let left = content.lines().count() - i;
                    out.push_str(&format!(
                        "[{left} more lines; read on with offset {}]",
                        i + 1
                    ));
                    return Ok(out);
                }
                // A first line over the cap is cut, so `offset` always moves on.
                let line = &line[..line.floor_char_boundary(READ_CAP)];
                out.push_str(&format!("{:>6}\t{line}\n", i + 1));
            }
            Ok(out)
        }
        "write" => {
            let p = path("path")?;
            let content = text("content")?;
            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent).map_err(at(parent))?;
            }
            fs::write(&p, content).map_err(at(&p))?;
            Ok(format!("wrote {} bytes to {}", content.len(), p.display()))
        }
        "edit" => {
            let p = path("path")?;
            let (old, new) = (text("old")?, text("new")?);
            if old.is_empty() {
                return Err("`old` is empty".into());
            }
            let content = fs::read_to_string(&p).map_err(at(&p))?;
            match content.matches(old).count() {
                0 => Err(format!("the text to replace is not in {}", p.display())),
                1 => {
                    fs::write(&p, content.replacen(old, new, 1)).map_err(at(&p))?;
                    Ok(format!("edited {}", p.display()))
                }
                n => Err(format!(
                    "the text to replace appears {n} times in {}; include more of the text around it",
                    p.display()
                )),
            }
        }
        "run" => {
            let secs = number("timeoutSecs").unwrap_or(120).clamp(1, 600);
            let mut cmd = Command::new("/bin/sh");
            cmd.arg("-c")
                .arg(text("command")?)
                .current_dir(dir)
                .env_remove(crate::model::KEY);
            match capture(cmd, Duration::from_secs(secs), control)? {
                (out, Some(0)) => Ok(out),
                (out, Some(code)) => Err(format!("{out}\nexit code {code}")),
                (out, None) => Err(format!("{out}\nkilled by a signal")),
            }
        }
        "search" => {
            let mut cmd = Command::new("grep");
            cmd.args(["-rnEI", "--exclude-dir=.git", "-e", text("pattern")?, "--"])
                .arg(args.get("path").and_then(Value::as_str).unwrap_or("."))
                .current_dir(dir);
            match capture(cmd, Duration::from_secs(30), control)? {
                (out, Some(0)) => Ok(out),
                (out, Some(1)) if out.is_empty() => Ok("no matches".into()),
                (out, _) => Err(out),
            }
        }
        _ => Err(format!("there is no tool `{tool}`")),
    }
}

fn at(p: &Path) -> impl Fn(io::Error) -> String + '_ {
    move |e| format!("{}: {e}", p.display())
}

/// Runs `cmd` in its own process group with stdout and stderr merged, capped at `OUTPUT_CAP`.
/// Returns the output and the exit code (`None`: killed by a signal).
fn capture(
    mut cmd: Command,
    timeout: Duration,
    control: &Control,
) -> Result<(String, Option<i32>), String> {
    let failed = |e: io::Error| format!("cannot run the command: {e}");
    let (mut reader, writer) = io::pipe().map_err(failed)?;
    cmd.stdin(Stdio::null())
        .stdout(writer.try_clone().map_err(failed)?)
        .stderr(writer)
        .process_group(0);
    let mut child = cmd.spawn().map_err(failed)?;
    drop(cmd); // closes our copies of the pipe's write end, so the reader sees end of file
    let group = child.id() as i32;
    *lock(&control.group) = Some(group);
    if control.aborted() {
        kill_group(group);
    }
    // Output collects in `captured`; `done` fires at end of file. A process that escaped the group
    // (its own session, a daemon) can hold the pipe open forever, so the turn never waits on the thread.
    let captured = Arc::new(Mutex::new((Vec::new(), false)));
    let (done, finished) = mpsc::channel();
    let sink = captured.clone();
    std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        while let Ok(n @ 1..) = reader.read(&mut chunk) {
            let (out, cut) = &mut *lock(&sink);
            let room = OUTPUT_CAP - out.len();
            out.extend_from_slice(&chunk[..n.min(room)]);
            *cut |= n > room;
        }
        let _ = done.send(());
    });
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if !timed_out && started.elapsed() >= timeout => {
                timed_out = true;
                kill_group(group);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => break Err(e),
        }
    };
    // shortcut: background processes the command left behind are killed too, so the pipe closes;
    // a tool that starts a long-lived server needs its own design.
    kill_group(group);
    *lock(&control.group) = None;
    let _ = finished.recv_timeout(Duration::from_secs(1));
    let (out, cut) = std::mem::take(&mut *lock(&captured));
    let status = status.map_err(failed)?;
    if control.aborted() {
        return Err("Aborted by the user".into());
    }
    if timed_out {
        return Err(format!(
            "the command timed out after {} s",
            timeout.as_secs()
        ));
    }
    let mut out = String::from_utf8_lossy(&out).into_owned();
    if cut {
        out.push_str("\n[output cut at 64 KiB]");
    }
    Ok((out, status.code()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("agentcore-tools-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn call(name: &str, args: Value, dir: &Path) -> Result<String, String> {
        run(name, args.as_object().unwrap(), dir, &Control::default())
    }

    #[test]
    fn edit_replaces_one_exact_unique_piece_and_refuses_missing_or_repeated_text() {
        let d = dir("edit");
        fs::write(d.join("f.txt"), "one two two").unwrap();
        let missing = call(
            "edit",
            json!({"path": "f.txt", "old": "three", "new": "x"}),
            &d,
        );
        assert!(missing.unwrap_err().contains("not in"));
        let repeated = call(
            "edit",
            json!({"path": "f.txt", "old": "two", "new": "x"}),
            &d,
        );
        assert!(repeated.unwrap_err().contains("2 times"));
        call(
            "edit",
            json!({"path": "f.txt", "old": "one", "new": "1"}),
            &d,
        )
        .unwrap();
        assert_eq!(fs::read_to_string(d.join("f.txt")).unwrap(), "1 two two");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn write_creates_folders_and_read_numbers_lines_from_an_offset() {
        let d = dir("rw");
        call(
            "write",
            json!({"path": "a/b.txt", "content": "x\ny\nz\n"}),
            &d,
        )
        .unwrap();
        let out = call(
            "read",
            json!({"path": "a/b.txt", "offset": 2, "limit": 1}),
            &d,
        )
        .unwrap();
        assert_eq!(out, "     2\ty\n[1 more lines; read on with offset 3]");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn run_reports_output_exit_code_timeout_and_caps_output() {
        let d = dir("run");
        assert_eq!(
            call("run", json!({"command": "echo hi; echo err >&2"}), &d).unwrap(),
            "hi\nerr\n"
        );
        assert!(
            call("run", json!({"command": "exit 3"}), &d)
                .unwrap_err()
                .contains("exit code 3")
        );
        let started = std::time::Instant::now();
        let slow = call("run", json!({"command": "sleep 5", "timeoutSecs": 1}), &d);
        assert!(slow.unwrap_err().contains("timed out after 1 s"));
        assert!(started.elapsed() < Duration::from_secs(4));
        let big = call("run", json!({"command": "yes | head -c 200000"}), &d).unwrap();
        assert!(big.len() < OUTPUT_CAP + 100 && big.ends_with("[output cut at 64 KiB]"));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_process_that_escaped_the_group_does_not_hold_the_call() {
        let d = dir("escaped");
        let started = std::time::Instant::now();
        // `setpgrp` leaves our process group, so the final kill misses it and it keeps the pipe open.
        let out = call(
            "run",
            json!({"command": "perl -e 'setpgrp(0,0); sleep 20' & echo started"}),
            &d,
        );
        assert_eq!(out.unwrap(), "started\n");
        assert!(started.elapsed() < Duration::from_secs(5));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn search_finds_matches_and_says_when_there_are_none() {
        let d = dir("search");
        fs::write(d.join("a.rs"), "fn main() {}\nfn helper() {}\n").unwrap();
        let out = call("search", json!({"pattern": "fn h[a-z]+"}), &d).unwrap();
        assert!(out.contains("a.rs:2:fn helper() {}"), "{out}");
        assert_eq!(
            call("search", json!({"pattern": "nothing_here"}), &d).unwrap(),
            "no matches"
        );
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn each_tool_names_its_kind_of_action_and_its_exact_target() {
        let of = |tool: &str, args: Value| {
            let (kind, target, _) = action(tool, args.as_object().unwrap()).unwrap();
            (kind, target)
        };
        assert_eq!(
            of("read", json!({"path": "a.txt"})),
            (Kind::Read, "a.txt".into())
        );
        assert_eq!(
            of("search", json!({"pattern": "x"})),
            (Kind::Read, ".".into())
        );
        assert_eq!(
            of("search", json!({"pattern": "x", "path": "src"})),
            (Kind::Read, "src".into())
        );
        assert_eq!(
            of("write", json!({"path": "a.txt", "content": ""})),
            (Kind::Write, "a.txt".into())
        );
        assert_eq!(
            of("edit", json!({"path": "a.txt"})),
            (Kind::Write, "a.txt".into())
        );
        assert_eq!(
            of("run", json!({"command": "ls -la"})),
            (Kind::Run, "ls -la".into())
        );
        assert!(action("fly", &Map::new()).is_none());
    }
}

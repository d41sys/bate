//! What a bot changed in its work folder, as unified diffs, for the window's Changes view (see
//! "Changes" in `PROTOCOL.md`). Read-only. In a Git folder it is the folder's uncommitted work as
//! the system's `git` shows it; in any other folder it is the copies the service keeps of what the
//! bot's own `write` and `edit` replaced (`undo`), compared with the files as they are now.
//!
//! The folder is the bot's, and the bot may write anything in it, `.git` included. So `git` runs
//! with no configuration of the user's or the system's, with the repository's helpers that run
//! programs (a filter, a text converter, an external diff, the file watcher) off, and is not run at
//! all in a repository that defines a filter. Every call is bounded in time and in output, and the
//! work runs on a thread of its own (`Core::reply_later`).

use crate::sys::{self, Dir};
use crate::{login_path, project_server};
use serde_json::{Value, json};
use std::fs::File;
use std::io::{self, Read};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;
use std::{fs, thread};

type Refusal = (&'static str, String);

/// Most files in a list.
const MAX_FILES: usize = 200;
/// Most bytes of one file, and of its diff, that are shown: more is named, not shown.
const MAX_SHOWN: usize = 512 << 10;
/// Most bytes read of what `git` lists (`status`, `config`).
const MAX_LIST: usize = 256 << 10;
/// How long one `git` call may run.
const WAIT: Duration = Duration::from_secs(10);
/// What Git compares with in a repository that has no commit yet.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// `path` as the folder's own relative path: parts joined by `/`, none empty and none starting
/// with a dot (so no `..`, no hidden file such as the engine's own, as the file viewer has it).
fn plain(path: &str) -> Option<&str> {
    path.split('/')
        .all(|part| !part.is_empty() && !part.starts_with('.'))
        .then_some(path)
}

/// The full path of `path` in `root`, if it is a plain relative one.
pub fn full(root: &Path, path: &str) -> Result<String, Refusal> {
    match plain(path) {
        Some(path) => Ok(root.join(path).to_string_lossy().into_owned()),
        None => Err((
            "bad_request",
            "that is not a path of a file in the bot's folder".into(),
        )),
    }
}

fn failed(e: impl std::fmt::Display) -> Refusal {
    ("changes_failed", e.to_string())
}

/// The files `copies` names (full paths; `true`: the first change made it) as a list, or those
/// of the Git folder `root`. Answers `{source, git, files: [{path, status}], more}`: `git` says
/// whether Git is installed.
pub fn list(root: &Path, copies: Vec<(String, bool)>) -> Result<Value, Refusal> {
    let git = git_path();
    let (source, files, more) = match &git {
        Some(git) if is_repo(root) => {
            let (files, more) = git_list(git, root)?;
            ("git", files, more)
        }
        _ => {
            let mut files = Vec::new();
            for (path, made) in copies {
                let rel = Path::new(&path)
                    .strip_prefix(root)
                    .ok()
                    .and_then(Path::to_str)
                    .and_then(plain);
                if let Some(rel) = rel {
                    let status = if made { "added" } else { "modified" };
                    files.push(json!({"path": rel, "status": status}));
                }
            }
            let more = files.len() > MAX_FILES;
            files.truncate(MAX_FILES);
            ("copies", files, more)
        }
    };
    Ok(json!({"source": source, "git": git.is_some(), "files": files, "more": more}))
}

/// What one file of the list is: `{kind, diff}`. `kind` is `text` (`diff` holds the hunks),
/// `binary` or `large` (named, not shown) or `none` (no difference, or not a plain file).
/// `copy` is what the file was before the bot's first change, when one is kept (see
/// `Store::first_copy`).
pub fn read(root: &Path, path: &str, copy: Option<Option<Vec<u8>>>) -> Result<Value, Refusal> {
    let path = plain(path).ok_or_else(|| {
        (
            "bad_request",
            "that is not a path of a file in the bot's folder".to_owned(),
        )
    })?;
    let git = git_path();
    match &git {
        Some(git) if is_repo(root) => git_read(git, root, path),
        _ => match copy {
            Some(earlier) => copy_read(git.as_deref(), root, path, earlier),
            None => Err((
                "no_such_change",
                "Bate keeps no earlier version of that file".into(),
            )),
        },
    }
}

fn text(diff: String) -> Value {
    json!({"kind": "text", "diff": diff})
}

fn named(kind: &str) -> Value {
    json!({"kind": kind, "diff": ""})
}

/// The diff Git printed: its own words for a binary file are told apart here.
fn shown(diff: &[u8]) -> Value {
    let diff = String::from_utf8_lossy(diff).into_owned();
    if diff.lines().any(|l| l.starts_with("Binary files ")) {
        named("binary")
    } else if diff.is_empty() {
        named("none")
    } else {
        text(diff)
    }
}

/// A whole file as one hunk: every line added (`+`) or removed (`-`).
fn whole(bytes: &[u8], sign: char) -> Value {
    if bytes.contains(&0) {
        return named("binary");
    }
    let body = String::from_utf8_lossy(bytes);
    let n = body.lines().count();
    let (old, new) = if sign == '+' { (0, n) } else { (n, 0) };
    let mut diff = format!(
        "@@ -{},{old} +{},{new} @@\n",
        usize::from(old > 0),
        usize::from(new > 0)
    );
    for line in body.lines() {
        diff.push(sign);
        diff.push_str(line);
        diff.push('\n');
    }
    text(diff)
}

/// A file of the folder as it is now.
enum Now {
    /// There is none, or it is not a plain file (a link, a pipe), or it cannot be read.
    Missing,
    Large,
    Bytes(Vec<u8>),
}

/// Reads `path` (plain, relative) in `root` without following a link at any step.
fn now(root: &Path, path: &str) -> Now {
    let open = || -> io::Result<Option<File>> {
        let (name, folders) = path.rsplit_once('/').map_or((path, ""), |(a, b)| (b, a));
        let mut dir = Dir::open(root)?;
        for folder in folders.split('/').filter(|f| !f.is_empty()) {
            dir = dir.subdir(folder, false)?;
        }
        dir.read(name)
    };
    let Ok(Some(file)) = open() else {
        return Now::Missing;
    };
    let mut bytes = Vec::new();
    match file.take(MAX_SHOWN as u64 + 1).read_to_end(&mut bytes) {
        Ok(_) if bytes.len() > MAX_SHOWN => Now::Large,
        Ok(_) => Now::Bytes(bytes),
        Err(_) => Now::Missing,
    }
}

fn copy_read(
    git: Option<&Path>,
    root: &Path,
    path: &str,
    earlier: Option<Vec<u8>>,
) -> Result<Value, Refusal> {
    Ok(match (earlier, now(root, path)) {
        (_, Now::Large) => named("large"),
        (None, Now::Missing) => named("none"),
        (None, Now::Bytes(new)) => whole(&new, '+'),
        (Some(old), _) if old.len() > MAX_SHOWN => named("large"),
        (Some(old), Now::Missing) => whole(&old, '-'),
        (Some(old), Now::Bytes(new)) if old == new => named("none"),
        (Some(old), Now::Bytes(new)) if old.contains(&0) || new.contains(&0) => named("binary"),
        (Some(old), Now::Bytes(new)) => {
            let git = git.ok_or((
                "no_git",
                "Git is not installed, so this change cannot be compared".to_owned(),
            ))?;
            compare(git, &old, &new)?
        }
    })
}

/// Git's diff of two texts, which it reads from two files of a private folder that is removed
/// after.
fn compare(git: &Path, old: &[u8], new: &[u8]) -> Result<Value, Refusal> {
    let dir = std::env::temp_dir().join(format!("bate-changes-{}", sys::random_hex(8)));
    let run = || -> Result<Value, Refusal> {
        sys::create_private_dir(&dir).map_err(failed)?;
        fs::write(dir.join("a"), old).map_err(failed)?;
        fs::write(dir.join("b"), new).map_err(failed)?;
        let mut cmd = git_command(git);
        cmd.current_dir(&dir).args([
            "diff",
            "--no-index",
            "--no-ext-diff",
            "--no-textconv",
            "--",
            "a",
            "b",
        ]);
        let out = capture(cmd, MAX_SHOWN)?;
        // 1: they differ.
        match out.code {
            _ if out.cut => Ok(named("large")),
            Some(0 | 1) => Ok(shown(&out.bytes)),
            _ => Err(failed("git could not compare the two versions")),
        }
    };
    let result = run();
    let _ = fs::remove_dir_all(&dir);
    result
}

/// The Git of the system, if there is one. On a Mac without the developer tools `/usr/bin/git` is
/// a stub that asks, on the person's screen, to install them: it is not run then.
fn git_path() -> Option<PathBuf> {
    let git = project_server::on_path_in(login_path::get(), "git")?;
    let ready = || {
        Command::new("/usr/bin/xcode-select")
            .arg("-p")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    (git != Path::new("/usr/bin/git") || ready()).then_some(git)
}

/// Whether `root` is a Git repository of its own: a real `.git` folder, not a link or a file that
/// names a folder elsewhere.
fn is_repo(root: &Path) -> bool {
    Dir::open(root)
        .and_then(|d| d.subdir(".git", false))
        .is_ok()
}

/// A `git` command with nothing of the user's or the system's configuration and no way to ask
/// anything or to lock the repository.
fn git_command(git: &Path) -> Command {
    let mut cmd = Command::new(git);
    cmd.env_clear()
        .env("PATH", login_path::get())
        .env("LC_ALL", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["--no-pager", "--literal-pathspecs"])
        .args(["-c", "core.fsmonitor=false", "-c", "core.quotepath=false"]);
    cmd
}

/// `git_command` for the repository `root`, whatever its own configuration says of where the
/// files are.
fn repo_command(git: &Path, root: &Path) -> Command {
    let mut cmd = git_command(git);
    cmd.arg("--git-dir")
        .arg(root.join(".git"))
        .arg("--work-tree")
        .arg(root)
        .current_dir(root);
    cmd
}

struct Out {
    bytes: Vec<u8>,
    /// There was more output than `cap`.
    cut: bool,
    code: Option<i32>,
}

/// Runs `cmd` to its end: at most `WAIT`, and at most `cap` bytes of its output are kept.
fn capture(mut cmd: Command, cap: usize) -> Result<Out, Refusal> {
    let (reader, writer) = io::pipe().map_err(failed)?;
    cmd.stdin(Stdio::null())
        .stdout(writer)
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = {
        let _spawning = sys::SPAWN.lock().unwrap_or_else(|e| e.into_inner());
        cmd.spawn().map_err(failed)?
    };
    // Closes this side of the pipe, so the reader sees the end of the output.
    drop(cmd);
    let (done, got) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = reader.take(cap as u64 + 1).read_to_end(&mut bytes);
        let _ = done.send(bytes);
    });
    let finished = got.recv_timeout(WAIT);
    // SAFETY: a signal to the group of a child that is ours, not yet waited for.
    unsafe { libc::killpg(child.id() as i32, libc::SIGKILL) };
    let status = child.wait().map_err(failed)?;
    let mut bytes = finished.map_err(|_| failed("git did not finish in 10 s"))?;
    let cut = bytes.len() > cap;
    bytes.truncate(cap);
    Ok(Out {
        bytes,
        cut,
        code: status.code(),
    })
}

/// Runs `git` in the repository `root`; an error when it fails.
fn git_run(git: &Path, root: &Path, args: &[&str], cap: usize) -> Result<Out, Refusal> {
    let mut cmd = repo_command(git, root);
    cmd.args(args);
    let out = capture(cmd, cap)?;
    match out.code {
        Some(0) => Ok(out),
        _ => Err(failed(format!("`git {}` failed", args[0]))),
    }
}

/// Refuses a repository that defines a filter: `git` would run the program it names on the bot's
/// files, which the bot may have named itself.
fn guard(git: &Path, root: &Path) -> Result<(), Refusal> {
    let names = git_run(git, root, &["config", "--list", "--name-only"], MAX_LIST)?;
    if String::from_utf8_lossy(&names.bytes)
        .lines()
        .any(|name| name.starts_with("filter."))
    {
        return Err((
            "git_refused",
            "this repository defines a filter, which Bate does not run: look at its changes with git itself".into(),
        ));
    }
    Ok(())
}

/// `(files, more)`: the uncommitted changes of the repository, untracked files too.
fn git_list(git: &Path, root: &Path) -> Result<(Vec<Value>, bool), Refusal> {
    guard(git, root)?;
    let args = [
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
        "--ignore-submodules=all",
        "--no-renames",
    ];
    let out = git_run(git, root, &args, MAX_LIST)?;
    let mut entries: Vec<&[u8]> = out.bytes.split(|b| *b == 0).collect();
    // The last entry of a cut list may be half of one.
    if out.cut {
        entries.pop();
    }
    let (mut files, mut more) = (Vec::new(), out.cut);
    for entry in entries.into_iter().filter(|e| e.len() > 3) {
        let (x, y) = (entry[0], entry[1]);
        let Some(path) = std::str::from_utf8(&entry[3..]).ok().and_then(plain) else {
            continue;
        };
        if files.len() == MAX_FILES {
            more = true;
            break;
        }
        let status = match (x, y) {
            (b'?', _) | (b'A', b'M' | b' ') => "added",
            (b'D', _) | (_, b'D') => "deleted",
            _ => "modified",
        };
        files.push(json!({"path": path, "status": status}));
    }
    // Git lists what it tracks before what it does not.
    files.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    Ok((files, more))
}

fn git_read(git: &Path, root: &Path, path: &str) -> Result<Value, Refusal> {
    guard(git, root)?;
    let base = match git_run(git, root, &["rev-parse", "--verify", "-q", "HEAD"], 256) {
        Ok(_) => "HEAD",
        Err(_) => EMPTY_TREE,
    };
    let args = [
        "diff",
        base,
        "--no-ext-diff",
        "--no-textconv",
        "--no-renames",
        "--ignore-submodules=all",
        "--",
        path,
    ];
    let out = git_run(git, root, &args, MAX_SHOWN)?;
    if out.cut {
        return Ok(named("large"));
    }
    if !out.bytes.is_empty() {
        return Ok(shown(&out.bytes));
    }
    // Not a change of a tracked file: maybe a file Git does not track yet, which has no diff.
    let args = ["ls-files", "--others", "--exclude-standard", "--", path];
    if git_run(git, root, &args, 4096)?.bytes.is_empty() {
        return Ok(named("none"));
    }
    Ok(match now(root, path) {
        Now::Bytes(bytes) => whole(&bytes, '+'),
        Now::Large => named("large"),
        Now::Missing => named("none"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_plain_relative_path_is_taken() {
        for good in ["a", "a/b.txt", "src/x y/z"] {
            assert_eq!(plain(good), Some(good));
        }
        for bad in [
            "",
            "/etc/passwd",
            "../a",
            "a/../b",
            "a//b",
            ".git/config",
            "a/.hidden",
            "a/",
        ] {
            assert_eq!(plain(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_whole_file_is_one_hunk() {
        assert_eq!(
            whole(b"one\ntwo\n", '+')["diff"],
            "@@ -0,0 +1,2 @@\n+one\n+two\n"
        );
        assert_eq!(whole(b"one\n", '-')["diff"], "@@ -1,1 +0,0 @@\n-one\n");
        assert_eq!(whole(b"a\0b", '+')["kind"], "binary");
    }
}

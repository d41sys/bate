//! The `PATH` an installed agent is looked up and started with: the service's own folders, then
//! those its user's login shell adds, then the folder of the adapters Bate installed itself (so
//! a copy the user has on their own `PATH` wins). Under launchd the service has only the system's; a program a
//! Node version manager or `~/.local/bin` provides (and the `node` it runs on) is on the shell's.

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{LazyLock, mpsc};
use std::time::Duration;
use std::{env, thread};

use crate::sys::{self, log};

/// How long the shell may take: a login shell runs the user's rc files, which may wait for anything.
const WAIT: Duration = Duration::from_secs(5);
/// Rc files may print what they like: the answer is what lies between these.
const BEGIN: &str = "bate-path-begin:";
const END: &str = ":bate-path-end";
/// The most output read from the shell.
const MAX_OUTPUT: u64 = 1 << 16;

static COMBINED: LazyLock<OsString> = LazyLock::new(combined);

/// The combined `PATH`, read from the shell once per run, at the first call. That call waits for
/// the shell (up to `WAIT`): the caller must not be the thread that answers requests, unless a
/// call on a thread of its own has made the answer ready.
pub fn get() -> &'static OsStr {
    &COMBINED
}

fn combined() -> OsString {
    let all = login_paths();
    let mut dirs: Vec<PathBuf> = env::split_paths(&all).collect();
    dirs.push(crate::adapters::bin_dir());
    env::join_paths(dirs).unwrap_or(all)
}

fn login_paths() -> OsString {
    let own = env::var_os("PATH").unwrap_or_default();
    match from_shell() {
        Ok(shell) => {
            let all = join(&own, &shell);
            let count = |p: &OsStr| env::split_paths(p).count();
            log!(
                "login shell PATH read: {} folders, {} added to the service's {}",
                count(&shell),
                count(&all) - count(&own),
                count(&own)
            );
            all
        }
        Err(why) => {
            log!("login shell PATH not read ({why}): the service's own is used");
            own
        }
    }
}

/// The service's folders first, then the shell's that are not among them. A folder that is not
/// absolute is dropped: it would mean the working folder of whoever uses the `PATH`.
fn join(own: &OsStr, shell: &OsStr) -> OsString {
    let mut dirs: Vec<PathBuf> = env::split_paths(own).collect();
    for dir in env::split_paths(shell) {
        if dir.is_absolute() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    env::join_paths(dirs).unwrap_or_else(|_| own.to_owned())
}

/// What the user's login shell has as `PATH`.
fn from_shell() -> Result<OsString, String> {
    let shell = env::var_os("SHELL")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/zsh".into());
    let mut command = Command::new(shell);
    command
        .args(["-l", "-i", "-c"])
        .arg(format!("printf '%s' \"{BEGIN}$PATH{END}\""))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // SAFETY: `setsid` only; a session of its own has no terminal for an interactive shell to
    // wait on, and one signal to the group ends whatever the rc files started.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        })
    };
    let mut child = {
        let _spawning = sys::SPAWN.lock().unwrap_or_else(|e| e.into_inner());
        command.spawn().map_err(|e| format!("cannot start: {e}"))?
    };
    let stdout = child.stdout.take().expect("stdout is piped");
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.take(MAX_OUTPUT).read_to_end(&mut out);
        let _ = tx.send(out);
    });
    let out = rx.recv_timeout(WAIT);
    // Only while it is not reaped: then its group is still its own.
    if matches!(child.try_wait(), Ok(None)) {
        // SAFETY: a signal to the group of a child that is still ours.
        unsafe { libc::killpg(child.id() as i32, libc::SIGKILL) };
    }
    let _ = child.kill();
    let _ = child.wait();
    let out = out.map_err(|_| format!("no answer in {} s", WAIT.as_secs()))?;
    between(&String::from_utf8_lossy(&out))
        .filter(|path| !path.is_empty())
        .map(OsString::from)
        .ok_or_else(|| "empty answer".into())
}

/// The text between the last `BEGIN` and the `END` after it.
fn between(text: &str) -> Option<&str> {
    let from = text.rfind(BEGIN)? + BEGIN.len();
    let len = text[from..].find(END)?;
    Some(&text[from..from + len])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_is_what_lies_between_the_markers() {
        assert_eq!(
            between("hello\nbate-path-begin:/a:/b:bate-path-end\nbye"),
            Some("/a:/b")
        );
        assert_eq!(between("noise only"), None);
        assert_eq!(between("bate-path-begin:/a"), None);
    }

    #[test]
    fn the_services_folders_stay_in_front() {
        let all = join(OsStr::new("/x:/y"), OsStr::new("/h/bin:/y:rel:/x:/z"));
        assert_eq!(all, OsString::from("/x:/y:/h/bin:/z"));
    }
}

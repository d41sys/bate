//! A routine's precheck: a command that runs before the bot, with no model, in the limits of the
//! bot's own commands (`limit::command`). Its time and its output are bounded.

use crate::sys;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use std::{io, thread};

/// How long it may run before it is killed.
const WAIT: Duration = Duration::from_secs(20);

/// How much of its output is kept; the rest is read and dropped.
const MAX_OUTPUT: usize = 4096;

pub enum Checked {
    /// Exit 0, with what it printed (standard output and error together).
    Passed(String),
    /// Any other end, with the first line it printed, or what happened if it printed none.
    Stopped(String),
}

/// Runs `command` to its end, or until `cancel` is set, which kills it. An error: it could not
/// be started.
pub fn run(mut command: Command, cancel: &AtomicBool) -> io::Result<Checked> {
    let (mut reader, writer) = io::pipe()?;
    command
        .stdin(Stdio::null())
        .stdout(writer.try_clone()?)
        .stderr(writer)
        .process_group(0);
    let mut child = {
        let _spawning = sys::SPAWN.lock().unwrap_or_else(|e| e.into_inner());
        command.spawn()?
    };
    // Closes this side of the pipe, so the reader sees the end of the output.
    drop(command);
    let (done, finished) = mpsc::channel();
    thread::spawn(move || {
        let (mut kept, mut chunk) = (Vec::new(), [0u8; 1024]);
        while let Ok(n @ 1..) = reader.read(&mut chunk) {
            kept.extend_from_slice(&chunk[..n.min(MAX_OUTPUT - kept.len())]);
        }
        let _ = done.send(kept);
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait()? {
            Some(status) => break Some(status),
            None if cancel.load(Ordering::Relaxed) => {
                killpg(&child);
                let _ = child.wait();
                return Ok(Checked::Stopped("cancelled".into()));
            }
            None if started.elapsed() >= WAIT => break None,
            None => thread::sleep(Duration::from_millis(20)),
        }
    };
    killpg(&child);
    let _ = child.wait();
    let out = finished
        .recv_timeout(Duration::from_secs(1))
        .unwrap_or_default();
    let out = String::from_utf8_lossy(&out).into_owned();
    Ok(match status {
        Some(s) if s.success() => Checked::Passed(out),
        Some(s) => Checked::Stopped(first_line(&out).unwrap_or_else(|| match s.code() {
            Some(code) => format!("exit code {code}"),
            None => "ended by a signal".into(),
        })),
        None => Checked::Stopped(format!("did not end in {} s", WAIT.as_secs())),
    })
}

/// Ends the command's whole group, so what it left behind ends too and the pipe closes.
fn killpg(child: &std::process::Child) {
    // SAFETY: a signal to the group of a child that is ours.
    unsafe { libc::killpg(child.id() as i32, libc::SIGKILL) };
}

fn first_line(out: &str) -> Option<String> {
    let line = out.lines().map(str::trim).find(|l| !l.is_empty())?;
    Some(line.chars().take(200).collect())
}

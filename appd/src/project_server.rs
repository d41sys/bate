//! A project's connector: the program the service runs once for the project's bots. It serves
//! MCP over HTTP on a unix socket in its own data folder; the bots reach it only through the
//! service's local port (see `proxy`). The sandbox is `limit`'s, with the data folder in the
//! place of a bot's folder.

use crate::core::Msg;
use crate::sys::{self, log};
use crate::{limit, paths};
use proto::projects::ProjectConnector;
use std::io::{self, BufRead};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{fs, thread};

/// The longest path a unix socket may have on macOS.
const MAX_SOCKET_PATH: usize = 103;
/// How often the exit of a program is looked for.
const LOOK: Duration = Duration::from_millis(100);

/// Where project `id`'s program keeps its files and its socket. Mode 0700.
pub fn data_dir(id: &str) -> PathBuf {
    paths::home().join("project-data").join(id)
}

/// A running program.
pub struct Program {
    child: Arc<Mutex<Child>>,
    /// Closes when the service goes, however it goes: the program may watch for that.
    _stdin: ChildStdin,
    pub socket: PathBuf,
}

impl Program {
    /// Ends the program and whatever it started.
    pub fn stop(&self) {
        let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
        // Only while it is not reaped: then its process group is still its own.
        if matches!(child.try_wait(), Ok(None)) {
            // SAFETY: a signal to the group of a child that is still ours.
            unsafe { libc::killpg(child.id() as i32, libc::SIGKILL) };
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// The program as the person wrote it: a path (relative ones start in `work`: the project's
/// folder, or a workplace's) or a name looked up in the service's `PATH`.
fn resolve(command: &str, work: &std::path::Path) -> io::Result<PathBuf> {
    if command.contains('/') {
        return Ok(work.join(command));
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|dir| dir.join(command))
        .find(|p| p.is_file())
        .ok_or_else(|| io::Error::other(format!("`{command}` is not on the service's PATH")))
}

/// Starts the program for project `id` for `work`, its folder. A workplace's program (`home`:
/// its folder) starts there and may read it; else it starts in `work`. `Msg::ProjectExited`
/// follows when it ends.
pub fn start(
    id: &str,
    spec: &ProjectConnector,
    work: &str,
    home: Option<&std::path::Path>,
    generation: u64,
    tx: Sender<Msg>,
) -> io::Result<Program> {
    let work = PathBuf::from(work);
    let data = data_dir(id);
    sys::create_private_dir(&data)?;
    let data = data.canonicalize()?;
    let socket = data.join("mcp");
    if socket.as_os_str().len() > MAX_SOCKET_PATH {
        return Err(io::Error::other(format!(
            "the path {} is too long for a socket",
            socket.display()
        )));
    }
    // A socket left by an earlier run would look like the program being ready.
    let _ = fs::remove_file(&socket);
    let start = home.unwrap_or(&work);
    let program = resolve(&spec.command, start)?;
    let mut command = limit::command(&program, &data, &work, home, false)?;
    command
        .args(&spec.args)
        .env("BATE_MCP_SOCKET", &socket)
        .current_dir(start)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = {
        let _spawning = sys::SPAWN.lock().unwrap_or_else(|e| e.into_inner());
        command.spawn()?
    };
    let stdin = child.stdin.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    let name = id.to_owned();
    thread::spawn(move || {
        for line in io::BufReader::new(stderr)
            .split(b'\n')
            .map_while(Result::ok)
        {
            log!(
                "project {name}: connector: {}",
                String::from_utf8_lossy(&line)
            );
        }
    });
    log!("project {id}: connector started (pid {})", child.id());
    let child = Arc::new(Mutex::new(child));
    let (watched, id) = (child.clone(), id.to_owned());
    thread::spawn(move || {
        let why = loop {
            thread::sleep(LOOK);
            let mut child = watched.lock().unwrap_or_else(|e| e.into_inner());
            match child.try_wait() {
                Ok(None) => {}
                Ok(Some(status)) => break format!("stopped ({status})"),
                Err(e) => break format!("cannot be watched: {e}"),
            }
        };
        let _ = tx.send(Msg::ProjectExited {
            id,
            generation,
            why,
        });
    });
    Ok(Program {
        child,
        _stdin: stdin,
        socket,
    })
}

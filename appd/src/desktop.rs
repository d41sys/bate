//! The helper app: `Bate Helper.app`, at a fixed path, holds the macOS permissions Accessibility
//! and Screen Recording, so the service and the window never need them. The service listens on
//! a socket in its run dir, starts the helper on the first desktop action, and passes each
//! call of a bot to it. Only the helper's own program may connect.

use crate::core::Msg;
use crate::sys::{self, log, now_ms};
use crate::{attach, paths};
use proto::desktop::{Call, Frame, MAX_FRAME, Request};
use proto::{read_frame, write_frame};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

/// How long the helper may take to start and connect.
const START: Duration = Duration::from_secs(10);
/// How long it may take over one call.
const CALL: Duration = Duration::from_secs(30);

/// The helper program: `$APPD_HELPER`, else inside `Bate Helper.app` in the user's
/// Applications folder.
fn exe() -> PathBuf {
    match std::env::var_os("APPD_HELPER").filter(|p| !p.is_empty()) {
        Some(path) => path.into(),
        None => PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
            .join("Applications/Bate Helper.app/Contents/MacOS/bate-helper"),
    }
}

/// The app around the program: `Bate Helper.app/Contents/MacOS/bate-helper`.
fn app(exe: &std::path::Path) -> &std::path::Path {
    exe.ancestors().nth(3).unwrap_or(exe)
}

/// What starts apps: `$APPD_OPEN` (tests), else `open`.
fn opener() -> PathBuf {
    std::env::var_os("APPD_OPEN")
        .filter(|p| !p.is_empty())
        .map_or_else(|| "/usr/bin/open".into(), PathBuf::from)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Default)]
struct State {
    /// Counts connections, so the end of an old one does not undo a newer.
    connection: u64,
    writer: Option<UnixStream>,
    /// The helper's macOS permissions, from its latest `status`: accessibility, screen recording.
    status: Option<(bool, bool)>,
    next: u64,
    waiting: HashMap<u64, Sender<Frame>>,
}

struct Inner {
    state: Mutex<State>,
    connected: Condvar,
    tx: Sender<Msg>,
}

/// The service's side of the helper app, shared by the core and the threads that wait on it.
#[derive(Clone)]
pub struct Link(Arc<Inner>);

impl Link {
    /// Listens for the helper. Without the socket (it could not be made) every action fails
    /// with a message that says so.
    pub fn start(tx: Sender<Msg>) -> Link {
        let link = Link(Arc::new(Inner {
            state: Mutex::default(),
            connected: Condvar::new(),
            tx,
        }));
        match listen() {
            Ok(listener) => {
                let link = link.clone();
                thread::spawn(move || link.accept(listener));
            }
            Err(e) => log!("the helper app cannot connect: {e}"),
        }
        link
    }

    fn accept(&self, listener: UnixListener) {
        for stream in listener.incoming().flatten() {
            // The user's own, and the helper's own program: not another program of theirs.
            let expected = exe().canonicalize().ok();
            let actual = sys::peer_exe(&stream).and_then(|p| p.canonicalize().ok());
            if !sys::peer_is_self(&stream) || actual.is_none() || actual != expected {
                log!("refused a connection to the helper's socket from another program");
                continue;
            }
            self.attach(stream);
        }
    }

    fn attach(&self, stream: UnixStream) {
        let Ok(writer) = stream.try_clone() else {
            return;
        };
        let mut state = lock(&self.0.state);
        state.connection += 1;
        let connection = state.connection;
        if let Some(old) = state.writer.replace(writer) {
            let _ = old.shutdown(std::net::Shutdown::Both);
        }
        state.status = None;
        drop(state);
        self.0.connected.notify_all();
        let link = self.clone();
        thread::spawn(move || link.read(stream, connection));
    }

    fn read(&self, stream: UnixStream, connection: u64) {
        let mut input = BufReader::new(stream);
        let mut buf = Vec::new();
        loop {
            match read_frame::<Frame>(&mut input, MAX_FRAME, &mut buf) {
                Ok(Some(Frame::Status {
                    accessibility,
                    screen_recording,
                })) => {
                    let mut state = lock(&self.0.state);
                    if state.connection == connection {
                        state.status = Some((accessibility, screen_recording));
                        drop(state);
                        let _ = self.0.tx.send(Msg::HelperChanged);
                    }
                }
                Ok(Some(reply @ Frame::Reply { id, .. })) => {
                    if let Some(waiter) = lock(&self.0.state).waiting.remove(&id) {
                        let _ = waiter.send(reply);
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::InvalidData => {}
                Ok(None) | Err(_) => break,
            }
        }
        let mut state = lock(&self.0.state);
        if state.connection == connection {
            state.writer = None;
            state.status = None;
            // Waiters see a closed channel: the helper quit before it answered.
            state.waiting.clear();
            drop(state);
            let _ = self.0.tx.send(Msg::HelperChanged);
        }
    }

    /// The helper's state for `get_helper`; the permissions are `null` while it is not running.
    pub fn info(&self) -> Value {
        let state = lock(&self.0.state);
        let exe = exe();
        json!({
            "installed": exe.exists(),
            "path": app(&exe),
            "connected": state.writer.is_some(),
            "accessibility": state.status.map(|s| s.0),
            "screenRecording": state.status.map(|s| s.1),
        })
    }

    /// Opens the privacy pane where the user allows the helper, starting the helper first when
    /// it is not running: that is what makes macOS list it there.
    pub fn open_settings(&self, pane: &str) -> Result<(), String> {
        let pane = match pane {
            "accessibility" => "Privacy_Accessibility",
            "screen_recording" => "Privacy_ScreenCapture",
            _ => return Err("`pane` must be accessibility or screen_recording".into()),
        };
        let link = self.clone();
        thread::spawn(move || {
            if exe().exists() && lock(&link.0.state).writer.is_none() {
                let _ = launch();
            }
            let url = format!("x-apple.systempreferences:com.apple.preference.security?{pane}");
            if let Err(e) = Command::new(opener()).arg(url).status() {
                log!("System Settings not opened: {e}");
            }
        });
        Ok(())
    }

    /// Does one call of `bot` in the helper and gives the text for the bot. A screenshot is
    /// saved in the bot's folder and its path given. Blocks: not for the core's thread.
    pub fn call(&self, bot: &str, call: Call) -> Result<String, String> {
        self.start_helper()?;
        let (waiter, answer) = mpsc::channel();
        let id = {
            let mut state = lock(&self.0.state);
            state.next += 1;
            let id = state.next;
            state.waiting.insert(id, waiter);
            let sent = match state.writer.as_mut() {
                Some(writer) => write_frame(writer, &Request { id, call }, MAX_FRAME),
                None => Err(io::Error::other("not connected")),
            };
            if let Err(e) = sent {
                state.waiting.remove(&id);
                return Err(format!("Bate Helper did not take the request: {e}"));
            }
            id
        };
        match answer.recv_timeout(CALL) {
            Ok(Frame::Reply {
                error: Some(error), ..
            }) => Err(error),
            Ok(Frame::Reply {
                output: Some(output),
                png,
                ..
            }) => match png {
                None => Ok(output),
                Some(png) => {
                    let png = attach::base64(&png)
                        .filter(|png| png.len() <= attach::MAX_BYTES)
                        .ok_or("Bate Helper sent a picture that is not usable.")?;
                    let name = format!("screenshot-{}.png", now_ms());
                    let path = attach::save(bot, &name, &png)
                        .map_err(|e| format!("The picture was not saved: {e}"))?;
                    Ok(format!("{output} Saved in your folder as {path}."))
                }
            },
            Ok(_) => Err("Bate Helper answered with nothing.".into()),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                lock(&self.0.state).waiting.remove(&id);
                Err(format!(
                    "Bate Helper did not answer within {} s.",
                    CALL.as_secs()
                ))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("Bate Helper quit before it answered.".into())
            }
        }
    }

    /// Waits for the helper to be connected, starting it if it is not running.
    fn start_helper(&self) -> Result<(), String> {
        if lock(&self.0.state).writer.is_some() {
            return Ok(());
        }
        let exe = exe();
        if !exe.exists() {
            return Err(format!(
                "Bate Helper is not installed: {} is missing. The user installs it with helper/bundle.sh.",
                app(&exe).display()
            ));
        }
        launch().map_err(|e| format!("Bate Helper did not start: {e}"))?;
        let state = lock(&self.0.state);
        let (state, _) = self
            .0
            .connected
            .wait_timeout_while(state, START, |s| s.writer.is_none())
            .unwrap_or_else(|e| e.into_inner());
        if state.writer.is_none() {
            return Err("Bate Helper did not connect: it may be blocked, or it quit.".into());
        }
        Ok(())
    }
}

/// Starts the helper app in the background, telling it where the service listens. Through
/// the system's `open`, so that macOS attributes the permissions to the app and not to the
/// service.
fn launch() -> io::Result<()> {
    let exe = exe();
    let status = Command::new(opener())
        .args(["-g", "-a"])
        .arg(app(&exe))
        .arg("--args")
        .arg(paths::helper_socket())
        .status()?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| io::Error::other(format!("open exited with {status}")))
}

fn listen() -> io::Result<UnixListener> {
    let socket = paths::helper_socket();
    // The service's lock is held, so a socket file left here is stale.
    let _ = fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

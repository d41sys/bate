//! The window's one connection to the service: lines of JSON over the service's socket. The
//! page sends a line and receives the next one; it never sees the socket. When the service
//! goes away the bridge connects again, and the service's new `hello` tells the page.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Most bytes of one line from the service (a transcript entry may be large).
const MAX_LINE: usize = proto::MAX_EVENT_FRAME;
/// How long to wait before trying the socket again.
const RETRY: Duration = Duration::from_millis(500);

pub struct Bridge {
    /// The connection lines are sent on; `None` while there is none.
    to: Arc<Mutex<Option<UnixStream>>>,
    lines: Mutex<Receiver<String>>,
}

impl Bridge {
    /// Connects to `socket` now and whenever the connection ends, on a thread of its own.
    pub fn start(socket: PathBuf) -> Bridge {
        let to = Arc::new(Mutex::new(None::<UnixStream>));
        let (tx, lines) = mpsc::channel();
        let writer = to.clone();
        thread::spawn(move || {
            loop {
                let Ok(stream) = UnixStream::connect(&socket) else {
                    thread::sleep(RETRY);
                    continue;
                };
                *writer.lock().unwrap_or_else(|e| e.into_inner()) = stream.try_clone().ok();
                let mut reader = BufReader::new(stream);
                let mut line = Vec::new();
                loop {
                    line.clear();
                    let read = (&mut reader)
                        .take(MAX_LINE as u64)
                        .read_until(b'\n', &mut line);
                    match read {
                        Ok(n) if n > 0 && line.last() == Some(&b'\n') => {}
                        // Ended, broken, or too long: this connection is over.
                        _ => break,
                    }
                    line.pop();
                    let text = String::from_utf8_lossy(&line).into_owned();
                    if tx.send(text).is_err() {
                        // The window is gone.
                        return;
                    }
                }
                *writer.lock().unwrap_or_else(|e| e.into_inner()) = None;
                thread::sleep(RETRY);
            }
        });
        Bridge {
            to,
            lines: Mutex::new(lines),
        }
    }

    /// Sends one line to the service.
    pub fn send(&self, line: &str) -> Result<(), String> {
        if line.contains(['\n', '\r']) {
            return Err("a line may not hold a line break".into());
        }
        if line.len() + 1 > proto::MAX_WINDOW_FRAME {
            return Err(format!(
                "a line may be {} bytes",
                proto::MAX_WINDOW_FRAME - 1
            ));
        }
        let mut to = self.to.lock().unwrap_or_else(|e| e.into_inner());
        let Some(stream) = to.as_mut() else {
            return Err("not connected to the service".into());
        };
        let sent = stream.write_all(format!("{line}\n").as_bytes());
        sent.map_err(|e: io::Error| format!("not sent: {e}"))
    }

    /// Waits for the next line from the service.
    pub fn recv(&self) -> Result<String, String> {
        let lines = self.lines.lock().unwrap_or_else(|e| e.into_inner());
        lines.recv().map_err(|_| "the bridge stopped".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    fn listen(path: &PathBuf) -> UnixListener {
        let _ = std::fs::remove_file(path);
        UnixListener::bind(path).unwrap()
    }

    #[test]
    fn lines_pass_both_ways_and_the_bridge_connects_again() {
        let dir = std::env::temp_dir().join(format!("bate-bridge-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("s.sock");
        // Not listening yet: the bridge keeps trying.
        let bridge = Bridge::start(socket.clone());
        assert_eq!(
            bridge.send("{}"),
            Err("not connected to the service".into())
        );

        let listener = listen(&socket);
        let (mut service, _) = listener.accept().unwrap();
        service
            .write_all(b"{\"type\":\"hello\"}\n{\"n\":1}\n")
            .unwrap();
        assert_eq!(bridge.recv().unwrap(), r#"{"type":"hello"}"#);
        assert_eq!(bridge.recv().unwrap(), r#"{"n":1}"#);
        bridge.send(r#"{"id":1,"type":"hello"}"#).unwrap();
        let mut got = String::new();
        BufReader::new(&service).read_line(&mut got).unwrap();
        assert_eq!(got, "{\"id\":1,\"type\":\"hello\"}\n");

        // One line is one frame.
        assert!(bridge.send("{}\n{}").is_err());
        assert!(bridge.send(&"x".repeat(proto::MAX_WINDOW_FRAME)).is_err());

        // The service stops and starts again: the page hears its new hello.
        drop(service);
        drop(listener);
        let listener = listen(&socket);
        let (mut service, _) = listener.accept().unwrap();
        service
            .write_all(b"{\"type\":\"hello\",\"again\":true}\n")
            .unwrap();
        assert_eq!(bridge.recv().unwrap(), r#"{"type":"hello","again":true}"#);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

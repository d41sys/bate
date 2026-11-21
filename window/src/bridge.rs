//! The window's one connection to the service: lines of JSON over the service's socket. The
//! page sends a line and receives the next one; it never sees the socket. When the service
//! goes away the bridge connects again, and the service's new `hello` tells the page.

use serde_json::{Value, json};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Most bytes of one line from the service (a transcript entry may be large).
const MAX_LINE: usize = proto::MAX_EVENT_FRAME;
/// How long to wait before trying the socket again.
const RETRY: Duration = Duration::from_millis(500);
/// How long `ask` waits for the service to say something.
const ASK_LIMIT: Duration = Duration::from_secs(10);

pub struct Bridge {
    /// The connection lines are sent on; `None` while there is none.
    to: Arc<Mutex<Option<UnixStream>>>,
    /// Set when its window is gone: no connecting again.
    stopped: Arc<AtomicBool>,
    lines: Mutex<Receiver<String>>,
}

impl Bridge {
    /// Connects to `socket` now and whenever the connection ends, on a thread of its own.
    pub fn start(socket: PathBuf) -> Bridge {
        let to = Arc::new(Mutex::new(None::<UnixStream>));
        let (tx, lines) = mpsc::channel();
        let writer = to.clone();
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let Ok(stream) = UnixStream::connect(&socket) else {
                    thread::sleep(RETRY);
                    continue;
                };
                {
                    // Checked under the lock `stop` takes: a stop now is seen, or sees this stream.
                    let mut to = writer.lock().unwrap_or_else(|e| e.into_inner());
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    *to = stream.try_clone().ok();
                }
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
            stopped,
            lines: Mutex::new(lines),
        }
    }

    /// Ends the connection for good; a `recv` waiting returns an error.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        if let Some(stream) = self.to.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
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

/// For work of the shell's own, with no page behind it (the answer a notification's button gives):
/// says `hello` on a connection of its own, sends `requests` (each a `type` and its fields) and
/// returns each one's response frame, in order. Gives up after `ASK_LIMIT` of silence.
pub fn ask(socket: &Path, requests: &[Value]) -> Result<Vec<Value>, String> {
    let stream = UnixStream::connect(socket).map_err(|e| e.to_string())?;
    let failed = |e: io::Error| e.to_string();
    stream.set_read_timeout(Some(ASK_LIMIT)).map_err(failed)?;
    stream.set_write_timeout(Some(ASK_LIMIT)).map_err(failed)?;
    // Id 0 is the `hello`; the requests are 1, 2, ...
    let mut lines = vec![json!({"id": 0, "type": "hello", "protocol": 1})];
    for (n, request) in requests.iter().enumerate() {
        let mut request = request.clone();
        request["id"] = json!(n + 1);
        lines.push(request);
    }
    let mut out = String::new();
    for line in &lines {
        out.push_str(&line.to_string());
        out.push('\n');
    }
    (&stream).write_all(out.as_bytes()).map_err(failed)?;

    let mut replies: Vec<Option<Value>> = vec![None; lines.len()];
    let mut reader = BufReader::new(&stream);
    let mut line = Vec::new();
    while replies.iter().any(Option::is_none) {
        line.clear();
        let read = (&mut reader)
            .take(MAX_LINE as u64)
            .read_until(b'\n', &mut line)
            .map_err(failed)?;
        if read == 0 || line.last() != Some(&b'\n') {
            return Err("the service closed the connection".into());
        }
        // Events and anything else that is not a reply to these are skipped.
        let Ok(frame) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        if frame["type"] == "response"
            && let Some(slot) = frame["id"]
                .as_u64()
                .and_then(|n| replies.get_mut(n as usize))
        {
            *slot = Some(frame);
        }
    }
    let mut replies = replies.into_iter().flatten();
    if replies.next().is_none_or(|hello| hello["success"] != true) {
        return Err("the service did not take hello".into());
    }
    Ok(replies.collect())
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

        // Its window closed: the connection ends for good, and a waiting `recv` returns.
        bridge.stop();
        assert!(bridge.recv().is_err());
        listener.set_nonblocking(true).unwrap();
        thread::sleep(RETRY * 3);
        assert!(listener.accept().is_err(), "it connected again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ask_says_hello_sends_its_requests_and_returns_each_reply_in_order() {
        let dir = std::env::temp_dir().join(format!("bate-ask-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("s.sock");
        let listener = listen(&socket);
        let service = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(b"{\"type\":\"hello\"}\n").unwrap();
            let mut seen = Vec::new();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            for _ in 0..3 {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                seen.push(serde_json::from_str::<Value>(&line).unwrap());
            }
            // An event between, and the replies out of order: ids match them.
            for id in [2, 0, 1] {
                stream.write_all(b"{\"type\":\"bots_changed\"}\n").unwrap();
                let reply =
                    json!({"type": "response", "id": id, "success": true, "data": {"n": id}});
                stream.write_all(format!("{reply}\n").as_bytes()).unwrap();
            }
            seen
        });
        let replies = ask(
            &socket,
            &[
                json!({"type": "answer", "question": "q1"}),
                json!({"type": "list_attention"}),
            ],
        )
        .unwrap();
        assert_eq!(
            replies
                .iter()
                .map(|r| r["data"]["n"].clone())
                .collect::<Vec<_>>(),
            [1, 2]
        );
        let seen = service.join().unwrap();
        assert_eq!(seen[0], json!({"id": 0, "type": "hello", "protocol": 1}));
        assert_eq!(
            seen[1],
            json!({"id": 1, "type": "answer", "question": "q1"})
        );
        assert_eq!(seen[2], json!({"id": 2, "type": "list_attention"}));

        // No service: an error, not a hang.
        let _ = std::fs::remove_dir_all(&dir);
        assert!(ask(&socket, &[]).is_err());
    }
}

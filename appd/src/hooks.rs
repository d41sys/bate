//! Local webhooks: `POST http://127.0.0.1:<port>/hooks/<token>` starts a routine. Only this
//! Mac can connect, and only who knows a routine's token can start it.

use crate::core::Msg;
use crate::sys::log;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

/// The port when `APPD_HOOK_PORT` does not name another (0: any free one).
const DEFAULT_PORT: u16 = 47821;
/// Most bytes of data a request may carry; they go into the routine's prompt.
pub const MAX_BODY: usize = 64 * 1024;
const MAX_HEAD: u64 = 8 * 1024;
/// How long a request may take to arrive, and the service to answer.
const WAIT: Duration = Duration::from_secs(5);
/// Requests served at once; more are dropped unanswered.
const MAX_OPEN: usize = 16;

/// What the service answers a webhook: the status and a line for whoever called.
pub type Answer = (u16, String);

/// A webhook request for the routine with `token`.
pub struct Hit {
    pub token: String,
    pub data: String,
    pub answer: Sender<Answer>,
}

/// Starts listening and returns the port.
pub fn start(tx: Sender<Msg>) -> io::Result<u16> {
    let port = match std::env::var("APPD_HOOK_PORT") {
        Ok(port) => port
            .parse()
            .map_err(|_| io::Error::other("APPD_HOOK_PORT is not a port number"))?,
        Err(_) => DEFAULT_PORT,
    };
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let port = listener.local_addr()?.port();
    static OPEN: AtomicUsize = AtomicUsize::new(0);
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            if OPEN.fetch_add(1, Ordering::SeqCst) >= MAX_OPEN {
                OPEN.fetch_sub(1, Ordering::SeqCst);
                continue;
            }
            let tx = tx.clone();
            thread::spawn(move || {
                let (status, line) = match request(&stream) {
                    Ok(Some((token, data))) => ask(&tx, token, data),
                    Ok(None) => (404, "not found".into()),
                    Err(answer) => answer,
                };
                let _ = respond(&stream, status, &line);
                OPEN.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    Ok(port)
}

/// Hands the request to the service's one thread and waits for what it says.
fn ask(tx: &Sender<Msg>, token: String, data: String) -> Answer {
    let (answer, answered) = mpsc::channel();
    let hit = Hit {
        token,
        data,
        answer,
    };
    if tx.send(Msg::Hook(hit)).is_err() {
        return (503, "the service is stopping".into());
    }
    answered
        .recv_timeout(WAIT)
        .unwrap_or((503, "the service is busy".into()))
}

/// Reads one request. `Ok(None)`: not a webhook path.
fn request(stream: &TcpStream) -> Result<Option<(String, String)>, Answer> {
    let bad = |line: &str| (400, line.to_owned());
    stream
        .set_read_timeout(Some(WAIT))
        .map_err(|_| bad("bad connection"))?;
    stream
        .set_write_timeout(Some(WAIT))
        .map_err(|_| bad("bad connection"))?;
    let mut reader = BufReader::new(stream.take(MAX_HEAD));
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|_| bad("bad request"))?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let mut length = 0usize;
    loop {
        let mut header = String::new();
        let n = reader
            .read_line(&mut header)
            .map_err(|_| bad("bad request"))?;
        if n == 0 {
            return Err(bad("the request's head is cut short or longer than 8 KiB"));
        }
        if header.trim_end().is_empty() {
            break;
        }
        let Some((name, value)) = header.split_once(':') else {
            return Err(bad("bad header"));
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => {
                length = value
                    .trim()
                    .parse()
                    .map_err(|_| bad("bad content-length"))?;
            }
            "transfer-encoding" => return Err((411, "send a content-length".into())),
            _ => {}
        }
    }
    let Some(token) = path.strip_prefix("/hooks/") else {
        return Ok(None);
    };
    if method != "POST" {
        return Err((405, "use POST".into()));
    }
    if length > MAX_BODY {
        return Err((413, format!("more than {MAX_BODY} bytes of data")));
    }
    // What the head's reader already took in comes first.
    let mut body = reader.buffer().to_vec();
    body.truncate(length);
    let mut rest = vec![0; length - body.len()];
    stream
        .try_clone()
        .and_then(|mut stream| stream.read_exact(&mut rest))
        .map_err(|_| bad("the data is shorter than its content-length"))?;
    body.extend(rest);
    Ok(Some((
        token.to_owned(),
        String::from_utf8_lossy(&body).into_owned(),
    )))
}

fn respond(mut stream: &TcpStream, status: u16, line: &str) -> io::Result<()> {
    let body = if status < 300 {
        serde_json::json!({"ok": true})
    } else {
        serde_json::json!({"error": line})
    }
    .to_string();
    if status >= 500 {
        log!("webhook answered {status}: {line}");
    }
    write!(
        stream,
        "HTTP/1.1 {status} {line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

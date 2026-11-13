//! The service's local HTTP port, `127.0.0.1` only. Three things arrive here:
//! - webhooks: `POST /hooks/<token>` starts a routine, for whoever knows its token;
//! - a bot's engine reaching a signed-in MCP server: `POST /mcp/<key>` (see `proxy`);
//! - the browser coming back from a sign-in: `GET /oauth/callback` (see `proxy`).

use crate::core::Msg;
use crate::proxy;
use crate::sys::log;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

/// The port when `APPD_HOOK_PORT` does not name another (0: any free one).
const DEFAULT_PORT: u16 = 47821;
/// Most bytes of data a webhook request may carry; they go into the routine's prompt.
pub const MAX_BODY: usize = 64 * 1024;
/// Most bytes of one message from an engine to an MCP server.
const MAX_MCP_BODY: usize = 1024 * 1024;
const MAX_HEAD: u64 = 8 * 1024;
/// How long a request may take to arrive, and the service to answer.
const WAIT: Duration = Duration::from_secs(5);
/// How long a connection is read on after its answer was sent.
const LINGER: Duration = Duration::from_secs(1);
/// Requests served at once; more are dropped unanswered. A call to an MCP server holds one
/// for as long as the server takes.
const MAX_OPEN: usize = 64;

/// What the service answers a webhook: the status and a line for whoever called.
pub type Answer = (u16, String);

/// A webhook request for the routine with `token`.
pub struct Hit {
    pub token: String,
    pub data: String,
    pub answer: Sender<Answer>,
}

/// One request, read whole.
pub struct Request {
    pub method: String,
    /// Without the query.
    pub path: String,
    pub query: String,
    /// Names in lower case.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        let found = self.headers.iter().find(|(n, _)| n == name);
        found.map(|(_, value)| value.as_str())
    }
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
                if let Err((status, line)) = serve(&stream, &tx) {
                    let _ = respond(&stream, status, &line);
                }
                finish(&stream);
                OPEN.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    Ok(port)
}

/// Answers one request. An error is the answer still to be sent.
fn serve(stream: &TcpStream, tx: &Sender<Msg>) -> Result<(), Answer> {
    let request = request(stream)?;
    let only = |method: &str| match request.method == method {
        true => Ok(()),
        false => Err((405, format!("use {method}"))),
    };
    if let Some(token) = request.path.strip_prefix("/hooks/") {
        only("POST")?;
        let data = String::from_utf8_lossy(&request.body).into_owned();
        Err(ask(tx, token.to_owned(), data))
    } else if let Some(key) = request.path.strip_prefix("/mcp/") {
        only("POST")?;
        proxy::forward(tx, key, &request, stream)
    } else if request.path == "/oauth/callback" {
        only("GET")?;
        let (status, text) = proxy::callback(tx, &request.query);
        let page = format!(
            "<!doctype html><meta charset=utf-8><title>Bate</title><p>{}</p>",
            text.replace('&', "&amp;").replace('<', "&lt;")
        );
        let _ = write!(
            &*stream,
            "HTTP/1.1 {status} -\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{page}",
            page.len()
        );
        Ok(())
    } else {
        Err((404, "not found".into()))
    }
}

/// Ends the connection so that the answer arrives. Whoever called may still be sending (a
/// request refused at its head); closing on unread data would reset the connection and the
/// answer would be lost with it. So: no more from us, then read what still comes, briefly.
fn finish(stream: &TcpStream) {
    let _ = stream.shutdown(Shutdown::Write);
    let mut rest = Until(stream, Instant::now() + LINGER).take(4 * MAX_BODY as u64);
    let _ = io::copy(&mut rest, &mut io::sink());
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

/// Reads from the connection until a moment that holds for the whole request, however slowly
/// its bytes come.
struct Until<'a>(&'a TcpStream, Instant);

impl Read for Until<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.1.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        self.0.set_read_timeout(Some(left))?;
        self.0.read(buf)
    }
}

/// Reads one request.
fn request(stream: &TcpStream) -> Result<Request, Answer> {
    let bad = |line: &str| (400, line.to_owned());
    stream
        .set_write_timeout(Some(WAIT))
        .map_err(|_| bad("bad connection"))?;
    let deadline = Instant::now() + WAIT;
    let mut reader = BufReader::new(Until(stream, deadline).take(MAX_HEAD));
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|_| bad("bad request"))?;
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut length = 0usize;
    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        let n = reader
            .read_line(&mut header)
            .map_err(|_| bad("bad request"))?;
        if n == 0 || !header.ends_with('\n') {
            return Err(bad("the request's head is cut short or longer than 8 KiB"));
        }
        if header.trim_end().is_empty() {
            break;
        }
        let Some((name, value)) = header.split_once(':') else {
            return Err(bad("bad header"));
        };
        let (name, value) = (name.trim().to_ascii_lowercase(), value.trim());
        match name.as_str() {
            "content-length" => {
                length = value.parse().map_err(|_| bad("bad content-length"))?;
            }
            "transfer-encoding" => return Err((411, "send a content-length".into())),
            // A web page that learnt an address here must not be able to use it: browsers say
            // where a request comes from, other callers have no reason to.
            "origin" => return Err((403, "not from a web page".into())),
            _ => {}
        }
        headers.push((name, value.to_owned()));
    }
    let most = match path.starts_with("/mcp/") {
        true => MAX_MCP_BODY,
        false => MAX_BODY,
    };
    if length > most {
        return Err((413, format!("more than {most} bytes of data")));
    }
    // What the head's reader already took in comes first.
    let mut body = reader.buffer().to_vec();
    body.truncate(length);
    let mut rest = vec![0; length - body.len()];
    Until(stream, deadline)
        .read_exact(&mut rest)
        .map_err(|_| bad("the data is shorter than its content-length"))?;
    body.extend(rest);
    Ok(Request {
        method: method.to_owned(),
        path: path.to_owned(),
        query: query.to_owned(),
        headers,
        body,
    })
}

pub fn respond(mut stream: &TcpStream, status: u16, line: &str) -> io::Result<()> {
    let body = if status < 300 {
        serde_json::json!({"ok": true})
    } else {
        serde_json::json!({"error": line})
    }
    .to_string();
    if status >= 500 {
        log!("local port answered {status}: {line}");
    }
    write!(
        stream,
        "HTTP/1.1 {status} {line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

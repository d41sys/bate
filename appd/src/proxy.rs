//! MCP servers, as the bot's engine reaches them: through the service's local port. The
//! engine is given `http://127.0.0.1:<port>/mcp/<key>` in place of the server's address.
//! - A signed-in server: the service adds the token, so none is ever in the engine or in the
//!   bot's folder. The browser's way back from a sign-in ends here too.
//! - A project's connector: the program listens on a unix socket in a folder only the service
//!   (and the program) can reach, and this is the one way to it.

use crate::core::Msg;
use crate::hooks::{Answer, Request};
use crate::oauth::{self, Pending, Tokens};
use crate::project_server;
use crate::sys::now_ms;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

/// How long the service's one thread may take to look something up.
const WAIT: Duration = Duration::from_secs(5);
/// How long an MCP server may take over one call (the engine gives up sooner or later too).
const CALL: Duration = Duration::from_secs(600);
/// Most bytes of one answer passed back to the engine.
const MAX_ANSWER: u64 = 64 * 1024 * 1024;
/// An access token this close to its end is renewed before it is used.
const EARLY_MS: u64 = 30_000;
/// The request headers an MCP server gets from the engine; anything else stays here.
const PASSED: [&str; 5] = [
    "content-type",
    "accept",
    "mcp-protocol-version",
    "mcp-session-id",
    "last-event-id",
];

/// The server behind a key: where it is, and the sign-in, if there is one.
pub struct Upstream {
    pub bot: String,
    pub name: String,
    pub url: String,
    /// A project's connector: the socket its program listens on, in place of `url`.
    pub socket: Option<PathBuf>,
    /// The connector's own headers.
    pub headers: Vec<(String, String)>,
    pub tokens: Option<Tokens>,
}

/// What the service's thread is asked from here.
pub enum Ask {
    /// The server behind `key`.
    Upstream(String, Sender<Option<Upstream>>),
    /// The sign-in that was given `state`; it is taken, and cannot come back twice.
    Take(String, Sender<Option<Pending>>),
    /// New tokens for the connector `name` of `bot`, made for the server at `resource`.
    /// `None`: the sign-in is no longer good. `after`: the access token they were renewed
    /// from; they take that one's place and no other's.
    Tokens {
        bot: String,
        name: String,
        resource: String,
        after: Option<String>,
        tokens: Option<Tokens>,
    },
}

fn ask<T>(tx: &Sender<Msg>, make: impl FnOnce(Sender<Option<T>>) -> Ask) -> Option<T> {
    let (answer, answered) = mpsc::channel();
    tx.send(Msg::Proxy(make(answer))).ok()?;
    answered.recv_timeout(WAIT).ok().flatten()
}

fn keep(tx: &Sender<Msg>, up: &Upstream, old: &Tokens, tokens: Option<Tokens>) {
    let _ = tx.send(Msg::Proxy(Ask::Tokens {
        bot: up.bot.clone(),
        name: up.name.clone(),
        resource: up.url.clone(),
        after: Some(old.access.clone()),
        tokens,
    }));
}

/// The browser came back from a sign-in: trades its code for tokens. Gives the status and
/// the words for the page.
pub fn callback(tx: &Sender<Msg>, query: &str) -> (u16, String) {
    let field = |name: &str| {
        query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find(|(key, _)| *key == name)
            .map(|(_, value)| oauth::decode(value))
    };
    let state = field("state").unwrap_or_default();
    let Some(pending) = ask(tx, |answer| Ask::Take(state, answer)) else {
        return (
            400,
            "This sign-in is not one Bate started, or it was used already. Start again from Bate."
                .into(),
        );
    };
    let Some(code) = field("code") else {
        let why = field("error_description").or(field("error"));
        return (
            400,
            format!(
                "The server did not sign you in: {}.",
                why.unwrap_or("no reason given".into())
            ),
        );
    };
    match oauth::finish(&pending, &code) {
        Ok(tokens) => {
            let _ = tx.send(Msg::Proxy(Ask::Tokens {
                bot: pending.bot.clone(),
                name: pending.name.clone(),
                resource: tokens.resource.clone(),
                after: None,
                tokens: Some(tokens),
            }));
            (
                200,
                "Signed in. You can close this tab and go back to Bate.".into(),
            )
        }
        Err(e) => (502, format!("Signing in failed: {e}")),
    }
}

/// Sends the engine's message on to the server behind `key`, with the token, and the answer
/// back as it comes.
pub fn forward(
    tx: &Sender<Msg>,
    key: &str,
    request: &Request,
    stream: &TcpStream,
) -> Result<(), Answer> {
    let Some(mut up) = ask(tx, |answer| Ask::Upstream(key.to_owned(), answer)) else {
        return Err((404, "not found".into()));
    };
    if let Some(socket) = &up.socket {
        return forward_unix(socket, request, stream);
    }
    let signed_out = || (401, "sign in to this connector in Bate first".to_owned());
    let mut tokens = up.tokens.take().ok_or_else(signed_out)?;
    let renewed = |up: &Upstream, old: &Tokens| match oauth::renew(old) {
        Ok(new) => {
            keep(tx, up, old, Some(new.clone()));
            Ok(new)
        }
        Err(failed) => {
            // Not a reason to forget the sign-in when the server merely cannot be reached.
            if failed.refused {
                keep(tx, up, old, None);
            }
            let why = failed.why;
            Err((
                401,
                format!("the sign-in ran out and was not renewed: {why}"),
            ))
        }
    };
    if tokens.expires_ms.is_some_and(|at| at < now_ms() + EARLY_MS) {
        tokens = renewed(&up, &tokens)?;
    }
    let mut response = send(&up, &tokens, request)?;
    if response.status() == 401 {
        // The server ended the token early: once more with a new one.
        tokens = renewed(&up, &tokens)?;
        response = send(&up, &tokens, request)?;
    }
    let header = |name: &str| {
        let value = response.headers().get(name).and_then(|v| v.to_str().ok());
        value
            .map(|value| format!("{name}: {value}\r\n"))
            .unwrap_or_default()
    };
    // No length: the answer may be a stream. The end of the connection ends it.
    let head = format!(
        "HTTP/1.1 {} -\r\n{}{}connection: close\r\n\r\n",
        response.status().as_u16(),
        header("content-type"),
        header("mcp-session-id"),
    );
    let mut out: &TcpStream = stream;
    let _ = out.set_write_timeout(Some(CALL));
    let sent = out.write_all(head.as_bytes()).and_then(|()| {
        let mut body = response.into_body().into_reader().take(MAX_ANSWER);
        copy_flushing(&mut body, &mut out)
    });
    // The engine went away, or the server stopped mid-answer: nothing more to say to either.
    let _ = sent;
    Ok(())
}

/// Sends the engine's message to a project's program on its socket and its answer back, head
/// and all, as it comes. No token: the program is not reachable but through here.
fn forward_unix(socket: &Path, request: &Request, stream: &TcpStream) -> Result<(), Answer> {
    let down = |e: io::Error| (502, format!("cannot reach the project's connector: {e}"));
    let mut up = project_server::connect(socket).map_err(down)?;
    let mut head = format!(
        "POST /mcp HTTP/1.1\r\nhost: localhost\r\nconnection: close\r\ncontent-length: {}\r\n",
        request.body.len()
    );
    for name in PASSED {
        if let Some(value) = request.header(name) {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    head.push_str("\r\n");
    up.set_read_timeout(Some(CALL)).map_err(down)?;
    up.write_all(head.as_bytes())
        .and_then(|()| up.write_all(&request.body))
        .map_err(down)?;
    let mut out: &TcpStream = stream;
    let _ = out.set_write_timeout(Some(CALL));
    // The engine went away, or the program stopped mid-answer: nothing more to say to either.
    match copy_flushing(&mut up.take(MAX_ANSWER), &mut out) {
        Ok(0) => Err((502, "the project's connector sent no answer".into())),
        _ => Ok(()),
    }
}

/// Copies as the bytes come: an event of a stream must not wait for the next one.
fn copy_flushing(from: &mut impl Read, to: &mut impl Write) -> io::Result<u64> {
    let (mut buf, mut total) = ([0; 8192], 0);
    loop {
        match from.read(&mut buf)? {
            0 => return Ok(total),
            n => {
                to.write_all(&buf[..n])?;
                to.flush()?;
                total += n as u64;
            }
        }
    }
}

fn send(
    up: &Upstream,
    tokens: &Tokens,
    request: &Request,
) -> Result<ureq::http::Response<ureq::Body>, Answer> {
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(CALL))
        // A redirect would carry the token to another address.
        .max_redirects(0)
        .build();
    let mut post = ureq::Agent::new_with_config(config).post(&up.url);
    for (name, value) in &up.headers {
        post = post.header(name.as_str(), value.as_str());
    }
    for name in PASSED {
        if let Some(value) = request.header(name) {
            post = post.header(name, value);
        }
    }
    post.header("Authorization", format!("Bearer {}", tokens.access))
        .send(&request.body[..])
        .map_err(|e| (502, format!("cannot reach the server: {e}")))
}

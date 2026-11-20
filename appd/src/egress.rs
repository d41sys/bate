//! The network limit's service half: a proxy on a port of its own (HTTP `CONNECT` and plain
//! HTTP). A limited process is told to use it, and the sandbox (see `limit`) lets it reach
//! nothing else, so a tool that ignores the proxy settings gets its connection refused.
//!
//! Each limited process holds a key, sent as the proxy's user name. The service's thread
//! says which hosts that key may reach, so a change of the person's list takes effect on the
//! next connection.

use crate::attach::base64;
use crate::core::Msg;
use crate::hooks::Until;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

/// Connections served at once; more are dropped unanswered.
const MAX_OPEN: usize = 256;
const MAX_HEAD: u64 = 16 * 1024;
/// How long a request's head may take to arrive, and the service to answer.
const WAIT: Duration = Duration::from_secs(5);
/// How long a host may take to accept a connection.
const CONNECT: Duration = Duration::from_secs(10);
/// How long a connection may carry nothing, either way.
const IDLE: Duration = Duration::from_secs(600);

/// The hosts the holder of `key` may reach, asked of the service's thread; `None` if the key is
/// not one that is limited now.
pub struct Ask {
    pub key: String,
    pub answer: Sender<Option<Vec<String>>>,
}

/// One allowed host: `host`, or `*.domain` (its subdomains, not the domain itself), with a
/// port; with none, 80 and 443.
struct Rule {
    host: String,
    wild: bool,
    port: Option<u16>,
}

fn parse(text: &str) -> Result<Rule, String> {
    let bad = || {
        format!("`{text}` is not a host: write `host`, `*.domain`, with `:port` if one port only")
    };
    let text = text.to_ascii_lowercase();
    let (name, port) = match text.rsplit_once(':') {
        Some((name, port)) => {
            let port = port.parse().ok().filter(|p| *p > 0).ok_or_else(bad)?;
            (name, Some(port))
        }
        None => (text.as_str(), None),
    };
    let (host, wild) = match name.strip_prefix("*.") {
        Some(host) => (host, true),
        None => (name, false),
    };
    // shortcut: no IPv6 address literals; add them when a workplace needs one.
    let label =
        |l: &str| !l.is_empty() && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    match host.split('.').all(label) {
        true => Ok(Rule {
            host: host.to_owned(),
            wild,
            port,
        }),
        false => Err(bad()),
    }
}

/// Whether `text` is a valid host rule.
pub fn check(text: &str) -> Result<(), String> {
    parse(text).map(drop)
}

/// The rule of `rules` that lets `host:port` through, if there is one.
fn permit<'a>(rules: &'a [Rule], host: &str, port: u16) -> Option<&'a Rule> {
    rules.iter().find(|r| {
        let port_ok = r.port.map_or(port == 80 || port == 443, |p| p == port);
        let host_ok = match r.wild {
            true => (host.strip_suffix(&r.host))
                .is_some_and(|rest| rest.len() > 1 && rest.ends_with('.')),
            false => host == r.host,
        };
        port_ok && host_ok
    })
}

/// Starts listening and returns the port.
pub fn start(tx: Sender<Msg>) -> io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
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
                    let _ = reply(&stream, status, &line);
                }
                let _ = stream.shutdown(Shutdown::Both);
                OPEN.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    Ok(port)
}

fn reply(mut stream: &TcpStream, status: u16, line: &str) -> io::Result<()> {
    let line = line.replace(char::is_control, " ");
    let challenge = match status {
        407 => "proxy-authenticate: Basic realm=\"Bate\"\r\n",
        _ => "",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {line}\r\n{challenge}content-length: 0\r\nconnection: close\r\n\r\n"
    )
}

/// Serves one connection. An error is the answer still to be sent.
fn serve(stream: &TcpStream, tx: &Sender<Msg>) -> Result<(), (u16, String)> {
    let bad = |line: &str| (400, line.to_owned());
    let deadline = Instant::now() + WAIT;
    let mut reader = BufReader::new(Until(stream, deadline).take(MAX_HEAD));
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|_| bad("bad request"))?;
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let (method, target) = (method.to_owned(), target.to_owned());
    let (mut key, mut kept) = (None, String::new());
    loop {
        let mut header = String::new();
        let n = reader
            .read_line(&mut header)
            .map_err(|_| bad("bad request"))?;
        if n == 0 || !header.ends_with('\n') {
            return Err(bad("the request's head is cut short or longer than 16 KiB"));
        }
        if header.trim_end().is_empty() {
            break;
        }
        let Some((name, value)) = header.split_once(':') else {
            return Err(bad("bad header"));
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "proxy-authorization" => {
                let basic = value.trim().strip_prefix("Basic ");
                let user = basic
                    .and_then(|b| base64(b.trim()))
                    .map(|b| String::from_utf8_lossy(&b).into_owned());
                key = user.map(|u| u.split(':').next().unwrap_or("").to_owned());
            }
            // The request goes on as one for the host alone, and to the end of the connection.
            "proxy-connection" | "connection" | "keep-alive" => {}
            _ => kept.push_str(&header),
        }
    }
    let rules = {
        let (answer, answered) = mpsc::channel();
        let key = key.ok_or((407, "the proxy needs its key".to_owned()))?;
        tx.send(Msg::Egress(Ask { key, answer }))
            .map_err(|_| (503, "the service is stopping".to_owned()))?;
        let hosts = answered
            .recv_timeout(WAIT)
            .map_err(|_| (503, "the service is busy".to_owned()))?;
        hosts.ok_or((407, "the proxy does not know this key".to_owned()))?
    };
    let rules: Vec<Rule> = rules.iter().filter_map(|r| parse(r).ok()).collect();
    let connect = method.eq_ignore_ascii_case("CONNECT");
    let uri: ureq::http::Uri = target.parse().map_err(|_| bad("bad address"))?;
    let (host, port) = match connect {
        true => (uri.host(), uri.port_u16()),
        false if uri.scheme_str() == Some("http") => (uri.host(), uri.port_u16().or(Some(80))),
        // Another scheme in a proxy request has no way through: https goes by `CONNECT`.
        false => return Err((400, "use CONNECT for https".into())),
    };
    let (Some(host), Some(port)) = (host, port) else {
        return Err(bad("the request names no host and port"));
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let Some(rule) = permit(&rules, &host, port) else {
        return Err((
            403,
            format!("Bate's network limit: {host}:{port} is not an allowed host"),
        ));
    };
    let upstream = connect_to(&host, port, rule)
        .map_err(|e| (502, format!("cannot reach {host}:{port}: {e}")))?;
    let mut to = &upstream;
    if connect {
        let mut out = stream;
        out.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .map_err(|_| bad("gone"))?;
    } else {
        let path = uri.path_and_query().map_or("/", |p| p.as_str());
        write!(
            to,
            "{method} {path} HTTP/1.1\r\n{kept}connection: close\r\n\r\n"
        )
        .map_err(|e| (502, e.to_string()))?;
    }
    // What came with the head (a request's start, for a plain request) goes first.
    to.write_all(reader.buffer())
        .map_err(|e| (502, e.to_string()))?;
    pipe(stream, &upstream);
    Ok(())
}

/// Opens the connection. A name that only a wildcard matched is not taken to this Mac's own
/// ports: the service's other ports are not for a name someone else controls.
fn connect_to(host: &str, port: u16, rule: &Rule) -> io::Result<TcpStream> {
    let mut last = io::Error::other("the name has no address");
    for addr in (host, port).to_socket_addrs()? {
        if rule.wild && addr.ip().is_loopback() {
            continue;
        }
        match TcpStream::connect_timeout(&addr, CONNECT) {
            Ok(stream) => return Ok(stream),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Copies both ways until the host is done; a client that is done only ends its own half.
fn pipe(client: &TcpStream, host: &TcpStream) {
    let _ = (
        client.set_read_timeout(Some(IDLE)),
        host.set_read_timeout(Some(IDLE)),
    );
    let (Ok(mut from), Ok(mut to)) = (client.try_clone(), host.try_clone()) else {
        return;
    };
    thread::spawn(move || {
        let _ = io::copy(&mut from, &mut to);
        let _ = to.shutdown(Shutdown::Write);
    });
    let _ = io::copy(&mut &*host, &mut &*client);
    let _ = host.shutdown(Shutdown::Both);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed(rules: &[&str], host: &str, port: u16) -> bool {
        let rules: Vec<Rule> = rules.iter().map(|r| parse(r).unwrap()).collect();
        permit(&rules, host, port).is_some()
    }

    #[test]
    fn a_host_is_let_through_by_name_wildcard_and_port() {
        // No port: the web's two.
        assert!(allowed(&["example.com"], "example.com", 443));
        assert!(allowed(&["EXAMPLE.com"], "example.com", 80));
        assert!(!allowed(&["example.com"], "example.com", 8080));
        assert!(!allowed(&["example.com"], "www.example.com", 443));
        assert!(!allowed(&["example.com"], "badexample.com", 443));
        // A wildcard is the subdomains, not the domain.
        assert!(allowed(&["*.example.com"], "a.b.example.com", 443));
        assert!(!allowed(&["*.example.com"], "example.com", 443));
        assert!(!allowed(&["*.example.com"], "badexample.com", 443));
        // A port is that port only.
        assert!(allowed(&["127.0.0.1:8080"], "127.0.0.1", 8080));
        assert!(!allowed(&["127.0.0.1:8080"], "127.0.0.1", 443));
        assert!(!allowed(&[], "example.com", 443));
    }

    #[test]
    fn a_rule_that_is_not_a_host_is_refused() {
        for good in [
            "a.com",
            "*.a.com",
            "a.com:8080",
            "10.0.0.1",
            "localhost:3000",
        ] {
            assert!(check(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "*",
            "*.",
            "a..com",
            "a.com:",
            "a.com:0",
            "a.com:99999",
            "http://a.com",
            "a.com/x",
            "a b",
            "*a.com",
        ] {
            assert!(check(bad).is_err(), "{bad}");
        }
    }

    /// The engine's own HTTP client, told by the environment to use the proxy.
    #[test]
    fn the_engines_http_client_gets_through_for_its_key_and_hosts_only() {
        let (tx, rx) = mpsc::channel();
        let proxy = start(tx).unwrap();
        let serve = |listener: TcpListener| {
            thread::spawn(move || {
                for mut stream in listener.incoming().flatten() {
                    let _ = stream.read(&mut [0; 4096]);
                    let _ = stream.write_all(
                        b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok",
                    );
                }
            });
        };
        let (web, other) = (
            TcpListener::bind("127.0.0.1:0").unwrap(),
            TcpListener::bind("127.0.0.1:0").unwrap(),
        );
        let (web_port, other_port) = (
            web.local_addr().unwrap().port(),
            other.local_addr().unwrap().port(),
        );
        serve(web);
        serve(other);
        thread::spawn(move || {
            for msg in rx {
                if let Msg::Egress(ask) = msg {
                    let _ = ask
                        .answer
                        .send((ask.key == "k").then(|| vec![format!("127.0.0.1:{web_port}")]));
                }
            }
        });
        let status = |key: &str, port: u16| {
            let via = ureq::Proxy::new(&format!("http://{key}:x@127.0.0.1:{proxy}")).unwrap();
            let config = ureq::Agent::config_builder()
                .proxy(Some(via))
                .http_status_as_error(false)
                .build();
            let agent = ureq::Agent::new_with_config(config);
            agent
                .get(&format!("http://127.0.0.1:{port}/"))
                .call()
                .map(|r| r.status().as_u16())
        };
        assert_eq!(status("k", web_port).unwrap(), 200);
        // Neither is let through, whatever the client makes of the proxy's 403 and 407.
        assert_ne!(status("k", other_port).ok(), Some(200));
        assert_ne!(status("wrong", web_port).ok(), Some(200));
    }
}

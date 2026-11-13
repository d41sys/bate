//! The window's page for other devices on the local network, over HTTPS with a certificate Bate
//! makes itself. Off until the user turns it on. A device pairs once with a code shown in the
//! window (5 minutes, 5 tries) and keeps a cookie; each page it opens is relayed to the
//! service's socket as one more window, so it can do what the window can.

use crate::paths;
use crate::sys::{self, log, now_ms};
use proto::MAX_WINDOW_FRAME;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

/// The port when `APPD_LAN_PORT` does not name another (0: any free one).
const DEFAULT_PORT: u16 = 47822;
const CODE_LIFE: Duration = Duration::from_secs(5 * 60);
const CODE_TRIES: u8 = 5;
// shortcut: long-poll over HTTPS, upgrade to WebSocket if latency requires it
/// How long a page's `recv` waits for a line.
const POLL: Duration = Duration::from_secs(25);
/// A page that has not asked for lines this long is gone.
const IDLE: Duration = Duration::from_secs(90);
const MAX_HEAD: u64 = 8 * 1024;
/// Connections served at once; more are closed.
const MAX_OPEN: usize = 32;
/// Most bytes of lines one `recv` answers with (more wait for the next).
const MAX_BATCH: usize = 4 << 20;
const COOKIE: &str = "bate_device";

/// The window's page, built into the service (see `build.rs`).
mod page {
    include!(concat!(env!("OUT_DIR"), "/page.rs"));
}

/// The page's own policy, as in the window, with the service as the one place it talks to.
const PAGE_POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self'; \
    img-src 'self' data:; font-src 'self'; connect-src 'self'; object-src 'none'; \
    base-uri 'none'; form-action 'none'; frame-src 'none'; frame-ancestors 'none'";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Device {
    id: String,
    name: String,
    /// SHA-256 of its cookie, in hex; the cookie itself is not kept.
    hash: String,
    created_at: u64,
    last_seen: u64,
}

struct Code {
    code: String,
    until: Instant,
    tries: u8,
}

/// One page open on a device: its own connection to the service's socket.
struct Session {
    device: String,
    to: Mutex<UnixStream>,
    tx: mpsc::Sender<String>,
    lines: Mutex<Receiver<String>>,
    seen: Mutex<Instant>,
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = lock(&self.to).shutdown(Shutdown::Both);
    }
}

#[derive(Default)]
struct State {
    /// Set while serving; cleared to stop the listener.
    on: Option<Arc<AtomicBool>>,
    port: u16,
    fingerprint: String,
    code: Option<Code>,
    devices: Vec<Device>,
    sessions: HashMap<String, Arc<Session>>,
}

pub struct Lan(Mutex<State>);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn dir() -> PathBuf {
    paths::home().join("lan")
}

fn sha256(bytes: &[u8]) -> Vec<u8> {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .to_vec()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Lan {
    /// Loads the paired devices, and serves if the user left it on.
    pub fn start() -> Arc<Lan> {
        let devices = fs::read(dir().join("devices.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let lan = Arc::new(Lan(Mutex::new(State {
            devices,
            ..State::default()
        })));
        if dir().join("on").exists()
            && let Err(e) = lan.set(true)
        {
            log!("the page for other devices is off: {e}");
        }
        lan
    }

    /// What the window shows: whether it is on, where, the certificate, and the devices.
    pub fn status(&self) -> Value {
        let state = lock(&self.0);
        let devices: Vec<Value> = state
            .devices
            .iter()
            .map(|d| json!({"id": d.id, "name": d.name, "createdAt": d.created_at, "lastSeen": d.last_seen}))
            .collect();
        match state.on {
            Some(_) => json!({
                "on": true,
                "port": state.port,
                "urls": addresses().iter().map(|ip| format!("https://{ip}:{}/", state.port)).collect::<Vec<_>>(),
                "fingerprint": state.fingerprint,
                "devices": devices,
            }),
            None => json!({"on": false, "devices": devices}),
        }
    }

    /// Turns serving on or off, and remembers it.
    pub fn set(self: &Arc<Self>, on: bool) -> Result<(), String> {
        let mut state = lock(&self.0);
        if let Some(flag) = state.on.take() {
            flag.store(false, Ordering::Relaxed);
            state.sessions.clear();
            state.code = None;
        }
        let mark = dir().join("on");
        if !on {
            let _ = fs::remove_file(mark);
            return Ok(());
        }
        sys::create_private_dir(&dir()).map_err(|e| e.to_string())?;
        let (config, fingerprint) = tls().map_err(|e| format!("no certificate: {e}"))?;
        let port = match std::env::var("APPD_LAN_PORT") {
            Ok(p) => p
                .parse()
                .map_err(|_| "APPD_LAN_PORT is not a port number")?,
            Err(_) => DEFAULT_PORT,
        };
        let listener = TcpListener::bind(("0.0.0.0", port)).map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        state.port = listener.local_addr().map_err(|e| e.to_string())?.port();
        state.fingerprint = fingerprint;
        let flag = Arc::new(AtomicBool::new(true));
        state.on = Some(flag.clone());
        let _ = fs::write(mark, "");
        let lan = self.clone();
        thread::spawn(move || lan.accept(listener, Arc::new(config), flag));
        Ok(())
    }

    /// A new pairing code, in place of any other.
    pub fn pair_code(&self) -> Result<Value, String> {
        let mut state = lock(&self.0);
        if state.on.is_none() {
            return Err("turn on the page for other devices first".into());
        }
        let mut n = [0u8; 4];
        // SAFETY: the pointer and length describe `n`.
        unsafe { libc::arc4random_buf(n.as_mut_ptr().cast(), n.len()) };
        let code = format!("{:06}", u32::from_le_bytes(n) % 1_000_000);
        state.code = Some(Code {
            code: code.clone(),
            until: Instant::now() + CODE_LIFE,
            tries: 0,
        });
        Ok(json!({"code": code, "expiresAt": now_ms() + CODE_LIFE.as_millis() as u64}))
    }

    /// Forgets a device; its open pages lose their connection at once.
    pub fn remove(&self, device: &str) -> Result<(), String> {
        let mut state = lock(&self.0);
        let before = state.devices.len();
        state.devices.retain(|d| d.id != device);
        if state.devices.len() == before {
            return Err(format!("no device `{device}`"));
        }
        state.sessions.retain(|_, s| s.device != device);
        save(&state.devices)
    }

    fn accept(
        self: Arc<Self>,
        listener: TcpListener,
        config: Arc<ServerConfig>,
        on: Arc<AtomicBool>,
    ) {
        static OPEN: AtomicUsize = AtomicUsize::new(0);
        let mut swept = Instant::now();
        while on.load(Ordering::Relaxed) {
            if swept.elapsed() > Duration::from_secs(10) {
                swept = Instant::now();
                lock(&self.0)
                    .sessions
                    .retain(|_, s| lock(&s.seen).elapsed() < IDLE);
            }
            let stream = match listener.accept() {
                Ok((stream, _)) => stream,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(100));
                    continue;
                }
                Err(e) => {
                    log!("the page for other devices: accept failed: {e}");
                    thread::sleep(Duration::from_millis(100));
                    continue;
                }
            };
            if OPEN.fetch_add(1, Ordering::SeqCst) >= MAX_OPEN {
                OPEN.fetch_sub(1, Ordering::SeqCst);
                continue;
            }
            let (lan, config, on) = (self.clone(), config.clone(), on.clone());
            thread::spawn(move || {
                let _ = lan.connection(stream, config, &on);
                OPEN.fetch_sub(1, Ordering::SeqCst);
            });
        }
    }

    fn connection(
        &self,
        tcp: TcpStream,
        config: Arc<ServerConfig>,
        on: &AtomicBool,
    ) -> io::Result<()> {
        tcp.set_nonblocking(false)?;
        tcp.set_read_timeout(Some(Duration::from_secs(10)))?;
        tcp.set_write_timeout(Some(Duration::from_secs(10)))?;
        let tls = ServerConnection::new(config).map_err(io::Error::other)?;
        let mut stream = StreamOwned::new(tls, tcp);
        let answer = match read(&mut stream) {
            Ok(request) if on.load(Ordering::Relaxed) => self.answer(&request),
            Ok(_) => return Ok(()),
            Err(answer) => answer,
        };
        answer.send(&mut stream)?;
        stream.conn.send_close_notify();
        let _ = stream.flush();
        Ok(())
    }

    fn answer(&self, r: &Request) -> Answer {
        let api = r.path.starts_with("/api/");
        if !api {
            return match r.method.as_str() {
                "GET" | "HEAD" => static_file(&r.path),
                _ => Answer::error(405, "use GET"),
            };
        }
        // Only the page itself sends this; a page of another site cannot without asking.
        if r.method != "POST" && r.path != "/api/recv" || r.header("x-bate") != Some("1") {
            return Answer::error(403, "not from Bate's page");
        }
        if r.path == "/api/pair" {
            return self.pair(r);
        }
        let Some(device) = self.device(r) else {
            return Answer::error(401, "pair this device first");
        };
        let session = query(&r.query, "session");
        match r.path.as_str() {
            "/api/session" => self.open_session(&device),
            "/api/send" | "/api/recv" => {
                let found = lock(&self.0).sessions.get(session).cloned();
                let Some(s) = found.filter(|s| s.device == device) else {
                    return Answer::error(410, "this page's connection is gone");
                };
                if r.path == "/api/send" {
                    send(&s, &r.body)
                } else {
                    recv(&s)
                }
            }
            _ => Answer::error(404, "not found"),
        }
    }

    /// The device whose cookie came with `r`, seen now.
    fn device(&self, r: &Request) -> Option<String> {
        let cookies = r.header("cookie")?;
        let token = cookies
            .split(';')
            .find_map(|c| c.trim().strip_prefix(&format!("{COOKIE}=")))?;
        let hash = hex(&sha256(token.as_bytes()));
        let mut state = lock(&self.0);
        let device = state.devices.iter_mut().find(|d| d.hash == hash)?;
        device.last_seen = now_ms();
        Some(device.id.clone())
    }

    fn pair(&self, r: &Request) -> Answer {
        let Ok(asked) = serde_json::from_slice::<Value>(&r.body) else {
            return Answer::error(400, "send {code, name}");
        };
        let given = asked["code"].as_str().unwrap_or("");
        let name: String = asked["name"]
            .as_str()
            .unwrap_or("")
            .trim()
            .chars()
            .take(60)
            .collect();
        let mut state = lock(&self.0);
        let Some(code) = state.code.as_mut().filter(|c| c.until > Instant::now()) else {
            state.code = None;
            return Answer::error(403, "no pairing code is open: make one in the window");
        };
        // Compared whole, so the time taken tells nothing.
        let same = code.code.len() == given.len()
            && code
                .code
                .bytes()
                .zip(given.bytes())
                .fold(0, |d, (a, b)| d | (a ^ b))
                == 0;
        if !same {
            code.tries += 1;
            if code.tries >= CODE_TRIES {
                state.code = None;
            }
            return Answer::error(403, "that is not the code");
        }
        state.code = None;
        let token = sys::random_hex(32);
        let device = Device {
            id: sys::random_hex(4),
            name: if name.is_empty() {
                "A device".into()
            } else {
                name
            },
            hash: hex(&sha256(token.as_bytes())),
            created_at: now_ms(),
            last_seen: now_ms(),
        };
        state.devices.push(device);
        if let Err(e) = save(&state.devices) {
            return Answer::error(500, &e);
        }
        let mut answer = Answer::json(200, &json!({}));
        answer.headers.push((
            "set-cookie",
            format!(
                "{COOKIE}={token}; Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age=31536000"
            ),
        ));
        answer
    }

    fn open_session(&self, device: &str) -> Answer {
        let stream = match UnixStream::connect(paths::socket()) {
            Ok(s) => s,
            Err(e) => return Answer::error(503, &format!("the service is not there: {e}")),
        };
        let Ok(reader) = stream.try_clone() else {
            return Answer::error(500, "no connection");
        };
        let (tx, lines) = mpsc::channel();
        let loop_tx = tx.clone();
        thread::spawn(move || {
            for line in BufReader::new(reader).lines() {
                let Ok(line) = line else { break };
                if loop_tx.send(line).is_err() {
                    break;
                }
            }
        });
        let id = sys::random_hex(16);
        let session = Session {
            device: device.to_owned(),
            to: Mutex::new(stream),
            tx,
            lines: Mutex::new(lines),
            seen: Mutex::new(Instant::now()),
        };
        lock(&self.0).sessions.insert(id.clone(), Arc::new(session));
        Answer::json(200, &json!({"session": id}))
    }
}

/// A paired device may do what the window does, but not change network settings or add devices.
const FORBIDDEN_OVER_LAN: &[&str] = &[
    "lan_status",
    "set_lan",
    "pair_device",
    "remove_device",
    "get_provider",
    "set_provider",
];

fn send(s: &Session, body: &[u8]) -> Answer {
    if body.contains(&b'\n') || body.contains(&b'\r') {
        return Answer::error(400, "one line, with no line break");
    }
    if let Ok(f) = serde_json::from_slice::<Value>(body)
        && let Some(cmd) = f.get("type").and_then(Value::as_str)
        && FORBIDDEN_OVER_LAN.contains(&cmd)
    {
        let id = f.get("id").cloned().unwrap_or(Value::Null);
        let error = json!({
            "type": "response",
            "id": id,
            "command": cmd,
            "success": false,
            "code": "forbidden",
            "error": "only Bate's own window can do this"
        });
        let _ = s.tx.send(error.to_string());
        return Answer::json(200, &json!({}));
    }
    let mut to = lock(&s.to);
    match to.write_all(body).and_then(|()| to.write_all(b"\n")) {
        Ok(()) => Answer::json(200, &json!({})),
        Err(e) => Answer::error(410, &format!("not sent: {e}")),
    }
}

/// The lines the service sent, waiting up to `POLL` for the first.
fn recv(s: &Session) -> Answer {
    *lock(&s.seen) = Instant::now();
    let lines = lock(&s.lines);
    let mut batch = Vec::new();
    match lines.recv_timeout(POLL) {
        Ok(line) => batch.push(line),
        Err(RecvTimeoutError::Timeout) => {}
        Err(RecvTimeoutError::Disconnected) => {
            return Answer::error(410, "the service closed this page's connection");
        }
    }
    let mut bytes = batch.first().map_or(0, String::len);
    while bytes < MAX_BATCH
        && let Ok(line) = lines.try_recv()
    {
        bytes += line.len();
        batch.push(line);
    }
    *lock(&s.seen) = Instant::now();
    // Each is a JSON line already: an array of them is JSON too.
    let body = format!("[{}]", batch.join(","));
    Answer {
        status: 200,
        headers: vec![("content-type", "application/json".into())],
        body: body.into_bytes(),
    }
}

fn save(devices: &[Device]) -> Result<(), String> {
    let path = dir().join("devices.json");
    let tmp = path.with_extension("tmp");
    let bytes = serde_json::to_vec(devices).map_err(|e| e.to_string())?;
    fs::write(&tmp, bytes)
        .and_then(|()| fs::rename(&tmp, &path))
        .map_err(|e| format!("devices not saved: {e}"))
}

/// The certificate (made on first use, kept in `lan/`) and its SHA-256 fingerprint.
fn tls() -> io::Result<(ServerConfig, String)> {
    let (cert, key) = (dir().join("cert.pem"), dir().join("key.pem"));
    if !cert.exists() || !key.exists() {
        let host = sys::hostname();
        let made = std::process::Command::new("/usr/bin/openssl")
            .args([
                "req",
                "-x509",
                "-newkey",
                "ec",
                "-pkeyopt",
                "ec_paramgen_curve:prime256v1",
            ])
            // A named curve: LibreSSL writes the curve out in full otherwise, which TLS libraries refuse.
            .args(["-pkeyopt", "ec_param_enc:named_curve"])
            .args(["-nodes", "-days", "3650", "-subj", "/CN=Bate"])
            .arg("-addext")
            .arg(format!("subjectAltName=DNS:{host}.local"))
            .arg("-keyout")
            .arg(&key)
            .arg("-out")
            .arg(&cert)
            .output()?;
        if !made.status.success() {
            return Err(io::Error::other(
                String::from_utf8_lossy(&made.stderr).trim().to_owned(),
            ));
        }
    }
    let certs: Vec<CertificateDer> = CertificateDer::pem_file_iter(&cert)
        .map_err(io::Error::other)?
        .collect::<Result<_, _>>()
        .map_err(io::Error::other)?;
    let first = certs
        .first()
        .ok_or_else(|| io::Error::other("no certificate in cert.pem"))?;
    let fingerprint = sha256(first)
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":");
    let key = PrivateKeyDer::from_pem_file(&key).map_err(io::Error::other)?;
    let config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(io::Error::other)?
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(io::Error::other)?;
    Ok((config, fingerprint))
}

// shortcut: IPv4 addresses only, upgrade if IPv6 local addresses are requested
/// This Mac's IPv4 addresses on its networks.
fn addresses() -> Vec<String> {
    let mut found = Vec::new();
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: `getifaddrs` fills `list`, read here and freed once.
    unsafe {
        if libc::getifaddrs(&mut list) != 0 {
            return found;
        }
        let mut at = list;
        while !at.is_null() {
            let addr = (*at).ifa_addr;
            if !addr.is_null() && (*addr).sa_family as i32 == libc::AF_INET {
                let ip = (*(addr as *const libc::sockaddr_in)).sin_addr.s_addr;
                let ip = std::net::Ipv4Addr::from(u32::from_be(ip));
                if !ip.is_loopback() && !ip.is_link_local() {
                    found.push(ip.to_string());
                }
            }
            at = (*at).ifa_next;
        }
        libc::freeifaddrs(list);
    }
    found
}

fn query<'a>(query: &'a str, name: &str) -> &'a str {
    query
        .split('&')
        .find_map(|p| p.strip_prefix(name)?.strip_prefix('='))
        .unwrap_or("")
}

fn static_file(path: &str) -> Answer {
    let path = if path == "/" { "/index.html" } else { path };
    let Some((_, body)) = page::FILES.iter().find(|(p, _)| *p == path) else {
        return match page::FILES.is_empty() {
            true => Answer::error(503, "the page was not built into this service"),
            false => Answer::error(404, "not found"),
        };
    };
    let kind = match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    };
    Answer {
        status: 200,
        headers: vec![
            ("content-type", kind.into()),
            ("content-security-policy", PAGE_POLICY.into()),
        ],
        body: body.to_vec(),
    }
}

struct Request {
    method: String,
    path: String,
    query: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

struct Answer {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

impl Answer {
    fn json(status: u16, value: &Value) -> Answer {
        Answer {
            status,
            headers: vec![("content-type", "application/json".into())],
            body: value.to_string().into_bytes(),
        }
    }

    fn error(status: u16, line: &str) -> Answer {
        Answer::json(status, &json!({"error": line}))
    }

    fn send(&self, out: &mut impl Write) -> io::Result<()> {
        let mut head = format!(
            "HTTP/1.1 {} -\r\ncontent-length: {}\r\nconnection: close\r\ncache-control: no-store\r\n\
             x-content-type-options: nosniff\r\nreferrer-policy: no-referrer\r\nx-frame-options: DENY\r\n",
            self.status,
            self.body.len()
        );
        for (name, value) in &self.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("\r\n");
        out.write_all(head.as_bytes())?;
        out.write_all(&self.body)
    }
}

/// One request, read whole; an error is the answer to send instead.
fn read(stream: &mut impl Read) -> Result<Request, Answer> {
    let bad = |line: &str| Answer::error(400, line);
    let mut reader = BufReader::new(stream.take(MAX_HEAD));
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
            return Err(bad("the request's head is cut short or too long"));
        }
        if header.trim_end().is_empty() {
            break;
        }
        let Some((name, value)) = header.split_once(':') else {
            return Err(bad("bad header"));
        };
        let (name, value) = (name.trim().to_ascii_lowercase(), value.trim().to_owned());
        match name.as_str() {
            "content-length" => length = value.parse().map_err(|_| bad("bad content-length"))?,
            "transfer-encoding" => return Err(Answer::error(411, "send a content-length")),
            _ => {}
        }
        headers.push((name, value));
    }
    if length >= MAX_WINDOW_FRAME {
        return Err(Answer::error(413, "too long"));
    }
    let mut body = reader.buffer().to_vec();
    body.truncate(length);
    let mut rest = vec![0; length - body.len()];
    reader
        .into_inner()
        .into_inner()
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

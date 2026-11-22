//! A web browser per bot: an installed Chrome (or Chromium) with a profile of the bot's own,
//! in its own window, which the service drives over the DevTools protocol. The bot asks for
//! actions through its engine (`browser` events); only those few actions exist, so nothing
//! the bot runs reaches the browser any other way. The user may click or type in the window:
//! that takes the browser over, and the bot's next action waits until the user hands it back.

use crate::core::Msg;
use crate::sys::now_ms;
use serde_json::{Map, Value, json};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

/// How long Chrome may take over one DevTools call.
const CALL: Duration = Duration::from_secs(30);
/// How long a page may take to load after an action.
const LOAD_MS: u64 = 15_000;
/// The keys `press` knows: the name the bot gives, the key's virtual code, and the text it types.
static KEYS: [(&str, u32, &str); 10] = [
    ("Enter", 13, "\r"),
    ("Tab", 9, ""),
    ("Escape", 27, ""),
    ("Backspace", 8, ""),
    ("ArrowLeft", 37, ""),
    ("ArrowUp", 38, ""),
    ("ArrowRight", 39, ""),
    ("ArrowDown", 40, ""),
    ("PageUp", 33, ""),
    ("PageDown", 34, ""),
];
/// Most characters of a page's text the bot is given.
const MAX_TEXT: usize = 20_000;
/// Most elements listed.
const MAX_ELEMENTS: usize = 200;
/// Most characters typed in one action.
const MAX_TYPED: usize = 10_000;
/// Most bytes of one message from Chrome.
const MAX_MESSAGE: usize = 64 << 20;
/// The name under which the page tells the service that the user clicked or typed.
const BINDING: &str = "bateUser";
/// The page's isolated world, where the page's own scripts cannot see or call the binding.
const WORLD: &str = "bate";
/// Runs in the isolated world of every page: input that a person made (not a script) is
/// the user taking over.
const WATCH: &str = "for (const t of ['pointerdown', 'mousedown', 'keydown', 'touchstart']) \
    addEventListener(t, e => { if (e.isTrusted) bateUser(t) }, true);";

/// What a `type` action puts in a field.
#[derive(Debug, PartialEq)]
pub enum Typed {
    Text(String),
    /// The name of a secret of the bot; the service types its value, only on its site.
    Secret(String),
}

/// What the bot may do. Checked here: the engine is the bot's.
#[derive(Debug, PartialEq)]
pub enum Action {
    Open(String),
    Look,
    Click(u32),
    Type {
        element: u32,
        text: Typed,
        submit: bool,
    },
    Back,
    Forward,
    Reload,
    /// One screen up or down.
    Scroll {
        down: bool,
    },
    /// A key, after focusing `element` when one is given.
    Press {
        key: &'static str,
        element: Option<u32>,
    },
    /// The option of a dropdown with this visible text.
    Select {
        element: u32,
        option: String,
    },
    Wait(u64),
}

impl Action {
    pub fn parse(args: &Map<String, Value>) -> Result<Action, String> {
        let element = || match args.get("element").and_then(Value::as_u64) {
            Some(n @ 1..=10_000) => Ok(n as u32),
            _ => Err("`element` must be the number of an element on the page".to_owned()),
        };
        match args.get("action").and_then(Value::as_str) {
            Some("open") => {
                let url = args.get("url").and_then(Value::as_str).unwrap_or("").trim();
                if !web(url) {
                    return Err("`url` must be an http or https address".into());
                }
                Ok(Action::Open(url.to_owned()))
            }
            Some("look") => Ok(Action::Look),
            Some("click") => Ok(Action::Click(element()?)),
            Some("type") => {
                let typed = match (args.get("text"), args.get("secret")) {
                    (Some(_), Some(_)) => return Err("give `text` or `secret`, not both".into()),
                    (_, Some(name)) => match name.as_str() {
                        Some(name) if proto::secrets::name_ok(name) => Typed::Secret(name.to_owned()),
                        _ => return Err("`secret` must be the name of a secret you were given".into()),
                    },
                    (text, None) => {
                        let text = text.and_then(Value::as_str).unwrap_or("");
                        if text.chars().count() > MAX_TYPED {
                            return Err(format!("`text` may be {MAX_TYPED} characters"));
                        }
                        Typed::Text(text.to_owned())
                    }
                };
                let submit = args.get("submit").and_then(Value::as_bool).unwrap_or(false);
                Ok(Action::Type {
                    element: element()?,
                    text: typed,
                    submit,
                })
            }
            Some("back") => Ok(Action::Back),
            Some("forward") => Ok(Action::Forward),
            Some("reload") => Ok(Action::Reload),
            Some("scroll") => match args.get("direction").and_then(Value::as_str) {
                Some("up") => Ok(Action::Scroll { down: false }),
                Some("down") => Ok(Action::Scroll { down: true }),
                _ => Err("`direction` must be up or down".into()),
            },
            Some("press") => {
                let name = args.get("key").and_then(Value::as_str).unwrap_or("");
                let Some(&(key, ..)) = KEYS.iter().find(|k| k.0 == name) else {
                    let names: Vec<_> = KEYS.iter().map(|k| k.0).collect();
                    return Err(format!("`key` must be one of {}", names.join(", ")));
                };
                let element = match args.get("element") {
                    None | Some(Value::Null) => None,
                    Some(_) => Some(element()?),
                };
                Ok(Action::Press { key, element })
            }
            Some("select") => {
                let element = element()?;
                let option = args.get("option").and_then(Value::as_str).unwrap_or("").trim();
                if option.is_empty() || option.chars().count() > 200 {
                    return Err("`option` must be the visible text of an option, at most 200 characters".into());
                }
                Ok(Action::Select {
                    element,
                    option: option.to_owned(),
                })
            }
            Some("wait") => match args.get("seconds").and_then(Value::as_u64) {
                Some(seconds @ 1..=10) => Ok(Action::Wait(seconds)),
                _ => Err("`seconds` must be a whole number from 1 to 10".into()),
            },
            _ => Err("`action` must be open, look, click, type, scroll, press, select, back, forward, reload or wait".into()),
        }
    }
}

/// An address the browser may be sent to: a web page, never a file of this Mac or the
/// browser's own pages.
fn web(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let odd = url.chars().any(|c| c.is_whitespace() || c.is_control());
    (lower.starts_with("http://") || lower.starts_with("https://")) && !odd && url.len() <= 8192
}

/// The Chrome to run: `$APPD_CHROME`, else Google Chrome or Chromium in `/Applications`.
fn program() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("APPD_CHROME").filter(|p| !p.is_empty()) {
        return Some(path.into());
    }
    [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|path| path.exists())
}

/// A secret of a bot as the browser needs it. No `Debug`: a value must not get into a log.
pub struct Secret {
    pub name: String,
    /// The host it may be typed on.
    pub site: String,
    pub value: String,
}

/// `value` as a form or an address writes it: letters, digits and a few marks as they are,
/// the rest as `%XX`, and a space as `+` in a form.
fn percent(value: &str, form: bool) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        match b {
            b'*' if form => out.push('*'),
            b'~' if !form => out.push('~'),
            b' ' if form => out.push('+'),
            b'-' | b'.' | b'_' => out.push(b as char),
            b if b.is_ascii_alphanumeric() => out.push(b as char),
            b => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `text` with each secret's value, as it is and as a form or an address would write it,
/// replaced by a marker: text that goes back to the model must not hold one.
// shortcut: other forms (base64, a value split by markup) are not recognised; upgrade if a
// page is seen to show one back.
pub fn redact(text: &str, secrets: &[Secret]) -> String {
    let mut longest_first: Vec<&Secret> = secrets.iter().filter(|s| !s.value.is_empty()).collect();
    longest_first.sort_by_key(|s| std::cmp::Reverse(s.value.len()));
    let mut text = text.to_owned();
    for s in longest_first {
        let marker = proto::secrets::marker(&s.name);
        for form in [
            s.value.clone(),
            percent(&s.value, true),
            percent(&s.value, false),
        ] {
            text = text.replace(&form, &marker);
        }
    }
    text
}

/// One action, and whom to tell what came of it.
pub struct Job {
    pub request: String,
    pub generation: u64,
    pub action: Action,
    /// Set when the engine no longer waits (its turn ended): a job not begun is dropped.
    pub cancelled: Arc<AtomicBool>,
    /// The bot's secrets with their values: for a `type` that names one, and for hiding any
    /// of them that a page shows back.
    pub secrets: Vec<Secret>,
}

enum Work {
    Act(Job),
    HandBack,
}

/// The service's side of a bot's browser: the thread that drives it runs until the service
/// stops.
pub struct Handle {
    work: Sender<Work>,
    taken: Arc<AtomicBool>,
}

impl Handle {
    /// `proxy`: the port of the proxy that holds the browser to the bot's hosts, for a bot
    /// with a network limit.
    pub fn start(bot: &str, profile: PathBuf, tx: Sender<Msg>, proxy: Option<u16>) -> Handle {
        let (work, jobs) = mpsc::channel();
        let taken = Arc::new(AtomicBool::new(false));
        let mut browser = Browser {
            bot: bot.to_owned(),
            profile,
            tx,
            taken: taken.clone(),
            proxy,
            chrome: None,
        };
        thread::spawn(move || browser.serve(jobs));
        Handle { work, taken }
    }

    pub fn act(&self, job: Job) {
        let _ = self.work.send(Work::Act(job));
    }

    /// Whether the user has the browser.
    pub fn taken(&self) -> bool {
        self.taken.load(Ordering::SeqCst)
    }

    /// The user hands the browser back: a waiting action goes on.
    pub fn hand_back(&self) {
        if self.taken.swap(false, Ordering::SeqCst) {
            let _ = self.work.send(Work::HandBack);
        }
    }
}

/// A running Chrome, and the page it is driven on.
struct Chrome {
    child: Child,
    to: File,
    replies: Receiver<Value>,
    /// Set by the reader when Chrome closed its end.
    gone: Arc<AtomicBool>,
    next: u64,
    /// The page's DevTools session, once attached.
    page: Option<String>,
    /// Set while the service itself presses a key: Chrome makes that a person's input.
    acting: Arc<AtomicBool>,
}

struct Browser {
    bot: String,
    profile: PathBuf,
    tx: Sender<Msg>,
    taken: Arc<AtomicBool>,
    proxy: Option<u16>,
    chrome: Option<Chrome>,
}

impl Browser {
    fn serve(&mut self, jobs: Receiver<Work>) {
        let mut waiting: Option<Job> = None;
        for work in jobs {
            match work {
                Work::Act(job) if self.taken.load(Ordering::SeqCst) => {
                    // The user has the browser: this waits. A newer action replaces an older
                    // one, whose turn has ended.
                    waiting = Some(job);
                    continue;
                }
                Work::Act(job) => self.run(job),
                Work::HandBack => {
                    if let Some(job) = waiting.take() {
                        self.run(job);
                    }
                }
            }
        }
        if let Some(mut chrome) = self.chrome.take() {
            let _ = chrome.child.kill();
            let _ = chrome.child.wait();
        }
    }

    fn run(&mut self, job: Job) {
        if job.cancelled.load(Ordering::SeqCst) {
            return;
        }
        let result = self
            .act(&job.action, &job.secrets)
            .map(|page| redact(&page, &job.secrets))
            .map_err(|why| redact(&why, &job.secrets));
        let _ = self.tx.send(Msg::Browsed {
            bot: self.bot.clone(),
            generation: job.generation,
            request: job.request,
            result,
        });
    }

    fn act(&mut self, action: &Action, secrets: &[Secret]) -> Result<String, String> {
        let gone = self
            .chrome
            .as_ref()
            .is_some_and(|c| c.gone.load(Ordering::SeqCst));
        if gone && let Some(mut chrome) = self.chrome.take() {
            let _ = chrome.child.kill();
            let _ = chrome.child.wait();
        }
        if self.chrome.is_none() {
            self.chrome = Some(self.launch()?);
        }
        let chrome = self.chrome.as_mut().expect("launched above");
        // The page may have been closed by the user: then another is found or opened, once.
        let page = match chrome.page() {
            Ok(page) => page,
            Err(_) => {
                chrome.page = None;
                chrome.page()?
            }
        };
        chrome.act(&page, action, secrets)
    }

    fn launch(&self) -> Result<Chrome, String> {
        let program = program().ok_or(
            "No Chrome or Chromium is installed in /Applications, so there is no browser. Bate does not install one.",
        )?;
        std::fs::create_dir_all(&self.profile)
            .map_err(|e| format!("cannot make the browser's folder: {e}"))?;
        // Until Chrome has its ends and they are closed here, no other program may start:
        // it would keep them, and with them the whole browser.
        let spawning = crate::sys::SPAWN.lock().unwrap_or_else(|e| e.into_inner());
        let (chrome_in, to_chrome) = pipe().map_err(|e| e.to_string())?;
        let (from_chrome, chrome_out) = pipe().map_err(|e| e.to_string())?;
        let mut command = Command::new(&program);
        command
            .arg(format!("--user-data-dir={}", self.profile.display()))
            .args([
                "--remote-debugging-pipe",
                "--no-first-run",
                "--no-default-browser-check",
                "about:blank",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // A bot with a network limit: every request of the browser goes to the proxy that holds
        // it to the bot's hosts (`<-loopback>`: this Mac's own addresses too, which Chrome
        // would otherwise reach direct), and no peer-to-peer way round it.
        if let Some(port) = self.proxy {
            command.args([
                format!("--proxy-server=http://127.0.0.1:{port}"),
                "--proxy-bypass-list=<-loopback>".to_owned(),
                "--force-webrtc-ip-handling-policy=disable_non_proxied_udp".to_owned(),
            ]);
        }
        let (fd_in, fd_out) = (chrome_in, chrome_out);
        // SAFETY: only async-signal-safe calls between fork and exec.
        unsafe {
            command.pre_exec(move || {
                // Chrome reads its commands on 3 and writes on 4. Copied high first, so
                // neither copy overwrites the other.
                let (a, b) = (
                    libc::fcntl(fd_in, libc::F_DUPFD, 10),
                    libc::fcntl(fd_out, libc::F_DUPFD, 10),
                );
                if a < 0 || b < 0 || libc::dup2(a, 3) < 0 || libc::dup2(b, 4) < 0 {
                    return Err(io::Error::last_os_error());
                }
                libc::close(a);
                libc::close(b);
                Ok(())
            });
        }
        let spawned = command.spawn();
        // SAFETY: the child's ends, which this process no longer needs.
        unsafe {
            libc::close(fd_in);
            libc::close(fd_out);
        }
        drop(spawning);
        let child = spawned.map_err(|e| format!("cannot start {}: {e}", program.display()))?;
        // SAFETY: fresh descriptors from `pipe`, owned from here on.
        let (to, from) = unsafe { (File::from_raw_fd(to_chrome), File::from_raw_fd(from_chrome)) };
        let (replies_tx, replies) = mpsc::channel();
        let gone = Arc::new(AtomicBool::new(false));
        let acting = Arc::new(AtomicBool::new(false));
        let (taken, tx, bot, ended, quiet) = (
            self.taken.clone(),
            self.tx.clone(),
            self.bot.clone(),
            gone.clone(),
            acting.clone(),
        );
        thread::spawn(move || {
            let mut reader = BufReader::new(from);
            let mut message = Vec::new();
            loop {
                message.clear();
                let read = (&mut reader)
                    .take(MAX_MESSAGE as u64)
                    .read_until(0, &mut message);
                match read {
                    Ok(n) if n > 0 && message.last() == Some(&0) => {}
                    // Too long (a page may log anything): passed over, not the end.
                    Ok(n) if n == MAX_MESSAGE => {
                        if reader.skip_until(0).is_ok_and(|n| n > 0) {
                            continue;
                        }
                        ended.store(true, Ordering::SeqCst);
                        return;
                    }
                    _ => {
                        ended.store(true, Ordering::SeqCst);
                        return;
                    }
                }
                let Ok(message) = serde_json::from_slice::<Value>(&message[..message.len() - 1])
                else {
                    continue;
                };
                if message["method"] == "Runtime.bindingCalled"
                    && message["params"]["name"] == BINDING
                {
                    if !quiet.load(Ordering::SeqCst) && !taken.swap(true, Ordering::SeqCst) {
                        let _ = tx.send(Msg::BrowserTaken { bot: bot.clone() });
                    }
                } else if message.get("id").is_some() && replies_tx.send(message).is_err() {
                    return;
                }
            }
        });
        let mut chrome = Chrome {
            child,
            to,
            replies,
            gone,
            next: 0,
            page: None,
            acting,
        };
        // No downloads: Chrome is not kept to the bot's folder, and would write wherever the
        // folder's links point.
        let kept = chrome.call(
            None,
            "Browser.setDownloadBehavior",
            json!({"behavior": "deny"}),
        );
        if let Err(e) = kept {
            let _ = chrome.child.kill();
            let _ = chrome.child.wait();
            return Err(format!("the browser did not start: {e}"));
        }
        Ok(chrome)
    }
}

/// A pipe whose ends are closed in any program this one starts.
fn pipe() -> io::Result<(RawFd, RawFd)> {
    let mut fds = [0; 2];
    // SAFETY: `fds` has room for the two descriptors.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    for fd in fds {
        // SAFETY: descriptors just made.
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    }
    // Read end, write end.
    Ok((fds[0], fds[1]))
}

fn gone(n: u32) -> String {
    format!("There is no element {n} on the page now: look again.")
}

impl Chrome {
    /// One DevTools call; `session`: on that page, else on the browser.
    fn call(
        &mut self,
        session: Option<&str>,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        self.next += 1;
        let id = self.next;
        let mut message = json!({"id": id, "method": method, "params": params});
        if let Some(session) = session {
            message["sessionId"] = json!(session);
        }
        let mut bytes = message.to_string().into_bytes();
        bytes.push(0);
        self.to
            .write_all(&bytes)
            .map_err(|_| "the browser was closed".to_owned())?;
        let deadline = std::time::Instant::now() + CALL;
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match self.replies.recv_timeout(left) {
                Ok(reply) if reply["id"] == id => {
                    return match reply.get("error") {
                        Some(error) => {
                            Err(error["message"].as_str().unwrap_or("failed").to_owned())
                        }
                        None => Ok(reply["result"].clone()),
                    };
                }
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => {
                    return Err(format!("the browser did not answer {method}"));
                }
                Err(RecvTimeoutError::Disconnected) => return Err("the browser was closed".into()),
            }
        }
    }

    /// The page the bot works on: the one attached to, else the first tab, else a new one.
    fn page(&mut self) -> Result<String, String> {
        if let Some(page) = &self.page {
            // Still there?
            let page = page.clone();
            self.call(Some(&page), "Runtime.evaluate", json!({"expression": "1"}))?;
            return Ok(page);
        }
        let targets = self.call(None, "Target.getTargets", json!({}))?;
        let found = targets["targetInfos"].as_array().and_then(|all| {
            all.iter()
                .find(|t| t["type"] == "page")
                .and_then(|t| t["targetId"].as_str().map(str::to_owned))
        });
        let target = match found {
            Some(target) => target,
            None => {
                let made = self.call(None, "Target.createTarget", json!({"url": "about:blank"}))?;
                made["targetId"]
                    .as_str()
                    .ok_or("no page was opened")?
                    .to_owned()
            }
        };
        let attached = self.call(
            None,
            "Target.attachToTarget",
            json!({"targetId": target, "flatten": true}),
        )?;
        let page = attached["sessionId"]
            .as_str()
            .ok_or("the page could not be used")?
            .to_owned();
        let session = Some(page.as_str());
        self.call(session, "Page.enable", json!({}))?;
        // Without it, the page's calls of the binding are not reported.
        self.call(session, "Runtime.enable", json!({}))?;
        self.call(
            session,
            "Runtime.addBinding",
            json!({"name": BINDING, "executionContextName": WORLD}),
        )?;
        self.call(
            session,
            "Page.addScriptToEvaluateOnNewDocument",
            json!({"source": WATCH, "worldName": WORLD}),
        )?;
        // The page already open gets it too.
        let tree = self.call(session, "Page.getFrameTree", json!({}))?;
        let frame = tree["frameTree"]["frame"]["id"].clone();
        let world = self.call(
            session,
            "Page.createIsolatedWorld",
            json!({"frameId": frame, "worldName": WORLD}),
        )?;
        let context = world["executionContextId"].clone();
        self.call(
            session,
            "Runtime.evaluate",
            json!({"expression": WATCH, "contextId": context}),
        )?;
        self.page = Some(page.clone());
        Ok(page)
    }

    /// Runs `script` in the page; gives what it returns.
    fn eval(&mut self, page: &str, script: &str) -> Result<Value, String> {
        let result = self.call(
            Some(page),
            "Runtime.evaluate",
            json!({"expression": script, "returnByValue": true, "awaitPromise": true}),
        )?;
        if let Some(e) = result.get("exceptionDetails") {
            let why = e["exception"]["description"]
                .as_str()
                .or(e["text"].as_str());
            let why: String = why.unwrap_or("an error").chars().take(300).collect();
            return Err(format!("the page refused: {why}"));
        }
        Ok(result["result"]["value"].clone())
    }

    /// Waits for the page's next frame, at most a second (a page that is not drawn has none):
    /// the page's scroll events are sent in it, so they have run before the page is read.
    fn frame(&mut self, page: &str) -> Result<(), String> {
        let script = "new Promise(r => { requestAnimationFrame(() => r()); setTimeout(r, 1000); })";
        self.eval(page, script).map(drop)
    }

    /// Waits until the page has loaded, for at most `LOAD_MS`.
    fn settle(&mut self, page: &str) {
        let end = now_ms() + LOAD_MS;
        while now_ms() < end {
            if self
                .eval(page, "document.readyState")
                .is_ok_and(|s| s == "complete")
            {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn act(&mut self, page: &str, action: &Action, secrets: &[Secret]) -> Result<String, String> {
        match action {
            Action::Open(url) => {
                let went = self.call(Some(page), "Page.navigate", json!({"url": url}))?;
                if let Some(error) = went["errorText"].as_str().filter(|e| !e.is_empty()) {
                    return Err(format!("{url} did not open: {error}"));
                }
            }
            Action::Look => {}
            Action::Click(n) => {
                let done = self.eval(page, &format!("(() => {{ const e = (window.__bate || [])[{n} - 1]; if (!e || !e.isConnected) return false; e.scrollIntoView({{block: 'center'}}); e.focus(); e.click(); return true; }})()"))?;
                if done != true {
                    return Err(gone(*n));
                }
                thread::sleep(Duration::from_millis(300));
            }
            Action::Type {
                element,
                text,
                submit,
            } => {
                // The host and the kind of field are checked in the page, in the same script that
                // types, so that no navigation can come between them. `===` and `||` only: nothing
                // the page can replace (a method of `String`, of `Array`) decides. A secret goes only
                // into a password field, whose value is never shown back or put back by the browser;
                // the field is marked, so that `look` never reports its value, whatever the page
                // makes of its type later (a "show password" button).
                let (value, guard, mark, site) = match text {
                    Typed::Text(text) => (text.as_str(), "true".to_owned(), String::new(), ""),
                    Typed::Secret(name) => {
                        let secret = secrets.iter().find(|s| s.name == *name).ok_or_else(|| {
                            format!("You have no secret named {name}. Ask the person for it with request_secret.")
                        })?;
                        let host = serde_json::to_string(&secret.site).expect("a string");
                        let guard = format!(
                            "location.hostname === {host} && (location.protocol === 'https:' || location.hostname === 'localhost' || location.hostname === '127.0.0.1')"
                        );
                        let mark = "if (!(e.tagName === 'INPUT' && e.type === 'password')) return 'not password'; (window.__bateSecret = window.__bateSecret || new WeakSet()).add(e);".to_owned();
                        (secret.value.as_str(), guard, mark, secret.site.as_str())
                    }
                };
                let text = serde_json::to_string(value).expect("a string");
                let done = self.eval(page, &format!("(() => {{ if (!({guard})) return 'other site'; const e = (window.__bate || [])[{element} - 1]; if (!e || !e.isConnected) return false; {mark} e.focus(); if (e.isContentEditable) {{ e.textContent = {text}; }} else {{ const kind = e instanceof HTMLTextAreaElement ? HTMLTextAreaElement : e instanceof HTMLSelectElement ? HTMLSelectElement : HTMLInputElement; const set = Object.getOwnPropertyDescriptor(kind.prototype, 'value').set; set.call(e, {text}); }} e.dispatchEvent(new Event('input', {{bubbles: true}})); e.dispatchEvent(new Event('change', {{bubbles: true}})); if ({submit}) {{ if (e.form) e.form.requestSubmit(); else e.dispatchEvent(new KeyboardEvent('keydown', {{key: 'Enter', bubbles: true}})); }} return true; }})()"))?;
                if done == "other site" {
                    return Err(format!(
                        "Nothing was typed: this page is not on {site} with https, the one place that secret may be typed."
                    ));
                }
                if done == "not password" {
                    return Err("Nothing was typed: a secret goes only into a password field (an element shown as `password field`), never into a text field, a text area or an editable page.".into());
                }
                if done != true {
                    return Err(gone(*element));
                }
                thread::sleep(Duration::from_millis(300));
            }
            Action::Back => {
                self.eval(page, "history.back()")?;
                thread::sleep(Duration::from_millis(300));
            }
            Action::Forward => {
                self.eval(page, "history.forward()")?;
                thread::sleep(Duration::from_millis(300));
            }
            Action::Reload => {
                self.call(Some(page), "Page.reload", json!({}))?;
                thread::sleep(Duration::from_millis(300));
            }
            Action::Scroll { down } => {
                let sign = if *down { "" } else { "-" };
                self.eval(
                    page,
                    &format!("window.scrollBy(0, {sign}window.innerHeight * 0.9)"),
                )?;
                self.frame(page)?;
            }
            Action::Press { key, element } => {
                if let Some(n) = element {
                    let done = self.eval(page, &format!("(() => {{ const e = (window.__bate || [])[{n} - 1]; if (!e || !e.isConnected) return false; e.scrollIntoView({{block: 'center'}}); e.focus(); return true; }})()"))?;
                    if done != true {
                        return Err(gone(*n));
                    }
                }
                // Deleting a letter of a secret leaves all but one of its letters in the field, which
                // a form sent with Enter would put in the address.
                if *key == "Backspace"
                    && self.eval(page, "(window.__bateSecret && window.__bateSecret.has(document.activeElement)) === true")? == true
                {
                    return Err("Nothing was pressed: the field holds a secret, and Backspace would change it. Type the secret again to replace it.".into());
                }
                let &(_, code, text) = KEYS.iter().find(|k| k.0 == *key).expect("parsed");
                // The page cannot tell this key from a person's: the watch must not take it
                // for the user, so it is silent until the key has been handled.
                self.acting.store(true, Ordering::SeqCst);
                let sent = [if text.is_empty() { "rawKeyDown" } else { "keyDown" }, "keyUp"]
                    .into_iter()
                    .try_for_each(|kind| {
                        let text = if kind == "keyUp" { "" } else { text };
                        let key = json!({"type": kind, "key": key, "code": key, "windowsVirtualKeyCode": code, "text": text});
                        self.call(Some(page), "Input.dispatchKeyEvent", key).map(drop)
                    });
                thread::sleep(Duration::from_millis(300));
                self.acting.store(false, Ordering::SeqCst);
                sent?;
                self.frame(page)?;
            }
            Action::Select { element, option } => {
                let option = serde_json::to_string(option).expect("a string");
                let done = self.eval(page, &format!("(() => {{ const e = (window.__bate || [])[{element} - 1]; if (!e || !e.isConnected) return null; if (!(e instanceof HTMLSelectElement) || e.multiple) return 'Element {element} is not a dropdown.'; const norm = s => s.trim().replace(/\\s+/g, ' '); const o = [...e.options].find(o => norm(o.text) === norm({option})); if (!o) return 'Element {element} has no option with that text. Its options: ' + [...e.options].map(o => norm(o.text)).join(' | ').slice(0, 500); if (o.disabled) return 'That option is disabled.'; e.focus(); o.selected = true; e.dispatchEvent(new Event('input', {{bubbles: true}})); e.dispatchEvent(new Event('change', {{bubbles: true}})); return 'ok'; }})()"))?;
                match done.as_str() {
                    Some("ok") => {}
                    Some(why) => return Err(why.to_owned()),
                    None => return Err(gone(*element)),
                }
                thread::sleep(Duration::from_millis(300));
            }
            Action::Wait(seconds) => thread::sleep(Duration::from_secs(*seconds)),
        }
        self.settle(page);
        self.look(page)
    }

    /// The page as the bot sees it; the elements it may act on are numbered.
    fn look(&mut self, page: &str) -> Result<String, String> {
        let seen = self.eval(page, &format!("(() => {{
            if (!['http:', 'https:', 'about:'].includes(location.protocol)) return null;
            const all = [...document.querySelectorAll('a[href], button, input, textarea, select, summary, [role=button], [role=link], [contenteditable=true]')];
            const shown = all.filter(e => {{ const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0 && !e.disabled && e.type !== 'hidden'; }}).slice(0, {MAX_ELEMENTS});
            window.__bate = shown;
            const secret = e => e.type === 'password' || e.autocomplete === 'one-time-code' || (window.__bateSecret && window.__bateSecret.has(e)) === true;
            const name = e => (e.getAttribute('aria-label') || e.innerText || (secret(e) ? '' : e.value) || e.placeholder || e.title || e.name || e.getAttribute('href') || '').trim().replace(/\\s+/g, ' ').slice(0, 80);
            const kind = e => e.tagName === 'A' ? 'link' : e.tagName === 'SELECT' ? 'select' + (e.multiple || !e.selectedOptions.length ? '' : ' (' + e.selectedOptions[0].text.trim().replace(/\\s+/g, ' ') + ')') : e.tagName === 'TEXTAREA' || e.isContentEditable ? 'field' : e.tagName === 'INPUT' ? (['submit', 'button', 'reset', 'image'].includes(e.type) ? 'button' : ['checkbox', 'radio'].includes(e.type) ? e.type + (e.checked ? ' (checked)' : '') : secret(e) ? 'password field' : 'field') : 'button';
            const text = (document.body ? document.body.innerText : '').replace(/\\n{{3,}}/g, '\\n\\n');
            return {{title: document.title.slice(0, 300), url: location.href.slice(0, 2000), text: text.slice(0, {MAX_TEXT}), cut: text.length > {MAX_TEXT},
                elements: shown.map((e, i) => `[${{i + 1}}] ${{kind(e)}} \"${{name(e)}}\"`).join('\\n')}};
        }})()"))?;
        if seen.is_null() {
            return Err(
                "The page is not a web page (the user opened another kind): open a web address."
                    .into(),
            );
        }
        let text = seen["text"].as_str().unwrap_or("");
        let cut = if seen["cut"] == true {
            "\n[the rest of the text is left out]"
        } else {
            ""
        };
        let elements = match seen["elements"].as_str().unwrap_or("") {
            "" => "none",
            list => list,
        };
        Ok(format!(
            "Title: {}\nAddress: {}\n\n{text}{cut}\n\nElements:\n{elements}",
            seen["title"].as_str().unwrap_or(""),
            seen["url"].as_str().unwrap_or(""),
        ))
    }
}

/// Where a bot's browser keeps its profile: in the service's folder, not the bot's, so that
/// the bot's own programs cannot read its cookies.
pub fn profile(home: &Path, bot: &str) -> PathBuf {
    home.join("browsers").join(bot)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn only_web_addresses_and_the_listed_actions_pass() {
        let open = |url: &str| Action::parse(&args(json!({"action": "open", "url": url})));
        assert_eq!(
            open("https://example.com/a?b=c"),
            Ok(Action::Open("https://example.com/a?b=c".into()))
        );
        assert_eq!(
            open("HTTP://example.com"),
            Ok(Action::Open("HTTP://example.com".into()))
        );
        for bad in [
            "file:///etc/hosts",
            "javascript:alert(1)",
            "chrome://settings",
            "about:blank",
            "data:text/html,x",
            "http://a b",
            "https://x\ny",
        ] {
            assert!(open(bad).is_err(), "{bad}");
        }
        assert!(Action::parse(&args(json!({"action": "click", "element": 0}))).is_err());
        assert!(Action::parse(&args(json!({"action": "click", "element": "1"}))).is_err());
        assert_eq!(
            Action::parse(&args(json!({"action": "click", "element": 3}))),
            Ok(Action::Click(3))
        );
        assert!(Action::parse(&args(json!({"action": "run"}))).is_err());
        let long = "x".repeat(MAX_TYPED + 1);
        assert!(
            Action::parse(&args(json!({"action": "type", "element": 1, "text": long}))).is_err()
        );
    }

    #[test]
    fn the_new_actions_are_checked_when_parsed() {
        let parse = |value: Value| Action::parse(&args(value));
        for (value, want) in [
            (json!({"action": "forward"}), Action::Forward),
            (json!({"action": "reload"}), Action::Reload),
            (
                json!({"action": "scroll", "direction": "up"}),
                Action::Scroll { down: false },
            ),
            (
                json!({"action": "scroll", "direction": "down"}),
                Action::Scroll { down: true },
            ),
            (
                json!({"action": "press", "key": "Tab"}),
                Action::Press {
                    key: "Tab",
                    element: None,
                },
            ),
            (
                json!({"action": "press", "key": "PageDown", "element": 4}),
                Action::Press {
                    key: "PageDown",
                    element: Some(4),
                },
            ),
            (
                json!({"action": "press", "key": "Enter", "element": null}),
                Action::Press {
                    key: "Enter",
                    element: None,
                },
            ),
            (
                json!({"action": "select", "element": 2, "option": "  Green  tea "}),
                Action::Select {
                    element: 2,
                    option: "Green  tea".into(),
                },
            ),
            (json!({"action": "wait", "seconds": 1}), Action::Wait(1)),
            (json!({"action": "wait", "seconds": 10}), Action::Wait(10)),
        ] {
            assert_eq!(parse(value.clone()), Ok(want), "{value}");
        }
        for key in [
            "Enter",
            "Tab",
            "Escape",
            "Backspace",
            "ArrowUp",
            "ArrowDown",
            "ArrowLeft",
            "ArrowRight",
            "PageUp",
            "PageDown",
        ] {
            assert!(
                parse(json!({"action": "press", "key": key})).is_ok(),
                "{key}"
            );
        }
        let long = "x".repeat(201);
        for bad in [
            json!({"action": "scroll"}),
            json!({"action": "scroll", "direction": "left"}),
            json!({"action": "scroll", "direction": 1}),
            json!({"action": "press"}),
            json!({"action": "press", "key": "enter"}),
            json!({"action": "press", "key": "F5"}),
            json!({"action": "press", "key": "Control+c"}),
            json!({"action": "press", "key": "Tab", "element": 0}),
            json!({"action": "press", "key": "Tab", "element": "1"}),
            json!({"action": "select", "option": "Red"}),
            json!({"action": "select", "element": 0, "option": "Red"}),
            json!({"action": "select", "element": 1}),
            json!({"action": "select", "element": 1, "option": "  "}),
            json!({"action": "select", "element": 1, "option": 5}),
            json!({"action": "select", "element": 1, "option": long}),
            json!({"action": "wait"}),
            json!({"action": "wait", "seconds": 0}),
            json!({"action": "wait", "seconds": 11}),
            json!({"action": "wait", "seconds": -1}),
            json!({"action": "wait", "seconds": "3"}),
            json!({"action": "wait", "seconds": 1.5}),
        ] {
            assert!(parse(bad.clone()).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_secret_is_named_not_given_and_hidden_where_a_page_shows_it_back() {
        let parse = |value: Value| Action::parse(&args(value));
        assert_eq!(
            parse(json!({"action": "type", "element": 2, "secret": "shop", "submit": true})),
            Ok(Action::Type {
                element: 2,
                text: Typed::Secret("shop".into()),
                submit: true
            })
        );
        for bad in [
            json!({"action": "type", "element": 2, "text": "x", "secret": "shop"}),
            json!({"action": "type", "element": 2, "secret": 5}),
            json!({"action": "type", "element": 2, "secret": "a b"}),
            json!({"action": "type", "element": 2, "secret": ""}),
        ] {
            assert!(parse(bad.clone()).is_err(), "{bad}");
        }

        let secret = |name: &str, value: &str| Secret {
            name: name.into(),
            site: "example.com".into(),
            value: value.into(),
        };
        let secrets = [secret("pin", "1234"), secret("shop", "p a&ss!1")];
        // As typed, as a form sends it, as an address writes it; a longer value goes first.
        let shown =
            "https://example.com/?q=p+a%26ss%211 p%20a%26ss%211 p a&ss!1 and 1234 and 12345";
        assert_eq!(
            redact(shown, &secrets),
            "https://example.com/?q=[secret shop] [secret shop] [secret shop] and [secret pin] and [secret pin]5"
        );
        // A value that is empty hides nothing (and cannot loop).
        assert_eq!(redact("text", &[secret("none", "")]), "text");
    }
}

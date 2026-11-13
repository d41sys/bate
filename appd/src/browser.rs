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
const WATCH: &str = "for (const t of ['mousedown', 'keydown', 'touchstart']) \
    addEventListener(t, e => { if (e.isTrusted) bateUser(t) }, true);";

/// What the bot may do. Checked here: the engine is the bot's.
#[derive(Debug, PartialEq)]
pub enum Action {
    Open(String),
    Look,
    Click(u32),
    Type {
        element: u32,
        text: String,
        submit: bool,
    },
    Back,
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
                let text = args.get("text").and_then(Value::as_str).unwrap_or("");
                if text.chars().count() > MAX_TYPED {
                    return Err(format!("`text` may be {MAX_TYPED} characters"));
                }
                let submit = args.get("submit").and_then(Value::as_bool).unwrap_or(false);
                Ok(Action::Type {
                    element: element()?,
                    text: text.to_owned(),
                    submit,
                })
            }
            Some("back") => Ok(Action::Back),
            _ => Err("`action` must be open, look, click, type or back".into()),
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

/// One action, and whom to tell what came of it.
pub struct Job {
    pub request: String,
    pub generation: u64,
    pub action: Action,
    /// Set when the engine no longer waits (its turn ended): a job not begun is dropped.
    pub cancelled: Arc<AtomicBool>,
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
    pub fn start(bot: &str, profile: PathBuf, downloads: PathBuf, tx: Sender<Msg>) -> Handle {
        let (work, jobs) = mpsc::channel();
        let taken = Arc::new(AtomicBool::new(false));
        let mut browser = Browser {
            bot: bot.to_owned(),
            profile,
            downloads,
            tx,
            taken: taken.clone(),
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
}

struct Browser {
    bot: String,
    profile: PathBuf,
    downloads: PathBuf,
    tx: Sender<Msg>,
    taken: Arc<AtomicBool>,
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
        let result = self.act(&job.action);
        let _ = self.tx.send(Msg::Browsed {
            bot: self.bot.clone(),
            generation: job.generation,
            request: job.request,
            result,
        });
    }

    fn act(&mut self, action: &Action) -> Result<String, String> {
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
        chrome.act(&page, action)
    }

    fn launch(&self) -> Result<Chrome, String> {
        let program = program().ok_or(
            "No Chrome or Chromium is installed in /Applications, so there is no browser. Bate does not install one.",
        )?;
        std::fs::create_dir_all(&self.profile)
            .and_then(|()| std::fs::create_dir_all(&self.downloads))
            .map_err(|e| format!("cannot make the browser's folders: {e}"))?;
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
                Ok(())
            });
        }
        let spawned = command.spawn();
        // SAFETY: the child's ends, which this process no longer needs.
        unsafe {
            libc::close(fd_in);
            libc::close(fd_out);
        }
        let child = spawned.map_err(|e| format!("cannot start {}: {e}", program.display()))?;
        // SAFETY: fresh descriptors from `pipe`, owned from here on.
        let (to, from) = unsafe { (File::from_raw_fd(to_chrome), File::from_raw_fd(from_chrome)) };
        let (replies_tx, replies) = mpsc::channel();
        let gone = Arc::new(AtomicBool::new(false));
        let (taken, tx, bot, ended) = (
            self.taken.clone(),
            self.tx.clone(),
            self.bot.clone(),
            gone.clone(),
        );
        thread::spawn(move || {
            let mut reader = BufReader::new(from);
            let mut message = Vec::new();
            loop {
                message.clear();
                let read = (&mut reader)
                    .take(MAX_MESSAGE as u64)
                    .read_until(0, &mut message);
                if !matches!(read, Ok(n) if n > 0 && message.last() == Some(&0)) {
                    ended.store(true, Ordering::SeqCst);
                    return;
                }
                let Ok(message) = serde_json::from_slice::<Value>(&message[..message.len() - 1])
                else {
                    continue;
                };
                if message["method"] == "Runtime.bindingCalled"
                    && message["params"]["name"] == BINDING
                {
                    if !taken.swap(true, Ordering::SeqCst) {
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
        };
        // Downloads stay in the bot's folder.
        let downloads = self.downloads.to_string_lossy().into_owned();
        let kept = chrome.call(
            None,
            "Browser.setDownloadBehavior",
            json!({"behavior": "allow", "downloadPath": downloads}),
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
            return Err(format!("the page refused: {}", why.unwrap_or("an error")));
        }
        Ok(result["result"]["value"].clone())
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

    fn act(&mut self, page: &str, action: &Action) -> Result<String, String> {
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
                    return Err(format!(
                        "There is no element {n} on the page now: look again."
                    ));
                }
                thread::sleep(Duration::from_millis(300));
            }
            Action::Type {
                element,
                text,
                submit,
            } => {
                let text = serde_json::to_string(text).expect("a string");
                let done = self.eval(page, &format!("(() => {{ const e = (window.__bate || [])[{element} - 1]; if (!e || !e.isConnected) return false; e.focus(); if (e.isContentEditable) {{ e.textContent = {text}; }} else {{ const kind = e instanceof HTMLTextAreaElement ? HTMLTextAreaElement : e instanceof HTMLSelectElement ? HTMLSelectElement : HTMLInputElement; const set = Object.getOwnPropertyDescriptor(kind.prototype, 'value').set; set.call(e, {text}); }} e.dispatchEvent(new Event('input', {{bubbles: true}})); e.dispatchEvent(new Event('change', {{bubbles: true}})); if ({submit}) {{ if (e.form) e.form.requestSubmit(); else e.dispatchEvent(new KeyboardEvent('keydown', {{key: 'Enter', bubbles: true}})); }} return true; }})()"))?;
                if done != true {
                    return Err(format!(
                        "There is no element {element} on the page now: look again."
                    ));
                }
                thread::sleep(Duration::from_millis(300));
            }
            Action::Back => {
                self.eval(page, "history.back()")?;
                thread::sleep(Duration::from_millis(300));
            }
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
            const name = e => (e.getAttribute('aria-label') || e.innerText || e.value || e.placeholder || e.title || e.name || e.getAttribute('href') || '').trim().replace(/\\s+/g, ' ').slice(0, 80);
            const kind = e => e.tagName === 'A' ? 'link' : e.tagName === 'SELECT' ? 'select' : e.tagName === 'TEXTAREA' || e.isContentEditable ? 'field' : e.tagName === 'INPUT' ? (['submit', 'button', 'reset', 'image'].includes(e.type) ? 'button' : ['checkbox', 'radio'].includes(e.type) ? e.type + (e.checked ? ' (checked)' : '') : 'field') : 'button';
            const text = (document.body ? document.body.innerText : '').replace(/\\n{{3,}}/g, '\\n\\n');
            return {{title: document.title, url: location.href, text: text.slice(0, {MAX_TEXT}), cut: text.length > {MAX_TEXT},
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
}

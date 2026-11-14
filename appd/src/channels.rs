//! Chat channels: a bot is messaged from Telegram or Discord. The service connects out from
//! this Mac (Telegram is asked what is new; Discord's gateway is held open), so nothing in
//! between relays the messages. All of it talks to the network, so none of it runs on the
//! service's one thread.

use crate::core::Msg;
use crate::sys::{log, now_ms};
use serde_json::{Value, json};
use std::io::{ErrorKind, Read};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};
use tungstenite::Message;
use tungstenite::stream::MaybeTlsStream;

pub const KINDS: [&str; 2] = ["telegram", "discord"];

const TELEGRAM: (&str, &str) = ("BATE_TELEGRAM_API", "https://api.telegram.org");
const DISCORD: (&str, &str) = ("BATE_DISCORD_API", "https://discord.com/api/v10");
/// Discord turns away clients that do not say what they are in this form.
const DISCORD_AGENT: &str = "DiscordBot (https://github.com/d41sys/bate, 0.1)";
/// Direct messages, and nothing else.
const DISCORD_INTENTS: u64 = 1 << 12;

/// How long Telegram may hold a request for news, in seconds.
const POLL_S: u64 = 25;
/// Updates asked for at a time; Telegram lets one be 256 KB at most.
const UPDATES: u64 = 10;
/// Most bytes read of any answer: more than `UPDATES` of them can be.
const MAX_ANSWER: u64 = 4 << 20;
/// A Telegram message older than this is not run: it was written while the service was
/// off, and whoever wrote it no longer waits. (Discord keeps none.)
const STALE_S: u64 = 120;
/// How long connecting to Discord's gateway, and its first word, may take.
const GATEWAY_WAIT: Duration = Duration::from_secs(30);
/// Messages on their way out.
static SENDING: AtomicUsize = AtomicUsize::new(0);

/// Whether so many messages are on their way out that one nobody waits for should not be
/// sent: every one is a thread and a request.
pub fn crowded() -> bool {
    SENDING.load(Ordering::SeqCst) >= 8
}

/// How long a code lets a chat in, and how many wrong guesses end it sooner.
pub const CODE_LIFE: Duration = Duration::from_secs(5 * 60);
pub const CODE_TRIES: u8 = 5;

/// The code the window shows; sent in a chat, it lets that chat in.
pub struct Code {
    pub code: String,
    pub until: Instant,
    pub tries: u8,
}

impl Code {
    pub fn new() -> Code {
        Code {
            code: crate::sys::pair_code(),
            until: Instant::now() + CODE_LIFE,
            tries: 0,
        }
    }
}

/// What a channel's reader tells the service.
pub enum Note {
    /// Someone wrote to the bot. `chat` is where the answer goes; `name` is who wrote; `text`
    /// is empty for what is no text (a photo, a file).
    Message {
        chat: String,
        name: String,
        text: String,
    },
    /// The channel works (`None`), or why it does not.
    State(Option<String>),
}

/// Reads one bot's channel until `stop` is set.
struct Reader {
    bot: String,
    kind: &'static str,
    token: String,
    tx: Sender<Msg>,
    stop: Arc<AtomicBool>,
    /// The state last told to the service.
    told: Option<Option<String>>,
    failures: u32,
}

impl Reader {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    fn note(&self, note: Note) {
        let _ = self.tx.send(Msg::Channel {
            bot: self.bot.clone(),
            kind: self.kind,
            reader: self.stop.clone(),
            note,
        });
    }

    /// Tells the service how the channel is, when that has changed.
    fn state(&mut self, error: Option<String>) {
        self.failures = if error.is_some() {
            self.failures + 1
        } else {
            0
        };
        if self.told.as_ref() != Some(&error) {
            if let Some(e) = &error {
                log!("bot {}: {}: {e}", self.bot, self.kind);
            }
            self.note(Note::State(error.clone()));
            self.told = Some(error);
        }
    }

    /// Waits until the reader is stopped: there is nothing to try again.
    fn wait_for_stop(&self) {
        while !self.stopped() {
            thread::sleep(Duration::from_millis(200));
        }
    }

    /// Waits before the next try: a second after the first failure, a minute after many.
    fn pause(&self) {
        let wait = Duration::from_secs((1u64 << (self.failures.clamp(1, 7) - 1)).min(60));
        let until = Instant::now() + wait;
        while !self.stopped() && Instant::now() < until {
            thread::sleep(Duration::from_millis(200));
        }
    }
}

/// Starts reading `kind` for `bot`. Setting the flag it returns stops the reader; what it
/// still sends after that names the flag, so the service can tell.
pub fn start(bot: &str, kind: &'static str, token: &str, tx: Sender<Msg>) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let reader = Reader {
        bot: bot.to_owned(),
        kind,
        token: token.to_owned(),
        tx,
        stop: stop.clone(),
        told: None,
        failures: 0,
    };
    thread::spawn(move || match kind {
        "telegram" => telegram(reader),
        _ => discord(reader),
    });
    stop
}

fn api((variable, default): (&str, &str)) -> String {
    std::env::var(variable).unwrap_or_else(|_| default.to_owned())
}

fn agent(timeout: Duration) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(timeout))
        // A redirect could carry the token to another host.
        .max_redirects(0)
        .build();
    ureq::Agent::new_with_config(config)
}

/// The JSON of an answer, or why there is none. The token is in Telegram's URLs, and so
/// could be in an error: it is taken out.
fn answer(
    result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    token: &str,
) -> Result<Value, String> {
    let clean = |text: String| text.replace(token, "[removed]");
    let response = result.map_err(|e| clean(format!("no connection: {e}")))?;
    let status = response.status();
    let mut text = String::new();
    let mut body = response.into_body().into_reader().take(MAX_ANSWER);
    body.read_to_string(&mut text)
        .map_err(|e| clean(format!("no answer: {e}")))?;
    let json: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if status.is_success() && !json.is_null() && json["ok"] != false {
        return Ok(json);
    }
    // Telegram says why in `description`, Discord in `message`.
    let why = json["description"].as_str().or(json["message"].as_str());
    Err(clean(format!("refused ({status}): {}", why.unwrap_or(""))))
}

fn telegram(mut r: Reader) {
    let api = api(TELEGRAM);
    let agent = agent(Duration::from_secs(POLL_S + 15));
    let mut offset = 0;
    while !r.stopped() {
        let url = format!(
            "{api}/bot{}/getUpdates?timeout={POLL_S}&limit={UPDATES}&offset={offset}&allowed_updates=%5B%22message%22%5D",
            r.token
        );
        let updates = match answer(agent.get(&url).call(), &r.token) {
            Ok(updates) => updates,
            Err(e) => {
                r.state(Some(e));
                r.pause();
                continue;
            }
        };
        r.state(None);
        for update in updates["result"].as_array().into_iter().flatten() {
            offset = update["update_id"].as_i64().map_or(offset, |id| id + 1);
            let message = &update["message"];
            let age_s =
                (now_ms() / 1000).saturating_sub(message["date"].as_u64().unwrap_or(u64::MAX));
            // shortcut: one-to-one chats and text only; groups are passed over, and of a
            // photo or a file only that it came is told. Add them when a bot is to sit in
            // a group or to be sent files.
            if let (Some(chat), Some("private"), true) = (
                message["chat"]["id"].as_i64(),
                message["chat"]["type"].as_str(),
                age_s <= STALE_S,
            ) {
                let text = message["text"].as_str().unwrap_or("");
                let from = &message["from"];
                let name = from["first_name"].as_str().or(from["username"].as_str());
                r.note(Note::Message {
                    chat: chat.to_string(),
                    name: name.unwrap_or("someone").to_owned(),
                    text: text.to_owned(),
                });
            }
        }
    }
}

fn discord(mut r: Reader) {
    let api = api(DISCORD);
    let agent = agent(Duration::from_secs(30));
    while !r.stopped() {
        if let Err(e) = discord_session(&mut r, &api, &agent) {
            r.state(Some(e));
            r.pause();
        }
    }
}

/// One connection to Discord's gateway, for as long as it holds.
// shortcut: a broken connection starts a new session rather than resuming the old one, so
// what was written in the seconds between is lost. Resume when that is missed.
fn discord_session(r: &mut Reader, api: &str, agent: &ureq::Agent) -> Result<(), String> {
    let asked = agent
        .get(format!("{api}/gateway/bot"))
        .header("Authorization", format!("Bot {}", r.token))
        .header("User-Agent", DISCORD_AGENT)
        .call();
    let gateway = answer(asked, &r.token)?;
    let url = gateway["url"].as_str().ok_or("Discord named no gateway")?;
    // Connected by hand, for the time limits `tungstenite::connect` does not set.
    let no_gateway = |e: &dyn std::fmt::Display| format!("no connection to the gateway: {e}");
    let uri: tungstenite::http::Uri = format!("{url}/?v=10&encoding=json")
        .parse()
        .map_err(|e| no_gateway(&e))?;
    let port = uri.port_u16();
    let port = port.unwrap_or(if uri.scheme_str() == Some("ws") {
        80
    } else {
        443
    });
    let found = (uri.host().unwrap_or_default(), port).to_socket_addrs();
    let address = found.map_err(|e| no_gateway(&e))?.next();
    let address = address.ok_or_else(|| no_gateway(&"its name names no address"))?;
    let stream = TcpStream::connect_timeout(&address, GATEWAY_WAIT).map_err(|e| no_gateway(&e))?;
    let _ = stream.set_read_timeout(Some(GATEWAY_WAIT));
    let _ = stream.set_write_timeout(Some(GATEWAY_WAIT));
    let (mut socket, _) = tungstenite::client_tls(uri, stream).map_err(|e| no_gateway(&e))?;
    let connected = Instant::now();
    // Short waits for news, so that heartbeats go out and a stop is noticed.
    let wait = Some(Duration::from_secs(1));
    let _ = match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => stream.set_read_timeout(wait),
        MaybeTlsStream::Rustls(stream) => stream.get_mut().set_read_timeout(wait),
        _ => Ok(()),
    };
    let broke = |e: tungstenite::Error| format!("the gateway connection broke: {e}");
    // How often the gateway wants a heartbeat, when the last went, and whether it was answered.
    let (mut every, mut last, mut answered) = (None, Instant::now(), true);
    let mut sequence = Value::Null;
    loop {
        if r.stopped() {
            let _ = socket.close(None);
            return Ok(());
        }
        if every.is_none() && connected.elapsed() >= GATEWAY_WAIT {
            return Err("the gateway did not say hello".into());
        }
        if every.is_some_and(|every| last.elapsed() >= every) {
            if !answered {
                return Err("the gateway stopped answering".into());
            }
            let beat = json!({"op": 1, "d": sequence});
            socket
                .send(Message::text(beat.to_string()))
                .map_err(broke)?;
            (last, answered) = (Instant::now(), false);
        }
        let text = match socket.read() {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(frame)) => {
                let code = frame.as_ref().map_or(0, |f| u16::from(f.code));
                let reason = frame.map(|f| f.reason.to_string()).unwrap_or_default();
                let why = format!("the gateway closed: {code} {reason}");
                // What this token is refused for it is refused for again, and Discord counts
                // the tries: none until the token is set anew.
                if matches!(code, 4004 | 4010..=4014) {
                    r.state(Some(why));
                    r.wait_for_stop();
                    return Ok(());
                }
                return Err(why);
            }
            Ok(_) => continue,
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                continue;
            }
            Err(e) => return Err(broke(e)),
        };
        let event: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if !event["s"].is_null() {
            sequence = event["s"].clone();
        }
        let data = &event["d"];
        match (event["op"].as_u64(), event["t"].as_str()) {
            // Hello: say who this is.
            (Some(10), _) => {
                let interval = data["heartbeat_interval"].as_u64().unwrap_or(41_250);
                (every, last) = (Some(Duration::from_millis(interval)), Instant::now());
                let identify = json!({"op": 2, "d": {
                    "token": r.token,
                    "intents": DISCORD_INTENTS,
                    "properties": {"os": "macos", "browser": "bate", "device": "bate"},
                }});
                socket
                    .send(Message::text(identify.to_string()))
                    .map_err(broke)?;
            }
            (Some(11), _) => answered = true,
            // Asked for a heartbeat now.
            (Some(1), _) => {
                let beat = json!({"op": 1, "d": sequence});
                socket
                    .send(Message::text(beat.to_string()))
                    .map_err(broke)?;
            }
            (Some(7 | 9), _) => return Err("the gateway asked for a new connection".into()),
            (Some(0), Some("READY")) => r.state(None),
            (Some(0), Some("MESSAGE_CREATE")) => {
                let author = &data["author"];
                // shortcut: direct messages only, as for Telegram; a bot's own are not news.
                if let (Some(chat), Some(text), true) = (
                    data["channel_id"].as_str(),
                    data["content"].as_str(),
                    data["guild_id"].is_null() && author["bot"] != true,
                ) {
                    let name = author["global_name"]
                        .as_str()
                        .or(author["username"].as_str());
                    r.note(Note::Message {
                        chat: chat.to_owned(),
                        name: name.unwrap_or("someone").to_owned(),
                        text: text.to_owned(),
                    });
                }
            }
            _ => {}
        }
    }
}

/// Sends `text` to `chat`, in as many messages as it takes. Does not wait for it.
pub fn send(kind: &'static str, token: &str, chat: &str, text: &str) {
    let (token, chat) = (token.to_owned(), chat.to_owned());
    // shortcut: Telegram takes 4096 UTF-16 units and Discord 2000 characters; counting
    // characters with room to spare keeps every piece under both.
    let all: Vec<char> = text.chars().collect();
    let pieces: Vec<String> = all
        .chunks(1900)
        .map(|piece| piece.iter().collect())
        .collect();
    SENDING.fetch_add(1, Ordering::SeqCst);
    thread::spawn(move || {
        deliver(kind, &token, &chat, pieces);
        SENDING.fetch_sub(1, Ordering::SeqCst);
    });
}

fn deliver(kind: &str, token: &str, chat: &str, pieces: Vec<String>) {
    {
        let agent = agent(Duration::from_secs(30));
        for (n, piece) in pieces.into_iter().enumerate() {
            // Both let a bot write about once a second to one chat.
            if n > 0 {
                thread::sleep(Duration::from_secs(1));
            }
            let sent = if kind == "telegram" {
                let chat_id = chat.parse::<i64>().map_or(json!(chat), Value::from);
                agent
                    .post(format!("{}/bot{token}/sendMessage", api(TELEGRAM)))
                    .header("Content-Type", "application/json")
                    .send(
                        json!({"chat_id": chat_id, "text": piece})
                            .to_string()
                            .as_bytes(),
                    )
            } else {
                // The chat's id becomes part of the address: digits or nothing.
                if !chat.bytes().all(|b| b.is_ascii_digit()) {
                    return;
                }
                agent
                    .post(format!("{}/channels/{chat}/messages", api(DISCORD)))
                    .header("Authorization", format!("Bot {token}"))
                    .header("User-Agent", DISCORD_AGENT)
                    .header("Content-Type", "application/json")
                    .send(json!({"content": piece}).to_string().as_bytes())
            };
            if let Err(e) = answer(sent, token) {
                log!("{kind}: a message was not sent: {e}");
                return;
            }
        }
    }
}

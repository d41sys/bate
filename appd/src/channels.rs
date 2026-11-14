//! Chat channels: a bot is messaged from Telegram or Discord. The service connects out from
//! this Mac (Telegram is asked what is new; Discord's gateway is held open), so nothing in
//! between relays the messages. All of it talks to the network, so none of it runs on the
//! service's one thread.

use crate::core::Msg;
use crate::sys::log;
use serde_json::{Value, json};
use std::io::{ErrorKind, Read};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
/// Most bytes read of any answer.
const MAX_ANSWER: u64 = 1 << 20;

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
    /// Someone wrote to the bot. `chat` is where the answer goes; `name` is who wrote.
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
    if status.is_success() && json["ok"] != false {
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
            "{api}/bot{}/getUpdates?timeout={POLL_S}&offset={offset}&allowed_updates=%5B%22message%22%5D",
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
            offset = offset.max(update["update_id"].as_i64().unwrap_or(0) + 1);
            let message = &update["message"];
            // shortcut: one-to-one chats and text only; groups, photos and files are passed
            // over. Add them when a bot is to sit in a group.
            if let (Some(chat), Some("private"), Some(text)) = (
                message["chat"]["id"].as_i64(),
                message["chat"]["type"].as_str(),
                message["text"].as_str(),
            ) {
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
    let (mut socket, _) = tungstenite::connect(format!("{url}/?v=10&encoding=json"))
        .map_err(|e| format!("no connection to the gateway: {e}"))?;
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
                let why = frame.map(|f| format!("{} {}", u16::from(f.code), f.reason));
                return Err(format!("the gateway closed: {}", why.unwrap_or_default()));
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
            (Some(1), _) => every = every.map(|_| Duration::ZERO),
            (Some(7 | 9), _) => return Err("the gateway asked for a new connection".into()),
            (Some(0), Some("READY")) => r.state(None),
            (Some(0), Some("MESSAGE_CREATE")) => {
                let author = &data["author"];
                // shortcut: direct messages only, as for Telegram; a bot's own are not news.
                if let (Some(chat), Some(text), true) = (
                    data["channel_id"].as_str(),
                    data["content"].as_str().filter(|text| !text.is_empty()),
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
    thread::spawn(move || {
        let agent = agent(Duration::from_secs(30));
        for piece in pieces {
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
            if let Err(e) = answer(sent, &token) {
                log!("{kind}: a message was not sent: {e}");
                return;
            }
        }
    });
}

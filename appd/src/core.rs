//! The service core: one thread that owns the store, the window connections
//! and one engine per active bot. Everything reaches it as a `Msg`.

use std::collections::HashMap;
use std::io::{self, BufRead, Read};
use std::process::Child;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use proto::connectors::{Connector, Transport};
use proto::routines::{Routine, RoutineRun, RunOutcome};
use proto::{Engine, EngineInput, Event, Op, PROTOCOL, WINDOW_PROTOCOL, engine_program};
use serde_json::{Value, json};

use crate::clients::ClientHandle;
use crate::store::{Ledger, Store};
use crate::sys::{self, log, now_ms};
use crate::{paths, schedule};

pub enum Msg {
    ClientConnected {
        id: u64,
        handle: ClientHandle,
    },
    ClientFrame {
        id: u64,
        frame: Value,
    },
    ClientGarbage {
        id: u64,
    },
    ClientGone {
        id: u64,
    },
    /// `event: None`: the engine closed its output (it exited or broke the protocol).
    Engine {
        bot: String,
        generation: u64,
        event: Option<Event>,
    },
}

/// Longest piece of engine stderr logged as one line.
const MAX_STDERR_LINE: u64 = 64 << 10;
const MAX_ROUTINES: usize = 50;
/// A due routine slot older than this is recorded as missed, not run.
const CATCH_UP_MS: u64 = 6 * 3600 * 1000;
/// The core wakes at least this often while a routine is enabled.
const TICK: Duration = Duration::from_secs(30);

/// `(code, message)` of a refused request.
type Refusal = (&'static str, String);
/// `Ok(None)`: the reply comes later, when the engine answers.
type Reply = Result<Option<Value>, Refusal>;

fn internal(e: rusqlite::Error) -> Refusal {
    log!("store: {e}");
    ("internal", e.to_string())
}

fn bad(msg: impl Into<String>) -> Refusal {
    ("bad_request", msg.into())
}

fn str_field<'a>(f: &'a Value, key: &str) -> Result<&'a str, Refusal> {
    f.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| bad(format!("`{key}` must be a string")))
}

struct Client {
    handle: ClientHandle,
    hello: bool,
}

/// A prompt on its way to the engine: in the ledger as `sending`, not yet in
/// the transcript, which only gets prompts the engine admitted.
struct Outgoing {
    nonce: String,
    text: String,
}

/// Who waits for an engine's `response`, and what the response settles.
enum Waiter {
    Window {
        client: u64,
        id: Value,
        command: String,
        settles: Settles,
    },
    Routine {
        routine: String,
        prompt: Outgoing,
    },
    /// The service's own `start` or `open`.
    Service,
    /// The service's own `connectors`.
    Connectors,
}

/// What a window's request changes in the store once the engine has accepted it.
enum Settles {
    Nothing,
    /// The prompt enters the transcript and the ledger entry becomes `sent`.
    Prompt(Outgoing),
    /// The answered question is closed.
    Question(String),
    /// The transcript goes back to before the prompt with this nonce.
    Rewind(String),
    /// The bot stops counting as busy with this compaction.
    Compact,
}

struct Running {
    generation: u64,
    child: Child,
    input: EngineInput,
    ready: bool,
    /// Requests held until the engine says `ready`.
    queued: Vec<(String, Op)>,
    waiting: HashMap<String, Waiter>,
    /// Between `turn_start` and `turn_end`.
    busy: bool,
    /// `compact` requests the engine has not answered yet.
    compacting: u32,
}

pub struct Core {
    store: Store,
    tx: Sender<Msg>,
    clients: HashMap<u64, Client>,
    engines: HashMap<String, Running>,
    /// Engine request ids and engine generations.
    counter: u64,
    /// Earliest `nextRunMs` of any enabled routine; 0 forces a recheck.
    next_due: u64,
}

impl Core {
    pub fn new(store: Store, tx: Sender<Msg>) -> Core {
        Core {
            store,
            tx,
            clients: HashMap::new(),
            engines: HashMap::new(),
            counter: 0,
            next_due: 0,
        }
    }

    pub fn run(mut self, rx: Receiver<Msg>) {
        loop {
            let wait = self.run_due_routines();
            match rx.recv_timeout(wait) {
                Ok(msg) => self.handle(msg),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::ClientConnected { id, handle } => {
                let hello = json!({"type": "hello", "app": "appd", "protocols": [WINDOW_PROTOCOL], "pid": std::process::id()});
                handle.send(&encode(&hello));
                self.clients.insert(
                    id,
                    Client {
                        handle,
                        hello: false,
                    },
                );
            }
            Msg::ClientGarbage { id } => self.reply(
                id,
                &Value::Null,
                "",
                Err(("parse", "not a JSON object".into())),
            ),
            Msg::ClientGone { id } => {
                self.clients.remove(&id);
            }
            Msg::ClientFrame { id, frame } => {
                let req_id = frame.get("id").cloned().unwrap_or(Value::Null);
                let command = frame
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                let said_hello = self.clients.get(&id).is_some_and(|c| c.hello);
                let result = if !said_hello && command != "hello" {
                    Err(("hello_required", "send `hello` first".into()))
                } else {
                    self.request(id, &req_id, &command, &frame)
                };
                match result {
                    Ok(None) => {}
                    Ok(Some(data)) => self.reply(id, &req_id, &command, Ok(data)),
                    Err(e) => self.reply(id, &req_id, &command, Err(e)),
                }
            }
            Msg::Engine {
                bot,
                generation,
                event,
            } => {
                if self
                    .engines
                    .get(&bot)
                    .is_none_or(|r| r.generation != generation)
                {
                    return;
                }
                match event {
                    Some(ev) => self.on_event(&bot, ev),
                    None => self.on_engine_gone(&bot),
                }
            }
        }
    }

    fn reply(&mut self, client: u64, id: &Value, command: &str, result: Result<Value, Refusal>) {
        let frame = match result {
            Ok(data) => {
                json!({"id": id, "type": "response", "command": command, "success": true, "data": data})
            }
            Err((code, error)) => {
                json!({"id": id, "type": "response", "command": command, "success": false, "error": error, "code": code})
            }
        };
        if let Some(c) = self.clients.get(&client)
            && !c.handle.send(&encode(&frame))
        {
            c.handle.close();
            self.clients.remove(&client);
        }
    }

    /// Sends `frame` to every window that said `hello`.
    fn broadcast(&mut self, frame: &Value) {
        let bytes = encode(frame);
        self.clients.retain(|_, c| {
            let kept = !c.hello || c.handle.send(&bytes);
            if !kept {
                c.handle.close();
            }
            kept
        });
    }

    /// Appends an entry to the bot's transcript and sends it to the windows.
    fn record(&mut self, bot: &str, entry: Value) {
        let at = now_ms();
        match self.store.append(bot, at, &entry) {
            Ok(seq) => self.broadcast(
                &json!({"type": "transcript", "bot": bot, "seq": seq, "at": at, "entry": entry}),
            ),
            Err(e) => log!("bot {bot}: transcript not written: {e}"),
        }
    }

    fn request(&mut self, client: u64, id: &Value, command: &str, f: &Value) -> Reply {
        match command {
            "hello" => {
                if f.get("protocol").and_then(Value::as_u64) != Some(u64::from(WINDOW_PROTOCOL)) {
                    return Err(bad(format!(
                        "this service speaks protocol {WINDOW_PROTOCOL}"
                    )));
                }
                if let Some(c) = self.clients.get_mut(&client) {
                    c.hello = true;
                }
                Ok(Some(json!({"protocol": WINDOW_PROTOCOL})))
            }
            "list_bots" => {
                let mut bots = Vec::new();
                for b in self.store.bots().map_err(internal)? {
                    bots.push(
                        self.bot_info(&b.id, serde_json::to_value(&b).expect("serializable")),
                    );
                }
                Ok(Some(json!({"bots": bots})))
            }
            "create_bot" => {
                let name = str_field(f, "name")?.trim();
                if name.is_empty() || name.chars().count() > 80 {
                    return Err(bad("`name` must be 1 to 80 characters"));
                }
                let bot = self
                    .store
                    .create_bot(&sys::random_hex(4), name, now_ms())
                    .map_err(internal)?;
                sys::create_private_dir(&paths::bot_dir(&bot.id))
                    .map_err(|e| ("internal", e.to_string()))?;
                self.broadcast(&json!({"type": "bots_changed"}));
                Ok(Some(
                    json!({"bot": self.bot_info(&bot.id, serde_json::to_value(&bot).expect("serializable"))}),
                ))
            }
            "open_bot" => {
                let bot = self.bot_of(f)?;
                let row = self
                    .store
                    .bot(&bot)
                    .map_err(internal)?
                    .expect("checked by bot_of");
                Ok(Some(json!({
                    "bot": self.bot_info(&bot, serde_json::to_value(&row).expect("serializable")),
                    "history": self.store.history(&bot).map_err(internal)?,
                    "questions": self.store.questions(&bot).map_err(internal)?,
                })))
            }
            "prompt" => {
                let bot = self.bot_of(f)?;
                let nonce = str_field(f, "nonce")?;
                if nonce.is_empty()
                    || nonce.len() > 128
                    || !nonce.bytes().all(|b| (0x20..0x7f).contains(&b))
                {
                    return Err(bad("`nonce` must be 1 to 128 printable ASCII characters"));
                }
                let text = str_field(f, "text")?;
                match self.store.ledger(&bot, nonce).map_err(internal)? {
                    Some(Ledger::Sending | Ledger::Sent) => Ok(Some(json!({"duplicate": true}))),
                    Some(Ledger::Uncertain) => {
                        Ok(Some(json!({"duplicate": true, "uncertain": true})))
                    }
                    None => {
                        let prompt = Outgoing {
                            nonce: nonce.into(),
                            text: text.into(),
                        };
                        self.send_prompt(&bot, prompt, |p| Waiter::Window {
                            client,
                            id: id.clone(),
                            command: command.into(),
                            settles: Settles::Prompt(p),
                        })?;
                        Ok(None)
                    }
                }
            }
            "answer" => {
                let bot = self.bot_of(f)?;
                let (question, choice) = (str_field(f, "question")?, str_field(f, "choice")?);
                let open = self.store.questions(&bot).map_err(internal)?;
                if !open.iter().any(|q| q["question"] == question) {
                    return Err((
                        "no_such_question",
                        format!("question `{question}` is not open"),
                    ));
                }
                let op = Op::Answer {
                    question: question.into(),
                    choice: choice.into(),
                };
                self.send_op(
                    &bot,
                    op,
                    Waiter::Window {
                        client,
                        id: id.clone(),
                        command: command.into(),
                        settles: Settles::Question(question.into()),
                    },
                )?;
                Ok(None)
            }
            "steer" | "abort" | "set_model" | "rewind" | "compact" => {
                let bot = self.bot_of(f)?;
                let op = match command {
                    "abort" => Op::Abort,
                    "compact" => Op::Compact,
                    _ => serde_json::from_value(f.clone()).map_err(|e| bad(e.to_string()))?,
                };
                let settles = match &op {
                    Op::Rewind { entry, .. } => Settles::Rewind(entry.clone()),
                    Op::Compact => Settles::Compact,
                    _ => Settles::Nothing,
                };
                let compact = matches!(settles, Settles::Compact);
                self.send_op(
                    &bot,
                    op,
                    Waiter::Window {
                        client,
                        id: id.clone(),
                        command: command.into(),
                        settles,
                    },
                )?;
                if compact && let Some(r) = self.engines.get_mut(&bot) {
                    r.compacting += 1;
                    self.broadcast(&json!({"type": "bots_changed"}));
                }
                Ok(None)
            }
            "list_routines" => {
                let bot = self.bot_of(f)?;
                Ok(Some(
                    json!({"routines": self.store.routines(&bot).map_err(internal)?}),
                ))
            }
            "set_routine" => {
                let bot = self.bot_of(f)?;
                let routine =
                    self.set_routine(&bot, f.get("routine").cloned().unwrap_or(Value::Null))?;
                self.next_due = 0;
                self.broadcast(&json!({"type": "routines_changed", "bot": bot}));
                Ok(Some(json!({"routine": routine})))
            }
            "delete_routine" => {
                let bot = self.bot_of(f)?;
                if !self
                    .store
                    .delete_routine(&bot, str_field(f, "routineId")?)
                    .map_err(internal)?
                {
                    return Err(("no_such_routine", "no such routine".into()));
                }
                self.next_due = 0;
                self.broadcast(&json!({"type": "routines_changed", "bot": bot}));
                Ok(Some(json!({})))
            }
            "list_connectors" => {
                let bot = self.bot_of(f)?;
                Ok(Some(
                    json!({"connectors": self.store.connectors(&bot).map_err(internal)?}),
                ))
            }
            "set_connector" => {
                let bot = self.bot_of(f)?;
                let c: Connector =
                    serde_json::from_value(f.get("connector").cloned().unwrap_or(Value::Null))
                        .map_err(|e| ("invalid_connector", e.to_string()))?;
                check_connector(&c).map_err(|e| ("invalid_connector", e))?;
                self.store.put_connector(&bot, &c).map_err(internal)?;
                self.connectors_changed(&bot);
                Ok(Some(json!({"connector": c})))
            }
            "delete_connector" => {
                let bot = self.bot_of(f)?;
                if !self
                    .store
                    .delete_connector(&bot, str_field(f, "name")?)
                    .map_err(internal)?
                {
                    return Err(("no_such_connector", "no such connector".into()));
                }
                self.connectors_changed(&bot);
                Ok(Some(json!({})))
            }
            _ => Err(("unknown_command", format!("unknown command `{command}`"))),
        }
    }

    fn bot_of(&self, f: &Value) -> Result<String, Refusal> {
        let bot = str_field(f, "bot")?;
        match self.store.bot(bot).map_err(internal)? {
            Some(_) => Ok(bot.to_owned()),
            None => Err(("no_such_bot", format!("no bot `{bot}`"))),
        }
    }

    /// A bot row plus what the core knows live.
    fn bot_info(&self, bot: &str, mut row: Value) -> Value {
        let running = self.engines.get(bot);
        row["running"] = json!(running.is_some());
        row["busy"] = json!(running.is_some_and(|r| r.busy || r.compacting > 0));
        row
    }

    /// Records the prompt in the ledger (committed before the engine sees it), then sends it.
    fn send_prompt(
        &mut self,
        bot: &str,
        prompt: Outgoing,
        waiter: impl FnOnce(Outgoing) -> Waiter,
    ) -> Result<(), Refusal> {
        let nonce = prompt.nonce.clone();
        self.store
            .set_ledger(bot, &nonce, Ledger::Sending, now_ms())
            .map_err(internal)?;
        let op = Op::Prompt {
            nonce: nonce.clone(),
            text: prompt.text.clone(),
        };
        let sent = self.send_op(bot, op, waiter(prompt));
        if sent.is_err() {
            // The engine never started, so it cannot have the prompt.
            let _ = self.store.forget_ledger(bot, &nonce);
        }
        sent
    }

    /// The engine answered a prompt: admitted (`sent`, and into the transcript) or refused (forgotten).
    fn settle_prompt(&mut self, bot: &str, p: &Outgoing, admitted: bool) {
        let stored = if admitted {
            self.store.set_ledger(bot, &p.nonce, Ledger::Sent, now_ms())
        } else {
            self.store.forget_ledger(bot, &p.nonce)
        };
        if let Err(e) = stored {
            log!("bot {bot}: ledger not updated: {e}");
        }
        if admitted {
            self.record(
                bot,
                json!({"type": "prompt", "nonce": p.nonce, "text": p.text}),
            );
        }
    }

    fn send_op(&mut self, bot: &str, op: Op, waiter: Waiter) -> Result<(), Refusal> {
        self.ensure_engine(bot)?;
        self.counter += 1;
        let id = self.counter.to_string();
        let r = self.engines.get_mut(bot).expect("engine just ensured");
        r.waiting.insert(id.clone(), waiter);
        if r.ready {
            write_op(bot, r, &id, &op);
        } else {
            r.queued.push((id, op));
        }
        Ok(())
    }

    fn ensure_engine(&mut self, bot: &str) -> Result<(), Refusal> {
        if self.engines.contains_key(bot) {
            return Ok(());
        }
        let dir = paths::bot_dir(bot);
        let failed = |e: io::Error| ("engine_failed", format!("cannot start the engine: {e}"));
        sys::create_private_dir(&dir).map_err(failed)?;
        // Read before spawning, so a store error never leaves an engine without `start`/`open`.
        let connectors = self.store.connectors(bot).map_err(internal)?;
        let open = match self.store.session(bot).map_err(internal)? {
            Some(session) => Op::Open {
                bot: bot.into(),
                session,
            },
            None => Op::Start { bot: bot.into() },
        };
        let Engine {
            child,
            input,
            mut output,
            stderr,
        } = Engine::spawn(&engine_program(), &dir).map_err(failed)?;
        let name = bot.to_owned();
        // Engine diagnostics go through the service log, so they are rotated with it.
        // Bytes, not `lines()`: a non-UTF-8 byte must not stop the draining (the engine would block),
        // and a line longer than `MAX_STDERR_LINE` is logged in pieces so memory stays bounded.
        thread::spawn(move || {
            let mut reader = io::BufReader::new(stderr);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match (&mut reader)
                    .take(MAX_STDERR_LINE)
                    .read_until(b'\n', &mut buf)
                {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {
                        let line = String::from_utf8_lossy(&buf);
                        log!("bot {name}: engine: {}", line.trim_end_matches('\n'));
                    }
                }
            }
        });
        self.counter += 1;
        let generation = self.counter;
        let (tx, name) = (self.tx.clone(), bot.to_owned());
        thread::spawn(move || {
            loop {
                match output.recv() {
                    Ok(Some(ev)) => {
                        if tx
                            .send(Msg::Engine {
                                bot: name.clone(),
                                generation,
                                event: Some(ev),
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                    Ok(None) => break,
                    Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                        log!("bot {name}: engine sent a bad line: {e}")
                    }
                    Err(e) => {
                        log!("bot {name}: engine output: {e}");
                        break;
                    }
                }
            }
            let _ = tx.send(Msg::Engine {
                bot: name,
                generation,
                event: None,
            });
        });
        log!("bot {bot}: engine started (pid {})", child.id());
        self.engines.insert(
            bot.to_owned(),
            Running {
                generation,
                child,
                input,
                ready: false,
                queued: Vec::new(),
                waiting: HashMap::new(),
                busy: false,
                compacting: 0,
            },
        );
        self.send_op(bot, open, Waiter::Service)?;
        self.send_op(bot, Op::Connectors { connectors }, Waiter::Connectors)
    }

    /// After a change to the list: a running engine gets it now, any other when it starts.
    fn connectors_changed(&mut self, bot: &str) {
        if self.engines.contains_key(bot) {
            let sent = self
                .store
                .connectors(bot)
                .map_err(internal)
                .and_then(|connectors| {
                    self.send_op(bot, Op::Connectors { connectors }, Waiter::Connectors)
                });
            if let Err((_, e)) = sent {
                log!("bot {bot}: connectors not sent: {e}");
            }
        }
        self.broadcast(&json!({"type": "connectors_changed", "bot": bot}));
    }

    fn on_event(&mut self, bot: &str, ev: Event) {
        let r = self.engines.get_mut(bot).expect("checked by generation");
        match &ev {
            Event::Ready { protocol } => {
                if *protocol != PROTOCOL {
                    log!(
                        "bot {bot}: engine speaks protocol {protocol}, not {PROTOCOL}; stopping it"
                    );
                    let _ = r.child.kill();
                    return;
                }
                r.ready = true;
                for (id, op) in std::mem::take(&mut r.queued) {
                    write_op(bot, r, &id, &op);
                }
                return;
            }
            Event::Response { id, error } => {
                let waiter = r.waiting.remove(id);
                let error = error.clone();
                match waiter {
                    Some(w) => self.on_response(bot, w, error),
                    None => log!("bot {bot}: response to unknown request {id}"),
                }
                return;
            }
            Event::TurnStart { .. } | Event::TurnEnd { .. } => {
                r.busy = matches!(ev, Event::TurnStart { .. });
                if !r.busy {
                    // A question the turn asked cannot outlive it (an aborted turn leaves it unanswered).
                    self.close_questions(bot);
                }
                self.broadcast(&json!({"type": "bots_changed"}));
            }
            Event::Session { session } => {
                if let Err(e) = self.store.set_session(bot, session) {
                    log!("bot {bot}: session not stored: {e}");
                }
            }
            Event::Question { question, .. } => {
                let entry = serde_json::to_value(&ev).expect("serializable");
                if let Err(e) = self.store.put_question(bot, question, &entry) {
                    log!("bot {bot}: question not stored: {e}");
                }
            }
            _ => {}
        }
        self.record(bot, serde_json::to_value(&ev).expect("serializable"));
    }

    fn close_questions(&mut self, bot: &str) {
        let open = match self.store.questions(bot) {
            Ok(open) => open,
            Err(e) => return log!("bot {bot}: store: {e}"),
        };
        for q in open {
            let q = q["question"].as_str().unwrap_or_default();
            match self.store.take_question(bot, q) {
                Ok(_) => {
                    self.broadcast(&json!({"type": "question_closed", "bot": bot, "question": q}))
                }
                Err(e) => log!("bot {bot}: question not closed: {e}"),
            }
        }
    }

    fn on_response(&mut self, bot: &str, waiter: Waiter, error: Option<String>) {
        match waiter {
            Waiter::Window {
                client,
                id,
                command,
                settles,
            } => {
                match (&settles, &error) {
                    (Settles::Prompt(p), _) => self.settle_prompt(bot, p, error.is_none()),
                    // A question is closed only once the engine took the answer.
                    (Settles::Question(q), None) => match self.store.take_question(bot, q) {
                        Ok(_) => self.broadcast(
                            &json!({"type": "question_closed", "bot": bot, "question": q}),
                        ),
                        Err(e) => log!("bot {bot}: question not closed: {e}"),
                    },
                    // The engine's conversation no longer has these entries, so history must not show them.
                    (Settles::Rewind(nonce), None) => {
                        match self.store.rewind_transcript(bot, nonce) {
                            Ok(()) => {
                                self.broadcast(&json!({"type": "history_changed", "bot": bot}))
                            }
                            Err(e) => log!("bot {bot}: history not rewound: {e}"),
                        }
                    }
                    (Settles::Compact, _) => {
                        if let Some(r) = self.engines.get_mut(bot) {
                            r.compacting = r.compacting.saturating_sub(1);
                            self.broadcast(&json!({"type": "bots_changed"}));
                        }
                    }
                    _ => {}
                }
                let result = match error {
                    Some(e) => Err(("engine_error", e)),
                    None => Ok(json!({})),
                };
                self.reply(client, &id, &command, result);
            }
            Waiter::Routine { routine, prompt } => {
                self.settle_prompt(bot, &prompt, error.is_none());
                if let Some(e) = error {
                    self.routine_failed(bot, &routine, e);
                }
            }
            Waiter::Connectors => {
                if let Some(e) = error {
                    log!("bot {bot}: engine refused the connectors: {e}");
                }
            }
            Waiter::Service => {
                let Some(e) = error else { return };
                // Only a refused `open` has a session to blame; a refused `start` is just logged.
                if !matches!(self.store.session(bot), Ok(Some(_))) {
                    log!("bot {bot}: engine refused to start a conversation: {e}");
                    return;
                }
                // A stale session would fail every later start too: forget it and begin anew.
                log!("bot {bot}: engine refused to open the conversation: {e}; starting a new one");
                if let Err(e) = self.store.clear_session(bot) {
                    log!("bot {bot}: session not cleared: {e}");
                    return;
                }
                if let Err((_, e)) =
                    self.send_op(bot, Op::Start { bot: bot.into() }, Waiter::Service)
                {
                    log!("bot {bot}: {e}");
                }
            }
        }
    }

    fn on_engine_gone(&mut self, bot: &str) {
        let Some(mut r) = self.engines.remove(bot) else {
            return;
        };
        let _ = r.child.kill();
        let status = r.child.wait();
        log!("bot {bot}: engine stopped ({status:?})");
        if r.busy {
            // Close the turn in the transcript, so history never shows a reply still running.
            let end = Event::TurnEnd {
                reason: proto::TurnEnd::Error,
                error: Some("the engine stopped".into()),
            };
            self.record(bot, serde_json::to_value(&end).expect("serializable"));
        }
        for e in [
            self.store.ledger_engine_stopped(bot),
            self.store.clear_questions(bot),
        ] {
            if let Err(e) = e {
                log!("bot {bot}: store: {e}");
            }
        }
        // Prompts still queued were never written: the engine cannot have them.
        let unwritten: Vec<String> = r.queued.iter().map(|(id, _)| id.clone()).collect();
        for (req, waiter) in r.waiting {
            if let Waiter::Window {
                settles: Settles::Prompt(p),
                ..
            }
            | Waiter::Routine { prompt: p, .. } = &waiter
                && unwritten.contains(&req)
                && let Err(e) = self.store.forget_ledger(bot, &p.nonce)
            {
                log!("bot {bot}: ledger not updated: {e}");
            }
            match waiter {
                Waiter::Window {
                    client,
                    id,
                    command,
                    ..
                } => self.reply(
                    client,
                    &id,
                    &command,
                    Err((
                        "engine_stopped",
                        "the engine stopped before answering".into(),
                    )),
                ),
                Waiter::Routine { routine, .. } => {
                    self.routine_failed(bot, &routine, "the engine stopped".into())
                }
                Waiter::Service | Waiter::Connectors => {}
            }
        }
        self.broadcast(&json!({"type": "engine_stopped", "bot": bot}));
        self.broadcast(&json!({"type": "bots_changed"}));
    }

    /// Validates and stores a routine from `set_routine`.
    fn set_routine(&mut self, bot: &str, raw: Value) -> Result<Routine, Refusal> {
        let invalid = |e: String| ("invalid_routine", e);
        let mut r: Routine = serde_json::from_value(raw).map_err(|e| invalid(e.to_string()))?;
        r.name = r.name.trim().to_owned();
        r.prompt = r.prompt.trim().to_owned();
        if r.name.is_empty() || r.name.chars().count() > 80 {
            return Err(invalid("`name` must be 1 to 80 characters".into()));
        }
        if r.prompt.is_empty() || r.prompt.chars().count() > 8000 {
            return Err(invalid("`prompt` must be 1 to 8000 characters".into()));
        }
        r.schedule = schedule::normalize(&r.schedule).map_err(invalid)?;
        let existing = self.store.routines(bot).map_err(internal)?;
        let old = match &r.id {
            Some(id) => Some(
                existing
                    .iter()
                    .find(|o| o.id.as_ref() == Some(id))
                    .ok_or(("no_such_routine", format!("no routine `{id}`")))?,
            ),
            None if existing.len() >= MAX_ROUTINES => {
                return Err(invalid(format!(
                    "a bot has at most {MAX_ROUTINES} routines"
                )));
            }
            None => None,
        };
        let now = now_ms();
        r.id.get_or_insert_with(|| sys::random_hex(4));
        r.anchor = match old
            .filter(|o| o.schedule == r.schedule)
            .and_then(|o| o.anchor.clone())
        {
            Some(anchor) => Some(anchor),
            None => Some(schedule::anchor_today(&r.schedule, now).map_err(invalid)?),
        };
        r.last_run = old.and_then(|o| o.last_run.clone());
        r.next_run_ms = None;
        if r.enabled {
            let slots = schedule::Slots::new(&r.schedule, r.anchor.as_deref().expect("set above"))
                .map_err(invalid)?;
            r.next_run_ms = slots.next_after(now);
        }
        self.store.put_routine(bot, &r).map_err(internal)?;
        Ok(r)
    }

    /// Sends every due routine. Returns how long the core may sleep.
    fn run_due_routines(&mut self) -> Duration {
        let now = now_ms();
        if now < self.next_due {
            return Duration::from_millis(self.next_due - now).min(TICK);
        }
        let all = match self.store.all_routines() {
            Ok(all) => all,
            Err(e) => {
                log!("routines not read: {e}");
                return TICK;
            }
        };
        let mut next_due = u64::MAX;
        for (bot, mut r) in all {
            let Some(due) = r.next_run_ms.filter(|_| r.enabled) else {
                continue;
            };
            if due > now {
                next_due = next_due.min(due);
                continue;
            }
            let id = r.id.clone().unwrap_or_default();
            let slots =
                match schedule::Slots::new(&r.schedule, r.anchor.as_deref().unwrap_or_default()) {
                    Ok(s) => s,
                    Err(e) => {
                        log!("bot {bot}: routine {id}: {e}; not scheduled");
                        r.next_run_ms = None;
                        let _ = self.store.put_routine(&bot, &r);
                        continue;
                    }
                };
            let (newest, next) = slots.catch_up(due, now);
            let outcome = if now - newest > CATCH_UP_MS {
                (RunOutcome::Missed, None)
            } else {
                let nonce = format!("routine-{id}-{newest}");
                match self.store.ledger(&bot, &nonce) {
                    Ok(None) => {
                        let text = format!("[Routine: {}]\n{}", r.name, r.prompt);
                        let prompt = Outgoing {
                            nonce: nonce.clone(),
                            text,
                        };
                        match self.send_prompt(&bot, prompt, |p| Waiter::Routine {
                            routine: id.clone(),
                            prompt: p,
                        }) {
                            Ok(()) => (RunOutcome::Sent, None),
                            Err((_, e)) => (RunOutcome::Failed, Some(e)),
                        }
                    }
                    Ok(Some(_)) => (
                        RunOutcome::Failed,
                        Some("this slot was already sent before a restart".into()),
                    ),
                    Err(e) => (RunOutcome::Failed, Some(e.to_string())),
                }
            };
            r.last_run = Some(RoutineRun {
                at_ms: now,
                outcome: outcome.0,
                error: outcome.1,
            });
            r.next_run_ms = next;
            if let Err(e) = self.store.put_routine(&bot, &r) {
                log!("bot {bot}: routine {id} not stored: {e}");
            }
            self.broadcast(&json!({"type": "routines_changed", "bot": bot}));
            if let Some(n) = next {
                next_due = next_due.min(n);
            }
        }
        self.next_due = next_due;
        Duration::from_millis(next_due.saturating_sub(now)).min(TICK)
    }

    fn routine_failed(&mut self, bot: &str, id: &str, error: String) {
        let Ok(routines) = self.store.routines(bot) else {
            return;
        };
        let Some(mut r) = routines.into_iter().find(|r| r.id.as_deref() == Some(id)) else {
            return;
        };
        r.last_run = Some(RoutineRun {
            at_ms: now_ms(),
            outcome: RunOutcome::Failed,
            error: Some(error),
        });
        if let Err(e) = self.store.put_routine(bot, &r) {
            log!("bot {bot}: routine {id} not stored: {e}");
        }
        self.broadcast(&json!({"type": "routines_changed", "bot": bot}));
    }
}

fn write_op(bot: &str, r: &mut Running, id: &str, op: &Op) {
    // A failed write means the engine is gone; its reader reports that and the waiters are answered then.
    if let Err(e) = r.input.send(id, op) {
        log!("bot {bot}: cannot write to the engine: {e}");
    }
}

fn check_connector(c: &Connector) -> Result<(), String> {
    let name_ok = (1..=40).contains(&c.name.len())
        && c.name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    if !name_ok {
        return Err("`name` must be 1 to 40 characters of a-z, 0-9, _ and -".into());
    }
    match &c.transport {
        Transport::Stdio { command, .. } if command.trim().is_empty() => {
            Err("`transport.command` is empty".into())
        }
        Transport::Http { url, .. }
            if !(url.starts_with("http://") || url.starts_with("https://")) =>
        {
            Err("`transport.url` must start with http:// or https://".into())
        }
        _ => Ok(()),
    }
}

fn encode(frame: &Value) -> Arc<[u8]> {
    let mut bytes = serde_json::to_vec(frame).expect("serializable");
    bytes.push(b'\n');
    bytes.into()
}

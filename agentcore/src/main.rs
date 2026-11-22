//! `agentcore`: Bate's own engine. Speaks the engine protocol (`PROTOCOL.md`) on
//! stdin/stdout in the bot's folder; one turn at a time runs on its own thread.

mod acp;
mod mcp;
mod memory;
mod model;
mod session;
mod skills;
mod tools;
mod turn;

use proto::{Event, MAX_EVENT_FRAME, MAX_OP_FRAME, Op, Request};
use session::Session;
use std::io::{self, BufReader};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use tools::{Control, lock};

/// Event output, shared by the operation loop and the turn thread.
#[derive(Clone)]
pub struct Out(Arc<Mutex<io::Stdout>>);

impl Out {
    pub fn send(&self, event: &Event) {
        if let Err(e) = proto::write_frame(&mut *lock(&self.0), event, MAX_EVENT_FRAME) {
            // The service reads our output; without it there is nothing left to do.
            eprintln!("cannot send an event: {e}");
            std::process::exit(1);
        }
    }
}

const NO_SESSION: &str = "no conversation is open: send start or open first";

enum Input {
    Op(Request),
    /// The worker thread is done (a turn, or `compact`) and hands its state back, with the
    /// event that ends its work: `turn_end`, or the response to `compact`.
    Idle(Work, Event),
    Closed,
}

/// What a turn works on. It is on the worker thread while one runs.
pub struct Work {
    pub session: Session,
    pub mcp: mcp::Servers,
}

/// Work in progress: the conversation is on the worker thread until `Idle`.
struct Running {
    control: Arc<Control>,
    answers: Sender<turn::Answer>,
    steer: Sender<String>,
}

struct Engine {
    out: Out,
    inputs: Sender<Input>,
    /// The bot's own folder: its conversations, notes and skills.
    dir: PathBuf,
    /// Where its tools work: the folder of its project, or `dir`.
    work: PathBuf,
    session: Option<Session>,
    /// Running MCP servers; they outlive a conversation.
    mcp: mcp::Servers,
    connectors: Vec<proto::connectors::Connector>,
    /// The bots this one may message, and whether it leads its project.
    peers: (Vec<proto::projects::Peer>, bool),
    /// Folders whose `skills/` are offered beside the bot's own; set by the service.
    skill_folders: Vec<PathBuf>,
    /// Names of the bot's own skills that are off; set by the service.
    skills_off: Vec<String>,
    /// Connector tools that are blocked, by the model's name; set by the service.
    blocked_tools: Vec<String>,
    /// From the user, through the service; first in the system prompt.
    instructions: String,
    /// Tool rounds a turn may take; set by the service.
    steps: u32,
    /// Whether the provider's web search tool is offered; set by the service.
    web_search: bool,
    /// Set once the provider refused that tool: it stays out until the engine ends.
    search_refused: Arc<AtomicBool>,
    usage_refused: Arc<AtomicBool>,
    running: Option<Running>,
    model: Option<String>,
    effort: Option<String>,
}

/// The model from the environment (`BATE_MODEL`), if any.
fn default_model() -> Option<String> {
    std::env::var("BATE_MODEL").ok().filter(|m| !m.is_empty())
}

/// The folder the tools work in: `BATE_WORK_DIR`, the folder of the bot's project, or else the
/// one the engine was started in.
fn work_dir() -> PathBuf {
    match std::env::var_os("BATE_WORK_DIR").filter(|dir| !dir.is_empty()) {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            dir.canonicalize().unwrap_or(dir)
        }
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}

fn main() {
    // Another engine in the same program: an installed agent in place of our own turns.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "acp") {
        std::process::exit(acp::run(&args[1..]));
    }
    let out = Out(Arc::new(Mutex::new(io::stdout())));
    out.send(&Event::Ready {
        protocol: proto::PROTOCOL,
    });
    let (inputs, rx) = mpsc::channel();
    let reader = inputs.clone();
    thread::spawn(move || {
        let mut stdin = BufReader::new(io::stdin());
        let mut buf = Vec::new();
        loop {
            match proto::read_frame::<Request>(&mut stdin, MAX_OP_FRAME, &mut buf) {
                Ok(Some(request)) => {
                    if reader.send(Input::Op(request)).is_err() {
                        return;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                    eprintln!("unreadable operation: {e}")
                }
                // End of input: the service stopped or crashed, so the engine exits too.
                end => {
                    if let Err(e) = end {
                        eprintln!("cannot read operations: {e}");
                    }
                    let _ = reader.send(Input::Closed);
                    return;
                }
            }
        }
    });
    let mut engine = Engine {
        out,
        inputs,
        dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        work: work_dir(),
        session: None,
        mcp: mcp::Servers::default(),
        connectors: Vec::new(),
        peers: (Vec::new(), false),
        skill_folders: Vec::new(),
        skills_off: Vec::new(),
        blocked_tools: Vec::new(),
        instructions: String::new(),
        steps: proto::STEP_LIMIT,
        web_search: true,
        search_refused: Arc::default(),
        usage_refused: Arc::default(),
        running: None,
        model: default_model(),
        effort: None,
    };
    while let Ok(input) = rx.recv() {
        match input {
            Input::Op(Request { id, op: Op::Stop }) => {
                engine.finish_turn(&rx);
                // The answer is the last thing written: nothing follows `stop`'s response.
                engine.respond(id, Ok(()));
                break;
            }
            Input::Op(Request { id, op }) => engine.handle(id, op),
            Input::Idle(Work { session, mcp }, end) => {
                // Sent here, not on the worker thread: a `prompt` that follows `turn_end` must find the engine idle.
                engine.running = None;
                engine.session = Some(session);
                engine.mcp = mcp;
                engine.out.send(&end);
            }
            Input::Closed => break,
        }
    }
    if let Some(r) = &engine.running {
        r.control.abort();
    }
}

impl Engine {
    fn respond(&self, id: String, result: Result<(), String>) {
        self.out.send(&Event::Response {
            id,
            error: result.err(),
        });
    }

    fn idle(&self) -> Result<(), String> {
        match self.running {
            Some(_) => Err("the engine is busy with a turn or a compaction".into()),
            None => Ok(()),
        }
    }

    /// An `open` or `start` failed: the engine has no conversation open now, not the one it had,
    /// which a prompt for the one asked for would go into. A turn that runs keeps its own.
    fn opening_failed(&mut self, id: String, error: String) {
        if self.running.is_none() {
            self.session = None;
        }
        self.respond(id, Err(error));
    }

    fn handle(&mut self, id: String, op: Op) {
        // What a `reply`, a `browser_result`, a `desktop_result` or an `apple_result` may answer: a
        // message, or a browser, desktop or Apple app request.
        let prefix = match op {
            Op::BrowserResult { .. } => 'b',
            Op::DesktopResult { .. } => 'd',
            Op::AppleResult { .. } => 'a',
            _ => 'm',
        };
        let saved = |e: io::Error| format!("cannot save the conversation: {e}");
        match op {
            Op::Start { .. } => match self
                .idle()
                .and_then(|()| Session::create(&self.dir).map_err(saved))
            {
                Ok(session) => {
                    self.respond(id, Ok(()));
                    self.out.send(&Event::Session {
                        session: session.id.clone(),
                    });
                    self.session = Some(session);
                }
                Err(e) => self.opening_failed(id, e),
            },
            Op::Open { session, .. } => {
                let opened = self.idle().and_then(|()| {
                    let mut s = Session::open(&self.dir, &session)
                        .map_err(|e| format!("cannot open conversation {session}: {e}"))?;
                    let interrupted = s.close_interrupted().map_err(saved)?;
                    Ok((s, interrupted))
                });
                match opened {
                    Ok((session, interrupted)) => {
                        self.respond(id, Ok(()));
                        self.out.send(&Event::Session {
                            session: session.id.clone(),
                        });
                        for call in interrupted {
                            let output = session::INTERRUPTED.into();
                            self.out.send(&Event::ToolEnd {
                                call,
                                output,
                                is_error: true,
                                undo: None,
                            });
                        }
                        self.session = Some(session);
                    }
                    Err(e) => self.opening_failed(id, e),
                }
            }
            Op::Prompt { nonce, text } => match self.begin_turn() {
                Ok((turn, mut work)) => {
                    // The response goes first: the service records the prompt before the turn's events.
                    self.respond(id, Ok(()));
                    let inputs = self.inputs.clone();
                    thread::spawn(move || {
                        let end = turn.run(&mut work, nonce, text);
                        let _ = inputs.send(Input::Idle(work, end));
                    });
                }
                Err(e) => self.respond(id, Err(e)),
            },
            Op::Steer { text } => {
                let sent = match &self.running {
                    Some(r) => r.steer.send(text).map_err(|_| "the turn has ended".into()),
                    None => Err("no turn is running".into()),
                };
                self.respond(id, sent);
            }
            Op::Abort => {
                // Respond first, so the response precedes the aborted turn's events.
                self.respond(id, Ok(()));
                if let Some(r) = &self.running {
                    r.control.abort();
                }
            }
            Op::Answer { question, choice } => {
                let open = self.running.as_ref().map(|r| lock(&r.control.question));
                match open {
                    // A message the turn waits on (`m…`) takes a `reply`, never an `answer`.
                    Some(mut open)
                        if open.as_deref() == Some(question.as_str())
                            && question.starts_with('q') =>
                    {
                        if turn::OPTIONS.contains(&choice.as_str()) {
                            // A question takes one answer: a second one is refused, not accepted
                            // and dropped.
                            *open = None;
                            drop(open);
                            // Respond before the turn thread wakes, so the response precedes the tool's events.
                            self.respond(id, Ok(()));
                            let r = self.running.as_ref().expect("checked above");
                            let _ = r.answers.send((question, Ok(choice)));
                        } else {
                            drop(open);
                            self.respond(
                                id,
                                Err(format!("`{choice}` is not one of {:?}", turn::OPTIONS)),
                            );
                        }
                    }
                    open => {
                        drop(open);
                        self.respond(id, Err(format!("question {question} is not open")));
                    }
                }
            }
            Op::SetModel { model, effort } => {
                // Empty: back to the default.
                self.model = Some(model).filter(|m| !m.is_empty()).or_else(default_model);
                self.effort = effort;
                self.respond(id, Ok(()));
            }
            Op::Rewind { entry, fork } => {
                let rewound = self.idle().and_then(|()| {
                    let session = self.session.as_mut().ok_or(NO_SESSION)?;
                    session.rewind(&entry, fork).map_err(|e| e.to_string())?;
                    Ok(session.id.clone())
                });
                match rewound {
                    Ok(session) => {
                        self.respond(id, Ok(()));
                        if fork {
                            self.out.send(&Event::Session { session });
                        }
                    }
                    Err(e) => self.respond(id, Err(e)),
                }
            }
            // A model call, so it runs like a turn: `abort` and `stop` reach it, and the response comes when it is done.
            Op::Compact => match self.begin_turn() {
                Ok((turn, mut work)) => {
                    let inputs = self.inputs.clone();
                    thread::spawn(move || {
                        let error = turn.compact_now(&mut work).err();
                        let _ = inputs.send(Input::Idle(work, Event::Response { id, error }));
                    });
                }
                Err(e) => self.respond(id, Err(e)),
            },
            Op::Connectors { connectors } => {
                self.connectors = connectors;
                self.respond(id, Ok(()));
            }
            Op::Peers { peers, leads } => {
                self.peers = (peers, leads);
                self.respond(id, Ok(()));
            }
            Op::SkillFolders { folders } => {
                self.skill_folders = folders;
                self.respond(id, Ok(()));
            }
            Op::SkillsOff { names } => {
                self.skills_off = names;
                self.respond(id, Ok(()));
            }
            Op::BlockedTools { names } => {
                self.blocked_tools = names;
                self.respond(id, Ok(()));
            }
            Op::Instructions { text } => {
                self.instructions = text;
                self.respond(id, Ok(()));
            }
            Op::WebSearch { on } => {
                self.web_search = on;
                self.respond(id, Ok(()));
            }
            Op::StepLimit { steps } => match steps {
                0 => self.respond(id, Err("`steps` must be at least 1".into())),
                steps => {
                    self.steps = steps;
                    self.respond(id, Ok(()));
                }
            },
            Op::Reply {
                message,
                text,
                error,
            }
            // What the browser, the desktop or an app did is waited on as a message is (`b…`, `d…` or `a…` in place of `m…`).
            | Op::BrowserResult {
                request: message,
                output: text,
                error,
            }
            | Op::DesktopResult {
                request: message,
                output: text,
                error,
            }
            | Op::AppleResult {
                request: message,
                output: text,
                error,
            } => {
                let open = self.running.as_ref().map(|r| lock(&r.control.question));
                                match open {
                    Some(mut open)
                        if open.as_deref() == Some(message.as_str())
                            && message.starts_with(prefix) =>
                    {
                        *open = None;
                        drop(open);
                        self.respond(id, Ok(()));
                        let reply = match (text, error) {
                            (Some(text), None) => Ok(text),
                            (_, error) => Err(error.unwrap_or_else(|| "no reply".into())),
                        };
                        let r = self.running.as_ref().expect("checked above");
                        let _ = r.answers.send((message, reply));
                    }
                    open => {
                        drop(open);
                        self.respond(id, Err(format!("message {message} is not open")));
                    }
                }
            }
            Op::Token {
                request,
                url,
                key,
                api,
                model,
                context,
                error,
            } => {
                let open = self.running.as_ref().map(|r| lock(&r.control.question));
                match open {
                    Some(mut open)
                        if open.as_deref() == Some(request.as_str())
                            && request.starts_with("tok") =>
                    {
                        *open = None;
                        drop(open);
                        self.respond(id, Ok(()));
                        let res = match (url, error) {
                            (Some(u), None) => {
                                let k = key.unwrap_or_default();
                                let a = api.unwrap_or_default();
                                let m = model.unwrap_or_default();
                                let c = context.map(|c| c.to_string()).unwrap_or_default();
                                Ok(format!("{u}\n{k}\n{a}\n{m}\n{c}"))
                            }
                            (_, Some(e)) => Err(e),
                            _ => Err("no model server is set up".into()),
                        };
                        let r = self.running.as_ref().expect("checked above");
                        let _ = r.answers.send((request, res));
                    }
                    open => {
                        drop(open);
                        self.respond(id, Err(format!("token request {request} is not open")));
                    }
                }
            }
            Op::Stop => unreachable!("handled by the operation loop"),
        }
    }

    /// Aborts a running turn and waits a bounded time for its `turn_end`.
    fn finish_turn(&mut self, rx: &mpsc::Receiver<Input>) {
        let Some(r) = &self.running else { return };
        r.control.abort();
        while let Ok(input) = rx.recv_timeout(std::time::Duration::from_secs(5)) {
            match input {
                Input::Idle(_, end) => return self.out.send(&end),
                Input::Op(Request { id, .. }) => {
                    self.respond(id, Err("the engine is stopping".into()))
                }
                Input::Closed => return,
            }
        }
    }

    fn begin_turn(&mut self) -> Result<(turn::Turn, Work), String> {
        self.idle()?;
        let model = self.model.clone();
        let provider =
            model::Provider::new(self.search_refused.clone(), self.usage_refused.clone());
        let context = match std::env::var("BATE_MODEL_CONTEXT") {
            Ok(tokens) => Some(
                tokens
                    .parse()
                    .map_err(|_| "BATE_MODEL_CONTEXT is not a number of tokens")?,
            ),
            Err(_) => None,
        };
        let session = self.session.take().ok_or(NO_SESSION)?;
        let control = Arc::new(Control::default());
        let (answers, answers_rx) = mpsc::channel();
        let (steer, steer_rx) = mpsc::channel();
        self.running = Some(Running {
            control: control.clone(),
            answers,
            steer,
        });
        let turn = turn::Turn {
            out: self.out.clone(),
            dir: self.dir.clone(),
            work: self.work.clone(),
            provider,
            model,
            effort: self.effort.clone(),
            context,
            connectors: self.connectors.clone(),
            peers: self.peers.0.clone(),
            leads: self.peers.1,
            skill_folders: self.skill_folders.clone(),
            skills_off: self.skills_off.clone(),
            blocked_tools: self.blocked_tools.clone(),
            instructions: self.instructions.clone(),
            steps: self.steps,
            web_search: self.web_search,
            control,
            answers: answers_rx,
            steer: steer_rx,
        };
        let mcp = std::mem::take(&mut self.mcp);
        Ok((turn, Work { session, mcp }))
    }
}

//! `agentcore acp <agent program> [arguments]`: an engine that runs an installed agent
//! through the Agent Client Protocol. To the service it is an engine like any other
//! (`PROTOCOL.md`); to the agent it is an ACP client: JSON-RPC, one message per line, over the
//! agent's standard input and output. The agent keeps its own model, login and tools; this
//! only translates.

use crate::tools::OUTPUT_CAP;
use proto::connectors::{Connector, Transport};
use proto::permissions::{APPROVE, DENY, Kind};
use proto::{Event, MAX_EVENT_FRAME, MAX_OP_FRAME, Op, PROTOCOL, Request, TurnEnd};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::io::Read;
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;

/// The ACP version this program speaks.
const ACP: u64 = 1;
/// Most bytes of one line from the agent.
const MAX_LINE: usize = MAX_EVENT_FRAME;

enum Input {
    Op(Request),
    /// The service closed our input.
    Closed,
    Agent(Value),
    /// The agent closed its output, or sent a line that is too long.
    AgentGone,
}

/// What an answer from the agent, by the id we gave the request, is the answer to.
enum Asked {
    Initialize,
    /// `session/new` or `session/load`, for the service's request with this id.
    Session {
        op: String,
        loaded: Option<String>,
    },
    Prompt,
}

/// A permission the agent waits for.
struct Permission {
    /// The agent's own id for its request.
    request: Value,
    allow: Option<Value>,
    reject: Option<Value>,
}

struct Bridge {
    agent: ChildStdin,
    next: u64,
    asked: HashMap<u64, Asked>,
    session: Option<String>,
    /// Between `turn_start` and `turn_end`.
    turn: bool,
    /// The agent replays an old conversation: its updates are history, not a turn.
    loading: bool,
    can_load: bool,
    takes_http: bool,
    /// How to sign in, as the agent says it.
    sign_in: String,
    connectors: Vec<Connector>,
    permissions: HashMap<String, Permission>,
    questions: u64,
    /// Tool calls shown as started.
    started: Vec<String>,
    /// The turn was aborted: however the agent ends it, it ends `aborted`.
    aborted: bool,
}

/// The agent's process, so that it is not left running when this one ends.
static AGENT_PID: AtomicI32 = AtomicI32::new(0);

fn out(event: &Event) {
    let mut stdout = io::stdout().lock();
    if let Err(e) = proto::write_frame(&mut stdout, event, MAX_EVENT_FRAME) {
        // The service is gone (or would not take the line): nothing left to translate for.
        eprintln!("cannot send an event: {e}");
        let pid = AGENT_PID.load(Ordering::SeqCst);
        if pid > 0 {
            // SAFETY: a plain signal to our own child.
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
        std::process::exit(1);
    }
}

fn respond(id: String, result: Result<(), String>) {
    out(&Event::Response {
        id,
        error: result.err(),
    });
}

/// The text in one ACP content block, or in a list of a tool call's content.
fn text_of(content: &Value) -> String {
    match content {
        Value::Array(items) => {
            let parts: Vec<String> = items
                .iter()
                .map(text_of)
                .filter(|t| !t.is_empty())
                .collect();
            parts.join("\n")
        }
        _ if content["type"] == "text" => content["text"].as_str().unwrap_or("").to_owned(),
        // A tool call wraps its content once more.
        _ if content["type"] == "content" => text_of(&content["content"]),
        _ if content["type"] == "diff" => {
            format!("changed {}", content["path"].as_str().unwrap_or("a file"))
        }
        _ => String::new(),
    }
}

/// What Bate's permissions call the agent's kind of tool, and what exactly it acts on.
fn action(call: &Value) -> (Option<Kind>, String) {
    let title = call["title"].as_str().unwrap_or("a tool").to_owned();
    let input = &call["rawInput"];
    let path = || {
        let found = call["locations"][0]["path"].as_str();
        let found = found
            .or(input["path"].as_str())
            .or(input["file_path"].as_str());
        found.or(input["filePath"].as_str()).map(str::to_owned)
    };
    match call["kind"].as_str() {
        Some("read" | "search") => (Some(Kind::Read), path().unwrap_or(title)),
        Some("edit" | "delete" | "move") => (Some(Kind::Write), path().unwrap_or(title)),
        Some("execute") => {
            let command = input["command"].as_str().map(str::to_owned);
            (Some(Kind::Run), command.unwrap_or(title))
        }
        // No kind Bate knows: the user is asked each time.
        _ => (None, title),
    }
}

impl Bridge {
    fn send(&mut self, message: &Value) {
        let mut line = message.to_string();
        line.push('\n');
        // A failed write means the agent is gone; its closed output ends the program.
        let _ = self
            .agent
            .write_all(line.as_bytes())
            .and_then(|()| self.agent.flush());
    }

    fn request(&mut self, method: &str, params: Value, asked: Asked) {
        self.next += 1;
        self.asked.insert(self.next, asked);
        self.send(&json!({"jsonrpc": "2.0", "id": self.next, "method": method, "params": params}));
    }

    /// The bot's connectors as ACP wants them; an agent that takes no http ones gets none.
    fn servers(&self) -> Value {
        let pairs = |map: &std::collections::BTreeMap<String, String>| -> Value {
            map.iter()
                .map(|(name, value)| json!({"name": name, "value": value}))
                .collect()
        };
        let servers = self
            .connectors
            .iter()
            .filter(|c| c.enabled)
            .filter_map(|c| match &c.transport {
                Transport::Stdio { command, args, env } => Some(
                    json!({"name": c.name, "command": command, "args": args, "env": pairs(env)}),
                ),
                Transport::Http { url, headers } if self.takes_http => Some(
                    json!({"type": "http", "name": c.name, "url": url, "headers": pairs(headers)}),
                ),
                Transport::Http { .. } => None,
            });
        servers.collect()
    }

    fn op(&mut self, id: String, op: Op) {
        let cwd = std::env::current_dir().unwrap_or_default();
        match op {
            Op::Start { .. } if self.turn => respond(id, Err("a turn is running".into())),
            Op::Open { .. } if self.turn => respond(id, Err("a turn is running".into())),
            Op::Start { .. } => {
                let params = json!({"cwd": cwd, "mcpServers": self.servers()});
                self.request(
                    "session/new",
                    params,
                    Asked::Session {
                        op: id,
                        loaded: None,
                    },
                );
            }
            Op::Open { session, .. } if self.can_load => {
                self.loading = true;
                let params =
                    json!({"sessionId": session, "cwd": cwd, "mcpServers": self.servers()});
                let asked = Asked::Session {
                    op: id,
                    loaded: Some(session),
                };
                self.request("session/load", params, asked);
            }
            Op::Open { .. } => respond(
                id,
                Err("this agent cannot reopen an earlier conversation".into()),
            ),
            Op::Prompt { nonce, text } => {
                let ready = match (&self.session, self.turn) {
                    (_, true) => Err("the engine is busy with a turn".to_owned()),
                    (None, _) => Err("no conversation is open: send start first".to_owned()),
                    (Some(session), false) => Ok(session.clone()),
                };
                match ready {
                    Ok(session) => {
                        respond(id, Ok(()));
                        self.turn = true;
                        out(&Event::TurnStart { nonce });
                        let prompt = json!([{"type": "text", "text": text}]);
                        let params = json!({"sessionId": session, "prompt": prompt});
                        self.request("session/prompt", params, Asked::Prompt);
                    }
                    Err(e) => respond(id, Err(e)),
                }
            }
            // shortcut: the turn ends when the agent answers the cancellation, as ACP requires of
            // it; an agent that never does keeps the bot busy until its engine is stopped.
            Op::Abort => {
                respond(id, Ok(()));
                if self.turn {
                    self.aborted = true;
                    self.close_permissions();
                    let params = json!({"sessionId": self.session});
                    self.send(
                        &json!({"jsonrpc": "2.0", "method": "session/cancel", "params": params}),
                    );
                }
            }
            Op::Answer { question, choice } => {
                match self.permissions.remove(&question) {
                    Some(p) if choice == APPROVE || choice == DENY => {
                        respond(id, Ok(()));
                        let option = if choice == APPROVE { p.allow } else { p.reject };
                        let outcome = match option {
                            Some(option) => json!({"outcome": "selected", "optionId": option}),
                            // The agent offered no way to say this: the request is called off.
                            None => json!({"outcome": "cancelled"}),
                        };
                        self.send(&json!({"jsonrpc": "2.0", "id": p.request, "result": {"outcome": outcome}}));
                    }
                    Some(p) => {
                        self.permissions.insert(question, p);
                        respond(
                            id,
                            Err(format!("`{choice}` is not one of [{APPROVE:?}, {DENY:?}]")),
                        );
                    }
                    None => respond(id, Err(format!("question {question} is not open"))),
                }
            }
            Op::Connectors { connectors } => {
                // The agent is told its servers when a conversation starts or is reopened.
                self.connectors = connectors;
                respond(id, Ok(()));
            }
            // The agent has its own model, chosen in the agent; and no other bots to reach.
            Op::SetModel { .. } | Op::Peers { .. } => respond(id, Ok(())),
            Op::Steer { .. } => {
                respond(id, Err("this agent takes no message while it works".into()))
            }
            Op::Rewind { .. } => respond(
                id,
                Err("this agent cannot go back in a conversation".into()),
            ),
            Op::Compact => respond(
                id,
                Err("this agent summarises its context by itself".into()),
            ),
            Op::Reply { message, .. } => respond(id, Err(format!("message {message} is not open"))),
            Op::Stop => unreachable!("handled by the loop"),
        }
    }

    /// Calls off every permission the agent waits for, in the agent and for the service.
    fn close_permissions(&mut self) {
        for (_, p) in std::mem::take(&mut self.permissions) {
            let result = json!({"outcome": {"outcome": "cancelled"}});
            self.send(&json!({"jsonrpc": "2.0", "id": p.request, "result": result}));
        }
    }

    fn end_turn(&mut self, reason: TurnEnd, error: Option<String>) {
        self.close_permissions();
        self.turn = false;
        self.aborted = false;
        // A call the agent never finished is shown as ended with the turn.
        for call in std::mem::take(&mut self.started) {
            out(&Event::ToolEnd {
                call,
                output: "the turn ended before this call did".into(),
                is_error: true,
            });
        }
        out(&Event::TurnEnd { reason, error });
    }

    /// A message from the agent. An error: the agent cannot be used.
    fn agent(&mut self, message: Value) -> Result<(), String> {
        let method = message["method"].as_str();
        match (method, message.get("id").filter(|id| !id.is_null())) {
            (Some("session/update"), _) => self.update(&message["params"]["update"]),
            (Some("session/request_permission"), Some(id)) => {
                self.permission(id.clone(), &message["params"])
            }
            // We offer the agent no files and no terminals of ours: it has its own.
            (Some(method), Some(id)) => {
                let error = json!({"code": -32601, "message": format!("{method} is not offered")});
                self.send(&json!({"jsonrpc": "2.0", "id": id, "error": error}));
            }
            (Some(_), None) => {}
            (None, Some(id)) => {
                let asked = id.as_u64().and_then(|id| self.asked.remove(&id));
                let result = match message.get("error") {
                    Some(error) => Err(error["message"].as_str().unwrap_or("it failed").to_owned()),
                    None => Ok(&message["result"]),
                };
                match asked {
                    Some(asked) => self.answered(asked, result)?,
                    None => eprintln!("the agent answered a request that was not made: {id}"),
                }
            }
            (None, None) => {}
        }
        Ok(())
    }

    fn answered(&mut self, asked: Asked, result: Result<&Value, String>) -> Result<(), String> {
        match asked {
            Asked::Initialize => {
                let result = result.map_err(|e| format!("the agent refused to start: {e}"))?;
                if result["protocolVersion"] != ACP {
                    return Err(format!(
                        "the agent speaks ACP version {}, not {ACP}",
                        result["protocolVersion"]
                    ));
                }
                let can = &result["agentCapabilities"];
                self.can_load = can["loadSession"] == true;
                self.takes_http = can["mcpCapabilities"]["http"] == true;
                let ways: Vec<&str> = result["authMethods"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|m| m["description"].as_str().or(m["name"].as_str()))
                    .collect();
                self.sign_in = ways.join("; or ");
                out(&Event::Ready { protocol: PROTOCOL });
            }
            Asked::Session { op, loaded } => {
                self.loading = false;
                let session = match (result, loaded) {
                    (Ok(_), Some(session)) => Ok(session),
                    (Ok(result), None) => result["sessionId"]
                        .as_str()
                        .map(str::to_owned)
                        .ok_or("the agent opened no conversation".to_owned()),
                    // The agent's own login: say how, in the agent's words.
                    (Err(e), _) if !self.sign_in.is_empty() => Err(format!(
                        "{e}. If the agent is not signed in: {}",
                        self.sign_in
                    )),
                    (Err(e), _) => Err(e),
                };
                match session {
                    Ok(session) => {
                        respond(op, Ok(()));
                        out(&Event::Session {
                            session: session.clone(),
                        });
                        self.session = Some(session);
                    }
                    Err(e) => respond(op, Err(e)),
                }
            }
            Asked::Prompt if self.aborted => self.end_turn(TurnEnd::Aborted, None),
            Asked::Prompt => match result.map(|r| r["stopReason"].as_str().unwrap_or("")) {
                Ok("end_turn") => self.end_turn(TurnEnd::Done, None),
                Ok("cancelled") => self.end_turn(TurnEnd::Aborted, None),
                Ok(other) => {
                    let why = format!("the agent stopped: {}", other.replace('_', " "));
                    self.end_turn(TurnEnd::Error, Some(why));
                }
                Err(e) => self.end_turn(TurnEnd::Error, Some(e)),
            },
        }
        Ok(())
    }

    fn start_tool(&mut self, call: &Value) {
        let Some(id) = call["toolCallId"].as_str() else {
            return;
        };
        if self.started.iter().any(|started| started == id) {
            return;
        }
        self.started.push(id.to_owned());
        let mut args = Map::new();
        for key in ["title", "rawInput", "locations"] {
            if let Some(value) = call.get(key).filter(|v| !v.is_null()) {
                args.insert(key.to_owned(), value.clone());
            }
        }
        out(&Event::ToolStart {
            call: id.to_owned(),
            tool: call["kind"].as_str().unwrap_or("tool").to_owned(),
            args,
        });
    }

    fn update(&mut self, update: &Value) {
        if self.loading || !self.turn {
            return;
        }
        let delta = || text_of(&update["content"]);
        match update["sessionUpdate"].as_str() {
            Some("agent_message_chunk") => out(&Event::Text { delta: delta() }),
            Some("agent_thought_chunk") => out(&Event::Thinking { delta: delta() }),
            Some("tool_call") => self.start_tool(update),
            Some("tool_call_update") => {
                let failed = update["status"] == "failed";
                if !(failed || update["status"] == "completed") {
                    return;
                }
                // Some agents report a call only when it is over.
                self.start_tool(update);
                let Some(id) = update["toolCallId"].as_str() else {
                    return;
                };
                let mut output = text_of(&update["content"]);
                if output.is_empty() && !update["rawOutput"].is_null() {
                    output = match &update["rawOutput"] {
                        Value::String(text) => text.clone(),
                        other => other.to_string(),
                    };
                }
                if output.len() > OUTPUT_CAP {
                    output.truncate(output.floor_char_boundary(OUTPUT_CAP));
                    output.push_str("\n[output cut at 64 KiB]");
                }
                self.started.retain(|started| started != id);
                out(&Event::ToolEnd {
                    call: id.to_owned(),
                    output,
                    is_error: failed,
                });
            }
            // Plans, commands, modes: nothing the transcript shows.
            _ => {}
        }
    }

    /// The agent asks whether a tool may run: a question for the service, which decides from
    /// the bot's permissions or asks the user.
    fn permission(&mut self, request: Value, params: &Value) {
        if !self.turn {
            let result = json!({"outcome": {"outcome": "cancelled"}});
            return self.send(&json!({"jsonrpc": "2.0", "id": request, "result": result}));
        }
        let call = &params["toolCall"];
        self.start_tool(call);
        let option = |kinds: [&str; 2]| {
            let options = params["options"].as_array()?;
            // This once, if the agent offers it: what is remembered, Bate remembers.
            let found = kinds
                .iter()
                .find_map(|kind| options.iter().find(|o| o["kind"] == *kind))?;
            Some(found["optionId"].clone())
        };
        self.questions += 1;
        let question = format!("q{}", self.questions);
        let permission = Permission {
            request,
            allow: option(["allow_once", "allow_always"]),
            reject: option(["reject_once", "reject_always"]),
        };
        self.permissions.insert(question.clone(), permission);
        let (kind, target) = action(call);
        out(&Event::Question {
            question,
            title: call["title"].as_str().unwrap_or("Use a tool").to_owned(),
            options: vec![APPROVE.into(), DENY.into()],
            target: kind.map(|_| target),
            kind,
        });
    }
}

fn start(program: &str, args: &[String]) -> io::Result<(Child, Bridge, mpsc::Receiver<Input>)> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // The agent's own diagnostics go where ours go: into the service's log.
        .stderr(Stdio::inherit())
        .spawn()?;
    AGENT_PID.store(child.id() as i32, Ordering::SeqCst);
    let agent = child.stdin.take().expect("piped");
    let from_agent = child.stdout.take().expect("piped");
    let (tx, rx) = mpsc::channel();
    let agent_tx: Sender<Input> = tx.clone();
    thread::spawn(move || {
        let mut reader = BufReader::new(from_agent);
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = (&mut reader)
                .take(MAX_LINE as u64 + 1)
                .read_until(b'\n', &mut line);
            if !matches!(read, Ok(n) if n > 0 && n <= MAX_LINE) {
                let _ = agent_tx.send(Input::AgentGone);
                return;
            }
            // Not JSON: some agents print a banner. Skipped.
            if let Ok(message) = serde_json::from_slice::<Value>(&line) {
                let _ = agent_tx.send(Input::Agent(message));
            }
        }
    });
    thread::spawn(move || {
        let mut stdin = io::stdin().lock();
        let mut buf = Vec::new();
        loop {
            match proto::read_frame::<Request>(&mut stdin, MAX_OP_FRAME, &mut buf) {
                Ok(Some(request)) => {
                    let _ = tx.send(Input::Op(request));
                }
                Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                    eprintln!("a request that is not one: {e}")
                }
                _ => {
                    let _ = tx.send(Input::Closed);
                    return;
                }
            }
        }
    });
    let bridge = Bridge {
        agent,
        next: 0,
        asked: HashMap::new(),
        session: None,
        turn: false,
        loading: false,
        can_load: false,
        takes_http: false,
        sign_in: String::new(),
        connectors: Vec::new(),
        permissions: HashMap::new(),
        questions: 0,
        started: Vec::new(),
        aborted: false,
    };
    Ok((child, bridge, rx))
}

/// Runs the agent `args[0]` with the rest as its arguments until the service says stop, or
/// either side goes away. Gives the exit code.
pub fn run(args: &[String]) -> i32 {
    let Some((program, rest)) = args.split_first() else {
        eprintln!("usage: agentcore acp <agent program> [arguments]");
        return 2;
    };
    let (mut child, mut bridge, inputs) = match start(program, rest) {
        Ok(started) => started,
        Err(e) => {
            eprintln!("cannot start the agent `{program}`: {e}");
            return 1;
        }
    };
    // We ask for nothing of ours: the agent reads, writes and runs by itself, in its folder.
    let caps = json!({"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false});
    let params = json!({"protocolVersion": ACP, "clientCapabilities": caps});
    bridge.request("initialize", params, Asked::Initialize);
    let mut code = 0;
    for input in inputs {
        match input {
            Input::Op(Request { id, op: Op::Stop }) => {
                respond(id, Ok(()));
                break;
            }
            Input::Op(Request { id, op }) => bridge.op(id, op),
            Input::Closed => break,
            Input::Agent(message) => {
                if let Err(e) = bridge.agent(message) {
                    eprintln!("{e}");
                    code = 1;
                    break;
                }
            }
            Input::AgentGone => {
                eprintln!("the agent `{program}` stopped");
                if bridge.turn {
                    bridge.end_turn(TurnEnd::Error, Some("the agent stopped".into()));
                }
                code = 1;
                break;
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    code
}

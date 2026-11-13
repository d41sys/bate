//! One turn: model call, tool calls, model call, … until the model stops calling tools.

use crate::mcp::Servers;
use crate::model::{Ask, Delta, Provider, Reply};
use crate::session::Session;
use crate::tools::{self, Control, lock};
use crate::{Out, Work};
use proto::connectors::Connector;
use proto::groups::Peer;
use proto::permissions::{APPROVE, DENY, Kind};
use proto::{Event, TurnEnd};
use serde_json::{Map, Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

pub struct Turn {
    pub out: Out,
    pub dir: PathBuf,
    pub provider: Provider,
    pub model: String,
    pub effort: Option<String>,
    /// The model's context size in tokens; compaction starts at four fifths of it.
    pub context: usize,
    /// The bot's MCP servers as last sent by the service.
    pub connectors: Vec<Connector>,
    pub control: Arc<Control>,
    /// The bots this one may message, and whether it leads a group.
    pub peers: Vec<Peer>,
    pub leads: bool,
    /// What the user told this bot to be and do.
    pub instructions: String,
    /// From `answer` and `reply`, already checked against what the turn waits on.
    pub answers: Receiver<Answer>,
    pub steer: Receiver<String>,
}

/// What the turn waited for: the id of the question or message, then the choice or the
/// other bot's reply, or why there is none.
pub type Answer = (String, Result<String, String>);

pub const OPTIONS: [&str; 2] = [APPROVE, DENY];
/// Most recent messages a compaction leaves as they are.
const KEEP: usize = 4;
const SUMMARISE: &str = "Summarise the conversation so far for your own later use: what the user wants, \
    what was decided, what was found, which files changed, and what is left to do. \
    Answer with the summary only and do not call tools.";

/// How deep sub-agents nest: one at this depth cannot start another.
pub const MAX_DEPTH: usize = 5;
/// The tool that starts a sub-agent.
const AGENT: &str = "agent";
const SUB_AGENT: &str = "\n\nYou are a sub-agent: another agent handed you the one task below and \
    sees nothing but your last message. Do the task, then answer with the result alone.";

/// The tool that messages another bot.
const MESSAGE: &str = "message_bot";
const BROWSER: &str = "browser";

/// The engine's own tools, then the connectors', then `agent` while `depth` allows one more,
/// and `message_bot` when there is a bot to message.
fn tool_list(mcp: &Servers, depth: usize, peers: bool) -> Value {
    let mut tools = tools::definitions();
    if let Value::Array(list) = &mut tools {
        list.extend(mcp.definitions().cloned());
        list.push(json!({"type": "function", "function": {"name": BROWSER,
            "description": "Use your own web browser, which the user can watch. `open` a web address, `look` at the page, `click` or `type` into one of the numbered elements that every result lists, or go `back`. Each result is the page as it is after the action: its title, address, text and elements. If the user takes the browser over, you wait until they hand it back.",
            "parameters": {"type": "object", "properties": {
                "action": {"type": "string", "enum": ["open", "look", "click", "type", "back"]},
                "url": {"type": "string", "description": "For `open`: an http or https address"},
                "element": {"type": "integer", "description": "For `click` and `type`: the element's number"},
                "text": {"type": "string", "description": "For `type`: what to put in the field"},
                "submit": {"type": "boolean", "description": "For `type`: send the form afterwards"}},
                "required": ["action"]}}}));
        if peers {
            list.push(json!({"type": "function", "function": {"name": MESSAGE,
                "description": "Send a message to another bot and wait for its reply. It works on the message in its own folder with its own tools and knows nothing of this conversation: put everything it needs in `text`. A bot does one thing at a time; if it is busy you are told so.",
                "parameters": {"type": "object", "properties": {
                    "bot": {"type": "string", "description": "The bot's name"},
                    "text": {"type": "string"}}, "required": ["bot", "text"]}}}));
        }
        if depth < MAX_DEPTH {
            list.push(json!({"type": "function", "function": {"name": AGENT,
                "description": "Hand one self-contained task to a sub-agent: a copy of you with the same tools and working folder that knows nothing of this conversation. It works alone; its final answer comes back as the result. Put everything it needs to know in `task`.",
                "parameters": {"type": "object", "properties": {"task": {"type": "string"}}, "required": ["task"]}}}));
        }
    }
    tools
}

/// The message that records what the model said and which tools it called.
fn assistant(text: &str, calls: &[crate::model::Call]) -> Value {
    let mut message = json!({"role": "assistant", "content": text});
    if !calls.is_empty() {
        message["tool_calls"] = calls
            .iter()
            .map(|c| json!({"id": c.id, "type": "function", "function": {"name": c.name, "arguments": c.arguments}}))
            .collect();
    }
    message
}

fn saved(e: std::io::Error) -> String {
    format!("cannot save the conversation: {e}")
}

impl Turn {
    /// Runs the turn and returns its `turn_end`; the caller sends it once the conversation is back.
    pub fn run(&self, work: &mut Work, nonce: String, text: String) -> Event {
        let Work { session, mcp } = work;
        self.out.send(&Event::TurnStart {
            nonce: nonce.clone(),
        });
        let turn = session
            .mark_prompt(&nonce)
            .map_err(saved)
            .and_then(|()| self.turn(session, mcp, &nonce, text));
        let (reason, error) = match turn {
            Ok(reason) => (reason, None),
            Err(e) => (TurnEnd::Error, Some(e)),
        };
        Event::TurnEnd { reason, error }
    }

    fn turn(
        &self,
        session: &mut Session,
        mcp: &mut Servers,
        nonce: &str,
        text: String,
    ) -> Result<TurnEnd, String> {
        // Numbers the tool calls a server sent without an id.
        let mut unnamed = 0;
        let system = self.system();
        mcp.sync(&self.connectors, &self.dir, &self.control);
        let tools = self.tools(mcp, 0);
        session
            .push(json!({"role": "user", "content": text}))
            .map_err(saved)?;
        loop {
            while let Ok(text) = self.steer.try_recv() {
                session
                    .push(json!({"role": "user", "content": text}))
                    .map_err(saved)?;
            }
            if self.control.aborted() {
                return Ok(TurnEnd::Aborted);
            }
            if session.tokens() > self.context / 5 * 4 {
                // A summary that fails must not end the turn, or every later prompt would fail the same way.
                if let Err(e) = self.compact(session, &system, &tools) {
                    eprintln!("the context was not compacted: {e}");
                }
                if self.control.aborted() {
                    return Ok(TurnEnd::Aborted);
                }
            }
            let ask = self.ask_model(&system, &session.messages, None, &tools);
            let (url, key) = self.get_token()?;
            let reply = self.provider.stream(
                Some(&url),
                key.as_deref(),
                &ask,
                self.control.aborted_flag(),
                |d| {
                    self.out.send(&match d {
                        Delta::Text(t) => Event::Text { delta: t.into() },
                        Delta::Thinking(t) => Event::Thinking { delta: t.into() },
                    })
                },
            )?;
            let Reply {
                text,
                mut calls,
                aborted,
            } = reply;
            for call in calls.iter_mut().filter(|c| c.id.is_empty()) {
                // Some local servers send no id; each call in the conversation still needs its own,
                // and the message count would repeat after a compaction.
                call.id = format!("call_{nonce}_{unnamed}");
                unnamed += 1;
            }
            if !text.is_empty() || !calls.is_empty() {
                session.push(assistant(&text, &calls)).map_err(saved)?;
            }
            if aborted {
                return Ok(TurnEnd::Aborted);
            }
            if calls.is_empty() {
                // A `steer` that came during the last reply was accepted: the model must see it.
                let late: Vec<String> = self.steer.try_iter().collect();
                if late.is_empty() {
                    return Ok(TurnEnd::Done);
                }
                for text in late {
                    session
                        .push(json!({"role": "user", "content": text}))
                        .map_err(saved)?;
                }
            }
            // Every call gets a result, even after an abort: the conversation must stay well-formed.
            for call in &calls {
                session
                    .push(self.run_call(mcp, call, 0, ""))
                    .map_err(saved)?;
            }
        }
    }

    /// Runs one tool call and gives the message with its result. `shown` goes before the
    /// call's id in the events, to tell a sub-agent's calls from the conversation's own.
    fn run_call(
        &self,
        mcp: &mut Servers,
        call: &crate::model::Call,
        depth: usize,
        shown: &str,
    ) -> Value {
        let args = match serde_json::from_str::<Value>(&call.arguments) {
            Ok(Value::Object(args)) => Ok(args),
            _ => Err(format!(
                "the arguments are not a JSON object: {}",
                call.arguments
            )),
        };
        let id = format!("{shown}{}", call.id);
        self.out.send(&Event::ToolStart {
            call: id.clone(),
            tool: call.name.clone(),
            args: args.clone().unwrap_or_default(),
        });
        let result = if self.control.aborted() {
            Err("Not run: the turn was aborted.".into())
        } else {
            args.and_then(|args| self.call(mcp, &call.name, &args, depth))
        };
        let (output, is_error) = match result {
            Ok(out) => (out, false),
            Err(out) => (out, true),
        };
        self.out.send(&Event::ToolEnd {
            call: id,
            output: output.clone(),
            is_error,
        });
        json!({"role": "tool", "tool_call_id": call.id, "content": output})
    }

    /// A sub-agent at `depth`: the same model, tools and folder, and a conversation of its own
    /// that starts with `task` and is not kept. Gives its last message. Its tool calls are
    /// shown and asked about like any other; what it says in between is not.
    // shortcut: no compaction and no steering in here, so a sub-agent that outgrows the
    // context ends with the model server's error; share `turn`'s loop if one needs either.
    fn sub_agent(&self, mcp: &mut Servers, task: &str, depth: usize) -> Result<String, String> {
        static STARTED: AtomicU64 = AtomicU64::new(0);
        let n = STARTED.fetch_add(1, Ordering::Relaxed) + 1;
        let system = self.system() + SUB_AGENT;
        let tools = self.tools(mcp, depth);
        let mut messages = vec![json!({"role": "user", "content": task})];
        let mut unnamed = 0;
        loop {
            if self.control.aborted() {
                return Err("Aborted by the user".into());
            }
            let ask = self.ask_model(&system, &messages, None, &tools);
            let (url, key) = self.get_token()?;
            let Reply {
                text,
                mut calls,
                aborted,
            } = self.provider.stream(
                Some(&url),
                key.as_deref(),
                &ask,
                self.control.aborted_flag(),
                |_| {},
            )?;
            if aborted {
                return Err("Aborted by the user".into());
            }
            for call in calls.iter_mut().filter(|c| c.id.is_empty()) {
                call.id = format!("call_agent{n}_{unnamed}");
                unnamed += 1;
            }
            if calls.is_empty() {
                return match text.trim() {
                    "" => Err("The sub-agent ended without an answer.".into()),
                    answer => Ok(answer.to_owned()),
                };
            }
            messages.push(assistant(&text, &calls));
            for call in &calls {
                messages.push(self.run_call(mcp, call, depth, &format!("agent{n}/")));
            }
        }
    }

    fn tools(&self, mcp: &Servers, depth: usize) -> Value {
        tool_list(mcp, depth, !self.peers.is_empty())
    }

    fn system(&self) -> String {
        let instructions = match self.instructions.trim() {
            "" => String::new(),
            text => format!("\n\nThe user's instructions for you:\n{text}"),
        };
        format!(
            "You are a helpful assistant. Your working folder is {}. Use the tools to read, change and search files and to run commands there. To show the user a file you made there (a web page, a diagram, a picture), link it by its path in that folder, such as [chart](chart.html): the window opens it in a frame with no network.{instructions}{}{}{}",
            self.dir.display(),
            self.peers_prompt(),
            crate::skills::prompt(&self.dir),
            crate::memory::prompt(&self.dir)
        )
    }

    /// The part of the system prompt about the other bots; empty when there are none.
    fn peers_prompt(&self) -> String {
        if self.peers.is_empty() {
            return String::new();
        }
        let mut text = format!(
            "\n\nYou can message other bots with the {MESSAGE} tool. Each has its own folder, \
             tools and conversation, and replies when it is done:"
        );
        for peer in &self.peers {
            let role = if peer.lead { " (leads your group)" } else { "" };
            // One line each: a name is the user's text.
            let name = peer.name.replace(char::is_control, " ");
            text.push_str(&format!("\n- {name}{role}"));
        }
        if self.leads {
            text.push_str(
                "\nYou lead a group: hand out tasks to the others and put their results \
                 together. You cannot stop or redirect a bot once it works on a task.",
            );
        }
        text
    }

    /// Sends `text` to the bot the model named and waits for its reply.
    fn message(&self, args: &Map<String, Value>) -> Result<String, String> {
        let name = args.get("bot").and_then(Value::as_str).unwrap_or("").trim();
        // shortcut: two bots with one name are told apart only by id, which the model is not
        // shown; list ids beside names if users give bots the same name.
        let Some(peer) = self
            .peers
            .iter()
            .find(|p| p.id == name || p.name.eq_ignore_ascii_case(name))
        else {
            let names: Vec<_> = self.peers.iter().map(|p| p.name.as_str()).collect();
            return Err(format!(
                "No bot `{name}`. You can message: {}.",
                names.join(", ")
            ));
        };
        let text = match args.get("text").and_then(Value::as_str) {
            Some(text) if !text.trim().is_empty() => text,
            _ => return Err("`text` must say what the message is.".into()),
        };
        // Another bot acts with its own permissions, so reaching it is asked about like a
        // connector's tool: once for this bot, whatever the message.
        let shown: String = text.chars().take(200).collect();
        self.allowed(
            Kind::Connector,
            format!("bot:{}", peer.id),
            format!("Message {}: {shown}", peer.name),
        )?;
        let id = self.wait_on("m");
        self.out.send(&Event::Message {
            message: id.clone(),
            to: peer.id.clone(),
            text: text.to_owned(),
        });
        match self.answer(&id) {
            Some(Ok(reply)) => Ok(reply),
            Some(Err(e)) => Err(format!("No reply from {}: {e}", peer.name)),
            None => Err(format!(
                "Aborted by the user. {} may still be working on the message.",
                peer.name
            )),
        }
    }

    fn ask_model<'a>(
        &'a self,
        system: &'a str,
        messages: &'a [Value],
        last: Option<&'a Value>,
        tools: &'a Value,
    ) -> Ask<'a> {
        Ask {
            model: &self.model,
            effort: self.effort.as_deref(),
            system,
            messages,
            last,
            tools,
        }
    }

    /// `compact` on request.
    pub fn compact_now(&self, work: &mut Work) -> Result<(), String> {
        self.compact(&mut work.session, &self.system(), &self.tools(&work.mcp, 0))
    }

    /// Asks the model to summarise all but the last `KEEP` messages and puts the summary in their
    /// place. Does nothing when there is nothing that old.
    fn compact(&self, session: &mut Session, system: &str, tools: &Value) -> Result<(), String> {
        let messages = &session.messages;
        let mut cut = messages.len().saturating_sub(KEEP);
        // A tool result stays with the call it answers.
        while cut > 0 && messages[cut]["role"] == "tool" {
            cut -= 1;
        }
        // Nothing old, or only an earlier summary: summarising that again would only lose detail.
        if cut == 0 || (cut == 1 && session.starts_with_summary()) {
            return Ok(());
        }
        // Same system text and tools as a turn, so a provider's prompt cache still applies.
        let instruction = json!({"role": "user", "content": SUMMARISE});
        let ask = self.ask_model(system, &messages[..cut], Some(&instruction), tools);
        let (url, key) = self.get_token()?;
        let reply = self.provider.stream(
            Some(&url),
            key.as_deref(),
            &ask,
            self.control.aborted_flag(),
            |_| {},
        )?;
        if reply.aborted {
            return Err("aborted".into());
        }
        let summary = reply.text.trim();
        if summary.is_empty() {
            return Err("the model returned no summary".into());
        }
        let kept = messages.len() - cut;
        session.compact(summary, kept).map_err(saved)
    }

    fn call(
        &self,
        mcp: &mut Servers,
        tool: &str,
        args: &Map<String, Value>,
        depth: usize,
    ) -> Result<String, String> {
        if tool == AGENT {
            if depth >= MAX_DEPTH {
                return Err(format!(
                    "Not started: sub-agents nest at most {MAX_DEPTH} deep."
                ));
            }
            return match args.get("task").and_then(Value::as_str) {
                Some(task) if !task.trim().is_empty() => self.sub_agent(mcp, task, depth + 1),
                _ => Err("`task` must say what the sub-agent is to do.".into()),
            };
        }
        if tool == MESSAGE && !self.peers.is_empty() {
            return self.message(args);
        }
        if tool == BROWSER {
            return self.browse(args);
        }
        // Every action is asked about: the service decides from the bot's permissions, and
        // asks the user where they say so. What is asked about is what then runs.
        let connector = mcp.title(tool);
        let action = match &connector {
            Some(title) => Some((Kind::Connector, tool.to_owned(), title.clone())),
            None => tools::action(tool, args),
        };
        if let Some((kind, target, title)) = action {
            self.allowed(kind, target, title)?;
        }
        match connector {
            Some(_) => mcp.call(tool, args, &self.control),
            None => tools::run(tool, args, &self.dir, &self.control),
        }
    }

    /// One action in the bot's browser, which the service runs. Asked about like a connector's
    /// tool: once for the browser, whatever the action.
    fn browse(&self, args: &Map<String, Value>) -> Result<String, String> {
        let arg = |key: &str| {
            args.get(key).map_or(String::new(), |v| match v {
                Value::String(text) => text.chars().take(200).collect(),
                other => other.to_string(),
            })
        };
        let title = match arg("action").as_str() {
            "open" => format!("Browser: open {}", arg("url")),
            "look" => "Browser: look at the page".to_owned(),
            "click" => format!("Browser: click element {}", arg("element")),
            "type" => format!(
                "Browser: type into element {}: {}",
                arg("element"),
                arg("text")
            ),
            "back" => "Browser: go back".to_owned(),
            other => {
                return Err(format!(
                    "`{other}` is not an action: open, look, click, type or back."
                ));
            }
        };
        self.allowed(Kind::Connector, BROWSER.to_owned(), title)?;
        let id = self.wait_on("b");
        self.out.send(&Event::Browser {
            request: id.clone(),
            action: args.clone(),
        });
        match self.answer(&id) {
            Some(result) => result,
            None => Err("Aborted by the user".into()),
        }
    }

    /// Asks whether the action may run; an error is what the model is told when it may not.
    fn allowed(&self, kind: Kind, target: String, title: String) -> Result<(), String> {
        let id = self.wait_on("q");
        self.out.send(&Event::Question {
            question: id.clone(),
            title,
            options: OPTIONS.map(String::from).to_vec(),
            kind: Some(kind),
            target: Some(target),
        });
        match self.answer(&id) {
            Some(Ok(choice)) if choice == APPROVE => Ok(()),
            Some(_) => Err("The user denied this action.".into()),
            None => Err("Aborted by the user".into()),
        }
    }

    /// Marks a new question or message as what the turn waits on, and gives its id. Its own
    /// id, never the model's call id: a model may repeat those, and an answer must not fit a
    /// later question.
    /// Asks the service for the current endpoint and token for the upcoming model call.
    fn get_token(&self) -> Result<(String, Option<String>), String> {
        static TOK: AtomicU64 = AtomicU64::new(0);
        let id = format!("tok{}", TOK.fetch_add(1, Ordering::Relaxed) + 1);
        *lock(&self.control.question) = Some(id.clone());
        self.out.send(&Event::Token {
            request: id.clone(),
        });
        match self.answer(&id) {
            Some(Ok(val)) => {
                let (url, key) = val.split_once('\n').unwrap_or((&val, ""));
                let key = if key.is_empty() {
                    None
                } else {
                    Some(key.to_owned())
                };
                Ok((url.to_owned(), key))
            }
            Some(Err(e)) => Err(e),
            None => Err("Aborted by the user".into()),
        }
    }

    fn wait_on(&self, prefix: &str) -> String {
        static ASKED: AtomicU64 = AtomicU64::new(0);
        let id = format!("{prefix}{}", ASKED.fetch_add(1, Ordering::Relaxed) + 1);
        *lock(&self.control.question) = Some(id.clone());
        id
    }

    /// Waits for the answer to `id`; `None` when the turn was aborted meanwhile.
    fn answer(&self, id: &str) -> Option<Result<String, String>> {
        let answer = loop {
            match self.answers.recv_timeout(Duration::from_millis(100)) {
                Ok((to, answer)) if to == id => break Some(answer),
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) if !self.control.aborted() => {}
                Err(_) => break None,
            }
        };
        *lock(&self.control.question) = None;
        answer
    }
}

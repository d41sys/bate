//! One turn: model call, tool calls, model call, … until the model stops calling tools.

use crate::mcp::Servers;
use crate::model::{Api, Ask, Delta, Provider, RETRIES, Reply};
use crate::session::Session;
use crate::tools::{self, Control, lock};
use crate::{Out, Work};
use proto::connectors::Connector;
use proto::permissions::{APPROVE, DENY, Kind};
use proto::projects::Peer;
use proto::{Event, TurnEnd};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

pub struct Turn {
    pub out: Out,
    /// The bot's own folder: its conversations, notes and skills.
    pub dir: PathBuf,
    /// Where the tools work: the folder of the bot's project, or `dir`.
    pub work: PathBuf,
    pub provider: Provider,
    /// The model chosen for the bot; without one, the service names it with the token.
    pub model: Option<String>,
    pub effort: Option<String>,
    /// `BATE_MODEL_CONTEXT`: the model's context size in tokens. It goes before the size that
    /// comes with the `token` answer; with neither, `CONTEXT`. Compaction starts at four
    /// fifths of it.
    pub context: Option<usize>,
    /// The bot's MCP servers as last sent by the service.
    pub connectors: Vec<Connector>,
    pub control: Arc<Control>,
    /// The bots this one may message, and whether it leads its project.
    pub peers: Vec<Peer>,
    pub leads: bool,
    /// Folders whose `skills/` are offered beside the bot's own.
    pub skill_folders: Vec<PathBuf>,
    /// Names of the bot's own skills that are off: not offered to the model.
    pub skills_off: Vec<String>,
    /// What the user told this bot to be and do.
    pub instructions: String,
    /// Tool rounds a turn may take.
    pub steps: u32,
    /// Offer the provider's web search tool, where the provider has one.
    pub web_search: bool,
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
/// The context size, in tokens, taken for a model that nobody named one for.
const CONTEXT: usize = 32_000;
/// A `token` answer: the model server, its key, the API it speaks, the model to call, and
/// that model's context size in tokens if the service knows it.
type Token = (String, Option<String>, Api, String, Option<usize>);
const SUMMARISE: &str = "Summarise the conversation so far for your own later use: what the user wants, \
    what was decided, what was found, which files changed, and what is left to do. \
    Answer with the summary only and do not call tools.";
/// Parts, in lower case, of what model servers answer when the conversation is more than the
/// model holds: the list pi keeps (`packages/ai/src/utils/overflow.ts`), without the entries
/// that also fit a rate limit.
const OVERFLOW: [&str; 21] = [
    "prompt is too long",
    "prompt too long",
    "request_too_large",
    "exceeds the context window",
    "maximum context length",
    "context window exceeded",
    "context window exceeds limit",
    "exceeds the maximum number of tokens",
    "maximum prompt length",
    "reduce the length of the messages",
    "maximum allowed input length",
    "longer than the model's context length",
    "exceeds the available context size",
    "greater than the context length",
    "exceeded model token limit",
    "configured context size",
    "context_length_exceeded",
    "context length exceeded",
    "context_window_exceeded",
    "token limit exceeded",
    "the model server answered 413",
];

/// How deep sub-agents nest: one at this depth cannot start another.
pub const MAX_DEPTH: usize = 5;
/// The tool that starts a sub-agent.
const AGENT: &str = "agent";
const SUB_AGENT: &str = "\n\nYou are a sub-agent: another agent handed you the one task below and \
    sees nothing but your last message. Do the task, then answer with the result alone.";

/// The tool that messages another bot.
const MESSAGE: &str = "message_bot";
const BROWSER: &str = "browser";
/// The tool that sees and controls other apps through the helper app.
const DESKTOP: &str = "desktop";
/// The tool that states a task's steps; the window shows them as a checklist.
const PLAN: &str = "plan";

/// The engine's own tools, then the connectors', then `agent` while `depth` allows one more,
/// and `message_bot` when there is a bot to message.
fn tool_list(mcp: &Servers, depth: usize, peers: bool) -> Value {
    let mut tools = tools::definitions();
    if let Value::Array(list) = &mut tools {
        list.extend(mcp.definitions().cloned());
        list.push(json!({"type": "function", "function": {"name": BROWSER,
            "description": "Use your own web browser, which the user can watch. `open` a web address, `look` at the page, `click` or `type` into one of the numbered elements that every result lists, `scroll` up or down by one screen, `press` a key (optionally on an element), `select` an option of a dropdown element by its text, go `back` or `forward`, `reload` the page, or `wait` a few seconds for a slow page. Each result is the page as it is after the action: its title, address, text and elements. If the user takes the browser over, you wait until they hand it back.",
            "parameters": {"type": "object", "properties": {
                "action": {"type": "string", "enum": ["open", "look", "click", "type", "scroll", "press", "select", "back", "forward", "reload", "wait"]},
                "url": {"type": "string", "description": "For `open`: an http or https address"},
                "element": {"type": "integer", "description": "For `click`, `type` and `select`, and optionally `press`: the element's number"},
                "text": {"type": "string", "description": "For `type`: what to put in the field"},
                "submit": {"type": "boolean", "description": "For `type`: send the form afterwards"},
                "direction": {"type": "string", "enum": ["up", "down"], "description": "For `scroll`"},
                "key": {"type": "string", "enum": ["Enter", "Tab", "Escape", "Backspace", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "PageUp", "PageDown"], "description": "For `press`"},
                "option": {"type": "string", "description": "For `select`: the option's visible text"},
                "seconds": {"type": "integer", "minimum": 1, "maximum": 10, "description": "For `wait`"}},
                "required": ["action"]}}}));
        list.push(json!({"type": "function", "function": {"name": DESKTOP,
            "description": "See and control other apps on this Mac without taking the focus from the user. `apps` lists the apps and their windows (window ids). `look` reads a window as a tree of numbered elements; `press` or `set_value` act on an element by its number from the latest `look` of that window, and `type` types text into an element or into whatever has the focus. `screenshot` saves a picture of a window in your folder and gives its path. Look again after any action: the numbers change. If the user lacks a macOS permission, the result says which.",
            "parameters": {"type": "object", "properties": {
                "action": {"type": "string", "enum": ["apps", "look", "screenshot", "press", "set_value", "type"]},
                "app": {"type": "string", "description": "The app's name, as `apps` lists it (all but `apps`)"},
                "window": {"type": "integer", "description": "A window id from `apps` (default: the app's front window)"},
                "element": {"type": "integer", "description": "For `press` and `set_value`, and optionally `type`: the element's number"},
                "text": {"type": "string", "description": "For `type`"},
                "value": {"type": "string", "description": "For `set_value`"}},
                "required": ["action"]}}}));
        list.extend(proto::apple::APPS.iter().map(apple_tool));
        if peers {
            list.push(json!({"type": "function", "function": {"name": MESSAGE,
                "description": "Send a message to another bot and wait for its reply. It works on the message in its own folder with its own tools and knows nothing of this conversation: put everything it needs in `text`. A bot does one thing at a time; if it is busy you are told so.",
                "parameters": {"type": "object", "properties": {
                    "bot": {"type": "string", "description": "The bot's name"},
                    "text": {"type": "string"}}, "required": ["bot", "text"]}}}));
        }
        list.push(json!({"type": "function", "function": {"name": PLAN,
            "description": "Show the user the steps of a task that takes three or more, and keep the list current while you work: mark a step `done` as soon as it is, before you start the next. Send the whole list every time.",
            "parameters": {"type": "object", "properties": {"steps": {"type": "array", "items": {"type": "object",
                "properties": {
                    "step": {"type": "string", "description": "A few words"},
                    "status": {"type": "string", "enum": ["pending", "in_progress", "done"]}},
                "required": ["step", "status"]}}},
                "required": ["steps"]}}}));
        if depth < MAX_DEPTH {
            list.push(json!({"type": "function", "function": {"name": AGENT,
                "description": "Hand one self-contained task to a sub-agent: a copy of you with the same tools and working folder that knows nothing of this conversation. It works alone; its final answer comes back as the result. Put everything it needs to know in `task`.",
                "parameters": {"type": "object", "properties": {"task": {"type": "string"}}, "required": ["task"]}}}));
        }
    }
    tools
}

/// Checks a `plan` call. The steps are kept nowhere but in the call itself: the window shows
/// them from its arguments, and the model reads them back from the conversation.
fn plan(args: &Map<String, Value>) -> Result<String, String> {
    let steps = match args.get("steps").and_then(Value::as_array) {
        Some(steps) if !steps.is_empty() => steps,
        _ => return Err("`steps` must list at least one step.".into()),
    };
    for step in steps {
        if step["step"].as_str().is_none_or(|s| s.trim().is_empty()) {
            return Err("Every step needs a `step` that says what it is.".into());
        }
        if !matches!(
            step["status"].as_str(),
            Some("pending" | "in_progress" | "done")
        ) {
            return Err("Every step needs a `status`: pending, in_progress or done.".into());
        }
    }
    Ok("Plan updated.".into())
}

/// The definition of one Apple app's tool.
fn apple_tool(app: &proto::apple::App) -> Value {
    let actions: Vec<_> = app.reads.iter().chain(app.changes).collect();
    let mut properties = Map::new();
    properties.insert("action".into(), json!({"type": "string", "enum": actions}));
    for (name, what) in app.fields {
        properties.insert(
            (*name).into(),
            json!({"type": "string", "description": what}),
        );
    }
    let about = format!(
        "{} Every send or change is shown to the user for approval first.",
        app.about
    );
    json!({"type": "function", "function": {"name": app.tool, "description": about,
        "parameters": {"type": "object", "properties": properties, "required": ["action"]}}})
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

/// What the model sent, as the event that shows it.
fn event(d: Delta) -> Event {
    match d {
        Delta::Text(t) => Event::Text { delta: t.into() },
        Delta::Thinking(t) => Event::Thinking { delta: t.into() },
        Delta::Search { .. } => unreachable!("shown as a tool call by `Turn::stream`"),
        Delta::Retry {
            attempt,
            wait,
            error,
        } => Event::Retry {
            attempt,
            of: RETRIES,
            wait_ms: wait.as_millis() as u64,
            error: error.into(),
        },
    }
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
            Ok(TurnEnd::Limit) => (
                TurnEnd::Limit,
                Some(format!("stopped after {} tool rounds", self.steps)),
            ),
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
        // Model replies that called tools, run so far.
        let mut rounds = 0;
        // Whether the conversation was already summarised for the model call that comes next.
        let mut shrunk = false;
        let system = self.system();
        for (name, tools) in mcp.sync(&self.connectors, &self.dir, &self.control) {
            self.out.send(&Event::ConnectorLoaded {
                name,
                tools: tools as u32,
            });
        }
        let tools = self.tools(mcp, 0);
        session
            .push(json!({"role": "user", "content": text}))
            .map_err(saved)?;
        loop {
            while let Ok(text) = self.steer.try_recv() {
                self.steered(session, text)?;
            }
            if self.control.aborted() {
                return Ok(TurnEnd::Aborted);
            }
            // The conversation ends on tool results, so one more prompt goes on from here.
            if rounds >= self.steps {
                return Ok(TurnEnd::Limit);
            }
            // Asked for first: with it comes how much the model holds.
            let mut token = self.get_token();
            let holds = token.as_ref().ok().and_then(|t| t.4);
            if session.tokens() > self.context.or(holds).unwrap_or(CONTEXT) / 5 * 4 {
                // A summary that fails must not end the turn, or every later prompt would fail the same way.
                if let Err(e) = self.compact(session, &system, &tools) {
                    eprintln!("the context was not compacted: {e}");
                }
                if self.control.aborted() {
                    return Ok(TurnEnd::Aborted);
                }
                // The summary was a model call of its own, and can have been a long one.
                token = self.get_token();
            }
            let (url, key, api, model, _) = match token {
                Ok(token) => token,
                Err(_) if self.control.aborted() => return Ok(TurnEnd::Aborted),
                Err(e) => return Err(e),
            };
            let ask = self.ask_model(&model, &system, &session.messages, None, &tools);
            let reply = match self.stream(&url, key.as_deref(), api, &ask, |d| {
                self.out.send(&event(d))
            }) {
                // More than the model holds, whatever size was taken for it: the older part
                // becomes a summary and the model is asked once more.
                Err(e)
                    if !shrunk && OVERFLOW.iter().any(|part| e.to_lowercase().contains(part)) =>
                {
                    let before = session.tokens();
                    if let Err(summary) = self.compact(session, &system, &tools) {
                        return Err(format!("{e} (and the summary failed: {summary})"));
                    }
                    // Nothing was old enough to summarise.
                    if session.tokens() >= before {
                        return Err(e);
                    }
                    shrunk = true;
                    continue;
                }
                reply => reply?,
            };
            shrunk = false;
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
                    self.steered(session, text)?;
                }
            }
            rounds += u32::from(!calls.is_empty());
            // Every call gets a result, even after an abort: the conversation must stay well-formed.
            for call in &calls {
                session
                    .push(self.run_call(mcp, call, 0, ""))
                    .map_err(saved)?;
            }
        }
    }

    /// Puts a `steer` message into the conversation and tells the service where it was taken.
    fn steered(&self, session: &mut Session, text: String) -> Result<(), String> {
        session
            .push(json!({"role": "user", "content": text}))
            .map_err(saved)?;
        self.out.send(&Event::Steered { text });
        Ok(())
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
        // The copy goes before the result, which names it.
        let undo = lock(&self.control.kept).take().and_then(|k| {
            let id = crate::session::new_id().ok()?;
            self.out.send(&Event::Kept {
                id: id.clone(),
                path: k.path,
                earlier: k.earlier,
                after: k.after,
            });
            Some(id)
        });
        self.out.send(&Event::ToolEnd {
            call: id,
            output: output.clone(),
            is_error,
            undo,
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
        let mut rounds = 0;
        loop {
            if self.control.aborted() {
                return Err("Aborted by the user".into());
            }
            if rounds >= self.steps {
                return Err(format!(
                    "The sub-agent stopped after {} tool rounds.",
                    self.steps
                ));
            }
            let (url, key, api, model, _) = self.get_token()?;
            let ask = self.ask_model(&model, &system, &messages, None, &tools);
            let Reply {
                text,
                mut calls,
                aborted,
            } = self.stream(&url, key.as_deref(), api, &ask, |_| {})?;
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
            rounds += 1;
            messages.push(assistant(&text, &calls));
            for call in &calls {
                messages.push(self.run_call(mcp, call, depth, &format!("agent{n}/")));
            }
        }
    }

    /// A model call; each wait before it is asked again is shown.
    fn stream(
        &self,
        url: &str,
        key: Option<&str>,
        api: Api,
        ask: &Ask,
        mut on: impl FnMut(Delta),
    ) -> Result<Reply, String> {
        let abort = self.control.aborted_flag();
        self.provider
            .stream(url, key, api, ask, abort, |d| match d {
                Delta::Retry { .. } => self.out.send(&event(d)),
                // The provider ran it: shown like a tool call of ours, done at once.
                Delta::Search { id, query, failed } => {
                    let mut args = Map::new();
                    args.insert("query".into(), query.into());
                    self.out.send(&Event::ToolStart {
                        call: id.into(),
                        tool: "web_search".into(),
                        args,
                    });
                    self.out.send(&Event::ToolEnd {
                        call: id.into(),
                        output: String::new(),
                        is_error: failed,
                        undo: None,
                    });
                }
                d => on(d),
            })
    }

    fn tools(&self, mcp: &Servers, depth: usize) -> Value {
        tool_list(mcp, depth, !self.peers.is_empty())
    }

    fn system(&self) -> String {
        let instructions = match self.instructions.trim() {
            "" => String::new(),
            text => format!("\n\nThe user's instructions for you:\n{text}"),
        };
        // In a project the bot's own folder is not where it works: what is in it is named
        // by its full path, and the project's notes come after its own.
        let (own, named, project) = if self.work == self.dir {
            (String::new(), Path::new(""), String::new())
        } else {
            (
                format!(
                    " Your own folder is {}: attachments and screenshots are named by their full path there. To show one to the user, link it as `attachments/<name>`.",
                    self.dir.display()
                ),
                self.dir.as_path(),
                crate::memory::project_prompt(&self.work),
            )
        };
        format!(
            "You are a helpful assistant. Your working folder is {}. Use the tools to read, change and search files and to run commands there. To show the user a file you made there (a web page, a diagram, a picture), link it by its path in that folder, such as [chart](chart.html): the window opens it in a frame with no network.{own}{instructions}{}{}{}{project}",
            self.work.display(),
            self.peers_prompt(),
            crate::skills::prompt(&self.dir, named, &self.skill_folders, &self.skills_off),
            crate::memory::prompt(&self.dir, named)
        )
    }

    /// The part of the system prompt about the other bots; empty when there are none.
    fn peers_prompt(&self) -> String {
        if self.peers.is_empty() {
            return String::new();
        }
        let mut text = format!(
            "\n\nYou can message other bots with the {MESSAGE} tool. Each has its own tools and \
             conversation, and replies when it is done:"
        );
        for peer in &self.peers {
            let role = if peer.lead {
                " (leads your project)"
            } else {
                ""
            };
            // One line each: a name is the user's text.
            let name = peer.name.replace(char::is_control, " ");
            text.push_str(&format!("\n- {name}{role}"));
        }
        if self.leads {
            text.push_str(
                "\nYou lead this project: hand out tasks to the others and put their results \
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
        model: &'a str,
        system: &'a str,
        messages: &'a [Value],
        last: Option<&'a Value>,
        tools: &'a Value,
    ) -> Ask<'a> {
        Ask {
            model,
            effort: self.effort.as_deref(),
            system,
            messages,
            last,
            tools,
            web_search: self.web_search,
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
        let (url, key, api, model, _) = self.get_token()?;
        let mut ask = self.ask_model(&model, system, &messages[..cut], Some(&instruction), tools);
        // The summary is no place for a search.
        ask.web_search = false;
        let reply = self.stream(&url, key.as_deref(), api, &ask, |_| {})?;
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
        // Nothing to ask about: a plan changes nothing.
        if tool == PLAN {
            return plan(args);
        }
        if tool == BROWSER {
            return self.browse(args);
        }
        if tool == DESKTOP {
            return self.desktop(args);
        }
        if let Some(app) = proto::apple::app(tool) {
            return self.apple(app, args);
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
            None => tools::run(tool, args, &self.work, &self.control),
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
            "forward" => "Browser: go forward".to_owned(),
            "reload" => "Browser: reload the page".to_owned(),
            "scroll" => format!("Browser: scroll {}", arg("direction")),
            "press" => match args.get("element") {
                Some(_) => format!(
                    "Browser: press {} on element {}",
                    arg("key"),
                    arg("element")
                ),
                None => format!("Browser: press {}", arg("key")),
            },
            "select" => format!(
                "Browser: choose {} in element {}",
                arg("option"),
                arg("element")
            ),
            "wait" => format!("Browser: wait {} seconds", arg("seconds")),
            other => {
                return Err(format!(
                    "`{other}` is not an action: open, look, click, type, scroll, press, select, back, forward, reload or wait."
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

    /// One action in other apps, which the service has the helper app do. Asked about as the
    /// `desktop` kind, once for each exact action (what is typed included).
    fn desktop(&self, args: &Map<String, Value>) -> Result<String, String> {
        let arg = |key: &str| {
            args.get(key).map_or(String::new(), |v| match v {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            })
        };
        let (action, app) = (arg("action"), arg("app"));
        let what = match action.as_str() {
            "apps" => "list the apps and windows".to_owned(),
            "look" => format!("look at {app}"),
            "screenshot" => format!("take a picture of {app}"),
            "press" => format!("press element {} in {app}", arg("element")),
            "set_value" => format!(
                "set element {} in {app} to: {}",
                arg("element"),
                arg("value")
            ),
            "type" => format!("type into {app}: {}", arg("text")),
            other => {
                return Err(format!(
                    "`{other}` is not an action: apps, look, screenshot, press, set_value or type."
                ));
            }
        };
        if action != "apps" && app.trim().is_empty() {
            return Err("`app` must be the name of an app, as `apps` lists it.".into());
        }
        // The whole action is the target, never cut: a grant is for exactly this, and the user
        // reads all of it.
        self.allowed(Kind::Desktop, what.clone(), format!("Desktop: {what}"))?;
        let id = self.wait_on("d");
        self.out.send(&Event::Desktop {
            request: id.clone(),
            action: args.clone(),
        });
        match self.answer(&id) {
            Some(result) => result,
            None => Err("Aborted by the user".into()),
        }
    }

    /// One action in an Apple app, which the service runs. The card carries every field the
    /// model gave, whole: for a send that is the recipient and the text.
    fn apple(&self, app: &proto::apple::App, args: &Map<String, Value>) -> Result<String, String> {
        let call = app.parse(args)?;
        let title = format!("{}: {}", app.tool, call.action);
        // The target names the action too, so an "Always" for one read is not one for another.
        let target = format!("{} {}\n{}", app.tool, call.action, call.shown);
        self.allowed(call.kind, target.trim_end().to_owned(), title)?;
        let id = self.wait_on("a");
        self.out.send(&Event::Apple {
            request: id.clone(),
            tool: app.tool.to_owned(),
            args: args.clone(),
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

    /// Asks the service for the model server and key of the coming model call. Its ids count
    /// apart from questions', so a turn's question ids stay what they were without it.
    fn get_token(&self) -> Result<Token, String> {
        static TOK: AtomicU64 = AtomicU64::new(0);
        let id = format!("tok{}", TOK.fetch_add(1, Ordering::Relaxed) + 1);
        *lock(&self.control.question) = Some(id.clone());
        self.out.send(&Event::Token {
            request: id.clone(),
        });
        match self.answer(&id) {
            Some(Ok(val)) => {
                let mut parts = val.split('\n');
                let mut part = || parts.next().unwrap_or("");
                let (url, key, api, named, context) = (part(), part(), part(), part(), part());
                let model = (self.model.clone())
                    .or_else(|| (!named.is_empty()).then(|| named.to_owned()))
                    .ok_or("no model chosen")?;
                let key = (!key.is_empty()).then(|| key.to_owned());
                let api = match api {
                    "responses" => Api::Responses,
                    _ => Api::Chat,
                };
                Ok((url.to_owned(), key, api, model, context.parse().ok()))
            }
            Some(Err(e)) => Err(e),
            None => Err("Aborted by the user".into()),
        }
    }

    /// Marks a new question or message as what the turn waits on, and gives its id. Its own
    /// id, never the model's call id: a model may repeat those, and an answer must not fit a
    /// later question.
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

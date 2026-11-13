//! One turn: model call, tool calls, model call, … until the model stops calling tools.

use crate::mcp::Servers;
use crate::model::{Ask, Delta, Provider, Reply};
use crate::session::Session;
use crate::tools::{self, Control, lock};
use crate::{Out, Work};
use proto::connectors::Connector;
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
    /// `(question, choice)` from `answer`, already checked against the open question.
    pub answers: Receiver<(String, String)>,
    pub steer: Receiver<String>,
}

pub const OPTIONS: [&str; 2] = [APPROVE, DENY];
/// Most recent messages a compaction leaves as they are.
const KEEP: usize = 4;
const SUMMARISE: &str = "Summarise the conversation so far for your own later use: what the user wants, \
    what was decided, what was found, which files changed, and what is left to do. \
    Answer with the summary only and do not call tools.";

/// The engine's own tools, then the connectors'.
fn tool_list(mcp: &Servers) -> Value {
    let mut tools = tools::definitions();
    if let Value::Array(list) = &mut tools {
        list.extend(mcp.definitions().cloned());
    }
    tools
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
        let tools = tool_list(mcp);
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
            let reply = self
                .provider
                .stream(&ask, self.control.aborted_flag(), |d| {
                    self.out.send(&match d {
                        Delta::Text(t) => Event::Text { delta: t.into() },
                        Delta::Thinking(t) => Event::Thinking { delta: t.into() },
                    })
                })?;
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
                let mut message = json!({"role": "assistant", "content": text});
                if !calls.is_empty() {
                    message["tool_calls"] = calls
                        .iter()
                        .map(|c| json!({"id": c.id, "type": "function", "function": {"name": c.name, "arguments": c.arguments}}))
                        .collect();
                }
                session.push(message).map_err(saved)?;
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
                let args = match serde_json::from_str::<Value>(&call.arguments) {
                    Ok(Value::Object(args)) => Ok(args),
                    _ => Err(format!(
                        "the arguments are not a JSON object: {}",
                        call.arguments
                    )),
                };
                self.out.send(&Event::ToolStart {
                    call: call.id.clone(),
                    tool: call.name.clone(),
                    args: args.clone().unwrap_or_default(),
                });
                let result = if self.control.aborted() {
                    Err("Not run: the turn was aborted.".into())
                } else {
                    args.and_then(|args| self.call(mcp, &call.name, &args))
                };
                let (output, is_error) = match result {
                    Ok(out) => (out, false),
                    Err(out) => (out, true),
                };
                self.out.send(&Event::ToolEnd {
                    call: call.id.clone(),
                    output: output.clone(),
                    is_error,
                });
                session
                    .push(json!({"role": "tool", "tool_call_id": call.id, "content": output}))
                    .map_err(saved)?;
            }
        }
    }

    fn system(&self) -> String {
        format!(
            "You are a helpful assistant. Your working folder is {}. Use the tools to read, change and search files and to run commands there.{}{}",
            self.dir.display(),
            crate::skills::prompt(&self.dir),
            crate::memory::prompt(&self.dir)
        )
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
        self.compact(&mut work.session, &self.system(), &tool_list(&work.mcp))
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
        let reply = self
            .provider
            .stream(&ask, self.control.aborted_flag(), |_| {})?;
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
    ) -> Result<String, String> {
        // Every action is asked about: the service decides from the bot's permissions, and
        // asks the user where they say so. What is asked about is what then runs.
        let connector = mcp.title(tool);
        let action = match &connector {
            Some(title) => Some((Kind::Connector, tool.to_owned(), title.clone())),
            None => tools::action(tool, args),
        };
        if let Some((kind, target, title)) = action {
            match self.ask(kind, target, title) {
                Some(true) => {}
                Some(false) => return Err("The user denied this action.".into()),
                None => return Err("Aborted by the user".into()),
            }
        }
        match connector {
            Some(_) => mcp.call(tool, args, &self.control),
            None => tools::run(tool, args, &self.dir, &self.control),
        }
    }

    /// `Some(approved)`, or `None` when the turn was aborted while waiting.
    fn ask(&self, kind: Kind, target: String, title: String) -> Option<bool> {
        // Its own id, never the model's call id: a model may repeat those, and an answer must
        // not fit a later question.
        static ASKED: AtomicU64 = AtomicU64::new(0);
        let id = &format!("q{}", ASKED.fetch_add(1, Ordering::Relaxed) + 1);
        *lock(&self.control.question) = Some(id.clone());
        self.out.send(&Event::Question {
            question: id.clone(),
            title,
            options: OPTIONS.map(String::from).to_vec(),
            kind: Some(kind),
            target: Some(target),
        });
        let answer = loop {
            match self.answers.recv_timeout(Duration::from_millis(100)) {
                Ok((question, choice)) if question == *id => break Some(choice == APPROVE),
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) if !self.control.aborted() => {}
                Err(_) => break None,
            }
        };
        *lock(&self.control.question) = None;
        answer
    }
}

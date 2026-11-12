//! One turn: model call, tool calls, model call, … until the model stops calling tools.

use crate::Out;
use crate::model::{Delta, Provider, Reply};
use crate::session::Session;
use crate::tools::{self, Control, lock};
use proto::{Event, TurnEnd};
use serde_json::{Map, Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

pub struct Turn {
    pub out: Out,
    pub dir: PathBuf,
    pub provider: Provider,
    pub model: String,
    pub effort: Option<String>,
    pub control: Arc<Control>,
    /// `(question, choice)` from `answer`, already checked against the open question.
    pub answers: Receiver<(String, String)>,
    pub steer: Receiver<String>,
}

const APPROVE: &str = "Approve";
const DENY: &str = "Deny";
pub const OPTIONS: [&str; 2] = [APPROVE, DENY];

impl Turn {
    pub fn run(&self, session: &mut Session, nonce: String, text: String) {
        self.out.send(&Event::TurnStart { nonce });
        let (reason, error) = match self.turn(session, text) {
            Ok(reason) => (reason, None),
            Err(e) => (TurnEnd::Error, Some(e)),
        };
        self.out.send(&Event::TurnEnd { reason, error });
    }

    fn turn(&self, session: &mut Session, text: String) -> Result<TurnEnd, String> {
        let saved = |e: std::io::Error| format!("cannot save the conversation: {e}");
        let system = format!(
            "You are a helpful assistant. Your working folder is {}. Use the tools to read, change and search files and to run commands there.",
            self.dir.display()
        );
        let tools = tools::definitions();
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
            let reply = self.provider.stream(
                &self.model,
                self.effort.as_deref(),
                &system,
                &session.messages,
                &tools,
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
            let base = session.messages.len();
            for (i, call) in calls.iter_mut().enumerate() {
                // Some local servers send no id; each call in the conversation still needs its own.
                if call.id.is_empty() {
                    call.id = format!("call_{base}_{i}");
                }
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
                return Ok(TurnEnd::Done);
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
                    args.and_then(|args| self.call(&call.id, &call.name, &args))
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

    fn call(&self, id: &str, tool: &str, args: &Map<String, Value>) -> Result<String, String> {
        if tools::needs_approval(tool) {
            match self.ask(id, tools::title(tool, args)) {
                Some(true) => {}
                Some(false) => return Err("The user denied this action.".into()),
                None => return Err("Aborted by the user".into()),
            }
        }
        tools::run(tool, args, &self.dir, &self.control)
    }

    /// `Some(approved)`, or `None` when the turn was aborted while waiting.
    fn ask(&self, id: &str, title: String) -> Option<bool> {
        *lock(&self.control.question) = Some(id.to_owned());
        self.out.send(&Event::Question {
            question: id.to_owned(),
            title,
            options: OPTIONS.map(String::from).to_vec(),
        });
        let answer = loop {
            match self.answers.recv_timeout(Duration::from_millis(100)) {
                Ok((question, choice)) if question == id => break Some(choice == APPROVE),
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) if !self.control.aborted() => {}
                Err(_) => break None,
            }
        };
        *lock(&self.control.question) = None;
        answer
    }
}

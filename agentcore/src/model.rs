//! One model provider: the OpenAI-compatible streaming chat API (LM Studio, Ollama,
//! llama.cpp server, or an API key). The server and the key come from the service, for each
//! call. #1 adds ChatGPT sign-in.

use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

/// The variable with the model server's key. It is the engine's alone: what the model starts
/// (commands, connectors) does not get it.
pub const KEY: &str = "BATE_MODEL_API_KEY";

/// The stream's last line.
const DONE: &str = "[DONE]";

/// What a `data:` line of the stream carries.
fn data(line: &str) -> Option<&str> {
    line.trim_end().strip_prefix("data:").map(str::trim_start)
}

/// How often a wait for the server looks for `abort`.
const POLL: Duration = Duration::from_millis(100);

pub struct Provider {
    agent: ureq::Agent,
}

pub enum Delta<'a> {
    Text(&'a str),
    Thinking(&'a str),
}

#[derive(Default)]
pub struct Reply {
    pub text: String,
    pub calls: Vec<Call>,
    /// The turn was aborted mid-stream; `text` holds what arrived, `calls` is empty.
    pub aborted: bool,
}

#[derive(Default)]
pub struct Call {
    /// Empty when the server sent none.
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Serialize)]
struct Body<'a> {
    model: &'a str,
    messages: Messages<'a>,
    tools: &'a Value,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<&'a str>,
}

/// The system message, the conversation, then `last`, without copying any of it.
struct Messages<'a>(&'a Value, &'a [Value], Option<&'a Value>);

impl Serialize for Messages<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(std::iter::once(self.0).chain(self.1).chain(self.2))
    }
}

/// One model call.
pub struct Ask<'a> {
    pub model: &'a str,
    pub effort: Option<&'a str>,
    pub system: &'a str,
    pub messages: &'a [Value],
    /// One more message after `messages`.
    pub last: Option<&'a Value>,
    pub tools: &'a Value,
}

impl Provider {
    pub fn new() -> Provider {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build();
        Provider {
            agent: ureq::Agent::new_with_config(config),
        }
    }

    /// `base_url` and `key` are the service's answer to this call's `token` request, and the
    /// only source: a key in the engine's own environment may belong to another server.
    // shortcut: after an abort the reading thread and its connection stay until the server next
    // sends or closes (ureq has no idle timeout and cannot be closed from here); switch HTTP
    // client if servers keep generating into it.
    pub fn stream(
        &self,
        base_url: &str,
        key: Option<&str>,
        ask: &Ask,
        abort: &AtomicBool,
        mut on: impl FnMut(Delta),
    ) -> Result<Reply, String> {
        let system = json!({"role": "system", "content": ask.system});
        let body = Body {
            model: ask.model,
            messages: Messages(&system, ask.messages, ask.last),
            tools: ask.tools,
            stream: true,
            reasoning_effort: ask.effort,
        };
        let body = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let key = key.map(String::from);
        let agent = self.agent.clone();
        // The request is made and read on its own thread: a server that sends nothing must not
        // hold `abort` up. Bounded, so a fast server waits for us.
        let (tx, lines) = mpsc::sync_channel::<Result<String, String>>(64);
        std::thread::spawn(move || {
            let read = || -> Result<(), String> {
                let mut request = agent.post(&url).header("Content-Type", "application/json");
                if let Some(key) = &key {
                    request = request.header("Authorization", format!("Bearer {key}"));
                }
                let response = request
                    .send(&body[..])
                    .map_err(|e| format!("cannot reach the model server at {url}: {e}"))?;
                let status = response.status();
                let mut reader = BufReader::new(response.into_body().into_reader());
                if !status.is_success() {
                    let mut text = String::new();
                    let _ = reader.take(4096).read_to_string(&mut text);
                    return Err(format!(
                        "the model server answered {status}: {}",
                        text.trim()
                    ));
                }
                loop {
                    let mut line = String::new();
                    let n = reader
                        .read_line(&mut line)
                        .map_err(|e| format!("the model stream broke: {e}"))?;
                    let done = n == 0 || data(&line) == Some(DONE);
                    // The end of the reply (a server may keep the body open after it), or nobody
                    // listens any more. Returning closes the connection.
                    if tx.send(Ok(line)).is_err() || done {
                        return Ok(());
                    }
                }
            };
            if let Err(e) = read() {
                let _ = tx.send(Err(e));
            }
        });
        let mut reply = Reply::default();
        // Where the call of each server-side `index` is in `reply.calls`.
        let mut at = HashMap::<u64, usize>::new();
        let mut finished = false;
        loop {
            let received = lines.recv_timeout(POLL);
            if abort.load(Ordering::SeqCst) {
                reply.calls.clear();
                reply.aborted = true;
                return Ok(reply);
            }
            let line = match received {
                Ok(line) => line?,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) if finished => break,
                // A body that just ends (a crashed server, a dropped proxy) is not a whole reply.
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("the model's reply was cut off".into());
                }
            };
            let Some(data) = data(&line) else {
                continue;
            };
            if data == DONE {
                break;
            }
            let chunk: Value =
                serde_json::from_str(data).map_err(|e| format!("the model sent bad data: {e}"))?;
            // Some servers end the body after the last chunk, with no `[DONE]` line.
            finished |= !chunk["choices"][0]["finish_reason"].is_null();
            if let Some(error) = chunk.get("error") {
                return Err(format!(
                    "the model server failed: {}",
                    error["message"].as_str().unwrap_or(&error.to_string())
                ));
            }
            let delta = &chunk["choices"][0]["delta"];
            if let Some(t) = delta["reasoning_content"]
                .as_str()
                .or(delta["reasoning"].as_str())
                && !t.is_empty()
            {
                on(Delta::Thinking(t));
            }
            if let Some(t) = delta["content"].as_str()
                && !t.is_empty()
            {
                reply.text.push_str(t);
                on(Delta::Text(t));
            }
            for c in delta["tool_calls"].as_array().into_iter().flatten() {
                let index = c["index"].as_u64().unwrap_or(0);
                let id = c["id"].as_str().filter(|id| !id.is_empty());
                // The server's `index` only says which pieces belong together; calls are kept in
                // the order they began, so a wild index cannot allocate or panic.
                // An id seen before names its call, whatever the index says.
                let by_id = id.and_then(|id| reply.calls.iter().position(|call| call.id == id));
                let known = by_id.or(at.get(&index).copied());
                // Some servers number every call 0, or not at all: another id under a known
                // index is the next call.
                let other = known.is_some_and(|i| {
                    let had = &reply.calls[i].id;
                    !had.is_empty() && id.is_some_and(|id| id != had)
                });
                let i = match known {
                    Some(i) if !other => {
                        at.insert(index, i);
                        i
                    }
                    _ => {
                        reply.calls.push(Call::default());
                        at.insert(index, reply.calls.len() - 1);
                        reply.calls.len() - 1
                    }
                };
                let call = &mut reply.calls[i];
                if let Some(id) = id {
                    call.id = id.to_owned();
                }
                // A name comes whole; some servers send it again with every piece of the arguments.
                if call.name.is_empty() {
                    call.name = c["function"]["name"].as_str().unwrap_or("").to_owned();
                }
                call.arguments
                    .push_str(c["function"]["arguments"].as_str().unwrap_or(""));
            }
        }
        Ok(reply)
    }
}

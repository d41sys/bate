//! The model providers: the OpenAI-compatible streaming chat API (LM Studio, Ollama,
//! llama.cpp server, or an API key), and OpenAI's Responses API, which is what a ChatGPT
//! sign-in may call. The server, the key and which of the two come from the service, for
//! each call.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

/// The variable with the model server's key. The service keeps it from its engines; should an
/// engine run by hand have it, what the model starts (commands, connectors) does not get it.
pub const KEY: &str = "BATE_MODEL_API_KEY";

/// The stream's last line.
const DONE: &str = "[DONE]";

/// What a `data:` line of the stream carries.
fn data(line: &str) -> Option<&str> {
    line.trim_end().strip_prefix("data:").map(str::trim_start)
}

/// How often a wait for the server looks for `abort`.
const POLL: Duration = Duration::from_millis(100);

/// The namespace the tools are sent in: a ChatGPT sign-in takes function tools only so.
const NAMESPACE: &str = "bate";

/// Which API the model server speaks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Api {
    Chat,
    Responses,
}

/// What a wait for the next line of the stream gave.
enum Next {
    Line(String),
    Aborted,
    /// The body ended.
    End,
}

fn next(
    lines: &mpsc::Receiver<Result<String, String>>,
    abort: &AtomicBool,
) -> Result<Next, String> {
    loop {
        let received = lines.recv_timeout(POLL);
        if abort.load(Ordering::SeqCst) {
            return Ok(Next::Aborted);
        }
        match received {
            Ok(line) => return line.map(Next::Line),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Ok(Next::End),
        }
    }
}

/// Whether a line of a chat stream is its last.
fn chat_done(line: &str) -> bool {
    data(line) == Some(DONE)
}

/// Whether a line of a Responses stream carries the event that ends it.
fn responses_done(line: &str) -> bool {
    #[derive(Deserialize)]
    struct Kind<'a> {
        #[serde(borrow, rename = "type")]
        kind: std::borrow::Cow<'a, str>,
    }
    let kind = data(line).and_then(|data| serde_json::from_str::<Kind>(data).ok());
    kind.is_some_and(|k| {
        matches!(
            &*k.kind,
            "response.completed" | "response.failed" | "response.incomplete" | "error"
        )
    })
}

/// The chat-shaped conversation as Responses input items.
// shortcut: the model's reasoning items are not kept between calls of a turn (`store` is
// false, so the server has none either); send them back when tool-heavy turns reason anew.
fn input<'a>(messages: impl Iterator<Item = &'a Value>) -> Vec<Value> {
    let mut items = Vec::new();
    for m in messages {
        let text = |v: &Value| v.as_str().map_or_else(|| v.to_string(), str::to_owned);
        match m["role"].as_str().unwrap_or("") {
            "tool" => items.push(json!({
                "type": "function_call_output",
                "call_id": m["tool_call_id"],
                "output": text(&m["content"]),
            })),
            "assistant" => {
                if m["content"].as_str().is_some_and(|t| !t.is_empty()) {
                    items.push(json!({"role": "assistant", "content": m["content"]}));
                }
                for c in m["tool_calls"].as_array().into_iter().flatten() {
                    items.push(json!({
                        "type": "function_call",
                        "call_id": c["id"],
                        "namespace": NAMESPACE,
                        "name": c["function"]["name"],
                        "arguments": c["function"]["arguments"],
                    }));
                }
            }
            // A system message as an item is refused; a developer one says the same.
            "system" => items.push(json!({"role": "developer", "content": m["content"]})),
            role => items.push(json!({"role": role, "content": m["content"]})),
        }
    }
    items
}

/// The body of a Responses request for `ask`.
fn responses_body(ask: &Ask) -> Value {
    let mut body = json!({
        "model": ask.model,
        "instructions": ask.system,
        "input": input(ask.messages.iter().chain(ask.last)),
        "store": false,
        "stream": true,
    });
    let tools: Vec<Value> = ask
        .tools
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| {
            let f = &t["function"];
            json!({"type": "function", "name": f["name"], "description": f["description"], "parameters": f["parameters"]})
        })
        .collect();
    if !tools.is_empty() {
        body["tools"] = json!([{
            "type": "namespace",
            "name": NAMESPACE,
            "description": "The tools of this app.",
            "tools": tools,
        }]);
    }
    if let Some(effort) = ask.effort {
        body["reasoning"] = json!({"effort": effort});
    }
    body
}

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
        api: Api,
        ask: &Ask,
        abort: &AtomicBool,
        on: impl FnMut(Delta),
    ) -> Result<Reply, String> {
        let base = base_url.trim_end_matches('/');
        match api {
            Api::Chat => {
                let system = json!({"role": "system", "content": ask.system});
                let body = Body {
                    model: ask.model,
                    messages: Messages(&system, ask.messages, ask.last),
                    tools: ask.tools,
                    stream: true,
                    reasoning_effort: ask.effort,
                };
                let body = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
                let url = format!("{base}/chat/completions");
                chat(&self.lines(url, key, body, chat_done), abort, on)
            }
            Api::Responses => {
                let body = serde_json::to_vec(&responses_body(ask)).map_err(|e| e.to_string())?;
                let url = format!("{base}/responses");
                responses(&self.lines(url, key, body, responses_done), abort, on).map_err(|e| {
                    // Comes as a 429 or as `response.failed` mid-stream; it may be the plan's limit
                    // or one ChatGPT sets for Bate, and says nothing of when it ends.
                    if e.contains("subscription_sharing_usage_limit_exceeded") {
                        format!(
                            "the ChatGPT plan's usage limit is reached (for the plan, or for \
                             Bate): see https://chatgpt.com/settings/usage ({e})"
                        )
                    } else if e
                        .contains("subscription_sharing_workplace_individual_usage_limit_exceeded")
                    {
                        // A ChatGPT Business plan: only the workspace's admin can raise it.
                        format!(
                            "your ChatGPT workspace's usage limit is reached: ask its admin to \
                             raise it at admin.openai.com, Workspace, Usage limits ({e})"
                        )
                    } else {
                        e
                    }
                })
            }
        }
    }

    /// Posts `body` to `url` on a thread of its own and gives the lines of the answer, up to
    /// and with the one `done` knows as the last.
    fn lines(
        &self,
        url: String,
        key: Option<&str>,
        body: Vec<u8>,
        done: fn(&str) -> bool,
    ) -> mpsc::Receiver<Result<String, String>> {
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
                    let done = n == 0 || done(&line);
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
        lines
    }
}

/// Reads a Responses stream to its `response.completed`.
fn responses(
    lines: &mpsc::Receiver<Result<String, String>>,
    abort: &AtomicBool,
    mut on: impl FnMut(Delta),
) -> Result<Reply, String> {
    let mut reply = Reply::default();
    loop {
        let line = match next(lines, abort)? {
            Next::Line(line) => line,
            Next::Aborted => {
                reply.calls.clear();
                reply.aborted = true;
                return Ok(reply);
            }
            Next::End => return Err("the model's reply was cut off".into()),
        };
        let Some(event) = data(&line).and_then(|d| serde_json::from_str::<Value>(d).ok()) else {
            continue;
        };
        let item = &event["item"];
        match event["type"].as_str().unwrap_or("") {
            "response.output_text.delta" => {
                if let Some(t) = event["delta"].as_str().filter(|t| !t.is_empty()) {
                    reply.text.push_str(t);
                    on(Delta::Text(t));
                }
            }
            "response.output_item.done" if item["type"] == "function_call" => {
                let field = |name: &str| item[name].as_str().unwrap_or("").to_owned();
                reply.calls.push(Call {
                    id: field("call_id"),
                    name: field("name"),
                    arguments: field("arguments"),
                });
            }
            "response.completed" => return Ok(reply),
            kind @ ("response.failed" | "response.incomplete" | "error") => {
                // Where the reason is depends on the event.
                let error = [&event["response"]["error"], &event["error"], &event]
                    .into_iter()
                    .find(|e| e["message"].is_string() || e["code"].is_string());
                let reason = &event["response"]["incomplete_details"]["reason"];
                let why = match error {
                    Some(e) => format!(
                        "{}: {}",
                        e["code"].as_str().unwrap_or("error"),
                        e["message"].as_str().unwrap_or("")
                    ),
                    None => reason.as_str().unwrap_or(kind).to_owned(),
                };
                return Err(format!("the model server failed: {why}"));
            }
            _ => {}
        }
    }
}

/// Reads a chat stream to its end.
fn chat(
    lines: &mpsc::Receiver<Result<String, String>>,
    abort: &AtomicBool,
    mut on: impl FnMut(Delta),
) -> Result<Reply, String> {
    let mut reply = Reply::default();
    // Where the call of each server-side `index` is in `reply.calls`.
    let mut at = HashMap::<u64, usize>::new();
    let mut finished = false;
    loop {
        let line = match next(lines, abort)? {
            Next::Line(line) => line,
            Next::Aborted => {
                reply.calls.clear();
                reply.aborted = true;
                return Ok(reply);
            }
            Next::End if finished => break,
            // A body that just ends (a crashed server, a dropped proxy) is not a whole reply.
            Next::End => return Err("the model's reply was cut off".into()),
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

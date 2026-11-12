//! One model provider: the OpenAI-compatible streaming chat API (LM Studio, Ollama,
//! llama.cpp server, or an API key). #8 adds ChatGPT sign-in and tokens from the service.

use serde::Serialize;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};

pub struct Provider {
    agent: ureq::Agent,
    url: String,
    key: Option<String>,
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
    /// shortcut: the server and key come from `BATE_MODEL_BASE_URL` / `BATE_MODEL_API_KEY`; #8 moves them into the service.
    pub fn from_env() -> Result<Provider, String> {
        let url = std::env::var("BATE_MODEL_BASE_URL")
            .ok()
            .filter(|u| !u.is_empty())
            .ok_or("no model server is set up: set BATE_MODEL_BASE_URL (for example http://127.0.0.1:1234/v1)")?;
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build();
        Ok(Provider {
            agent: ureq::Agent::new_with_config(config),
            url: format!("{}/chat/completions", url.trim_end_matches('/')),
            key: std::env::var("BATE_MODEL_API_KEY")
                .ok()
                .filter(|k| !k.is_empty()),
        })
    }

    /// Streams one reply. `abort` is checked between streamed lines.
    // shortcut: a server that stalls without sending blocks abort until it sends or closes; add a read timeout if that shows up.
    pub fn stream(
        &self,
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
        let mut request = self
            .agent
            .post(&self.url)
            .header("Content-Type", "application/json");
        if let Some(key) = &self.key {
            request = request.header("Authorization", format!("Bearer {key}"));
        }
        let response = request
            .send(&body[..])
            .map_err(|e| format!("cannot reach the model server at {}: {e}", self.url))?;
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
        let mut reply = Reply::default();
        let mut line = String::new();
        loop {
            line.clear();
            let n = reader
                .read_line(&mut line)
                .map_err(|e| format!("the model stream broke: {e}"))?;
            if abort.load(Ordering::SeqCst) {
                reply.calls.clear();
                reply.aborted = true;
                return Ok(reply);
            }
            if n == 0 {
                break;
            }
            let Some(data) = line.trim_end().strip_prefix("data:").map(str::trim_start) else {
                continue;
            };
            if data == "[DONE]" {
                break;
            }
            let chunk: Value =
                serde_json::from_str(data).map_err(|e| format!("the model sent bad data: {e}"))?;
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
                // At most one past the last call: a wild index must not allocate or panic.
                let i = (c["index"].as_u64().unwrap_or(0) as usize).min(reply.calls.len());
                if reply.calls.len() <= i {
                    reply.calls.resize_with(i + 1, Call::default);
                }
                let call = &mut reply.calls[i];
                if let Some(id) = c["id"].as_str() {
                    call.id = id.to_owned();
                }
                call.name
                    .push_str(c["function"]["name"].as_str().unwrap_or(""));
                call.arguments
                    .push_str(c["function"]["arguments"].as_str().unwrap_or(""));
            }
        }
        Ok(reply)
    }
}

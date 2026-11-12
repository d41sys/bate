//! MCP connectors: the bot's servers, their tools, and calls to them.
//! JSON-RPC 2.0, over a child's stdin/stdout (one message per line) or over HTTP.

use crate::tools::{Control, OUTPUT_CAP, kill_group};
use proto::connectors::{Connector, Transport};
use serde_json::{Map, Value, json};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

const VERSION: &str = "2025-06-18";
/// Longest wait for `initialize` and `tools/list`.
const SETUP: Duration = Duration::from_secs(30);
/// Longest wait for one tool call.
const CALL: Duration = Duration::from_secs(120);
/// Largest HTTP reply read.
const MAX_REPLY: u64 = 64 << 20;

/// The connectors tried so far. `None`: it failed to load, and is not tried again until it changes.
#[derive(Default)]
pub struct Servers(Vec<(Connector, Option<Server>)>);

pub struct Server {
    link: Link,
    next_id: u64,
    tools: Vec<Tool>,
}

struct Tool {
    /// The server's name for it.
    name: String,
    /// The model's name for it: `<connector>__<name>`.
    exposed: String,
    /// In the OpenAI function-calling shape.
    definition: Value,
}

enum Link {
    Stdio {
        child: Child,
        stdin: ChildStdin,
        messages: Receiver<Value>,
    },
    Http {
        agent: ureq::Agent,
        url: String,
        headers: Vec<(String, String)>,
        /// `Mcp-Session-Id` from the server, sent back on every later request.
        session: Option<String>,
    },
}

impl Drop for Link {
    fn drop(&mut self) {
        if let Link::Stdio { child, .. } = self {
            // Not reaped yet, so the group id is still the child's.
            kill_group(child.id() as i32);
            let _ = child.wait();
        }
    }
}

impl Servers {
    /// Makes the running servers match `connectors`: stops those removed, changed or disabled, and
    /// starts the new enabled ones. A connector that fails is reported on stderr (the service log).
    pub fn sync(&mut self, connectors: &[Connector], dir: &Path, control: &Control) {
        self.0.retain(|(known, _)| connectors.contains(known));
        for connector in connectors.iter().filter(|c| c.enabled) {
            if self.0.iter().any(|(known, _)| known == connector) {
                continue;
            }
            let server = Server::connect(connector, dir, control)
                .map_err(|e| eprintln!("connector {}: {e}", connector.name))
                .ok();
            self.0.push((connector.clone(), server));
        }
    }

    fn tools(&self) -> impl Iterator<Item = (&Connector, &Tool)> {
        self.0
            .iter()
            .filter_map(|(c, server)| Some((c, server.as_ref()?)))
            .flat_map(|(c, server)| server.tools.iter().map(move |t| (c, t)))
    }

    pub fn definitions(&self) -> impl Iterator<Item = &Value> {
        self.tools().map(|(_, t)| &t.definition)
    }

    /// The approval question for `tool`, if it is one of ours.
    pub fn title(&self, tool: &str) -> Option<String> {
        let (connector, tool) = self.tools().find(|(_, t)| t.exposed == tool)?;
        Some(format!("Use {}: {}", connector.name, tool.name))
    }

    pub fn call(
        &mut self,
        tool: &str,
        args: &Map<String, Value>,
        control: &Control,
    ) -> Result<String, String> {
        for server in self.0.iter_mut().filter_map(|(_, server)| server.as_mut()) {
            if let Some(name) = server
                .tools
                .iter()
                .find(|t| t.exposed == tool)
                .map(|t| t.name.clone())
            {
                return server.call(&name, args, control);
            }
        }
        Err(format!("there is no tool `{tool}`"))
    }
}

impl Server {
    fn connect(connector: &Connector, dir: &Path, control: &Control) -> Result<Server, String> {
        let link = match &connector.transport {
            Transport::Stdio { command, args, env } => {
                let mut child = Command::new(command)
                    .args(args)
                    .envs(env)
                    .current_dir(dir)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .process_group(0)
                    .spawn()
                    .map_err(|e| format!("cannot start `{command}`: {e}"))?;
                let stdin = child.stdin.take().expect("stdin is piped");
                let mut stdout = BufReader::new(child.stdout.take().expect("stdout is piped"));
                let (tx, messages) = mpsc::channel();
                std::thread::spawn(move || {
                    let mut buf = Vec::new();
                    loop {
                        match proto::read_frame::<Value>(
                            &mut stdout,
                            proto::MAX_EVENT_FRAME,
                            &mut buf,
                        ) {
                            Ok(Some(message)) => {
                                if tx.send(message).is_err() {
                                    return;
                                }
                            }
                            // Not JSON: some servers print a banner on stdout.
                            Err(e) if e.kind() == io::ErrorKind::InvalidData => {}
                            _ => return,
                        }
                    }
                });
                Link::Stdio {
                    child,
                    stdin,
                    messages,
                }
            }
            Transport::Http { url, headers } => {
                // shortcut: `abort` cannot interrupt an HTTP call; it ends at this timeout at the latest.
                let config = ureq::Agent::config_builder()
                    .http_status_as_error(false)
                    .timeout_global(Some(CALL))
                    .build();
                Link::Http {
                    agent: ureq::Agent::new_with_config(config),
                    url: url.clone(),
                    headers: headers
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                    session: None,
                }
            }
        };
        let mut server = Server {
            link,
            next_id: 0,
            tools: Vec::new(),
        };
        let hello = json!({"protocolVersion": VERSION, "capabilities": {},
            "clientInfo": {"name": "bate", "version": env!("CARGO_PKG_VERSION")}});
        server.request("initialize", hello, SETUP, control)?;
        server.send(
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            None,
        )?;
        let mut cursor = None;
        // At most 50 pages: a server that always names a next page must not hold the turn.
        for _ in 0..50 {
            let params = match &cursor {
                Some(cursor) => json!({"cursor": cursor}),
                None => json!({}),
            };
            let page = server.request("tools/list", params, SETUP, control)?;
            for tool in page["tools"].as_array().into_iter().flatten() {
                let Some(name) = tool["name"].as_str() else {
                    continue;
                };
                let exposed = format!("{}__{name}", connector.name);
                // What model APIs accept as a function name.
                if exposed.len() > 64
                    || !exposed
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                {
                    eprintln!(
                        "connector {}: tool `{name}` left out: `{exposed}` is not a usable tool name",
                        connector.name
                    );
                    continue;
                }
                let parameters = match &tool["inputSchema"] {
                    schema @ Value::Object(_) => schema.clone(),
                    _ => json!({"type": "object"}),
                };
                let description = tool["description"].as_str().unwrap_or("");
                server.tools.push(Tool {
                    name: name.to_owned(),
                    definition: json!({"type": "function", "function":
                        {"name": exposed, "description": description, "parameters": parameters}}),
                    exposed,
                });
            }
            cursor = page["nextCursor"].as_str().map(str::to_owned);
            if cursor.is_none() {
                break;
            }
        }
        Ok(server)
    }

    /// Sends `message`. With `want`, the reply to that request id comes back over HTTP; over stdio
    /// replies arrive on `messages`.
    fn send(&mut self, message: &Value, want: Option<u64>) -> Result<Option<Value>, String> {
        match &mut self.link {
            Link::Stdio { stdin, .. } => {
                let mut line = serde_json::to_vec(message).map_err(|e| e.to_string())?;
                line.push(b'\n');
                stdin
                    .write_all(&line)
                    .map_err(|e| format!("the server stopped: {e}"))?;
                Ok(None)
            }
            Link::Http {
                agent,
                url,
                headers,
                session,
            } => {
                let mut request = agent
                    .post(url.as_str())
                    .header("Content-Type", "application/json")
                    .header("Accept", "application/json, text/event-stream")
                    .header("MCP-Protocol-Version", VERSION);
                for (name, value) in headers.iter() {
                    request = request.header(name.as_str(), value.as_str());
                }
                if let Some(session) = session {
                    request = request.header("Mcp-Session-Id", session.as_str());
                }
                let body = serde_json::to_vec(message).map_err(|e| e.to_string())?;
                let response = request
                    .send(&body[..])
                    .map_err(|e| format!("cannot reach {url}: {e}"))?;
                let status = response.status();
                let header =
                    |name: &str| response.headers().get(name).and_then(|v| v.to_str().ok());
                if let Some(id) = header("mcp-session-id") {
                    *session = Some(id.to_owned());
                }
                let stream =
                    header("content-type").is_some_and(|t| t.starts_with("text/event-stream"));
                let mut reader = BufReader::new(response.into_body().into_reader().take(MAX_REPLY));
                if !status.is_success() {
                    let mut text = String::new();
                    let _ = reader.take(4096).read_to_string(&mut text);
                    return Err(format!("{url} answered {status}: {}", text.trim()));
                }
                let Some(id) = want else { return Ok(None) };
                if !stream {
                    return serde_json::from_reader(reader)
                        .map(Some)
                        .map_err(|e| format!("{url} sent a bad reply: {e}"));
                }
                // The reply is one of the stream's events; others are notifications.
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) => return Err(format!("{url} closed the stream without a reply")),
                        Ok(_) => {}
                        Err(e) => return Err(format!("the stream from {url} broke: {e}")),
                    }
                    let Some(data) = line.trim_end().strip_prefix("data:") else {
                        continue;
                    };
                    if let Ok(message) = serde_json::from_str::<Value>(data)
                        && is_reply(&message, id)
                    {
                        return Ok(Some(message));
                    }
                }
            }
        }
    }

    /// One request and its result. `Err` is the server's error text, or what went wrong reaching it.
    fn request(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
        control: &Control,
    ) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let mut reply = match self.send(&message, Some(id))? {
            Some(reply) => reply,
            None => {
                let Link::Stdio { messages, .. } = &self.link else {
                    unreachable!("HTTP replies at once")
                };
                let deadline = Instant::now() + timeout;
                loop {
                    match messages.recv_timeout(Duration::from_millis(100)) {
                        Ok(message) if is_reply(&message, id) => break message,
                        // Notifications, requests to us (we offer no capabilities), replies to calls given up on.
                        Ok(_) => {}
                        Err(RecvTimeoutError::Disconnected) => {
                            return Err("the server stopped".into());
                        }
                        Err(RecvTimeoutError::Timeout) if control.aborted() => {
                            return Err("Aborted by the user".into());
                        }
                        Err(RecvTimeoutError::Timeout) if Instant::now() >= deadline => {
                            return Err(format!("no reply to {method} in {} s", timeout.as_secs()));
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                    }
                }
            }
        };
        if let Some(error) = reply.get("error") {
            return Err(error["message"]
                .as_str()
                .map_or_else(|| error.to_string(), str::to_owned));
        }
        Ok(reply["result"].take())
    }

    fn call(
        &mut self,
        name: &str,
        args: &Map<String, Value>,
        control: &Control,
    ) -> Result<String, String> {
        let result = self.request(
            "tools/call",
            json!({"name": name, "arguments": args}),
            CALL,
            control,
        )?;
        let mut text = String::new();
        for part in result["content"].as_array().into_iter().flatten() {
            if !text.is_empty() {
                text.push('\n');
            }
            match part["text"].as_str() {
                Some(t) => text.push_str(t),
                None => text.push_str(&format!(
                    "[{} content]",
                    part["type"].as_str().unwrap_or("unknown")
                )),
            }
        }
        if text.len() > OUTPUT_CAP {
            text.truncate(text.floor_char_boundary(OUTPUT_CAP));
            text.push_str("\n[output cut at 64 KiB]");
        }
        if result["isError"] == true {
            Err(text)
        } else {
            Ok(text)
        }
    }
}

/// A response to our request `id` (a request from the server has `method`).
fn is_reply(message: &Value, id: u64) -> bool {
    message["id"] == id && message.get("method").is_none()
}

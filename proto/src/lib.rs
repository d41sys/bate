//! Shared contract between the service (`appd`) and the engines it runs, one
//! engine program per bot. Frames are JSON objects, one per line, size-capped
//! per direction. The full list is in `PROTOCOL.md`.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};

pub mod connectors;
pub mod groups;
pub mod memory;
pub mod paths;
pub mod permissions;
pub mod routines;
pub mod skills;

/// Engine protocol version this build speaks; an engine announces it in `ready`.
pub const PROTOCOL: u32 = 1;
/// Largest service → engine frame, newline included.
pub const MAX_OP_FRAME: usize = 1 << 20;
/// Largest engine → service frame, newline included. Tool output is forwarded whole.
pub const MAX_EVENT_FRAME: usize = 64 << 20;
/// Window protocol version this build speaks (`hello`).
pub const WINDOW_PROTOCOL: u32 = 1;
/// Largest window → service frame, newline included.
pub const MAX_WINDOW_FRAME: usize = 1 << 20;

/// One operation sent to an engine, with the id its `response` carries back.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    #[serde(flatten)]
    pub op: Op,
}

/// Every operation an engine implements. Closed: an unknown `type` does not parse.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Op {
    /// Begin a new conversation for `bot`. The engine answers with a `session` event.
    Start { bot: String },
    /// Reopen an earlier conversation.
    Open { bot: String, session: String },
    /// Send a user message. `nonce` is the prompt ledger's key, echoed in `turn_start`.
    Prompt { nonce: String, text: String },
    /// Add a message to the turn that is running.
    Steer { text: String },
    /// End the running turn now.
    Abort,
    /// Answer an open `question` with one of its `options`.
    Answer { question: String, choice: String },
    SetModel {
        model: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
    },
    /// Go back to `entry`: in place, or as a new conversation when `fork`.
    Rewind { entry: String, fork: bool },
    /// Summarise the working context now.
    Compact,
    /// The bot's MCP servers: the whole list each time. The engine uses the enabled ones from its next turn on.
    Connectors {
        connectors: Vec<connectors::Connector>,
    },
    /// What the user told the bot to be and do, put first in its system prompt; empty: nothing.
    Instructions { text: String },
    /// The bots this one may message: the whole list each time. `leads`: it leads a group.
    Peers {
        peers: Vec<groups::Peer>,
        #[serde(default)]
        leads: bool,
    },
    /// What came of the open `message`: the other bot's reply, or why there is none.
    Reply {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// What came of the open `browser` request: the page as the bot sees it, or why not.
    BrowserResult {
        request: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// What came of the open `token` request: the endpoint and authorization token/key for the model call, or why there is none.
    Token {
        request: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Finish and exit.
    Stop,
}

/// Every event an engine sends. Closed: an unknown `type` does not parse.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Event {
    /// First line from a started engine.
    Ready {
        protocol: u32,
    },
    /// Outcome of the request `id`; `error` is absent on success.
    Response {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// The conversation now open.
    Session {
        session: String,
    },
    TurnStart {
        nonce: String,
    },
    Text {
        delta: String,
    },
    Thinking {
        delta: String,
    },
    ToolStart {
        call: String,
        tool: String,
        args: Map<String, Value>,
    },
    ToolEnd {
        call: String,
        output: String,
        is_error: bool,
    },
    /// The engine waits until the service sends `answer` for `question`.
    Question {
        question: String,
        title: String,
        options: Vec<String>,
        /// Set when the question is whether an action may run: the service decides, and asks
        /// the user only where the bot's permissions say so.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<permissions::Kind>,
        /// With `kind`: the exact action (the command, the path, the connector tool).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<String>,
    },
    /// A message for the bot `to`. The engine waits until the service sends `reply` for
    /// `message`.
    Message {
        message: String,
        to: String,
        text: String,
    },
    /// Something to do in the bot's browser, which the service runs. The engine waits until
    /// the service sends `browser_result` for `request`.
    Browser {
        request: String,
        action: Map<String, Value>,
    },
    /// The engine asks the service for the endpoint and token before a model call.
    /// The engine waits until the service sends `token` for `request`.
    Token {
        request: String,
    },
    TurnEnd {
        reason: TurnEnd,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnEnd {
    Done,
    Aborted,
    Error,
}

/// Writes one frame. Frames over `limit` (newline included) are refused, not split.
pub fn write_frame(out: &mut impl Write, frame: &impl Serialize, limit: usize) -> io::Result<()> {
    let mut line = serde_json::to_vec(frame)?;
    line.push(b'\n');
    if line.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame of {} bytes exceeds {limit}", line.len()),
        ));
    }
    out.write_all(&line)?;
    out.flush()
}

/// Reads one frame. `Ok(None)` at end of stream.
/// - A line over `limit` (newline included) is `Err(FileTooLarge)`: the rest of
///   that line is still unread, so the caller must drop the connection.
/// - A line that does not parse as `T` is `Err(InvalidData)` after it has been
///   consumed, so the caller may read on.
pub fn read_frame<T: DeserializeOwned>(
    input: &mut impl BufRead,
    limit: usize,
    buf: &mut Vec<u8>,
) -> io::Result<Option<T>> {
    buf.clear();
    loop {
        let chunk = input.fill_buf()?;
        if chunk.is_empty() {
            if buf.is_empty() {
                return Ok(None);
            }
            break;
        }
        let n = chunk
            .iter()
            .position(|&b| b == b'\n')
            .unwrap_or(chunk.len());
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() >= limit {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "frame too large",
            ));
        }
        if n < chunk.len() {
            input.consume(n + 1);
            break;
        }
        input.consume(n);
    }
    serde_json::from_slice(buf)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// The engine program: `$APPD_ENGINE`, else `agentcore` next to the running
/// executable, else `agentcore` on `PATH`. A relative `$APPD_ENGINE` resolves
/// against the service's working directory, not the bot's folder.
pub fn engine_program() -> PathBuf {
    if let Some(p) = std::env::var_os("APPD_ENGINE").filter(|p| !p.is_empty()) {
        let p = PathBuf::from(p);
        // The engine runs in the bot's folder, so a relative path would resolve there; a bare name stays a `PATH` lookup.
        return if p.components().count() > 1 {
            std::path::absolute(&p).unwrap_or(p)
        } else {
            p
        };
    }
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("agentcore")));
    sibling.unwrap_or_else(|| PathBuf::from("agentcore"))
}

/// A running engine program. `output` can move to its own reader thread.
pub struct Engine {
    pub child: Child,
    pub input: EngineInput,
    pub output: EngineOutput,
    /// The engine's own diagnostics; the service copies them into its log.
    pub stderr: ChildStderr,
}

pub struct EngineInput(ChildStdin);

pub struct EngineOutput {
    stdout: BufReader<ChildStdout>,
    buf: Vec<u8>,
}

impl Engine {
    /// Starts `program` with the bot's folder `workdir` as its working directory.
    /// The caller reads `ready` first, and must drain `stderr` so the engine never blocks on it.
    pub fn spawn(program: &Path, workdir: &Path) -> io::Result<Engine> {
        let mut command = Command::new(program);
        command.current_dir(workdir);
        Engine::run(command)
    }

    /// Starts an engine with a command the caller prepared (its program, folder and limits).
    pub fn run(mut command: Command) -> io::Result<Engine> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        Ok(Engine {
            child,
            input: EngineInput(stdin),
            output: EngineOutput {
                stdout: BufReader::new(stdout),
                buf: Vec::new(),
            },
            stderr,
        })
    }
}

impl EngineInput {
    pub fn send(&mut self, id: &str, op: &Op) -> io::Result<()> {
        #[derive(Serialize)]
        struct Borrowed<'a> {
            id: &'a str,
            #[serde(flatten)]
            op: &'a Op,
        }
        write_frame(&mut self.0, &Borrowed { id, op }, MAX_OP_FRAME)
    }
}

impl EngineOutput {
    /// Next event; `Ok(None)` once the engine closed its output. Errors as in `read_frame`.
    pub fn recv(&mut self) -> io::Result<Option<Event>> {
        read_frame(&mut self.stdout, MAX_EVENT_FRAME, &mut self.buf)
    }
}

//! Shared contract between the service (`appd`) and the engines it runs, one
//! engine program per bot. Frames are JSON objects, one per line, size-capped
//! per direction. The full list is in `PROTOCOL.md`.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

/// Engine protocol version this build speaks; an engine announces it in `ready`.
pub const PROTOCOL: u32 = 1;
/// Largest service → engine frame, newline included.
pub const MAX_OP_FRAME: usize = 1 << 20;
/// Largest engine → service frame, newline included. Tool output is forwarded whole.
pub const MAX_EVENT_FRAME: usize = 64 << 20;

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
}

pub struct EngineInput(ChildStdin);

pub struct EngineOutput {
    stdout: BufReader<ChildStdout>,
    buf: Vec<u8>,
}

impl Engine {
    /// Starts `program` with the bot's folder `workdir` as its working directory.
    /// Its stderr goes to the service's own (the log). The caller reads `ready` first.
    pub fn spawn(program: &Path, workdir: &Path) -> io::Result<Engine> {
        let mut child = Command::new(program)
            .current_dir(workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        Ok(Engine {
            child,
            input: EngineInput(stdin),
            output: EngineOutput {
                stdout: BufReader::new(stdout),
                buf: Vec::new(),
            },
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

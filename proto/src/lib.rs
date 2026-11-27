//! Shared contract between the service (`appd`) and the engines it runs, one
//! engine program per bot. Frames are JSON objects, one per line, size-capped
//! per direction. The full list is in `PROTOCOL.md`.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};

pub mod apple;
pub mod connectors;
#[cfg(target_os = "macos")]
pub mod desktop;
pub mod memory;
pub mod paths;
pub mod permissions;
pub mod projects;
pub mod quick;
pub mod routines;
pub mod secrets;
#[cfg(target_os = "macos")]
pub mod signer;
pub mod skills;
pub mod undo;

/// Engine protocol version this build speaks; an engine announces it in `ready`.
pub const PROTOCOL: u32 = 1;
/// Largest service → engine frame, newline included.
pub const MAX_OP_FRAME: usize = 1 << 20;
/// Largest engine → service frame, newline included. Tool output is forwarded whole.
pub const MAX_EVENT_FRAME: usize = 64 << 20;
/// Tool rounds a turn may take unless the bot has its own limit.
pub const STEP_LIMIT: u32 = 100;
/// Most tool rounds a bot may be given.
pub const MAX_STEP_LIMIT: u32 = 1000;
/// The most characters of an acceptance command (`message_bot`'s `check`).
pub const MAX_CHECK: usize = 1000;
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
    ///
    /// `check`: the turn is one the person started, and the bot's check is on: the engine has
    /// the answer checked before the turn ends (see `Event::Checked`).
    ///
    /// `check_back`: the turn is offered the `check_back` tool (only a turn the person started
    /// in the bot's own chat, and the later turns that follow from it, are).
    Prompt {
        nonce: String,
        text: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        check: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        check_back: bool,
    },
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
    /// Call one tool of a connector (`tool` as the model names it, `<connector>__<tool>`) with `args`, with no model and no
    /// turn, and give up after 30 s. The response comes when the call is over: an error if it failed,
    /// the tool refused it, the connector's tool is blocked, or the engine is busy with a turn. The
    /// answer is sent as `call_output` before it. The service decides beforehand whether the bot may call
    /// the tool without asking.
    CallTool {
        tool: String,
        args: Map<String, Value>,
    },
    /// The bot's MCP servers: the whole list each time. The engine uses the enabled ones from its next turn on.
    Connectors {
        connectors: Vec<connectors::Connector>,
    },
    /// What the user told the bot to be and do, put first in its system prompt; empty: nothing.
    Instructions { text: String },
    /// Folders, beside the bot's own, whose `skills/` the model is told about: the whole list each time, empty for none.
    SkillFolders { folders: Vec<PathBuf> },
    /// Skills of the bot's own `skills/` that are switched off, by name: the model is not told about them. The whole list each time, empty for none.
    SkillsOff { names: Vec<String> },
    /// Connector tools the user blocked for this bot, by the model's name (`<connector>__<tool>`): left out of what the model is offered, and refused if called. The whole list each time, empty for none.
    BlockedTools { names: Vec<String> },
    /// Tool rounds a turn may take before it ends with `limit`, 1 or more; without it, `STEP_LIMIT`.
    StepLimit { steps: u32 },
    /// Whether the model provider's own web search tool is offered, where it has one; on until told otherwise.
    WebSearch { on: bool },
    /// The bots this one may message: the whole list each time. `leads`: it leads its project.
    Peers {
        peers: Vec<projects::Peer>,
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
    /// What came of the open `desktop` request: what the helper saw or did, or why not.
    DesktopResult {
        request: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// What came of the open `check_back` request: what the model is told, or why it is refused.
    CheckBackResult {
        request: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// What came of the open `apple` request: what the app returned, or why not.
    AppleResult {
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
        /// `responses` when the server speaks OpenAI's Responses API; else the chat API.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api: Option<String>,
        /// The model to call when the engine has none of its own.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// The model's context size in tokens, when the server names one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context: Option<usize>,
        /// For the request of a check (`Event::Token` with `check`) that another bot makes: who.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        checker: Option<Checker>,
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
    /// The running turn took a `steer` message into the conversation, between its model calls.
    Steered {
        text: String,
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
        /// The id of the copy a `write` or `edit` kept (`Kept`), which the window's Undo names.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        undo: Option<String>,
    },
    /// The file a `write` or `edit` has just changed, as it was before, kept by the service
    /// under `id` (`undo::MAX_FILE` at most; not for a file that was longer, a link or one that
    /// is not text). Sent before the call's `tool_end`; not part of the transcript.
    Kept {
        id: String,
        /// Full path of the file, with no link in the folders above it.
        path: String,
        /// Its text before; `null`: there was no file.
        earlier: Option<String>,
        /// `undo::digest` of the file as the change left it.
        after: String,
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
        /// Set when the question asks the person for a secret: the options are `Deny` only,
        /// and the service answers `Approve` once the person has typed it and it is in the Keychain (see `secrets`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        secret: Option<secrets::Wanted>,
    },
    /// A message for the bot `to`. The engine waits until the service sends `reply` for
    /// `message`. `check`: the acceptance command the service runs when `to` has done the work.
    Message {
        message: String,
        to: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        check: Option<String>,
    },
    /// Something to do in the bot's browser, which the service runs. The engine waits until
    /// the service sends `browser_result` for `request`.
    Browser {
        request: String,
        action: Map<String, Value>,
    },
    /// Something to do in other apps, which the service has the helper app do. The engine
    /// waits until the service sends `desktop_result` for `request`.
    Desktop {
        request: String,
        action: Map<String, Value>,
    },
    /// Something to do in one of the user's Apple apps (`apple` module), which the service
    /// runs. The engine waits until the service sends `apple_result` for `request`.
    Apple {
        request: String,
        tool: String,
        args: Map<String, Value>,
    },
    /// The model set itself a later turn (`check_back`, with `minutes` or `at`, and `note`),
    /// which the service stores. The engine waits until the service sends `check_back_result`
    /// for `request`. Not part of the transcript.
    CheckBack {
        request: String,
        args: Map<String, Value>,
    },
    /// The connector `name` has just been loaded (started or reached) and has the tools `names`,
    /// each as the model names it (`<connector>__<tool>`), blocked ones included. `read_only` are
    /// those the server marks read-only (`readOnlyHint: true`), a claim of the server's. A connector
    /// that fails is reported on standard error instead. Not part of the transcript.
    ConnectorLoaded {
        name: String,
        names: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        read_only: Vec<String>,
    },
    /// What the open `call_tool` request got: the tool's answer, cut at 64 KiB. Sent before the
    /// request's response, and only when the call went through. Not part of the transcript.
    CallOutput {
        request: String,
        output: String,
    },
    /// What one model request used, as the model server reported it. Sent after each request
    /// that was answered, also when the server reported nothing (`reported` false, the numbers
    /// 0). Not part of the transcript.
    Usage {
        reported: bool,
        #[serde(flatten)]
        tokens: Tokens,
    },
    /// How full the model is: `used` tokens of the `size` it holds. Sent after each request of the
    /// turn itself whose input the server counted (not a sub-agent's, a check's or a summary's),
    /// and after a summary took the older messages' place (then `used` is a guess). Part of the
    /// transcript: the windows show it.
    Context {
        used: u64,
        size: u64,
    },
    /// The engine asks the service for the endpoint and token before a model call.
    /// The engine waits until the service sends `token` for `request`.
    Token {
        request: String,
        /// The request is for the check of an answer (see `Event::Checked`): the service may
        /// answer with the model that checks, and who.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        check: bool,
    },
    /// A model call failed in a way that may pass; the engine asks again after `wait_ms`.
    /// This is retry `attempt` of at most `of`.
    Retry {
        attempt: u32,
        of: u32,
        wait_ms: u64,
        error: String,
    },
    /// What the check of the bot's answer said, with the check's own words in `note`, and the
    /// `by` that made it: a model's name, or a bot's (empty when none was reached). Part of the transcript, between the
    /// bot's text and the end of the turn; never the bot's text.
    Checked {
        result: CheckResult,
        note: String,
        #[serde(default)]
        by: String,
    },
    TurnEnd {
        reason: TurnEnd,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

/// The bot that makes a check: its name, and its own instructions, which the check's request
/// has after the rules of the check.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checker {
    pub name: String,
    pub instructions: String,
}

/// How a check of an answer went.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckResult {
    /// The check found nothing to fix.
    Passed,
    /// The check found something: its notes go to the bot, which works once more.
    Failed,
    /// The last check the turn allows found something: the turn ends with the answer as it is.
    NotPassed,
    /// The check could not be made (`note` says why): the turn ends as if it had no check.
    NotChecked,
}

/// Tokens a model server said were used. `input` counts all the request's input, and `cached`
/// is the part of it that came from the server's cache.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tokens {
    #[serde(default)]
    pub input: u64,
    #[serde(default)]
    pub cached: u64,
    #[serde(default)]
    pub output: u64,
}

impl Tokens {
    /// What a limit counts: input and output.
    pub fn total(&self) -> u64 {
        self.input.saturating_add(self.output)
    }
}

impl std::ops::AddAssign for Tokens {
    fn add_assign(&mut self, other: Tokens) {
        self.input = self.input.saturating_add(other.input);
        self.cached = self.cached.saturating_add(other.cached);
        self.output = self.output.saturating_add(other.output);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnEnd {
    Done,
    Aborted,
    Error,
    /// A limit stopped the turn; `error` says which: the tool rounds it used up, or the
    /// bot's token limit.
    Limit,
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

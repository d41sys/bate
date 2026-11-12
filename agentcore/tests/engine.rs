//! `agentcore` over stdin/stdout against a mock OpenAI-compatible model server.

use proto::connectors::{Connector, Transport};
use proto::{Event, MAX_EVENT_FRAME, MAX_OP_FRAME, Op, Request, TurnEnd};
use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(10);

/// Serves one scripted streaming reply per request, in order, and records each request body.
struct Model {
    url: String,
    requests: Arc<Mutex<Vec<Value>>>,
}

impl Model {
    fn serve(replies: Vec<Vec<Value>>) -> Model {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        thread::spawn(move || {
            for (stream, chunks) in listener.incoming().zip(replies) {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                seen.lock()
                    .unwrap()
                    .push(serde_json::from_slice(&body).unwrap());
                let mut reply = String::from(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n",
                );
                for chunk in chunks {
                    reply.push_str(&format!("data: {chunk}\n\n"));
                }
                reply.push_str("data: [DONE]\n\n");
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        Model { url, requests }
    }

    fn requests(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }
}

fn delta(d: Value) -> Value {
    json!({"choices": [{"index": 0, "delta": d}]})
}
fn text(t: &str) -> Value {
    delta(json!({"content": t}))
}
fn thinking(t: &str) -> Value {
    delta(json!({"reasoning_content": t}))
}
fn call(id: &str, tool: &str, args: Value) -> Value {
    delta(
        json!({"tool_calls": [{"index": 0, "id": id, "type": "function",
        "function": {"name": tool, "arguments": args.to_string()}}]}),
    )
}

struct Engine {
    child: Child,
    input: ChildStdin,
    events: Receiver<Event>,
    next_id: u32,
}

impl Engine {
    fn start(dir: &Path, model: &Model) -> Engine {
        Engine::start_with(dir, model, &[])
    }

    fn start_with(dir: &Path, model: &Model, env: &[(&str, &str)]) -> Engine {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agentcore"))
            .current_dir(dir)
            .env("BATE_MODEL_BASE_URL", &model.url)
            .env_remove("BATE_MODEL_API_KEY")
            .env_remove("BATE_MODEL")
            .env_remove("BATE_MODEL_CONTEXT")
            .envs(env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let (tx, events) = mpsc::channel();
        thread::spawn(move || {
            let mut buf = Vec::new();
            while let Ok(Some(event)) =
                proto::read_frame::<Event>(&mut output, MAX_EVENT_FRAME, &mut buf)
            {
                if tx.send(event).is_err() {
                    return;
                }
            }
        });
        let engine = Engine {
            child,
            input,
            events,
            next_id: 0,
        };
        assert_eq!(
            engine.next(),
            Event::Ready {
                protocol: proto::PROTOCOL
            }
        );
        engine
    }

    fn next(&self) -> Event {
        self.events.recv_timeout(WAIT).expect("an event in time")
    }

    fn send(&mut self, op: Op) -> String {
        self.next_id += 1;
        let id = self.next_id.to_string();
        proto::write_frame(
            &mut self.input,
            &Request { id: id.clone(), op },
            MAX_OP_FRAME,
        )
        .unwrap();
        id
    }

    /// Sends `op` and returns its response's error, skipping nothing: the response must come first.
    fn request(&mut self, op: Op) -> Option<String> {
        let id = self.send(op);
        match self.next() {
            Event::Response { id: got, error } if got == id => error,
            other => panic!("expected the response to {id}, got {other:?}"),
        }
    }

    /// Every event up to and including the first that matches.
    fn until(&self, done: impl Fn(&Event) -> bool) -> Vec<Event> {
        let mut seen = Vec::new();
        loop {
            let e = self.next();
            let stop = done(&e);
            seen.push(e);
            if stop {
                return seen;
            }
        }
    }

    /// `start` and `set_model`; returns the session id.
    fn open_new(&mut self) -> String {
        assert_eq!(self.request(Op::Start { bot: "b1".into() }), None);
        let Event::Session { session } = self.next() else {
            panic!("expected session")
        };
        assert_eq!(
            self.request(Op::SetModel {
                model: "m1".into(),
                effort: None
            }),
            None
        );
        session
    }

    fn prompt(&mut self, text: &str) {
        self.prompt_as("n1", text);
    }

    fn prompt_as(&mut self, nonce: &str, text: &str) {
        let op = Op::Prompt {
            nonce: nonce.into(),
            text: text.into(),
        };
        assert_eq!(self.request(op), None);
        assert_eq!(
            self.next(),
            Event::TurnStart {
                nonce: nonce.into()
            }
        );
    }

    /// One whole turn with no tool calls.
    fn exchange(&mut self, nonce: &str, text: &str) {
        self.prompt_as(nonce, text);
        self.until(is_turn_end);
    }

    /// Kills this engine and opens `session` in a new one.
    fn reopen(self, dir: &Path, model: &Model, session: &str) -> Engine {
        drop(self);
        let mut engine = Engine::start(dir, model);
        let op = Op::Open {
            bot: "b1".into(),
            session: session.into(),
        };
        assert_eq!(engine.request(op), None);
        assert_eq!(
            engine.next(),
            Event::Session {
                session: session.into()
            }
        );
        let set = Op::SetModel {
            model: "m1".into(),
            effort: None,
        };
        assert_eq!(engine.request(set), None);
        engine
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A working folder, removed when the test passes; kept after a failure, for a look at what is in it.
struct Dir(PathBuf);

impl std::ops::Deref for Dir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

fn workdir(name: &str) -> Dir {
    let d = std::env::temp_dir().join(format!("agentcore-engine-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    Dir(d)
}

fn is_turn_end(e: &Event) -> bool {
    matches!(e, Event::TurnEnd { .. })
}

#[test]
fn a_turn_streams_thinking_and_text_reads_a_file_and_ends_done() {
    let dir = workdir("turn");
    fs::write(dir.join("a.txt"), "hello from the file\n").unwrap();
    let model = Model::serve(vec![
        vec![
            thinking("Look first."),
            text("Reading."),
            call("c1", "read", json!({"path": "a.txt"})),
        ],
        vec![text("It says hello.")],
    ]);
    let mut engine = Engine::start(&dir, &model);
    engine.open_new();
    engine.prompt("what is in a.txt?");
    let events = engine.until(is_turn_end);
    assert_eq!(
        events[0],
        Event::Thinking {
            delta: "Look first.".into()
        }
    );
    assert_eq!(
        events[1],
        Event::Text {
            delta: "Reading.".into()
        }
    );
    assert!(
        matches!(&events[2], Event::ToolStart { call, tool, .. } if call == "c1" && tool == "read")
    );
    assert!(
        matches!(&events[3], Event::ToolEnd { output, is_error: false, .. } if output.contains("hello from the file"))
    );
    assert_eq!(
        events[4],
        Event::Text {
            delta: "It says hello.".into()
        }
    );
    assert_eq!(
        events[5],
        Event::TurnEnd {
            reason: TurnEnd::Done,
            error: None
        }
    );

    let requests = model.requests();
    assert_eq!(requests[0]["model"], "m1");
    assert_eq!(requests[0]["stream"], true);
    let fed_back = &requests[1]["messages"];
    let tool_message = fed_back
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "tool")
        .unwrap();
    assert_eq!(tool_message["tool_call_id"], "c1");
    assert!(
        tool_message["content"]
            .as_str()
            .unwrap()
            .contains("hello from the file")
    );
}

fn question(events: &[Event]) -> String {
    match events.last() {
        Some(Event::Question {
            question, options, ..
        }) => {
            assert_eq!(options, &["Approve", "Deny"]);
            question.clone()
        }
        other => panic!("expected a question, got {other:?}"),
    }
}

fn is_question(e: &Event) -> bool {
    matches!(e, Event::Question { .. })
}

#[test]
fn a_changing_tool_asks_first_approve_writes_and_deny_does_not() {
    let dir = workdir("ask");
    let model = Model::serve(vec![
        vec![call(
            "w1",
            "write",
            json!({"path": "yes.txt", "content": "ok"}),
        )],
        vec![text("Written.")],
        vec![call(
            "w2",
            "write",
            json!({"path": "no.txt", "content": "ok"}),
        )],
        vec![text("Not written.")],
    ]);
    let mut engine = Engine::start(&dir, &model);
    engine.open_new();

    engine.prompt("write yes.txt");
    let q = question(&engine.until(is_question));
    assert!(
        engine
            .request(Op::Answer {
                question: "other".into(),
                choice: "Approve".into()
            })
            .is_some()
    );
    assert!(
        engine
            .request(Op::Answer {
                question: q.clone(),
                choice: "Maybe".into()
            })
            .is_some()
    );
    assert_eq!(
        engine.request(Op::Answer {
            question: q,
            choice: "Approve".into()
        }),
        None
    );
    let events = engine.until(is_turn_end);
    assert!(
        matches!(
            &events[0],
            Event::ToolEnd {
                is_error: false,
                ..
            }
        ),
        "{events:?}"
    );
    assert_eq!(fs::read_to_string(dir.join("yes.txt")).unwrap(), "ok");

    engine.prompt("write no.txt");
    let q = question(&engine.until(is_question));
    assert_eq!(
        engine.request(Op::Answer {
            question: q,
            choice: "Deny".into()
        }),
        None
    );
    let events = engine.until(is_turn_end);
    assert!(
        matches!(&events[0], Event::ToolEnd { is_error: true, output, .. } if output.contains("denied")),
        "{events:?}"
    );
    assert!(!dir.join("no.txt").exists());
    let told = model.requests()[3]["messages"].to_string();
    assert!(told.contains("The user denied this action."));
}

/// Starts a `run` of `sleep 30` that records its pid, approves it, and returns the pid once it runs.
fn start_long_command(engine: &mut Engine, dir: &Path) -> i32 {
    engine.prompt("run something long");
    let q = question(&engine.until(is_question));
    assert_eq!(
        engine.request(Op::Answer {
            question: q,
            choice: "Approve".into()
        }),
        None
    );
    let pid_file = dir.join("pid");
    for _ in 0..200 {
        if let Ok(pid) = fs::read_to_string(&pid_file).map(|p| p.trim().parse::<i32>()) {
            return pid.unwrap();
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("the command never started");
}

fn alive(pid: i32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

fn long_command() -> Value {
    call(
        "r1",
        "run",
        json!({"command": "sh -c 'echo $$ > pid; exec sleep 30'"}),
    )
}

#[test]
fn abort_ends_the_turn_aborted_and_kills_the_running_command() {
    let dir = workdir("abort");
    let model = Model::serve(vec![vec![long_command()]]);
    let mut engine = Engine::start(&dir, &model);
    engine.open_new();
    let pid = start_long_command(&mut engine, &dir);
    assert!(alive(pid));
    assert_eq!(engine.request(Op::Abort), None);
    let events = engine.until(is_turn_end);
    assert!(
        matches!(&events[0], Event::ToolEnd { is_error: true, output, .. } if output.contains("Aborted")),
        "{events:?}"
    );
    assert_eq!(
        events.last().unwrap(),
        &Event::TurnEnd {
            reason: TurnEnd::Aborted,
            error: None
        }
    );
    thread::sleep(Duration::from_millis(100));
    assert!(!alive(pid), "the command still runs");
}

#[test]
fn a_killed_engine_leaves_its_unfinished_call_interrupted_on_open() {
    let dir = workdir("interrupted");
    let model = Model::serve(vec![vec![long_command()], vec![text("Back.")]]);
    let mut engine = Engine::start(&dir, &model);
    let session = engine.open_new();
    let pid = start_long_command(&mut engine, &dir);
    drop(engine); // SIGKILL mid-call
    let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();

    let mut engine = Engine::start(&dir, &model);
    assert_eq!(
        engine.request(Op::Open {
            bot: "b1".into(),
            session: session.clone()
        }),
        None
    );
    assert_eq!(engine.next(), Event::Session { session });
    match engine.next() {
        Event::ToolEnd {
            call,
            output,
            is_error,
        } => {
            assert_eq!(call, "r1");
            assert!(is_error && output.starts_with("Interrupted"));
        }
        other => panic!("expected the interrupted call, got {other:?}"),
    }
    // The conversation stays well-formed: the next request carries the closed call.
    assert_eq!(
        engine.request(Op::SetModel {
            model: "m1".into(),
            effort: None
        }),
        None
    );
    engine.prompt("go on");
    engine.until(is_turn_end);
    let messages = model.requests()[1]["messages"].to_string();
    assert!(messages.contains("Interrupted"), "{messages}");
}

/// One scripted text reply per string.
fn replies(texts: &[&str]) -> Vec<Vec<Value>> {
    texts.iter().map(|t| vec![text(t)]).collect()
}

/// The conversation part of request `i`, as text to search.
fn sent(model: &Model, i: usize) -> String {
    model.requests()[i]["messages"].to_string()
}

#[test]
fn compact_replaces_older_messages_with_a_summary_and_it_survives_a_restart() {
    let dir = workdir("compact");
    let model = Model::serve(replies(&["A1-old", "A2", "A3", "SUMMARY-XYZ", "A4", "A5"]));
    let mut engine = Engine::start(&dir, &model);
    let session = engine.open_new();
    engine.exchange("n1", "Q1-old");
    engine.exchange("n2", "Q2");
    engine.exchange("n3", "Q3");

    // Nothing but the response comes back: the summary is not streamed as a reply.
    assert_eq!(engine.request(Op::Compact), None);
    let asked = sent(&model, 3);
    assert!(
        asked.contains("Q1-old") && asked.contains("Summarise"),
        "{asked}"
    );

    engine.exchange("n4", "Q4");
    let after = sent(&model, 4);
    assert!(
        after.contains("SUMMARY-XYZ") && after.contains("Q2") && after.contains("A3"),
        "{after}"
    );
    assert!(
        !after.contains("Q1-old") && !after.contains("A1-old"),
        "{after}"
    );

    let mut engine = engine.reopen(&dir, &model, &session);
    engine.exchange("n5", "Q5");
    let reopened = sent(&model, 5);
    assert!(
        reopened.contains("SUMMARY-XYZ") && reopened.contains("Q4"),
        "{reopened}"
    );
    assert!(!reopened.contains("Q1-old"), "{reopened}");
}

#[test]
fn a_context_near_its_limit_is_compacted_before_the_model_call() {
    let dir = workdir("autocompact");
    let model = Model::serve(replies(&["A1", "A2", "AUTO-SUMMARY", "A3"]));
    // 100 tokens: compaction starts above 80, which is about 320 bytes of messages.
    let mut engine = Engine::start_with(&dir, &model, &[("BATE_MODEL_CONTEXT", "100")]);
    engine.open_new();
    let long = "x".repeat(150);
    engine.exchange("n1", &format!("Q1-old {long}"));
    engine.exchange("n2", &format!("Q2 {long}"));
    engine.exchange("n3", "Q3");

    assert_eq!(model.requests().len(), 4);
    assert!(sent(&model, 2).contains("Summarise"));
    let last = sent(&model, 3);
    assert!(
        last.contains("AUTO-SUMMARY") && last.contains("Q3"),
        "{last}"
    );
    assert!(!last.contains("Q1-old"), "{last}");
}

#[test]
fn rewind_in_place_drops_the_prompt_and_everything_after_it() {
    let dir = workdir("rewind");
    let model = Model::serve(replies(&["A1", "A2-dropped", "A3", "A4"]));
    let mut engine = Engine::start(&dir, &model);
    let session = engine.open_new();
    engine.exchange("n1", "Q1");
    engine.exchange("n2", "Q2-dropped");

    let unknown = Op::Rewind {
        entry: "nope".into(),
        fork: false,
    };
    assert!(engine.request(unknown).is_some());
    let rewind = Op::Rewind {
        entry: "n2".into(),
        fork: false,
    };
    assert_eq!(engine.request(rewind), None);

    // The next event is this prompt's response: an in-place rewind sends no `session`.
    engine.exchange("n3", "Q3");
    let after = sent(&model, 2);
    assert!(
        after.contains("Q1") && after.contains("A1") && after.contains("Q3"),
        "{after}"
    );
    assert!(!after.contains("dropped"), "{after}");

    let mut engine = engine.reopen(&dir, &model, &session);
    engine.exchange("n4", "Q4");
    assert!(!sent(&model, 3).contains("dropped"));
}

#[test]
fn rewind_as_a_fork_starts_a_new_conversation_and_keeps_the_original() {
    let dir = workdir("fork");
    let model = Model::serve(replies(&["A1", "A2-original", "A3", "A4", "A5"]));
    let mut engine = Engine::start(&dir, &model);
    let original = engine.open_new();
    engine.exchange("n1", "Q1");
    engine.exchange("n2", "Q2-original");

    let fork = Op::Rewind {
        entry: "n2".into(),
        fork: true,
    };
    assert_eq!(engine.request(fork), None);
    let Event::Session { session: copy } = engine.next() else {
        panic!("expected the new session")
    };
    assert_ne!(copy, original);
    engine.exchange("n3", "Q3");
    let forked = sent(&model, 2);
    assert!(
        forked.contains("Q1") && !forked.contains("original"),
        "{forked}"
    );

    // The copy is what was written, not only what this engine remembers.
    let mut engine = engine.reopen(&dir, &model, &copy);
    engine.exchange("n4", "Q4");
    let copied = sent(&model, 3);
    assert!(
        copied.contains("Q3") && !copied.contains("original"),
        "{copied}"
    );

    let mut engine = engine.reopen(&dir, &model, &original);
    engine.exchange("n5", "Q5");
    let kept = sent(&model, 4);
    assert!(
        kept.contains("Q2-original") && kept.contains("A2-original"),
        "{kept}"
    );
    assert!(!kept.contains("Q3"), "{kept}");
}

/// An MCP server over stdio: one tool, `echo`, which answers "echoed" and logs each call to `calls.log`.
const FAKE_MCP: &str = r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"protocolVersion\":\"2025-06-18\",\"capabilities\":{\"tools\":{}},\"serverInfo\":{\"name\":\"fake\",\"version\":\"1\"}}}" ;;
    *'"method":"tools/list"'*) echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"tools\":[{\"name\":\"echo\",\"description\":\"Echo\",\"inputSchema\":{\"type\":\"object\",\"properties\":{\"text\":{\"type\":\"string\"}}}}]}}" ;;
    *'"method":"tools/call"'*) printf '%s\n' "$line" >> calls.log
      echo '{"jsonrpc":"2.0","method":"notifications/message","params":{"level":"info","data":"working"}}'
      echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"echoed\"}]}}" ;;
  esac
done
"#;

fn connector(name: &str, transport: Transport) -> Connector {
    Connector {
        name: name.into(),
        enabled: true,
        transport,
    }
}

fn stdio(command: &Path) -> Transport {
    Transport::Stdio {
        command: command.to_str().unwrap().into(),
        args: Vec::new(),
        env: Default::default(),
    }
}

/// The function names in request `i`'s tool list.
fn tool_names(model: &Model, i: usize) -> Vec<String> {
    model.requests()[i]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["function"]["name"].as_str().unwrap().to_owned())
        .collect()
}

fn answer(engine: &mut Engine, choice: &str) -> Vec<Event> {
    let events = engine.until(is_question);
    let q = question(&events);
    assert_eq!(
        engine.request(Op::Answer {
            question: q,
            choice: choice.into()
        }),
        None
    );
    events
}

#[test]
fn a_stdio_connector_lists_its_tools_and_each_call_asks_first() {
    let dir = workdir("mcp-stdio");
    let script = dir.join("fake-mcp");
    fs::write(&script, FAKE_MCP).unwrap();
    fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let model = Model::serve(vec![
        vec![call("c1", "fake__echo", json!({"text": "hi"}))],
        vec![text("Done.")],
        vec![call("c2", "fake__echo", json!({"text": "again"}))],
        vec![text("Not done.")],
        vec![text("Bye.")],
    ]);
    let mut engine = Engine::start(&dir, &model);
    engine.open_new();
    let mut off = connector("off", stdio(&script));
    off.enabled = false;
    let connectors = vec![
        connector("fake", stdio(&script)),
        connector("broken", stdio(Path::new("/nonexistent/mcp-server"))),
        off,
    ];
    assert_eq!(engine.request(Op::Connectors { connectors }), None);

    engine.prompt_as("n1", "echo hi");
    let asked = answer(&mut engine, "Approve");
    assert!(
        matches!(asked.last(), Some(Event::Question { title, .. }) if title == "Use fake: echo"),
        "{asked:?}"
    );
    let events = engine.until(is_turn_end);
    assert!(
        matches!(&events[0], Event::ToolEnd { output, is_error: false, .. } if output == "echoed"),
        "{events:?}"
    );
    let names = tool_names(&model, 0);
    assert!(names.contains(&"fake__echo".to_owned()) && names.contains(&"read".to_owned()));
    assert!(
        !names
            .iter()
            .any(|n| n.starts_with("broken") || n.starts_with("off")),
        "{names:?}"
    );
    let calls = fs::read_to_string(dir.join("calls.log")).unwrap();
    assert!(
        calls.lines().count() == 1 && calls.contains(r#""text":"hi""#),
        "{calls}"
    );

    engine.prompt_as("n2", "echo again");
    answer(&mut engine, "Deny");
    let events = engine.until(is_turn_end);
    assert!(
        matches!(&events[0], Event::ToolEnd { is_error: true, .. }),
        "{events:?}"
    );
    assert_eq!(
        fs::read_to_string(dir.join("calls.log"))
            .unwrap()
            .lines()
            .count(),
        1
    );

    // A connector taken off the list is gone from the next turn.
    let none = Op::Connectors {
        connectors: Vec::new(),
    };
    assert_eq!(engine.request(none), None);
    engine.exchange("n3", "bye");
    assert!(!tool_names(&model, 4).iter().any(|n| n.starts_with("fake")));
}

/// Reads one HTTP request: its header block in lower case, and its JSON body.
fn read_http(stream: &std::net::TcpStream) -> (String, Value) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let (mut head, mut length) = (String::new(), 0);
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        let line = line.to_ascii_lowercase();
        if let Some(v) = line.strip_prefix("content-length:") {
            length = v.trim().parse().unwrap();
        }
        head.push_str(&line);
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    (head, serde_json::from_slice(&body).unwrap())
}

/// An MCP server over HTTP with one tool, `hello`. Replies to `tools/call` as an event stream,
/// to the rest as plain JSON. Hands over each request's headers and body.
fn mcp_http() -> (String, Receiver<(String, Value)>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let (tx, seen) = mpsc::channel();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let (head, body) = read_http(&stream);
            let id = body["id"].clone();
            let result =
                |result: Value| json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string();
            let (status, kind, extra, payload) = match body["method"].as_str().unwrap() {
                "initialize" => (
                    "200 OK",
                    "application/json",
                    "mcp-session-id: sess-1\r\n",
                    result(
                        json!({"protocolVersion": "2025-06-18", "capabilities": {"tools": {}},
                        "serverInfo": {"name": "web", "version": "1"}}),
                    ),
                ),
                "tools/list" => (
                    "200 OK",
                    "application/json",
                    "",
                    result(
                        json!({"tools": [{"name": "hello", "inputSchema": {"type": "object"}}]}),
                    ),
                ),
                "tools/call" => (
                    "200 OK",
                    "text/event-stream",
                    "",
                    format!(
                        "event: message\ndata: {}\n\n",
                        result(json!({"content": [{"type": "text", "text": "from http"}]}))
                    ),
                ),
                _ => ("202 Accepted", "application/json", "", String::new()),
            };
            let _ = tx.send((head, body));
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\ncontent-type: {kind}\r\n{extra}content-length: {}\r\nconnection: close\r\n\r\n{payload}",
                payload.len()
            );
        }
    });
    (url, seen)
}

#[test]
fn an_http_connector_keeps_its_session_and_headers_and_reads_a_streamed_reply() {
    let dir = workdir("mcp-http");
    let (url, seen) = mcp_http();
    let model = Model::serve(vec![
        vec![call("c1", "web__hello", json!({}))],
        vec![text("Done.")],
    ]);
    let mut engine = Engine::start(&dir, &model);
    engine.open_new();
    let transport = Transport::Http {
        url,
        headers: [("Authorization".to_owned(), "Bearer t0ken".to_owned())].into(),
    };
    let connectors = vec![connector("web", transport)];
    assert_eq!(engine.request(Op::Connectors { connectors }), None);

    engine.prompt_as("n1", "say hello");
    answer(&mut engine, "Approve");
    let events = engine.until(is_turn_end);
    assert!(
        matches!(&events[0], Event::ToolEnd { output, is_error: false, .. } if output == "from http"),
        "{events:?}"
    );
    let requests: Vec<(String, Value)> = seen.try_iter().collect();
    let methods: Vec<&str> = requests
        .iter()
        .map(|(_, b)| b["method"].as_str().unwrap())
        .collect();
    assert_eq!(
        methods,
        [
            "initialize",
            "notifications/initialized",
            "tools/list",
            "tools/call"
        ]
    );
    let (head, body) = &requests[3];
    assert!(
        head.contains("mcp-session-id: sess-1") && head.contains("authorization: bearer t0ken"),
        "{head}"
    );
    assert_eq!(body["params"]["name"], "hello");
}

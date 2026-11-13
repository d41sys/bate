//! `agentcore` over stdin/stdout against a mock OpenAI-compatible model server.

use proto::connectors::{Connector, Transport};
use proto::groups::Peer;
use proto::permissions::Kind;
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
/// A `null` chunk holds that reply open.
struct Model {
    url: String,
    requests: Arc<Mutex<Vec<Value>>>,
}

impl Model {
    fn serve(replies: Vec<Vec<Value>>) -> Model {
        let body = |chunks: Vec<Value>| {
            let mut body = String::new();
            for chunk in chunks {
                // `null` holds the reply open until the engine hangs up.
                if chunk.is_null() {
                    return body.into_bytes();
                }
                body.push_str(&format!("data: {chunk}\n\n"));
            }
            (body + "data: [DONE]\n\n").into_bytes()
        };
        // Like a server that keeps the body open: the engine must hang up by itself, or the
        // next request is never served.
        Model::send(replies.into_iter().map(body).collect(), true)
    }

    /// Sends recorded reply bodies byte for byte, one per request, and closes after each.
    fn replay(bodies: Vec<Vec<u8>>) -> Model {
        Model::send(bodies, false)
    }

    fn send(bodies: Vec<Vec<u8>>, keep_open: bool) -> Model {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        thread::spawn(move || {
            for (stream, reply) in listener.incoming().zip(bodies) {
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
                let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&reply);
                if keep_open {
                    let _ = reader.read(&mut [0]);
                }
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
    // Reading is asked about too: whether it needs the user is the service's decision.
    let mut events = answer(&mut engine, "Approve");
    assert!(
        matches!(events.pop(), Some(Event::Question { kind: Some(Kind::Read), target: Some(t), .. }) if t == "a.txt")
    );
    events.extend(engine.until(is_turn_end));
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
            "w1",
            "write",
            json!({"path": "no.txt", "content": "ok"}),
        )],
        vec![text("Not written.")],
    ]);
    let mut engine = Engine::start(&dir, &model);
    engine.open_new();

    engine.prompt("write yes.txt");
    let asked = engine.until(is_question);
    assert!(
        matches!(asked.last(), Some(Event::Question { kind: Some(Kind::Write), target: Some(t), .. }) if t == "yes.txt"),
        "{asked:?}"
    );
    let q = question(&asked);
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
            question: q.clone(),
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
    let first = q;
    let q = question(&engine.until(is_question));
    // The model repeated its call id; the question has its own, so a late answer to the first
    // question cannot approve this one.
    assert_ne!(q, first);
    assert!(
        engine
            .request(Op::Answer {
                question: first,
                choice: "Approve".into()
            })
            .is_some()
    );
    // One answer per question: an Approve sent right behind the Deny is refused, not taken
    // and dropped.
    let answer = |choice: &str| Op::Answer {
        question: q.clone(),
        choice: choice.into(),
    };
    let (deny, approve) = (engine.send(answer("Deny")), engine.send(answer("Approve")));
    assert_eq!(
        engine.next(),
        Event::Response {
            id: deny,
            error: None
        }
    );
    let events = engine.until(is_turn_end);
    let refused = events
        .iter()
        .position(|e| matches!(e, Event::Response { id, error: Some(_) } if *id == approve))
        .expect("the second answer is refused");
    let events = [&events[..refused], &events[refused + 1..]].concat();
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

/// An MCP server over stdio: one tool, `echo`, which answers "echoed". It writes its process id
/// to `mcp.pid` and each call to `calls.log`.
const FAKE_MCP: &str = r#"#!/bin/sh
echo $$ > mcp.pid
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
        oauth: false,
        client_id: None,
        signed_in: None,
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
        matches!(asked.last(), Some(Event::Question { title, kind: Some(Kind::Connector), target: Some(t), .. })
            if title == "Use fake: echo" && t == "fake__echo"),
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
                        json!({"tools": [{"name": "hello", "inputSchema": {"type": "object"}},
                            {"name": "slow", "inputSchema": {"type": "object"}}]}),
                    ),
                ),
                "tools/call" => ("200 OK", "text/event-stream", "", {
                    // One event whose data is split over two lines.
                    let reply = result(json!({"content": [{"type": "text", "text": "from http"}]}));
                    let (a, b) = reply.split_at(reply.find(',').unwrap() + 1);
                    format!("event: message\ndata: {a}\ndata: {b}\n\n")
                }),
                _ => ("202 Accepted", "application/json", "", String::new()),
            };
            let slow = body["params"]["name"] == "slow";
            let _ = tx.send((head, body));
            if slow {
                thread::sleep(WAIT * 3);
            }
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
        vec![call("c2", "web__hello", json!({}))],
        vec![text("Not done.")],
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
    assert!(tool_names(&model, 0).contains(&"web__hello".to_owned()));

    // A denied call never reaches the server: one `tools/call` below.
    engine.prompt_as("n2", "say hello again");
    answer(&mut engine, "Deny");
    engine.until(is_turn_end);
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

#[test]
fn compaction_keeps_a_tool_result_together_with_its_call() {
    let dir = workdir("compact-tools");
    fs::write(dir.join("a.txt"), "content\n").unwrap();
    let model = Model::serve(vec![
        vec![call("c1", "read", json!({"path": "a.txt"}))],
        vec![text("A1")],
        vec![text("A2")],
        vec![text("SUMMARY")],
        vec![text("A3")],
    ]);
    let mut engine = Engine::start(&dir, &model);
    engine.open_new();
    engine.prompt_as("n1", "Q1");
    answer(&mut engine, "Approve");
    engine.until(is_turn_end);
    engine.exchange("n2", "Q2");
    // Six messages, the last four kept: the cut would fall on the tool result.
    assert_eq!(engine.request(Op::Compact), None);
    engine.exchange("n3", "Q3");

    let messages = model.requests()[4]["messages"].as_array().unwrap().clone();
    let result = messages
        .iter()
        .position(|m| m["role"] == "tool")
        .expect("the result is kept");
    let call_before = messages[..result]
        .iter()
        .any(|m| m["tool_calls"][0]["id"] == "c1");
    assert!(call_before, "{messages:?}");
}

#[test]
fn a_summary_that_fails_does_not_end_the_turn() {
    let dir = workdir("compact-fails");
    // The third reply answers the request for a summary with a tool call and no text.
    let model = Model::serve(vec![
        vec![text("A1")],
        vec![text("A2")],
        vec![call("c1", "read", json!({"path": "a.txt"}))],
        vec![text("A3")],
    ]);
    let mut engine = Engine::start_with(&dir, &model, &[("BATE_MODEL_CONTEXT", "100")]);
    engine.open_new();
    let long = "x".repeat(150);
    engine.exchange("n1", &format!("Q1 {long}"));
    engine.exchange("n2", &format!("Q2 {long}"));
    engine.prompt_as("n3", "Q3");
    let events = engine.until(is_turn_end);
    assert_eq!(
        events,
        [
            Event::Text { delta: "A3".into() },
            Event::TurnEnd {
                reason: TurnEnd::Done,
                error: None
            }
        ]
    );
    // Nothing was summarised, so the model still gets the whole conversation.
    assert!(sent(&model, 3).contains("Q1"));
}

fn aborted() -> Event {
    Event::TurnEnd {
        reason: TurnEnd::Aborted,
        error: None,
    }
}

#[test]
fn abort_ends_a_call_to_an_http_connector_that_does_not_answer() {
    let dir = workdir("mcp-http-abort");
    let (url, seen) = mcp_http();
    let model = Model::serve(vec![vec![call("c1", "web__slow", json!({}))]]);
    let mut engine = Engine::start(&dir, &model);
    engine.open_new();
    let transport = Transport::Http {
        url,
        headers: Default::default(),
    };
    let connectors = vec![connector("web", transport)];
    assert_eq!(engine.request(Op::Connectors { connectors }), None);

    engine.prompt_as("n1", "wait for it");
    answer(&mut engine, "Approve");
    // The server has the call and holds it for longer than this test waits.
    while seen.recv_timeout(WAIT).unwrap().1["method"] != "tools/call" {}
    assert_eq!(engine.request(Op::Abort), None);
    let events = engine.until(is_turn_end);
    assert!(
        matches!(&events[0], Event::ToolEnd { is_error: true, output, .. } if output.contains("Aborted")),
        "{events:?}"
    );
    assert_eq!(events.last(), Some(&aborted()));
}

fn script(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, text).unwrap();
    fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    path
}

fn eventually(what: &str, done: impl Fn() -> bool) {
    for _ in 0..400 {
        if done() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn a_stdio_server_that_stopped_is_started_again_for_the_next_turn() {
    let dir = workdir("mcp-restart");
    let server = script(&dir, "fake-mcp", FAKE_MCP);
    let model = Model::serve(vec![
        vec![text("Hi.")],
        vec![call("c1", "fake__echo", json!({"text": "hi"}))],
        vec![text("Done.")],
    ]);
    let mut engine = Engine::start(&dir, &model);
    engine.open_new();
    let connectors = vec![connector("fake", stdio(&server))];
    assert_eq!(engine.request(Op::Connectors { connectors }), None);
    engine.exchange("n1", "hello");

    let pid_file = dir.join("mcp.pid");
    let first = fs::read_to_string(&pid_file).unwrap();
    let state = || {
        let ps = Command::new("ps")
            .args(["-o", "state=", "-p", first.trim()])
            .output()
            .unwrap();
        String::from_utf8_lossy(&ps.stdout).trim().to_owned()
    };
    Command::new("kill").arg(first.trim()).status().unwrap();
    // Gone, or dead and not yet collected by the engine.
    eventually("the server to stop", || {
        state().is_empty() || state().starts_with('Z')
    });

    engine.prompt_as("n2", "echo hi");
    answer(&mut engine, "Approve");
    let events = engine.until(is_turn_end);
    assert!(
        matches!(&events[0], Event::ToolEnd { output, is_error: false, .. } if output == "echoed"),
        "{events:?}"
    );
    assert_ne!(fs::read_to_string(&pid_file).unwrap(), first);
}

#[test]
fn a_connector_start_that_was_aborted_is_tried_again_on_the_next_turn() {
    let dir = workdir("mcp-slow-start");
    script(&dir, "fake-mcp", FAKE_MCP);
    // Hangs the first time it is started, and is the fake server after that.
    let slow = script(
        &dir,
        "slow-mcp",
        "#!/bin/sh\nif [ ! -e started ]; then : > started; sleep 30; fi\nexec ./fake-mcp\n",
    );
    let model = Model::serve(vec![vec![text("Hi.")]]);
    let mut engine = Engine::start(&dir, &model);
    engine.open_new();
    let connectors = vec![connector("slow", stdio(&slow))];
    assert_eq!(engine.request(Op::Connectors { connectors }), None);

    engine.prompt_as("n1", "hello");
    eventually("the server to start", || dir.join("started").exists());
    assert_eq!(engine.request(Op::Abort), None);
    assert_eq!(engine.until(is_turn_end), [aborted()]);

    engine.exchange("n2", "hello again");
    assert!(tool_names(&model, 0).contains(&"slow__echo".to_owned()));
}

#[test]
fn abort_during_a_compaction_ends_it_with_an_error() {
    let dir = workdir("compact-abort");
    let mut script = replies(&["A1", "A2", "A3"]);
    script.push(vec![Value::Null]);
    let model = Model::serve(script);
    let mut engine = Engine::start(&dir, &model);
    engine.open_new();
    for (nonce, text) in [("n1", "Q1"), ("n2", "Q2"), ("n3", "Q3")] {
        engine.exchange(nonce, text);
    }
    let compact = engine.send(Op::Compact);
    eventually("the request for a summary", || model.requests().len() == 4);
    assert_eq!(engine.request(Op::Abort), None);
    match engine.next() {
        Event::Response { id, error: Some(e) } if id == compact => assert_eq!(e, "aborted"),
        other => panic!("expected the refused compact, got {other:?}"),
    }
}

/// What `tests/recorded/<case>/case.json` holds; see the README there.
#[derive(serde::Deserialize)]
struct Recorded {
    files: std::collections::BTreeMap<String, String>,
    prompts: Vec<String>,
    answers: Vec<String>,
}

/// Replays one recorded conversation and returns the engine's events as JSON lines.
fn replay(case: &Path) -> String {
    let recorded: Recorded =
        serde_json::from_slice(&fs::read(case.join("case.json")).unwrap()).unwrap();
    let replies = (1..)
        .map_while(|n| fs::read(case.join(format!("reply-{n}.sse"))).ok())
        .collect();
    let name = case.file_name().unwrap().to_str().unwrap();
    let dir = workdir(&format!("recorded-{name}"));
    for (path, content) in &recorded.files {
        fs::write(dir.join(path), content).unwrap();
    }
    let model = Model::replay(replies);
    let mut engine = Engine::start(&dir, &model);
    engine.open_new();
    let mut answers = recorded.answers.iter();
    let mut lines = String::new();
    for (n, prompt) in recorded.prompts.iter().enumerate() {
        engine.prompt_as(&format!("n{}", n + 1), prompt);
        loop {
            let event = engine.next();
            lines.push_str(&serde_json::to_string(&event).unwrap());
            lines.push('\n');
            match event {
                Event::Question { question, .. } => {
                    let choice = answers.next().expect("an answer for each question");
                    let answer = Op::Answer {
                        question,
                        choice: choice.clone(),
                    };
                    assert_eq!(engine.request(answer), None);
                }
                Event::TurnEnd { .. } => break,
                _ => {}
            }
        }
    }
    assert!(
        answers.next().is_none(),
        "{name}: an answer was never asked for"
    );
    lines
}

#[test]
fn recorded_conversations_replay_to_their_expected_transcripts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/recorded");
    let mut cases: Vec<PathBuf> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    cases.sort();
    assert!(!cases.is_empty());
    let mut different = Vec::new();
    for case in &cases {
        let got = replay(case);
        let expected = case.join("expected.jsonl");
        if std::env::var_os("BATE_RECORD").is_some() {
            fs::write(&expected, &got).unwrap();
        } else if fs::read_to_string(&expected).ok().as_deref() != Some(got.as_str()) {
            different.push(format!("{}:\n{got}", case.display()));
        }
    }
    assert!(
        different.is_empty(),
        "the engine's events differ from expected.jsonl (BATE_RECORD=1 rewrites it):\n{}",
        different.join("\n")
    );
}

#[test]
fn a_command_the_model_runs_does_not_get_the_model_servers_key() {
    let dir = workdir("key");
    let model = Model::serve(vec![
        vec![call(
            "c1",
            "run",
            json!({"command": "echo key=$BATE_MODEL_API_KEY other=$OTHER"}),
        )],
        vec![text("Done.")],
    ]);
    let env = [("BATE_MODEL_API_KEY", "sk-secret"), ("OTHER", "kept")];
    let mut engine = Engine::start_with(&dir, &model, &env);
    engine.open_new();
    engine.prompt("print it");
    answer(&mut engine, "Approve");
    let events = engine.until(is_turn_end);
    assert!(
        matches!(&events[0], Event::ToolEnd { output, .. } if output == "key= other=kept\n"),
        "{events:?}"
    );
}

#[test]
fn notes_in_the_memory_folder_reach_the_model_in_every_conversation() {
    let dir = workdir("memory");
    fs::create_dir(dir.join("memory")).unwrap();
    fs::write(dir.join("memory/user.md"), "The user is called Ada.\n").unwrap();
    let model = Model::serve(vec![
        vec![text("Hello Ada.")],
        vec![text("Tea it is.")],
        vec![text("Still Ada.")],
    ]);
    let system = |n: usize| model.requests()[n]["messages"][0]["content"].to_string();
    let mut engine = Engine::start(&dir, &model);
    engine.open_new();
    engine.exchange("n1", "hi");
    assert!(
        system(0).contains("The user is called Ada."),
        "{}",
        system(0)
    );

    // The user edits a note between turns: the next turn has it.
    fs::write(dir.join("memory/likes.md"), "Prefers tea.\n").unwrap();
    engine.exchange("n2", "what do I drink?");
    assert!(system(1).contains("Prefers tea."), "{}", system(1));

    // A new conversation starts with the same notes.
    assert_eq!(engine.request(Op::Start { bot: "b1".into() }), None);
    engine.until(|e| matches!(e, Event::Session { .. }));
    engine.exchange("n3", "who am I?");
    assert_eq!(model.requests()[2]["messages"].as_array().unwrap().len(), 2);
    assert!(
        system(2).contains("The user is called Ada."),
        "{}",
        system(2)
    );
}

#[test]
fn the_model_is_told_which_skills_it_has_and_where_to_read_them() {
    let dir = workdir("skills");
    fs::create_dir_all(dir.join("skills/pdf-forms")).unwrap();
    fs::write(
        dir.join("skills/pdf-forms/SKILL.md"),
        "---\nname: pdf-forms\ndescription: Fill in PDF forms.\n---\nStep one: never guess a field.\n",
    )
    .unwrap();
    let model = Model::serve(vec![
        vec![call(
            "c1",
            "read",
            json!({"path": "skills/pdf-forms/SKILL.md"}),
        )],
        vec![text("Filled.")],
    ]);
    let mut engine = Engine::start(&dir, &model);
    engine.open_new();
    engine.prompt("fill in form.pdf");
    answer(&mut engine, "Approve");
    engine.until(is_turn_end);

    let requests = model.requests();
    let system = requests[0]["messages"][0]["content"].as_str().unwrap();
    assert!(
        system.contains("- pdf-forms (skills/pdf-forms/SKILL.md): Fill in PDF forms."),
        "{system}"
    );
    // The instructions are not sent until the model reads them: then they are.
    assert!(!system.contains("never guess a field"));
    assert!(
        requests[1]["messages"]
            .to_string()
            .contains("never guess a field")
    );
}

/// The names of the tools a request offered the model.
fn offered(request: &Value) -> Vec<&str> {
    request["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str())
        .collect()
}

#[test]
fn a_sub_agent_does_a_task_in_a_conversation_of_its_own_and_its_answer_comes_back() {
    let dir = workdir("sub-agent");
    fs::write(dir.join("a.txt"), "three\n").unwrap();
    let model = Model::serve(vec![
        vec![
            text("I will hand that out."),
            call(
                "c1",
                "agent",
                json!({"task": "Read a.txt and say what it holds."}),
            ),
        ],
        vec![
            text("Reading."),
            call("s1", "read", json!({"path": "a.txt"})),
        ],
        vec![text("It holds: three")],
        vec![text("The file says three.")],
    ]);
    let mut engine = Engine::start(&dir, &model);
    engine.open_new();
    engine.prompt("what is in a.txt? my code word is tulip");
    let mut events = answer(&mut engine, "Approve");
    events.extend(engine.until(is_turn_end));

    let requests = model.requests();
    // The sub-agent starts from its task alone: nothing of the conversation.
    let sub = requests[1]["messages"].as_array().unwrap();
    assert_eq!(sub.len(), 2, "{sub:?}");
    assert!(
        sub[0]["content"]
            .as_str()
            .unwrap()
            .contains("You are a sub-agent")
    );
    assert_eq!(
        sub[1],
        json!({"role": "user", "content": "Read a.txt and say what it holds."})
    );
    assert!(!requests[2].to_string().contains("tulip"));
    assert!(offered(&requests[0]).contains(&"agent") && offered(&requests[1]).contains(&"agent"));
    // Its last message is the tool's result, and all the conversation keeps of it.
    let last = requests[3]["messages"].as_array().unwrap();
    assert_eq!(last.last().unwrap()["content"], "It holds: three");
    assert!(!requests[3].to_string().contains("Reading."));

    // The window sees the sub-agent's tool calls, not its words.
    let said: String = events
        .iter()
        .filter_map(|e| match e {
            Event::Text { delta } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(said, "I will hand that out.The file says three.");
    let ended: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::ToolEnd {
                call,
                output,
                is_error,
            } => Some((call.as_str(), output.as_str(), *is_error)),
            _ => None,
        })
        .collect();
    assert_eq!(ended.len(), 2, "{ended:?}");
    assert!(
        ended[0].0.ends_with("/s1") && ended[0].1.contains("three"),
        "{ended:?}"
    );
    assert_eq!(ended[1], ("c1", "It holds: three", false));
}

#[test]
fn sub_agents_nest_five_deep_and_no_deeper() {
    let dir = workdir("sub-agent-depth");
    let deeper = |id: &str| vec![call(id, "agent", json!({"task": "go deeper"}))];
    let mut replies: Vec<Vec<Value>> = (0..6).map(|n| deeper(&format!("c{n}"))).collect();
    // Level 5 was refused; then every level answers, innermost first.
    replies.extend(
        (0..6)
            .rev()
            .map(|n| vec![text(&format!("answer from level {n}"))]),
    );
    let model = Model::serve(replies);
    let mut engine = Engine::start(&dir, &model);
    engine.open_new();
    engine.prompt("go");
    let events = engine.until(is_turn_end);

    let requests = model.requests();
    assert_eq!(requests.len(), 12);
    for (level, request) in requests[..6].iter().enumerate() {
        assert_eq!(
            offered(request).contains(&"agent"),
            level < 5,
            "level {level}"
        );
    }
    // The model at level 5 asked anyway: told no, and it goes on.
    let refused = requests[6]["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert!(
        refused["content"]
            .as_str()
            .unwrap()
            .contains("at most 5 deep"),
        "{refused}"
    );
    let main = requests[11]["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(main["content"], "answer from level 1");
    assert!(matches!(
        events.last(),
        Some(Event::TurnEnd {
            reason: TurnEnd::Done,
            ..
        })
    ));
}

#[test]
fn a_bot_messages_another_bot_after_asking_and_gets_its_reply_or_why_there_is_none() {
    let dir = workdir("message");
    let tell = |id: &str| {
        vec![call(
            id,
            "message_bot",
            json!({"bot": "scout", "text": "How many tests fail?"}),
        )]
    };
    let model = Model::serve(vec![
        tell("c1"),
        tell("c2"),
        vec![call(
            "c3",
            "message_bot",
            json!({"bot": "Nobody", "text": "hi"}),
        )],
        vec![text("Two fail.")],
    ]);
    let mut engine = Engine::start(&dir, &model);
    engine.open_new();
    let peers = vec![
        Peer {
            id: "b2".into(),
            name: "Scout".into(),
            lead: false,
        },
        Peer {
            id: "b3".into(),
            name: "Boss".into(),
            lead: true,
        },
    ];
    assert_eq!(
        engine.request(Op::Peers {
            peers,
            leads: false
        }),
        None
    );
    engine.prompt("ask around");

    // Reaching another bot is asked about, for that bot.
    let events = engine.until(is_question);
    let Some(Event::Question {
        question,
        kind,
        target,
        title,
        ..
    }) = events.last()
    else {
        unreachable!()
    };
    assert_eq!(
        (kind, target.as_deref()),
        (&Some(Kind::Connector), Some("bot:b2"))
    );
    assert_eq!(title, "Message Scout: How many tests fail?");
    let approve = Op::Answer {
        question: question.clone(),
        choice: "Approve".into(),
    };
    assert_eq!(engine.request(approve), None);
    let sent = engine.until(|e| matches!(e, Event::Message { .. }));
    let Some(Event::Message { message, to, text }) = sent.last() else {
        unreachable!()
    };
    assert_eq!((to.as_str(), text.as_str()), ("b2", "How many tests fail?"));
    // Only the open message takes a reply.
    let reply = |message: &str| Op::Reply {
        message: message.into(),
        text: Some("Two.".into()),
        error: None,
    };
    assert!(engine.request(reply("m999")).is_some());
    assert_eq!(engine.request(reply(message)), None);

    // The second message gets no reply: the model is told why.
    answer(&mut engine, "Approve");
    let sent = engine.until(|e| matches!(e, Event::Message { .. }));
    let Some(Event::Message { message, .. }) = sent.last() else {
        unreachable!()
    };
    let busy = Op::Reply {
        message: message.clone(),
        text: None,
        error: Some("it is busy".into()),
    };
    assert_eq!(engine.request(busy), None);
    engine.until(is_turn_end);

    let requests = model.requests();
    let system = requests[0]["messages"][0]["content"].as_str().unwrap();
    assert!(
        system.contains("- Scout\n- Boss (leads your group)"),
        "{system}"
    );
    assert!(!system.contains("You lead a group"));
    assert!(offered(&requests[0]).contains(&"message_bot"));
    let result =
        |n: usize| requests[n]["messages"].as_array().unwrap().last().unwrap()["content"].clone();
    assert_eq!(result(1), "Two.");
    assert_eq!(result(2), "No reply from Scout: it is busy");
    // A bot it was not given is not asked about, nor messaged.
    assert_eq!(result(3), "No bot `Nobody`. You can message: Scout, Boss.");
}

#[test]
fn a_bot_alone_has_no_message_tool_and_a_lead_is_told_what_it_may_do() {
    let dir = workdir("message-none");
    let model = Model::serve(vec![vec![text("ok")], vec![text("ok")]]);
    let mut engine = Engine::start(&dir, &model);
    engine.open_new();
    engine.prompt("hi");
    engine.until(is_turn_end);
    let peers = vec![Peer {
        id: "b2".into(),
        name: "Scout".into(),
        lead: false,
    }];
    assert_eq!(engine.request(Op::Peers { peers, leads: true }), None);
    engine.prompt_as("n2", "hi again");
    engine.until(is_turn_end);

    let requests = model.requests();
    assert!(!offered(&requests[0]).contains(&"message_bot"));
    assert!(!requests[0].to_string().contains("other bots"));
    let system = requests[1]["messages"][0]["content"].as_str().unwrap();
    assert!(
        system.contains("You lead a group") && system.contains("cannot stop or redirect"),
        "{system}"
    );
}

#[test]
fn the_users_instructions_come_first_in_the_system_prompt() {
    let dir = workdir("instructions");
    let model = Model::serve(vec![vec![text("ok")], vec![text("ok")]]);
    let mut engine = Engine::start(&dir, &model);
    engine.open_new();
    engine.prompt("hi");
    engine.until(is_turn_end);
    let text = "Answer in French.".to_owned();
    assert_eq!(engine.request(Op::Instructions { text }), None);
    engine.prompt_as("n2", "hi again");
    engine.until(is_turn_end);

    let requests = model.requests();
    let system = |i: usize| {
        requests[i]["messages"][0]["content"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert!(
        !system(0).contains("The user's instructions"),
        "{}",
        system(0)
    );
    assert!(
        system(1).contains("The user's instructions for you:\nAnswer in French."),
        "{}",
        system(1)
    );
}

#[test]
fn an_empty_model_goes_back_to_the_default() {
    let dir = workdir("model-default");
    let model = Model::serve(vec![vec![text("ok")]]);
    let mut engine = Engine::start_with(&dir, &model, &[("BATE_MODEL", "base")]);
    engine.open_new();
    let set = |model: &str| Op::SetModel {
        model: model.into(),
        effort: None,
    };
    assert_eq!(engine.request(set("m1")), None);
    assert_eq!(engine.request(set("")), None);
    engine.prompt("hi");
    engine.until(is_turn_end);
    assert_eq!(model.requests()[0]["model"], "base");
}

#[test]
fn the_browser_tool_asks_once_for_the_browser_and_waits_for_what_the_service_did() {
    let dir = workdir("browser");
    let model = Model::serve(vec![
        vec![call(
            "c1",
            "browser",
            json!({"action": "open", "url": "https://example.com/"}),
        )],
        vec![call(
            "c2",
            "browser",
            json!({"action": "click", "element": 3}),
        )],
        vec![call("c3", "browser", json!({"action": "fly"}))],
        vec![text("Done.")],
    ]);
    let mut engine = Engine::start(&dir, &model);
    engine.open_new();
    engine.prompt("look it up");

    let events = answer(&mut engine, "Approve");
    let Some(Event::Question {
        kind,
        target,
        title,
        ..
    }) = events.last()
    else {
        unreachable!()
    };
    assert_eq!(
        (kind, target.as_deref()),
        (&Some(Kind::Connector), Some("browser"))
    );
    assert_eq!(title, "Browser: open https://example.com/");
    let asked = engine.until(|e| matches!(e, Event::Browser { .. }));
    let Some(Event::Browser { request, action }) = asked.last() else {
        unreachable!()
    };
    assert_eq!(action["url"], "https://example.com/");
    let result = |request: &str, output: Option<&str>, error: Option<&str>| Op::BrowserResult {
        request: request.into(),
        output: output.map(str::to_owned),
        error: error.map(str::to_owned),
    };
    // Only the open request takes a result, and a reply to a message is not one.
    assert!(engine.request(result("b999", Some("x"), None)).is_some());
    let reply = Op::Reply {
        message: request.clone(),
        text: Some("x".into()),
        error: None,
    };
    assert!(engine.request(reply).is_some());
    assert_eq!(
        engine.request(result(request, Some("Title: Example"), None)),
        None
    );

    answer(&mut engine, "Approve");
    let asked = engine.until(|e| matches!(e, Event::Browser { .. }));
    let Some(Event::Browser { request, .. }) = asked.last() else {
        unreachable!()
    };
    let gone = "There is no element 3 on the page now: look again.";
    assert_eq!(engine.request(result(request, None, Some(gone))), None);
    engine.until(is_turn_end);

    let requests = model.requests();
    assert!(offered(&requests[0]).contains(&"browser"));
    let result =
        |n: usize| requests[n]["messages"].as_array().unwrap().last().unwrap()["content"].clone();
    assert_eq!(result(1), "Title: Example");
    assert_eq!(result(2), gone);
    // Not an action: not asked about, nor sent.
    assert_eq!(
        result(3),
        "`fly` is not an action: open, look, click, type or back."
    );
}

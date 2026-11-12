//! `agentcore` over stdin/stdout against a mock OpenAI-compatible model server.

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
        let mut child = Command::new(env!("CARGO_BIN_EXE_agentcore"))
            .current_dir(dir)
            .env("BATE_MODEL_BASE_URL", &model.url)
            .env_remove("BATE_MODEL_API_KEY")
            .env_remove("BATE_MODEL")
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
        let op = Op::Prompt {
            nonce: "n1".into(),
            text: text.into(),
        };
        assert_eq!(self.request(op), None);
        assert_eq!(self.next(), Event::TurnStart { nonce: "n1".into() });
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn workdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("agentcore-engine-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
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

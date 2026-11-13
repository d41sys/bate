//! `agentcore acp`: the engine protocol on one side, an ACP agent on the other. The agent
//! here is a shell script that speaks enough ACP for these tests.

use proto::permissions::Kind;
use proto::{Engine, Event, Op, TurnEnd};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Requests and their ids arrive as `{"id":N,"jsonrpc":"2.0","method":…}`. What the agent is
/// sent goes to `agent.log` in its folder. Prompts: `use a tool` asks for permission (request
/// id 900) and goes on when answered; `hang` works until cancelled; `stop early` runs out of
/// tokens; `die` exits; anything else is answered with a thought and two pieces of text.
/// With a file `signed-out` in the folder, no conversation is opened.
const AGENT: &str = r#"#!/bin/sh
say() { printf '%s\n' "$1"; }
result() { say "{\"jsonrpc\":\"2.0\",\"id\":$1,\"result\":$2}"; }
update() { say "{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"params\":{\"sessionId\":\"s1\",\"update\":$1}}"; }
chunk() { update "{\"sessionUpdate\":\"$1\",\"content\":{\"type\":\"text\",\"text\":\"$2\"}}"; }
echo "not json: a banner"
while IFS= read -r line; do
  printf '%s\n' "$line" >> agent.log
  id=$(printf '%s' "$line" | sed -n 's/^{"id":\([0-9]*\),.*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      result "$id" '{"protocolVersion":1,"agentCapabilities":{"loadSession":true},"authMethods":[{"id":"login","name":"Login","description":"run `agent login` in a terminal"}]}' ;;
    *'"method":"session/new"'*)
      if [ -e signed-out ]; then
        say "{\"jsonrpc\":\"2.0\",\"id\":$id,\"error\":{\"code\":-32000,\"message\":\"Authentication required\"}}"
      else
        result "$id" '{"sessionId":"s1"}'
      fi ;;
    *'"method":"session/load"'*)
      chunk agent_message_chunk "words from before"
      result "$id" null ;;
    *'"method":"session/prompt"'*)
      prompt=$id
      case "$line" in
        *'use a tool'*)
          update '{"sessionUpdate":"tool_call","toolCallId":"t1","title":"List files","kind":"execute","status":"pending","rawInput":{"command":"ls -la"}}'
          say '{"jsonrpc":"2.0","id":900,"method":"session/request_permission","params":{"sessionId":"s1","toolCall":{"toolCallId":"t1","title":"List files","kind":"execute","rawInput":{"command":"ls -la"}},"options":[{"optionId":"ever","name":"Always allow","kind":"allow_always"},{"optionId":"yes","name":"Allow","kind":"allow_once"},{"optionId":"no","name":"Reject","kind":"reject_once"}]}}' ;;
        *'hang'*) chunk agent_message_chunk "working" ;;
        *'stop early'*) result "$id" '{"stopReason":"max_tokens"}' ;;
        *'die'*) chunk agent_message_chunk "oh"; exit 3 ;;
        *)
          say '{"jsonrpc":"2.0","id":901,"method":"fs/read_text_file","params":{"sessionId":"s1","path":"/etc/hosts"}}'
          update '{"sessionUpdate":"available_commands_update","availableCommands":[]}'
          chunk agent_thought_chunk "Thinking."
          chunk agent_message_chunk "Hello"
          chunk agent_message_chunk " there"
          result "$id" '{"stopReason":"end_turn"}' ;;
      esac ;;
    *'"method":"session/cancel"'*) result "$prompt" '{"stopReason":"cancelled"}' ;;
    '{"id":900,'*)
      case "$line" in
        *'"optionId":"yes"'*)
          update '{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"in_progress"}'
          update '{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed","content":[{"type":"content","content":{"type":"text","text":"notes.txt"}}]}'
          chunk agent_message_chunk "One file." ;;
        *)
          update '{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"failed","rawOutput":"not allowed"}' ;;
      esac
      result "$prompt" '{"stopReason":"end_turn"}' ;;
  esac
done
"#;

struct Bridge {
    engine: Engine,
    dir: PathBuf,
    sent: u32,
}

impl Bridge {
    /// The engine in a fresh folder, with the agent of these tests; waits until it is ready.
    fn start(name: &str) -> Bridge {
        Bridge::start_in(name, |_| {})
    }

    fn start_in(name: &str, prepare: impl FnOnce(&Path)) -> Bridge {
        let dir = std::env::temp_dir().join(format!("bate-acp-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("agent.sh"), AGENT).unwrap();
        prepare(&dir);
        let mut command = Command::new(env!("CARGO_BIN_EXE_agentcore"));
        command
            .args(["acp", "/bin/sh", "agent.sh"])
            .current_dir(&dir);
        let mut bridge = Bridge {
            engine: Engine::run(command).unwrap(),
            dir,
            sent: 0,
        };
        assert_eq!(
            bridge.next(),
            Event::Ready {
                protocol: proto::PROTOCOL
            }
        );
        bridge
    }

    fn next(&mut self) -> Event {
        self.engine
            .output
            .recv()
            .unwrap()
            .expect("the engine stopped")
    }

    /// Sends `op`; the next event must be its response. Gives the error, if it was refused.
    fn request(&mut self, op: Op) -> Option<String> {
        self.sent += 1;
        let id = format!("r{}", self.sent);
        self.engine.input.send(&id, &op).unwrap();
        match self.next() {
            Event::Response { id: of, error } if of == id => error,
            other => panic!("expected the response to {id}, got {other:?}"),
        }
    }

    /// Starts a conversation and a turn with `text`.
    fn prompt(&mut self, text: &str) {
        assert_eq!(self.request(Op::Start { bot: "b".into() }), None);
        assert_eq!(
            self.next(),
            Event::Session {
                session: "s1".into()
            }
        );
        self.turn(text);
    }

    fn turn(&mut self, text: &str) {
        let (nonce, text) = ("n".to_owned(), text.to_owned());
        assert_eq!(self.request(Op::Prompt { nonce, text }), None);
        assert_eq!(self.next(), Event::TurnStart { nonce: "n".into() });
    }

    /// The events up to and with the end of the turn.
    fn rest(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        loop {
            events.push(self.next());
            if matches!(events.last(), Some(Event::TurnEnd { .. })) {
                return events;
            }
        }
    }

    fn log(&self) -> String {
        fs::read_to_string(self.dir.join("agent.log")).unwrap()
    }
}

fn text(delta: &str) -> Event {
    Event::Text {
        delta: delta.into(),
    }
}

fn end(reason: TurnEnd, error: Option<&str>) -> Event {
    Event::TurnEnd {
        reason,
        error: error.map(str::to_owned),
    }
}

#[test]
fn a_turn_is_the_agents_words_and_thoughts() {
    let mut bridge = Bridge::start("turn");
    bridge.prompt("hello");
    assert_eq!(
        bridge.rest(),
        [
            Event::Thinking {
                delta: "Thinking.".into()
            },
            text("Hello"),
            text(" there"),
            end(TurnEnd::Done, None)
        ]
    );
    let log = bridge.log();
    let dir = bridge.dir.canonicalize().unwrap();
    assert!(
        log.contains(&format!("\"cwd\":\"{}\"", dir.display())),
        "{log}"
    );
    assert!(
        log.contains(r#""prompt":[{"text":"hello","type":"text"}],"sessionId":"s1""#),
        "{log}"
    );
    // The agent asked to read a file through us: not offered, and told so. It reads that
    // answer in its own time.
    let told = r#"{"error":{"code":-32601,"message":"fs/read_text_file is not offered"},"id":901"#;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !bridge.log().contains(told) {
        assert!(std::time::Instant::now() < deadline, "{}", bridge.log());
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // A second turn in the same conversation.
    bridge.turn("again");
    assert_eq!(bridge.rest().last(), Some(&end(TurnEnd::Done, None)));
    assert_eq!(bridge.request(Op::Stop), None);
}

#[test]
fn the_agents_permission_request_is_a_question_the_service_decides() {
    for (choice, option) in [("Approve", "yes"), ("Deny", "no")] {
        let mut bridge = Bridge::start(&format!("ask-{option}"));
        bridge.prompt("use a tool");
        let Event::ToolStart { call, tool, args } = bridge.next() else {
            panic!("no tool start");
        };
        assert_eq!((call.as_str(), tool.as_str()), ("t1", "execute"));
        assert_eq!(args["title"], "List files");
        assert_eq!(args["rawInput"]["command"], "ls -la");
        // What it is and what it acts on, in Bate's terms, so that the bot's levels apply.
        let question = Event::Question {
            question: "q1".into(),
            title: "List files".into(),
            options: vec!["Approve".into(), "Deny".into()],
            kind: Some(Kind::Run),
            target: Some("ls -la".into()),
        };
        assert_eq!(bridge.next(), question);
        let wrong = Op::Answer {
            question: "q1".into(),
            choice: "Always allow".into(),
        };
        assert!(bridge.request(wrong).is_some());
        let answer = Op::Answer {
            question: "q1".into(),
            choice: choice.into(),
        };
        assert_eq!(bridge.request(answer.clone()), None);
        // This once: what is to be remembered, Bate remembers, not the agent.
        let events = bridge.rest();
        let log = bridge.log();
        assert!(
            log.contains(&format!(r#""optionId":"{option}","outcome":"selected""#)),
            "{log}"
        );
        let approved = choice == "Approve";
        let output = if approved { "notes.txt" } else { "not allowed" };
        let tool_end = Event::ToolEnd {
            call: "t1".into(),
            output: output.into(),
            is_error: !approved,
        };
        assert_eq!(events[0], tool_end, "{events:?}");
        assert_eq!(events.len(), if approved { 3 } else { 2 }, "{events:?}");
        assert!(bridge.request(answer).is_some(), "answered twice");
    }
}

#[test]
fn abort_cancels_the_agents_turn_and_what_it_asked() {
    let mut bridge = Bridge::start("abort");
    bridge.prompt("hang");
    assert_eq!(bridge.next(), text("working"));
    let busy = Op::Prompt {
        nonce: "n2".into(),
        text: "more".into(),
    };
    assert!(bridge.request(busy).is_some());
    assert_eq!(bridge.request(Op::Abort), None);
    assert_eq!(bridge.rest(), [end(TurnEnd::Aborted, None)]);
    assert!(
        bridge
            .log()
            .contains(r#""method":"session/cancel","params":{"sessionId":"s1"}"#)
    );

    // An open permission request is called off with the turn.
    bridge.turn("use a tool");
    bridge.next();
    assert!(matches!(bridge.next(), Event::Question { .. }));
    assert_eq!(bridge.request(Op::Abort), None);
    bridge.rest();
    assert!(
        bridge
            .log()
            .contains(r#"{"id":900,"jsonrpc":"2.0","result":{"outcome":{"outcome":"cancelled"}}}"#)
    );
    let late = Op::Answer {
        question: "q1".into(),
        choice: "Approve".into(),
    };
    assert!(bridge.request(late).is_some());
}

#[test]
fn an_earlier_conversation_is_reopened_without_its_words_coming_again() {
    let mut bridge = Bridge::start("reopen");
    let open = Op::Open {
        bot: "b".into(),
        session: "s7".into(),
    };
    assert_eq!(bridge.request(open), None);
    assert_eq!(
        bridge.next(),
        Event::Session {
            session: "s7".into()
        }
    );
    bridge.turn("hello");
    // The first event of the turn is the turn's own: nothing of what the agent replayed.
    assert_eq!(
        bridge.next(),
        Event::Thinking {
            delta: "Thinking.".into()
        }
    );
    let log = bridge.log();
    assert!(
        log.contains(r#""method":"session/load""#) && log.contains(r#""sessionId":"s7"}"#),
        "{log}"
    );
}

#[test]
fn what_the_agent_cannot_do_is_refused_or_ends_the_turn_with_the_reason() {
    let mut bridge = Bridge::start("limits");
    bridge.prompt("stop early");
    assert_eq!(
        bridge.rest(),
        [end(TurnEnd::Error, Some("the agent stopped: max tokens"))]
    );
    assert!(bridge.request(Op::Steer { text: "x".into() }).is_some());
    assert!(bridge.request(Op::Compact).is_some());
    assert!(
        bridge
            .request(Op::Rewind {
                entry: "e1".into(),
                fork: false
            })
            .is_some()
    );
    // The agent's own model stays: the service's choice is taken and not used.
    assert_eq!(
        bridge.request(Op::SetModel {
            model: "m".into(),
            effort: None
        }),
        None
    );

    // The agent going away mid-turn ends the turn, and the engine with it.
    bridge.turn("die");
    assert_eq!(
        bridge.rest(),
        [text("oh"), end(TurnEnd::Error, Some("the agent stopped"))]
    );
    assert_eq!(bridge.engine.output.recv().unwrap(), None);
}

#[test]
fn an_agent_that_is_not_signed_in_says_how_to() {
    let mut bridge = Bridge::start_in("signed-out", |dir| {
        fs::write(dir.join("signed-out"), "").unwrap()
    });
    let error = bridge.request(Op::Start { bot: "b".into() }).unwrap();
    assert!(
        error.contains("Authentication required") && error.contains("agent login"),
        "{error}"
    );
    let (nonce, text) = ("n".to_owned(), "hi".to_owned());
    assert!(bridge.request(Op::Prompt { nonce, text }).is_some());
}

#[test]
fn the_bots_connectors_are_given_to_the_agent() {
    use proto::connectors::{Connector, Transport};
    let mut bridge = Bridge::start("servers");
    let stdio = Transport::Stdio {
        command: "/bin/notes".into(),
        args: vec!["--all".into()],
        env: [("KEY".to_owned(), "v".to_owned())].into(),
    };
    let http = Transport::Http {
        url: "https://mcp.example.com/".into(),
        headers: Default::default(),
    };
    let make = |name: &str, enabled, transport| Connector {
        name: name.into(),
        enabled,
        transport,
        oauth: false,
        client_id: None,
        signed_in: None,
    };
    let connectors = vec![
        make("notes", true, stdio.clone()),
        make("off", false, stdio),
        // This agent takes no servers over http.
        make("web", true, http),
    ];
    assert_eq!(bridge.request(Op::Connectors { connectors }), None);
    bridge.prompt("hello");
    let log = bridge.log();
    assert!(
        log.contains(r#""mcpServers":[{"args":["--all"],"command":"/bin/notes","env":[{"name":"KEY","value":"v"}],"name":"notes"}]"#),
        "{log}"
    );
}

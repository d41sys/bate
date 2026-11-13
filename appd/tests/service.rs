//! Starts the real `appd` with a private home folder and a fake engine, and
//! talks to it over its socket the way a window does.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// Answers every request. A prompt `hang` is never answered; a prompt `ask`
/// asks question `q1` (the answer `Maybe` is refused); any other prompt streams "hel", waits a second, then "lo".
/// `abort` ends the turn `aborted`. Each `connectors` operation is logged to `connectors.log`. `compact` takes 0.3 s. A prompt `do <kind> <target>` asks whether that action may run, and
/// the turn then says which choice the engine was given.
const FAKE_ENGINE: &str = r#"#!/bin/sh
echo '{"type":"ready","protocol":1}'
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed 's/^{"id":"\([^"]*\)".*/\1/')
  ok="{\"type\":\"response\",\"id\":\"$id\"}"
  case "$line" in
    *'"type":"start"'*) printf 'bad \377 byte\n' >&2; echo "engine says hi" >&2; echo "$ok"; echo '{"type":"session","session":"s1"}' ;;
    *'"type":"prompt"'*)
      printf '%s\n' "$line" >> prompts.log
      nonce=$(printf '%s' "$line" | sed 's/.*"nonce":"\([^"]*\)".*/\1/')
      case "$line" in
        *'"text":"hang"'*) ;;
        *'"text":"do '*)
          kind=$(printf '%s' "$line" | sed 's/.*"text":"do \([a-z]*\) .*/\1/')
          target=$(printf '%s' "$line" | sed 's/.*"text":"do [a-z]* \([^"]*\)".*/\1/')
          echo "$ok"; echo "{\"type\":\"turn_start\",\"nonce\":\"$nonce\"}"
          echo "{\"type\":\"question\",\"question\":\"q-$nonce\",\"title\":\"$kind $target\",\"options\":[\"Approve\",\"Deny\"],\"kind\":\"$kind\",\"target\":\"$target\"}" ;;
        *'"text":"ask"'*)
          echo "$ok"; echo "{\"type\":\"turn_start\",\"nonce\":\"$nonce\"}"
          echo '{"type":"question","question":"q1","title":"Write file?","options":["Approve","Deny"]}' ;;
        *)
          echo "$ok"; echo "{\"type\":\"turn_start\",\"nonce\":\"$nonce\"}"
          echo '{"type":"text","delta":"hel"}'; sleep 1
          echo '{"type":"text","delta":"lo"}'; echo '{"type":"turn_end","reason":"done"}' ;;
      esac ;;
    *'"choice":"Maybe"'*) echo "{\"type\":\"response\",\"id\":\"$id\",\"error\":\"not an option\"}" ;;
    *'"type":"answer"'*)
      choice=$(printf '%s' "$line" | sed 's/.*"choice":"\([^"]*\)".*/\1/')
      echo "$ok"; echo "{\"type\":\"text\",\"delta\":\"chose $choice\"}"; echo '{"type":"turn_end","reason":"done"}' ;;
    *'"type":"abort"'*) echo "$ok"; echo '{"type":"turn_end","reason":"aborted"}' ;;
    *'"type":"compact"'*) sleep 0.3; echo "$ok" ;;
    *'"type":"connectors"'*) printf '%s\n' "$line" >> connectors.log; echo "$ok" ;;
    *) echo "$ok" ;;
  esac
done
"#;

const TIMEOUT: Duration = Duration::from_secs(10);

/// A private home folder, removed when the test passes; kept after a failure, for a look at what is in it.
struct Home(PathBuf);

impl std::ops::Deref for Home {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

fn home(name: &str) -> Home {
    let home = std::env::temp_dir().join(format!("appd-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).unwrap();
    let engine = home.join("fake-engine");
    fs::write(&engine, FAKE_ENGINE).unwrap();
    fs::set_permissions(&engine, fs::Permissions::from_mode(0o755)).unwrap();
    Home(home)
}

struct Service(Child);

impl Service {
    fn start(home: &Path) -> Service {
        Service::start_with(home, |c| {
            c.env("APPD_ENGINE", home.join("fake-engine"));
        })
    }

    /// `engine` sets the environment that picks and configures the engine.
    fn start_with(home: &Path, engine: impl FnOnce(&mut Command)) -> Service {
        let mut appd = Command::new(env!("CARGO_BIN_EXE_appd"));
        engine(&mut appd);
        let child = appd
            .env("APPD_HOME", home)
            .stdin(Stdio::null())
            .stderr(
                fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(home.join("appd.log"))
                    .unwrap(),
            )
            .spawn()
            .unwrap();
        let deadline = Instant::now() + TIMEOUT;
        while UnixStream::connect(home.join("run/appd.sock")).is_err() {
            assert!(Instant::now() < deadline, "appd did not open its socket");
            std::thread::sleep(Duration::from_millis(20));
        }
        Service(child)
    }

    /// SIGKILL: no chance to clean up, like a crash or power loss.
    fn kill(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.kill();
    }
}

struct Window {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next: u64,
}

impl Window {
    fn connect(home: &Path) -> Window {
        let stream = UnixStream::connect(home.join("run/appd.sock")).unwrap();
        stream.set_read_timeout(Some(TIMEOUT)).unwrap();
        let mut w = Window {
            reader: BufReader::new(stream.try_clone().unwrap()),
            writer: stream,
            next: 0,
        };
        assert_eq!(w.frame()["type"], "hello");
        w.ok("hello", json!({"protocol": 1}));
        w
    }

    fn frame(&mut self) -> Value {
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .expect("no frame from appd in time");
        serde_json::from_str(&line).unwrap()
    }

    fn send(&mut self, command: &str, mut fields: Value) -> String {
        self.next += 1;
        let id = self.next.to_string();
        fields["id"] = json!(id);
        fields["type"] = json!(command);
        writeln!(self.writer, "{fields}").unwrap();
        id
    }

    /// The `data` of a successful reply; other frames that arrive first are skipped.
    fn ok(&mut self, command: &str, fields: Value) -> Value {
        let id = self.send(command, fields);
        let reply = self.wait_for(|f| f["type"] == "response" && f["id"] == id.as_str());
        assert_eq!(reply["success"], true, "{command} failed: {reply}");
        reply["data"].clone()
    }

    fn wait_for(&mut self, pred: impl Fn(&Value) -> bool) -> Value {
        loop {
            let f = self.frame();
            if pred(&f) {
                return f;
            }
        }
    }

    fn create_bot(&mut self) -> String {
        self.ok("create_bot", json!({"name": "Ada"}))["bot"]["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

fn prompts_received(home: &Path, bot: &str, nonce: &str) -> usize {
    let log =
        fs::read_to_string(home.join("bots").join(bot).join("prompts.log")).unwrap_or_default();
    log.lines()
        .filter(|l| l.contains(&format!("\"nonce\":\"{nonce}\"")))
        .count()
}

fn eventually(what: &str, check: impl Fn() -> bool) {
    let deadline = Instant::now() + TIMEOUT;
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_reply_finishes_after_the_window_quits_and_history_has_all_of_it() {
    let home = home("socket");
    let _service = Service::start(&home);
    let mut w = Window::connect(&home);
    let bot = w.create_bot();
    let bots = w.ok("list_bots", json!({}));
    assert_eq!(bots["bots"][0]["id"], bot.as_str());
    assert_eq!(bots["bots"][0]["name"], "Ada");

    assert_eq!(
        w.ok("prompt", json!({"bot": bot, "nonce": "n1", "text": "hi"})),
        json!({})
    );
    w.wait_for(|f| f["type"] == "transcript" && f["entry"]["delta"] == "hel");
    drop(w); // The window quits mid-reply: the engine still has a second to go.

    let mut w = Window::connect(&home);
    let deadline = Instant::now() + TIMEOUT;
    let history = loop {
        let history = w.ok("open_bot", json!({"bot": bot}))["history"]
            .as_array()
            .unwrap()
            .clone();
        if history.iter().any(|e| e["entry"]["type"] == "turn_end") {
            break history;
        }
        assert!(Instant::now() < deadline, "the turn did not end");
        std::thread::sleep(Duration::from_millis(50));
    };
    let kinds: Vec<&str> = history
        .iter()
        .map(|e| e["entry"]["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        [
            "session",
            "prompt",
            "turn_start",
            "text",
            "text",
            "turn_end"
        ]
    );
    let text: String = history
        .iter()
        .filter_map(|e| e["entry"]["delta"].as_str())
        .collect();
    assert_eq!(text, "hello");
    assert_eq!(w.ok("open_bot", json!({"bot": bot}))["bot"]["busy"], false);
}

#[test]
fn a_nonce_reaches_the_engine_once_even_across_a_crash() {
    let home = home("ledger");
    let mut service = Service::start(&home);
    let mut w = Window::connect(&home);
    let bot = w.create_bot();

    assert_eq!(
        w.ok("prompt", json!({"bot": bot, "nonce": "n1", "text": "hi"})),
        json!({})
    );
    assert_eq!(
        w.ok("prompt", json!({"bot": bot, "nonce": "n1", "text": "hi"})),
        json!({"duplicate": true})
    );
    w.wait_for(|f| f["type"] == "transcript" && f["entry"]["type"] == "turn_end");
    assert_eq!(prompts_received(&home, &bot, "n1"), 1);

    // The engine takes n2 but never answers; the service dies before it learns the outcome.
    w.send("prompt", json!({"bot": bot, "nonce": "n2", "text": "hang"}));
    eventually("the engine to receive n2", || {
        prompts_received(&home, &bot, "n2") == 1
    });
    service.kill();
    let _service = Service::start(&home);
    let mut w = Window::connect(&home);
    assert_eq!(
        w.ok("prompt", json!({"bot": bot, "nonce": "n2", "text": "hang"})),
        json!({"duplicate": true, "uncertain": true})
    );
    assert_eq!(prompts_received(&home, &bot, "n2"), 1);
}

#[test]
fn an_open_question_survives_a_reconnect_and_the_answer_reaches_the_engine() {
    let home = home("questions");
    let _service = Service::start(&home);
    let mut w = Window::connect(&home);
    let bot = w.create_bot();
    w.ok("prompt", json!({"bot": bot, "nonce": "n1", "text": "ask"}));
    w.wait_for(|f| f["type"] == "transcript" && f["entry"]["type"] == "question");
    drop(w);

    let mut w = Window::connect(&home);
    let questions = w.ok("open_bot", json!({"bot": bot}))["questions"].clone();
    assert_eq!(questions[0]["question"], "q1");
    assert_eq!(questions[0]["options"], json!(["Approve", "Deny"]));

    let refused = w.send(
        "answer",
        json!({"bot": bot, "question": "q1", "choice": "Maybe"}),
    );
    let reply = w.wait_for(|f| f["type"] == "response" && f["id"] == refused.as_str());
    assert_eq!(reply["code"], "engine_error");
    assert_eq!(
        w.ok("open_bot", json!({"bot": bot}))["questions"][0]["question"],
        "q1"
    );

    w.ok(
        "answer",
        json!({"bot": bot, "question": "q1", "choice": "Approve"}),
    );
    w.wait_for(|f| f["type"] == "transcript" && f["entry"]["delta"] == "chose Approve");
    assert_eq!(
        w.ok("open_bot", json!({"bot": bot}))["questions"],
        json!([])
    );
    let again = w.send(
        "answer",
        json!({"bot": bot, "question": "q1", "choice": "Approve"}),
    );
    let reply = w.wait_for(|f| f["type"] == "response" && f["id"] == again.as_str());
    assert_eq!(reply["code"], "no_such_question");
}

#[test]
fn a_due_routine_is_sent_once_across_restarts_and_connectors_round_trip() {
    let home = home("routines");
    let mut service = Service::start(&home);
    let mut w = Window::connect(&home);
    let bot = w.create_bot();
    let routine = w.ok(
        "set_routine",
        json!({"bot": bot, "routine": {"name": "Brief", "prompt": "hi", "enabled": true,
            "schedule": {"frequency": "daily", "time": "09:00", "timezone": "UTC"}}}),
    )["routine"]
        .clone();
    let id = routine["id"].as_str().unwrap().to_owned();
    assert!(routine["nextRunMs"].as_u64().unwrap() > 0);

    // While the service is down, the routine's slot passes (one minute ago).
    service.kill();
    let due = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
        - 60_000;
    let db = rusqlite::Connection::open(home.join("app.db")).unwrap();
    db.execute(
        "UPDATE routines SET routine = json_set(routine, '$.nextRunMs', ?1) WHERE id = ?2",
        rusqlite::params![due, id],
    )
    .unwrap();
    drop(db);

    let nonce = format!("routine-{id}-{due}");
    let mut service = Service::start(&home);
    eventually("the routine's prompt", || {
        prompts_received(&home, &bot, &nonce) == 1
    });
    let mut w = Window::connect(&home);
    let listed = w.ok("list_routines", json!({"bot": bot}))["routines"][0].clone();
    assert_eq!(listed["lastRun"]["outcome"], "sent");
    assert!(listed["nextRunMs"].as_i64().unwrap() > due);

    // Another restart does not send the slot again.
    service.kill();
    let _service = Service::start(&home);
    let mut w = Window::connect(&home);
    w.ok("list_bots", json!({}));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(prompts_received(&home, &bot, &nonce), 1);

    let connector = json!({"name": "github", "enabled": true,
        "transport": {"type": "stdio", "command": "npx", "args": ["-y", "pkg"], "env": {"TOKEN": "t"}}});
    assert_eq!(
        w.ok("set_connector", json!({"bot": bot, "connector": connector}))["connector"],
        connector
    );
    assert_eq!(
        w.ok("list_connectors", json!({"bot": bot}))["connectors"],
        json!([connector])
    );
    let bad = w.send(
        "set_connector",
        json!({"bot": bot, "connector": {"name": "Bad Name", "enabled": true,
        "transport": {"type": "http", "url": "https://x"}}}),
    );
    assert_eq!(
        w.wait_for(|f| f["type"] == "response" && f["id"] == bad.as_str())["code"],
        "invalid_connector"
    );
    w.ok("delete_connector", json!({"bot": bot, "name": "github"}));
    assert_eq!(
        w.ok("list_connectors", json!({"bot": bot}))["connectors"],
        json!([])
    );
}

#[test]
fn engine_error_output_lands_in_the_service_log_tagged_with_its_bot() {
    let home = home("stderr");
    let _service = Service::start(&home);
    let mut w = Window::connect(&home);
    let bot = w.create_bot();
    w.ok("prompt", json!({"bot": bot, "nonce": "n1", "text": "hi"}));
    let log = home.join("logs/appd.log");
    eventually("the engine's line in the log", || {
        fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .any(|l| l.contains(&format!("bot {bot}")) && l.ends_with("engine says hi"))
    });
}

#[test]
fn a_question_is_closed_when_its_turn_ends_unanswered() {
    let home = home("abandoned");
    let _service = Service::start(&home);
    let mut w = Window::connect(&home);
    let bot = w.create_bot();
    w.ok("prompt", json!({"bot": bot, "nonce": "n1", "text": "ask"}));
    w.wait_for(|f| f["type"] == "transcript" && f["entry"]["type"] == "question");
    w.send("abort", json!({"bot": bot}));
    let closed = w.wait_for(|f| f["type"] == "question_closed");
    assert_eq!(closed["question"], "q1");
    assert_eq!(
        w.ok("open_bot", json!({"bot": bot}))["questions"],
        json!([])
    );
}

/// A model server that answers one request with a streamed "Hello." and hands over the request body.
fn mock_model() -> (String, std::sync::mpsc::Receiver<Value>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let (tx, requests) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
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
        tx.send(serde_json::from_slice(&body).unwrap()).unwrap();
        let chunk = json!({"choices": [{"index": 0, "delta": {"content": "Hello."}}]});
        write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\ndata: {chunk}\n\ndata: [DONE]\n\n"
        )
        .unwrap();
    });
    (url, requests)
}

#[test]
fn the_real_engine_runs_a_turn_through_the_service() {
    // Built next to `appd`, where the service looks for its engine when `APPD_ENGINE` is unset.
    let mut cargo = Command::new(env!("CARGO"));
    cargo.args(["build", "-q", "-p", "agentcore"]);
    if !cfg!(debug_assertions) {
        cargo.arg("--release");
    }
    assert!(cargo.status().unwrap().success(), "agentcore did not build");

    let (url, requests) = mock_model();
    let home = home("agentcore");
    let _service = Service::start_with(&home, |c| {
        c.env_remove("APPD_ENGINE")
            .env_remove("BATE_MODEL_API_KEY")
            .env("BATE_MODEL_BASE_URL", &url);
    });
    let mut w = Window::connect(&home);
    let bot = w.create_bot();
    w.ok("set_model", json!({"bot": bot, "model": "m1"}));
    w.ok("prompt", json!({"bot": bot, "nonce": "n1", "text": "hi"}));
    let end = w.wait_for(|f| f["type"] == "transcript" && f["entry"]["type"] == "turn_end");
    assert_eq!(end["entry"]["reason"], "done", "{end}");

    let history = w.ok("open_bot", json!({"bot": bot}))["history"].clone();
    let entries: Vec<&Value> = history
        .as_array()
        .unwrap()
        .iter()
        .map(|e| &e["entry"])
        .collect();
    let kinds: Vec<&str> = entries
        .iter()
        .map(|e| e["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        ["session", "prompt", "turn_start", "text", "turn_end"]
    );
    assert_eq!(entries[3]["delta"], "Hello.");
    let request = requests.recv_timeout(TIMEOUT).unwrap();
    assert_eq!(request["model"], "m1");
    assert_eq!(
        request["messages"][1],
        json!({"role": "user", "content": "hi"})
    );
}

#[test]
fn connectors_reach_the_engine_when_it_starts_and_when_they_change() {
    let home = home("engine-connectors");
    let _service = Service::start(&home);
    let mut w = Window::connect(&home);
    let bot = w.create_bot();
    let connector = |name: &str| {
        json!({"name": name, "enabled": true,
            "transport": {"type": "http", "url": "https://example.com/mcp", "headers": {}}})
    };
    w.ok(
        "set_connector",
        json!({"bot": bot, "connector": connector("first")}),
    );
    // Storing a connector does not start an engine.
    assert_eq!(w.ok("list_bots", json!({}))["bots"][0]["running"], false);

    let log = home.join("bots").join(&bot).join("connectors.log");
    let sent = || -> Vec<String> {
        let log = fs::read_to_string(&log).unwrap_or_default();
        log.lines().map(str::to_owned).collect()
    };
    w.ok("set_model", json!({"bot": bot, "model": "m1"})); // starts the engine
    eventually("the list at start", || sent().len() == 1);
    assert!(sent()[0].contains(r#""name":"first""#), "{:?}", sent());

    w.ok(
        "set_connector",
        json!({"bot": bot, "connector": connector("second")}),
    );
    eventually("the list after a change", || sent().len() == 2);
    assert!(
        sent()[1].contains("first") && sent()[1].contains("second"),
        "{:?}",
        sent()
    );

    w.ok("delete_connector", json!({"bot": bot, "name": "first"}));
    eventually("the list after a delete", || sent().len() == 3);
    assert!(
        !sent()[2].contains("first") && sent()[2].contains("second"),
        "{:?}",
        sent()
    );
}

#[test]
fn rewind_removes_the_prompt_and_what_followed_from_history() {
    let home = home("rewind");
    let _service = Service::start(&home);
    let mut w = Window::connect(&home);
    let bot = w.create_bot();
    for nonce in ["n1", "n2"] {
        w.ok("prompt", json!({"bot": bot, "nonce": nonce, "text": "hi"}));
        w.wait_for(|f| f["type"] == "transcript" && f["entry"]["type"] == "turn_end");
    }
    w.send("rewind", json!({"bot": bot, "entry": "n2", "fork": false}));
    let changed = w.wait_for(|f| f["type"] == "history_changed");
    assert_eq!(changed["bot"], bot.as_str());

    let history = w.ok("open_bot", json!({"bot": bot}))["history"].clone();
    let entries: Vec<&Value> = history
        .as_array()
        .unwrap()
        .iter()
        .map(|e| &e["entry"])
        .collect();
    let prompts: Vec<&Value> = entries
        .iter()
        .filter(|e| e["type"] == "prompt")
        .map(|e| &e["nonce"])
        .collect();
    assert_eq!(prompts, ["n1"]);
    assert_eq!(entries.last().unwrap()["type"], "turn_end");
    assert_eq!(
        entries.iter().filter(|e| e["type"] == "turn_end").count(),
        1
    );

    // A late retry of the rewound prompt must not bring it back.
    let retry = w.ok("prompt", json!({"bot": bot, "nonce": "n2", "text": "hi"}));
    assert_eq!(retry["duplicate"], true, "{retry}");
    assert_eq!(prompts_received(&home, &bot, "n2"), 1);
}

#[test]
fn a_bot_is_busy_while_its_conversation_is_compacted() {
    let home = home("compact-busy");
    let _service = Service::start(&home);
    let mut w = Window::connect(&home);
    let bot = w.create_bot();
    // Past the `bots_changed` of the bot's creation.
    w.ok("list_bots", json!({}));
    w.send("compact", json!({"bot": bot}));
    // Windows are told each time to look again.
    for busy in [true, false] {
        w.wait_for(|f| f["type"] == "bots_changed");
        assert_eq!(w.ok("list_bots", json!({}))["bots"][0]["busy"], busy);
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[test]
fn an_hourly_routine_lists_its_past_runs_newest_first() {
    let home = home("routine-runs");
    let mut service = Service::start(&home);
    let mut w = Window::connect(&home);
    let bot = w.create_bot();
    // Half an hour from now, so no real slot passes during the test.
    let minute = (now_ms() / 60_000 + 30) % (24 * 60);
    let time = format!("{:02}:{:02}", minute / 60, minute % 60);
    let routine = w.ok(
        "set_routine",
        json!({"bot": bot, "routine": {"name": "Check", "prompt": "hi", "enabled": true,
            "schedule": {"frequency": "hourly", "interval": 2, "time": time, "timezone": "UTC"}}}),
    )["routine"]
        .clone();
    let id = routine["id"].as_str().unwrap().to_owned();
    let wait = routine["nextRunMs"].as_i64().unwrap() - now_ms();
    assert!(wait > 0 && wait <= 2 * 3600 * 1000, "{wait}");
    let runs = |w: &mut Window| {
        w.ok("list_routine_runs", json!({"bot": bot, "routineId": id}))["runs"]
            .as_array()
            .unwrap()
            .clone()
    };
    assert!(runs(&mut w).is_empty());

    // Two slots pass, each while the service is down.
    for n in 1..=2 {
        service.kill();
        let db = rusqlite::Connection::open(home.join("app.db")).unwrap();
        db.execute(
            "UPDATE routines SET routine = json_set(routine, '$.nextRunMs', ?1) WHERE id = ?2",
            rusqlite::params![now_ms() - 1000 * n, id],
        )
        .unwrap();
        drop(db);
        service = Service::start(&home);
        w = Window::connect(&home);
        eventually("the run", || {
            let log = fs::read_to_string(home.join("bots").join(&bot).join("prompts.log"));
            log.unwrap_or_default().lines().count() == n as usize
        });
    }
    let listed = runs(&mut w);
    assert_eq!(listed.len(), 2, "{listed:?}");
    assert!(listed.iter().all(|r| r["outcome"] == "sent"), "{listed:?}");
    assert!(listed[0]["atMs"].as_i64() > listed[1]["atMs"].as_i64());

    let missing = w.send(
        "list_routine_runs",
        json!({"bot": bot, "routineId": "nope"}),
    );
    let refused = w.wait_for(|f| f["type"] == "response" && f["id"] == missing.as_str());
    assert_eq!(refused["code"], "no_such_routine", "{refused}");

    // A deleted routine takes its runs with it.
    w.ok("delete_routine", json!({"bot": bot, "routineId": id}));
    let db = rusqlite::Connection::open(home.join("app.db")).unwrap();
    let left: i64 = db
        .query_row("SELECT count(*) FROM routine_runs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 0);
}

#[test]
fn a_bug_report_archive_has_logs_and_settings_and_no_secret() {
    let home = home("report");
    let _service = Service::start(&home);
    let mut w = Window::connect(&home);
    let bot = w.create_bot();
    let stdio = json!({"name": "files", "enabled": true, "transport": {"type": "stdio",
        "command": "npx", "args": ["serve", "--key=arg-SECRET-777"], "env": {"API_TOKEN": "env-SECRET-12345"}}});
    let http = json!({"name": "web", "enabled": true, "transport": {"type": "http",
        "url": "https://someone:pass-SECRET-444@example.com/mcp/path-SECRET-333-long/mcp?sig=url-SECRET-555",
        "headers": {"Authorization": "Bearer hdr-SECRET-999"}}});
    for connector in [stdio, http] {
        w.ok("set_connector", json!({"bot": bot, "connector": connector}));
    }
    w.ok(
        "set_routine",
        json!({"bot": bot, "routine": {"name": "name-SECRET", "prompt": "prompt-SECRET-text", "enabled": true,
            "schedule": {"frequency": "daily", "time": "09:00", "timezone": "UTC"}}}),
    );
    // What an engine or a connector might print.
    let mut log = fs::OpenOptions::new()
        .append(true)
        .open(home.join("logs/appd.log"))
        .unwrap();
    writeln!(
        log,
        "bot x: connector web: cannot reach https://someone:pass-SECRET-444@example.com/mcp/path-SECRET-333-long/mcp?sig=url-SECRET-555\n\
         bot x: GET /mcp/path-SECRET-333-long/mcp as someone with pass-SECRET-444 and url-SECRET-555; serve ok\n\
         bot x: sent Authorization: Bearer hdr-SECRET-999 and env-SECRET-12345\n\
         bot x: the server says hdr-SECRET-999 expired; ran with arg-SECRET-777\n\
         bot x: access_token=unknown-SECRET-000 harmless-marker"
    )
    .unwrap();

    let path = w.ok("export_report", json!({}))["path"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(path.ends_with(".tar.gz"), "{path}");
    let out = home.join("unpacked");
    fs::create_dir(&out).unwrap();
    let tar = Command::new("/usr/bin/tar")
        .args(["-xzf", &path, "-C"])
        .arg(&out)
        .status()
        .unwrap();
    assert!(tar.success());

    let settings = fs::read_to_string(out.join("settings.json")).unwrap();
    let log = fs::read_to_string(out.join("logs/appd.log")).unwrap();
    for text in [&settings, &log] {
        assert!(!text.contains("SECRET"), "{text}");
    }
    // What a bug report needs is still there.
    assert!(
        log.contains("harmless-marker") && log.contains("serving") && log.contains("serve ok"),
        "{log}"
    );
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let settings: Value = serde_json::from_str(&settings).unwrap();
    let connectors = &settings["bots"][0]["connectors"];
    assert_eq!(connectors[0]["name"], "files");
    assert_eq!(connectors[0]["transport"]["env"], json!(["API_TOKEN"]));
    assert_eq!(connectors[1]["transport"]["url"], "https://example.com");
    assert_eq!(
        connectors[1]["transport"]["headers"],
        json!(["Authorization"])
    );
    assert_eq!(
        settings["bots"][0]["routines"][0]["schedule"]["time"],
        "09:00"
    );
    // Nothing is left behind next to the archive.
    let left: Vec<_> = fs::read_dir(home.join("reports")).unwrap().collect();
    assert_eq!(left.len(), 1, "{left:?}");
}

/// Makes the bot's engine ask whether it may do `target`, and returns what it was told. `answer`
/// is the user's choice if the user gets asked; `None` fails the test if a question shows up.
fn act(w: &mut Window, bot: &str, nonce: &str, action: &str, answer: Option<&str>) -> String {
    w.ok(
        "prompt",
        json!({"bot": bot, "nonce": nonce, "text": format!("do {action}")}),
    );
    loop {
        let entry = w.wait_for(|f| f["type"] == "transcript")["entry"].clone();
        match entry["type"].as_str().unwrap() {
            "question" => {
                let choice = answer.unwrap_or_else(|| panic!("the user was asked: {entry}"));
                assert_eq!(entry["options"], json!(["Approve", "Always", "Deny"]));
                w.ok(
                    "answer",
                    json!({"bot": bot, "question": entry["question"], "choice": choice}),
                );
            }
            "text" => {
                w.wait_for(|f| f["entry"]["type"] == "turn_end");
                return entry["delta"].as_str().unwrap().to_owned();
            }
            _ => {}
        }
    }
}

#[test]
fn an_always_answer_is_remembered_for_exactly_that_action_until_it_is_removed() {
    let home = home("grants");
    let _service = Service::start(&home);
    let mut w = Window::connect(&home);
    let bot = w.create_bot();
    let permissions = |w: &mut Window| w.ok("get_permissions", json!({"bot": bot}));
    assert_eq!(
        permissions(&mut w),
        json!({"levels": {"read": "always", "write": "ask", "run": "ask", "connector": "ask"},
            "grants": []})
    );

    assert_eq!(
        act(&mut w, &bot, "n1", "run ls", Some("Always")),
        "chose Approve"
    );
    let grants = permissions(&mut w)["grants"].clone();
    assert_eq!(grants.as_array().unwrap().len(), 1, "{grants}");
    assert_eq!(
        (&grants[0]["kind"], &grants[0]["target"]),
        (&json!("run"), &json!("ls"))
    );

    // The same action no longer asks; any other still does, and so does another kind.
    assert_eq!(act(&mut w, &bot, "n2", "run ls", None), "chose Approve");
    assert_eq!(
        act(&mut w, &bot, "n3", "run ls -la", Some("Deny")),
        "chose Deny"
    );
    assert_eq!(
        act(&mut w, &bot, "n4", "write ls", Some("Deny")),
        "chose Deny"
    );
    // Approve is for once: nothing new is remembered.
    assert_eq!(
        act(&mut w, &bot, "n4b", "run date", Some("Approve")),
        "chose Approve"
    );
    assert_eq!(
        act(&mut w, &bot, "n4c", "run date", Some("Deny")),
        "chose Deny"
    );
    assert_eq!(permissions(&mut w)["grants"].as_array().unwrap().len(), 1);

    // Removed in the panel, it asks again.
    w.send(
        "delete_grant",
        json!({"bot": bot, "kind": "run", "target": "ls"}),
    );
    w.wait_for(|f| f["type"] == "permissions_changed");
    assert_eq!(permissions(&mut w)["grants"], json!([]));
    assert_eq!(
        act(&mut w, &bot, "n5", "run ls", Some("Deny")),
        "chose Deny"
    );

    let gone = w.send(
        "delete_grant",
        json!({"bot": bot, "kind": "run", "target": "ls"}),
    );
    let refused = w.wait_for(|f| f["type"] == "response" && f["id"] == gone.as_str());
    assert_eq!(refused["code"], "no_such_grant", "{refused}");
}

#[test]
fn a_level_of_always_or_never_is_answered_by_the_service_without_asking() {
    let home = home("levels");
    let _service = Service::start(&home);
    let mut w = Window::connect(&home);
    let bot = w.create_bot();
    let set = |w: &mut Window, kind: &str, level: &str| {
        w.ok(
            "set_permission",
            json!({"bot": bot, "kind": kind, "level": level}),
        );
    };
    // Reading is free unless the user says otherwise.
    assert_eq!(
        act(&mut w, &bot, "n1", "read notes.txt", None),
        "chose Approve"
    );
    set(&mut w, "read", "never");
    assert_eq!(
        act(&mut w, &bot, "n2", "read notes.txt", None),
        "chose Deny"
    );
    set(&mut w, "run", "always");
    assert_eq!(
        act(&mut w, &bot, "n3", "run rm -rf x", None),
        "chose Approve"
    );
    assert_eq!(
        w.ok("get_permissions", json!({"bot": bot}))["levels"],
        json!({"read": "never", "write": "ask", "run": "always", "connector": "ask"})
    );

    // Never beats a remembered answer.
    assert_eq!(
        act(&mut w, &bot, "n4", "write a.txt", Some("Always")),
        "chose Approve"
    );
    set(&mut w, "write", "never");
    assert_eq!(act(&mut w, &bot, "n5", "write a.txt", None), "chose Deny");

    // Nothing the service answered was left open or shown as a question.
    let opened = w.ok("open_bot", json!({"bot": bot}));
    assert_eq!(opened["questions"], json!([]));
    let asked = opened["history"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["entry"]["type"] == "question")
        .count();
    assert_eq!(asked, 1);

    let bad = w.send(
        "set_permission",
        json!({"bot": bot, "kind": "fly", "level": "always"}),
    );
    let refused = w.wait_for(|f| f["type"] == "response" && f["id"] == bad.as_str());
    assert_eq!(refused["code"], "bad_request", "{refused}");
}

#[test]
fn always_is_refused_for_a_question_that_is_not_about_an_action() {
    let home = home("always-plain");
    let _service = Service::start(&home);
    let mut w = Window::connect(&home);
    let bot = w.create_bot();
    w.ok("prompt", json!({"bot": bot, "nonce": "n1", "text": "ask"}));
    let asked = w.wait_for(|f| f["type"] == "transcript" && f["entry"]["type"] == "question");
    assert_eq!(asked["entry"]["options"], json!(["Approve", "Deny"]));
    let id = w.send(
        "answer",
        json!({"bot": bot, "question": "q1", "choice": "Always"}),
    );
    let refused = w.wait_for(|f| f["type"] == "response" && f["id"] == id.as_str());
    assert_eq!(refused["code"], "bad_request", "{refused}");
    assert_eq!(
        w.ok("get_permissions", json!({"bot": bot}))["grants"],
        json!([])
    );
}

#[test]
fn a_connector_that_is_changed_or_removed_loses_its_grants() {
    let home = home("connector-grants");
    let _service = Service::start(&home);
    let mut w = Window::connect(&home);
    let bot = w.create_bot();
    let set = |w: &mut Window, name: &str, command: &str| {
        w.send(
            "set_connector",
            json!({"bot": bot, "connector": {"name": name, "enabled": true, "transport": {"type": "stdio", "command": command}}}),
        )
    };
    let granted = |w: &mut Window| {
        let grants = w.ok("get_permissions", json!({"bot": bot}))["grants"].clone();
        let targets = grants.as_array().unwrap().iter();
        targets
            .map(|g| g["target"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    for (nonce, target) in [
        ("n1", "web__search"),
        ("n2", "web2__search"),
        ("n3", "web__fetch"),
    ] {
        let action = format!("connector {target}");
        assert_eq!(
            act(&mut w, &bot, nonce, &action, Some("Always")),
            "chose Approve"
        );
    }
    // A file that happens to be called like a connector's tool is another kind of action.
    assert_eq!(
        act(&mut w, &bot, "n4", "write web__notes", Some("Always")),
        "chose Approve"
    );

    // Another server under the same name: its tools ask again. Other connectors keep theirs.
    set(&mut w, "web", "other-server");
    w.wait_for(|f| f["type"] == "permissions_changed");
    assert_eq!(granted(&mut w), ["web2__search", "web__notes"]);
    assert_eq!(
        act(&mut w, &bot, "n5", "connector web__search", Some("Deny")),
        "chose Deny"
    );

    assert_eq!(
        act(&mut w, &bot, "n6", "connector web__fetch", Some("Always")),
        "chose Approve"
    );
    w.send("delete_connector", json!({"bot": bot, "name": "web"}));
    w.wait_for(|f| f["type"] == "permissions_changed");
    assert_eq!(granted(&mut w), ["web2__search", "web__notes"]);

    // A name that could pass for another connector's tool is refused.
    let id = set(&mut w, "web__search", "x");
    let refused = w.wait_for(|f| f["type"] == "response" && f["id"] == id.as_str());
    assert_eq!(refused["code"], "invalid_connector", "{refused}");
}

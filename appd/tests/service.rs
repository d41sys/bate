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
/// `abort` ends the turn `aborted`. Each `connectors` operation is logged to `connectors.log`. `compact` takes 0.3 s.
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
    let routine = w.ok(
        "set_routine",
        json!({"bot": bot, "routine": {"name": "Check", "prompt": "hi", "enabled": true,
            "schedule": {"frequency": "hourly", "interval": 2, "time": "00:10", "timezone": "UTC"}}}),
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

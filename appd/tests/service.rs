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
/// the turn then says which choice the engine was given. A prompt `probe write <path>` or
/// `probe read <path>` (or `connect`, to a socket) tries just that and says `ok` or `refused`.
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
        *'"text":"probe '*)
          what=$(printf '%s' "$line" | sed 's/.*"text":"probe \([a-z]*\) .*/\1/')
          path=$(printf '%s' "$line" | sed 's/.*"text":"probe [a-z]* \([^"]*\)".*/\1/')
          case "$what" in
            write) (echo x > "$path") 2>/dev/null && r=ok || r=refused ;;
            connect) nc -U -w 2 "$path" </dev/null >/dev/null 2>&1 && r=ok || r=refused ;;
            keychain) security list-keychains >/dev/null 2>&1 && r=ok || r=refused ;;
            signal) kill -0 "$path" 2>/dev/null && r=ok || r=refused ;;
            prefs) defaults write "$path" k v >/dev/null 2>&1 && r=ok || r=refused ;;
            env) [ -n "$(printenv "$path")" ] && r=ok || r=refused ;;
            *) cat "$path" >/dev/null 2>&1 && r=ok || r=refused ;;
          esac
          echo "$ok"; echo "{\"type\":\"turn_start\",\"nonce\":\"$nonce\"}"
          echo "{\"type\":\"text\",\"delta\":\"$r\"}"; echo '{"type":"turn_end","reason":"done"}' ;;
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
            // Any free port: the tests run side by side.
            .env("APPD_HOOK_PORT", "0")
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

/// Has the bot's engine try `write <path>` or `read <path>`; returns `ok` or `refused`.
fn probe(w: &mut Window, bot: &str, nonce: &str, what: &str) -> String {
    w.ok(
        "prompt",
        json!({"bot": bot, "nonce": nonce, "text": format!("probe {what}")}),
    );
    let said = w.wait_for(|f| f["type"] == "transcript" && f["entry"]["type"] == "text");
    w.wait_for(|f| f["entry"]["type"] == "turn_end");
    said["entry"]["delta"].as_str().unwrap().to_owned()
}

#[test]
fn a_bot_is_kept_in_its_folder_by_the_system_until_it_is_given_the_whole_disk() {
    let home = home("folder-limit");
    let _service = Service::start_with(&home, |c| {
        c.env("APPD_ENGINE", home.join("fake-engine"))
            .env("USER_TOKEN", "t0ken")
            .env("BATE_MODEL", "m1");
    });
    let mut w = Window::connect(&home);
    let bot = w.create_bot();
    let outside = home.join("outside.txt");
    let write_outside = format!("write {}", outside.display());
    // A file of the user's, outside the bot's folder: this crate's manifest.
    let read_outside = format!("read {}/Cargo.toml", env!("CARGO_MANIFEST_DIR"));

    assert_eq!(w.ok("list_bots", json!({}))["bots"][0]["wholeDisk"], false);
    assert_eq!(probe(&mut w, &bot, "n1", "write inside.txt"), "ok");
    assert_eq!(probe(&mut w, &bot, "n2", "read inside.txt"), "ok");
    assert_eq!(probe(&mut w, &bot, "n3", &write_outside), "refused");
    assert!(!outside.exists());
    assert_eq!(probe(&mut w, &bot, "n4", &read_outside), "refused");
    // The service's own files and its socket: whoever connects there is taken for a window and
    // could lift the limit.
    let socket = format!("connect {}", home.join("run/appd.sock").display());
    assert_eq!(probe(&mut w, &bot, "n4b", &socket), "refused");
    assert_eq!(probe(&mut w, &bot, "n4d", "keychain x"), "refused");
    let signal = format!("signal {}", std::process::id());
    assert_eq!(probe(&mut w, &bot, "n4e", &signal), "refused");
    // Preferences are written by a system service into the user's real home, for whoever
    // asks: an app's startup settings would be a way out.
    let domain = format!("com.bate.probe-{}", std::process::id());
    let wrote = probe(&mut w, &bot, "n4j", &format!("prefs {domain}"));
    let read = Command::new("/usr/bin/defaults")
        .args(["read", &domain])
        .output();
    let _ = Command::new("/usr/bin/defaults")
        .args(["delete", &domain])
        .output();
    assert_eq!(wrote, "refused");
    assert!(
        !read.unwrap().status.success(),
        "the preference was written"
    );
    // Other programs' temporary files.
    let tmp = PathBuf::from(format!("/private/tmp/appd-probe-{}", std::process::id()));
    fs::write(&tmp, "x").unwrap();
    let read_tmp = format!("read {}", tmp.display());
    assert_eq!(probe(&mut w, &bot, "n4f", &read_tmp), "refused");
    // The service's environment may hold the user's tokens; the engine's own settings pass.
    assert_eq!(probe(&mut w, &bot, "n4g", "env USER_TOKEN"), "refused");
    assert_eq!(probe(&mut w, &bot, "n4h", "env BATE_MODEL"), "ok");
    // Still there: looking up names, which every hosted model needs.
    let lookup = "connect /private/var/run/mDNSResponder";
    assert_eq!(probe(&mut w, &bot, "n4i", lookup), "ok");
    let db = format!("read {}", home.join("app.db").display());
    assert_eq!(probe(&mut w, &bot, "n4c", &db), "refused");
    // A way round through the folder itself is closed too.
    std::os::unix::fs::symlink(&home.0, home.join("bots").join(&bot).join("up")).unwrap();
    assert_eq!(probe(&mut w, &bot, "n5", "write up/outside.txt"), "refused");
    assert!(!outside.exists());

    // The limit is set when an engine starts, so the running one is stopped.
    let stopped = |w: &mut Window, whole_disk: bool| {
        w.ok(
            "set_disk_access",
            json!({"bot": bot, "wholeDisk": whole_disk}),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while w.ok("list_bots", json!({}))["bots"][0]["running"] != false {
            assert!(Instant::now() < deadline, "the engine was not stopped");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            w.ok("list_bots", json!({}))["bots"][0]["wholeDisk"],
            whole_disk
        );
    };
    stopped(&mut w, true);
    assert_eq!(probe(&mut w, &bot, "n6", &write_outside), "ok");
    assert!(outside.exists());
    assert_eq!(probe(&mut w, &bot, "n7", &read_outside), "ok");
    assert_eq!(probe(&mut w, &bot, "n7b", &socket), "ok");
    assert_eq!(probe(&mut w, &bot, "n7c", "keychain x"), "ok");
    assert_eq!(probe(&mut w, &bot, "n7d", &signal), "ok");
    assert_eq!(probe(&mut w, &bot, "n7e", "env USER_TOKEN"), "ok");
    assert_eq!(probe(&mut w, &bot, "n7f", &read_tmp), "ok");
    fs::remove_file(&tmp).unwrap();

    // And back: the limit holds again from the next thing the bot does.
    // A link the bot left where the service makes its temporary folder is not followed.
    let tmp_dir = home.join("bots").join(&bot).join(".tmp");
    let _ = fs::remove_dir_all(&tmp_dir);
    std::os::unix::fs::symlink(&home.0, &tmp_dir).unwrap();
    stopped(&mut w, false);
    fs::remove_file(&outside).unwrap();
    assert_eq!(probe(&mut w, &bot, "n8", &write_outside), "refused");
    assert!(!outside.exists());
    assert!(fs::symlink_metadata(&tmp_dir).unwrap().is_dir());

    // Not in the middle of a turn: it would go on under the old rule.
    w.ok("prompt", json!({"bot": bot, "nonce": "n9", "text": "hi"}));
    w.wait_for(|f| f["entry"]["type"] == "turn_start");
    let id = w.send("set_disk_access", json!({"bot": bot, "wholeDisk": true}));
    let refused = w.wait_for(|f| f["type"] == "response" && f["id"] == id.as_str());
    assert_eq!(refused["code"], "busy", "{refused}");
    assert_eq!(w.ok("list_bots", json!({}))["bots"][0]["wholeDisk"], false);
}

#[test]
fn memory_notes_are_plain_files_the_panel_lists_reads_writes_and_deletes() {
    let home = home("memory");
    let _service = Service::start(&home);
    let mut w = Window::connect(&home);
    let bot = w.create_bot();
    let folder = home.join("bots").join(&bot).join("memory");
    let refused = |w: &mut Window, command: &str, args: Value| {
        let id = w.send(command, args);
        w.wait_for(|f| f["type"] == "response" && f["id"] == id.as_str())["code"].clone()
    };
    assert_eq!(w.ok("list_memory", json!({"bot": bot}))["notes"], json!([]));

    w.ok(
        "write_memory",
        json!({"bot": bot, "name": "user.md", "text": "Called Ada.\n"}),
    );
    assert_eq!(
        fs::read_to_string(folder.join("user.md")).unwrap(),
        "Called Ada.\n"
    );
    // What the bot itself wrote there is listed too, by name; other files are not notes.
    fs::write(folder.join("Likes tea.md"), "Tea.").unwrap();
    fs::write(folder.join("scratch.txt"), "x").unwrap();
    let notes = w.ok("list_memory", json!({"bot": bot}))["notes"].clone();
    let names: Vec<_> = notes
        .as_array()
        .unwrap()
        .iter()
        .map(|n| &n["name"])
        .collect();
    assert_eq!(names, ["Likes tea.md", "user.md"]);
    assert_eq!(notes[1]["bytes"], 12);
    assert!(notes[1]["modifiedMs"].as_i64().unwrap() > now_ms() - 60_000);
    assert_eq!(
        w.ok("read_memory", json!({"bot": bot, "name": "Likes tea.md"}))["text"],
        "Tea."
    );

    w.ok("delete_memory", json!({"bot": bot, "name": "user.md"}));
    assert!(!folder.join("user.md").exists());
    let gone = json!({"bot": bot, "name": "user.md"});
    assert_eq!(refused(&mut w, "read_memory", gone.clone()), "no_such_note");
    assert_eq!(refused(&mut w, "delete_memory", gone), "no_such_note");

    // A name cannot leave the folder, and a note has a size limit.
    for name in ["../app.md", "a/b.md", ".hidden.md", "notes.txt", ".md"] {
        let args = json!({"bot": bot, "name": name, "text": "x"});
        assert_eq!(
            refused(&mut w, "write_memory", args),
            "bad_request",
            "{name}"
        );
    }
    let long = json!({"bot": bot, "name": "long.md", "text": "x".repeat(64 * 1024 + 1)});
    assert_eq!(refused(&mut w, "write_memory", long), "bad_request");

    // The bot can write in its folder, so it can leave a link under a note's name: the
    // service, which is not limited, neither reads nor writes through it.
    let secret = home.join("secret.txt");
    fs::write(&secret, "outside").unwrap();
    std::os::unix::fs::symlink(&secret, folder.join("link.md")).unwrap();
    let link = json!({"bot": bot, "name": "link.md"});
    assert_eq!(names_of(&mut w, &bot), ["Likes tea.md"]);
    assert_eq!(refused(&mut w, "read_memory", link), "no_such_note");
    // A second name for a file elsewhere is a link too.
    let other = home.join("other.txt");
    fs::write(&other, "outside too").unwrap();
    fs::hard_link(&other, folder.join("hard.md")).unwrap();
    assert_eq!(names_of(&mut w, &bot), ["Likes tea.md"]);
    let hard = json!({"bot": bot, "name": "hard.md"});
    assert_eq!(refused(&mut w, "read_memory", hard), "no_such_note");
    w.ok(
        "write_memory",
        json!({"bot": bot, "name": "link.md", "text": "mine"}),
    );
    assert_eq!(fs::read_to_string(&secret).unwrap(), "outside");
    assert_eq!(fs::read_to_string(folder.join("link.md")).unwrap(), "mine");

    // Nor when the folder itself is a link.
    fs::remove_dir_all(&folder).unwrap();
    std::os::unix::fs::symlink(&home.0, &folder).unwrap();
    let args = json!({"bot": bot, "name": "secret.md", "text": "x"});
    assert_eq!(refused(&mut w, "write_memory", args), "internal");
    assert!(!home.join("secret.md").exists());
    assert_eq!(
        refused(&mut w, "list_memory", json!({"bot": bot})),
        "internal"
    );
}

fn names_of(w: &mut Window, bot: &str) -> Vec<String> {
    let notes = w.ok("list_memory", json!({"bot": bot}))["notes"].clone();
    let names = notes.as_array().unwrap().iter();
    names
        .map(|n| n["name"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn a_skill_folder_is_installed_for_one_bot_listed_and_removed() {
    let home = home("skills");
    let _service = Service::start(&home);
    let mut w = Window::connect(&home);
    let (bot, other) = (w.create_bot(), w.create_bot());
    let refused = |w: &mut Window, command: &str, args: Value| {
        let id = w.send(command, args);
        w.wait_for(|f| f["type"] == "response" && f["id"] == id.as_str())["code"].clone()
    };
    let skill_md = |description: &str| {
        format!("---\nname: pdf-forms\ndescription: {description}\n---\nHow to.\n")
    };
    let source = home.join("picked/Some Folder");
    fs::create_dir_all(source.join("scripts")).unwrap();
    fs::write(source.join("SKILL.md"), skill_md("Fill in PDF forms.")).unwrap();
    fs::write(source.join("scripts/fill.sh"), "#!/bin/sh\n").unwrap();
    // Runnable stays runnable; setuid does not come along.
    fs::set_permissions(
        source.join("scripts/fill.sh"),
        fs::Permissions::from_mode(0o4755),
    )
    .unwrap();
    let skills = home.join("bots").join(&bot).join("skills");
    let install = |w: &mut Window, path: &Path| {
        w.ok("install_skill", json!({"bot": bot, "path": path}))["skill"].clone()
    };
    let listed =
        |w: &mut Window, bot: &str| w.ok("list_skills", json!({"bot": bot}))["skills"].clone();

    // Installed under the skill's own name, whatever the picked folder is called.
    let expected = json!({"name": "pdf-forms", "description": "Fill in PDF forms."});
    assert_eq!(install(&mut w, &source), expected);
    assert_eq!(listed(&mut w, &bot), json!([expected]));
    assert_eq!(listed(&mut w, &other), json!([]));
    let script = skills.join("pdf-forms/scripts/fill.sh");
    assert_eq!(fs::read_to_string(&script).unwrap(), "#!/bin/sh\n");
    assert_eq!(
        fs::metadata(&script).unwrap().permissions().mode() & 0o7777,
        0o755
    );

    // Installing again replaces the whole skill.
    fs::write(skills.join("pdf-forms/stale.txt"), "x").unwrap();
    fs::write(source.join("SKILL.md"), skill_md("Fill in any PDF form.")).unwrap();
    assert_eq!(
        install(&mut w, &source)["description"],
        "Fill in any PDF form."
    );
    assert!(!skills.join("pdf-forms/stale.txt").exists());

    // Not a skill: no folder, no SKILL.md, a link inside. Nothing is left half installed.
    let bad = home.join("picked/bad");
    fs::create_dir_all(&bad).unwrap();
    let args = |path: &Path| json!({"bot": other, "path": path});
    assert_eq!(
        refused(&mut w, "install_skill", args(&source.join("SKILL.md"))),
        "invalid_skill"
    );
    assert_eq!(
        refused(&mut w, "install_skill", args(&bad)),
        "invalid_skill"
    );
    fs::write(bad.join("SKILL.md"), skill_md("Bad.")).unwrap();
    std::os::unix::fs::symlink(home.join("app.db"), bad.join("data")).unwrap();
    assert_eq!(
        refused(&mut w, "install_skill", args(&bad)),
        "invalid_skill"
    );
    assert_eq!(listed(&mut w, &other), json!([]));
    eventually("an empty staging folder", || {
        fs::read_dir(home.join("staging")).unwrap().count() == 0
    });

    // Nor a pipe for a SKILL.md (which must not be waited on), nor too many files.
    let pipe = home.join("picked/pipe");
    fs::create_dir_all(&pipe).unwrap();
    assert!(
        Command::new("mkfifo")
            .arg(pipe.join("SKILL.md"))
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        refused(&mut w, "install_skill", args(&pipe)),
        "invalid_skill"
    );
    let many = home.join("picked/many");
    fs::create_dir_all(&many).unwrap();
    fs::write(many.join("SKILL.md"), skill_md("Many.")).unwrap();
    for n in 0..500 {
        fs::write(many.join(format!("{n}.txt")), "").unwrap();
    }
    assert_eq!(
        refused(&mut w, "install_skill", args(&many)),
        "invalid_skill"
    );
    assert_eq!(listed(&mut w, &other), json!([]));

    // A folder in the skill that nobody may enter does not keep a new copy from going in.
    let locked = skills.join("pdf-forms/locked");
    fs::create_dir_all(locked.join("sub")).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    install(&mut w, &source);
    assert!(!locked.exists());

    // The bot can write in its folder, so it can put a link where a skill goes. The service
    // replaces the link; what it pointed to is untouched.
    let victim = home.join("victim");
    fs::create_dir_all(&victim).unwrap();
    fs::write(victim.join("keep.txt"), "mine").unwrap();
    // A skill of its own: were the link followed, it would be listed.
    fs::write(victim.join("SKILL.md"), skill_md("Not installed.")).unwrap();
    fs::remove_dir_all(skills.join("pdf-forms")).unwrap();
    std::os::unix::fs::symlink(&victim, skills.join("pdf-forms")).unwrap();
    assert_eq!(listed(&mut w, &bot), json!([]));
    install(&mut w, &source);
    assert!(
        fs::symlink_metadata(skills.join("pdf-forms"))
            .unwrap()
            .is_dir()
    );
    assert_eq!(fs::read_dir(&victim).unwrap().count(), 2);

    w.ok("delete_skill", json!({"bot": bot, "name": "pdf-forms"}));
    assert!(!skills.join("pdf-forms").exists());
    let gone = json!({"bot": bot, "name": "pdf-forms"});
    assert_eq!(
        refused(&mut w, "delete_skill", gone.clone()),
        "no_such_skill"
    );
    // Removing a link removes the link.
    std::os::unix::fs::symlink(&victim, skills.join("pdf-forms")).unwrap();
    w.ok("delete_skill", gone);
    assert_eq!(fs::read_to_string(victim.join("keep.txt")).unwrap(), "mine");
    let up = json!({"bot": bot, "name": "../victim"});
    assert_eq!(refused(&mut w, "delete_skill", up), "bad_request");

    // Nor is anything installed through a skills folder that is itself a link.
    fs::remove_dir_all(&skills).unwrap();
    std::os::unix::fs::symlink(&victim, &skills).unwrap();
    let args = json!({"bot": bot, "path": source});
    assert_eq!(refused(&mut w, "install_skill", args), "invalid_skill");
    assert_eq!(fs::read_dir(&victim).unwrap().count(), 2);
    eventually("an empty staging folder", || {
        fs::read_dir(home.join("staging")).unwrap().count() == 0
    });
}

/// One HTTP request to the service's webhook port; gives the status.
fn http(url: &str, method: &str, path: Option<&str>, head: &str, body: &str) -> u16 {
    let rest = url.strip_prefix("http://").unwrap();
    let (host, hook) = rest.split_once('/').unwrap();
    let path = path.map_or(format!("/{hook}"), str::to_owned);
    let mut stream = std::net::TcpStream::connect(host).unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nhost: {host}\r\n{head}\r\n{body}"
    )
    .unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    reply.split(' ').nth(1).unwrap_or("0").parse().unwrap()
}

#[test]
fn a_local_webhook_starts_a_routine_and_passes_the_requests_data_on() {
    let home = home("webhook");
    let _service = Service::start(&home);
    let mut w = Window::connect(&home);
    let bot = w.create_bot();
    let prompts =
        || fs::read_to_string(home.join("bots").join(&bot).join("prompts.log")).unwrap_or_default();
    let post = |url: &str, body: &str| {
        let head = format!("content-length: {}\r\n", body.len());
        http(url, "POST", None, &head, body)
    };

    // No schedule: only the webhook starts it.
    let routine = w.ok(
        "set_routine",
        json!({"bot": bot, "routine": {"name": "Deploys", "prompt": "Look at this build.",
            "enabled": true, "webhook": true}}),
    )["routine"]
        .clone();
    let (id, url) = (
        routine["id"].as_str().unwrap(),
        routine["hookUrl"].as_str().unwrap(),
    );
    assert!(
        routine["nextRunMs"].is_null() && routine["schedule"].is_null(),
        "{routine}"
    );
    assert!(
        url.starts_with("http://127.0.0.1:") && url.len() > 60,
        "{url}"
    );

    assert_eq!(post(url, "build 42 failed"), 202);
    eventually("the routine's prompt", || {
        prompts().contains("build 42 failed")
    });
    let sent = prompts();
    assert!(sent.contains(&format!("\"nonce\":\"hook-{id}-")), "{sent}");
    assert!(
        sent.contains("[Routine: Deploys]\\nLook at this build.\\n\\n[Data sent"),
        "{sent}"
    );
    let runs = w.ok("list_routine_runs", json!({"bot": bot, "routineId": id}))["runs"].clone();
    assert_eq!(runs.as_array().unwrap().len(), 1, "{runs}");
    assert_eq!(runs[0]["outcome"], "sent");

    // Nothing else starts it: another token, another path, another method, too much data.
    let other = format!("{}0000", &url[..url.len() - 4]);
    assert_eq!(post(&other, "x"), 404);
    assert_eq!(
        http(url, "POST", Some("/"), "content-length: 0\r\n", ""),
        404
    );
    assert_eq!(http(url, "GET", None, "", ""), 405);
    assert_eq!(
        http(url, "POST", None, "content-length: 70000\r\n", ""),
        413
    );
    assert_eq!(
        http(
            url,
            "POST",
            None,
            "transfer-encoding: chunked\r\n",
            "1\r\nx\r\n0\r\n\r\n"
        ),
        411
    );

    // Changing the routine keeps its address; a routine that is off does not answer to it.
    let mut edited = routine.clone();
    edited["enabled"] = json!(false);
    edited["schedule"] = json!({"frequency": "daily", "time": "09:00"});
    let off = w.ok("set_routine", json!({"bot": bot, "routine": edited}))["routine"].clone();
    assert_eq!(off["hookUrl"], url);
    assert_eq!(post(url, "while off"), 404);
    edited["enabled"] = json!(true);
    let on = w.ok("set_routine", json!({"bot": bot, "routine": edited}))["routine"].clone();
    assert!(on["nextRunMs"].as_i64().unwrap() > now_ms(), "{on}");
    assert_eq!(
        w.ok("list_routines", json!({"bot": bot}))["routines"][0]["hookUrl"],
        url
    );

    // Webhook off: the address is gone, and so is a routine nothing could start.
    edited["webhook"] = json!(false);
    let plain = w.ok("set_routine", json!({"bot": bot, "routine": edited}))["routine"].clone();
    assert!(
        plain["hookUrl"].is_null() && plain["hookToken"].is_null(),
        "{plain}"
    );
    assert_eq!(post(url, "after"), 404);
    edited.as_object_mut().unwrap().remove("schedule");
    let id = w.send("set_routine", json!({"bot": bot, "routine": edited}));
    let refused = w.wait_for(|f| f["type"] == "response" && f["id"] == id.as_str());
    assert_eq!(refused["code"], "invalid_routine", "{refused}");
    assert!(!prompts().contains("while off") && !prompts().contains("after"));
}

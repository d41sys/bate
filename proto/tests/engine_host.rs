use proto::{Engine, Event, Op, TurnEnd};
use std::fs;
use std::os::unix::fs::PermissionsExt;

// A fake engine: says ready, answers every request, replies "pong" to a prompt, exits on stop.
const FAKE_ENGINE: &str = r#"#!/bin/sh
echo '{"type":"ready","protocol":1}'
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed 's/.*"id":"\([^"]*\)".*/\1/')
  echo "{\"type\":\"response\",\"id\":\"$id\"}"
  case "$line" in
    *'"type":"prompt"'*) echo '{"type":"text","delta":"pong"}'; echo '{"type":"turn_end","reason":"done"}' ;;
    *'"type":"stop"'*) exit 0 ;;
  esac
done
"#;

#[test]
fn the_service_runs_the_engine_named_by_appd_engine_over_stdio() {
    // A misbehaving engine would block `recv` forever; fail instead of hanging `cargo test`.
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(10));
        eprintln!("engine test timed out");
        std::process::exit(1);
    });
    // The script sits under the workspace target folder and is named by a path
    // relative to this test's working directory; the engine runs in another folder.
    let rel = format!("../target/tmp/proto-engine-{}", std::process::id());
    fs::create_dir_all(&rel).unwrap();
    let script = format!("{rel}/fake-engine");
    fs::write(&script, FAKE_ENGINE).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let workdir = std::env::temp_dir().join(format!("proto-bot-{}", std::process::id()));
    fs::create_dir_all(&workdir).unwrap();
    // Only test in this binary, so no other thread reads the environment.
    unsafe { std::env::set_var("APPD_ENGINE", &script) };

    let mut engine = Engine::spawn(&proto::engine_program(), &workdir).unwrap();
    assert_eq!(
        engine.output.recv().unwrap(),
        Some(Event::Ready { protocol: 1 })
    );

    engine
        .input
        .send(
            "1",
            &Op::Prompt {
                nonce: "n1".into(),
                text: "ping".into(),
            },
        )
        .unwrap();
    assert_eq!(
        engine.output.recv().unwrap(),
        Some(Event::Response {
            id: "1".into(),
            error: None
        })
    );
    assert_eq!(
        engine.output.recv().unwrap(),
        Some(Event::Text {
            delta: "pong".into()
        })
    );
    assert_eq!(
        engine.output.recv().unwrap(),
        Some(Event::TurnEnd {
            reason: TurnEnd::Done,
            error: None
        })
    );

    engine.input.send("2", &Op::Stop).unwrap();
    assert_eq!(
        engine.output.recv().unwrap(),
        Some(Event::Response {
            id: "2".into(),
            error: None
        })
    );
    assert_eq!(engine.output.recv().unwrap(), None);
    assert!(engine.child.wait().unwrap().success());
    fs::remove_dir_all(&rel).unwrap();
    fs::remove_dir_all(&workdir).unwrap();
}

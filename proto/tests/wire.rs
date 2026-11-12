use proto::{Event, MAX_OP_FRAME, Op, Request, TurnEnd, read_frame, write_frame};
use serde_json::json;
use std::io::Cursor;

#[test]
fn a_prompt_request_is_one_flat_json_object() {
    let req = Request {
        id: "7".into(),
        op: Op::Prompt {
            nonce: "n1".into(),
            text: "hi".into(),
        },
    };
    assert_eq!(
        serde_json::to_value(&req).unwrap(),
        json!({"id": "7", "type": "prompt", "nonce": "n1", "text": "hi"})
    );
    let unit = Request {
        id: "8".into(),
        op: Op::Abort,
    };
    assert_eq!(
        serde_json::to_value(&unit).unwrap(),
        json!({"id": "8", "type": "abort"})
    );
    let back: Request =
        serde_json::from_value(json!({"id": "9", "type": "rewind", "entry": "e3", "fork": true}))
            .unwrap();
    assert_eq!(
        back.op,
        Op::Rewind {
            entry: "e3".into(),
            fork: true
        }
    );
}

#[test]
fn engine_events_parse_from_the_documented_json() {
    let ev: Event = serde_json::from_value(
        json!({"type": "tool_end", "call": "c1", "output": "x", "isError": true}),
    )
    .unwrap();
    assert_eq!(
        ev,
        Event::ToolEnd {
            call: "c1".into(),
            output: "x".into(),
            is_error: true
        }
    );
    let ev: Event = serde_json::from_value(
        json!({"type": "turn_end", "reason": "error", "error": "rate limited"}),
    )
    .unwrap();
    assert_eq!(
        ev,
        Event::TurnEnd {
            reason: TurnEnd::Error,
            error: Some("rate limited".into())
        }
    );
}

#[test]
fn an_event_type_outside_the_list_is_refused() {
    assert!(serde_json::from_value::<Event>(json!({"type": "bogus"})).is_err());
    assert!(serde_json::from_value::<Op>(json!({"type": "delete_everything"})).is_err());
}

#[test]
fn bad_lines_are_refused_without_losing_the_stream() {
    let mut input = Cursor::new(b"not json\n[1,2]\n{\"type\":\"abort\"}\n".to_vec());
    let mut buf = Vec::new();
    assert!(read_frame(&mut input, MAX_OP_FRAME, &mut buf).is_err());
    assert!(read_frame(&mut input, MAX_OP_FRAME, &mut buf).is_err());
    assert_eq!(
        read_frame(&mut input, MAX_OP_FRAME, &mut buf).unwrap(),
        Some(json!({"type": "abort"}))
    );
    assert_eq!(
        read_frame(&mut input, MAX_OP_FRAME, &mut buf).unwrap(),
        None
    );
}

#[test]
fn oversize_frames_are_refused_both_ways() {
    let big = json!({"text": "x".repeat(64)});
    assert!(write_frame(&mut Vec::new(), &big, 32).is_err());
    let mut line = serde_json::to_vec(&big).unwrap();
    line.push(b'\n');
    assert!(read_frame(&mut Cursor::new(line), 32, &mut Vec::new()).is_err());
}

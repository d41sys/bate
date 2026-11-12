use proto::{Event, MAX_OP_FRAME, Op, Request, TurnEnd, read_frame, write_frame};
use serde_json::json;
use std::io::{Cursor, ErrorKind};

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
    let mut input =
        Cursor::new(b"not json\n[1,2]\n{\"type\":\"bogus\"}\n{\"type\":\"abort\"}\n".to_vec());
    let mut buf = Vec::new();
    for _ in 0..3 {
        let e = read_frame::<Op>(&mut input, MAX_OP_FRAME, &mut buf).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::InvalidData);
    }
    assert_eq!(
        read_frame::<Op>(&mut input, MAX_OP_FRAME, &mut buf).unwrap(),
        Some(Op::Abort)
    );
    assert_eq!(
        read_frame::<Op>(&mut input, MAX_OP_FRAME, &mut buf).unwrap(),
        None
    );
}

#[test]
fn oversize_frames_are_refused_both_ways() {
    // `{"a":1}` plus newline is 8 bytes: a limit of 8 takes it, 7 refuses it, on both sides.
    let frame = json!({"a": 1});
    assert!(write_frame(&mut Vec::new(), &frame, 8).is_ok());
    assert!(write_frame(&mut Vec::new(), &frame, 7).is_err());
    let read = |limit| {
        read_frame::<serde_json::Value>(
            &mut Cursor::new(b"{\"a\":1}\n".to_vec()),
            limit,
            &mut Vec::new(),
        )
    };
    assert_eq!(read(8).unwrap(), Some(frame));
    assert_eq!(read(7).unwrap_err().kind(), ErrorKind::FileTooLarge);
}

# Recorded conversations

Each folder is one conversation. `cargo test -p agentcore recorded` replays every one through the
engine with no network: a local server sends the `reply-N.sse` files byte for byte, in order, and
the engine's events must equal `expected.jsonl`.

- `case.json`: what it shows (`about`), whose wire format it follows (`style`), the files in the
  working folder, the prompts, and the answers to the engine's questions, in order.
- `reply-N.sse`: the body of the model server's Nth reply.
- `expected.jsonl`: the engine's events, one per line (from each prompt's `turn_start` on; `ready`, `response`, `session` and
  `turn_start` left out).

The replies in this folder are written by hand after each server's wire format; none is a capture
from a live server yet. To add a capture, save the response bodies as `reply-N.sse`, write
`case.json`, run `BATE_RECORD=1 cargo test -p agentcore recorded`, then read the new
`expected.jsonl` before committing it: the test only proves the engine still does the same.

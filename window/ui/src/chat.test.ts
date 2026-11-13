import { test } from "node:test";
import assert from "node:assert/strict";
import { add, build, type Entry } from "./chat.ts";

test("a transcript becomes the chat the user reads", () => {
  const entries: Entry[] = [
    { type: "session", session: "s1" },
    { type: "prompt", nonce: "n1", text: "hi" },
    { type: "turn_start", nonce: "n1" },
    { type: "thinking", delta: "let me " },
    { type: "thinking", delta: "look" },
    { type: "tool_start", call: "c1", tool: "read", args: { path: "a.txt" } },
    { type: "tool_start", call: "c2", tool: "run", args: { command: "ls" } },
    { type: "tool_end", call: "c1", output: "A", isError: false },
    { type: "text", delta: "Hel" },
    { type: "text", delta: "lo" },
    { type: "turn_end", reason: "done" },
    { type: "prompt", nonce: "n2", text: "again" },
    { type: "text", delta: "par" },
    { type: "turn_end", reason: "error", error: "the engine stopped" },
  ];
  const rows = entries.map((entry, i) => ({ seq: i + 1, entry }));
  const chat = build(rows);
  // Live, one entry at a time, it comes out the same.
  assert.deepEqual(rows.reduce(add, []), chat);
  assert.deepEqual(chat, [
    { id: 2, kind: "user", text: "hi", nonce: "n1" },
    { id: 4, kind: "thinking", text: "let me look" },
    { id: 6, kind: "tool", call: "c1", tool: "read", args: { path: "a.txt" }, output: "A", isError: false },
    { id: 7, kind: "tool", call: "c2", tool: "run", args: { command: "ls" } },
    { id: 9, kind: "reply", text: "Hello" },
    { id: 12, kind: "user", text: "again", nonce: "n2" },
    { id: 13, kind: "reply", text: "par" },
    { id: 14, kind: "ended", reason: "error", error: "the engine stopped" },
  ]);
});

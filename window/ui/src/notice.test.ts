import { test } from "node:test";
import assert from "node:assert/strict";
import { noticeFor } from "./notice.ts";
import type { Question } from "./chat.ts";

const ask = (over: Partial<Question> = {}): Question => ({ question: "q1", title: "Run a command?", options: ["Approve", "Conversation", "Always", "Deny"], kind: "run", target: "ls -l", ...over });

test("a question that may be granted gets the two buttons, and the notification names the bot, the action and the target", () => {
  const n = noticeFor("Analyst", ask());
  assert.equal(n.actions, true);
  assert.equal(n.body, "Analyst: Run a command?\nls -l");
});

test("each kind that is asked about once has buttons; a kind asked about every time, or none, opens the window", () => {
  for (const kind of ["read", "write", "run", "connector", "apple_read"]) assert.equal(noticeFor("A", ask({ kind })).actions, true, kind);
  for (const kind of ["apple_change", undefined, "something_new"]) assert.equal(noticeFor("A", ask({ kind })).actions, false, String(kind));
});

test("a question without both plain choices has no buttons", () => {
  assert.equal(noticeFor("A", ask({ options: ["Approve"] })).actions, false);
  assert.equal(noticeFor("A", ask({ options: ["Deny", "Maybe"] })).actions, false);
});

test("hidden or direction-changing characters anywhere in what it shows leave out the buttons and the text", () => {
  for (const over of [{ target: "rm\u200B -rf x" }, { title: "Run\u202E?" }, { folder: "/a\u2066b" }, { askedBy: ["Bo\u200Db"] }]) {
    const n = noticeFor("A", ask(over));
    assert.equal(n.actions, false);
    assert.doesNotMatch(n.body, /rm|Run|\/a|Bo/);
  }
  assert.equal(noticeFor("Bo\u200Eb", ask()).actions, false);
});

test("a long target is cut by characters, the notification says so and has no buttons; a short one is whole", () => {
  const long = noticeFor("A", ask({ target: "x".repeat(500) }));
  assert.match(long.body, /^A: Run a command\?\nx{160}… \(cut: open Bate to read all of it\)$/);
  assert.equal(long.actions, false);
  const whole = noticeFor("A", ask({ target: "y".repeat(160) }));
  assert.equal(whole.body.endsWith("y".repeat(160)), true);
  assert.equal(whole.actions, true);
  // What the notification cannot show as it is has no buttons either: more than one line, a long title.
  assert.equal(noticeFor("A", ask({ target: "ls\nrm -rf x" })).actions, false);
  assert.equal(noticeFor("A", ask({ title: "t".repeat(161) })).actions, false);
  // A character outside the basic plane is one character, never half of one.
  const emoji = noticeFor("A", ask({ target: "😀".repeat(161) }));
  assert.match(emoji.body, /\n(😀){160}… \(cut/);
});

test("line breaks in a target are shown as spaces, so a notification is one line of it", () => {
  assert.equal(noticeFor("A", ask({ target: "to: ada\n\n  text: hi" })).body, "A: Run a command?\nto: ada text: hi");
});

import { test } from "node:test";
import assert from "node:assert/strict";
import { ago, answered, grouped, said, sectionOf, type AttentionItem } from "./attention.ts";

const item = (over: Partial<AttentionItem>): AttentionItem => ({
  bot: "b", conversation: "c", title: "Title", atMs: 0, busy: false, questions: [], line: "", ended: null, error: null, ...over,
});
const question = { question: "q1", title: "Run a command?", options: ["Approve", "Deny"] };

test("each state is in its own group", () => {
  assert.equal(sectionOf(item({ busy: true, questions: [question] })), "needs");
  assert.equal(sectionOf(item({ busy: true })), "working");
  assert.equal(sectionOf(item({ ended: "done" })), "done");
  assert.equal(sectionOf(item({ ended: "error", error: "no network" })), "failed");
  assert.equal(sectionOf(item({ ended: "limit" })), "failed");
  assert.equal(sectionOf(item({})), undefined);
});

test("a conversation with a question and a running turn is in Needs you only", () => {
  const groups = grouped([item({ busy: true, questions: [question], ended: "done" })]);
  assert.deepEqual(groups.map((g) => g.id), ["needs"]);
});

test("answering moves the card from Needs you to Working, and a new turn over an unseen one is Working too", () => {
  const asked = item({ busy: true, questions: [question] });
  assert.deepEqual(grouped([asked]).map((g) => g.id), ["needs"]);
  assert.deepEqual(grouped(answered([asked], "c", "q1")).map((g) => g.id), ["working"]);
  assert.deepEqual(grouped(answered([asked], "other", "q1")).map((g) => g.id), ["needs"]);
  assert.deepEqual(grouped([item({ busy: true, ended: "done" })]).map((g) => g.id), ["working"]);
});

test("groups come in order, an empty one is left out, each with its newest first, and idle ones are not shown", () => {
  const groups = grouped([
    item({ conversation: "failed", ended: "error", atMs: 5 }),
    item({ conversation: "old", ended: "done", atMs: 1 }),
    item({ conversation: "new", ended: "done", atMs: 9 }),
    item({ conversation: "asking", questions: [question], busy: true, atMs: 3 }),
    item({ conversation: "idle" }),
  ]);
  assert.deepEqual(groups.map((g) => [g.id, g.items.map((i) => i.conversation)]), [
    ["needs", ["asking"]],
    ["done", ["new", "old"]],
    ["failed", ["failed"]],
  ]);
  assert.deepEqual(grouped([]), []);
});

test("a card says why it failed, or the last line", () => {
  assert.equal(said(item({ ended: "error", error: "model refused" })), "model refused");
  assert.equal(said(item({ ended: "limit" })), "Stopped at its step limit.");
  assert.equal(said(item({ ended: "error" })), "Stopped with an error.");
  assert.equal(said(item({ ended: "done", line: "All set." })), "All set.");
  assert.equal(said(item({ busy: true })), "Working…");
});

test("how long ago is short", () => {
  const now = 10 * 86_400_000;
  assert.equal(ago(now - 20_000, now), "Just now");
  assert.equal(ago(now - 5 * 60_000, now), "5 min ago");
  assert.equal(ago(now - 3 * 3_600_000, now), "3 h ago");
  assert.equal(ago(now - 86_400_000, now), "1 day ago");
  assert.equal(ago(now - 4 * 86_400_000, now), "4 days ago");
});

import { test } from "node:test";
import assert from "node:assert/strict";
import { checkFields, checkValue } from "./check.ts";
import { add, aim, build, changedFiles, checkText, describe, fold, given, isLong, planSteps, type Entry, type Part } from "./chat.ts";

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

test("a write that kept the earlier file offers Undo, and an undone entry marks only its own call", () => {
  const entries: Entry[] = [
    { type: "tool_start", call: "c1", tool: "write", args: { path: "a.txt" } },
    { type: "tool_end", call: "c1", output: "wrote", isError: false, undo: "k1" },
    { type: "tool_start", call: "c2", tool: "write", args: { path: "b.txt" } },
    { type: "tool_end", call: "c2", output: "wrote", isError: false, undo: "k2" },
    { type: "tool_start", call: "c3", tool: "write", args: { path: "big.txt" } },
    { type: "tool_end", call: "c3", output: "wrote", isError: false },
    { type: "undone", undo: "k2" },
  ];
  const rows = entries.map((entry, i) => ({ seq: i + 1, entry }));
  const calls = build(rows).flatMap((i) => (i.kind === "tool" ? [[i.undo, i.undone]] : []));
  // The same live, one entry at a time, and after a reload that brings every entry at once.
  assert.deepEqual(rows.reduce(add, []), build(rows));
  assert.deepEqual(calls, [["k1", undefined], ["k2", true], [undefined, undefined]]);
});

test("a fork keeps what came after, for reading only", () => {
  const entries: Entry[] = [
    { type: "prompt", nonce: "n1", text: "one" },
    { type: "text", delta: "1" },
    { type: "prompt", nonce: "n2", text: "two" },
    { type: "text", delta: "2" },
    { type: "prompt", nonce: "n3", text: "three" },
    { type: "fork", nonce: "n2" },
    { type: "prompt", nonce: "n4", text: "two, again" },
  ];
  const rows = entries.map((entry, i) => ({ seq: i + 1, entry }));
  const chat = build(rows);
  assert.deepEqual(rows.reduce(add, []), chat);
  assert.deepEqual(chat, [
    { id: 1, kind: "user", text: "one", nonce: "n1" },
    { id: 2, kind: "reply", text: "1" },
    { id: 3, kind: "user", text: "two", nonce: "n2", past: true },
    { id: 4, kind: "reply", text: "2" },
    { id: 5, kind: "user", text: "three", nonce: "n3", past: true },
    { id: 6, kind: "forked" },
    { id: 7, kind: "user", text: "two, again", nonce: "n4" },
  ]);
});

test("a turn shows one plan, the newest, and a sub-agent's beside it", () => {
  const plan = (call: string, status: string): Entry => ({
    type: "tool_start", call, tool: "plan", args: { steps: [{ step: "Fix it", status }] },
  });
  const entries: Entry[] = [
    { type: "prompt", nonce: "n1", text: "fix it" },
    plan("c1", "pending"),
    { type: "tool_end", call: "c1", output: "Plan updated.", isError: false },
    plan("agent1/s1", "in_progress"),
    plan("c2", "done"),
    { type: "prompt", nonce: "n2", text: "and again" },
    plan("c3", "pending"),
  ];
  const rows = entries.map((entry, i) => ({ seq: i + 1, entry }));
  const chat = build(rows);
  assert.deepEqual(rows.reduce(add, []), chat);
  assert.deepEqual(chat.map((item) => (item.kind === "tool" ? item.call : item.kind)), [
    "user", "agent1/s1", "c2", "user", "c3",
  ]);
});

test("only a well-formed plan is shown as a checklist", () => {
  const steps = [{ step: "Fix it", status: "done" }];
  assert.deepEqual(planSteps({ steps }), steps);
  for (const args of [null, {}, { steps: [] }, { steps: "Fix it" }, { steps: [{ step: "Fix it", status: "started" }] }, { steps: [null] }])
    assert.equal(planSteps(args), undefined);
});

test("a steer stays in the chat where the bot took it and splits the reply around it", () => {
  const entries: Entry[] = [
    { type: "prompt", nonce: "n1", text: "go" },
    { type: "text", delta: "Walking." },
    { type: "steered", text: "go left" },
    { type: "text", delta: "Left." },
    { type: "turn_end", reason: "done" },
  ];
  const rows = entries.map((entry, i) => ({ seq: i + 1, entry }));
  assert.deepEqual(build(rows), [
    { id: 1, kind: "user", text: "go", nonce: "n1" },
    { id: 2, kind: "reply", text: "Walking." },
    { id: 3, kind: "steered", text: "go left" },
    { id: 4, kind: "reply", text: "Left." },
  ]);
  assert.deepEqual(rows.reduce(add, []), build(rows));
});

test("a check that found something is its own line between the first answer and the second, not the bot's text", () => {
  const entries: Entry[] = [
    { type: "prompt", nonce: "n1", text: "go" },
    { type: "text", delta: "First." },
    { type: "checked", result: "failed", note: "A file was not read.", by: "m-check" },
    { type: "text", delta: "Second." },
    { type: "checked", result: "passed", note: "", by: "Reviewer" },
    { type: "turn_end", reason: "done" },
  ];
  const rows = entries.map((entry, i) => ({ seq: i + 1, entry }));
  assert.deepEqual(build(rows), [
    { id: 1, kind: "user", text: "go", nonce: "n1" },
    { id: 2, kind: "reply", text: "First." },
    { id: 3, kind: "checked", result: "failed", note: "A file was not read.", by: "m-check" },
    { id: 4, kind: "reply", text: "Second." },
    { id: 5, kind: "checked", result: "passed", note: "", by: "Reviewer" },
  ]);
  assert.deepEqual(rows.reduce(add, []), build(rows));
});

test("each result of a check says plainly what happened", () => {
  assert.equal(checkText("passed", ""), "Checked: nothing to fix.");
  assert.equal(checkText("failed", "No test ran."), "Checked: something to fix. No test ran. The bot is working on it.");
  assert.match(checkText("not_passed", "Still wrong."), /did not pass.*Still wrong\./);
  assert.equal(checkText("not_checked", "the model server answered 401"), "Not checked: the model server answered 401");
});

test("a check says who made it: the model or the bot", () => {
  assert.equal(checkText("passed", "", "Reviewer"), "Checked by Reviewer: nothing to fix.");
  assert.match(checkText("failed", "No test ran.", "m-check"), /^Checked by m-check: something to fix\./);
});

test("the check select holds what the bot has and sends it back whole", () => {
  assert.equal(checkValue({ check: "off", checkModel: "", checkBot: "" }), "off");
  assert.equal(checkValue({ check: "model", checkModel: "gpt:x", checkBot: "" }), "model:gpt:x");
  assert.deepEqual(checkFields("model:gpt:x"), { check: "model", checkModel: "gpt:x" });
  assert.deepEqual(checkFields(checkValue({ check: "bot", checkModel: "", checkBot: "ab12" })), { check: "bot", checkBot: "ab12" });
  assert.deepEqual(checkFields("own"), { check: "own" });
});

test("a retry and a turn that hit its step limit each get a line", () => {
  const entries: Entry[] = [
    { type: "prompt", nonce: "n1", text: "go" },
    { type: "retry", attempt: 1, of: 5, waitMs: 2000, error: "the model server answered 503" },
    { type: "text", delta: "ok" },
    { type: "turn_end", reason: "limit", error: "stopped after 100 tool rounds" },
  ];
  const chat = build(entries.map((entry, i) => ({ seq: i + 1, entry })));
  assert.deepEqual(chat.slice(1), [
    { id: 2, kind: "retrying", attempt: 1, of: 5, waitMs: 2000, error: "the model server answered 503" },
    { id: 3, kind: "reply", text: "ok" },
    { id: 4, kind: "ended", reason: "limit", error: "stopped after 100 tool rounds" },
  ]);
});

const call = (n: number, tool: string, args: unknown, output?: string, isError = false): Entry[] => [
  { type: "tool_start", call: `c${n}`, tool, args },
  ...(output === undefined ? [] : [{ type: "tool_end", call: `c${n}`, output, isError } as Entry]),
];
const chatOf = (entries: Entry[]) => build(entries.map((entry, i) => ({ seq: i + 1, entry })));

test("calls that follow each other, and the thinking among them, fold into one run", () => {
  const shown = fold(chatOf([
    { type: "prompt", nonce: "n1", text: "go" },
    { type: "thinking", delta: "look first" },
    ...call(1, "run", { command: "ls" }, "a"),
    ...call(2, "run", { command: "pwd" }, "/"),
    ...call(3, "read", { path: "a.txt" }, "A"),
    { type: "text", delta: "Done." },
    ...call(4, "run", { command: "ls" }, "a"),
  ]));
  assert.deepEqual(shown.map((s) => s.kind), ["user", "run", "reply", "run"]);
  const run = shown[1];
  assert.ok(run.kind === "run");
  // The run is named by its first part, so it keeps its id while it grows.
  assert.equal(run.id, 2);
  assert.deepEqual(run.parts.map((p) => p.kind), ["thinking", "tool", "tool", "tool"]);
});

test("a plan is a checklist of its own and ends the run before it", () => {
  const plan = { steps: [{ step: "look", status: "in_progress" }] };
  const shown = fold(chatOf([...call(1, "run", { command: "ls" }, "a"), ...call(2, "plan", plan, "Plan updated."), ...call(3, "run", { command: "ls" }, "a")]));
  assert.deepEqual(shown.map((s) => s.kind), ["run", "plan", "run"]);
  // A plan that is not one is a call like any other.
  assert.deepEqual(fold(chatOf(call(1, "plan", { steps: [] }, "no", true))).map((s) => s.kind), ["run"]);
});

const parts = (entries: Entry[]) => fold(chatOf(entries)).flatMap((s) => (s.kind === "run" ? s.parts : [])) as Part[];

test("a run says what happened in the person's words", () => {
  const done = parts([
    ...call(1, "run", { command: "ls" }, "a"),
    ...call(2, "run", { command: "pwd" }, "/"),
    ...call(3, "run", { command: "id" }, "me"),
    ...call(4, "read", { path: "a.txt" }, "A"),
  ]);
  assert.deepEqual(describe(done, false), { text: "Ran 3 commands, read 1 file", running: false, failed: 0 });
  assert.equal(describe(parts([...call(1, "search", { pattern: "x" }, "y"), ...call(2, "write", { path: "a" }, "ok"), ...call(3, "edit", { path: "a" }, "ok")]), false).text, "Made 1 search, wrote 1 file, edited 1 file");
  // An installed agent's kinds, and a connector's tool, are said too.
  assert.equal(describe(parts([...call(1, "execute", {}, "a"), ...call(2, "run", {}, "b"), ...call(3, "notes__find", {}, "c"), ...call(4, "mystery", {}, "d")]), false).text, "Ran 2 commands, used 1 notes tool, used 1 tool");
});

test("a run still going says so; one that is not going does not, however it ended", () => {
  const going = parts([...call(1, "run", { command: "ls" }, "a"), ...call(2, "run", { command: "sleep 9" }), ...call(3, "read", { path: "a" }, "A")]);
  assert.deepEqual(describe(going, true), { text: "Running 2 commands, read 1 file", running: true, failed: 0 });
  // The same calls in a chat whose bot stopped: no spinner for a call that will never end.
  assert.equal(describe(going, false).running, false);
  assert.equal(describe(parts([{ type: "thinking", delta: "hm" }]), true).text, "Thinking…");
  assert.equal(describe(parts([{ type: "thinking", delta: "hm" }]), false).text, "Thought");
});

test("a failed call shows as failed in the line, whether or not the run is open", () => {
  const run = parts([...call(1, "run", { command: "ls" }, "a"), ...call(2, "run", { command: "nope" }, "not found", true), ...call(3, "run", { command: "bad" }, "no", true)]);
  assert.deepEqual(describe(run, false), { text: "Ran 3 commands", running: false, failed: 2 });
});

test("a row is named by what the call acted on, not by the tool", () => {
  const first = (entries: Entry[]) => aim(parts(entries)[0] as Extract<Part, { kind: "tool" }>);
  assert.equal(first(call(1, "run", { command: "ls -la" }, "")), "ls -la");
  assert.equal(first(call(1, "read", { path: "src/a.ts" }, "")), "src/a.ts");
  assert.equal(first(call(1, "search", { pattern: "TODO", path: "src" }, "")), "TODO in src");
  assert.equal(first(call(1, "search", { pattern: "TODO" }, "")), "TODO");
  // An installed agent sends a title, the raw input and where it acted.
  assert.equal(first(call(1, "execute", { title: "List files", rawInput: { command: "ls" } }, "")), "ls");
  assert.equal(first(call(1, "read", { title: "Read a file", locations: [{ path: "/x/y.txt" }] }, "")), "/x/y.txt");
  assert.equal(first(call(1, "browser", { action: "open", url: "https://example.com" }, "")), "https://example.com");
  assert.equal(first(call(1, "mystery", null, "")), "mystery");
  // The provider's own search folds with the other calls and says what was looked for.
  assert.equal(first(call(1, "web_search", { query: "newest stable rust" }, "")), "Searched the web: newest stable rust");
  assert.equal(first(call(1, "web_search", { query: "" }, "")), "Searched the web");
  assert.equal(describe(parts([...call(1, "run", { command: "ls" }, "a"), ...call(2, "web_search", { query: "x" }, "")]), false).text, "Ran 1 command, made 1 web search");
});

test("the files a run changed are those of its write and edit calls that worked, each once", () => {
  const run = parts([
    ...call(1, "write", { path: "a.txt" }, "wrote"),
    ...call(2, "edit", { path: "a.txt" }, "edited"),
    ...call(3, "write", { path: "b.txt" }, "no", true),
    ...call(4, "write", { path: "c.txt" }),
    ...call(5, "read", { path: "d.txt" }, "D"),
    ...call(6, "run", { command: "touch e.txt" }, ""),
    ...call(7, "edit", { title: "Edit", locations: [{ path: "/x/f.txt" }] }, "ok"),
  ]);
  assert.deepEqual(changedFiles(run), ["a.txt", "/x/f.txt"]);
});

test("a call given one text shows that text; anything else is shown as JSON", () => {
  assert.equal(given({ command: "ls -la" }), "ls -la");
  assert.equal(given({ path: "a.txt", content: "x" }), '{\n  "path": "a.txt",\n  "content": "x"\n}');
  assert.equal(given({ limit: 3 }), '{\n  "limit": 3\n}');
  assert.equal(given(["ls"]), '[\n  "ls"\n]');
  assert.equal(given(null), "null");
});

test("a message is long past 8 lines or 600 characters", () => {
  assert.equal(isLong("a\n".repeat(7) + "a"), false);
  assert.equal(isLong("a\n".repeat(8) + "a"), true);
  assert.equal(isLong("a".repeat(600)), false);
  assert.equal(isLong("a".repeat(601)), true);
});

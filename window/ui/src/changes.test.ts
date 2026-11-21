import assert from "node:assert/strict";
import { test } from "node:test";
import { commentOn, countLines, covers, message, parseDiff, where } from "./changes.ts";

const DIFF = [
  "diff --git a/a.ts b/a.ts",
  "index 1..2 100644",
  "--- a/a.ts",
  "+++ b/a.ts",
  "@@ -3,4 +3,4 @@ function f() {",
  " keep",
  "-old one",
  "-old two",
  "+new one",
  " tail",
  "\\ No newline at end of file",
  "@@ -20,2 +20,3 @@",
  " x",
  "+y",
  " z",
  "",
].join("\n");

test("a diff's hunks give each line its number in the old and the new file", () => {
  const rows = parseDiff(DIFF);
  assert.deepEqual(
    rows.map((r) => (r.kind === "hunk" ? "@@" : `${r.kind} ${r.old ?? "-"} ${r.now ?? "-"}`)),
    ["@@", "context 3 3", "del 4 -", "del 5 -", "add - 4", "context 6 5", "@@", "context 20 20", "add - 21", "context 21 22"],
  );
  assert.deepEqual(countLines(rows), { added: 2, removed: 2 });
});

test("a file with no hunk, or text that is not a diff, has no rows", () => {
  assert.deepEqual(parseDiff(""), []);
  assert.deepEqual(parseDiff("diff --git a/x b/x\nBinary files a/x and b/x differ\n"), []);
});

test("a comment holds the lines it is about, with their signs, and numbers of the new file", () => {
  const rows = parseDiff(DIFF);
  const c = commentOn("src/a.ts", rows, 4, 2, "Why?")!;
  assert.deepEqual([c.path, c.side, c.from, c.to, c.text], ["src/a.ts", "new", 4, 4, "Why?"]);
  assert.equal(c.quote, "-old one\n-old two\n+new one");
  assert.equal(where(c), "line 4");
  const wide = commentOn("src/a.ts", rows, 1, 5, "x")!;
  assert.equal(where(wide), "lines 3-5");
});

test("a comment on removed lines only is numbered by the old file", () => {
  const c = commentOn("a.ts", parseDiff(DIFF), 2, 3, "Keep these")!;
  assert.deepEqual([c.side, c.from, c.to], ["old", 4, 5]);
  assert.equal(where(c), "removed lines 4-5");
});

test("rows that are no line give no comment", () => {
  assert.equal(commentOn("a.ts", parseDiff(DIFF), 0, 0, "x"), undefined);
});

test("a comment marks its own lines in its own file's diff", () => {
  const rows = parseDiff(DIFF);
  const c = commentOn("a.ts", rows, 2, 3, "x")!;
  assert.deepEqual(rows.map((r) => covers(c, r)), [false, false, true, true, false, false, false, false, false, false]);
});

test("the message lists each comment with its file, its line numbers and its lines", () => {
  const rows = parseDiff(DIFF);
  const text = message([
    commentOn("src/a.ts", rows, 4, 4, "  Use a name here.  ")!,
    commentOn("src/b.ts", rows, 7, 8, "Why add this?")!,
  ]);
  assert.equal(
    text,
    [
      "Comments on your changes (2 comments):",
      "",
      "1. src/a.ts, line 4:",
      "```diff",
      "+new one",
      "```",
      "Use a name here.",
      "",
      "2. src/b.ts, lines 20-21:",
      "```diff",
      " x",
      "+y",
      "```",
      "Why add this?",
    ].join("\n"),
  );
});

test("lines with backticks cannot close the code fence", () => {
  const rows = parseDiff("@@ -1 +1 @@\n-a\n+```js\n");
  const text = message([commentOn("n.md", rows, 1, 2, "Fence")!]);
  assert.match(text, /^````diff\n-a\n\+```js\n````\nFence$/m);
  assert.match(message([commentOn("n.md", rows, 1, 1, "One")!]), /^Comments on your changes \(1 comment\):/);
});

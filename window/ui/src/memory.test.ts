import { test } from "node:test";
import assert from "node:assert/strict";
import { firstLine, keep, kept, size, unsaved } from "./memory.ts";

test("sizes read in bytes, then KiB", () => {
  assert.equal(size(0), "0 B");
  assert.equal(size(1023), "1023 B");
  assert.equal(size(1536), "1.5 KiB");
});

test("a note is titled by its first line with words", () => {
  assert.equal(firstLine("# Build\n\nrun make"), "Build");
  assert.equal(firstLine("\n\n  Prefers tabs  \nmore"), "Prefers tabs");
  assert.equal(firstLine("#\n\n"), "");
  assert.equal(firstLine(""), "");
});

test("the editor holds unsaved text only when it differs from the service's", () => {
  assert.equal(unsaved({ open: null, name: "", text: "", saved: "" }), false);
  assert.equal(unsaved({ open: "a.md", name: "a.md", text: "x", saved: "x" }), false);
  assert.equal(unsaved({ open: "a.md", name: "a.md", text: "xy", saved: "x" }), true);
  assert.equal(unsaved({ open: "", name: "", text: "", saved: "" }), false);
  assert.equal(unsaved({ open: "", name: "a.md", text: "", saved: "" }), true);
});

test("unsaved text is kept for the screen that comes back, and let go when saved", () => {
  const dirty = { open: "a.md", name: "a.md", text: "xy", saved: "x" };
  keep("bot:b1", dirty);
  assert.deepEqual(kept("bot:b1"), dirty);
  assert.equal(kept("bot:b2"), undefined);
  keep("bot:b1", { ...dirty, saved: "xy" });
  assert.equal(kept("bot:b1"), undefined);
});

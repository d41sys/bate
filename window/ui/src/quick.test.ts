import { test } from "node:test";
import assert from "node:assert/strict";
import { build, type Entry } from "./chat.ts";
import { answerOf, display, message, shortcutOf, usedFirst, used } from "./quick.ts";
import type { Bot } from "./types.ts";

const press = (code: string, held: Partial<Record<"ctrlKey" | "altKey" | "shiftKey" | "metaKey", boolean>>) =>
  shortcutOf({ code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...held });

test("a key press is a shortcut only with Ctrl, Alt or Cmd and a key the Mac takes for one", () => {
  assert.equal(press("Space", { ctrlKey: true, altKey: true }), "Ctrl+Alt+Space");
  assert.equal(press("KeyK", { metaKey: true, shiftKey: true }), "Shift+Cmd+K");
  assert.equal(press("Digit7", { altKey: true }), "Alt+7");
  assert.equal(press("F12", { ctrlKey: true }), "Ctrl+F12");
  // A plain letter, Shift alone, a lone modifier, and keys outside the list are no shortcut.
  assert.equal(press("KeyK", {}), undefined);
  assert.equal(press("KeyK", { shiftKey: true }), undefined);
  assert.equal(press("MetaLeft", { metaKey: true }), undefined);
  assert.equal(press("Enter", { metaKey: true }), undefined);
  assert.equal(press("F13", { metaKey: true }), undefined);
  assert.equal(display("Ctrl+Alt+Space"), "⌃⌥Space");
  assert.equal(display("Shift+Cmd+K"), "⇧⌘K");
});

test("the question goes first, then the quoted text, then the files; no question sends nothing", () => {
  assert.equal(message("  What is this?  ", undefined, []), "What is this?");
  assert.equal(message("What is this?", "one\ntwo", ["attachments/shot.png"]), "What is this?\n\nSelected text:\n> one\n> two\n\nAttached:\n- attachments/shot.png");
  assert.equal(message("Why?", "   ", []), "Why?");
  assert.equal(message("   ", "quoted", ["attachments/shot.png"]), "");
});

test("the bots used last come first, and the rest keep their order", () => {
  const bots = ["a", "b", "c", "d"].map((id) => ({ id }) as Bot);
  assert.deepEqual(usedFirst(bots, []).map((b) => b.id), ["a", "b", "c", "d"]);
  // A bot that is gone is skipped.
  assert.deepEqual(usedFirst(bots, ["c", "gone", "a"]).map((b) => b.id), ["c", "a", "b", "d"]);
  assert.deepEqual(used(["c", "a"], "a"), ["a", "c"]);
  assert.deepEqual(used(["c", "a"], "b"), ["b", "c", "a"]);
});

test("the answer is what the bot said since the last question, and how its turn ended", () => {
  const rows = (entries: Entry[]) => build(entries.map((entry, i) => ({ seq: i + 1, entry })));
  const asked: Entry[] = [{ type: "prompt", nonce: "n1", text: "hi" }, { type: "turn_start", nonce: "n1" }];
  assert.deepEqual(answerOf(rows(asked)), { text: "", ended: undefined });
  const streaming = rows([...asked, { type: "text", delta: "Hel" }, { type: "tool_start", call: "c", tool: "run", args: {} }, { type: "tool_end", call: "c", output: "", isError: false }, { type: "text", delta: "lo" }]);
  assert.deepEqual(answerOf(streaming), { text: "Hel\n\nlo", ended: undefined });
  assert.deepEqual(answerOf(rows([{ type: "prompt", nonce: "n0", text: "old" }, { type: "text", delta: "older" }, { type: "turn_end", reason: "done" }, ...asked, { type: "text", delta: "new" }, { type: "turn_end", reason: "error", error: "no model" }])), {
    text: "new",
    ended: { reason: "error", error: "no model" },
  });
});

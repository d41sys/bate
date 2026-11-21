import { test } from "node:test";
import assert from "node:assert/strict";
import { hasHidden, reveal } from "./reveal.ts";

const zero = "zero-width or joining character";
const order = "character that changes the reading order";
const space = "space that is not an ordinary space";
const control = "control character";

test("each character a person cannot see, or that reorders text, is marked; the rest is left as it is", () => {
  const table: [input: string, pieces: { text: string; hidden?: string }[]][] = [
    ["ls -la", [{ text: "ls -la" }]],
    ["", []],
    // Newline and tab are how a command is laid out.
    ["a\n\tb", [{ text: "a\n\tb" }]],
    // Zero-width and joiners.
    ["rm\u200B -rf", [{ text: "rm" }, { text: "U+200B", hidden: zero }, { text: " -rf" }]],
    ["a\u200Cb\u200Dc", [{ text: "a" }, { text: "U+200C", hidden: zero }, { text: "b" }, { text: "U+200D", hidden: zero }, { text: "c" }]],
    ["x\u2060\uFEFF", [{ text: "x" }, { text: "U+2060", hidden: zero }, { text: "U+FEFF", hidden: zero }]],
    // Reading order: a right-to-left override can make `txt` look like the end of the name.
    ["evil\u202Etxt.sh", [{ text: "evil" }, { text: "U+202E", hidden: order }, { text: "txt.sh" }]],
    ["a\u2066b\u2069", [{ text: "a" }, { text: "U+2066", hidden: order }, { text: "b" }, { text: "U+2069", hidden: order }]],
    ["\u200Fx\u061C", [{ text: "U+200F", hidden: order }, { text: "x" }, { text: "U+061C", hidden: order }]],
    // Controls, but not newline and tab.
    ["a\u0000b\rc\u001Bd\u007Fe\u0085", [{ text: "a" }, { text: "U+0000", hidden: control }, { text: "b" }, { text: "U+000D", hidden: control }, { text: "c" }, { text: "U+001B", hidden: control }, { text: "d" }, { text: "U+007F", hidden: control }, { text: "e" }, { text: "U+0085", hidden: control }]],
    // Spaces that are not the ordinary one.
    ["a\u00A0b\u2003c\u3000d", [{ text: "a" }, { text: "U+00A0", hidden: space }, { text: "b" }, { text: "U+2003", hidden: space }, { text: "c" }, { text: "U+3000", hidden: space }, { text: "d" }]],
    // The tag block, which can carry a whole message that nobody sees.
    ["ok\u{E0041}\u{E007F}", [{ text: "ok" }, { text: "U+E0041", hidden: "tag character" }, { text: "U+E007F", hidden: "tag character" }]],
    // A visible letter that only looks like another is not marked.
    ["l\u0455 /tmp", [{ text: "l\u0455 /tmp" }]],
    // Text that spells a mark is plain text: only the real character gets one.
    ["U+200B", [{ text: "U+200B" }]],
    ["😀 é 日本", [{ text: "😀 é 日本" }]],
  ];
  for (const [input, pieces] of table) assert.deepEqual(reveal(input), pieces, JSON.stringify(input));
});

test("the plain pieces are the text without what was marked, and nothing else changes", () => {
  const input = "a\u200Bb\u202Ec\u00A0d";
  assert.equal(reveal(input).filter((p) => !p.hidden).map((p) => p.text).join(""), "abcd");
  assert.equal(input.length, 7);
});

test("a warning is due when any of the texts holds a marked character", () => {
  assert.equal(hasHidden("ls", undefined, "x\u200By"), true);
  assert.equal(hasHidden("ls", undefined, "a\nb\tc"), false);
  assert.equal(hasHidden(), false);
});

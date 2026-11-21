import assert from "node:assert/strict";
import { test } from "node:test";
import { SHOWN, keyOf, remember } from "./recent.ts";

test("the latest pick comes first and is not repeated", () => {
  assert.deepEqual(remember(["a", "b", "c"], "b"), ["b", "a", "c"]);
});

test("only the last few picks are kept", () => {
  const keys = Array.from({ length: SHOWN }, (_, i) => `k${i}`);
  assert.deepEqual(remember(keys, "new"), ["new", ...keys.slice(0, SHOWN - 1)]);
});

test("a key tells a group and a label apart", () => {
  assert.notEqual(keyOf({ group: "Bots", label: "Daisy" }), keyOf({ group: "Projects", label: "Daisy" }));
});

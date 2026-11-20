import assert from "node:assert/strict";
import { test } from "node:test";
import { hostLines } from "./network.ts";

test("hosts are read one per line, or split at commas and spaces", () => {
  assert.deepEqual(hostLines("a.com\n  *.b.com:8080 ,c.com\n\n"), ["a.com", "*.b.com:8080", "c.com"]);
  assert.deepEqual(hostLines(""), []);
});

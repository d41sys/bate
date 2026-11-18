import { test } from "node:test";
import assert from "node:assert/strict";
import { size } from "./memory.ts";

test("sizes read in bytes, then KiB", () => {
  assert.equal(size(0), "0 B");
  assert.equal(size(1023), "1023 B");
  assert.equal(size(1536), "1.5 KiB");
});

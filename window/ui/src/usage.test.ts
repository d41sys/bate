import assert from "node:assert/strict";
import { test } from "node:test";
import { tokenCount, usedText } from "./usage.ts";

test("a count adds input and output, and says what it is made of", () => {
  assert.equal(tokenCount({ input: 1000, cached: 400, output: 234 }), "1,234 tokens");
  assert.equal(tokenCount({ input: 1, cached: 0, output: 0 }), "1 token");
  assert.deepEqual(usedText({ input: 1000, cached: 400, output: 234, unreported: 0 }), { value: "1,234 tokens", hint: "Input 1,000 (400 from the cache), output 234." });
  assert.equal(usedText({ input: 5, cached: 0, output: 5, unreported: 0 }).hint, "Input 5, output 5.");
});

test("a period where the server reported nothing is not counted, and a mixed one says how many requests are missing", () => {
  assert.equal(usedText({ input: 0, cached: 0, output: 0, unreported: 3 }).value, "Not counted");
  assert.equal(usedText({ input: 0, cached: 0, output: 0, unreported: 0 }).value, "0 tokens");
  assert.match(usedText({ input: 10, cached: 0, output: 5, unreported: 2 }).hint, /2 requests were not reported and are not in it\.$/);
  assert.match(usedText({ input: 10, cached: 0, output: 5, unreported: 1 }).hint, /1 request was not reported and is not in it\.$/);
});

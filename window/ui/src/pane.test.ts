import assert from "node:assert/strict";
import { test } from "node:test";
import { MIN_CHAT, MIN_PANE, SIDEBAR, placePane } from "./pane.ts";

test("the pane keeps the width it was dragged to when there is room", () => {
  assert.deepEqual(placePane(400, 1400), { width: 400, overlay: false });
});

test("beside the conversation, the pane never leaves it under its minimum", () => {
  const { width, overlay } = placePane(2000, 1000);
  assert.equal(overlay, false);
  assert.equal(1000 - SIDEBAR - width, MIN_CHAT);
});

test("the pane is never narrower than its minimum", () => {
  assert.equal(placePane(10, 1400).width, MIN_PANE);
});

test("in a narrow window the pane floats over the conversation", () => {
  const { width, overlay } = placePane(500, 700);
  assert.equal(overlay, true);
  assert.ok(width <= 700 * 0.9);
});

import { readFileSync } from "node:fs";
import { test } from "node:test";
import assert from "node:assert/strict";
import { PART, clampOffset, nextOffset, prevOffset, range, visibleNodes } from "./surface.ts";
import type { SurfacePart } from "./types.ts";

const code = (offset: number, n: number, total: number): SurfacePart => ({
  kind: "code",
  language: "",
  lines: Array(n).fill("x"),
  offset,
  total,
  actions: [],
});

test("a part says where it is in the whole and where the others are", () => {
  const p = code(100, 100, 250);
  assert.equal(range(p), "101 to 200 of 250");
  assert.equal(nextOffset(p), 200);
  assert.equal(prevOffset(p), 0);
  assert.equal(nextOffset(code(200, 50, 250)), undefined);
  assert.equal(prevOffset(code(0, 100, 250)), undefined);
  assert.equal(range(code(0, 0, 0)), "Nothing here");
});

test("content that shrank under the open part opens at its last part", () => {
  assert.equal(clampOffset(code(300, 0, 120)), PART);
  assert.equal(clampOffset(code(300, 0, 0)), 0);
  assert.equal(clampOffset(code(300, 5, 305)), 300);
});

test("the nodes under a collapsed node are hidden, and only they", () => {
  const nodes = [
    { id: "a", depth: 0 },
    { id: "a1", depth: 1 },
    { id: "a1x", depth: 2 },
    { id: "a2", depth: 1 },
    { id: "b", depth: 0 },
  ];
  const shown = (collapsed: string[]) => visibleNodes(nodes, new Set(collapsed)).map((n) => nodes[n.index].id);
  assert.deepEqual(shown([]), ["a", "a1", "a1x", "a2", "b"]);
  assert.deepEqual(shown(["a"]), ["a", "b"]);
  assert.deepEqual(shown(["a1"]), ["a", "a1", "a2", "b"]);
  assert.deepEqual(visibleNodes(nodes, new Set()).map((n) => n.hasChildren), [true, true, false, false, false]);
});

test("the surface is drawn as text: nothing in it can run markup or script", () => {
  const source = readFileSync(new URL("./Surfaces.tsx", import.meta.url), "utf8");
  for (const way of ["dangerouslySetInnerHTML", "innerHTML", "<iframe", "srcDoc", "Markdown", "eval("]) {
    assert.ok(!source.includes(way), way);
  }
});

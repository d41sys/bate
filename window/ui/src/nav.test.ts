import { test } from "node:test";
import assert from "node:assert/strict";
import { neighbour, sidebar } from "./nav.ts";
import type { Bot, Project } from "./types.ts";

const bot = (id: string, project?: string) => ({ id, project: project ? { id: project, name: project } : undefined }) as Bot;
const project = (id: string) => ({ id, name: id, folder: "", bots: [] }) as Project;
const bots = [bot("a", "p"), bot("b"), bot("c", "p"), bot("d")];

test("a bot is listed under its project, or among the bots in none", () => {
  const lists = sidebar(bots, [project("p"), project("empty")]);
  assert.deepEqual(lists.projects.map((g) => g.bots.map((b) => b.id)), [["a", "c"], []]);
  assert.deepEqual(lists.loose.map((b) => b.id), ["b", "d"]);
});

test("moving a bot goes by the list it is shown in", () => {
  assert.equal(neighbour(bots, "a", 1)?.id, "c");
  assert.equal(neighbour(bots, "d", -1)?.id, "b");
  assert.equal(neighbour(bots, "a", -1), undefined);
  assert.equal(neighbour(bots, "d", 1), undefined);
});

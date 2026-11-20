import { test } from "node:test";
import assert from "node:assert/strict";
import { endsUnseen, neighbour, rowState, sidebar, waitingCount } from "./nav.ts";
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

test("a row shows the most urgent of its states", () => {
  const state = (b: Partial<Bot>, unread = false) => rowState({ ...bot("x"), ...b } as Bot, unread);
  assert.equal(state({ waiting: true, busy: true }, true), "waiting");
  assert.equal(state({ busy: true }, true), "working");
  assert.equal(state({}, true), "unread");
  assert.equal(state({}), "idle");
});

test("the Dock counts the bots that wait for the person", () => {
  assert.equal(waitingCount([{ waiting: true }, { waiting: false }, { waiting: true }] as Bot[]), 2);
});

test("a turn that ends in another chat, or while the window is not looked at, is unread", () => {
  const ended = (bot: string, reason = "done") => ({ type: "transcript", bot, entry: { type: "turn_end", reason } });
  assert.equal(endsUnseen(ended("a"), "b"), true);
  assert.equal(endsUnseen(ended("a"), undefined), true);
  assert.equal(endsUnseen(ended("a"), "a"), false);
  assert.equal(endsUnseen(ended("a", "aborted"), "b"), false);
  assert.equal(endsUnseen({ type: "transcript", bot: "a", entry: { type: "text" } }, "b"), false);
  assert.equal(endsUnseen({ type: "bots_changed" }, "b"), false);
});

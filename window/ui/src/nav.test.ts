import { test } from "node:test";
import assert from "node:assert/strict";
import { endsUnseen, neighbour, rowState, shortcut, shownBots, sidebar, stepBot } from "./nav.ts";
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

test("the next and previous bot follow the sidebar, wrap round, and start from the ends when no bot is open", () => {
  const list = shownBots(bots, [project("p")]);
  assert.deepEqual(list.map((b) => b.id), ["a", "c", "b", "d"]);
  assert.equal(stepBot(list, "c", 1)?.id, "b");
  assert.equal(stepBot(list, "d", 1)?.id, "a");
  assert.equal(stepBot(list, "a", -1)?.id, "d");
  assert.equal(stepBot(list, undefined, 1)?.id, "a");
  assert.equal(stepBot(list, undefined, -1)?.id, "d");
  assert.equal(stepBot([], undefined, 1), undefined);
});

test("each shortcut stands for one action, by the physical key, and none while composing text", () => {
  const key = (code: string, mods: Partial<Record<"metaKey" | "ctrlKey" | "altKey" | "shiftKey" | "isComposing", boolean>> = { metaKey: true }) =>
    shortcut({ code, metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, isComposing: false, ...mods });
  assert.equal(key("Comma"), "settings");
  assert.equal(key("BracketLeft"), "previous-bot");
  assert.equal(key("BracketRight"), "next-bot");
  assert.equal(key("Digit1"), "bot-1");
  assert.equal(key("Digit9"), "bot-9");
  assert.equal(key("Digit0"), undefined);
  assert.equal(key("Tab", { ctrlKey: true }), "next-bot");
  assert.equal(key("Tab", { ctrlKey: true, shiftKey: true }), "previous-bot");
  assert.equal(key("KeyK"), "palette");
  assert.equal(key("KeyF"), "find");
  assert.equal(key("KeyN"), "new-bot");
  assert.equal(key("KeyN", { metaKey: true, shiftKey: true }), "new-window");
  // Not the same chord with another modifier, nor a bare key, nor the other shifted keys.
  assert.equal(key("Comma", { metaKey: true, altKey: true }), undefined);
  assert.equal(key("Comma", { ctrlKey: true }), undefined);
  assert.equal(key("BracketRight", { metaKey: true, shiftKey: true }), undefined);
  assert.equal(key("Tab", {}), undefined);
  assert.equal(key("Comma", { metaKey: true, isComposing: true }), undefined);
  assert.equal(key("Tab", { ctrlKey: true, isComposing: true }), undefined);
});

test("a row shows the most urgent of its states", () => {
  const state = (b: Partial<Bot>, unread = false) => rowState({ ...bot("x"), ...b } as Bot, unread);
  assert.equal(state({ waiting: true, busy: true }, true), "waiting");
  assert.equal(state({ paused: true, busy: true }, true), "paused");
  assert.equal(state({ busy: true }, true), "working");
  assert.equal(state({}, true), "unread");
  assert.equal(state({}), "idle");
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

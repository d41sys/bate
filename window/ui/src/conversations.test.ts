import { test } from "node:test";
import assert from "node:assert/strict";
import { label, listed, when } from "./conversations.ts";
import type { Conversation } from "./types.ts";

const made = (id: string, more: Partial<Conversation> = {}): Conversation => ({
  id, bot: "b", createdAt: 1, touchedAt: 1, title: "", name: "", pinned: false, archived: false, busy: false, ...more,
});

test("a conversation shows the person's name, else its title, else that it has none yet", () => {
  assert.equal(label(made("a", { name: "Mine", title: "Plan" })), "Mine");
  assert.equal(label(made("a", { title: "Plan" })), "Plan");
  assert.equal(label(made("a")), "Untitled");
});

test("a date is Today and the time for today and the day for any other, with the year only when it is not this one", () => {
  const now = new Date(2026, 10, 16, 15, 30).getTime();
  assert.match(when(new Date(2026, 10, 16, 9, 5).getTime(), now), /^Today, .*(9:05|09:05)/);
  assert.doesNotMatch(when(new Date(2026, 10, 2, 9, 5).getTime(), now), /2026/);
  assert.match(when(new Date(2025, 10, 2, 9, 5).getTime(), now), /2025/);
});

test("archived conversations are hidden until asked for, but not the one that is open", () => {
  const all = [made("a"), made("b", { archived: true }), made("c", { archived: true })];
  assert.deepEqual(listed(all, "a", false).map((c) => c.id), ["a"]);
  assert.deepEqual(listed(all, "b", false).map((c) => c.id), ["a", "b"]);
  assert.deepEqual(listed(all, "a", true).map((c) => c.id), ["a", "b", "c"]);
});

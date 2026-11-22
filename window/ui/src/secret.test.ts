import { test } from "node:test";
import assert from "node:assert/strict";
import { answerCommand } from "./secret.ts";
import { noticeFor } from "./notice.ts";
import type { Question } from "./chat.ts";

const ask: Question = { question: "q1", title: "Sign in to the shop", options: ["Deny"], secret: { name: "shop", site: "shop.example" } };

test("a typed secret travels only in answer_secret, and a refusal carries no value", () => {
  assert.deepEqual(answerCommand("b", "c", ask, "", "typed-value"), ["answer_secret", { bot: "b", conversation: "c", question: "q1", value: "typed-value" }]);
  const [type, data] = answerCommand("b", "c", ask, "Deny");
  assert.equal(type, "answer");
  assert.deepEqual(data, { bot: "b", conversation: "c", question: "q1", choice: "Deny" });
  assert.equal("value" in data, false);
});

test("a secret card never gets notification buttons and its notification does not show the bot's own words", () => {
  const n = noticeFor("Analyst", ask);
  assert.equal(n.actions, false);
  assert.equal(n.body, "Analyst asks for a secret for shop.example. Open Bate to give or refuse it.");
  // Even when it claims to be a question that may be allowed.
  assert.equal(noticeFor("Analyst", { ...ask, kind: "run", options: ["Approve", "Deny"] }).actions, false);
});

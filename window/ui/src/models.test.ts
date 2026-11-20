import { test } from "node:test";
import assert from "node:assert/strict";
import { modelChoices } from "./models.ts";

test("the search keeps the listed models that contain what is typed, whatever the case", () => {
  assert.deepEqual(modelChoices(["gpt-6.2-sol", "gpt-6.2-astra", "other"], [], "", "AST"), { models: ["gpt-6.2-astra"], typed: undefined });
  assert.deepEqual(modelChoices(["a", "b"], [], "", "").models, ["a", "b"]);
});

test("a model the server does not list stays in the list while it is the bot's", () => {
  assert.deepEqual(modelChoices(["a"], [], "mine", "").models, ["a", "mine"]);
});

test("a typed name is not a row when the server lists models", () => {
  assert.equal(modelChoices(["a"], [], "", "zzz").typed, undefined);
});

test("with no list, the typed name is a row of its own unless it is already one", () => {
  assert.deepEqual(modelChoices([], ["x-1", "y"], "", " x "), { models: ["x-1"], typed: "x" });
  assert.deepEqual(modelChoices([], ["x-1"], "", "x-1"), { models: ["x-1"], typed: undefined });
  assert.deepEqual(modelChoices([], [], "", ""), { models: [], typed: undefined });
});

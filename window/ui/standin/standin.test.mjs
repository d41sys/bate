// What the stand-in must keep true as the page changes; no browser needed.
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { test } from "node:test";
import { answers } from "./answers.mjs";
import { screens } from "./screens.mjs";

const src = new URL("../src/", import.meta.url);
const files = readdirSync(src)
  .filter((f) => /\.tsx?$/.test(f) && !/\.test\./.test(f))
  .map((f) => ({ name: f, text: readFileSync(new URL(f, src), "utf8") }));
const code = files.map((f) => f.text).join("\n");

test("every command the page sends has an answer in the stand-in", () => {
  const sent = new Map();
  const note = (name, file) => sent.set(name, [...(sent.get(name) ?? []), file]);
  for (const { name, text } of files) {
    // request("x"), request<T>("x"), usePerBot<T>(bots, "x", …), and the pair answerCommand returns.
    for (const m of text.matchAll(/\brequest\b(?:<[^(]*?>)?\(\s*"(\w+)"/g)) note(m[1], name);
    for (const m of text.matchAll(/\busePerBot<[^(]*?>\([^,]+,\s*"(\w+)"/g)) note(m[1], name);
    if (name === "secret.ts") for (const m of text.matchAll(/\["(\w+)", \{/g)) note(m[1], name);
  }
  assert.ok(sent.size > 100, `found only ${sent.size} commands: the patterns of this test no longer match how the page sends one`);
  const have = answers(0);
  const missing = [...sent].filter(([name]) => !(name in have));
  assert.deepEqual(
    missing.map(([name, from]) => `${name} (sent by ${[...new Set(from)].join(", ")})`),
    [],
    "add an answer for each of these to `answers` in window/ui/standin/answers.mjs (PROTOCOL.md gives the command's data)",
  );
});

test("every selector the screen list names is in the page's code", () => {
  const names = new Set();
  for (const screen of screens) {
    assert.ok(/^[a-z][a-z0-9-]*$/.test(screen.name) && !names.has(screen.name), `${screen.name}: the name is not unique or not lowercase words`);
    names.add(screen.name);
    assert.ok(screen.title && screen.parts?.length, `${screen.name}: a title and its main parts are needed`);
    for (const chord of screen.steps.filter((s) => typeof s === "object" && !Array.isArray(s))) assert.ok(code.includes(chord.code), `${screen.name}: no key ${chord.code} in window/ui/src`);
    const selectors = [...screen.steps.filter((s) => typeof s === "string" || Array.isArray(s)).map((s) => [].concat(s)), ...screen.parts.map((p) => [p])];
    for (const [selector, text] of selectors) {
      const where = `${screen.name}: \`${selector}\``;
      for (const [, id] of selector.matchAll(/#([\w-]+)/g)) {
        // An id written out, or built from a prefix and a name (`tab-${t}`, `attention-${g.id}`).
        const written = code.includes(`id="${id}"`) || code.includes(`id: "${id}"`);
        const built = [...code.matchAll(/\bid=\{`([\w-]+)\$\{/g)].some((m) => id.startsWith(m[1]) && id.length > m[1].length);
        assert.ok(written || built, `${where}: no element with the id ${id} in window/ui/src`);
        if (id.startsWith("tab-")) assert.ok(new RegExp(`\\b${id.slice(4)}:`).test(files.find((f) => f.name === "main.tsx").text), `${where}: main.tsx has no tab ${id.slice(4)}`);
      }
      for (const [, cls] of selector.replace(/\[[^\]]*\]/g, "").matchAll(/\.([\w-]+)/g)) {
        assert.ok(new RegExp(`(?<![\\w-])${cls}(?![\\w-])`).test(code), `${where}: no class ${cls} in window/ui/src`);
      }
      for (const [, label] of selector.matchAll(/\[aria-label=['"]([^'"]+)['"]\]/g)) assert.ok(code.includes(label), `${where}: no label "${label}" in window/ui/src`);
      // The words of a click: in the page, or something the stand-in made up (a bot's name, a conversation's title).
      if (text) assert.ok(code.includes(text) || readFileSync(new URL("./answers.mjs", import.meta.url), "utf8").includes(text), `${where}: the text "${text}" is not in window/ui/src or in the stand-in`);
    }
  }
});

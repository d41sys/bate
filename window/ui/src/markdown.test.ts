import { test } from "node:test";
import assert from "node:assert/strict";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { Markdown } from "./Markdown.ts";

const show = (text: string) => renderToStaticMarkup(createElement(Markdown, { text }));

test("a reply's HTML is left out, so nothing in it can run", () => {
  const html = show(
    [
      "<script>window.ran = 1</script>",
      '<img src="x" onerror="window.ran = 2">',
      'Inline <b onmouseover="window.ran = 3">bold</b> and <iframe src="https://evil.example"></iframe>',
      "[one](javascript:alert(1)) [two](data:text/html,x) [three](file:///etc/hosts) [four](elsewhere.html)",
      "![pixel](https://evil.example/seen.png)",
    ].join("\n\n"),
  );
  for (const bad of ["<script", "onerror", "onmouseover", "<iframe", "<img", "javascript:", "data:", "file:", "elsewhere.html", "evil.example"]) {
    assert.ok(!html.includes(bad), `${bad} in ${html}`);
  }
  assert.match(html, /\[pixel\]/);
});

test("Markdown, tables, highlighted code and web links are shown", () => {
  const html = show("**bold** [site](https://example.com)\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n```js\nconst a = 1;\n```");
  assert.match(html, /<strong>bold<\/strong>/);
  assert.match(html, /<a href="https:\/\/example.com"/);
  assert.match(html, /<table>[\s\S]*<td>1<\/td>/);
  assert.match(html, /<code class="hljs language-js"><span class="hljs-keyword">const<\/span>/);
});

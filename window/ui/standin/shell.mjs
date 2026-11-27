// Runs in the page, before the page's own script (`serve.mjs` puts it first): the stand-in for the
// shell's two calls, `window.__TAURI_INTERNALS__.invoke("send" | "recv")`, with the fixed answers of
// `answers.mjs`; then, when the address names a screen of `screens.mjs`, it walks that screen's clicks.
// `window.__standin` says when the screen is ready and what went wrong on the way.
import { answers } from "./answers.mjs";
import { screens } from "./screens.mjs";

const screen = screens.find((s) => s.name === decodeURIComponent(location.pathname.slice(1)));
const usual = answers(Date.now());
const data = { ...usual, ...(typeof screen?.answers === "function" ? screen.answers(usual) : screen?.answers) };
const lines = [JSON.stringify({ type: "hello" })];
const standin = (window.__standin = { ready: false, problems: [] });
let wake;
let lastSend = 0;

Object.assign(window, screen?.globals);

window.__TAURI_INTERNALS__ = {
  invoke: async (command, args) => {
    if (command === "recv") {
      while (!lines.length) await new Promise((resolve) => (wake = resolve));
      return lines.shift();
    }
    if (command === "send") {
      const { id, type, ...fields } = JSON.parse(args.line);
      lastSend = performance.now();
      let reply;
      try {
        if (!(type in data)) throw new Error(`not in the stand-in: ${type}`);
        const answer = data[type];
        reply = { id, type: "response", command: type, success: true, data: typeof answer === "function" ? answer(fields) : answer };
      } catch (e) {
        reply = { id, type: "response", command: type, success: false, error: e.message };
        standin.problems.push(e.message);
      }
      lines.push(JSON.stringify(reply));
      wake?.();
      return;
    }
    // service_note, set_badge, start_dictation and the rest: the shell's own, with nothing to say.
    return null;
  },
};

const until = async (find) => {
  for (let i = 0; i < 400; i++) {
    const found = find();
    if (found) return found;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error("not on the page after 10 s");
};

// A step is a selector, `[selector, text]` (the first match that holds the text), or a key chord.
const walk = async (step) => {
  if (!Array.isArray(step) && typeof step === "object") {
    // The page listens for keys once it has put the bots in the sidebar.
    await until(() => document.querySelector(".sidebar .bot"));
    const { code, ...modifiers } = step;
    dispatchEvent(new KeyboardEvent("keydown", { code, key: code, bubbles: true, ...modifiers }));
    return;
  }
  const [selector, text] = [].concat(step);
  const target = await until(() => [...document.querySelectorAll(selector)].find((e) => !text || e.textContent.includes(text)));
  target.click();
};

if (screen) {
  addEventListener("DOMContentLoaded", async () => {
    try {
      for (const step of screen.steps) await walk(step);
      await until(() => screen.parts.every((part) => document.querySelector(part)));
      // The page asked for what it shows and got it: nothing was asked for a while.
      await until(() => !lines.length && performance.now() - lastSend > 400);
    } catch (e) {
      standin.problems.push(`${screen.name}: ${e.message}`);
    }
    standin.ready = true;
  });
}

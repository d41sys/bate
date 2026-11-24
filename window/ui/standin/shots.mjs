// Saves pictures of the built page, running against the stand-in (made-up data, not the real window),
// as PNG files: every screen of `screens.mjs` (or the ones named) in light and dark, 1200 and 390 wide.
//   npm run build && npm run shots -- <folder> [screen ...]
// Drives Google Chrome headless through its DevTools protocol, with a profile folder of its own that
// is deleted at the end; Chrome is closed by its process id. Exits 1 when a screen showed an error
// line, a step did not find its click, or a control is cut at the window's right edge; the picture
// is saved all the same.
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { serve } from "./serve.mjs";
import { screens } from "./screens.mjs";

const CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const HEIGHT = 800;
const MAX_HEIGHT = 3000;
const looks = ["light", "dark"].flatMap((scheme) => [1200, 390].map((width) => ({ scheme, width })));
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
// Run in the page: the controls and times that end past the window's right edge, unless a part
// below `main` is made to scroll sideways there (the bar of bots at a narrow width, a block of code).
const CUT = `[...document.querySelectorAll("button, a, input, textarea, select, time, .when")].filter((e) => {
  const r = e.getBoundingClientRect();
  if (!r.width || r.right <= innerWidth + 1) return false;
  for (let p = e.parentElement; p && p.tagName !== "MAIN"; p = p.parentElement) if (/auto|scroll/.test(getComputedStyle(p).overflowX)) return false;
  return true;
}).map((e) => (e.getAttribute("aria-label") || e.textContent.trim() || e.tagName).slice(0, 40))`;

const [folder, ...asked] = process.argv.slice(2);
if (!folder) {
  console.error("usage: npm run shots -- <folder> [screen ...]\nscreens: " + screens.map((s) => s.name).join(" "));
  process.exit(2);
}
const unknown = asked.filter((name) => !screens.some((s) => s.name === name));
if (unknown.length) {
  console.error(`no such screen: ${unknown.join(" ")}\nscreens: ${screens.map((s) => s.name).join(" ")}`);
  process.exit(2);
}
if (!existsSync(CHROME)) {
  console.error(`Google Chrome is not at ${CHROME}: the pictures need it.`);
  process.exit(1);
}
const chosen = asked.length ? screens.filter((s) => asked.includes(s.name)) : screens;

const started = Date.now();
await mkdir(folder, { recursive: true });
const server = await serve(0);
const base = `http://127.0.0.1:${server.address().port}`;
const profile = await mkdtemp(join(tmpdir(), "bate-shots-"));
const chrome = spawn(
  CHROME,
  ["--headless=new", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "--no-first-run", "--no-default-browser-check", "--hide-scrollbars",
    "--disable-background-timer-throttling", "--disable-renderer-backgrounding", "--disable-backgrounding-occluded-windows", "about:blank"],
  { stdio: "ignore" },
);
let problems = 0;
try {
  // Chrome says where it listens in a file of its profile.
  let endpoint;
  for (let i = 0; i < 400 && !endpoint; i++) {
    const [port, path] = await readFile(join(profile, "DevToolsActivePort"), "utf8").then((t) => t.split("\n"), () => []);
    if (path) endpoint = `ws://127.0.0.1:${port}${path}`;
    else await sleep(25);
  }
  if (!endpoint) throw new Error("Chrome did not start");
  const socket = new WebSocket(endpoint);
  await new Promise((open, fail) => ((socket.onopen = open), (socket.onerror = () => fail(new Error("no DevTools connection")))));
  let next = 0;
  const waiting = new Map();
  socket.onmessage = ({ data }) => {
    const m = JSON.parse(data);
    const w = waiting.get(m.id);
    if (!w) return;
    waiting.delete(m.id);
    if (m.error) w.reject(new Error(`${w.method}: ${m.error.message}`));
    else w.resolve(m.result);
  };
  const call = (method, params = {}, sessionId) =>
    new Promise((resolve, reject) => {
      const id = ++next;
      waiting.set(id, { resolve, reject, method });
      setTimeout(() => reject(new Error(`${method}: no answer from Chrome in 30 s`)), 30_000).unref();
      socket.send(JSON.stringify({ id, method, params, sessionId }));
    });

  const look = async ({ scheme, width }) => {
    const { targetId } = await call("Target.createTarget", { url: "about:blank" });
    const { sessionId } = await call("Target.attachToTarget", { targetId, flatten: true });
    const page = (method, params) => call(method, params, sessionId);
    const size = (height) => page("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
    // No animation, so a picture is the same each time.
    await page("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }, { name: "prefers-reduced-motion", value: "reduce" }] });
    for (const screen of chosen) {
      await size(HEIGHT);
      await page("Page.navigate", { url: `${base}/${screen.name}` });
      // The page of the screen before it may still be there for a moment: the address tells.
      const state = `({ ready: location.pathname === "/${screen.name}" && !!window.__standin?.ready, problems: window.__standin?.problems ?? [], alert: document.querySelector("[role=alert], .banner.error")?.textContent ?? "" })`;
      let seen;
      for (let i = 0; i < 1200; i++) {
        seen = (await page("Runtime.evaluate", { expression: state, returnByValue: true })).result.value;
        if (seen.ready) break;
        await sleep(25);
      }
      const cut = seen.ready ? (await page("Runtime.evaluate", { expression: CUT, returnByValue: true })).result.value : [];
      const said = [...(seen.ready ? [] : ["never ready"]), ...seen.problems, ...(seen.alert ? [`error line: ${seen.alert}`] : []), ...cut.map((what) => `cut at the right edge: ${what}`)];
      for (const line of said) console.error(`${screen.name} (${scheme}, ${width}): ${line}`);
      problems += said.length;
      // A screen that scrolls is pictured whole (up to MAX_HEIGHT), not cut at the window's foot.
      const tall = (await page("Runtime.evaluate", { expression: "document.querySelector('main')?.scrollHeight ?? 0", returnByValue: true })).result.value;
      await size(Math.min(Math.max(tall, HEIGHT), MAX_HEIGHT));
      // Let the new size settle.
      await sleep(100);
      const { data } = await page("Page.captureScreenshot", { format: "png" });
      await writeFile(join(folder, `${screen.name}-${scheme}-${width}.png`), Buffer.from(data, "base64"));
    }
  };
  await Promise.all(looks.map(look));
  socket.close();
  console.log(`${chosen.length * looks.length} pictures of a stand-in (${chosen.length} screens, ${looks.length} looks) in ${folder}, ${((Date.now() - started) / 1000).toFixed(1)} s`);
} finally {
  if (chrome.exitCode === null && chrome.signalCode === null) {
    const gone = new Promise((done) => chrome.once("exit", done));
    process.kill(chrome.pid);
    await gone;
  }
  await rm(profile, { recursive: true, force: true });
  server.close();
  server.closeAllConnections();
}
process.exit(problems ? 1 : 0);

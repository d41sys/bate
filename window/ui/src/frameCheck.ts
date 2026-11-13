// Built in only by `window/check-frames.sh` (VITE_FRAME_CHECK): a bot's page tries every way it
// could reach the service from its frame; the outcome goes to `frame-check.md` in its memory.
import { listen, request } from "./service.ts";
import { SANDBOX, fileUrl } from "./frame.ts";

const ATTACK = `<!doctype html><script>
const line = JSON.stringify({ id: "x", type: "create_bot", name: "escaped" });
const tried = {};
const attempt = async (name, f) => { try { tried[name] = "ran: " + String(await f()); } catch (e) { tried[name] = "blocked: " + e.name; } };
(async () => {
  await attempt("own internals", () => typeof window.__TAURI_INTERNALS__);
  await attempt("parent internals", () => typeof parent.__TAURI_INTERNALS__.invoke);
  await attempt("parent document", () => parent.document.title);
  await attempt("message handler", () => {
    window.webkit.messageHandlers.ipc.postMessage(JSON.stringify({ cmd: "send", callback: 1, error: 2, payload: { line }, options: {} }));
    return "posted";
  });
  await attempt("ipc fetch", () => fetch("ipc://localhost/send", { method: "POST", headers: { "Content-Type": "application/json", "Tauri-Callback": "1", "Tauri-Error": "2" }, body: JSON.stringify({ line }) }).then((r) => r.status));
  await attempt("web fetch", () => fetch("https://example.com/").then((r) => r.status));
  await attempt("web image", () => new Promise((ok, no) => { const i = new Image(); i.onload = () => ok("loaded"); i.onerror = () => no(new Error("refused")); i.src = "https://example.com/favicon.ico"; }));
  await attempt("top navigation", () => { top.location.href = "https://example.com/"; return "asked"; });
  await attempt("origin", () => origin);
  parent.postMessage({ frameCheck: tried }, "*");
})();
</script>`;

const off = listen(async (frame) => {
  if (frame.type !== "connected") return;
  off();
  const { bot } = await request<{ bot: { id: string } }>("create_bot", { name: "frame-check" });
  const data = btoa(ATTACK);
  const { path } = await request<{ path: string }>("attach", { bot: bot.id, name: "attack.html", data, last: true });
  const tried = await new Promise<Record<string, string>>((done) => {
    addEventListener("message", (e) => e.data?.frameCheck && done(e.data.frameCheck));
    const iframe = document.createElement("iframe");
    iframe.sandbox.value = SANDBOX;
    iframe.src = fileUrl(bot.id, path.slice(path.indexOf("attachments/")));
    document.body.append(iframe);
  });
  // Give anything that did get through time to arrive.
  await new Promise((r) => setTimeout(r, 1500));
  const { bots } = await request<{ bots: { name: string }[] }>("list_bots");
  const escaped = bots.some((b) => b.name === "escaped");
  const reached = ["own internals", "parent internals", "parent document", "web fetch", "web image"].filter(
    (k) => !(tried[k] ?? "").startsWith("blocked") && tried[k] !== "ran: undefined",
  );
  const pass = !escaped && reached.length === 0 && tried.origin === "ran: null";
  const lines = [pass ? "PASS" : "FAIL", `escaped bot: ${escaped}`, ...Object.entries(tried).map(([k, v]) => `${k}: ${v}`)];
  await request("write_memory", { bot: bot.id, name: "frame-check.md", text: lines.join("\n") });
});

// The page's one connection to the service, through the shell's two calls (see PROTOCOL.md,
// "Window ⇄ service"), or, on another device, through the service's own HTTPS (`lan`). Replies
// are matched by id; everything else goes to the listeners.

type Invoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>;
const shell = (window as unknown as { __TAURI_INTERNALS__?: { invoke: Invoke } }).__TAURI_INTERNALS__;
/** In Bate's own window, not in a browser on another device. */
export const inShell = shell !== undefined;

// On another device: the same lines, by HTTPS to the service that served this page.
let session: Promise<string> | undefined;
let unpaired = false;
const lines: string[] = [];
let paired: () => void = () => {};
const api = (path: string, body?: string) =>
  fetch(path, { method: body === undefined ? "GET" : "POST", headers: { "x-bate": "1" }, body, cache: "no-store" });
const openSession = (): Promise<string> =>
  (session ??= api("/api/session", "").then(async (r) => {
    if (r.ok) return ((await r.json()) as { session: string }).session;
    session = undefined;
    if (r.status === 401) {
      // Asks to pair, then tries again once paired.
      unpaired = true;
      tell({ type: "unpaired" });
      await new Promise<void>((done) => (paired = done));
      return openSession();
    }
    await new Promise((wait) => setTimeout(wait, 2000));
    return openSession();
  }));
const overNetwork: Invoke = async (command, args) => {
  if (command === "new_window") return void window.open(location.href, "_blank");
  if (command === "send") {
    const r = await api(`/api/send?session=${await openSession()}`, String(args?.line));
    if (!r.ok) throw new Error(((await r.json()) as { error: string }).error);
    return;
  }
  // recv: the next line, asking for more when there is none.
  while (!lines.length) {
    const r = await api(`/api/recv?session=${await openSession()}`).catch(() => undefined);
    // Gone, dropped, or timed out: a new connection, a new hello.
    if (!r || r.status === 410 || r.status === 401) {
      session = undefined;
      await new Promise((wait) => setTimeout(wait, 2000));
    } else if (r.ok) {
      lines.push(...((await r.json()) as unknown[]).map((v) => JSON.stringify(v)));
    } else {
      await new Promise((wait) => setTimeout(wait, 2000));
    }
  }
  return lines.shift();
};
const invoke: Invoke = shell ? shell.invoke : overNetwork;

/** Pairs this device with the code shown in Bate's window. */
export async function pair(code: string, name: string) {
  const r = await api("/api/pair", JSON.stringify({ code, name }));
  if (!r.ok) throw new Error(((await r.json()) as { error: string }).error);
  paired();
}

/** Opens one more window, a page of its own with its own connection. */
export const newWindow = () => invoke("new_window");

export type Frame = { type: string; bot?: string; [field: string]: unknown };

let next = 0;
const pending = new Map<number, { resolve: (data: unknown) => void; reject: (e: Error) => void }>();
const listeners = new Set<(frame: Frame) => void>();
let connected = false;

/** Sends a command; resolves with its `data`, rejects with the service's `error`. */
/** `T` is the command's `data` as PROTOCOL.md gives it; the service is not checked against it. */
export function request<T = unknown>(type: string, fields: Record<string, unknown> = {}): Promise<T> {
  const id = ++next;
  // Not `Promise.withResolvers`: the system web view of macOS 13 may not have it.
  return new Promise<T>((resolve, reject) => {
    pending.set(id, { resolve: (data) => resolve(data as T), reject });
    invoke("send", { line: JSON.stringify({ id, type, ...fields }) }).catch((e) => {
      pending.delete(id);
      reject(new Error(String(e)));
    });
  });
}

/** Every frame that is not a reply, plus `{type: "connected"}` once each connection said hello. */
export function listen(listener: (frame: Frame) => void): () => void {
  listeners.add(listener);
  // One that comes later still learns that the service is there or asks to pair.
  if (connected) listener({ type: "connected" });
  else if (unpaired) listener({ type: "unpaired" });
  return () => listeners.delete(listener);
}

// One screen's mistake must not stop the others hearing the service.
const tell = (frame: Frame) =>
  listeners.forEach((listener) => {
    try {
      listener(frame);
    } catch (e) {
      console.error(e);
    }
  });

async function pump() {
  for (;;) {
    const frame = JSON.parse(String(await invoke("recv")));
    if (frame.type === "response") {
      const waiting = pending.get(frame.id);
      pending.delete(frame.id);
      if (frame.success) waiting?.resolve(frame.data);
      else waiting?.reject(new Error(frame.error ?? "failed"));
    } else if (frame.type === "hello") {
      // A new connection: what was asked on the old one gets no reply.
      connected = false;
      for (const waiting of pending.values()) waiting.reject(new Error("the service restarted"));
      pending.clear();
      // Not awaited: its reply comes through this loop.
      request("hello", { protocol: 1 }).then(
        () => {
          unpaired = false;
          connected = true;
          tell({ type: "connected" });
        },
        (e) => tell({ type: "failed", error: String(e) }),
      );
    } else {
      tell(frame);
    }
  }
}

pump().catch((e) => tell({ type: "failed", error: String(e) }));

/** Opens an address in the user's browser: the shell turns the view's navigation into that. */
// A new window is the one way out of the page: the shell sends it to the user's browser.
export const openInBrowser = (url: string) => void window.open(url, "_blank");

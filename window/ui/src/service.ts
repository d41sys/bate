// The page's one connection to the service, through the shell's two calls (see PROTOCOL.md,
// "Window ⇄ service"). Replies are matched by id; everything else goes to the listeners.

type Invoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>;
// The shell's own call, always there in its view (and the page runs nowhere else).
const invoke: Invoke = (window as unknown as { __TAURI_INTERNALS__: { invoke: Invoke } })
  .__TAURI_INTERNALS__.invoke;

export type Frame = { type: string; bot?: string; [field: string]: unknown };

let next = 0;
const pending = new Map<number, { resolve: (data: unknown) => void; reject: (e: Error) => void }>();
const listeners = new Set<(frame: Frame) => void>();

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
  return () => listeners.delete(listener);
}

const tell = (frame: Frame) => listeners.forEach((listener) => listener(frame));

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
      for (const waiting of pending.values()) waiting.reject(new Error("the service restarted"));
      pending.clear();
      // Not awaited: its reply comes through this loop.
      request("hello", { protocol: 1 }).then(
        () => tell({ type: "connected" }),
        (e) => tell({ type: "failed", error: String(e) }),
      );
    } else {
      tell(frame);
    }
  }
}

pump().catch((e) => tell({ type: "failed", error: String(e) }));

/** Opens an address in the user's browser: the shell turns the view's navigation into that. */
export const openInBrowser = (url: string) => window.location.assign(url);

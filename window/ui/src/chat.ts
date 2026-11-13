// A bot's transcript entries (PROTOCOL.md, "Transcript") turned into what the chat shows.

export type Entry =
  | { type: "session"; session: string }
  | { type: "prompt"; nonce: string; text: string }
  | { type: "turn_start"; nonce: string }
  | { type: "text" | "thinking"; delta: string }
  | { type: "tool_start"; call: string; tool: string; args: unknown }
  | { type: "tool_end"; call: string; output: string; isError: boolean }
  | ({ type: "question" } & Question)
  | { type: "turn_end"; reason: string; error?: string };

export type Item =
  | { kind: "user"; text: string; nonce: string }
  | { kind: "reply"; text: string }
  | { kind: "thinking"; text: string }
  | { kind: "tool"; call: string; tool: string; args: unknown; output?: string; isError?: boolean }
  | { kind: "ended"; reason: string; error?: string };

export type Question = {
  question: string;
  title: string;
  options: string[];
  kind?: string;
  target?: string;
};

/** Puts one more entry into `items`, in place. Text and thinking deltas join the item they continue. */
function put(items: Item[], entry: Entry) {
  const last = items[items.length - 1];
  switch (entry.type) {
    case "prompt":
      items.push({ kind: "user", text: entry.text, nonce: entry.nonce });
      break;
    case "text":
    case "thinking": {
      const kind = entry.type === "text" ? "reply" : "thinking";
      if (last?.kind === kind) items[items.length - 1] = { kind, text: last.text + entry.delta };
      else items.push({ kind, text: entry.delta });
      break;
    }
    case "tool_start":
      items.push({ kind: "tool", call: entry.call, tool: entry.tool, args: entry.args });
      break;
    case "tool_end": {
      // Its start is near the end.
      for (let i = items.length - 1; i >= 0; i--) {
        const item = items[i];
        if (item.kind === "tool" && item.call === entry.call) {
          items[i] = { ...item, output: entry.output, isError: entry.isError };
          break;
        }
      }
      break;
    }
    case "turn_end":
      // A turn that ended as it should needs no line of its own.
      if (entry.reason !== "done") items.push({ kind: "ended", reason: entry.reason, error: entry.error });
      break;
  }
}

/** The chat a run of entries makes; linear, for a history of any length. */
export function build(entries: Entry[]): Item[] {
  const items: Item[] = [];
  for (const entry of entries) put(items, entry);
  return items;
}

/** The chat after one more entry, as a new array (for React state). */
export function add(items: Item[], entry: Entry): Item[] {
  const next = items.slice();
  put(next, entry);
  return next;
}

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

/** An entry as the service numbers it. */
export type Row = { seq: number; entry: Entry };

/** `id` is the `seq` of the entry that began the item: it stays when older items come in front. */
export type Item = { id: number } & (
  | { kind: "user"; text: string; nonce: string }
  | { kind: "reply"; text: string }
  | { kind: "thinking"; text: string }
  | { kind: "tool"; call: string; tool: string; args: unknown; output?: string; isError?: boolean }
  | { kind: "ended"; reason: string; error?: string }
);

export type Question = {
  question: string;
  title: string;
  options: string[];
  kind?: string;
  target?: string;
};

/** Puts one more entry into `items`, in place. Text and thinking deltas join the item they continue. */
function put(items: Item[], { seq: id, entry }: Row) {
  const last = items[items.length - 1];
  switch (entry.type) {
    case "prompt":
      items.push({ id, kind: "user", text: entry.text, nonce: entry.nonce });
      break;
    case "text":
    case "thinking": {
      const kind = entry.type === "text" ? "reply" : "thinking";
      if (last?.kind === kind) items[items.length - 1] = { id: last.id, kind, text: last.text + entry.delta };
      else items.push({ id, kind, text: entry.delta });
      break;
    }
    case "tool_start":
      items.push({ id, kind: "tool", call: entry.call, tool: entry.tool, args: entry.args });
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
      if (entry.reason !== "done") items.push({ id, kind: "ended", reason: entry.reason, error: entry.error });
      break;
  }
}

/** The chat a run of entries makes; linear, for a history of any length. */
export function build(rows: Row[]): Item[] {
  const items: Item[] = [];
  for (const row of rows) put(items, row);
  return items;
}

/** The chat after one more entry, as a new array (for React state). */
export function add(items: Item[], row: Row): Item[] {
  const next = items.slice();
  put(next, row);
  return next;
}

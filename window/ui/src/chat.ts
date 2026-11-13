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

/** The chat after one more entry. Text and thinking deltas join the item they continue. */
export function add(items: Item[], entry: Entry): Item[] {
  const last = items[items.length - 1];
  switch (entry.type) {
    case "prompt":
      return [...items, { kind: "user", text: entry.text, nonce: entry.nonce }];
    case "text":
    case "thinking": {
      const kind = entry.type === "text" ? "reply" : "thinking";
      if (last?.kind === kind) return [...items.slice(0, -1), { kind, text: last.text + entry.delta }];
      return [...items, { kind, text: entry.delta }];
    }
    case "tool_start":
      return [...items, { kind: "tool", call: entry.call, tool: entry.tool, args: entry.args }];
    case "tool_end":
      return items.map((item) =>
        item.kind === "tool" && item.call === entry.call
          ? { ...item, output: entry.output, isError: entry.isError }
          : item,
      );
    case "turn_end":
      // A turn that ended as it should needs no line of its own.
      return entry.reason === "done" ? items : [...items, { kind: "ended", reason: entry.reason, error: entry.error }];
    default:
      return items;
  }
}

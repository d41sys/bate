// A bot's transcript entries (PROTOCOL.md, "Transcript") turned into what the chat shows.

export type Entry =
  | { type: "session"; session: string }
  | { type: "prompt"; nonce: string; text: string }
  | { type: "turn_start"; nonce: string }
  | { type: "text" | "thinking"; delta: string }
  | { type: "tool_start"; call: string; tool: string; args: unknown }
  | { type: "tool_end"; call: string; output: string; isError: boolean }
  | ({ type: "question" } & Question)
  | { type: "turn_end"; reason: string; error?: string }
  | { type: "steered"; text: string }
  | { type: "fork"; nonce: string };

/** An entry as the service numbers it. */
export type Row = { seq: number; entry: Entry };

/** `id` is the `seq` of the entry that began the item: it stays when older items come in front. */
export type Item = { id: number } & (
  // `past`: left behind by a fork. The bot no longer has it, so it is for reading only.
  | { kind: "user"; text: string; nonce: string; past?: boolean }
  | { kind: "reply"; text: string }
  | { kind: "thinking"; text: string }
  | { kind: "tool"; call: string; tool: string; args: unknown; output?: string; isError?: boolean }
  | { kind: "ended"; reason: string; error?: string }
  // A message the user put into the running turn; the bot saw it at this point.
  | { kind: "steered"; text: string }
  | { kind: "forked" }
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
    case "steered":
      items.push({ id, kind: "steered", text: entry.text });
      break;
    case "text":
    case "thinking": {
      const kind = entry.type === "text" ? "reply" : "thinking";
      if (last?.kind === kind) items[items.length - 1] = { id: last.id, kind, text: last.text + entry.delta };
      else items.push({ id, kind, text: entry.delta });
      break;
    }
    case "tool_start":
      // A newer plan takes the place of the same agent's older one in this turn: one checklist, kept current.
      if (entry.tool === "plan") {
        for (let i = items.length - 1; i >= 0 && items[i].kind !== "user"; i--) {
          const item = items[i];
          if (item.kind === "tool" && item.tool === "plan" && scope(item.call) === scope(entry.call)) {
            items.splice(i, 1);
            break;
          }
        }
      }
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
    case "fork":
      // The bot went on from before the prompt `nonce`: that prompt and the later ones stay to read.
      for (let i = items.length - 1; i >= 0; i--) {
        const item = items[i];
        if (item.kind !== "user") continue;
        items[i] = { ...item, past: true };
        if (item.nonce === entry.nonce) break;
      }
      items.push({ id, kind: "forked" });
      break;
  }
}

/** Whose call it is: empty for the conversation's own, `agent<n>/` for a sub-agent's. */
const scope = (call: string) => call.slice(0, call.lastIndexOf("/") + 1);

export type Step = { step: string; status: "pending" | "in_progress" | "done" };

/** The steps of a `plan` call; nothing when the arguments are not a plan (they are then shown as they are). */
export function planSteps(args: unknown): Step[] | undefined {
  const steps = (args as { steps?: unknown } | null)?.steps;
  if (!Array.isArray(steps) || steps.length === 0) return undefined;
  const ok = steps.every((s) => typeof s?.step === "string" && ["pending", "in_progress", "done"].includes(s?.status));
  return ok ? (steps as Step[]) : undefined;
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

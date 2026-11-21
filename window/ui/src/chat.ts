// A bot's transcript entries (PROTOCOL.md, "Transcript") turned into what the chat shows.

export type Entry =
  | { type: "session"; session: string }
  | { type: "prompt"; nonce: string; text: string }
  | { type: "turn_start"; nonce: string }
  | { type: "text" | "thinking"; delta: string }
  | { type: "tool_start"; call: string; tool: string; args: unknown }
  | { type: "tool_end"; call: string; output: string; isError: boolean }
  | ({ type: "question" } & Question)
  | { type: "retry"; attempt: number; of: number; waitMs: number; error: string }
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
  | { kind: "retrying"; attempt: number; of: number; waitMs: number; error: string }
  | { kind: "forked" }
);

export type Question = {
  question: string;
  title: string;
  options: string[];
  kind?: string;
  target?: string;
  /** Where a file or a command would act: the bot's folder, or its project's. */
  folder?: string;
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
    case "retry":
      items.push({ id, kind: "retrying", attempt: entry.attempt, of: entry.of, waitMs: entry.waitMs, error: entry.error });
      break;
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

/** A call, or a stretch of thinking: what a run is made of. */
export type Part = Extract<Item, { kind: "tool" | "thinking" }>;

/**
 * What the chat draws. Calls and thinking that follow each other are one `run` (a fold: one line that
 * opens to its parts); a plan is its checklist. Everything else is as the transcript made it, and `working`
 * is the mark under the last message while the bot works. `id` is the first item's, so it stays as a run grows.
 */
export type Shown =
  | Exclude<Item, { kind: "tool" | "thinking" }>
  | { id: number; kind: "run"; parts: Part[] }
  | { id: number; kind: "plan"; steps: Step[] }
  | { id: number; kind: "working" };

export const WORKING: Shown = { id: -1, kind: "working" };

/** The chat's items with each run of calls and thinking folded into one. */
export function fold(items: Item[]): Shown[] {
  const shown: Shown[] = [];
  for (const item of items) {
    const steps = item.kind === "tool" && item.tool === "plan" && !item.isError ? planSteps(item.args) : undefined;
    if (steps) {
      shown.push({ id: item.id, kind: "plan", steps });
    } else if (item.kind === "tool" || item.kind === "thinking") {
      const last = shown[shown.length - 1];
      if (last?.kind === "run") last.parts.push(item);
      else shown.push({ id: item.id, kind: "run", parts: [item] });
    } else {
      shown.push(item);
    }
  }
  return shown;
}

// How a kind of call reads: its verb in the past and as it goes, and its thing in the singular and the plural.
type Say = [past: string, now: string, one: string, many: string];
const FILES: [string, string] = ["file", "files"];
const SAY: Record<string, Say> = {
  run: ["ran", "running", "command", "commands"],
  read: ["read", "reading", ...FILES],
  write: ["wrote", "writing", ...FILES],
  edit: ["edited", "editing", ...FILES],
  change: ["changed", "changing", ...FILES],
  search: ["made", "making", "search", "searches"],
  fetch: ["opened", "opening", "page", "pages"],
  browser: ["made", "making", "browser action", "browser actions"],
  // The provider's own search, shown as a call of ours (PROTOCOL.md, Web search).
  web_search: ["made", "making", "web search", "web searches"],
  other: ["used", "using", "tool", "tools"],
};
// An installed agent names its kinds of tool its own way.
const ALIAS: Record<string, string> = { execute: "run", delete: "change", move: "change" };

/** How a call is counted and said. A connector's tool is `<connector>__<tool>`: said by the connector's name. */
function say(tool: string): [key: string, say: Say] {
  if (tool.includes("__")) {
    const name = tool.slice(0, tool.indexOf("__"));
    return [`connector ${name}`, ["used", "using", `${name} tool`, `${name} tools`]];
  }
  const key = ALIAS[tool] ?? (tool in SAY ? tool : "other");
  return [key, SAY[key]];
}

/**
 * What a run did, in the person's words: "Ran 3 commands, read 1 file". `live`: the bot is still at it,
 * so a kind with a call not finished is said as it goes ("Running 2 commands") and `running` is true.
 * `failed` counts the calls that failed: the line must show it without the run being opened.
 */
export function describe(parts: Part[], live: boolean): { text: string; running: boolean; failed: number } {
  const kinds = new Map<string, { say: Say; n: number; going: boolean }>();
  let failed = 0;
  for (const part of parts) {
    if (part.kind !== "tool") continue;
    const [key, words] = say(part.tool);
    const k = kinds.get(key) ?? { say: words, n: 0, going: false };
    k.n++;
    k.going ||= live && part.output === undefined;
    if (part.isError) failed++;
    kinds.set(key, k);
  }
  const running = live && (kinds.size === 0 || [...kinds.values()].some((k) => k.going));
  if (kinds.size === 0) return { text: running ? "Thinking…" : "Thought", running, failed };
  const text = [...kinds.values()].map(({ say: [past, now, one, many], n, going }) => `${going ? now : past} ${n} ${n === 1 ? one : many}`).join(", ");
  return { text: text[0].toUpperCase() + text.slice(1), running, failed };
}

/** What a call acted on, to name its row: the command, else the path, else what was searched for; the tool's name only when nothing else is known. */
export function aim(part: Extract<Part, { kind: "tool" }>): string {
  const a = (part.args && typeof part.args === "object" ? part.args : {}) as Record<string, unknown>;
  if (part.tool === "web_search") return typeof a.query === "string" && a.query ? `Searched the web: ${a.query}` : "Searched the web";
  const input = (a.rawInput && typeof a.rawInput === "object" ? a.rawInput : {}) as Record<string, unknown>;
  const first = (...values: unknown[]) => values.find((v): v is string => typeof v === "string" && v !== "");
  const acted = first(a.command, input.command, a.url, input.url);
  if (acted) return acted;
  const path = first(a.path, input.path, input.file_path, (a.locations as { path?: unknown }[] | undefined)?.[0]?.path);
  const pattern = first(a.pattern, input.pattern, input.query);
  if (pattern) return path ? `${pattern} in ${path}` : pattern;
  return first(path, a.title, a.action) ?? part.tool;
}

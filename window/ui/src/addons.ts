import type { Connector, ConnectorListed } from "./types.ts";

/** Who made an add-on, as far as Bate can say, shown before it is installed. */
export const madeBy = {
  catalog: "Bate's catalog, a list in Bate's own repository on GitHub (d41sys/bate). Each entry pins the exact archive by its hash.",
  folder: "A folder you chose on this Mac. Bate cannot tell who made it.",
  file: "A file you chose on this Mac. Bate cannot tell who made it.",
  typed: "You, here. Bate cannot tell who runs the server or the program.",
  copy: (bot: string) => `${bot}'s copy. Bate cannot tell who made it.`,
  connectorList: "Bate's list of connectors, a file in Bate's own repository on GitHub (d41sys/bate). An entry is a pointer, not a recommendation: Bate has not checked the server or the program it names.",
  skillList: "Bate's list of skills, a file in Bate's own repository on GitHub (d41sys/bate). Each entry pins the exact archive by its hash, but is not a recommendation: Bate has not checked what the skill says.",
};

/** Who the file of an add-on says made it: shown as the claim it is, never as a fact. Nothing when there is none. */
export const byLine = (author?: string) => (author ? `By ${author} (not checked)` : undefined);

/** One add-on and the bots that have it, each with its own copy's data. */
export type Placed<T> = { name: string; users: { bot: string; item: T }[] };

/** Groups what each bot has by `same` (the name, unless the bots' copies must also match), sorted by name. */
export function placed<T extends { name: string }>(lists: { bot: string; items: T[] }[], same: (item: T) => string = (i) => i.name): Placed<T>[] {
  const rows = new Map<string, Placed<T>>();
  for (const { bot, items } of lists) {
    for (const item of items) {
      const key = same(item);
      const row = rows.get(key) ?? { name: item.name, users: [] };
      row.users.push({ bot, item });
      rows.set(key, row);
    }
  }
  return [...rows.entries()].sort(([a, x], [b, y]) => x.name.localeCompare(y.name) || a.localeCompare(b)).map(([, row]) => row);
}

/** Runs `run` for each id in turn, so one failure does not stop the others: the ones that failed, and why. */
export async function each(ids: string[], run: (id: string) => Promise<unknown>) {
  const failed: { id: string; error: string }[] = [];
  for (const id of ids) await run(id).catch((e: Error) => failed.push({ id, error: e.message }));
  return failed;
}

/** What a connector's line says: off, waiting for the sign-in, failed to load (and why), working (the engine loaded it), or no problem reported. An installed agent reports nothing, so it is never said to work. */
export function connectorState(c: Connector, agent = false): { text: string; problem: boolean } {
  if (!c.enabled) return { text: "Off", problem: false };
  if (c.error) return { text: `Could not start: ${c.error}`, problem: true };
  if (c.oauth && !c.signedIn) return { text: "On, but not signed in", problem: true };
  if (agent) return { text: "On; the agent does not report", problem: false };
  if (c.tools !== undefined) return { text: `Working, ${c.tools} ${c.tools === 1 ? "tool" : "tools"}`, problem: false };
  return { text: "On, no problem reported", problem: false };
}

/** The tools of a connector for one bot, to switch to blocked: those its engine listed and those blocked (a blocked one may not be listed yet), each with its own name and whether the connector marks it read-only. */
export function connectorTools(c: Connector): { name: string; own: string; blocked: boolean; marked: boolean }[] {
  const blocked = c.blocked ?? [];
  const names = [...new Set([...(c.toolNames ?? []), ...blocked])].sort();
  return names.map((name) => ({ name, own: name.slice(c.name.length + 2), blocked: blocked.includes(name), marked: (c.readOnly ?? []).includes(name) }));
}

/** The one line of a row: on for how many of the bots that have it. */
export function onFor(copies: { enabled: boolean }[], bots: number): string {
  const on = copies.filter((c) => c.enabled).length;
  const plural = copies.length === 1 ? "bot" : "bots";
  const some = copies.length < bots ? " that have it" : "";
  return `On for ${on} of ${copies.length} ${plural}${some}`;
}

/** The one line of a connector's row: on for how many of the bots that have it (and how many of those need a look). */
export function connectorSummary(copies: Connector[], bots: number): { text: string; problems: number } {
  return { text: onFor(copies, bots), problems: copies.filter((c) => connectorState(c).problem).length };
}

/** What a connector may do, one line each, as shown before it is added. */
export function connectorMay(c: Connector): string[] {
  const t = c.transport;
  return [
    t.type === "http"
      ? `The bot calls the server at ${t.url}: what it sends a tool goes there.${c.oauth ? " The server signs you in." : ""}`
      : `Runs ${[t.command, ...(t.args ?? [])].join(" ")} on this Mac, in the bot's folder, while the bot works.`,
    "Every tool it offers asks you first, until you answer Always for that tool.",
  ];
}

export type ConnectorForm = { name: string; type: "http" | "stdio"; url: string; command: string; args: string; oauth: boolean };

/** The connector a form describes: each line of the arguments is one argument, so none is split at a space. */
export function connectorOf(f: ConnectorForm): Connector {
  return {
    name: f.name.trim(),
    enabled: true,
    transport:
      f.type === "http"
        ? { type: "http", url: f.url.trim() }
        : { type: "stdio", command: f.command.trim(), args: f.args.split("\n").filter(Boolean) },
    oauth: f.type === "http" && f.oauth,
  };
}

/** The form a connector of the list fills in: its address or program, and nothing of the person's own. A line is left to type in what `fill` names. */
export function formOf(e: ConnectorListed): ConnectorForm {
  const t = e.transport;
  if (t.type === "http") return { name: e.name, type: "http", url: t.url, command: "", args: "", oauth: !!e.oauth };
  return { name: e.name, type: "stdio", url: "", command: t.command, args: (t.args ?? []).join("\n") + (e.fill?.length ? "\n" : ""), oauth: false };
}

/** Every word of `query` is in one of the texts, in any case; no query matches all. */
export const matches = (query: string, ...texts: string[]) => {
  const hay = texts.join("\n").toLowerCase();
  return query.toLowerCase().split(/\s+/).filter(Boolean).every((w) => hay.includes(w));
};

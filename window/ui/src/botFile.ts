import { when } from "./times.ts";
import type { BotFilePreview } from "./types.ts";

/** What a bot file holds, one line each, as the person is shown it before anything is made. */
export function botFileLines(p: BotFilePreview): string[] {
  const list = (what: string, items: string[]) => `${what}: ${items.length ? items.join(", ") : "none"}`;
  return [
    p.instructionsLength ? `Instructions: ${p.instructionsLength} characters` : "Instructions: none",
    list("Skills", p.skills.map((s) => {
      const notes = [s.runsScripts && "runs scripts", s.author && `by ${s.author}, not checked`].filter(Boolean);
      return notes.length ? `${s.name} (${notes.join("; ")})` : s.name;
    })),
    list("Routines (off until you turn them on)", p.routines.map((r) => `${r.name} (${when({ schedule: r.schedule ?? undefined }).toLowerCase()})`)),
    `Notes: ${p.notes}`,
    list("Connectors (off until you turn them on)", p.connectors.map((c) => `${c.name} at ${c.address}`)),
  ];
}

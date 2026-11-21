import { sizeText } from "./workplaces.ts";

/** Bate's own copy of an agent's adapter: `version` is `null` when it has none, `used` when the program found is that copy (else one the person has on their PATH comes first). */
export type Adapter = { name: string; version: string | null; used: boolean };

/** An installed agent the service found: what `list_agents` answers. */
export type Agent = { label: string; command: string[]; path: string; adapter?: Adapter };

/** What `list_agents` answers: the installed agents, and the known ones that are not, each with the command that gets it (`null`: none). */
export type Found = { agents: Agent[]; missing: { label: string; install: string | null; adapter?: Adapter }[] };

/** What `preview_agent` answers: what installing an adapter would fetch, and the `node` it would run on (`runtime`; `missing` says why there is none). */
export type AdapterPreview = {
  name: string;
  description: string;
  version: string;
  home: string;
  installedVersion: string | null;
  update: boolean;
  packages: number;
  bytes: number;
  hosts: string[];
  node: number;
  runtime: { path: string; version: string } | null;
  missing: string | null;
};

/** The lines of the preview that say what is fetched, from where, and what it runs on. */
export function adapterFacts(p: AdapterPreview): string[] {
  return [
    `Version ${p.version}: ${p.packages} packages, ${sizeText(p.bytes)} to download from ${p.hosts.join(", ")}.`,
    "Each package is checked against the size and checksum Bate lists for it. No install script is run.",
    p.runtime ? `It runs on the Node.js you have: ${p.runtime.version}, ${p.runtime.path}.` : `It needs Node.js ${p.node} or newer, which Bate does not install.`,
  ];
}

/** The select's value for Bate's own engine, and for a command that is none of the listed agents. */
export const OWN = "";
export const OTHER = "other";

/**
 * The option that shows the bot's saved command: Bate's own for none, an installed agent (its label)
 * for its exact command, else Other. A known agent that is not installed now is Other too.
 */
export function agentChoice(saved: string[] | null | undefined, agents: Agent[]): string {
  if (!saved?.length) return OWN;
  return agents.find((a) => a.command.length === saved.length && a.command.every((word, i) => word === saved[i]))?.label ?? OTHER;
}

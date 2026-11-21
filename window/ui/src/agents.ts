/** An installed agent the service found: what `list_agents` answers. */
export type Agent = { label: string; command: string[]; path: string };

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

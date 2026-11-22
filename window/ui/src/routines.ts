import { connectorTools } from "./addons.ts";
import { dayTime } from "./times.ts";
import type { After, Backoff, Check, Connector, Permissions, RoutineRun, Watch } from "./types.ts";

const ENDS: Record<After["outcome"], string> = { done: "finishes a turn", failed: "fails a turn", any: "ends a turn" };

/** When a routine that follows another bot starts, in words; `name`: that bot's name, if it is known. */
export const afterText = ({ bot, outcome }: After, name?: string) => `After ${name ?? bot} ${ENDS[outcome]}`;

/** Why a run was skipped, in words; nothing for a run that was not skipped. */
export function skipWhy({ reason, detail }: Pick<RoutineRun, "reason" | "detail">): string | undefined {
  switch (reason) {
    case undefined:
      return undefined;
    case "paused":
      return "The bot was paused.";
    case "signed_out":
      return `The connector ${detail} needs a sign-in.`;
    case "backoff":
      return "Held back: the last runs failed.";
    case "limit":
      return `The token limit stopped it: ${detail}.`;
    case "busy":
      return "The bot was busy.";
    case "rate":
      return "Held back: it already started 20 times in the last hour.";
    case "precheck":
      return `The precheck stopped it: ${detail}`;
    default:
      return reason;
  }
}

/** The quiet badge of a routine that is held to fewer runs: how many failed in a row, and when it next runs. */
export const heldText = ({ failures, nextRunMs }: Backoff, now = Date.now()) => `${failures} failed in a row${nextRunMs === undefined ? "" : ` · next run ${dayTime(nextRunMs, now)}`}`;

/** What a watch does, in words. */
export const watchText = ({ connector, tool, every }: Watch) => `Watches ${connector}: ${tool}, every ${every} minutes`;

/** The last check of a watch, in words. */
export const checkText = ({ atMs, changed, error }: Check, now = Date.now()) => `Last check ${dayTime(atMs, now)}: ${error ?? (changed ? "the answer changed" : "no change")}`;

/** The arguments the person typed for a watch's tool: a JSON object. */
export function watchArguments(text: string): Record<string, unknown> {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text.trim() || "{}");
  } catch {
    throw new Error("The arguments must be JSON, like {\"state\": \"open\"}.");
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) throw new Error("The arguments must be a JSON object, like {\"state\": \"open\"}.");
  return parsed as Record<string, unknown>;
}

/**
 * The tools of a connector to choose from for a watch, as the connector names them, each marked when the bot may not call it
 * without asking (the service refuses such a watch when it is saved). Nothing when the bot's engine has not loaded the connector,
 * so its tools are not known; `current` (a tool already set) is kept in the list.
 */
export function watchTools(c: Connector | undefined, current: string, p?: Pick<Permissions, "levels" | "grants">): { own: string; label: string }[] | undefined {
  if (!c?.toolNames) return undefined;
  const tools = connectorTools(c);
  if (current && !tools.some((t) => t.own === current)) tools.push({ name: `${c.name}__${current}`, own: current, blocked: (c.blocked ?? []).includes(`${c.name}__${current}`) });
  return tools.map(({ name, own, blocked }) => {
    const level = p?.levels.connector;
    const granted = p?.grants.some((g) => g.kind === "connector" && g.target === name);
    const why = blocked ? "blocked" : level === "never" ? "not allowed" : level === "ask" && !granted ? "would ask" : "";
    return { own, label: why ? `${own} (${why})` : own };
  });
}

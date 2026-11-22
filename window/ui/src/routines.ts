import { dayTime } from "./times.ts";
import type { After, Backoff, RoutineRun } from "./types.ts";

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

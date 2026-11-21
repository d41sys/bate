import { dayTime } from "./times.ts";
import type { Backoff, RoutineRun } from "./types.ts";

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
    case "precheck":
      return `The precheck stopped it: ${detail}`;
    default:
      return reason;
  }
}

/** The quiet badge of a routine that is held to fewer runs: how many failed in a row, and when it next runs. */
export const heldText = ({ failures, nextRunMs }: Backoff, now = Date.now()) => `${failures} failed in a row${nextRunMs === undefined ? "" : ` · next run ${dayTime(nextRunMs, now)}`}`;

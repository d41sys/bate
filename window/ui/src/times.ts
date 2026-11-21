import type { Frequency, Routine } from "./types.ts";

/** The unit of each frequency, in the plural. */
export const EVERY: Record<Frequency, string> = {
  minutes: "minutes",
  hourly: "hours",
  daily: "days",
  weekly: "weeks",
  monthly: "months",
  yearly: "years",
};

/** When a routine runs, in words. */
export const when = (r: Pick<Routine, "schedule">) => (r.schedule ? `Every ${r.schedule.interval === 1 ? EVERY[r.schedule.frequency].replace(/s$/, "") : `${r.schedule.interval} ${EVERY[r.schedule.frequency]}`} from ${r.schedule.time}` : "Not on a schedule");

const DAY = 86_400_000;
const clock = (ms: number) => new Date(ms).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
const midnight = (ms: number) => new Date(new Date(ms).setHours(0, 0, 0, 0)).getTime();

/**
 * When, short, for a row of a list: "Today 08:00", "Tomorrow 08:00", "Yesterday 11:41", the weekday and time
 * within six days either way ("Fri 17:30"), else the date ("12 Nov"; the year too when it is not this one).
 * The clock is the system's: 12 or 24 hours as its locale has it.
 */
export function dayTime(ms: number, now = Date.now()): string {
  // Whole calendar days apart; a day of 23 or 25 hours (the clocks changed) still counts as one.
  const apart = Math.round((midnight(ms) - midnight(now)) / DAY);
  if (apart === 0) return `Today ${clock(ms)}`;
  if (apart === 1) return `Tomorrow ${clock(ms)}`;
  if (apart === -1) return `Yesterday ${clock(ms)}`;
  const at = new Date(ms);
  if (Math.abs(apart) <= 6) return `${at.toLocaleDateString(undefined, { weekday: "short" })} ${clock(ms)}`;
  const year = at.getFullYear() !== new Date(now).getFullYear();
  return at.toLocaleDateString(undefined, { day: "numeric", month: "short", ...(year && { year: "numeric" as const }) });
}

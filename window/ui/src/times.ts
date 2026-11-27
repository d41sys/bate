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
export const clock = (ms: number) => new Date(ms).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
export const midnight = (ms: number) => new Date(new Date(ms).setHours(0, 0, 0, 0)).getTime();
// Whole calendar days apart; a day of 23 or 25 hours (the clocks changed) still counts as one.
const daysApart = (ms: number, now: number) => Math.round((midnight(ms) - midnight(now)) / DAY);

/** The day alone: "Today", "Tomorrow", "Yesterday", the weekday within six days either way ("Fri"), else the date ("12 Nov"; the year too when it is not this one). */
export function day(ms: number, now = Date.now()): string {
  const apart = daysApart(ms, now);
  if (apart === 0) return "Today";
  if (apart === 1) return "Tomorrow";
  if (apart === -1) return "Yesterday";
  const at = new Date(ms);
  if (Math.abs(apart) <= 6) return at.toLocaleDateString(undefined, { weekday: "short" });
  const year = at.getFullYear() !== new Date(now).getFullYear();
  return at.toLocaleDateString(undefined, { day: "numeric", month: "short", ...(year && { year: "numeric" as const }) });
}

/**
 * When, short, for a row of a list: "Today 08:00", "Tomorrow 08:00", "Yesterday 11:41", the weekday and time
 * within six days either way ("Fri 17:30"), else the date alone.
 * The clock is the system's: 12 or 24 hours as its locale has it.
 */
export const dayTime = (ms: number, now = Date.now()): string => (Math.abs(daysApart(ms, now)) <= 6 ? `${day(ms, now)} ${clock(ms)}` : day(ms, now));

/** How long something took, short: "12 s", "2 min 5 s", "1 h 4 min". */
export function took(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s} s`;
  if (s < 3600) return `${Math.floor(s / 60)} min ${s % 60} s`;
  return `${Math.floor(s / 3600)} h ${Math.floor((s % 3600) / 60)} min`;
}

import type { Tally, Tokens } from "./types.ts";

const number = new Intl.NumberFormat("en");

/** What a limit counts: input and output. The cached input is part of the input. */
export const total = (t: Tokens) => t.input + t.output;

/** "12,345 tokens". */
export function tokenCount(t: Tokens): string {
  const n = total(t);
  return `${number.format(n)} ${n === 1 ? "token" : "tokens"}`;
}

/** What a period shows: its count with what it is made of, or that it is not counted when the server said nothing of every request. */
export function usedText(t: Tally): { value: string; hint: string } {
  if (t.unreported > 0 && total(t) === 0) return { value: "Not counted", hint: "The model server (or the agent) does not say what a request used." };
  const parts = `Input ${number.format(t.input)}${t.cached ? ` (${number.format(t.cached)} from the cache)` : ""}, output ${number.format(t.output)}.`;
  const missing = t.unreported > 0 ? ` ${number.format(t.unreported)} ${t.unreported === 1 ? "request was" : "requests were"} not reported and ${t.unreported === 1 ? "is" : "are"} not in it.` : "";
  return { value: tokenCount(t), hint: parts + missing };
}

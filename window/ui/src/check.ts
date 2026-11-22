import type { Bot } from "./types.ts";

/** The value of the check select for what the bot has: `off`, `own`, `model:<name>` or `bot:<id>`. */
export const checkValue = (bot: Pick<Bot, "check" | "checkModel" | "checkBot">): string =>
  bot.check === "model" ? `model:${bot.checkModel}` : bot.check === "bot" ? `bot:${bot.checkBot}` : bot.check;

/** The fields of `set_bot` for a value of the check select. */
export function checkFields(value: string): { check: string; checkModel?: string; checkBot?: string } {
  if (value.startsWith("model:")) return { check: "model", checkModel: value.slice(6) };
  if (value.startsWith("bot:")) return { check: "bot", checkBot: value.slice(4) };
  return { check: value };
}

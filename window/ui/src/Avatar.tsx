import type { CSSProperties } from "react";
import type { Bot } from "./types.ts";

/** The bot's sign, or its first letter, on its colour. The name is always beside it. */
export const Avatar = ({ bot }: { bot: Bot }) => (
  <span className="avatar" aria-hidden="true" style={{ "--tint": bot.color || undefined } as CSSProperties}>
    {bot.avatar || [...bot.name][0]?.toUpperCase()}
  </span>
);

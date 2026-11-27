import type { CSSProperties } from "react";
import { BOT_ICONS } from "./icons.ts";
import type { Bot } from "./types.ts";

const letter = (name: string) => [...name][0]?.toUpperCase();

/** A sign on its colour: the icon of that name, else the characters given before there were icons, else the first letter. The name is always beside it. */
export const Face = ({ avatar, name, color }: { avatar: string; name: string; color?: string }) => {
  const Icon = BOT_ICONS.get(avatar);
  return (
    <span className="avatar" aria-hidden="true" style={{ "--tint": color || undefined } as CSSProperties}>
      {Icon ? <Icon /> : avatar || letter(name)}
    </span>
  );
};

export const Avatar = ({ bot }: { bot: Bot }) => <Face avatar={bot.avatar} name={bot.name} color={bot.color} />;

/** The choice of a bot's icon: its first letter, or one of the set. Radio buttons, so the arrow keys go through them. */
export const IconPicker = ({ value, name, onChange }: { value: string; name: string; onChange: (avatar: string) => void }) => (
  <span className="icon-picker" role="radiogroup" aria-label="Icon">
    {["", ...BOT_ICONS.keys()].map((key) => {
      const Icon = BOT_ICONS.get(key);
      return (
        <span key={key} data-tip={key || "First letter"}>
          <input type="radio" name="bot-icon" aria-label={key || "First letter"} checked={value === key} onChange={() => onChange(key)} />
          {Icon ? <Icon /> : letter(name) || "A"}
        </span>
      );
    })}
  </span>
);

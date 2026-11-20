import { useState } from "react";
import { request } from "./service.ts";
import { STARTERS } from "./firstRun.ts";
import type { Bot } from "./types.ts";

/** A new bot: a name, what it is for (the first line of its instructions) and a sign. A card fills the fields. */
export function NewBot({ opened, report }: { opened: (bot: string) => void; report: (message: string) => void }) {
  const [name, setName] = useState("");
  const [purpose, setPurpose] = useState("");
  const [avatar, setAvatar] = useState("");
  const [error, setError] = useState("");
  const create = async () => {
    let bot: Bot;
    try {
      bot = (await request<{ bot: Bot }>("create_bot", { name: name.trim() })).bot;
    } catch (e) {
      return setError((e as Error).message);
    }
    // The bot exists now: if its purpose cannot be saved, it opens all the same, and Settings has the rest.
    if (purpose.trim() || avatar.trim()) await request("set_bot", { bot: bot.id, instructions: purpose.trim(), avatar: avatar.trim() }).catch((e: Error) => report(e.message));
    opened(bot.id);
  };
  return (
    <section className="fields stack">
      <h2>Start from a card</h2>
      <ul className="starters">
        {STARTERS.map((s) => (
          <li key={s.name}>
            <button
              type="button"
              aria-pressed={name === s.name && purpose === s.purpose}
              onClick={() => {
                setName(s.name);
                setPurpose(s.purpose);
                setAvatar(s.avatar);
              }}
            >
              <span className="avatar" aria-hidden="true">{s.avatar}</span>
              <span className="label"><span>{s.name}</span><small>{s.purpose}</small></span>
            </button>
          </li>
        ))}
      </ul>
      <form onSubmit={(e) => { e.preventDefault(); void create(); }}>
        <h2>Or write your own</h2>
        <label>Name <input autoFocus required maxLength={80} value={name} onChange={(e) => setName(e.target.value)} /></label>
        <label>
          Purpose
          <textarea maxLength={8000} rows={3} placeholder="What this bot is for and how it should answer" value={purpose} onChange={(e) => setPurpose(e.target.value)} />
        </label>
        <label>Avatar <input maxLength={8} placeholder="🙂" value={avatar} onChange={(e) => setAvatar(e.target.value)} /></label>
        <button disabled={!name.trim()}>Create</button>
        {error && <p className="error" role="alert">{error}</p>}
      </form>
    </section>
  );
}

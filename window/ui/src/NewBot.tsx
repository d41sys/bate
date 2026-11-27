import { useState } from "react";
import { madeBy } from "./addons.ts";
import { botFileLines } from "./botFile.ts";
import { Choose } from "./ProjectDialog.tsx";
import { Sheet } from "./Sheet.tsx";
import { inShell, request } from "./service.ts";
import { STARTERS } from "./firstRun.ts";
import type { Bot, BotFilePreview } from "./types.ts";
import { Face, IconPicker } from "./Avatar.tsx";

/** A new bot: a name, what it is for (the first line of its instructions) and a sign. A card fills the fields. */
export function NewBot({ opened, report }: { opened: (bot: string) => void; report: (message: string) => void }) {
  const [name, setName] = useState("");
  const [purpose, setPurpose] = useState("");
  const [avatar, setAvatar] = useState("");
  const [error, setError] = useState("");
  const [importError, setImportError] = useState("");
  /** The file chosen and what it holds, shown before anything is made; the import must be of this very file. */
  const [asked, setAsked] = useState<{ path: string; preview: BotFilePreview }>();
  const look = (path: string) =>
    request<BotFilePreview>("preview_bot_file", { path }).then((preview) => (setAsked({ path, preview }), setImportError("")), (e: Error) => setImportError(e.message));
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
              <Face avatar={s.avatar} name={s.name} />
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
        <div className="field">Icon <IconPicker value={avatar} name={name} onChange={setAvatar} /></div>
        <button disabled={!name.trim()}>Create</button>
        {error && <p className="error" role="alert">{error}</p>}
      </form>
      {inShell && (
        <div className="stack">
          <h2>Or import a bot from a file</h2>
          <p className="note">A file made by Export bot, in a bot's settings.</p>
          <div className="actions"><Choose folder={false} set={(path) => void look(path)} fail={setImportError} /></div>
          {importError && <p className="error" role="alert">{importError}</p>}
        </div>
      )}
      {asked && (
        <Sheet
          title={`Import ${asked.preview.name}`}
          about="Made as a new bot, with the usual permissions and none of the old bot's answers."
          maker={madeBy.file}
          facts={botFileLines(asked.preview)}
          action="Import"
          go={async () => opened((await request<{ bot: Bot }>("import_bot", { path: asked.path, sha256: asked.preview.sha256 })).bot.id)}
          close={() => setAsked(undefined)}
        />
      )}
    </section>
  );
}

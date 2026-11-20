import { useEffect, useState } from "react";
import { listen, request } from "./service.ts";
import { hostLines, hostsHint } from "./network.ts";
import { KINDS, type Bot, type Kind, type Level, type Permissions } from "./types.ts";

const ACTIONS: Record<Kind, string> = {
  read: "Read files",
  write: "Change files",
  run: "Run commands",
  connector: "Use connectors",
  desktop: "See and control other apps",
  apple_read: "Read Messages, Mail, Notes, Reminders, Calendar, Contacts",
  apple_change: "Send and change in those apps (always shows what it sends)",
};

export function Settings({ bot, onDeleted }: { bot: Bot; onDeleted: () => void }) {
  const [permissions, setPermissions] = useState<Permissions>();
  const looks = () => ({ name: bot.name, instructions: bot.instructions, avatar: bot.avatar, color: bot.color, stepLimit: bot.stepLimit });
  const [look, setLook] = useState(looks);
  const [deleting, setDeleting] = useState(false);
  // Saved here or elsewhere: show what the service has.
  useEffect(() => setLook(looks()), [bot.name, bot.instructions, bot.avatar, bot.color, bot.stepLimit]);
  const [hosts, setHosts] = useState((bot.network ?? []).join("\n"));
  useEffect(() => setHosts((bot.network ?? []).join("\n")), [bot.id, (bot.network ?? []).join("\n")]);
  const [agent, setAgent] = useState((bot.agent ?? []).join(" "));
  const [error, setError] = useState("");

  useEffect(() => {
    const load = () =>
      request<Permissions>("get_permissions", { bot: bot.id }).then(setPermissions, (e) => setError(e.message));
    load();
    return listen((frame) => {
      if (frame.type === "permissions_changed" && frame.bot === bot.id) load();
    });
  }, [bot.id]);

  const run = (p: Promise<unknown>) => p.then(() => setError(""), (e: Error) => setError(e.message));
  const words = agent.trim().split(/\s+/).filter(Boolean);

  return (
    <section className="fields">
      <form onSubmit={(e) => { e.preventDefault(); run(request("set_bot", { bot: bot.id, ...look })); }}>
        <h2>The bot</h2>
        <label>Name <input required maxLength={80} value={look.name} onChange={(e) => setLook({ ...look, name: e.target.value })} /></label>
        <label>
          Instructions
          <textarea maxLength={8000} rows={6} placeholder="What this bot is for and how it should answer" value={look.instructions} onChange={(e) => setLook({ ...look, instructions: e.target.value })} />
        </label>
        <label>Avatar <input maxLength={8} placeholder="🙂" value={look.avatar} onChange={(e) => setLook({ ...look, avatar: e.target.value })} /></label>
        <label>
          Tool rounds per turn <input type="number" required min={1} max={1000} step={1} value={look.stepLimit} onChange={(e) => setLook({ ...look, stepLimit: Number(e.target.value) })} />
          {" "}<span className="hint">A turn stops after this many; the chat then offers Continue.</span>
        </label>
        <label>
          Colour <input type="color" value={look.color || "#888888"} onChange={(e) => setLook({ ...look, color: e.target.value })} />{" "}
          {look.color && <button type="button" onClick={() => setLook({ ...look, color: "" })}>No colour</button>}
        </label>
        <button>Save</button>
      </form>
      <h2>What the bot may do</h2>
      {permissions &&
        KINDS.map((kind) => (
          <label key={kind}>
            {ACTIONS[kind]}{" "}
            <select value={permissions.levels[kind]} onChange={(e) => run(request("set_permission", { bot: bot.id, kind, level: e.target.value as Level }))}>
              {kind !== "apple_change" && <option value="always">always</option>}
              <option value="ask">ask me</option>
              <option value="never">never</option>
            </select>
          </label>
        ))}
      {permissions && permissions.grants.length > 0 && (
        <>
          <h3>Always allowed</h3>
          <ul className="rows">
            {permissions.grants.map((g) => (
              <li key={`${g.kind} ${g.target}`}>
                {ACTIONS[g.kind]}: <code>{g.target}</code>
                <button onClick={() => run(request("delete_grant", { bot: bot.id, kind: g.kind, target: g.target }))}>Remove</button>
              </li>
            ))}
          </ul>
        </>
      )}
      <label>
        <input type="checkbox" checked={bot.wholeDisk} disabled={bot.network !== null} onChange={(e) => run(request("set_disk_access", { bot: bot.id, wholeDisk: e.target.checked }))} /> Whole disk (otherwise only the bot's folder)
      </label>
      <label>
        <input type="checkbox" checked={bot.network !== null} disabled={bot.wholeDisk} onChange={(e) => run(request("set_network", { bot: bot.id, hosts: e.target.checked ? hostLines(hosts) : null }))} /> Commands reach only these hosts (otherwise any host)
      </label>
      {bot.network !== null && (
        <form onSubmit={(e) => { e.preventDefault(); run(request("set_network", { bot: bot.id, hosts: hostLines(hosts) })); }}>
          <textarea rows={4} placeholder="example.com" value={hosts} onChange={(e) => setHosts(e.target.value)} />
          <button>Save hosts</button>
          <p className="note">{hostsHint} The hosts of the bot's project are added. A tool that ignores the proxy Bate sets cannot connect. The bot's model is reached as before. Needs the folder limit (not the whole disk).</p>
        </form>
      )}
      {bot.browserTaken && (
        <p>
          You have the bot's browser. <button onClick={() => run(request("hand_back_browser", { bot: bot.id }))}>Hand it back</button>
        </p>
      )}
      <h2>Engine</h2>
      <form onSubmit={(e) => { e.preventDefault(); run(request("set_agent", { bot: bot.id, command: words.length ? words : null })); }}>
        <label>Installed agent (empty: Bate's own) <input placeholder="opencode acp" value={agent} onChange={(e) => setAgent(e.target.value)} /></label>
        <button>Save</button>
      </form>
      <h2>Delete</h2>
      {deleting ? (
        <p>
          Remove {bot.name} from Bate and move its folder to the Trash?{" "}
          <button onClick={() => run(request("delete_bot", { bot: bot.id }).then(onDeleted))}>Delete</button>{" "}
          <button onClick={() => setDeleting(false)}>Cancel</button>
        </p>
      ) : (
        <button onClick={() => setDeleting(true)}>Delete this bot…</button>
      )}
      {error && <p className="error" role="alert">{error}</p>}
    </section>
  );
}

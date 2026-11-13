import { useEffect, useState } from "react";
import { listen, request } from "./service.ts";
import { KINDS, type Bot, type Level, type Permissions } from "./types.ts";

const ACTIONS = { read: "Read files", write: "Change files", run: "Run commands", connector: "Use connectors" };

export function Settings({ bot }: { bot: Bot }) {
  const [permissions, setPermissions] = useState<Permissions>();
  const [agent, setAgent] = useState((bot.agent ?? []).join(" "));
  const [report, setReport] = useState("");
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
      <h2>What the bot may do</h2>
      {permissions &&
        KINDS.map((kind) => (
          <label key={kind}>
            {ACTIONS[kind]}{" "}
            <select value={permissions.levels[kind]} onChange={(e) => run(request("set_permission", { bot: bot.id, kind, level: e.target.value as Level }))}>
              <option value="always">always</option>
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
        <input type="checkbox" checked={bot.wholeDisk} onChange={(e) => run(request("set_disk_access", { bot: bot.id, wholeDisk: e.target.checked }))} /> Whole disk (otherwise only the bot's folder)
      </label>
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
      <h2>Bug report</h2>
      <button onClick={() => run(request<{ path: string }>("export_report").then((d) => setReport(d.path)))}>Export a bug report</button>
      {report && <p>Saved to <code>{report}</code></p>}
      {error && <p className="error" role="alert">{error}</p>}
    </section>
  );
}

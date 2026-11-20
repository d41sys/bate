import { useEffect, useState } from "react";
import { Confirm } from "./Confirm.tsx";
import { Field, Problem, Saved, Toggle, useSaves } from "./Form.tsx";
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
  const saves = useSaves();
  const { run, edit, saved, error } = saves;

  useEffect(() => {
    const load = () =>
      request<Permissions>("get_permissions", { bot: bot.id }).then(setPermissions, (e) => saves.fail("load", e.message));
    load();
    return listen((frame) => {
      if (frame.type === "permissions_changed" && frame.bot === bot.id) load();
    });
  }, [bot.id]);

  const words = agent.trim().split(/\s+/).filter(Boolean);
  const change = (next: Partial<typeof look>) => (edit("look"), setLook({ ...look, ...next }));

  return (
    <>
      <section>
        <h2>The bot</h2>
        <form className="fields stack" onSubmit={(e) => { e.preventDefault(); run("look", request("set_bot", { bot: bot.id, ...look })); }}>
          <Field id="bot-name" label="Name">
            <input required maxLength={80} value={look.name} onChange={(e) => change({ name: e.target.value })} />
          </Field>
          <Field id="bot-instructions" label="Instructions">
            <textarea maxLength={8000} rows={6} placeholder="What this bot is for and how it should answer" value={look.instructions} onChange={(e) => change({ instructions: e.target.value })} />
          </Field>
          <Field id="bot-avatar" label="Avatar">
            <input maxLength={8} placeholder="🙂" value={look.avatar} onChange={(e) => change({ avatar: e.target.value })} />
          </Field>
          <Field id="bot-steps" label="Tool rounds per turn" hint="A turn stops after this many; the chat then offers Continue.">
            <input type="number" required min={1} max={1000} step={1} value={look.stepLimit} onChange={(e) => change({ stepLimit: Number(e.target.value) })} />
          </Field>
          <div className="field">
            <label htmlFor="bot-color">Colour</label>
            <div className="find">
              <input id="bot-color" type="color" value={look.color || "#888888"} onChange={(e) => change({ color: e.target.value })} />
              {look.color && <button type="button" onClick={() => change({ color: "" })}>No colour</button>}
            </div>
          </div>
          <Problem text={error("look")} />
          <div className="actions">
            <button>Save</button>
            <Saved on={saved("look")} />
          </div>
        </form>
      </section>
      <section>
        <h2>What the bot may do</h2>
        {!permissions && !error("load") && <p className="note">Loading…</p>}
        <Problem text={error("load")} />
        {permissions && (
          <ul className="rows">
            {KINDS.map((kind) => (
              <li key={kind}>
                <label className="what" htmlFor={`perm-${kind}`}>{ACTIONS[kind]}</label>
                <div className="actions">
                  <select id={`perm-${kind}`} value={permissions.levels[kind]} onChange={(e) => { edit(`perm-${kind}`); run(`perm-${kind}`, request("set_permission", { bot: bot.id, kind, level: e.target.value as Level })); }}>
                    {kind !== "apple_change" && <option value="always">always</option>}
                    <option value="ask">ask me</option>
                    <option value="never">never</option>
                  </select>
                  <Saved on={saved(`perm-${kind}`)} />
                </div>
                {error(`perm-${kind}`) && <small className="error" role="alert">{error(`perm-${kind}`)}</small>}
              </li>
            ))}
          </ul>
        )}
        {permissions && (
          <>
            <h3>Always allowed</h3>
            {permissions.grants.length === 0 && <p className="note">Nothing is always allowed yet. Choose Always on a question and it shows here.</p>}
            <ul className="rows">
              {permissions.grants.map((g) => (
                <li key={`${g.kind} ${g.target}`}>
                  <div className="what">
                    <strong>{ACTIONS[g.kind]}</strong>
                    <div className="note"><code>{g.target}</code></div>
                  </div>
                  <div className="actions">
                    <button className="small" aria-label={`Remove: ${ACTIONS[g.kind]}, ${g.target}`} onClick={() => run("grants", request("delete_grant", { bot: bot.id, kind: g.kind, target: g.target }))}>Remove</button>
                  </div>
                </li>
              ))}
            </ul>
            <Problem text={error("grants")} />
          </>
        )}
      </section>
      <section>
        <h2>Files and network</h2>
        <Toggle
          label="Whole disk (otherwise only the bot's folder)"
          checked={bot.wholeDisk}
          disabled={bot.network !== null}
          hint={bot.network !== null ? "Off while commands reach only some hosts." : undefined}
          saved={saved("disk")}
          error={error("disk")}
          onChange={(wholeDisk) => run("disk", request("set_disk_access", { bot: bot.id, wholeDisk }))}
        />
        <Toggle
          label="Commands reach only these hosts (otherwise any host)"
          checked={bot.network !== null}
          disabled={bot.wholeDisk}
          hint={bot.wholeDisk ? "Needs the folder limit: turn off the whole disk first." : undefined}
          saved={saved("network")}
          error={error("network")}
          onChange={(on) => run("network", request("set_network", { bot: bot.id, hosts: on ? hostLines(hosts) : null }))}
        />
        {bot.network !== null && (
          <form className="fields stack" onSubmit={(e) => { e.preventDefault(); run("hosts", request("set_network", { bot: bot.id, hosts: hostLines(hosts) })); }}>
            <Field
              id="bot-hosts"
              label="Hosts"
              hint={`${hostsHint} The hosts of the bot's project are added. A tool that ignores the proxy Bate sets cannot connect. The bot's model is reached as before. Needs the folder limit (not the whole disk).`}
              error={error("hosts")}
            >
              <textarea rows={4} placeholder="example.com" value={hosts} onChange={(e) => (edit("hosts"), setHosts(e.target.value))} />
            </Field>
            <div className="actions">
              <button>Save hosts</button>
              <Saved on={saved("hosts")} />
            </div>
          </form>
        )}
      </section>
      {bot.browserTaken && (
        <section>
          <h2>Browser</h2>
          <p>You have the bot's browser.</p>
          <div className="actions">
            <button onClick={() => run("browser", request("hand_back_browser", { bot: bot.id }))}>Hand it back</button>
          </div>
          <Problem text={error("browser")} />
        </section>
      )}
      <section>
        <h2>Engine</h2>
        <form className="fields stack" onSubmit={(e) => { e.preventDefault(); run("agent", request("set_agent", { bot: bot.id, command: words.length ? words : null })); }}>
          <Field id="bot-agent" label="Installed agent" hint="Empty: Bate's own." error={error("agent")}>
            <input placeholder="opencode acp" value={agent} onChange={(e) => (edit("agent"), setAgent(e.target.value))} />
          </Field>
          <div className="actions">
            <button>Save</button>
            <Saved on={saved("agent")} />
          </div>
        </form>
      </section>
      <section>
        <h2>Delete</h2>
        <button className="danger" onClick={() => setDeleting(true)}>Delete this bot…</button>
        {deleting && (
          <Confirm title="Delete this bot?" action="Delete" go={() => request("delete_bot", { bot: bot.id }).then(onDeleted)} close={() => setDeleting(false)}>
            <p>Remove {bot.name} from Bate and move its folder to the Trash?</p>
          </Confirm>
        )}
      </section>
    </>
  );
}

import { useEffect, useState } from "react";
import { agentChoice, OTHER, OWN, type Agent, type Found } from "./agents.ts";
import { Confirm } from "./Confirm.tsx";
import { CodeBlock, Group, Problem, Saved, Setting, useSaves } from "./Form.tsx";
import { inShell, listen, request } from "./service.ts";
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
  const savedAgent = (bot.agent ?? []).join(" ");
  const [agent, setAgent] = useState(savedAgent);
  // The user chose Other… and has not saved a command yet; `undefined` until the service has said what is installed.
  const [other, setOther] = useState(false);
  const [found, setFound] = useState<Found>();
  useEffect(() => {
    setAgent(savedAgent);
    setOther(false);
  }, [bot.id, savedAgent]);
  useEffect(() => {
    request<Found>("list_agents").then(setFound, () => setFound({ agents: [], missing: [] }));
  }, []);
  const saves = useSaves();
  const { run, edit, saved, error } = saves;
  // Whether bots run on the ChatGPT plan; only Bate's own window may read the sign-in, as in Model.
  const [onPlan, setOnPlan] = useState<boolean>();
  useEffect(() => {
    if (!inShell) return;
    const load = () => request<{ signedIn: boolean }>("get_chatgpt").then((c) => setOnPlan(c.signedIn), () => {});
    load();
    return listen((frame) => frame.type === "chatgpt_changed" && load());
  }, []);

  useEffect(() => {
    const load = () =>
      request<Permissions>("get_permissions", { bot: bot.id }).then(setPermissions, (e) => saves.fail("load", e.message));
    load();
    return listen((frame) => {
      if (frame.type === "permissions_changed" && frame.bot === bot.id) load();
    });
  }, [bot.id]);

  const words = agent.trim().split(/\s+/).filter(Boolean);
  const choice = other ? OTHER : agentChoice(bot.agent, found?.agents ?? []);
  const chosen = found?.agents.find((a) => a.label === choice);
  const change = (next: Partial<typeof look>) => (edit("look"), setLook({ ...look, ...next }));

  return (
    <>
      <Group title="The bot">
        <form onSubmit={(e) => { e.preventDefault(); run("look", request("set_bot", { bot: bot.id, ...look })); }}>
          <Setting id="bot-name" title="Name">
            <input required maxLength={80} value={look.name} onChange={(e) => change({ name: e.target.value })} />
          </Setting>
          <Setting id="bot-instructions" title="Instructions" wide>
            <textarea maxLength={8000} rows={6} placeholder="What this bot is for and how it should answer" value={look.instructions} onChange={(e) => change({ instructions: e.target.value })} />
          </Setting>
          <Setting id="bot-avatar" title="Avatar">
            <input className="short" maxLength={8} placeholder="🙂" value={look.avatar} onChange={(e) => change({ avatar: e.target.value })} />
          </Setting>
          <Setting id="bot-steps" title="Tool rounds per turn" hint="A turn stops after this many; the chat then offers Continue.">
            <input className="short" type="number" required min={1} max={1000} step={1} value={look.stepLimit} onChange={(e) => change({ stepLimit: Number(e.target.value) })} />
          </Setting>
          <Setting id="bot-color" title="Colour">
            <input id="bot-color" type="color" value={look.color || "#888888"} onChange={(e) => change({ color: e.target.value })} />
            {look.color && <button type="button" className="small" onClick={() => change({ color: "" })}>No colour</button>}
          </Setting>
          <div className="actions">
            <button>Save</button>
            <Saved on={saved("look")} />
            <Problem text={error("look")} />
          </div>
        </form>
      </Group>
      <Group title="What the bot may do">
        {!permissions && !error("load") && <p className="note">Loading…</p>}
        <Problem text={error("load")} />
        {permissions &&
          KINDS.map((kind) => (
            <Setting key={kind} id={`perm-${kind}`} title={ACTIONS[kind]} saved={saved(`perm-${kind}`)} error={error(`perm-${kind}`)}>
              <select value={permissions.levels[kind]} onChange={(e) => { edit(`perm-${kind}`); run(`perm-${kind}`, request("set_permission", { bot: bot.id, kind, level: e.target.value as Level })); }}>
                {kind !== "apple_change" && <option value="always">always</option>}
                <option value="ask">ask me</option>
                <option value="never">never</option>
              </select>
            </Setting>
          ))}
      </Group>
      {permissions && (
        <Group title="Always allowed">
          {permissions.grants.length === 0 && <p className="note">Nothing yet. Choose Always on a question and it shows here.</p>}
          {permissions.grants.map((g) => (
            <Setting key={`${g.kind} ${g.target}`} title={ACTIONS[g.kind]} hint={<code>{g.target}</code>}>
              <button type="button" className="small" aria-label={`Remove: ${ACTIONS[g.kind]}, ${g.target}`} onClick={() => run("grants", request("delete_grant", { bot: bot.id, kind: g.kind, target: g.target }))}>Remove</button>
            </Setting>
          ))}
          <Problem text={error("grants")} />
        </Group>
      )}
      <Group title="Files and network">
        <Setting
          id="bot-disk"
          inline
          title="Whole disk (otherwise only the bot's folder)"
          hint={bot.network !== null ? "Off while commands reach only some hosts." : undefined}
          saved={saved("disk")}
          error={error("disk")}
        >
          <input type="checkbox" role="switch" checked={bot.wholeDisk} disabled={bot.network !== null} onChange={(e) => run("disk", request("set_disk_access", { bot: bot.id, wholeDisk: e.target.checked }))} />
        </Setting>
        <Setting
          id="bot-network"
          inline
          title="Commands reach only these hosts (otherwise any host)"
          hint={bot.wholeDisk ? "Needs the folder limit: turn off the whole disk first." : undefined}
          saved={saved("network")}
          error={error("network")}
        >
          <input type="checkbox" role="switch" checked={bot.network !== null} disabled={bot.wholeDisk} onChange={(e) => run("network", request("set_network", { bot: bot.id, hosts: e.target.checked ? hostLines(hosts) : null }))} />
        </Setting>
        {bot.network !== null && (
          <form onSubmit={(e) => { e.preventDefault(); run("hosts", request("set_network", { bot: bot.id, hosts: hostLines(hosts) })); }}>
            <Setting
              id="bot-hosts"
              title="Hosts"
              wide
              hint={`${hostsHint} The hosts of the bot's project are added. A tool that ignores the proxy Bate sets cannot connect. The bot's model is reached as before. Needs the folder limit (not the whole disk).`}
              error={error("hosts")}
            >
              <textarea rows={4} placeholder="example.com" value={hosts} onChange={(e) => (edit("hosts"), setHosts(e.target.value))} />
            </Setting>
            <div className="actions">
              <button>Save hosts</button>
              <Saved on={saved("hosts")} />
            </div>
          </form>
        )}
        <Setting
          id="bot-web-search"
          inline
          title="Search the web"
          hint={
            bot.network !== null
              ? "Off while commands reach only some hosts."
              : onPlan === false
                ? "Needs the ChatGPT sign-in: this bot runs on the model server, which has no search."
                : "The search runs at ChatGPT."
          }
          saved={saved("search")}
          error={error("search")}
        >
          <input type="checkbox" role="switch" checked={bot.webSearch && bot.network === null} disabled={bot.network !== null} onChange={(e) => run("search", request("set_bot", { bot: bot.id, webSearch: e.target.checked }))} />
        </Setting>
      </Group>
      {bot.browserTaken && (
        <Group title="Browser">
          <Setting title="You have the bot's browser" error={error("browser")}>
            <button type="button" onClick={() => run("browser", request("hand_back_browser", { bot: bot.id }))}>Hand it back</button>
          </Setting>
        </Group>
      )}
      {found && (
        <Group title="Engine">
          <Setting
            id="bot-engine"
            title="Engine"
            hint={chosen ? <code>{chosen.path}</code> : undefined}
            error={choice === OTHER ? undefined : error("agent")}
            saved={choice !== OTHER && saved("agent")}
          >
            <select
              value={choice}
              onChange={(e) => {
                edit("agent");
                setOther(e.target.value === OTHER);
                if (e.target.value !== OTHER) run("agent", request("set_agent", { bot: bot.id, command: found.agents.find((a) => a.label === e.target.value)?.command ?? null }));
              }}
            >
              <option value={OWN}>Bate's own engine</option>
              {found.agents.map((a) => <option key={a.label} value={a.label}>{a.label}</option>)}
              <option value={OTHER}>Other…</option>
            </select>
          </Setting>
          {choice === OTHER && (
            <form onSubmit={(e) => { e.preventDefault(); run("agent", request("set_agent", { bot: bot.id, command: words.length ? words : null })); }}>
              <Setting id="bot-agent" title="Command" hint="The program, then its arguments. It uses its own login, model and tools." error={error("agent")} saved={saved("agent")}>
                <input id="bot-agent" aria-describedby="bot-agent-hint" placeholder="opencode acp" value={agent} onChange={(e) => (edit("agent"), setAgent(e.target.value))} />
                <button>Save</button>
              </Setting>
            </form>
          )}
          {found.missing.map((m) => (
            <Setting key={m.label} title={`${m.label} (not installed)`} wide={!!m.install}>
              {m.install && <CodeBlock key={m.install} text={m.install} label={`the install command for ${m.label}`} />}
            </Setting>
          ))}
          {found.missing.length > 0 && <p className="note">Bate looks again each time these settings are opened.</p>}
        </Group>
      )}
      <Group title="Delete">
        <Setting title="Delete this bot" hint="Its folder moves to the Trash.">
          <button type="button" className="danger" onClick={() => setDeleting(true)}>Delete this bot…</button>
        </Setting>
        {deleting && (
          <Confirm title="Delete this bot?" action="Delete" go={() => request("delete_bot", { bot: bot.id }).then(onDeleted)} close={() => setDeleting(false)}>
            <p>Remove {bot.name} from Bate and move its folder to the Trash?</p>
          </Confirm>
        )}
      </Group>
    </>
  );
}

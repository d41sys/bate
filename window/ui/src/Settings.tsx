import { Fragment, useEffect, useState } from "react";
import { AdapterRow } from "./AdapterRow.tsx";
import { agentChoice, OTHER, OWN, type Agent, type Found } from "./agents.ts";
import { Confirm } from "./Confirm.tsx";
import { CodeBlock, Group, Problem, Saved, Setting, useSaves } from "./Form.tsx";
import { Revealed } from "./Reveal.tsx";
import { inShell, listen, request } from "./service.ts";
import { hostLines, hostsHint } from "./network.ts";
import { usedText } from "./usage.ts";
import { KINDS, type Bot, type Kind, type Level, type Permissions, type Usage } from "./types.ts";

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
  const [withNotes, setWithNotes] = useState(false);
  const [exported, setExported] = useState<{ path: string; check: { name: string; command: string }[] }>();
  useEffect(() => setExported(undefined), [bot.id]);
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
  const lookForAgents = () => request<Found>("list_agents").then(setFound, () => setFound({ agents: [], missing: [] }));
  useEffect(() => void lookForAgents(), []);
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

  const [usage, setUsage] = useState<Usage>();
  const [caps, setCaps] = useState({ day: "", month: "", stops: "unattended" as Usage["limit"]["stops"] });
  // `fill`: the limit boxes take what the service has (the bot changed, or the limits were just saved); else only the counts are renewed.
  const loadUsage = (fill: boolean) =>
    request<Usage>("get_usage", { bot: bot.id }).then((u) => {
      setUsage(u);
      if (fill) setCaps({ day: u.limit.day?.toString() ?? "", month: u.limit.month?.toString() ?? "", stops: u.limit.stops });
    }, (e: Error) => saves.fail("usage", e.message));
  useEffect(() => void loadUsage(true), [bot.id]);
  // A turn that ended has counted.
  useEffect(() => void loadUsage(false), [bot.busy]);

  // Only Bate's own window may list or remove a bot's secrets, as only it may type one.
  const [secrets, setSecrets] = useState<{ name: string; site: string }[]>([]);
  useEffect(() => {
    if (!inShell) return;
    const load = () => request<{ secrets: { name: string; site: string }[] }>("list_secrets", { bot: bot.id }).then((d) => setSecrets(d.secrets), (e) => saves.fail("secrets", e.message));
    load();
    return listen((frame) => {
      if (frame.type === "secrets_changed" && frame.bot === bot.id) load();
    });
  }, [bot.id]);

  const words = agent.trim().split(/\s+/).filter(Boolean);
  const choice = other ? OTHER : agentChoice(bot.agent, found?.agents ?? []);
  const chosen = found?.agents.find((a) => a.label === choice);
  const change = (next: Partial<typeof look>) => (edit("look"), setLook({ ...look, ...next }));

  return (
    <>
      <Group title="Pause">
        <Setting
          id="bot-paused"
          inline
          title={bot.paused ? "Paused" : "Pause this bot"}
          hint="Stops its turn, skips its routines and holds the messages other bots send it. It takes no message from you until you resume."
          saved={saved("paused")}
          error={error("paused")}
        >
          <button type="button" className="small" onClick={() => run("paused", request("set_paused", { bot: bot.id, paused: !bot.paused }))}>{bot.paused ? "Resume" : "Pause"}</button>
        </Setting>
      </Group>
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
      <Group title="Tokens">
        {usage && (
          <>
            {(["today", "month"] as const).map((period) => (
              <Setting key={period} title={period === "today" ? "Used today" : "Used this month"} hint={usedText(usage[period]).hint}>
                <span>{usedText(usage[period]).value}</span>
              </Setting>
            ))}
            {bot.agent && <p className="note">This bot runs on an installed agent: it is counted only if the agent says what it used, and a limit does not stop it.</p>}
            {inShell ? (
              <form onSubmit={(e) => { e.preventDefault(); run("limits", request("set_token_limit", { bot: bot.id, day: caps.day.trim() ? Number(caps.day) : null, month: caps.month.trim() ? Number(caps.month) : null, stops: caps.stops }).then(() => loadUsage(true))); }}>
                <Setting id="bot-limit-day" title="Limit per day" hint="Input and output tokens, counted from midnight. Empty: no limit.">
                  <input className="short" type="number" min={1} step={1} placeholder="No limit" value={caps.day} onChange={(e) => (edit("limits"), setCaps({ ...caps, day: e.target.value }))} />
                </Setting>
                <Setting id="bot-limit-month" title="Limit per month" hint="Counted from the 1st. Empty: no limit.">
                  <input className="short" type="number" min={1} step={1} placeholder="No limit" value={caps.month} onChange={(e) => (edit("limits"), setCaps({ ...caps, month: e.target.value }))} />
                </Setting>
                <Setting id="bot-limit-stops" title="The limit stops" hint="Unattended work is routines and messages from other bots and from channels. A turn that reaches the limit ends with the reason; a routine is skipped.">
                  <select value={caps.stops} onChange={(e) => (edit("limits"), setCaps({ ...caps, stops: e.target.value as Usage["limit"]["stops"] }))}>
                    <option value="unattended">Unattended work</option>
                    <option value="everything">Everything, your messages too</option>
                  </select>
                </Setting>
                <div className="actions">
                  <button>Save limits</button>
                  <Saved on={saved("limits")} />
                  <Problem text={error("limits")} />
                </div>
              </form>
            ) : (
              <p className="note">{usage.limit.day === null && usage.limit.month === null ? "No limit is set." : "A limit is set."} Limits are set in Bate's own window.</p>
            )}
          </>
        )}
        <Problem text={error("usage")} />
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
            <Setting key={`${g.kind} ${g.target}`} title={ACTIONS[g.kind]} hint={<code><Revealed text={g.target} /></code>}>
              <button type="button" className="small" aria-label={`Remove: ${ACTIONS[g.kind]}, ${g.target}`} onClick={() => run("grants", request("delete_grant", { bot: bot.id, kind: g.kind, target: g.target }))}>Remove</button>
            </Setting>
          ))}
          <Problem text={error("grants")} />
        </Group>
      )}
      {permissions && permissions.conversationGrants.length > 0 && (
        <Group title="Allowed in one conversation">
          {permissions.conversationGrants.map((g) => (
            <Setting key={`${g.conversation} ${g.kind} ${g.target}`} title={ACTIONS[g.kind]} hint={<><code><Revealed text={g.target} /></code> in {g.title ? `“${g.title}”` : "a conversation with no title yet"}. It ends with that conversation.</>}>
              <button type="button" className="small" aria-label={`Remove: ${ACTIONS[g.kind]}, ${g.target}, in one conversation`} onClick={() => run("grants", request("delete_grant", { bot: bot.id, kind: g.kind, target: g.target, conversation: g.conversation }))}>Remove</button>
            </Setting>
          ))}
        </Group>
      )}
      {permissions && permissions.blocked.length > 0 && (
        <Group title="Blocked tools">
          {bot.agent && <p className="note">This bot runs on an installed agent, which uses its connectors itself: blocking does not reach it.</p>}
          {permissions.blocked.map((tool) => (
            <Setting key={tool} title="Use connectors" hint={<><code>{tool}</code>: not offered to the bot, and refused if called.</>}>
              <button type="button" className="small" aria-label={`Unblock ${tool}`} onClick={() => run("blocked", request("block_tool", { bot: bot.id, tool, blocked: false }))}>Unblock</button>
            </Setting>
          ))}
          <Problem text={error("blocked")} />
        </Group>
      )}
      {inShell && (
        <Group title="Secrets">
          {secrets.length === 0 && <p className="note">Nothing yet. A bot asks for one in a card when a page needs a sign-in.</p>}
          {secrets.map((s) => (
            <Setting key={s.name} title={s.name} hint={<>For <code>{s.site}</code>. The bot types it there and never sees it.</>}>
              <button type="button" className="small" aria-label={`Remove the secret ${s.name} for ${s.site}`} onClick={() => run("secrets", request("remove_secret", { bot: bot.id, name: s.name }))}>Remove</button>
            </Setting>
          ))}
          <Problem text={error("secrets")} />
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
          {found.agents.map((a) => a.adapter?.version != null && <AdapterRow key={a.label} label={a.label} adapter={a.adapter} reload={lookForAgents} />)}
          {found.missing.map((m) => (
            <Fragment key={m.label}>
              <Setting title={`${m.label} (not installed)`} wide={!!m.install}>
                {m.install && <CodeBlock key={m.install} text={m.install} label={`the install command for ${m.label}`} />}
              </Setting>
              {m.adapter && <AdapterRow label={m.label} adapter={m.adapter} reload={lookForAgents} />}
            </Fragment>
          ))}
          {found.missing.length > 0 && <p className="note">Bate looks again each time these settings are opened.</p>}
        </Group>
      )}
      <Group title="Export">
        <Setting id="bot-export-notes" inline title="With notes" hint="Its memory notes go in the file too.">
          <input type="checkbox" role="switch" checked={withNotes} onChange={(e) => setWithNotes(e.target.checked)} />
        </Setting>
        <Setting
          title="Export bot"
          hint="One file to import as a new bot. No sign-ins, keys or chats."
          saved={saved("export")}
          error={error("export")}
        >
          <button type="button" onClick={() => run("export", request<NonNullable<typeof exported>>("export_bot", { bot: bot.id, notes: withNotes }).then(setExported))}>Export bot</button>
        </Setting>
        {exported && <p className="note" role="status">Saved to <code>{exported.path}</code></p>}
        {exported && exported.check.length > 0 && (
          <>
            <p className="note">Before you share it, check these commands for keys:</p>
            <ul className="rows">
              {exported.check.map((c) => (
                <li key={c.name}>
                  <div className="what">
                    <strong>{c.name}</strong>
                    <code>{c.command}</code>
                  </div>
                </li>
              ))}
            </ul>
          </>
        )}
      </Group>
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

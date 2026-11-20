import { useState } from "react";
import { type ConnectorForm, connectorMay, connectorOf, connectorState, connectorSummary, each, madeBy, placed } from "./addons.ts";
import { Choose } from "./ProjectDialog.tsx";
import { RowMenu } from "./RowMenu.tsx";
import { Sheet } from "./Sheet.tsx";
import { usePerBot } from "./usePerBot.ts";
import { inShell, openInBrowser, request } from "./service.ts";
import type { Bot, Connector } from "./types.ts";

const blank: ConnectorForm = { name: "", type: "http", url: "", command: "", args: "", oauth: false };
type Ask = { form: ConnectorForm } | { copy: Connector; from: string; has: string[] } | { remove: string; users: string[] };
const address = (c: Connector) => (c.transport.type === "http" ? c.transport.url : [c.transport.command, ...(c.transport.args ?? [])].join(" "));
/** What `set_connector` takes: the output-only fields are left out. */
const sent = ({ error, signedIn, ...c }: Connector) => c;

/** The connectors of every bot in one list: is each on, does it work, who has it; add to more bots in one step. */
export function Connectors({ bots }: { bots: Bot[] }) {
  const { lists, error: loadError } = usePerBot<Connector>(bots, "list_connectors", "connectors", "connectors_changed");
  // One row for a connector the bots have alike; one that differs in address or sign-in gets its own.
  const rows = placed(lists, (c) => JSON.stringify([c.name, c.transport, !!c.oauth]));
  const [ask, setAsk] = useState<Ask>();
  const [form, setForm] = useState(blank);
  const [error, setError] = useState("");
  const botName = (id: string) => bots.find((b) => b.id === id)?.name ?? id;
  const hasName = (bot: string, name: string) => lists.find((l) => l.bot === bot)?.items.some((c) => c.name === name);

  /** Whether it worked; if not, the error shows. */
  const run = (p: Promise<unknown>) => p.then(() => (setError(""), true), (e: Error) => (setError(e.message), false));
  const signIn = (bot: string, name: string) => run(request<{ url: string }>("sign_in_connector", { bot, name }).then((d) => openInBrowser(d.url)));
  const who = (c: Connector, taken: string[] = []) => ({
    legend: "Add for",
    items: bots.map((b) => ({ id: b.id, bot: b, label: b.name, note: taken.includes(b.id) ? "Has it already" : hasName(b.id, c.name) ? "Has one of this name: this replaces it" : undefined, off: taken.includes(b.id) })),
  });
  const put = (c: Connector) => (ids: string[]) => each(ids, (bot) => request("set_connector", { bot, connector: sent(c) }));
  const draft = connectorOf(form);

  return (
    <section>
      <div className="head">
        <h2>Installed</h2>
        <button className="small" onClick={() => (setForm(blank), setAsk({ form: blank }))}>New connector…</button>
      </div>
      {rows.length === 0 && <p className="note">No connectors yet.</p>}
      <ul className="rows">
        {rows.map((r) => {
          const sum = connectorSummary(r.users.map((u) => u.item), bots.length);
          return (
            <li key={`${r.name}${address(r.users[0].item)}`}>
              <div className="what">
                <strong>{r.name}</strong>
                <small><code>{address(r.users[0].item)}</code></small>
              </div>
              <div className="actions">
                <RowMenu
                  label={r.name}
                  items={[
                    { label: "Add to bots…", run: () => setAsk({ copy: r.users[0].item, from: botName(r.users[0].bot), has: r.users.map((u) => u.bot) }) },
                    { label: "Remove…", danger: true, run: () => setAsk({ remove: r.name, users: r.users.map((u) => u.bot) }) },
                  ]}
                />
              </div>
              <details>
                <summary>
                  {sum.text}
                  {sum.problems > 0 && <span className="error"> · {sum.problems} to look at</span>}
                </summary>
                <ul className="rows">
                  {r.users.map(({ bot, item }) => {
                    const state = connectorState(item);
                    return (
                      <li key={bot}>
                        <label>
                          <input type="checkbox" role="switch" checked={item.enabled} onChange={(e) => void run(request("set_connector", { bot, connector: { ...sent(item), enabled: e.target.checked } }))} />
                          {botName(bot)}
                        </label>
                        <small className={state.problem ? "error" : undefined}>{state.text}</small>
                        {item.oauth &&
                          (item.signedIn ? (
                            <button className="small" onClick={() => void run(request("sign_out_connector", { bot, name: item.name }))}>Sign out</button>
                          ) : (
                            <button className="small" onClick={() => void signIn(bot, item.name)}>Sign in</button>
                          ))}
                      </li>
                    );
                  })}
                </ul>
              </details>
            </li>
          );
        })}
      </ul>
      {(error || loadError) && <p className="error" role="alert">{error || loadError}</p>}
      {ask && "form" in ask && (
        <Sheet
          title="New connector"
          maker={madeBy.typed}
          may={connectorMay(draft)}
          who={who(draft)}
          action="Add"
          go={put(draft)}
          close={() => setAsk(undefined)}
        >
          <label>Name <input required maxLength={40} pattern="[a-z0-9_\-]{1,40}" title="a-z, 0-9, _ and -" value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} /></label>
          <label>
            Reached
            <select value={form.type} onChange={(e) => setForm({ ...form, type: e.target.value as ConnectorForm["type"] })}>
              <option value="http">over the web</option>
              <option value="stdio">by running a program</option>
            </select>
          </label>
          {form.type === "http" ? (
            <>
              <label>Address <input type="url" required placeholder="https://example.com/mcp" value={form.url} onChange={(e) => setForm({ ...form, url: e.target.value })} /></label>
              <label><input type="checkbox" checked={form.oauth} onChange={(e) => setForm({ ...form, oauth: e.target.checked })} /> The server signs me in</label>
            </>
          ) : (
            <>
              <label>Program <input required placeholder="npx" value={form.command} onChange={(e) => setForm({ ...form, command: e.target.value })} /></label>
              {inShell && <Choose folder={false} set={(command) => setForm((f) => ({ ...f, command }))} fail={setError} />}
              <label>
                Arguments, one on each line
                <textarea rows={3} value={form.args} onChange={(e) => setForm({ ...form, args: e.target.value })} />
              </label>
            </>
          )}
        </Sheet>
      )}
      {ask && "copy" in ask && (
        <Sheet
          title={`Add ${ask.copy.name} to more bots`}
          about={address(ask.copy)}
          maker={madeBy.copy(ask.from)}
          facts={ask.copy.oauth ? ["It signs in on its own for each bot: you sign in once per bot."] : undefined}
          may={connectorMay(ask.copy)}
          who={who(ask.copy, ask.has)}
          action="Add"
          go={put(ask.copy)}
          close={() => setAsk(undefined)}
        />
      )}
      {ask && "remove" in ask && (
        <Sheet
          title={`Remove ${ask.remove}`}
          about="This removes the connector from each bot ticked: its saved address, headers and environment, the sign-in if it has one, and the Always answers given for its tools. The server or program itself is not touched."
          who={{ legend: "Remove from", items: ask.users.map((id) => ({ id, bot: bots.find((b) => b.id === id), label: botName(id) })), ticked: ask.users }}
          action="Remove"
          danger
          go={(ids) => each(ids, (bot) => request("delete_connector", { bot, name: ask.remove }))}
          close={() => setAsk(undefined)}
        />
      )}
    </section>
  );
}

import { useEffect, useState } from "react";
import { listen, openInBrowser, request } from "./service.ts";
import type { Connector } from "./types.ts";

const blank = { name: "", type: "http" as "http" | "stdio", url: "", command: "", oauth: false };

export function Connectors({ bot }: { bot: string }) {
  const [connectors, setConnectors] = useState<Connector[]>([]);
  const [form, setForm] = useState(blank);
  const [error, setError] = useState("");

  useEffect(() => {
    const load = () =>
      request<{ connectors: Connector[] }>("list_connectors", { bot }).then((d) => setConnectors(d.connectors), (e) => setError(e.message));
    load();
    return listen((frame) => {
      if (frame.type === "connectors_changed" && frame.bot === bot) load();
    });
  }, [bot]);

  const run = (p: Promise<unknown>) => p.then(() => setError(""), (e: Error) => setError(e.message));
  const save = (connector: Connector) => run(request("set_connector", { bot, connector }));
  const signIn = (name: string) =>
    run(request<{ url: string }>("sign_in_connector", { bot, name }).then((d) => openInBrowser(d.url)));
  const create = async () => {
    // shortcut: a command is split at spaces, so no argument may hold one; add an argument list when that matters.
    const [command, ...args] = form.command.trim().split(/\s+/);
    await save({
      name: form.name,
      enabled: true,
      transport: form.type === "http" ? { type: "http", url: form.url.trim() } : { type: "stdio", command, args },
      oauth: form.type === "http" && form.oauth,
    });
    setForm(blank);
  };

  return (
    <section>
      <ul className="rows">
        {connectors.map((c) => (
          <li key={c.name}>
            <label>
              <input type="checkbox" checked={c.enabled} onChange={(e) => save({ ...c, enabled: e.target.checked })} /> {c.name}
            </label>
            <code>{c.transport.type === "http" ? c.transport.url : [c.transport.command, ...(c.transport.args ?? [])].join(" ")}</code>
            {c.oauth &&
              (c.signedIn ? (
                <button onClick={() => run(request("sign_out_connector", { bot, name: c.name }))}>Sign out</button>
              ) : (
                <button onClick={() => signIn(c.name)}>Sign in</button>
              ))}
            <button onClick={() => run(request("delete_connector", { bot, name: c.name }))}>Delete</button>
          </li>
        ))}
      </ul>
      <form className="fields" onSubmit={(e) => { e.preventDefault(); create(); }}>
        <h2>New connector</h2>
        <label>Name <input required pattern="[a-z0-9_\-]{1,40}" title="a-z, 0-9, _ and -" value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} /></label>
        <label>
          Reached{" "}
          <select value={form.type} onChange={(e) => setForm({ ...form, type: e.target.value as "http" | "stdio" })}>
            <option value="http">over the web</option>
            <option value="stdio">by running a program</option>
          </select>
        </label>
        {form.type === "http" ? (
          <>
            <label>Address <input type="url" required value={form.url} onChange={(e) => setForm({ ...form, url: e.target.value })} /></label>
            <label><input type="checkbox" checked={form.oauth} onChange={(e) => setForm({ ...form, oauth: e.target.checked })} /> The server signs me in</label>
          </>
        ) : (
          <label>Command <input required value={form.command} onChange={(e) => setForm({ ...form, command: e.target.value })} /></label>
        )}
        <button>Add</button>
      </form>
      {error && <p className="error" role="alert">{error}</p>}
    </section>
  );
}

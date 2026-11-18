import { useEffect, useState } from "react";
import { listen, request } from "./service.ts";
import type { Bot, Group } from "./types.ts";

const blank = (bots: Bot[]): Group => ({ name: "", lead: bots[0]?.id ?? "", members: [] });

export function Groups({ bots }: { bots: Bot[] }) {
  const [groups, setGroups] = useState<Group[]>([]);
  const [form, setForm] = useState<Group>(blank(bots));
  const [deleting, setDeleting] = useState<string>();
  const [error, setError] = useState("");

  useEffect(() => {
    const load = () => request<{ groups: Group[] }>("list_groups").then((d) => setGroups(d.groups), (e) => setError(e.message));
    load();
    return listen((frame) => {
      if (frame.type === "groups_changed") load();
    });
  }, []);

  const name = (id: string) => {
    const b = bots.find((o) => o.id === id);
    return b ? `${b.avatar ? `${b.avatar} ` : ""}${b.name}` : "(deleted bot)";
  };

  /** Whether it worked; if not, the error shows. */
  const run = (p: Promise<unknown>) =>
    p.then(() => (setError(""), true), (e: Error) => (setError(e.message), false));
  const save = async () => {
    if (await run(request("set_group", { group: form }))) setForm(blank(bots));
  };
  const setLead = (lead: string) => setForm({ ...form, lead, members: form.members.filter((m) => m !== lead) });
  const toggle = (id: string, on: boolean) =>
    setForm({ ...form, members: on ? [...form.members, id] : form.members.filter((m) => m !== id) });

  return (
    <section>
      <p className="note">Bots in a group can message each other, and the lead hands out the work.</p>
      {groups.length === 0 && <p className="note">No groups yet.</p>}
      <ul className="rows">
        {groups.map((g) => (
          <li key={g.id}>
            <strong>{g.name}</strong>
            <span className="note">Lead: {name(g.lead)} · Members: {g.members.map(name).join(", ")}</span>
            <button onClick={() => setForm(g)}>Edit</button>
            {deleting === g.id ? (
              <span>
                Delete {g.name}?{" "}
                <button onClick={() => run(request("delete_group", { groupId: g.id })).then(() => setDeleting(undefined))}>Delete</button>{" "}
                <button onClick={() => setDeleting(undefined)}>Cancel</button>
              </span>
            ) : (
              <button onClick={() => setDeleting(g.id)}>Delete…</button>
            )}
          </li>
        ))}
      </ul>
      {bots.length < 2 ? (
        <p className="note">A group needs at least two bots.</p>
      ) : (
        <form className="fields" onSubmit={(e) => { e.preventDefault(); save(); }}>
          <h2>{form.id ? "Edit group" : "New group"}</h2>
          <label>Name <input required maxLength={80} value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} /></label>
          <label>
            Lead{" "}
            <select value={form.lead} onChange={(e) => setLead(e.target.value)}>
              {bots.map((b) => <option key={b.id} value={b.id}>{name(b.id)}</option>)}
            </select>
          </label>
          <fieldset>
            <legend>Members</legend>
            {bots.filter((b) => b.id !== form.lead).map((b) => (
              <label key={b.id}>
                <input type="checkbox" checked={form.members.includes(b.id)} onChange={(e) => toggle(b.id, e.target.checked)} /> {name(b.id)}
              </label>
            ))}
          </fieldset>
          <button disabled={form.members.length === 0 || form.members.length > 20}>{form.id ? "Save" : "Add"}</button>
          {form.id && <button type="button" onClick={() => setForm(blank(bots))}>Cancel</button>}
        </form>
      )}
      {error && <p className="error" role="alert">{error}</p>}
    </section>
  );
}

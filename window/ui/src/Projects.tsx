import { useEffect, useState } from "react";
import { listen, request } from "./service.ts";
import type { Bot, Project } from "./types.ts";

const blank: Project = { name: "", folder: "", bots: [] };

export function Projects({ bots }: { bots: Bot[] }) {
  const [projects, setProjects] = useState<Project[]>([]);
  const [form, setForm] = useState<Project>(blank);
  const [deleting, setDeleting] = useState<string>();
  const [error, setError] = useState("");

  useEffect(() => {
    const load = () => request<{ projects: Project[] }>("list_projects").then((d) => setProjects(d.projects), (e) => setError(e.message));
    load();
    return listen((frame) => {
      if (frame.type === "projects_changed") load();
    });
  }, []);

  const name = (id: string) => {
    const b = bots.find((o) => o.id === id);
    return b ? `${b.avatar ? `${b.avatar} ` : ""}${b.name}` : "(deleted bot)";
  };
  /** The other project a bot is in: a bot is in at most one. */
  const elsewhere = (id: string) => projects.find((p) => p.id !== form.id && p.bots.includes(id));

  /** Whether it worked; if not, the error shows. */
  const run = (p: Promise<unknown>) =>
    p.then(() => (setError(""), true), (e: Error) => (setError(e.message), false));
  const save = async () => {
    if (await run(request("set_project", { project: form }))) setForm(blank);
  };
  const toggle = (id: string, on: boolean) =>
    setForm({ ...form, bots: on ? [...form.bots, id] : form.bots.filter((b) => b !== id) });

  return (
    <section>
      <p className="note">The bots in a project work in the project's folder and can message each other. A bot kept in its folder can read and change everything in the project's folder.</p>
      {projects.length === 0 && <p className="note">No projects yet.</p>}
      <ul className="rows">
        {projects.map((p) => (
          <li key={p.id}>
            <strong>{p.name}</strong>
            <span className="note"><code>{p.folder}</code> · Bots: {p.bots.map(name).join(", ") || "none"}</span>
            <button onClick={() => setForm(p)}>Edit</button>
            {deleting === p.id ? (
              <span>
                Delete {p.name}? Its folder and its bots stay.{" "}
                <button onClick={() => run(request("delete_project", { projectId: p.id })).then(() => setDeleting(undefined))}>Delete</button>{" "}
                <button onClick={() => setDeleting(undefined)}>Cancel</button>
              </span>
            ) : (
              <button onClick={() => setDeleting(p.id)}>Delete…</button>
            )}
          </li>
        ))}
      </ul>
      <form className="fields" onSubmit={(e) => { e.preventDefault(); save(); }}>
        <h2>{form.id ? "Edit project" : "New project"}</h2>
        <label>Name <input required maxLength={80} value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} /></label>
        <label>
          Folder{" "}
          <input size={50} placeholder="~/code/my-site" value={form.folder} onChange={(e) => setForm({ ...form, folder: e.target.value })} />
        </label>
        {!form.id && <p className="note">The folder's full path. Leave it empty and Bate makes one.</p>}
        <fieldset>
          <legend>Bots</legend>
          {bots.map((b) => {
            const other = elsewhere(b.id);
            return (
              <label key={b.id}>
                <input type="checkbox" disabled={!!other} checked={form.bots.includes(b.id)} onChange={(e) => toggle(b.id, e.target.checked)} /> {name(b.id)}
                {other && <span className="note"> (in {other.name})</span>}
              </label>
            );
          })}
        </fieldset>
        <button disabled={form.bots.length > 20}>{form.id ? "Save" : "Add"}</button>
        {form.id && <button type="button" onClick={() => setForm(blank)}>Cancel</button>}
      </form>
      {error && <p className="error" role="alert">{error}</p>}
    </section>
  );
}

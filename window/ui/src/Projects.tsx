import { useEffect, useState } from "react";
import { listen, request } from "./service.ts";
import type { Bot, Project, Role } from "./types.ts";

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
            {p.bots.some((id) => bots.find((b) => b.id === id)?.agent) && (
              <span className="note">
                {" "}· {p.bots.filter((id) => bots.find((b) => b.id === id)?.agent).map(name).join(", ")}: an installed agent gets neither the project's instructions nor its notes.
              </span>
            )}
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
            <Roles project={p} />
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
        <label>
          Instructions for all its bots, after their own{" "}
          <textarea rows={6} maxLength={8000} value={form.instructions ?? ""} onChange={(e) => setForm({ ...form, instructions: e.target.value })} />
        </label>
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

/** Adds ready-made bots to the project from role files (format: docs/roles.md). */
function Roles({ project }: { project: Project }) {
  const [paths, setPaths] = useState("");
  const [roles, setRoles] = useState<Role[]>();
  const [raise, setRaise] = useState(false);
  const [error, setError] = useState("");
  const list = paths.split("\n").map((s) => s.trim()).filter(Boolean);
  const raised = (roles ?? []).flatMap((r) => r.raises.map((k) => `${r.name}: ${k}`));
  const fail = (e: Error) => (setRoles(undefined), setError(e.message));
  const preview = () =>
    request<{ roles: Role[] }>("read_roles", { paths: list }).then((d) => (setRoles(d.roles), setRaise(false), setError("")), fail);
  const add = () =>
    request("add_roles", { projectId: project.id, paths: list, raise }).then(() => (setPaths(""), setRoles(undefined), setError("")), fail);

  return (
    <details>
      <summary>Add bots from role files</summary>
      <label>
        Role files, one whole path per line{" "}
        <textarea rows={3} placeholder="/Users/you/roles/reviewer.md" value={paths} onChange={(e) => (setPaths(e.target.value), setRoles(undefined))} />
      </label>
      <button type="button" disabled={!list.length} onClick={preview}>Show what they create</button>
      {roles && (
        <>
          <ul>
            {roles.map((r) => (
              <li key={r.path}>
                <strong>{r.name}</strong>
                {r.model && ` · ${r.model}${r.effort ? ` (${r.effort})` : ""}`}
                {r.skills.length > 0 && ` · skills: ${r.skills.join(", ")}`}
                {Object.keys(r.levels).length > 0 && ` · ${Object.entries(r.levels).map(([k, v]) => `${k}: ${v}`).join(", ")}`}
              </li>
            ))}
          </ul>
          {raised.length > 0 && (
            <label>
              <input type="checkbox" checked={raise} onChange={(e) => setRaise(e.target.checked)} /> Let them do without asking: {raised.join("; ")}. Left unchecked, these stay on ask.
            </label>
          )}
          <button type="button" onClick={add}>Create {roles.length === 1 ? "1 bot" : `${roles.length} bots`} in {project.name}</button>
        </>
      )}
      {error && <p className="error" role="alert">{error}</p>}
    </details>
  );
}

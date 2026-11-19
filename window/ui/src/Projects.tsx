import { useEffect, useState } from "react";
import { listen, request } from "./service.ts";
import type { Bot, Project, Role, Workplace } from "./types.ts";
import { Workplaces } from "./Workplaces.tsx";
import { needsLines, unusable } from "./workplaces.ts";

const blank: Project = { name: "", folder: "", bots: [] };

export function Projects({ bots }: { bots: Bot[] }) {
  const [projects, setProjects] = useState<Project[]>([]);
  const [form, setForm] = useState<Project>(blank);
  const [workplaces, setWorkplaces] = useState<Workplace[]>([]);
  /** The person has read what the chosen workplace needs. */
  const [confirmed, setConfirmed] = useState(false);
  const [deleting, setDeleting] = useState<string>();
  const [error, setError] = useState("");
  /** The connector's program and arguments, as one line to type. */
  const [line, setLine] = useState("");

  useEffect(() => {
    const load = () => request<{ projects: Project[] }>("list_projects").then((d) => setProjects(d.projects), (e) => setError(e.message));
    const loadWorkplaces = () =>
      request<{ workplaces: Workplace[] }>("list_workplaces").then((d) => setWorkplaces(d.workplaces), (e) => setError(e.message));
    load();
    loadWorkplaces();
    return listen((frame) => {
      if (frame.type === "projects_changed") load();
      if (frame.type === "workplaces_changed" || frame.type === "projects_changed") loadWorkplaces();
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
  const edit = (p: Project) => {
    setForm(p);
    setConfirmed(false);
    setLine(p.connector ? [p.connector.command, ...p.connector.args].join(" ") : "");
  };
  const save = async () => {
    const [command, ...args] = line.trim().split(/\s+/);
    const connector = line.trim() && !form.workplace ? { name: form.connector?.name ?? "", command, args } : undefined;
    if (await run(request("set_project", { project: { ...form, connector, connectorError: undefined } }))) edit(blank);
  };
  const saved = projects.find((p) => p.id === form.id);
  const chosen = workplaces.find((w) => w.name === form.workplace);
  /** A workplace that this project does not have on yet: the person confirms what it needs first. */
  const switchingOn = !!chosen && saved?.workplace !== chosen.name;
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
            {p.connector && (
              <span className="note"> · Connector <code>{p.connector.name}</code>: <code>{[p.connector.command, ...p.connector.args].join(" ")}</code></span>
            )}
            {p.workplace && <span className="note"> · Workplace <code>{p.workplace}</code></span>}
            {p.connectorError && <span className="error" role="alert"> · {p.connectorError}</span>}
            <button onClick={() => edit(p)}>Edit</button>
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
          <legend>Workplace</legend>
          <label>
            Workplace{" "}
            <select value={form.workplace ?? ""} onChange={(e) => (setConfirmed(false), setForm({ ...form, workplace: e.target.value || undefined }))}>
              <option value="">None</option>
              {workplaces.map((w) => (
                <option key={w.name} value={w.name} disabled={!!unusable(w) && w.name !== saved?.workplace}>
                  {w.name} {w.version}{unusable(w) ? " (cannot be switched on now)" : ""}
                </option>
              ))}
            </select>
          </label>
          {chosen && (
            <>
              <p className="note">{chosen.name} needs: {needsLines(chosen).join("; ")}.</p>
              <p className="note">Its program runs as this project's connector, and its skills go to every bot of the project.</p>
              {switchingOn && (
                <label>
                  <input type="checkbox" checked={confirmed} onChange={(e) => setConfirmed(e.target.checked)} /> I have read what {chosen.name} needs, and I switch it on for this project
                </label>
              )}
            </>
          )}
          {form.workplace && <p className="note">A project has a workplace or a connector of its own, not both.</p>}
        </fieldset>
        <fieldset disabled={!!form.workplace}>
          <legend>Connector for all its bots</legend>
          <label>
            Name{" "}
            <input maxLength={40} pattern="[a-z0-9_\-]*" placeholder="notes" value={form.connector?.name ?? ""} onChange={(e) => setForm({ ...form, connector: { command: "", args: [], ...form.connector, name: e.target.value } })} />
          </label>
          <label>
            Program and arguments{" "}
            <input size={50} placeholder="python3 server.py" value={line} onChange={(e) => setLine(e.target.value)} />
          </label>
          <p className="note">Bate runs this program once for the project, limited to the project's folder and a folder of its own, and every bot of the project gets its tools. It serves MCP over HTTP on the unix socket named in <code>BATE_MCP_SOCKET</code>. Arguments are split at spaces; leave the program empty for no connector.</p>
        </fieldset>
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
        <button disabled={form.bots.length > 20 || (switchingOn && !confirmed)}>{form.id ? "Save" : "Add"}</button>
        {form.id && <button type="button" onClick={() => edit(blank)}>Cancel</button>}
      </form>
      {error && <p className="error" role="alert">{error}</p>}
      <Workplaces workplaces={workplaces} projects={projects} />
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

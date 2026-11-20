import { useState } from "react";
import { request } from "./service.ts";
import type { Bot, Project, Role } from "./types.ts";
import { useWorkplaces } from "./Workplaces.tsx";
import { hostLines, hostsHint } from "./network.ts";
import { needsLines, unusable } from "./workplaces.ts";

const blank: Project = { name: "", folder: "", bots: [], hosts: [] };
const label = (b?: Bot) => (b ? `${b.avatar ? `${b.avatar} ` : ""}${b.name}` : "(deleted bot)");
/** What `set_project` takes: the output-only field is left out. */
const sent = (p: Project) => ({ ...p, connectorError: undefined });

/** The projects: each opens to its own page. A project is made here, with no bot open. */
export function Projects({ bots, projects, open }: { bots: Bot[]; projects: Project[]; open: (id: string) => void }) {
  const name = (id: string) => label(bots.find((o) => o.id === id));
  return (
    <section>
      <p className="note">The bots in a project work in the project's folder and can message each other. Open a project to see its room, its bots and its settings.</p>
      {projects.length === 0 && <p className="note">No projects yet.</p>}
      <ul className="rows">
        {projects.map((p) => (
          <li key={p.id}>
            <strong>{p.name}</strong>
            <span className="note"><code>{p.folder}</code> · Bots: {p.bots.map(name).join(", ") || "none"}</span>
            {p.connectorError && <span className="error" role="alert"> · {p.connectorError}</span>}
            <button onClick={() => open(p.id!)}>Open</button>
          </li>
        ))}
      </ul>
      <ProjectForm bots={bots} onSaved={(p) => open(p.id!)} />
    </section>
  );
}

/** A new project (no `project`), or the settings of one: its name, folder, instructions, lead, hosts, workplace or connector. */
export function ProjectForm({ project, bots, onSaved }: { project?: Project; bots: Bot[]; onSaved?: (p: Project) => void }) {
  const [form, setForm] = useState<Project>(project ?? blank);
  const { workplaces, error: loadError } = useWorkplaces();
  /** The person has read what the chosen workplace needs. */
  const [confirmed, setConfirmed] = useState(false);
  const [error, setError] = useState("");
  /** The connector's program and arguments, as one line to type. */
  const [line, setLine] = useState(project?.connector ? [project.connector.command, ...project.connector.args].join(" ") : "");
  /** The hosts added for its limited bots, one per line. */
  const [hosts, setHosts] = useState((project?.hosts ?? []).join("\n"));

  const save = async () => {
    const [command, ...args] = line.trim().split(/\s+/);
    const connector = line.trim() && !form.workplace ? { name: form.connector?.name ?? "", command, args } : undefined;
    // Its bots are the project's own now: the Bots page changes them.
    try {
      const { project: saved } = await request<{ project: Project }>("set_project", { project: { ...sent(form), bots: project?.bots ?? [], hosts: hostLines(hosts), connector } });
      setError("");
      setConfirmed(false);
      if (!project) {
        setForm(blank);
        setHosts("");
        setLine("");
      }
      onSaved?.(saved);
    } catch (e) {
      setError((e as Error).message);
    }
  };
  const chosen = workplaces.find((w) => w.name === form.workplace);
  /** A workplace that this project does not have on yet: the person confirms what it needs first. */
  const switchingOn = !!chosen && project?.workplace !== chosen.name;

  return (
    <form className="fields" onSubmit={(e) => { e.preventDefault(); save(); }}>
      <h2>{project ? "Settings" : "New project"}</h2>
      {project?.connectorError && <p className="error" role="alert">{project.connectorError}</p>}
      <label>Name <input required maxLength={80} value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} /></label>
      <label>
        Folder{" "}
        <input size={50} placeholder="~/code/my-site" value={form.folder} onChange={(e) => setForm({ ...form, folder: e.target.value })} />
      </label>
      {!project && <p className="note">The folder's full path. Leave it empty and Bate makes one.</p>}
      <label>
        Instructions for all its bots, after their own{" "}
        <textarea rows={6} maxLength={8000} value={form.instructions ?? ""} onChange={(e) => setForm({ ...form, instructions: e.target.value })} />
      </label>
      <label>
        Hosts its bots with a network limit may also reach{" "}
        <textarea rows={3} placeholder="example.com" value={hosts} onChange={(e) => setHosts(e.target.value)} />
      </label>
      <p className="note">{hostsHint}</p>
      <fieldset>
        <legend>Workplace</legend>
        <label>
          Workplace{" "}
          <select value={form.workplace ?? ""} onChange={(e) => (setConfirmed(false), setForm({ ...form, workplace: e.target.value || undefined }))}>
            <option value="">None</option>
            {workplaces.map((w) => (
              <option key={w.name} value={w.name} disabled={!!unusable(w) && w.name !== project?.workplace}>
                {w.name} {w.version}{unusable(w) ? " (cannot be switched on now)" : ""}
              </option>
            ))}
          </select>
        </label>
        {chosen && (
          <>
            <p className="note">{chosen.name} needs: {needsLines(chosen).join("; ")}.</p>
            <p className="note">Its program runs as this project's connector and reaches only the network hosts listed above; its skills go to every bot of the project.</p>
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
      {project && (
        <label>
          Lead, who gets a room message with no @name{" "}
          <select value={form.lead ?? ""} onChange={(e) => setForm({ ...form, lead: e.target.value || undefined })}>
            <option value="">The first bot</option>
            {project.bots.map((id) => <option key={id} value={id}>{label(bots.find((b) => b.id === id))}</option>)}
          </select>
        </label>
      )}
      <button disabled={switchingOn && !confirmed}>{project ? "Save" : "Add"}</button>
      {(error || loadError) && <p className="error" role="alert">{error || loadError}</p>}
    </form>
  );
}

/** The bots of a project: who is in, and ready-made ones from role files. A bot is in at most one project. */
export function ProjectBots({ project, bots }: { project: Project; bots: Bot[] }) {
  const [error, setError] = useState("");
  const toggle = (id: string, on: boolean) =>
    request("set_project", {
      project: { ...sent(project), bots: on ? [...project.bots, id] : project.bots.filter((b) => b !== id), lead: project.lead === id && !on ? undefined : project.lead },
    }).then(() => setError(""), (e: Error) => setError(e.message));
  const agents = project.bots.map((id) => bots.find((b) => b.id === id)).filter((b) => b?.agent);

  return (
    <section className="fields">
      <p className="note">The bots in a project work in the project's folder and can message each other. A bot kept in its folder can read and change everything in the project's folder.</p>
      {agents.length > 0 && (
        <p className="note">{agents.map(label).join(", ")}: an installed agent gets neither the project's instructions nor its notes.</p>
      )}
      <fieldset>
        <legend>Bots</legend>
        {bots.length === 0 && <p className="note">No bots yet. Make one in the sidebar, or add some from role files below.</p>}
        {bots.map((b) => (
          <label key={b.id}>
            <input
              type="checkbox"
              disabled={(!!b.project && b.project.id !== project.id) || (!project.bots.includes(b.id) && project.bots.length >= 20)}
              checked={project.bots.includes(b.id)}
              onChange={(e) => toggle(b.id, e.target.checked)}
            /> {label(b)}
            {b.project && b.project.id !== project.id && <span className="note"> (in {b.project.name})</span>}
          </label>
        ))}
      </fieldset>
      <Roles project={project} />
      {error && <p className="error" role="alert">{error}</p>}
    </section>
  );
}

/** Deletes the project, and its bots too when asked: several commands, not one transaction. */
export function DeleteProject({ project, bots, gone }: { project: Project; bots: Bot[]; gone: () => void }) {
  const [asking, setAsking] = useState(false);
  const [withBots, setWithBots] = useState(false);
  const [error, setError] = useState("");
  const members = bots.filter((b) => b.project?.id === project.id);
  const remove = async () => {
    try {
      // The bots first: if one cannot go, the project is still there to show why.
      if (withBots) for (const b of members) await request("delete_bot", { bot: b.id });
      await request("delete_project", { projectId: project.id });
      gone();
    } catch (e) {
      setError((e as Error).message);
    }
  };
  return (
    <section>
      <h2>Delete</h2>
      {asking ? (
        <>
          <p>Delete {project.name}? Its folder stays.</p>
          {members.length > 0 && (
            <label>
              <input type="checkbox" checked={withBots} onChange={(e) => setWithBots(e.target.checked)} /> Also delete its {members.length === 1 ? "bot" : `${members.length} bots`} ({members.map(label).join(", ")}) and move {members.length === 1 ? "its folder" : "their folders"} to the Trash. Left unticked, {members.length === 1 ? "it stays" : "they stay"} in no project.
            </label>
          )}
          <div className="actions">
            <button className="danger" onClick={remove}>Delete</button>
            <button onClick={() => setAsking(false)}>Cancel</button>
          </div>
        </>
      ) : (
        <button className="danger" onClick={() => setAsking(true)}>Delete this project…</button>
      )}
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

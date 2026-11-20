import { useState } from "react";
import { request } from "./service.ts";
import type { Bot, Project } from "./types.ts";
import { Choose, ProjectDialog } from "./ProjectDialog.tsx";
import { sent } from "./projects.ts";
import { useWorkplaces } from "./Workplaces.tsx";
import { hostLines, hostsHint } from "./network.ts";
import { needsLines, unusable } from "./workplaces.ts";

const label = (b?: Bot) => (b ? `${b.avatar ? `${b.avatar} ` : ""}${b.name}` : "(deleted bot)");

/** The projects: each opens to its own page. A project is made here, in a dialog, with no bot open. */
export function Projects({ bots, projects, open }: { bots: Bot[]; projects: Project[]; open: (id: string) => void }) {
  const [adding, setAdding] = useState(false);
  const name = (id: string) => label(bots.find((o) => o.id === id));
  return (
    <section>
      <p className="note">The bots in a project work in the project's folder and can message each other. Open a project to see its room, its bots and its settings.</p>
      <button onClick={() => setAdding(true)}>New project…</button>
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
      {adding && <ProjectDialog bots={bots} done={(p) => open(p.id!)} close={() => setAdding(false)} />}
    </section>
  );
}

/** The settings of a project: its name, folder, instructions, lead, hosts, workplace or connector. */
export function ProjectForm({ project, bots }: { project: Project; bots: Bot[] }) {
  const [form, setForm] = useState<Project>(project);
  const { workplaces, error: loadError } = useWorkplaces();
  /** The person has read what the chosen workplace needs. */
  const [confirmed, setConfirmed] = useState(false);
  const [error, setError] = useState("");
  /** The connector's program and arguments, as one line to type. */
  const [line, setLine] = useState(project.connector ? [project.connector.command, ...project.connector.args].join(" ") : "");
  /** The hosts added for its limited bots, one per line. */
  const [hosts, setHosts] = useState((project.hosts ?? []).join("\n"));

  const save = async () => {
    const [command, ...args] = line.trim().split(/\s+/);
    const connector = line.trim() && !form.workplace ? { name: form.connector?.name ?? "", command, args } : undefined;
    // Its bots are the project's own now: the Bots page changes them.
    try {
      await request<{ project: Project }>("set_project", { project: { ...sent(form), bots: project.bots, hosts: hostLines(hosts), connector } });
      setError("");
      setConfirmed(false);
    } catch (e) {
      setError((e as Error).message);
    }
  };
  const chosen = workplaces.find((w) => w.name === form.workplace);
  /** A workplace that this project does not have on yet: the person confirms what it needs first. */
  const switchingOn = !!chosen && project.workplace !== chosen.name;

  return (
    <form className="fields" onSubmit={(e) => { e.preventDefault(); save(); }}>
      <h2>Settings</h2>
      {project.connectorError && <p className="error" role="alert">{project.connectorError}</p>}
      <label>Name <input required maxLength={80} value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} /></label>
      <div className="field">
        <label htmlFor="project-folder">Folder</label>
        <div className="find">
          <input id="project-folder" placeholder="Pick one, or type its whole path" value={form.folder} onChange={(e) => setForm({ ...form, folder: e.target.value })} />
          <Choose folder set={(folder) => setForm({ ...form, folder })} fail={setError} />
        </div>
      </div>
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
              <option key={w.name} value={w.name} disabled={!!unusable(w) && w.name !== project.workplace}>
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
      <label>
        Lead, who gets a room message with no @name{" "}
        <select value={form.lead ?? ""} onChange={(e) => setForm({ ...form, lead: e.target.value || undefined })}>
          <option value="">The first bot</option>
          {project.bots.map((id) => <option key={id} value={id}>{label(bots.find((b) => b.id === id))}</option>)}
        </select>
      </label>
      <button disabled={switchingOn && !confirmed}>Save</button>
      {(error || loadError) && <p className="error" role="alert">{error || loadError}</p>}
    </form>
  );
}

/** The bots of a project, and a dialog to add, remove or make them. A bot is in at most one project. */
export function ProjectBots({ project, bots }: { project: Project; bots: Bot[] }) {
  const [editing, setEditing] = useState(false);
  const agents = project.bots.map((id) => bots.find((b) => b.id === id)).filter((b) => b?.agent);

  return (
    <section>
      <p className="note">The bots in a project work in the project's folder and can message each other. A bot kept in its folder can read and change everything in the project's folder.</p>
      {agents.length > 0 && (
        <p className="note">{agents.map(label).join(", ")}: an installed agent gets neither the project's instructions nor its notes.</p>
      )}
      {project.bots.length === 0 && <p className="empty">No bots in this project yet.</p>}
      <ul className="rows">
        {project.bots.map((id) => <li key={id}>{label(bots.find((b) => b.id === id))}</li>)}
      </ul>
      <button onClick={() => setEditing(true)}>Add or remove bots…</button>
      {editing && <ProjectDialog project={project} bots={bots} done={() => setEditing(false)} close={() => setEditing(false)} />}
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

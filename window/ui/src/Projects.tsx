import { useState } from "react";
import { request } from "./service.ts";
import type { Bot, Project } from "./types.ts";
import { Group, Setting } from "./Form.tsx";
import { Avatar } from "./Avatar.tsx";
import { Choose, ProjectDialog } from "./ProjectDialog.tsx";
import { sent } from "./projects.ts";
import { useWorkplaces } from "./Workplaces.tsx";
import { hostLines, hostsHint } from "./network.ts";
import { needsLines, unusable } from "./workplaces.ts";

const label = (b?: Bot) => (b ? `${b.avatar ? `${b.avatar} ` : ""}${b.name}` : "(deleted bot)");

/** The projects: each opens to its own page. A project is made here, in a dialog, with no bot open. */
export function Projects({ bots, projects, open, adding: asked }: { bots: Bot[]; projects: Project[]; open: (id: string) => void; adding: boolean }) {
  const [adding, setAdding] = useState(asked);
  const name = (id: string) => label(bots.find((o) => o.id === id));
  return (
    <section>
      <button onClick={() => setAdding(true)}>New project…</button>
      {projects.length === 0 && <p className="note">No projects yet.</p>}
      <ul className="rows">
        {projects.map((p) => (
          <li key={p.id}>
            <div className="what">
              <strong>{p.name}</strong>
              <small><code>{p.folder}</code></small>
              <small>Bots: {p.bots.map(name).join(", ") || "none"}</small>
              {p.connectorError && <small className="error" role="alert">{p.connectorError}</small>}
            </div>
            <div className="actions">
              <button className="small" onClick={() => open(p.id!)}>Open</button>
            </div>
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
    <form onSubmit={(e) => { e.preventDefault(); save(); }}>
      {project.connectorError && <p className="error" role="alert">{project.connectorError}</p>}
      <Group title="Project">
        <Setting id="project-name" title="Name">
          <input required maxLength={80} value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} />
        </Setting>
        <Setting id="project-folder" title="Folder" wide>
          <div className="find">
            <input id="project-folder" className="path" placeholder="Pick one, or type its whole path" value={form.folder} onChange={(e) => setForm({ ...form, folder: e.target.value })} />
            <Choose folder set={(folder) => setForm({ ...form, folder })} fail={setError} />
          </div>
        </Setting>
        <Setting id="project-instructions" title="Instructions for all its bots, after their own" wide>
          <textarea rows={6} maxLength={8000} value={form.instructions ?? ""} onChange={(e) => setForm({ ...form, instructions: e.target.value })} />
        </Setting>
        <Setting id="project-lead" title="Lead, who gets a room message with no @name">
          <select value={form.lead ?? ""} onChange={(e) => setForm({ ...form, lead: e.target.value || undefined })}>
            <option value="">The first bot</option>
            {project.bots.map((id) => <option key={id} value={id}>{label(bots.find((b) => b.id === id))}</option>)}
          </select>
        </Setting>
      </Group>
      <Group title="Network">
        <Setting id="project-hosts" title="Hosts its bots with a network limit may also reach" hint={hostsHint} wide>
          <textarea rows={3} placeholder="example.com" value={hosts} onChange={(e) => setHosts(e.target.value)} />
        </Setting>
      </Group>
      <Group title="Workplace">
        <Setting id="project-workplace" title="Workplace" hint={form.workplace ? "A project has a workplace or a connector of its own, not both." : undefined}>
          <select value={form.workplace ?? ""} onChange={(e) => (setConfirmed(false), setForm({ ...form, workplace: e.target.value || undefined }))}>
            <option value="">None</option>
            {workplaces.map((w) => (
              <option key={w.name} value={w.name} disabled={!!unusable(w) && w.name !== project.workplace}>
                {w.name} {w.version}{unusable(w) ? " (cannot be switched on now)" : ""}
              </option>
            ))}
          </select>
        </Setting>
        {chosen && (
          <p className="note">
            {chosen.name} needs: {needsLines(chosen).join("; ")}. Its program runs as this project's connector and reaches only the network hosts listed above; its skills go to every bot of the project.
          </p>
        )}
        {chosen && switchingOn && (
          <Setting id="project-confirm" title={`I have read what ${chosen.name} needs, and I switch it on for this project`} inline>
            <input type="checkbox" checked={confirmed} onChange={(e) => setConfirmed(e.target.checked)} />
          </Setting>
        )}
      </Group>
      <Group title="Connector for all its bots">
        <Setting id="project-connector-name" title="Name">
          <input disabled={!!form.workplace} maxLength={40} pattern="[a-z0-9_\-]*" placeholder="notes" value={form.connector?.name ?? ""} onChange={(e) => setForm({ ...form, connector: { command: "", args: [], ...form.connector, name: e.target.value } })} />
        </Setting>
        <Setting
          id="project-connector-line"
          title="Program and arguments"
          hint={<>Bate runs it once for the project, limited to the project's folder and a folder of its own, and every bot of the project gets its tools. It serves MCP over HTTP on the unix socket named in <code>BATE_MCP_SOCKET</code>. Arguments are split at spaces; leave it empty for no connector.</>}
          wide
        >
          <input disabled={!!form.workplace} placeholder="python3 server.py" value={line} onChange={(e) => setLine(e.target.value)} />
        </Setting>
      </Group>
      <div className="actions">
        <button disabled={switchingOn && !confirmed}>Save</button>
      </div>
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
      <p className="note">A bot kept in its folder can read and change everything in the project's folder.</p>
      {agents.length > 0 && (
        <p className="note">{agents.map(label).join(", ")}: an installed agent gets neither the project's instructions nor its notes.</p>
      )}
      {project.bots.length === 0 && <p className="empty">No bots in this project yet.</p>}
      <ul className="rows">
        {project.bots.map((id) => {
          const b = bots.find((x) => x.id === id);
          return <li key={id}>{b && <Avatar bot={b} />}{b?.name ?? "(deleted bot)"}</li>;
        })}
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
  const one = members.length === 1;
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
    <Group title="Delete">
      {asking ? (
        <>
          <p>Delete {project.name}? Its folder stays.</p>
          {members.length > 0 && (
            <Setting
              id="delete-bots"
              inline
              title={`Also delete its ${one ? "bot" : `${members.length} bots`} (${members.map(label).join(", ")}) and move ${one ? "its folder" : "their folders"} to the Trash`}
              hint={`Left unticked, ${one ? "it stays" : "they stay"} in no project.`}
            >
              <input type="checkbox" checked={withBots} onChange={(e) => setWithBots(e.target.checked)} />
            </Setting>
          )}
          <div className="actions">
            <button className="danger" onClick={remove}>Delete</button>
            <button onClick={() => setAsking(false)}>Cancel</button>
          </div>
        </>
      ) : (
        <Setting title="Delete this project" hint="Its folder stays.">
          <button className="danger" onClick={() => setAsking(true)}>Delete this project…</button>
        </Setting>
      )}
      {error && <p className="error" role="alert">{error}</p>}
    </Group>
  );
}

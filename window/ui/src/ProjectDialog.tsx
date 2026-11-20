import { useEffect, useRef, useState } from "react";
import { Avatar } from "./Avatar.tsx";
import { inShell, pickPath, request } from "./service.ts";
import { MAX_BOTS, findBots, folderName, sent, takenBy } from "./projects.ts";
import type { Bot, Project, Role } from "./types.ts";

/** A bot made with the project: from a name, or from a role file (format: docs/roles.md). */
type Fresh = { name: string; role?: Role };

/** The system's panel for a folder or a file, beside the box that takes a typed path. Only in Bate's own window. */
export function Choose({ folder, set, fail }: { folder: boolean; set: (path: string) => void; fail: (message: string) => void }) {
  if (!inShell) return null;
  const pick = () => pickPath(folder).then((p) => p && set(p), (e) => fail(String(e)));
  return <button type="button" onClick={pick}>Choose {folder ? "folder" : "file"}…</button>;
}

/**
 * A new project (no `project`): a folder and a name, and in the same dialog the bots that go in it, ticked
 * from a searchable list or made now. With a `project`: only the bots. Nothing is sent until Make/Save.
 */
export function ProjectDialog({ project, bots, done, close }: { project?: Project; bots: Bot[]; done: (p: Project) => void; close: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  /** The project, once it exists: a second try after a failure goes on with this one, never makes another. */
  const [made, setMade] = useState(project);
  const [folder, setFolder] = useState("");
  const [typed, setTyped] = useState("");
  const [ticked, setTicked] = useState(project?.bots ?? []);
  const [query, setQuery] = useState("");
  const [fresh, setFresh] = useState<Fresh[]>([]);
  const [botName, setBotName] = useState("");
  const [rolePath, setRolePath] = useState("");
  const [raise, setRaise] = useState(false);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const name = typed || folderName(folder);
  const full = ticked.length + fresh.length >= MAX_BOTS;
  const raised = fresh.flatMap((f) => (f.role?.raises ?? []).map((k) => `${f.name}: ${k}`));

  useEffect(() => {
    // Focus goes to the first field, and comes back where it was when the dialog closes.
    const before = document.activeElement as HTMLElement | null;
    const d = dialog.current!;
    if (!d.open) d.showModal();
    d.querySelector("input")?.focus();
    return () => before?.focus();
  }, []);

  const addName = () => {
    if (!botName.trim() || full) return;
    setFresh([...fresh, { name: botName.trim() }]);
    setBotName("");
  };
  const addRole = async (path: string) => {
    path = path.trim();
    if (!path || full || fresh.some((f) => f.role?.path === path)) return;
    try {
      const { roles } = await request<{ roles: Role[] }>("read_roles", { paths: [path] });
      setFresh((all) => [...all, { name: roles[0].name, role: roles[0] }]);
      setRolePath("");
      setError("");
    } catch (e) {
      setError((e as Error).message);
    }
  };

  const save = async () => {
    setBusy(true);
    setError("");
    try {
      const p = made ?? { name: name.trim(), folder: folder.trim(), bots: [], hosts: [] };
      let saved = (await request<{ project: Project }>("set_project", { project: { ...sent(p), bots: ticked, lead: p.lead && ticked.includes(p.lead) ? p.lead : undefined } })).project;
      setMade(saved);
      // A name makes the bot, then puts it in; one at a time, so a failure leaves the rest to try again.
      for (const f of fresh.filter((f) => !f.role)) {
        const { bot } = await request<{ bot: Bot }>("create_bot", { name: f.name });
        saved = (await request<{ project: Project }>("set_project", { project: { ...sent(saved), bots: [...saved.bots, bot.id] } })).project;
        setMade(saved);
        setTicked(saved.bots);
        setFresh((all) => all.filter((o) => o !== f));
      }
      const roles = fresh.filter((f) => f.role);
      if (roles.length) await request("add_roles", { projectId: saved.id, paths: roles.map((f) => f.role!.path), raise });
      done(saved);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <dialog ref={dialog} className="sheet" aria-labelledby="dialog-title" onClose={close} onClick={(e) => e.target === e.currentTarget && close()}>
      <form className="fields stack" onSubmit={(e) => { e.preventDefault(); void save(); }}>
        <h2 id="dialog-title">{project ? `Bots in ${project.name}` : made ? `Bots for ${made.name}` : "New project"}</h2>
        {!made && (
          <>
            <div className="field">
              <label htmlFor="project-folder">Folder</label>
              <div className="find">
                <input id="project-folder" placeholder="Pick one, or type its whole path" value={folder} onChange={(e) => setFolder(e.target.value)} />
                <Choose folder set={setFolder} fail={setError} />
              </div>
              <small>Its bots work in this folder. Leave it empty and Bate makes one.</small>
            </div>
            <label>Name <input required maxLength={80} value={name} onChange={(e) => setTyped(e.target.value)} /></label>
          </>
        )}
        <fieldset>
          <legend>Bots in it · {ticked.length + fresh.length} of {MAX_BOTS}</legend>
          {bots.length > 0 && <label>Find a bot <input type="search" value={query} onChange={(e) => setQuery(e.target.value)} /></label>}
          <ul className="rows">
            {findBots(bots, query).map((b) => {
              const other = takenBy(b, made);
              return (
                <li key={b.id}>
                  <label className="row">
                    <input
                      type="checkbox"
                      checked={ticked.includes(b.id)}
                      disabled={!!other || (!ticked.includes(b.id) && full)}
                      onChange={(e) => setTicked(e.target.checked ? [...ticked, b.id] : ticked.filter((id) => id !== b.id))}
                    />
                    <Avatar bot={b} />
                    <span className="label"><span>{b.name}</span>{other && <small>In {other}: a bot is in one project</small>}</span>
                  </label>
                </li>
              );
            })}
          </ul>
          {bots.length === 0 && <p className="note">No bots yet. Make one below.</p>}
          {bots.length > 0 && findBots(bots, query).length === 0 && <p className="note">No bot has that name.</p>}
        </fieldset>
        <fieldset>
          <legend>New bot in this project</legend>
          <div className="field">
            <label htmlFor="new-bot-name">Name</label>
            <div className="find">
              <input id="new-bot-name" maxLength={80} value={botName} onChange={(e) => setBotName(e.target.value)} onKeyDown={(e) => e.key === "Enter" && (e.preventDefault(), addName())} />
              <button type="button" disabled={!botName.trim() || full} onClick={addName}>Add</button>
            </div>
          </div>
          <div className="field">
            <label htmlFor="role-file">Or from a role file</label>
            <div className="find">
              <input id="role-file" placeholder="Pick one, or type its whole path" value={rolePath} onChange={(e) => setRolePath(e.target.value)} onKeyDown={(e) => e.key === "Enter" && (e.preventDefault(), void addRole(rolePath))} />
              <Choose folder={false} set={(p) => void addRole(p)} fail={setError} />
              <button type="button" disabled={!rolePath.trim() || full} onClick={() => void addRole(rolePath)}>Add</button>
            </div>
          </div>
          {fresh.length > 0 && (
            <ul className="rows" aria-label="Bots to make">
              {fresh.map((f, i) => (
                <li key={i}>
                  <span>{f.name}</span>
                  <small>{f.role ? "from a role file" : "new"}</small>
                  <button type="button" className="small ghost" aria-label={`Do not make ${f.name}`} onClick={() => setFresh(fresh.filter((o) => o !== f))}>Remove</button>
                </li>
              ))}
            </ul>
          )}
          {raised.length > 0 && (
            <label className="row">
              <input type="checkbox" checked={raise} onChange={(e) => setRaise(e.target.checked)} /> Let them do without asking: {raised.join("; ")}. Left unchecked, these stay on ask.
            </label>
          )}
        </fieldset>
        {error && <p className="error" role="alert">{error}</p>}
        <div className="actions">
          <button disabled={busy || (!made && !name.trim())}>{project ? "Save" : made ? "Finish" : "Make project"}</button>
          <button type="button" onClick={close}>Cancel</button>
        </div>
      </form>
    </dialog>
  );
}

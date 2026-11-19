import { useState } from "react";
import { request } from "./service.ts";
import type { Project, Workplace } from "./types.ts";
import { needsLines } from "./workplaces.ts";

/** Installed workplaces: install from a folder, look at what each needs, approve a changed one, remove. */
export function Workplaces({ workplaces, projects }: { workplaces: Workplace[]; projects: Project[] }) {
  const [path, setPath] = useState("");
  const [removing, setRemoving] = useState<string>();
  const [error, setError] = useState("");

  /** Whether it worked; if not, the error shows. */
  const run = (p: Promise<unknown>) =>
    p.then(() => (setError(""), true), (e: Error) => (setError(e.message), false));
  const install = async () => {
    // As for a skill: the service does not expand `~`.
    if (!path.startsWith("/")) return setError("Give the whole path, starting with /, for example /Users/you/Downloads/my-workplace.");
    if (await run(request("install_workplace", { path }))) setPath("");
  };
  const using = (w: Workplace) => w.usedBy.map((id) => projects.find((p) => p.id === id)?.name ?? id);

  return (
    <section>
      <h2>Workplaces</h2>
      <p className="note">A workplace equips a project for one kind of work: a program that Bate runs for the project, with skills and roles. Install one from a folder, then choose it in a project.</p>
      {workplaces.length === 0 && <p className="note">No workplaces installed.</p>}
      <ul className="rows">
        {workplaces.map((w) => (
          <li key={w.name}>
            <strong>{w.name}</strong>
            <span className="note"> {w.version}{w.manifest?.description ? ` · ${w.manifest.description}` : ""}</span>
            {w.manifest && <span className="note"> · Program: <code>{[w.manifest.program.command, ...w.manifest.program.args].join(" ")}</code></span>}
            <span className="note"> · Needs: {needsLines(w).join("; ")}</span>
            {w.skills.length > 0 && <span className="note"> · Skills: {w.skills.map((s) => s.name).join(", ")}</span>}
            {w.roles.length > 0 && (
              <span className="note"> · Roles (paths for “Add bots from role files”): {w.roles.map((r) => <code key={r}>{r} </code>)}</span>
            )}
            {w.usedBy.length > 0 && <span className="note"> · On in: {using(w).join(", ")}</span>}
            {w.error && <span className="error" role="alert"> · {w.error}</span>}
            {w.changed && !w.error && (
              <span className="error" role="alert"> · Its files changed since they were approved, so it is off in every project until you approve it.</span>
            )}
            {w.changed && <button onClick={() => run(request("approve_workplace", { name: w.name }))}>Approve the files as they are</button>}
            {removing === w.name ? (
              <span>
                {w.usedBy.length > 0
                  ? `${w.name} is on in ${using(w).join(", ")}. Remove it and switch it off there? What it kept for those projects stays.`
                  : `Remove ${w.name}?`}{" "}
                <button onClick={() => run(request("remove_workplace", { name: w.name, force: w.usedBy.length > 0 })).then(() => setRemoving(undefined))}>Remove</button>{" "}
                <button onClick={() => setRemoving(undefined)}>Cancel</button>
              </span>
            ) : (
              <button aria-label={`Remove ${w.name}`} onClick={() => setRemoving(w.name)}>Remove…</button>
            )}
          </li>
        ))}
      </ul>
      <form className="fields" onSubmit={(e) => { e.preventDefault(); install(); }}>
        <h3>Install a workplace</h3>
        <p className="note">
          A workplace is a folder on the Mac that runs Bate, with a <code>workplace.json</code> in it (format: <code>docs/workplaces.md</code>). It may hold up to 2000 files and folders and 200 MiB, and no links. Installing one with a name that is installed replaces it, unless a project has it on.
        </p>
        <label>
          Folder path <input required size={50} placeholder="/Users/you/Downloads/my-workplace" value={path} onChange={(e) => setPath(e.target.value)} />
        </label>
        <button>Install</button>
      </form>
      {error && <p className="error" role="alert">{error}</p>}
    </section>
  );
}

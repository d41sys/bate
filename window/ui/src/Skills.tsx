import { useEffect, useState } from "react";
import { listen, request } from "./service.ts";
import type { Skill } from "./types.ts";

export function Skills({ bot }: { bot: string }) {
  const [skills, setSkills] = useState<Skill[]>([]);
  const [path, setPath] = useState("");
  const [deleting, setDeleting] = useState<string>();
  const [error, setError] = useState("");

  useEffect(() => {
    const load = () =>
      request<{ skills: Skill[] }>("list_skills", { bot }).then((d) => setSkills(d.skills), (e) => setError(e.message));
    load();
    return listen((frame) => {
      if (frame.type === "skills_changed" && frame.bot === bot) load();
    });
  }, [bot]);

  /** Whether it worked; if not, the error shows. */
  const run = (p: Promise<unknown>) =>
    p.then(() => (setError(""), true), (e: Error) => (setError(e.message), false));
  const install = async () => {
    // The service does not expand `~` or look in a folder of its own: a short path would only fail with a puzzling message.
    if (!path.startsWith("/")) return setError("Give the whole path, starting with /, for example /Users/you/Downloads/my-skill.");
    if (await run(request("install_skill", { bot, path }))) setPath("");
  };

  return (
    <section>
      {skills.length === 0 && <p className="note">No skills yet.</p>}
      <ul className="rows">
        {skills.map((s) => (
          <li key={s.name}>
            <strong>{s.name}</strong>
            <span className="note">{s.description}</span>
            {deleting === s.name ? (
              <span>
                Delete {s.name}?{" "}
                <button onClick={() => run(request("delete_skill", { bot, name: s.name })).then(() => setDeleting(undefined))}>Delete</button>{" "}
                <button onClick={() => setDeleting(undefined)}>Cancel</button>
              </span>
            ) : (
              <button aria-label={`Delete ${s.name}`} onClick={() => setDeleting(s.name)}>Delete…</button>
            )}
          </li>
        ))}
      </ul>
      <form className="fields" onSubmit={(e) => { e.preventDefault(); install(); }}>
        <h2>Install a skill</h2>
        <p className="note">
          A skill is a folder on this Mac with a <code>SKILL.md</code> in it. The file starts with a <code>---</code> block holding a <code>name</code> (a-z, 0-9 and -) and a one-line <code>description</code>. Installing a skill with a name the bot already has replaces it. A folder may hold up to 500 files and folders and 20 MiB, and no links.
        </p>
        <label>
          Folder path <input required placeholder="/Users/you/Downloads/my-skill" value={path} onChange={(e) => setPath(e.target.value)} />
        </label>
        <button>Install</button>
      </form>
      {error && <p className="error" role="alert">{error}</p>}
    </section>
  );
}

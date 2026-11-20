import { type ComponentProps, useEffect, useState } from "react";
import { each, madeBy, matches } from "./addons.ts";
import { Choose } from "./ProjectDialog.tsx";
import { Sheet } from "./Sheet.tsx";
import { sent } from "./projects.ts";
import { inShell, listen, request } from "./service.ts";
import type { CatalogEntry, CatalogPreview, Project, Workplace, WorkplacePreview } from "./types.ts";
import { downloadText, needsLines, newlyLines, progressText, sizeText, unusable } from "./workplaces.ts";

/** The installed workplaces, read again when they or the projects change. */
export function useWorkplaces() {
  const [workplaces, setWorkplaces] = useState<Workplace[]>([]);
  const [error, setError] = useState("");
  useEffect(() => {
    const load = () =>
      request<{ workplaces: Workplace[] }>("list_workplaces").then((d) => setWorkplaces(d.workplaces), (e) => setError(e.message));
    load();
    return listen((frame) => {
      if (frame.type === "workplaces_changed" || frame.type === "projects_changed") load();
    });
  }, []);
  return { workplaces, error };
}

type SheetProps = Omit<ComponentProps<typeof Sheet>, "close">;

/** Workplaces: what is installed and which projects have each on; find one in the catalog or in a folder, and install it in one step. */
export function Workplaces({ projects }: { projects: Project[] }) {
  const { workplaces, error: loadError } = useWorkplaces();
  const [sheet, setSheet] = useState<SheetProps>();
  const [error, setError] = useState("");
  /** The bytes fetched so far and in all, while an install downloads. */
  const [progress, setProgress] = useState<[number, number]>();
  useEffect(() => listen((f) => f.type === "workplace_progress" && setProgress([f.done as number, f.total as number])), []);

  /** Whether it worked; if not, the error shows. */
  const run = (p: Promise<unknown>) => p.then(() => (setError(""), true), (e: Error) => (setError(e.message), false));
  const using = (w: Workplace) => w.usedBy.map((id) => projects.find((p) => p.id === id)?.name ?? id);
  const switchTo = (p: Project, workplace?: string) => request("set_project", { project: { ...sent(p), workplace } });
  /** Who may have a new workplace switched on at once: a project with a connector of its own cannot. */
  const who = (): SheetProps["who"] => ({
    legend: "Switch it on in",
    optional: true,
    items: projects.map((p) => ({ id: p.id!, label: p.name, off: !!p.connector, note: p.connector ? "Has a connector of its own: a project has a workplace or a connector, not both" : undefined })),
  });
  /** Installs, once (a second try after a project failed does not install again), then switches it on where ticked. */
  const installing = (name: string, install: () => Promise<unknown>): SheetProps["go"] => {
    let installed = false;
    return async (ids) => {
      if (!installed) {
        setProgress(undefined);
        await install().finally(() => setProgress(undefined));
        installed = true;
      }
      return each(ids, (id) => switchTo(projects.find((p) => p.id === id)!, name));
    };
  };

  // From a folder.
  const fromFolder = async (path: string) => {
    const preview = await request<WorkplacePreview>("preview_workplace", { path }).catch((e: Error) => (setError(e.message), undefined));
    if (!preview) return;
    const m = preview.manifest;
    const agree = downloadText(preview);
    setError("");
    setSheet({
      title: `Install ${m.name} ${m.version}`,
      about: m.description,
      maker: madeBy.folder,
      facts: [`Program: ${[m.program.command, ...m.program.args].join(" ")}`, "Installing one with the name of one that is installed replaces it, unless a project has it on."],
      may: needsLines({ manifest: m }),
      agree,
      who: who(),
      action: "Install",
      go: installing(m.name, () => request("install_workplace", { path, download: !!agree })),
    });
  };

  // From the catalog, read only when asked: reading it contacts GitHub.
  const [catalog, setCatalog] = useState<CatalogEntry[]>();
  const [query, setQuery] = useState("");
  const [reading, setReading] = useState(false);
  const loadCatalog = () => {
    setReading(true);
    return run(request<{ catalog: CatalogEntry[] }>("list_catalog").then((d) => setCatalog(d.catalog))).finally(() => setReading(false));
  };
  // What is installed changed (here or elsewhere): the entries say which are installed or have an update.
  const installed = workplaces.map((w) => `${w.name} ${w.version} ${w.archive}`).join();
  useEffect(() => {
    if (catalog) void loadCatalog();
  }, [installed]);
  const fromCatalog = async (entry: CatalogEntry) => {
    const preview = await request<CatalogPreview>("preview_catalog", { name: entry.name }).catch((e: Error) => (setError(e.message), undefined));
    if (!preview) return;
    const updating = entry.installedVersion;
    setError("");
    setSheet({
      title: updating ? `Update ${entry.name} to ${preview.entry.version}` : `Install ${entry.name} ${preview.entry.version}`,
      about: entry.description,
      maker: madeBy.catalog,
      facts: [`Archive of ${sizeText(entry.size)}, pinned by its hash`, ...(updating ? [`Installed now: ${updating}. What follows is only what it newly needs.`] : [])],
      may: newlyLines(preview.newly),
      agree: downloadText(preview),
      who: updating ? undefined : who(),
      action: updating ? "Update" : "Install",
      // The hash is the one shown: if the catalog changed meanwhile, the service refuses.
      go: installing(entry.name, () => request("install_catalog", { name: entry.name, sha256: preview.entry.sha256, download: (preview.manifest.downloads?.length ?? 0) > 0 })),
    });
  };

  const switchOn = (w: Workplace, p: Project) =>
    setSheet({
      title: `Switch ${w.name} on in ${p.name}`,
      about: "Its program runs as this project's connector and reaches only the network hosts it lists; its skills go to every bot of the project.",
      may: needsLines(w),
      action: "Switch on",
      go: async () => void (await switchTo(p, w.name)),
    });
  const remove = (w: Workplace) =>
    setSheet({
      title: `Remove ${w.name}`,
      about: w.usedBy.length
        ? `${w.name} is on in ${using(w).join(", ")}. Removing it deletes its folder and switches it off there; what it kept for those projects stays.`
        : `Removing ${w.name} deletes its folder. What it kept for projects stays.`,
      action: "Remove",
      danger: true,
      go: async () => void (await request("remove_workplace", { name: w.name, force: w.usedBy.length > 0 })),
    });

  return (
    <section>
      <p className="note">A workplace equips a project for one kind of work: a program that Bate runs for the project, with skills and roles. It is installed once and switched on per project.</p>
      {!inShell && <p className="note">Installing, removing and switching on workplaces is done in Bate's own window on the Mac; here you can look.</p>}
      <h2>Installed</h2>
      {workplaces.length === 0 && <p className="note">No workplaces installed.{inShell ? " Find one under Browse." : ""}</p>}
      <ul className="rows">
        {workplaces.map((w) => (
          <li key={w.name}>
            <div className="what">
              <strong>{w.name}</strong> <span className="note">{w.version}{w.manifest?.description ? ` · ${w.manifest.description}` : ""}</span>
              {w.manifest && <small>Program: <code>{[w.manifest.program.command, ...w.manifest.program.args].join(" ")}</code></small>}
              <small>Needs: {needsLines(w).join("; ")}</small>
              {w.skills.length > 0 && <small>Skills: {w.skills.map((s) => s.name).join(", ")}</small>}
              {w.roles.length > 0 && <small>Roles: {w.roles.map((r) => r.split("/").pop()).join(", ")}</small>}
              {w.archive && <small>From the catalog, pinned to <code>{w.archive.slice(0, 12)}</code></small>}
              {w.error && <small className="error" role="alert">{w.error}</small>}
              {w.changed && !w.error && <small className="error" role="alert">Its files changed since they were approved, so it is off in every project until you approve it.</small>}
            </div>
            <div className="actions">
              {inShell && w.changed && <button className="small" onClick={() => void run(request("approve_workplace", { name: w.name }))}>Approve the files as they are</button>}
              {inShell && <button className="small danger" aria-label={`Remove ${w.name}`} onClick={() => remove(w)}>Remove…</button>}
            </div>
            {!inShell && w.usedBy.length > 0 && <small>On in: {using(w).join(", ")}</small>}
            {inShell && projects.length > 0 && (
              <ul className="rows" aria-label={`Projects that can have ${w.name} on`}>
                {projects.map((p) => (
                  <li key={p.id}>
                    <label>
                      <input
                        type="checkbox"
                        role="switch"
                        checked={w.usedBy.includes(p.id!)}
                        disabled={!w.usedBy.includes(p.id!) && (!!unusable(w) || !!p.connector)}
                        onChange={(e) => (e.target.checked ? switchOn(w, p) : void run(switchTo(p)))}
                      />
                      {p.name}
                    </label>
                    {!w.usedBy.includes(p.id!) && p.connector && <small>Has a connector of its own</small>}
                  </li>
                ))}
              </ul>
            )}
          </li>
        ))}
      </ul>
      {inShell && (
        <>
          <h2>Browse</h2>
          <p className="note">
            The catalog is a list kept in Bate's repository on GitHub. Each entry pins the exact archive by its size and hash; an installed workplace stays at the version it was installed at, and an update is only offered, with what it newly needs. Reading the list contacts GitHub, so it waits until you ask.
          </p>
          <button disabled={reading} onClick={() => void loadCatalog()}>{catalog ? "Reload the catalog" : "Show the catalog"}</button>
          {catalog?.length === 0 && <p className="note">The catalog is empty.</p>}
          {catalog && catalog.length > 0 && (
            <div className="find">
              <input type="search" aria-label="Search the catalog" placeholder="Search the catalog" value={query} onChange={(e) => setQuery(e.target.value)} />
            </div>
          )}
          <ul className="rows">
            {catalog?.filter((e) => matches(query, e.name, e.description)).map((e) => (
              <li key={e.name}>
                <div className="what">
                  <strong>{e.name}</strong> <span className="note">{e.version} · {sizeText(e.size)}</span>
                  <small>{e.description}</small>
                  {e.installedVersion && <small>Installed: {e.installedVersion}{e.update ? "" : ", up to date"}</small>}
                </div>
                {(!e.installedVersion || e.update) && (
                  <div className="actions">
                    <button className="small" onClick={() => void fromCatalog(e)}>{e.installedVersion ? `Update to ${e.version}…` : "Install…"}</button>
                  </div>
                )}
              </li>
            ))}
          </ul>
          {catalog && query && !catalog.some((e) => matches(query, e.name, e.description)) && <p className="note">Nothing in the catalog matches.</p>}
          <h3>From a folder on this Mac</h3>
          <p className="note">
            A folder with a <code>workplace.json</code> in it (format: <code>docs/workplaces.md</code>), up to 2000 files and folders and 200 MiB, and no links. What it lists under <code>downloads</code> is fetched at install, after you have seen its size and hosts.
          </p>
          <Choose folder set={(path) => void fromFolder(path)} fail={setError} />
        </>
      )}
      {(error || loadError) && <p className="error" role="alert">{error || loadError}</p>}
      {sheet && <Sheet {...sheet} status={progress && progressText(...progress)} close={() => setSheet(undefined)} />}
    </section>
  );
}

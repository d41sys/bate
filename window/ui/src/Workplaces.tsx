import { useEffect, useState } from "react";
import { listen, request } from "./service.ts";
import type { CatalogEntry, CatalogPreview, Project, Workplace, WorkplacePreview } from "./types.ts";
import { downloadText, needsLines, newlyLines, progressText, sizeText } from "./workplaces.ts";

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

/** Installed workplaces: install from the catalog or a folder, look at what each needs, approve a changed one, remove. */
export function Workplaces({ projects }: { projects: Project[] }) {
  const { workplaces, error: loadError } = useWorkplaces();
  const [path, setPath] = useState("");
  const [removing, setRemoving] = useState<string>();
  const [error, setError] = useState("");

  /** Whether it worked; if not, the error shows. */
  const run = (p: Promise<unknown>) =>
    p.then(() => (setError(""), true), (e: Error) => (setError(e.message), false));
  /** What the person is asked to agree to before the downloads start. */
  const [asking, setAsking] = useState<string>();
  /** The bytes fetched so far and in all, while an install downloads. */
  const [progress, setProgress] = useState<[number, number]>();
  useEffect(
    () => listen((f) => f.type === "workplace_progress" && setProgress([f.done as number, f.total as number])),
    [],
  );
  const install = async (download: boolean) => {
    // As for a skill: the service does not expand `~`.
    if (!path.startsWith("/")) return setError("Give the whole path, starting with /, for example /Users/you/Downloads/my-workplace.");
    if (!download) {
      const preview = await request<WorkplacePreview>("preview_workplace", { path }).catch((e: Error) => (setError(e.message), undefined));
      if (!preview) return;
      const text = downloadText(preview);
      if (text) return setAsking(text), setError("");
    }
    setAsking(undefined);
    setProgress(undefined);
    if (await run(request("install_workplace", { path, download }))) setPath("");
    setProgress(undefined);
  };
  /** The catalog once read (only when asked: reading it contacts GitHub), and the entry being looked at. */
  const [catalog, setCatalog] = useState<CatalogEntry[]>();
  const [offer, setOffer] = useState<{ entry: CatalogEntry; preview: CatalogPreview }>();
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
  const look = async (entry: CatalogEntry) => {
    const preview = await request<CatalogPreview>("preview_catalog", { name: entry.name }).catch((e: Error) => (setError(e.message), undefined));
    if (preview) setOffer({ entry, preview }), setError("");
  };
  const take = async () => {
    if (!offer) return;
    const { entry, preview } = offer;
    setOffer(undefined);
    setProgress(undefined);
    // The hash is the one shown: if the catalog changed meanwhile, the service refuses.
    await run(request("install_catalog", { name: entry.name, sha256: preview.entry.sha256, download: (preview.manifest.downloads?.length ?? 0) > 0 }));
    setProgress(undefined);
  };
  const using = (w: Workplace) => w.usedBy.map((id) => projects.find((p) => p.id === id)?.name ?? id);

  return (
    <section>
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
            {w.archive && <span className="note"> · From the catalog, pinned to <code>{w.archive.slice(0, 12)}</code></span>}
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
      <h3>Catalog</h3>
      <p className="note">
        A list kept in Bate's repository on GitHub. Each entry pins the exact archive by its size and hash; an installed workplace stays at the version it was installed at, and an update is only offered, with what it newly needs. Reading the list contacts GitHub, so it waits until you ask.
      </p>
      <button disabled={reading} onClick={() => void loadCatalog()}>{catalog ? "Reload the catalog" : "Show the catalog"}</button>
      {catalog?.length === 0 && <p className="note">The catalog is empty.</p>}
      <ul className="rows">
        {catalog?.map((e) => (
          <li key={e.name}>
            <strong>{e.name}</strong>
            <span className="note"> {e.version} · {e.description} · {sizeText(e.size)}</span>
            {e.installedVersion && <span className="note"> · Installed: {e.installedVersion}{e.update ? "" : ", up to date"}</span>}
            {(!e.installedVersion || e.update) && (
              <button disabled={!!progress} onClick={() => void look(e)}>{e.installedVersion ? `Update to ${e.version}…` : "Install…"}</button>
            )}
          </li>
        ))}
      </ul>
      {offer && (
        <div role="alert">
          <p>
            {offer.entry.installedVersion ? `Update ${offer.entry.name} from ${offer.entry.installedVersion} to ${offer.preview.entry.version}` : `Install ${offer.entry.name} ${offer.preview.entry.version}`}.{" "}
            {offer.entry.installedVersion ? "It newly needs" : "It needs"}: {newlyLines(offer.preview.newly).join("; ")}
          </p>
          {downloadText(offer.preview) && <p>{downloadText(offer.preview)} Nothing is downloaded until you agree.</p>}
          <button onClick={() => void take()}>{offer.entry.installedVersion ? "Update" : "Install"}</button>{" "}
          <button onClick={() => setOffer(undefined)}>Cancel</button>
        </div>
      )}
      <form className="fields" onSubmit={(e) => { e.preventDefault(); install(false); }}>
        <h3>Install a workplace</h3>
        <p className="note">
          A workplace is a folder on the Mac that runs Bate, with a <code>workplace.json</code> in it (format: <code>docs/workplaces.md</code>). It may hold up to 2000 files and folders and 200 MiB, and no links; what it lists under <code>downloads</code> is fetched at install, after you have seen its size and hosts. Installing one with a name that is installed replaces it, unless a project has it on.
        </p>
        <label>
          Folder path <input required size={50} placeholder="/Users/you/Downloads/my-workplace" value={path} onChange={(e) => setPath(e.target.value)} />
        </label>
        <button disabled={!!progress}>Install</button>
      </form>
      {asking && (
        <p role="alert">
          {asking} Nothing is downloaded until you agree. <button onClick={() => install(true)}>Download and install</button>{" "}
          <button onClick={() => setAsking(undefined)}>Cancel</button>
        </p>
      )}
      {progress && <p className="note" role="status">{progressText(...progress)}</p>}
      {(error || loadError) && <p className="error" role="alert">{error || loadError}</p>}
    </section>
  );
}

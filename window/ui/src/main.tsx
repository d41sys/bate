import { type CSSProperties, StrictMode, useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { inShell, listen, newWindow, openInBrowser, pair, request, serviceNote } from "./service.ts";
import { Pair } from "./Devices.tsx";
import type { Bot, Project } from "./types.ts";
import { Model } from "./Model.tsx";
import { Chat } from "./Chat.tsx";
import { Mail } from "./Mail.tsx";
import { FilePane, Pane } from "./Pane.tsx";
import { SurfaceView, Surfaces } from "./Surfaces.tsx";
import { Routines } from "./Routines.tsx";
import { Connectors } from "./Connectors.tsx";
import { Memory } from "./Memory.tsx";
import { Skills } from "./Skills.tsx";
import { DeleteProject, ProjectBots, ProjectForm, Projects } from "./Projects.tsx";
import { Workplaces } from "./Workplaces.tsx";
import { Channels } from "./Channels.tsx";
import { Room } from "./Room.tsx";
import { Settings } from "./Settings.tsx";
import { AppSettings } from "./AppSettings.tsx";
import { neighbour, sidebar } from "./nav.ts";
import { FirstRun } from "./FirstRun.tsx";
import { Palette, type Choice } from "./Palette.tsx";
import "./highlight.css";
import "./style.css";

// Only in the build `window/check-frames.sh` makes.
if (import.meta.env.VITE_FRAME_CHECK) void import("./frameCheck.ts");

// A bot opens to its own things, a project to its own. What belongs to neither is in the sidebar's foot.
const BOT_TABS = { chat: "Chat", routines: "Routines", connectors: "Connectors", memory: "Memory", skills: "Skills", channels: "Channels", settings: "Settings" };
const PROJECT_TABS = { room: "Room", bots: "Bots", surfaces: "Surfaces", memory: "Memory", settings: "Settings" };
type BotTab = keyof typeof BOT_TABS;
type ProjectTab = keyof typeof PROJECT_TABS;
/** What the page shows. The channels and the surfaces are for Bate's own window only. */
type View =
  | { page: "projects" | "workplaces" | "settings" }
  | { page: "project"; id: string; tab: ProjectTab }
  | { page: "bot"; id: string; tab: BotTab };
const botTabs = (Object.keys(BOT_TABS) as BotTab[]).filter((t) => inShell || t !== "channels");
const projectTabs = (Object.keys(PROJECT_TABS) as ProjectTab[]).filter((t) => inShell || t !== "surfaces");

function Tabs<T extends string>({ ids, names, tab, set }: { ids: T[]; names: Record<T, string>; tab: T; set: (t: T) => void }) {
  return (
    <div role="tablist">
      {ids.map((t) => (
        <button key={t} role="tab" aria-selected={t === tab} onClick={() => set(t)}>{names[t]}</button>
      ))}
    </div>
  );
}

/** The bot's sign, or its first letter, on its colour. The name is always beside it. */
const Avatar = ({ bot }: { bot: Bot }) => (
  <span className="avatar" aria-hidden="true" style={{ "--tint": bot.color || undefined } as CSSProperties}>
    {bot.avatar || [...bot.name][0]?.toUpperCase()}
  </span>
);

function App() {
  const [state, setState] = useState("Connecting to the service…");
  const [bots, setBots] = useState<Bot[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [view, setView] = useState<View>();
  const [name, setName] = useState("");
  const [error, setError] = useState("");
  // On another device that has not paired yet.
  const [unpaired, setUnpaired] = useState(false);
  // Bumped on each new connection, so every screen loads again.
  const [connection, setConnection] = useState(0);
  const [dragged, setDragged] = useState<string>();
  const [palette, setPalette] = useState(false);
  const [jump, setJump] = useState<{ seq: number }>();
  const [find, setFind] = useState(false);
  // A file a bot shows, or a surface of a project's program, in the pane beside the conversation.
  const [shown, setShown] = useState<{ bot: string; path: string } | { projectId: string; uri: string; name: string }>();
  const newName = useRef<HTMLInputElement>(null);
  // What the shell says of starting the service with the app (macOS may want an approval).
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    if (!inShell || !state) return setNote(null);
    const ask = () => serviceNote().then(setNote, () => {});
    void ask();
    const timer = setInterval(ask, 2000);
    return () => clearInterval(timer);
  }, [state]);

  // The service tells a window of the surface it shows, and of the list of its project's.
  const watched = shown && "uri" in shown ? shown : undefined;
  const surfaceProject = watched?.projectId ?? (view?.page === "project" ? view.id : undefined);
  const surfaceUri = watched?.uri;
  useEffect(() => {
    if (inShell) request("watch_surfaces", { projectId: surfaceProject ?? null, uri: surfaceUri }).catch(() => {});
  }, [surfaceProject, surfaceUri, connection]);

  useEffect(() => {
    const load = () => request<{ bots: Bot[] }>("list_bots").then((d) => setBots(d.bots), (e) => setError(e.message));
    const loadProjects = () => request<{ projects: Project[] }>("list_projects").then((d) => setProjects(d.projects), (e) => setError(e.message));
    return listen((frame) => {
      if (frame.type === "unpaired") {
        setUnpaired(true);
      } else if (frame.type === "connected") {
        setUnpaired(false);
        setState("");
        // What failed while the service was away is tried again by every screen.
        setError("");
        setConnection((n) => n + 1);
        load();
        loadProjects();
      } else if (frame.type === "failed") {
        setState(`The connection failed: ${frame.error}`);
      } else if (frame.type === "bots_changed") {
        load();
      } else if (frame.type === "projects_changed") {
        load();
        loadProjects();
      }
    });
  }, []);

  const create = async () => {
    try {
      const { bot } = await request<{ bot: Bot }>("create_bot", { name: name.trim() });
      setName("");
      openBot(bot.id);
    } catch (e) {
      setError((e as Error).message);
    }
  };

  // Puts `moved` where `onto` is; the service keeps the order.
  const reorder = (moved: string, onto: string) => {
    const ids = bots.map((b) => b.id);
    ids.splice(ids.indexOf(moved), 1);
    // Down: after `onto`; up: before it. Both are `onto`'s old place.
    ids.splice(bots.findIndex((b) => b.id === onto), 0, moved);
    // At once, so a quick second move starts from this order.
    setBots(ids.map((id) => bots.find((b) => b.id === id)!));
    request("order_bots", { bots: ids }).catch((e: Error) => setError(e.message));
  };
  const move = (id: string, by: number) => {
    const onto = neighbour(bots, id, by);
    if (onto) reorder(id, onto.id);
  };

  const bot = view?.page === "bot" ? bots.find((b) => b.id === view.id) : undefined;
  const project = view?.page === "project" ? projects.find((p) => p.id === view.id) : undefined;
  const openBot = (id: string, tab: BotTab = "chat") => setView({ page: "bot", id, tab });
  const openProject = (id: string, tab: ProjectTab = "room") => setView({ page: "project", id, tab });
  // What the menus, the keys and the palette can do, by the menu item's id.
  const actions: Record<string, () => void> = {
    palette: () => setPalette(true),
    find: () => {
      if (!bot) return;
      openBot(bot.id);
      setFind(true);
    },
    "new-bot": () => newName.current?.focus(),
    "new-project": () => setView({ page: "projects" }),
    "new-window": () => void newWindow().catch((e: Error) => setError(e.message)),
  };
  const act = useRef(actions);
  act.current = actions;
  useEffect(() => {
    // By the key, not the letter it typed (Caps Lock).
    const keys: Record<string, string> = { KeyK: "palette", KeyF: "find", KeyN: "new-bot", "Shift+KeyN": "new-window" };
    const onKey = (e: KeyboardEvent) => {
      const id = e.metaKey && !e.altKey && !e.ctrlKey ? keys[e.shiftKey ? `Shift+${e.code}` : e.code] : undefined;
      if (!id) return;
      e.preventDefault();
      act.current[id]();
    };
    // The shell's menus send their items' ids.
    const onMenu = (e: Event) => act.current[(e as CustomEvent<string>).detail]?.();
    addEventListener("keydown", onKey);
    addEventListener("bate-menu", onMenu);
    return () => {
      removeEventListener("keydown", onKey);
      removeEventListener("bate-menu", onMenu);
    };
  }, []);
  const choices: Choice[] = [
    ...projects.map((p) => ({ label: p.name, hint: "project", run: () => openProject(p.id!) })),
    ...bots.map((b) => ({ label: `${b.avatar ? `${b.avatar} ` : ""}${b.name}`, hint: "bot", run: () => openBot(b.id) })),
    ...(bot && view?.page === "bot" ? botTabs.map((t) => ({ label: BOT_TABS[t], hint: bot.name, run: () => openBot(bot.id, t) })) : []),
    ...(project ? projectTabs.map((t) => ({ label: PROJECT_TABS[t], hint: project.name, run: () => openProject(project.id!, t) })) : []),
    ...(bot ? [{ label: "Find in chat", hint: "⌘F", run: actions.find }] : []),
    { label: "New bot", hint: "⌘N", run: actions["new-bot"] },
    { label: "New project", run: actions["new-project"] },
    { label: "Workplaces", run: () => setView({ page: "workplaces" }) },
    { label: "Settings", hint: "the app's", run: () => setView({ page: "settings" }) },
    { label: "New window", hint: "⇧⌘N", run: actions["new-window"] },
  ];

  const lists = sidebar(bots, projects);
  const botRow = (b: Bot) => (
    <li
      key={b.id}
      draggable
      onDragStart={() => setDragged(b.id)}
      onDragOver={(e) => e.preventDefault()}
      onDrop={(e) => {
        e.preventDefault();
        if (dragged && dragged !== b.id) reorder(dragged, b.id);
        setDragged(undefined);
      }}
    >
      <button
        className={b.id === bot?.id ? "row current" : "row"}
        aria-current={b.id === bot?.id ? "page" : undefined}
        onClick={() => openBot(b.id, view?.page === "bot" ? view.tab : "chat")}
        title="Alt+↑ and Alt+↓ move it"
        onKeyDown={(e) => {
          if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
            e.preventDefault();
            move(b.id, e.key === "ArrowUp" ? -1 : 1);
          }
        }}
      >
        <Avatar bot={b} />
        <span className="label">
          <span>{b.name}</span>
          {b.busy && <small>Working…</small>}
        </span>
      </button>
    </li>
  );
  /** A row with no avatar: a project, or a page of the app. */
  const plain = (text: string, current: boolean, go: () => void) => (
    <button className={current ? "row plain current" : "row plain"} aria-current={current ? "page" : undefined} onClick={go}>
      <span className="label"><span>{text}</span></span>
    </button>
  );

  if (unpaired) return <Pair onPair={pair} />;
  return (
    <div className="app">
      <nav className="sidebar" aria-label="Projects and bots">
        <h2>Projects</h2>
        <ul>
          {lists.projects.map(({ project: p, bots: members }) => (
            <li key={p.id}>
              {plain(p.name, view?.page === "project" && view.id === p.id, () => openProject(p.id!, view?.page === "project" ? view.tab : "room"))}
              {members.length > 0 && <ul>{members.map(botRow)}</ul>}
            </li>
          ))}
          <li>{plain("New project", view?.page === "projects", () => setView({ page: "projects" }))}</li>
        </ul>
        <h2>Bots</h2>
        <ul>{lists.loose.map(botRow)}</ul>
        <form onSubmit={(e) => { e.preventDefault(); create(); }}>
          <input ref={newName} aria-label="New bot's name" placeholder="New bot" maxLength={80} value={name} onChange={(e) => setName(e.target.value)} />
          <button disabled={!name.trim()}>Create</button>
        </form>
        <ul className="foot">
          <li>{plain("Workplaces", view?.page === "workplaces", () => setView({ page: "workplaces" }))}</li>
          <li>{plain("Settings", view?.page === "settings", () => setView({ page: "settings" }))}</li>
        </ul>
      </nav>
      <main>
        {state && <p className="note">{state}</p>}
        {note && <p className="note">{note}</p>}
        {inShell && !state && <FirstRun reload={`${connection}-${view?.page}`} onSettings={() => setView({ page: "settings" })} />}
        {error && <p className="banner error" role="alert" onClick={() => setError("")}>{error}</p>}
        {view?.page === "projects" && (
          <>
            <header><div className="who"><h1>Projects</h1></div></header>
            <Projects key={connection} bots={bots} projects={projects} open={openProject} />
          </>
        )}
        {view?.page === "workplaces" && (
          <>
            <header><div className="who"><h1>Workplaces</h1></div></header>
            <Workplaces key={connection} projects={projects} />
          </>
        )}
        {view?.page === "settings" && (
          <>
            <header><div className="who"><h1>Settings</h1></div></header>
            <AppSettings key={connection} />
          </>
        )}
        {view?.page === "project" && project && (
          <>
            <header>
              <div className="who"><h1>{project.name}</h1></div>
              <Tabs ids={projectTabs} names={PROJECT_TABS} tab={view.tab} set={(t) => openProject(project.id!, t)} />
            </header>
            {view.tab === "room" && <Room key={`${project.id}-${connection}`} project={{ id: project.id!, name: project.name }} bots={bots} />}
            {view.tab === "bots" && <ProjectBots key={`${project.id}-${connection}`} project={project} bots={bots} />}
            {/* Surfaces open in the window's pane only, not on other devices. */}
            {view.tab === "surfaces" && inShell && (
              <Surfaces
                key={`surfaces-${project.id}-${connection}`}
                projectId={project.id!}
                open={(s) => setShown({ projectId: project.id!, uri: s.uri, name: s.name })}
              />
            )}
            {view.tab === "memory" && <Memory key={`${project.id}-${connection}`} target={{ projectId: project.id! }} />}
            {view.tab === "settings" && (
              <>
                <ProjectForm key={`${project.id}-${connection}`} project={project} bots={bots} />
                <DeleteProject key={`delete-${project.id}`} project={project} bots={bots} gone={() => setView({ page: "projects" })} />
              </>
            )}
          </>
        )}
        {view?.page === "bot" && bot && (
          <>
            <header>
              <div className="who">
                <Avatar bot={bot} />
                <h1>{bot.name}</h1>
              </div>
              <Tabs ids={botTabs} names={BOT_TABS} tab={view.tab} set={(t) => openBot(bot.id, t)} />
            </header>
            {view.tab === "chat" && <Mail key={`mail-${bot.id}-${connection}`} bot={bot.id} bots={bots} />}
            {view.tab === "chat" && <Chat
                key={`${bot.id}-${connection}`}
                bot={bot}
                jump={jump}
                find={find}
                // Bots' files open in the window's pane only, not on other devices.
                showFile={inShell ? (path) => setShown({ bot: bot.id, path }) : undefined}
                // An installed agent chooses its own model.
                model={bot.agent ? undefined : <Model key={bot.id} bot={bot} bots={bots} report={setError} />}
                used={() => {
                  setJump(undefined);
                  setFind(false);
                }}
              />}
            {view.tab === "routines" && <Routines key={`${bot.id}-${connection}`} bot={bot.id} />}
            {view.tab === "connectors" && <Connectors key={`${bot.id}-${connection}`} bot={bot.id} />}
            {view.tab === "memory" && (
              <>
                {bot.agent && <p className="note">An installed agent keeps its own memory: these notes, and the project's notes and instructions, do not reach it.</p>}
                <Memory key={`${bot.id}-${connection}`} target={{ bot: bot.id }} />
              </>
            )}
            {view.tab === "skills" && <Skills key={`${bot.id}-${connection}`} bot={bot.id} />}
            {view.tab === "channels" && inShell && <Channels key={`${bot.id}-${connection}`} bot={bot.id} />}
            {view.tab === "settings" && <Settings key={`${bot.id}-${connection}`} bot={bot} onDeleted={() => setView(undefined)} />}
          </>
        )}
        {!state && !(view?.page === "bot" ? bot : view?.page === "project" ? project : view) && (
          <p className="empty">{bots.length || projects.length ? "Choose a bot or a project." : "Create a bot, or start a project, to begin."}</p>
        )}
      </main>
      {shown && "path" in shown && <FilePane bot={shown.bot} path={shown.path} close={() => setShown(undefined)} />}
      {shown && "uri" in shown && (
        <Pane label="Surface" title={shown.name} close={() => setShown(undefined)}>
          <SurfaceView key={`${shown.projectId}-${shown.uri}`} projectId={shown.projectId} uri={shown.uri} />
        </Pane>
      )}
      {palette && (
        <Palette
          choices={choices}
          bots={bots}
          found={(id, seq) => {
            openBot(id);
            setJump({ seq });
          }}
          close={() => setPalette(false)}
        />
      )}
    </div>
  );
}

// The shell denies a link's navigation out of the page; only a new window reaches the browser.
// A browser (the network client) opens the link itself, keeping its `rel="noreferrer"`.
addEventListener("click", (e) => {
  if (!inShell || e.defaultPrevented) return;
  const link = (e.target as Element).closest?.("a[target=_blank]");
  if (link instanceof HTMLAnchorElement && link.href) {
    e.preventDefault();
    openInBrowser(link.href);
  }
});

createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

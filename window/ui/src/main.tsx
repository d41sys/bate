import { type ReactNode, StrictMode, useEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { inShell, listen, newWindow, notifyWaiting, openInBrowser, pair, request, serviceNote, setBadge } from "./service.ts";
import { Pair } from "./Devices.tsx";
import type { Bot, Project } from "./types.ts";
import { Model } from "./Model.tsx";
import { Chat } from "./Chat.tsx";
import { Conversations } from "./Conversations.tsx";
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
import { Avatar } from "./Avatar.tsx";
import { Settings } from "./Settings.tsx";
import { AppSettings } from "./AppSettings.tsx";
import { endsUnseen, neighbour, rowState, shortcut, shownBots, sidebar, stepBot, stepTab, waitingCount } from "./nav.ts";
import { FirstRun, useNeedsProvider } from "./FirstRun.tsx";
import { NewBot } from "./NewBot.tsx";
import { LATER } from "./firstRun.ts";
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
  | { page: "workplaces" | "settings" | "new-bot" }
  /** `asked`: when "New project" was asked for, so that its dialog opens at once, each time. */
  | { page: "projects"; asked?: number }
  | { page: "project"; id: string; tab: ProjectTab }
  | { page: "bot"; id: string; tab: BotTab };
const botTabs = (Object.keys(BOT_TABS) as BotTab[]).filter((t) => inShell || t !== "channels");
const projectTabs = (Object.keys(PROJECT_TABS) as ProjectTab[]).filter((t) => inShell || t !== "surfaces");

function Tabs<T extends string>({ ids, names, tab, set }: { ids: T[]; names: Record<T, string>; tab: T; set: (t: T) => void }) {
  return (
    <div
      role="tablist"
      onKeyDown={(e) => {
        const to = e.metaKey || e.ctrlKey || e.altKey || e.shiftKey || e.nativeEvent.isComposing ? undefined : stepTab(ids, tab, e.key);
        if (!to) return;
        e.preventDefault();
        set(to);
        e.currentTarget.querySelector<HTMLElement>(`[id="tab-${to}"]`)?.focus();
      }}
    >
      {ids.map((t) => (
        // Only the chosen tab is in the Tab order; the arrows move among the rest.
        <button key={t} role="tab" id={`tab-${t}`} aria-controls="tabpanel" aria-selected={t === tab} tabIndex={t === tab ? 0 : -1} onClick={() => set(t)}>{names[t]}</button>
      ))}
    </div>
  );
}

/** What the chosen tab shows (a bot's or a project's). */
const Panel = ({ tab, children }: { tab: string; children: ReactNode }) => (
  <div role="tabpanel" id="tabpanel" aria-labelledby={`tab-${tab}`}>{children}</div>
);

function App() {
  const [state, setState] = useState("Connecting to the service…");
  const [bots, setBots] = useState<Bot[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [view, setView] = useState<View>();
  const [error, setError] = useState("");
  // On another device that has not paired yet.
  const [unpaired, setUnpaired] = useState(false);
  // Bumped on each new connection, so every screen loads again.
  const [connection, setConnection] = useState(0);
  const [dragged, setDragged] = useState<string>();
  const [palette, setPalette] = useState(false);
  const [jump, setJump] = useState<{ seq: number; conversation: string }>();
  const [find, setFind] = useState(false);
  // "Set up later" on the first screen, which the page remembers; and what the first bot's box starts with.
  const [later, setLater] = useState(() => localStorage.getItem(LATER) === "1");
  const [first, setFirst] = useState<{ bot: string; message: string }>();
  // A file a bot shows, or a surface of a project's program, in the pane beside the conversation.
  const [shown, setShown] = useState<{ bot: string; path: string } | { projectId: string; uri: string; name: string }>();
  // What the shell says of starting the service with the app (macOS may want an approval).
  const [note, setNote] = useState<string | null>(null);
  // Bots whose turn ended while their chat was not in front of the person (`endsUnseen`).
  const [unread, setUnread] = useState<string[]>([]);
  // The chat in front of the person, for the frames that come in later.
  const looking = view?.page === "bot" && view.tab === "chat" ? view.id : undefined;
  const now = useRef({ looking, bots, palette, shown });
  now.current = { looking, bots, palette, shown };

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
      } else if (frame.type === "transcript") {
        const front = document.hasFocus() ? now.current.looking : undefined;
        if (endsUnseen(frame, front)) setUnread((u) => (u.includes(frame.bot!) ? u : [...u, frame.bot!]));
        // The service sends a question's card live only; what history holds is not announced.
        const entry = frame.entry as { type: string; question?: string; title?: string };
        if (inShell && entry.type === "question" && frame.bot !== front) {
          const name = now.current.bots.find((b) => b.id === frame.bot)?.name ?? "A bot";
          notifyWaiting(frame.bot!, entry.question ?? "", `${name} needs you`, entry.title ?? "").catch(() => {});
        }
      } else if (frame.type === "bots_changed") {
        load();
      } else if (frame.type === "projects_changed") {
        load();
        loadProjects();
      }
    });
  }, []);

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
  const [needed, refreshProvider] = useNeedsProvider(`${connection}-${view?.page}-${bot?.id}`);
  const setSetupLater = (on: boolean) => {
    if (on) localStorage.setItem(LATER, "1");
    else localStorage.removeItem(LATER);
    setLater(on);
  };
  // Bots that wait for an answer while another bot's page is open: one click away from here.
  const others = bot ? bots.filter((b) => b.waiting && b.id !== bot.id) : [];
  const project = view?.page === "project" ? projects.find((p) => p.id === view.id) : undefined;
  const openBot = (id: string, tab: BotTab = "chat") => setView({ page: "bot", id, tab });
  const openProject = (id: string, tab: ProjectTab = "room") => setView({ page: "project", id, tab });
  const order = shownBots(bots, projects);
  // Like a click on its row: the tab in front stays.
  const goBot = (b?: Bot) => b && openBot(b.id, view?.page === "bot" ? view.tab : "chat");
  // The app's settings, at one of its sections.
  const settingsAt = (id: string) => {
    flushSync(() => setView({ page: "settings" }));
    document.getElementById(id)?.focus();
  };
  // What the menus, the keys and the palette can do, by the menu item's id.
  const actions: Record<string, () => void> = {
    palette: () => setPalette(true),
    find: () => {
      if (!bot) return;
      openBot(bot.id);
      setFind(true);
    },
    "new-bot": () => setView({ page: "new-bot" }),
    "new-project": () => setView({ page: "projects", asked: Date.now() }),
    "new-window": () => void newWindow().catch((e: Error) => setError(e.message)),
    settings: () => setView({ page: "settings" }),
    "previous-bot": () => goBot(stepBot(order, bot?.id, -1)),
    "next-bot": () => goBot(stepBot(order, bot?.id, 1)),
    ...Object.fromEntries(Array.from({ length: 9 }, (_, i) => [`bot-${i + 1}`, () => goBot(order[i])])),
  };
  const act = useRef(actions);
  act.current = actions;
  useEffect(() => {
    // Behind the palette or an open <dialog> nothing else runs.
    const covered = () => now.current.palette || !!document.querySelector("dialog[open]");
    const run = (id: string) => covered() || act.current[id]?.();
    // This is the first of the window's keydown listeners (the chat's come later), so Esc is spent
    // here before the chat's own sees it: a <dialog> keeps it (the browser closes the dialog, and
    // nothing behind reacts), then the palette closes, then the pane, and only then does Esc deny a
    // question or stop the bot.
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (document.querySelector("dialog[open]")) return e.stopImmediatePropagation();
        if (e.defaultPrevented || e.isComposing || !(now.current.palette || now.current.shown)) return;
        e.preventDefault();
        if (now.current.palette) setPalette(false);
        else setShown(undefined);
        return;
      }
      const id = shortcut(e);
      if (!id) return;
      e.preventDefault();
      run(id);
    };
    // The shell's menus send their items' ids.
    const onMenu = (e: Event) => run((e as CustomEvent<string>).detail);
    addEventListener("keydown", onKey);
    addEventListener("bate-menu", onMenu);
    return () => {
      removeEventListener("keydown", onKey);
      removeEventListener("bate-menu", onMenu);
    };
  }, []);
  // The Dock icon counts the bots that wait for the person.
  const waiting = waitingCount(bots);
  useEffect(() => {
    if (inShell) setBadge(waiting).catch(() => {});
  }, [waiting, connection]);
  // A chat in front of the person, in a window that has focus, is read.
  useEffect(() => {
    const read = () => {
      if (looking && document.hasFocus()) setUnread((u) => (u.includes(looking) ? u.filter((id) => id !== looking) : u));
    };
    read();
    addEventListener("focus", read);
    return () => removeEventListener("focus", read);
  }, [looking]);
  // A click on the shell's notification opens that bot.
  const open = useRef(openBot);
  open.current = openBot;
  useEffect(() => {
    const onOpen = (e: Event) => open.current((e as CustomEvent<string>).detail);
    addEventListener("bate-open-bot", onOpen);
    return () => removeEventListener("bate-open-bot", onOpen);
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
    { label: "App settings", hint: "⌘,", run: actions.settings },
    ...(inShell
      ? [
          { label: "ChatGPT sign-in", hint: "app settings", run: () => settingsAt("settings-chatgpt") },
          { label: "Devices", hint: "app settings", run: () => settingsAt("settings-devices") },
        ]
      : []),
    { label: "Updates", hint: "app settings", run: () => settingsAt("settings-updates") },
    { label: "New window", hint: "⇧⌘N", run: actions["new-window"] },
  ];

  const lists = sidebar(bots, projects);
  const botRow = (b: Bot) => {
    const state = rowState(b, unread.includes(b.id));
    return (
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
          {state === "working" && <small>Working…</small>}
        </span>
        {state === "waiting" && <span className="badge">Needs you</span>}
        {state === "unread" && <span className="badge quiet">New reply</span>}
      </button>
    </li>
    );
  };
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
          <li>{plain("New project", view?.page === "projects", actions["new-project"])}</li>
        </ul>
        <div className="heading">
          <h2>Bots</h2>
          <button className="round ghost" aria-label="New bot" data-tip="New bot (⌘N)" onClick={actions["new-bot"]}>+</button>
        </div>
        <ul>{lists.loose.map(botRow)}</ul>
        <ul className="foot">
          <li>{plain("Workplaces", view?.page === "workplaces", () => setView({ page: "workplaces" }))}</li>
          <li>{plain("Settings", view?.page === "settings", () => setView({ page: "settings" }))}</li>
        </ul>
      </nav>
      <main>
        {state && <p className="note">{state}</p>}
        {note && <p className="note">{note}</p>}
        {inShell && !state && !view && !later && (
          <FirstRun
            needed={needed}
            hasBots={bots.length > 0}
            refresh={refreshProvider}
            later={() => setSetupLater(true)}
            opened={(id, message) => {
              setFirst({ bot: id, message });
              openBot(id);
            }}
          />
        )}
        {error && <p className="banner error" role="alert" onClick={() => setError("")}>{error}</p>}
        {view?.page === "new-bot" && (
          <>
            <header><div className="who"><h1>New bot</h1></div></header>
            <NewBot opened={(id) => openBot(id)} report={setError} />
          </>
        )}
        {view?.page === "projects" && (
          <>
            <header><div className="who"><h1>Projects</h1></div></header>
            <Projects key={`${connection}-${view.asked}`} adding={view.asked !== undefined} bots={bots} projects={projects} open={openProject} />
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
            <Panel tab={view.tab}>
            {view.tab === "room" && <Room key={`${project.id}-${connection}`} project={project} bots={bots} />}
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
            </Panel>
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
            {others.length > 0 && (
              <p className="banner actions" role="status">
                {others.length === 1 ? `${others[0].name} waits for your answer.` : `${others.length} bots wait for your answer.`}
                {others.map((b) => (
                  <button key={b.id} className="small" onClick={() => openBot(b.id)}>Open {b.name}</button>
                ))}
              </p>
            )}
            <Panel tab={view.tab}>
            {view.tab === "chat" && <Mail key={`mail-${bot.id}-${connection}`} bot={bot.id} bots={bots} />}
            {view.tab === "chat" && (
              <Conversations key={`conversations-${bot.id}-${connection}`} bot={bot.id} want={jump?.conversation}>
                {(conversation, show, offerCompact) => <Chat
                  key={`${bot.id}-${conversation}-${connection}`}
                  bot={bot}
                  conversation={conversation}
                  onFork={show}
                  offerCompact={offerCompact}
                  jump={jump}
                  find={find}
                  // Bots' files open in the window's pane only, not on other devices.
                  showFile={inShell ? (path) => setShown({ bot: bot.id, path }) : undefined}
                  // An installed agent chooses its own model.
                  // With no way to reach a model there is the sign-in instead; an installed agent needs none.
                  model={bot.agent || needed ? undefined : <Model key={bot.id} bot={bot} bots={bots} />}
                  signIn={needed && !bot.agent ? () => { setSetupLater(false); setView(undefined); } : undefined}
                  start={first?.bot === bot.id ? first.message : ""}
                  started={() => setFirst(undefined)}
                  used={() => {
                    setJump(undefined);
                    setFind(false);
                  }}
                />}
              </Conversations>
            )}
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
            </Panel>
          </>
        )}
        {!state && !(view?.page === "bot" ? bot : view?.page === "project" ? project : view) && !(inShell && needed && !later) && (
          <div className="empty">
            <p>
              {bots.length || projects.length
                ? "Choose a bot or a project."
                : inShell
                  ? "Make a bot with +, or start a project, to begin."
                  : "Make a bot with +, or start a project, to begin. Its model is set up in Bate on the Mac, under Settings."}
            </p>
            {needed && later && <button onClick={() => setSetupLater(false)}>Sign in</button>}
          </div>
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
          found={(id, conversation, seq) => {
            openBot(id);
            setJump({ seq, conversation });
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

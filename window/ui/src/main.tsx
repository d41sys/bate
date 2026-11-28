import { type ReactNode, StrictMode, useEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { inQuickPanel, inShell, listen, newWindow, notifyWaiting, openAtStart, openInBrowser, pair, request, savedLook, serviceNote, setBadge, setLook } from "./service.ts";
import { QuickAsk } from "./QuickAsk.tsx";
import { Pair } from "./Devices.tsx";
import type { Bot, Project } from "./types.ts";
import { Model } from "./Model.tsx";
import { Chat } from "./Chat.tsx";
import { Changes } from "./Changes.tsx";
import { Conversations, Talks } from "./Conversations.tsx";
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
import { Attention } from "./Attention.tsx";
import { changesAttention, needsCount, type AttentionItem } from "./attention.ts";
import type { Question } from "./chat.ts";
import { noticeFor } from "./notice.ts";
import { Avatar } from "./Avatar.tsx";
import { Settings } from "./Settings.tsx";
import { AppSettings, type SettingsSection } from "./AppSettings.tsx";
import { endsUnseen, neighbour, rowState, shortcut, shownBots, sidebar, stepBot } from "./nav.ts";
import { FirstRun, useNeedsProvider } from "./FirstRun.tsx";
import { NewBot } from "./NewBot.tsx";
import { LATER } from "./firstRun.ts";
import { Palette, type Choice } from "./Palette.tsx";
import { RowMenu } from "./RowMenu.tsx";
import "./highlight.css";
import { Blocks, Folder, Inbox, type LucideIcon, Plus, Settings as Gear, SquarePen } from "lucide-react";
import "./style.css";

// Only in the build `window/check-frames.sh` makes.
if (import.meta.env.VITE_FRAME_CHECK) void import("./frameCheck.ts");

// A bot opens to its own things, a project to its own. What belongs to neither is in the sidebar's foot.
const BOT_TABS = { chat: "Chat", routines: "Routines", memory: "Memory", channels: "Channels", settings: "Settings" };
// What a bot can be given, for every bot and project at once.
const ADDON_TABS = { connectors: "Connectors", skills: "Skills", workplaces: "Workplaces" };
const PROJECT_TABS = { room: "Room", bots: "Bots", surfaces: "Surfaces", memory: "Memory", settings: "Settings" };
type BotTab = keyof typeof BOT_TABS;
type ProjectTab = keyof typeof PROJECT_TABS;
type AddonTab = keyof typeof ADDON_TABS;
/** What the page shows. The channels and the surfaces are for Bate's own window only. */
type View =
  | { page: "new-bot" }
  | { page: "attention" }
  | { page: "settings"; section?: SettingsSection }
  | { page: "addons"; tab: AddonTab }
  /** `asked`: when "New project" was asked for, so that its dialog opens at once, each time. */
  | { page: "projects"; asked?: number }
  | { page: "project"; id: string; tab: ProjectTab }
  /** `conversation`: the one to show, else the bot's current. */
  | { page: "bot"; id: string; tab: BotTab; conversation?: string };
const botTabs = (Object.keys(BOT_TABS) as BotTab[]).filter((t) => inShell || t !== "channels");
const projectTabs = (Object.keys(PROJECT_TABS) as ProjectTab[]).filter((t) => inShell || t !== "surfaces");

/** The page's title, which is also how to change page: its menu lists the pages, the current one checked. A page that is not the first is named after the title. */
function PageMenu<T extends string>({ label, ids, names, tab, set, children }: { label: string; ids: T[]; names: Record<T, string>; tab: T; set: (t: T) => void; children: ReactNode }) {
  const title = <>{children}{tab !== ids[0] && <span className="page">{names[tab]}</span>}</>;
  return <RowMenu label={label} title={title} items={ids.map((t) => ({ label: names[t], current: t === tab, run: () => set(t) }))} />;
}

function App() {
  const [state, setState] = useState("Connecting to the service…");
  const [bots, setBots] = useState<Bot[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  // A window the shell opens for "Open in Bate" starts on that bot's chat.
  const [view, setView] = useState<View | undefined>(openAtStart ? { page: "bot", id: openAtStart, tab: "chat" } : undefined);
  const [error, setError] = useState("");
  // On another device that has not paired yet.
  const [unpaired, setUnpaired] = useState(false);
  // Bumped on each new connection, so every screen loads again.
  const [connection, setConnection] = useState(0);
  const [dragged, setDragged] = useState<string>();
  // The place in a bot's header where its conversation chip is drawn.
  const [conversationSlot, setConversationSlot] = useState<HTMLElement | null>(null);
  const [palette, setPalette] = useState(false);
  const [jump, setJump] = useState<{ seq: number; conversation: string }>();
  const [find, setFind] = useState(false);
  // "Set up later" on the first screen, which the page remembers; and what the first bot's box starts with.
  const [later, setLater] = useState(() => localStorage.getItem(LATER) === "1");
  const [first, setFirst] = useState<{ bot: string; message: string }>();
  // A file a bot shows, or a surface of a project's program, in the pane beside the conversation.
  const [shown, setShown] = useState<{ bot: string; path: string } | { projectId: string; uri: string; name: string } | { changes: { bot: string; conversation: string } }>();
  // What the shell says of starting the service with the app (macOS may want an approval).
  const [note, setNote] = useState<string | null>(null);
  // Bots whose turn ended while their chat was not in front of the person (`endsUnseen`).
  const [unread, setUnread] = useState<string[]>([]);
  // The chat in front of the person, for the frames that come in later.
  const looking = view?.page === "bot" && view.tab === "chat" ? view.id : undefined;
  const now = useRef({ looking, bots, shown });
  now.current = { looking, bots, shown };

  useEffect(() => {
    if (!inShell || !state) return setNote(null);
    const ask = () => serviceNote().then(setNote, () => {});
    void ask();
    const timer = setInterval(ask, 2000);
    return () => clearInterval(timer);
  }, [state]);

  const changesBot = shown && "changes" in shown ? bots.find((b) => b.id === shown.changes.bot) : undefined;
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
        const entry = frame.entry as { type: string } & Question;
        if (inShell && entry.type === "question" && frame.bot !== front) {
          const name = now.current.bots.find((b) => b.id === frame.bot)?.name ?? "A bot";
          const notice = noticeFor(name, entry);
          notifyWaiting(frame.bot!, String(frame.conversation), entry.question, notice.title, notice.body, notice.actions).catch(() => {});
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
  const project = view?.page === "project" ? projects.find((p) => p.id === view.id) : undefined;
  // The same bot again keeps the conversation it shows, unless another is named.
  const openBot = (id: string, tab: BotTab = "chat", conversation?: string) =>
    setView((v) => ({ page: "bot", id, tab, conversation: conversation ?? (v?.page === "bot" && v.id === id ? v.conversation : undefined) }));
  const newTalk = (id: string) => request<{ conversation: string }>("new_conversation", { bot: id }).then((d) => openBot(id, "chat", d.conversation), (e: Error) => setError(e.message));
  const openProject = (id: string, tab: ProjectTab = "room") => setView({ page: "project", id, tab });
  const order = shownBots(bots, projects);
  // Like a click on its row: the tab in front stays.
  const goBot = (b?: Bot) => b && openBot(b.id, view?.page === "bot" ? view.tab : "chat");
  // The app's settings, at one of its sections.
  const settingsAt = (section: SettingsSection) => {
    flushSync(() => setView({ page: "settings", section }));
    document.getElementById(`settings-${section}`)?.focus();
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
    // Behind an open <dialog> (the palette is one) nothing else runs.
    const covered = () => !!document.querySelector("dialog[open]");
    const run = (id: string) => covered() || act.current[id]?.();
    // This is the first of the window's keydown listeners (the chat's come later), so Esc is spent
    // here before the chat's own sees it: a <dialog> keeps it (the browser closes the dialog, and
    // nothing behind reacts), then the pane, and only then does Esc deny a question or stop the bot.
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (document.querySelector("dialog[open]")) return e.stopImmediatePropagation();
        if (e.defaultPrevented || e.isComposing || !now.current.shown) return;
        e.preventDefault();
        setShown(undefined);
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
  // The conversations that need the person, from the list the Activity screen shows: the Dock icon's count and the sidebar's.
  const [waiting, setWaiting] = useState(0);
  useEffect(() => {
    let gone = false;
    let latest = 0;
    const load = () => {
      const mine = ++latest;
      request<{ items: AttentionItem[] }>("list_attention").then((d) => mine === latest && !gone && setWaiting(needsCount(d.items)), () => {});
    };
    load();
    const stop = listen((frame) => changesAttention(frame) && load());
    return () => {
      gone = true;
      stop();
    };
  }, [connection]);
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
  // One bot's, or (no `bot`) every bot's.
  const setPaused = (paused: boolean, id?: string) => request("set_paused", { bot: id, paused }).catch((e: Error) => setError(e.message));
  const choices: Choice[] = [
    ...projects.map((p) => ({ group: "Projects", label: p.name, run: () => openProject(p.id!) })),
    ...bots.map((b) => ({ group: "Bots", label: b.name, bot: b, run: () => openBot(b.id) })),
    ...(bot && view?.page === "bot" ? botTabs.map((t) => ({ group: `In ${bot.name}`, label: BOT_TABS[t], run: () => openBot(bot.id, t) })) : []),
    ...(project ? projectTabs.map((t) => ({ group: `In ${project.name}`, label: PROJECT_TABS[t], run: () => openProject(project.id!, t) })) : []),
    ...(bot ? [{ group: "Commands", label: "Find in chat", keys: "⌘F", run: actions.find }] : []),
    { group: "Commands", label: "New bot", keys: "⌘N", run: actions["new-bot"] },
    { group: "Commands", label: "New project", run: actions["new-project"] },
    { group: "Commands", label: "New window", keys: "⇧⌘N", run: actions["new-window"] },
    ...(bots.some((b) => !b.paused) ? [{ group: "Commands", label: "Pause all bots", run: () => setPaused(true) }] : []),
    ...(bots.some((b) => b.paused) ? [{ group: "Commands", label: "Resume all bots", run: () => setPaused(false) }] : []),
    ...(Object.keys(ADDON_TABS) as AddonTab[]).map((t) => ({ group: "Pages", label: `Add-ons: ${ADDON_TABS[t]}`, run: () => setView({ page: "addons", tab: t }) })),
    { group: "Pages", label: "Activity", run: () => setView({ page: "attention" }) },
    { group: "Pages", label: "App settings", keys: "⌘,", run: actions.settings },
    ...(inShell
      ? [
          { group: "Pages", label: "ChatGPT sign-in", run: () => settingsAt("chatgpt") },
          { group: "Pages", label: "Devices", run: () => settingsAt("devices") },
        ]
      : []),
    { group: "Pages", label: "Updates", run: () => settingsAt("updates") },
  ];

  const lists = sidebar(bots, projects);
  const botRow = (b: Bot) => {
    const state = rowState(b, unread.includes(b.id));
    return (
    <li
      key={b.id}
      className="bot"
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
          {state === "waiting" && <span className="badge">Needs you</span>}
          {state === "paused" && <span className="badge quiet">Paused</span>}
          {state === "unread" && <span className="badge quiet">New reply</span>}
        </span>
        {state === "working" && <span className="pulse" aria-hidden="true" />}
      </button>
      <RowMenu label={b.name} items={[{ label: b.paused ? "Resume" : "Pause", run: () => setPaused(!b.paused, b.id) }]} />
    </li>
    );
  };
  /** A row with an icon and no avatar: a project, or a page of the app. */
  const plain = (text: string, Icon: LucideIcon, current: boolean, go: () => void, badge?: ReactNode) => (
    <button className={current ? "row plain current" : "row plain"} aria-current={current ? "page" : undefined} onClick={go}>
      <Icon />
      <span className="label"><span>{text}</span></span>
      {badge}
    </button>
  );

  if (unpaired) return <Pair onPair={pair} />;
  return (
    // In Bate's window there is no title bar: the ground round the sheets, a header's gaps and the page's title move the window; nothing that scrolls or opens does.
    <div className={inShell ? "app shell" : "app"} data-tauri-drag-region="">
      <nav className="sidebar" aria-label="Projects and bots">
        <ul>
          <li>{plain("Activity", Inbox, view?.page === "attention", () => setView({ page: "attention" }), waiting > 0 && <span className="badge">{waiting} {waiting === 1 ? "needs" : "need"} you</span>)}</li>
        </ul>
        <div className="heading">
          <h2>Projects</h2>
          <button className="round ghost" aria-label="New project" data-tip="New project" onClick={actions["new-project"]}><Plus /></button>
        </div>
        <ul>
          {lists.projects.map(({ project: p, bots: members }) => (
            <li key={p.id}>
              {plain(p.name, Folder, view?.page === "project" && view.id === p.id, () => openProject(p.id!, view?.page === "project" ? view.tab : "room"))}
              {members.length > 0 && <ul>{members.map(botRow)}</ul>}
            </li>
          ))}
        </ul>
        <div className="heading">
          <h2>Bots</h2>
          <button className="round ghost" aria-label="New bot" data-tip="New bot (⌘N)" onClick={actions["new-bot"]}><Plus /></button>
        </div>
        <ul>{lists.loose.map(botRow)}</ul>
        {bot && view?.page === "bot" && <Talks key={`${bot.id}-${connection}`} bot={bot.id} shown={view.conversation} open={(c) => openBot(bot.id, "chat", c)} />}
        <ul className="foot">
          {bot && view?.page === "bot" && <li><button className="row new" onClick={() => void newTalk(bot.id)}><SquarePen />New conversation</button></li>}
          <li>{plain("Add-ons", Blocks, view?.page === "addons", () => setView({ page: "addons", tab: view?.page === "addons" ? view.tab : "connectors" }))}</li>
          <li>{plain("Settings", Gear, view?.page === "settings", () => setView({ page: "settings" }))}</li>
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
        {view?.page === "attention" && (
          <>
            <header data-tauri-drag-region=""><div className="who" data-tauri-drag-region="deep"><h1>Activity</h1></div></header>
            <Attention key={connection} bots={bots} open={(id, conversation) => openBot(id, "chat", conversation)} />
          </>
        )}
        {view?.page === "new-bot" && (
          <>
            <header data-tauri-drag-region=""><div className="who" data-tauri-drag-region="deep"><h1>New bot</h1></div></header>
            <NewBot opened={(id) => openBot(id)} report={setError} />
          </>
        )}
        {view?.page === "projects" && (
          <>
            <header data-tauri-drag-region=""><div className="who" data-tauri-drag-region="deep"><h1>Projects</h1></div></header>
            <Projects key={`${connection}-${view.asked}`} adding={view.asked !== undefined} bots={bots} projects={projects} open={openProject} />
          </>
        )}
        {view?.page === "addons" && (
          <>
            <header data-tauri-drag-region="">
              <div className="who titled" data-tauri-drag-region="">
                <PageMenu label="Pages of Add-ons" ids={Object.keys(ADDON_TABS) as AddonTab[]} names={ADDON_TABS} tab={view.tab} set={(tab) => setView({ page: "addons", tab })}>
                  <span className="name">Add-ons</span>
                </PageMenu>
              </div>
            </header>
              {view.tab === "connectors" && <Connectors key={connection} bots={bots} />}
              {view.tab === "skills" && <Skills key={connection} bots={bots} />}
              {view.tab === "workplaces" && <Workplaces key={connection} projects={projects} />}
          </>
        )}
        {view?.page === "settings" && (
          <>
            <header data-tauri-drag-region=""><div className="who" data-tauri-drag-region="deep"><h1>Settings</h1></div></header>
            <AppSettings key={connection} section={view.section} choose={(section) => setView({ page: "settings", section })} />
          </>
        )}
        {view?.page === "project" && project && (
          <>
            <header data-tauri-drag-region="">
              <div className="who titled" data-tauri-drag-region="">
                <PageMenu label={`Pages of ${project.name}`} ids={projectTabs} names={PROJECT_TABS} tab={view.tab} set={(t) => openProject(project.id!, t)}>
                  <span className="name">{project.name}</span>
                </PageMenu>
              </div>
            </header>
            {view.tab === "room" && <Room key={`${project.id}-${connection}`} project={project} bots={bots} />}
            {view.tab === "bots" && <ProjectBots key={`${project.id}-${connection}`} project={project} bots={bots} />}
            {/* Surfaces open in the window's pane only, not on other devices. */}
            {view.tab === "surfaces" && inShell && (
              <Surfaces
                key={`surfaces-${project.id}-${connection}`}
                projectId={project.id!}
                current={watched?.projectId === project.id ? watched?.uri : undefined}
                open={(s) => setShown({ projectId: project.id!, uri: s.uri, name: s.name })}
                workplaces={() => setView({ page: "addons", tab: "workplaces" })}
              />
            )}
            {view.tab === "memory" && <Memory key={`${project.id}-${connection}`} target={{ projectId: project.id! }} title="Project notes" />}
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
            <header data-tauri-drag-region="">
              <div className="who titled" data-tauri-drag-region="">
                {bot.project && (
                  <>
                    <button type="button" className="crumb" onClick={() => openProject(bot.project!.id, "room")}>{bot.project.name}</button>
                    <span className="slash" aria-hidden="true">/</span>
                  </>
                )}
                <PageMenu label={`Pages of ${bot.name}`} ids={botTabs} names={BOT_TABS} tab={view.tab} set={(t) => openBot(bot.id, t)}>
                  <Avatar bot={bot} />
                  <span className="name">{bot.name}</span>
                </PageMenu>
              </div>
              {/* The conversation chip is drawn here by Conversations. */}
              {view.tab === "chat" && <div className="conversation" ref={setConversationSlot} data-tauri-drag-region="" />}
            </header>
            {view.tab === "chat" && <Mail key={`mail-${bot.id}-${connection}`} bot={bot.id} bots={bots} />}
            {view.tab === "chat" && (
              <Conversations
                key={`conversations-${bot.id}-${connection}`}
                bot={bot.id}
                want={jump?.conversation ?? view.conversation}
                slot={conversationSlot}
                showChanges={inShell ? (conversation) => setShown({ changes: { bot: bot.id, conversation } }) : undefined}
                shownAs={(c) => setView((v) => (v?.page === "bot" && v.id === bot.id && v.conversation !== c ? { ...v, conversation: c } : v))}
              >
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
                  // So does the diff of what the bot changed.
                  showChanges={inShell ? () => setShown({ changes: { bot: bot.id, conversation } }) : undefined}
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
            {view.tab === "memory" && (
              <>
                {bot.agent && <p className="banner" role="status">An installed agent keeps its own memory: these notes, and the project's notes and instructions, do not reach it.</p>}
                <Memory key={`${bot.id}-${connection}`} target={{ bot: bot.id }} title={`${bot.name}'s notes`} />
                {bot.project && <Memory key={`${bot.project.id}-${connection}`} target={{ projectId: bot.project.id }} title="Project notes" />}
              </>
            )}
            {view.tab === "channels" && inShell && <Channels key={`${bot.id}-${connection}`} bot={bot.id} />}
            {view.tab === "settings" && <Settings key={`${bot.id}-${connection}`} bot={bot} onDeleted={() => setView(undefined)} />}
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
      {shown && "changes" in shown && changesBot && (
        <Pane label="Changes" title={`Changes: ${changesBot.name}`} close={() => setShown(undefined)}>
          <Changes key={`${changesBot.id}-${shown.changes.conversation}`} bot={changesBot} conversation={shown.changes.conversation} />
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

// The shell starts with the system's appearance; a choice made in Settings is set again in each window's page.
if (inShell && savedLook() !== "system") void setLook(savedLook());

createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    {inQuickPanel ? <QuickAsk /> : <App />}
  </StrictMode>,
);

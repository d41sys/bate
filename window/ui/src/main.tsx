import { StrictMode, useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { inShell, listen, newWindow, openInBrowser, pair, request, serviceNote } from "./service.ts";
import { Pair } from "./Devices.tsx";
import type { Bot } from "./types.ts";
import { Model } from "./Model.tsx";
import { Chat } from "./Chat.tsx";
import { Mail } from "./Mail.tsx";
import { FilePane, Pane } from "./Pane.tsx";
import { SurfaceView, Surfaces } from "./Surfaces.tsx";
import { Routines } from "./Routines.tsx";
import { Connectors } from "./Connectors.tsx";
import { Memory } from "./Memory.tsx";
import { Skills } from "./Skills.tsx";
import { Groups } from "./Groups.tsx";
import { Projects } from "./Projects.tsx";
import { Room } from "./Room.tsx";
import { Settings } from "./Settings.tsx";
import { FirstRun } from "./FirstRun.tsx";
import { Palette, type Choice } from "./Palette.tsx";
import "./style.css";

// Only in the build `window/check-frames.sh` makes.
if (import.meta.env.VITE_FRAME_CHECK) void import("./frameCheck.ts");

const TABS = { chat: "Chat", routines: "Routines", connectors: "Connectors", memory: "Memory", skills: "Skills", groups: "Groups", projects: "Projects", room: "Room", settings: "Settings" };
type Tab = keyof typeof TABS;

function App() {
  const [state, setState] = useState("Connecting to the service…");
  const [bots, setBots] = useState<Bot[]>([]);
  const [selected, setSelected] = useState<string>();
  const [tab, setTab] = useState<Tab>("chat");
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
  const surfaceProject = watched?.projectId ?? bots.find((b) => b.id === selected)?.project?.id;
  const surfaceUri = watched?.uri;
  useEffect(() => {
    if (inShell) request("watch_surfaces", { projectId: surfaceProject ?? null, uri: surfaceUri }).catch(() => {});
  }, [surfaceProject, surfaceUri, connection]);

  useEffect(() => {
    const load = () => request<{ bots: Bot[] }>("list_bots").then((d) => setBots(d.bots), (e) => setError(e.message));
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
      } else if (frame.type === "failed") {
        setState(`The connection failed: ${frame.error}`);
      } else if (frame.type === "bots_changed" || frame.type === "projects_changed") {
        load();
      }
    });
  }, []);

  const create = async () => {
    try {
      const { bot } = await request<{ bot: Bot }>("create_bot", { name: name.trim() });
      setName("");
      setSelected(bot.id);
      setTab("chat");
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
    const at = bots.findIndex((b) => b.id === id);
    const onto = bots[at + by];
    if (onto) reorder(id, onto.id);
  };

  const bot = bots.find((b) => b.id === selected);
  const open = (id: string, t: Tab = "chat") => {
    setSelected(id);
    setTab(t);
  };
  // What the menus, the keys and the palette can do, by the menu item's id.
  const actions: Record<string, () => void> = {
    palette: () => setPalette(true),
    find: () => {
      if (!bot) return;
      setTab("chat");
      setFind(true);
    },
    "new-bot": () => newName.current?.focus(),
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
    ...bots.map((b) => ({ label: `${b.avatar ? `${b.avatar} ` : ""}${b.name}`, hint: "bot", run: () => open(b.id) })),
    ...(bot ? (Object.keys(TABS) as Tab[]).map((t) => ({ label: TABS[t], hint: bot.name, run: () => setTab(t) })) : []),
    ...(bot ? [{ label: "Find in chat", hint: "⌘F", run: actions.find }] : []),
    { label: "New bot", hint: "⌘N", run: actions["new-bot"] },
    { label: "New window", hint: "⇧⌘N", run: actions["new-window"] },
  ];

  if (unpaired) return <Pair onPair={pair} />;
  return (
    <div className="app">
      <nav className="bots" aria-label="Bots">
        <ul>
          {bots.map((b) => (
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
              style={{ borderLeft: `4px solid ${b.color || "transparent"}` }}
            >
              <button
                className={b.id === selected ? "current" : ""}
                onClick={() => setSelected(b.id)}
                title="Alt+↑ and Alt+↓ move it"
                onKeyDown={(e) => {
                  if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
                    e.preventDefault();
                    move(b.id, e.key === "ArrowUp" ? -1 : 1);
                  }
                }}
              >
                {b.avatar && <span className="avatar">{b.avatar} </span>}
                {b.name}
                {b.busy ? " · working" : ""}
              </button>
            </li>
          ))}
        </ul>
        <form onSubmit={(e) => { e.preventDefault(); create(); }}>
          <input ref={newName} aria-label="New bot's name" placeholder="New bot" maxLength={80} value={name} onChange={(e) => setName(e.target.value)} />
          <button disabled={!name.trim()}>Create</button>
        </form>
      </nav>
      <main>
        {state && <p className="note">{state}</p>}
        {note && <p className="note">{note}</p>}
        {inShell && !state && <FirstRun hasBots={bots.length > 0} reload={`${connection}-${tab}`} onSettings={() => open(bot?.id ?? bots[0].id, "settings")} />}
        {error && <p className="error" role="alert" onClick={() => setError("")}>{error}</p>}
        {bot ? (
          <>
            <header>
              <h1>{bot.avatar && `${bot.avatar} `}{bot.name}</h1>
              <div role="tablist">
                {(Object.keys(TABS) as Tab[]).map((t) => (
                  <button key={t} role="tab" aria-selected={t === tab} onClick={() => setTab(t)}>{TABS[t]}</button>
                ))}
              </div>
            </header>
            {/* An installed agent chooses its own model. */}
            {tab === "chat" && !bot.agent && <Model key={bot.id} bot={bot} bots={bots} />}
            {tab === "chat" && <Mail key={`mail-${bot.id}-${connection}`} bot={bot.id} bots={bots} />}
            {/* Surfaces open in the window's pane only, not on other devices. */}
            {tab === "chat" && inShell && bot.project && (
              <Surfaces
                key={`surfaces-${bot.project.id}-${connection}`}
                projectId={bot.project.id}
                open={(s) => setShown({ projectId: bot.project!.id, uri: s.uri, name: s.name })}
              />
            )}
            {tab === "chat" && <Chat
                key={`${bot.id}-${connection}`}
                bot={bot}
                jump={jump}
                find={find}
                // Bots' files open in the window's pane only, not on other devices.
                showFile={inShell ? (path) => setShown({ bot: bot.id, path }) : undefined}
                used={() => {
                  setJump(undefined);
                  setFind(false);
                }}
              />}
            {tab === "routines" && <Routines key={`${bot.id}-${connection}`} bot={bot.id} />}
            {tab === "connectors" && <Connectors key={`${bot.id}-${connection}`} bot={bot.id} />}
            {tab === "memory" && (
              <>
                {bot.agent && <p className="note">An installed agent keeps its own memory: these notes, and the project's notes and instructions, do not reach it.</p>}
                <Memory key={`${bot.id}-${connection}`} target={{ bot: bot.id }} />
                {bot.project && (
                  <>
                    <h2>Notes of the project {bot.project.name}, shared by its bots</h2>
                    <Memory key={`${bot.project.id}-${connection}`} target={{ projectId: bot.project.id }} />
                  </>
                )}
              </>
            )}
            {tab === "skills" && <Skills key={`${bot.id}-${connection}`} bot={bot.id} />}
            {tab === "groups" && <Groups key={connection} bots={bots} />}
            {tab === "projects" && <Projects key={connection} bots={bots} />}
            {tab === "room" && (bot.project ? <Room key={`${bot.project.id}-${connection}`} project={bot.project} bots={bots} /> : <p className="note">This bot is in no project, so it has no room.</p>)}
            {tab === "settings" && <Settings key={`${bot.id}-${connection}`} bot={bot} onDeleted={() => setSelected(undefined)} />}
          </>
        ) : (
          !state && <p className="note">{bots.length ? "Choose a bot." : "Create a bot to begin."}</p>
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
            open(id);
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

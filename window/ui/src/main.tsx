import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { listen, request } from "./service.ts";
import type { Bot } from "./types.ts";
import { Chat } from "./Chat.tsx";
import { Routines } from "./Routines.tsx";
import { Connectors } from "./Connectors.tsx";
import { Settings } from "./Settings.tsx";
import "./style.css";

const TABS = { chat: "Chat", routines: "Routines", connectors: "Connectors", settings: "Settings" };
type Tab = keyof typeof TABS;

function App() {
  const [state, setState] = useState("Connecting to the service…");
  const [bots, setBots] = useState<Bot[]>([]);
  const [selected, setSelected] = useState<string>();
  const [tab, setTab] = useState<Tab>("chat");
  const [name, setName] = useState("");
  const [error, setError] = useState("");
  // Bumped on each new connection, so every screen loads again.
  const [connection, setConnection] = useState(0);

  useEffect(() => {
    const load = () => request<{ bots: Bot[] }>("list_bots").then((d) => setBots(d.bots), (e) => setError(e.message));
    return listen((frame) => {
      if (frame.type === "connected") {
        setState("");
        setConnection((n) => n + 1);
        load();
      } else if (frame.type === "failed") {
        setState(`The connection failed: ${frame.error}`);
      } else if (frame.type === "bots_changed") {
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

  const bot = bots.find((b) => b.id === selected);
  return (
    <div className="app">
      <nav className="bots" aria-label="Bots">
        <ul>
          {bots.map((b) => (
            <li key={b.id}>
              <button className={b.id === selected ? "current" : ""} onClick={() => setSelected(b.id)}>
                {b.name}
                {b.busy ? " · working" : ""}
              </button>
            </li>
          ))}
        </ul>
        <form onSubmit={(e) => { e.preventDefault(); create(); }}>
          <input aria-label="New bot's name" placeholder="New bot" maxLength={80} value={name} onChange={(e) => setName(e.target.value)} />
          <button disabled={!name.trim()}>Create</button>
        </form>
      </nav>
      <main>
        {state && <p className="note">{state}</p>}
        {error && <p className="error" role="alert" onClick={() => setError("")}>{error}</p>}
        {bot ? (
          <>
            <header>
              <h1>{bot.name}</h1>
              <div role="tablist">
                {(Object.keys(TABS) as Tab[]).map((t) => (
                  <button key={t} role="tab" aria-selected={t === tab} onClick={() => setTab(t)}>{TABS[t]}</button>
                ))}
              </div>
            </header>
            {tab === "chat" && <Chat key={`${bot.id}-${connection}`} bot={bot} />}
            {tab === "routines" && <Routines key={`${bot.id}-${connection}`} bot={bot.id} />}
            {tab === "connectors" && <Connectors key={`${bot.id}-${connection}`} bot={bot.id} />}
            {tab === "settings" && <Settings key={`${bot.id}-${connection}`} bot={bot} />}
          </>
        ) : (
          !state && <p className="note">{bots.length ? "Choose a bot." : "Create a bot to begin."}</p>
        )}
      </main>
    </div>
  );
}

createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

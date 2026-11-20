import { useEffect, useState } from "react";
import { Devices } from "./Devices.tsx";
import { Group, Problem, Saved, Setting, useSaves } from "./Form.tsx";
import { Helper } from "./Helper.tsx";
import { Updates } from "./Updates.tsx";
import { inShell, listen, openInBrowser, request } from "./service.ts";

/** The app's own settings, in one scroll. `settings-<key>` is the id of a section's heading, which the list and the palette focus. */
const SECTIONS = { chatgpt: "ChatGPT", model: "Model server", devices: "Other devices", helper: "Bate Helper", updates: "Updates", report: "Bug report" };
export type SettingsSection = keyof typeof SECTIONS;
const PAGES: Record<SettingsSection, () => React.JSX.Element> = { chatgpt: ChatGpt, model: ModelServer, devices: Devices, helper: Helper, updates: Updates, report: Report };
/** Another device on the network may not read the sign-in, the model server, the devices or the helper. */
const SHELL_ONLY: SettingsSection[] = ["chatgpt", "model", "devices", "helper"];
export const settingsSections = (Object.keys(SECTIONS) as SettingsSection[]).filter((s) => inShell || !SHELL_ONLY.includes(s));

/** What belongs to the app, not to a bot: the sign-in, the model server, devices, the helper, updates and the bug report. */
export function AppSettings({ section, choose }: { section?: SettingsSection; choose: (s: SettingsSection) => void }) {
  const at = section && settingsSections.includes(section) ? section : settingsSections[0];
  return (
    <div className="settings">
      <nav aria-label="Settings sections">
        <ul>
          {settingsSections.map((s) => (
            <li key={s}>
              <button
                className={`row plain${s === at ? " current" : ""}`}
                aria-current={s === at ? "page" : undefined}
                onClick={() => {
                  choose(s);
                  // The heading is on the page already: the focus takes the scroll there.
                  document.getElementById(`settings-${s}`)?.focus();
                }}
              >
                {SECTIONS[s]}
              </button>
            </li>
          ))}
        </ul>
      </nav>
      <div>
        {settingsSections.map((s) => {
          const Page = PAGES[s];
          return <Page key={s} />;
        })}
      </div>
    </div>
  );
}

function ChatGpt() {
  const [signedIn, setSignedIn] = useState<boolean>();
  const { run, fail, error } = useSaves();
  useEffect(() => {
    const load = () => request<{ signedIn: boolean }>("get_chatgpt").then((c) => setSignedIn(c.signedIn), (e: Error) => fail("load", e.message));
    load();
    return listen((frame) => {
      if (frame.type !== "chatgpt_changed") return;
      if (typeof frame.error === "string") fail("chatgpt", frame.error);
      load();
    });
  }, []);
  return (
    <Group id="settings-chatgpt" title="ChatGPT">
      {signedIn === undefined && !error("load") && <p className="note">Loading…</p>}
      <Problem text={error("load")} />
      {signedIn === true && (
        <Setting title="Signed in" hint="Every bot runs on your ChatGPT plan, not on the model server.">
          <button onClick={() => run("chatgpt", request("sign_out_chatgpt"))}>Sign out</button>
        </Setting>
      )}
      {signedIn === false && (
        <Setting title="Not signed in" hint="Bots then run on your ChatGPT plan. The sign-in happens in your browser.">
          <button className="primary" onClick={() => run("chatgpt", request<{ url: string }>("sign_in_chatgpt").then((d) => openInBrowser(d.url)))}>Sign in with ChatGPT</button>
        </Setting>
      )}
      <Problem text={error("chatgpt")} />
    </Group>
  );
}

type Provider = { url: string | null; hasKey: boolean; fromEnv: boolean };

function ModelServer() {
  const [provider, setProvider] = useState<Provider>();
  const [url, setUrl] = useState("");
  const [key, setKey] = useState("");
  const { run, fail, edit, saved, error } = useSaves();
  // What the service has, after every change: the field shows the saved server.
  const load = () =>
    request<Provider>("get_provider").then((p) => {
      setUrl(p.url ?? "");
      setKey("");
      setProvider(p);
    }, (e: Error) => fail("load", e.message));
  useEffect(() => void load(), []);
  return (
    <Group id="settings-model" title="Model server">
      {!provider && !error("load") && <p className="note">Loading…</p>}
      <Problem text={error("load")} />
      {provider && (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            run("save", request("set_provider", { url: url.trim(), key: key.trim() }).then(load));
          }}
        >
          <Setting id="provider-url" title="URL" hint="For when you are not signed in to ChatGPT. http:// or https:// with a host, and no user name, password or query." error={error("save")}>
            <input placeholder="http://127.0.0.1:1234/v1" value={url} onChange={(e) => (edit("save"), setUrl(e.target.value))} />
          </Setting>
          <Setting id="provider-key" title="API key" hint="Optional. A saved key is never shown again; enter a new one to change it.">
            <input type="password" autoComplete="off" placeholder={provider.hasKey ? (provider.fromEnv ? "(key from the environment)" : "(key saved; enter new to change)") : "optional"} value={key} onChange={(e) => (edit("save"), setKey(e.target.value))} />
          </Setting>
          <div className="actions">
            <button>Save</button>
            {provider.hasKey && !provider.fromEnv && (
              <button type="button" onClick={() => run("remove", request("set_provider", { url: provider.url, key: null }).then(load))}>Remove key</button>
            )}
            <Saved on={saved("save") || saved("remove")} />
            <Problem text={error("remove")} />
          </div>
        </form>
      )}
    </Group>
  );
}

function Report() {
  const [path, setPath] = useState("");
  const { run, error } = useSaves();
  return (
    <Group id="settings-report" title="Bug report">
      <Setting title="Logs and a summary of the settings" hint="Conversations are not included.">
        <button onClick={() => run("report", request<{ path: string }>("export_report").then((d) => setPath(d.path)))}>Export a bug report</button>
      </Setting>
      {path && <p>Saved to <code>{path}</code></p>}
      <Problem text={error("report")} />
    </Group>
  );
}

import { useEffect, useState } from "react";
import { Devices } from "./Devices.tsx";
import { Field, Problem, Saved, useSaves } from "./Form.tsx";
import { Helper } from "./Helper.tsx";
import { Updates } from "./Updates.tsx";
import { inShell, listen, openInBrowser, request } from "./service.ts";

/** The app's own settings, one section at a time. `settings-<key>` is the id of a section's heading, which the palette focuses. */
const SECTIONS = { chatgpt: "ChatGPT", model: "Model server", devices: "Other devices", helper: "Bate Helper", updates: "Updates", report: "Bug report" };
export type SettingsSection = keyof typeof SECTIONS;
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
              <button className={`row plain${s === at ? " current" : ""}`} aria-current={s === at ? "page" : undefined} onClick={() => choose(s)}>{SECTIONS[s]}</button>
            </li>
          ))}
        </ul>
      </nav>
      <section aria-labelledby={`settings-${at}`}>
        {at === "chatgpt" && <ChatGpt />}
        {at === "model" && <ModelServer />}
        {at === "devices" && <Devices />}
        {at === "helper" && <Helper />}
        {at === "updates" && <Updates />}
        {at === "report" && <Report />}
      </section>
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
    <>
      <h2 id="settings-chatgpt" tabIndex={-1}>ChatGPT</h2>
      {signedIn === undefined && !error("load") && <p className="note">Loading…</p>}
      <Problem text={error("load")} />
      {signedIn === true && (
        <>
          <p>Signed in: every bot runs on your ChatGPT plan, not on the model server.</p>
          <div className="actions">
            <button onClick={() => run("chatgpt", request("sign_out_chatgpt"))}>Sign out</button>
          </div>
        </>
      )}
      {signedIn === false && (
        <>
          <p>Bots then run on your ChatGPT plan. The sign-in happens in your browser.</p>
          <div className="actions">
            <button className="primary" onClick={() => run("chatgpt", request<{ url: string }>("sign_in_chatgpt").then((d) => openInBrowser(d.url)))}>Sign in with ChatGPT</button>
          </div>
        </>
      )}
      <Problem text={error("chatgpt")} />
    </>
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
    <>
      <h2 id="settings-model" tabIndex={-1}>Model server</h2>
      {!provider && !error("load") && <p className="note">Loading…</p>}
      <Problem text={error("load")} />
      {provider && (
        <>
          {!provider.url && <p className="note">No model server is set. Enter its address to use one when you are not signed in to ChatGPT.</p>}
          <form
            className="fields stack"
            onSubmit={(e) => {
              e.preventDefault();
              run("save", request("set_provider", { url: url.trim(), key: key.trim() }).then(load));
            }}
          >
            <Field id="provider-url" label="URL" hint="http:// or https:// with a host, and no user name, password or query." error={error("save")}>
              <input placeholder="http://127.0.0.1:1234/v1" value={url} onChange={(e) => (edit("save"), setUrl(e.target.value))} />
            </Field>
            <Field id="provider-key" label="API key" hint="Optional. A saved key is never shown again; enter a new one to change it.">
              <input type="password" autoComplete="off" placeholder={provider.hasKey ? (provider.fromEnv ? "(key from the environment)" : "(key saved; enter new to change)") : "optional"} value={key} onChange={(e) => (edit("save"), setKey(e.target.value))} />
            </Field>
            <div className="actions">
              <button>Save</button>
              {provider.hasKey && !provider.fromEnv && (
                <button type="button" onClick={() => run("remove", request("set_provider", { url: provider.url, key: null }).then(load))}>Remove key</button>
              )}
              <Saved on={saved("save") || saved("remove")} />
            </div>
            <Problem text={error("remove")} />
          </form>
        </>
      )}
    </>
  );
}

function Report() {
  const [path, setPath] = useState("");
  const { run, error } = useSaves();
  return (
    <>
      <h2 id="settings-report" tabIndex={-1}>Bug report</h2>
      <p className="note">A file with the service's logs and a summary of the settings. Conversations are not included.</p>
      <div className="actions">
        <button onClick={() => run("report", request<{ path: string }>("export_report").then((d) => setPath(d.path)))}>Export a bug report</button>
      </div>
      {path && <p>Saved to <code>{path}</code></p>}
      <Problem text={error("report")} />
    </>
  );
}

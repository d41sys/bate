import { useEffect, useState } from "react";
import { Devices } from "./Devices.tsx";
import { Helper } from "./Helper.tsx";
import { Updates } from "./Updates.tsx";
import { inShell, listen, openInBrowser, request } from "./service.ts";

/** What belongs to the app, not to a bot: the sign-in, the model server, devices, the helper, updates and the bug report. */
export function AppSettings() {
  const [report, setReport] = useState("");
  const [serverUrl, setServerUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [provider, setProvider] = useState<{ url: string | null; hasKey: boolean; fromEnv: boolean }>({ url: null, hasKey: false, fromEnv: false });
  const [error, setError] = useState("");
  const [chatgpt, setChatgpt] = useState(false);

  useEffect(() => {
    if (!inShell) return;
    const load = () => request<{ signedIn: boolean }>("get_chatgpt").then((c) => setChatgpt(c.signedIn), () => {});
    load();
    loadProvider().catch(() => {});
    return listen((frame) => {
      if (frame.type !== "chatgpt_changed") return;
      if (typeof frame.error === "string") setError(frame.error);
      load();
    });
  }, []);

  // What the service has, after every change: the field shows the saved server.
  const loadProvider = () =>
    request<typeof provider>("get_provider").then((p) => {
      setServerUrl(p.url ?? "");
      setApiKey("");
      setProvider(p);
    });
  const run = (p: Promise<unknown>) => p.then(() => setError(""), (e: Error) => setError(e.message));

  return (
    <section className="fields">
      {inShell && (
        <>
          <h2 id="settings-chatgpt" tabIndex={-1}>ChatGPT</h2>
          {chatgpt ? (
            <p>
              Signed in: every bot runs on your ChatGPT plan, not on the model server below.{" "}
              <button onClick={() => run(request("sign_out_chatgpt"))}>Sign out</button>
            </p>
          ) : (
            <p>
              <button onClick={() => run(request<{ url: string }>("sign_in_chatgpt").then((d) => openInBrowser(d.url)))}>Sign in with ChatGPT</button>{" "}
              Bots then run on your ChatGPT plan. The sign-in happens in your browser.
            </p>
          )}
          <h2>Model server</h2>
          <form onSubmit={(e) => {
            e.preventDefault();
            run(request("set_provider", { url: serverUrl.trim(), key: apiKey.trim() }).then(loadProvider));
          }}>
            <label>URL <input placeholder="http://127.0.0.1:1234/v1" value={serverUrl} onChange={(e) => setServerUrl(e.target.value)} /></label>
            <label>API key <input type="password" placeholder={provider.hasKey ? (provider.fromEnv ? "(key from the environment)" : "(key saved; enter new to change)") : "optional"} value={apiKey} onChange={(e) => setApiKey(e.target.value)} /></label>
            <button>Save</button>
            {provider.hasKey && !provider.fromEnv && (
              <button type="button" onClick={() => run(request("set_provider", { url: provider.url, key: null }).then(loadProvider))}>Remove key</button>
            )}
          </form>
          <Devices />
          <Helper />
        </>
      )}
      <Updates />
      <h2>Bug report</h2>
      <button onClick={() => run(request<{ path: string }>("export_report").then((d) => setReport(d.path)))}>Export a bug report</button>
      {report && <p>Saved to <code>{report}</code></p>}
      {error && <p className="error" role="alert">{error}</p>}
    </section>
  );
}

import { useEffect, useState } from "react";
import { listen, request } from "./service.ts";
import { DISMISSED, needsProvider, type Provider } from "./firstRun.ts";

/**
 * Shown in Bate's own window until a provider is set or the user says "Not now": it points at
 * Settings, where ChatGPT and the model server are. `reload` changes when the user may have
 * been to Settings, so the screen goes once a provider is saved.
 */
export function FirstRun({ reload, onSettings }: { reload: string; onSettings: () => void }) {
  const [needed, setNeeded] = useState(false);
  const [dismissed, setDismissed] = useState(() => localStorage.getItem(DISMISSED) === "1");
  useEffect(() => {
    const load = () =>
      Promise.all([request<{ signedIn: boolean }>("get_chatgpt"), request<Provider>("get_provider")]).then(
        ([chatgpt, provider]) => setNeeded(needsProvider(chatgpt.signedIn, provider)),
        () => {},
      );
    void load();
    return listen((frame) => frame.type === "chatgpt_changed" && void load());
  }, [reload]);
  if (!needed || dismissed) return null;
  return (
    <section className="first-run" aria-label="First run">
      <h2>Welcome to Bate</h2>
      <p>
        A bot needs a model to answer. Open <b>Settings</b>, at the foot of the sidebar, and either sign in under <b>ChatGPT</b> (your plan; the
        sign-in happens in your browser) or give a <b>Model server</b> (a URL such as <code>http://127.0.0.1:1234/v1</code>).
      </p>
      <button onClick={onSettings}>Open Settings</button>{" "}
      <button
        onClick={() => {
          localStorage.setItem(DISMISSED, "1");
          setDismissed(true);
        }}
      >
        Not now
      </button>
    </section>
  );
}

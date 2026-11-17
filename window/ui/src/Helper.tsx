import { useEffect, useState } from "react";
import { listen, request } from "./service.ts";

type Helper = { installed: boolean; path: string; connected: boolean; accessibility: boolean | null; screenRecording: boolean | null };

/** Bate Helper: the app that holds the macOS permissions for seeing and controlling other apps. */
export function Helper() {
  const [helper, setHelper] = useState<Helper>();
  const [error, setError] = useState("");
  const load = () => request<Helper>("get_helper").then(setHelper, (e: Error) => setError(e.message));
  useEffect(() => {
    void load();
    return listen((frame) => frame.type === "helper_changed" && void load());
  }, []);
  const open = (pane: "accessibility" | "screen_recording") =>
    request("open_helper_settings", { pane }).then(() => setError(""), (e: Error) => setError(e.message));
  // Unknown until the helper runs: it starts when a bot first uses it, or when a button below is pressed.
  const state = (granted: boolean | null) => (granted === null ? "not known yet" : granted ? "allowed" : "not allowed");
  return (
    <>
      <h2>Bate Helper</h2>
      {helper && !helper.installed && (
        <p>
          Bots cannot see or control other apps until Bate Helper is installed at <code>{helper.path}</code>. Build it with <code>helper/bundle.sh</code>.
        </p>
      )}
      {helper?.installed && (
        <ul className="rows">
          <li>
            Accessibility (read and control other apps): {state(helper.accessibility)}{" "}
            <button onClick={() => open("accessibility")}>Open System Settings</button>
          </li>
          <li>
            Screen Recording (pictures of windows): {state(helper.screenRecording)}{" "}
            <button onClick={() => open("screen_recording")}>Open System Settings</button>
          </li>
        </ul>
      )}
      {error && <p className="error" role="alert">{error}</p>}
    </>
  );
}

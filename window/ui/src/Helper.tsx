import { useEffect, useState } from "react";
import { Problem, useSaves } from "./Form.tsx";
import { listen, request } from "./service.ts";

type Helper = { installed: boolean; path: string; connected: boolean; accessibility: boolean | null; screenRecording: boolean | null; fullDiskAccess: boolean | null };

/** Bate Helper: the app that holds the macOS permissions for seeing and controlling other apps. */
export function Helper() {
  const [helper, setHelper] = useState<Helper>();
  const { run, fail, error } = useSaves();
  const load = () => request<Helper>("get_helper").then(setHelper, (e: Error) => fail("load", e.message));
  useEffect(() => {
    void load();
    return listen((frame) => frame.type === "helper_changed" && void load());
  }, []);
  const open = (pane: "accessibility" | "screen_recording" | "full_disk_access") => run(pane, request("open_helper_settings", { pane }));
  // Unknown until the helper runs: it starts when a bot first uses it, or when a button below is pressed.
  const state = (granted: boolean | null) => (granted === null ? "Not known yet" : granted ? "Allowed" : "Not allowed");
  const panes = helper && [
    { pane: "accessibility" as const, name: "Accessibility", what: "Read and control other apps", granted: helper.accessibility },
    { pane: "screen_recording" as const, name: "Screen Recording", what: "Pictures of windows", granted: helper.screenRecording },
    { pane: "full_disk_access" as const, name: "Full Disk Access", what: "Read your Messages history", granted: helper.fullDiskAccess },
  ];
  return (
    <>
      <h2 id="settings-helper" tabIndex={-1}>Bate Helper</h2>
      {!helper && !error("load") && <p className="note">Loading…</p>}
      <Problem text={error("load")} />
      {helper && !helper.installed && (
        <p>
          Bots cannot see or control other apps until Bate Helper is installed at <code>{helper.path}</code>. Build it with <code>helper/bundle.sh</code>.
        </p>
      )}
      {helper?.installed && (
        <ul className="rows">
          {panes!.map(({ pane, name, what, granted }) => (
            <li key={pane}>
              <div className="what">
                <strong>{name}</strong>
                <div className="note">{what}</div>
                <small>{state(granted)}</small>
                {pane === "full_disk_access" && granted === false && <small>If Bate Helper is not listed there, add it with the + button.</small>}
              </div>
              <div className="actions">
                <button className="small" aria-label={`Open System Settings: ${name}`} onClick={() => open(pane)}>Open System Settings</button>
              </div>
              {error(pane) && <small className="error" role="alert">{error(pane)}</small>}
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

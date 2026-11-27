import { useEffect, useState } from "react";
import { Group, Problem, Setting, useSaves } from "./Form.tsx";
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
    <Group id="settings-helper" title="Bate Helper">
      {!helper && !error("load") && <p className="note">Loading…</p>}
      <Problem text={error("load")} />
      {helper && !helper.installed && (
        <p>
          Bots cannot see or control other apps until Bate Helper is installed at <code>{helper.path}</code>. Build it with <code>helper/bundle.sh</code>.
        </p>
      )}
      {helper?.installed &&
        panes!.map(({ pane, name, what, granted }) => (
          <Setting
            key={pane}
            title={name}
            hint={`${what}: ${state(granted)}${pane === "full_disk_access" && granted === false ? ". If Bate Helper is not listed there, add it with the + button." : ""}`}
            error={error(pane)}
          >
            <button className="small" aria-label={`Open System Settings: ${name}`} onClick={() => open(pane)}>Open System Settings</button>
          </Setting>
        ))}
    </Group>
  );
}

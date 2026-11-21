import { useEffect, useState } from "react";
import { Group, Problem, Setting, useSaves } from "./Form.tsx";
import { inShell, listen, request } from "./service.ts";

type Update = { state: "off" | "current" | "checking" | "ready" | "waiting" | "error"; current: string; available: string | null; error: string | null };

/** The version, a check, and the install of a release the service has downloaded and verified. */
export function Updates() {
  const [update, setUpdate] = useState<Update>();
  const { run, fail, error } = useSaves();
  const load = () => request<Update>("get_update").then(setUpdate, (e: Error) => fail("load", e.message));
  useEffect(() => {
    void load();
    return listen((frame) => frame.type === "update_changed" && void load());
  }, []);
  const act = (p: Promise<unknown>) => run("update", p).then(load);
  const ready = update?.available && (update.state === "ready" || update.state === "waiting");
  const says = update && {
    off: "This build has no update key, so it does not update itself.",
    current: "It is up to date.",
    checking: "Checking…",
    waiting: "Waiting for the replies to end. Bate restarts then.",
    ready: inShell ? "Installs when no reply is running." : "Install it in Bate's own window.",
    error: "",
  }[update.state];
  return (
    <Group id="settings-updates" title="Updates">
      {!update && !error("load") && <p className="note">Loading…</p>}
      {update && (
        <Setting title={`Bate ${update.current}`} hint={ready ? `Version ${update.available} is ready. ${says}` : says}>
          {update.state !== "off" && (
            <button disabled={update.state === "checking"} onClick={() => act(request("check_update"))}>Check for updates</button>
          )}
          {ready && inShell && (
            <button className="primary" disabled={update.state === "waiting"} onClick={() => act(request("install_update"))}>Install {update.available}</button>
          )}
        </Setting>
      )}
      <Problem text={error("update") || error("load") || update?.error || undefined} />
    </Group>
  );
}

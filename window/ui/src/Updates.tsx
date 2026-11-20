import { useEffect, useState } from "react";
import { Problem, useSaves } from "./Form.tsx";
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
  return (
    <>
      <h2 id="settings-updates" tabIndex={-1}>Updates</h2>
      {!update && !error("load") && <p className="note">Loading…</p>}
      {update && (
        <p>
          Bate {update.current}.{" "}
          {update.state === "off" && "This build has no update key, so it does not update itself."}
          {update.state === "current" && "It is up to date."}
          {update.state === "checking" && "Checking…"}
          {update.state === "waiting" && "Waiting for the replies to end. Bate restarts then."}
        </p>
      )}
      <div className="actions">
        {update && update.state !== "off" && (
          <button disabled={update.state === "checking"} onClick={() => act(request("check_update"))}>Check for updates</button>
        )}
        {ready && inShell && (
          <>
            <button className="primary" disabled={update.state === "waiting"} onClick={() => act(request("install_update"))}>Install {update.available}</button>
            <span className="note">Installs when no reply is running.</span>
          </>
        )}
      </div>
      {ready && !inShell && <p>Version {update.available} is ready. Install it in Bate's own window.</p>}
      <Problem text={error("update") || error("load") || update?.error || undefined} />
    </>
  );
}

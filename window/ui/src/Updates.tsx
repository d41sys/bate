import { useEffect, useState } from "react";
import { inShell, listen, request } from "./service.ts";

type Update = { state: "off" | "current" | "checking" | "ready" | "waiting" | "error"; current: string; available: string | null; error: string | null };

/** The version, a check, and the install of a release the service has downloaded and verified. */
export function Updates() {
  const [update, setUpdate] = useState<Update>();
  const [error, setError] = useState("");
  const load = () => request<Update>("get_update").then(setUpdate, (e: Error) => setError(e.message));
  useEffect(() => {
    void load();
    return listen((frame) => frame.type === "update_changed" && void load());
  }, []);
  const run = (p: Promise<unknown>) => p.then(() => setError(""), (e: Error) => setError(e.message)).then(load);
  const ready = update?.available && (update.state === "ready" || update.state === "waiting");
  return (
    <>
      <h2 id="settings-updates" tabIndex={-1}>Updates</h2>
      {update && (
        <p>
          Bate {update.current}.{" "}
          {update.state === "off" && "This build has no update key, so it does not update itself."}
          {update.state === "current" && "It is up to date."}
          {update.state === "checking" && "Checking…"}
          {update.state === "waiting" && "Waiting for the replies to end. Bate restarts then."}
        </p>
      )}
      {update && update.state !== "off" && (
        <button disabled={update.state === "checking"} onClick={() => run(request("check_update"))}>Check for updates</button>
      )}{" "}
      {ready && inShell && (
        <>
          <button disabled={update.state === "waiting"} onClick={() => run(request("install_update"))}>Install {update.available}</button>{" "}
          <span>Installs when no reply is running.</span>
        </>
      )}
      {ready && !inShell && <p>Version {update.available} is ready. Install it in Bate's own window.</p>}
      {(update?.error || error) && <p className="error" role="alert">{error || update?.error}</p>}
    </>
  );
}
